//! Simulated NPC skaters share `GamePhysics` through the per-skater context swap: the local
//! player's simulation is bit-identical with or without an NPC skater ticking in between.
use super::*;
use skate_core::physics::board::BodyId;

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
