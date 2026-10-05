//! Data-gated checks of the ped navmesh (peds milestone M3) on the user's own export. Skips (passes
//! with a note) without data. `SKATE3_ASSET_ROOT`: one or more asset roots joined like PATH; reads
//! `private/living_world/navmesh.bin` (+ `roads.bin` and `tables.json` for the road and
//! crosswalk checks).
//!
//! Expected values are the shipped DownTown NavPower graphs as decoded for M3 (asserted here, not
//! embedded in the engine): 36443 polygons (24991 area 0x11, 8997 area 0xA1, 2455 area 0xF1).

use skate_core::living_world::peds::anim::{Intent, Locomotion};
use skate_core::living_world::peds::crosswalk::{RoadWalkSignals, signalled_arms};
use skate_core::living_world::peds::nav::{AREA_DEFAULT, AREA_OTHER, AREA_ROAD};
use skate_core::living_world::peds::wander::{NoSignals, constrain_step, crosswalk_ok, forward, separation_ok};
use skate_core::living_world::peds::{CrosswalkRule, NavMesh, NavRules, Neighbour, PedNav, WalkSignals, WanderParams};
use skate_core::living_world::traffic::{RoadNetwork, SignalClock};
use skate_core::living_world::traffic::signals::Light;
use std::path::PathBuf;

fn find(rel: &str) -> Option<PathBuf> {
    let raw = std::env::var_os("SKATE3_ASSET_ROOT")?;
    std::env::split_paths(&raw).map(|r| r.join(rel)).find(|p| p.is_file())
}

fn downtown() -> Option<NavMesh> {
    let bytes = std::fs::read(find(skate_data::ped_nav::NAVMESH)?).ok()?;
    let input = skate_data::ped_nav::district(&bytes, "DownTown").unwrap()?;
    Some(NavMesh::build(&input, NavRules::default()))
}

#[test]
fn downtown_navmesh_decodes_with_the_shipped_areas() {
    let Some(m) = downtown() else {
        eprintln!("skipped: set SKATE3_ASSET_ROOT to an export with living_world/navmesh.bin");
        return;
    };
    let count = |a: u8| m.polys.iter().filter(|p| p.area == a).count();
    assert_eq!((m.polys.len(), count(AREA_DEFAULT), count(AREA_ROAD), count(AREA_OTHER)), (36443, 24991, 8997, 2455));
    assert_eq!(m.agent, [0.12, 0.35, 0.2, 1.6]);
    for p in &m.polys {
        assert!(p.verts.len() >= 3);
        assert!(p.verts.iter().all(|v| v.iter().all(|c| c.is_finite())));
    }
    // Road piece starts lie on road polygons (the area byte's meaning).
    let Some(roads) = find("private/living_world/roads.bin") else { return };
    let graph = skate_data::roads::RoadGraph::parse(&std::fs::read(roads).unwrap()).unwrap();
    let net = RoadNetwork::build(&graph.traffic_input()).unwrap();
    let (mut on_mesh, mut on_road) = (0, 0);
    for s in &net.segments {
        for piece in &s.pieces {
            if let Some(p) = m.locate(piece.centre.start) {
                on_mesh += 1;
                on_road += (m.polys[p.poly as usize].area == AREA_ROAD) as u32;
            }
        }
    }
    eprintln!("road piece starts on the DownTown mesh: {on_mesh}, on road polygons: {on_road}");
    assert!(on_mesh > 1000 && on_road as f32 > 0.95 * on_mesh as f32);
}

struct Body {
    id: u64,
    pos: [f32; 3],
    heading: f32,
    nav: PedNav,
    state: Locomotion,
}

/// Peds on pavement polygons within 60 m of Aletown's recomp ped positions (about -150, 12, 480).
fn spawn(m: &NavMesh, n: usize) -> Vec<Body> {
    let centre = [-150.0f32, 12.0, 470.0];
    let pick: Vec<usize> = (0..m.polys.len())
        .filter(|&k| {
            let p = &m.polys[k];
            p.area == AREA_DEFAULT && ((p.centre[0] - centre[0]).powi(2) + (p.centre[2] - centre[2]).powi(2)).sqrt() < 60.0 && (p.centre[1] - centre[1]).abs() < 10.0
        })
        .collect();
    assert!(pick.len() >= n, "pavement near Aletown: {}", pick.len());
    (0..n).map(|i| {
        let k = pick[i * pick.len() / n];
        Body { id: i as u64 + 1, pos: m.polys[k].centre, heading: i as f32 * 0.7, nav: PedNav::default(), state: Locomotion::Idle }
    }).collect()
}

/// Kinematic walk (the walk clip's 1.325 m/s), the mesh and separation rules as in the game.
fn run(m: &NavMesh, bodies: &mut [Body], ticks: u32, rule: CrosswalkRule, signals: &dyn WalkSignals, mut each: impl FnMut(&Body, [f32; 3])) {
    let p = WanderParams::default();
    let dt = 1.0 / 60.0;
    for _ in 0..ticks {
        for k in 0..bodies.len() {
            let others: Vec<Neighbour> = bodies.iter().map(|b| Neighbour { order: b.id, position: b.pos }).collect();
            let b = &mut bodies[k];
            let before = b.pos;
            let out = b.nav.step(m, &p, rule, signals, b.id, b.pos, b.heading, b.state, &others, dt);
            b.heading += out.turn;
            match out.intent {
                Intent::Walk => {
                    b.state = Locomotion::Walk;
                    let f = forward(b.heading);
                    let (next, ok) = constrain_step(m, b.pos, [b.pos[0] + f[0] * 1.325 * dt, b.pos[1], b.pos[2] + f[1] * 1.325 * dt]);
                    if ok && separation_ok(b.pos, next, b.id, &others, m.agent[1]) && crosswalk_ok(m, rule, signals, b.pos, next) {
                        b.pos = next;
                    }
                }
                Intent::TurnLeft | Intent::TurnRight => {
                    b.heading += if out.intent == Intent::TurnLeft { std::f32::consts::PI } else { -std::f32::consts::PI };
                    b.state = Locomotion::Idle;
                }
                Intent::Idle => b.state = Locomotion::Idle,
            }
            each(b, before);
        }
    }
}

#[test]
fn downtown_peds_wander_on_walkable_ground() {
    let Some(m) = downtown() else {
        eprintln!("skipped: needs living_world/navmesh.bin under SKATE3_ASSET_ROOT");
        return;
    };
    let mut a = spawn(&m, 15);
    let (mut samples, mut road, mut off) = (0u32, 0u32, 0u32);
    run(&m, &mut a, 60 * 90, CrosswalkRule::Off, &NoSignals, |b, _| {
        samples += 1;
        match m.locate(b.pos) {
            Some(p) => {
                assert_ne!(m.polys[p.poly as usize].area, AREA_OTHER, "ped {} on a 0xF1 polygon", b.id);
                road += (m.polys[p.poly as usize].area == AREA_ROAD) as u32;
            }
            None => off += 1,
        }
    });
    let moved: Vec<f32> = a.iter().zip(spawn(&m, 15)).map(|(x, y)| ((x.pos[0] - y.pos[0]).powi(2) + (x.pos[2] - y.pos[2]).powi(2)).sqrt()).collect();
    let targets: u32 = a.iter().map(|b| b.nav.targets_chosen).sum();
    eprintln!("90 s x 15 peds: on road polygons {:.1} % (recomp 3-4 %), off mesh {off}, targets {targets}, moved {moved:?}", 100.0 * road as f32 / samples as f32);
    assert_eq!(off, 0, "every sample on walkable ground");
    assert!(moved.iter().filter(|d| **d > 5.0).count() >= 10, "most peds walked away from their spawn");
    // Deterministic: a second run lands every ped on the same spot.
    let mut b = spawn(&m, 15);
    run(&m, &mut b, 60 * 90, CrosswalkRule::Off, &NoSignals, |_, _| {});
    assert!(a.iter().zip(&b).all(|(x, y)| x.pos == y.pos && x.heading == y.heading && x.nav == y.nav));
}

#[test]
fn downtown_crosswalk_rule_crosses_only_on_walk() {
    let (Some(m), Some(roads), Some(tables)) = (downtown(), find("private/living_world/roads.bin"), find("private/living_world/tables.json")) else {
        eprintln!("skipped: needs navmesh.bin, roads.bin and tables.json under SKATE3_ASSET_ROOT");
        return;
    };
    let graph = skate_data::roads::RoadGraph::parse(&std::fs::read(roads).unwrap()).unwrap();
    let net = RoadNetwork::build(&graph.traffic_input()).unwrap();
    let tables: serde_json::Value = serde_json::from_slice(&std::fs::read(tables).unwrap()).unwrap();
    let timings = skate_data::roads::signal_timings(&tables).expect("trafficlights record");
    let arms = signalled_arms(&net);
    let mut clock = SignalClock::new(timings);
    // Peds on pavement next to signalled junction arms of this district.
    let near_arm = |c: [f32; 3]| arms.iter().any(|a| ((a.2[0] - c[0]).powi(2) + (a.2[2] - c[2]).powi(2)).sqrt() < 14.0 && (a.2[1] - c[1]).abs() < 3.0);
    let pick: Vec<usize> = (0..m.polys.len()).filter(|&k| m.polys[k].area == AREA_DEFAULT && near_arm(m.polys[k].centre)).collect();
    assert!(pick.len() >= 30, "pavement next to signalled arms: {}", pick.len());
    let mut bodies: Vec<Body> = (0..30)
        .map(|i| {
            let k = pick[i * pick.len() / 30];
            Body { id: i as u64 + 1, pos: m.polys[k].centre, heading: i as f32 * 1.3, nav: PedNav::default(), state: Locomotion::Idle }
        })
        .collect();
    let (mut entries, mut signalled, mut waits) = (0u32, 0u32, 0u32);
    for _ in 0..(60 * 120) {
        let signals = RoadWalkSignals { arms: &arms, clock: &clock, radius: 20.0 };
        run(&m, &mut bodies, 1, CrosswalkRule::WalkSignal, &signals, |b, before| {
            let area = |p: [f32; 3]| m.locate(p).map(|x| m.polys[x.poly as usize].area);
            if b.nav.waiting == skate_core::living_world::peds::NavWait::Crosswalk {
                waits += 1;
            }
            if area(before) != Some(AREA_ROAD) && area(b.pos) == Some(AREA_ROAD) {
                entries += 1;
                let light = signals.walk_light(b.pos);
                signalled += light.is_some() as u32;
                assert!(light.is_none() || light == Some(Light::Green), "ped {} stepped onto the road at {:?} on {light:?}", b.id, b.pos);
            }
        });
        clock.advance(1.0 / 60.0);
    }
    eprintln!("120 s x 30 peds with the crosswalk rule: {entries} road entries ({signalled} at signalled arms), {waits} wait ticks");
    assert!(signalled > 0 && waits > 0, "the rule was exercised");
}
