//! Simulated NPC skaters (doc 26, "Simulated NPC skaters", M7): near the player an NPC skater is
//! a full physics skater (its own board context and `SkaterRuntime` in the shared world) driven by
//! its AI record; farther away it stays on the replay tier.
//!
//! Retail runs every ambient NPC skater simulated (at most 3 offline). The distance switch is an
//! engine choice for cost (design: simulated within 40 m, never switched while airborne or
//! grinding); the handover keeps the line's speed (retail never hands over). Off by default until
//! play-tested: `SKATE_NPC_SIM=1` or `sdk.world.set_tuning("living_world", {npc_simulated = {...}})`.
//!
//! Per tick (after the population and the replay cursors advanced): the record is built from the
//! cursor (`skate_core::living_world::ai_record`), the retail spawn push applies while the skater
//! is still on its node, and `GamePhysics::advance_npc_skater` runs the same frame as the player.
//! The NPC is drawn from its simulated pose (`render_pose`, like the player); its population
//! position and audio still follow the cursor. A physics error drops it back to the replay tier.

use super::npc_skaters::{NpcReplay, NpcSkater};
use super::{LivingWorldObservers, LivingWorldSettings, NetRole, PopulationState};
use crate::physics::{GamePhysics, PlayerControls, SkaterPhysicsContext, SkaterRuntime};
use bevy::prelude::*;
use skate_core::living_world::ai_record;
use skate_core::living_world::replay::node_flags;
use skate_core::player::state::PhysicalStateId;

/// The simulated tier's rules (a mod may change them; `Default` = off, 40 m, 3 skaters).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SimulatedTierSettings {
    pub enabled: bool,
    /// Simulated within this distance of an observer (m); handed back beyond 1.1 x.
    pub radius: f32,
    /// At most this many simulated at once (retail keeps 3 ambient skaters).
    pub max: usize,
    /// Bail respawn delay of an ambient skater, seconds, and its clamp (retail
    /// `sub_8246EE30`: 5.0 `0x821F1790` for AI kind 0, clamped to 1.5 `0x822249B4` .. 7.9
    /// `0x822572EC`).
    pub respawn_seconds: f32,
    pub respawn_min: f32,
    pub respawn_max: f32,
    /// The PathController's ActionGraph signals (anticipation and trick dispatch, retail values).
    pub signals: skate_core::living_world::ai_signals::SignalSettings,
    /// Mode 7 (a prop blocks the line): step off and walk back to the line past it with controller B
    /// ("NavMeshController", retail on; a mod may turn it off) and its numbers, plus the sub-mode / reposition
    /// numbers of the line controller.
    pub walk_back: bool,
    /// The PathController keeps a skater "on path" only within this distance of its line (`sub_8246DB50`: 1.0 m,
    /// b109); off it, the recorded trick is not dispatched (the sim is not where the recording jumped).
    pub on_path: f32,
    pub navmesh: skate_core::living_world::controller_b::ControllerBSettings,
    pub controller: skate_core::living_world::ai_controller::ControllerSettings,
}

impl Default for SimulatedTierSettings {
    fn default() -> Self {
        // `SKATE_NPC_SIM_RADIUS` (m): a test knob so an unattended run sees many switches to physics.
        let radius = std::env::var("SKATE_NPC_SIM_RADIUS").ok().and_then(|v| v.parse::<f32>().ok()).filter(|r| r.is_finite() && *r > 0.0).unwrap_or(40.0);
        Self { enabled: std::env::var("SKATE_NPC_SIM").ok().as_deref() == Some("1"), radius, max: 3, respawn_seconds: 5.0, respawn_min: 1.5, respawn_max: 7.9, signals: Default::default(), walk_back: true, on_path: 1.0, navmesh: Default::default(), controller: Default::default() }
    }
}

impl SimulatedTierSettings {
    /// The respawn delay in 60 Hz ticks.
    pub fn respawn_ticks(&self) -> u32 {
        let lo = self.respawn_min.min(self.respawn_max);
        (self.respawn_seconds.clamp(lo, self.respawn_max.max(lo)) * 60.0).round() as u32
    }
}

/// The wipeout bit of the skater's flags word (`flags2468` bit 18, the "Wipeout" graph
/// attribute): retail's physics step copies it to skater component `+59` (`sub_82DB6EC0`), the
/// PathController latches it into `pc+924` each tick (`sub_8246EF78`) and its rising edge starts
/// the respawn (`sub_8246EE30`).
pub(crate) const WIPEOUT_BIT: u32 = 0x0004_0000;

/// A simulated NPC skater's own physics and runtime.
#[derive(Component)]
pub(crate) struct NpcSim {
    context: SkaterPhysicsContext,
    body: SimBody,
    /// Ticks left until the bail respawn (`pc+884` / `pc+892`), while bailing.
    respawn_in: Option<u32>,
    /// The node the trick dispatcher handled last (`pc+820`).
    last_node: Option<u32>,
    /// Controller B while it is the active controller (`brain+104`), with the line and node it walks back to.
    walker: Option<Walker>,
}

/// Controller B's run: the line (`ctrl+592`) and node A re-attaches to when B hands back.
struct Walker {
    b: skate_core::living_world::controller_b::ControllerB,
    line: [u8; 16],
    node: u32,
}

/// What B reads from the simulated skater (b80): `[rec+56]+161` = SkaterOffBoard (Processed +2484 0x08000000),
/// `+160` = TransitioningOnOffBoard (+2480 0x4), both written by `82DB6EC0`; "can move" (`82466ED0`) = off the board
/// and not toggling (its `[rec+72]+308 / +309` terms are not identified: taken as clear); the hand-back byte
/// `[rec+28]+59` = the wipeout bit (`82DB6EC0` copies it there).
pub(crate) fn walker_input(runtime: &SkaterRuntime) -> skate_core::living_world::controller_b::BInput {
    // The skater, not the board (left lying once off): P = the animation root ([rec+20]+416), B+112 = its forward
    // row ([rec+20]+0 [inference: forward]).
    let root = runtime.animated_skeleton.roots.animation_to_world;
    let position = [root[3][0], root[3][1], root[3][2]];
    let forward = [root[2][0], 0.0, root[2][2]];
    let p = &runtime.player_input.processed;
    let off_board = p.flags_2484 & 0x0800_0000 != 0;
    let toggling = p.flags_2480 & 0x4 != 0;
    skate_core::living_world::controller_b::BInput {
        position,
        body: forward,
        facing: forward[0].atan2(forward[2]),
        abort_flag: false,
        state_hand_back: p.flags_2468 & WIPEOUT_BIT != 0,
        online: false,
        can_move: off_board && !toggling,
        byte_161: off_board,
        byte_160: toggling,
    }
}

/// A loaded skater (runtime, controls, camera): about 220 ms to load from disk (session 2026-10-13: 35 of 39
/// switches to physics hitched over 150 ms), so loaded ones are kept in `NpcSimPool` and placed again with the
/// player's teleport (`SkaterRuntime::travel`), as retail places its AI skater object again on a respawn.
pub(crate) struct SimBody {
    runtime: Box<SkaterRuntime>,
    controls: Box<PlayerControls>,
    camera: Box<crate::camera::CameraRuntime>,
}

/// Loaded skaters waiting for a simulated NPC; filled up to the sim limit on the first enabled tick.
#[derive(Resource, Default)]
pub(crate) struct NpcSimPool {
    /// With each its own physics context: its clock keeps running, so the camera's subject ticks stay monotonic.
    bodies: Vec<(SkaterPhysicsContext, SimBody)>,
    prewarmed: bool,
}

fn load_body(physics: &mut GamePhysics, config: &crate::config::Config, graphs: &crate::graph_runtime::StockGraphs, spawn: skate_core::physics::drive_frames::RetailAffineTransform) -> Result<(SkaterPhysicsContext, SimBody), String> {
    let t0 = std::time::Instant::now();
    let mut context = physics.new_skater_context(spawn)?;
    let runtime = Box::new(physics.load_skater_in_context(&mut context, &config.asset_root, graphs, config.difficulty.profile_key())?);
    let controls = Box::new(PlayerControls::load(&config.asset_root)?);
    let camera = Box::new(crate::camera::CameraRuntime::load(&config.asset_root)?);
    info!("NPC_SKATER_SIM_LOAD loaded a skater from disk in {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
    Ok((context, SimBody { runtime, controls, camera }))
}

/// The line frame as the teleport matrix ([right, up, forward, translation], as `modding::session::spawn_matrix`).
fn spawn_matrix(spawn: &skate_core::physics::drive_frames::RetailAffineTransform) -> [[f32; 4]; 4] {
    let c = spawn.basis.columns;
    let t = spawn.translation;
    [[c[0][0], c[0][1], c[0][2], 0.], [c[1][0], c[1][1], c[1][2], 0.], [c[2][0], c[2][1], c[2][2], 0.], [t.x, t.y, t.z, 0.]]
}

/// Move a simulated skater's loaded body into the pool (its entity goes back to replay or is despawned).
pub(crate) fn pool_from(world: &mut World, e: Entity) {
    let body = world.get_entity_mut(e).ok().and_then(|mut entity| entity.take::<NpcSim>()).map(|sim| (sim.context, sim.body));
    if let (Some(body), Some(mut pool)) = (body, world.get_resource_mut::<NpcSimPool>()) {
        pool.bodies.push(body);
    }
}

impl NpcSim {
    /// The traffic car this simulated skater holds in state 104 (the car side's held bit, b57 / b65).
    pub(crate) fn held_car(&self) -> Option<u32> {
        (self.body.runtime.player_state.current() == skate_core::player::state::PhysicalStateId::Skitching).then(|| self.body.runtime.skitch_state.held_car()).flatten()
    }

    pub(crate) fn render_pose(&self) -> &[skate_core::animation::output::NativeMatrix] {
        &self.body.runtime.render_pose
    }
}

fn nearest(observers: &LivingWorldObservers, p: [f32; 3]) -> f32 {
    observers.observers.iter().map(|o| ((o.position[0] - p[0]).powi(2) + (o.position[2] - p[2]).powi(2)).sqrt()).fold(f32::INFINITY, f32::min)
}

fn spawn_sim(
    physics: &mut GamePhysics,
    config: &crate::config::Config,
    graphs: &crate::graph_runtime::StockGraphs,
    target: &ai_record::LineTarget,
    pool: &mut NpcSimPool,
) -> Result<NpcSim, String> {
    let spawn = line_spawn(target);
    let (context, body) = match pool.bodies.pop() {
        Some(pooled) => pooled,
        None => load_body(physics, config, graphs, spawn)?,
    };
    let mut sim = NpcSim { context, body, respawn_in: None, last_node: None, walker: None };
    place(&mut sim, target)?;
    Ok(sim)
}

fn line_spawn(target: &ai_record::LineTarget) -> skate_core::physics::drive_frames::RetailAffineTransform {
    let q = target.frame;
    let basis = skate_core::physics::rigid_body::basis_from_quaternion(skate_core::physics::rigid_body::RetailQuaternion { x: q[0], y: q[1], z: q[2], w: q[3] });
    skate_core::physics::drive_frames::RetailAffineTransform {
        basis,
        translation: skate_core::math::Vector3::new(target.position[0], target.position[1], target.position[2]),
    }
}

/// Place a simulated skater on its line with the player's teleport and the spawn push as its velocity (engine
/// handover; retail never hands over), so the teleport's reset does not zero the speed (b109: the context velocity
/// alone was zeroed on the first step and the skater dropped in from rest). Its own context is kept (a fresh one
/// restarted the clock and the camera refused the older subject tick).
fn place(sim: &mut NpcSim, target: &ai_record::LineTarget) -> Result<(), String> {
    let spawn = line_spawn(target);
    // Retail's spawn push (`82C04168`, `ai_record::spawn_push`): on the node, the node's displacement x 60 x 0.75.
    let velocity = ai_record::spawn_push(target.position, target.position, target.step, false, false).unwrap_or([0.0; 3]);
    GamePhysics::set_context_velocity(&mut sim.context, velocity);
    if let Err(error) = sim.body.runtime.travel(spawn_matrix(&spawn), Some(velocity)) {
        debug!("NPC_SKATER_SIM place: teleport not queued ({error}); context velocity only");
    }
    sim.respawn_in = None;
    sim.last_node = None;
    sim.walker = None;
    Ok(())
}

/// Switch NPC skaters between the tiers and step the simulated ones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn simulate(
    mut commands: Commands,
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    state: Res<PopulationState>,
    config: Option<Res<crate::config::Config>>,
    graphs: Option<Res<crate::graph_runtime::StockGraphs>>,
    physics: Option<ResMut<GamePhysics>>,
    peds: Option<Res<super::peds::PedData>>,
    mut npcs: Query<(Entity, &NpcSkater, &mut NpcReplay, Option<&mut NpcSim>)>,
    mut events: MessageWriter<super::npc_skaters::NpcSkaterEvent>,
    mut calls: Local<u64>,
    mut pool: ResMut<NpcSimPool>,
) {
    let _span = super::timing::span(super::timing::NPC_SIM);
    let (Some(config), Some(graphs), Some(mut physics)) = (config, graphs, physics) else { return };
    if physics.failed {
        return;
    }
    let rules = settings.npc_simulated;
    *calls += 1;
    // Load the skaters once, up front (the first enabled tick, i.e. the map start), so a switch to physics later
    // takes a loaded one instead of reading it from disk mid-ride.
    if rules.enabled && !pool.prewarmed && settings.net_role != NetRole::Client {
        pool.prewarmed = true;
        let at = observers.observers.first().map_or([0.0; 3], |o| o.position);
        let spawn = skate_core::physics::drive_frames::RetailAffineTransform {
            basis: skate_core::physics::rigid_body::basis_from_quaternion(skate_core::physics::rigid_body::RetailQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }),
            translation: skate_core::math::Vector3::new(at[0], at[1] + 50.0, at[2]),
        };
        let t0 = std::time::Instant::now();
        let have = pool.bodies.len() + npcs.iter().filter(|n| n.3.is_some()).count();
        for _ in have..rules.max {
            match load_body(&mut physics, &config, &graphs, spawn) {
                Ok(pooled) => pool.bodies.push(pooled),
                Err(error) => {
                    warn!("NPC_SKATER_SIM pool: cannot load a skater: {error}");
                    break;
                }
            }
        }
        info!("NPC_SKATER_SIM pool: {} loaded skaters ready ({:.0} ms)", pool.bodies.len(), t0.elapsed().as_secs_f64() * 1000.0);
    }
    // Every 2 s: why each replay-tier NPC is not simulated (logs must diagnose).
    let report = rules.enabled && *calls % 120 == 0;
    let lines = super::npc_skaters::npc_lines(&state);
    let mut count = npcs.iter().filter(|n| n.3.is_some()).count();
    let mut sorted: Vec<_> = npcs.iter_mut().collect();
    sorted.sort_by_key(|n| n.1.id);
    for (e, npc, mut replay, sim) in sorted {
        let Some(target) = replay.cursor.line_target(&*lines) else { continue };
        let distance = nearest(&observers, target.position);
        match sim {
            Some(mut sim) => {
                let state_now = sim.body.runtime.player_state.current();
                let settled = matches!(state_now, PhysicalStateId::PhysicsGround | PhysicalStateId::SlideGround);
                // A skater simulated because a prop blocks it (mode 7, allowed at any distance below) stays simulated
                // until it is clear; handing it back while still blocked restarted the sim the next frame, every
                // frame (session game-20261013-183803: 12 restarts in one second at 79 m, ~75 ms each).
                let blocked = rules.walk_back && replay.avoid.last.mode == skate_core::living_world::avoid::AvoidMode::StepOff;
                let too_far = distance > rules.radius * 1.1 && !blocked;
                if (!rules.enabled || too_far || replay.cursor.finished) && settled {
                    // Back to the pool (the loaded skater is kept for the next switch to physics).
                    commands.queue(move |world: &mut World| pool_from(world, e));
                    count -= 1;
                    info!("NPC_SKATER_SIM #{} {} -> replay (distance {distance:.1} m)", npc.id.serial, npc.character);
                    continue;
                }
                let sim = &mut *sim;
                let replay = &mut *replay;
                // Bail: the line waits (the cursor is held back) until the respawn places the
                // skater back on it (retail: placed at the chosen node of its current path, path
                // state 8, then the spawn push; no fade).
                let wiped = sim.body.runtime.player_input.processed.flags_2468 & WIPEOUT_BIT != 0;
                if wiped && sim.respawn_in.is_none() {
                    let ticks = rules.respawn_ticks();
                    sim.respawn_in = Some(ticks);
                    info!("NPC_SKATER_BAIL #{} {} respawn in {:.2} s", npc.id.serial, npc.character, ticks as f32 / 60.0);
                    events.write(super::npc_skaters::NpcSkaterEvent::Bail { id: npc.id, respawn_seconds: ticks as f32 / 60.0 });
                }
                if let Some(left) = sim.respawn_in {
                    replay.avoid.lag += super::npc_skaters::FRAMES_PER_TICK as f32;
                    if left == 0 {
                        // The same skater placed again on its line, on the node (retail places its AI skater object
                        // again at the chosen node).
                        replay.cursor.frame_in_segment = 0;
                        let Some(target) = replay.cursor.line_target(&*lines) else { continue };
                        match place(sim, &target) {
                            Ok(()) => {
                                info!("NPC_SKATER_RESPAWN #{} {} at line node {}", npc.id.serial, npc.character, replay.cursor.node);
                                events.write(super::npc_skaters::NpcSkaterEvent::Respawned { id: npc.id, node: replay.cursor.node });
                            }
                            Err(error) => {
                                warn!("NPC_SKATER_SIM #{} {}: respawn failed, back to replay: {error}", npc.id.serial, npc.character);
                                commands.entity(e).remove::<NpcSim>();
                                count -= 1;
                            }
                        }
                        continue;
                    }
                    sim.respawn_in = Some(left - 1);
                }
                let deck = GamePhysics::context_deck(&sim.context);
                let forward = [deck.basis.columns[2][0], deck.basis.columns[2][1], deck.basis.columns[2][2]];
                let direct = skate_core::living_world::controller_b::DirectPath;
                let paths: &dyn skate_core::living_world::controller_b::PathService = match peds.as_deref().and_then(|p| p.nav.as_deref()) {
                    Some(mesh) => mesh,
                    None => &direct,
                };
                // Mode 7 (b66 / b70 / b78 / b79): the line controller steps off (sub-mode 3 and a one-shot
                // reposition; a second tick in mode 7 sets sub-mode 5 and posts WipeOutRequest), and the
                // reposition swaps to controller B, which walks the skater back to the chosen line node.
                let mut walk_intents: Vec<(String, f32)> = Vec::new();
                if rules.walk_back && sim.walker.is_none() {
                    use skate_core::living_world::{ai_controller, avoid::AvoidMode};
                    let a = &mut replay.avoid;
                    let mode_7 = a.last.mode == AvoidMode::StepOff;
                    let prop_node = a.last.steer_target.and_then(|id| a.last.entries.iter().find(|x| x.id == id)).and_then(|x| x.path).map_or(replay.cursor.node, |p| p.node);
                    let airborne = matches!(state_now as u32, 200..300);
                    if let Some(w) = ai_controller::step_off(&mut a.controller, &rules.controller, mode_7, airborne, false, prop_node) {
                        walk_intents.push(("WipeOutRequest".into(), w));
                        info!("NPC_STEP_OFF #{} {} wipe-out request (still blocked, node {prop_node})", npc.id.serial, npc.character);
                    }
                    if let Some(line) = lines.get(&replay.cursor.line) {
                        let nodes = line.nodes.len() as u32;
                        if let Some(node) = ai_controller::take_reposition(&mut a.controller, &rules.controller, false, *calls, mode_7, replay.cursor.node, prop_node, nodes) {
                            // 8246FE38 -> 82455728: the first calm node from there.
                            let node = ai_controller::safe_node(line, node, &rules.controller);
                            let n = &line.nodes[node as usize];
                            let l = (n.step[0] * n.step[0] + n.step[1] * n.step[1] + n.step[2] * n.step[2]).sqrt();
                            let dir = if l > 1e-6 { n.step.map(|x| x / l) } else { [0.0; 3] };
                            let input = walker_input(&sim.body.runtime);
                            match skate_core::living_world::controller_b::ControllerB::activate(&rules.navmesh, paths, n.position, dir, &input) {
                                Some(b) => {
                                    info!("NPC_WALK_BACK #{} {} start: node {node} at [{:.1}, {:.1}, {:.1}], {} waypoints", npc.id.serial, npc.character, n.position[0], n.position[1], n.position[2], b.path.len());
                                    events.write(super::npc_skaters::NpcSkaterEvent::WalkBack { id: npc.id, node, started: true });
                                    sim.walker = Some(Walker { b, line: replay.cursor.line, node });
                                }
                                None => info!("NPC_WALK_BACK #{} {} no plan to node {node}: stays with the line controller", npc.id.serial, npc.character),
                            }
                        }
                    }
                }
                if let Some(w) = sim.walker.as_mut() {
                    use skate_core::living_world::controller_b::BTick;
                    let input = walker_input(&sim.body.runtime);
                    match w.b.tick(&rules.navmesh, paths, &input) {
                        BTick::Idle => {}
                        BTick::Controls(c) => {
                            walk_intents.extend(c.intents().into_iter().map(|(k, v)| (k.to_string(), v)));
                        }
                        BTick::HandBack { reason, wipe_out } => {
                            if wipe_out {
                                walk_intents.push(("WipeOutRequest".into(), 1.0));
                            }
                            // A re-attaches by its saved line id at the node (`824689C0`); a success resets the
                            // line controller's sub-mode (b66).
                            replay.cursor = skate_core::living_world::replay::LineCursor::spawn(&*lines, w.line, w.node);
                            skate_core::living_world::ai_controller::reposition_done(&mut replay.avoid.controller);
                            info!("NPC_WALK_BACK #{} {} hand back ({reason:?}) at line node {}", npc.id.serial, npc.character, w.node);
                            events.write(super::npc_skaters::NpcSkaterEvent::WalkBack { id: npc.id, node: w.node, started: false });
                            sim.walker = None;
                        }
                    }
                }
                if sim.walker.is_some() {
                    // Controller B is the active controller: no PathController record, signals or spawn push (the
                    // AI source stays present but not fresh: +10689 set, +10688 clear, so the biped runs in the AI's
                    // direct mode on OB_Mag / OB_Turn, `82D310F8`; b80); the line waits.
                    if let Some(source) = sim.body.runtime.ai_physics.as_mut() {
                        source.fresh = false;
                    }
                    replay.avoid.lag += super::npc_skaters::FRAMES_PER_TICK as f32;
                    if let Err(error) = physics.advance_npc_skater(&mut sim.context, &mut sim.body.runtime, &mut sim.body.controls, &graphs, &mut sim.body.camera, &walk_intents) {
                        warn!("NPC_SKATER_SIM #{} {}: physics error, back to replay: {error}", npc.id.serial, npc.character);
                        commands.entity(e).remove::<NpcSim>();
                        count -= 1;
                    }
                    continue;
                }
                // The obstacle avoider's answer (`npc_avoid`): retail caps / floors the record
                // speed (`sub_82470830`) and moves the target across the path (`sub_82464E30`,
                // controller `+933` = steering).
                let avoid = &replay.avoid;
                let mut steered = target.clone();
                let v = target.step.map(|x| x * ai_record::FRAMES_PER_SECOND);
                let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if speed > 1e-3 && !avoid.low_prop() {
                    let k = avoid.last.shape_speed(speed, false) / speed;
                    steered.step = target.step.map(|x| x * k);
                }
                let right = super::npc_avoid::right_of(target.step);
                steered.position = std::array::from_fn(|i| target.position[i] + right[i] * avoid.offset);
                let steer = ai_record::SteerState {
                    byte_933: matches!(avoid.last.mode, skate_core::living_world::avoid::AvoidMode::Steer | skate_core::living_world::avoid::AvoidMode::Skitch),
                    ..Default::default()
                };
                let mut record = ai_record::build(&steered, forward, &steer);
                // The recorded jump ahead (`sub_8246DA18`): its arc and whether it lands in a grind.
                if let Some(line) = lines.get(&replay.cursor.line) {
                    if let Some((node, jump)) = ai_record::upcoming_trajectory(line, replay.cursor.node) {
                        ai_record::with_trajectory(&mut record, jump, ai_record::lands_in_grind(line, node.saturating_sub(1)));
                    }
                }
                sim.body.runtime.ai_physics = Some(crate::physics::AiPhysicsSource { record, fresh: true });
                if let Some(line) = lines.get(&replay.cursor.line) {
                    let node = &line.nodes[replay.cursor.node as usize];
                    let at = [deck.translation.x, deck.translation.y, deck.translation.z];
                    if let Some(v) = ai_record::spawn_push(at, node.position, target.step, replay.cursor.node == 0, false) {
                        GamePhysics::set_context_velocity(&mut sim.context, v);
                    }
                }
                // The PathController's ActionGraph signals: anticipation and the trick at its node.
                let intents = match lines.get(&replay.cursor.line) {
                    Some(line) => {
                        let at = [deck.translation.x, deck.translation.y, deck.translation.z];
                        let off_line = (at[0] - target.position[0]).hypot(at[2] - target.position[2]);
                        let along = target.step;
                        let heading_error = if along[0].hypot(along[2]) > 1e-5 { (forward[0] * along[2] - forward[2] * along[0]).atan2(forward[0] * along[0] + forward[2] * along[2]) } else { 0.0 };
                        let input = skate_core::living_world::ai_signals::SignalInput {
                            node: replay.cursor.node,
                            frame_in_segment: replay.cursor.frame_in_segment,
                            last_node: sim.last_node,
                            off_line,
                            heading_error,
                            in_trick: replay.cursor.current_trick() >= 0,
                        };
                        let cursor_line = replay.cursor.line;
                        let tricks = &replay.tricks;
                        let chosen = |n: u32| tricks.iter().rev().find(|r| r.line == cursor_line && r.node == n).map_or_else(|| line.node_trick(&line.nodes[n as usize]), |r| r.chosen);
                        let v = skate_core::living_world::ai_signals::signals(line, &rules.signals, &input, &chosen);
                        v
                    }
                    None => Vec::new(),
                };
                // Mode 4 (skitch; b65, main checked 0x8246FC78): GrabWorld every tick on the ground, no tricks.
                let state = sim.body.runtime.player_state.current();
                let mut intents = intents;
                intents.extend(walk_intents);
                skate_core::living_world::ai_signals::apply_skitch_mode(
                    &mut intents,
                    &rules.signals,
                    avoid.last.mode == skate_core::living_world::avoid::AvoidMode::Skitch,
                    (200..300).contains(&(state as u32)),
                    state == skate_core::player::state::PhysicalStateId::Skitching,
                );
                // Off the path (`sub_8246DB50`, 1.0 m) or off the board: no recorded trick (b109: tricks dispatched
                // to a sim away from its line or walking were the second bail cause).
                let deck_now = [deck.translation.x, deck.translation.z];
                let off_path = (deck_now[0] - target.position[0]).hypot(deck_now[1] - target.position[2]) > rules.on_path;
                let off_board = sim.body.runtime.player_input.processed.flags_2484 & 0x0800_0000 != 0;
                if off_path || off_board {
                    if intents.iter().any(|x| x.0 == "Trick") {
                        info!("NPC_SKATER_SIM_TRICK #{} {} dropped at node {}: {}", npc.id.serial, npc.character, replay.cursor.node, if off_board { "off the board" } else { "off the path" });
                    }
                    skate_core::living_world::ai_signals::drop_trick_dispatch(&mut intents);
                }
                // Sub-mode 4 (mode 6): the recorded node action is dropped (`ctrl+832 = 0`; b66).
                if avoid.low_prop() {
                    skate_core::living_world::ai_signals::drop_trick_dispatch(&mut intents);
                }
                // Logged after mode 4 so a dropped trick dispatch is not reported.
                if let Some(name) = intents.iter().position(|x| x.0 == "Trick").and_then(|i| intents.get(i + 1)) {
                    info!("NPC_SKATER_SIM_TRICK #{} {} {} node {} state {:?}", npc.id.serial, npc.character, name.0, replay.cursor.node, state);
                }
                sim.last_node = Some(replay.cursor.node);
                if let Err(error) = physics.advance_npc_skater(&mut sim.context, &mut sim.body.runtime, &mut sim.body.controls, &graphs, &mut sim.body.camera, &intents) {
                    warn!("NPC_SKATER_SIM #{} {}: physics error, back to replay: {error}", npc.id.serial, npc.character);
                    commands.entity(e).remove::<NpcSim>();
                    count -= 1;
                }
            }
            None => {
                // Clients draw what the host sends; only the authority simulates.
                let Some(line) = lines.get(&replay.cursor.line) else { continue };
                let flags = line.nodes[replay.cursor.node as usize].flags;
                let wait = if !rules.enabled {
                    Some("off")
                } else if settings.net_role == NetRole::Client {
                    Some("client")
                } else if count >= rules.max {
                    Some("at max")
                } else if distance > rules.radius && !(rules.walk_back && replay.avoid.last.mode == skate_core::living_world::avoid::AvoidMode::StepOff) {
                    // Retail simulates every ambient skater: one a prop blocks (mode 7) is simulated at any distance so
                    // it can step off and walk round (controller B); our distance switch is an engine cost choice.
                    Some("too far")
                } else if flags & (node_flags::AIRBORNE | node_flags::OFF_BOARD) != 0 || replay.cursor.current_trick() >= 0 {
                    Some("in the air, off board or in a trick")
                } else if replay.cursor.frame_in_segment != 0 {
                    // Retail starts an AI skater on a path node (node 0, or the chosen node on a respawn) with the
                    // spawn push; between nodes the push never applies and the sim dropped in from rest (b109:
                    // 21 of 44 bails within 1 s of the start).
                    Some("between nodes")
                } else {
                    None
                };
                if let Some(reason) = wait {
                    if report {
                        info!("NPC_SKATER_SIM #{} {} replay: {reason} (distance {distance:.1} m, radius {:.0} m)", npc.id.serial, npc.character, rules.radius);
                    }
                    continue;
                }
                match spawn_sim(&mut physics, &config, &graphs, &target, &mut pool) {
                    Ok(sim) => {
                        let why = if replay.avoid.last.mode == skate_core::living_world::avoid::AvoidMode::StepOff { ", blocked by a prop (mode 7)" } else { "" };
                        info!("NPC_SKATER_SIM #{} {} <- replay (distance {distance:.1} m{why})", npc.id.serial, npc.character);
                        commands.entity(e).insert(sim);
                        count += 1;
                    }
                    Err(error) => warn!("NPC_SKATER_SIM #{} {}: cannot simulate: {error}", npc.id.serial, npc.character),
                }
            }
        }
    }
}
