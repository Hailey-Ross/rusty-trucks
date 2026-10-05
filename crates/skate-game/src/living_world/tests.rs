//! Headless, seeded tests of the living-world plugin: a fake player drives along a path, the
//! population systems run in a Bevy app with manual time, and the published messages are checked.

use super::*;
use skate_core::living_world::census::CensusCategory;
use skate_core::living_world::{CensusCircle, CensusGrid, CensusRange, CensusRecord, SkaterCharacter, SkaterLine};
use std::collections::BTreeMap;

fn census() -> CensusMap {
    let (w, h) = (600u32, 600u32);
    let mut layers = BTreeMap::new();
    layers.insert("livingworld_npc_census".to_string(), vec![1u16; (w * h) as usize]);
    layers.insert("livingworld_vehicle_census".to_string(), vec![2u16; (w * h) as usize]);
    let grid = CensusGrid { cell: 4.0, origin: [-1200.0, -1200.0], width: w, height: h, names: vec!["aletown".into(), "dwntwn".into()], layers };
    let rec = |max, cats: &[(&str, f32)]| CensusRecord { max_population: max, categories: cats.iter().map(|(n, w)| CensusCategory { name: n.to_string(), weight: *w }).collect() };
    let mut records = BTreeMap::new();
    records.insert("aletown".into(), rec(15, &[("adult", 0.5), ("teen", 0.5)]));
    records.insert("dwntwn".into(), rec(30, &[("sedans", 0.5), ("taxis", 0.5)]));
    CensusMap { grids: vec![grid], records }
}

fn data() -> LoadedData {
    let mut config = PopulationConfig::retail();
    let c = |i, o, cull, f, s| CensusCircle { spawn_inner: i, spawn_outer: o, cull, forward_offset: f, speed_kmh: s };
    config.pedestrians.range = Some(CensusRange { slow: c(50.0, 60.0, 70.0, 0.0, 45.0), fast: c(50.0, 80.0, 90.0, 20.0, 80.0) });
    config.vehicles.range = Some(CensusRange { slow: c(80.0, 100.0, 110.0, 0.0, 0.0), fast: c(80.0, 100.0, 110.0, 0.0, 0.0) });
    // Lines every 25 m along the path's side streets.
    let lines = (0..80)
        .map(|i| {
            let mut id = [0u8; 16];
            id[0] = i as u8;
            SkaterLine { id, start: [-1000.0 + i as f32 * 25.0, 0.0, 70.0], heading: 0.0, valid: true, allowed_skaters: u64::MAX, flags: 0 }
        })
        .collect();
    let characters = (0..6).map(|i| SkaterCharacter { key: format!("pro_{i}"), pro_index: Some(i), capabilities: [false; 3], community: false }).collect();
    LoadedData { config, census: Some(census()), skaters: Some(SkaterData { lines, characters }), npc: Default::default(), status: "test".into() }
}

#[derive(Resource, Default)]
struct Collected(Vec<WireRecord>);

#[derive(Resource)]
struct Path {
    t: f32,
    speed: f32,
}

/// The fake player: along +x at `speed` m/s from x = -900.
fn drive(time: Res<Time>, mut path: ResMut<Path>, mut obs: ResMut<LivingWorldObservers>) {
    path.t += time.delta_secs();
    let x = -900.0 + path.t * path.speed;
    obs.observers = vec![Observer { position: [x, 0.0, 0.0], velocity: [path.speed, 0.0, 0.0] }];
    obs.player_slots = 1;
}

fn collect(mut s: MessageReader<LivingWorldSpawn>, mut d: MessageReader<LivingWorldDespawn>, mut out: ResMut<Collected>) {
    for m in s.read() {
        out.0.push(WireRecord::from_decision(&Decision::Spawn(m.0.clone())));
    }
    for m in d.read() {
        out.0.push(WireRecord::from_decision(&Decision::Despawn(m.0.clone())));
    }
}

fn app(seed: u64, speed: f32) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    state.install("Test", 1, &settings, data());
    app.insert_resource(settings).insert_resource(state).init_resource::<LivingWorldObservers>().init_resource::<Collected>();
    app.insert_resource(Path { t: 0.0, speed });
    app.add_message::<LivingWorldSpawn>().add_message::<LivingWorldDespawn>();
    app.add_systems(Update, (drive, step_population, collect).chain());
    app
}

/// Run `seconds` of game time at an engine rate of `hz`.
fn run(app: &mut App, seconds: f32, hz: f32) {
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / hz)));
    app.update(); // the first update has no delta
    for _ in 0..(seconds * hz) as u32 {
        app.update();
    }
}

fn spawned(c: &Collected) -> Vec<(&str, [f32; 3], bool)> {
    c.0.iter().filter_map(|r| match r { WireRecord::Spawn { kind, position, initial, .. } => Some((kind.as_str(), *position, *initial)), _ => None }).collect()
}

#[test]
fn living_world_population_follows_a_driving_player() {
    let mut app = app(7, 8.0);
    let mut max = (0, 0, 0);
    for _ in 0..60 {
        run(&mut app, 2.0, 60.0);
        let st = app.world().resource::<PopulationState>();
        let obs = app.world().resource::<LivingWorldObservers>().observers[0].position;
        let w = &st.world;
        max = (max.0.max(w.count(Kind::Skater)), max.1.max(w.count(Kind::Pedestrian)), max.2.max(w.count(Kind::Vehicle)));
        assert!(w.count(Kind::Skater) <= 3 && w.count(Kind::Pedestrian) <= 15 && w.count(Kind::Vehicle) <= 30);
        // Census entities never outlive their cull radius by more than one rotation (4 ticks at 8 m/s).
        for l in w.live(Kind::Pedestrian) {
            let d = ((l.position[0] - obs[0]).powi(2) + (l.position[2] - obs[2]).powi(2)).sqrt();
            assert!(d <= 90.0 + 2.0, "ped at {d}");
        }
    }
    assert_eq!(max, (3, 15, 30));
    let c = app.world().resource::<Collected>();
    for (kind, p, initial) in spawned(c) {
        assert!(p[1] == 0.0);
        if kind == "skater" {
            assert!(!initial);
        }
    }
    assert!(c.0.iter().any(|r| matches!(r, WireRecord::Despawn { reason, .. } if reason == "distance")));
}

#[test]
fn living_world_same_seed_same_stream_at_any_engine_rate() {
    let mut a = app(99, 8.0);
    run(&mut a, 60.0, 60.0);
    let mut b = app(99, 8.0);
    run(&mut b, 60.0, 60.0);
    let (ra, rb) = (&a.world().resource::<Collected>().0, &b.world().resource::<Collected>().0);
    assert!(ra.len() > 40);
    assert_eq!(ra, rb);
    // 144 Hz: the same console ticks, so the same decisions while the player stands still.
    let mut c = app(5, 0.0);
    run(&mut c, 30.0, 60.0);
    let mut d = app(5, 0.0);
    run(&mut d, 30.0, 144.0);
    let (rc, rd) = (&c.world().resource::<Collected>().0, &d.world().resource::<Collected>().0);
    let n = rc.len().min(rd.len());
    assert!(n > 20);
    assert_eq!(rc[..n], rd[..n]);
}

#[test]
fn living_world_online_spawns_nothing_and_client_mirrors_records() {
    let mut a = app(3, 8.0);
    run(&mut a, 20.0, 60.0);
    let records = a.world().resource::<Collected>().0.clone();
    // Wire round trip and a client that only mirrors.
    let json = serde_json::to_string(&records).unwrap();
    let back: Vec<WireRecord> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, records);
    let mut client = PopulationState::default();
    client.apply_records(&back);
    for k in Kind::ALL {
        let host: Vec<_> = a.world().resource::<PopulationState>().world.live(k).map(|l| l.id).collect();
        let mirror: Vec<_> = client.world.live(k).map(|l| l.id).collect();
        assert_eq!(host, mirror);
    }
    // Online: nothing new spawns; a Client role runs no rules at all.
    a.world_mut().resource_mut::<Collected>().0.clear();
    a.add_systems(Update, (|mut o: ResMut<LivingWorldObservers>| o.online = true).after(drive).before(step_population));
    run(&mut a, 20.0, 60.0);
    assert!(a.world().resource::<Collected>().0.iter().all(|r| matches!(r, WireRecord::Despawn { .. })));
    let mut c = app(3, 8.0);
    c.world_mut().resource_mut::<LivingWorldSettings>().net_role = NetRole::Client;
    run(&mut c, 10.0, 60.0);
    assert!(c.world().resource::<Collected>().0.is_empty());
}

#[test]
fn living_world_settings_disable_and_density() {
    let mut a = app(4, 0.0);
    {
        let mut s = a.world_mut().resource_mut::<LivingWorldSettings>();
        s.vehicles.density = 0.5;
        s.skaters.enabled = false;
    }
    run(&mut a, 20.0, 60.0);
    let w = &a.world().resource::<PopulationState>().world;
    assert_eq!(w.count(Kind::Vehicle), 15);
    assert_eq!(w.count(Kind::Skater), 0);
    assert_eq!(w.count(Kind::Pedestrian), 15);
    assert!(readout(a.world().resource::<PopulationState>()).contains("vehicles 15"));
}
