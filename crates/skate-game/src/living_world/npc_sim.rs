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
}

impl Default for SimulatedTierSettings {
    fn default() -> Self {
        Self { enabled: std::env::var("SKATE_NPC_SIM").ok().as_deref() == Some("1"), radius: 40.0, max: 3 }
    }
}

/// A simulated NPC skater's own physics and runtime.
#[derive(Component)]
pub(crate) struct NpcSim {
    context: SkaterPhysicsContext,
    runtime: Box<SkaterRuntime>,
    controls: Box<PlayerControls>,
    camera: Box<crate::camera::CameraRuntime>,
}

impl NpcSim {
    pub(crate) fn render_pose(&self) -> &[skate_core::animation::output::NativeMatrix] {
        &self.runtime.render_pose
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
) -> Result<NpcSim, String> {
    let q = target.frame;
    let basis = skate_core::physics::rigid_body::basis_from_quaternion(skate_core::physics::rigid_body::RetailQuaternion { x: q[0], y: q[1], z: q[2], w: q[3] });
    let spawn = skate_core::physics::drive_frames::RetailAffineTransform {
        basis,
        translation: skate_core::math::Vector3::new(target.position[0], target.position[1], target.position[2]),
    };
    let mut context = physics.new_skater_context(spawn)?;
    let runtime = physics.load_skater_in_context(&mut context, &config.asset_root, graphs, config.difficulty.profile_key())?;
    // Engine handover (retail never hands over): start at the line's speed.
    GamePhysics::set_context_velocity(&mut context, target.step.map(|x| x * ai_record::FRAMES_PER_SECOND));
    Ok(NpcSim {
        context,
        runtime: Box::new(runtime),
        controls: Box::new(PlayerControls::load(&config.asset_root)?),
        camera: Box::new(crate::camera::CameraRuntime::load(&config.asset_root)?),
    })
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
    mut npcs: Query<(Entity, &NpcSkater, &NpcReplay, Option<&mut NpcSim>)>,
    mut calls: Local<u64>,
) {
    let (Some(config), Some(graphs), Some(mut physics)) = (config, graphs, physics) else { return };
    if physics.failed {
        return;
    }
    let rules = settings.npc_simulated;
    *calls += 1;
    // Every 2 s: why each replay-tier NPC is not simulated (logs must diagnose).
    let report = rules.enabled && *calls % 120 == 0;
    let lines = super::npc_skaters::npc_lines(&state);
    let mut count = npcs.iter().filter(|n| n.3.is_some()).count();
    let mut sorted: Vec<_> = npcs.iter_mut().collect();
    sorted.sort_by_key(|n| n.1.id);
    for (e, npc, replay, sim) in sorted {
        let Some(target) = replay.cursor.line_target(&*lines) else { continue };
        let distance = nearest(&observers, target.position);
        match sim {
            Some(mut sim) => {
                let state_now = sim.runtime.player_state.current();
                let settled = matches!(state_now, PhysicalStateId::PhysicsGround | PhysicalStateId::SlideGround);
                if (!rules.enabled || distance > rules.radius * 1.1 || replay.cursor.finished) && settled {
                    commands.entity(e).remove::<NpcSim>();
                    count -= 1;
                    info!("NPC_SKATER_SIM #{} {} -> replay (distance {distance:.1} m)", npc.id.serial, npc.character);
                    continue;
                }
                let sim = &mut *sim;
                let deck = GamePhysics::context_deck(&sim.context);
                let forward = [deck.basis.columns[2][0], deck.basis.columns[2][1], deck.basis.columns[2][2]];
                let record = ai_record::build(&target, forward, &Default::default());
                sim.runtime.ai_physics = Some(crate::physics::AiPhysicsSource { record, fresh: true });
                if let Some(line) = lines.get(&replay.cursor.line) {
                    let node = &line.nodes[replay.cursor.node as usize];
                    let at = [deck.translation.x, deck.translation.y, deck.translation.z];
                    if let Some(v) = ai_record::spawn_push(at, node.position, target.step, replay.cursor.node == 0, false) {
                        GamePhysics::set_context_velocity(&mut sim.context, v);
                    }
                }
                if let Err(error) = physics.advance_npc_skater(&mut sim.context, &mut sim.runtime, &mut sim.controls, &graphs, &mut sim.camera) {
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
                } else if distance > rules.radius {
                    Some("too far")
                } else if flags & (node_flags::AIRBORNE | node_flags::OFF_BOARD) != 0 || replay.cursor.current_trick() >= 0 {
                    Some("in the air, off board or in a trick")
                } else {
                    None
                };
                if let Some(reason) = wait {
                    if report {
                        info!("NPC_SKATER_SIM #{} {} replay: {reason} (distance {distance:.1} m, radius {:.0} m)", npc.id.serial, npc.character, rules.radius);
                    }
                    continue;
                }
                match spawn_sim(&mut physics, &config, &graphs, &target) {
                    Ok(sim) => {
                        info!("NPC_SKATER_SIM #{} {} <- replay (distance {distance:.1} m)", npc.id.serial, npc.character);
                        commands.entity(e).insert(sim);
                        count += 1;
                    }
                    Err(error) => warn!("NPC_SKATER_SIM #{} {}: cannot simulate: {error}", npc.id.serial, npc.character),
                }
            }
        }
    }
}
