//! `SKATE_DIAG_WORLD=1`: every few seconds, logs how far each camera view's visible meshes get
//! through Bevy's draw chain, split into world leaves, sky and the rest. Each count is a
//! condition bevy_pbr's `specialize_material_meshes` and `queue_material_meshes` (material.rs)
//! check before a mesh reaches an opaque bin: a material instance, mesh instance, prepared
//! mesh, mesh on the GPU, prepared material, specialization tick, specialized pipeline and a
//! compiled pipeline. Read-only; it changes nothing it looks at.
//!
//! It also logs every skinned mesh: the skin offset its `MeshInputUniform` was built with
//! against the offset `SkinUniforms` holds now, its mesh ranges against the allocator's, and
//! how many of its joint matrices are identity or zero (bevy_pbr's skin.rs writes identity
//! for a joint it cannot resolve).
use std::time::{Duration, Instant};

use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d};
use bevy::core_pipeline::prepass::Opaque3dPrepass;
use bevy::ecs::entity::EntityHashSet;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{
    EntitySpecializationTicks, MeshInputUniform, MeshUniform, PreparedMaterial,
    RenderMaterialInstances, RenderMeshInstances, SkinUniforms, SpecializedMaterialPipelineCache,
    SpecializedPrepassMaterialPipelineCache,
};
use bevy::render::batching::gpu_preprocessing::BatchedInstanceBuffers;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::mesh::RenderMesh;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{BinnedPhaseItem, BinnedRenderPhase, ViewBinnedRenderPhases};
use bevy::render::render_resource::{CachedPipelineState, CachedRenderPipelineId, PipelineCache};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{ExtractedView, RenderVisibleEntities};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

const PERIOD: Duration = Duration::from_secs(4);

pub(crate) struct WorldDiagPlugin;

impl Plugin for WorldDiagPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("SKATE_DIAG_WORLD").as_deref() != Ok("1") {
            return;
        }
        eprintln!("[Diag] SKATE_DIAG_WORLD: logging the camera views' draw chain every {PERIOD:?}");
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render.init_resource::<Classes>().add_systems(ExtractSchedule, extract_classes).add_systems(
            Render,
            (report, report_skins).after(RenderSystems::QueueSweep).before(RenderSystems::PhaseSort),
        );
    }
}

#[derive(Resource, Default)]
struct Classes {
    due: bool,
    world: EntityHashSet,
    sky: EntityHashSet,
    skins: Vec<SkinRow>,
}

struct SkinRow {
    entity: Entity,
    name: String,
    joints: usize,
    visible: bool,
}

fn extract_classes(
    mut classes: ResMut<Classes>,
    mut next: Local<Option<Instant>>,
    world: Extract<Query<Entity, With<MeshMaterial3d<crate::retail_render::WorldMaterial>>>>,
    sky: Extract<Query<Entity, With<MeshMaterial3d<crate::retail_sky::SkyMaterial>>>>,
    skins: Extract<Query<(Entity, &SkinnedMesh, &ViewVisibility, Option<&ChildOf>)>>,
    names: Extract<Query<&Name>>,
) {
    let now = Instant::now();
    classes.due = next.is_none_or(|at| now >= at);
    if !classes.due {
        return;
    }
    *next = Some(now + PERIOD);
    classes.world.clear();
    classes.world.extend(world.iter());
    classes.sky.clear();
    classes.sky.extend(sky.iter());
    classes.skins.clear();
    for (entity, skin, visibility, parent) in &skins {
        let name = names
            .get(entity)
            .or_else(|_| names.get(parent.map_or(entity, ChildOf::parent)))
            .map_or_else(|_| String::new(), |name| name.as_str().to_owned());
        classes.skins.push(SkinRow {
            entity,
            name,
            joints: skin.joints.len(),
            visible: visibility.get(),
        });
    }
}

/// One line per skinned mesh; see the module comment.
fn report_skins(
    classes: Res<Classes>,
    skins: Option<Res<SkinUniforms>>,
    mesh_instances: Res<RenderMeshInstances>,
    allocator: Res<MeshAllocator>,
    buffers: Option<Res<BatchedInstanceBuffers<MeshUniform, MeshInputUniform>>>,
) {
    let (true, Some(skins), RenderMeshInstances::GpuBuilding(instances)) =
        (classes.due && !classes.skins.is_empty(), skins, &*mesh_instances)
    else {
        return;
    };
    // Two instances sharing one input slot would draw one's data for the other.
    let mut slots = HashMap::<u32, u32>::default();
    for instance in instances.values() {
        *slots.entry(instance.current_uniform_index.get()).or_default() += 1;
    }
    let shared_slots = slots.values().filter(|&&count| count > 1).count();
    info!(
        "[Diag] skins: {} skinned meshes, {} skins allocated, staging {} joints, buffer {} bytes; \
         {} mesh instances, {shared_slots} input slots shared",
        classes.skins.len(),
        skins.all_skins().count(),
        skins.current_staging_buffer.len(),
        skins.current_buffer.size(),
        instances.len(),
    );
    for row in &classes.skins {
        let main = MainEntity::from(row.entity);
        let offset = skins.skin_index(main);
        let instance = instances.get(&main);
        let uniform = instance.zip(buffers.as_deref()).and_then(|(instance, buffers)| {
            buffers.current_input_buffer.get(instance.current_uniform_index.get())
        });
        let slices = instance.map(|instance| {
            let mesh = &instance.shared.mesh_asset_id;
            (
                allocator.mesh_vertex_slice(mesh).map(|slice| slice.range),
                allocator.mesh_index_slice(mesh).map(|slice| slice.range),
            )
        });
        let (mut identity, mut zero, mut nonfinite, mut other) = (0u32, 0u32, 0u32, 0u32);
        let mut first = None;
        if let Some(offset) = offset {
            for matrix in skins.current_staging_buffer.iter().skip(offset as usize).take(row.joints) {
                match matrix {
                    m if !m.is_finite() => nonfinite += 1,
                    m if *m == Mat4::IDENTITY => identity += 1,
                    m if *m == Mat4::ZERO => zero += 1,
                    _ => other += 1,
                }
                first.get_or_insert(matrix.w_axis.truncate());
            }
        }
        let built = uniform.map(|uniform| {
            (
                uniform.current_skin_index,
                uniform.first_vertex_index,
                uniform.first_index_index,
                uniform.index_count,
            )
        });
        let skin_match = match (built, offset) {
            (Some((built, ..)), Some(offset)) if built == offset => "ok",
            (Some((u32::MAX, ..)), None) => "ok (none)",
            (None, _) => "no uniform",
            _ => "STALE",
        };
        let slot = instance.map(|instance| instance.current_uniform_index.get());
        let sharers = slot.and_then(|slot| slots.get(&slot)).copied().unwrap_or(0);
        info!(
            "[Diag] skin {:?} '{}' visible {} joints {}: input slot {:?} (used by {sharers}), \
             skin offset now {:?}, built (skin, first vertex, first index, count) {:?} -> \
             {skin_match}; allocator vertex {:?} index {:?}; joints identity {identity} zero \
             {zero} nonfinite {nonfinite} other {other}, joint 0 at {:?}",
            row.entity,
            row.name,
            row.visible,
            row.joints,
            slot,
            offset,
            built,
            slices.as_ref().map(|slices| slices.0.clone()),
            slices.as_ref().map(|slices| slices.1.clone()),
            first,
        );
    }
}

#[derive(Default)]
struct Chain {
    visible: u32,
    material: u32,
    instance: u32,
    mesh: u32,
    on_gpu: u32,
    prepared: u32,
    tick: u32,
    specialized: u32,
    ready: u32,
    failed: u32,
    binned: u32,
    prepass_specialized: u32,
    prepass_ready: u32,
    prepass_binned: u32,
    /// The entity's `MeshInputUniform` holds its mesh's current allocator slices.
    ranges_ok: u32,
    /// Its `MeshInputUniform` draws nothing: zero indices, as when the mesh had no slice yet.
    ranges_zero: u32,
    /// Its `MeshInputUniform` holds other, nonzero ranges.
    ranges_stale: u32,
}

impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "visible {} material {} instance {} mesh {} on_gpu {} prepared {} tick {} \
             specialized {} ready {} failed {} binned {} | prepass specialized {} ready {} binned {} \
             | gpu ranges ok {} zero {} stale {}",
            self.visible,
            self.material,
            self.instance,
            self.mesh,
            self.on_gpu,
            self.prepared,
            self.tick,
            self.specialized,
            self.ready,
            self.failed,
            self.binned,
            self.prepass_specialized,
            self.prepass_ready,
            self.prepass_binned,
            self.ranges_ok,
            self.ranges_zero,
            self.ranges_stale,
        )
    }
}

/// Compares the `MeshInputUniform` the GPU preprocessing reads for `main` with the slices the
/// mesh allocator holds for its mesh now.
fn ranges(
    chain: &mut Chain,
    main: MainEntity,
    mesh_instances: &RenderMeshInstances,
    allocator: &MeshAllocator,
    buffers: Option<&BatchedInstanceBuffers<MeshUniform, MeshInputUniform>>,
) {
    let (RenderMeshInstances::GpuBuilding(instances), Some(buffers)) = (mesh_instances, buffers)
    else {
        return;
    };
    let Some(instance) = instances.get(&main) else {
        return;
    };
    let Some(uniform) = buffers.current_input_buffer.get(instance.current_uniform_index.get())
    else {
        return;
    };
    if uniform.index_count == 0 {
        chain.ranges_zero += 1;
        return;
    }
    let mesh = instance.shared.mesh_asset_id;
    let vertex = allocator.mesh_vertex_slice(&mesh).map(|slice| slice.range);
    let matches = match (vertex, allocator.mesh_index_slice(&mesh).map(|slice| slice.range)) {
        (Some(vertex), Some(index)) => {
            uniform.first_vertex_index == vertex.start
                && uniform.first_index_index == index.start
                && uniform.index_count == index.end - index.start
        }
        (Some(vertex), None) => {
            uniform.first_vertex_index == vertex.start
                && uniform.index_count == vertex.end - vertex.start
        }
        _ => false,
    };
    if matches {
        chain.ranges_ok += 1;
    } else {
        chain.ranges_stale += 1;
    }
}

fn binned<BPI: BinnedPhaseItem>(phase: &BinnedRenderPhase<BPI>, into: &mut HashSet<MainEntity>) {
    for bins in phase.multidrawable_meshes.values() {
        for bin in bins.values() {
            into.extend(bin.entities().keys().copied());
        }
    }
    for bin in phase.batchable_meshes.values() {
        into.extend(bin.entities().keys().copied());
    }
    for unbatchable in phase.unbatchable_meshes.values() {
        into.extend(unbatchable.entities.keys().copied());
    }
}

/// 0 queued or compiling, 1 compiled, 2 failed.
fn pipeline_state(
    pipelines: &PipelineCache,
    id: CachedRenderPipelineId,
    errors: &mut u32,
) -> u8 {
    match pipelines.get_render_pipeline_state(id) {
        CachedPipelineState::Ok(_) => 1,
        CachedPipelineState::Err(error) => {
            if *errors < 4 {
                *errors += 1;
                warn!("[Diag] pipeline {id:?} failed: {error}");
            }
            2
        }
        _ => 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn report(
    classes: Res<Classes>,
    views: Query<(&ExtractedView, &RenderVisibleEntities)>,
    (material_instances, mesh_instances, render_meshes, allocator, materials, ticks): (
        Res<RenderMaterialInstances>,
        Res<RenderMeshInstances>,
        Res<RenderAssets<RenderMesh>>,
        Res<MeshAllocator>,
        Res<ErasedRenderAssets<PreparedMaterial>>,
        Res<EntitySpecializationTicks>,
    ),
    (specialized, prepass_specialized, pipelines): (
        Res<SpecializedMaterialPipelineCache>,
        Option<Res<SpecializedPrepassMaterialPipelineCache>>,
        Res<PipelineCache>,
    ),
    (opaque, alpha_mask, prepass, buffers): (
        Res<ViewBinnedRenderPhases<Opaque3d>>,
        Res<ViewBinnedRenderPhases<AlphaMask3d>>,
        Option<Res<ViewBinnedRenderPhases<Opaque3dPrepass>>>,
        Option<Res<BatchedInstanceBuffers<MeshUniform, MeshInputUniform>>>,
    ),
    mut errors: Local<u32>,
) {
    if !classes.due {
        return;
    }
    let world_type = std::any::TypeId::of::<crate::retail_render::WorldMaterial>();
    let sky_type = std::any::TypeId::of::<crate::retail_sky::SkyMaterial>();
    let (mut prepared_world, mut prepared_sky, mut prepared_other) = (0u32, 0u32, 0u32);
    for (id, _) in materials.iter() {
        match id.type_id() {
            t if t == world_type => prepared_world += 1,
            t if t == sky_type => prepared_sky += 1,
            _ => prepared_other += 1,
        }
    }
    let world_instances = material_instances
        .instances
        .keys()
        .filter(|entity| classes.world.contains(&entity.id()))
        .count();
    info!(
        "[Diag] entities: world {} sky {}; material instances {} (world {world_instances}); \
         prepared materials world {prepared_world} sky {prepared_sky} other {prepared_other}; \
         specialization ticks {}",
        classes.world.len(),
        classes.sky.len(),
        material_instances.instances.len(),
        ticks.len(),
    );

    for (view, visible) in &views {
        let retained = view.retained_view_entity;
        let Some(opaque_phase) = opaque.get(&retained) else {
            continue;
        };
        let mut in_bins = HashSet::default();
        binned(opaque_phase, &mut in_bins);
        if let Some(phase) = alpha_mask.get(&retained) {
            binned(phase, &mut in_bins);
        }
        let mut in_prepass_bins = HashSet::default();
        if let Some(phase) = prepass.as_ref().and_then(|phases| phases.get(&retained)) {
            binned(phase, &mut in_prepass_bins);
        }
        let view_cache = specialized.get(&retained);
        let prepass_cache = prepass_specialized.as_ref().and_then(|cache| cache.get(&retained));

        let mut chains: [Chain; 3] = Default::default();
        for (_, main) in visible.iter::<Mesh3d>() {
            let chain = &mut chains[if classes.world.contains(&main.id()) {
                0
            } else if classes.sky.contains(&main.id()) {
                1
            } else {
                2
            }];
            chain.visible += 1;
            let instance = material_instances.instances.get(main);
            chain.material += u32::from(instance.is_some());
            let queue_data = mesh_instances.render_mesh_queue_data(*main);
            chain.instance += u32::from(queue_data.is_some());
            if let Some(data) = &queue_data {
                chain.mesh += u32::from(render_meshes.get(data.mesh_asset_id).is_some());
                chain.on_gpu += u32::from(allocator.mesh_vertex_slice(&data.mesh_asset_id).is_some());
            }
            if let Some(instance) = instance {
                chain.prepared += u32::from(materials.get(instance.asset_id).is_some());
            }
            chain.tick += u32::from(ticks.contains_key(main));
            if let Some((_, id)) = view_cache.and_then(|cache| cache.get(main)) {
                chain.specialized += 1;
                match pipeline_state(&pipelines, *id, &mut errors) {
                    1 => chain.ready += 1,
                    2 => chain.failed += 1,
                    _ => {}
                }
            }
            chain.binned += u32::from(in_bins.contains(main));
            if let Some((_, id)) = prepass_cache.and_then(|cache| cache.get(main)) {
                chain.prepass_specialized += 1;
                chain.prepass_ready += u32::from(pipeline_state(&pipelines, *id, &mut errors) == 1);
            }
            chain.prepass_binned += u32::from(in_prepass_bins.contains(main));
            ranges(chain, *main, &mesh_instances, &allocator, buffers.as_deref());
        }
        info!(
            "[Diag] view {retained:?} (opaque bins {} entities, prepass {}): world: {} ; sky: {} ; other: {}",
            in_bins.len(),
            in_prepass_bins.len(),
            chains[0],
            chains[1],
            chains[2],
        );
    }
}
