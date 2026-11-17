//! Big texture arrays (the retail map's texture pages, up to ~440 MB each) go to the GPU a few layers per frame.
//! Bevy uploads an image whole in the frame it is prepared, which froze the loading screen for 60-110 ms per page at
//! the end of a map change. Such an image is published without data (Bevy only allocates the texture) and its layers
//! are written here within the render upload budget (`map_render::render_upload_budget`); the map change and the
//! startup loading screen wait until it is done (`map_render::render_upload_backlog`).
use bevy::prelude::*;
use bevy::render::render_asset::{RenderAssetBytesPerFrameLimiter, RenderAssets};
use bevy::render::render_resource::{Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureFormat, TextureViewDimension};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use std::sync::{Mutex, atomic::{AtomicBool, Ordering::Relaxed}};
use std::time::{Duration, Instant};

struct Pending {
    id: AssetId<Image>,
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    layers: u32,
    levels: u32,
    next: u32,
    queued: Instant,
    started: bool,
}

static QUEUE: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
static BUSY: AtomicBool = AtomicBool::new(false);
/// An image that never reaches the render world (its map was retired first) is dropped after this.
const ABANDON: Duration = Duration::from_secs(60);

/// Layers are still to be written.
pub(crate) fn busy() -> bool {
    BUSY.load(Relaxed)
}

/// Called as `id` goes into `Assets<Image>`: an RGBA8 2D array larger than the frame budget keeps its texture
/// description and hands its layer-major bytes to the render world's writer.
pub(crate) fn defer(id: AssetId<Image>, image: &mut Image) {
    let Some(budget) = crate::map_render::render_upload_budget() else { return };
    let d = &image.texture_descriptor;
    let array = image.texture_view_descriptor.as_ref().and_then(|v| v.dimension) == Some(TextureViewDimension::D2Array);
    if !array || d.format != TextureFormat::Rgba8Unorm || d.size.depth_or_array_layers < 2
        || image.data_order != bevy::render::render_resource::TextureDataOrder::LayerMajor
        || image.data.as_ref().is_none_or(|b| b.len() <= budget) {
        return;
    }
    let (width, height, layers, levels) = (d.size.width, d.size.height, d.size.depth_or_array_layers, d.mip_level_count);
    let bytes = image.data.take().expect("checked above");
    if bytes.len() != layer_bytes(width, height, levels) * layers as usize {
        image.data = Some(bytes);
        return;
    }
    BUSY.store(true, Relaxed);
    QUEUE.lock().unwrap_or_else(|e| e.into_inner())
        .push(Pending { id, bytes, width, height, layers, levels, next: 0, queued: Instant::now(), started: false });
}

fn mip(size: u32, level: u32) -> u32 {
    (size >> level).max(1)
}

fn layer_bytes(width: u32, height: u32, levels: u32) -> usize {
    (0..levels).map(|m| mip(width, m) as usize * mip(height, m) as usize * 4).sum()
}

/// Render world, after `PrepareAssets`: write whole layers of the queued images until the frame's budget is used
/// (at least one layer a frame).
pub(crate) fn write_layers(images: Res<RenderAssets<GpuImage>>, queue: Res<RenderQueue>,
    limiter: Res<RenderAssetBytesPerFrameLimiter>) {
    let mut pending = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    if pending.is_empty() {
        BUSY.store(false, Relaxed);
        return;
    }
    let max = limiter.max_bytes.unwrap_or(usize::MAX);
    let mut left = max.saturating_sub(limiter.bytes_written.load(Relaxed));
    let (mut wrote, mut written) = (false, 0usize);
    pending.retain_mut(|p| {
        let Some(gpu) = images.get(p.id) else {
            // Retired after its writes began, or never prepared at all.
            return !p.started && p.queued.elapsed() < ABANDON;
        };
        if gpu.size.depth_or_array_layers != p.layers || gpu.mip_level_count != p.levels {
            return false;
        }
        p.started = true;
        let per_layer = layer_bytes(p.width, p.height, p.levels);
        while p.next < p.layers && (!wrote || left >= per_layer) {
            let mut offset = p.next as usize * per_layer;
            for level in 0..p.levels {
                let (w, h) = (mip(p.width, level), mip(p.height, level));
                let len = w as usize * h as usize * 4;
                queue.write_texture(
                    TexelCopyTextureInfo { texture: &gpu.texture, mip_level: level,
                        origin: Origin3d { x: 0, y: 0, z: p.next }, aspect: TextureAspect::All },
                    &p.bytes[offset..offset + len],
                    TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
                    Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
                offset += len;
            }
            p.next += 1;
            left = left.saturating_sub(per_layer);
            written += per_layer;
            wrote = true;
        }
        p.next < p.layers
    });
    if limiter.max_bytes.is_some() {
        limiter.bytes_written.fetch_add(written, Relaxed);
    }
    BUSY.store(!pending.is_empty(), Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_bytes_sum_the_mip_chain() {
        assert_eq!(layer_bytes(4, 2, 3), 4 * 2 * 4 + 2 * 1 * 4 + 1 * 1 * 4);
        assert_eq!(layer_bytes(1, 1, 1), 4);
    }
}
