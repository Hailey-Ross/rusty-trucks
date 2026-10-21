// skate: per-triangle culling for the main opaque passes (vendor/README.md, triangle_cull.rs).
//
// One workgroup per indexed draw of the phase, and one dispatch per pair of vertex and index
// slabs that holds tagged meshes. A draw whose mesh carries the leaf tag (bit 31 of `MeshTag`),
// comes from this dispatch's slabs and is drawn once has each of its triangles tested; the ones
// that can't touch a pixel are left out of a copy of its index range in its index slab's mirror,
// in their original order, and the draw's `index_count` becomes the survivors'. `DrawMesh` binds
// the mirror for these draws. Every test is conservative, so the image doesn't change: a triangle
// is dropped only when it lies wholly outside one clip plane, faces away (one-sided materials),
// covers no pixel centre, or lies behind the depth pyramid built from this frame's prepass.

#import bevy_pbr::mesh_preprocess_types::{
    IndirectParametersGpuMetadata,
    IndirectParametersIndexed,
    MeshInput
}
#import bevy_pbr::occlusion_culling
#import bevy_render::maths
#import bevy_render::view::View

struct PushConstants {
    // The draws in this phase's indirect parameters buffer.
    slot_count: u32,
    // Workgroups per row of the dispatch (the x dimension is limited to 65535).
    slots_per_row: u32,
    // The vertex stride, and the position's offset within a vertex, in 4-byte words.
    stride: u32,
    position: u32,
    // The pyramid test's span (`occluded_finely`); 0 skips that test.
    span: u32,
    flags: u32,
    // The ids of the vertex and index slabs bound here, as `MeshInput::pad` carries a mesh's
    // (`triangle_cull::slab_word`); draws from other slabs are left alone.
    slabs: u32,
}

// The view draws one sample a pixel, at its centre.
const FLAG_PIXEL_CENTRES: u32 = 1u;
// Count the triangles by outcome into `stats`, for a readback.
const FLAG_STATS: u32 = 2u;
// Debugging (`TRIANGLE_CULL_DEBUG_EMPTY`): return at once, leaving every draw as it was.
const FLAG_EMPTY: u32 = 4u;

const TAG_LEAF: u32 = 0x80000000u;
const TAG_TWO_SIDED: u32 = 0x40000000u;

const MODE_SKIP: u32 = 0u;
const MODE_COPY: u32 = 1u;
const MODE_CULL: u32 = 2u;
const MODE_CULL_TWO_SIDED: u32 = 3u;
const MODE_INVALID: u32 = 4u;

// A triangle's outcome, and its counter in `outcomes` and `stats`.
const KEEP: u32 = 0u;
const OUTSIDE_THE_VIEW: u32 = 1u;
const FACING_AWAY: u32 = 2u;
const NO_PIXEL_CENTRE: u32 = 3u;
const BEHIND_THE_PYRAMID: u32 = 4u;
// Then the draws: tested, copied whole (several instances), and out of range.
const STAT_DRAWS_TESTED: u32 = 5u;
const STAT_DRAWS_COPIED: u32 = 6u;
const STAT_DRAWS_INVALID: u32 = 7u;

const WORKGROUP_SIZE: u32 = 256u;
const MASK_WORDS: u32 = 8u;

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage> current_input: array<MeshInput>;
@group(0) @binding(2) var<storage> indirect_parameters_gpu_metadata:
    array<IndirectParametersGpuMetadata>;
@group(0) @binding(3) var<storage, read_write> indirect_parameters:
    array<IndirectParametersIndexed>;
@group(0) @binding(4) var<storage> vertices: array<f32>;
@group(0) @binding(5) var<storage> indices: array<u32>;
@group(0) @binding(6) var<storage, read_write> mirror: array<u32>;
@group(0) @binding(7) var depth_pyramid: texture_2d<f32>;
@group(0) @binding(8) var<storage, read_write> stats: array<atomic<u32>, 8>;

var<push_constant> push_constants: PushConstants;

struct Control {
    mode: u32,
    first_index: u32,
    triangle_count: u32,
    base_vertex: u32,
    mesh_index: u32,
}

var<workgroup> control: Control;
// Which of the chunk's triangles survive, one bit each, in three rotating sets so that one
// barrier a chunk is enough: the set a chunk fills was cleared two chunks earlier.
var<workgroup> masks: array<atomic<u32>, 24>;
var<workgroup> outcomes: array<atomic<u32>, 5>;

fn position(vertex: u32) -> vec3<f32> {
    let at = vertex * push_constants.stride + push_constants.position;
    return vec3(vertices[at], vertices[at + 1u], vertices[at + 2u]);
}

// `KEEP` if the rasteriser could give the triangle a fragment in the main pass that passes the
// depth test, else the first test that rules that out.
fn test_triangle(
    triangle: vec3<u32>,
    base_vertex: u32,
    two_sided: bool,
    clip_from_local: mat4x4<f32>,
    half_size: vec2<f32>,
) -> u32 {
    let c0 = clip_from_local * vec4(position(base_vertex + triangle.x), 1.0);
    let c1 = clip_from_local * vec4(position(base_vertex + triangle.y), 1.0);
    let c2 = clip_from_local * vec4(position(base_vertex + triangle.z), 1.0);
    let x = vec3(c0.x, c1.x, c2.x);
    let y = vec3(c0.y, c1.y, c2.y);
    let z = vec3(c0.z, c1.z, c2.z);
    let w = vec3(c0.w, c1.w, c2.w);

    // Wholly outside one clip plane. In clip space, so it holds whatever the signs of w. Reverse
    // Z: the near plane is z = w, the far plane (if any) z = 0.
    if (all(x > w) || all(x < -w) || all(y > w) || all(y < -w) || all(z > w) ||
            all(z < vec3(0.0))) {
        return OUTSIDE_THE_VIEW;
    }
    // The clipper cuts a triangle that crosses the near plane; the tests below need a projection.
    if (any(z > w) || any(w <= vec3(0.0))) {
        return KEEP;
    }

    let n0 = c0.xyz / c0.w;
    let n1 = c1.xyz / c1.w;
    let n2 = c2.xyz / c2.w;
    let ndc_min = min(n0.xy, min(n1.xy, n2.xy));
    let ndc_max = max(n0.xy, max(n1.xy, n2.xy));
    // Pixels from the viewport's centre, y up.
    let s0 = n0.xy * half_size;
    let s1 = n1.xy * half_size;
    let s2 = n2.xy * half_size;
    let s_min = ndc_min * half_size;
    let s_max = ndc_max * half_size;

    // Facing away: counter-clockwise is the front, in wgpu's y-up NDC. The rasteriser decides from
    // positions snapped to 1/256 pixel; moving each vertex by e changes this doubled area by at
    // most 4e(width + height), so a margin of (width + height) / 32 covers e up to 1/128 pixel.
    if (!two_sided) {
        let area = (s1.x - s0.x) * (s2.y - s0.y) - (s2.x - s0.x) * (s1.y - s0.y);
        if (area < -(s_max.x - s_min.x + s_max.y - s_min.y) * (1.0 / 32.0)) {
            return FACING_AWAY;
        }
    }

    // No pixel centre (k + 0.5 from the viewport's top-left corner) inside the bounding box,
    // widened by 1/64 pixel for the snapping and the fill rule's ties.
    if ((push_constants.flags & FLAG_PIXEL_CENTRES) != 0u) {
        let margin = 1.0 / 64.0;
        let low = vec2(s_min.x + half_size.x, half_size.y - s_max.y) - margin - 0.5;
        let high = vec2(s_max.x + half_size.x, half_size.y - s_min.y) + margin - 0.5;
        if (any(floor(high) < ceil(low))) {
            return NO_PIXEL_CENTRE;
        }
    }

    // Behind every texel of the depth pyramid that its rectangle touches. The pyramid holds this
    // frame's prepass depth, which the main pass tests against, so a triangle behind it would
    // fail that test everywhere. Its nearest point (reverse Z: the largest depth), nudged nearer
    // to absorb the rounding between this transform and the vertex shader's.
    if (push_constants.span != 0u) {
        let aabb = vec4(
            ndc_min.x * 0.5 + 0.5,
            0.5 - ndc_max.y * 0.5,
            ndc_max.x * 0.5 + 0.5,
            0.5 - ndc_min.y * 0.5
        );
        let pixels = (aabb.zw - aabb.xy) * vec2<f32>(textureDimensions(depth_pyramid, 0));
        let nearest = max(n0.z, max(n1.z, n2.z)) * 1.0001 + 1e-7;
        if (occlusion_culling::occluded_finely(
                aabb, pixels, nearest, f32(push_constants.span), depth_pyramid)) {
            return BEHIND_THE_PYRAMID;
        }
    }
    return KEEP;
}

@compute
@workgroup_size(256)
fn main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    if ((push_constants.flags & FLAG_EMPTY) != 0u) {
        return;
    }
    let slot = workgroup_id.x + workgroup_id.y * push_constants.slots_per_row;

    if (local_index < 24u) {
        atomicStore(&masks[local_index], 0u);
    }
    if (local_index < 5u) {
        atomicStore(&outcomes[local_index], 0u);
    }
    if (local_index == 0u) {
        var c = Control(MODE_SKIP, 0u, 0u, 0u, 0u);
        if (slot < push_constants.slot_count) {
            let parameters = indirect_parameters[slot];
            if (parameters.instance_count != 0u) {
                let mesh_index = indirect_parameters_gpu_metadata[slot].mesh_index;
                if (mesh_index < arrayLength(&current_input) &&
                        (current_input[mesh_index].tag & TAG_LEAF) != 0u &&
                        current_input[mesh_index].pad == push_constants.slabs) {
                    let end = parameters.first_index + parameters.index_count;
                    if (end < parameters.first_index || end > arrayLength(&indices) ||
                            end > arrayLength(&mirror)) {
                        c.mode = MODE_INVALID;
                    } else if (parameters.instance_count != 1u) {
                        // Several transforms; the copy keeps them all.
                        c.mode = MODE_COPY;
                    } else if ((current_input[mesh_index].tag & TAG_TWO_SIDED) != 0u) {
                        c.mode = MODE_CULL_TWO_SIDED;
                    } else {
                        c.mode = MODE_CULL;
                    }
                    c.first_index = parameters.first_index;
                    c.triangle_count = parameters.index_count / 3u;
                    c.base_vertex = parameters.base_vertex;
                    c.mesh_index = mesh_index;
                }
            }
        }
        control = c;
    }
    let c = workgroupUniformLoad(&control);
    if (c.mode == MODE_SKIP) {
        return;
    }
    let counting = (push_constants.flags & FLAG_STATS) != 0u;
    if (c.mode == MODE_INVALID) {
        // Its range isn't in the mirror; drawing nothing beats drawing stale indices.
        if (local_index == 0u) {
            indirect_parameters[slot].index_count = 0u;
            if (counting) {
                atomicAdd(&stats[STAT_DRAWS_INVALID], 1u);
            }
        }
        return;
    }

    let clip_from_local =
        view.clip_from_world * maths::affine3_to_square(current_input[c.mesh_index].world_from_local);
    let half_size = view.viewport.zw * 0.5;
    let two_sided = c.mode == MODE_CULL_TWO_SIDED;
    let word = local_index >> 5u;
    let bit = 1u << (local_index & 31u);

    var kept = 0u;
    var rotation = 0u;
    for (var first = 0u; first < c.triangle_count; first += WORKGROUP_SIZE) {
        let triangle_index = first + local_index;
        var keep = false;
        var triangle = vec3(0u);
        if (triangle_index < c.triangle_count) {
            let at = c.first_index + 3u * triangle_index;
            triangle = vec3(indices[at], indices[at + 1u], indices[at + 2u]);
            var outcome = KEEP;
            if (c.mode != MODE_COPY) {
                outcome =
                    test_triangle(triangle, c.base_vertex, two_sided, clip_from_local, half_size);
                if (counting) {
                    atomicAdd(&outcomes[outcome], 1u);
                }
            }
            keep = outcome == KEEP;
            if (keep) {
                atomicOr(&masks[rotation * MASK_WORDS + word], bit);
            }
        }
        workgroupBarrier();

        // Survivors before this one, for an output in the original order: coplanar triangles
        // drawn in a different order would fight.
        var before = 0u;
        var total = 0u;
        for (var w = 0u; w < MASK_WORDS; w += 1u) {
            let bits = atomicLoad(&masks[rotation * MASK_WORDS + w]);
            total += countOneBits(bits);
            if (w < word) {
                before += countOneBits(bits);
            } else if (w == word) {
                before += countOneBits(bits & (bit - 1u));
            }
        }
        if (keep) {
            let out = c.first_index + 3u * (kept + before);
            mirror[out] = triangle.x;
            mirror[out + 1u] = triangle.y;
            mirror[out + 2u] = triangle.z;
        }
        // Clears the previous chunk's set: everyone read it before this chunk's barrier, and the
        // chunk after next, the first to fill it again, does so only after the next chunk's.
        if (local_index < MASK_WORDS) {
            atomicStore(&masks[((rotation + 2u) % 3u) * MASK_WORDS + local_index], 0u);
        }
        kept += total;
        rotation = (rotation + 1u) % 3u;
    }

    if (local_index == 0u) {
        indirect_parameters[slot].index_count = 3u * kept;
        if (counting) {
            // The last chunk's barrier ordered every count before this.
            if (c.mode == MODE_COPY) {
                atomicAdd(&stats[STAT_DRAWS_COPIED], 1u);
            } else {
                atomicAdd(&stats[STAT_DRAWS_TESTED], 1u);
                for (var i = 0u; i < 5u; i += 1u) {
                    atomicAdd(&stats[i], atomicLoad(&outcomes[i]));
                }
            }
        }
    }
}
