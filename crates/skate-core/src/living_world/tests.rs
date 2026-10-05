//! Population core tests against the retail code constants (labels and addresses in
//! `config::retail`). The census ranges and caps used here are the shipped data values, set up
//! by hand as test fixtures (the data-gated tests in `skate-data` read them from the export).

use super::census::{cap_at, pick_category, ring_point, CensusCategory};
use super::config::retail;
use super::skaters::{near_skater_penalty, SkaterData};
use super::*;
use std::collections::BTreeMap;

const PEDS: CensusRange = CensusRange {
    slow: CensusCircle { spawn_inner: 50.0, spawn_outer: 60.0, cull: 70.0, forward_offset: 0.0, speed_kmh: 45.0 },
    fast: CensusCircle { spawn_inner: 50.0, spawn_outer: 80.0, cull: 90.0, forward_offset: 20.0, speed_kmh: 80.0 },
};
const VEHICLES: CensusRange = CensusRange {
    slow: CensusCircle { spawn_inner: 80.0, spawn_outer: 100.0, cull: 110.0, forward_offset: 0.0, speed_kmh: 0.0 },
    fast: CensusCircle { spawn_inner: 80.0, spawn_outer: 100.0, cull: 110.0, forward_offset: 0.0, speed_kmh: 0.0 },
};

fn record(max: u32, cats: &[(&str, f32)]) -> CensusRecord {
    CensusRecord {
        max_population: max,
        categories: cats.iter().map(|(n, w)| CensusCategory { name: n.to_string(), weight: *w }).collect(),
    }
}

/// 2 km x 2 km grid centred on the origin, 4 m cells. Peds: `aletown` (15) everywhere except the
/// strip x > 600 (unpainted); vehicles: `dwntwn` (30) everywhere.
fn map() -> CensusMap {
    let (w, h) = (500u32, 500u32);
    let mut peds = vec![1u16; (w * h) as usize];
    for j in 0..h {
        for i in 0..w {
            if -1000.0 + i as f32 * 4.0 >= 600.0 {
                peds[(j * w + i) as usize] = 0;
            }
        }
    }
    let mut layers = BTreeMap::new();
    layers.insert("livingworld_npc_census".to_string(), peds);
    layers.insert("livingworld_vehicle_census".to_string(), vec![2u16; (w * h) as usize]);
    let grid = CensusGrid { cell: 4.0, origin: [-1000.0, -1000.0], width: w, height: h, names: vec!["aletown".into(), "dwntwn".into()], layers };
    let mut records = BTreeMap::new();
    records.insert("aletown".into(), record(15, &[("adult", 0.2), ("jock", 0.1), ("tourist", 0.1), ("skater", 0.1), ("teen", 0.05), ("business", 0.3), ("bum", 0.15)]));
    records.insert("dwntwn".into(), record(30, &[("hatchbacks", 0.1), ("minivans", 0.1), ("muscles", 0.075), ("sedans", 0.2), ("sports", 0.175), ("suvs", 0.125), ("taxis", 0.1)]));
    CensusMap { grids: vec![grid], records }
}

fn config() -> PopulationConfig {
    let mut c = PopulationConfig::retail();
    c.pedestrians.range = Some(PEDS);
    c.vehicles.range = Some(VEHICLES);
    c
}

fn still(x: f32, z: f32) -> Observer {
    Observer { position: [x, 0.0, z], velocity: [0.0; 3] }
}

fn spawns(d: &[Decision], kind: Kind) -> Vec<&SpawnRecord> {
    d.iter().filter_map(|d| match d { Decision::Spawn(s) if s.id.kind == kind => Some(s), _ => None }).collect()
}
fn despawns(d: &[Decision], kind: Kind) -> Vec<&DespawnRecord> {
    d.iter().filter_map(|d| match d { Decision::Despawn(s) if s.id.kind == kind => Some(s), _ => None }).collect()
}
fn h(a: Vec3, b: Vec3) -> f32 {
    dist2(a, b).sqrt()
}

/// Skater lines on circles around the origin, every 10 degrees at radii 40, 75, 110 m.
fn skater_data() -> SkaterData {
    let mut lines = Vec::new();
    for (r_i, r) in [40.0f32, 75.0, 110.0].into_iter().enumerate() {
        for a in 0..36 {
            let t = a as f32 * std::f32::consts::TAU / 36.0;
            let mut id = [0u8; 16];
            id[0] = r_i as u8;
            id[1] = a as u8;
            lines.push(SkaterLine { id, start: [r * t.cos(), 0.0, r * t.sin()], heading: t, valid: true, allowed_skaters: u64::MAX, flags: 0 });
        }
    }
    let characters = (0..8)
        .map(|i| SkaterCharacter { key: format!("pro_{i}"), pro_index: Some(i), capabilities: [false; 3], community: false })
        .collect();
    SkaterData { lines, characters }
}

// ---------------------------------------------------------------- census circle

#[test]
fn ped_circle_lerps_by_speed_like_sub_826b7d60() {
    let slow = PEDS.at(30.0);
    assert_eq!((slow.spawn_inner, slow.spawn_outer, slow.cull, slow.forward_offset), (50.0, 60.0, 70.0, 0.0));
    let fast = PEDS.at(100.0);
    assert_eq!((fast.spawn_inner, fast.spawn_outer, fast.cull, fast.forward_offset), (50.0, 80.0, 90.0, 20.0));
    let mid = PEDS.at(62.5);
    assert_eq!((mid.spawn_outer, mid.cull, mid.forward_offset), (70.0, 80.0, 10.0));
    // Vehicles: equal sets (A.key >= B.key) → set A as is.
    let v = VEHICLES.at(120.0);
    assert_eq!((v.spawn_inner, v.spawn_outer, v.cull), (80.0, 100.0, 110.0));
    // Speed key from |velocity| x 3.6: 22.2 m/s = 80 km/h; the centre moves 20 m forward.
    let (c, centre) = PEDS.around(&Observer { position: [0.0; 3], velocity: [0.0, 0.0, 80.0 / 3.6] });
    assert!((c.spawn_outer - 80.0).abs() < 1e-3);
    assert!((centre[2] - 20.0).abs() < 1e-3 && centre[0].abs() < 1e-6);
}

#[test]
fn ring_points_stay_in_the_ring() {
    let mut rng = Rng::new(3);
    for _ in 0..5000 {
        let p = ring_point(&mut rng, [10.0, 2.0, -5.0], 50.0, 60.0);
        let d = h(p, [10.0, 2.0, -5.0]);
        assert!((50.0 - 1e-3..=60.0 + 1e-3).contains(&d), "{d}");
        assert_eq!(p[1], 2.0);
    }
}

#[test]
fn cap_is_record_max_times_density_truncated_and_zero_when_unpainted() {
    let r = record(30, &[("a", 1.0)]);
    assert_eq!(cap_at(Some(&r), 1.0, false), 30);
    assert_eq!(cap_at(Some(&r), 0.55, false), 16); // 16.5 truncated
    assert_eq!(cap_at(Some(&r), 0.0, true), 30); // zombie: no density scaling
    assert_eq!(cap_at(None, 1.0, true), 0); // unpainted: no record, cap 0 (sub_826B8A28)
}

#[test]
fn category_roll_misses_when_weights_sum_below_one() {
    let r = record(10, &[("only", 0.5)]);
    let mut rng = Rng::new(9);
    let hits = (0..10_000).filter(|_| pick_category(&mut rng, &r).is_some()).count();
    assert!((4_700..5_300).contains(&hits), "{hits}");
    let full = record(10, &[("a", 0.25), ("b", 0.75)]);
    assert!((0..1000).all(|_| pick_category(&mut rng, &full).is_some()));
}

// ---------------------------------------------------------------- census passes

#[test]
fn peds_initial_populate_then_one_spawn_per_pass_in_the_ring_up_to_the_cap() {
    let map = map();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    let mut world = LivingWorld::new(config(), 42);
    // Tick 0 is the peds slot: initial populate (8-80 m), up to the cap of 15.
    let d = world.step(&inputs);
    let first = spawns(&d, Kind::Pedestrian);
    assert_eq!(first.len(), 15);
    assert!(first.iter().all(|s| s.initial && (8.0 - 1e-3..=80.0 + 1e-3).contains(&h(s.position, obs[0].position))));
    assert!(first.iter().all(|s| (0.0..std::f32::consts::TAU + 1e-4).contains(&s.heading)));
    // Remove the far ones (beyond 70 m) the way the cull would, then the regular passes refill
    // one per pass, only on ped ticks (tick % 4 == 0), always 50-60 m away.
    for _ in 1..2000 {
        let t = world.tick();
        let d = world.step(&inputs);
        let s = spawns(&d, Kind::Pedestrian);
        assert!(s.len() <= retail::SPAWNS_PER_PASS as usize);
        if !s.is_empty() {
            assert_eq!(t % 4, 0, "ped spawns run in rotation slot 0");
            let r = h(s[0].position, obs[0].position);
            assert!((50.0 - 1e-3..=60.0 + 1e-3).contains(&r), "spawn at {r}");
            assert!(!s[0].initial);
        }
        assert!(world.count(Kind::Pedestrian) <= 15);
        for x in despawns(&d, Kind::Pedestrian) {
            assert_eq!(x.reason, DespawnReason::Distance);
        }
    }
}

#[test]
fn census_cull_at_the_cull_radius() {
    let map = map();
    let mut world = LivingWorld::new(config(), 1);
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    world.step(&inputs);
    let ids: Vec<_> = world.live(Kind::Pedestrian).map(|l| l.id).take(2).collect();
    world.update_position(ids[0], [69.9, 0.0, 0.0]);
    world.update_position(ids[1], [70.1, 0.0, 0.0]);
    let others: Vec<_> = world.live(Kind::Pedestrian).map(|l| l.id).skip(2).collect();
    for id in others {
        world.update_position(id, [0.0, 0.0, 10.0]);
    }
    for _ in 0..3 {
        world.step(&inputs); // vehicles, DMOs, props slots
    }
    let d = world.step(&inputs); // next ped pass: cull first
    let gone = despawns(&d, Kind::Pedestrian);
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].id, ids[1]);
    // Height counts too (3-D distance): 70.1 m straight up goes.
    world.update_position(ids[0], [0.0, 70.1, 0.0]);
    for _ in 0..3 {
        world.step(&inputs);
    }
    assert!(despawns(&world.step(&inputs), Kind::Pedestrian).iter().any(|x| x.id == ids[0]));
}

#[test]
fn unpainted_ground_spawns_no_peds() {
    let map = map();
    let obs = [still(800.0, 0.0)]; // the ring 8-80 m stays in x > 600: unpainted for peds
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    let mut world = LivingWorld::new(config(), 5);
    for _ in 0..400 {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Pedestrian), 0);
    assert!(world.count(Kind::Vehicle) > 0, "the vehicle layer is painted there");
}

#[test]
fn vehicles_ring_80_100_cull_110_cap_30_rotation_slot_1() {
    let map = map();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    let mut world = LivingWorld::new(config(), 77);
    let mut regular = 0;
    for _ in 0..4000 {
        let t = world.tick();
        let d = world.step(&inputs);
        for s in spawns(&d, Kind::Vehicle) {
            assert_eq!(t % 4, 1);
            let r = h(s.position, obs[0].position);
            if s.initial {
                assert!((8.0 - 1e-3..=80.0 + 1e-3).contains(&r));
            } else {
                regular += 1;
                assert!((80.0 - 1e-3..=100.0 + 1e-3).contains(&r), "{r}");
            }
        }
        assert!(world.count(Kind::Vehicle) <= 30);
    }
    assert_eq!(world.count(Kind::Vehicle), 30);
    assert_eq!(regular, 0, "nothing culls a still observer's cars, so the cap stays full");
    let id = world.live(Kind::Vehicle).next().unwrap().id;
    world.update_position(id, [0.0, 0.0, 110.5]);
    while world.tick() % 4 != 1 {
        world.step(&inputs);
    }
    assert_eq!(despawns(&world.step(&inputs), Kind::Vehicle)[0].id, id);
}

#[test]
fn free_play_scales_caps_and_zero_removes_all_at_once() {
    let map = map();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    inputs.free_play = Some(FreePlay { traffic: 0.5, pedestrians: 1.0, ai_skaters: true });
    let mut world = LivingWorld::new(config(), 8);
    for _ in 0..400 {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Vehicle), 15); // trunc(30 x 0.5)
    assert_eq!(world.count(Kind::Pedestrian), 15);
    inputs.free_play = Some(FreePlay { traffic: 0.0, pedestrians: 0.0, ai_skaters: true });
    let mut gone = Vec::new();
    for _ in 0..4 {
        gone.extend(world.step(&inputs));
    }
    assert_eq!(despawns(&gone, Kind::Pedestrian).len(), 15);
    assert_eq!(despawns(&gone, Kind::Vehicle).len(), 15);
    assert!(despawns(&gone, Kind::Vehicle).iter().all(|d| d.reason == DespawnReason::FreePlayOff));
    for _ in 0..200 {
        assert!(spawns(&world.step(&inputs), Kind::Pedestrian).is_empty());
    }
    // Outside Free Play the option values do nothing (scale 1.0).
    inputs.free_play = None;
    for _ in 0..400 {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Vehicle), 30);
}

#[test]
fn online_spawns_nothing_but_still_culls() {
    let map = map();
    let mut world = LivingWorld::new(config(), 11);
    let data = skater_data();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    inputs.skater_world = Some(&data);
    for _ in 0..240 {
        world.step(&inputs);
    }
    assert!(world.count(Kind::Pedestrian) > 0 && world.count(Kind::Vehicle) > 0 && world.count(Kind::Skater) > 0);
    inputs.online = true;
    let far = [still(5000.0, 5000.0)];
    let mut online = inputs;
    online.observers = &far;
    let mut all = Vec::new();
    for _ in 0..240 {
        all.extend(world.step(&online));
    }
    assert!(all.iter().all(|d| matches!(d, Decision::Despawn(_))), "online: no spawns of any kind");
    assert_eq!(world.count(Kind::Pedestrian), 0);
    assert_eq!(world.count(Kind::Vehicle), 0);
    assert_eq!(world.count(Kind::Skater), 0);
}

#[test]
fn zombie_mode_peds_ignore_the_cap_no_traffic_no_skaters() {
    let map = map();
    let data = skater_data();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    inputs.skater_world = Some(&data);
    inputs.zombie = true;
    let mut world = LivingWorld::new(config(), 13);
    for _ in 0..400 {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Pedestrian), 31, "no census cap in zombie mode; the pool of 31 limits");
    assert_eq!(world.count(Kind::Vehicle), 0);
    assert_eq!(world.count(Kind::Skater), 0);
}

#[test]
fn disabled_kind_despawns_and_density_setting_scales() {
    let map = map();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    let mut cfg = config();
    cfg.vehicles.density = 0.2;
    let mut world = LivingWorld::new(cfg, 2);
    for _ in 0..400 {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Vehicle), 6);
    world.config.pedestrians.enabled = false;
    let d: Vec<_> = (0..4).flat_map(|_| world.step(&inputs)).collect();
    assert!(despawns(&d, Kind::Pedestrian).iter().all(|x| x.reason == DespawnReason::Disabled));
    assert_eq!(world.count(Kind::Pedestrian), 0);
}

#[test]
fn no_export_means_no_census_population() {
    let map = map();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    let mut world = LivingWorld::new(PopulationConfig::retail(), 2);
    for _ in 0..100 {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Pedestrian) + world.count(Kind::Vehicle), 0);
}

// ---------------------------------------------------------------- skaters

#[test]
fn skaters_three_ambient_spawn_on_phase_30_at_60_to_90_m() {
    let data = skater_data();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.skater_world = Some(&data);
    let mut world = LivingWorld::new(config(), 21);
    for _ in 0..(60 * 20) {
        let t = world.tick();
        let d = world.step(&inputs);
        let s = spawns(&d, Kind::Skater);
        assert!(s.len() <= 1);
        if let Some(s) = s.first() {
            assert_eq!(t % 60, 30);
            let r = h(s.position, obs[0].position);
            assert!((60.0 - 1e-3..=90.0 + 1e-3).contains(&r), "{r}");
            let SpawnChoice::Skater { slot, .. } = &s.choice else { panic!() };
            assert!((1..7).contains(slot));
        }
        assert!(world.count(Kind::Skater) <= 3);
    }
    assert_eq!(world.count(Kind::Skater), 3);
    assert_eq!(world.skater_pool().len(), 5);
}

#[test]
fn skaters_never_spawn_within_5_m_of_another_skater() {
    // Two lines only: one 3 m from a standing player 75 m out, one free.
    let mut data = skater_data();
    data.lines.truncate(0);
    let mk = |b: u8, p: Vec3| SkaterLine { id: [b; 16], start: p, heading: 0.0, valid: true, allowed_skaters: u64::MAX, flags: 0 };
    data.lines.push(mk(1, [75.0, 0.0, 0.0]));
    data.lines.push(mk(2, [0.0, 0.0, 75.0]));
    let obs = [still(0.0, 0.0), still(77.0, 2.0)]; // a second skater (remote player later) near line 1
    let mut inputs = TickInputs::offline(&obs);
    inputs.skater_world = Some(&data);
    inputs.player_slots = 2;
    let mut world = LivingWorld::new(config(), 4);
    let mut lines = Vec::new();
    for _ in 0..(60 * 10) {
        for s in spawns(&world.step(&inputs), Kind::Skater) {
            let SpawnChoice::Skater { line, slot, .. } = &s.choice else { panic!() };
            lines.push(line[0]);
            assert!(*slot >= 2, "slots 0 and 1 belong to the players");
        }
    }
    assert_eq!(lines, vec![2], "line 1 starts 2.8 m from a skater: rejected (d^2 < 25)");
}

#[test]
fn skater_near_term_and_cull() {
    let cfg = SkaterConfig::retail();
    assert_eq!(near_skater_penalty(100.0, &cfg), 0); // 10 m: no term
    assert_eq!(near_skater_penalty(0.1, &cfg), 600);
    assert_eq!(near_skater_penalty(64.0, &cfg), ((10.0f32 - 8.0) * retail::SKATER_NEAR_K * 600.0) as i64);
    let data = skater_data();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.skater_world = Some(&data);
    let mut world = LivingWorld::new(config(), 6);
    while world.count(Kind::Skater) < 1 {
        world.step(&inputs);
    }
    let id = world.live(Kind::Skater).next().unwrap().id;
    world.update_position(id, [119.0, 0.0, 0.0]);
    let mut d = Vec::new();
    for _ in 0..60 {
        d.extend(world.step(&inputs));
    }
    assert!(despawns(&d, Kind::Skater).is_empty());
    world.update_position(id, [121.0, 0.0, 0.0]);
    let mut d = Vec::new();
    for _ in 0..60 {
        d.extend(world.step(&inputs));
    }
    let gone = despawns(&d, Kind::Skater);
    assert_eq!(gone.len(), 1);
    assert_eq!((gone[0].id, gone[0].reason), (id, DespawnReason::Distance));
    assert_eq!(gone[0].tick % 60, 0, "the cull runs on phase 0");
}

#[test]
fn skaters_free_play_off_and_online_excess() {
    let data = skater_data();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.skater_world = Some(&data);
    let mut world = LivingWorld::new(config(), 31);
    for _ in 0..(60 * 10) {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Skater), 3);
    inputs.free_play = Some(FreePlay { ai_skaters: false, ..FreePlay::default() });
    while world.tick() % 60 != 0 {
        world.step(&inputs);
    }
    world.step(&inputs); // phase 0, 1, 2: no check yet
    world.step(&inputs);
    world.step(&inputs);
    assert_eq!(world.count(Kind::Skater), 3);
    let d = world.step(&inputs); // phase 3: per-skater checks despawn all
    assert_eq!(despawns(&d, Kind::Skater).len(), 3);
    for _ in 0..600 {
        assert!(spawns(&world.step(&inputs), Kind::Skater).is_empty());
    }
    // Online: desired 0, the phase-0 cull removes the excess even nearby.
    inputs.free_play = None;
    for _ in 0..600 {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Skater), 3);
    inputs.online = true;
    let mut d = Vec::new();
    for _ in 0..60 {
        d.extend(world.step(&inputs));
    }
    assert!(despawns(&d, Kind::Skater).iter().all(|x| x.reason == DespawnReason::Excess));
    assert_eq!(world.count(Kind::Skater), 0);
}

#[test]
fn skater_slots_are_shared_with_players() {
    let data = skater_data();
    let obs = [still(0.0, 0.0)];
    let mut inputs = TickInputs::offline(&obs);
    inputs.skater_world = Some(&data);
    inputs.player_slots = 5; // 5 players: slots 5 and 6 are left
    let mut world = LivingWorld::new(config(), 3);
    for _ in 0..(60 * 10) {
        world.step(&inputs);
    }
    assert_eq!(world.count(Kind::Skater), 2);
}

// ---------------------------------------------------------------- determinism and ids

fn run(seed: u64) -> Vec<Decision> {
    run_world(seed).1
}

fn run_world(seed: u64) -> (LivingWorld, Vec<Decision>) {
    let map = map();
    let data = skater_data();
    let mut world = LivingWorld::new(config(), seed);
    let mut out = Vec::new();
    for step in 0..3000u32 {
        // A player moving in a circle of 150 m at 8 m/s.
        let t = step as f32 / 30.0;
        let a = t * 8.0 / 150.0;
        let obs = [Observer { position: [150.0 * a.cos(), 0.0, 150.0 * a.sin()], velocity: [-8.0 * a.sin(), 0.0, 8.0 * a.cos()] }];
        let mut inputs = TickInputs::offline(&obs);
        inputs.census = Some(&map);
        inputs.skater_world = Some(&data);
        out.extend(world.step(&inputs));
    }
    (world, out)
}

#[test]
fn same_inputs_same_decision_stream() {
    let a = run(1234);
    let b = run(1234);
    assert!(a.len() > 50);
    assert_eq!(a, b);
    assert_ne!(a, run(1235));
}

#[test]
fn ids_are_stable_and_unique_and_a_client_can_mirror_the_records() {
    let (host, a) = run_world(99);
    let mut seen = std::collections::BTreeSet::new();
    let mut client = LivingWorld::new(config(), 0);
    for d in &a {
        if let Decision::Spawn(s) = d {
            assert!(seen.insert(s.id), "id reused: {:?}", s.id);
            assert_eq!(LivingWorldId::from_u64(s.id.to_u64()), Some(s.id));
        }
        client.apply(d);
    }
    // The client's roster equals the host's after replaying the records (no rules, no RNG).
    for k in Kind::ALL {
        let x: Vec<_> = client.live(k).map(|l| l.id).collect();
        let y: Vec<_> = host.live(k).map(|l| l.id).collect();
        assert_eq!(x, y);
    }
}
