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
use skate_core::living_world::peds::wander::{NoSignals, constrain_move, crosswalk_ok, separation_blocker};
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
    /// The stock ped AI graph on the shared graph runtime (behaviour runtime; `None` when the
    /// install has none).
    pub graph: Option<Arc<super::ped_graph::PedGraph>>,
    /// The stock mood tables and each entity type's reaction set (`ped_mood`).
    pub mood: Option<Arc<skate_core::living_world::peds::mood::MoodTables>>,
    pub reaction_sets: Arc<BTreeMap<String, String>>,
    /// Each entity type's chase record and the chase manager's `global` record (`ped_mood::chase_records`).
    pub chase: Arc<BTreeMap<String, skate_core::living_world::peds::chase::ChaseRecord>>,
    pub chase_global: Option<Arc<skate_core::living_world::peds::chase::ChaseRecord>>,
    /// The `zombie` chase record (zombie cheat: every chase group and every ped's own record, `ped_mood::ZOMBIE_KEY`).
    pub chase_zombie: Option<Arc<skate_core::living_world::peds::chase::ChaseRecord>>,
    /// The conversation plugin graph (`plugin/conversation.stategraph`) and the conversation tables.
    pub conversation_graph: Option<Arc<super::ped_graph::PedGraph>>,
    /// The world-prop plugin graphs by prop class (each descriptor's `file`, compiled).
    pub plugin_graphs: BTreeMap<String, Arc<super::ped_graph::PedGraph>>,
    pub conversations: Arc<super::ped_mood::ConversationTables>,
    /// Ped plugins on world props: classes, descriptors, `plugin_odds`, placed props (`ped_plugins`).
    pub plugins: Arc<super::ped_plugins::PluginData>,
    /// Hand props: records and models (`ped_hand_props`).
    pub hand_props: Arc<super::ped_hand_props::HandPropData>,
    /// Each entity type's starting hand prop chance and list (`ped_mood::starting_hand_props`).
    pub starting_props: Arc<BTreeMap<String, (f32, Vec<(String, f32)>)>>,
    /// The sit plugin's values by entity type (`ped_mood::sit_values`).
    pub sit: Arc<BTreeMap<String, skate_core::living_world::peds::brain::SitValues>>,
    /// Each entity type's vision test ranges (`ped_mood::sight`).
    pub sight: Arc<BTreeMap<String, skate_core::living_world::peds::perception::Sight>>,
    /// Each entity type's perception scales (near, far, fov) for the mood post's vision test.
    pub sight_scales: Arc<BTreeMap<String, [f32; 3]>>,
    /// Each entity type's takedown table (`ped_mood::takedown_tables`).
    pub takedowns: Arc<BTreeMap<String, skate_core::living_world::peds::takedown::TakedownTable>>,
    /// The `zombie` takedown table (zombie cheat, every ped).
    pub takedown_zombie: Option<Arc<skate_core::living_world::peds::takedown::TakedownTable>>,
    pub(super) loaded_for: Option<(String, u64)>,
    /// Loaded behind the loading screen: the first fixed tick leaves `arms` empty, as a late load did.
    pub(super) skip_arms_once: bool,
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
        Self::load_with(asset_root, None)
    }

    /// [`Self::load`] with `tables.json` already parsed (`None`: read and parse it here).
    pub(crate) fn load_with(asset_root: &std::path::Path, tables: Option<&serde_json::Value>) -> Self {
        let tables = match tables {
            Some(doc) => skate_data::ped_anim::PedTables::from_value(doc),
            None => std::fs::read(asset_root.join("private/living_world/tables.json")).map_err(|e| e.to_string()).and_then(|b| skate_data::ped_anim::PedTables::parse(&b)),
        };
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
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct PedNavSettings {
    pub wander: WanderParams,
    pub rules: NavRules,
    pub crosswalk: CrosswalkRule,
}

/// The ped ground probe's retry reach after a miss (`82EA5F48`: 4.0 at `0x82257308`) [code, b107].
pub(crate) const GROUND_RETRY_FACTOR: f32 = 4.0;

/// Dynamic objects as ped navigation obstacles (fix 11): props and mod bodies by stable id.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct PedObstacles(pub NavObstacles);

/// Obstacle id of a mod body (props keep their prop id; mod bodies live above 2^40).
pub(crate) const MOD_BODY_OBSTACLE_BASE: u64 = 1 << 40;

/// The obstacle list of this tick: prop boxes (current pose, held flag) and mod bodies (world
/// AABBs, the attached one held). Held is not the retail obstacle-off gate (that word is never set by
/// Move Object), so a held prop stays an obstacle (`ObstacleParams::held_is_obstacle`, retail on).
pub(crate) fn obstacle_inputs(physics: Option<&crate::physics::GamePhysics>, mod_solids: &[(u64, [f32; 3], [f32; 3], [f32; 3], bool)]) -> Vec<ObstacleInput> {
    let mut out = physics.and_then(|p| p.prop_dynamics()).map(prop_obstacle_inputs).unwrap_or_default();
    for (id, min, max, v, attached) in mod_solids {
        let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
        let half = [(max[0] - min[0]) * 0.5, (max[1] - min[1]) * 0.5, (max[2] - min[2]) * 0.5];
        if !half.iter().all(|h| h.is_finite()) {
            continue;
        }
        out.push(ObstacleInput { id: MOD_BODY_OBSTACLE_BASE | id, center, axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], half_extents: half, velocity: *v, inactive: false, held: *attached });
    }
    out
}

/// The prop boxes of this tick as obstacle inputs (current pose and velocity, held flag).
pub(crate) fn prop_obstacle_inputs(d: &crate::physics::prop_dynamics::PropDynamics) -> Vec<ObstacleInput> {
    d.obstacle_boxes()
        .into_iter()
        .map(|(id, c, basis, h, v, held)| ObstacleInput { id: id as u64, center: [c.x, c.y, c.z], axes: basis.columns, half_extents: [h.x, h.y, h.z], velocity: [v.x, v.y, v.z], inactive: false, held })
        .collect()
}

/// Diagnostics for moved props as ped obstacles (2026-10-07: "Moving objects does not update the
/// collision for peds", and the sessions had no obstacle lines). Spawn = where an id was first seen.
#[derive(Resource, Default)]
pub(crate) struct PedObstacleTrace {
    spawn: BTreeMap<u64, [f32; 3]>,
    /// Ids now more than 0.1 m from their spawn, with the spawn centre.
    pub moved: Vec<(u64, [f32; 3])>,
    /// Last logged (cut, cut centre, off, held, moving, tick) of a moved id.
    last: BTreeMap<u64, (bool, [f32; 3], bool, bool, bool, u64)>,
    next_summary: u64,
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Retail's per-object obstacle update, once per population tick before the peds step. Only a
/// changed cut bumps the version (resting props cost no rebuild).
pub(crate) fn update_ped_obstacles(
    settings: Res<LivingWorldSettings>,
    data: Res<PedData>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mods: Option<Res<crate::modding::Mods>>,
    mut obstacles: ResMut<PedObstacles>,
    state: Res<PopulationState>,
    mut trace: ResMut<PedObstacleTrace>,
    observers: Res<super::LivingWorldObservers>,
) {
    let _span = super::timing::span(super::timing::PED_OBSTACLES);
    obstacles.0.set_params(settings.ped_obstacles.clone());
    if data.nav.is_none() {
        if !obstacles.0.states.is_empty() {
            obstacles.0.update(&[]);
        }
        return;
    }
    let solids = mods.as_deref().map(crate::modding::bridge::obstacle_solids).unwrap_or_default();
    let mut inputs = obstacle_inputs(physics.as_deref(), &solids);
    // Every player skater (b135: always NavPower's moving avoider for peds), by player index.
    if settings.ped_obstacles.skaters {
        let h = settings.ped_obstacles.skater_half_extents;
        for (n, o) in observers.observers.iter().enumerate() {
            let center = [o.position[0], o.position[1] + h[1], o.position[2]];
            inputs.push(ObstacleInput { id: skate_core::living_world::peds::obstacles::SKATER_OBSTACLE_BASE + n as u64, center, axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], half_extents: h, velocity: o.velocity, inactive: false, held: false });
        }
    }
    obstacles.0.update(&inputs);
    let tick = state.world.tick();
    let hz = state.world.clock().hz as u64;
    let trace = &mut *trace;
    trace.moved.clear();
    for input in &inputs {
        let spawn = *trace.spawn.entry(input.id).or_insert(input.center);
        if dist3(spawn, input.center) <= 0.1 && !trace.last.contains_key(&input.id) {
            continue;
        }
        trace.moved.push((input.id, spawn));
        let Some(s) = obstacles.0.states.get(&input.id) else { continue };
        let now = (s.cut.is_some(), s.cut_at, s.inactive, s.held, s.moving, tick);
        // A state change, a re-cut, or once a second while held (the body pose of a dragged prop
        // next to its cut pose: "peds walk through the prop I am holding", 2026-10-08).
        let changed = trace.last.get(&input.id).is_none_or(|l| {
            l.0 != now.0 || l.2 != now.2 || l.3 != now.3 || l.4 != now.4 || (now.0 && dist3(l.1, now.1) > 0.05) || (now.3 && tick >= l.5 + hz)
        });
        if changed {
            trace.last.insert(input.id, now);
            let speed = dist3(input.velocity, [0.0; 3]);
            let role = if s.inactive { "off" } else if s.cut.is_some() { "cut" } else if obstacles.0.params.moving_solid { "solid" } else { "none" };
            info!(
                "PED_OBSTACLE id={} spawn=[{:.2}, {:.2}, {:.2}] now=[{:.2}, {:.2}, {:.2}] moved={:.2} cut={} cut_at=[{:.2}, {:.2}, {:.2}] cut_off={:.2} speed={speed:.2} moving={} held={} off={} role={role} half=[{:.2}, {:.2}] y=[{:.2}, {:.2}] version={} tick={tick}",
                input.id, spawn[0], spawn[1], spawn[2], input.center[0], input.center[1], input.center[2], dist3(spawn, input.center),
                now.0, now.1[0], now.1[1], now.1[2], if now.0 { dist3(now.1, input.center) } else { -1.0 }, s.moving, s.held, s.inactive, s.now.half[0], s.now.half[1], s.now.y_min, s.now.y_max, obstacles.0.version,
            );
        }
    }
    if tick >= trace.next_summary {
        trace.next_summary = tick + hz * if trace.moved.is_empty() { 10 } else { 2 };
        let o = &obstacles.0;
        info!(
            "PED_OBSTACLES inputs={} props={} states={} cut={} moving={} held={} off={} moved={} version={} tick={tick}",
            inputs.len(), inputs.iter().filter(|i| i.id < MOD_BODY_OBSTACLE_BASE).count(), o.states.len(), o.cut_count(),
            o.states.values().filter(|s| s.moving).count(), o.states.values().filter(|s| s.held).count(), o.states.values().filter(|s| s.inactive).count(), trace.moved.len(), o.version,
        );
    }
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
    /// The last console tick's movement over its length, m/s (the attack throw's look-ahead, `82E3E960`).
    pub velocity: Vec3,
    pub heading: f32,
    /// Console ticks stepped since the spawn.
    pub ticks: u64,
    pub feet_down: [bool; 2],
    pub body_fall: f32,
    /// The taunt clip (motiongraph_taunt): requested by TakedownTauntVictim, playing, finished (the brain then
    /// drops its "SGIntent").
    pub taunt: TauntClip,
    pub plugin_motion: PluginMotionRun,
}

/// Where a ped's taunt clip is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TauntClip {
    #[default]
    None,
    Requested,
    Playing,
    Done,
}

/// A plugin state's run on the motion side (`skate_core::living_world::peds::plugin_motion`): the packet it plays,
/// its progress (the taunt's handshake) and a pending release of its held cycle.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PluginMotionRun {
    pub packet: Option<String>,
    pub state: TauntClip,
    pub release: bool,
    /// The packet ended while its clips still play (the AI left the sit because the seat moved away). Retail's sit
    /// state is non-interruptable (`motiongraph_sit.xml`): Stand2Sit plays out, then SittingCyc leaves on
    /// "not HasIntent Sit" to Sit2Stand, so the held cycle is released as soon as it is reached.
    pub orphaned: bool,
    /// The step that spawns the hand prop reached its branch window (`SpawnInteractionBasedHandProp`); the brain side
    /// takes it once (`think_peds`) and `hand_prop_taken` keeps it from firing again in the same run.
    pub hand_prop_due: bool,
    pub hand_prop_taken: bool,
    /// A hand prop throw clip the brain asked for (`82E3E648` plays it on the ped, blend 0.2) and its blend.
    pub throw_clip: Option<(&'static str, f32)>,
    /// A tazer clip the brain asked for (`ChaseRequest::TazerClip`), and the last one the channel took with whether
    /// the ped's set had it (read by `ped_hand_props::sync_hand_props` for `PedBrain::update_tazer_clip`).
    pub tazer_clip: Option<&'static str>,
    pub tazer_played: Option<(&'static str, bool)>,
}

/// A ped counts as still while it stays within this distance of where it stopped, m.
const STILL_RADIUS: f32 = 0.1;
/// Ticks still before the first report and between reports (5 s on the 60 Hz world clock).
const STILL_REPORT_TICKS: u64 = 300;
/// Full still-ped reports per tick; due peds beyond this wait for a later tick, so many stuck
/// peds do not all format their report in the same frame.
const STILL_REPORTS_PER_TICK: u32 = 1;

/// Diagnostics: a ped that has not moved for 5 s logs `PED_STILL` with everything about it (brain,
/// mood, graph and plugin state, animation, navigation, conversation), then every 5 s while it
/// stays; a report identical to the previous one logs a short line.
fn log_still_ped(ped: &Pedestrian, body: &PedBody, mind: &mut PedMind, state: &str, conversations: &BTreeMap<u64, skate_core::living_world::peds::conversation::Conversation>, tick: u64, budget: &mut u32) {
    let at = body.position.to_array();
    let still = &mut mind.still;
    let moved = still.anchor.is_none_or(|(p, _)| (p[0] - at[0]).hypot(p[2] - at[2]) > STILL_RADIUS || (p[1] - at[1]).abs() > STILL_RADIUS);
    if moved {
        still.anchor = Some((at, tick));
        still.due = tick + STILL_REPORT_TICKS;
        still.last.clear();
        return;
    }
    let since = still.anchor.map_or(tick, |a| a.1);
    if tick < still.due || *budget == 0 {
        return;
    }
    *budget -= 1;
    still.due = tick + STILL_REPORT_TICKS;
    let me = ped.id.to_u64();
    let seconds = (tick - since) as f32 / 60.0;
    let member = conversations.values().find(|c| c.members.iter().any(|m| m.ped == me));
    let near = conversations.values().filter(|c| (c.center[0] - at[0]).hypot(c.center[2] - at[2]) < 5.0).map(|c| (c.id, c.state, c.members.len())).collect::<Vec<_>>();
    let conversation = member.map(|c| format!("member of {} state={} speaker={} members={:?} row={:?}", c.id, c.state, c.speaker, c.members, c.row));
    let mut brain = mind.brain.clone();
    brain.rng = None;
    let nav = &body.nav;
    let report = format!(
        "type={} census={} model={} state={state} plugin_graph={:?} plugin_state={:?} prop={:?} conversation={conversation:?} conversations_within_5m={near:?} \
         anim={{locomotion={} clip={} clip_time={:.2} channel={:?} taunt={:?} plugin_motion={:?}}} \
         nav={{target={:?} corner={}/{} waiting={:?} waited={:.1} route={} poly={:?} blocked={:.1}}} heading={:.2} velocity={:?} \
         flee_goal={:?} chase_goal={:?} mood={:?} brain={brain:?}",
        ped.entity,
        ped.census,
        ped.model,
        mind.plugin_graph,
        mind.plugin_state,
        mind.prop,
        body.player.state.name(),
        body.player.current_clip(),
        body.player.current_time(),
        body.player.channel_clip(),
        body.taunt,
        body.plugin_motion,
        nav.target,
        nav.next,
        nav.corners.len(),
        nav.waiting,
        nav.waited,
        nav.route.is_some(),
        nav.poly,
        body.blocked,
        body.heading,
        body.velocity.to_array(),
        mind.flee_goal,
        mind.chase_goal,
        mind.mood,
    );
    if report == still.last {
        info!("PED_STILL ped=#{} at=[{:.2}, {:.2}, {:.2}] still={seconds:.0}s unchanged tick={tick}", ped.id.serial, at[0], at[1], at[2]);
    } else {
        info!("PED_STILL ped=#{} at=[{:.2}, {:.2}, {:.2}] still={seconds:.0}s {report} tick={tick}", ped.id.serial, at[0], at[1], at[2]);
        still.last = report;
    }
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
    /// The mood system picked a reaction (`skate_core::living_world::peds::mood`); `wants` were
    /// raised when `passed`.
    Mood { id: LivingWorldId, result: String, passed: bool, wants: Vec<String> },
    /// A skater knocked the ped down or made it stumble (`skate_core::living_world::peds::skater_contact`).
    Hit {
        id: LivingWorldId,
        kind: skate_core::living_world::peds::skater_contact::ReactionKind,
        direction: skate_core::living_world::peds::skater_contact::ReactionDirection,
        /// The skater's speed into the ped, m/s (our stand-in for the ped body's speed).
        closing: f32,
        /// Who touched the ped (the brain's contact record `+3184`; players: `PLAYER_TARGET_BASE + n`).
        toucher: u64,
    },
    /// The ped's Colliding state began (PedestrianColliding Begin `8269DA00`): the collision mood broadcast
    /// (`sub_8269D8A8`), read by every ped's think on the next step.
    Colliding { id: LivingWorldId },
    /// A chase group changed (`kind`: join, join_refused, leave, primary, group_end); `chasee` is
    /// the chased id (players: `PLAYER_TARGET_BASE + n`).
    Chase { id: LivingWorldId, chasee: u64, kind: &'static str, reason: Option<i32> },
    /// A ped's tazer hit `target` (the takedown's knock-down).
    Taze { id: LivingWorldId, target: u64 },
    /// A ped's takedown attempt ended (`success`: the target is knocked down).
    Takedown { id: LivingWorldId, target: u64, success: bool },
    /// The ped's AI graph changed its speech value (`SendSpeechEvent` / the warn); `state` is the
    /// graph state that sent it.
    /// `topic`: a conversation turn's variant and row value (`ped+2472` / `+2476`).
    Speech { id: LivingWorldId, value: i32, topic: Option<(u8, i32)>, state: String },
    /// The ped took a hand prop (`livingworld_handprops` key) at a plugin prop (SpawnInteractionBasedHandProp).
    HandProp { id: LivingWorldId, key: String },
}

/// Skater against ped contact (doc 26, "Skater hits peds"). NOT RETAIL YET: the skater is a
/// vertical cylinder at the observer (the board) of this radius and 2 m tall, and the ped body's
/// speed after the contact (retail: the Havok solve) is the skater's speed into the ped.
pub(crate) const SKATER_CONTACT_RADIUS: f32 = 0.35;
/// `82E3DBE0`'s gate on the owned object's step vector, |d|^2 > 2.5e-5 m^2 (`0x822F8BE0`) [code, b219: a step, not a
/// speed; smaller steps carry the ped].
pub(crate) const SEAT_PUSH_STEP_SQ: f32 = 2.5e-5;
/// The period that step is taken over, s: the DMO post-update `82C562D0` sends its pose change since the last
/// committed pose to its listeners once per 1/60 s sim tick (b220 [code]; main re-read the pose difference and the
/// slot 0x68 call), so the gate is 0.3 m/s (the speed-squared reading before 2026-10-17 fired at 5 mm/s).
pub(crate) const SEAT_PUSH_STEP_SECONDS: f32 = 1.0 / 60.0;
/// `82C539A8`'s seat probe tolerance, m (the navmesh point within this of the seat in x and z).
pub(crate) const SEAT_PROBE_TOLERANCE: f32 = 0.1;
/// The sit plugin's held cycle (`plugin_motion` Sit) and the Avoid graph's forward jump off a seat
/// (`motiongraph_avoid.xml` JumpForward, `PlayRemappedAnimation SitAvoidFwd`) [data].
pub(crate) const SIT_CYCLE: &str = "SitIdleCyc";
pub(crate) const SIT_AVOID_CLIP: &str = "SitAvoidFwd";
/// `motiongraph_avoid.xml` SitJump: `PedestrianSpeech` PedestrianAvoidJump [data].
pub(crate) const SIT_AVOID_SPEECH: i32 = 4;

/// The animation set's collision rules for one contact: DisableHeavyCollision (`brain+3277` bit 0x20) turns the
/// knock-down off and leaves the standing stumble (`sub_82E38FB8` clears its knock-down test when the bit is set).
pub(crate) fn contact_rules(
    set: &skate_core::living_world::peds::skater_contact::CollisionRules,
    heavy_collision_disabled: bool,
) -> skate_core::living_world::peds::skater_contact::CollisionRules {
    skate_core::living_world::peds::skater_contact::CollisionRules { can_knock_down: set.can_knock_down && !heavy_collision_disabled, ..*set }
}

/// The retail reaction to a skater at `skater` moving at `velocity` touching the ped at `ped`
/// facing `heading`: `None` when not touching, not moving into it, or no reaction.
pub(crate) fn skater_hit(
    ped: Vec3,
    heading: f32,
    ped_radius: f32,
    skater: [f32; 3],
    velocity: [f32; 3],
    rules: &skate_core::living_world::peds::skater_contact::CollisionRules,
    ignore_standing: bool,
) -> Option<(skate_core::living_world::peds::skater_contact::ReactionKind, skate_core::living_world::peds::skater_contact::ReactionDirection, f32)> {
    use skate_core::living_world::peds::skater_contact::{reaction_direction, skater_reaction};
    let d = [ped.x - skater[0], ped.z - skater[2]];
    let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if dist >= ped_radius + SKATER_CONTACT_RADIUS || (ped.y - skater[1]).abs() > 2.0 || dist <= 1e-4 {
        return None;
    }
    let n = [d[0] / dist, d[1] / dist];
    let closing = velocity[0] * n[0] + velocity[2] * n[1];
    if closing <= 0.0 {
        return None;
    }
    // IgnoreStandingCollisions (`brain+3277` bit 0x10, e.g. a seated ped): only a knock-down reacts.
    let kind = skater_reaction([closing, 0.0], [0.0, 0.0], rules, ignore_standing)?;
    Some((kind, reaction_direction(n, [heading.sin(), heading.cos()]), closing))
}

/// A ped touching the ped at `ped` (retail `sub_82E38FB8` kind 5 with a ped toucher): `toucher` stood there and moved
/// at `velocity` relative to the ped. NOT RETAIL YET: the ped body speed after the contact (retail: the Havok solve) is
/// the closing speed along the line between the two.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ped_hit(
    ped: Vec3,
    heading: f32,
    toucher: [f32; 3],
    velocity: [f32; 3],
    rules: &skate_core::living_world::peds::skater_contact::CollisionRules,
    ignore_standing: bool,
    zombie: bool,
) -> Option<(skate_core::living_world::peds::skater_contact::ReactionKind, skate_core::living_world::peds::skater_contact::ReactionDirection, f32)> {
    use skate_core::living_world::peds::skater_contact::{ped_reaction, reaction_direction};
    let d = [ped.x - toucher[0], ped.z - toucher[2]];
    let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if dist <= 1e-4 {
        return None;
    }
    let n = [d[0] / dist, d[1] / dist];
    let closing = velocity[0] * n[0] + velocity[2] * n[1];
    if closing <= 0.0 {
        return None;
    }
    let kind = ped_reaction([closing, 0.0], [0.0, 0.0], rules, ignore_standing, zombie, false)?;
    Some((kind, reaction_direction(n, [heading.sin(), heading.cos()]), closing))
}

/// Two peds' bodies touched: a step refused by another ped (`separation_blocker`). Each ped takes the other as its
/// toucher on its next step.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PedContact {
    touched: LivingWorldId,
    toucher: LivingWorldId,
    toucher_at: [f32; 3],
    velocity: [f32; 3],
}

impl PedClips for PedData {
    fn clip(&self, name: &str) -> Option<&PedClip> {
        self.clips.get(name)
    }
}

/// Ground height near a point: the first hit of a line from `up` above to `down` below, `None`
/// when nothing is hit. The first hit from the top wins, so `up` must not reach overhead geometry.
pub(crate) fn ground(physics: Option<&crate::physics::GamePhysics>, at: Vec3, up: f32, down: f32) -> Option<f32> {
    use skate_core::math::Vector3;
    let p = physics?;
    match p.world().query_thin_line(Vector3::new(at.x, at.y + up, at.z), Vector3::new(at.x, at.y - down, at.z)) {
        Ok(Some(hit)) => Some(hit.geometry.position.y),
        _ => None,
    }
}

/// Fixed tick: keep the loaded data current, then load the map's ped data unless the loading screen already did.
pub(super) fn load_ped_data(
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
    // A late load ran this upkeep on the old data and then replaced it, so a map's first tick had no arms; an early
    // load skips it once to keep that tick the same.
    let skip_arms = std::mem::take(&mut data.skip_arms_once);
    if !skip_arms && data.arms.is_empty() {
        if let Some(roads) = state.roads.as_ref() {
            data.arms = Arc::new(skate_core::living_world::peds::crosswalk::signalled_arms(roads));
        }
    }
    let (Some(config), Some(map)) = (config, map) else { return };
    load_ped_data_for_map(&config, &map, &mut data, audio, &settings, &nav);
}

/// Behind the loading screen (`super::behind_loading_gate`): the same load, so the first fixed tick finds it done.
pub(super) fn early_load_ped_data(
    config: Res<crate::config::Config>,
    map: Res<crate::map_transition::CurrentMap>,
    mut data: ResMut<PedData>,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
    settings: Res<LivingWorldSettings>,
    nav: Res<PedNavSettings>,
) {
    if load_ped_data_for_map(&config, &map, &mut data, audio, &settings, &nav) {
        data.skip_arms_once = true;
    }
}

/// Load the map's ped data unless it is loaded already; true when it loaded.
fn load_ped_data_for_map(
    config: &crate::config::Config,
    map: &crate::map_transition::CurrentMap,
    data: &mut PedData,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
    settings: &LivingWorldSettings,
    nav: &PedNavSettings,
) -> bool {
    let key = (map.name.clone(), map.generation);
    if data.loaded_for.as_ref() == Some(&key) {
        return false;
    }
    // Where the first fixed tick after a map switch goes (low-end PCs: 141 ms here on the desktop, 2026-10-17).
    let t0 = std::time::Instant::now();
    // Parsed once for every reader below (1.8 MB; the eleven readers parsed it again each before, about 85 ms on the
    // desktop). A reader given none reads it itself, so a missing or broken file reports the same errors as before.
    let tables = std::fs::read(config.asset_root.join("private/living_world/tables.json")).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| e.to_string()));
    let mut loaded = PedData::load_with(&config.asset_root, tables.as_ref().ok());
    let t_data = t0.elapsed();
    loaded.load_nav(&config.asset_root, &map.name, &nav.rules);
    let t_nav = t0.elapsed();
    match super::ped_graph::PedGraph::load(&config.asset_root, super::ped_graph::AI_GRAPH) {
        Ok(graph) => {
            let pending = graph.pending();
            info!(
                "PED_GRAPH loaded: {} behaviours, {} conditions; {} operation names not ported yet: {}",
                graph.behaviors.len(),
                graph.conditions.len(),
                pending.len(),
                pending.keys().cloned().collect::<Vec<_>>().join(", ")
            );
            loaded.graph = Some(Arc::new(graph));
            match super::ped_graph::PedGraph::load(&config.asset_root, super::ped_graph::CONVERSATION_GRAPH) {
                Ok(g) => {
                    info!("PED_GRAPH conversation plugin: {} behaviours, {} conditions; not ported yet: {:?}", g.behaviors.len(), g.conditions.len(), g.pending().keys().collect::<Vec<_>>());
                    loaded.conversation_graph = Some(Arc::new(g));
                }
                Err(error) => warn!("PED_GRAPH conversation plugin not loaded (no conversations): {error}"),
            }
        }
        Err(error) => warn!("PED_GRAPH not loaded (peds keep wandering without the behaviour graph): {error}"),
    }
    let t_graphs = t0.elapsed();
    match tables.as_ref().map_err(String::clone).and_then(|b| { use super::ped_mood::*; Ok((parse_in(&b)?, reaction_sets_in(&b), chase_records_in(&b), takedown_tables_in(&b), sight_in(&b), conversation_tables_in(&b), sit_values_in(&b), starting_hand_props_in(&b))) }) {
        Ok((tables, sets, (chase, global, zombie), (takedowns, takedown_zombie), (sight, sight_scales), conversations, sit, starting_props)) => {
            info!(
                "PED_MOOD tables: {} categories, {} results, {} reaction sets, {} entity types, {} chase records (global {}, zombie {}, zombie takedowns {})",
                tables.categories.len(),
                tables.results.len(),
                tables.reactions.len(),
                sets.len(),
                chase.len(),
                global.is_some(),
                zombie.is_some(),
                takedown_zombie.is_some()
            );
            loaded.mood = Some(Arc::new(tables));
            loaded.reaction_sets = Arc::new(sets);
            loaded.chase = Arc::new(chase);
            loaded.chase_global = global.map(Arc::new);
            loaded.chase_zombie = zombie.map(Arc::new);
            loaded.takedown_zombie = takedown_zombie.map(Arc::new);
            loaded.takedowns = Arc::new(takedowns);
            loaded.sight = Arc::new(sight);
            loaded.sight_scales = Arc::new(sight_scales);
            loaded.conversations = Arc::new(conversations);
            loaded.sit = Arc::new(sit);
            loaded.starting_props = Arc::new(starting_props);
        }
        Err(error) => warn!("PED_MOOD tables not loaded (no mood reactions): {error}"),
    }
    let t_tables = t0.elapsed();
    match super::ped_plugins::PluginData::load_with(&config.asset_root, tables.as_ref().ok()) {
        Ok(mut p) => {
            // The map's DMO hotpoint props (benches, bins, newspaper boxes) beside the placed waypoint groups.
            let seats = super::ped_plugins::map_props(&config.asset_root, &map.name);
            info!("PED_PLUGINS {}: {} hotpoint props ({} seats)", map.name, seats.len(), seats.iter().filter(|s| s.class == "waypoint_sit").count());
            p.placed.entry(map.name.clone()).or_default().extend(seats);
            info!(
                "PED_PLUGINS {} prop classes ({} with descriptors), {} ped types with plugin_odds, placed props {:?}",
                p.classes.len(),
                p.classes.values().filter(|c| c.descriptor.is_some()).count(),
                p.odds.len(),
                p.placed.iter().map(|(k, v)| (k.clone(), v.len())).collect::<Vec<_>>()
            );
            // Each class's plugin graph (`plugin/<name>.xml` compiled to `.stategraph`).
            for (class, c) in &p.classes {
                let Some(d) = c.descriptor.as_ref().filter(|d| !d.graph.is_empty() && class != "waypoint_conversation") else { continue };
                let relative = format!("private/stock/data/{}", d.graph.replace('\\', "/").trim_end_matches(".xml").to_string() + ".stategraph");
                match super::ped_graph::PedGraph::load(&config.asset_root, &relative) {
                    Ok(g) => {
                        info!("PED_GRAPH plugin {class}: {} behaviours, {} conditions; not ported yet: {:?}", g.behaviors.len(), g.conditions.len(), g.pending().keys().collect::<Vec<_>>());
                        loaded.plugin_graphs.insert(class.clone(), Arc::new(g));
                    }
                    Err(error) => warn!("PED_GRAPH plugin {class} not loaded: {error}"),
                }
            }
            loaded.plugins = Arc::new(p);
        }
        Err(error) => warn!("PED_PLUGINS not loaded (no plugin props): {error}"),
    }
    let t_plugins = t0.elapsed();
    match super::ped_hand_props::HandPropData::load_with(&config.asset_root, tables.as_ref().ok()) {
        Ok(h) => {
            info!("PED_HAND_PROPS {} records, {} with models", h.props.len(), h.props.values().filter(|p| p.glb.is_some()).count());
            loaded.hand_props = Arc::new(h);
        }
        Err(error) => warn!("PED_HAND_PROPS not loaded (no hand props): {error}"),
    }
    let ms = |a: std::time::Duration, b: std::time::Duration| (b - a).as_secs_f64() * 1000.0;
    let t_end = t0.elapsed();
    info!(
        "PED_DATA_LOAD {}: {:.1} ms (bodies {:.1}, nav {:.1}, graphs {:.1}, tables {:.1}, plugins {:.1}, hand props {:.1})",
        map.name, t_end.as_secs_f64() * 1000.0, t_data.as_secs_f64() * 1000.0, ms(t_data, t_nav), ms(t_nav, t_graphs), ms(t_graphs, t_tables), ms(t_tables, t_plugins), ms(t_plugins, t_end)
    );
    info!("LIVING_WORLD {}", loaded.status);
    loaded.loaded_for = Some(key);
    if let Some(mut audio) = audio {
        audio.expected |= settings.enabled && settings.pedestrians.enabled && loaded.ready();
    }
    *data = loaded;
    true
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
    bodies: Query<&PedBody>,
    observers: Option<Res<super::LivingWorldObservers>>,
) {
    for LivingWorldDespawn(r) in despawns.read() {
        if r.id.kind != Kind::Pedestrian {
            continue;
        }
        if let Some(e) = index.0.remove(&r.id) {
            // Where the ped was and how far from the census focus, so a "vanished up close" report can be checked.
            let at = bodies.get(e).ok().map(|b| b.position);
            let focus = observers.as_deref().and_then(|o| o.observers.first()).map(|o| Vec3::from_array(o.position));
            let distance = at.zip(focus).map(|(a, f)| a.distance(f));
            info!(
                "PED_DESPAWN ped=#{} reason={:?} tick={} at={:?} focus={:?} distance={}",
                r.id.serial, r.reason, r.tick, at.map(|p| [p.x, p.y, p.z]), focus.map(|p| [p.x, p.y, p.z]),
                distance.map_or("none".to_string(), |d| format!("{d:.1}"))
            );
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
            // The census ring point carries the observer's height, not the floor's
            // (census::ring_point). Where the navmesh has no floor within locate_height of it
            // (the player on a ledge, ramp or roof), the ped kept that height and hung in the air
            // or under the ground (2026-10-06 sessions: 32 of 34 and 44 of 44 PED_FLOATING peds had
            // no navmesh polygon). Release the spawn like an unresolved look; the census tries
            // another point next pass. Not retail yet: retail's spawn validation is not decoded.
            None if data.nav.is_some() => {
                events.write(PedEvent::Rejected { id: s.id, category: category.clone() });
                rejected.0.push(s.id);
                continue;
            }
            None => {
                if let Some(y) = ground(physics.as_deref(), at, 3.0, 3.0) {
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
        let body = PedBody { player, path: TestPath::new(s.seed), nav, blocked: 0.0, position: at, velocity: Vec3::ZERO, heading: s.heading, ticks: 0, feet_down: [false; 2], body_fall: 0.0, taunt: TauntClip::None, plugin_motion: PluginMotionRun::default() };
        let e = commands
            .spawn((
                Name::new(format!("Pedestrian {} ({})", s.id.serial, look.recipe)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(s.heading)),
                Visibility::Inherited,
                PedAudio { voice: look.voice, ..Default::default() },
                // Spawn fade in starts at 0 (retail `+576`); `present_ped_pose` raises it.
                super::npc_skaters::NpcFade { alpha: 0.0, ..Default::default() },
                body,
                PedMind::default(),
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
    mut peds: Query<(&Pedestrian, &mut PedBody, &mut Transform, &mut PedAudio, Option<&PedMind>)>,
    mut events: MessageWriter<PedEvent>,
    (mut floating_logged, mut blocked_logged): (Local<std::collections::HashMap<LivingWorldId, u64>>, Local<std::collections::HashMap<LivingWorldId, u64>>),
    trace: Res<PedObstacleTrace>,
    skater_runtime: Option<Res<crate::physics::SkaterRuntime>>,
    observers: Res<super::LivingWorldObservers>,
    npcs: Query<(&super::npc_skaters::NpcSkater, &super::npc_skaters::NpcReplay)>,
    settings: Res<LivingWorldSettings>,
    mut ped_contacts: Local<Vec<PedContact>>,
    mut played_cues: Local<BTreeMap<LivingWorldId, u32>>,
) {
    let _span = super::timing::span(super::timing::ADVANCE_PEDS);
    let obstacles = &obstacles.0;
    let hz = state.world.clock().hz as u64;
    let tick = state.world.tick();
    let dt = tick_seconds(state.world.clock().hz);
    let mut list: Vec<_> = peds.iter_mut().collect();
    list.sort_by_key(|(p, ..)| p.id);
    let mut neighbours: Vec<Neighbour> = list.iter().map(|(p, b, ..)| Neighbour { order: id_order(p.id), position: b.position.to_array() }).collect();
    let ids: Vec<LivingWorldId> = list.iter().map(|(p, ..)| p.id).collect();
    let pending = std::mem::take(&mut *ped_contacts);
    // Skater touchers (`IActor` owners, contact kind 5 with the skater thresholds): the players, then the NPC skaters
    // in id order (the user, 2026-10-16: NPC skaters knock peds over in retail).
    let mut actors: Vec<(u64, [f32; 3], [f32; 3])> = observers.observers.iter().enumerate().map(|(i, o)| (PLAYER_TARGET_BASE + i as u64, o.position, o.velocity)).collect();
    let mut npc_actors: Vec<(u64, [f32; 3], [f32; 3])> = npcs.iter().filter_map(|(n, r)| r.last.as_ref().map(|s| (n.id.to_u64(), s.position, s.velocity))).collect();
    npc_actors.sort_by_key(|a| a.0);
    actors.extend(npc_actors);
    let clock = traffic.as_ref().and_then(|t| t.clock.as_ref());
    let road_signals = clock.map(|clock| skate_core::living_world::peds::crosswalk::RoadWalkSignals { arms: &data.arms, clock, radius: 20.0 });
    let signals: &dyn WalkSignals = match &road_signals {
        Some(s) => s,
        None => &NoSignals,
    };
    for (k, (ped, mut body, mut transform, mut audio, mind)) in list.into_iter().enumerate() {
        // OverrideAnimData (zombie mode: the `zombie` set) wins over the ped's own set while its state runs.
        let anim_set = mind.and_then(|m| m.brain.anim_override.as_deref()).filter(|n| data.anim_sets.contains_key(*n)).unwrap_or(&ped.anim_set);
        let Some(set) = data.anim_sets.get(anim_set).or_else(|| data.anim_sets.get("default")) else { continue };
        let target = tick.saturating_sub(ped.spawn_tick);
        let body = &mut *body;
        let me = id_order(ped.id);
        while body.ticks < target {
            let mut turn = 0.0;
            let step_from = body.position;
            // The taunt clip (motiongraph_taunt `PlayTaunt`: the remapped "Taunt" once, blend 0.1, then
            // `MajorIntentComplete`); a set without the clip completes at once.
            match body.taunt {
                TauntClip::Requested if body.player.state != Locomotion::Reaction => {
                    let step = skate_core::living_world::peds::skater_contact::ReactionStep { anim: "Taunt", mirror: false, blend: 0.1, cycle: false };
                    body.taunt = if body.player.react(set, vec![step], 0.0) { TauntClip::Playing } else { TauntClip::Done };
                }
                TauntClip::Playing if body.player.state != Locomotion::Reaction => body.taunt = TauntClip::Done,
                _ => {}
            }
            // A hand prop throw clip (`82E3E648`, ped vfunc +240 = the channel request `82E3A418`): a channel clip (its
            // weights pick the upper body, b99), played once over locomotion; it replaces the carry pose, which comes back
            // once the slot is free and the prop is still held.
            if let Some((clip, blend)) = body.plugin_motion.throw_clip.take() {
                if !body.player.channel_request(set, clip, blend, blend) {
                    info!("PED_HAND_PROP ped=#{} no clip {clip} tick={tick}", ped.id.serial);
                }
            }
            // The tazer's draw / cycle / holster clip (ped vfn248, blends 0.2, b248).
            if let Some(clip) = body.plugin_motion.tazer_clip.take() {
                let blend = settings.ped_brain.values.tazer_clip_blend;
                let ok = body.player.channel_request(set, clip, blend, blend);
                body.plugin_motion.tazer_played = Some((clip, ok));
                info!("PED_TAZER_CLIP ped=#{} {clip}{} tick={tick}", ped.id.serial, if ok { "" } else { " (not in the anim set)" });
            }
            // A plugin state's clips (sit, ATM, vending machine, water fountain, newspaper box).
            let run = &mut body.plugin_motion;
            // The Avoid graph's SitJump out of the sitting cycle (`think_peds` raises `sit_avoid`).
            if mind.is_some_and(|m| m.sit_avoid) && body.player.reaction_anim() == Some(SIT_CYCLE) {
                let steps = vec![skate_core::living_world::peds::skater_contact::ReactionStep { anim: SIT_AVOID_CLIP, mirror: false, blend: 0.1, cycle: false }];
                if body.player.interrupt(set, steps, 0.0) {
                    run.state = TauntClip::Done;
                }
            }
            match (run.state, run.packet.as_deref().and_then(skate_core::living_world::peds::plugin_motion::motion_for)) {
                (TauntClip::Requested, Some(m)) if !body.player.reacting() => {
                    use skate_core::living_world::peds::plugin_motion::BLEND;
                    let steps = m.steps.iter().map(|st| skate_core::living_world::peds::skater_contact::ReactionStep { anim: st.anim, mirror: false, blend: BLEND, cycle: st.hold_until.is_some() }).collect();
                    run.state = if body.player.react(set, steps, f32::INFINITY) { TauntClip::Playing } else { TauntClip::Done };
                }
                (TauntClip::Playing, m) => {
                    if std::mem::take(&mut run.release) || run.orphaned {
                        body.player.release_hold();
                    }
                    // VendHandProp / CollectHandProp: the collect clip's branch window (`InTurnBranchWindow`).
                    let collect = m.and_then(|m| m.steps.iter().find(|st| st.hand_prop)).map(|st| st.anim);
                    if !run.hand_prop_taken && collect.is_some() && body.player.reaction_anim() == collect && body.player.in_branch_window() {
                        run.hand_prop_due = true;
                        run.hand_prop_taken = true;
                    }
                    if !body.player.reacting() {
                        run.state = TauntClip::Done;
                    }
                }
                _ => {}
            }
            // A skater running into the ped (retail `sub_82E38FB8` kind 5). Retail skips the contact only while the ped is
            // already in a collision reaction (controller byte +9), not during any clip: a ped sitting on a bench or
            // using a prop (its plugin clip plays as a reaction) can be knocked over.
            let clip_reaction = body.taunt == TauntClip::Playing || body.plugin_motion.state == TauntClip::Playing;
            let in_collision = mind.is_some_and(|m| m.brain.colliding) || (body.player.state == Locomotion::Reaction && !clip_reaction);
            if !in_collision {
                let radius = data.nav.as_deref().map_or(super::vehicle_contacts::FALLBACK_PED_RADIUS, |m| m.agent[1]);
                // Contacts from the entity the ped ignores (`ped+5680`, its takedown target) are dropped (`sub_82E38400`).
                let ignored = mind.and_then(|m| m.brain.ignore_collisions_with);
                let rules = contact_rules(&set.collision, mind.is_some_and(|m| m.brain.heavy_collision_disabled));
                let ignore_standing = mind.is_some_and(|m| m.brain.ignore_standing_collisions);
                // The local skater meets a near ped's body in its own solve (`ped_body::push_ped_proxies`, group 11):
                // the reaction comes from those contacts, as retail's comes from the Havok contact (b185). Before
                // 2026-10-17 the distance test below knocked the ped down about 0.35 m before the bodies met and the
                // body left the solve, so a fast hit never bailed the skater. Other skaters and peds without a body
                // keep the distance test (NOT RETAIL).
                let local = PLAYER_TARGET_BASE;
                let body_in_solve = skater_runtime.is_some() && settings.ped_body.enabled && !settings.ped_body.parts.is_empty()
                    && observers.observers.first().is_some_and(|o| Vec3::from_array(o.position).distance(body.position) <= settings.ped_body.range);
                let solve_hit = || {
                    let observer = observers.observers.first()?;
                    if ignored == Some(local) {
                        return None;
                    }
                    // Retail reads the ped part's linear and angular speed after the solve (|v| / |w| > 3.0).
                    let runtime = skater_runtime.as_ref()?;
                    let mine = |h: &&(u64, [f32; 3], [f32; 2])| super::ped_body::proxy_serial(h.0) == Some(ped.id.serial);
                    // On board and on foot the skeleton's solve reports each part it touched with the part's speed
                    // after the solve (`skeleton_feedback::publish`). Any touch counts, but only the reaction part's
                    // speed decides, whichever part was touched (retail: the torso; b283). Without it (no such part)
                    // the fastest touched part decides (ours).
                    let touched_max = runtime.ped_hits.iter().filter(mine).map(|h| {
                        info!("PED_BODY_CONTACT ped=#{} part={} part_speed={:.2} part_spin={:.2} tick={tick}", ped.id.serial, h.0 & 3, h.2[0], h.2[1]);
                        h.2
                    }).fold(None, |m: Option<[f32; 2]>, s| Some(m.map_or(s, |m| [m[0].max(s[0]), m[1].max(s[1])])))?;
                    let reaction = super::ped_body::reaction_part_speed(&runtime.ped_after, ped.id.serial, &settings.ped_body);
                    let speed = reaction.unwrap_or(touched_max);
                    info!("PED_REACTION_BODY ped=#{} part={} speed={:.2} spin={:.2} touched_max_speed={:.2} touched_max_spin={:.2} tick={tick}",
                        ped.id.serial, if reaction.is_some() { settings.ped_body.reaction_part.to_string() } else { "touched_max".into() },
                        speed[0], speed[1], touched_max[0], touched_max[1]);
                    let closing = speed[0];
                    let d = [body.position.x - observer.position[0], body.position.z - observer.position[2]];
                    let len = d[0].hypot(d[1]).max(1e-4);
                    let n = [d[0] / len, d[1] / len];
                    use skate_core::living_world::peds::skater_contact::{reaction_direction, skater_reaction};
                    let Some(kind) = skater_reaction(speed, [0.0, 0.0], &rules, ignore_standing) else {
                        // The contact decided nothing (below the reaction rules, or a standing ped ignored).
                        info!("PED_BODY_CONTACT_NO_REACTION ped=#{} closing={closing:.2} ignore_standing={ignore_standing} tick={tick}", ped.id.serial);
                        return None;
                    };
                    Some(((kind, reaction_direction(n, [body.heading.sin(), body.heading.cos()]), closing), local))
                };
                let skater = actors.iter()
                    .filter(|(id, ..)| ignored != Some(*id) && !(body_in_solve && *id == local))
                    .find_map(|&(id, at, v)| skater_hit(body.position, body.heading, radius, at, v, &rules, ignore_standing).map(|h| (h, id)));
                let skater = if body_in_solve { solve_hit().or(skater) } else { skater };
                let touched = || pending.iter()
                    .filter(|c| c.touched == ped.id && ignored != Some(c.toucher.to_u64()))
                    .find_map(|c| ped_hit(body.position, body.heading, c.toucher_at, c.velocity, &rules, ignore_standing, settings.zombie).map(|h| (h, c.toucher.to_u64())));
                if let Some(((kind, direction, closing), toucher)) = skater.or_else(touched) {
                    let steps = skate_core::living_world::peds::skater_contact::reaction_steps(kind, direction);
                    if body.player.react(set, steps, set.collision.ground_seconds) {
                        info!("PED_SKATER_CONTACT ped=#{} by={toucher} kind={} direction={} closing={closing:.2} at=[{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, kind.name(), direction.name(), body.position.x, body.position.y, body.position.z);
                        events.write(PedEvent::Hit { id: ped.id, kind, direction, closing, toucher });
                    }
                }
            }
            // The behaviour graph stops the ped and faces a point (StopAndFaceWantTarget,
            // watching, StandAndWatchSkater: speed suggestion 0.0 and the face point): stand and
            // turn towards it at the nav's turn rate (ours until the motion graph's turn
            // branches run), instead of the wander step.
            let face = mind.filter(|m| m.brain.speed_suggestion == Some(0.0)).and_then(|m| m.brain.face);
            // LockToCurrentPosition: stand where the ped is (turning only for a face point).
            // DisableNavigation (ped `+5936` bit 0x80 clear): no navigation, the ped stands the same way.
            let locked = mind.is_some_and(|m| m.brain.position_locked || m.brain.navigation_disabled);
            // TargetWaypoint's slide: within the slide distance the think step moves the ped onto the point; the body
            // stands (no wander target while the route is off).
            let sliding = mind.is_some_and(|m| {
                m.brain.approach.zip(m.brain.approach_slide).is_some_and(|((p, _), (slide, _))| (p[0] - body.position.x).hypot(p[2] - body.position.z) <= slide)
            });
            match data.nav.as_deref() {
                _ if body.player.state == Locomotion::Reaction => body.player.intent = skate_core::living_world::peds::anim::Intent::Idle,
                _ if (locked || sliding) && face.is_none() => {
                    body.player.intent = skate_core::living_world::peds::anim::Intent::Idle;
                    // The explicit turn direction (`+2100` bit 0x20): a ped standing on its waypoint keeps turning to
                    // the waypoint's orientation.
                    if let Some(dir) = mind.and_then(|m| m.brain.explicit_turn).filter(|d| d[0] * d[0] + d[2] * d[2] > 1e-6) {
                        let mut error = dir[0].atan2(dir[2]) - body.heading;
                        while error > std::f32::consts::PI {
                            error -= std::f32::consts::TAU;
                        }
                        while error < -std::f32::consts::PI {
                            error += std::f32::consts::TAU;
                        }
                        let step = nav_settings.wander.turn_rate * dt;
                        turn = error.clamp(-step, step);
                    }
                }
                _ if face.is_some() => {
                    let p = face.unwrap_or_default();
                    let d = [p[0] - body.position.x, p[2] - body.position.z];
                    body.player.intent = skate_core::living_world::peds::anim::Intent::Idle;
                    if d[0] * d[0] + d[1] * d[1] > 1e-6 {
                        let desired = d[0].atan2(d[1]);
                        let mut error = desired - body.heading;
                        while error > std::f32::consts::PI {
                            error -= std::f32::consts::TAU;
                        }
                        while error < -std::f32::consts::PI {
                            error += std::f32::consts::TAU;
                        }
                        let step = nav_settings.wander.turn_rate * dt;
                        turn = error.clamp(-step, step);
                    }
                }
                Some(mesh) => {
                    // The graph's Pedestrian nav modifier off (a chase): no avoiding other peds.
                    let avoid_peds = mind.is_none_or(|m| m.brain.nav_modifier(skate_core::living_world::peds::brain::nav_modifier::PEDESTRIAN));
                    let neighbours_now: &[_] = if avoid_peds { &neighbours } else { &[] };
                    let out = body.nav.step_avoiding(mesh, &nav_settings.wander, nav_settings.crosswalk, signals, me, body.position.to_array(), body.heading, body.player.state, neighbours_now, Some(obstacles), dt);
                    // Fleeing: run (the chase run cycle) where the nav walks.
                    let fleeing = mind.is_some_and(|m| m.flee_goal.is_some() || m.chase_goal.is_some());
                    body.player.intent = if fleeing && out.intent == skate_core::living_world::peds::anim::Intent::Walk { skate_core::living_world::peds::anim::Intent::Run } else { out.intent };
                    turn = out.turn;
                    if out.turn_degrees != 0 {
                        use skate_core::living_world::peds::anim::{names, Intent};
                        let walking = body.player.state == Locomotion::Walk;
                        let clip = if walking { names::walk_turn(out.turn_degrees) } else { names::stand_turn(out.turn_degrees) };
                        if set.entries.contains_key(clip) {
                            body.player.turn_degrees = out.turn_degrees;
                        } else {
                            // Ours: a set without the bucket's clip (a mod set, a test set) pivots / steers instead.
                            body.player.intent = if walking { Intent::Walk } else { Intent::Idle };
                            turn = out.steer;
                        }
                    }
                }
                None => body.player.intent = body.path.intent(dt, body.player.state),
            }
            // The carry pose (`82E3F048`): while the hand prop is held and the channel is free, its record's carry
            // animation is requested again (blend 0.2); a released or dropped prop stops it with a 0.2 s fade (unlink
            // `82E3FAE0`). Retail requests it only while the prop is linked; ours: while held.
            if let Some(h) = mind.map(|m| &m.brain.hand_prop) {
                let carry = h.key.as_ref().and_then(|k| data.hand_props.props.get(k)).and_then(|r| r.carry_channel.as_deref());
                // The idle action's cue (`82E3F470`): a new cue plays once; the talk / read cycles are asked for every
                // frame in retail, so ours asks again when their clip ends early.
                use skate_core::living_world::peds::hand_prop::ActionCue;
                if let Some((cue, seq)) = mind.and_then(|m| m.action_cue.map(|c| (c, m.action_seq))) {
                    let blend = settings.ped_brain.values.hand_prop.actions.clip_blend;
                    let new = played_cues.get(&ped.id) != Some(&seq);
                    match cue {
                        ActionCue::Clip(name) if new || (body.player.channel_idle() && matches!(name, "HandPropPhoneTalk" | "HandPropReadCyc")) => {
                            body.player.channel_request(set, name, blend, blend);
                        }
                        ActionCue::Carry if new => {
                            if let Some(name) = carry {
                                body.player.channel_request(set, name, blend, blend);
                            }
                        }
                        _ => {}
                    }
                    played_cues.insert(ped.id, seq);
                }
                match (h.holding, carry) {
                    (true, Some(name)) if body.player.channel_idle() => {
                        body.player.channel_request(set, name, 0.2, 0.2);
                    }
                    (false, _) if body.player.channel_clip().is_some_and(|c| c.starts_with("NPC_CARRY")) => body.player.channel_stop(0.2),
                    _ => {}
                }
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
                    // Never into a prop or mod body: slide along its face or stay (fix 11). A ped in a plugin whose
                    // `DisableCollisionsWithBehaviourSource` is active ignores the plugin's own prop (its body id is the
                    // plugin prop id's low 32 bits), so it can reach a seat inside its bench.
                    let ignore = skate_core::living_world::peds::obstacles::Ignore {
                        id: mind.filter(|m| m.brain.ignore_source_collision).and_then(|m| m.prop).map(|(_, id, _)| u64::from(id as u32)),
                        // SkaterAvoidance off (ChangeNavModifierSetting index 2): the skaters are not obstacles.
                        skaters: mind.is_some_and(|m| !m.brain.nav_modifier(skate_core::living_world::peds::brain::nav_modifier::SKATER_AVOIDANCE)),
                    };
                    let (next, clear) = match obstacles.resolve_step_ignoring(from, next, mesh.agent[1], ignore) {
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
                    let blocker = separation_blocker(body.position.to_array(), next, me, &neighbours, mesh.agent[1]);
                    let ok = on_mesh && clear && progressed && blocker.is_none() && crosswalk_ok(mesh, nav_settings.crosswalk, signals, body.position.to_array(), next);
                    // Refused by another ped: the two bodies touch; both react to the other on their next step.
                    if let Some(j) = blocker.filter(|_| moving) {
                        let v = ((to - body.position) / dt).with_y(0.0).to_array();
                        ped_contacts.push(PedContact { touched: ids[j], toucher: ped.id, toucher_at: from, velocity: v });
                        ped_contacts.push(PedContact { touched: ped.id, toucher: ids[j], toucher_at: neighbours[j].position, velocity: [-v[0], 0.0, -v[2]] });
                    }
                    // Moved-prop diagnostics: a refused step into a prop, or anywhere near the spot a
                    // moved prop left (once per ped per second).
                    if !ok && moving {
                        let by = obstacles.blocker_at(to.to_array(), mesh.agent[1]);
                        let near = trace.moved.iter().find(|(_, c)| (c[0] - from[0]).hypot(c[2] - from[2]) < 3.0).map(|(id, _)| *id);
                        let now = state.world.tick();
                        if (by.is_some() || near.is_some()) && blocked_logged.get(&ped.id).is_none_or(|t| now >= t + hz) {
                            blocked_logged.insert(ped.id, now);
                            // The blocker as the log reader sees it: the prop body's model and where it is (its id is a
                            // prop body id, not a waypoint group id; 2026-10-14 a tree planter read as a newspaper box).
                            let blocker = by.filter(|id| *id < MOD_BODY_OBSTACLE_BASE).and_then(|id| {
                                let d = physics.as_deref()?.prop_dynamics()?;
                                let (c, ..) = d.body_state(id as u32)?;
                                // "<id>/DMO_DT_BenchFlat_1015_0x..." -> "DMO_DT_BenchFlat_1015".
                                let t = d.template_of(id as u32)?;
                                let model = t.split_once('/').map_or(t, |x| x.1).split("_0x").next().unwrap_or(t);
                                Some(format!("{model} at [{:.2}, {:.2}, {:.2}]", c.x, c.y, c.z))
                            });
                            let blocker = blocker.unwrap_or_else(|| if by.is_some_and(|id| id >= MOD_BODY_OBSTACLE_BASE) { "mod body".into() } else { "-".into() });
                            info!(
                                "PED_BLOCKED ped=#{} at=[{:.2}, {:.2}, {:.2}] to=[{:.2}, {:.2}, {:.2}] by={by:?} ({blocker}) near_moved_spawn={near:?} on_mesh={on_mesh} clear={clear} progressed={progressed} tick={now}",
                                ped.id.serial, from[0], from[1], from[2], to.x, to.y, to.z,
                            );
                        }
                    }
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
            body.velocity = (body.position - step_from) / dt;
            body.ticks += 1;
            if let Some(s) = out.entered {
                events.write(PedEvent::State { id: ped.id, state: s });
            }
        }
        // The render ground: a line query round the navmesh height. It never feeds back into the
        // navigation position (fix 17). Upward it only searches the NavPower step height (agent
        // block [2], 0.2 m [data]): NavPower keeps its polygons within one step of the walkable
        // floor, and a window of one agent height (1.6 m) took the first hit from the top, so an
        // awning, ledge, sign or invisible collision overhead drew the ped floating on it
        // (2026-10-06 session, peds in the air all over DownTown). Downward one agent height.
        // Not retail yet: retail's own ped render placement is not decoded.
        let mut shown = body.position;
        let (up, down) = data.nav.as_deref().map_or((3.0, 3.0), |m| (m.agent[2].max(0.05), m.agent[3].max(0.5)));
        // Retail's NavPower mover probe (`82EA5F48`, debug bool `probe_for_ground` on by default) casts again with
        // 4x the reach after a miss (`0x82257308` 4.0) [code, b107]: a coarse navmesh triangle can sit 1.7 m above the
        // pavement (DownTown poly 7376). Upward stays one step so overhead geometry is still never taken.
        if let Some(y) = ground(physics.as_deref(), body.position, up, down).or_else(|| ground(physics.as_deref(), body.position, up, down * GROUND_RETRY_FACTOR)) {
            shown.y = y;
        }
        transform.translation = shown;
        // Floating check, always on (2026-10-06: floating peds reported four times with nothing in
        // the log). Every 2 s per ped, staggered: a deep probe finds the floor under the drawn ped;
        // drawn more than 0.3 m above it, or over no floor, logs PED_FLOATING with the position,
        // navmesh polygon and its area code (once per ped per 10 s).
        if (body.ticks + ped.id.serial as u64) % 120 == 0 {
            let floor = ground(physics.as_deref(), shown, 0.2, 30.0);
            if floor.map_or(true, |y| shown.y - y > 0.3)
                && floating_logged.get(&ped.id).map_or(true, |t| tick >= t + 600)
            {
                floating_logged.insert(ped.id, tick);
                let area = body.nav.poly.and_then(|k| data.nav.as_deref().and_then(|m| m.polys.get(k as usize)).map(|p| p.area));
                warn!(
                    "PED_FLOATING ped=#{} model={} drawn=[{:.2}, {:.2}, {:.2}] nav_y={:.2} floor_y={} gap={} poly={:?} area={:?} tick={tick}",
                    ped.id.serial, ped.model, shown.x, shown.y, shown.z, body.position.y,
                    floor.map_or("none".to_string(), |y| format!("{y:.2}")),
                    floor.map_or("none".to_string(), |y| format!("{:.2}", shown.y - y)),
                    body.nav.poly, area,
                );
            }
        }
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
    pub(crate) scene: Option<Entity>,
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
    mut peds: Query<(&Pedestrian, &PedBody, &mut PedPuppet, Option<&mut super::npc_skaters::NpcFade>, Option<&super::ped_hand_props::HeldHandProp>)>,
    mut joints: Query<&mut Transform, Without<PedBody>>,
    mut vis: Query<&mut Visibility>,
) {
    let hz = state.world.clock().hz;
    let ahead = ((state.world.clock().overstep() + fixed.overstep_fraction() as f64 * fixed.timestep().as_secs_f64() * hz).clamp(0.0, 1.0) as f32) * tick_seconds(hz);
    let camera = cameras.iter().next().map(|c| c.translation());
    let hand_bone = data.rig.names.iter().position(|n| n.eq_ignore_ascii_case(super::ped_hand_props::HAND_PROP_BONE));
    for (ped, body, mut puppet, fade, held) in &mut peds {
        let Some(bindings) = puppet.bindings.as_ref() else { continue };
        let Some(globals) = ped_globals(&data.rig, body, &*data, ahead, &puppet.followers) else { continue };
        // The held hand prop at the hand bone's native frame and the record's offset (`82E3E4F0`, every frame).
        if let (Some(h), Some(bone)) = (held, hand_bone.and_then(|i| globals.get(i)))
            && let Ok(mut t) = joints.get_mut(h.entity)
        {
            *t = Transform::from_matrix(*bone * h.local);
        }
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

fn log_ped_readout(state: Res<PopulationState>, observers: Res<super::LivingWorldObservers>, peds: Query<(&Pedestrian, &PedBody, Option<&PedMind>)>, mut last: Local<u64>) {
    // Always on (was debug only, so sessions never had ped positions): every 2.5 s the count, the
    // nearest ped and the player position, so a reported spot can be found in the log.
    if !super::report_due(state.world.tick(), &mut last, 150) {
        return;
    }
    let list: Vec<_> = peds.iter().map(|(p, b, _)| (p.id, p.recipe.clone(), b.player.state, b.position)).collect();
    // Zombie mode: how many peds are on ZombieFollow (0 lines out of the mode).
    let zombies = peds.iter().filter(|(_, _, m)| m.is_some_and(|m| m.brain.motion_intent == Some(skate_core::living_world::peds::brain::motion::ZOMBIE_FOLLOW))).count();
    // What the peds are doing (session checks of "too much sitting / gathering / no chasing"): per plugin class
    // (conversation included), chasing, ZombieFollow, and the rest (free: wander, greet, react).
    let mut activity: BTreeMap<String, u32> = BTreeMap::new();
    for (_, _, m) in &peds {
        let key = match m {
            Some(m) if m.brain.chasee.is_some() => "chasing".to_owned(),
            Some(m) if m.brain.in_plugin => m.plugin_graph.clone().unwrap_or_else(|| "plugin".to_owned()),
            Some(m) if m.brain.motion_intent == Some(skate_core::living_world::peds::brain::motion::ZOMBIE_FOLLOW) => "zombie_follow".to_owned(),
            _ => "free".to_owned(),
        };
        *activity.entry(key).or_default() += 1;
    }
    let activity = activity.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" ");
    let player = observers.observers.first().map(|o| Vec3::from_array(o.position));
    let nearest = player.and_then(|at| list.iter().min_by(|a, b| a.3.distance(at).total_cmp(&b.3.distance(at))).map(|n| n.3));
    info!(
        "LIVING_WORLD {} player={:?} nearest_ped_at={:?}{} activity: {activity}",
        ped_readout(&list, player),
        player.map(|p| [p.x, p.y, p.z]),
        nearest.map(|p| [p.x, p.y, p.z]),
        if zombies > 0 { format!(" zombie_followers={zombies}") } else { String::new() },
    );
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<PedData>()
        .init_resource::<PedLooks>()
        .init_resource::<PedIndex>()
        .init_resource::<super::ped_hand_props::ReleasedHandProps>()
        .init_resource::<PedRejected>()
        .init_resource::<PedNavSettings>()
        .add_systems(PreUpdate, sync_nav_settings)
        .init_resource::<PedObstacles>()
        .init_resource::<PedObstacleTrace>()
        .add_message::<PedEvent>()
        .add_message::<crate::world_audio::PedSpeechEvent>()
        .add_message::<SkaterTakedownRequest>()
        .init_resource::<PedChaseGroups>()
        .init_resource::<PedConversations>()
        .init_resource::<PedPluginProps>()
        .add_systems(FixedUpdate, (load_ped_data, apply_ped_records, release_rejected, update_ped_obstacles, think_peds, super::ped_hand_props::sync_hand_props, apply_skater_takedowns, advance_peds, log_ped_readout).chain().after(super::step_population))
        .init_resource::<PedMaterials>()
        .add_systems(Update, (present_ped_looks, present_ped_tints, present_ped_pose).chain().after(crate::app::FrameSet::Animation).before(super::npc_skaters::present_fade));
}

/// `PedMind::plugin_graph` of the conversation graph (prop graphs use their class name).
const CONVERSATION_GRAPH: &str = "conversation";

/// The plugin controller for graph `key` with `states` states: kept while the graph is the same, made fresh when there
/// is none or the ped switched graphs (a controller only fits the graph it was sized for). Returns the old graph's key
/// on a switch.
pub(crate) fn plugin_controller_for(
    slot: &mut Option<skate_core::graph::controller::Controller>,
    graph: &mut Option<String>,
    key: &str,
    states: usize,
) -> Option<String> {
    let switched = graph.as_deref().is_some_and(|g| g != key);
    if switched || slot.is_none() {
        *slot = Some(skate_core::graph::controller::Controller::new(states));
    }
    let old = graph.replace(key.to_owned());
    if switched { old } else { None }
}

/// No brain move this tick: whether the route a chase or an approach set must go now (both markers are cleared).
pub(crate) fn ended_move_drops_route(chase_goal: &mut Option<[f32; 3]>, approach_route: &mut bool) -> bool {
    chase_goal.take().is_some() | std::mem::take(approach_route)
}

/// A ped's behaviour brain and its controller on the stock ped AI graph (behaviour runtime,
/// doc 26). Host-owned plain data.
#[derive(Component, Default)]
pub(crate) struct PedMind {
    pub brain: skate_core::living_world::peds::brain::PedBrain,
    /// The idle hand prop action's channel cue and its change count (`ped_hand_props::sync_hand_props` sets them,
    /// `advance_peds` plays a new cue once).
    pub action_cue: Option<skate_core::living_world::peds::hand_prop::ActionCue>,
    pub action_seq: u32,
    pub mood: skate_core::living_world::peds::mood::MoodStore,
    /// The current flee leg's goal (`skate_core::living_world::peds::flee`).
    pub flee_goal: Option<[f32; 3]>,
    /// The current intercept goal (`skate_core::living_world::peds::chase::intercept`).
    pub chase_goal: Option<[f32; 3]>,
    /// The nav route is ApproachWantTarget's one-point route: it goes when the approach ends, as retail's mover drops
    /// a move's path with the move (research b177; before, the point stayed and every give-up re-planned it, a ped stood
    /// 20 s at a wall, laptop session 2026-10-17 #111).
    pub approach_route: bool,
    /// Where the body was at the last think (the ped's speed for the takedown window).
    last_at: Option<[f32; 3]>,
    presence_timer: f32,
    /// Time to the next zombie broadcast (`mood::BROADCAST_PERIOD`).
    zombie_timer: f32,
    rng: Option<skate_core::living_world::Rng>,
    controller: Option<skate_core::graph::controller::Controller>,
    /// The plugin graph's controller while the main graph runs the Plugin state.
    plugin_controller: Option<skate_core::graph::controller::Controller>,
    plugin_state: Option<usize>,
    /// Which plugin graph `plugin_controller` was made for (the prop class, or the conversation graph).
    plugin_graph: Option<String>,
    /// The seat's prop moved under the seated owner: the body jumps off forward (`82E3DBE0`). Held until the jump
    /// clip starts or the ped is no longer sitting (a one-tick pulse lost the race against the plugin's stand-up).
    sit_avoid: bool,
    /// Where the jump off started (`PED_SIT_AVOID_DONE` reports how far the ped got).
    sit_avoid_from: Option<[f32; 3]>,
    /// The speech value last sent to the audio side (`PedestrianSpeech` requests a line only when
    /// the value changes; a fresh ped holds the constructor's 68).
    spoken: Option<i32>,
    /// Population ticks run so far.
    ticks: u64,
    state: Option<usize>,
    /// The world prop the ped took (index in `PedPluginProps`, prop id, waypoint) and the props it refused (`82E40BD0`).
    prop: Option<(usize, u64, usize)>,
    refusals: skate_core::living_world::peds::plugins::RefusalMemory,
    /// A release the brain posted (DropHandProp) for `sync_hand_props`: the velocity, m/s.
    pub hand_prop_release: Option<[f32; 3]>,
    /// Log repeat limiter for PED_BRAIN / PED_MOOD lines (a refused startconversation re-raises its want every few
    /// ticks, retail by b28 / b29, and filled the log with 27k lines in 4 minutes).
    log_repeats: LogRepeats,
    /// Where the ped stopped and since when, for the still-ped report (`log_still_ped`).
    still: StillWatch,
}

/// A ped that has not moved: where, since which tick, the next tick a report is due, and the
/// last full report (a repeat that matches it logs a short line instead).
#[derive(Default)]
struct StillWatch {
    anchor: Option<([f32; 3], u64)>,
    due: u64,
    last: String,
}

/// Per-ped log repeat limiter: the same line key logs at most once per `LOG_REPEAT_TICKS`; the next line of that
/// key carries how many were held back.
#[derive(Default)]
struct LogRepeats(BTreeMap<u64, (u64, u32)>);

/// 5 s of the 60 Hz world tick.
const LOG_REPEAT_TICKS: u64 = 300;

impl LogRepeats {
    /// `Some(held back count)` when the line should be logged now, `None` when it is held back.
    fn allow(&mut self, key: u64, tick: u64) -> Option<u32> {
        match self.0.get_mut(&key) {
            Some((last, held)) if tick < *last + LOG_REPEAT_TICKS => {
                *held += 1;
                None
            }
            Some((last, held)) => {
                *last = tick;
                Some(std::mem::take(held))
            }
            None => {
                self.0.insert(key, (tick, 0));
                Some(0)
            }
        }
    }
}

/// The world props offering ped plugins on the current map (host-owned; `skate_core::living_world::peds::plugins`).
#[derive(Resource, Default)]
pub(crate) struct PedPluginProps {
    pub key: Option<(String, u64)>,
    pub props: Vec<skate_core::living_world::peds::plugins::PluginProp>,
    pub settings: skate_core::living_world::peds::plugins::PluginSettings,
    /// World ticks since the last offer scan (`826C0058` every 11th tick) and the last world tick seen.
    pub scan_ticks: u32,
    pub last_tick: Option<u64>,
    pub rng: Option<skate_core::living_world::Rng>,
    /// Moved props by id and whether a seat is in water (the water rule, `PluginSettings::water_disables_seats`):
    /// re-checked only when a seat moved, so a resting prop costs no water query.
    pub in_water: BTreeMap<u64, bool>,
    /// Moved seat props whose seats moved on the last think: when one stops, `PED_PLUGIN_WATER_REST` logs its water
    /// state where it came to rest (laptop 2026-10-19: a bench pushed into a canal never logged wet=true).
    pub settling: std::collections::BTreeSet<u64>,
}

/// The live conversations by id (host-owned plain data, `skate_core::living_world::peds::conversation`).
#[derive(Resource, Default, Debug)]
pub(crate) struct PedConversations {
    pub map: BTreeMap<u64, skate_core::living_world::peds::conversation::Conversation>,
    pub next_id: u64,
    rng: Option<skate_core::living_world::Rng>,
    /// Refusals a founder's offer test stored on the partner (ped, area key), stored by that ped's next think.
    pending_refusals: Vec<(u64, u64)>,
}

/// The chase groups by chasee id (host-owned; retail keeps one on every chasee: the player's
/// actor and each ped).
#[derive(Resource, Default, Debug)]
pub(crate) struct PedChaseGroups(pub BTreeMap<u64, skate_core::living_world::peds::chase::ChaseGroup>);

/// A ped took player `player` down from `chaser` (retail `T.vfn20(chaser position)`, `82592390`):
/// the skater's takedown latch. Host-owned; a mod can write it too.
#[derive(Message, Clone, Copy, Debug)]
pub(crate) struct SkaterTakedownRequest {
    pub player: usize,
    pub chaser: [f32; 3],
}

/// Set the skater's takedown latch (direction from the chaser to the deck).
pub(crate) fn apply_skater_takedowns(
    mut requests: MessageReader<SkaterTakedownRequest>,
    skater: Option<ResMut<crate::physics::SkaterRuntime>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
) {
    let (Some(mut skater), Some(physics)) = (skater, physics) else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        // Local play: player 0 is the simulated skater.
        if r.player != 0 {
            continue;
        }
        let deck = physics.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.position;
        skater.takedown = Some(crate::physics::Takedown::from_positions([deck.x, deck.y, deck.z], r.chaser));
        info!("SKATER_TAKEDOWN from=[{:.1}, {:.1}, {:.1}]", r.chaser[0], r.chaser[1], r.chaser[2]);
    }
}

/// Obstacle / want target ids of the players (above every living-world id).
pub(crate) const PLAYER_TARGET_BASE: u64 = u64::MAX - 64;

/// Run every ped's AI graph once per population tick (before the bodies step), in id order.
/// Wants come from the mood system (not ported yet) or a mod; without them a ped wanders as
/// before. Logs `PED_BRAIN` on every state change.
pub(crate) fn think_peds(
    state: Res<PopulationState>,
    data: Res<PedData>,
    settings: Res<super::LivingWorldSettings>,
    observers: Res<super::LivingWorldObservers>,
    mut peds: Query<(Entity, &Pedestrian, &mut PedBody, &mut PedMind)>,
    mut speech: MessageWriter<crate::world_audio::PedSpeechEvent>,
    mut chase_groups: ResMut<PedChaseGroups>,
    mut conversations: ResMut<PedConversations>,
    mut skater_takedowns: MessageWriter<SkaterTakedownRequest>,
    mut tazers: MessageWriter<crate::world_audio::PedTazerEvent>,
    // `greeted` events for other peds' mood stores (posted when the target thinks next).
    mut greeted: Local<Vec<(u64, u64, [f32; 3])>>,
    // Conversation listeners that get the speech value 41 when they think next (`82E3DAD8`).
    mut listening: Local<Vec<u64>>,
    // One store for reading the hits and writing mood events (a reader and a writer of the
    // same message in one system conflict).
    mut events: ResMut<bevy::ecs::message::Messages<PedEvent>>,
    mut cursor: Local<Option<bevy::ecs::message::MessageCursor<PedEvent>>>,
    mut touchers: Local<BTreeMap<u64, u64>>,
    (mut traffic_events, cars, mut plugin_props, physics): (MessageReader<super::vehicles::TrafficEvent>, Query<(&super::vehicles::TrafficCar, &super::vehicles::CarMotion)>, ResMut<PedPluginProps>, Option<Res<crate::physics::GamePhysics>>),
) {
    let _span = super::timing::span(super::timing::THINK_PEDS);
    // A map change (b216; not the first map, whose key goes from none to some): the conversations, chase groups,
    // pending refusals and greetings belong to the old world's peds, and the RNG streams restart from the new
    // world's seed as on a fresh start there. The prop list itself reloads below.
    if plugin_props.key.is_some() && plugin_props.key != data.loaded_for {
        conversations.map.clear();
        conversations.pending_refusals.clear();
        conversations.rng = None;
        chase_groups.0.clear();
        plugin_props.rng = None;
        plugin_props.last_tick = None;
        greeted.clear();
        listening.clear();
        touchers.clear();
        info!("PED_WORLD_RESET map changed: conversations, chase groups and ped RNG cleared");
    }
    // Streamed-out props (DMO streaming): their plugins are not offered and a ped using one lets go. A world prop's
    // plugin id carries its DMO body id in the low 32 bits (`ped_plugins::map_props`).
    let dormant = physics.as_ref().and_then(|p| p.prop_dynamics()).map(|d| d.dormant_ids()).unwrap_or_default();
    let streamed_out = |plugin_id: u64| dormant.contains(&(plugin_id as u32));
    // Peds whose seat's prop moved this tick (grabbed, pushed, knocked): `82E3DBE0` fires the seated owner's avoid.
    let mut seat_pushed: Vec<u64> = Vec::new();
    // Horn kind 2 at a ped (`sub_82E3C3D0`: the honker id into brain `+3232`; the last car wins).
    let honked: BTreeMap<u64, u64> = traffic_events.read().filter_map(|e| match e { super::vehicles::TrafficEvent::HonkedAt { id, ped } => Some((*ped, id.to_u64())), _ => None }).collect();
    let Some(graph) = data.graph.as_deref() else { return };
    // The honking cars' pose for RunFromHonker (`826A1358` looks the car up every frame): position and forward.
    let car_pose: BTreeMap<u64, ([f32; 3], [f32; 3])> = cars.iter().map(|(c, m)| (c.id.to_u64(), (m.curr.translation.to_array(), (m.curr.rotation * Vec3::Z).to_array()))).collect();
    // Collisions from the skater contact of the last steps (`PedEvent::Hit`): retail posts
    // `collision` to every receiver (broadcast radius 0 = unlimited); a ped that was not the one
    // hit fails its "ped itself" prerequisite and records `nearbycollision` instead
    // (`sub_82E41060`). The instigator is the ped's last toucher (the contact record `+3184`), kept until its
    // Colliding state begins.
    let cursor = cursor.get_or_insert_with(|| events.get_cursor());
    // A reacting contact sets the hit ped's collision component (`sub_82E38FB8`: needs-to-begin); the mood broadcast
    // waits for its Colliding state to begin (`PedEvent::Colliding`), so a ped whose state does not allow Colliding
    // (a non-interruptible chase, a plugin with the collision override) posts nothing yet.
    let (mut hits, mut collisions) = (Vec::<u64>::new(), Vec::<(u64, Option<u64>)>::new());
    for e in cursor.read(&events) {
        match e {
            PedEvent::Hit { id, toucher, .. } => {
                hits.push(id.to_u64());
                touchers.insert(id.to_u64(), *toucher);
            }
            PedEvent::Colliding { id } => collisions.push((id.to_u64(), touchers.remove(&id.to_u64()))),
            _ => {}
        }
    }
    if settings.net_role == super::NetRole::Client || !settings.ped_brain.enabled {
        return;
    }
    let program = &graph.graph.runtime.program;
    let dt = tick_seconds(state.world.clock().hz);
    let tick = state.world.tick();
    let players: Vec<[f32; 3]> = observers.observers.iter().map(|o| o.position).collect();
    let mut list: Vec<_> = peds.iter_mut().collect();
    list.sort_by_key(|(_, p, ..)| p.id);
    // Entity type and position of every id this tick (players are the `skater` type).
    let entities: BTreeMap<u64, (String, [f32; 3])> = list
        .iter()
        .map(|(_, p, b, _)| (p.id.to_u64(), (p.entity.clone(), b.position.to_array())))
        .chain(players.iter().enumerate().map(|(i, p)| (PLAYER_TARGET_BASE + i as u64, ("skater".to_string(), *p))))
        .collect();
    // A hit ped that is gone no longer waits for its Colliding state.
    touchers.retain(|id, _| entities.contains_key(id));
    // Velocity of every id this tick (the attack throw's look-ahead).
    let velocities: BTreeMap<u64, [f32; 3]> = list
        .iter()
        .map(|(_, p, b, _)| (p.id.to_u64(), b.velocity.to_array()))
        .chain(observers.observers.iter().enumerate().map(|(i, o)| (PLAYER_TARGET_BASE + i as u64, o.velocity)))
        .collect();
    let velocity_of = |id: u64| velocities.get(&id).copied();
    // World-prop plugins: the map's props, then the offer scan every 11th world tick (`826C0058` -> `826BFE18`).
    {
        let pp = &mut *plugin_props;
        let key = data.loaded_for.clone();
        if pp.key != key {
            pp.props = key.as_ref().and_then(|k| data.plugins.placed.get(&k.0)).cloned().unwrap_or_default();
            pp.key = key;
            pp.scan_ticks = 0;
            pp.in_water.clear();
            info!("PED_PLUGINS props on this map: {}", pp.props.len());
        }
        pp.settings = settings.ped_brain.plugins;
        // Hotpoint seats ride on their object (a prop's plugin id carries its DMO body id in the low 32 bits): every
        // think they follow the body from its authored pose, so a moved bench takes its seats along.
        if let (Some(authored), Some(dynamics)) = (pp.key.as_ref().and_then(|k| data.plugins.placed.get(&k.0)), physics.as_ref().and_then(|p| p.prop_dynamics())) {
            let pose = |(o, b): (skate_core::math::Vector3, skate_core::math::Basis3)| ([o.x, o.y, o.z], b.columns);
            for (prop, original) in pp.props.iter_mut().zip(authored) {
                let body = prop.id as u32;
                let (Some(spawn), Some(now)) = (dynamics.spawn_pose(body), dynamics.pose(body)) else { continue };
                let (spawn, now) = (pose(spawn), pose(now));
                let mut seats_moved = false;
                for (w, a) in prop.waypoints.iter_mut().zip(&original.waypoints) {
                    let (position, facing) = super::ped_plugins::follow_prop(a.position, a.facing, spawn, now);
                    // A seat whose prop moves (grabbed, pushed, knocked): retail keeps the ped's plugin and seat
                    // (research b102). The owned object reporting a move to its seated ped (`82E3DBE0`: |v|^2 > 2.5e-5,
                    // `0x822F8BE0`) makes it jump off forward; the graph then walks it to the seat's new place and sits
                    // it again (the locked waypoint is re-read every evaluation, `826AB9C0`). The seat's own speed stands
                    // in for the object's vector: its move over one step of `SEAT_PUSH_STEP_SECONDS`.
                    let step = (0..3).map(|k| (position[k] - w.position[k]).powi(2)).sum::<f32>().sqrt();
                    seats_moved |= step > 1e-3;
                    let speed = step / dt.max(1e-4);
                    let per_step = speed * SEAT_PUSH_STEP_SECONDS;
                    if let Some(occupant) = w.occupant.filter(|_| per_step * per_step > SEAT_PUSH_STEP_SQ) {
                        info!("PED_PLUGIN_SEAT_MOVED prop={:016X} {} occupant={occupant} speed={speed:.2} from=[{:.2}, {:.2}, {:.2}] to=[{:.2}, {:.2}, {:.2}] tick={tick}", prop.id, prop.class, w.position[0], w.position[1], w.position[2], position[0], position[1], position[2]);
                        seat_pushed.push(occupant);
                    }
                    (w.position, w.facing) = (position, facing);
                }
                // The DMO's plugin check (`82C56DB0`, b136 / b137): moved more than its type's max distance, or turned
                // past its per-axis tolerance -> off (seated peds evicted); else each seat must stand on pavement navmesh.
                // NOT RETAIL YET: when retail runs it (DMO vtable +264's caller) is open, likely on a move / settle
                // message; ours every think, only for a prop away from its authored pose.
                use skate_core::living_world::peds::plugins::{check_prop, seats_in_water, PropCheck, ROTATION_EPSILON};
                let moved = (0..3).any(|k| (now.0[k] - spawn.0[k]).abs() > 1e-3)
                    || (0..3).any(|c| (0..3).any(|r| (now.1[c][r] - spawn.1[c][r]).abs() > ROTATION_EPSILON));
                // The per-seat pavement probe (`82C539A8`) is not applied yet: our probe fails 10-23% of untouched
                // authored seats (test `living_world_authored_plugin_seats_pass_the_pavement_probe`), so it is not
                // retail's yet; a moved, upright prop keeps its seats as before.
                let tipped = moved && dynamics.plugin_check_limits(body)
                    .is_some_and(|(tolerance, max_distance)| check_prop(spawn, now, tolerance, max_distance) == PropCheck::DisableAll);
                // NOT RETAIL (ours, user 2026-10-18: a seat stops being offered once its bench is in the water): a moved
                // prop with a seat in water is off too; asked again only when a seat moved (or first seen moved).
                let wet = if !moved {
                    pp.in_water.remove(&prop.id);
                    false
                } else if let (false, Some(&cached)) = (seats_moved, pp.in_water.get(&prop.id)) {
                    cached
                } else {
                    let world = physics.as_ref().map(|p| p.world());
                    let at = |p: [f32; 3]| skate_core::math::Vector3::new(p[0], p[1], p[2]);
                    let wet = world.is_some_and(|w| seats_in_water(prop, &pp.settings, |p, above, depth| w.water_surface_at(at(p), above, depth)));
                    // PED_PLUGIN_WATER (laptop 2026-10-19: benches pushed into the water left no line): each change of
                    // a moved prop's water state, with its lowest seat and any water surface up to 2 m above it, so a
                    // bench standing in shallow water with dry seats shows too.
                    if pp.in_water.insert(prop.id, wet) != Some(wet) && pp.settings.water_disables_seats {
                        let low = prop.waypoints.iter().map(|w| w.position).min_by(|a, b| a[1].total_cmp(&b[1]));
                        let surface = low.zip(world).and_then(|(p, w)| w.water_surface_at(at(p), 2.0, pp.settings.water_max_depth));
                        let occupants = prop.waypoints.iter().filter(|w| w.occupant.is_some()).count();
                        info!("PED_PLUGIN_WATER {} {:016X} wet={wet} lowest_seat={:?} surface={surface:?} occupants={occupants} tick={tick}",
                            prop.class, prop.id, low.map(|p| [(p[0] * 100.0).round() / 100.0, (p[1] * 100.0).round() / 100.0, (p[2] * 100.0).round() / 100.0]));
                    }
                    wet
                };
                // PED_PLUGIN_WATER_REST: once per stop, the seats' water state at rest and the nearest water surface
                // within 5 m above or below the lowest seat (so a canal whose surface the rule never meets shows).
                if moved && seats_moved {
                    pp.settling.insert(prop.id);
                } else if pp.settling.remove(&prop.id) && pp.settings.water_disables_seats {
                    let world = physics.as_ref().map(|p| p.world());
                    let low = prop.waypoints.iter().map(|w| w.position).min_by(|a, b| a[1].total_cmp(&b[1]));
                    let near = low.zip(world).and_then(|(p, w)| w.water_surface_at(skate_core::math::Vector3::new(p[0], p[1], p[2]), 5.0, 5.0));
                    info!("PED_PLUGIN_WATER_REST {} {:016X} wet={wet} lowest_seat={:?} surface_within_5m={near:?} rule_above={} rule_depth={} tick={tick}",
                        prop.class, prop.id, low.map(|p| [(p[0] * 100.0).round() / 100.0, (p[1] * 100.0).round() / 100.0, (p[2] * 100.0).round() / 100.0]),
                        pp.settings.water_above, pp.settings.water_max_depth);
                }
                let off = tipped || wet;
                if off != prop.disabled {
                    prop.disabled = off;
                    let why = if off && wet { "disabled (a seat in water), occupants evicted" } else if off { "disabled (moved / tipped), occupants evicted" } else { "enabled again" };
                    info!("PED_PLUGIN_PROP {} {:016X} {why} tick={tick}", prop.class, prop.id);
                }
            }
        }
        let seed = state.world.seed();
        let rng = pp.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(seed, &[0x504c_5547])));
        let elapsed = pp.last_tick.map_or(1, |t| tick.saturating_sub(t)) as u32;
        pp.last_tick = Some(tick);
        pp.scan_ticks += elapsed;
        // Waypoints held by peds that are gone or no longer on the prop are released.
        let holding: Vec<(u64, u64)> = list.iter().filter_map(|(_, p, _, m)| m.prop.map(|(_, id, _)| (p.id.to_u64(), id))).collect();
        for prop in &mut pp.props {
            for w in &mut prop.waypoints {
                if let Some(o) = w.occupant.filter(|o| !holding.contains(&(*o, prop.id))) {
                    info!("PED_PLUGIN_PROP {} {:016X} released: ped {o} gone tick={tick}", prop.class, prop.id);
                    w.occupant = None;
                }
            }
        }
        while pp.scan_ticks >= pp.settings.scan_period.max(1) {
            pp.scan_ticks -= pp.settings.scan_period.max(1);
            let mut attaches = Vec::new();
            {
                // The descriptors' transfer expressions (`UseWorldProp`: e.g. usetrashbin needs a disposable hand prop).
                let views: std::collections::HashMap<u64, skate_core::living_world::peds::plugins::TransferView> =
                    list.iter().map(|(_, p, _, m)| (p.id.to_u64(), skate_core::living_world::peds::plugins::TransferView::of(&m.brain))).collect();
                let offer_settings = pp.settings;
                let mut qualifies = |prop: &skate_core::living_world::peds::plugins::PluginProp, ped: &skate_core::living_world::peds::plugins::OfferPed| {
                    let view = views.get(&ped.id).copied().unwrap_or_default();
                    let transfer = data.plugins.classes.get(&prop.class).and_then(|c| c.descriptor.as_ref()).and_then(|d| d.transfer.as_ref());
                    transfer.is_none_or(|t| t.evaluate(&mut |name, _| view.condition(name, prop, ped.position, &offer_settings)))
                };
                let mut offer_peds: Vec<skate_core::living_world::peds::plugins::OfferPed> = list
                    .iter_mut()
                    .map(|(_, p, b, m)| {
                        let m = &mut **m;
                        skate_core::living_world::peds::plugins::OfferPed {
                            id: p.id.to_u64(),
                            entity: &p.entity,
                            position: b.position.to_array(),
                            has_plugin: m.brain.has_plugin || m.prop.is_some(),
                            odds: data.plugins.odds.get(&p.entity).map(Vec::as_slice).unwrap_or(&[]),
                            memory: &mut m.refusals,
                        }
                    })
                    .collect();
                for (index, prop) in pp.props.iter_mut().enumerate() {
                    if !data.plugin_graphs.contains_key(&prop.class) || streamed_out(prop.id) || prop.disabled {
                        continue;
                    }
                    let class = data.plugins.classes.get(&prop.class);
                    // [inference] the offer radius: the class field E2101F2B17A0E6B5 (10 sit / 5 bin / 15 ATM).
                    let radius = class.and_then(|c| c.numbers.get("Hash_E2101F2B17A0E6B5").copied()).unwrap_or(10.0);
                    let max = class.and_then(|c| c.descriptor.as_ref()).map_or(1, |d| d.max_participants);
                    for a in skate_core::living_world::peds::plugins::offer(prop, radius, max, &mut offer_peds, &mut qualifies, &pp.settings, rng) {
                        attaches.push((index, a));
                    }
                }
                // Spawned conversation areas are offers too (b212; after the placed props [inference]): passers-by join
                // while they gather, through the descriptor's transfer expression.
                use skate_core::living_world::peds::plugins::{conversation_qualifies, offer_conversation, CONVERSATION_CLASS};
                let class = data.plugins.classes.get(CONVERSATION_CLASS);
                let radius = class.and_then(|c| c.numbers.get("Hash_E2101F2B17A0E6B5").copied()).unwrap_or(20.0);
                let transfer = class.and_then(|c| c.descriptor.as_ref()).and_then(|d| d.transfer.as_ref());
                let rows = &data.conversations.rows;
                let mut qualifies = |c: &skate_core::living_world::peds::conversation::Conversation, ped: &skate_core::living_world::peds::plugins::OfferPed| {
                    conversation_qualifies(transfer, c, ped.entity, views.get(&ped.id).copied().unwrap_or_default(), rows)
                };
                for c in conversations.map.values_mut() {
                    for ped in offer_peds.iter_mut() {
                        if offer_conversation(c, radius, ped, &mut qualifies, rows, &offer_settings, rng) {
                            info!("PED_CONVERSATION id={} join ped={} members={} state={} tick={tick}", c.id, ped.id, c.members.len(), c.state);
                        }
                    }
                }
            }
            for (index, a) in attaches {
                if let Some((_, p, _, m)) = list.iter_mut().find(|(_, p, ..)| p.id.to_u64() == a.ped) {
                    m.prop = Some((index, a.prop, a.waypoint));
                    info!("PED_PLUGIN_PROP ped=#{} takes {} {:016X} waypoint {} tick={tick}", p.id.serial, pp.props[index].class, a.prop, a.waypoint);
                }
            }
        }
    }
    let entity = |id: u64| entities.get(&id).cloned();
    let target = |id: u64| entities.get(&id).map(|e| e.1);
    let player_list: Vec<(u64, [f32; 3])> = players.iter().enumerate().map(|(i, p)| (PLAYER_TARGET_BASE + i as u64, *p)).collect();
    let world_seed = state.world.seed();
    // Live peds for the presence scan (`sub_82E3CA20`: every spawned, not torn-down ped counts).
    let ped_list: Vec<(u64, [f32; 3])> = list.iter().map(|(_, p, b, _)| (p.id.to_u64(), b.position.to_array())).collect();
    let greeted_now = std::mem::take(&mut *greeted);
    // Who is busy this tick (check 8 of the mood results; players: not busy, unverified), and what a conversation
    // founder's offer test reads of the partner (in a plugin, its transfer view).
    let busy_now: BTreeMap<u64, (bool, bool, skate_core::living_world::peds::plugins::TransferView)> = list
        .iter()
        .map(|(_, p, _, m)| (p.id.to_u64(), (m.brain.busy(), m.brain.has_plugin || m.prop.is_some(), skate_core::living_world::peds::plugins::TransferView::of(&m.brain))))
        .collect();
    let busy = |id: u64| busy_now.get(&id).is_some_and(|b| b.0);
    // Chasers that left the world leave their groups (the ped's destruction; retail removes it
    // with the chaser component).
    for g in chase_groups.0.values_mut() {
        g.chasers.retain(|c| entities.contains_key(c));
    }
    chase_groups.0.retain(|chasee, g| !g.chasers.is_empty() && entities.contains_key(chasee));
    // Conversations: members that left the world leave (an unfinished conversation aborts); an
    // empty one is gone (every area is spawned: `82E1BD08` removes it).
    for c in conversations.map.values_mut() {
        let gone: Vec<u64> = c.members.iter().map(|m| m.ped).filter(|p| !entities.contains_key(p)).collect();
        for p in gone {
            c.leave(p);
            info!("PED_CONVERSATION id={} leave ped={p} result=despawned members={} tick={tick}", c.id, c.members.len());
        }
    }
    conversations.map.retain(|_, c| !c.members.is_empty());
    listening.retain(|p| entities.contains_key(p));
    let conversations = &mut *conversations;
    let conv_rng_seed = world_seed;
    let conv_rng = conversations.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(conv_rng_seed, &[0x434f_4e56])));
    let mut conv_rand = || conv_rng.unit();
    let conv_params = skate_core::living_world::peds::conversation::ConversationParams {
        turn_seconds: settings.ped_brain.values.conversation_turn_seconds,
        gather_seconds: settings.ped_brain.values.conversation_gather_seconds,
        // The descriptor's maxNumberOfParticipants (3).
        max_members: data
            .plugins
            .classes
            .get(skate_core::living_world::peds::plugins::CONVERSATION_CLASS)
            .and_then(|c| c.descriptor.as_ref())
            .map_or(skate_core::living_world::peds::conversation::MAX_MEMBERS, |d| d.max_participants as usize),
        ..Default::default()
    };
    // The gather timer (`82E1EB10`): a conversation starts with whoever is there when it runs out.
    for c in conversations.map.values_mut() {
        if c.tick(dt, &data.conversations.rows, &mut conv_rand) {
            info!("PED_CONVERSATION id={} gather_timeout state={} members={} tick={tick}", c.id, c.state, c.members.len());
        }
    }
    // How many chasers a chasee takes: every chase group (skater and ped alike) reads `global`, or `zombie` under the
    // zombie cheat (group init `82D967B0`, b105).
    let group_record = if settings.zombie { data.chase_zombie.as_deref() } else { data.chase_global.as_deref() };
    let max_chasers = |_chasee: u64| skate_core::living_world::peds::chase::ChaseRecord::max_chasers(group_record);
    // Still-ped reports queue: at most this many full reports per tick, the rest wait their turn.
    let mut still_budget = STILL_REPORTS_PER_TICK;
    for (entity_id, ped, mut body, mut mind) in list {
        let mind = &mut *mind;
        let body = &mut *body;
        let me = ped.id.to_u64();
        let at = body.position.to_array();
        // Perception (`82E418E8`): memory, age and suppression run, the vision test from the eye
        // (ours: 1.6 m above the feet; retail's eye point is set elsewhere) along the heading; the
        // line of sight is our navmesh line (retail: a physics ray).
        {
            let steps = tick.saturating_sub(ped.spawn_tick).saturating_sub(mind.ticks) as f32;
            if steps > 0.0 && !mind.brain.perceptions.entries.is_empty() {
                use skate_core::living_world::peds::perception;
                let sight = data.sight.get(&ped.entity).copied();
                let eye = [at[0], at[1] + 1.6, at[2]];
                let forward = [body.heading.sin(), 0.0, body.heading.cos()];
                let here = data.nav.as_deref().and_then(|m| m.locate(at).map(|h| (m, h)));
                let los = |_: [f32; 3], t: [f32; 3]| here.is_none_or(|(m, h)| m.clear_line(h, t));
                let look = |t: u64| {
                    let p = target(t)?;
                    let v = observers.observers.get(t.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
                    Some((p, v, sight.is_some_and(|s| perception::sees(at, eye, forward, p, &s, &los))))
                };
                mind.brain.perceptions.tick(dt * steps, &look);
            }
        }
        // Mood: tick the records, post presence and collisions, produce wants.
        if let (Some(tables), true) = (data.mood.as_deref(), settings.ped_brain.mood) {
            use skate_core::living_world::peds::mood::{self, MoodContext, MoodEvent};
            let set = data.reaction_sets.get(&ped.entity).cloned().unwrap_or_else(|| "default".into());
            let magnitude = |c: &str| tables.categories.get(c).map_or(0.0, |c| c.magnitude);
            let lifetime = |c: &str| tables.categories.get(c).map_or(30.0, |c| c.lifetime);
            // `82BFF308`: the category's eviction Priority (absent = the attribute default, 0).
            let priority = |c: &str| tables.categories.get(c).map_or(0, |c| c.priority);
            // The post's vision test (`sub_82E41060` -> `82E27240`, b5 / b18): the event position against the
            // category's near / far ranges times the type's scales, the same eye, cone and line as the perception
            // pass. A type without a perception record keeps every post (retail's record default is unread).
            let sight_scales = data.sight_scales.get(&ped.entity).copied();
            let post_eye = [at[0], at[1] + 1.6, at[2]];
            let post_forward = [body.heading.sin(), 0.0, body.heading.cos()];
            let post_here = data.nav.as_deref().and_then(|m| m.locate(at).map(|h| (m, h)));
            let post_los = |_: [f32; 3], t: [f32; 3]| post_here.is_none_or(|(m, h)| m.clear_line(h, t));
            let seen = |e: &MoodEvent| match (sight_scales, tables.categories.get(&e.category)) {
                (Some(s), Some(c)) => skate_core::living_world::peds::perception::sees(
                    at,
                    post_eye,
                    post_forward,
                    e.position,
                    &skate_core::living_world::peds::perception::Sight::new(c.near, c.far, s[0], s[1], s[2]),
                    &post_los,
                ),
                _ => true,
            };
            let steps = tick.saturating_sub(ped.spawn_tick).saturating_sub(mind.ticks) as f32;
            if steps > 0.0 {
                mind.mood.tick(dt * steps, &lifetime);
                mind.presence_timer -= dt * steps;
                if mind.presence_timer <= 0.0 {
                    mind.presence_timer = magnitude(mood::category::PRESENCE).max(dt);
                    // `sub_82E41060` drops a post whose prerequisites fail (presence: within 12 m in
                    // the stock sets).
                    let post_ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: settings.zombie, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
                    for e in mood::presence(me, at, &ped_list, &player_list) {
                        // A suppressed entity raises no mood (perception `+64`).
                        if !e.instigator.is_some_and(|i| mind.brain.perceptions.suppressed(i)) && tables.accepts(&set, &e, &post_ctx) && seen(&e) {
                            mind.mood.post_ranked(e, magnitude(mood::category::PRESENCE), &priority);
                        }
                    }
                }
                // The zombie broadcast (`sub_82E24B60`): every console frame while the cheat is on.
                if settings.zombie {
                    mind.zombie_timer -= dt * steps;
                    let mut posts = 0;
                    while mind.zombie_timer <= 0.0 && posts < 8 {
                        mind.zombie_timer += mood::BROADCAST_PERIOD;
                        posts += 1;
                    }
                    if posts > 0 {
                        let post_ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: true, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
                        for e in mood::zombie_mode(&player_list) {
                            let suppressed = e.instigator.is_some_and(|i| mind.brain.perceptions.suppressed(i));
                            let in_sight = seen(&e);
                                let accepted = !suppressed && in_sight && tables.accepts(&set, &e, &post_ctx);
                            if let Some(held) = mind.log_repeats.allow(0x5a4f_4d42 ^ u64::from(accepted), tick) {
                                info!(
                                    "PED_ZOMBIE_POST ped=#{} {set} accepted={accepted} seen={in_sight} suppressed={suppressed} posts={posts} busy={} tick={tick}{}",
                                    ped.id.serial,
                                    mind.brain.busy(),
                                    if held > 0 { format!(" (+{held} held back)") } else { String::new() }
                                );
                            }
                            if accepted {
                                for _ in 0..posts {
                                    mind.mood.post_ranked(e.clone(), magnitude(mood::category::ZOMBIE_MODE), &priority);
                                }
                            }
                        }
                    }
                } else {
                    mind.zombie_timer = 0.0;
                }
            }
            if let Some(&car) = honked.get(&me) {
                mind.brain.honker = Some(car);
            }
            if let Some(i) = listening.iter().position(|&p| p == me) {
                listening.swap_remove(i);
                mind.brain.speech = Some(skate_core::living_world::peds::conversation::LISTENER_SPEECH);
            }
            // Every post runs the ped's prerequisites (`sub_82E41060`).
            let post_ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: settings.zombie, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
            // ChannelGreetWantTarget's `greeted` (magnitude 1.0, `8269FAC0`).
            for &(_, instigator, position) in greeted_now.iter().filter(|g| g.0 == me) {
                let e = MoodEvent { category: mood::category::GREETED.into(), instigator: Some(instigator), second: Some(me), position };
                if !mind.brain.perceptions.suppressed(instigator) && tables.accepts(&set, &e, &post_ctx) && seen(&e) {
                    mind.mood.post_ranked(e, 1.0, &priority);
                }
            }
            if hits.contains(&me) {
                mind.brain.begin_colliding = true;
                mind.brain.colliding = true;
            }
            // IsPedestrianColliding holds while the contact reaction plays.
            if mind.brain.colliding && !body.player.reacting() {
                mind.brain.colliding = false;
            }
            for &(hit, toucher) in &collisions {
                let instigator = toucher.or_else(|| player_list.first().map(|p| p.0));
                if instigator.is_some_and(|i| mind.brain.perceptions.suppressed(i) || mind.brain.ignore_collisions_with == Some(i)) {
                    continue;
                }
                // Event origin (`sub_8269D8A8`: the contact's entity position): the hit ped's, so bystanders run the
                // vision test against it [inferred: which entity +188 is was not read].
                let origin = entity(hit).map_or(at, |(_, p)| p);
                // `sub_82E41060`: the contact is posted as `collision`; when the prerequisites refuse it and the hit ped
                // is another ped, it is posted again as `nearbycollision` (stock sets refuse a bystander's `collision`
                // with kind 6; the security sets have no such item, so their guards react to `collision`).
                let mut e = MoodEvent { category: mood::category::COLLISION.into(), instigator, second: Some(hit), position: origin };
                if !tables.accepts(&set, &e, &post_ctx) {
                    if hit == me {
                        continue;
                    }
                    e.category = mood::category::NEARBY_COLLISION.into();
                    if !tables.accepts(&set, &e, &post_ctx) {
                        continue;
                    }
                }
                if seen(&e) {
                    let m = magnitude(&e.category);
                    mind.mood.post_ranked(e, m, &priority);
                }
            }
            // The brain's own rolls (sit time, stand-up) and its type's sit values, once per ped.
            if mind.brain.rng.is_none() {
                mind.brain.rng = Some(skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(world_seed, &[0x4252_4e52, me])));
                mind.brain.sit = entity(me).and_then(|(name, _)| data.sit.get(name.as_str()).copied()).unwrap_or_default();
                // The starting hand prop (ped constructor `82E33198`): the type's chance, then its weighted list.
                let start = entity(me).and_then(|(name, _)| data.starting_props.get(name.as_str()));
                if let (Some((chance, list)), true) = (start, settings.ped_brain.values.hand_prop.starting_props) {
                    let r = mind.brain.rng.as_mut().expect("seeded above");
                    let (a, b) = (r.modulo(100) + 1, r.modulo(100) + 1);
                    if let Some(key) = skate_core::living_world::peds::brain::HandProp::starting_pick(*chance, list, a, b).map(str::to_string) {
                        mind.brain.hand_prop.request(&key);
                        if let Some(r) = data.hand_props.props.get(&key) {
                            let h = &mut mind.brain.hand_prop;
                            (h.disposable, h.can_sit, h.can_attack_throw) = (r.disposable, r.can_sit, r.can_attack_throw);
                        }
                        info!("PED_HAND_PROP ped=#{} requested {key} at spawn tick={tick}", ped.id.serial);
                    }
                }
            }
            let rng = mind.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(world_seed, &[0x4d4f_4f44, me])));
            let ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: settings.zombie, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
            let brain = &mind.brain;
            // `7FF7`: the reactions about the instigator the ped has finished (its perception entry `+68`, `82E42A58`).
            let outstanding = |id: u64| brain.perceptions.get(id).map_or(0, |e| e.reacted);
            let pending = |w: &str| brain.wants.get(w).is_some_and(|x| x.needs_addressing);
            // `sub_82E41B98` does nothing for a busy ped (`sub_82E3BF70`) or while DisableAllMoods holds (`brain+0xCCE` bit
            // 0x08, set / restored by `826A5A20` / `826A5A50`, research b176); a pass sets WaitingToReact.
            // `072D`: the hand-prop bit (`brain+3278` 0x02, set while a hand prop is attached, `82E3EC60` / `82E3EBE0`).
            let produced = if brain.busy() || brain.moods_disabled { None } else { tables.produce(&mut mind.mood, &ctx, brain.hand_prop.holding, &outstanding, &pending, rng) };
            if let Some(reaction) = produced {
                let key = reaction.result.bytes().chain([0]).chain(reaction.category.bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) & !(1 << 63);
                if let Some(held) = mind.log_repeats.allow(key, tick) {
                    info!(
                        "PED_MOOD ped=#{} {} result={} category={} roll={:?} passed={} wants={:?} tick={tick}{}",
                        ped.id.serial,
                        set,
                        reaction.result,
                        reaction.category,
                        reaction.rolled,
                        reaction.passed,
                        reaction.wants.iter().map(|w| w.want.as_str()).collect::<Vec<_>>(),
                        if held > 0 { format!(" (+{held} held back)") } else { String::new() }
                    );
                }
                if reaction.passed {
                    mind.brain.waiting_to_react = true;
                }
                for w in &reaction.wants {
                    mind.brain.raise_mood_want(&w.want, w.target, w.flag_40, settings.ped_brain.values.mood_want_memory_seconds);
                }
                events.write(PedEvent::Mood { id: ped.id, result: reaction.result.clone(), passed: reaction.passed, wants: reaction.wants.iter().map(|w| w.want.clone()).collect() });
            }
        }
        let due = tick.saturating_sub(ped.spawn_tick);
        let controller = mind.controller.get_or_insert_with(|| skate_core::graph::controller::Controller::new(program.topology.states.len()));
        // The ped's own chase record: its type's, or `zombie` under the zombie cheat (`82E33FAC`, b105).
        let record = if settings.zombie { data.chase_zombie.as_deref() } else { data.chase.get(&ped.entity) };
        let ped_speed = mind.last_at.map_or(0.0, |l| ((at[0] - l[0]).powi(2) + (at[2] - l[2]).powi(2)).sqrt() / (dt * tick.saturating_sub(ped.spawn_tick).saturating_sub(mind.ticks).max(1) as f32));
        mind.last_at = Some(at);
        // The takedown contact (`82E38FB8` records a touch of the takedown target and counts it,
        // `brain+3248`): our skater cylinder against the ped while the takedown plays.
        if mind.brain.takedown_active && !mind.brain.takedown_contact {
            let ped_radius = data.nav.as_deref().map_or(super::vehicle_contacts::FALLBACK_PED_RADIUS, |m| m.agent[1]);
            if let Some(p) = mind.brain.takedown_target.filter(|t| *t >= PLAYER_TARGET_BASE).and_then(|t| target(t)) {
                let d = ((p[0] - at[0]).powi(2) + (p[2] - at[2]).powi(2)).sqrt();
                if d < ped_radius + SKATER_CONTACT_RADIUS && (p[1] - at[1]).abs() <= 2.0 {
                    mind.brain.takedown_contact = true;
                    mind.brain.takedowns += 1;
                }
            }
        }
        // HasLineOfSightToTazeTarget (`+3280` bit 0x10, retail's batched ray pass `82E23D68`):
        // ours is the navmesh line to the tazer want's target while the tazer is out.
        if mind.brain.tazer_drawn_requested {
            let goal = mind.brain.tazer_want.as_ref().and_then(|w| mind.brain.wants.get(w)).and_then(|w| target(w.target));
            mind.brain.tazer_line_of_sight = match (goal, data.nav.as_deref()) {
                (Some(g), Some(mesh)) => mesh.locate(at).is_some_and(|h| mesh.clear_line(h, g)),
                (Some(_), None) => true,
                _ => false,
            };
        }
        let takedown_table = if settings.zombie { data.takedown_zombie.as_deref() } else { data.takedowns.get(&ped.entity) };
        let forward = [body.heading.sin(), body.heading.cos()];
        let choose = |t: u64| {
            let (table, p) = (takedown_table?, target(t)?);
            let v = observers.observers.get(t.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
            skate_core::living_world::peds::takedown::choose(table, at, forward, ped_speed, p, v)
        };
        // LockToCurrentPosition: the body stays on the locked point (root motion does not move it).
        if let Some(p) = mind.brain.locked_at {
            body.position.x = p[0];
            body.position.z = p[2];
        }
        while mind.ticks < due {
            mind.ticks += 1;
            mind.brain.tick_timers(dt);
            // The taunt clip ended (`MajorIntentComplete`): the intent goes, the graph leaves DoTaunt.
            if body.taunt == TauntClip::Done {
                body.taunt = TauntClip::None;
                mind.brain.monitored.remove("SGIntent");
            }
            // ApproachWantTarget sets its goal each tick it runs.
            mind.brain.approach = None;
            mind.brain.approach_slide = None;
            // HasPlugin (`826A99C0`): the ped is a participant of a live conversation.
            if mind.brain.plugin.is_none_or(|id| !conversations.map.get(&id).is_some_and(|c| c.members.iter().any(|m| m.ped == me))) {
                mind.brain.plugin = conversations.map.values().find(|c| c.members.iter().any(|m| m.ped == me)).map(|c| c.id);
            }
            conversations.pending_refusals.retain(|&(p, key)| {
                if p == me {
                    mind.refusals.remember(key, &plugin_props.settings);
                }
                p != me
            });
            mind.refusals.tick(dt, &plugin_props.settings);
            // The Plugin behaviour's Update (`8269F248` -> `82E40BD0`, b104) re-stores the 30 s refusal for its prop (or
            // conversation area) every tick it runs, so a ped that leaves it is not offered it again for 30 s.
            if mind.brain.in_plugin {
                if let Some(prop) = mind.prop.and_then(|(i, ..)| plugin_props.props.get(i)) {
                    mind.refusals.remember(prop.id, &plugin_props.settings);
                } else if let Some(id) = mind.brain.plugin {
                    mind.refusals.remember(skate_core::living_world::peds::plugins::conversation_key(id), &plugin_props.settings);
                }
            }
            // `82E3DBE0`: the seated owner (OwnPluginObject, `brain+3277` bit 0x01) of a moving object jumps off forward:
            // intent Avoid / Jump / Forward -> motiongraph_avoid SitJump -> JumpForward `SitAvoidFwd` (blend 0.1), speech
            // PedestrianAvoidJump (4). The gate `[ped+5712]+1416 == 0` is not decoded (taken as open).
            // The body plays it (`advance_peds`) while the flag is up.
            let anim = body.player.reaction_anim();
            let seated = mind.brain.own_plugin_object && anim == Some(SIT_CYCLE);
            if seat_pushed.contains(&me) && seated && !mind.sit_avoid {
                mind.brain.speech = Some(SIT_AVOID_SPEECH);
                mind.sit_avoid = true;
                mind.sit_avoid_from = Some(at);
                info!("PED_SIT_AVOID ped=#{} jumps off forward (its seat moved) locked={} tick={tick}", ped.id.serial, mind.brain.locked_at.is_some());
            } else if mind.sit_avoid && !seated {
                // `advance_peds` started the jump (or the ped left the seat another way first).
                mind.sit_avoid = false;
                info!("PED_SIT_AVOID ped=#{} played={} anim={anim:?} locked={} tick={tick}", ped.id.serial, anim == Some(SIT_AVOID_CLIP), mind.brain.locked_at.is_some());
            } else if !mind.sit_avoid && anim != Some(SIT_AVOID_CLIP) {
                if let Some(from) = mind.sit_avoid_from.take() {
                    let moved = ((at[0] - from[0]).powi(2) + (at[2] - from[2]).powi(2)).sqrt();
                    info!("PED_SIT_AVOID_DONE ped=#{} moved={moved:.2} anim={anim:?} tick={tick}", ped.id.serial);
                }
            }
            // HasPlugin: a conversation member, or a ped that took a world prop.
            let prop_wp = mind.prop.and_then(|(i, id, w)| plugin_props.props.get(i).filter(|p| p.id == id && !streamed_out(id) && !p.disabled).and_then(|p| p.waypoints.get(w)).copied());
            // A prop lost without ExitPlugin (streamed out, disabled, moved away) releases its waypoint like ExitPlugin
            // does: retail's waypoint slot `ped+0x1718` belongs to the plugin, and LockFirstWaypoint (`826A0580`) only
            // locks an empty slot, so a stale prop waypoint would steer a later conversation (b186).
            if mind.prop.is_some() && prop_wp.is_none() {
                if let Some((i, id, _)) = mind.prop.take() {
                    let class = plugin_props.props.get_mut(i).filter(|p| p.id == id).map(|p| {
                        p.release(me);
                        p.class.clone()
                    });
                    info!("PED_PLUGIN_PROP ped=#{} leaves {} {:016X} (dropped) tick={tick}", ped.id.serial, class.as_deref().unwrap_or("?"), id);
                    // A seated ped stands up (Sit2Stand) before its next behaviour, the way a packet that ends mid-clip
                    // does: the sit cycle was held until a StandUp stage that never comes once the seat is dropped
                    // (laptop 2026-10-19: #105 left its seat for a greet and floated seated in the sit idle).
                    if body.player.reaction_anim() == Some(SIT_CYCLE) && body.plugin_motion.state == TauntClip::Playing && !body.plugin_motion.orphaned {
                        body.plugin_motion.orphaned = true;
                        info!("PED_PLUGIN_MOTION ped=#{} seat dropped while seated, standing up tick={tick}", ped.id.serial);
                    }
                }
                mind.brain.release_plugin_waypoint();
            }
            // The locked waypoint is read live (`826AB9C0` re-reads its position on every evaluation): a seat that moved
            // with its prop is where the ped walks and sits.
            if let (Some(w), true) = (prop_wp, mind.brain.waypoint.is_some()) {
                mind.brain.waypoint = Some(w.position);
            }
            mind.brain.has_plugin = mind.brain.plugin.is_some() || mind.prop.is_some();
            // ThrowHandPropAtTrashBin's target, the plugin object's hotpoint 0 (`826A7998`, b87 §3): our hotpoint props
            // are one prop per hotpoint and every stock bin has only hotpoint 0, so the taken prop's own point.
            mind.brain.plugin_target = mind.prop.and_then(|(i, id, _)| plugin_props.props.get(i).filter(|p| p.id == id)).and_then(|p| p.waypoints.first()).map(|w| w.position);
            let conv = mind.brain.plugin.and_then(|id| conversations.map.get(&id));
            let free: Vec<[f32; 3]> = match (conv, prop_wp) {
                (Some(c), _) => c.waypoints.iter().filter(|w| w.1.is_none()).map(|w| w.0).collect(),
                (None, Some(w)) => vec![w.position],
                _ => Vec::new(),
            };
            // A world prop's view: its waypoint (reserved at the offer) and a facing point along its orientation.
            let conversation = match (conv, prop_wp) {
                (Some(c), _) => Some(skate_core::living_world::peds::brain::ConversationInfo { complete: c.is_complete(), speaker: c.speaker(), center: c.center, free_waypoints: &free, speech: c.turn_speech() }),
                (None, Some(w)) => {
                    mind.brain.waypoint_facing = Some(w.facing);
                    let center = [w.position[0] + w.facing[0] * 10.0, w.position[1], w.position[2] + w.facing[2] * 10.0];
                    Some(skate_core::living_world::peds::brain::ConversationInfo { complete: false, speaker: None, center, free_waypoints: &free, speech: None })
                }
                _ => None,
            };
            let groups_now = &chase_groups.0;
            let groups = |chasee: u64| groups_now.get(&chasee).map(|g| g.info(max_chasers(chasee)));
            // UpdateBlockPrediction (`82D99540`): the chasee radius `G+1648` has no known writer: 0.0.
            let block = |chasee: u64| {
                let (r, q, g) = (record?, target(chasee)?, groups_now.get(&chasee)?);
                let v = observers.observers.get(chasee.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
                Some((skate_core::living_world::peds::chase::should_block(at, q, v, 0.0, r), g.formation_point(me, q)))
            };
            mind.brain.zombie = settings.zombie;
            // ChaseeIsWipedOut / the takedown gate: a player chasee's wiping-out bit; a ped chasee's flag (brain `+3277`
            // bit 0x04) is not modelled yet (false).
            let wiping_of = |id: u64| id >= PLAYER_TARGET_BASE && observers.wiping.get((id - PLAYER_TARGET_BASE) as usize).copied().unwrap_or(false);
            let mut host = skate_core::living_world::peds::brain::BrainHost {
                behaviors: &graph.behaviors,
                conditions: &graph.conditions,
                brain: &mut mind.brain,
                settings: &settings.ped_brain.values,
                position: body.position.to_array(),
                heading: body.heading,
                skater: observers.observers.first().map(|o| (o.position, o.velocity)),
                target_position: &target,
                chase: skate_core::living_world::peds::brain::ChaseView { me, record, groups: Some(&groups), takedowns: Some(&choose), block: Some(&block), conversation, velocity: Some(&velocity_of), own_velocity: body.velocity.to_array(), wiping: Some(&wiping_of) },
            };
            controller.update(program, dt, &mut host);
            if std::mem::take(&mut mind.brain.collision_began) {
                info!("PED_COLLIDING ped=#{} begins: collision broadcast tick={tick}", ped.id.serial);
                events.write(PedEvent::Colliding { id: ped.id });
            }
            // The Plugin state runs the plugin's own graph on the same brain (`8269F248`).
            let plugin_graph = match mind.prop {
                Some((i, ..)) => plugin_props.props.get(i).and_then(|p| Some((p.class.clone(), data.plugin_graphs.get(&p.class)?))).map(|(k, g)| (k, &**g)),
                None => data.conversation_graph.as_deref().map(|g| (CONVERSATION_GRAPH.to_owned(), g)),
            };
            match (mind.brain.in_plugin, plugin_graph) {
                (true, Some((key, pg))) => {
                    let states = pg.graph.runtime.program.topology.states.len();
                    if let Some(old) = plugin_controller_for(&mut mind.plugin_controller, &mut mind.plugin_graph, &key, states) {
                        // The prop the ped used streamed out or was dropped while it stayed in the Plugin state: the
                        // other graph starts fresh (its controller was sized for the old one; a session crashed, 2026-10-12).
                        info!("PED_PLUGIN ped=#{} graph {old} -> {key} tick={tick}", ped.id.serial);
                        mind.plugin_state = None;
                    }
                    let pc = mind.plugin_controller.as_mut().expect("plugin controller just made");
                    let mut host = skate_core::living_world::peds::brain::BrainHost {
                        behaviors: &pg.behaviors,
                        conditions: &pg.conditions,
                        brain: &mut mind.brain,
                        settings: &settings.ped_brain.values,
                        position: body.position.to_array(),
                        heading: body.heading,
                        skater: observers.observers.first().map(|o| (o.position, o.velocity)),
                        target_position: &target,
                        chase: skate_core::living_world::peds::brain::ChaseView { me, record, groups: Some(&groups), takedowns: Some(&choose), block: Some(&block), conversation, velocity: Some(&velocity_of), own_velocity: body.velocity.to_array(), wiping: Some(&wiping_of) },
                    };
                    pc.update(&pg.graph.runtime.program, dt, &mut host);
                    if mind.prop.is_some() && pc.frame.current == mind.plugin_state && tick % 150 == 0 {
                        // Progress of a ped on a world prop (logs must diagnose a stall).
                        let name = |s: Option<usize>| s.and_then(|s| pg.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
                        let dist = mind.brain.waypoint.map(|w| ((w[0] - body.position.x).powi(2) + (w[2] - body.position.z).powi(2)).sqrt());
                        // The plugin's give-up timer (`PluginTimeout`, 60 s from EnterInteraction) and how many brain timers
                        // run: a ped stuck past 60 s must show why the timer did not expire (2026-10-14).
                        let timeout = mind.brain.timers.get(&skate_core::living_world::peds::brain::timers::index("PluginTimeout")).copied();
                        info!("PED_PLUGIN_WAIT ped=#{} in {} dist={:?} at=[{:.2}, {:.2}, {:.2}] approach={:?} slide={:?} route={} plugin_timeout={timeout:?} timers={} tick={tick}", ped.id.serial, name(pc.frame.current), dist.map(|d| (d * 1000.0).round() / 1000.0), body.position.x, body.position.y, body.position.z, mind.brain.approach.map(|a| a.1), mind.brain.approach_slide, body.nav.route.is_some(), mind.brain.timers.len());
                    }
                    if pc.frame.current != mind.plugin_state {
                        let name = |s: Option<usize>| s.and_then(|s| pg.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
                        let dist = mind.brain.waypoint.map(|w| ((w[0] - body.position.x).powi(2) + (w[2] - body.position.z).powi(2)).sqrt());
                        info!("PED_PLUGIN ped=#{} {} -> {} waypoint={:?} dist={:?} at=[{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, name(mind.plugin_state), name(pc.frame.current), mind.brain.waypoint.map(|w| [w[0].round(), w[2].round()]), dist.map(|d| (d * 1000.0).round() / 1000.0), body.position.x, body.position.y, body.position.z);
                        mind.plugin_state = pc.frame.current;
                    }
                }
                _ => {
                    mind.plugin_controller = None;
                    mind.plugin_state = None;
                    mind.plugin_graph = None;
                }
            }
            // The motion side of the monitored packets (`82E29540`): a plugin state plays the front packet, releases its
            // held cycle on the next stage intent and ends the packet when its clips are done (`IntentStageComplete`
            // decrement, `MajorIntentComplete`); packets whose motion state is not ported end at their last stage.
            {
                use skate_core::living_world::peds::plugin_motion::motion_for;
                let run = &mut body.plugin_motion;
                match run.packet.clone() {
                    None => {
                        if let Some(name) = mind.brain.monitored.iter().find(|(n, p)| p.active && motion_for(n).is_some()).map(|(n, _)| n.clone()) {
                            info!("PED_PLUGIN_MOTION ped=#{} {name} start tick={tick}", ped.id.serial);
                            *run = PluginMotionRun { packet: Some(name), state: TauntClip::Requested, ..Default::default() };
                        }
                    }
                    Some(name) => match mind.brain.monitored.get_mut(&name).filter(|_| !run.orphaned) {
                        // The packet ended mid-clip: its clips play out (the cycle released) before the run ends.
                        None if run.state == TauntClip::Playing => {
                            if !run.orphaned {
                                info!("PED_PLUGIN_MOTION ped=#{} {name} ended mid-clip, standing up tick={tick}", ped.id.serial);
                            }
                            run.orphaned = true;
                        }
                        None => *run = PluginMotionRun::default(),
                        Some(p) => {
                            let m = motion_for(&name).expect("only plugin states start a run");
                            if run.state == TauntClip::Done {
                                if m.decrement && p.stage > 0 {
                                    p.stage -= 1;
                                }
                                p.active = false;
                                info!("PED_PLUGIN_MOTION ped=#{} {name} complete tick={tick}", ped.id.serial);
                                *run = PluginMotionRun::default();
                            } else if m.steps.iter().any(|st| st.hold_until.is_some() && st.hold_until == p.current()) {
                                run.release = true;
                            }
                        }
                    },
                }
            }
            // SpawnInteractionBasedHandProp (`826AE950` -> `82E3DDA0`): the plugin prop's hand prop is requested
            // (`brain+3279` 0x01) with its record's bools; `ped_hand_props::sync_hand_props` creates the object.
            if std::mem::take(&mut body.plugin_motion.hand_prop_due) {
                let class = mind.prop.and_then(|(i, _, _)| plugin_props.props.get(i)).map(|p| p.class.clone());
                let list = class.as_ref().and_then(|c| data.plugins.classes.get(c)).map(|c| c.hand_props.clone()).unwrap_or_default();
                let roll = mind.brain.rng.as_mut().map_or(0.0, |r| r.unit());
                match skate_core::living_world::peds::brain::HandProp::pick(&list, roll) {
                    Some(key) => {
                        mind.brain.hand_prop.request(key);
                        if let Some(r) = data.hand_props.props.get(key) {
                            let h = &mut mind.brain.hand_prop;
                            (h.disposable, h.can_sit, h.can_attack_throw) = (r.disposable, r.can_sit, r.can_attack_throw);
                        }
                        info!("PED_HAND_PROP ped=#{} requested {key} from {} tick={tick}", ped.id.serial, class.as_deref().unwrap_or("?"));
                        events.write(PedEvent::HandProp { id: ped.id, key: key.to_string() });
                    }
                    None => info!("PED_HAND_PROP ped=#{} none offered by {} tick={tick}", ped.id.serial, class.as_deref().unwrap_or("?")),
                }
            }
            mind.brain.settle_packets(&|name| name == "SGIntent" || skate_core::living_world::peds::plugin_motion::motion_for(name).is_some());
            // Group changes, in the order the graph asked for them.
            for request in std::mem::take(&mut mind.brain.chase_requests) {
                use skate_core::living_world::peds::brain::ChaseRequest;
                let (chasee, kind, reason) = match request {
                    ChaseRequest::Join { chasee } => {
                        // The formation offset: chaser - chasee at the join (`82D96C38`).
                        let offset = target(chasee).map_or([0.0; 3], |q| [at[0] - q[0], at[1] - q[1], at[2] - q[2]]);
                        let joined = chase_groups.0.entry(chasee).or_default().add(me, offset, max_chasers(chasee));
                        if !joined && mind.brain.chasee == Some(chasee) {
                            mind.brain.chasee = None;
                        }
                        (chasee, if joined { "join" } else { "join_refused" }, None)
                    }
                    ChaseRequest::Leave { chasee } => {
                        if let Some(g) = chase_groups.0.get_mut(&chasee) {
                            g.remove(me);
                        }
                        (chasee, "leave", None)
                    }
                    ChaseRequest::GiveUpPrimary { chasee } => {
                        chase_groups.0.get_mut(&chasee).map(|g| g.give_up_primary(me));
                        (chasee, "primary", None)
                    }
                    // Only a player gets the marker message (`vfn92` "is a player").
                    ChaseRequest::StateMessage { target, state } => {
                        if target >= PLAYER_TARGET_BASE {
                            let kind = ["state_warn", "state_chase", "state_tired", "state_giveup", "state_other"][usize::from(state.min(4))];
                            info!("PED_CHASE ped=#{} {kind} chasee={target} tick={tick}", ped.id.serial);
                            events.write(PedEvent::Chase { id: ped.id, chasee: target, kind, reason: None });
                        }
                        continue;
                    }
                    ChaseRequest::Takedown { target } | ChaseRequest::TakedownFailed { target } => {
                        let success = matches!(request, ChaseRequest::Takedown { .. });
                        let player = target >= PLAYER_TARGET_BASE;
                        if success {
                            // `826A4E50`: speech 65 when the player was taken down, else 19.
                            mind.brain.speech = Some(if player { 65 } else { 19 });
                            if player {
                                skater_takedowns.write(SkaterTakedownRequest { player: (target - PLAYER_TARGET_BASE) as usize, chaser: at });
                            }
                        }
                        info!("PED_TAKEDOWN ped=#{} target={target} success={success} entry={:?} takedowns={} tick={tick}", ped.id.serial, mind.brain.takedown_choice.map(|c| c.entry), mind.brain.takedowns);
                        events.write(PedEvent::Takedown { id: ped.id, target, success });
                        continue;
                    }
                    // `826A8420`: `T.vfn20(ped position)`, the takedown's latch on the skater.
                    ChaseRequest::Taze { target } => {
                        if target >= PLAYER_TARGET_BASE {
                            skater_takedowns.write(SkaterTakedownRequest { player: (target - PLAYER_TARGET_BASE) as usize, chaser: at });
                        }
                        info!("PED_TAZE ped=#{} target={target} tick={tick}", ped.id.serial);
                        events.write(PedEvent::Taze { id: ped.id, target });
                        continue;
                    }
                    // The tazer's burst sound for the state graph's TazerCycTime (2.0 s).
                    ChaseRequest::TazerOn => {
                        tazers.write(crate::world_audio::PedTazerEvent { ped: entity_id, seconds: None });
                        continue;
                    }
                    ChaseRequest::Greeted { target } => {
                        greeted.push((target, me, at));
                        info!("PED_GREET ped=#{} target={target} tick={tick}", ped.id.serial);
                        continue;
                    }
                    // SpawnConversationArea (`826A6CD8`): none within 50 m of another; 3 m ahead (no check there);
                    // the starter and its target join. The waypoint generator (`82E1D620`, b134) drops the centre
                    // with a +-3 m world ray (`82E166C0`; a miss = no waypoints) and keeps the waypoints only when
                    // all land on low-cost navmesh (no roads): an area without waypoints dissolves.
                    ChaseRequest::SpawnConversation { target: partner } => {
                        let near = conversations.map.values().any(|c| (c.center[0] - at[0]).powi(2) + (c.center[2] - at[2]).powi(2) < conv_params.exclusion * conv_params.exclusion);
                        if !near && target(partner).is_some() {
                            let ahead = [at[0] + body.heading.sin() * conv_params.ahead, at[1], at[2] + body.heading.cos() * conv_params.ahead];
                            // Without a physics world (tests) the centre is used as it is.
                            let center = match physics.as_deref() {
                                Some(p) => ground(Some(p), Vec3::from_array(ahead), conv_params.drop_height, conv_params.drop_height).map(|y| [ahead[0], y, ahead[2]]),
                                None => Some(ahead),
                            };
                            let probe = |w: [f32; 3]| data.nav.as_deref().and_then(|n| n.locate_any(w).map(|p| (p.position, n.area(p.poly))));
                            // The group's category by probability, its rows the candidates.
                            let cats = data.conversations.by_entity.get(&ped.entity).cloned().unwrap_or_default();
                            let total: f32 = cats.iter().map(|c| c.0).sum();
                            let mut pick = conv_rand() * total;
                            let candidates = cats.iter().find(|c| {
                                pick -= c.0;
                                pick < 0.0
                            }).or(cats.last()).map(|c| c.1.clone()).unwrap_or_default();
                            conversations.next_id += 1;
                            let id = conversations.next_id;
                            let mut c = skate_core::living_world::peds::conversation::Conversation::new(id, center, candidates, &conv_params, data.nav.is_some().then_some(&probe as skate_core::living_world::peds::conversation::Probe), &mut conv_rand);
                            // The founders go through the area's offer test (`826A6CD8` -> manager slot 13 `826BFE18`, its
                            // result ignored): the starter, then the target (a ped only; a player has no participant).
                            use skate_core::living_world::peds::plugins::{conversation_qualifies, offer_conversation, OfferPed, RefusalMemory, TransferView, CONVERSATION_CLASS};
                            let rows = &data.conversations.rows;
                            let class = data.plugins.classes.get(CONVERSATION_CLASS);
                            let radius = class.and_then(|c| c.numbers.get("Hash_E2101F2B17A0E6B5").copied()).unwrap_or(20.0);
                            let transfer = class.and_then(|c| c.descriptor.as_ref()).and_then(|d| d.transfer.as_ref());
                            let odds = |e: &str| data.plugins.odds.get(e).map(Vec::as_slice).unwrap_or(&[]);
                            let pp = &mut *plugin_props;
                            let offer_rng = pp.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(world_seed, &[0x504c_5547])));
                            let my_view = TransferView::of(&mind.brain);
                            let mut starter = OfferPed { id: me, entity: &ped.entity, position: at, has_plugin: mind.brain.has_plugin || mind.prop.is_some(), odds: odds(&ped.entity), memory: &mut mind.refusals };
                            let mut q = |c: &skate_core::living_world::peds::conversation::Conversation, p: &OfferPed| conversation_qualifies(transfer, c, p.entity, my_view, rows);
                            let starter_joined = offer_conversation(&mut c, radius, &mut starter, &mut q, rows, &pp.settings, offer_rng);
                            let mut partner_joined = false;
                            if let (Some((kind, pos)), Some(&(_, in_plugin, view))) = (entities.get(&partner), busy_now.get(&partner)) {
                                // The partner's refusal memory is stored by its own think (`pending_refusals`); a new area is in
                                // nobody's memory yet.
                                let mut memory = RefusalMemory::default();
                                let mut other = OfferPed { id: partner, entity: kind, position: *pos, has_plugin: in_plugin, odds: odds(kind), memory: &mut memory };
                                let mut q = |c: &skate_core::living_world::peds::conversation::Conversation, p: &OfferPed| conversation_qualifies(transfer, c, p.entity, view, rows);
                                partner_joined = offer_conversation(&mut c, radius, &mut other, &mut q, rows, &pp.settings, offer_rng);
                                conversations.pending_refusals.extend(memory.entries.iter().map(|e| (partner, e.0)));
                            }
                            info!(
                                "PED_CONVERSATION id={id} start ped=#{} with={partner} joined={starter_joined}/{partner_joined} center=[{:.1}, {:.1}, {:.1}] dropped={} waypoints={} tick={tick}",
                                ped.id.serial, c.center[0], c.center[1], c.center[2], center.is_some(), c.waypoints.len()
                            );
                            conversations.map.insert(id, c);
                            if starter_joined {
                                mind.brain.plugin = Some(id);
                            }
                        }
                        continue;
                    }
                    ChaseRequest::LockWaypoint { at: w } => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            c.lock_closest_waypoint(me, w);
                        }
                        continue;
                    }
                    ChaseRequest::UnlockWaypoint => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            c.unlock_waypoint(me);
                        }
                        continue;
                    }
                    ChaseRequest::ExitPlugin if mind.prop.is_some() => {
                        if let Some((i, id, _)) = mind.prop.take() {
                            if let Some(p) = plugin_props.props.get_mut(i).filter(|p| p.id == id) {
                                p.release(me);
                                info!("PED_PLUGIN_PROP ped=#{} leaves {} {:016X} tick={tick}", ped.id.serial, p.class, id);
                            }
                        }
                        mind.brain.has_plugin = false;
                        mind.brain.release_plugin_waypoint();
                        mind.plugin_controller = None;
                        mind.plugin_state = None;
                        mind.plugin_graph = None;
                        continue;
                    }
                    ChaseRequest::ExitPlugin => {
                        if let Some(id) = mind.brain.plugin.take() {
                            use skate_core::living_world::peds::conversation::Left;
                            let left = conversations.map.get_mut(&id).map(|c| (c.is_complete(), c.leave(me)));
                            let how = match left {
                                Some((false, Left::Remaining)) => "abort",
                                Some((_, Left::Empty)) => "empty",
                                _ => "leave",
                            };
                            if matches!(left, Some((_, Left::Empty))) {
                                conversations.map.remove(&id);
                            }
                            info!("PED_CONVERSATION id={id} leave ped=#{} result={how} tick={tick}", ped.id.serial);
                        }
                        mind.brain.has_plugin = false;
                        mind.brain.release_plugin_waypoint();
                        continue;
                    }
                    ChaseRequest::SignalInPosition => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            let rows = &data.conversations.rows;
                            let before = c.state;
                            c.signal_in_position(me, rows, &mut conv_rand);
                            if before < 2 && c.state == 2 {
                                info!("PED_CONVERSATION id={} begins row={:?} value={:?} tick={tick}", c.id, c.row.and_then(|r| rows.get(r)).map(|r| r.name.as_str()), c.value);
                            }
                        }
                        continue;
                    }
                    ChaseRequest::PassTurn => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            c.pass_turn(&mut conv_rand);
                            info!("PED_CONVERSATION id={} turn state={} speaker={:?} tick={tick}", c.id, c.state, c.speaker());
                        }
                        continue;
                    }
                    ChaseRequest::Spoke => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get(&id)) {
                            listening.extend(c.listeners());
                        }
                        continue;
                    }
                    // TakedownTauntVictim (`826A70E0`): speech 65 when the victim is the player, else 19.
                    ChaseRequest::Taunt { target } => {
                        mind.brain.speech = Some(if target >= PLAYER_TARGET_BASE { 65 } else { 19 });
                        body.taunt = TauntClip::Requested;
                        info!("PED_TAUNT ped=#{} target={target} tick={tick}", ped.id.serial);
                        continue;
                    }
                    ChaseRequest::HandPropClip { clip } => {
                        body.plugin_motion.throw_clip = Some((clip, settings.ped_brain.values.hand_prop.clip_blend));
                        info!("PED_HAND_PROP ped=#{} throw clip {clip} target={:?} tick={tick}", ped.id.serial, mind.brain.hand_prop.throw.map(|t| t.target));
                        continue;
                    }
                    ChaseRequest::TazerClip { clip } => {
                        body.plugin_motion.tazer_clip = Some(clip);
                        continue;
                    }
                    ChaseRequest::HandPropReleased { velocity } => {
                        mind.hand_prop_release = Some(velocity);
                        continue;
                    }
                    ChaseRequest::MoodReset { target } => {
                        mind.mood.forget(target);
                        continue;
                    }
                    ChaseRequest::GroupEnd { chasee, reason } => {
                        if let Some(g) = chase_groups.0.get_mut(&chasee) {
                            g.end_reason = Some(reason);
                        }
                        (chasee, "group_end", Some(reason))
                    }
                };
                info!("PED_CHASE ped=#{} {kind} chasee={chasee} reason={reason:?} group={:?} tick={tick}", ped.id.serial, chase_groups.0.get(&chasee).map(|g| &g.chasers));
                events.write(PedEvent::Chase { id: ped.id, chasee, kind, reason });
            }
        }
        // Intercept: each tick a new goal from the solver (`826A3BF0`): the chasee must be
        // reachable (`sub_82C465A8`, our navmesh line) and the goal on the mesh (`sub_82C460E0`).
        // A locked ped drops its route (the plugin's waypoint walk is over).
        if mind.brain.position_locked && body.nav.route.is_some() {
            body.nav.set_route(None);
        }
        use skate_core::living_world::peds::brain::ChaseSteer;
        let one_point = |p: [f32; 3]| Some(skate_core::living_world::peds::PedRoute { points: vec![p], looped: false });
        match (mind.brain.motion_intent, mind.brain.chasee, data.nav.as_deref()) {
            // LostChasee: walk to the last known position (a one-point route).
            _ if mind.brain.search_point.is_some() => {
                let p = mind.brain.search_point.unwrap_or_default();
                if mind.chase_goal.take().is_some() || body.nav.route.as_ref().is_none_or(|r| r.points.first() != Some(&p)) {
                    body.nav.set_route(one_point(p));
                }
            }
            // ZombieFollow (`826A9250`): walk / run to the brain's goal (the player or a point of the ring round them).
            (Some(skate_core::living_world::peds::brain::motion::ZOMBIE_FOLLOW), ..) if mind.brain.zombie_goal.is_some() => {
                let p = mind.brain.zombie_goal.unwrap_or_default();
                mind.chase_goal = None;
                if body.nav.route.as_ref().is_none_or(|r| r.points.first() != Some(&p)) {
                    body.nav.set_route(one_point(p));
                }
            }
            // ApproachWantTarget (`8269FDF0`): walk to the want's target.
            _ if mind.brain.approach.is_some() => {
                let p = mind.brain.approach.map(|a| a.0).unwrap_or_default();
                mind.chase_goal = None;
                let d = [p[0] - at[0], p[2] - at[2]];
                let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
                match mind.brain.approach_slide {
                    // TargetWaypoint's slide (`slideDistance` / `slideSpeed`): the last stretch
                    // moves the body straight onto the point (ours: no slide clip).
                    Some((slide, speed)) if dist <= slide => {
                        body.nav.set_route(None);
                        mind.approach_route = false;
                        if dist > 1e-4 {
                            let step = (speed * dt).min(dist);
                            body.position.x += d[0] / dist * step;
                            body.position.z += d[1] / dist * step;
                        }
                    }
                    _ => {
                        body.nav.set_route(one_point(p));
                        mind.approach_route = true;
                    }
                }
            }
            // PursueChasee (`826A43B0`): run to the formation point.
            (Some(skate_core::living_world::peds::brain::motion::INTERCEPT), Some(chasee), Some(_)) if mind.brain.chase_steer == Some(ChaseSteer::Pursue) => {
                if let (Some(q), Some(g)) = (target(chasee), chase_groups.0.get(&chasee)) {
                    let p = g.formation_point(me, q);
                    body.nav.set_route(one_point(p));
                    mind.chase_goal = Some(p);
                }
            }
            // BlockChasee (`826A40E8`): walk to the block point; at it the brain stands and faces.
            (Some(skate_core::living_world::peds::brain::motion::INTERCEPT), Some(_), Some(_)) if matches!(mind.brain.chase_steer, Some(ChaseSteer::Block { .. })) => {
                mind.chase_goal = None;
                match mind.brain.block_point.filter(|_| mind.brain.speed_suggestion != Some(0.0)) {
                    Some(p) => body.nav.set_route(one_point(p)),
                    None => body.nav.set_route(None),
                }
            }
            (Some(skate_core::living_world::peds::brain::motion::INTERCEPT), Some(chasee), Some(mesh)) => {
                use skate_core::living_world::peds::chase;
                // CannotReachChaseTarget (`82E31D58`, b108): the intercept move's result sets the bit within 5 m of the
                // chasee and clears it beyond. Ours: a chasee off the navmesh (in water, on a roof) is a failed move
                // [inferred mapping of retail's result codes 0 / 1 / 3]; back on the navmesh clears it.
                if let Some(q) = target(chasee) {
                    let off_mesh = mesh.locate(q).is_none();
                    let near = (q[0] - at[0]).hypot(q[2] - at[2]) <= settings.ped_brain.values.unreachable_radius;
                    let now = off_mesh && near;
                    if now != mind.brain.target_unreachable {
                        info!("PED_UNREACHABLE ped=#{} chasee={chasee} {} off_mesh={off_mesh} near={near} tick={tick}", ped.id.serial, if now { "cannot reach" } else { "reachable again" });
                    }
                    if off_mesh {
                        mind.brain.target_unreachable = near;
                    } else {
                        mind.brain.target_unreachable = false;
                    }
                }
                if let (Some(q), Some(here)) = (target(chasee), mesh.locate(at)) {
                    let velocity = observers.observers.get(chasee.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
                    let speed = record.and_then(|r| r.run_speed()).unwrap_or(0.0);
                    // No record: the max lead time reads 0.0, so no goal.
                    let max_lead = record.map_or(0.0, |r| r.max_lead_time());
                    let goal = mesh.clear_line(here, q).then(|| chase::intercept(at, q, velocity, speed, max_lead, chase::predict_angle(data.chase_global.as_deref()))).flatten().filter(|i| mesh.locate(i.point).is_some());
                    if let Some(i) = goal {
                        if mind.chase_goal.is_none() {
                            info!("PED_INTERCEPT ped=#{} chasee={chasee} goal=[{:.1}, {:.1}, {:.1}] leads={} t={:.2} tick={tick}", ped.id.serial, i.point[0], i.point[1], i.point[2], i.leads, i.time);
                        }
                        body.nav.set_route(Some(skate_core::living_world::peds::PedRoute { points: vec![i.point], looped: false }));
                        mind.chase_goal = Some(i.point);
                    }
                }
            }
            _ => {
                if ended_move_drops_route(&mut mind.chase_goal, &mut mind.approach_route) {
                    body.nav.set_route(None);
                }
            }
        }
        // Flee movement: 15 m legs away from the threat, a new one within 2 m of the goal; the
        // leg goes to the nav as a one-point route (cleared when the flee ends).
        let flee = settings.ped_brain.flee;
        match (mind.brain.motion_intent, mind.brain.flee_from, data.nav.as_deref()) {
            (Some(skate_core::living_world::peds::brain::motion::FLEE), Some(threat), Some(mesh)) => {
                if mind.flee_goal.is_none_or(|g| skate_core::living_world::peds::flee::arrived(at, g, &flee)) {
                    if let Some(threat_at) = target(threat) {
                        let velocity = observers.observers.get(threat.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
                        let mut goal = skate_core::living_world::peds::flee::goal(at, threat_at, velocity, &flee);
                        // The navmesh cast (`sub_82C465A8`): a blocked leg ends at the last clear
                        // point along it (`sub_82C46208`'s rewrite, inferred).
                        if let Some(here) = mesh.locate(at) {
                            if !mesh.clear_line(here, goal) {
                                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                                for _ in 0..8 {
                                    let mid = (lo + hi) * 0.5;
                                    let p = [at[0] + (goal[0] - at[0]) * mid, at[1], at[2] + (goal[2] - at[2]) * mid];
                                    if mesh.clear_line(here, p) {
                                        lo = mid;
                                    } else {
                                        hi = mid;
                                    }
                                }
                                goal = [at[0] + (goal[0] - at[0]) * lo, at[1], at[2] + (goal[2] - at[2]) * lo];
                            }
                        }
                        body.nav.set_route(Some(skate_core::living_world::peds::PedRoute { points: vec![goal], looped: false }));
                        mind.flee_goal = Some(goal);
                        info!("PED_FLEE ped=#{} from={} goal=[{:.1}, {:.1}, {:.1}] tick={tick}", ped.id.serial, threat, goal[0], goal[1], goal[2]);
                    }
                }
            }
            // RunFromHonker (`826A1358`): every frame while the car exists, a goal 10 m sideways of its line; a
            // car that is gone leaves the last goal (retail keeps it).
            (Some(skate_core::living_world::peds::brain::motion::RUN_FROM_HONKER), _, _) => {
                if let Some((car_at, dir)) = mind.brain.honker.and_then(|c| car_pose.get(&c)) {
                    if let Some((goal, speed)) = skate_core::living_world::peds::honk::run_goal(at, *car_at, *dir, &settings.ped_brain.run_from_honker) {
                        if mind.flee_goal.is_none() {
                            info!("PED_HONKED ped=#{} car={:?} goal=[{:.1}, {:.1}, {:.1}] tick={tick}", ped.id.serial, mind.brain.honker, goal[0], goal[1], goal[2]);
                        }
                        body.nav.set_route(Some(skate_core::living_world::peds::PedRoute { points: vec![goal], looped: false }));
                        mind.flee_goal = Some(goal);
                        mind.brain.speed_suggestion = Some(speed);
                    }
                }
            }
            _ => {
                if mind.flee_goal.take().is_some() {
                    body.nav.set_route(None);
                }
            }
        }
        // Speech: the brain stores the value on the ped (`ped+2468`); PedestrianSpeech speaks when
        // it changes, so only a change goes to the audio side (a state re-sending its value stays
        // silent, as in retail).
        if mind.brain.speech != mind.spoken {
            mind.spoken = mind.brain.speech;
            // A conversation turn's variant and row value go with its line only.
            let topic = mind.brain.speech_topic.take();
            if let Some(value) = mind.brain.speech {
                let state = controller.frame.current.and_then(|s| graph.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
                info!("PED_SPEECH ped=#{} value={value} topic={topic:?} state={state} tick={tick}", ped.id.serial);
                speech.write(crate::world_audio::PedSpeechEvent { ped: entity_id, value: crate::world_audio::SpeechValue(value), topic });
                events.write(PedEvent::Speech { id: ped.id, value, topic, state: state.to_string() });
            }
        }
        let state_now = controller.frame.current.and_then(|s| graph.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str()).to_string();
        if controller.frame.current != mind.state {
            let name = |s: Option<usize>| s.and_then(|s| graph.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
            let key = (1 << 63) | (mind.state.map_or(0, |s| s as u64 + 1) << 24) | controller.frame.current.map_or(0, |s| s as u64 + 1);
            if let Some(held) = mind.log_repeats.allow(key, tick) {
                let held = if held > 0 { format!(" (+{held} held back)") } else { String::new() };
                info!("PED_BRAIN ped=#{} {} -> {} intent={:?} wants={:?} tick={tick}{held}", ped.id.serial, name(mind.state), name(controller.frame.current), mind.brain.motion_intent, mind.brain.wants.keys().collect::<Vec<_>>());
            }
            // Why a new chase was refused (NewChasee/CancelChase: not CanNewChaseStart or not CanChaseeAddNewChaser).
            if name(controller.frame.current) == "CancelChase" {
                let chasee = mind.brain.wants.get("angrychase").map(|w| w.target);
                let group = chasee.and_then(|c| chase_groups.0.get(&c).map(|g| g.info(max_chasers(c))));
                if let Some(held) = mind.log_repeats.allow(0x4341_4e43, tick) {
                    info!(
                        "PED_CHASE_CANCEL ped=#{} chasee={chasee:?} chases_enabled={} group={:?} max_chasers={:?} tick={tick}{}",
                        ped.id.serial,
                        settings.ped_brain.values.chases_enabled,
                        group,
                        chasee.map(&max_chasers),
                        if held > 0 { format!(" (+{held} held back)") } else { String::new() }
                    );
                }
            }
            mind.state = controller.frame.current;
        }
        log_still_ped(ped, body, mind, &state_now, &conversations.map, tick, &mut still_budget);
    }
}

/// The mod-facing nav settings (`LivingWorldSettings::ped_nav`, mod `ped_nav`; reset when the mod is disabled) into the
/// resource the ped step reads.
pub(super) fn sync_nav_settings(settings: Res<super::LivingWorldSettings>, mut nav: ResMut<PedNavSettings>) {
    if settings.is_changed() && *nav != settings.ped_nav {
        *nav = settings.ped_nav.clone();
    }
}

