//! The movie player's picture (step 2, docs/hails-additions/47-movie-player.md): a worker thread decodes the disc's VP6
//! movie ahead (`skate-movie`), the frame clock picks the newest due frame (retail 82A92DC8: target frame = elapsed ms
//! x fps x 0.001, late frames dropped, no slow-down), and the three YUV planes are written straight into persistent
//! single-channel textures in the render world (no asset re-upload per frame); a UI material converts to RGB.
//! Audio, the FE 24 screen with the retail skip rules, subtitles and the boot / career hooks are the next steps.
//! Dev entry: `SKATE_MOVIE=<file.vp6>` plays one movie fullscreen at startup; Escape ends it.
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::time::Instant;

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    image::ImageSampler,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{AsBindGroup, Extent3d, TexelCopyBufferLayout, TextureDimension, TextureFormat, TextureUsages},
        renderer::RenderQueue,
        texture::GpuImage,
    },
    shader::ShaderRef,
    ui_render::UiMaterialPlugin,
};
use skate_movie::{FrameDecoder, Vp6Decoder, YuvFrame, container};

/// How many decoded frames the worker keeps ready.
const AHEAD: usize = 6;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct MovieMaterial {
    #[texture(0)]
    #[sampler(1)]
    luma: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    u: Handle<Image>,
    #[texture(4)]
    #[sampler(5)]
    v: Handle<Image>,
}

impl UiMaterial for MovieMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://skate3rust/movie.wgsl".into()
    }
}

enum Decoded {
    Frame(u32, Arc<YuvFrame>),
    Done,
}

struct Playback {
    rx: std::sync::Mutex<Receiver<Decoded>>,
    fps: f64,
    started: Option<Instant>,
    pending: Option<(u32, Arc<YuvFrame>)>,
    done: bool,
    root: Entity,
    planes: [Handle<Image>; 3],
    shown: u32,
    dropped: u32,
}

#[derive(Resource, Default)]
pub(crate) struct MoviePlayer {
    playback: Option<Playback>,
}

/// The frame the render world writes into the planes (new `seq` = new frame).
#[derive(Resource, Clone, Default, ExtractResource)]
struct MovieUpload {
    seq: u64,
    frame: Option<Arc<YuvFrame>>,
    planes: Option<[AssetId<Image>; 3]>,
}

impl MoviePlayer {
    pub(crate) fn playing(&self) -> bool {
        self.playback.is_some()
    }
}

/// Starts `path`; the old movie, if any, ends.
pub(crate) fn play(path: &std::path::Path, player: &mut MoviePlayer, commands: &mut Commands, images: &mut Assets<Image>,
                   materials: &mut Assets<MovieMaterial>) -> Result<(), String> {
    stop(player, commands);
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let reader = container::Reader::new(std::io::BufReader::new(file)).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let header = reader.header;
    let (w, h) = (u32::from(header.width), u32::from(header.height));
    if w == 0 || h == 0 {
        return Err(format!("{}: no picture size", path.display()));
    }
    let mut plane = |w: u32, h: u32| {
        let mut img = Image::new_fill(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, &[16],
            TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
        img.texture_descriptor.usage |= TextureUsages::COPY_DST;
        img.sampler = ImageSampler::linear();
        images.add(img)
    };
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let planes = [plane(w, h), plane(cw, ch), plane(cw, ch)];
    let material = materials.add(MovieMaterial { luma: planes[0].clone(), u: planes[1].clone(), v: planes[2].clone() });
    let root = commands.spawn((Name::new("movie"), GlobalZIndex(1000), BackgroundColor(Color::BLACK),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center,
            align_items: AlignItems::Center, ..default() }))
        .with_child((MaterialNode(material), Node { width: percent(100), max_height: percent(100), aspect_ratio: Some(w as f32 / h as f32), ..default() }))
        .id();
    let (tx, rx) = sync_channel(AHEAD);
    std::thread::Builder::new().name("movie-decode".into()).spawn(move || decode(reader, tx)).map_err(|e| e.to_string())?;
    info!("MOVIE_START {} {}x{} {:.3} fps {} frames", path.display(), w, h, header.fps(), header.frames);
    player.playback = Some(Playback { rx: std::sync::Mutex::new(rx), fps: header.fps(), started: None, pending: None, done: false, root, planes, shown: 0, dropped: 0 });
    Ok(())
}

pub(crate) fn stop(player: &mut MoviePlayer, commands: &mut Commands) {
    if let Some(p) = player.playback.take() {
        commands.entity(p.root).despawn();
        info!("MOVIE_END shown {} dropped {}", p.shown, p.dropped);
    }
}

fn decode<R: std::io::Read + std::io::Seek>(mut reader: container::Reader<R>, tx: SyncSender<Decoded>) {
    let mut dec = Vp6Decoder::new();
    let mut index = 0u32;
    while let Ok(Some(chunk)) = reader.next_chunk() {
        if let container::Chunk::Video { data, .. } = chunk {
            if let Ok(frame) = dec.decode(&data) {
                if tx.send(Decoded::Frame(index, Arc::new(frame))).is_err() {
                    return;
                }
            }
            index += 1;
        }
    }
    let _ = tx.send(Decoded::Done);
}

/// The frame due at `elapsed_s` (0-based): retail's frame pick, floor(elapsed ms x fps x 0.001).
pub(crate) fn due_frame(elapsed_s: f64, fps: f64) -> u32 {
    (elapsed_s * 1000.0 * fps * 0.001).floor().max(0.0) as u32
}

fn advance(mut player: ResMut<MoviePlayer>, mut upload: ResMut<MovieUpload>, mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if player.playback.is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || next_frame(&mut *player, &mut *upload) {
        stop(&mut *player, &mut commands);
        upload.frame = None;
    }
}

/// Moves to the newest due frame; true when the movie has ended.
fn next_frame(player: &mut MoviePlayer, upload: &mut MovieUpload) -> bool {
    let Some(p) = player.playback.as_mut() else { return false };
    let due = p.started.map_or(0, |t| due_frame(t.elapsed().as_secs_f64(), p.fps));
    let mut newest: Option<Arc<YuvFrame>> = None;
    loop {
        if p.pending.is_none() && !p.done {
            let rx = p.rx.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
            match rx.try_recv() {
                Ok(Decoded::Frame(i, f)) => p.pending = Some((i, f)),
                Ok(Decoded::Done) | Err(TryRecvError::Disconnected) => p.done = true,
                Err(TryRecvError::Empty) => {}
            }
        }
        match &p.pending {
            Some((i, _)) if *i <= due => {
                if newest.is_some() {
                    p.dropped += 1;
                }
                newest = p.pending.take().map(|(_, f)| f);
            }
            _ => break,
        }
    }
    if let Some(f) = newest {
        p.started.get_or_insert_with(Instant::now);
        p.shown += 1;
        upload.seq += 1;
        upload.frame = Some(f);
        upload.planes = Some([p.planes[0].id(), p.planes[1].id(), p.planes[2].id()]);
        return false;
    }
    p.done && p.pending.is_none()
}

fn write_planes(upload: Res<MovieUpload>, images: Res<RenderAssets<GpuImage>>, queue: Res<RenderQueue>, mut last: Local<u64>) {
    if upload.seq == *last {
        return;
    }
    let (Some(frame), Some(planes)) = (&upload.frame, upload.planes) else { return };
    for (p, id) in planes.iter().enumerate() {
        let Some(gpu) = images.get(*id) else { return };
        let (w, h) = frame.plane_size(p);
        let size = gpu.texture.size();
        if size.width as usize != w || size.height as usize != h {
            return;
        }
        queue.write_texture(gpu.texture.as_image_copy(), &frame.planes[p],
            TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w as u32), rows_per_image: Some(h as u32) }, size);
    }
    *last = upload.seq;
}

fn dev_start(mut player: ResMut<MoviePlayer>, mut commands: Commands, mut images: ResMut<Assets<Image>>,
             mut materials: ResMut<Assets<MovieMaterial>>) {
    let Ok(path) = std::env::var("SKATE_MOVIE") else { return };
    if let Err(e) = play(std::path::Path::new(&path), &mut player, &mut commands, &mut images, &mut materials) {
        warn!("SKATE_MOVIE {e}");
    }
}

pub(crate) struct MoviePlayerPlugin;

impl Plugin for MoviePlayerPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "movie.wgsl");
        app.add_plugins(UiMaterialPlugin::<MovieMaterial>::default())
            .add_plugins(ExtractResourcePlugin::<MovieUpload>::default())
            .init_resource::<MoviePlayer>()
            .init_resource::<MovieUpload>()
            .add_systems(Startup, dev_start)
            .add_systems(Update, advance);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            // As video_screens' copy: after the frame's render, so the new frame shows on the next one.
            render_app.add_systems(Render, write_planes.after(RenderSystems::Render).before(RenderSystems::Cleanup));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_clock_picks_floor_of_elapsed_times_fps() {
        assert_eq!(due_frame(0.0, 29.9699), 0);
        assert_eq!(due_frame(1.0, 29.9699), 29);
        assert_eq!(due_frame(1.0, 60.0), 60);
        assert_eq!(due_frame(10.0, 29.9699), 299);
    }
}
