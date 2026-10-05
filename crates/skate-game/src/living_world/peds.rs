//! Pedestrians, body tier (doc 26, peds milestone M2): turns the population's pedestrian spawn
//! / despawn records into visible, animated peds with footstep audio.
//!
//! - **Look** ([`Pedestrian`]): the entity inside the census category and its model from the
//!   spawn record's seed (`skate_core::living_world::peds::choice`, retail `sub_826B8B88` /
//!   `sub_826BB058`), the model's tint pair (`sub_827B4170`), painted onto the body's mask
//!   texels by [`present_ped_tints`] (retail's ped shader rule, `peds::colorize`). GLB `private/living_world/models/
//!   <recipe>.glb` (both LODs, parts, textures), or a mod's GLB from [`PedLooks`].
//! - **Animation** ([`PedBody`]): the ped animation player on the clips of
//!   `PedestrianSkeletonPres.abin`, stepped once per population world tick (1/60 s, `clock::RETAIL_TICK_HZ`) so a ped's state is a
//!   function of its spawn record and the population tick. Root motion moves the ped; it is
//!   snapped to the ground below (a line query, like the audio's ground material probe).
//!   The intent comes from navigation (M3, below); on a map without a navmesh a ped follows
//!   [`TestPath`] (idle, walk a few metres, stop, turn round).
//! - **Navigation** (milestone M3): the district's NavPower navmesh (`private/living_world/
//!   navmesh.bin`, [`PedData::nav`]) and retail's `NoRoadWander` goal
//!   (`skate_core::living_world::peds::wander`: probe fans 40 m / 10 m, A* + funnel corners,
//!   re-target on arrival), ped-to-ped avoidance and separation, every step kept on walkable
//!   polygons. Retail ambient peds never use crosswalks (the road branch of `Pedestrian.xml` is
//!   unreachable in TU3); [`PedNavSettings::crosswalk`] = `WalkSignal` is a mod option that waits
//!   for the walk light of the shared traffic signal clock.
//! - **Dynamic obstacles** (fix 11, retail DynamicObject NavPower obstacles, see
//!   `skate_core::living_world::peds::obstacles`): [`PedObstacles`] holds every prop (the
//!   physics' prop bodies at their current pose, so a prop the player moved counts where it lies)
//!   and every mod body. At rest they are cut out of the walkable area (re-cut after moving more
//!   than a quarter of the smallest half extent, no cut while moving faster than 0.4 m/s or
//!   carried); targets inside a cut do not fit, paths bend round cuts, and a body is never stepped
//!   into ([`NavObstacles::resolve_step`]). Rules: [`LivingWorldSettings::ped_obstacles`]
//!   (`sdk.world.set_tuning("living_world", {ped_obstacles = {...}})`).
//! - **Skinning**: the GLB's 39 joints bind to the 50-bone rig by name (all match, data test);
//!   bones the clips do not carry (fingers, face) follow their rig parent with the GLB's bind
//!   offset. The skin matrix is `bone global x inverse(GLB bind)` with NO extra bone-local
//!   basis: the ped GLBs keep the retail model's bind frames, which are the rig's reference
//!   frames (data test `ped_glb_bind_frames_are_the_rig_reference_frames`), so the reference pose
//!   skins to the bind mesh. The skater's `render_basis` (its GLBs bake a matching basis into the
//!   bind) twisted every ped bone 90 degrees about its own axis (torso -90, legs +90 at the
//!   hips): the pinched waist / warped peds of fix10. [`ped_bone_basis`] picks the basis per
//!   model from its bind frames, so a mod GLB written the skater way still works.
//! - **Draw fade** (retail `sub_827C1188`, `skate_core::living_world::peds::fade`): opaque up to
//!   the model's first distance pair (45 m [data]) from the camera, gone at 55 m, plus a 1 s
//!   spawn fade in; the opacity goes into [`NpcFade::alpha`](super::npc_skaters::NpcFade) and is
//!   drawn by the shared NPC fade; at 0 the ped's scene is hidden. This is what keeps the census
//!   cull (70 m, a pure distance test) out of sight, as in retail.
//! - **LOD** (placeholder, the LOD pick is not decoded): `LOD0` within 45 m, `LOD1` beyond 55 m,
//!   hysteresis between; with the fade above `LOD1` only shows while fading.
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
use skate_core::living_world::peds::wander::{NoSignals, constrain_move, crosswalk_ok, separation_ok};
use skate_core::living_world::peds::{CrosswalkRule, Locomotion, NavMesh, NavObstacles, NavRules, Neighbour, ObstacleInput, PedAnimPlayer, PedCatalog, PedEvaluator, PedNav, PedOverrides, PedRig, WalkSignals, WanderParams};
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
    /// The district's navmesh (M3); `None` on maps without one (peds use [`TestPath`]).
    pub nav: Option<Arc<NavMesh>>,
    /// The navmesh records as loaded (rebuilt with new [`NavRules`] when a mod changes them).
    pub nav_input: Option<Arc<skate_core::living_world::peds::NavMeshInput>>,
    /// Signalled junction arms of the loaded roads (mod crosswalk rule).
    pub arms: Arc<Vec<(usize, u8, [f32; 3])>>,
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

    /// Read the district's navmesh (M3) from `navmesh.bin` (none: peds use the test path).
    pub(crate) fn load_nav(&mut self, asset_root: &std::path::Path, district: &str, rules: &NavRules) {
        let input = std::fs::read(asset_root.join(skate_data::ped_nav::NAVMESH))
            .map_err(|e| e.to_string())
            .and_then(|b| skate_data::ped_nav::district(&b, district));
        match input {
            Ok(Some(input)) => {
                let mesh = NavMesh::build(&input, rules.clone());
                self.status.push_str(&format!(", navmesh {} polygons", mesh.polys.len()));
                self.nav = Some(Arc::new(mesh));
                self.nav_input = Some(Arc::new(input));
            }
            Ok(None) => self.status.push_str(", no navmesh for this map"),
            Err(e) => self.status.push_str(&format!(", navmesh: {e}")),
        }
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
                Self { catalog: Arc::new(t.catalog), anim_sets: Arc::new(t.anim_sets), rig: Arc::new(bank.rig), clips: Arc::new(clips), status, loaded_for: None, ..Self::default() }
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

/// Navigation settings (mod-facing; default = retail): wander parameters, which navmesh areas
/// peds may use and what they cost, and the crosswalk rule (retail `Off`).
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct PedNavSettings {
    pub wander: WanderParams,
    pub rules: NavRules,
    pub crosswalk: CrosswalkRule,
}

/// Dynamic objects as ped navigation obstacles (fix 11): props and mod bodies by stable id.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct PedObstacles(pub NavObstacles);

/// Obstacle id of a mod body (props keep their prop id; mod bodies live above 2^40).
pub(crate) const MOD_BODY_OBSTACLE_BASE: u64 = 1 << 40;

/// The obstacle list of this tick: prop boxes (current pose, carried flag) and mod bodies
/// (world AABBs, the attached one carried).
pub(crate) fn obstacle_inputs(physics: Option<&crate::physics::GamePhysics>, mod_solids: &[(u64, [f32; 3], [f32; 3], [f32; 3], bool)]) -> Vec<ObstacleInput> {
    let mut out = Vec::new();
    if let Some(d) = physics.and_then(|p| p.prop_dynamics()) {
        for (id, c, basis, h, v, held) in d.obstacle_boxes() {
            out.push(ObstacleInput { id: id as u64, center: [c.x, c.y, c.z], axes: basis.columns, half_extents: [h.x, h.y, h.z], velocity: [v.x, v.y, v.z], inactive: held });
        }
    }
    for (id, min, max, v, attached) in mod_solids {
        let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
        let half = [(max[0] - min[0]) * 0.5, (max[1] - min[1]) * 0.5, (max[2] - min[2]) * 0.5];
        if !half.iter().all(|h| h.is_finite()) {
            continue;
        }
        out.push(ObstacleInput { id: MOD_BODY_OBSTACLE_BASE | id, center, axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], half_extents: half, velocity: *v, inactive: *attached });
    }
    out
}

/// Retail's per-object obstacle update, once per population tick before the peds step. Only a
/// changed cut bumps the version (resting props cost no rebuild).
pub(crate) fn update_ped_obstacles(
    settings: Res<LivingWorldSettings>,
    data: Res<PedData>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mods: Option<Res<crate::modding::Mods>>,
    mut obstacles: ResMut<PedObstacles>,
) {
    obstacles.0.set_params(settings.ped_obstacles.clone());
    if data.nav.is_none() {
        if !obstacles.0.states.is_empty() {
            obstacles.0.update(&[]);
        }
        return;
    }
    let solids = mods.as_deref().map(crate::modding::bridge::obstacle_solids).unwrap_or_default();
    let inputs = obstacle_inputs(physics.as_deref(), &solids);
    obstacles.0.update(&inputs);
}

/// The simulated body: animation player, navigation, position and heading.
#[derive(Component, Clone, Debug)]
pub(crate) struct PedBody {
    pub player: PedAnimPlayer,
    /// Placeholder intent source on maps without a navmesh.
    pub path: TestPath,
    /// Navigation state (M3); a mod route goes in `nav.route`.
    pub nav: PedNav,
    /// Seconds the body's steps have been refused (walls, other peds, the crosswalk rule).
    pub blocked: f32,
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
fn ground(physics: Option<&crate::physics::GamePhysics>, at: Vec3, reach: f32) -> Option<f32> {
    use skate_core::math::Vector3;
    let p = physics?;
    match p.world().query_thin_line(Vector3::new(at.x, at.y + reach, at.z), Vector3::new(at.x, at.y - reach, at.z)) {
        Ok(Some(hit)) => Some(hit.geometry.position.y),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn load_ped_data(
    config: Option<Res<crate::config::Config>>,
    map: Option<Res<crate::map_transition::CurrentMap>>,
    mut data: ResMut<PedData>,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
    settings: Res<LivingWorldSettings>,
    nav: Res<PedNavSettings>,
    state: Res<PopulationState>,
) {
    // A mod changed the nav rules: rebuild the mesh from the loaded records.
    if data.nav.as_ref().is_some_and(|m| m.rules != nav.rules) {
        if let Some(input) = data.nav_input.clone() {
            data.nav = Some(Arc::new(NavMesh::build(&input, nav.rules.clone())));
        }
    }
    if data.arms.is_empty() {
        if let Some(roads) = state.roads.as_ref() {
            data.arms = Arc::new(skate_core::living_world::peds::crosswalk::signalled_arms(roads));
        }
    }
    let (Some(config), Some(map)) = (config, map) else { return };
    let key = (map.name.clone(), map.generation);
    if data.loaded_for.as_ref() == Some(&key) {
        return;
    }
    let mut loaded = PedData::load(&config.asset_root);
    loaded.load_nav(&config.asset_root, &map.name, &nav.rules);
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
        let mut nav = PedNav::default();
        // On the navmesh at the record's own height (a ground probe first could land on a wall
        // top above the record and put the ped on that layer, fix 17); maps without a navmesh
        // take the ground below.
        match data.nav.as_ref().and_then(|m| m.locate(at.to_array())) {
            Some(p) => {
                at = Vec3::from_array(p.position);
                nav.poly = Some(p.poly);
            }
            None => {
                if let Some(y) = ground(physics.as_deref(), at, 3.0) {
                    at.y = y;
                }
            }
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
        let body = PedBody { player, path: TestPath::new(s.seed), nav, blocked: 0.0, position: at, heading: s.heading, ticks: 0, feet_down: [false; 2], body_fall: 0.0 };
        let e = commands
            .spawn((
                Name::new(format!("Pedestrian {} ({})", s.id.serial, look.recipe)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(s.heading)),
                Visibility::Inherited,
                PedAudio { voice: look.voice, ..Default::default() },
                // Spawn fade in starts at 0 (retail `+576`); `present_ped_pose` raises it.
                super::npc_skaters::NpcFade { alpha: 0.0, ..Default::default() },
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

/// Step every ped to the population tick: navigation (M3), animation, root motion kept on the
/// navmesh and apart from other peds, ground, audio, position. Peds step in id order against a
/// shared position list, so the result does not depend on query order (host-deterministic).
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_peds(
    mut state: ResMut<PopulationState>,
    data: Res<PedData>,
    nav_settings: Res<PedNavSettings>,
    traffic: Option<Res<super::vehicles::TrafficState>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    obstacles: Res<PedObstacles>,
    mut peds: Query<(&Pedestrian, &mut PedBody, &mut Transform, &mut PedAudio)>,
    mut events: MessageWriter<PedEvent>,
) {
    let obstacles = &obstacles.0;
    let tick = state.world.tick();
    let dt = tick_seconds(state.world.clock().hz);
    let mut list: Vec<_> = peds.iter_mut().collect();
    list.sort_by_key(|(p, ..)| p.id);
    let mut neighbours: Vec<Neighbour> = list.iter().map(|(p, b, ..)| Neighbour { order: id_order(p.id), position: b.position.to_array() }).collect();
    let clock = traffic.as_ref().and_then(|t| t.clock.as_ref());
    let road_signals = clock.map(|clock| skate_core::living_world::peds::crosswalk::RoadWalkSignals { arms: &data.arms, clock, radius: 20.0 });
    let signals: &dyn WalkSignals = match &road_signals {
        Some(s) => s,
        None => &NoSignals,
    };
    for (k, (ped, mut body, mut transform, mut audio)) in list.into_iter().enumerate() {
        let Some(set) = data.anim_sets.get(&ped.anim_set).or_else(|| data.anim_sets.get("default")) else { continue };
        let target = tick.saturating_sub(ped.spawn_tick);
        let body = &mut *body;
        let me = id_order(ped.id);
        while body.ticks < target {
            let mut turn = 0.0;
            match data.nav.as_deref() {
                Some(mesh) => {
                    let out = body.nav.step_avoiding(mesh, &nav_settings.wander, nav_settings.crosswalk, signals, me, body.position.to_array(), body.heading, body.player.state, &neighbours, Some(obstacles), dt);
                    body.player.intent = out.intent;
                    turn = out.turn;
                }
                None => body.player.intent = body.path.intent(dt, body.player.state),
            }
            let out = body.player.step(dt, set, &*data);
            body.heading += turn;
            let rotation = Quat::from_rotation_y(body.heading);
            let to = body.position + rotation * Vec3::from_array(out.root.translation);
            match data.nav.as_deref() {
                Some(mesh) => {
                    // Over linked polygons only: across tile seams, never onto an unconnected
                    // layer such as a wall top (fix 17).
                    let from = body.position.to_array();
                    let (next, mut poly, on_mesh) = constrain_move(mesh, body.nav.poly, from, to.to_array());
                    let moving = (to - body.position).length_squared() > 1e-10;
                    // Never into a prop or mod body: slide along its face or stay (fix 11).
                    let (next, clear) = match obstacles.resolve_step(from, next, mesh.agent[1]) {
                        Some(n) if n == next => (n, true),
                        Some(n) => {
                            let (m, k, ok) = constrain_move(mesh, body.nav.poly, from, n);
                            poly = k;
                            (m, ok)
                        }
                        None => (next, false),
                    };
                    // A step that slid to (almost) nothing against an edge counts as refused, so
                    // a ped walking into a boundary re-plans instead of walking in place.
                    let wanted = (to - body.position).with_y(0.0).length();
                    let progressed = !moving || Vec3::from_array(next).with_y(0.0).distance(body.position.with_y(0.0)) >= 0.25 * wanted;
                    let ok = on_mesh
                        && clear
                        && progressed
                        && separation_ok(body.position.to_array(), next, me, &neighbours, mesh.agent[1])
                        && crosswalk_ok(mesh, nav_settings.crosswalk, signals, body.position.to_array(), next);
                    if ok {
                        body.position = Vec3::from_array(next);
                        body.nav.poly = poly;
                        body.blocked = 0.0;
                    } else if moving {
                        body.blocked += dt;
                        if body.blocked > nav_settings.wander.yield_patience {
                            // Stuck against a wall / ped / red light: pick another way (short fan).
                            body.blocked = 0.0;
                            body.nav.skip_long = true;
                            body.nav.corners.clear();
                        }
                    }
                    neighbours[k].position = body.position.to_array();
                }
                None => body.position = to,
            }
            body.heading += out.root.yaw;
            body.feet_down = out.feet_down;
            body.body_fall = out.body_fall;
            body.ticks += 1;
            if let Some(s) = out.entered {
                events.write(PedEvent::State { id: ped.id, state: s });
            }
        }
        // The render ground: a line query in a window round the navmesh height (one agent height
        // up and down, the NavPower agent block [data]). It never feeds back into the navigation
        // position, so geometry above the ped (a wall top, a ledge) cannot lift it onto another
        // layer (fix 17).
        let mut shown = body.position;
        let reach = data.nav.as_deref().map_or(3.0, |m| m.agent[3].max(0.5));
        if let Some(y) = ground(physics.as_deref(), body.position, reach) {
            shown.y = y;
        }
        transform.translation = shown;
        transform.rotation = Quat::from_rotation_y(body.heading);
        audio.feet_down = body.feet_down;
        audio.body_fall = body.body_fall;
        state.world.update_position(ped.id, body.position.to_array());
    }
}

/// A stable ordering key for a ped (avoidance priority: lower first).
pub(crate) fn id_order(id: LivingWorldId) -> u64 {
    id.serial as u64
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
    /// Bone-local basis of this GLB's joint frames relative to the rig's ([`ped_bone_basis`]).
    basis: Mat4,
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
                // GLB bind globals per rig bone from the skin's inverse bind matrices.
                let mut glb_bind: BTreeMap<usize, Mat4> = BTreeMap::new();
                for (mesh, skin) in &skins {
                    if !parents.iter_ancestors(mesh).any(|p| p == scene) {
                        continue;
                    }
                    commands.entity(mesh).insert(bevy::camera::visibility::NoFrustumCulling);
                    let Some(ibms) = bindposes.get(&skin.inverse_bindposes) else { continue };
                    for (joint, ibm) in skin.joints.iter().zip(ibms.iter()) {
                        let Ok((name, _)) = nodes.get(*joint) else { continue };
                        if let Some(i) = data.rig.names.iter().position(|n| n.eq_ignore_ascii_case(name.as_str())) {
                            glb_bind.insert(i, ibm.inverse());
                        }
                    }
                }
                let basis = ped_bone_basis(&reference_globals(&data.rig), &glb_bind);
                // Bind in the rig's bone frames (native space).
                let bind: BTreeMap<usize, Mat4> = glb_bind.iter().map(|(&i, m)| (i, *m * basis.inverse())).collect();
                puppet.basis = basis;
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

/// A ped whose body materials carry its tint pair (look side, set once per ped).
#[derive(Component)]
pub(crate) struct PedTinted;

/// Recoloured ped materials per (source material, tint pair): peds sharing a model and a palette
/// entry share one texture copy. Weak ids: a copy (and its texture) is freed with the last ped
/// mesh using it, and its entry is dropped.
#[derive(Resource, Default)]
pub(crate) struct PedMaterials(BTreeMap<(AssetId<StandardMaterial>, [u32; 8]), AssetId<StandardMaterial>>);

/// Whether a ped GLB material gets the tint: its retail material type from the export's
/// material extras (`{"shader": "pedestrian_high_stamp"}`, `colorize::colorized_shader`); for
/// a GLB exported before the type was written (no extras), the body slot `Rostral_*` (every
/// shipped ped body is `pedestrian_high_stamp` / `pedestrian_low` [data]; only one ped hair uses
/// the ped shader and is missed by this fallback until the next export).
pub(crate) fn ped_material_colorized(name: Option<&str>, extras: Option<&str>) -> bool {
    match extras {
        Some(json) => serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .and_then(|v| v.get("shader").and_then(|s| s.as_str()).map(skate_core::living_world::peds::colorize::colorized_shader))
            .unwrap_or(false),
        None => name.is_some_and(|n| n.starts_with("Rostral_")),
    }
}

/// Paint each bound ped's body materials with its tint pair (retail's ped shader rule,
/// `skate_core::living_world::peds::colorize`): a copy of the diffuse texture per tint pair.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_ped_tints(
    mut commands: Commands,
    peds: Query<(Entity, &Pedestrian, &PedPuppet), Without<PedTinted>>,
    children: Query<&Children>,
    meshes: Query<(&MeshMaterial3d<StandardMaterial>, Option<&bevy::gltf::GltfMaterialName>, Option<&bevy::gltf::GltfMaterialExtras>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<PedMaterials>,
) {
    cache.0.retain(|_, id| materials.contains(*id));
    for (e, ped, puppet) in &peds {
        let (Some(scene), true) = (puppet.scene, puppet.bindings.is_some()) else { continue };
        let bits = |c: [f32; 4]| c.map(f32::to_bits);
        let (a, b) = (bits(ped.tint_a), bits(ped.tint_b));
        let pair = [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]];
        let mut pending = false;
        for ent in children.iter_descendants(scene) {
            let Ok((handle, name, extras)) = meshes.get(ent) else { continue };
            if !ped_material_colorized(name.map(|n| n.0.as_str()), extras.map(|x| x.value.as_str())) {
                continue;
            }
            let key = (handle.0.id(), pair);
            if let Some(m) = cache.0.get(&key).and_then(|id| materials.get_strong_handle(*id)) {
                commands.entity(ent).insert(MeshMaterial3d(m));
                continue;
            }
            let Some(source) = materials.get(&handle.0).cloned() else {
                pending = true;
                continue;
            };
            let Some(tex) = source.base_color_texture.clone() else { continue };
            let Some(image) = images.get(&tex) else {
                pending = true;
                continue;
            };
            let mut tinted = image.clone();
            match tinted.data.as_mut() {
                Some(px) if tinted.texture_descriptor.format.block_copy_size(None) == Some(4) => {
                    skate_core::living_world::peds::colorize::colorize_rgba8(px, ped.tint_a, ped.tint_b)
                }
                _ => warn!("LIVING_WORLD peds: {} base texture is not RGBA8; drawn untinted", ped.recipe),
            }
            let mut m = source;
            m.base_color_texture = Some(images.add(tinted));
            let h = materials.add(m);
            cache.0.insert(key, h.id());
            commands.entity(ent).insert(MeshMaterial3d(h));
        }
        if !pending {
            commands.entity(e).insert(PedTinted);
        }
    }
}

/// Model-space globals of the rig's reference pose (`PEDESTRIAN_RIG_TPOSE`, root at the origin
/// like [`PedAnimPlayer::pose`](skate_core::living_world::peds::PedAnimPlayer::pose)).
pub(crate) fn reference_globals(rig: &PedRig) -> Vec<Mat4> {
    let mut locals = rig.reference.clone();
    if let Some(root) = locals.first_mut() {
        *root = skate_core::living_world::peds::anim::IDENTITY;
    }
    PedEvaluator::globals(rig, &locals).into_iter().map(crate::animation::native_matrix).collect()
}

/// The bone-local basis between a ped GLB's joint frames and the rig's: the candidate (identity,
/// the retail ped GLBs; or the skater GLB convention `render_basis`) whose
/// `reference x basis` is closest in rotation to the GLB bind over the matched bones. Identity
/// for every shipped ped model (data test); a pure function of the model, so deterministic.
pub(crate) fn ped_bone_basis(reference: &[Mat4], glb_bind: &BTreeMap<usize, Mat4>) -> Mat4 {
    let angle = |a: Mat4, b: Mat4| {
        let (qa, qb) = (Quat::from_mat4(&a).normalize(), Quat::from_mat4(&b).normalize());
        2.0 * qa.dot(qb).abs().min(1.0).acos()
    };
    let score = |basis: Mat4| -> f32 { glb_bind.iter().filter_map(|(&i, b)| Some(angle(*reference.get(i)? * basis, *b))).sum() };
    [Mat4::IDENTITY, render_basis()].into_iter().map(|b| (score(b), b)).fold((f32::INFINITY, Mat4::IDENTITY), |best, x| if x.0 < best.0 { x } else { best }).1
}

/// Joint globals for [`AnimationStatus::pose_transforms`](crate::animation::AnimationStatus)
/// (which right-multiplies the skater's `render_basis`): the rig globals in the GLB's joint
/// frames, so the skin matrix is `global x basis x inverse(GLB bind)`.
pub(crate) fn ped_joint_globals(globals: &[Mat4], basis: Mat4) -> Vec<Mat4> {
    let cancel = render_basis().inverse() * basis;
    globals.iter().map(|g| *g * cancel).collect()
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

/// Drawn ped opacity with the NPC draw distance (QoL, not retail): the fade pair (model pair or
/// the configured default) x the multiplier, so the fade keeps ending before the scaled census
/// cull. At retail (1x) the pair is used as is. The LOD distances stay retail (far peds keep the
/// cheaper LOD).
pub(crate) fn ped_draw_alpha(settings: &LivingWorldSettings, pair: Option<[f32; 2]>, distance: f32, since_spawn: f32) -> f32 {
    let dd = settings.draw_distance();
    if dd.is_retail() {
        return skate_core::living_world::peds::draw_alpha(&settings.ped_fade, pair, distance, since_spawn);
    }
    let scale = |p: [f32; 2]| [dd.distance(p[0]), dd.distance(p[1])];
    let cfg = skate_core::living_world::peds::PedFadeConfig { distance: scale(settings.ped_fade.distance), ..settings.ped_fade };
    skate_core::living_world::peds::draw_alpha(&cfg, pair.map(scale), distance, since_spawn)
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_ped_pose(
    data: Res<PedData>,
    state: Res<PopulationState>,
    settings: Res<LivingWorldSettings>,
    fixed: Res<Time<Fixed>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut peds: Query<(&Pedestrian, &PedBody, &mut PedPuppet, Option<&mut super::npc_skaters::NpcFade>)>,
    mut joints: Query<&mut Transform, Without<PedBody>>,
    mut vis: Query<&mut Visibility>,
) {
    let hz = state.world.clock().hz;
    let ahead = ((state.world.clock().overstep() + fixed.overstep_fraction() as f64 * fixed.timestep().as_secs_f64() * hz).clamp(0.0, 1.0) as f32) * tick_seconds(hz);
    let camera = cameras.iter().next().map(|c| c.translation());
    for (ped, body, mut puppet, fade) in &mut peds {
        let Some(bindings) = puppet.bindings.as_ref() else { continue };
        let Some(globals) = ped_globals(&data.rig, body, &*data, ahead, &puppet.followers) else { continue };
        for (joint, local) in bindings.pose_transforms(&ped_joint_globals(&globals, puppet.basis)) {
            if let Ok(mut t) = joints.get_mut(joint) {
                *t = local;
            }
        }
        if let (Some(cam), Some(mut fade)) = (camera, fade) {
            let pair = data.catalog.models.get(&ped.model).and_then(|m| m.lod_near);
            let since = state.world.tick().saturating_sub(ped.spawn_tick) as f32 / hz.max(1.0) as f32;
            let alpha = ped_draw_alpha(&settings, pair, cam.distance(body.position), since);
            if fade.alpha != alpha {
                fade.alpha = alpha;
            }
            if let Some(scene) = puppet.scene
                && let Ok(mut v) = vis.get_mut(scene)
            {
                let want = if alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
                if *v != want {
                    *v = want;
                }
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
    if !settings.debug || !super::report_due(state.world.tick(), &mut last, 150) {
        return;
    }
    let list: Vec<_> = peds.iter().map(|(p, b)| (p.id, p.recipe.clone(), b.player.state, b.position)).collect();
    info!("LIVING_WORLD {}", ped_readout(&list, observers.observers.first().map(|o| Vec3::from_array(o.position))));
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<PedData>()
        .init_resource::<PedLooks>()
        .init_resource::<PedIndex>()
        .init_resource::<PedRejected>()
        .init_resource::<PedNavSettings>()
        .init_resource::<PedObstacles>()
        .add_message::<PedEvent>()
        .add_systems(FixedUpdate, (load_ped_data, apply_ped_records, release_rejected, update_ped_obstacles, advance_peds, log_ped_readout).chain().after(super::step_population))
        .init_resource::<PedMaterials>()
        .add_systems(Update, (present_ped_looks, present_ped_tints, present_ped_pose).chain().after(crate::app::FrameSet::Animation).before(super::npc_skaters::present_fade));
}
