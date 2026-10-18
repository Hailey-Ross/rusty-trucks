// skate: per-triangle culling, second version (vendor/README.md, triangle_cull.rs). The tests
// and their outcomes are `triangle_cull.wgsl`'s; what changes is how the work is spread.
//
// The first version starts a 256-thread workgroup for every draw slot, once per pair of slabs,
// and one thread finds out alone whether the slot draws a leaf of that pair while the rest
// wait; most of them don't. Here `list` runs one thread per slot instead, and appends each slot
// that draws a leaf of this dispatch's slabs to `items_out`, with its clip-from-local transform
// worked out once, counting the items in this dispatch's word of `counts`. That count is the
// width of the indirect dispatch of `cull` that follows: one 64-thread workgroup per item (and
// one for an empty item first, so that the dispatch is never empty), which
// tests its leaf's triangles 256 at a time, four a thread (triangles t, t + 64, t + 128 and
// t + 192 of the round, so the reads of neighbouring threads are neighbours), with one barrier a
// round, and writes the survivors in their original order, as the first version does.
//
// Under `FLAG_MARKS` (`TRIANGLE_CULL_MARKS`) no test runs: `keep_marked`, at the end, takes
// `cull`'s place (or `cull` itself, under `TRIANGLE_CULL_MARKS_IN_CULL`), and keeps each
// triangle whose word in `marks` the view's depth prepass set, and sets the word back to 0 for
// the next frame.
// A triangle's word is its leaf's `TAG_MARK_BASE` bits plus its index in the leaf's draw, which
// is what the prepass's `primitive_index` counts, as the prepass draws the leaf's whole range.
// For debugging, `FLAG_MARKS_INVERT` keeps the unmarked triangles instead.
//
// Under `FLAG_CLUSTERS` (`TRIANGLE_CULL_CLUSTERS`) the prepass drew each leaf from its own mirror
// of the slab, which `triangle_cull_clusters.wgsl` filled, and which this dispatch's `indices`
// is: for a leaf with boxes, the clusters that might be seen, in their order, which the prepass
// marked as it drew them. `list` reads how many triangles that holds from the leaf's count word,
// and leaves out a leaf whose word that frame's cluster pass didn't write. With `FLAG_DIRECT`
// too, the main pass draws those triangles from the prepass's mirror as they are, so `list` only
// sets the draw's count and lists no item.

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
    // The index slab's size in indices (its mirror's is the same): a draw that would end past it
    // is out of range.
    index_words: u32,
    // The vertex stride, and the position's offset within a vertex, in 4-byte words.
    stride: u32,
    position: u32,
    // The pyramid test's span (`occluded_finely`); 0 skips that test.
    span: u32,
    flags: u32,
    // The ids of the vertex and index slabs bound here, as `MeshInput::pad` carries a mesh's
    // (`triangle_cull::slab_word`); draws from other slabs are left alone.
    slabs: u32,
    // This dispatch's first item in the items buffer, and its word in `counts`: the x of its
    // indirect dispatch arguments, which the CPU sets to 1 (and y and z to 1) every frame. Item
    // 0 is empty, and those that `list` appends start at 1, so `cull` never runs as an empty
    // dispatch, whose handling NVK leaves to the GPU.
    item_base: u32,
    count_word: u32,
}

const FLAG_PIXEL_CENTRES: u32 = 1u;
const FLAG_STATS: u32 = 2u;
// Debugging (`TRIANGLE_CULL_DEBUG_EMPTY`): list nothing, so no workgroup of `cull` starts and
// every draw keeps its count.
const FLAG_EMPTY: u32 = 4u;
// Keep the marked triangles instead of testing them (`TRIANGLE_CULL_MARKS`).
const FLAG_MARKS: u32 = 8u;
// Debugging, with `FLAG_MARKS` (`TRIANGLE_CULL_MARKS_INVERT`): keep the unmarked triangles
// instead, and leave out the marked ones.
const FLAG_MARKS_INVERT: u32 = 16u;
// With `FLAG_MARKS`: the prepass drew the leaves from their clusters (`TRIANGLE_CULL_CLUSTERS`).
// The frame's stamp is in the flags' top 8 bits.
const FLAG_CLUSTERS: u32 = 32u;
// With `FLAG_CLUSTERS`: the draws read the prepass's mirror as it is
// (`TRIANGLE_CULL_CLUSTERS_CUTOUTS`), so `list` sets each one's count from its leaf's count word
// and lists no item.
const FLAG_DIRECT: u32 = 64u;
const FLAG_STAMP_SHIFT: u32 = 24u;

const TAG_LEAF: u32 = 0x80000000u;
const TAG_TWO_SIDED: u32 = 0x40000000u;
const TAG_MARK_BASE: u32 = 0x1fffffffu;

// A dispatch's empty first item.
const MODE_NONE: u32 = 0u;
const MODE_COPY: u32 = 1u;
const MODE_CULL: u32 = 2u;
const MODE_CULL_TWO_SIDED: u32 = 3u;
const MODE_MARKS: u32 = 4u;

const KEEP: u32 = 0u;
const OUTSIDE_THE_VIEW: u32 = 1u;
// Under `FLAG_MARKS`, the one outcome besides `KEEP`, counted in the first test's place: left
// out as unmarked, or under `FLAG_MARKS_INVERT` as marked.
const UNMARKED: u32 = 1u;
const FACING_AWAY: u32 = 2u;
const NO_PIXEL_CENTRE: u32 = 3u;
const BEHIND_THE_PYRAMID: u32 = 4u;
const STAT_DRAWS_TESTED: u32 = 5u;
const STAT_DRAWS_COPIED: u32 = 6u;
const STAT_DRAWS_INVALID: u32 = 7u;

// A leaf's count word (`triangle_cull_clusters.wgsl`): the triangles its prepass draw held, and
// the stamp of the frame that wrote it. A leaf has boxes, and so a count word, if its mark base
// is a multiple of `CLUSTER_TRIANGLES` and its clusters fit.
const CLUSTER_TRIANGLES: u32 = 32u;
const CLUSTER_COUNT_MASK: u32 = 0x7fffffu;
const CLUSTER_STAMP_SHIFT: u32 = 24u;

const LIST_WORKGROUP_SIZE: u32 = 64u;
const CULL_WORKGROUP_SIZE: u32 = 64u;
const PER_THREAD: u32 = 4u;
// Triangles a round, and the words of their bit set.
const ROUND: u32 = 256u;
const ROUND_WORDS: u32 = 8u;

// A leaf draw for `cull`.
struct Item {
    clip_from_local: mat4x4<f32>,
    slot: u32,
    mode: u32,
    first_index: u32,
    triangle_count: u32,
    base_vertex: u32,
    // Under `MODE_MARKS`, the leaf's first word in `marks`.
    mark_base: u32,
}

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
// The same buffer: written by `list`, read by `cull`.
@group(0) @binding(9) var<storage, read_write> items_out: array<Item>;
@group(0) @binding(10) var<storage> items_in: array<Item>;
@group(0) @binding(11) var<storage, read_write> counts: array<atomic<u32>>;
// One word for each tagged triangle, set by the prepass (`TriangleCullMarks`); a placeholder
// unless `FLAG_MARKS` is set.
@group(0) @binding(12) var<storage, read_write> marks: array<u32>;
// `list`'s, under `FLAG_CLUSTERS`: one word for each cluster, then each leaf's count word at its
// first cluster's place in the second half. A placeholder otherwise.
@group(0) @binding(13) var<storage> cluster_words: array<u32>;

var<push_constant> push_constants: PushConstants;

// `keep_marked_coalesced`'s (below). Both hold the round's triangle i in component i % 4 of
// element i / 4, so that a thread reads or writes its four neighbouring triangles' with one
// access. They come first because NAK lays workgroup variables out in order, 4-byte aligned:
// after `outcomes` they'd start at byte 116, and it splits each access in three.
var<workgroup> round_marks: array<vec4<u32>, 64>;
var<workgroup> round_ranks: array<vec4<u32>, 64>;
// Which of a round's triangles survive, one bit each (triangle t + 64k of the round is bit t % 32
// of word 2k + t / 32), in three rotating sets so that one barrier a round is enough: the set a
// round fills was cleared two rounds earlier.
var<workgroup> masks: array<atomic<u32>, 24>;
var<workgroup> outcomes: array<atomic<u32>, 5>;
// The item's clip-from-local transform, read from here as each triangle needs it rather than
// held in registers through the loop: NAK gives `cull` 56 registers that way, against 64, so 36
// warps fit on an SM instead of 32.
var<workgroup> transform: mat4x4<f32>;

@compute
@workgroup_size(64)
fn list(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let slot = global_id.x;
    if (slot == 0u) {
        // Rewritten every frame, as the CPU may have moved the dispatches' shares.
        items_out[push_constants.item_base] = Item();
    }
    if ((push_constants.flags & FLAG_EMPTY) != 0u || slot >= push_constants.slot_count) {
        return;
    }
    let parameters = indirect_parameters[slot];
    if (parameters.instance_count == 0u) {
        return;
    }
    let mesh_index = indirect_parameters_gpu_metadata[slot].mesh_index;
    if (mesh_index >= arrayLength(&current_input)) {
        return;
    }
    let tag = current_input[mesh_index].tag;
    if ((tag & TAG_LEAF) == 0u || current_input[mesh_index].pad != push_constants.slabs) {
        return;
    }
    let end = parameters.first_index + parameters.index_count;
    if (end < parameters.first_index || end > push_constants.index_words) {
        // Its range isn't in the mirror; drawing nothing beats drawing stale indices.
        indirect_parameters[slot].index_count = 0u;
        if ((push_constants.flags & FLAG_STATS) != 0u) {
            atomicAdd(&stats[STAT_DRAWS_INVALID], 1u);
        }
        return;
    }
    var triangle_count = parameters.index_count / 3u;
    if ((push_constants.flags & FLAG_CLUSTERS) != 0u) {
        let mark = tag & TAG_MARK_BASE;
        let cluster_count = arrayLength(&cluster_words) / 2u;
        let first_cluster = mark / CLUSTER_TRIANGLES;
        let clusters = (triangle_count + CLUSTER_TRIANGLES - 1u) / CLUSTER_TRIANGLES;
        if (triangle_count != 0u && triangle_count <= CLUSTER_COUNT_MASK &&
                mark % CLUSTER_TRIANGLES == 0u && first_cluster + clusters <= cluster_count) {
            let count = cluster_words[cluster_count + first_cluster];
            if ((count >> CLUSTER_STAMP_SHIFT) != (push_constants.flags >> FLAG_STAMP_SHIFT)) {
                // The prepass didn't draw it this frame, so it has no marks, and the mirror may
                // hold an older frame's clusters.
                indirect_parameters[slot].index_count = 0u;
                if ((push_constants.flags & FLAG_STATS) != 0u) {
                    atomicAdd(&stats[STAT_DRAWS_INVALID], 1u);
                }
                return;
            }
            triangle_count = count & CLUSTER_COUNT_MASK;
        }
    }
    if ((push_constants.flags & FLAG_DIRECT) != 0u) {
        // Drawn as the prepass drew it, from its mirror: only the count changes.
        indirect_parameters[slot].index_count = 3u * triangle_count;
        return;
    }
    var mode = MODE_CULL;
    if (parameters.instance_count != 1u) {
        // Several transforms; the copy keeps them all.
        mode = MODE_COPY;
    } else if ((push_constants.flags & FLAG_MARKS) != 0u) {
        mode = MODE_MARKS;
    } else if ((tag & TAG_TWO_SIDED) != 0u) {
        mode = MODE_CULL_TWO_SIDED;
    }
    // At most one item a slot, after the empty one: the CPU sized this dispatch's share of the
    // buffer to its slots and one more.
    let at = atomicAdd(&counts[push_constants.count_word], 1u);
    items_out[push_constants.item_base + at] = Item(
        view.clip_from_world * maths::affine3_to_square(current_input[mesh_index].world_from_local),
        slot,
        mode,
        parameters.first_index,
        triangle_count,
        parameters.base_vertex,
        tag & TAG_MARK_BASE,
    );
}

fn position(vertex: u32) -> vec3<f32> {
    let at = vertex * push_constants.stride + push_constants.position;
    return vec3(vertices[at], vertices[at + 1u], vertices[at + 2u]);
}

// As `triangle_cull.wgsl`'s: `KEEP` if the rasteriser could give the triangle a fragment in the
// main pass that passes the depth test, else the first test that rules that out.
fn test_triangle(
    triangle: vec3<u32>,
    base_vertex: u32,
    two_sided: bool,
    half_size: vec2<f32>,
) -> u32 {
    let clip_from_local = transform;
    let c0 = clip_from_local * vec4(position(base_vertex + triangle.x), 1.0);
    let c1 = clip_from_local * vec4(position(base_vertex + triangle.y), 1.0);
    let c2 = clip_from_local * vec4(position(base_vertex + triangle.z), 1.0);
    let x = vec3(c0.x, c1.x, c2.x);
    let y = vec3(c0.y, c1.y, c2.y);
    let z = vec3(c0.z, c1.z, c2.z);
    let w = vec3(c0.w, c1.w, c2.w);

    if (all(x > w) || all(x < -w) || all(y > w) || all(y < -w) || all(z > w) ||
            all(z < vec3(0.0))) {
        return OUTSIDE_THE_VIEW;
    }
    if (any(z > w) || any(w <= vec3(0.0))) {
        return KEEP;
    }

    let n0 = c0.xyz / c0.w;
    let n1 = c1.xyz / c1.w;
    let n2 = c2.xyz / c2.w;
    let ndc_min = min(n0.xy, min(n1.xy, n2.xy));
    let ndc_max = max(n0.xy, max(n1.xy, n2.xy));
    let s0 = n0.xy * half_size;
    let s1 = n1.xy * half_size;
    let s2 = n2.xy * half_size;
    let s_min = ndc_min * half_size;
    let s_max = ndc_max * half_size;

    if (!two_sided) {
        let area = (s1.x - s0.x) * (s2.y - s0.y) - (s2.x - s0.x) * (s1.y - s0.y);
        if (area < -(s_max.x - s_min.x + s_max.y - s_min.y) * (1.0 / 32.0)) {
            return FACING_AWAY;
        }
    }

    if ((push_constants.flags & FLAG_PIXEL_CENTRES) != 0u) {
        let margin = 1.0 / 64.0;
        let low = vec2(s_min.x + half_size.x, half_size.y - s_max.y) - margin - 0.5;
        let high = vec2(s_max.x + half_size.x, half_size.y - s_min.y) + margin - 0.5;
        if (any(floor(high) < ceil(low))) {
            return NO_PIXEL_CENTRE;
        }
    }

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
@workgroup_size(64)
fn cull(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    // The same address in every thread, from a buffer this dispatch only reads: uniform.
    let item = items_in[push_constants.item_base + workgroup_id.x];
    if (item.mode == MODE_NONE) {
        return;
    }
    if (local_index == 0u) {
        transform = item.clip_from_local;
    }
    if (local_index < 24u) {
        atomicStore(&masks[local_index], 0u);
    }
    if (local_index < 5u) {
        atomicStore(&outcomes[local_index], 0u);
    }
    workgroupBarrier();

    let half_size = view.viewport.zw * 0.5;
    let marking = item.mode == MODE_MARKS;
    let inverted = (push_constants.flags & FLAG_MARKS_INVERT) != 0u;
    let testing = item.mode == MODE_CULL || item.mode == MODE_CULL_TWO_SIDED;
    let two_sided = item.mode == MODE_CULL_TWO_SIDED;
    let counting = (push_constants.flags & FLAG_STATS) != 0u;
    let lane_word = local_index >> 5u;
    let bit = 1u << (local_index & 31u);

    var kept = 0u;
    var rotation = 0u;
    for (var first = 0u; first < item.triangle_count; first += ROUND) {
        let base = rotation * ROUND_WORDS;
        // Bit k: this thread's k-th triangle of the round survives.
        var mine = 0u;
        for (var k = 0u; k < PER_THREAD; k += 1u) {
            let triangle_index = first + local_index + k * CULL_WORKGROUP_SIZE;
            if (triangle_index >= item.triangle_count) {
                break;
            }
            var outcome = KEEP;
            if (marking) {
                // A word past the buffer keeps its triangle.
                let mark = item.mark_base + triangle_index;
                if (mark < arrayLength(&marks)) {
                    let marked = marks[mark] != 0u;
                    if (marked) {
                        marks[mark] = 0u;
                    }
                    if (marked == inverted) {
                        outcome = UNMARKED;
                    }
                }
                if (counting) {
                    atomicAdd(&outcomes[outcome], 1u);
                }
            } else if (testing) {
                let at = item.first_index + 3u * triangle_index;
                let triangle = vec3(indices[at], indices[at + 1u], indices[at + 2u]);
                outcome = test_triangle(triangle, item.base_vertex, two_sided, half_size);
                if (counting) {
                    atomicAdd(&outcomes[outcome], 1u);
                }
            }
            if (outcome == KEEP) {
                mine |= 1u << k;
                atomicOr(&masks[base + 2u * k + lane_word], bit);
            }
        }
        workgroupBarrier();

        // Survivors before each of this thread's, for an output in the original order: coplanar
        // triangles drawn in a different order would fight. The indices are read again rather
        // than held through the barrier.
        var before = 0u;
        for (var word = 0u; word < ROUND_WORDS; word += 1u) {
            let bits = atomicLoad(&masks[base + word]);
            if ((word & 1u) == lane_word && (mine & (1u << (word >> 1u))) != 0u) {
                let source = item.first_index +
                    3u * (first + local_index + (word >> 1u) * CULL_WORKGROUP_SIZE);
                let to = item.first_index + 3u * (kept + before + countOneBits(bits & (bit - 1u)));
                mirror[to] = indices[source];
                mirror[to + 1u] = indices[source + 1u];
                mirror[to + 2u] = indices[source + 2u];
            }
            before += countOneBits(bits);
        }
        // Clears the previous round's set: everyone read it before this round's barrier, and the
        // round after next, the first to fill it again, does so only after the next round's.
        if (local_index < ROUND_WORDS) {
            atomicStore(&masks[((rotation + 2u) % 3u) * ROUND_WORDS + local_index], 0u);
        }
        kept += before;
        rotation = (rotation + 1u) % 3u;
    }

    if (local_index == 0u) {
        indirect_parameters[item.slot].index_count = 3u * kept;
        if (counting) {
            // The last round's barrier (or the first, if there was no round) ordered every count
            // before this.
            if (testing || marking) {
                atomicAdd(&stats[STAT_DRAWS_TESTED], 1u);
                for (var i = 0u; i < 5u; i += 1u) {
                    atomicAdd(&stats[i], atomicLoad(&outcomes[i]));
                }
            } else {
                atomicAdd(&stats[STAT_DRAWS_COPIED], 1u);
            }
        }
    }
}

// Bit k set where component k is.
fn bits4(b: vec4<bool>) -> u32 {
    return select(0u, 1u, b.x) | select(0u, 2u, b.y) | select(0u, 4u, b.z) | select(0u, 8u, b.w);
}

// Under `FLAG_MARKS`, `keep_marked` runs in `cull`'s place (unless `TRIANGLE_CULL_MARKS_IN_CULL`),
// for the same items and with the same outcome: the kept triangles in their order, and the marks
// cleared. `cull` hands each thread every 64th triangle of a round, and sets each survivor's bit
// with an `atomicOr` on a word all 32 threads of its warp share, which the GPU serialises; and
// NAK issues each of its loads only after the store before it. Here a thread takes four
// neighbouring triangles, whose bits go into one word with one `atomicOr`, eight threads to a
// word, and it loads its marks, then its survivors' indices, before it stores anything.
@compute
@workgroup_size(64)
fn keep_marked(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    // The same address in every thread, from a buffer this dispatch only reads: uniform.
    let item = items_in[push_constants.item_base + workgroup_id.x];
    if (item.mode == MODE_NONE) {
        return;
    }
    if (local_index < 24u) {
        atomicStore(&masks[local_index], 0u);
    }
    if (local_index < 5u) {
        atomicStore(&outcomes[local_index], 0u);
    }
    workgroupBarrier();

    let marking = item.mode == MODE_MARKS;
    let inverted = (push_constants.flags & FLAG_MARKS_INVERT) != 0u;
    let counting = (push_constants.flags & FLAG_STATS) != 0u;
    let mark_words = arrayLength(&marks);
    let last_mark = max(mark_words, 1u) - 1u;
    // Triangle i of a round is bit i % 32 of word i / 32 of its set, so this thread's four are
    // the four bits from `shift` of word `word`.
    let word = local_index >> 3u;
    let shift = (local_index & 7u) * 4u;
    let below = (1u << shift) - 1u;

    var kept = 0u;
    var rotation = 0u;
    for (var first = 0u; first < item.triangle_count; first += ROUND) {
        let base = rotation * ROUND_WORDS;
        let triangle = first + PER_THREAD * local_index;
        // Bit k: triangle + k is one of the item's.
        let present = (1u << min(item.triangle_count - min(triangle, item.triangle_count), 4u)) -
            1u;
        // Bit k of `mine`: triangle + k is kept; of `clear`: its mark is to be set back to 0.
        var mine = present;
        var clear = 0u;
        if (marking) {
            // A word past the buffer keeps its triangle, as in `cull`. Words past the item's
            // range are read too, and ignored.
            let at = item.mark_base + triangle;
            let marked = bits4(vec4(
                marks[min(at, last_mark)],
                marks[min(at + 1u, last_mark)],
                marks[min(at + 2u, last_mark)],
                marks[min(at + 3u, last_mark)],
            ) != vec4(0u));
            let inside = present & bits4(vec4(at, at + 1u, at + 2u, at + 3u) < vec4(mark_words));
            clear = inside & marked;
            mine = (present & ~inside) | (inside & select(marked, ~marked, inverted));
            if (counting) {
                atomicAdd(&outcomes[KEEP], countOneBits(mine));
                atomicAdd(&outcomes[UNMARKED], countOneBits(present & ~mine));
            }
        }
        // The survivors' indices, held through the barrier. The rest read the item's first
        // triangle again, which is cached, rather than branch.
        let source = item.first_index + 3u * triangle;
        let a0 = select(item.first_index, source, (mine & 1u) != 0u);
        let a1 = select(item.first_index, source + 3u, (mine & 2u) != 0u);
        let a2 = select(item.first_index, source + 6u, (mine & 4u) != 0u);
        let a3 = select(item.first_index, source + 9u, (mine & 8u) != 0u);
        let t0 = vec3(indices[a0], indices[a0 + 1u], indices[a0 + 2u]);
        let t1 = vec3(indices[a1], indices[a1 + 1u], indices[a1 + 2u]);
        let t2 = vec3(indices[a2], indices[a2 + 1u], indices[a2 + 2u]);
        let t3 = vec3(indices[a3], indices[a3 + 1u], indices[a3 + 2u]);
        if (clear != 0u) {
            let at = item.mark_base + triangle;
            if ((clear & 1u) != 0u) {
                marks[at] = 0u;
            }
            if ((clear & 2u) != 0u) {
                marks[at + 1u] = 0u;
            }
            if ((clear & 4u) != 0u) {
                marks[at + 2u] = 0u;
            }
            if ((clear & 8u) != 0u) {
                marks[at + 3u] = 0u;
            }
        }
        if (mine != 0u) {
            atomicOr(&masks[base + word], mine << shift);
        }
        workgroupBarrier();

        // Survivors before this thread's first: all those of the set's earlier words, and those
        // of the threads before it in its own.
        var before = 0u;
        var total = 0u;
        for (var w = 0u; w < ROUND_WORDS; w += 1u) {
            let bits = atomicLoad(&masks[base + w]);
            let earlier = select(bits, bits & below, w == word);
            before += select(0u, countOneBits(earlier), w <= word);
            total += countOneBits(bits);
        }
        var to = item.first_index + 3u * (kept + before);
        if ((mine & 1u) != 0u) {
            mirror[to] = t0.x;
            mirror[to + 1u] = t0.y;
            mirror[to + 2u] = t0.z;
            to += 3u;
        }
        if ((mine & 2u) != 0u) {
            mirror[to] = t1.x;
            mirror[to + 1u] = t1.y;
            mirror[to + 2u] = t1.z;
            to += 3u;
        }
        if ((mine & 4u) != 0u) {
            mirror[to] = t2.x;
            mirror[to + 1u] = t2.y;
            mirror[to + 2u] = t2.z;
            to += 3u;
        }
        if ((mine & 8u) != 0u) {
            mirror[to] = t3.x;
            mirror[to + 1u] = t3.y;
            mirror[to + 2u] = t3.z;
        }
        // As in `cull`: clears the previous round's set, which everyone read before this round's
        // barrier.
        if (local_index < ROUND_WORDS) {
            atomicStore(&masks[((rotation + 2u) % 3u) * ROUND_WORDS + local_index], 0u);
        }
        kept += total;
        rotation = (rotation + 1u) % 3u;
    }

    if (local_index == 0u) {
        indirect_parameters[item.slot].index_count = 3u * kept;
        if (counting) {
            // The last round's barrier (or the first, if there was no round) ordered every count
            // before this.
            if (marking) {
                atomicAdd(&stats[STAT_DRAWS_TESTED], 1u);
                for (var i = 0u; i < 5u; i += 1u) {
                    atomicAdd(&stats[i], atomicLoad(&outcomes[i]));
                }
            } else {
                atomicAdd(&stats[STAT_DRAWS_COPIED], 1u);
            }
        }
    }
}

// Under `TRIANGLE_CULL_MARKS_COALESCED`, `keep_marked_coalesced` runs in `keep_marked`'s place,
// for the same items and with the same outcome. In `keep_marked`, a warp's loads and stores touch
// words 16 or 48 bytes apart, as each thread takes four neighbouring triangles, and the GPU
// fetches a sector for each word: perf27's cull took about 5 ns for every triangle it tested
// (findings 47). Here neighbouring threads touch neighbouring words. They read the round's marks
// into `round_marks`, triangles t, t + 64, t + 128 and t + 192 a thread; each thread then takes
// its four neighbouring triangles from there for the round's bit set, as `keep_marked` does, and
// writes where its survivors go into `round_ranks`; then they copy the survivors' index words
// to the mirror a word a thread. Three barriers a round, against one.

// In `round_marks`. A triangle whose word is past the buffer's end is kept, as in `keep_marked`.
const MARK_UNSET: u32 = 0u;
const MARK_SET: u32 = 1u;
const MARK_PAST_END: u32 = 2u;
// In `round_ranks`: the triangle isn't kept.
const NOT_KEPT: u32 = 0xffffffffu;

// Where the round's index word `w` goes in the mirror, whose word `to` the round's first survivor
// takes: three words further for each survivor before its triangle, plus its corner; or
// `NOT_KEPT`. `w` is below 3 * ROUND, and `round_ranks` holds `NOT_KEPT` for every triangle past
// the round's.
fn word_destination(w: u32, to: u32) -> u32 {
    // w / 3, exact for every w below 3 * ROUND.
    let triangle = (w * 43691u) >> 17u;
    let rank = round_ranks[triangle >> 2u][triangle & 3u];
    return select(to + 3u * rank + w - 3u * triangle, NOT_KEPT, rank == NOT_KEPT);
}

@compute
@workgroup_size(64)
fn keep_marked_coalesced(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    // The same address in every thread, from a buffer this dispatch only reads: uniform.
    let item = items_in[push_constants.item_base + workgroup_id.x];
    if (item.mode == MODE_NONE) {
        return;
    }
    if (local_index < 24u) {
        atomicStore(&masks[local_index], 0u);
    }
    if (local_index < 5u) {
        atomicStore(&outcomes[local_index], 0u);
    }
    workgroupBarrier();

    let marking = item.mode == MODE_MARKS;
    let inverted = (push_constants.flags & FLAG_MARKS_INVERT) != 0u;
    let counting = (push_constants.flags & FLAG_STATS) != 0u;
    let mark_words = arrayLength(&marks);
    let last_mark = max(mark_words, 1u) - 1u;
    // The triangles of a round whose marks this thread reads, and the four words of each pass
    // of its copy: neighbouring threads, neighbouring triangles and words.
    let spread = vec4(local_index) + vec4(0u, 64u, 128u, 192u);
    // This thread's four neighbouring triangles of a round, from `triangle`, are the four bits
    // from `shift` of word `word` of its set.
    let triangle = PER_THREAD * local_index;
    let word = local_index >> 3u;
    let shift = (local_index & 7u) * 4u;
    let below = (1u << shift) - 1u;

    var kept = 0u;
    var rotation = 0u;
    for (var first = 0u; first < item.triangle_count; first += ROUND) {
        let base = rotation * ROUND_WORDS;
        let count = min(item.triangle_count - first, ROUND);
        if (marking) {
            // Every load, then every store, as in `keep_marked`. Words past the round's
            // triangles are read too, and ignored. A round wholly inside the buffer reads its
            // words at offsets from one address, which NAK can issue together.
            let at = vec4(item.mark_base + first) + spread;
            var words: vec4<u32>;
            if (item.mark_base + first + ROUND <= mark_words) {
                words = vec4(
                    marks[at.x],
                    marks[at.x + 64u],
                    marks[at.x + 128u],
                    marks[at.x + 192u],
                );
            } else {
                words = vec4(
                    marks[min(at.x, last_mark)],
                    marks[min(at.y, last_mark)],
                    marks[min(at.z, last_mark)],
                    marks[min(at.w, last_mark)],
                );
            }
            let in_round = spread < vec4(count);
            let inside = at < vec4(mark_words);
            // Its mark is to be set back to 0.
            let clear = in_round & inside & (words != vec4(0u));
            let state = select(
                select(vec4(MARK_UNSET), vec4(MARK_PAST_END), in_round & !inside),
                vec4(MARK_SET),
                clear,
            );
            if (clear.x) {
                marks[at.x] = 0u;
            }
            if (clear.y) {
                marks[at.y] = 0u;
            }
            if (clear.z) {
                marks[at.z] = 0u;
            }
            if (clear.w) {
                marks[at.w] = 0u;
            }
            round_marks[spread.x >> 2u][spread.x & 3u] = state.x;
            round_marks[spread.y >> 2u][spread.y & 3u] = state.y;
            round_marks[spread.z >> 2u][spread.z & 3u] = state.z;
            round_marks[spread.w >> 2u][spread.w & 3u] = state.w;
        }
        workgroupBarrier();

        // Bit k of `mine`: triangle + k is kept, as in `keep_marked`.
        let present = (1u << min(count - min(triangle, count), 4u)) - 1u;
        var mine = present;
        if (marking) {
            let state = round_marks[local_index];
            let marked = bits4(state == vec4(MARK_SET));
            let inside = present & ~bits4(state == vec4(MARK_PAST_END));
            mine = (present & ~inside) | (inside & select(marked, ~marked, inverted));
            if (counting) {
                atomicAdd(&outcomes[KEEP], countOneBits(mine));
                atomicAdd(&outcomes[UNMARKED], countOneBits(present & ~mine));
            }
        }
        if (mine != 0u) {
            atomicOr(&masks[base + word], mine << shift);
        }
        workgroupBarrier();

        // Survivors before this thread's first, as in `keep_marked`, and in the round.
        var before = 0u;
        var total = 0u;
        for (var w = 0u; w < ROUND_WORDS; w += 1u) {
            let bits = atomicLoad(&masks[base + w]);
            let earlier = select(bits, bits & below, w == word);
            before += select(0u, countOneBits(earlier), w <= word);
            total += countOneBits(bits);
        }
        // Each of its four triangles' rank among the round's survivors, or `NOT_KEPT`.
        let keep = (vec4(mine) & vec4(1u, 2u, 4u, 8u)) != vec4(0u);
        let rank = vec4(before) + countOneBits(vec4(mine) & vec4(0u, 1u, 3u, 7u));
        round_ranks[local_index] = select(vec4(NOT_KEPT), rank, keep);
        workgroupBarrier();

        // The survivors' index words, in their order, from where the round's triangles start to
        // where its first survivor goes. `w` is 2 * ROUND at most, so `at` stays below
        // 3 * ROUND.
        if (total != 0u) {
            let source = item.first_index + 3u * first;
            let to = item.first_index + 3u * kept;
            for (var w = 0u; w < 3u * count; w += PER_THREAD * CULL_WORKGROUP_SIZE) {
                let at = vec4(w) + spread;
                let destination = vec4(
                    word_destination(at.x, to),
                    word_destination(at.y, to),
                    word_destination(at.z, to),
                    word_destination(at.w, to),
                );
                let copy = destination != vec4(NOT_KEPT);
                // Every load, then every store. The rest read the round's first word again,
                // which is cached, rather than branch.
                let origin = select(vec4(source), vec4(source) + at, copy);
                let value = vec4(
                    indices[origin.x],
                    indices[origin.y],
                    indices[origin.z],
                    indices[origin.w],
                );
                if (copy.x) {
                    mirror[destination.x] = value.x;
                }
                if (copy.y) {
                    mirror[destination.y] = value.y;
                }
                if (copy.z) {
                    mirror[destination.z] = value.z;
                }
                if (copy.w) {
                    mirror[destination.w] = value.w;
                }
            }
        }
        // As in `keep_marked`: clears the previous round's set, which everyone read before this
        // round's first barrier.
        if (local_index < ROUND_WORDS) {
            atomicStore(&masks[((rotation + 2u) % 3u) * ROUND_WORDS + local_index], 0u);
        }
        kept += total;
        rotation = (rotation + 1u) % 3u;
    }

    if (local_index == 0u) {
        indirect_parameters[item.slot].index_count = 3u * kept;
        if (counting) {
            // The last round's barriers (or the first, if there was no round) ordered every
            // count before this.
            if (marking) {
                atomicAdd(&stats[STAT_DRAWS_TESTED], 1u);
                for (var i = 0u; i < 5u; i += 1u) {
                    atomicAdd(&stats[i], atomicLoad(&outcomes[i]));
                }
            } else {
                atomicAdd(&stats[STAT_DRAWS_COPIED], 1u);
            }
        }
    }
}

#ifdef TRIANGLE_CULL_BALLOT
// Under `TRIANGLE_CULL_MARKS_BALLOT`, `keep_marked_ballot` runs in `keep_marked`'s place, for the
// same items and with the same outcome. `keep_marked` works out where each survivor goes through
// workgroup memory, with an atomic and a barrier a round, and its two warps issue about 800
// instructions for every 256 triangles. Here an item's workgroup is 32 threads, one subgroup on
// the Switch's GPU, and a subgroup ballot hands every thread the same bit set of which of 32
// triangles survive, so a survivor's place is a count of the set bits below its own: no workgroup
// memory, atomic or barrier. A round takes 128 triangles, thread t taking triangles t, t + 32,
// t + 64 and t + 96 of it, one in each quarter, so that neighbouring threads read and write
// neighbouring words; each quarter has its own ballot, taken in order. A round in which no thread
// keeps a triangle reads no indices and writes nothing to the mirror.
// `triangle_cull.rs` runs it only where every subgroup has at least 32 invocations, so that the
// workgroup is one subgroup, and compiles it only then (`TRIANGLE_CULL_BALLOT`), so that the
// other entry points still validate on a device without subgroups.
// Under `TRIANGLE_CULL_BALLOT_8` the workgroup is 8 threads and a round 32 triangles, only to
// test the kernel in Ryujinx on llvmpipe: it hands a warp's lanes and ballots to the host's
// subgroups of 8, so there the 32-thread workgroup is four subgroups that each take themselves
// for the whole of it.
#ifdef TRIANGLE_CULL_BALLOT_8
const BALLOT_LANES: u32 = 8u;
#else
const BALLOT_LANES: u32 = 32u;
#endif
const BALLOT_ROUND: u32 = 4u * BALLOT_LANES;

@compute
#ifdef TRIANGLE_CULL_BALLOT_8
@workgroup_size(8)
#else
@workgroup_size(32)
#endif
fn keep_marked_ballot(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(subgroup_invocation_id) lane: u32,
) {
    // Uniform, as in `keep_marked`, so every ballot below runs in all the workgroup's threads.
    let item = items_in[push_constants.item_base + workgroup_id.x];
    if (item.mode == MODE_NONE) {
        return;
    }
    let marking = item.mode == MODE_MARKS;
    let inverted = vec4((push_constants.flags & FLAG_MARKS_INVERT) != 0u);
    let mark_words = arrayLength(&marks);
    let last_mark = max(mark_words, 1u) - 1u;
    let below = vec4((1u << lane) - 1u);
    let quarters = vec4(0u, BALLOT_LANES, 2u * BALLOT_LANES, 3u * BALLOT_LANES);
    let first_index = vec4(item.first_index);

    var kept = 0u;
    for (var first = 0u; first < item.triangle_count; first += BALLOT_ROUND) {
        let triangle = vec4(first + lane) + quarters;
        let present = triangle < vec4(item.triangle_count);
        // Component k: this thread's triangle in quarter k is kept; its mark is to be set back
        // to 0.
        var mine = present;
        var clear = vec4(false);
        if (marking) {
            // As in `keep_marked`: a word past the buffer keeps its triangle, and words past the
            // item's range are read too, and ignored.
            let at = vec4(item.mark_base) + triangle;
            let marked = vec4(
                marks[min(at.x, last_mark)],
                marks[min(at.y, last_mark)],
                marks[min(at.z, last_mark)],
                marks[min(at.w, last_mark)],
            ) != vec4(0u);
            let inside = present & (at < vec4(mark_words));
            clear = inside & marked;
            mine = (present & !inside) | (inside & (marked != inverted));
        }
        if (any(clear)) {
            let at = item.mark_base + first + lane;
            if (clear.x) {
                marks[at] = 0u;
            }
            if (clear.y) {
                marks[at + BALLOT_LANES] = 0u;
            }
            if (clear.z) {
                marks[at + 2u * BALLOT_LANES] = 0u;
            }
            if (clear.w) {
                marks[at + 3u * BALLOT_LANES] = 0u;
            }
        }
        // The same in every thread.
        let ballots = vec4(
            subgroupBallot(mine.x).x,
            subgroupBallot(mine.y).x,
            subgroupBallot(mine.z).x,
            subgroupBallot(mine.w).x,
        );
        let counts = countOneBits(ballots);
        if (any(ballots != vec4(0u))) {
            // The survivors' indices, all read before any is written. The rest read the item's
            // first triangle again, which is cached, rather than branch.
            let source = select(first_index, first_index + 3u * triangle, mine);
            let t0 = vec3(indices[source.x], indices[source.x + 1u], indices[source.x + 2u]);
            let t1 = vec3(indices[source.y], indices[source.y + 1u], indices[source.y + 2u]);
            let t2 = vec3(indices[source.z], indices[source.z + 1u], indices[source.z + 2u]);
            let t3 = vec3(indices[source.w], indices[source.w + 1u], indices[source.w + 2u]);
            // The survivors before each of this thread's: the earlier rounds', the earlier
            // quarters', and those of the threads before it in its own quarter.
            let earlier = vec4(
                kept,
                kept + counts.x,
                kept + counts.x + counts.y,
                kept + counts.x + counts.y + counts.z,
            );
            let to = first_index + 3u * (earlier + countOneBits(ballots & below));
            if (mine.x) {
                mirror[to.x] = t0.x;
                mirror[to.x + 1u] = t0.y;
                mirror[to.x + 2u] = t0.z;
            }
            if (mine.y) {
                mirror[to.y] = t1.x;
                mirror[to.y + 1u] = t1.y;
                mirror[to.y + 2u] = t1.z;
            }
            if (mine.z) {
                mirror[to.z] = t2.x;
                mirror[to.z + 1u] = t2.y;
                mirror[to.z + 2u] = t2.z;
            }
            if (mine.w) {
                mirror[to.w] = t3.x;
                mirror[to.w + 1u] = t3.y;
                mirror[to.w + 2u] = t3.z;
            }
        }
        kept += counts.x + counts.y + counts.z + counts.w;
    }

    if (lane == 0u) {
        indirect_parameters[item.slot].index_count = 3u * kept;
        if ((push_constants.flags & FLAG_STATS) != 0u) {
            // Each of the item's triangles is either kept or left out, so `keep_marked`'s counts
            // follow from `kept`.
            if (marking) {
                atomicAdd(&stats[STAT_DRAWS_TESTED], 1u);
                atomicAdd(&stats[KEEP], kept);
                atomicAdd(&stats[UNMARKED], item.triangle_count - kept);
            } else {
                atomicAdd(&stats[STAT_DRAWS_COPIED], 1u);
            }
        }
    }
}
#endif
