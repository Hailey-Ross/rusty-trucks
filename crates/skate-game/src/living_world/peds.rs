//! Pedestrians, body tier (doc 26, peds milestone M2): turns the population's pedestrian spawn
//! / despawn records into visible, animated peds with footstep audio.
//!
//! - **Look** ([`Pedestrian`]): the entity inside the census category and its model from the
//!   spawn record's seed (`skate_core::living_world::peds::choice`, retail `sub_826B8B88` /
//!   `sub_826BB058`), the model's tint pair (`sub_827B4170`; kept on the component, not drawn
//!   yet: the shader mask they modulate is not decoded). GLB `private/living_world/models/
//!   <recipe>.glb` (both LODs, parts, textures), or a mod's GLB from [`PedLooks`].
//! - **Animation** ([`PedBody`]): the ped animation player on the clips of
//!   `PedestrianSkeletonPres.abin`, stepped once per population world tick (1/60 s, `clock::RETAIL_TICK_HZ`) so a ped's state is a
//!   function of its spawn record and the population tick. Root motion moves the ped; it is
//!   snapped to the ground below (a line query, like the audio's ground material probe).
//!   Without navigation (M3) a ped follows [`TestPath`]: idle, start, walk a few metres straight,
//!   stop, idle, turn round, repeat (placeholder).
//! - **Skinning**: the GLB's 39 joints bind to the 50-bone rig by name (all match, data test);
//!   bones the clips do not carry (fingers, face) follow their rig parent with the GLB's bind
//!   offset.
//! - **LOD** (placeholder until `sub_827C1188` is read): `LOD0` within the model's first
//!   distance pair (45 m [data]), `LOD1` beyond its second (55 m), hysteresis between.
//! - **Audio**: [`PedAudio`](crate::world_audio::PedAudio) with the model's voice, the clip's
//!   `LEFTTOEDOWN` / `RIGHTTOEDOWN` windows as `feet_down` and `BODYFALLTYPE` as `body_fall`, so
//!   #32's ped footsteps and body falls play.
//! - **Events** for engine systems and the planned `sdk.living_world`: [`PedEvent`].
//!
//! Moddability: [`PedLooks`] (category entity lists, entity -> model / animation set, recipe ->
//! GLB path) is the one place overrides go; restoring `PedLooks::default()` undoes a mod (new
//! spawns use retail looks again). Multiplayer: nothing here decides a spawn; a client rebuilds
//! the same ped from the same `SpawnRecord` and tick.

use super::{LivingWorldDespawn, LivingWorldSettings, LivingWorldSpawn, PopulationState};
use crate::world_audio::PedAudio;
use bevy::prelude::*;
use skate_core::living_world::peds::anim::{PedClip, PedClips, TestPath};
use skate_core::living_world::peds::{Locomotion, PedAnimPlayer, PedCatalog, PedEvaluator, PedOverrides, PedRig};
use skate_core::living_world::{Decision, DespawnReason, Kind, LivingWorldId, SpawnChoice};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Seconds per population tick: the ped player steps once per world tick (retail's 60 Hz world
/// step, `clock::RETAIL_TICK_HZ`), so a ped's state is a function of its spawn record and the tick.
pub(crate) fn tick_seconds(hz: f64) -> f32 {
    (1.0 / hz.max(1.0)) as f32
}

/// Loaded ped data for the current world.
#[derive(Resource, Default, Clone)]
pub(crate) struct PedData {
    pub catalog: Arc<PedCatalog>,
    pub anim_sets: Arc<BTreeMap<String, skate_core::living_world::peds::PedAnimSet>>,
    pub rig: Arc<PedRig>,
    pub clips: Arc<BTreeMap<String, PedClip>>,
    pub status: String,
    loaded_for: Option<(String, u64)>,
}

impl PedData {
    /// The animation set to play: the entity's, or `default` when the set's idle clips are not
    /// in the bank (the `granny` set names `GRAN_WNDR_*` clips that no shipped bank holds [data]).
    pub(crate) fn playable_set(&self, name: &str) -> Option<(&String, &skate_core::living_world::peds::PedAnimSet)> {
        let playable = |set: &skate_core::living_world::peds::PedAnimSet| {
            set.entries.get(skate_core::living_world::peds::anim::names::IDLE).is_some_and(|l| !l.is_empty() && l.iter().all(|c| self.clips.contains_key(&c.clip)))
        };
        self.anim_sets.get_key_value(name).filter(|(_, s)| playable(s)).or_else(|| self.anim_sets.get_key_value("default").filter(|(_, s)| playable(s)))
    }

    pub(crate) fn ready(&self) -> bool {
        !self.rig.names.is_empty() && !self.catalog.categories.is_empty()
    }

    /// Read the tables and the bank, decode every clip an animation set names.
    pub(crate) fn load(asset_root: &std::path::Path) -> Self {
        let tables = std::fs::read(asset_root.join("private/living_world/tables.json")).map_err(|e| e.to_string()).and_then(|b| skate_data::ped_anim::PedTables::parse(&b));
        let bank = skate_data::ped_anim::PedBank::load(asset_root);
        match (tables, bank) {
            (Ok(t), Ok(bank)) => {
                let mut clips = BTreeMap::new();
                let mut failed = 0;
                for set in t.anim_sets.values() {
                    for c in set.entries.values().flatten() {
                        if !clips.contains_key(&c.clip) {
                            match bank.clip(&c.clip) {
                                Ok(clip) => {
                                    clips.insert(c.clip.clone(), clip);
                                }
                                Err(_) => failed += 1,
                            }
                        }
                    }
                }
                let status = format!("peds: {} categories, {} sets, {} clips ({} missing, {} unresolved remaps)", t.catalog.categories.len(), t.anim_sets.len(), clips.len(), failed, t.unresolved);
                Self { catalog: Arc::new(t.catalog), anim_sets: Arc::new(t.anim_sets), rig: Arc::new(bank.rig), clips: Arc::new(clips), status, loaded_for: None }
            }
            (t, b) => Self { status: format!("peds: no body data ({})", [t.err(), b.err()].into_iter().flatten().collect::<Vec<_>>().join("; ")), ..Self::default() },
        }
    }
}

/// Mod / engine look overrides. Default = retail.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct PedLooks {
    pub overrides: PedOverrides,
    /// recipe -> GLB asset path (with the ped bone names).
    pub glb: BTreeMap<String, String>,
}

/// One ped's identity (from its spawn record).
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct Pedestrian {
    pub id: LivingWorldId,
    pub census: String,
    pub category: String,
    pub entity: String,
    pub model: String,
    pub recipe: String,
    pub anim_set: String,
    pub seed: u64,
    pub spawn_tick: u64,
    pub tint_a: [f32; 4],
    pub tint_b: [f32; 4],
}

/// The simulated body: animation player, placeholder path, position and heading.
#[derive(Component, Clone, Debug)]
pub(crate) struct PedBody {
    pub player: PedAnimPlayer,
    pub path: TestPath,
    pub position: Vec3,
    pub heading: f32,
    /// Console ticks stepped since the spawn.
    pub ticks: u64,
    pub feet_down: [bool; 2],
    pub body_fall: f32,
}

/// LivingWorldId -> entity.
#[derive(Resource, Default)]
pub(crate) struct PedIndex(pub BTreeMap<LivingWorldId, Entity>);

/// What happened to a ped (engine systems, mods).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) enum PedEvent {
    Spawned { id: LivingWorldId, entity: String, recipe: String },
    /// The look could not be resolved (no entities / model): the population slot is released.
    Rejected { id: LivingWorldId, category: String },
    Despawned { id: LivingWorldId, reason: DespawnReason },
    State { id: LivingWorldId, state: Locomotion },
}

impl PedClips for PedData {
    fn clip(&self, name: &str) -> Option<&PedClip> {
        self.clips.get(name)
    }
}

/// Ground height below / near a point (a 9 m line from 3 m above), `None` when nothing is hit.
fn ground(physics: Option<&crate::physics::GamePhysics>, at: Vec3) -> Option<f32> {
    use skate_core::math::Vector3;
    let p = physics?;
    match p.world().query_thin_line(Vector3::new(at.x, at.y + 3.0, at.z), Vector3::new(at.x, at.y - 6.0, at.z)) {
        Ok(Some(hit)) => Some(hit.geometry.position.y),
        _ => None,
    }
}

fn load_ped_data(config: Option<Res<crate::config::Config>>, map: Option<Res<crate::map_transition::CurrentMap>>, mut data: ResMut<PedData>, audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>, settings: Res<LivingWorldSettings>) {
    let (Some(config), Some(map)) = (config, map) else { return };
    let key = (map.name.clone(), map.generation);
    if data.loaded_for.as_ref() == Some(&key) {
        return;
    }
    let mut loaded = PedData::load(&config.asset_root);
    info!("LIVING_WORLD {}", loaded.status);
    loaded.loaded_for = Some(key);
    if let Some(mut audio) = audio {
        audio.expected |= settings.enabled && settings.pedestrians.enabled && loaded.ready();
    }
    *data = loaded;
}

/// Spawn and despawn ped entities from the population's records.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_ped_records(
    mut commands: Commands,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    data: Res<PedData>,
    looks: Res<PedLooks>,
    mut index: ResMut<PedIndex>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mut rejected: ResMut<PedRejected>,
    mut events: MessageWriter<PedEvent>,
) {
    for LivingWorldDespawn(r) in despawns.read() {
        if r.id.kind != Kind::Pedestrian {
            continue;
        }
        if let Some(e) = index.0.remove(&r.id) {
            commands.entity(e).despawn();
            events.write(PedEvent::Despawned { id: r.id, reason: r.reason });
        }
    }
    for LivingWorldSpawn(s) in spawns.read() {
        if s.id.kind != Kind::Pedestrian || index.0.contains_key(&s.id) {
            continue;
        }
        let SpawnChoice::Census { record, category } = &s.choice else { continue };
        if !data.ready() {
            continue; // no body data: the population still runs (audio-less, invisible)
        }
        let look = data.catalog.choose(category, s.seed, &looks.overrides);
        let (Some(mut look), Some(set)) = (look.clone(), look.as_ref().and_then(|l| data.playable_set(&l.anim_set))) else {
            events.write(PedEvent::Rejected { id: s.id, category: category.clone() });
            rejected.0.push(s.id);
            continue;
        };
        look.anim_set = set.0.clone();
        let Some(player) = PedAnimPlayer::new(set.1, s.seed) else { continue };
        let mut at = Vec3::from_array(s.position);
        if let Some(y) = ground(physics.as_deref(), at) {
            at.y = y;
        }
        let ped = Pedestrian {
            id: s.id,
            census: record.clone(),
            category: category.clone(),
            entity: look.entity.clone(),
            model: look.model.clone(),
            recipe: look.recipe.clone(),
            anim_set: look.anim_set.clone(),
            seed: s.seed,
            spawn_tick: s.tick,
            tint_a: look.tint_a,
            tint_b: look.tint_b,
        };
        let body = PedBody { player, path: TestPath::new(s.seed), position: at, heading: s.heading, ticks: 0, feet_down: [false; 2], body_fall: 0.0 };
        let e = commands
            .spawn((
                Name::new(format!("Pedestrian {} ({})", s.id.serial, look.recipe)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(s.heading)),
                Visibility::Inherited,
                PedAudio { voice: look.voice, ..Default::default() },
                body,
                ped,
            ))
            .id();
        index.0.insert(s.id, e);
        events.write(PedEvent::Spawned { id: s.id, entity: look.entity, recipe: look.recipe });
    }
}

/// Spawn records whose look did not resolve (retail: no spawn): released next.
#[derive(Resource, Default)]
pub(crate) struct PedRejected(pub Vec<LivingWorldId>);

/// Give the population slot of a rejected spawn back (a host decision; a client waits for it).
pub(crate) fn release_rejected(mut rejected: ResMut<PedRejected>, mut state: ResMut<PopulationState>, settings: Res<LivingWorldSettings>, mut out: MessageWriter<LivingWorldDespawn>) {
    for id in rejected.0.drain(..) {
        if settings.net_role == super::NetRole::Client {
            continue;
        }
        if let Some(Decision::Despawn(r)) = state.world.despawn(id, DespawnReason::External) {
            state.despawned += 1;
            out.write(LivingWorldDespawn(r));
        }
    }
}

/// Step every ped to the population tick: animation, root motion, ground, audio, position.
pub(crate) fn advance_peds(
    mut state: ResMut<PopulationState>,
    data: Res<PedData>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mut peds: Query<(&Pedestrian, &mut PedBody, &mut Transform, &mut PedAudio)>,
    mut events: MessageWriter<PedEvent>,
) {
    let tick = state.world.tick();
    let dt = tick_seconds(state.world.clock().hz);
    let mut list: Vec<_> = peds.iter_mut().collect();
    list.sort_by_key(|(p, ..)| p.id);
    for (ped, mut body, mut transform, mut audio) in list {
        let Some(set) = data.anim_sets.get(&ped.anim_set).or_else(|| data.anim_sets.get("default")) else { continue };
        let target = tick.saturating_sub(ped.spawn_tick);
        let body = &mut *body;
        while body.ticks < target {
            body.player.intent = body.path.intent(dt, body.player.state);
            let out = body.player.step(dt, set, &*data);
            let rotation = Quat::from_rotation_y(body.heading);
            body.position += rotation * Vec3::from_array(out.root.translation);
            body.heading += out.root.yaw;
            body.feet_down = out.feet_down;
            body.body_fall = out.body_fall;
            body.ticks += 1;
            if let Some(s) = out.entered {
                events.write(PedEvent::State { id: ped.id, state: s });
            }
        }
        if let Some(y) = ground(physics.as_deref(), body.position) {
            body.position.y = y;
        }
        transform.translation = body.position;
        transform.rotation = Quat::from_rotation_y(body.heading);
        audio.feet_down = body.feet_down;
        audio.body_fall = body.body_fall;
        state.world.update_position(ped.id, body.position.to_array());
    }
}

/// The look of one ped (render side).
#[derive(Component, Default)]
pub(crate) struct PedPuppet {
    scene: Option<Entity>,
    bindings: Option<crate::animation::AnimationStatus>,
    /// Rig bones without clip data: (bone, rig parent, bind offset from the parent, native space).
    followers: Vec<(usize, usize, Mat4)>,
    lods: Vec<(Entity, u8)>,
    lod: u8,
}

fn render_basis() -> Mat4 {
    Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::W)
}

/// GLB path of a ped look (mod override first).
pub(crate) fn glb_path(looks: &PedLooks, recipe: &str) -> String {
    looks.glb.get(recipe).cloned().unwrap_or_else(|| format!("private/living_world/models/{recipe}.glb"))
}

/// Load the GLB and bind its joints to the ped rig by name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_ped_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    looks: Res<PedLooks>,
    data: Res<PedData>,
    mut peds: Query<(Entity, &Pedestrian, Option<&mut PedPuppet>)>,
    skins: Query<(Entity, &bevy::mesh::skinning::SkinnedMesh)>,
    nodes: Query<(&Name, &Transform)>,
    named: Query<(Entity, &Name)>,
    parents: Query<&ChildOf>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
    bindposes: Res<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    for (e, ped, puppet) in &mut peds {
        let Some(mut puppet) = puppet else {
            let path = glb_path(&looks, &ped.recipe);
            let scene = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))), Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            commands.entity(e).insert(PedPuppet { scene: Some(scene), ..Default::default() });
            continue;
        };
        let Some(scene) = puppet.scene else { continue };
        if puppet.bindings.is_some() || !instances.get(scene).is_ok_and(|i| spawner.instance_is_ready(**i)) {
            continue;
        }
        match crate::animation::AnimationStatus::for_scene(scene, &data.rig.names, &skins, &nodes, &parents) {
            Ok(b) => {
                // Bind globals (native space) per rig bone from the skin's inverse bind matrices.
                let mut bind: BTreeMap<usize, Mat4> = BTreeMap::new();
                for (mesh, skin) in &skins {
                    if !parents.iter_ancestors(mesh).any(|p| p == scene) {
                        continue;
                    }
                    commands.entity(mesh).insert(bevy::camera::visibility::NoFrustumCulling);
                    let Some(ibms) = bindposes.get(&skin.inverse_bindposes) else { continue };
                    for (joint, ibm) in skin.joints.iter().zip(ibms.iter()) {
                        let Ok((name, _)) = nodes.get(*joint) else { continue };
                        if let Some(i) = data.rig.names.iter().position(|n| n.eq_ignore_ascii_case(name.as_str())) {
                            bind.insert(i, ibm.inverse() * render_basis().inverse());
                        }
                    }
                }
                puppet.followers = follower_offsets(&data.rig, &bind);
                puppet.lods = named
                    .iter()
                    .filter(|(n, name)| (name.as_str() == "LOD0" || name.as_str() == "LOD1") && parents.iter_ancestors(*n).any(|p| p == scene))
                    .map(|(n, name)| (n, if name.as_str() == "LOD0" { 0 } else { 1 }))
                    .collect();
                commands.entity(scene).insert(Visibility::Inherited);
                puppet.bindings = Some(b);
            }
            Err(err) => {
                warn!("Ped look rejected ({}): {err}", ped.recipe);
                commands.entity(scene).despawn();
                puppet.scene = None;
            }
        }
    }
}

/// Bones the clips do not carry follow their rig parent with the bind offset between them.
pub(crate) fn follower_offsets(rig: &PedRig, bind: &BTreeMap<usize, Mat4>) -> Vec<(usize, usize, Mat4)> {
    (0..rig.names.len())
        .filter(|&i| !rig.animated.get(i).copied().unwrap_or(false))
        .filter_map(|i| {
            let p = usize::try_from(*rig.parents.get(i)?).ok()?;
            let (bp, bi) = (bind.get(&p)?, bind.get(&i)?);
            Some((i, p, bp.inverse() * *bi))
        })
        .collect()
}

/// Model-space bone matrices for a ped pose (column convention, like `puppet_pose`).
pub(crate) fn ped_globals(rig: &PedRig, body: &PedBody, clips: &dyn PedClips, ahead: f32, followers: &[(usize, usize, Mat4)]) -> Option<Vec<Mat4>> {
    let locals = body.player.pose(rig, clips, ahead)?;
    let mut g: Vec<Mat4> = PedEvaluator::globals(rig, &locals).into_iter().map(crate::animation::native_matrix).collect();
    for &(i, p, offset) in followers {
        if p < g.len() && i < g.len() {
            g[i] = g[p] * offset;
        }
    }
    Some(g)
}

/// LOD placeholder: LOD0 within the model's first distance (45 m), LOD1 beyond its second
/// (55 m), hysteresis between [data pair `Hash_73B6874C7B46C7C6`, meaning unconfirmed].
pub(crate) fn lod_for(distance: f32, current: u8, near: [f32; 2]) -> u8 {
    if distance < near[0] {
        0
    } else if distance > near[1] {
        1
    } else {
        current
    }
}

/// Place the root and pose the skeleton between world ticks.
pub(crate) fn present_ped_pose(
    data: Res<PedData>,
    state: Res<PopulationState>,
    fixed: Res<Time<Fixed>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut peds: Query<(&Pedestrian, &PedBody, &mut PedPuppet)>,
    mut joints: Query<&mut Transform, Without<PedBody>>,
    mut vis: Query<&mut Visibility>,
) {
    let hz = state.world.clock().hz;
    let ahead = ((state.world.clock().overstep() + fixed.overstep_fraction() as f64 * fixed.timestep().as_secs_f64() * hz).clamp(0.0, 1.0) as f32) * tick_seconds(hz);
    let camera = cameras.iter().next().map(|c| c.translation());
    for (ped, body, mut puppet) in &mut peds {
        let Some(bindings) = puppet.bindings.as_ref() else { continue };
        let Some(globals) = ped_globals(&data.rig, body, &*data, ahead, &puppet.followers) else { continue };
        for (joint, local) in bindings.pose_transforms(&globals) {
            if let Ok(mut t) = joints.get_mut(joint) {
                *t = local;
            }
        }
        if let Some(cam) = camera {
            let near = data.catalog.models.get(&ped.model).and_then(|m| m.lod_near).unwrap_or([45.0, 55.0]);
            let lod = lod_for(cam.distance(body.position), puppet.lod, near);
            if lod != puppet.lod || puppet.lod == 0 {
                puppet.lod = lod;
                for &(node, level) in &puppet.lods {
                    if let Ok(mut v) = vis.get_mut(node) {
                        *v = if level == lod { Visibility::Inherited } else { Visibility::Hidden };
                    }
                }
            }
        }
    }
}

/// One-line ped summary for the debug readout.
pub(crate) fn ped_readout(peds: &[(LivingWorldId, String, Locomotion, Vec3)], player: Option<Vec3>) -> String {
    let nearest = player.and_then(|p| peds.iter().map(|x| (x, x.3.distance(p))).min_by(|a, b| a.1.total_cmp(&b.1)));
    match nearest {
        Some(((id, recipe, s, _), d)) => format!("peds {} nearest #{} {recipe} {d:.0} m {}", peds.len(), id.serial, s.name()),
        None => format!("peds {}", peds.len()),
    }
}

fn log_ped_readout(settings: Res<LivingWorldSettings>, state: Res<PopulationState>, observers: Res<super::LivingWorldObservers>, peds: Query<(&Pedestrian, &PedBody)>, mut last: Local<u64>) {
    if !settings.debug || state.world.tick() < *last + 150 {
        return;
    }
    *last = state.world.tick();
    let list: Vec<_> = peds.iter().map(|(p, b)| (p.id, p.recipe.clone(), b.player.state, b.position)).collect();
    info!("LIVING_WORLD {}", ped_readout(&list, observers.observers.first().map(|o| Vec3::from_array(o.position))));
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<PedData>()
        .init_resource::<PedLooks>()
        .init_resource::<PedIndex>()
        .init_resource::<PedRejected>()
        .add_message::<PedEvent>()
        .add_systems(FixedUpdate, (load_ped_data, apply_ped_records, release_rejected, advance_peds, log_ped_readout).chain().after(super::step_population))
        .add_systems(Update, (present_ped_looks, present_ped_pose).chain().after(crate::app::FrameSet::Animation));
}
