#import bevy_pbr::view_transformations::{position_world_to_clip, frag_coord_to_ndc}
#import bevy_pbr::mesh_view_bindings as frame
#import bevy_pbr::shadows::fetch_directional_shadow
#import skate_retail::material_bindings as bindings
#ifdef WORLD_FETCH
#import bevy_pbr::mesh_bindings::mesh
#endif

// Merged world geometry: one draw covers many materials, each vertex naming its
// own. `material_index` must be `@interpolate(flat)` — interpolating it corrupts
// material lookup across triangle interiors without any visible error.
//
// The fragment body below is a direct port. The only edits are `slot` coming from
// the vertex attribute rather than the mesh instance, and explicit gradients
// replacing implicit derivatives (see RFC 1 §7). The family math is unchanged.
//
// The water families (14 and 30-33) are compiled only into the `WORLD_WATER`
// variant, which draws the leaves holding water materials (`skate_world::spawn`
// buckets them apart). Their branches are most of this shader's code and hold
// the most values live at once, and every other fragment paid for that in
// registers and occupancy (findings doc 20).

// `WORLD_FEATURES` (`SKATE_WORLD_FEATURES`, retail_render.rs `world_features`): which parts of
// the fragment below some material drawn with this pipeline reaches, a bit each. A part none
// of them reaches is compiled out, with whatever only it reads: the tangent frame goes with
// the normal map. Each bit stands for the condition the fragment tests before it enters that
// part, so every fragment takes the same path, and gets the same value, as without it. The
// water families' own branches test their channels themselves and are not gated. Unset,
// every part is compiled.
#ifdef WORLD_FEATURES
const FEATURES: u32 = u32(#{WORLD_FEATURES});
#else
const FEATURES: u32 = 0xffffffffu;
#endif
// fam <= 8, 13 or 16: the skater's shadow.
const HAS_SHADOW: bool = (FEATURES & 1u) != 0u;
// flags & 1, and fam <= 6 or 13: the normal map, and the tangent frame it is applied in.
const HAS_NORMAL: bool = (FEATURES & 2u) != 0u;
// flags & 2, and fam not 2: the detail map.
const HAS_DETAIL: bool = (FEATURES & 4u) != 0u;
// flags & 128, and fam 5, 6 or 13: the constant detail texel.
const HAS_LOAD_DETAIL: bool = (FEATURES & 8u) != 0u;
// flags & 4: the macro overlay.
const HAS_MACRO: bool = (FEATURES & 16u) != 0u;
// flags & 8, and fam 3 or 4: the decal.
const HAS_DECAL: bool = (FEATURES & 32u) != 0u;
// flags & 16: the specular map.
const HAS_SPECULAR: bool = (FEATURES & 64u) != 0u;
// flags & 64, and fam 5, 6 or 13: the environment cube.
const HAS_ENVIRONMENT: bool = (FEATURES & 128u) != 0u;

#ifdef WORLD_FETCH
// `WORLD_FETCH` (`SKATE_WORLD_FETCH`): the vertex stage reads only the position,
// and passes on one flat number, where its leaf's triangles start in
// `bindings::fetch_triangles` (the leaf's `MeshTag`). The fragment stage adds
// `primitive_index` to that to find its triangle, and builds the same `Varyings`
// from the triangle's corners (`fetch_triangle`). The position stays
// `@invariant`, for the reason given below.
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) @interpolate(flat) first_triangle: u32,
}

struct FragmentInput {
    @builtin(position) @invariant frag_coord: vec4<f32>,
    @location(0) @interpolate(flat) first_triangle: u32,
    @builtin(primitive_index) primitive_index: u32,
}
#else
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    // The decal UVs. Packed meshes carry only these two components, the
    // unpacked ones a colour whose other two nothing reads; normals and
    // tangents arrive as Snorm16 when packed (skate_world.rs
    // `ATTRIBUTE_PACKED_NORMAL`).
    @location(4) color: vec2<f32>,
    @location(5) material_index: u32,
    // Zero where the map authored no frame; the fragment falls back to
    // screen-space derivatives per vertex rather than per mesh.
    @location(6) tangent: vec4<f32>,
}

// `world_position` drops the constant w and `color` keeps only the decal UVs in
// `.xy`, the only part the fragment reads: three fewer interpolated components
// for every vertex and fragment (findings doc 20). The position is `@invariant`
// so that it comes out bit for bit as `retail_depth.wgsl` computes it: under a
// depth prepass, the cutout classes are drawn only where their depth equals the
// prepass's (`WorldMaterial::specialize`).
//
// `WORLD_PACKED_VARYINGS` (`SKATE_WORLD_PACKED`, on unless set to 0) carries the
// same seventeen components four to a location, in five locations rather than
// seven: `(world_position, uv.x)`, `(world_normal, uv.y)`, `(uv_b, color)`, the
// tangent and the material index. Each component is interpolated on its own
// either way, so the fragment receives the same values. `WORLD_PROBE_GEOMETRY`
// (`SKATE_WORLD_PROBE=2`) carries none, for measuring.
struct VertexOutput {
    @builtin(position) @invariant clip_position: vec4<f32>,
#ifndef WORLD_PROBE_GEOMETRY
#ifdef WORLD_PACKED_VARYINGS
    @location(0) position_u: vec4<f32>,
    @location(1) normal_v: vec4<f32>,
    @location(2) uv_b_color: vec4<f32>,
    @location(3) world_tangent: vec4<f32>,
    @location(4) @interpolate(flat) material_index: u32,
#else
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(4) color: vec2<f32>,
    @location(5) @interpolate(flat) material_index: u32,
    @location(6) world_tangent: vec4<f32>,
#endif
#endif
}
#endif // WORLD_FETCH

// What the fragment reads, under the names it reads them by, however the vertex
// stage carried them.
struct Varyings {
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
    uv: vec2<f32>,
    uv_b: vec2<f32>,
    color: vec2<f32>,
    material_index: u32,
    world_tangent: vec4<f32>,
}

#ifdef WORLD_FETCH
// A corner's record is four `vec4<u32>` (skate_world.rs `FetchGeometry`): its
// position; its UV and lightmap UV; its decal UV and normal; its tangent.
fn fetch_vertex(corner: u32, record: u32) -> vec4<u32> {
    return bindings::fetch_vertices[corner * 4u + record];
}

// What `vertex` does to each corner's normal and tangent before the rasterizer
// interpolates them, so they are interpolated as the same values.
fn corner_normal(words: vec2<u32>) -> vec3<f32> {
    let n = vec3<f32>(unpack2x16snorm(words.x), unpack2x16snorm(words.y).x);
    return select(n, normalize(n), any(n != vec3<f32>(0.0)));
}

fn corner_tangent(words: vec2<u32>) -> vec4<f32> {
    let t = vec4<f32>(unpack2x16snorm(words.x), unpack2x16snorm(words.y));
    return select(t, vec4<f32>(normalize(t.xyz), t.w), any(t != vec4<f32>(0.0)));
}

// The weights the rasterizer interpolates a triangle's corners with at the pixel
// `frag_coord`: its perspective-correct barycentric coordinates. Clip space is an
// affine image of world space, so the triangle's point seen through the pixel at
// `ndc` is the corners' weighted sum, weights summing to one, whose clip x and y
// are `ndc` times its clip w. Each corner's `q` is what it adds to the two
// residuals, and the weights that cancel them are, up to scale, the 2D cross
// products of the other corners' `q`. Those are the edge functions of Olano and
// Greer's 2D homogeneous rasterization (1997) at the pixel, taken in an order
// that keeps the precision of a small, distant triangle: the pixel's ray comes
// off each corner first, where forming the products of whole clip positions
// first would cancel most of their digits. Nothing is divided by a corner's w, so
// a corner behind the camera needs no special case. A quad's helper lanes are in
// the same triangle, and their weights extrapolate it as the rasterizer's do, so
// the derivatives the shading takes across the quad are the triangle's.
fn barycentrics(frag_coord: vec4<f32>, p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>) -> vec3<f32> {
    let ndc = frag_coord_to_ndc(frag_coord).xy;
    let c0 = position_world_to_clip(p0);
    let c1 = position_world_to_clip(p1);
    let c2 = position_world_to_clip(p2);
    let q0 = c0.xy - ndc * c0.w;
    let q1 = c1.xy - ndc * c1.w;
    let q2 = c2.xy - ndc * c2.w;
    let k = vec3<f32>(
        q1.x * q2.y - q1.y * q2.x,
        q2.x * q0.y - q2.y * q0.x,
        q0.x * q1.y - q0.y * q1.x);
    let sum = k.x + k.y + k.z;
    // Zero only for a triangle seen edge-on, of which the rasterizer draws nothing.
    return select(vec3<f32>(1.0, 0.0, 0.0), k / sum, sum != 0.0);
}

struct Fetched {
    varyings: Varyings,
    weights: vec3<f32>,
}

// The fragment's triangle, and what the vertex stage and the rasterizer would
// have given it. The material comes from the triangle's record, which holds the
// first corner's, the one flat interpolation takes.
fn fetch_triangle(o: FragmentInput) -> Fetched {
    let triangle = bindings::fetch_triangles[o.first_triangle + o.primitive_index];
    let p0 = bitcast<vec3<f32>>(fetch_vertex(triangle.x, 0u).xyz);
    let p1 = bitcast<vec3<f32>>(fetch_vertex(triangle.y, 0u).xyz);
    let p2 = bitcast<vec3<f32>>(fetch_vertex(triangle.z, 0u).xyz);
    let w = barycentrics(o.frag_coord, p0, p1, p2);
    let uvs = bitcast<vec4<f32>>(fetch_vertex(triangle.x, 1u)) * w.x
        + bitcast<vec4<f32>>(fetch_vertex(triangle.y, 1u)) * w.y
        + bitcast<vec4<f32>>(fetch_vertex(triangle.z, 1u)) * w.z;
    let s0 = fetch_vertex(triangle.x, 2u);
    let s1 = fetch_vertex(triangle.y, 2u);
    let s2 = fetch_vertex(triangle.z, 2u);
    let color = bitcast<vec2<f32>>(s0.xy) * w.x + bitcast<vec2<f32>>(s1.xy) * w.y
        + bitcast<vec2<f32>>(s2.xy) * w.z;
    let normal = corner_normal(s0.zw) * w.x + corner_normal(s1.zw) * w.y
        + corner_normal(s2.zw) * w.z;
    let tangent = corner_tangent(fetch_vertex(triangle.x, 3u).xy) * w.x
        + corner_tangent(fetch_vertex(triangle.y, 3u).xy) * w.y
        + corner_tangent(fetch_vertex(triangle.z, 3u).xy) * w.z;
    return Fetched(
        Varyings(p0 * w.x + p1 * w.y + p2 * w.z, normal, uvs.xy, uvs.zw, color, triangle.w, tangent),
        w);
}
#else
#ifndef WORLD_PROBE_GEOMETRY
fn unpack(o: VertexOutput) -> Varyings {
#ifdef WORLD_PACKED_VARYINGS
    return Varyings(
        o.position_u.xyz,
        o.normal_v.xyz,
        vec2<f32>(o.position_u.w, o.normal_v.w),
        o.uv_b_color.xy,
        o.uv_b_color.zw,
        o.material_index,
        o.world_tangent);
#else
    return Varyings(
        o.world_position, o.world_normal, o.uv, o.uv_b, o.color, o.material_index, o.world_tangent);
#endif
}
#endif
#endif // WORLD_FETCH

#ifdef WORLD_PROBE_VARYINGS
// `SKATE_WORLD_PROBE=1`: every varying is read and folded into a false colour,
// and none of the shading is done. Measures the main pass less its fragment work.
fn probe_varyings(i: Varyings) -> vec4<f32> {
    let sum = dot(i.world_position, vec3<f32>(1.0)) + dot(i.world_normal, vec3<f32>(1.0))
        + i.uv.x + i.uv.y + i.uv_b.x + i.uv_b.y + i.color.x + i.color.y
        + f32(i.material_index) + dot(i.world_tangent, vec4<f32>(1.0));
    return vec4<f32>(fract(sum * vec3<f32>(0.37, 0.61, 0.83)), 1.0);
}
#endif

// World leaves are spawned at the identity transform (`skate_world::spawn`), so
// local space is world space and the mesh's transform is never read. Bevy's
// `mesh_functions` would multiply by the identity, which changes no bit of the
// result, after loading it from the mesh uniform for every vertex. The
// normalisation and zero checks are the ones `mesh_normal_local_to_world` and
// `mesh_tangent_local_to_world` make, and the determinant sign they apply to
// `tangent.w` is +1 for the identity. `retail_depth.wgsl` computes the clip
// position with the same expression, which a depth prepass relies on.
@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = position_world_to_clip(v.position);
#ifdef WORLD_FETCH
    out.first_triangle = mesh[v.instance_index].tag;
#else
#ifndef WORLD_PROBE_GEOMETRY
    // A zero normal stays zero rather than becoming NaN.
    let world_normal = select(v.normal, normalize(v.normal), any(v.normal != vec3<f32>(0.0)));
    // Leaves a zero tangent zero, which the fragment tests for.
    let world_tangent = select(
        v.tangent,
        vec4<f32>(normalize(v.tangent.xyz), v.tangent.w),
        any(v.tangent != vec4<f32>(0.0)));
#ifdef WORLD_PACKED_VARYINGS
    out.position_u = vec4<f32>(v.position, v.uv.x);
    out.normal_v = vec4<f32>(world_normal, v.uv.y);
    out.uv_b_color = vec4<f32>(v.uv_b, v.color);
#else
    out.world_position = v.position;
    out.world_normal = world_normal;
    out.uv = v.uv;
    out.uv_b = v.uv_b;
    out.color = v.color;
#endif
    out.material_index = v.material_index;
    out.world_tangent = world_tangent;
#endif
#endif // WORLD_FETCH
    return out;
}

#ifndef WORLD_PROBE_GEOMETRY
#ifndef WORLD_PROBE_VARYINGS
#ifdef WORLD_SHORT_LIVES
struct TangentFrame { t: vec3<f32>, b: vec3<f32> }

// `shade`'s tangent frame, built in the branches that use it rather than for every fragment.
// Its derivatives hold there: those branches test only `fam` and `flags`, which are the same
// across a 2x2 quad, as its four lanes share a triangle and so a material.
#ifdef WORLD_LEAN
// `WORLD_LEAN` (`SKATE_WORLD_LEAN`) builds the stand-in from derivatives only where the map
// supplied no frame (skate_world.rs `tangent`), rather than everywhere and then replacing it,
// and takes the UV's derivatives from `g`: `dpdx(uv * (1, -1))` is `dpdx(uv) * (1, -1)`
// exactly, as a difference of negated values is the negated difference. The position's
// derivatives are still taken before the branch: its test reads interpolated values, so it
// can differ within a quad, and a derivative needs all four lanes.
fn tangent_frame(i: Varyings, wn: vec3<f32>, g: bindings::Gradients) -> TangentFrame {
    let dp1 = dpdx(i.world_position);
    let dp2 = dpdy(i.world_position);
    if dot(i.world_tangent.xyz,i.world_tangent.xyz) > 0.01 {
        let t = normalize(i.world_tangent.xyz);
        return TangentFrame(t, normalize(cross(wn,t)) * i.world_tangent.w);
    }
    let du1 = g.ddx * vec2<f32>(1.0,-1.0);
    let du2 = g.ddy * vec2<f32>(1.0,-1.0);
    let dp2p = cross(dp2,wn);
    let dp1p = cross(wn,dp1);
    var kt = dp2p * du1.x + dp1p * du2.x;
    var kb = dp2p * du1.y + dp1p * du2.y;
    kt *= -inverseSqrt(max(dot(kt,kt),1e-12));
    kb *= inverseSqrt(max(dot(kb,kb),1e-12));
    return TangentFrame(kt, kb);
}
#else
fn tangent_frame(i: Varyings, wn: vec3<f32>, g: bindings::Gradients) -> TangentFrame {
    let dp1 = dpdx(i.world_position);
    let dp2 = dpdy(i.world_position);
    // Exporter flips V and texture rows; recover original UV derivatives.
    let du1 = dpdx(i.uv * vec2<f32>(1.0,-1.0));
    let du2 = dpdy(i.uv * vec2<f32>(1.0,-1.0));
    let dp2p = cross(dp2,wn);
    let dp1p = cross(wn,dp1);
    var kt = dp2p * du1.x + dp1p * du2.x;
    var kb = dp2p * du1.y + dp1p * du2.y;
    kt *= -inverseSqrt(max(dot(kt,kt),1e-12));
    kb *= inverseSqrt(max(dot(kb,kb),1e-12));
    // Prefer the authored frame where the map supplied one. The derivative basis
    // above is only a stand-in, and it disagrees with the authored orientation on
    // surfaces whose UVs were laid out by hand.
    if dot(i.world_tangent.xyz,i.world_tangent.xyz) > 0.01 {
        kt = normalize(i.world_tangent.xyz);
        kb = normalize(cross(wn,kt)) * i.world_tangent.w;
    }
    return TangentFrame(kt, kb);
}
#endif

#endif
// One fragment's shading, as material `slot` (`fragment` below).
fn shade(i: Varyings, slot: u32) -> vec4<f32> {
    let world_position = vec4<f32>(i.world_position, 1.0);
    // Each parameter and channel word is loaded where it is used. Loaded
    // together here, as `WORLD_HOIST` (`SKATE_WORLD_HOIST`) does, they are all
    // live across the whole fragment, which then takes 96 registers instead of
    // 64 and runs 20 warps at once instead of 32 (findings doc 21).
#ifdef WORLD_HOIST
    var hoisted_params = bindings::params[slot];
    var hoisted_channels = bindings::slots[slot];
    let p = &hoisted_params;
    let channels = &hoisted_channels;
#else
    let p = &bindings::params[slot];
    let channels = &bindings::slots[slot];
#endif
    let frame_state = bindings::frame_state();
    let fam = u32(p.mode.x);
    let flags = u32(p.mode.y);

    // The skater's shadow is looked up first, while little else is live, and
    // only where a caster can reach (`near_casters`): elsewhere it is 1.
    var visibility = 1.0;
    var shadowed = false;
#ifdef WORLD_SHORT_LIVES
    // Normalised once, here, so the raw normal isn't live beside it through the lookup.
    let n0 = normalize(i.world_normal);
#endif
    if HAS_SHADOW && bindings::shadow_enabled() && (fam<=8u || fam==13u || fam==16u) {
        for (var light_id=0u; light_id<frame::lights.n_directional_lights; light_id+=1u) {
            // The lightmapped receiver source contains only player/board casters.
            if (frame::lights.directional_lights[light_id].flags & 5u)==5u {
                let to_light=frame::lights.directional_lights[light_id].direction_to_light;
                if bindings::near_casters(frame_state,i.world_position,to_light) {
                    let view_z=(frame::view.view_from_world*world_position).z;
#ifdef WORLD_SHORT_LIVES
                    visibility=fetch_directional_shadow(light_id,world_position,n0,view_z);
#else
                    visibility=fetch_directional_shadow(light_id,world_position,normalize(i.world_normal),view_z);
#endif
                }
                shadowed = true;
                break;
            }
        }
    }

#ifdef WORLD_SHORT_LIVES
    // `WORLD_SHORT_LIVES` (`SKATE_WORLD_SHORT_LIVES`) does the arithmetic below in the same
    // order on the same values, but reads each texture where its value is folded in, so fewer
    // values are live at once. The decal comes first, as its gradients would otherwise be live
    // beside everything sampled after it. Each derived gradient is built in the branch that
    // reads it: `fam` and `flags` are the same across a 2x2 quad, which shares a triangle and so
    // a material, so its derivatives hold there.
    let g = bindings::gradients(i.uv);
    var art = vec4<f32>(0.0);
    if HAS_DECAL && (flags & 8u) != 0u && (fam == 3u || fam == 4u) {
        art = bindings::sample_decal_map(*channels,i.color,bindings::gradients(i.color));
    }
    var diffuse_uv=i.uv;
#ifdef WORLD_WATER
    // The scroll offset is constant across the primitive, so gradients are unchanged.
    if fam==14u { diffuse_uv+=fract(frame_state.clock.x*p.water[1].xy*vec2<f32>(1.0,-1.0)); }
#endif
    let a = bindings::sample_diffuse(*channels, diffuse_uv, g);
    var d = a.rgb*a.rgb;
    if HAS_DECAL && (fam == 3u || fam == 4u) && (flags & 8u) != 0u && (flags & 512u) == 0u {
        d = mix(d,art.rgb*art.rgb,art.a*p.decal.x);
    }
    let lm = bindings::sample_lightmap(*channels, i.uv_b, 0.0).rgb;
    var baked = lm*lm;
    if shadowed { baked=min(baked,vec3<f32>(visibility)+bindings::shadow_floor()); }
    var nm = vec3<f32>(0.5,0.5,1.0);
    var detail = vec2<f32>(0.5);
    if HAS_NORMAL && (flags & 1u) != 0u && (fam <= 6u || fam == 13u) {
        nm = bindings::sample_normal_map(*channels,i.uv,g).rgb;
    }
    if HAS_DETAIL && (flags & 2u) != 0u && fam != 2u {
        detail = bindings::sample_detail_map(
            *channels,
            bindings::scaled_uv(i.uv,p.surface.z),
            bindings::gradients_scaled(g, p.surface.z)
        ).rg;
    }
    if HAS_LOAD_DETAIL && (fam == 5u || fam == 6u || fam == 13u) {
        // The reference folds the reflective material's constant detail texel.
        if (flags & 128u) != 0u { detail = bindings::load_detail(*channels,vec2<i32>(0),0).rg; }
    }
    var raw = vec3<f32>(0.0);
    if HAS_NORMAL && (fam <= 6u || fam == 13u) && (flags & 1u) != 0u {
        var dxy = vec2<f32>(0.5);
        if (flags & 2u) != 0u && fam != 2u { dxy = detail; }
        if (fam == 5u || fam == 6u || fam == 13u) && (flags & 128u) != 0u { dxy = detail; }
        raw = vec3<f32>(nm.xy*2.0+dxy*2.0-2.0,nm.z*2.0-1.0);
    }
    if HAS_MACRO && (flags & 4u) != 0u {
        let overlay_sample = bindings::sample_macro_map(
            *channels,
            bindings::scaled_uv(i.uv,p.surface.x),
            bindings::gradients_scaled(g, p.surface.x)
        ).rgb;
        // Families 9 to 12 take their own branch below, so this is its `fam < 13u`.
        if fam <= 8u && (flags & 256u) == 0u {
            d *= saturate((overlay_sample-0.5)*p.surface.y+0.5);
        }
    }
    var masks = vec3<f32>(0.0);
    if HAS_SPECULAR && (flags & 16u) != 0u {
        masks = bindings::sample_specular_map(*channels,i.uv,g).rgb;
    }
    var wn = n0;
#else
    // Gradients are built here, in uniform control flow, and threaded into the
    // branches below. Every derived UV is an affine transform of one of these,
    // so its gradients are the same transform of theirs.
    let g = bindings::gradients(i.uv);
    let g_decal = bindings::gradients(i.color);
    let g_detail = bindings::gradients_scaled(g, p.surface.z);
    let g_macro = bindings::gradients_scaled(g, p.surface.x);

    var diffuse_uv=i.uv;
#ifdef WORLD_WATER
    // The scroll offset is constant across the primitive, so gradients are unchanged.
    if fam==14u { diffuse_uv+=fract(frame_state.clock.x*p.water[1].xy*vec2<f32>(1.0,-1.0)); }
#endif
    let a = bindings::sample_diffuse(*channels, diffuse_uv, g);
    let lm = bindings::sample_lightmap(*channels, i.uv_b, 0.0).rgb;
    // Sample before alpha rejection: gradients must stay uniform.
    var nm = vec3<f32>(0.5,0.5,1.0);
    var detail = vec2<f32>(0.5);
    var overlay_sample = vec3<f32>(0.5);
    var art = vec4<f32>(0.0);
    var masks = vec3<f32>(0.0);
    if HAS_NORMAL && (flags & 1u) != 0u && (fam <= 6u || fam == 13u) { nm = bindings::sample_normal_map(*channels,i.uv,g).rgb; }
    if HAS_DETAIL && (flags & 2u) != 0u && fam != 2u {
        detail = bindings::sample_detail_map(*channels,bindings::scaled_uv(i.uv,p.surface.z),g_detail).rg;
    }
    if HAS_LOAD_DETAIL && (fam == 5u || fam == 6u || fam == 13u) {
        // The reference folds the reflective material's constant detail texel.
        if (flags & 128u) != 0u { detail = bindings::load_detail(*channels,vec2<i32>(0),0).rg; }
    }
    if HAS_MACRO && (flags & 4u) != 0u { overlay_sample = bindings::sample_macro_map(*channels,bindings::scaled_uv(i.uv,p.surface.x),g_macro).rgb; }
    if HAS_DECAL && (flags & 8u) != 0u && (fam == 3u || fam == 4u) { art = bindings::sample_decal_map(*channels,i.color,g_decal); }
    if HAS_SPECULAR && (flags & 16u) != 0u { masks = bindings::sample_specular_map(*channels,i.uv,g).rgb; }
    var wn = normalize(i.world_normal);
#endif
#ifndef WORLD_SHORT_LIVES
    let dp1 = dpdx(i.world_position);
    let dp2 = dpdy(i.world_position);
    // Exporter flips V and texture rows; recover original UV derivatives.
    let du1 = dpdx(i.uv * vec2<f32>(1.0,-1.0));
    let du2 = dpdy(i.uv * vec2<f32>(1.0,-1.0));
    let dp2p = cross(dp2,wn);
    let dp1p = cross(wn,dp1);
    var kt = dp2p * du1.x + dp1p * du2.x;
    var kb = dp2p * du1.y + dp1p * du2.y;
    kt *= -inverseSqrt(max(dot(kt,kt),1e-12));
    kb *= inverseSqrt(max(dot(kb,kb),1e-12));
    // Prefer the authored frame where the map supplied one. The derivative basis
    // above is only a stand-in, and it disagrees with the authored orientation on
    // surfaces whose UVs were laid out by hand.
    if dot(i.world_tangent.xyz,i.world_tangent.xyz) > 0.01 {
        kt = normalize(i.world_tangent.xyz);
        kb = normalize(cross(wn,kt)) * i.world_tangent.w;
    }
#endif
    let rpos = i.world_position - frame::view.world_position;
    let vd = -normalize(rpos);
    // Authored render-location direction for the tangent-space sign terms.
    // This does not add directional light energy or a shadow source.
    let sun = p.sun_direction.xyz;
#ifndef WORLD_SHORT_LIVES
    var d = a.rgb*a.rgb;
#endif
    var alpha = 1.0;
    var lin = vec3<f32>(0.0);
#ifndef WORLD_SHORT_LIVES
    var baked = lm*lm;
    if shadowed { baked=min(baked,vec3<f32>(visibility)+bindings::shadow_floor()); }
#endif
#ifdef WORLD_WATER
    if fam==14u {
        lin=d*p.water[0].y;
    } else if fam==32u {
        lin=d*p.water[0].x;
        alpha=saturate((i.world_position.y-p.water[0].z)/max(p.water[0].y-p.water[0].z,1e-4));
    } else if fam==31u {
        let raw_uv=vec2<f32>(i.uv.x,1.0-i.uv.y);
        let uv=raw_uv*p.water[1].w;
        let sample_uv=vec2<f32>(uv.x,1.0-uv.y);
        let g_pca=bindings::gradients_scaled(g,p.water[1].w);
        let c0=bindings::sample_normal_map(*channels,sample_uv,g_pca)*2.0-1.0;
        let c1=bindings::sample_detail_map(*channels,sample_uv,g_pca)*2.0-1.0;
        let pca=(vec3<f32>(dot(c0,frame_state.pca[1])+dot(c1,frame_state.pca[2]),
            dot(c0,frame_state.pca[3])+dot(c1,frame_state.pca[4]),
            dot(c0,frame_state.pca[5])+dot(c1,frame_state.pca[6]))+frame_state.pca[0].xyz)*2.0-1.0;
        var overlay=1.0;
        if (flags & 4u)!=0u {
            let uv_overlay=raw_uv*p.surface.x;
#ifdef WORLD_SHORT_LIVES
            let g_macro=bindings::gradients_scaled(g,p.surface.x);
#endif
            overlay=bindings::sample_macro_map(*channels,vec2<f32>(uv_overlay.x,1.0-uv_overlay.y),g_macro).r;
        }
        let tonedown=2.0*p.water[1].z*saturate(dot(c1,frame_state.pca[6])+overlay);
        let nt=normalize(max(abs(normalize(mix(vec3<f32>(0.0,0.0,1.0),pca,tonedown))),vec3<f32>(0.001)));
        let n=nt.xzy;
        let rv=vd-2.0*n*dot(vd,n);
        var cube=vec3<f32>(0.0);
        if (flags & 64u)!=0u { cube=bindings::sample_environment(*channels,vec3<f32>(-rv.x,-rv.y,rv.z),log2(frame::view.viewport.w/640.0)).rgb; }
        let olm=bindings::sample_lightmap(*channels,i.uv_b+0.01*nt.xy*vec2<f32>(1.0,-1.0),0.0).rgb;
        let fres=0.25+0.75*pow(1.0-abs(dot(n,vd)),5.0);
        let u=normalize(vec3<f32>(-n.z,0.0,n.x));
        let v=cross(n,u);
        let ax=p.water[2].z/p.water[2].x;
        let ay=p.water[2].z/p.water[2].y;
        let h=normalize(sun+vd);
        let eu=dot(h,u)/ax;
        let ev=dot(h,v)/ay;
        let den=sqrt(abs(dot(n,sun)*dot(n,vd)))*12.566371*ax*ay;
        let ward=exp(-2.0*(eu*eu+ev*ev)/(1.0+dot(h,n)))/max(den,1e-6);
        lin=(cube*olm*olm*fres+ward*p.water[0].rgb)*p.water[1].y;
        alpha=1.0;
    } else if fam==30u || fam==33u {
        // Native PCA update time: see retail_render::WaterAnimation.
        let t=frame_state.clock.z;
        // Convert to original UVs for scale/scroll, then back to flipped rows.
        let raw_uv=vec2<f32>(i.uv.x,1.0-i.uv.y);
        // water_defaultPS 11..13: c20 * c21.x, with no extra ripple scale.
        let uv_scale=select(1.0,p.water[3].x,fam==33u);
        let uv1=raw_uv*p.water[2].xy*uv_scale+p.water[1].xy*t;
        let uv2=raw_uv*p.water[2].zw*uv_scale+p.water[1].zw*t;
        // The double V flip cancels, so per-axis scaling is all that carries over.
        let s1=p.water[2].xy*uv_scale;
        let s2=p.water[2].zw*uv_scale;
        let g1=bindings::Gradients(g.ddx*s1,g.ddy*s1);
        let g2=bindings::Gradients(g.ddx*s2,g.ddy*s2);
        let n1=bindings::sample_normal_map(*channels,vec2<f32>(uv1.x,1.0-uv1.y),g1);
        let n2=bindings::sample_normal_map(*channels,vec2<f32>(uv2.x,1.0-uv2.y),g2);
        var vn=normalize((2.0*n1.rgb+2.0*n2.rgb-2.0)*p.water[0].xzw);
#ifdef WORLD_SHORT_LIVES
        let tbn=tangent_frame(i,wn,g);
        let kt=tbn.t;
        let kb=tbn.b;
#endif
        var water_n=normalize(vn.x*kt+vn.y*kb+vn.z*wn);
        var sample_uv=i.uv;
        if fam==33u {
            let c1=bindings::sample_detail_map(*channels,vec2<f32>(uv1.x,1.0-uv1.y),g1)*2.0-1.0;
            let c2=bindings::sample_detail_map(*channels,vec2<f32>(uv2.x,1.0-uv2.y),g2)*2.0-1.0;
            let a1=n1*2.0-1.0;
            let a2=n2*2.0-1.0;
            // water_defaultPS instructions 22..54: the native mean is XYZ,
            // weights are R/G/B pairs. FrameState stores the ocean's R/B/G
            // arrangement; recover those original registers here.
            let mean=frame_state.pca[0].xzy;
            let pca1=vec3<f32>(dot(a1,frame_state.pca[1])+dot(c1,frame_state.pca[2])+mean.x,
                dot(a1,frame_state.pca[5])+dot(c1,frame_state.pca[6])+mean.y,
                dot(a1,frame_state.pca[3])+dot(c1,frame_state.pca[4])+mean.z);
            let pca2=vec3<f32>(dot(a2,frame_state.pca[1])+dot(c2,frame_state.pca[2])+mean.x,
                dot(a2,frame_state.pca[5])+dot(c2,frame_state.pca[6])+mean.y,
                dot(a2,frame_state.pca[3])+dot(c2,frame_state.pca[4])+mean.z);
            let first=vec3<f32>((pca2.x*2.0-1.0)*p.water[0].z,
                1.0+2.0*(pca2.y-1.0)*p.water[0].z,(pca2.z*2.0-1.0)*p.water[0].z);
            let second=vec3<f32>((pca1.x*2.0-1.0)*p.water[0].w,
                1.0+2.0*(pca1.y-1.0)*p.water[0].w,(pca1.z*2.0-1.0)*p.water[0].w);
            // Instructions 47..54 and 69..71 normalize separate reflection
            // and refraction vectors. There is no third wave or area blend.
            water_n=first*inverseSqrt(max(dot(first,first),1e-12));
            let refract=second*inverseSqrt(max(dot(second,second),1e-12));
            sample_uv+=0.02*refract.xz*vec2<f32>(1.0,-1.0);
            d=bindings::sample_diffuse(*channels,sample_uv,g).rgb;
            d*=d;
            vn=water_n;
        }
        let water_lm_uv=i.uv_b+0.01*vn.xz*vec2<f32>(1.0,-1.0);
        var wlm=bindings::sample_lightmap(*channels,water_lm_uv,0.0).rgb;
        if fam==33u {
            // Native tf3 fetches at all four half-texel corners, averages,
            // then squares. Squaring each tap would change baked lighting.
            let texel=0.5/vec2<f32>(bindings::lightmap_dimensions(*channels));
            wlm=(bindings::sample_lightmap(*channels,water_lm_uv+texel,0.0).rgb
                +bindings::sample_lightmap(*channels,water_lm_uv-texel,0.0).rgb
                +bindings::sample_lightmap(*channels,water_lm_uv+texel*vec2<f32>(-1.0,1.0),0.0).rgb
                +bindings::sample_lightmap(*channels,water_lm_uv+texel*vec2<f32>(1.0,-1.0),0.0).rgb)*0.25;
        }
        var lml=wlm*wlm;
        if bindings::shadow_enabled() {
            for(var id=0u;id<frame::lights.n_directional_lights;id+=1u) {
                if (frame::lights.directional_lights[id].flags & 5u)==5u {
                    let floor=select(bindings::shadow_floor(),vec3<f32>(0.09,0.13,0.05),fam==33u);
                    var lit=1.0;
                    if bindings::near_casters(frame_state,i.world_position,frame::lights.directional_lights[id].direction_to_light) {
                        let vz=(frame::view.view_from_world*world_position).z;
                        lit=fetch_directional_shadow(id,world_position,wn,vz);
                    }
                    lml=min(lml,vec3<f32>(lit)+floor); break;
                }
            }
        }
        let kd=dot(water_n,vec3<f32>(0.58*sign(sun.x),0.62*sign(sun.y),0.39))*2.39562;
        var wm=vec2<f32>(0.0);
        if (flags & 16u)!=0u { wm=saturate(bindings::sample_specular_map(*channels,sample_uv,g).xz-p.water[3].y); }
        let reflected_light=2.0*water_n*dot(water_n,sun)-sun;
        let ks=pow(max(saturate(dot(vd,reflected_light)),1e-6),p.water[3].z);
        var spec=ks*wm.x*vec3<f32>(2.1,1.8,1.5)*saturate(lml.g-0.1);
        if (flags & 64u)!=0u {
            let rv=vd-2.0*water_n*dot(vd,water_n);
            // Instruction 95 uses the environment fetch without an added
            // mip bias or color multiplier.
            let cube=bindings::sample_environment(*channels,vec3<f32>(-rv.x,-rv.y,rv.z),log2(frame::view.viewport.w/640.0)).rgb;
            let lum=0.3*saturate(4.0*wm.y-2.6);
            spec+=cube*(lml.g+lum*(1.0-lml.g))*wm.y*1.5;
        }
        lin=(lml*kd*d+spec)*p.water[0].y;
        alpha=max(spec.g,p.water[3].w);
    } else
#endif
    if fam == 9u || fam == 10u {
#ifdef WORLD_SHORT_LIVES
        // Never shadowed (the lookup above is for families 0 to 8, 13 and 16), so `baked` is
        // `lm*lm` here.
        lin = d * max(baked,vec3<f32>(p.family.y)) * p.family.x;
#else
        lin = d * max(lm*lm,vec3<f32>(p.family.y)) * p.family.x;
#endif
        if fam == 9u { lin *= p.family.z; }
        alpha = a.a;
    } else if fam == 11u || fam == 12u {
        lin = d;
        if fam == 11u { lin *= p.family.w; }
    } else {
#ifndef WORLD_SHORT_LIVES
        if HAS_DECAL && (fam == 3u || fam == 4u) && (flags & 8u) != 0u && (flags & 512u) == 0u { d = mix(d,art.rgb*art.rgb,art.a*p.decal.x); }
        if HAS_MACRO && (flags & 4u) != 0u && fam < 13u && (flags & 256u) == 0u { d *= saturate((overlay_sample-0.5)*p.surface.y+0.5); }
#endif
        var kd = 0.93429;
        if HAS_NORMAL && (fam <= 6u || fam == 13u) && (flags & 1u) != 0u {
#ifdef WORLD_SHORT_LIVES
            let tbn = tangent_frame(i, wn, g);
            let kt = tbn.t;
            let kb = tbn.b;
#else
            var dxy = vec2<f32>(0.5);
            if (flags & 2u) != 0u && fam != 2u { dxy = detail; }
            if (fam == 5u || fam == 6u || fam == 13u) && (flags & 128u) != 0u { dxy = detail; }
            let raw = vec3<f32>(nm.xy*2.0+dxy*2.0-2.0,nm.z*2.0-1.0);
#endif
            var vnd = raw;
            if fam != 2u && fam != 6u { vnd = raw * inverseSqrt(max(dot(raw,raw),1e-12)); }
            var tt = kt;
            var bb = kb;
            if fam == 5u || fam == 6u || fam == 13u {
                let up = vec3<f32>(0.0,1.0,0.0)-wn*wn.y;
                if length(up) > 0.05 {
                    let b2 = normalize(up);
                    let t2 = cross(b2,wn);
                    tt = t2*select(-1.0,1.0,dot(t2,kt)>=0.0);
                    bb = b2*select(-1.0,1.0,dot(b2,kb)>=0.0);
                }
            }
            wn = normalize(raw.x*tt+raw.y*bb+wn*max(raw.z,0.05));
            kd = (vnd.x*0.58*sign(dot(kt,sun))+vnd.y*0.62*sign(dot(kb,sun))+vnd.z*0.39)*2.39562;
        }
        var lml = baked;
        lin = lml*kd*d;
        // transparentenvironment_defaultPS 39, 43: diffuse^2 * lightmap * alpha, no kd.
        if fam == 13u || fam == 16u { lin = lml*d*a.a; }
        if HAS_SPECULAR && (flags & 16u) != 0u {
            let lit = vec3<f32>(-0.14,0.5,0.9);
            let reflected = lit-2.0*wn*dot(wn,lit);
            let bp = saturate(dot(vd,-reflected));
            let ks = pow(max(bp,1e-6),10.0+290.0*masks.y);
            lin += ks*vec3<f32>(2.1,1.8,1.5)*lml.g*masks.x;
        }
        if HAS_ENVIRONMENT && (fam == 5u || fam == 6u || fam == 13u) && (flags & 64u) != 0u {
            let rv = vd-2.0*wn*dot(vd,wn);
            let dir = vec3<f32>(-rv.x,-rv.y,rv.z);
            let cube = bindings::sample_environment(*channels,dir,log2(frame::view.viewport.w/640.0)).rgb;
            let rl = 0.3*saturate(4.0*masks.z-2.6);
            let lum = lml.g+rl*(1.0-lml.g);
            lin += cube*lum*masks.z*1.5;
        }
        if fam >= 7u { alpha = a.a; }
        // transparentenvironment_defaultPS 53: oC0.w = alpha * alpha.
        if fam == 13u || fam == 16u { alpha *= alpha; }
    }
    var f = saturate(length(rpos)*p.fog_ramp.x+p.fog_ramp.y);
    if p.fog_ramp.z != 1.0 { f = pow(max(f,1e-6),p.fog_ramp.z); }
    var fog_a = 1.0+p.fog_color.a*f;
    if fam <= 8u || fam == 13u || fam == 16u { fog_a *= p.surface.w; }
    var xe = max((lin*fog_a+p.fog_color.rgb*f)*p.mode.w,vec3<f32>(0.0));
    // Reduced curve is the full curve with the linear input capped at one.
    if fam == 8u { xe = min(xe,vec3<f32>(1.0)); }
#ifndef WORLD_LEAN
    // Always false: nothing has set `foliage_debug` since its diagnostic went (retail_render.rs
    // `WorldParams`). `WORLD_LEAN` leaves out the test and its load.
    if p.foliage_debug.w != 0.0 { return vec4<f32>(p.foliage_debug.rgb, 1.0); }
#endif
    // Compiled only for the classes whose materials carry a cutoff, and left out
    // of the cutout classes again under a depth prepass, which has already run
    // this test (`WorldMaterial::specialize`). The opaque classes leave it out so
    // the hardware can write depth before shading and reject what is hidden;
    // see RenderClass.
#ifdef WORLD_ALPHA_CUTOFF
    if a.a < p.mode.z { discard; }
#endif
    return vec4<f32>(xe,alpha);
}
#endif
#endif

// `SKATE_WORLD_GPR48` names the same fragment `fragment_gpr48`, which NVK's
// compiler takes as a cap of 48 registers for it (bevy-horizon
// `patches/mesa-switch/0014`): register allocation spills what does not fit, so
// that more warps fit on the GPU at once. The same pixels.
@fragment
#ifdef WORLD_FETCH
#ifdef WORLD_GPR48
fn fragment_gpr48(interpolated: FragmentInput) -> @location(0) vec4<f32> {
#else
fn fragment(interpolated: FragmentInput) -> @location(0) vec4<f32> {
#endif
#else
#ifdef WORLD_GPR48
fn fragment_gpr48(interpolated: VertexOutput) -> @location(0) vec4<f32> {
#else
fn fragment(interpolated: VertexOutput) -> @location(0) vec4<f32> {
#endif
#endif
#ifdef WORLD_PROBE_GEOMETRY
#ifdef WORLD_FETCH
    // The triangle's number is all that is read: what the fetch variant's
    // geometry costs before anything is fetched.
    let triangle = interpolated.first_triangle + interpolated.primitive_index;
    return vec4<f32>(vec3<f32>(select(0.5, 0.4, triangle == 0xffffffffu)), 1.0);
#else
    return vec4<f32>(0.5, 0.5, 0.5, 1.0);
#endif
#else
#ifdef WORLD_FETCH
    let fetched = fetch_triangle(interpolated);
    let i = fetched.varyings;
#else
    let i = unpack(interpolated);
#endif
#ifdef WORLD_PROBE_VARYINGS
#ifdef WORLD_FETCH
    // Magenta where the triangle fetched does not cover the pixel, so is not the
    // one being drawn: `primitive_index` is not counting as `MeshTag` assumes.
    // Covered pixels' weights are never below zero by more than rounding, and the
    // helper lanes outside the triangle are never seen.
    if any(fetched.weights < vec3<f32>(-0.1)) {
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }
#endif
    return probe_varyings(i);
#else
#ifdef WORLD_SLOT_WATERFALL
    // `SKATE_WORLD_SLOT_WATERFALL`: each pass of the loop shades the fragments of
    // one material, the first one the subgroup (a warp of 32 on NVIDIA) still
    // holds, with a `slot` the compiler knows is the same across the subgroup. Its
    // parameters are read once for the warp, and its page lookups compile without
    // `WORLD_PAGE_UNIFORM`'s test or the loop NVK otherwise builds around a page
    // index that may differ (retail_material_bindings.wgsl `sample_page`). A warp
    // holding k materials shades k times, each fragment once. A 2x2 quad belongs
    // to one triangle, so to one material, and its four fragments shade in the
    // same pass, so their derivatives hold (findings 31).
#ifdef WORLD_PROBE_ONE_PASS
    // `SKATE_WORLD_PROBE=6`, for measuring: one pass shades every fragment with
    // the first material the subgroup holds, so a warp holding k materials shades
    // once, not k times. Against the waterfall it gives what the repeats cost; a
    // fragment whose own material differs is drawn wrong.
    return shade(i, subgroupBroadcastFirst(i.material_index));
#else
    var shaded = vec4<f32>(0.0);
    loop {
        let first = subgroupBroadcastFirst(i.material_index);
        if i.material_index == first {
            shaded = shade(i, first);
            break;
        }
    }
    return shaded;
#endif
#else
    return shade(i, i.material_index);
#endif
#endif
#endif
}
