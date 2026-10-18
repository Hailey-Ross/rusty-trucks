// skate: the depth prepass's cluster cull (vendor/README.md, triangle_cull.rs,
// `TRIANGLE_CULL_CLUSTERS`). The prepass draws a world leaf whole, though most of its triangles
// end up behind others; this draws only the 32-triangle clusters of each leaf whose box could
// be seen, by writing their indices, in their order, into the prepass's own mirror of the index
// slab, which its draws of the leaves then read in place of the slab.
//
// `early` runs after the early prepass's indirect parameters are built, and tests each cluster
// of a leaf drawn there against the previous frame's view and depth pyramid, as Bevy's early
// pass tests meshes; it records each cluster's outcome in `cluster_words`. `late` runs after the
// late prepass's are built, and tests against this frame's view and the pyramid the early pass
// left: a leaf first drawn in the late pass has all its clusters tested, and a leaf drawn in the
// early pass has those the early pass left out tested again, and the survivors drawn by its slot
// in the late pass, after those already drawn.
//
// A leaf's clusters start at its mark base (`TAG_MARK_BASE` bits of its tag) over 32, which the
// game keeps a multiple of 32; their boxes are in `boxes`, their outcomes in the first half of
// `cluster_words`, and the leaf's count word in the second, at the same place: the triangles
// its prepass draws read from the mirror, `COMPACTED` if they're its kept clusters' rather than
// the whole leaf, and the frame's stamp, so the main pass's `list` (`FLAG_CLUSTERS`) knows to
// keep the marked triangles among those, not among the slab's.
//
// Anything but one instance drawn as its mesh's input says (several instances, or one whose
// transform or tag differs) is copied whole, as is a leaf without boxes; such a draw stays as
// Bevy built it.
//
// `early_fast` and `late_fast`, the second kernel (`TRIANGLE_CULL_CLUSTERS_KERNEL_SHIFT` in
// triangle_cull.rs), do the same jobs with the same outcomes, but wait on memory less often. A
// workgroup gets only about two leaves among its eight slots, so its time goes on reads waiting
// for the reads before them: these ask for the pyramid's size once, not twice a box; read a
// slot's draw in two waits, not five; read a box with its early outcome; read the pyramid 24
// texels a wait, not 8; and copy four surviving clusters a wait, not one.

#import bevy_pbr::mesh_preprocess_types::{
    IndirectParametersCpuMetadata,
    IndirectParametersGpuMetadata,
    IndirectParametersIndexed,
    MeshInput
}
#import bevy_pbr::mesh_types::Mesh
#import bevy_pbr::occlusion_culling
#import bevy_render::maths
#import bevy_render::view::View

struct PushConstants {
    // The draws in this phase's indirect parameters buffer.
    slot_count: u32,
    // The index slab's size in indices, which the mirror's is too.
    index_words: u32,
    // The ids of the vertex and index slabs whose draws this dispatch covers, as
    // `MeshInput::pad` carries a mesh's (`triangle_cull::slab_word`).
    slabs: u32,
    // The pyramid test's span (`occluded_finely_batched`); 0 skips that test.
    span: u32,
    flags: u32,
    // This frame's stamp, 1 to 255, for the count words.
    stamp: u32,
}

// `bevy_core_pipeline::prepass::PreviousViewData`, declared here as `prepass_bindings` binds its
// own.
struct PreviousViewData {
    view_from_world: mat4x4<f32>,
    clip_from_world: mat4x4<f32>,
    clip_from_view: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    view_from_clip: mat4x4<f32>,
}

// The box around one cluster's vertices, in its leaf's local space (the game's
// `push_cluster_boxes`).
struct ClusterBox {
    low: vec4<f32>,
    high: vec4<f32>,
}

// What a slot's leaf needs, worked out by one thread and done by the workgroup.
struct Job {
    // The test's transform: the previous frame's view in `early`, this frame's in `late`.
    clip_from_local: mat4x4<f32>,
    slot: u32,
    action: u32,
    // The leaf's first index in the slab, and the mirror, and its triangles.
    first_index: u32,
    triangle_count: u32,
    first_cluster: u32,
    // The leaf's count word in `cluster_words`, or `NO_WORD` if it has no boxes.
    count_word: u32,
    // `ACTION_RETEST`: the triangles the early pass kept, which the late draw starts after.
    kept_early: u32,
    // The draw's first instance in `meshes`.
    base: u32,
    // `ACTION_COPY` in `late`: if non-zero, the instances to draw from `base`, early and late
    // alike, as the early draw had only the kept clusters.
    instances: u32,
    // `ACTION_TEST` in `early`: record each cluster's outcome for `late`.
    record: u32,
}

const FLAG_STATS: u32 = 2u;
// This dispatch's draws are the cutouts': its counts go to words 16 to 23 of `stats`, not 8 to
// 15.
const FLAG_CUTOUTS: u32 = 4u;

const TAG_LEAF: u32 = 0x80000000u;
const TAG_MARK_BASE: u32 = 0x1fffffffu;

const ACTION_NONE: u32 = 0u;
// Test every cluster, and write the survivors' triangles from the leaf's first index.
const ACTION_TEST: u32 = 1u;
// Test the clusters the early pass left out, and write the survivors' after those it kept.
const ACTION_RETEST: u32 = 2u;
const ACTION_COPY: u32 = 3u;

const CLUSTER_TRIANGLES: u32 = 32u;
const SLOTS_PER_WORKGROUP: u32 = 8u;
const NO_WORD: u32 = 0xffffffffu;
// In a count word.
const COUNT_MASK: u32 = 0x7fffffu;
const COMPACTED: u32 = 0x800000u;
const STAMP_SHIFT: u32 = 24u;

// In `outcomes`, added to `stats` from word 8: clusters tested and kept (words 8 and 9 for
// `early`, 10 and 11 for `late`), then words 12 to 15.
const OUTCOME_TESTED: u32 = 0u;
const OUTCOME_KEPT: u32 = 1u;
// The triangles of the leaves whose clusters were tested, once a leaf a frame.
const OUTCOME_LEAF_TRIANGLES: u32 = 2u;
// The triangles of theirs the prepass draws.
const OUTCOME_DRAWN: u32 = 3u;
// Those leaves.
const OUTCOME_LEAVES: u32 = 4u;
// The leaves copied whole.
const OUTCOME_COPIED: u32 = 5u;

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage> current_input: array<MeshInput>;
@group(0) @binding(2) var<storage> indirect_parameters_gpu_metadata:
    array<IndirectParametersGpuMetadata>;
@group(0) @binding(3) var<storage, read_write> indirect_parameters:
    array<IndirectParametersIndexed>;
@group(0) @binding(4) var<storage> indirect_parameters_cpu_metadata:
    array<IndirectParametersCpuMetadata>;
@group(0) @binding(5) var<storage> indices: array<u32>;
// The prepass's mirror of the index slab.
@group(0) @binding(6) var<storage, read_write> mirror: array<u32>;
@group(0) @binding(7) var depth_pyramid: texture_2d<f32>;
@group(0) @binding(8) var<storage, read_write> stats: array<atomic<u32>, 24>;
@group(0) @binding(9) var<storage> boxes: array<ClusterBox>;
// One word for each cluster (1: the early pass kept it), then one for each, of which a leaf's
// first is its count word.
@group(0) @binding(10) var<storage, read_write> cluster_words: array<u32>;
@group(0) @binding(12) var<uniform> previous_view: PreviousViewData;
// The phase's instances, as the preprocessing wrote them for the prepass's draws.
@group(0) @binding(13) var<storage, read_write> meshes: array<Mesh>;

var<push_constant> push_constants: PushConstants;

var<workgroup> jobs: array<Job, 8>;
var<workgroup> job_bits: atomic<u32>;
var<workgroup> job_mask: u32;
// Which of a round's 32 clusters survive, in three rotating words so that one barrier a round
// is enough: the word a round fills was cleared two rounds earlier.
var<workgroup> round_masks: array<atomic<u32>, 3>;
var<workgroup> outcomes: array<atomic<u32>, 6>;

// This thread's counts, for `outcomes`.
var<private> lane_outcomes: array<u32, 6>;

@compute
@workgroup_size(32)
fn early(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    run(workgroup_id.x, lane, false);
}

@compute
@workgroup_size(32)
fn late(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    run(workgroup_id.x, lane, true);
}

@compute
@workgroup_size(32)
fn early_fast(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    run_fast(workgroup_id.x, lane, false);
}

@compute
@workgroup_size(32)
fn late_fast(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    run_fast(workgroup_id.x, lane, true);
}

fn run(workgroup: u32, lane: u32, late: bool) {
    if (lane < 3u) {
        atomicStore(&round_masks[lane], 0u);
    }
    if (lane < 6u) {
        atomicStore(&outcomes[lane], 0u);
    }
    if (lane == 0u) {
        atomicStore(&job_bits, 0u);
    }
    workgroupBarrier();
    if (lane < SLOTS_PER_WORKGROUP) {
        let job = plan(workgroup * SLOTS_PER_WORKGROUP + lane, late);
        if (job.action != ACTION_NONE) {
            jobs[lane] = job;
            atomicOr(&job_bits, 1u << lane);
        }
    }
    workgroupBarrier();
    if (lane == 0u) {
        job_mask = atomicLoad(&job_bits);
    }
    // Every loop below runs the same in each thread: its bounds come from here and from the
    // jobs, which `workgroupUniformLoad` hands every thread alike.
    var pending = workgroupUniformLoad(&job_mask);
    var rotation = 0u;
    while (pending != 0u) {
        let i = firstTrailingBit(pending);
        pending &= pending - 1u;
        let job = workgroupUniformLoad(&jobs[i]);
        if (job.action == ACTION_COPY) {
            copy_leaf(job, lane);
        } else {
            rotation = cull_clusters(job, lane, rotation);
        }
    }
    add_outcomes(lane, late);
}

// `run` for `early_fast` and `late_fast`.
fn run_fast(workgroup: u32, lane: u32, late: bool) {
    // Asked for once rather than for each box: a level's size follows from the first's.
    let size = textureDimensions(depth_pyramid, 0);
    let levels = textureNumLevels(depth_pyramid);
    if (lane < 3u) {
        atomicStore(&round_masks[lane], 0u);
    }
    if (lane < 6u) {
        atomicStore(&outcomes[lane], 0u);
    }
    if (lane == 0u) {
        atomicStore(&job_bits, 0u);
    }
    workgroupBarrier();
    if (lane < SLOTS_PER_WORKGROUP) {
        let job = plan_fast(workgroup * SLOTS_PER_WORKGROUP + lane, late);
        if (job.action != ACTION_NONE) {
            jobs[lane] = job;
            atomicOr(&job_bits, 1u << lane);
        }
    }
    workgroupBarrier();
    if (lane == 0u) {
        job_mask = atomicLoad(&job_bits);
    }
    var pending = workgroupUniformLoad(&job_mask);
    var rotation = 0u;
    while (pending != 0u) {
        let i = firstTrailingBit(pending);
        pending &= pending - 1u;
        let job = workgroupUniformLoad(&jobs[i]);
        if (job.action == ACTION_COPY) {
            copy_leaf(job, lane);
        } else {
            rotation = cull_clusters_fast(job, lane, rotation, size, levels);
        }
    }
    add_outcomes(lane, late);
}

// Adds the workgroup's counts to `stats`, if it's counting.
fn add_outcomes(lane: u32, late: bool) {
    if ((push_constants.flags & FLAG_STATS) != 0u) {
        for (var o = 0u; o < 6u; o += 1u) {
            if (lane_outcomes[o] != 0u) {
                atomicAdd(&outcomes[o], lane_outcomes[o]);
            }
        }
        workgroupBarrier();
        if (lane < 6u) {
            let count = atomicLoad(&outcomes[lane]);
            if (count != 0u) {
                // Clusters to words 8 and 9, or 10 and 11; the rest to 12 to 15. The cutouts'
                // to the same words 8 further on.
                let word = select(10u + lane, 8u + lane + select(0u, 2u, late), lane < 2u) +
                    select(0u, 8u, (push_constants.flags & FLAG_CUTOUTS) != 0u);
                atomicAdd(&stats[word], count);
            }
        }
    }
}

// The slot's job in this pass. Also leaves out a leaf's draw whose range isn't in the mirror.
fn plan(slot: u32, late: bool) -> Job {
    var job = Job();
    if (slot >= push_constants.slot_count) {
        return job;
    }
    let early_count = indirect_parameters_gpu_metadata[slot].early_instance_count;
    let late_count = indirect_parameters_gpu_metadata[slot].late_instance_count;
    if (early_count == 0u && (!late || late_count == 0u)) {
        return job;
    }
    let mesh_index = indirect_parameters_gpu_metadata[slot].mesh_index;
    if (mesh_index >= arrayLength(&current_input)) {
        return job;
    }
    let input = current_input[mesh_index];
    if ((input.tag & TAG_LEAF) == 0u || input.pad != push_constants.slabs) {
        return job;
    }
    // As this pass's build left them: the leaf's whole range, and this pass's instances.
    let parameters = indirect_parameters[slot];
    let end = parameters.first_index + parameters.index_count;
    if (end < parameters.first_index || end > push_constants.index_words) {
        // Its range isn't in the mirror; drawing nothing beats drawing stale indices.
        indirect_parameters[slot].index_count = 0u;
        return job;
    }
    let triangles = parameters.index_count / 3u;
    let clusters = (triangles + CLUSTER_TRIANGLES - 1u) / CLUSTER_TRIANGLES;
    let mark = input.tag & TAG_MARK_BASE;
    let cluster_count = arrayLength(&cluster_words) / 2u;
    let first_cluster = mark / CLUSTER_TRIANGLES;
    let boxed = triangles != 0u && triangles <= COUNT_MASK && mark % CLUSTER_TRIANGLES == 0u &&
        first_cluster + clusters <= min(cluster_count, arrayLength(&boxes));

    job.slot = slot;
    job.first_index = parameters.first_index;
    job.triangle_count = triangles;
    job.first_cluster = first_cluster;
    job.count_word = select(NO_WORD, cluster_count + first_cluster, boxed);
    job.base = indirect_parameters_cpu_metadata[slot].base_output_index;

    if (!late || early_count == 0u) {
        // The leaf's first draw this frame.
        let instances = select(early_count, late_count, late);
        if (boxed && instances == 1u && drawn_as_given(job.base, input)) {
            job.action = ACTION_TEST;
            job.record = select(1u, 0u, late);
            var clip_from_world = view.clip_from_world;
            if (!late) {
                clip_from_world = previous_view.clip_from_world;
            }
            job.clip_from_local =
                clip_from_world * maths::affine3_to_square(input.world_from_local);
        } else {
            job.action = ACTION_COPY;
        }
        return job;
    }

    // Drawn in the early pass, and not copied whole there: the clusters it left out.
    if (!boxed) {
        return job;
    }
    let count = cluster_words[job.count_word];
    if ((count >> STAMP_SHIFT) != push_constants.stamp || (count & COMPACTED) == 0u) {
        return job;
    }
    if (late_count == 0u) {
        job.kept_early = count & COUNT_MASK;
        if (job.kept_early < triangles) {
            job.action = ACTION_RETEST;
            job.clip_from_local =
                view.clip_from_world * maths::affine3_to_square(input.world_from_local);
        }
    } else {
        // Instances of it in both passes (not a leaf of the game's): draw them all again, whole.
        job.action = ACTION_COPY;
        job.instances = early_count + late_count;
    }
    return job;
}

// Whether the draw's one instance has the transform and tag of the mesh's input, which the
// boxes and the count word are found by: a batch's first input may not be the one drawn.
fn drawn_as_given(base: u32, input: MeshInput) -> bool {
    if (base >= arrayLength(&meshes)) {
        return false;
    }
    let drawn = meshes[base].world_from_local;
    let given = input.world_from_local;
    return all(drawn[0] == given[0]) && all(drawn[1] == given[1]) && all(drawn[2] == given[2]) &&
        meshes[base].tag == input.tag;
}

// `plan`, with the slot's reads made together, then its input's and its instance's together:
// the instance is read before it's known to be wanted, which saves a wait. `late` still waits
// a third time for a leaf the early pass tested, for its count word.
fn plan_fast(slot: u32, late: bool) -> Job {
    var job = Job();
    if (slot >= push_constants.slot_count) {
        return job;
    }
    let early_count = indirect_parameters_gpu_metadata[slot].early_instance_count;
    let late_count = indirect_parameters_gpu_metadata[slot].late_instance_count;
    let mesh_index = indirect_parameters_gpu_metadata[slot].mesh_index;
    let parameters = indirect_parameters[slot];
    let base = indirect_parameters_cpu_metadata[slot].base_output_index;
    if (early_count == 0u && (!late || late_count == 0u)) {
        return job;
    }
    if (mesh_index >= arrayLength(&current_input)) {
        return job;
    }
    let input = current_input[mesh_index];
    // A place in `meshes` whether or not `base` is one, so the read needn't wait for the test.
    let base_in_meshes = base < arrayLength(&meshes);
    let instance = select(0u, base, base_in_meshes);
    let drawn = meshes[instance].world_from_local;
    let drawn_tag = meshes[instance].tag;
    if ((input.tag & TAG_LEAF) == 0u || input.pad != push_constants.slabs) {
        return job;
    }
    let end = parameters.first_index + parameters.index_count;
    if (end < parameters.first_index || end > push_constants.index_words) {
        indirect_parameters[slot].index_count = 0u;
        return job;
    }
    let triangles = parameters.index_count / 3u;
    let clusters = (triangles + CLUSTER_TRIANGLES - 1u) / CLUSTER_TRIANGLES;
    let mark = input.tag & TAG_MARK_BASE;
    let cluster_count = arrayLength(&cluster_words) / 2u;
    let first_cluster = mark / CLUSTER_TRIANGLES;
    let boxed = triangles != 0u && triangles <= COUNT_MASK && mark % CLUSTER_TRIANGLES == 0u &&
        first_cluster + clusters <= min(cluster_count, arrayLength(&boxes));

    job.slot = slot;
    job.first_index = parameters.first_index;
    job.triangle_count = triangles;
    job.first_cluster = first_cluster;
    job.count_word = select(NO_WORD, cluster_count + first_cluster, boxed);
    job.base = base;

    if (!late || early_count == 0u) {
        let instances = select(early_count, late_count, late);
        let given = input.world_from_local;
        let as_given = base_in_meshes && all(drawn[0] == given[0]) &&
            all(drawn[1] == given[1]) && all(drawn[2] == given[2]) && drawn_tag == input.tag;
        if (boxed && instances == 1u && as_given) {
            job.action = ACTION_TEST;
            job.record = select(1u, 0u, late);
            var clip_from_world = view.clip_from_world;
            if (!late) {
                clip_from_world = previous_view.clip_from_world;
            }
            job.clip_from_local =
                clip_from_world * maths::affine3_to_square(input.world_from_local);
        } else {
            job.action = ACTION_COPY;
        }
        return job;
    }

    if (!boxed) {
        return job;
    }
    let count = cluster_words[job.count_word];
    if ((count >> STAMP_SHIFT) != push_constants.stamp || (count & COMPACTED) == 0u) {
        return job;
    }
    if (late_count == 0u) {
        job.kept_early = count & COUNT_MASK;
        if (job.kept_early < triangles) {
            job.action = ACTION_RETEST;
            job.clip_from_local =
                view.clip_from_world * maths::affine3_to_square(input.world_from_local);
        }
    } else {
        job.action = ACTION_COPY;
        job.instances = early_count + late_count;
    }
    return job;
}

// Whether the box might give a fragment that passes the depth test: `test_triangle`'s tests in
// `triangle_cull_v2.wgsl`, but for the box's eight corners, and without the facing and pixel
// centre tests, which don't apply to a box. `fast` tests the pyramid with `occluded_fast`, given
// its first level's size and its levels.
fn visible(
    low: vec3<f32>,
    high: vec3<f32>,
    clip_from_local: mat4x4<f32>,
    fast: bool,
    size: vec2<u32>,
    levels: u32,
) -> bool {
    // The least, over the corners, of x - w, -x - w, y - w and -y - w: positive if all are
    // past one side.
    var past = vec4(3.4e38);
    var least_z_beyond = 3.4e38;
    var most_z_beyond = -3.4e38;
    var most_z = -3.4e38;
    var least_w = 3.4e38;
    var ndc_min = vec2(3.4e38);
    var ndc_max = vec2(-3.4e38);
    var nearest = -3.4e38;
    for (var corner = 0u; corner < 8u; corner += 1u) {
        let at = select(low, high, (vec3(corner) & vec3(1u, 2u, 4u)) != vec3(0u));
        let c = clip_from_local * vec4(at, 1.0);
        past = min(past, vec4(c.x - c.w, -c.x - c.w, c.y - c.w, -c.y - c.w));
        least_z_beyond = min(least_z_beyond, c.z - c.w);
        most_z_beyond = max(most_z_beyond, c.z - c.w);
        most_z = max(most_z, c.z);
        least_w = min(least_w, c.w);
        // Meaningless where w <= 0, but then the box is kept before these are used.
        let n = c.xyz / c.w;
        ndc_min = min(ndc_min, n.xy);
        ndc_max = max(ndc_max, n.xy);
        nearest = max(nearest, n.z);
    }
    if (any(past > vec4(0.0)) || least_z_beyond > 0.0 || most_z < 0.0) {
        return false;
    }
    if (most_z_beyond > 0.0 || least_w <= 0.0 || push_constants.span == 0u) {
        return true;
    }
    let aabb = vec4(
        ndc_min.x * 0.5 + 0.5,
        0.5 - ndc_max.y * 0.5,
        ndc_max.x * 0.5 + 0.5,
        0.5 - ndc_min.y * 0.5
    );
    let depth = nearest * 1.0001 + 1e-7;
    if (fast) {
        let pixels = (aabb.zw - aabb.xy) * vec2<f32>(size);
        return !occluded_fast(aabb, pixels, depth, f32(push_constants.span), size, levels);
    }
    let pixels = (aabb.zw - aabb.xy) * vec2<f32>(textureDimensions(depth_pyramid, 0));
    return !occlusion_culling::occluded_finely_batched(
        aabb, pixels, depth, f32(push_constants.span), depth_pyramid);
}

// `occluded_finely_batched`'s answer in fewer waits. The pyramid's first size and its levels are
// given, not asked for: a level's size is the first's halved for each level down, but never
// less than 1, as Vulkan has it. And the texels are read 24 at a time, three rows of eight: a
// read past the rectangle repeats its last row or column, which changes no minimum.
fn occluded_fast(
    aabb: vec4<f32>,
    aabb_pixel_size: vec2<f32>,
    depth: f32,
    span: f32,
    first_size: vec2<u32>,
    levels: u32,
) -> bool {
    let top = f32(levels - 1u);
    let level = i32(clamp(ceil(log2(max(aabb_pixel_size.x, aabb_pixel_size.y) / span)), 0.0, top));
    let size = vec2<i32>(max(first_size >> vec2(u32(level)), vec2(1u)));
    let lo = clamp(
        vec2<i32>(clamp(aabb.xy, vec2(0.0), vec2(1.0)) * vec2<f32>(size)),
        vec2(0),
        size - 1
    );
    let hi = min(
        vec2<i32>(clamp(aabb.zw, vec2(0.0), vec2(1.0)) * vec2<f32>(size)),
        min(size - 1, lo + i32(span) + 1)
    );
    if (any(hi < lo)) {
        return false;
    }
    for (var y = lo.y; y <= hi.y; y += 3) {
        let y1 = min(y + 1, hi.y);
        let y2 = min(y + 2, hi.y);
        for (var x = lo.x; x <= hi.x; x += 8) {
            // No compare between the 24 reads, so none waits for another.
            let farthest = min(
                farthest_of_eight(x, y, hi.x, level),
                min(farthest_of_eight(x, y1, hi.x, level), farthest_of_eight(x, y2, hi.x, level))
            );
            if (!(depth < farthest)) {
                return false;
            }
        }
    }
    return true;
}

// The farthest of the eight texels of row `y` from `x`, repeating texel `last` past it.
fn farthest_of_eight(x: i32, y: i32, last: i32, level: i32) -> f32 {
    let d0 = textureLoad(depth_pyramid, vec2(x, y), level).x;
    let d1 = textureLoad(depth_pyramid, vec2(min(x + 1, last), y), level).x;
    let d2 = textureLoad(depth_pyramid, vec2(min(x + 2, last), y), level).x;
    let d3 = textureLoad(depth_pyramid, vec2(min(x + 3, last), y), level).x;
    let d4 = textureLoad(depth_pyramid, vec2(min(x + 4, last), y), level).x;
    let d5 = textureLoad(depth_pyramid, vec2(min(x + 5, last), y), level).x;
    let d6 = textureLoad(depth_pyramid, vec2(min(x + 6, last), y), level).x;
    let d7 = textureLoad(depth_pyramid, vec2(min(x + 7, last), y), level).x;
    return min(min(min(d0, d1), min(d2, d3)), min(min(d4, d5), min(d6, d7)));
}

// Tests the job's clusters 32 a round, one a thread, and writes the survivors' triangles to the
// mirror in their order. Returns the rotation of `round_masks` for the next job.
fn cull_clusters(job: Job, lane: u32, first_rotation: u32) -> u32 {
    let retest = job.action == ACTION_RETEST;
    let clusters = (job.triangle_count + CLUSTER_TRIANGLES - 1u) / CLUSTER_TRIANGLES;
    var rotation = first_rotation;
    var kept = job.kept_early;
    for (var first = 0u; first < clusters; first += 32u) {
        let cluster = first + lane;
        var keep = false;
        if (cluster < clusters) {
            let at = job.first_cluster + cluster;
            if (!retest || cluster_words[at] == 0u) {
                let bounds = boxes[at];
                keep = visible(
                    bounds.low.xyz, bounds.high.xyz, job.clip_from_local, false, vec2(0u), 0u);
                lane_outcomes[OUTCOME_TESTED] += 1u;
                if (keep) {
                    lane_outcomes[OUTCOME_KEPT] += 1u;
                }
            }
            if (job.record != 0u) {
                cluster_words[at] = select(0u, 1u, keep);
            }
        }
        if (keep) {
            atomicOr(&round_masks[rotation], 1u << lane);
        }
        workgroupBarrier();

        // Each survivor's triangles, one a thread, after those of the survivors before it.
        let survivors = atomicLoad(&round_masks[rotation]);
        var left = survivors;
        while (left != 0u) {
            let k = firstTrailingBit(left);
            left &= left - 1u;
            let first_triangle = (first + k) * CLUSTER_TRIANGLES;
            let count = min(CLUSTER_TRIANGLES, job.triangle_count - first_triangle);
            if (lane < count) {
                let source = job.first_index + 3u * (first_triangle + lane);
                let place = job.first_index + 3u * (kept + lane);
                let triangle = vec3(indices[source], indices[source + 1u], indices[source + 2u]);
                mirror[place] = triangle.x;
                mirror[place + 1u] = triangle.y;
                mirror[place + 2u] = triangle.z;
            }
            kept += count;
        }
        // Clears the previous round's word: everyone read it before this round's barrier, and
        // the round after next, the first to fill it again, does so only after the next round's.
        if (lane == 0u) {
            atomicStore(&round_masks[(rotation + 2u) % 3u], 0u);
        }
        rotation = (rotation + 1u) % 3u;
    }
    finish_leaf(job, lane, kept);
    return rotation;
}

// Sets the leaf's draw to the triangles kept, and its count word, once its clusters are done.
fn finish_leaf(job: Job, lane: u32, kept: u32) {
    if (lane == 0u) {
        let retest = job.action == ACTION_RETEST;
        let count_word = kept | COMPACTED | (push_constants.stamp << STAMP_SHIFT);
        if (retest) {
            let added = kept - job.kept_early;
            if (added != 0u) {
                // The late build left this slot no instances. Its one early instance draws the
                // added triangles, whose marks start after those the early draw set.
                indirect_parameters[job.slot].instance_count = 1u;
                indirect_parameters[job.slot].first_instance = job.base;
                indirect_parameters[job.slot].first_index = job.first_index + 3u * job.kept_early;
                indirect_parameters[job.slot].index_count = 3u * added;
                meshes[job.base].tag += job.kept_early;
            }
            lane_outcomes[OUTCOME_DRAWN] += added;
        } else {
            indirect_parameters[job.slot].index_count = 3u * kept;
            lane_outcomes[OUTCOME_LEAF_TRIANGLES] += job.triangle_count;
            lane_outcomes[OUTCOME_DRAWN] += kept;
            lane_outcomes[OUTCOME_LEAVES] += 1u;
        }
        cluster_words[job.count_word] = count_word;
    }
}

// `cull_clusters`, waiting on memory less: a cluster's box is read with its early outcome,
// whether or not it's wanted, and the survivors' triangles are copied four clusters at once, all
// twelve reads made before any write.
fn cull_clusters_fast(
    job: Job,
    lane: u32,
    first_rotation: u32,
    size: vec2<u32>,
    levels: u32,
) -> u32 {
    let retest = job.action == ACTION_RETEST;
    let clusters = (job.triangle_count + CLUSTER_TRIANGLES - 1u) / CLUSTER_TRIANGLES;
    var rotation = first_rotation;
    var kept = job.kept_early;
    for (var first = 0u; first < clusters; first += 32u) {
        let cluster = first + lane;
        var keep = false;
        if (cluster < clusters) {
            let at = job.first_cluster + cluster;
            let early_outcome = cluster_words[at];
            let bounds = boxes[at];
            if (!retest || early_outcome == 0u) {
                keep = visible(
                    bounds.low.xyz, bounds.high.xyz, job.clip_from_local, true, size, levels);
                lane_outcomes[OUTCOME_TESTED] += 1u;
                if (keep) {
                    lane_outcomes[OUTCOME_KEPT] += 1u;
                }
            }
            if (job.record != 0u) {
                cluster_words[at] = select(0u, 1u, keep);
            }
        }
        if (keep) {
            atomicOr(&round_masks[rotation], 1u << lane);
        }
        workgroupBarrier();

        var left = atomicLoad(&round_masks[rotation]);
        while (left != 0u) {
            // The next four survivors, or as many as are left: `firstTrailingBit(0u)` is all
            // ones, and such a survivor has no triangles.
            let k0 = firstTrailingBit(left);
            left &= left - 1u;
            let k1 = firstTrailingBit(left);
            left &= left - 1u;
            let k2 = firstTrailingBit(left);
            left &= left - 1u;
            let k3 = firstTrailingBit(left);
            left &= left - 1u;
            let k = vec4(k0, k1, k2, k3);
            let first_triangle = (vec4(first) + k) * CLUSTER_TRIANGLES;
            let count = select(
                vec4(0u),
                min(vec4(CLUSTER_TRIANGLES), vec4(job.triangle_count) - first_triangle),
                k != vec4(0xffffffffu)
            );
            let mine = vec4(lane) < count;
            // A thread without a triangle of the survivor reads the leaf's first, and writes
            // nothing.
            let source = select(
                vec4(job.first_index),
                vec4(job.first_index) + 3u * (first_triangle + lane),
                mine
            );
            let a = vec3(indices[source.x], indices[source.x + 1u], indices[source.x + 2u]);
            let b = vec3(indices[source.y], indices[source.y + 1u], indices[source.y + 2u]);
            let c = vec3(indices[source.z], indices[source.z + 1u], indices[source.z + 2u]);
            let d = vec3(indices[source.w], indices[source.w + 1u], indices[source.w + 2u]);
            let before = kept + vec4(0u, count.x, count.x + count.y, count.x + count.y + count.z);
            let place = job.first_index + 3u * (before + lane);
            if (mine.x) {
                write_triangle(place.x, a);
            }
            if (mine.y) {
                write_triangle(place.y, b);
            }
            if (mine.z) {
                write_triangle(place.z, c);
            }
            if (mine.w) {
                write_triangle(place.w, d);
            }
            kept += count.x + count.y + count.z + count.w;
        }
        if (lane == 0u) {
            atomicStore(&round_masks[(rotation + 2u) % 3u], 0u);
        }
        rotation = (rotation + 1u) % 3u;
    }
    finish_leaf(job, lane, kept);
    return rotation;
}

fn write_triangle(place: u32, triangle: vec3<u32>) {
    mirror[place] = triangle.x;
    mirror[place + 1u] = triangle.y;
    mirror[place + 2u] = triangle.z;
}

// Copies the leaf's whole range into the mirror, for a draw left as Bevy built it.
fn copy_leaf(job: Job, lane: u32) {
    let words = 3u * job.triangle_count;
    for (var word = lane; word < words; word += 32u) {
        mirror[job.first_index + word] = indices[job.first_index + word];
    }
    if (lane == 0u) {
        if (job.instances != 0u) {
            indirect_parameters[job.slot].instance_count = job.instances;
            indirect_parameters[job.slot].first_instance = job.base;
        }
        if (job.count_word != NO_WORD) {
            cluster_words[job.count_word] =
                job.triangle_count | (push_constants.stamp << STAMP_SHIFT);
        }
        lane_outcomes[OUTCOME_COPIED] += 1u;
    }
}
