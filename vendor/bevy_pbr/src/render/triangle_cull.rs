//! skate: per-triangle culling for the world leaves the main opaque passes draw
//! (`triangle_cull.wgsl`, vendor/README.md).
//!
//! After the main pass's indirect parameters are built, a compute pass tests each triangle of
//! every tagged mesh drawn once in [`Opaque3d`] or [`AlphaMask3d`], writes the survivors, in
//! order, to a mirror of its index slab at the mesh's own offsets, and lowers its draw's
//! `index_count` to match. `DrawMesh` then binds the mirror for those draws. A mesh opts in with
//! [`TRIANGLE_CULL_LEAF`] in its `MeshTag`; the game tags its world leaves.
//!
//! The tagged meshes may fill several slabs (the game's world is larger than one): there is one
//! dispatch per pair of vertex and index slabs that holds any, and one mirror per index slab.
//! Each mesh's `MeshInputUniform::pad` carries its slab ids ([`slab_word`]), so a dispatch
//! touches only the draws from its own slabs, including those of meshes that arrive in them
//! after the last scan.
//!
//! The pass runs only while it can be right for every draw it touches:
//! - one view draws opaque or cutout meshes (the draws of all views share one buffer, and the
//!   tests use one view's transform and depth);
//! - every tagged mesh that is ready has 32-bit indices and 32-bit float positions at a
//!   4-byte-aligned offset, in slabs usable as storage buffers;
//! - under [`TRIANGLE_CULL_MARKS`], every pipeline of the view's depth prepass is ready, since a
//!   draw whose pipeline isn't is skipped and marks nothing.
//!
//! Otherwise nothing is dispatched, no index count is changed and no draw binds a mirror.
//!
//! Every two seconds or so, one frame's pass also counts its triangles by outcome, and the
//! counts are read back and logged (at `info`), so a log shows what each test removes.
//!
//! The pass has two versions with the same tests and outcomes. The second, the default
//! (`triangle_cull_v2.wgsl`), lists the leaf draws of each dispatch with one thread per draw
//! slot, then tests their triangles with one 64-thread workgroup per listed draw, through an
//! indirect dispatch. The first (`triangle_cull.wgsl`, [`TRIANGLE_CULL_V1`]) starts a 256-thread
//! workgroup for every draw slot, most of which find nothing to do.
//!
//! Under [`TRIANGLE_CULL_MARKS`] the second version runs none of the tests. The view's depth
//! prepass has marked each tagged triangle that got a fragment through it (the game's world
//! materials do the marking), and the pass keeps exactly the marked triangles and clears their
//! marks for the next frame. The marks are a word for each triangle in [`TriangleCullMarks`], from
//! the [`TRIANGLE_CULL_MARK_BASE`] bits of its mesh's tag. The kernel that keeps them,
//! `keep_marked`, hands each thread four neighbouring triangles, and issues all its loads before
//! its stores; [`TRIANGLE_CULL_MARKS_IN_CULL`] keeps them with `cull` instead, as before,
//! [`TRIANGLE_CULL_MARKS_COALESCED`] with `keep_marked_coalesced`, whose neighbouring threads
//! read and write neighbouring words, and [`TRIANGLE_CULL_MARKS_BALLOT`] with
//! `keep_marked_ballot`, which places each survivor with a subgroup ballot.
//!
//! Under [`TRIANGLE_CULL_CLUSTERS`], with the marks, the depth prepass itself draws only part of
//! each leaf (`triangle_cull_clusters.wgsl`). The game cuts each leaf into clusters of
//! [`TRIANGLE_CULL_CLUSTER_TRIANGLES`] and puts a box around each in [`TriangleCullClusters`].
//! Before the early prepass, a pass tests the boxes against the previous frame's view and depth
//! pyramid, as Bevy's early pass tests meshes, and writes the survivors' triangles, in order, to
//! the prepass's own mirror of the index slab, which its draws of the leaves read. Before the
//! late prepass, it tests the clusters the early one left out against this frame's view and the
//! pyramid the early prepass built, and the late prepass draws the survivors after the others.
//! The main pass then keeps the marked triangles among those the prepass's mirror holds.

use core::any::TypeId;
use core::fmt::Write as _;
use core::num::NonZeroU64;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use bevy_app::{App, SubApp};
use bevy_asset::{embedded_asset, load_embedded_asset, AssetId, Handle};
use bevy_core_pipeline::{
    core_3d::{
        graph::{Core3d, Node3d},
        AlphaMask3d, Opaque3d,
    },
    experimental::mip_generation::ViewDepthPyramid,
    prepass::{
        AlphaMask3dPrepass, DepthPrepass, Opaque3dPrepass, OpaqueNoLightmap3dBatchSetKey,
        PreviousViewData, PreviousViewUniformOffset, PreviousViewUniforms,
    },
};
use bevy_ecs::{
    entity::Entity,
    query::Has,
    resource::Resource,
    schedule::IntoScheduleConfigs as _,
    system::{Query, Res, ResMut},
    world::{FromWorld, World},
};
use bevy_log::info;
use bevy_mesh::Mesh;
use bevy_platform::{sync::Arc, time::Instant};
use bevy_render::{
    batching::gpu_preprocessing::{
        BatchedInstanceBuffers, IndirectParametersBuffers, IndirectParametersCpuMetadata,
        IndirectParametersGpuMetadata, IndirectParametersIndexed,
    },
    diagnostic::RecordDiagnostics,
    experimental::occlusion_culling::OcclusionCulling,
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    mesh::{
        allocator::{MeshAllocator, SlabId},
        RenderMesh, RenderMeshBufferInfo,
    },
    render_asset::RenderAssets,
    render_graph::{Node, NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel},
    render_phase::{BinnedPhaseItem, ViewBinnedRenderPhases},
    render_resource::{
        binding_types::{
            storage_buffer, storage_buffer_read_only, storage_buffer_read_only_sized,
            storage_buffer_sized, texture_2d, uniform_buffer,
        },
        BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
        BufferBinding, BufferDescriptor, BufferUsages, CachedComputePipelineId,
        ComputePassDescriptor, ComputePipelineDescriptor, Extent3d, IndexFormat, MapMode,
        PipelineCache, PushConstantRange, ShaderStages, TextureDescriptor, TextureDimension,
        TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
        VertexFormat,
    },
    renderer::{RenderContext, RenderDevice, RenderQueue},
    settings::WgpuFeatures,
    storage::{GpuShaderStorageBuffer, ShaderStorageBuffer},
    view::{ExtractedView, Msaa, RetainedViewEntity, ViewUniform, ViewUniformOffset, ViewUniforms},
    Render, RenderSystems,
};
use bevy_shader::Shader;
use bevy_utils::default;

use crate::{graph::NodePbr, MeshInputUniform, MeshUniform, RenderMeshInstances};

/// 0: off. Otherwise on: the low 8 bits are the depth pyramid test's span in texels
/// (`occluded_finely`), and [`TRIANGLE_CULL_NO_DEPTH`] leaves that test out. Read every frame.
pub static TRIANGLE_CULL: AtomicU32 = AtomicU32::new(0);

/// In [`TRIANGLE_CULL`]: on, without the depth pyramid test.
pub const TRIANGLE_CULL_NO_DEPTH: u32 = 1 << 8;

/// In [`TRIANGLE_CULL`], for narrowing down a fault: make the mirrors, but dispatch nothing.
pub const TRIANGLE_CULL_DEBUG_MIRRORS_ONLY: u32 = 1 << 9;
/// In [`TRIANGLE_CULL`], for narrowing down a fault: dispatch, but have every workgroup return
/// at once, so the draws keep their counts.
pub const TRIANGLE_CULL_DEBUG_EMPTY: u32 = 1 << 10;
/// In [`TRIANGLE_CULL`], for narrowing down a fault: draw from the index slabs, not the mirrors.
pub const TRIANGLE_CULL_DEBUG_NO_SWAP: u32 = 1 << 11;
/// In [`TRIANGLE_CULL`], for narrowing down a fault: never count the triangles or read the counts
/// back.
pub const TRIANGLE_CULL_DEBUG_NO_STATS: u32 = 1 << 12;
/// In [`TRIANGLE_CULL`], for narrowing down a fault: clear the mirrors every frame, before any
/// dispatch.
pub const TRIANGLE_CULL_DEBUG_CLEAR: u32 = 1 << 13;
/// In [`TRIANGLE_CULL`], for narrowing down a fault, with
/// [`TRIANGLE_CULL_DEBUG_MIRRORS_ONLY`]: still clear, copy and read back the counts, though
/// nothing counts them.
pub const TRIANGLE_CULL_DEBUG_STATS_ONLY: u32 = 1 << 14;
const TRIANGLE_CULL_DEBUG: u32 = 0x3f << 9;

/// In [`TRIANGLE_CULL`]: run the pass's first version (`triangle_cull.wgsl`) rather than its
/// second (`triangle_cull_v2.wgsl`), to compare them. The tests and outcomes are the same.
pub const TRIANGLE_CULL_V1: u32 = 1 << 15;

/// In [`TRIANGLE_CULL`]: instead of testing the triangles, keep those the view's depth prepass
/// marked in [`TriangleCullMarks`] this frame, and clear their marks. The game's world materials
/// mark an opaque triangle wherever a fragment of it passes the prepass's depth test, and a
/// cutout one wherever it gets a fragment at all, so every triangle that could pass the main
/// pass's depth test is kept. Second version only. Nothing is dispatched until the marks' buffer
/// is uploaded.
pub const TRIANGLE_CULL_MARKS: u32 = 1 << 16;

/// In [`TRIANGLE_CULL`], with [`TRIANGLE_CULL_MARKS`]: only [`Opaque3d`]'s draws. The
/// [`AlphaMask3d`] draws keep all their triangles.
pub const TRIANGLE_CULL_MARKS_OPAQUE_ONLY: u32 = 1 << 17;

/// In [`TRIANGLE_CULL`], with [`TRIANGLE_CULL_MARKS`], for debugging: keep the triangles the
/// prepass left unmarked instead, and leave out the marked ones, whose marks are still cleared.
/// A leaf's pixels that the main pass then draws are those whose triangle no fragment marked,
/// which the marks would have lost.
pub const TRIANGLE_CULL_MARKS_INVERT: u32 = 1 << 18;

/// In [`TRIANGLE_CULL`], with [`TRIANGLE_CULL_MARKS`]: keep the marked triangles with `cull`,
/// the kernel that tests them otherwise, rather than with `keep_marked`, to compare them. They
/// keep the same triangles, in the same order.
pub const TRIANGLE_CULL_MARKS_IN_CULL: u32 = 1 << 19;

/// In [`TRIANGLE_CULL`], with [`TRIANGLE_CULL_MARKS`] and not [`TRIANGLE_CULL_MARKS_IN_CULL`]:
/// keep the marked triangles with `keep_marked_coalesced`, whose neighbouring threads read and
/// write neighbouring words, rather than `keep_marked`. The same triangles, in the same order.
pub const TRIANGLE_CULL_MARKS_COALESCED: u32 = 1 << 20;

/// In [`TRIANGLE_CULL`], with [`TRIANGLE_CULL_MARKS`] and not [`TRIANGLE_CULL_MARKS_IN_CULL`]:
/// keep the marked triangles with `keep_marked_ballot`, a 32-thread workgroup for each draw that
/// places each survivor with a subgroup ballot, rather than `keep_marked`, and before
/// [`TRIANGLE_CULL_MARKS_COALESCED`]. The same triangles, in the same order. It needs subgroups
/// of at least 32 invocations ([`WgpuFeatures::SUBGROUP`] and `min_subgroup_size`); without
/// them the other flags choose the kernel, as if this were off.
pub const TRIANGLE_CULL_MARKS_BALLOT: u32 = 1 << 21;

/// In [`TRIANGLE_CULL`], with [`TRIANGLE_CULL_MARKS_BALLOT`]: `keep_marked_ballot` with an
/// 8-thread workgroup and rounds of 32 triangles, only to test it where subgroups have 8
/// invocations, as in Ryujinx on llvmpipe, which hands a warp's lanes and ballots to the host's
/// subgroups. It needs subgroups of at least 8.
pub const TRIANGLE_CULL_MARKS_BALLOT_8: u32 = 1 << 22;

/// In a `MeshTag`: the mesh's triangles may be culled. Its draw must use the triangles in their
/// winding as given, with no vertex shader that moves them off the mesh's positions.
pub const TRIANGLE_CULL_LEAF: u32 = 1 << 31;

/// In a `MeshTag` with [`TRIANGLE_CULL_LEAF`]: the material draws both faces.
pub const TRIANGLE_CULL_TWO_SIDED: u32 = 1 << 30;

/// In a `MeshTag` with [`TRIANGLE_CULL_LEAF`]: where the mesh's marks start in
/// [`TriangleCullMarks`], one word for each triangle, in the order its indices draw them.
pub const TRIANGLE_CULL_MARK_BASE: u32 = (1 << 29) - 1;

/// The marks for [`TRIANGLE_CULL_MARKS`]: a storage buffer holding one `u32` for each tagged
/// triangle, at its mesh's [`TRIANGLE_CULL_MARK_BASE`] plus the triangle's index. The prepass
/// sets a triangle's word non-zero, and the pass reads it and sets it back to 0. The game inserts
/// this resource into the main world with the buffer its materials bind. It holds the id rather
/// than a handle, so it keeps no map's buffer alive.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, ExtractResource)]
pub struct TriangleCullMarks(pub AssetId<ShaderStorageBuffer>);

/// 0: off. Otherwise, with [`TRIANGLE_CULL_MARKS`] in the second version, the view's depth
/// prepass draws only the clusters of each tagged leaf ([`TriangleCullClusters`]) whose box could
/// be seen, and the low 8 bits are the depth pyramid test's span in texels for the boxes, 1 to
/// 32. Read every frame. It needs the view's occlusion culling, as the boxes are tested as Bevy
/// tests meshes: against the previous frame's view and pyramid before the early prepass, and
/// those left out against this frame's view and the early prepass's pyramid before the late one.
/// Nothing is dispatched until the boxes' buffer is uploaded. [`TRIANGLE_CULL_CLUSTERS_CUTOUTS`]
/// adds the cutouts under [`TRIANGLE_CULL_MARKS_OPAQUE_ONLY`], and
/// [`TRIANGLE_CULL_CLUSTERS_KERNEL_SHIFT`] picks the kernel.
pub static TRIANGLE_CULL_CLUSTERS: AtomicU32 = AtomicU32::new(0);

/// The triangles in a cluster of [`TRIANGLE_CULL_CLUSTERS`]: a leaf's first 32, in the order its
/// indices draw them, then its next 32, and so on, the last perhaps fewer.
pub const TRIANGLE_CULL_CLUSTER_TRIANGLES: u32 = 32;

/// In [`TRIANGLE_CULL_CLUSTERS`], under [`TRIANGLE_CULL_MARKS_OPAQUE_ONLY`]: the cutouts too. The
/// view's [`AlphaMask3dPrepass`] draws only the clusters of each tagged leaf whose box could be
/// seen, as its [`Opaque3dPrepass`] does, and the main pass's [`AlphaMask3d`] draws the same
/// clusters, from the prepass's mirror, rather than the whole leaf. Nothing marks them, so `list`
/// only sets each such draw's count from its leaf's count word. A cluster left out lies behind
/// the depth the early prepass drew, and the main pass's depth test passes only fragments at or
/// nearer than the depth the prepass ends with, so it would have drawn no pixel there either.
pub const TRIANGLE_CULL_CLUSTERS_CUTOUTS: u32 = 1 << 8;

/// In [`TRIANGLE_CULL_CLUSTERS`], bits 16 to 23 pick the kernel that tests the boxes: 0 the
/// first, `early` and `late`, and 1 or more the second, `early_fast` and `late_fast`, which
/// keeps the same clusters but waits on memory less often (triangle_cull_clusters.wgsl). Each is
/// compiled on its first use.
pub const TRIANGLE_CULL_CLUSTERS_KERNEL_SHIFT: u32 = 16;

/// The boxes for [`TRIANGLE_CULL_CLUSTERS`]: a storage buffer holding 32 bytes for each cluster
/// of each tagged leaf, the box around its vertices in the leaf's local space, as its least
/// corner and its greatest, each four `f32`s of which the fourth is unused. A leaf's first box is
/// at its [`TRIANGLE_CULL_MARK_BASE`] over [`TRIANGLE_CULL_CLUSTER_TRIANGLES`], so the game starts
/// each leaf's marks at a multiple of that; a leaf whose boxes don't fit is drawn whole. The game
/// inserts this resource into the main world, holding the id as [`TriangleCullMarks`] does.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, ExtractResource)]
pub struct TriangleCullClusters(pub AssetId<ShaderStorageBuffer>);

const PIXEL_CENTRES: u32 = 1;
const COUNT: u32 = 2;
const EMPTY: u32 = 4;
const MARKS: u32 = 8;
const MARKS_INVERT: u32 = 16;
/// In the second version's flags: the main pass keeps the marked triangles among those the
/// prepass's mirror holds for each leaf, as the leaf's count word says, and the top 8 bits are
/// this frame's stamp.
const CLUSTERS: u32 = 32;
/// With [`CLUSTERS`]: the draws read the prepass's mirror as it is
/// ([`TRIANGLE_CULL_CLUSTERS_CUTOUTS`]), so `list` sets each one's count from its leaf's count
/// word and lists no item.
const DIRECT: u32 = 64;
/// In a cluster dispatch's flags: its draws are the cutouts', whose counts go to the third eight
/// words of `stats`.
const CLUSTER_CUTOUTS: u32 = 4;
const STAMP_SHIFT: u32 = 24;
const WORKGROUPS_PER_ROW: u32 = 65535;
const PUSH_CONSTANT_BYTES: u32 = 7 * 4;
const PUSH_CONSTANT_BYTES_V2: u32 = 9 * 4;
const PUSH_CONSTANT_BYTES_CLUSTERS: u32 = 6 * 4;
/// The draw slots each workgroup of `triangle_cull_clusters.wgsl` plans, and a box's size.
const CLUSTER_SLOTS_PER_WORKGROUP: u32 = 8;
const CLUSTER_BOX_BYTES: u64 = 32;
/// `triangle_cull_v2.wgsl`'s `list` workgroup size, and the size of its `Item` (a matrix and six
/// words, padded to the matrix's 16-byte alignment).
const LIST_WORKGROUP_SIZE: u32 = 64;
const ITEM_BYTES: u64 = 96;
/// The second version's indirect arguments for one dispatch of `cull` in `counts`: x, y and z,
/// and a spare word. Prepare sets them to one workgroup every frame, for the empty item that
/// starts the dispatch's share of the items; `list` adds one for each item it appends.
const COUNT_RESET: [u32; 4] = [1, 1, 1, 0];
const COUNT_BYTES: u64 = 16;
/// The widest indirect dispatch that every device must take.
const MAX_WORKGROUPS: u32 = 65535;
const RESCAN_FRAMES: u32 = 120;
/// While some tagged meshes aren't uploaded. Those that arrive in a pair of slabs the pass
/// already covers are culled at once; only a new pair waits for a scan.
const PENDING_RESCAN_FRAMES: u32 = 15;
/// In a [`slab_word`]: no slab, or one whose id doesn't fit in 16 bits.
const NO_SLAB: u32 = 0xffff;
/// The phases a frame's dispatches cover ([`TriangleCull::phases`]).
const PHASE_OPAQUE: u8 = 1;
const PHASE_ALPHA_MASK: u8 = 2;
/// In [`TriangleCull::clusters_state`]: the cluster cull's passes that recorded this frame's
/// dispatches.
const CLUSTERS_EARLY: u32 = 1;
const CLUSTERS_LATE: u32 = 2;

/// The counters of `stats`: the triangle pass's 8, then `triangle_cull_clusters.wgsl`'s 8 for the
/// opaque draws and 8 for the cutouts'.
const STATS_BYTES: u64 = 24 * 4;
const STATS_REPORT_SECONDS: f32 = 2.0;
/// Where the counts are: nowhere, wanted from this frame's pass, copied for the readback by it,
/// or being mapped.
const STATS_IDLE: u32 = 0;
const STATS_WANTED: u32 = 1;
const STATS_COPIED: u32 = 2;
const STATS_MAPPING: u32 = 3;
/// The map's outcome, from its callback.
const MAP_PENDING: u32 = 0;
const MAP_DONE: u32 = 1;
const MAP_FAILED: u32 = 2;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct TriangleCullLabel;

/// The cluster cull's nodes ([`TRIANGLE_CULL_CLUSTERS`]): before the early prepass, and before
/// the late one.
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub enum TriangleCullClusterLabel {
    Early,
    Late,
}

/// The word a mesh's `MeshInputUniform::pad` carries for the pass: its vertex slab's id in the
/// low 16 bits and its index slab's in the high, each [`NO_SLAB`] if none (or too large).
#[inline]
pub(crate) fn slab_word((vertex, index): (Option<SlabId>, Option<SlabId>)) -> u32 {
    let id = |slab: Option<SlabId>| slab.map_or(NO_SLAB, |slab| slab.0.get().min(NO_SLAB));
    id(vertex) | (id(index) << 16)
}

/// A pair of vertex and index slabs that holds ready tagged meshes, as last scanned.
struct Group {
    /// The pair's [`slab_word`].
    slabs: u32,
    vertex: Buffer,
    index: Buffer,
    stride_words: u32,
    position_words: u32,
    /// The tagged instances that were ready in these slabs.
    instances: usize,
    /// A tagged mesh in these slabs whose buffers are checked every frame, so a slab that grows
    /// (and moves) is noticed before a mirror is used.
    representative: AssetId<Mesh>,
    /// The index slab's mirror, set by this frame's prepare.
    mirror: Option<Buffer>,
    /// The index slab's mirror for the depth prepass, set by this frame's prepare under
    /// [`TRIANGLE_CULL_CLUSTERS`].
    prepass_mirror: Option<Buffer>,
}

/// A copy of an index slab, which the pass fills with the survivors of the draws from it.
struct Mirror {
    index_slab: u32,
    buffer: Buffer,
}

struct Dispatch {
    /// The first version's bind group, or the second's for `list`.
    bind_group: BindGroup,
    /// The second version's for `cull`.
    cull_bind_group: Option<BindGroup>,
    /// Seven words for the first version, nine for the second.
    push_constants: [u32; 9],
    /// The first version's workgroups, or the second's for `list`.
    workgroups: (u32, u32),
}

/// One of the cluster cull's dispatches, for a prepass phase's draws from a pair of slabs. Its
/// `early` and `late` both make it, with the same bindings.
struct ClusterDispatch {
    bind_group: BindGroup,
    /// The draw slots, the index slab's size in indices, the slab word, the span, the flags and
    /// the stamp.
    push_constants: [u32; 6],
    workgroups: u32,
}

/// In [`TriangleCull::version_logged`]: `keep_marked_ballot` was asked for, and the device's
/// subgroups can't run it.
const VERSION_NO_BALLOT: u8 = 0x10;

#[derive(Resource)]
pub struct TriangleCull {
    layout: BindGroupLayoutDescriptor,
    list_layout: BindGroupLayoutDescriptor,
    cull_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    shader_v2: Handle<Shader>,
    cluster_layout: BindGroupLayoutDescriptor,
    shader_clusters: Handle<Shader>,
    /// Each cluster cull kernel's `early` and `late`, queued when it's first used.
    pipelines_clusters: [Option<[CachedComputePipelineId; 2]>; 2],
    /// The kernel this frame's cluster cull uses ([`TRIANGLE_CULL_CLUSTERS_KERNEL_SHIFT`]).
    cluster_kernel: usize,
    /// Each version's pipelines, queued when it's first used: the first's, and the second's
    /// `list`, `cull` and `keep_marked`.
    pipeline: Option<CachedComputePipelineId>,
    pipelines_v2: Option<[CachedComputePipelineId; 3]>,
    /// `keep_marked_coalesced`'s, queued when it's first used.
    pipeline_coalesced: Option<CachedComputePipelineId>,
    /// `keep_marked_ballot`'s, queued when it's first used, and its 8-thread test's.
    pipeline_ballot: Option<CachedComputePipelineId>,
    pipeline_ballot_8: Option<CachedComputePipelineId>,
    /// The device's smallest subgroup, or 0 without [`WgpuFeatures::SUBGROUP`].
    min_subgroup_size: u32,
    /// Whether this frame's dispatches are the second version's.
    v2: bool,
    /// Whether they keep the marked triangles with `keep_marked` rather than `cull`.
    marks_kernel: bool,
    /// Whether that's `keep_marked_coalesced` ([`TRIANGLE_CULL_MARKS_COALESCED`]).
    coalesced: bool,
    /// Whether it's `keep_marked_ballot` ([`TRIANGLE_CULL_MARKS_BALLOT`]).
    ballot: bool,
    /// Whether that's its 8-thread test ([`TRIANGLE_CULL_MARKS_BALLOT_8`]).
    ballot_8: bool,
    /// The version last reported in the log: 0 for none yet, 3 for the second with
    /// `keep_marked`, 4 with `keep_marked_coalesced`, 5 with `keep_marked_ballot` and 6 with its
    /// 8-thread test, plus [`VERSION_NO_BALLOT`] when the ballot was asked for and the device
    /// can't run it.
    version_logged: u8,
    /// The second version's items and indirect arguments, grown as needed.
    items: Option<Buffer>,
    counts: Option<Buffer>,
    no_pyramid: TextureView,
    /// Bound in the marks' place when [`TRIANGLE_CULL_MARKS`] is off.
    no_marks: Buffer,
    groups: Vec<Group>,
    mirrors: Vec<Mirror>,
    /// The depth prepass's mirrors, one for each index slab, under [`TRIANGLE_CULL_CLUSTERS`].
    prepass_mirrors: Vec<Mirror>,
    /// A word for each box, 1 if the early pass kept its cluster, then a word for each box, of
    /// which a leaf's first is its count word: the triangles the prepass's mirror holds for it,
    /// whether those are its kept clusters' rather than all of its own, and the stamp of the frame
    /// that wrote it. Made the size the boxes need, zeroed.
    cluster_words: Option<Buffer>,
    /// This frame's stamp for the count words, 1 to 255.
    stamp: u32,
    cluster_dispatches: Vec<ClusterDispatch>,
    /// The phases (`PHASE_*`) whose prepass draws this frame's cluster dispatches cover, each for
    /// every group. The main pass's dispatches for them read the prepass's mirrors.
    cluster_phases: u8,
    /// The phases whose main pass draws read the prepass's mirrors as they are, with the counts
    /// this frame's `list` set ([`TRIANGLE_CULL_CLUSTERS_CUTOUTS`]), each for every group.
    direct_phases: u8,
    /// `CLUSTERS_*`, set by [`TriangleCullClusterNode`] as it records this frame's dispatches.
    clusters_state: AtomicU32,
    cluster_status: &'static str,
    instances_scanned: usize,
    frames_since_scan: u32,
    /// The tagged instances whose meshes weren't uploaded at the last scan.
    pending: usize,
    status: &'static str,
    view: Option<Entity>,
    view_offset: u32,
    dispatches: Vec<Dispatch>,
    /// The phases (`PHASE_*`) this frame's dispatches cover, each for every group.
    phases: u8,
    /// Set by [`TriangleCullNode`] once it has recorded this frame's dispatches.
    dispatched: AtomicBool,
    /// Whether the draws use the mirrors ([`TRIANGLE_CULL_DEBUG_NO_SWAP`] clears it).
    swap: bool,
    /// The `TRIANGLE_CULL_DEBUG_*` bits in force.
    debug: u32,
    stats: Buffer,
    stats_readback: Buffer,
    stats_state: AtomicU32,
    /// Whether the counts being read back came from [`TRIANGLE_CULL_MARKS`]' pass.
    stats_marks: bool,
    /// Whether that pass kept the unmarked triangles ([`TRIANGLE_CULL_MARKS_INVERT`]).
    stats_inverted: bool,
    /// Whether the cluster cull counted in that frame too, and whether it covered the cutouts.
    stats_clusters: bool,
    stats_cutouts: bool,
    map_state: Arc<AtomicU32>,
    last_report: Option<Instant>,
}

impl FromWorld for TriangleCull {
    fn from_world(world: &mut World) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "triangle cull bind group layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    uniform_buffer::<ViewUniform>(true),
                    storage_buffer_read_only::<MeshInputUniform>(false),
                    storage_buffer_read_only::<IndirectParametersGpuMetadata>(false),
                    storage_buffer::<IndirectParametersIndexed>(false),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_read_only_sized(false, None),
                    storage_buffer_sized(false, None),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    storage_buffer_sized(false, None),
                ),
            ),
        );
        let shader = load_embedded_asset!(world, "triangle_cull.wgsl");
        // The second version's two entry points bind different sets: `counts` is `cull`'s
        // indirect argument buffer, and the items are written by `list` and read by `cull`, and
        // wgpu takes no buffer as both storage written and something else in one dispatch.
        let list_layout = BindGroupLayoutDescriptor::new(
            "triangle cull list bind group layout",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::COMPUTE,
                (
                    (0, uniform_buffer::<ViewUniform>(true)),
                    (1, storage_buffer_read_only::<MeshInputUniform>(false)),
                    (
                        2,
                        storage_buffer_read_only::<IndirectParametersGpuMetadata>(false),
                    ),
                    (3, storage_buffer::<IndirectParametersIndexed>(false)),
                    (8, storage_buffer_sized(false, None)),
                    (9, storage_buffer_sized(false, None)),
                    (11, storage_buffer_sized(false, None)),
                    // The cluster cull's words, read under `CLUSTERS`.
                    (13, storage_buffer_read_only_sized(false, None)),
                ),
            ),
        );
        let cull_layout = BindGroupLayoutDescriptor::new(
            "triangle cull cull bind group layout",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::COMPUTE,
                (
                    (0, uniform_buffer::<ViewUniform>(true)),
                    (3, storage_buffer::<IndirectParametersIndexed>(false)),
                    (4, storage_buffer_read_only_sized(false, None)),
                    (5, storage_buffer_read_only_sized(false, None)),
                    (6, storage_buffer_sized(false, None)),
                    (7, texture_2d(TextureSampleType::Float { filterable: false })),
                    (8, storage_buffer_sized(false, None)),
                    (10, storage_buffer_read_only_sized(false, None)),
                    (12, storage_buffer_sized(false, None)),
                ),
            ),
        );
        let shader_v2 = load_embedded_asset!(world, "triangle_cull_v2.wgsl");
        // The cluster cull's, for both its entry points: the slab is read at 5 and its prepass
        // mirror written at 6, and 13 is the prepass phase's instances.
        let cluster_layout = BindGroupLayoutDescriptor::new(
            "triangle cull clusters bind group layout",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::COMPUTE,
                (
                    (0, uniform_buffer::<ViewUniform>(true)),
                    (1, storage_buffer_read_only::<MeshInputUniform>(false)),
                    (
                        2,
                        storage_buffer_read_only::<IndirectParametersGpuMetadata>(false),
                    ),
                    (3, storage_buffer::<IndirectParametersIndexed>(false)),
                    (
                        4,
                        storage_buffer_read_only::<IndirectParametersCpuMetadata>(false),
                    ),
                    (5, storage_buffer_read_only_sized(false, None)),
                    (6, storage_buffer_sized(false, None)),
                    (7, texture_2d(TextureSampleType::Float { filterable: false })),
                    (8, storage_buffer_sized(false, None)),
                    (9, storage_buffer_read_only_sized(false, None)),
                    (10, storage_buffer_sized(false, None)),
                    (12, uniform_buffer::<PreviousViewData>(false)),
                    (13, storage_buffer::<MeshUniform>(false)),
                ),
            ),
        );
        let shader_clusters = load_embedded_asset!(world, "triangle_cull_clusters.wgsl");
        let no_pyramid = world
            .resource::<RenderDevice>()
            .create_texture(&TextureDescriptor {
                label: Some("triangle cull: no depth pyramid"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::R32Float,
                usage: TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let render_device = world.resource::<RenderDevice>();
        let min_subgroup_size = if render_device.features().contains(WgpuFeatures::SUBGROUP) {
            render_device.limits().min_subgroup_size
        } else {
            0
        };
        let stats = render_device.create_buffer(&BufferDescriptor {
            label: Some("triangle cull stats"),
            size: STATS_BYTES,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let stats_readback = render_device.create_buffer(&BufferDescriptor {
            label: Some("triangle cull stats readback"),
            size: STATS_BYTES,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let no_marks = render_device.create_buffer(&BufferDescriptor {
            label: Some("triangle cull: no marks"),
            size: 16,
            usage: BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Self {
            layout,
            list_layout,
            cull_layout,
            shader,
            shader_v2,
            cluster_layout,
            shader_clusters,
            pipelines_clusters: [None; 2],
            cluster_kernel: 0,
            pipeline: None,
            pipelines_v2: None,
            v2: false,
            pipeline_coalesced: None,
            pipeline_ballot: None,
            pipeline_ballot_8: None,
            min_subgroup_size,
            marks_kernel: false,
            coalesced: false,
            ballot: false,
            ballot_8: false,
            version_logged: 0,
            items: None,
            counts: None,
            no_pyramid,
            no_marks,
            groups: Vec::new(),
            mirrors: Vec::new(),
            prepass_mirrors: Vec::new(),
            cluster_words: None,
            stamp: 0,
            cluster_dispatches: Vec::new(),
            cluster_phases: 0,
            direct_phases: 0,
            clusters_state: AtomicU32::new(0),
            cluster_status: "off",
            instances_scanned: usize::MAX,
            frames_since_scan: 0,
            pending: 0,
            status: "off",
            view: None,
            view_offset: 0,
            dispatches: Vec::new(),
            phases: 0,
            dispatched: AtomicBool::new(false),
            swap: true,
            debug: 0,
            stats,
            stats_readback,
            stats_state: AtomicU32::new(STATS_IDLE),
            stats_marks: false,
            stats_inverted: false,
            stats_clusters: false,
            stats_cutouts: false,
            map_state: Arc::new(AtomicU32::new(MAP_PENDING)),
            last_report: None,
        }
    }
}

impl TriangleCull {
    /// The buffer to draw a phase `P` item's indices from, in place of `index_buffer`, when its
    /// draws are indirect: its index slab's mirror, if this frame's pass covered `P` and the
    /// item's pair of slabs, and `tag` (looked up only then) marks the item's meshes. All meshes
    /// of a batch share the material, and the game tags every mesh of its world material, so the
    /// representative speaks for the batch. For a depth prepass phase, it's the prepass's mirror,
    /// if this frame's cluster cull covered the phase. Both its passes must have run: the draws
    /// are recorded after the graph's nodes, and the early and late prepass share them. So it is
    /// for a main pass phase drawn from the prepass's mirrors as they are
    /// ([`TRIANGLE_CULL_CLUSTERS_CUTOUTS`]), once this frame's pass has set its counts.
    #[inline]
    pub(crate) fn index_buffer<'a, P: 'static>(
        &'a self,
        vertex_buffer: &Buffer,
        index_buffer: &'a Buffer,
        tag: impl FnOnce() -> Option<u32>,
    ) -> &'a Buffer {
        let (phase, prepass) = if TypeId::of::<P>() == TypeId::of::<Opaque3d>() {
            (PHASE_OPAQUE, false)
        } else if TypeId::of::<P>() == TypeId::of::<AlphaMask3d>() {
            (PHASE_ALPHA_MASK, false)
        } else if TypeId::of::<P>() == TypeId::of::<Opaque3dPrepass>() {
            (PHASE_OPAQUE, true)
        } else if TypeId::of::<P>() == TypeId::of::<AlphaMask3dPrepass>() {
            (PHASE_ALPHA_MASK, true)
        } else {
            return index_buffer;
        };
        let from_prepass = if prepass {
            if self.cluster_phases & phase == 0
                || self.clusters_state.load(Ordering::Relaxed) != CLUSTERS_EARLY | CLUSTERS_LATE
            {
                return index_buffer;
            }
            true
        } else if (self.phases | self.direct_phases) & phase == 0
            || !self.swap
            || !self.dispatched.load(Ordering::Relaxed)
        {
            return index_buffer;
        } else {
            self.direct_phases & phase != 0
        };
        let mirror = self
            .groups
            .iter()
            .find(|group| {
                group.vertex.id() == vertex_buffer.id() && group.index.id() == index_buffer.id()
            })
            .and_then(|group| {
                if from_prepass {
                    group.prepass_mirror.as_ref()
                } else {
                    group.mirror.as_ref()
                }
            });
        match mirror {
            Some(mirror) if tag().is_some_and(|tag| tag & TRIANGLE_CULL_LEAF != 0) => mirror,
            _ => index_buffer,
        }
    }

    /// Moves the counts' readback on: maps them once the frame that copied them is submitted,
    /// and logs them once mapped.
    fn advance_stats(&mut self) {
        let state = self.stats_state.get_mut();
        match *state {
            // The pass didn't run.
            STATS_WANTED => *state = STATS_IDLE,
            STATS_COPIED => {
                self.map_state.store(MAP_PENDING, Ordering::Relaxed);
                let map_state = self.map_state.clone();
                self.stats_readback
                    .map_async(MapMode::Read, .., move |result| {
                        map_state.store(
                            if result.is_ok() { MAP_DONE } else { MAP_FAILED },
                            Ordering::Release,
                        );
                    });
                *state = STATS_MAPPING;
            }
            STATS_MAPPING => match self.map_state.load(Ordering::Acquire) {
                MAP_DONE => {
                    let [counts, clusters, cutout_clusters]: [[u32; 8]; 3] =
                        bytemuck::pod_read_unaligned(&self.stats_readback.get_mapped_range(..));
                    self.stats_readback.unmap();
                    *state = STATS_IDLE;
                    self.last_report = Some(Instant::now());
                    let [kept, outside, away, no_pixel, behind, tested, copied, invalid] = counts;
                    if self.stats_clusters {
                        log_clusters("clusters", clusters);
                    }
                    if self.stats_cutouts {
                        log_clusters("cutout clusters", cutout_clusters);
                    }
                    if self.stats_marks {
                        // The pass ran no test: `outside` counts the triangles it left out, the
                        // unmarked ones, or the marked ones when it inverts the marks.
                        let total = (u64::from(kept) + u64::from(outside)).max(1) as f64;
                        let (kept_as, culled_as) = if self.stats_inverted {
                            ("unmarked", "marked")
                        } else {
                            ("marked", "unmarked")
                        };
                        info!(
                            "[triangle cull] {tested} draws by their marks, {copied} copied, \
                             {invalid} out of range: kept {kept} {kept_as} triangles ({:.1}%); \
                             culled {outside} {culled_as} ({:.1}%)",
                            100.0 * kept as f64 / total,
                            100.0 * outside as f64 / total,
                        );
                        return;
                    }
                    let total = [kept, outside, away, no_pixel, behind]
                        .iter()
                        .map(|&count| u64::from(count))
                        .sum::<u64>()
                        .max(1) as f64;
                    let share = |count: u32| 100.0 * count as f64 / total;
                    info!(
                        "[triangle cull] {tested} draws tested, {copied} copied, {invalid} out of \
                         range: kept {kept} triangles ({:.1}%); culled {outside} outside the \
                         view ({:.1}%), {away} facing away ({:.1}%), {no_pixel} covering no \
                         pixel centre ({:.1}%), {behind} behind the depth pyramid ({:.1}%)",
                        share(kept),
                        share(outside),
                        share(away),
                        share(no_pixel),
                        share(behind),
                    );
                }
                MAP_FAILED => *state = STATS_IDLE,
                _ => {}
            },
            _ => {}
        }
    }

    /// Whether this frame should count the triangles: about every [`STATS_REPORT_SECONDS`],
    /// unless [`TRIANGLE_CULL_DEBUG_NO_STATS`] is set.
    fn stats_due(&self) -> bool {
        self.debug & TRIANGLE_CULL_DEBUG_NO_STATS == 0
            && self.stats_state.load(Ordering::Relaxed) == STATS_IDLE
            && self
                .last_report
                .is_none_or(|last| last.elapsed().as_secs_f32() >= STATS_REPORT_SECONDS)
    }

    fn set_status(&mut self, status: &'static str) {
        if self.status != status {
            info!("[triangle cull] {status}");
            self.status = status;
        }
    }

    fn set_cluster_status(&mut self, status: &'static str) {
        if self.cluster_status != status {
            info!("[triangle cull] clusters: {status}");
            self.cluster_status = status;
        }
    }

    /// Queues `kernel`'s `early` and `late` on its first use, and returns whether both are ready.
    fn cluster_pipelines_ready(&mut self, pipeline_cache: &PipelineCache, kernel: usize) -> bool {
        let layout = &self.cluster_layout;
        let shader = &self.shader_clusters;
        let entry_points = [["early", "late"], ["early_fast", "late_fast"]][kernel];
        let ids = *self.pipelines_clusters[kernel].get_or_insert_with(|| {
            entry_points.map(|entry_point| {
                pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
                    label: Some(format!("triangle cull clusters {entry_point}").into()),
                    layout: vec![layout.clone()],
                    push_constant_ranges: vec![PushConstantRange {
                        stages: ShaderStages::COMPUTE,
                        range: 0..PUSH_CONSTANT_BYTES_CLUSTERS,
                    }],
                    shader: shader.clone(),
                    entry_point: Some(entry_point.into()),
                    ..default()
                })
            })
        });
        ids.iter()
            .all(|id| pipeline_cache.get_compute_pipeline(*id).is_some())
    }

    /// Queues the chosen version's pipelines on its first use, and returns whether those this
    /// frame needs are ready: the second's `list`, and `keep_marked` if `marks_kernel` (or
    /// `keep_marked_ballot` if `ballot` too, in 8 threads if `ballot_8`, else
    /// `keep_marked_coalesced` if `coalesced`), else `cull`.
    fn pipelines_ready(
        &mut self,
        pipeline_cache: &PipelineCache,
        v2: bool,
        marks_kernel: bool,
        coalesced: bool,
        ballot: bool,
        ballot_8: bool,
    ) -> bool {
        let queue = |label: &'static str,
                     layout: &BindGroupLayoutDescriptor,
                     shader: &Handle<Shader>,
                     entry_point: Option<&'static str>,
                     shader_defs: &[&'static str],
                     bytes: u32| {
            pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(label.into()),
                layout: vec![layout.clone()],
                push_constant_ranges: vec![PushConstantRange {
                    stages: ShaderStages::COMPUTE,
                    range: 0..bytes,
                }],
                shader: shader.clone(),
                shader_defs: shader_defs.iter().map(|def| (*def).into()).collect(),
                entry_point: entry_point.map(Into::into),
                ..default()
            })
        };
        let ids = if v2 {
            let [list, test, keep] = *self.pipelines_v2.get_or_insert_with(|| {
                [
                    queue(
                        "triangle cull list",
                        &self.list_layout,
                        &self.shader_v2,
                        Some("list"),
                        &[],
                        PUSH_CONSTANT_BYTES_V2,
                    ),
                    queue(
                        "triangle cull cull",
                        &self.cull_layout,
                        &self.shader_v2,
                        Some("cull"),
                        &[],
                        PUSH_CONSTANT_BYTES_V2,
                    ),
                    queue(
                        "triangle cull keep marked",
                        &self.cull_layout,
                        &self.shader_v2,
                        Some("keep_marked"),
                        &[],
                        PUSH_CONSTANT_BYTES_V2,
                    ),
                ]
            });
            let keep = if ballot && ballot_8 {
                *self.pipeline_ballot_8.get_or_insert_with(|| {
                    queue(
                        "triangle cull keep marked ballot 8",
                        &self.cull_layout,
                        &self.shader_v2,
                        Some("keep_marked_ballot"),
                        &["TRIANGLE_CULL_BALLOT", "TRIANGLE_CULL_BALLOT_8"],
                        PUSH_CONSTANT_BYTES_V2,
                    )
                })
            } else if ballot {
                // Only this pipeline's shader has the entry point, which needs subgroups.
                *self.pipeline_ballot.get_or_insert_with(|| {
                    queue(
                        "triangle cull keep marked ballot",
                        &self.cull_layout,
                        &self.shader_v2,
                        Some("keep_marked_ballot"),
                        &["TRIANGLE_CULL_BALLOT"],
                        PUSH_CONSTANT_BYTES_V2,
                    )
                })
            } else if coalesced {
                *self.pipeline_coalesced.get_or_insert_with(|| {
                    queue(
                        "triangle cull keep marked coalesced",
                        &self.cull_layout,
                        &self.shader_v2,
                        Some("keep_marked_coalesced"),
                        &[],
                        PUSH_CONSTANT_BYTES_V2,
                    )
                })
            } else {
                keep
            };
            vec![list, if marks_kernel { keep } else { test }]
        } else {
            vec![*self.pipeline.get_or_insert_with(|| {
                queue(
                    "triangle cull",
                    &self.layout,
                    &self.shader,
                    None,
                    &[],
                    PUSH_CONSTANT_BYTES,
                )
            })]
        };
        ids.iter()
            .all(|id| pipeline_cache.get_compute_pipeline(*id).is_some())
    }
}

fn describe_debug(debug: u32) -> String {
    [
        (TRIANGLE_CULL_DEBUG_MIRRORS_ONLY, "mirrors only"),
        (TRIANGLE_CULL_DEBUG_EMPTY, "empty dispatches"),
        (TRIANGLE_CULL_DEBUG_NO_SWAP, "no swap"),
        (TRIANGLE_CULL_DEBUG_NO_STATS, "no counts"),
        (TRIANGLE_CULL_DEBUG_CLEAR, "mirrors cleared every frame"),
        (TRIANGLE_CULL_DEBUG_STATS_ONLY, "counts read back without a pass"),
    ]
    .iter()
    .filter(|(bit, _)| debug & bit != 0)
    .map(|(_, name)| *name)
    .collect::<Vec<_>>()
    .join(", ")
}

/// Logs a frame's counts from `triangle_cull_clusters.wgsl`, for the opaque draws or the
/// cutouts'.
fn log_clusters(label: &str, clusters: [u32; 8]) {
    let [early_tested, early_kept, late_tested, late_kept, leaf_triangles, drawn, leaves, whole] =
        clusters;
    info!(
        "[triangle cull] {label}: the prepass drew {drawn} of the {leaf_triangles} triangles of \
         {leaves} leaves ({:.1}%), and {whole} leaves whole; the early pass kept {early_kept} of \
         {early_tested} clusters, the late pass {late_kept} of {late_tested}",
        100.0 * drawn as f64 / f64::from(leaf_triangles.max(1)),
    );
}

pub(crate) fn build(app: &mut App) {
    embedded_asset!(app, "triangle_cull.wgsl");
    embedded_asset!(app, "triangle_cull_v2.wgsl");
    embedded_asset!(app, "triangle_cull_clusters.wgsl");
    app.add_plugins((
        ExtractResourcePlugin::<TriangleCullMarks>::default(),
        ExtractResourcePlugin::<TriangleCullClusters>::default(),
    ));
}

pub(crate) fn finish(render_app: &mut SubApp) {
    // Before any slab exists: the pass reads the vertex and index slabs as storage buffers.
    if let Some(mut allocator) = render_app.world_mut().get_resource_mut::<MeshAllocator>() {
        allocator.extra_buffer_usages |= BufferUsages::STORAGE;
    }
    render_app
        .init_resource::<TriangleCull>()
        .add_systems(
            Render,
            prepare_triangle_cull.in_set(RenderSystems::PrepareBindGroups),
        )
        .add_render_graph_node::<TriangleCullNode>(Core3d, TriangleCullLabel)
        .add_render_graph_node::<TriangleCullClusterNode<false>>(
            Core3d,
            TriangleCullClusterLabel::Early,
        )
        .add_render_graph_node::<TriangleCullClusterNode<true>>(
            Core3d,
            TriangleCullClusterLabel::Late,
        )
        .add_render_graph_edges(
            Core3d,
            (
                NodePbr::MainBuildIndirectParameters,
                TriangleCullLabel,
                Node3d::StartMainPass,
            ),
        )
        .add_render_graph_edges(
            Core3d,
            (
                NodePbr::EarlyPrepassBuildIndirectParameters,
                TriangleCullClusterLabel::Early,
                Node3d::EarlyPrepass,
            ),
        )
        .add_render_graph_edges(
            Core3d,
            (
                NodePbr::LatePrepassBuildIndirectParameters,
                TriangleCullClusterLabel::Late,
                Node3d::LatePrepass,
            ),
        );
}

/// The vertex stride and the position's offset, in 4-byte words, if positions are 32-bit floats.
fn position_words(mesh: &RenderMesh) -> Option<(u32, u32)> {
    let layout = &mesh.layout.0;
    let index = layout
        .attribute_ids()
        .iter()
        .position(|id| *id == Mesh::ATTRIBUTE_POSITION.id)?;
    let attribute = layout.layout().attributes.get(index)?;
    let stride = layout.layout().array_stride;
    (attribute.format == VertexFormat::Float32x3 && attribute.offset % 4 == 0 && stride % 4 == 0)
        .then(|| ((stride / 4) as u32, (attribute.offset / 4) as u32))
}

/// What a scan found: the pairs of slabs that hold ready tagged meshes, in slab order, and how
/// many tagged instances wait for their meshes.
struct Scanned {
    groups: Vec<Group>,
    pending: usize,
}

/// Groups the ready tagged meshes by their slabs; `Err` names why the pass can't take them.
fn scan(
    instances: &crate::RenderMeshInstancesGpu,
    meshes: &RenderAssets<RenderMesh>,
    allocator: &MeshAllocator,
) -> Result<Scanned, &'static str> {
    let mut groups: Vec<Group> = Vec::new();
    let mut pending = 0;
    for instance in instances.values() {
        if instance.shared.tag & TRIANGLE_CULL_LEAF == 0 {
            continue;
        }
        let id = instance.shared.mesh_asset_id;
        let (Some(mesh), Some(vertex), Some(index)) = (
            meshes.get(id),
            allocator.mesh_vertex_slice(&id),
            allocator.mesh_index_slice(&id),
        ) else {
            // Not uploaded yet, so not drawn.
            pending += 1;
            continue;
        };
        if !matches!(
            mesh.buffer_info,
            RenderMeshBufferInfo::Indexed {
                index_format: IndexFormat::Uint32,
                ..
            }
        ) {
            return Err("off: a tagged mesh doesn't have 32-bit indices");
        }
        let Some((stride_words, position_words)) = position_words(mesh) else {
            return Err("off: a tagged mesh's positions aren't 32-bit floats");
        };
        let slabs = slab_word(allocator.mesh_slabs(&id));
        if slabs & NO_SLAB == NO_SLAB || slabs >> 16 == NO_SLAB {
            return Err("off: a tagged mesh's slab id doesn't fit in 16 bits");
        }
        match groups.iter_mut().find(|group| group.slabs == slabs) {
            Some(group) => {
                if group.stride_words != stride_words || group.position_words != position_words {
                    return Err("off: the tagged meshes of one slab differ in layout");
                }
                group.instances += 1;
            }
            None => {
                let storage = |buffer: &Buffer| buffer.usage().contains(BufferUsages::STORAGE);
                if !storage(vertex.buffer) || !storage(index.buffer) {
                    return Err("off: the mesh slabs can't be read as storage buffers");
                }
                groups.push(Group {
                    slabs,
                    vertex: vertex.buffer.clone(),
                    index: index.buffer.clone(),
                    stride_words,
                    position_words,
                    instances: 1,
                    representative: id,
                    mirror: None,
                    prepass_mirror: None,
                });
            }
        }
    }
    groups.sort_unstable_by_key(|group| group.slabs);
    Ok(Scanned { groups, pending })
}

/// Logs where the tagged meshes are.
fn log_groups(groups: &[Group], pending: usize) {
    let mib = |buffer: &Buffer| buffer.size() as f64 / 1048576.0;
    let mut text = String::new();
    for group in groups {
        let _ = write!(
            text,
            "{}{} in vertex slab {} ({:.1} MiB, {} words a vertex, position at word {}) with \
             index slab {} ({:.1} MiB)",
            if text.is_empty() { "" } else { "; " },
            group.instances,
            group.slabs & NO_SLAB,
            mib(&group.vertex),
            group.stride_words,
            group.position_words,
            group.slabs >> 16,
            mib(&group.index),
        );
    }
    if text.is_empty() {
        text.push_str("none ready");
    }
    info!("[triangle cull] tagged instances: {text}; {pending} not uploaded yet");
}

/// Whether no view but `view` has anything to draw in `phases`.
fn only_view<BPI: BinnedPhaseItem>(
    phases: Option<&ViewBinnedRenderPhases<BPI>>,
    view: &RetainedViewEntity,
) -> bool {
    phases.is_none_or(|phases| {
        phases
            .iter()
            .all(|(id, phase)| id == view || phase.is_empty())
    })
}

/// Whether the pipeline of every mesh draw in `view`'s phase `BPI` of the depth prepass is ready.
fn prepass_pipelines_ready<BPI>(
    pipeline_cache: &PipelineCache,
    phases: Option<&ViewBinnedRenderPhases<BPI>>,
    view: &RetainedViewEntity,
) -> bool
where
    BPI: BinnedPhaseItem<BatchSetKey = OpaqueNoLightmap3dBatchSetKey>,
{
    let Some(phase) = phases.and_then(|phases| phases.get(view)) else {
        return true;
    };
    phase
        .multidrawable_meshes
        .keys()
        .chain(phase.batchable_meshes.keys().map(|(key, _)| key))
        .chain(phase.unbatchable_meshes.keys().map(|(key, _)| key))
        .all(|key| pipeline_cache.get_render_pipeline(key.pipeline).is_some())
}

/// Checks where the tagged meshes live, (re)creates the mirror, and builds this frame's
/// dispatches, one per phase with indexed draws.
pub fn prepare_triangle_cull(
    cull: ResMut<TriangleCull>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    mesh_instances: Res<RenderMeshInstances>,
    meshes: Res<RenderAssets<RenderMesh>>,
    mesh_allocator: Res<MeshAllocator>,
    batched_instance_buffers: Option<Res<BatchedInstanceBuffers<MeshUniform, MeshInputUniform>>>,
    indirect_parameters_buffers: Res<IndirectParametersBuffers>,
    (view_uniforms, previous_view_uniforms): (
        Res<ViewUniforms>,
        Option<Res<PreviousViewUniforms>>,
    ),
    opaque_phases: Res<ViewBinnedRenderPhases<Opaque3d>>,
    alpha_mask_phases: Res<ViewBinnedRenderPhases<AlphaMask3d>>,
    prepass_phases: (
        Option<Res<ViewBinnedRenderPhases<Opaque3dPrepass>>>,
        Option<Res<ViewBinnedRenderPhases<AlphaMask3dPrepass>>>,
    ),
    (marks_id, boxes_id): (
        Option<Res<TriangleCullMarks>>,
        Option<Res<TriangleCullClusters>>,
    ),
    storage_buffers: Res<RenderAssets<GpuShaderStorageBuffer>>,
    views: Query<(
        Entity,
        &ExtractedView,
        &ViewUniformOffset,
        Option<&ViewDepthPyramid>,
        Option<&Msaa>,
        Has<DepthPrepass>,
        Option<&PreviousViewUniformOffset>,
        Has<OcclusionCulling>,
    )>,
) {
    let cull = cull.into_inner();
    cull.dispatches.clear();
    cull.view = None;
    cull.phases = 0;
    *cull.dispatched.get_mut() = false;
    cull.cluster_dispatches.clear();
    cull.cluster_phases = 0;
    cull.direct_phases = 0;
    *cull.clusters_state.get_mut() = 0;
    cull.stamp = cull.stamp % 255 + 1;
    cull.advance_stats();

    let cluster_setting = TRIANGLE_CULL_CLUSTERS.load(Ordering::Relaxed);
    let setting = TRIANGLE_CULL.load(Ordering::Relaxed);
    if setting == 0 || cluster_setting == 0 {
        cull.prepass_mirrors.clear();
        cull.cluster_words = None;
        for group in &mut cull.groups {
            group.prepass_mirror = None;
        }
        cull.set_cluster_status("off");
    }
    if setting == 0 {
        cull.groups.clear();
        cull.mirrors.clear();
        cull.items = None;
        cull.counts = None;
        cull.pending = 0;
        cull.instances_scanned = usize::MAX;
        cull.debug = 0;
        cull.version_logged = 0;
        cull.set_status("off");
        return;
    }
    if setting & TRIANGLE_CULL_DEBUG != cull.debug {
        cull.debug = setting & TRIANGLE_CULL_DEBUG;
        if cull.debug != 0 {
            info!("[triangle cull] debug: {}", describe_debug(cull.debug));
        }
    }
    let RenderMeshInstances::GpuBuilding(instances) = &*mesh_instances else {
        cull.set_status("off: meshes aren't built on the GPU");
        return;
    };

    // Where the tagged meshes are: scanned when the instances change, when a group's buffers
    // move, every few frames while some meshes aren't uploaded, and every few seconds regardless.
    cull.frames_since_scan += 1;
    let moved = cull.groups.iter().any(|group| {
        let vertex = mesh_allocator.mesh_vertex_slice(&group.representative);
        let index = mesh_allocator.mesh_index_slice(&group.representative);
        !matches!(
            (vertex, index),
            (Some(vertex), Some(index))
                if vertex.buffer.id() == group.vertex.id()
                    && index.buffer.id() == group.index.id()
        )
    });
    let rescan_frames = if cull.pending > 0 {
        PENDING_RESCAN_FRAMES
    } else {
        RESCAN_FRAMES
    };
    if moved
        || cull.instances_scanned != instances.len()
        || cull.frames_since_scan >= rescan_frames
    {
        cull.frames_since_scan = 0;
        cull.instances_scanned = instances.len();
        match scan(instances, &meshes, &mesh_allocator) {
            Ok(Scanned { groups, pending }) => {
                let changed = groups.len() != cull.groups.len()
                    || groups.iter().zip(&cull.groups).any(|(new, old)| {
                        new.slabs != old.slabs
                            || new.vertex.id() != old.vertex.id()
                            || new.index.id() != old.index.id()
                    });
                if changed || (pending == 0) != (cull.pending == 0) {
                    log_groups(&groups, pending);
                }
                if groups.is_empty() {
                    cull.set_status("waiting: no tagged mesh is ready");
                }
                cull.groups = groups;
                cull.pending = pending;
            }
            Err(reason) => {
                cull.groups.clear();
                cull.pending = 0;
                cull.set_status(reason);
                return;
            }
        }
    }
    if cull.groups.is_empty() {
        return;
    }

    // The one view that draws opaque or cutout meshes.
    let mut retained = None;
    for (id, empty) in opaque_phases
        .iter()
        .map(|(id, phase)| (*id, phase.is_empty()))
        .chain(
            alpha_mask_phases
                .iter()
                .map(|(id, phase)| (*id, phase.is_empty())),
        )
    {
        if empty {
            continue;
        }
        match retained {
            None => retained = Some(id),
            Some(retained) if retained == id => {}
            Some(_) => {
                cull.set_status("off: more than one view draws opaque meshes");
                return;
            }
        }
    }
    let Some(retained) = retained else {
        cull.set_status("waiting: no view draws opaque meshes");
        return;
    };
    let Some((view, _, view_offset, pyramid, msaa, depth_prepass, previous_offset, occlusion)) =
        views
            .iter()
            .find(|(_, view, ..)| view.retained_view_entity == retained)
    else {
        return;
    };

    let (Some(view_binding), Some(current_input)) = (
        view_uniforms.uniforms.binding(),
        batched_instance_buffers
            .as_ref()
            .and_then(|buffers| buffers.current_input_buffer.buffer().buffer()),
    ) else {
        return;
    };
    let v2 = setting & TRIANGLE_CULL_V1 == 0;
    let marks_mode = setting & TRIANGLE_CULL_MARKS != 0;
    let opaque_only = marks_mode && setting & TRIANGLE_CULL_MARKS_OPAQUE_ONLY != 0;
    let inverted = marks_mode && setting & TRIANGLE_CULL_MARKS_INVERT != 0;
    let marks_kernel = marks_mode && setting & TRIANGLE_CULL_MARKS_IN_CULL == 0;
    let ballot_asked = marks_kernel && setting & TRIANGLE_CULL_MARKS_BALLOT != 0;
    let ballot_lanes = if setting & TRIANGLE_CULL_MARKS_BALLOT_8 != 0 { 8 } else { 32 };
    let ballot = ballot_asked && cull.min_subgroup_size >= ballot_lanes;
    let ballot_8 = ballot && ballot_lanes == 8;
    let coalesced = marks_kernel && !ballot && setting & TRIANGLE_CULL_MARKS_COALESCED != 0;
    if marks_mode && !v2 {
        cull.set_status("off: the marks need the second version");
        return;
    }
    // Without the prepass nothing marks, and the draws would keep no triangle.
    if marks_mode && !depth_prepass {
        cull.set_status("off: the marks need the view's depth prepass");
        return;
    }
    if !cull.pipelines_ready(&pipeline_cache, v2, marks_kernel, coalesced, ballot, ballot_8) {
        cull.set_status("waiting: the pipeline is compiling");
        return;
    }
    // A prepass draw whose pipeline isn't ready is skipped and marks nothing, so the main pass
    // would keep none of its triangles: until the view's whole depth prepass is ready, as after
    // the marking variants are first asked for, the draws keep all their triangles.
    if marks_mode
        && !(prepass_pipelines_ready(&pipeline_cache, prepass_phases.0.as_deref(), &retained)
            && prepass_pipelines_ready(&pipeline_cache, prepass_phases.1.as_deref(), &retained))
    {
        cull.set_status("waiting: the depth prepass is compiling");
        return;
    }
    // Uploaded with the map that marks into it; until then the draws keep all their triangles.
    let marks = if marks_mode {
        let Some(marks) = marks_id
            .as_deref()
            .and_then(|marks| storage_buffers.get(marks.0))
        else {
            cull.set_status("waiting: the marks' buffer isn't uploaded");
            return;
        };
        Some(&marks.buffer)
    } else {
        None
    };
    cull.v2 = v2;
    cull.marks_kernel = marks_kernel;
    cull.coalesced = coalesced;
    cull.ballot = ballot;
    cull.ballot_8 = ballot_8;
    let kernel = match (v2, marks_kernel, coalesced, ballot) {
        (false, ..) => 1,
        (true, false, ..) => 2,
        (true, true, _, true) if ballot_8 => 6,
        (true, true, _, true) => 5,
        (true, true, true, false) => 4,
        (true, true, false, false) => 3,
    };
    let ballot_missing = ballot_asked && !ballot;
    let version = kernel | if ballot_missing { VERSION_NO_BALLOT } else { 0 };
    if cull.version_logged != version {
        cull.version_logged = version;
        let no_ballot = match (ballot_missing, cull.min_subgroup_size) {
            (false, _) => String::new(),
            (true, 0) => "; keep_marked_ballot needs subgroups, which this device lacks".into(),
            (true, size) => format!(
                "; keep_marked_ballot needs subgroups of at least {ballot_lanes} invocations, \
                 and this device's can have {size}"
            ),
        };
        info!(
            "[triangle cull] version {}: {}{no_ballot}",
            kernel.min(2),
            match kernel {
                1 => "a 256-thread workgroup for every draw slot",
                2 => "a list of the leaf draws, then a 64-thread workgroup for each",
                3 => {
                    "a list of the leaf draws, then a 64-thread workgroup for each, keeping \
                     the marked triangles four to a thread (keep_marked)"
                }
                4 => {
                    "a list of the leaf draws, then a 64-thread workgroup for each, keeping \
                     the marked triangles with neighbouring words a thread \
                     (keep_marked_coalesced)"
                }
                5 => {
                    "a list of the leaf draws, then a 32-thread workgroup for each, keeping \
                     the marked triangles in the places subgroup ballots give them \
                     (keep_marked_ballot)"
                }
                _ => {
                    "a list of the leaf draws, then an 8-thread workgroup for each, keeping \
                     the marked triangles in the places subgroup ballots give them \
                     (keep_marked_ballot's 8-thread test)"
                }
            }
        );
    }

    // One mirror per index slab that holds tagged meshes, as large as the slab.
    let groups = &mut cull.groups;
    cull.mirrors.retain(|mirror| {
        groups.iter().any(|group| {
            group.slabs >> 16 == mirror.index_slab && group.index.size() == mirror.buffer.size()
        })
    });
    for group in groups.iter_mut() {
        let index_slab = group.slabs >> 16;
        let mirror = match cull.mirrors.iter().find(|mirror| mirror.index_slab == index_slab) {
            Some(mirror) => mirror.buffer.clone(),
            None => {
                let buffer = render_device.create_buffer(&BufferDescriptor {
                    label: Some("triangle cull mirror"),
                    size: group.index.size(),
                    // COPY_DST only for TRIANGLE_CULL_DEBUG_CLEAR: wgpu adds it underneath
                    // anyway, to zero the buffer before its first use.
                    usage: BufferUsages::INDEX | BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                cull.mirrors.push(Mirror {
                    index_slab,
                    buffer: buffer.clone(),
                });
                buffer
            }
        };
        group.mirror = Some(mirror);
    }
    if setting & TRIANGLE_CULL_DEBUG_MIRRORS_ONLY != 0 {
        if setting & TRIANGLE_CULL_DEBUG_STATS_ONLY != 0 && cull.stats_due() {
            *cull.stats_state.get_mut() = STATS_WANTED;
            cull.stats_marks = marks_mode;
            cull.stats_inverted = inverted;
        }
        cull.view = Some(view);
        cull.view_offset = view_offset.offset;
        cull.set_status("on, debug: the mirrors are made but nothing is dispatched");
        return;
    }

    // What the depth prepass's cluster cull needs, if it's to run this frame: the boxes, the
    // previous frame's view and its offset, and the cluster words. The prepass's mirrors are made
    // here too, one per index slab, as large as the slab.
    let cluster_inputs = 'clusters: {
        if cluster_setting == 0 {
            break 'clusters Err("off");
        }
        if !v2 || !marks_mode || inverted || cull.debug != 0 {
            break 'clusters Err("off: it needs the marks in the second version, and no debugging");
        }
        if !depth_prepass || !occlusion || pyramid.is_none() {
            break 'clusters Err(
                "off: it needs the view's depth prepass, occlusion culling and depth pyramid",
            );
        }
        // Then the build packs the draws, and a slot's metadata isn't its draw's.
        if render_device
            .features()
            .contains(WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT)
        {
            break 'clusters Err("off: the device packs the indirect draws");
        }
        // The prepass's draws in any view would read its mirrors.
        if !only_view(prepass_phases.0.as_deref(), &retained)
            || !only_view(prepass_phases.1.as_deref(), &retained)
        {
            break 'clusters Err("off: more than one view draws a depth prepass");
        }
        let Some(boxes) = boxes_id
            .as_deref()
            .and_then(|boxes| storage_buffers.get(boxes.0))
        else {
            break 'clusters Err("waiting: the boxes' buffer isn't uploaded");
        };
        let (Some(previous_view), Some(previous_offset)) = (
            previous_view_uniforms
                .as_deref()
                .and_then(|uniforms| uniforms.uniforms.buffer()),
            previous_offset,
        ) else {
            break 'clusters Err("waiting: the previous view's uniforms aren't uploaded");
        };
        let words_bytes = boxes.buffer.size() / CLUSTER_BOX_BYTES * 8;
        if words_bytes == 0 {
            break 'clusters Err("waiting: the boxes' buffer is empty");
        }
        let kernel =
            usize::from((cluster_setting >> TRIANGLE_CULL_CLUSTERS_KERNEL_SHIFT) & 0xff != 0);
        if !cull.cluster_pipelines_ready(&pipeline_cache, kernel) {
            break 'clusters Err("waiting: the pipelines are compiling");
        }
        cull.cluster_kernel = kernel;
        // Zeroed before its first use, so no count word has a frame's stamp.
        let words = match cull.cluster_words.take() {
            Some(words) if words.size() == words_bytes => words,
            _ => render_device.create_buffer(&BufferDescriptor {
                label: Some("triangle cull cluster words"),
                size: words_bytes,
                usage: BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        };
        cull.cluster_words = Some(words.clone());
        let groups = &mut cull.groups;
        cull.prepass_mirrors.retain(|mirror| {
            groups.iter().any(|group| {
                group.slabs >> 16 == mirror.index_slab && group.index.size() == mirror.buffer.size()
            })
        });
        for group in groups.iter_mut() {
            let index_slab = group.slabs >> 16;
            let mirror = match cull
                .prepass_mirrors
                .iter()
                .find(|mirror| mirror.index_slab == index_slab)
            {
                Some(mirror) => mirror.buffer.clone(),
                None => {
                    let buffer = render_device.create_buffer(&BufferDescriptor {
                        label: Some("triangle cull prepass mirror"),
                        size: group.index.size(),
                        usage: BufferUsages::INDEX | BufferUsages::STORAGE,
                        mapped_at_creation: false,
                    });
                    cull.prepass_mirrors.push(Mirror {
                        index_slab,
                        buffer: buffer.clone(),
                    });
                    buffer
                }
            };
            group.prepass_mirror = Some(mirror);
        }
        Ok((boxes, previous_view, previous_offset.offset, words))
    };

    let span = if setting & TRIANGLE_CULL_NO_DEPTH != 0 || pyramid.is_none() {
        0
    } else {
        (setting & 0xff).clamp(1, 32)
    };
    let pyramid = pyramid.map_or(&cull.no_pyramid, |pyramid| &pyramid.all_mips);
    let mut flags = if msaa.is_some_and(|msaa| msaa.samples() == 1) {
        PIXEL_CENTRES
    } else {
        0
    };
    if setting & TRIANGLE_CULL_DEBUG_EMPTY != 0 {
        flags |= EMPTY;
    }
    if marks_mode {
        flags |= MARKS;
    }
    if inverted {
        flags |= MARKS_INVERT;
    }
    if cull.stats_due() {
        *cull.stats_state.get_mut() = STATS_WANTED;
        cull.stats_marks = marks_mode;
        cull.stats_inverted = inverted;
        flags |= COUNT;
    }
    // Each phase's dispatches, one per group: its indirect parameters, their GPU metadata, its
    // slots, the group, the phase and whether its draws read the prepass's mirrors as they are.
    // Under `TRIANGLE_CULL_MARKS_OPAQUE_ONLY` the cutouts' draws are left whole, unless
    // `TRIANGLE_CULL_CLUSTERS_CUTOUTS` has them drawn as the cluster cull leaves their prepass's.
    let cutouts = cluster_setting & TRIANGLE_CULL_CLUSTERS_CUTOUTS != 0;
    let mut plans = Vec::new();
    let mut phases = 0;
    let mut direct_phases = 0;
    for (phase, bit) in [
        (TypeId::of::<Opaque3d>(), PHASE_OPAQUE),
        (TypeId::of::<AlphaMask3d>(), PHASE_ALPHA_MASK),
    ] {
        let direct = opaque_only && bit == PHASE_ALPHA_MASK;
        if direct && (!cutouts || !v2) {
            continue;
        }
        let Some(buffers) = indirect_parameters_buffers.get(&phase) else {
            continue;
        };
        let slots = buffers.indexed.batch_count() as u32;
        let (Some(parameters), Some(gpu_metadata)) = (
            buffers.indexed.data_buffer(),
            buffers.indexed.gpu_metadata_buffer(),
        ) else {
            continue;
        };
        if slots == 0 {
            continue;
        }
        for (group, _) in cull
            .groups
            .iter()
            .enumerate()
            .filter(|(_, group)| group.mirror.is_some())
        {
            plans.push((parameters, gpu_metadata, slots, group, bit, direct));
        }
        if direct {
            direct_phases |= bit;
        } else {
            phases |= bit;
        }
    }
    // `cull`'s indirect dispatch takes a workgroup for each item and one for the empty item.
    if v2 && plans.iter().any(|&(_, _, slots, ..)| slots >= MAX_WORKGROUPS) {
        cull.set_status("off: too many draws for one dispatch of the second version");
        return;
    }

    // The cluster cull covers the prepass phase of each phase the main pass keeps the marked
    // triangles of, or draws as the prepass did, with a dispatch per group that its `early` and
    // `late` share.
    let counting = flags & COUNT != 0;
    let mut cluster_dispatches = Vec::new();
    let mut cluster_phases = 0;
    let cluster_status = match cluster_inputs {
        Err(status) => status,
        Ok((boxes, previous_view, previous_offset, words)) => {
            let layout = pipeline_cache.get_bind_group_layout(&cull.cluster_layout);
            let cluster_span = (cluster_setting & 0xff).clamp(1, 32);
            for (phase, bit) in [
                (TypeId::of::<Opaque3dPrepass>(), PHASE_OPAQUE),
                (TypeId::of::<AlphaMask3dPrepass>(), PHASE_ALPHA_MASK),
            ] {
                if (phases | direct_phases) & bit == 0 {
                    continue;
                }
                let Some(buffers) = indirect_parameters_buffers.get(&phase) else {
                    continue;
                };
                let slots = buffers.indexed.batch_count() as u32;
                let meshes = batched_instance_buffers
                    .as_deref()
                    .and_then(|buffers| buffers.phase_instance_buffers.get(&phase))
                    .and_then(|buffers| buffers.data_buffer.buffer());
                let (Some(parameters), Some(gpu_metadata), Some(cpu_metadata), Some(meshes)) = (
                    buffers.indexed.data_buffer(),
                    buffers.indexed.gpu_metadata_buffer(),
                    buffers.indexed.cpu_metadata_buffer(),
                    meshes,
                ) else {
                    continue;
                };
                let workgroups = slots.div_ceil(CLUSTER_SLOTS_PER_WORKGROUP);
                if slots == 0 || workgroups > MAX_WORKGROUPS {
                    continue;
                }
                for group in &cull.groups {
                    let Some(mirror) = &group.prepass_mirror else {
                        continue;
                    };
                    let bind_group = render_device.create_bind_group(
                        "triangle cull clusters bind group",
                        &layout,
                        &BindGroupEntries::with_indices((
                            (0, view_binding.clone()),
                            (1, current_input.as_entire_binding()),
                            (2, gpu_metadata.as_entire_binding()),
                            (3, parameters.as_entire_binding()),
                            (4, cpu_metadata.as_entire_binding()),
                            (5, group.index.as_entire_binding()),
                            (6, mirror.as_entire_binding()),
                            (7, pyramid),
                            (8, cull.stats.as_entire_binding()),
                            (9, boxes.buffer.as_entire_binding()),
                            (10, words.as_entire_binding()),
                            (
                                12,
                                BufferBinding {
                                    buffer: previous_view,
                                    offset: u64::from(previous_offset),
                                    size: NonZeroU64::new(size_of::<PreviousViewData>() as u64),
                                },
                            ),
                            (13, meshes.as_entire_binding()),
                        )),
                    );
                    let index_words =
                        (group.index.size().min(mirror.size()) / 4).min(u32::MAX.into());
                    let cutout = if bit == PHASE_ALPHA_MASK { CLUSTER_CUTOUTS } else { 0 };
                    cluster_dispatches.push(ClusterDispatch {
                        bind_group,
                        push_constants: [
                            slots,
                            index_words as u32,
                            group.slabs,
                            cluster_span,
                            flags & COUNT | cutout,
                            cull.stamp,
                        ],
                        workgroups,
                    });
                }
                cluster_phases |= bit;
            }
            if cluster_phases == 0 {
                "waiting: no depth prepass draws to cull"
            } else {
                "on"
            }
        }
    };

    let mut dispatches = Vec::with_capacity(plans.len());
    let mut direct_dispatched = 0;
    if v2 && !plans.is_empty() {
        let items_bytes = plans
            .iter()
            .map(|&(_, _, slots, ..)| (u64::from(slots) + 1) * ITEM_BYTES)
            .sum::<u64>();
        if cull
            .items
            .as_ref()
            .is_none_or(|items| items.size() < items_bytes)
        {
            // Grown in powers of two, so a draw count that wavers doesn't remake it every frame.
            cull.items = Some(render_device.create_buffer(&BufferDescriptor {
                label: Some("triangle cull items"),
                size: items_bytes.next_power_of_two(),
                usage: BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
        }
        let counts_bytes = plans.len() as u64 * COUNT_BYTES;
        if cull
            .counts
            .as_ref()
            .is_none_or(|counts| counts.size() < counts_bytes)
        {
            cull.counts = Some(render_device.create_buffer(&BufferDescriptor {
                label: Some("triangle cull counts"),
                size: counts_bytes.next_power_of_two(),
                usage: BufferUsages::STORAGE | BufferUsages::INDIRECT | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let (Some(items), Some(counts)) = (&cull.items, &cull.counts) else {
            return;
        };
        // Before this frame's commands, as the queue runs its writes first.
        render_queue.write_buffer(
            counts,
            0,
            bytemuck::cast_slice(&COUNT_RESET.repeat(plans.len())),
        );
        let list_layout = pipeline_cache.get_bind_group_layout(&cull.list_layout);
        let cull_layout = pipeline_cache.get_bind_group_layout(&cull.cull_layout);
        let marks = marks.unwrap_or(&cull.no_marks);
        let mut item_base = 0;
        for &(parameters, gpu_metadata, slots, group, bit, direct) in &plans {
            let group = &cull.groups[group];
            let Some(mirror) = &group.mirror else {
                continue;
            };
            // A phase the cluster cull covered keeps the marked triangles among those the
            // prepass drew, from its mirror; `list` reads the count words. One drawn from the
            // prepass's mirror as it is needs the cluster cull, and only `list`, which sets the
            // draws' counts; without them it's drawn whole.
            let (indices, words, flags) = match (&group.prepass_mirror, &cull.cluster_words) {
                (Some(prepass_mirror), Some(words)) if cluster_phases & bit != 0 => {
                    (prepass_mirror, words, flags | CLUSTERS | cull.stamp << STAMP_SHIFT)
                }
                _ if direct => continue,
                _ => (&group.index, &cull.no_marks, flags),
            };
            let flags = if direct { flags | DIRECT } else { flags };
            let bind_group = render_device.create_bind_group(
                "triangle cull list bind group",
                &list_layout,
                &BindGroupEntries::with_indices((
                    (0, view_binding.clone()),
                    (1, current_input.as_entire_binding()),
                    (2, gpu_metadata.as_entire_binding()),
                    (3, parameters.as_entire_binding()),
                    (8, cull.stats.as_entire_binding()),
                    (9, items.as_entire_binding()),
                    (11, counts.as_entire_binding()),
                    (13, words.as_entire_binding()),
                )),
            );
            let cull_bind_group = (!direct).then(|| {
                render_device.create_bind_group(
                    "triangle cull cull bind group",
                    &cull_layout,
                    &BindGroupEntries::with_indices((
                        (0, view_binding.clone()),
                        (3, parameters.as_entire_binding()),
                        (4, group.vertex.as_entire_binding()),
                        (5, indices.as_entire_binding()),
                        (6, mirror.as_entire_binding()),
                        (7, pyramid),
                        (8, cull.stats.as_entire_binding()),
                        (10, items.as_entire_binding()),
                        (12, marks.as_entire_binding()),
                    )),
                )
            });
            if direct {
                direct_dispatched |= bit;
            }
            let index_words = (group.index.size().min(mirror.size()) / 4).min(u32::MAX.into());
            let count_word = dispatches.len() as u64 * COUNT_BYTES / 4;
            dispatches.push(Dispatch {
                bind_group,
                cull_bind_group,
                push_constants: [
                    slots,
                    index_words as u32,
                    group.stride_words,
                    group.position_words,
                    span,
                    flags,
                    group.slabs,
                    item_base,
                    count_word as u32,
                ],
                workgroups: (slots.div_ceil(LIST_WORKGROUP_SIZE), 1),
            });
            item_base += slots + 1;
        }
    } else if !v2 {
        let layout = pipeline_cache.get_bind_group_layout(&cull.layout);
        for &(parameters, gpu_metadata, slots, group, ..) in &plans {
            let group = &cull.groups[group];
            let Some(mirror) = &group.mirror else {
                continue;
            };
            let per_row = slots.min(WORKGROUPS_PER_ROW);
            let bind_group = render_device.create_bind_group(
                "triangle cull bind group",
                &layout,
                &BindGroupEntries::sequential((
                    view_binding.clone(),
                    current_input.as_entire_binding(),
                    gpu_metadata.as_entire_binding(),
                    parameters.as_entire_binding(),
                    group.vertex.as_entire_binding(),
                    group.index.as_entire_binding(),
                    mirror.as_entire_binding(),
                    pyramid,
                    cull.stats.as_entire_binding(),
                )),
            );
            dispatches.push(Dispatch {
                bind_group,
                cull_bind_group: None,
                push_constants: [
                    slots,
                    per_row,
                    group.stride_words,
                    group.position_words,
                    span,
                    flags,
                    group.slabs,
                    0,
                    0,
                ],
                workgroups: (per_row, slots.div_ceil(per_row)),
            });
        }
    }
    cull.dispatches = dispatches;
    cull.phases = phases;
    cull.cluster_dispatches = cluster_dispatches;
    cull.cluster_phases = cluster_phases;
    cull.direct_phases = direct_dispatched;
    if counting {
        cull.stats_clusters = cluster_phases & PHASE_OPAQUE != 0;
        cull.stats_cutouts = cluster_phases & PHASE_ALPHA_MASK != 0;
    }
    cull.set_cluster_status(cluster_status);
    cull.view = Some(view);
    cull.view_offset = view_offset.offset;
    cull.swap = setting & TRIANGLE_CULL_DEBUG_NO_SWAP == 0;
    cull.set_status(
        match (
            setting & TRIANGLE_CULL_DEBUG_EMPTY != 0,
            cull.swap,
            span == 0,
        ) {
            (true, true, _) => "on, debug: every workgroup returns at once",
            (true, false, _) => {
                "on, debug: every workgroup returns at once, and the draws keep the index slabs"
            }
            (false, false, _) => "on, debug: the draws keep the index slabs",
            (false, true, _) if inverted => {
                "on, debug: keeping the triangles the prepass left unmarked"
            }
            (false, true, _) if opaque_only && direct_dispatched != 0 => {
                "on, keeping the opaque triangles the prepass marked, and drawing the cutouts' \
                 clusters it drew"
            }
            (false, true, _) if opaque_only => {
                "on, keeping the opaque triangles the prepass marked"
            }
            (false, true, _) if marks_mode => "on, keeping the triangles the prepass marked",
            (false, true, true) => "on, without the depth pyramid test",
            (false, true, false) => "on",
        },
    );
}

/// Runs this frame's dispatches in the one view's graph, between the main pass's indirect
/// parameters and the main pass.
#[derive(Default)]
pub struct TriangleCullNode;

impl Node for TriangleCullNode {
    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let Some(cull) = world.get_resource::<TriangleCull>() else {
            return Ok(());
        };
        if cull.view.is_none() || graph.get_view_entity() != cull.view {
            return Ok(());
        }
        // The cluster cull's mirrors and count words are this frame's only once both its passes
        // ran.
        if cull.cluster_phases != 0
            && cull.clusters_state.load(Ordering::Relaxed) != CLUSTERS_EARLY | CLUSTERS_LATE
        {
            return Ok(());
        }
        let clear = cull.debug & TRIANGLE_CULL_DEBUG_CLEAR != 0;
        let counting = cull.stats_state.load(Ordering::Relaxed) == STATS_WANTED;
        let pipelines = if cull.dispatches.is_empty() {
            None
        } else {
            let pipeline_cache = world.resource::<PipelineCache>();
            let pipelines = if cull.v2 {
                cull.pipelines_v2.and_then(|[list, test, keep]| {
                    let test = match (cull.marks_kernel, cull.ballot, cull.coalesced) {
                        (true, true, _) if cull.ballot_8 => cull.pipeline_ballot_8?,
                        (true, true, _) => cull.pipeline_ballot?,
                        (true, false, true) => cull.pipeline_coalesced?,
                        (true, false, false) => keep,
                        (false, ..) => test,
                    };
                    Some((
                        pipeline_cache.get_compute_pipeline(list)?,
                        Some(pipeline_cache.get_compute_pipeline(test)?),
                    ))
                })
            } else {
                cull.pipeline
                    .and_then(|id| pipeline_cache.get_compute_pipeline(id))
                    .map(|pipeline| (pipeline, None))
            };
            let Some(pipelines) = pipelines else {
                return Ok(());
            };
            Some(pipelines)
        };
        if pipelines.is_none() && !clear && !counting {
            return Ok(());
        }

        let diagnostics = render_context.diagnostic_recorder();
        let command_encoder = render_context.command_encoder();
        if clear {
            for mirror in &cull.mirrors {
                command_encoder.clear_buffer(&mirror.buffer, 0, None);
            }
        }
        // The cluster cull's `early` cleared the counts before adding its own.
        if counting && cull.cluster_phases == 0 {
            command_encoder.clear_buffer(&cull.stats, 0, None);
        }
        if let Some((pipeline, test)) = pipelines {
            let mut compute_pass = command_encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("triangle_cull"),
                timestamp_writes: None,
            });
            let pass_span = diagnostics.pass_span(&mut compute_pass, "triangle_cull");
            let bytes = if test.is_some() {
                PUSH_CONSTANT_BYTES_V2 as usize
            } else {
                PUSH_CONSTANT_BYTES as usize
            };
            // The second version's `list` (all of them, then each `cull` or `keep_marked`), or the
            // first's pass.
            compute_pass.set_pipeline(pipeline);
            for dispatch in &cull.dispatches {
                compute_pass.set_bind_group(0, &dispatch.bind_group, &[cull.view_offset]);
                compute_pass.set_push_constants(
                    0,
                    &bytemuck::cast_slice(&dispatch.push_constants)[..bytes],
                );
                compute_pass.dispatch_workgroups(dispatch.workgroups.0, dispatch.workgroups.1, 1);
            }
            if let (Some(test), Some(counts)) = (test, &cull.counts) {
                compute_pass.set_pipeline(test);
                for (index, dispatch) in cull.dispatches.iter().enumerate() {
                    let Some(bind_group) = &dispatch.cull_bind_group else {
                        continue;
                    };
                    compute_pass.set_bind_group(0, bind_group, &[cull.view_offset]);
                    compute_pass.set_push_constants(
                        0,
                        &bytemuck::cast_slice(&dispatch.push_constants)[..bytes],
                    );
                    compute_pass.dispatch_workgroups_indirect(counts, index as u64 * COUNT_BYTES);
                }
            }
            pass_span.end(&mut compute_pass);
        }
        if counting {
            command_encoder.copy_buffer_to_buffer(
                &cull.stats,
                0,
                &cull.stats_readback,
                0,
                STATS_BYTES,
            );
            cull.stats_state.store(STATS_COPIED, Ordering::Relaxed);
        }
        if pipelines.is_some() {
            cull.dispatched.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

/// Runs the depth prepass's cluster cull in the one view's graph: `early` (`LATE` false) between
/// the early prepass's indirect parameters and the early prepass, and `late` between the late
/// prepass's and the late prepass.
#[derive(Default)]
pub struct TriangleCullClusterNode<const LATE: bool>;

impl<const LATE: bool> Node for TriangleCullClusterNode<LATE> {
    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let Some(cull) = world.get_resource::<TriangleCull>() else {
            return Ok(());
        };
        if cull.cluster_dispatches.is_empty()
            || cull.view.is_none()
            || graph.get_view_entity() != cull.view
        {
            return Ok(());
        }
        if LATE && cull.clusters_state.load(Ordering::Relaxed) & CLUSTERS_EARLY == 0 {
            return Ok(());
        }
        // Both or neither, so `early` never runs without `late`.
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipelines = cull.pipelines_clusters[cull.cluster_kernel];
        let Some([early, late]) = pipelines.and_then(|[early, late]| {
            Some([
                pipeline_cache.get_compute_pipeline(early)?,
                pipeline_cache.get_compute_pipeline(late)?,
            ])
        }) else {
            return Ok(());
        };

        let label = if LATE {
            "triangle_cull_clusters_late"
        } else {
            "triangle_cull_clusters_early"
        };
        let diagnostics = render_context.diagnostic_recorder();
        let command_encoder = render_context.command_encoder();
        if !LATE && cull.stats_state.load(Ordering::Relaxed) == STATS_WANTED {
            command_encoder.clear_buffer(&cull.stats, 0, None);
        }
        let mut compute_pass = command_encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        let pass_span = diagnostics.pass_span(&mut compute_pass, label);
        compute_pass.set_pipeline(if LATE { late } else { early });
        for dispatch in &cull.cluster_dispatches {
            compute_pass.set_bind_group(0, &dispatch.bind_group, &[cull.view_offset]);
            compute_pass.set_push_constants(0, bytemuck::cast_slice(&dispatch.push_constants));
            compute_pass.dispatch_workgroups(dispatch.workgroups, 1, 1);
        }
        pass_span.end(&mut compute_pass);
        cull.clusters_state.fetch_or(
            if LATE { CLUSTERS_LATE } else { CLUSTERS_EARLY },
            Ordering::Relaxed,
        );
        Ok(())
    }
}
