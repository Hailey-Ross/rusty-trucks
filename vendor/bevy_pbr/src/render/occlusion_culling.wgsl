// Occlusion culling utility functions.

#define_import_path bevy_pbr::occlusion_culling

fn get_aabb_size_in_pixels(aabb: vec4<f32>, depth_pyramid: texture_2d<f32>) -> vec2<f32> {
    let depth_pyramid_size_mip_0 = vec2<f32>(textureDimensions(depth_pyramid, 0));
    let aabb_width_pixels = (aabb.z - aabb.x) * depth_pyramid_size_mip_0.x;
    let aabb_height_pixels = (aabb.w - aabb.y) * depth_pyramid_size_mip_0.y;
    return vec2(aabb_width_pixels, aabb_height_pixels);
}

fn get_occluder_depth(
    aabb: vec4<f32>,
    aabb_pixel_size: vec2<f32>,
    depth_pyramid: texture_2d<f32>
) -> f32 {
    let aabb_width_pixels = aabb_pixel_size.x;
    let aabb_height_pixels = aabb_pixel_size.y;

    let depth_pyramid_size_mip_0 = vec2<f32>(textureDimensions(depth_pyramid, 0));
    let depth_level = max(0, i32(ceil(log2(max(aabb_width_pixels, aabb_height_pixels))))); // TODO: Naga doesn't like this being a u32
    let depth_pyramid_size = vec2<f32>(textureDimensions(depth_pyramid, depth_level));
    let aabb_top_left = vec2<u32>(aabb.xy * depth_pyramid_size);

    let depth_quad_a = textureLoad(depth_pyramid, aabb_top_left, depth_level).x;
    let depth_quad_b = textureLoad(depth_pyramid, aabb_top_left + vec2(1u, 0u), depth_level).x;
    let depth_quad_c = textureLoad(depth_pyramid, aabb_top_left + vec2(0u, 1u), depth_level).x;
    let depth_quad_d = textureLoad(depth_pyramid, aabb_top_left + vec2(1u, 1u), depth_level).x;
    return min(min(depth_quad_a, depth_quad_b), min(depth_quad_c, depth_quad_d));
}

// skate: a finer test than `get_occluder_depth`'s 2 x 2 texels (vendor/README.md). True when
// `depth`, the box's nearest point in reverse-Z NDC, is behind every texel the UV rectangle
// `aabb` touches, at the finest level where the rectangle spans at most `span` texels each way:
// up to (span + 1)^2 reads, stopping at the first texel it isn't behind. Each texel of a level is
// the farthest of those under it, so this culls whatever the 2 x 2 test culls, and more. The
// rectangle is clamped to the pyramid: what lies off it isn't on screen.
fn occluded_finely(
    aabb: vec4<f32>,
    aabb_pixel_size: vec2<f32>,
    depth: f32,
    span: f32,
    depth_pyramid: texture_2d<f32>
) -> bool {
    let top = f32(textureNumLevels(depth_pyramid) - 1u);
    let level = i32(clamp(ceil(log2(max(aabb_pixel_size.x, aabb_pixel_size.y) / span)), 0.0, top));
    let size = vec2<i32>(textureDimensions(depth_pyramid, level));
    // A finite rectangle touches at most span + 1 texels each way (span + 2 if log2 rounds down);
    // the bounds only matter for non-finite UVs, whose conversion to i32 is unspecified.
    let lo = clamp(vec2<i32>(clamp(aabb.xy, vec2(0.0), vec2(1.0)) * vec2<f32>(size)), vec2(0), size - 1);
    let hi = min(
        vec2<i32>(clamp(aabb.zw, vec2(0.0), vec2(1.0)) * vec2<f32>(size)),
        min(size - 1, lo + i32(span) + 1)
    );
    if (any(hi < lo)) {
        return false;
    }
    for (var y = lo.y; y <= hi.y; y += 1) {
        for (var x = lo.x; x <= hi.x; x += 1) {
            // Written so a NaN depth is never culled, as in the 2 x 2 test's comparison.
            if (!(depth < textureLoad(depth_pyramid, vec2(x, y), level).x)) {
                return false;
            }
        }
    }
    return true;
}

// skate: `occluded_finely` on the same texels, with the same answer, but read eight at a time
// along each row (vendor/README.md). No compare sits between the eight reads, so they can all
// be in flight at once and a mesh behind them all waits for each eight rather than for each
// texel. Past the row's end a read repeats its last texel. The answer differs only if the
// pyramid held a NaN, which a pyramid of rasterised depths cannot.
fn occluded_finely_batched(
    aabb: vec4<f32>,
    aabb_pixel_size: vec2<f32>,
    depth: f32,
    span: f32,
    depth_pyramid: texture_2d<f32>
) -> bool {
    let top = f32(textureNumLevels(depth_pyramid) - 1u);
    let level = i32(clamp(ceil(log2(max(aabb_pixel_size.x, aabb_pixel_size.y) / span)), 0.0, top));
    let size = vec2<i32>(textureDimensions(depth_pyramid, level));
    let lo = clamp(vec2<i32>(clamp(aabb.xy, vec2(0.0), vec2(1.0)) * vec2<f32>(size)), vec2(0), size - 1);
    let hi = min(
        vec2<i32>(clamp(aabb.zw, vec2(0.0), vec2(1.0)) * vec2<f32>(size)),
        min(size - 1, lo + i32(span) + 1)
    );
    if (any(hi < lo)) {
        return false;
    }
    for (var y = lo.y; y <= hi.y; y += 1) {
        for (var x = lo.x; x <= hi.x; x += 8) {
            let d0 = textureLoad(depth_pyramid, vec2(x, y), level).x;
            let d1 = textureLoad(depth_pyramid, vec2(min(x + 1, hi.x), y), level).x;
            let d2 = textureLoad(depth_pyramid, vec2(min(x + 2, hi.x), y), level).x;
            let d3 = textureLoad(depth_pyramid, vec2(min(x + 3, hi.x), y), level).x;
            let d4 = textureLoad(depth_pyramid, vec2(min(x + 4, hi.x), y), level).x;
            let d5 = textureLoad(depth_pyramid, vec2(min(x + 5, hi.x), y), level).x;
            let d6 = textureLoad(depth_pyramid, vec2(min(x + 6, hi.x), y), level).x;
            let d7 = textureLoad(depth_pyramid, vec2(min(x + 7, hi.x), y), level).x;
            // Behind the farthest of the eight is behind them all, as in `get_occluder_depth`.
            // NAK issues all eight reads before the first min only when its pre-RA scheduler
            // honours latencies (its `plat` flag, bevy-horizon's mesa patch 0015, on by default in
            // Switch builds since patch 0019); without it the reads go two at a time.
            let farthest = min(min(min(d0, d1), min(d2, d3)), min(min(d4, d5), min(d6, d7)));
            if (!(depth < farthest)) {
                return false;
            }
        }
    }
    return true;
}
