//! Data-gated checks of the living-world population data on the user's own export. Skips (passes
//! with a note) when no data is configured. `SKATE3_ASSET_ROOT`: converted assets (one root, or
//! several joined like PATH); reads `private/living_world/{tables.json, census.json,
//! <District>.census.bin, skater_profiles.json, skater_paths/*.bin}`.
//!
//! The expected values are the shipped Skate 3 data the research measured (labels in
//! `.claude/notes/npc-livingworld-re.md` §1 and doc 26); they are asserted, not embedded in the
//! engine.

use skate_core::living_world::{Kind, LivingWorld, Observer, PopulationConfig, TickInputs};
use skate_data::aipath;
use skate_data::living_world::{self, LivingWorldTables};
use std::path::PathBuf;

fn folders() -> Vec<PathBuf> {
    let Some(raw) = std::env::var_os("SKATE3_ASSET_ROOT") else { return Vec::new() };
    std::env::split_paths(&raw)
        .flat_map(|root| ["private/living_world", "living_world"].map(|p| root.join(p)))
        .filter(|p| p.is_dir())
        .collect()
}

fn find(name: &str) -> Option<PathBuf> {
    folders().into_iter().map(|f| f.join(name)).find(|p| p.exists())
}

fn tables() -> Option<LivingWorldTables> {
    let path = find("tables.json")?;
    Some(LivingWorldTables::parse(&std::fs::read(path).unwrap()).unwrap())
}

const DISTRICTS: [(&str, u32, u32); 3] = [("DownTown", 384, 448), ("Industrial", 640, 288), ("University", 384, 448)];

#[test]
fn census_tables_hold_the_shipped_caps_and_ranges() {
    let Some(t) = tables() else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to an export with living_world/tables.json");
        return;
    };
    let cap = |k: &str| t.census[k].max_population;
    // Peds [data]: districts 20, sub-areas 15, default 40, reclaimed 10, loadingdocks 8, observatory 6.
    for k in ["downtown", "university", "industrial"] {
        assert_eq!(cap(k), 20, "{k}");
    }
    for k in ["aletown", "business_center", "mall", "memorial", "residential", "campus"] {
        assert_eq!(cap(k), 15, "{k}");
    }
    assert_eq!((cap("pedestrians"), cap("reclaimed"), cap("loadingdocks"), cap("observatory")), (40, 10, 8, 6));
    // Vehicles [data]: dwntwn 30, indust 25, univ 10, default 25.
    assert_eq!((cap("dwntwn"), cap("indust"), cap("univ"), cap("vehicles")), (30, 25, 10, 25));
    // Category weights of the DownTown traffic group sum to 0.875 (dwntwn); the roll misses the rest.
    let w: f32 = t.census["dwntwn"].categories.iter().map(|c| c.weight).sum();
    assert!((w - 0.875).abs() < 1e-4, "{w}");

    let peds = t.ranges["pedestrians"];
    assert_eq!((peds.slow.spawn_inner, peds.slow.spawn_outer, peds.slow.cull, peds.slow.forward_offset, peds.slow.speed_kmh), (50.0, 60.0, 70.0, 0.0, 45.0));
    assert_eq!((peds.fast.spawn_inner, peds.fast.spawn_outer, peds.fast.cull, peds.fast.forward_offset, peds.fast.speed_kmh), (50.0, 80.0, 90.0, 20.0, 80.0));
    let cars = t.ranges["vehicles"].at(50.0);
    assert_eq!((cars.spawn_inner, cars.spawn_outer, cars.cull, cars.forward_offset), (80.0, 100.0, 110.0, 0.0));
    let mut cfg = PopulationConfig::retail();
    t.apply_to(&mut cfg);
    assert_eq!(cfg.pedestrians.range, Some(peds));
}

#[test]
fn census_grids_parse_and_name_known_records() {
    let Some(t) = tables() else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to an export with living_world census grids");
        return;
    };
    for (district, w, h) in DISTRICTS {
        let Some(path) = find(&format!("{district}.census.bin")) else { panic!("{district} grid missing") };
        let g = living_world::parse_census_grid(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!((g.width, g.height, g.cell), (w, h, 4.0), "{district}");
        for name in &g.names {
            assert!(t.census.contains_key(name), "{district}: {name} is not a census record");
        }
        for layer in ["livingworld_npc_census", "livingworld_vehicle_census"] {
            let painted = g.layers[layer].iter().filter(|&&v| v != 0).count();
            assert!(painted > 1000, "{district} {layer}: {painted} painted cells");
        }
    }
}

/// The census point of a grid with the most painted ring around it (8-80 m, peds layer).
fn busy_point(g: &skate_core::living_world::CensusGrid) -> [f32; 3] {
    let mut best = (0, [0.0f32; 3]);
    for j in (0..g.height).step_by(8) {
        for i in (0..g.width).step_by(8) {
            let x = g.origin[0] + (i as f32 + 0.5) * g.cell;
            let z = g.origin[1] + (j as f32 + 0.5) * g.cell;
            let score = (0..48)
                .filter(|k| {
                    let a = *k as f32 * std::f32::consts::TAU / 16.0;
                    let r = [20.0, 50.0, 60.0][*k as usize % 3];
                    g.record_at("livingworld_npc_census", x + r * a.cos(), z + r * a.sin()).is_some()
                })
                .count();
            if score > best.0 {
                best = (score, [x, 0.0, z]);
            }
        }
    }
    best.1
}

#[test]
fn population_on_the_exported_downtown_census_respects_caps_and_radii() {
    let (Some(t), Some(path)) = (tables(), find("DownTown.census.bin")) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to an export with the DownTown census grid");
        return;
    };
    let grid = living_world::parse_census_grid(&std::fs::read(path).unwrap()).unwrap();
    let at = busy_point(&grid);
    let map = t.census_map(vec![grid]);
    let mut cfg = PopulationConfig::retail();
    t.apply_to(&mut cfg);
    let mut world = LivingWorld::new(cfg, 2026);
    let obs = [Observer { position: at, velocity: [0.0; 3] }];
    let mut inputs = TickInputs::offline(&obs);
    inputs.census = Some(&map);
    let mut max_peds = 0;
    for _ in 0..(30 * 60) {
        for d in world.step(&inputs) {
            if let skate_core::living_world::Decision::Spawn(s) = d {
                let r = ((s.position[0] - at[0]).powi(2) + (s.position[2] - at[2]).powi(2)).sqrt();
                let (lo, hi) = if s.initial { (8.0, 80.0) } else if s.id.kind == Kind::Pedestrian { (50.0, 60.0) } else { (80.0, 100.0) };
                assert!(r >= lo - 1e-3 && r <= hi + 1e-3, "{:?} at {r}", s.id);
            }
        }
        max_peds = max_peds.max(world.count(Kind::Pedestrian));
        assert!(world.count(Kind::Vehicle) <= 30);
    }
    // DownTown ped records cap at 15 (sub-areas) and 40 (`pedestrians`, a sliver): with the pool of
    // 31 the live count never passes the largest cap painted around the point.
    assert!(max_peds > 0 && max_peds <= 15, "peds {max_peds}");
    eprintln!("DownTown at {at:?}: {max_peds} peds max, {} vehicles", world.count(Kind::Vehicle));
}

#[test]
fn skater_pool_and_lines_load() {
    let (Some(profiles), Some(paths)) = (find("skater_profiles.json"), find("skater_paths")) else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to an export with skater_profiles.json and skater_paths");
        return;
    };
    let chars = living_world::skater_characters(&std::fs::read(profiles).unwrap(), &[]).unwrap();
    // 42-character pool [data] minus the 4 teammates nobody recruited yet.
    assert_eq!(chars.len(), 38, "{:?}", chars.iter().map(|c| &c.key).collect::<Vec<_>>());
    let mut total = 0;
    for entry in std::fs::read_dir(paths).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("bin") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let tiles = aipath::parse_pack(&bytes).unwrap();
        let (unique, _) = aipath::district_paths(&tiles).unwrap();
        let lines = living_world::skater_lines(unique.iter().map(|d| &d.path));
        assert_eq!(lines.len(), unique.len());
        total += lines.len();
    }
    assert_eq!(total, 1_691);
}

/// Milestone 3: every exported line rides end to end with the replay cursor at the recording
/// rate, with the retail branch choice (a player standing at the line start), deterministically.
#[test]
fn replay_cursor_rides_every_exported_line() {
    use skate_core::living_world::replay::{BranchContext, CursorEvent, Decider, LineCursor, ReplayLine};
    use std::collections::BTreeMap;
    let Some(paths) = find("skater_paths") else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to an export with skater_paths");
        return;
    };
    let mut total = (0usize, 0usize, 0usize, 0u64);
    for district in ["DownTown", "Industrial", "University"] {
        let pack = std::fs::read(paths.join(format!("{district}.bin"))).unwrap();
        let tiles = aipath::parse_pack(&pack).unwrap();
        let (unique, _) = aipath::district_paths(&tiles).unwrap();
        let lines: BTreeMap<[u8; 16], ReplayLine> = unique.iter().map(|p| (p.path.id.0, living_world::replay_line(&p.path))).collect();
        // Every branch target resolves to a line of the same district with the node in range.
        for l in lines.values() {
            for g in &l.groups {
                assert!((g.node as usize) < l.nodes.len().saturating_sub(1), "group on the last node");
                for b in &g.branches {
                    assert!(lines.get(&b.target).is_some_and(|t| (b.target_node as usize) < t.nodes.len()));
                }
            }
        }
        let run = |id: [u8; 16]| {
            let start = lines[&id].nodes[0].position;
            let players = [start];
            let mut c = LineCursor::spawn(&lines, id, 0);
            let mut ev = Vec::new();
            let mut frames = 0u64;
            let mut last = c.sample(&lines, 0.0).unwrap().position;
            let mut max_jump = 0.0f32;
            while !c.finished && frames < 60 * 60 * 10 {
                let s = c.sample(&lines, 0.0).unwrap();
                let speed = (s.velocity[0].powi(2) + s.velocity[1].powi(2) + s.velocity[2].powi(2)).sqrt();
                let ctx = BranchContext { position: s.position, forward: s.velocity, speed, players: &players, others: &[], in_use: &[], preferred_skill: -1, online: false };
                c.step(&lines, &mut Decider::Decide(ctx), &mut ev);
                frames += 1;
                let p = c.sample(&lines, 0.0).unwrap().position;
                let branched = matches!(ev.last(), Some(CursorEvent::Branch(_)));
                if !branched {
                    max_jump = max_jump.max(((p[0] - last[0]).powi(2) + (p[1] - last[1]).powi(2) + (p[2] - last[2]).powi(2)).sqrt());
                }
                last = p;
            }
            (c, ev.into_iter().filter(|e| matches!(e, CursorEvent::Branch(_))).count(), frames, max_jump)
        };
        for &id in lines.keys() {
            let (c, branches, frames, max_jump) = run(id);
            // A recorded line moves at most a few metres per 60 Hz frame (p90 speed 16 m/s [data]).
            assert!(max_jump < 3.0, "{district} {:02x?}: {max_jump} m in one frame", &id[..8]);
            total.0 += 1;
            total.1 += branches;
            total.2 += c.finished as usize;
            total.3 += frames;
        }
        // Deterministic: the same line twice gives the same cursor.
        let first = *lines.keys().next().unwrap();
        assert_eq!(run(first).0, run(first).0);
    }
    eprintln!("replay: {} lines, {} branches taken, {} finished, {} frames", total.0, total.1, total.2, total.3);
    assert_eq!(total.0, 1691);
    assert!(total.2 > 1600, "most rides end at a line end within 10 minutes");
}
