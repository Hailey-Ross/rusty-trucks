//! Simulated NPC skaters share `GamePhysics` through the per-skater context swap: the local
//! player's simulation is bit-identical with or without an NPC skater ticking in between.
use super::*;
use skate_core::physics::board::BodyId;
use skate_core::player::state::PhysicalStateId;

fn pad() -> skate_core::input::xbox::XboxState {
    skate_core::input::xbox::XboxState::default()
}

struct Rig {
    graphs: crate::graph_runtime::StockGraphs,
    root_dir: std::path::PathBuf,
}

struct Skater {
    runtime: SkaterRuntime,
    camera: crate::camera::CameraRuntime,
    controls: PlayerControls,
    input: crate::input::ControllerInput,
}

impl Rig {
    fn skater(&self, physics: &GamePhysics) -> Skater {
        Skater {
            runtime: SkaterRuntime::load(&self.root_dir, &self.graphs, physics, "easy").unwrap(),
            camera: crate::camera::CameraRuntime::load(&self.root_dir).unwrap(),
            controls: PlayerControls::default(),
            input: crate::input::ControllerInput::default(),
        }
    }
    fn step(&self, physics: &mut GamePhysics, s: &mut Skater, state: skate_core::input::xbox::XboxState) {
        s.input.sample_raw_for_test(state);
        let mut actions = s.input.player_actions();
        s.controls.update_for_physics(&mut actions, physics, &mut s.runtime, &mut s.camera).unwrap();
        frame::advance(physics, &mut s.runtime, &mut s.controls, &self.graphs, &mut actions, true, &mut s.camera).unwrap();
    }
}

/// Deck position and velocity bits per tick.
fn trace(physics: &GamePhysics) -> [u32; 6] {
    let deck = &physics.board.bodies()[BodyId::Deck.index()].rates;
    let (p, v) = (deck.position, deck.linear_velocity);
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

#[test]
#[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn the_player_is_bit_identical_with_a_simulated_npc_skater_in_the_same_world() {
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let load = || GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    // Push forward (A held) for a second, then roll.
    let input = |t: u32| skate_core::input::xbox::XboxState { buttons: if t < 60 { 0x1000 } else { 0 }, ..pad() };

    let mut alone = load();
    let mut player = rig.skater(&alone);
    let mut expected = Vec::new();
    for t in 0..300 {
        rig.step(&mut alone, &mut player, input(t));
        expected.push(trace(&alone));
    }

    let mut shared = load();
    let mut player = rig.skater(&shared);
    // The NPC skater: its own board 6 m to the player's side, its own runtime.
    let mut spawn = shared.board.part_transforms()[BodyId::Deck.index()];
    spawn.translation.x += 6.0;
    let mut context = shared.new_skater_context(spawn).unwrap();
    shared.swap_skater_context(&mut context);
    let mut npc = rig.skater(&shared);
    shared.swap_skater_context(&mut context);
    let start = context.board.part_transforms()[BodyId::Deck.index()].translation;
    for t in 0..300 {
        rig.step(&mut shared, &mut player, input(t));
        assert_eq!(trace(&shared), expected[t as usize], "player diverged at tick {t}");
        shared.swap_skater_context(&mut context);
        rig.step(&mut shared, &mut npc, input(t));
        shared.swap_skater_context(&mut context);
    }
    // The NPC skater simulated on its own board: it moved and did not fall through the world.
    let end = context.board.part_transforms()[BodyId::Deck.index()].translation;
    let moved = ((end.x - start.x).powi(2) + (end.z - start.z).powi(2)).sqrt();
    assert!(moved > 1.0 && (end.y - start.y).abs() < 2.0, "npc board {start:?} -> {end:?}");
    assert!(!context.failed && context.owns_props == false && shared.owns_props);
    assert_eq!(context.ticks, 300);
}

/// Steps a + b of the simulated tier: a skater with only the AI record (no pad) rides a recorded
/// DownTown NPC line on its own physics, steered by the ground states' board path.
#[test]
#[ignore = "requires private stock graphs, the living-world export and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn a_simulated_skater_rides_a_recorded_line_from_its_ai_record() {
    use skate_core::living_world::replay::{path_frame, Decider, LineCursor, ReplayLine};
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let pack = std::fs::read(root_dir.join("private/living_world/skater_paths/DownTown.bin")).unwrap();
    let tiles = skate_data::aipath::parse_pack(&pack).unwrap();
    let (paths, _) = skate_data::aipath::district_paths(&tiles).unwrap();
    let lines: std::collections::BTreeMap<[u8; 16], ReplayLine> =
        paths.iter().filter(|p| p.path.id.is_ambient()).map(|p| (p.path.id.0, skate_data::living_world::replay_line(&p.path))).collect();
    let which = std::env::var("LINE_INDEX").ok().and_then(|v| v.parse().ok()).unwrap_or(0usize);
    // A line rolling on the ground for its first 300 frames.
    let line = lines.values().filter(|l| l.duration_frames() > 600 && l.nodes.iter().take(40).all(|n| n.flags & 0x0e == 0)).nth(which).expect("a ground line");
    let node = &line.nodes[0];
    let q = path_frame(node);
    let basis = skate_core::physics::rigid_body::basis_from_quaternion(skate_core::physics::rigid_body::RetailQuaternion { x: q[0], y: q[1], z: q[2], w: q[3] });
    let spawn = RetailAffineTransform { basis, translation: skate_core::math::Vector3::new(node.position[0], node.position[1], node.position[2]) };
    let mut context = physics.new_skater_context(spawn).unwrap();
    physics.swap_skater_context(&mut context);
    let mut npc = rig.skater(&physics);
    let mut cursor = LineCursor::spawn(&lines, line.id, 0);
    let mut errors = Vec::new();
    let start = physics.board.part_transforms()[BodyId::Deck.index()].translation;
    for tick in 0..300 {
        let target = cursor.line_target(&lines).unwrap();
        let deck = physics.board.part_transforms()[BodyId::Deck.index()];
        let forward = [deck.basis.columns[2][0], deck.basis.columns[2][1], deck.basis.columns[2][2]];
        let record = skate_core::living_world::ai_record::build(&target, forward, &Default::default());
        npc.runtime.ai_physics = Some(super::skater::AiPhysicsSource { record, fresh: true });
        // Retail spawn push while still on the start node (824701F8 -> 82C04168, every part).
        let node = &line.nodes[cursor.node as usize];
        let deck_at = [deck.translation.x, deck.translation.y, deck.translation.z];
        if let Some(v) = skate_core::living_world::ai_record::spawn_push(deck_at, node.position, target.step, cursor.node == 0, false) {
            for body in physics.board.bodies_mut() {
                body.rates.linear_velocity = skate_core::math::Vector3::new(v[0], v[1], v[2]);
            }
        }
        rig.step(&mut physics, &mut npc, pad());
        assert_eq!(npc.runtime.player_state.current(), PhysicalStateId::PhysicsGround, "on-board steering (bit 25) keeps the skater in ground physics, tick {tick}");
        cursor.step(&lines, &mut Decider::Stay, &mut Vec::new());
        let deck = physics.board.part_transforms()[BodyId::Deck.index()].translation;
        let e = ((deck.x - target.position[0]).powi(2) + (deck.z - target.position[2]).powi(2)).sqrt();
        errors.push(e);
        if tick % 30 == 0 {
            let v = physics.board.bodies()[BodyId::Deck.index()].rates.linear_velocity;
            let speed = (v.x * v.x + v.z * v.z).sqrt();
            let want = (target.step[0].powi(2) + target.step[2].powi(2)).sqrt() * 60.0;
            eprintln!("tick {tick} state {:?} speed {speed:.2} target speed {want:.2} error {e:.2}", npc.runtime.player_state.current());
        }
    }
    let end = physics.board.part_transforms()[BodyId::Deck.index()].translation;
    physics.swap_skater_context(&mut context);
    let mut sorted = errors.clone();
    sorted.sort_by(f32::total_cmp);
    eprintln!("error median {:.3} p90 {:.3} max {:.3}", sorted[150], sorted[270], sorted[299]);
    // With the retail spawn push it holds the recorded line: on DownTown lines 0..6 the median
    // error is 0.06..0.2 m (max under 0.6 m) except line 5, where the board loses speed on ground
    // geometry the recording rolls through (open, doc 26).
    if std::env::var_os("LINE_INDEX").is_none() {
        assert!(sorted[299] < 1.0 && sorted[150] < 0.5, "tracking: median {} max {}", sorted[150], sorted[299]);
    }
    let moved = ((end.x - start.x).powi(2) + (end.z - start.z).powi(2)).sqrt();
    assert!(moved > 5.0 && (end.y - start.y).abs() < 1.0, "{start:?} -> {end:?}");
}

/// A ped takedown on the player (`82592390`: actor `1904` bit 30 + direction) is published as the
/// packet's external impulse; retail's motion graph enters WipeOut on `IsPhysicsWiping` (b15).
/// Prints whether our skater reaches WipeoutGround (retail's switch is not proven statically).
#[test]
#[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn a_ped_takedown_publishes_the_external_impulse_and_wipes_the_skater_out() {
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let mut player = rig.skater(&physics);
    let input = |t: u32| skate_core::input::xbox::XboxState { buttons: if t < 60 { 0x1000 } else { 0 }, ..pad() };
    for t in 0..120 {
        rig.step(&mut physics, &mut player, input(t));
    }
    assert_eq!(player.runtime.player_state.current(), PhysicalStateId::PhysicsGround);
    // The chaser hits from behind: 1 m against the board's motion.
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates;
    let v = deck.linear_velocity;
    let speed = (v.x * v.x + v.z * v.z).sqrt().max(1e-3);
    let at = [deck.position.x, deck.position.y, deck.position.z];
    let chaser = [at[0] - v.x / speed, at[1], at[2] - v.z / speed];
    player.runtime.takedown = Some(super::skater::Takedown::from_positions(at, chaser));
    let mut states = Vec::new();
    for _ in 0..90 {
        rig.step(&mut physics, &mut player, pad());
        states.push(player.runtime.player_state.current());
    }
    eprintln!("takedown states: {:?}", states.iter().map(|s| *s as u32).collect::<Vec<_>>());
    assert!(states.contains(&PhysicalStateId::WipeoutGround), "no wipeout within 3 s of the takedown");
    assert!(player.runtime.takedown.is_none(), "the latch is used by WipeoutGround Enter");
}
