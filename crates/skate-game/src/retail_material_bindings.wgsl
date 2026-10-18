#define_import_path skate_retail::material_bindings

// Material data for a whole texture slab, indexed by a per-vertex material
// index rather than a per-draw bind group slot. That is the entire point of the
// rewrite: one draw can cover thousands of materials (RFC 1 §1).
//
// Two consequences shape this file.
//
// 1. `slot` is non-uniform within a draw, so every texture read uses explicit
//    gradients. Implicit-derivative sampling (`textureSample`) inside a branch
//    keyed on `slot` is invalid WGSL and Naga rejects it.
// 2. Textures live in `texture_2d_array` pages grouped by size class, selected
//    with a `switch`. Array layers are whole textures, so tiling still works —
//    an atlas would bleed across neighbours on repeat.

struct WorldParams {
    mode: vec4<f32>, foliage_debug: vec4<f32>, surface: vec4<f32>, family: vec4<f32>,
    fog_ramp: vec4<f32>, fog_color: vec4<f32>, shadow_color: vec4<f32>, sun_direction: vec4<f32>, decal: vec4<f32>, water: array<vec4<f32>, 4>,
}
struct FrameState { shadow: vec4<f32>, clock: vec4<f32>, pca: array<vec4<f32>, 7>, casters: vec4<f32> }

/// Where each texture channel of one material lives.
///
/// Each field packs `(size_class << 16) | layer`. `ABSENT` means the channel has
/// no texture; callers must gate on the presence bits in `mode.y` exactly as
/// before, since a missing channel has no defined layer to sample.
struct MaterialSlots {
    diffuse: u32,
    lightmap: u32,
    normal_map: u32,
    detail_map: u32,
    macro_map: u32,
    decal_map: u32,
    specular_map: u32,
    environment: u32,
}

const ABSENT: u32 = 0xffffffffu;

// One page per distinct texture size present in the map. A `texture_2d_array`
// requires every layer to share dimensions, and retail packages use more than a
// handful of sizes, so four pages would force rescaling. Eight covers the
// observed spread and still leaves the sampled-texture count well inside
// `max_sampled_textures_per_shader_stage` (16 on the strictest tier we target).
const PAGE_CLASSES: u32 = 8u;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> params: array<WorldParams>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> slots: array<MaterialSlots>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> frame_state_buffer: FrameState;
#ifdef WORLD_PAGE_ARRAY
// `SKATE_WORLD_PAGE_ARRAY`: the same eight pages as one binding array, so each lookup is one
// texture instruction indexed by its page rather than a switch over eight (retail_render.rs
// `WorldMaterial::as_bind_group`). Bindings 4 to 10 go unused.
//
// Before Turing, NVK turns an index that may differ across the subgroup into a loop
// over the distinct pages, which waits for each texture before it exits.
// `WORLD_PAGE_UNIFORM` (`SKATE_WORLD_PAGE_UNIFORM`; findings doc 24 §11) first tries
// the subgroup's first page: when every invocation wants it, the index is uniform
// and the lookup is direct. It needs `Features::SUBGROUP`, so `WorldMaterial::specialize`
// sets it only where the device has that, and only for fragment stages.
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var pages: binding_array<texture_2d_array<f32>, 8>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var page_0: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var page_1: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var page_2: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var page_3: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var page_4: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var page_5: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var page_6: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var page_7: texture_2d_array<f32>;
#endif
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var repeat_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var cubes: texture_cube_array<f32>;
#ifdef WORLD_FETCH
// `SKATE_WORLD_FETCH`'s copy of the world's vertices and triangles, which the
// main pass reads in place of interpolated values (retail_world.wgsl
// `fetch_triangle`, skate_world.rs `FetchGeometry`).
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var<storage, read> fetch_vertices: array<vec4<u32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var<storage, read> fetch_triangles: array<vec4<u32>>;
#endif
fn page_class(packed: u32) -> u32 { return (packed >> 16u) & 0xffu; }
fn page_layer(packed: u32) -> u32 { return packed & 0xffffu; }

/// The page a lookup reads. `WORLD_PROBE_ONE_PAGE` (`SKATE_WORLD_PROBE=5`), for measuring only,
/// reads the first for every channel. Each switch below then folds to its one case, so the
/// shader keeps a texture instruction for each lookup but loses the other seven, and with
/// them most of its code. The layer is clamped to that page's, so the picture is wrong.
fn page_select(packed: u32) -> u32 {
#ifdef WORLD_PROBE_ONE_PAGE
    return 0u;
#else
    return page_class(packed);
#endif
}

/// Clamped channels — lightmaps, decals and authored clamp-addressed diffuse —
/// saturate their coordinates instead of using a second sampler. `AsBindGroup`
/// derives a sampler from an image's own descriptor, so a clamp sampler would
/// need a second carrier image; the difference from `ClampToEdge` is confined to
/// the outer half-texel, whereas leaving these on repeat would tile a decal
/// across its whole surface.
fn page_uv(packed: u32, uv: vec2<f32>) -> vec2<f32> {
    return select(uv, saturate(uv), (packed & 0x80000000u) != 0u);
}

/// Textures and base UVs both have V flipped by the exporter. Scale in the
/// authored coordinate system, then return to the flipped texture rows.
fn scaled_uv(uv: vec2<f32>, scale: f32) -> vec2<f32> {
    return vec2<f32>(uv.x*scale, 1.0-(1.0-uv.y)*scale);
}

/// Gradients for a UV set, carried from uniform control flow into the branches.
///
/// A derived UV is an affine function of the base UV, so its gradients are the
/// same transform applied to the base gradients — which is why the caller can
/// build these before knowing which family branch will run.
struct Gradients { ddx: vec2<f32>, ddy: vec2<f32> }

fn gradients(uv: vec2<f32>) -> Gradients {
    return Gradients(dpdx(uv), dpdy(uv));
}

fn gradients_scaled(g: Gradients, scale: f32) -> Gradients {
    return Gradients(g.ddx*scale, g.ddy*scale);
}

#ifdef WORLD_EXPLICIT_LOD
// Each page's size in texels (retail_render.rs `WorldMaterial::page_sizes`).
const PAGE_SIZES = array<vec2<f32>, 8>(
    vec2<f32>(f32(#{WORLD_PAGE_0_W}), f32(#{WORLD_PAGE_0_H})),
    vec2<f32>(f32(#{WORLD_PAGE_1_W}), f32(#{WORLD_PAGE_1_H})),
    vec2<f32>(f32(#{WORLD_PAGE_2_W}), f32(#{WORLD_PAGE_2_H})),
    vec2<f32>(f32(#{WORLD_PAGE_3_W}), f32(#{WORLD_PAGE_3_H})),
    vec2<f32>(f32(#{WORLD_PAGE_4_W}), f32(#{WORLD_PAGE_4_H})),
    vec2<f32>(f32(#{WORLD_PAGE_5_W}), f32(#{WORLD_PAGE_5_H})),
    vec2<f32>(f32(#{WORLD_PAGE_6_W}), f32(#{WORLD_PAGE_6_H})),
    vec2<f32>(f32(#{WORLD_PAGE_7_W}), f32(#{WORLD_PAGE_7_H})),
);

/// The level of detail sampling `page` with gradients `g` picks when
/// anisotropic filtering is off, as it is for `repeat_sampler`: log2 of the
/// longer of the two screen axes' footprints in texels (the Vulkan
/// specification's scale factor, ρ = max(ρx, ρy)).
fn page_level(page: u32, g: Gradients) -> f32 {
    let size = PAGE_SIZES[min(page, 7u)];
    let x = g.ddx * size;
    let y = g.ddy * size;
    return 0.5 * log2(max(dot(x, x), dot(y, y)));
}
#endif

// Keep resource-array access at the sampling sites. Passing a selected resource
// into a shading function crashes the tested Vulkan pipeline compiler.
//
// Under `WORLD_EXPLICIT_LOD` (`SKATE_WORLD_LOD`) the page is sampled at the
// level its gradients give (`page_level`) instead of with them: the texture
// instruction then takes a layer, two coordinates and a level rather than a
// layer, two coordinates and four gradient components. The level is worked out
// before the switch, so only it is live across the cases; worked out in each
// case, the gradients were, and the world fragment took 72 registers rather
// than 64 (NAK, SM53).
//
// Under `WORLD_IMPLICIT_LOD` (`SKATE_WORLD_IMPLICIT`) the page is sampled with
// `textureSample`, so the texture unit works out the level from the quad itself,
// and the gradients go unused. That holds wherever a lookup runs: every branch
// above one tests only the material, and a 2x2 quad's four lanes share a
// triangle and so a material, so they reach each lookup together (findings 31).
// The level can differ from the gradients' in the last bits, so the picture can
// differ by a shade where a pixel sits between two levels. Where `page_uv`
// clamps, the derivatives are of the clamped coordinates. naga 27 doesn't hold
// derivatives to uniform control flow in fragment stages, so it accepts this,
// where the WGSL specification, as the note at the top of this file says, would not.
//
// `WORLD_PROBE_TEXELS` (`SKATE_WORLD_PROBE=3`) is for measuring only. It
// stretches the gradients, and pins the level-sampled reads, so that every
// lookup lands on a page's smallest level. The texture instructions, the
// switches and the shading arithmetic all stay. What goes is the traffic for
// texels, which then sit in cache.
//
// `WORLD_PROBE_NO_TEXTURES` (`SKATE_WORLD_PROBE=4`), also for measuring only,
// goes further: no page or cube is read at all. Each lookup returns a value
// made from its coordinates, so the shading arithmetic still has something to
// work on, and the gradients, switches and texture instructions all go.
#ifdef WORLD_PROBE_NO_TEXTURES
fn probe_texel(uv: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(fract(uv), 0.5, 1.0);
}
#endif

fn sample_page(packed: u32, uv_in: vec2<f32>, g_in: Gradients) -> vec4<f32> {
#ifdef WORLD_PROBE_NO_TEXTURES
    return probe_texel(uv_in);
#else
#ifdef WORLD_PROBE_TEXELS
    let g = gradients_scaled(g_in, 65536.0);
#else
    let g = g_in;
#endif
    let layer = page_layer(packed);
    let uv = page_uv(packed, uv_in);
#ifdef WORLD_PAGE_ARRAY
    let page = page_select(packed);
    if page >= PAGE_CLASSES { return vec4<f32>(0.0,0.0,0.0,1.0); }
#ifdef WORLD_PAGE_UNIFORM
    let first = subgroupBroadcastFirst(page);
    if subgroupAll(page == first) {
#ifdef WORLD_EXPLICIT_LOD
        return textureSampleLevel(pages[first],repeat_sampler,uv,layer,page_level(first, g));
#else ifdef WORLD_IMPLICIT_LOD
        return textureSample(pages[first],repeat_sampler,uv,layer);
#else
        return textureSampleGrad(pages[first],repeat_sampler,uv,layer,g.ddx,g.ddy);
#endif
    }
#endif
#ifdef WORLD_EXPLICIT_LOD
    return textureSampleLevel(pages[page],repeat_sampler,uv,layer,page_level(page, g));
#else ifdef WORLD_IMPLICIT_LOD
    return textureSample(pages[page],repeat_sampler,uv,layer);
#else
    return textureSampleGrad(pages[page],repeat_sampler,uv,layer,g.ddx,g.ddy);
#endif
#else
#ifdef WORLD_EXPLICIT_LOD
    let page = page_select(packed);
    let level = page_level(page, g);
    switch page {
        case 0u: { return textureSampleLevel(page_0,repeat_sampler,uv,layer,level); }
        case 1u: { return textureSampleLevel(page_1,repeat_sampler,uv,layer,level); }
        case 2u: { return textureSampleLevel(page_2,repeat_sampler,uv,layer,level); }
        case 3u: { return textureSampleLevel(page_3,repeat_sampler,uv,layer,level); }
        case 4u: { return textureSampleLevel(page_4,repeat_sampler,uv,layer,level); }
        case 5u: { return textureSampleLevel(page_5,repeat_sampler,uv,layer,level); }
        case 6u: { return textureSampleLevel(page_6,repeat_sampler,uv,layer,level); }
        case 7u: { return textureSampleLevel(page_7,repeat_sampler,uv,layer,level); }
        default: { return vec4<f32>(0.0,0.0,0.0,1.0); }
    }
#else ifdef WORLD_IMPLICIT_LOD
    switch page_select(packed) {
        case 0u: { return textureSample(page_0,repeat_sampler,uv,layer); }
        case 1u: { return textureSample(page_1,repeat_sampler,uv,layer); }
        case 2u: { return textureSample(page_2,repeat_sampler,uv,layer); }
        case 3u: { return textureSample(page_3,repeat_sampler,uv,layer); }
        case 4u: { return textureSample(page_4,repeat_sampler,uv,layer); }
        case 5u: { return textureSample(page_5,repeat_sampler,uv,layer); }
        case 6u: { return textureSample(page_6,repeat_sampler,uv,layer); }
        case 7u: { return textureSample(page_7,repeat_sampler,uv,layer); }
        default: { return vec4<f32>(0.0,0.0,0.0,1.0); }
    }
#else
    switch page_select(packed) {
        case 0u: { return textureSampleGrad(page_0,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 1u: { return textureSampleGrad(page_1,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 2u: { return textureSampleGrad(page_2,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 3u: { return textureSampleGrad(page_3,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 4u: { return textureSampleGrad(page_4,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 5u: { return textureSampleGrad(page_5,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 6u: { return textureSampleGrad(page_6,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        case 7u: { return textureSampleGrad(page_7,repeat_sampler,uv,layer,g.ddx,g.ddy); }
        default: { return vec4<f32>(0.0,0.0,0.0,1.0); }
    }
#endif
#endif // WORLD_PAGE_ARRAY
#endif // WORLD_PROBE_NO_TEXTURES
}

/// Level-sampled and clamped: lightmaps are unique per surface, never tiled, and
/// the reference always reads mip 0.
fn sample_page_level(packed: u32, uv_in: vec2<f32>, level_in: f32) -> vec4<f32> {
#ifdef WORLD_PROBE_NO_TEXTURES
    return probe_texel(uv_in);
#else
#ifdef WORLD_PROBE_TEXELS
    let level = 16.0;
#else
    let level = level_in;
#endif
    let layer = page_layer(packed);
    let uv = page_uv(packed, uv_in);
#ifdef WORLD_PAGE_ARRAY
    let page = page_select(packed);
    if page >= PAGE_CLASSES { return vec4<f32>(1.0); }
#ifdef WORLD_PAGE_UNIFORM
    let first = subgroupBroadcastFirst(page);
    if subgroupAll(page == first) {
        return textureSampleLevel(pages[first],repeat_sampler,uv,layer,level);
    }
#endif
    return textureSampleLevel(pages[page],repeat_sampler,uv,layer,level);
#else
    switch page_select(packed) {
        case 0u: { return textureSampleLevel(page_0,repeat_sampler,uv,layer,level); }
        case 1u: { return textureSampleLevel(page_1,repeat_sampler,uv,layer,level); }
        case 2u: { return textureSampleLevel(page_2,repeat_sampler,uv,layer,level); }
        case 3u: { return textureSampleLevel(page_3,repeat_sampler,uv,layer,level); }
        case 4u: { return textureSampleLevel(page_4,repeat_sampler,uv,layer,level); }
        case 5u: { return textureSampleLevel(page_5,repeat_sampler,uv,layer,level); }
        case 6u: { return textureSampleLevel(page_6,repeat_sampler,uv,layer,level); }
        case 7u: { return textureSampleLevel(page_7,repeat_sampler,uv,layer,level); }
        default: { return vec4<f32>(1.0); }
    }
#endif // WORLD_PAGE_ARRAY
#endif // WORLD_PROBE_NO_TEXTURES
}

fn load_page(packed: u32, uv: vec2<i32>, level: i32) -> vec4<f32> {
#ifdef WORLD_PROBE_NO_TEXTURES
    return vec4<f32>(0.5);
#else
    let layer = i32(page_layer(packed));
#ifdef WORLD_PAGE_ARRAY
    let page = page_select(packed);
    if page >= PAGE_CLASSES { return vec4<f32>(0.5); }
#ifdef WORLD_PAGE_UNIFORM
    let first = subgroupBroadcastFirst(page);
    if subgroupAll(page == first) {
        return textureLoad(pages[first],uv,layer,level);
    }
#endif
    return textureLoad(pages[page],uv,layer,level);
#else
    switch page_select(packed) {
        case 0u: { return textureLoad(page_0,uv,layer,level); }
        case 1u: { return textureLoad(page_1,uv,layer,level); }
        case 2u: { return textureLoad(page_2,uv,layer,level); }
        case 3u: { return textureLoad(page_3,uv,layer,level); }
        case 4u: { return textureLoad(page_4,uv,layer,level); }
        case 5u: { return textureLoad(page_5,uv,layer,level); }
        case 6u: { return textureLoad(page_6,uv,layer,level); }
        case 7u: { return textureLoad(page_7,uv,layer,level); }
        default: { return vec4<f32>(0.5); }
    }
#endif // WORLD_PAGE_ARRAY
#endif
}

fn page_dimensions(packed: u32) -> vec2<u32> {
#ifdef WORLD_PAGE_ARRAY
    let page = page_select(packed);
    if page >= PAGE_CLASSES { return vec2<u32>(1u); }
#ifdef WORLD_PAGE_UNIFORM
    let first = subgroupBroadcastFirst(page);
    if subgroupAll(page == first) {
        return textureDimensions(pages[first]);
    }
#endif
    return textureDimensions(pages[page]);
#else
    switch page_select(packed) {
        case 0u: { return textureDimensions(page_0); }
        case 1u: { return textureDimensions(page_1); }
        case 2u: { return textureDimensions(page_2); }
        case 3u: { return textureDimensions(page_3); }
        case 4u: { return textureDimensions(page_4); }
        case 5u: { return textureDimensions(page_5); }
        case 6u: { return textureDimensions(page_6); }
        case 7u: { return textureDimensions(page_7); }
        default: { return vec2<u32>(1u); }
    }
#endif // WORLD_PAGE_ARRAY
}

// --- Channel accessors, matching the reference surface -----------------------
//
// They take the material's slots rather than its index. The world fragment
// hands them over where it samples, so each channel's word is loaded there,
// or, under `WORLD_HOIST`, all of them once at the top (retail_world.wgsl).

fn frame_state() -> FrameState { return frame_state_buffer; }

// `shadow` is read where it is used rather than through `frame_state()`. Copied
// at the top of the world fragment it was one vec4 load, so `rgb` stayed live
// from the early `w` test to the one late add: three registers at the peak.
fn shadow_enabled() -> bool { return frame_state_buffer.shadow.w > 0.0; }
fn shadow_floor() -> vec3<f32> { return frame_state_buffer.shadow.rgb; }

/// Whether the layer-28 casters can shadow `position` from the light shining
/// along `-to_light`. `state.casters` is a sphere round all of them
/// (retail_shadow_bounds.rs `caster_bounds`); outside its shadow, that light's map
/// holds nothing that can darken a lookup. `w` is its radius, 0 when the casters
/// are not bounded (every lookup is made), and negative when there are none.
fn near_casters(state: FrameState, position: vec3<f32>, to_light: vec3<f32>) -> bool {
    let bounds = state.casters;
    if bounds.w == 0.0 { return true; }
    let offset = position - bounds.xyz;
    let along = dot(offset, to_light);
    return bounds.w > 0.0 && along <= bounds.w
        && dot(offset, offset) - along * along <= bounds.w * bounds.w;
}

fn sample_diffuse(channels: MaterialSlots, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(channels.diffuse, uv, g);
}

fn sample_normal_map(channels: MaterialSlots, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(channels.normal_map, uv, g);
}

fn sample_detail_map(channels: MaterialSlots, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(channels.detail_map, uv, g);
}

fn sample_macro_map(channels: MaterialSlots, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(channels.macro_map, uv, g);
}

fn sample_decal_map(channels: MaterialSlots, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(channels.decal_map, uv, g);
}

fn sample_specular_map(channels: MaterialSlots, uv: vec2<f32>, g: Gradients) -> vec4<f32> {
    return sample_page(channels.specular_map, uv, g);
}

fn sample_lightmap(channels: MaterialSlots, uv: vec2<f32>, level: f32) -> vec4<f32> {
    return sample_page_level(channels.lightmap, uv, level);
}

fn load_detail(channels: MaterialSlots, uv: vec2<i32>, level: i32) -> vec4<f32> {
    return load_page(channels.detail_map, uv, level);
}

fn lightmap_dimensions(channels: MaterialSlots) -> vec2<u32> {
    return page_dimensions(channels.lightmap);
}

/// `textureSampleBias` in the reference; bias cannot be used under non-uniform
/// control flow, so the caller's mip bias becomes a gradient scale instead.
/// `exp2(bias)` is the mip-ratio the bias represented.
fn sample_environment(channels: MaterialSlots, direction: vec3<f32>, bias: f32) -> vec4<f32> {
    let packed = channels.environment;
    if packed == ABSENT { return vec4<f32>(0.0); }
#ifdef WORLD_PROBE_NO_TEXTURES
    return vec4<f32>(direction * 0.5 + 0.5, 1.0);
#else
    let scale = exp2(bias);
    let ddx = dpdx(direction)*scale;
    let ddy = dpdy(direction)*scale;
    return textureSampleGrad(cubes,repeat_sampler,direction,page_layer(packed),ddx,ddy);
#endif
}
