// Shadow-map (prepass) stages for merged world geometry.
//
// This pass cannot use Bevy's default prepass shaders. Those declare Bevy's own
// prepass attribute locations — where `COLOR` lands on location 7 rather than
// the main pass's 5 — while `WorldMaterial::specialize` installs one layout for
// every pipeline it is asked to build, because `material_index` is a vertex
// attribute the default shaders know nothing about. Supplying both stages here
// keeps the shader and that layout describing the same vertex.
//
// `WORLD_DEPTH_TRIM` (`SKATE_WORLD_TRIM`, on unless set to 0) leaves out the
// attributes no depth stage reads, and `WORLD_DEPTH_POSITION_ONLY` the UV and
// material index too, for the pipelines with no alpha test. NVK fetches every
// attribute a pipeline declares, and compiles each stage on its own, so a
// vertex stage writes all its outputs even where no fragment stage follows;
// the opaque classes' depth pipelines have none.
//
// `WORLD_CUTOUT_FLAT` (`SKATE_CUTOUT_FLAT`) moves the alpha test's two reads
// of the material, its diffuse slot and its cutoff, from the fragment stage to
// the vertex stage, which hands them on flat. Flat values come from the
// triangle's provoking vertex, as the material index does, so each fragment
// tests the same texel against the same cutoff: the same pixels.
//
// `WORLD_MARK` (`SKATE_VIS_CULL`) has the view's depth prepass mark each
// triangle that gets a fragment into the depth test, in a word per triangle
// that bevy_pbr's triangle cull reads before the main pass: it keeps the
// marked triangles and clears their words (vendor/README.md). A leaf's words
// start at its tag's low 29 bits (skate_world.rs), and a triangle's is that
// plus its `primitive_index`, its place in the leaf's draw. The opaque classes
// mark under `WORLD_MARK_EARLY`, which tests and writes depth before the
// fragment stage runs, so only their fragments that pass reach the mark. A
// stage that writes memory otherwise runs before the test, as the cutout
// classes' does: it marks before its alpha test, so they keep every triangle
// the prepass rasterises. Under `WORLD_MARK_AFTER_TEST`
// (`SKATE_VIS_CULL_AFTER_TEST`) it marks after it, so only the fragments the
// test keeps store, and a triangle it rejects everywhere isn't kept.
#import bevy_pbr::view_transformations::position_world_to_clip
#import skate_retail::material_bindings as bindings
#ifdef WORLD_MARK
#import bevy_pbr::mesh_bindings::mesh

@group(#{MATERIAL_BIND_GROUP}) @binding(15) var<storage, read_write> marks: array<u32>;
#endif

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifndef WORLD_DEPTH_TRIM
    @location(1) normal: vec3<f32>,
#endif
#ifndef WORLD_DEPTH_POSITION_ONLY
    @location(2) uv: vec2<f32>,
#endif
#ifndef WORLD_DEPTH_TRIM
    @location(3) uv_b: vec2<f32>,
    @location(4) color: vec2<f32>,
#endif
#ifndef WORLD_DEPTH_POSITION_ONLY
    @location(5) material_index: u32,
#endif
#ifndef WORLD_DEPTH_TRIM
    @location(6) tangent: vec4<f32>,
#endif
}

struct VertexOutput {
    // `@invariant` like the main pass's (retail_world.wgsl), which a depth
    // prepass needs to match bit for bit.
    @builtin(position) @invariant clip_position: vec4<f32>,
#ifndef WORLD_DEPTH_POSITION_ONLY
    @location(0) uv: vec2<f32>,
#ifdef WORLD_CUTOUT_FLAT
    @location(1) @interpolate(flat) diffuse: u32,
    @location(3) @interpolate(flat) cutoff: f32,
#else
    @location(1) @interpolate(flat) material_index: u32,
#endif
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    @location(2) unclipped_depth: f32,
#endif
#ifdef WORLD_MARK
    @location(4) @interpolate(flat) mark_base: u32,
#endif
}

// World leaves sit at the identity transform; see the vertex stage in
// `retail_world.wgsl`. The clip position is the same expression as there, so a
// depth prepass and the main pass compute the same depth.
@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = position_world_to_clip(v.position);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    // Casters behind the cascade's near plane still occlude, so carry the true
    // depth and clamp the clipped one, as the reference prepass does.
    out.unclipped_depth = out.clip_position.z;
    out.clip_position.z = min(out.clip_position.z, 1.0);
#endif
#ifndef WORLD_DEPTH_POSITION_ONLY
    out.uv = v.uv;
#ifdef WORLD_CUTOUT_FLAT
    out.diffuse = bindings::slots[v.material_index].diffuse;
    out.cutoff = bindings::params[v.material_index].mode.z;
#else
    out.material_index = v.material_index;
#endif
#endif
#ifdef WORLD_MARK
    out.mark_base = mesh[v.instance_index].tag & 0x1fffffffu;
#endif
    return out;
}

#ifdef WORLD_MARK_EARLY
@early_depth_test(force)
#endif
@fragment
fn fragment(
    i: VertexOutput,
#ifdef WORLD_MARK
    @builtin(primitive_index) primitive_index: u32,
#endif
)
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    -> @builtin(frag_depth) f32
#endif
{
#ifdef WORLD_MARK
#ifndef WORLD_MARK_AFTER_TEST
    marks[i.mark_base + primitive_index] = 1u;
#endif
#endif
#ifdef WORLD_ALPHA_CUTOFF
#ifdef WORLD_PROBE_KILL_ONLY
    // `SKATE_PREPASS_PROBE=6`, for measuring only: a test that can discard but
    // never does, and reads nothing but the UV. The cards draw solid.
    if i.uv.x < -3.0e38 { discard; }
#else
    // Only the classes that can reject a texel pay for the fetch, so fences and
    // foliage cast their silhouette instead of their quad.
#ifdef WORLD_CUTOUT_FLAT
    let alpha = bindings::sample_page(i.diffuse, i.uv, bindings::gradients(i.uv)).a;
    if alpha < i.cutoff { discard; }
#else
    let slot = i.material_index;
    let alpha = bindings::sample_diffuse(bindings::slots[slot], i.uv, bindings::gradients(i.uv)).a;
    if alpha < bindings::params[slot].mode.z { discard; }
#endif
#endif
#endif
#ifdef WORLD_MARK
#ifdef WORLD_MARK_AFTER_TEST
    // Only the fragments the alpha test kept mark: `discard` makes the others
    // helper invocations, whose writes to storage have no effect.
    marks[i.mark_base + primitive_index] = 1u;
#endif
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    return i.unclipped_depth;
#endif
}
