#ifdef MESHLET_VISIBILITY_BUFFER_RASTER_PASS_OUTPUT
@group(0) @binding(0) var mip_0: texture_storage_2d<r64uint, read>;
#else
#ifdef MESHLET
@group(0) @binding(0) var mip_0: texture_storage_2d<r32uint, read>;
#else   // MESHLET
#ifdef MULTISAMPLE
@group(0) @binding(0) var mip_0: texture_depth_multisampled_2d;
#else   // MULTISAMPLE
@group(0) @binding(0) var mip_0: texture_depth_2d;
#endif  // MULTISAMPLE
#endif  // MESHLET
#endif  // MESHLET_VISIBILITY_BUFFER_RASTER_PASS_OUTPUT
@group(0) @binding(1) var mip_1: texture_storage_2d<r32float, write>;
@group(0) @binding(2) var mip_2: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var mip_3: texture_storage_2d<r32float, write>;
@group(0) @binding(4) var mip_4: texture_storage_2d<r32float, write>;
@group(0) @binding(5) var mip_5: texture_storage_2d<r32float, write>;
@group(0) @binding(6) var mip_6: texture_storage_2d<r32float, read_write>;
@group(0) @binding(7) var mip_7: texture_storage_2d<r32float, write>;
@group(0) @binding(8) var mip_8: texture_storage_2d<r32float, write>;
@group(0) @binding(9) var mip_9: texture_storage_2d<r32float, write>;
@group(0) @binding(10) var mip_10: texture_storage_2d<r32float, write>;
@group(0) @binding(11) var mip_11: texture_storage_2d<r32float, write>;
@group(0) @binding(12) var mip_12: texture_storage_2d<r32float, write>;
@group(0) @binding(13) var samplr: sampler;
struct Constants { max_mip_level: u32 }
var<push_constant> constants: Constants;

/// Generates a hierarchical depth buffer.
/// Based on FidelityFX SPD v2.1 https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/blob/d7531ae47d8b36a5d4025663e731a47a38be882f/sdk/include/FidelityFX/gpu/spd/ffx_spd.h#L528
///
/// `mip_0` may be of any size, but `mip_1` and down must have side lengths that
/// are powers of two.

// TODO:
// * Subgroup support
// * True single pass downsampling

var<workgroup> intermediate_memory: array<array<f32, 16>, 16>;

#ifdef SKATE_PYRAMID_TILE
// skate (Switch port, vendor/README.md): the depth block a workgroup reads, `depth_tile_fits` in
// mod.rs keeps views whose blocks don't fit on Bevy's pipeline.
const DEPTH_TILE_CAPACITY: u32 = 2304u;
var<workgroup> depth_tile: array<f32, DEPTH_TILE_CAPACITY>;
#endif

@compute
@workgroup_size(256, 1, 1)
fn downsample_depth_first(
    @builtin(workgroup_id) workgroup_id: vec3u,
    @builtin(local_invocation_index) local_invocation_index: u32,
) {
    let sub_xy = remap_for_wave_reduction(local_invocation_index % 64u);
    let x = sub_xy.x + 8u * ((local_invocation_index >> 6u) % 2u);
    let y = sub_xy.y + 8u * (local_invocation_index >> 7u);

#ifdef SKATE_PYRAMID_TILE
    downsample_mips_0_to_2_tiled(x, y, workgroup_id.xy, local_invocation_index);
#else ifdef SKATE_PYRAMID_BLOCK
    downsample_mips_0_to_2_block(x, y, workgroup_id.xy);
#else
    downsample_mips_0_and_1(x, y, workgroup_id.xy, local_invocation_index);
#endif

    downsample_mips_2_to_5(x, y, workgroup_id.xy, local_invocation_index);
}

@compute
@workgroup_size(256, 1, 1)
fn downsample_depth_second(@builtin(local_invocation_index) local_invocation_index: u32) {
    let sub_xy = remap_for_wave_reduction(local_invocation_index % 64u);
    let x = sub_xy.x + 8u * ((local_invocation_index >> 6u) % 2u);
    let y = sub_xy.y + 8u * (local_invocation_index >> 7u);

    downsample_mips_6_and_7(x, y);

    downsample_mips_8_to_11(x, y, local_invocation_index);
}

fn downsample_mips_0_and_1(x: u32, y: u32, workgroup_id: vec2u, local_invocation_index: u32) {
    var v: vec4f;

    var tex = vec2(workgroup_id * 64u) + vec2(x * 2u, y * 2u);
    var pix = vec2(workgroup_id * 32u) + vec2(x, y);
    v[0] = reduce_load_mip_0(tex);
    textureStore(mip_1, pix, vec4(v[0]));

    tex = vec2(workgroup_id * 64u) + vec2(x * 2u + 32u, y * 2u);
    pix = vec2(workgroup_id * 32u) + vec2(x + 16u, y);
    v[1] = reduce_load_mip_0(tex);
    textureStore(mip_1, pix, vec4(v[1]));

    tex = vec2(workgroup_id * 64u) + vec2(x * 2u, y * 2u + 32u);
    pix = vec2(workgroup_id * 32u) + vec2(x, y + 16u);
    v[2] = reduce_load_mip_0(tex);
    textureStore(mip_1, pix, vec4(v[2]));

    tex = vec2(workgroup_id * 64u) + vec2(x * 2u + 32u, y * 2u + 32u);
    pix = vec2(workgroup_id * 32u) + vec2(x + 16u, y + 16u);
    v[3] = reduce_load_mip_0(tex);
    textureStore(mip_1, pix, vec4(v[3]));

    if constants.max_mip_level <= 1u { return; }

    for (var i = 0u; i < 4u; i++) {
        intermediate_memory[x][y] = v[i];
        workgroupBarrier();
        if local_invocation_index < 64u {
            v[i] = reduce_4(vec4(
                intermediate_memory[x * 2u + 0u][y * 2u + 0u],
                intermediate_memory[x * 2u + 1u][y * 2u + 0u],
                intermediate_memory[x * 2u + 0u][y * 2u + 1u],
                intermediate_memory[x * 2u + 1u][y * 2u + 1u],
            ));
            pix = (workgroup_id * 16u) + vec2(
                x + (i % 2u) * 8u,
                y + (i / 2u) * 8u,
            );
            textureStore(mip_2, pix, vec4(v[i]));
        }
        workgroupBarrier();
    }

    if local_invocation_index < 64u {
        intermediate_memory[x + 0u][y + 0u] = v[0];
        intermediate_memory[x + 8u][y + 0u] = v[1];
        intermediate_memory[x + 0u][y + 8u] = v[2];
        intermediate_memory[x + 8u][y + 8u] = v[3];
    }
}

#ifdef SKATE_PYRAMID_TILE
// skate: mips 1 and 2 with one read of each depth texel. Bevy's `reduce_load_mip_0` takes four
// gathers a mip 1 texel, 16 a thread, and NAK waits for each before the next. Here the workgroup
// first gathers the block of depth its 64x64 virtual texels read into `depth_tile`, about two
// gathers a thread at 1280x720. Each thread then takes a 2x2 block of mip 1 texels, each the
// minimum over exactly the texels Bevy's gathers read (`tile_footprint_min`), and their minimum
// is its mip 2 texel, so both levels are Bevy's without its shared-memory rounds for mip 2.
fn downsample_mips_0_to_2_tiled(x: u32, y: u32, workgroup_id: vec2u, local_invocation_index: u32) {
    let size = textureDimensions(mip_0);
    // `next_power_of_two(size)` is 1 << shift.
    let shift = vec2(32u) - countLeadingZeros(size);
    let virtual_first = workgroup_id * 64u;
    let first = max(vec2(
        first_gathered_texel(virtual_first.x, size.x, shift.x),
        first_gathered_texel(virtual_first.y, size.y, shift.y),
    ), vec2(0));
    let last = min(vec2(
        first_gathered_texel(virtual_first.x + 63u, size.x, shift.x),
        first_gathered_texel(virtual_first.y + 63u, size.y, shift.y),
    ) + 1, vec2<i32>(size) - 1);
    let extent = vec2<u32>(last - first) + 1u;
    // An odd row stride spreads a warp's reads over more banks.
    let stride = extent.x | 1u;
    let blocks = (extent + 1u) / 2u;
    let texel_size = 1.0 / vec2<f32>(size);
    for (var block = local_invocation_index; block < blocks.x * blocks.y; block += 256u) {
        let t = vec2(block % blocks.x, block / blocks.x) * 2u;
        // At the corner texels t and t + 1 share, the gather reads exactly those four. Past the
        // last column or row it clamps to the edge; those lie outside the block and aren't kept.
        let g = textureGather(mip_0, samplr, vec2<f32>(vec2<i32>(t) + first + 1) * texel_size);
        let i = t.y * stride + t.x;
        depth_tile[i] = g.w;
        if t.x + 1u < extent.x {
            depth_tile[i + 1u] = g.z;
        }
        if t.y + 1u < extent.y {
            depth_tile[i + stride] = g.x;
            if t.x + 1u < extent.x {
                depth_tile[i + stride + 1u] = g.y;
            }
        }
    }
    workgroupBarrier();

    let p = workgroup_id * 32u + vec2(x, y) * 2u;
    let a = tile_footprint_min(p, size, shift, first, stride);
    let b = tile_footprint_min(p + vec2(1u, 0u), size, shift, first, stride);
    let c = tile_footprint_min(p + vec2(0u, 1u), size, shift, first, stride);
    let d = tile_footprint_min(p + vec2(1u, 1u), size, shift, first, stride);
    textureStore(mip_1, p, vec4(a));
    textureStore(mip_1, p + vec2(1u, 0u), vec4(b));
    textureStore(mip_1, p + vec2(0u, 1u), vec4(c));
    textureStore(mip_1, p + vec2(1u, 1u), vec4(d));

    if constants.max_mip_level <= 1u { return; }

    let v = reduce_4(vec4(a, b, c, d));
    textureStore(mip_2, workgroup_id * 16u + vec2(x, y), vec4(v));
    intermediate_memory[x][y] = v;
}

// The minimum over the depth texels that `reduce_load_mip_0` gathers for mip 1 texel `p`: on each
// axis, from the first texel read at virtual texel 2p to the second read at 2p + 1, two or three
// texels, clamped to the depth buffer as the sampler clamps.
fn tile_footprint_min(p: vec2u, size: vec2u, shift: vec2u, first: vec2<i32>, stride: u32) -> f32 {
    let lo = vec2<u32>(max(vec2(
        first_gathered_texel(2u * p.x, size.x, shift.x),
        first_gathered_texel(2u * p.y, size.y, shift.y),
    ), vec2(0)) - first);
    let hi = vec2<u32>(min(vec2(
        first_gathered_texel(2u * p.x + 1u, size.x, shift.x),
        first_gathered_texel(2u * p.y + 1u, size.y, shift.y),
    ) + 1, vec2<i32>(size) - 1) - first);
    // When the footprint is two wide, the third column or row repeats the second.
    let r0 = lo.y * stride;
    let r1 = r0 + stride;
    let r2 = hi.y * stride;
    return min(
        min(min(depth_tile[r0 + lo.x], depth_tile[r0 + lo.x + 1u]), depth_tile[r0 + hi.x]),
        min(
            min(min(depth_tile[r1 + lo.x], depth_tile[r1 + lo.x + 1u]), depth_tile[r1 + hi.x]),
            min(min(depth_tile[r2 + lo.x], depth_tile[r2 + lo.x + 1u]), depth_tile[r2 + hi.x]),
        ),
    );
}
#endif

#ifdef SKATE_PYRAMID_BLOCK
// skate: mips 1 and 2 with each depth texel gathered about once (vendor/README.md). Bevy's
// `reduce_load_mip_0` makes four gathers for each mip 1 texel, sixteen a thread, whose texels
// overlap, and NAK waits for each before the next. Here each thread takes a 2x2 block of mip 1
// texels and gathers the depth under all four in 2x2 blocks side by side, two or three each way,
// none needing another's result: six at 1280x720. Each mip 1 texel is then the minimum over
// exactly the texels Bevy's gathers read, and their minimum is the thread's mip 2 texel, as with
// the tile.
fn downsample_mips_0_to_2_block(x: u32, y: u32, workgroup_id: vec2u) {
    let size = textureDimensions(mip_0);
    // `next_power_of_two(size)` is 1 << shift.
    let shift = vec2(32u) - countLeadingZeros(size);
    let p = workgroup_id * 32u + vec2(x, y) * 2u;
    // On each axis, Bevy gathers mip 1 texels p and p + 1 at virtual texels 2p to 2p + 3, whose
    // first texels are `first` and d.x, d.y and d.z texels after it, and each gather reads that
    // texel and the next. The virtual size is under twice the size, so each step is 0 or 1
    // texels: p reads from 0 to d.x + 1 texels after `first`, and p + 1 from d.y to d.z + 1,
    // with d.x at most 1, d.y 1 or 2 and d.z d.y or d.y + 1.
    let first = vec2(
        first_gathered_texel(2u * p.x, size.x, shift.x),
        first_gathered_texel(2u * p.y, size.y, shift.y),
    );
    let dx = vec3<u32>(vec3(
        first_gathered_texel(2u * p.x + 1u, size.x, shift.x),
        first_gathered_texel(2u * p.x + 2u, size.x, shift.x),
        first_gathered_texel(2u * p.x + 3u, size.x, shift.x),
    ) - first.x);
    let dy = vec3<u32>(vec3(
        first_gathered_texel(2u * p.y + 1u, size.y, shift.y),
        first_gathered_texel(2u * p.y + 2u, size.y, shift.y),
        first_gathered_texel(2u * p.y + 3u, size.y, shift.y),
    ) - first.y);

    // Each gather sits on the corner of texels `first + 2i` and `first + 2i + 1` each way, so it
    // reads exactly those four; past the edge the sampler clamps, as it does for Bevy's. An axis
    // needs a third gather only if d.z can be 3, when three virtual texels span over two texels.
    // The third row is gathered and reduced on its own, to keep fewer registers.
    let wide = 3u * size > vec2(2u) << shift;
    let uv = vec2<f32>(first + 1) / vec2<f32>(size);
    let g00 = textureGather(mip_0, samplr, uv);
    let g10 = textureGather(mip_0, samplr, uv, vec2(2, 0));
    let g01 = textureGather(mip_0, samplr, uv, vec2(0, 2));
    let g11 = textureGather(mip_0, samplr, uv, vec2(2, 2));
    // Only picked where dx.z is 3, which needs `wide.x`.
    var g20 = g10;
    var g21 = g11;
    if wide.x {
        g20 = textureGather(mip_0, samplr, uv, vec2(4, 0));
        g21 = textureGather(mip_0, samplr, uv, vec2(4, 2));
    }
    let rows_0 = block_row_minima_third(block_row_minima(g00, g10, dx), g20.wx, dx);
    let rows_1 = block_row_minima_third(block_row_minima(g01, g11, dx), g21.wx, dx);
    let top = block_min_first(rows_0[0], rows_0[1], rows_1[0], dy);
    var bottom = block_min_second(rows_0[1], rows_1[0], rows_1[1], dy);
    if wide.y {
        let g02 = textureGather(mip_0, samplr, uv, vec2(0, 4));
        let g12 = textureGather(mip_0, samplr, uv, vec2(2, 4));
        // Only picked where dx.z is 3, which needs `wide.x`.
        var g22 = g12;
        if wide.x {
            g22 = textureGather(mip_0, samplr, uv, vec2(4, 4));
        }
        let rows_2 = block_row_minima_third(block_row_minima(g02, g12, dx), g22.wx, dx);
        bottom = block_min_third(bottom, rows_2[0], dy);
    }
    textureStore(mip_1, p, vec4(top.x));
    textureStore(mip_1, p + vec2(1u, 0u), vec4(top.y));
    textureStore(mip_1, p + vec2(0u, 1u), vec4(bottom.x));
    textureStore(mip_1, p + vec2(1u, 1u), vec4(bottom.y));

    if constants.max_mip_level <= 1u { return; }

    let v = reduce_4(vec4(top, bottom));
    textureStore(mip_2, workgroup_id * 16u + vec2(x, y), vec4(v));
    intermediate_memory[x][y] = v;
}

// For two gathered rows, from two gathers side by side, each row's minima for mip 1 texels p and
// p + 1 (`.x` and `.y`) without a third gather's texels: `[0]` for the upper row, `[1]` for the
// lower.
fn block_row_minima(g0: vec4f, g1: vec4f, d: vec3u) -> array<vec2f, 2> {
    // A gather's `.w` and `.z` are its upper row's texels, left to right, and `.x` and `.y` its
    // lower row's, so each of these columns holds a texel of both rows.
    let first = block_min_first(g0.wx, g0.zy, g1.wx, d);
    let second = block_min_second(g0.zy, g1.wx, g1.zy, d);
    return array(vec2(first.x, second.x), vec2(first.y, second.y));
}

// `block_row_minima`'s minima with a third gather's first column, `t4`, added.
fn block_row_minima_third(rows: array<vec2f, 2>, t4: vec2f, d: vec3u) -> array<vec2f, 2> {
    let second = block_min_third(vec2(rows[0].y, rows[1].y), t4, d);
    return array(vec2(rows[0].x, second.x), vec2(rows[1].x, second.y));
}

// The minimum over texels 0 to d.x + 1 after `first`, given texels 0, 1 and 2, for two rows or
// columns at once.
fn block_min_first(t0: vec2f, t1: vec2f, t2: vec2f, d: vec3u) -> vec2f {
    return min(min(t0, t1), select(t1, t2, d.x == 1u));
}

// The minimum over texels d.y to d.z + 1 after `first` up to texel 3, given texels 1 to 3. Texel
// 2 is always among them.
fn block_min_second(t1: vec2f, t2: vec2f, t3: vec2f, d: vec3u) -> vec2f {
    return min(min(t2, select(t2, t1, d.y == 1u)), select(t2, t3, d.z >= 2u));
}

// `block_min_second`'s minimum `m` with texel 4 added where d.z is 3, so it reaches texel 4.
fn block_min_third(m: vec2f, t4: vec2f, d: vec3u) -> vec2f {
    return min(m, select(m, t4, d.z == 3u));
}
#endif

// The first texel on one axis that `load_mip_0`'s gather reads at virtual texel `v`,
// floor((v + 0.5) * size / 2^shift - 0.5), in integers; the gather's other texel is the next one.
fn first_gathered_texel(v: u32, size: u32, shift: u32) -> i32 {
    return (i32((2u * v + 1u) * size) - (1i << shift)) >> (shift + 1u);
}

fn downsample_mips_2_to_5(x: u32, y: u32, workgroup_id: vec2u, local_invocation_index: u32) {
    if constants.max_mip_level <= 2u { return; }
    workgroupBarrier();
    downsample_mip_2(x, y, workgroup_id, local_invocation_index);

    if constants.max_mip_level <= 3u { return; }
    workgroupBarrier();
    downsample_mip_3(x, y, workgroup_id, local_invocation_index);

    if constants.max_mip_level <= 4u { return; }
    workgroupBarrier();
    downsample_mip_4(x, y, workgroup_id, local_invocation_index);

    if constants.max_mip_level <= 5u { return; }
    workgroupBarrier();
    downsample_mip_5(workgroup_id, local_invocation_index);
}

fn downsample_mip_2(x: u32, y: u32, workgroup_id: vec2u, local_invocation_index: u32) {
    if local_invocation_index < 64u {
        let v = reduce_4(vec4(
            intermediate_memory[x * 2u + 0u][y * 2u + 0u],
            intermediate_memory[x * 2u + 1u][y * 2u + 0u],
            intermediate_memory[x * 2u + 0u][y * 2u + 1u],
            intermediate_memory[x * 2u + 1u][y * 2u + 1u],
        ));
        textureStore(mip_3, (workgroup_id * 8u) + vec2(x, y), vec4(v));
        intermediate_memory[x * 2u + y % 2u][y * 2u] = v;
    }
}

fn downsample_mip_3(x: u32, y: u32, workgroup_id: vec2u, local_invocation_index: u32) {
    if local_invocation_index < 16u {
        let v = reduce_4(vec4(
            intermediate_memory[x * 4u + 0u + 0u][y * 4u + 0u],
            intermediate_memory[x * 4u + 2u + 0u][y * 4u + 0u],
            intermediate_memory[x * 4u + 0u + 1u][y * 4u + 2u],
            intermediate_memory[x * 4u + 2u + 1u][y * 4u + 2u],
        ));
        textureStore(mip_4, (workgroup_id * 4u) + vec2(x, y), vec4(v));
        intermediate_memory[x * 4u + y][y * 4u] = v;
    }
}

fn downsample_mip_4(x: u32, y: u32, workgroup_id: vec2u, local_invocation_index: u32) {
    if local_invocation_index < 4u {
        let v = reduce_4(vec4(
            intermediate_memory[x * 8u + 0u + 0u + y * 2u][y * 8u + 0u],
            intermediate_memory[x * 8u + 4u + 0u + y * 2u][y * 8u + 0u],
            intermediate_memory[x * 8u + 0u + 1u + y * 2u][y * 8u + 4u],
            intermediate_memory[x * 8u + 4u + 1u + y * 2u][y * 8u + 4u],
        ));
        textureStore(mip_5, (workgroup_id * 2u) + vec2(x, y), vec4(v));
        intermediate_memory[x + y * 2u][0u] = v;
    }
}

fn downsample_mip_5(workgroup_id: vec2u, local_invocation_index: u32) {
    if local_invocation_index < 1u {
        let v = reduce_4(vec4(
            intermediate_memory[0u][0u],
            intermediate_memory[1u][0u],
            intermediate_memory[2u][0u],
            intermediate_memory[3u][0u],
        ));
        textureStore(mip_6, workgroup_id, vec4(v));
    }
}

fn downsample_mips_6_and_7(x: u32, y: u32) {
    var v: vec4f;

    var tex = vec2(x * 4u + 0u, y * 4u + 0u);
    var pix = vec2(x * 2u + 0u, y * 2u + 0u);
    v[0] = reduce_load_mip_6(tex);
    textureStore(mip_7, pix, vec4(v[0]));

    tex = vec2(x * 4u + 2u, y * 4u + 0u);
    pix = vec2(x * 2u + 1u, y * 2u + 0u);
    v[1] = reduce_load_mip_6(tex);
    textureStore(mip_7, pix, vec4(v[1]));

    tex = vec2(x * 4u + 0u, y * 4u + 2u);
    pix = vec2(x * 2u + 0u, y * 2u + 1u);
    v[2] = reduce_load_mip_6(tex);
    textureStore(mip_7, pix, vec4(v[2]));

    tex = vec2(x * 4u + 2u, y * 4u + 2u);
    pix = vec2(x * 2u + 1u, y * 2u + 1u);
    v[3] = reduce_load_mip_6(tex);
    textureStore(mip_7, pix, vec4(v[3]));

    if constants.max_mip_level <= 7u { return; }

    let vr = reduce_4(v);
    textureStore(mip_8, vec2(x, y), vec4(vr));
    intermediate_memory[x][y] = vr;
}

fn downsample_mips_8_to_11(x: u32, y: u32, local_invocation_index: u32) {
    if constants.max_mip_level <= 8u { return; }
    workgroupBarrier();
    downsample_mip_8(x, y, local_invocation_index);

    if constants.max_mip_level <= 9u { return; }
    workgroupBarrier();
    downsample_mip_9(x, y, local_invocation_index);

    if constants.max_mip_level <= 10u { return; }
    workgroupBarrier();
    downsample_mip_10(x, y, local_invocation_index);

    if constants.max_mip_level <= 11u { return; }
    workgroupBarrier();
    downsample_mip_11(local_invocation_index);
}

fn downsample_mip_8(x: u32, y: u32, local_invocation_index: u32) {
    if local_invocation_index < 64u {
        let v = reduce_4(vec4(
            intermediate_memory[x * 2u + 0u][y * 2u + 0u],
            intermediate_memory[x * 2u + 1u][y * 2u + 0u],
            intermediate_memory[x * 2u + 0u][y * 2u + 1u],
            intermediate_memory[x * 2u + 1u][y * 2u + 1u],
        ));
        textureStore(mip_9, vec2(x, y), vec4(v));
        intermediate_memory[x * 2u + y % 2u][y * 2u] = v;
    }
}

fn downsample_mip_9(x: u32, y: u32, local_invocation_index: u32) {
    if local_invocation_index < 16u {
        let v = reduce_4(vec4(
            intermediate_memory[x * 4u + 0u + 0u][y * 4u + 0u],
            intermediate_memory[x * 4u + 2u + 0u][y * 4u + 0u],
            intermediate_memory[x * 4u + 0u + 1u][y * 4u + 2u],
            intermediate_memory[x * 4u + 2u + 1u][y * 4u + 2u],
        ));
        textureStore(mip_10, vec2(x, y), vec4(v));
        intermediate_memory[x * 4u + y][y * 4u] = v;
    }
}

fn downsample_mip_10(x: u32, y: u32, local_invocation_index: u32) {
    if local_invocation_index < 4u {
        let v = reduce_4(vec4(
            intermediate_memory[x * 8u + 0u + 0u + y * 2u][y * 8u + 0u],
            intermediate_memory[x * 8u + 4u + 0u + y * 2u][y * 8u + 0u],
            intermediate_memory[x * 8u + 0u + 1u + y * 2u][y * 8u + 4u],
            intermediate_memory[x * 8u + 4u + 1u + y * 2u][y * 8u + 4u],
        ));
        textureStore(mip_11, vec2(x, y), vec4(v));
        intermediate_memory[x + y * 2u][0u] = v;
    }
}

fn downsample_mip_11(local_invocation_index: u32) {
    if local_invocation_index < 1u {
        let v = reduce_4(vec4(
            intermediate_memory[0u][0u],
            intermediate_memory[1u][0u],
            intermediate_memory[2u][0u],
            intermediate_memory[3u][0u],
        ));
        textureStore(mip_12, vec2(0u, 0u), vec4(v));
    }
}

fn remap_for_wave_reduction(a: u32) -> vec2u {
    return vec2(
        insertBits(extractBits(a, 2u, 3u), a, 0u, 1u),
        insertBits(extractBits(a, 3u, 3u), extractBits(a, 1u, 2u), 0u, 2u),
    );
}

fn reduce_load_mip_0(tex: vec2u) -> f32 {
    let a = load_mip_0(tex.x, tex.y);
    let b = load_mip_0(tex.x + 1u, tex.y);
    let c = load_mip_0(tex.x, tex.y + 1u);
    let d = load_mip_0(tex.x + 1u, tex.y + 1u);
    return reduce_4(vec4(a, b, c, d));
}

fn reduce_load_mip_6(tex: vec2u) -> f32 {
    return reduce_4(vec4(
        textureLoad(mip_6, tex + vec2(0u, 0u)).r,
        textureLoad(mip_6, tex + vec2(0u, 1u)).r,
        textureLoad(mip_6, tex + vec2(1u, 0u)).r,
        textureLoad(mip_6, tex + vec2(1u, 1u)).r,
    ));
}

// Loads the top mip level at virtual position (x, y).
//
// This is the value that *would be* returned from sampling a scaled depth
// buffer with side lengths rounded up to the next power of two, without
// actually constructing such a depth buffer.
//
// See the comments in `ViewDepthPyramid::downsample_depth` for more
// information.
fn load_mip_0(x: u32, y: u32) -> f32 {
    let actual_size = textureDimensions(mip_0).xy;
    let virtual_size = vec2<u32>(
        next_power_of_two(actual_size.x),
        next_power_of_two(actual_size.y)
    );
    let virtual_uv = (vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(virtual_size);
#ifdef MESHLET_VISIBILITY_BUFFER_RASTER_PASS_OUTPUT
    let virtual_st = virtual_uv * vec2<f32>(actual_size);
    let visibility = load_mip_0_meshlet(virtual_st, 32u);
    return reduce_4(visibility);
#else   // MESHLET_VISIBILITY_BUFFER_RASTER_PASS_OUTPUT
#ifdef MESHLET
    let virtual_st = virtual_uv * vec2<f32>(actual_size);
    let visibility = load_mip_0_meshlet(virtual_st, 0u);
    return reduce_4(visibility);
#else   // MESHLET
    // Downsample the top level.
#ifdef MULTISAMPLE
    // The top level is multisampled, so we need to loop over all the samples
    // and reduce them to 1.
    let virtual_st = virtual_uv * vec2<f32>(actual_size);
    var result = load_mip_0_single_sample(virtual_st, 0);
    let sample_count = i32(textureNumSamples(mip_0));
    for (var sample = 1; sample < sample_count; sample += 1) {
        result = min(result, load_mip_0_single_sample(virtual_st, sample));
    }
    return result;
#else   // MULTISAMPLE
    return reduce_4(textureGather(mip_0, samplr, virtual_uv));
#endif  // MULTISAMPLE
#endif  // MESHLET
#endif  // MESHLET_VISIBILITY_BUFFER_RASTER_PASS_OUTPUT
}

#ifdef MESHLET
// Loads a single 2×2 square of texels at the given position from the source
// image and returns all four (like `textureGather` does).
//
// `st` should be in texels, not in the [0, 1] range like UVs. That is, `st` is
// `uv * textureDimensions(mip_0).xy`.
fn load_mip_0_meshlet(st: vec2<f32>, shift: u32) -> vec4<f32> {
    let st0 = vec2<u32>(floor(st - 0.5));
    let st1 = st0 + 1u;
    return vec4<f32>(
        bitcast<f32>(u32(textureLoad(mip_0, vec2<u32>(st0.x, st0.y)).r) >> shift),
        bitcast<f32>(u32(textureLoad(mip_0, vec2<u32>(st0.x, st1.y)).r) >> shift),
        bitcast<f32>(u32(textureLoad(mip_0, vec2<u32>(st1.x, st0.y)).r) >> shift),
        bitcast<f32>(u32(textureLoad(mip_0, vec2<u32>(st1.x, st1.y)).r) >> shift)
    );
}
#endif  // MESHLET

#ifdef MULTISAMPLE
// Loads a single 2×2 square of texels at the given position from the source
// image, reduces them, and returns the result.
//
// `st` should be in texels, not in the [0, 1] range like UVs. That is, `st` is
// `uv * textureDimensions(mip_0).xy`.
fn load_mip_0_single_sample(st: vec2<f32>, sample: i32) -> f32 {
    let st0 = vec2<u32>(floor(st - 0.5));
    let st1 = st0 + 1u;
    let v = vec4<f32>(
        textureLoad(mip_0, vec2<u32>(st0.x, st0.y), sample),
        textureLoad(mip_0, vec2<u32>(st0.x, st1.y), sample),
        textureLoad(mip_0, vec2<u32>(st1.x, st0.y), sample),
        textureLoad(mip_0, vec2<u32>(st1.x, st1.y), sample)
    );
    return reduce_4(v);
}
#endif  // MULTISAMPLE

fn reduce_4(v: vec4f) -> f32 {
    return min(min(v.x, v.y), min(v.z, v.w));
}

// Returns the next power of two of x.
//
// If x is itself a power of two, this still returns the *next* power of two.
// This is different from Rust's `next_power_of_two` function.
fn next_power_of_two(x: u32) -> u32 {
    return 1u << (32u - countLeadingZeros(x));
}
