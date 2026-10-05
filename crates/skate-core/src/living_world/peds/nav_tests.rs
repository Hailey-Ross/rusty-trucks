//! Peds milestone M3: navmesh, retail wander target choice, crosswalk rule, avoidance.

use super::anim::{Intent, Locomotion};
use super::crosswalk::RoadWalkSignals;
use super::nav::{AREA_DEFAULT, AREA_OTHER, AREA_ROAD, NavMesh, NavMeshInput, NavPolyInput, NavRules, funnel};
use super::wander::*;
use crate::living_world::traffic::signals::{Light, SignalClock, SignalTimings};

/// A grid of `cell` m squares; `area(i, j)` gives each square's area byte or `None` (a hole).
fn grid(nx: i32, nz: i32, cell: f32, area: impl Fn(i32, i32) -> Option<u8>) -> NavMeshInput {
    let mut index = std::collections::BTreeMap::new();
    let mut polys = Vec::new();
    for j in 0..nz {
        for i in 0..nx {
            if let Some(a) = area(i, j) {
                index.insert((i, j), polys.len() as u32);
                let (x, z) = (i as f32 * cell, j as f32 * cell);
                // Counter-clockwise seen from above (x right, z up the page): (x,z) (x+c,z) (x+c,z+c) (x,z+c).
                polys.push(NavPolyInput { verts: vec![[x, 0.0, z], [x + cell, 0.0, z], [x + cell, 0.0, z + cell], [x, 0.0, z + cell]], neighbours: Vec::new(), area: a });
            }
        }
    }
    for j in 0..nz {
        for i in 0..nx {
            let Some(&k) = index.get(&(i, j)) else { continue };
            let n = [(i, j - 1), (i + 1, j), (i, j + 1), (i - 1, j)];
            polys[k as usize].neighbours = n.iter().map(|c| index.get(c).copied()).collect();
        }
    }
    NavMeshInput { agent: [0.12, 0.35, 0.2, 1.6], polygons: polys }
}

fn mesh(input: &NavMeshInput) -> NavMesh {
    NavMesh::build(input, NavRules::default())
}

#[test]
fn fan_angles_follow_the_retail_probe_order() {
    let deg = |v: Vec<f32>| v.into_iter().map(|a| (a.to_degrees() * 100.0).round() / 100.0).collect::<Vec<_>>();
    let p = WanderParams::default();
    assert_eq!(deg(fan_angles(&p.long)), vec![0.0, -22.5, 22.5, -45.0, 45.0, -67.5]);
    assert_eq!(deg(fan_angles(&p.short)), vec![0.0, -33.75, 33.75, -67.5, 67.5, -101.25, 101.25, -135.0, 135.0, -168.75]);
    assert_eq!((p.long.distance, p.short.distance, p.fallback_min, p.fallback_scale, p.mover_value), (40.0, 10.0, 3.0, 1.1, 2.0));
}

#[test]
fn locate_snaps_reachability_and_blocked_areas() {
    // Two islands: x 0..10 and x 14..20 (hole at 10..14); one blocked square inside the first.
    let input = grid(10, 5, 2.0, |i, j| match (i, j) {
        (5..=6, _) => None,
        (2, 2) => Some(AREA_OTHER),
        _ => Some(AREA_DEFAULT),
    });
    let m = mesh(&input);
    let a = m.locate([1.0, 0.0, 1.0]).unwrap();
    assert_eq!(a.position, [1.0, 0.0, 1.0]);
    assert!(m.locate([5.0, 0.0, 5.0]).is_none(), "blocked area");
    let snapped = m.locate([10.3, 0.0, 3.0]).expect("0.3 m outside snaps (radius 0.5)");
    assert!((snapped.position[0] - 10.0).abs() < 1e-5);
    assert!(m.locate([11.0, 0.0, 3.0]).is_none(), "1 m outside does not");
    let b = m.locate([15.0, 0.0, 1.0]).unwrap();
    assert!(!m.reachable(a.poly, b.poly));
    assert!(m.find_path(a, b).is_none());
    assert!(m.locate([1.0, 9.0, 1.0]).is_none(), "outside the height window");
}

#[test]
fn paths_bend_round_holes_and_stay_on_the_mesh() {
    // U-shape: a wall from z 0 to 14 at x 8..10 (open at the top).
    let input = grid(10, 10, 2.0, |i, j| if i == 4 && j < 7 { None } else { Some(AREA_DEFAULT) });
    let m = mesh(&input);
    let from = m.locate([3.0, 0.0, 1.0]).unwrap();
    let to = m.locate([15.0, 0.0, 1.0]).unwrap();
    let path = m.find_path(from, to).unwrap();
    assert_eq!(*path.last().unwrap(), [15.0, 0.0, 1.0]);
    assert!(path.len() >= 3, "goes round the wall: {path:?}");
    let mut prev = [3.0, 0.0, 1.0];
    for c in &path {
        for s in 0..=40 {
            let t = s as f32 / 40.0;
            let p = [prev[0] + (c[0] - prev[0]) * t, 0.0, prev[2] + (c[2] - prev[2]) * t];
            assert!(m.locate(p).is_some(), "segment leaves the mesh at {p:?} ({path:?})");
        }
        prev = *c;
    }
    // Determinism: the same query gives the same corners.
    assert_eq!(m.find_path(from, to), Some(path));
    // The funnel on a straight corridor is the goal alone.
    let straight = funnel([0.0, 0.0, 0.0], [0.0, 0.0, 10.0], &[([-1.0, 0.0, 5.0], [1.0, 0.0, 5.0])]);
    assert_eq!(straight, vec![[0.0, 0.0, 10.0]]);
}

#[test]
fn target_choice_is_retail_and_deterministic() {
    let p = WanderParams::default();
    // Open 100 x 100 m: straight ahead 40 m.
    let open = mesh(&grid(50, 50, 2.0, |_, _| Some(AREA_DEFAULT)));
    let from = open.locate([50.0, 0.0, 10.0]).unwrap();
    let (t, _) = choose_target(&open, &p, from, forward(0.0), false);
    assert!((t[0] - 50.0).abs() < 1e-3 && (t[2] - 50.0).abs() < 1e-3, "{t:?}");
    // Mesh ends 20 m ahead (a 30 m deep strip): straight, -22.5, +22.5, -45 and +45 degrees
    // land beyond it; the last long probe (-67.5 degrees, the extra odd one) fits.
    let strip = mesh(&grid(50, 15, 2.0, |_, _| Some(AREA_DEFAULT)));
    let from = strip.locate([50.0, 0.0, 10.0]).unwrap();
    let (t, located) = choose_target(&strip, &p, from, forward(0.0), false);
    assert!(located.is_some());
    let a = (-67.5f32).to_radians();
    assert!((t[0] - (50.0 + 40.0 * a.sin())).abs() < 1e-3 && (t[2] - (10.0 + 40.0 * a.cos())).abs() < 1e-3, "{t:?}");
    // A 16 m deep strip: no 40 m probe fits; the short fan's straight probe (10 m) does.
    let shallow = mesh(&grid(50, 8, 2.0, |_, _| Some(AREA_DEFAULT)));
    let from = shallow.locate([50.0, 0.0, 4.0]).unwrap();
    let (t, _) = choose_target(&shallow, &p, from, forward(0.0), false);
    assert!((t[0] - 50.0).abs() < 1e-3 && (t[2] - 14.0).abs() < 1e-3, "short fan straight: {t:?}");
    // Facing +x along the strip: the long fan's straight probe fits.
    let from = strip.locate([50.0, 0.0, 10.0]).unwrap();
    let (t, _) = choose_target(&strip, &p, from, forward(std::f32::consts::FRAC_PI_2), false);
    assert!((t[0] - 90.0).abs() < 1e-3 && (t[2] - 10.0).abs() < 1e-3, "{t:?}");
    // Blocked straight ahead at 40 m but open at -22.5 degrees: the first odd probe wins.
    let wedge = mesh(&grid(60, 60, 2.0, |i, j| if (24..27).contains(&i) && j >= 20 { None } else { Some(AREA_DEFAULT) }));
    let from = wedge.locate([51.0, 0.0, 10.0]).unwrap();
    let (t, _) = choose_target(&wedge, &p, from, forward(0.0), false);
    let a = (-22.5f32).to_radians();
    let expect = [51.0 + 40.0 * a.sin(), 10.0 + 40.0 * a.cos()];
    assert!((t[0] - expect[0]).abs() < 1e-3 && (t[2] - expect[1]).abs() < 1e-3, "{t:?} vs {expect:?}");
    // skip_long (after a failure event): straight at 10 m.
    let (t, _) = choose_target(&wedge, &p, from, forward(0.0), true);
    assert!((t[2] - 20.0).abs() < 1e-3);
    // Nothing fits (a 2 x 2 m island): the fallback step max(3, 2.0 x 1.1) = 3 m.
    let island = mesh(&grid(1, 1, 2.0, |_, _| Some(AREA_DEFAULT)));
    let from = island.locate([1.0, 0.0, 1.0]).unwrap();
    let (t, located) = choose_target(&island, &p, from, forward(0.0), false);
    assert!((t[2] - 4.0).abs() < 1e-5 && located.is_none());
}

/// A kinematic body for the tests: walks 1.325 m/s (the walk clip) when told to walk, turns
/// 180 degrees on a turn intent, keeps to the mesh and to the separation rule.
#[derive(Clone, Debug, PartialEq)]
struct Body {
    id: u64,
    pos: [f32; 3],
    heading: f32,
    nav: PedNav,
    state: Locomotion,
}

fn run(m: &NavMesh, bodies: &mut [Body], ticks: u32, rule: CrosswalkRule, signals: &dyn WalkSignals, mut each: impl FnMut(u32, &[Body])) {
    let p = WanderParams::default();
    let dt = 1.0 / 60.0;
    for t in 0..ticks {
        for k in 0..bodies.len() {
            let others: Vec<Neighbour> = bodies.iter().map(|b| Neighbour { order: b.id, position: b.pos }).collect();
            let b = &mut bodies[k];
            let out = b.nav.step(m, &p, rule, signals, b.id, b.pos, b.heading, b.state, &others, dt);
            b.heading += out.turn;
            match out.intent {
                Intent::Walk => {
                    b.state = Locomotion::Walk;
                    let f = forward(b.heading);
                    let to = [b.pos[0] + f[0] * 1.325 * dt, b.pos[1], b.pos[2] + f[1] * 1.325 * dt];
                    let (next, ok) = constrain_step(m, b.pos, to);
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
        }
        each(t, bodies);
    }
}

#[test]
fn wandering_stays_on_walkable_ground_and_is_deterministic() {
    // A plaza with a blocked block and a hole.
    let input = grid(30, 30, 2.0, |i, j| match (i, j) {
        (10..=14, 10..=14) => None,
        (20..=22, 4..=8) => Some(AREA_OTHER),
        _ => Some(AREA_DEFAULT),
    });
    let m = mesh(&input);
    let start = || vec![Body { id: 1, pos: [5.0, 0.0, 5.0], heading: 0.3, nav: PedNav::default(), state: Locomotion::Idle }];
    let mut a = start();
    let mut b = start();
    run(&m, &mut a, 60 * 120, CrosswalkRule::Off, &NoSignals, |_, bs| {
        assert!(m.locate(bs[0].pos).is_some(), "off the mesh at {:?}", bs[0].pos);
    });
    run(&m, &mut b, 60 * 120, CrosswalkRule::Off, &NoSignals, |_, _| {});
    assert_eq!(a, b, "same inputs, same walk");
    assert!(a[0].nav.targets_chosen >= 3, "kept choosing targets: {}", a[0].nav.targets_chosen);
    let moved = ((a[0].pos[0] - 5.0).powi(2) + (a[0].pos[2] - 5.0).powi(2)).sqrt();
    assert!(moved > 1.0, "walked somewhere");
}

struct Lamp(std::cell::Cell<Light>);
impl WalkSignals for Lamp {
    fn walk_light(&self, _: [f32; 3]) -> Option<Light> {
        Some(self.0.get())
    }
}

#[test]
fn crosswalk_rule_waits_for_the_walk_light() {
    // Pavement z 0..10, road z 10..20, pavement z 20..30; a route across.
    let input = grid(5, 15, 2.0, |_, j| Some(if (5..10).contains(&j) { AREA_ROAD } else { AREA_DEFAULT }));
    let m = mesh(&input);
    let lamp = Lamp(std::cell::Cell::new(Light::Red));
    let mut nav = PedNav::default();
    nav.set_route(Some(PedRoute { points: vec![[5.0, 0.0, 26.0]], looped: false }));
    let mut bodies = vec![Body { id: 1, pos: [5.0, 0.0, 6.0], heading: 0.0, nav, state: Locomotion::Idle }];
    let mut waited = 0;
    run(&m, &mut bodies, 60 * 10, CrosswalkRule::WalkSignal, &lamp, |_, bs| {
        assert!(bs[0].pos[2] < 10.0, "entered the road on red: {:?}", bs[0].pos);
        if bs[0].nav.waiting == NavWait::Crosswalk {
            waited += 1;
        }
    });
    assert!(waited > 60 * 5, "stood at the kerb: {waited}");
    lamp.0.set(Light::Amber);
    run(&m, &mut bodies, 60, CrosswalkRule::WalkSignal, &lamp, |_, bs| assert!(bs[0].pos[2] < 10.0));
    lamp.0.set(Light::Green);
    run(&m, &mut bodies, 60 * 20, CrosswalkRule::WalkSignal, &lamp, |_, _| {});
    assert!(bodies[0].pos[2] > 20.0, "crossed on green: {:?}", bodies[0].pos);
    // Retail default (Off): crosses without looking.
    let mut nav = PedNav::default();
    nav.set_route(Some(PedRoute { points: vec![[5.0, 0.0, 26.0]], looped: false }));
    let mut bodies = vec![Body { id: 1, pos: [5.0, 0.0, 6.0], heading: 0.0, nav, state: Locomotion::Idle }];
    lamp.0.set(Light::Red);
    run(&m, &mut bodies, 60 * 20, CrosswalkRule::Off, &lamp, |_, _| {});
    assert!(bodies[0].pos[2] > 20.0);
}

#[test]
fn crosswalk_lights_come_from_the_shared_clock() {
    let timings = SignalTimings { green: 7.0, amber: 1.0, all_red: 0.5, walk_split: 0.4 };
    let mut clock = SignalClock::new(timings);
    let arms = vec![(0usize, 0u8, [0.0, 0.0, -10.0]), (0, 1, [10.0, 0.0, 0.0]), (0, 2, [0.0, 0.0, 10.0]), (0, 3, [-10.0, 0.0, 0.0])];
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..(17 * 60) {
        let s = RoadWalkSignals { arms: &arms, clock: &clock, radius: 20.0 };
        for (_, e, p) in &arms {
            let got = s.walk_light([p[0] * 0.9, 0.0, p[2] * 0.9]).unwrap();
            assert_eq!(got, clock.walk_for_end((e + 1) % 4).light);
            seen.insert((*e, got as u8));
        }
        assert!(s.walk_light([100.0, 0.0, 100.0]).is_none());
        clock.advance(1.0 / 60.0);
    }
    assert!(seen.contains(&(0, Light::Green as u8)) && seen.contains(&(0, Light::Red as u8)) && seen.contains(&(1, Light::Green as u8)));
}

#[test]
fn avoidance_keeps_the_agent_radius() {
    // A 4 m wide corridor; two peds walk at each other, a third stands in the way of a fourth.
    let input = grid(40, 2, 2.0, |_, _| Some(AREA_DEFAULT));
    let m = mesh(&input);
    let route = |x: f32| {
        let mut n = PedNav::default();
        n.set_route(Some(PedRoute { points: vec![[x, 0.0, 2.0]], looped: false }));
        n
    };
    let mut bodies = vec![
        Body { id: 1, pos: [10.0, 0.0, 2.0], heading: std::f32::consts::FRAC_PI_2, nav: route(70.0), state: Locomotion::Idle },
        Body { id: 2, pos: [30.0, 0.0, 2.2], heading: -std::f32::consts::FRAC_PI_2, nav: route(2.0), state: Locomotion::Idle },
    ];
    let r = m.agent[1];
    let mut closest = f32::MAX;
    run(&m, &mut bodies, 60 * 60, CrosswalkRule::Off, &NoSignals, |_, bs| {
        let d = ((bs[0].pos[0] - bs[1].pos[0]).powi(2) + (bs[0].pos[2] - bs[1].pos[2]).powi(2)).sqrt();
        closest = closest.min(d);
        assert!(d >= 2.0 * r - 1e-4, "peds overlap: {d}");
    });
    assert!(closest < 3.0, "they did meet ({closest})");
    assert!(bodies[0].pos[0] > 30.0 && bodies[1].pos[0] < 10.0, "and got past each other: {:?} {:?}", bodies[0].pos, bodies[1].pos);
    // separation_ok lets overlapping peds move apart but not closer.
    let others = [Neighbour { order: 9, position: [0.0, 0.0, 0.0] }];
    assert!(separation_ok([0.2, 0.0, 0.0], [0.3, 0.0, 0.0], 1, &others, r));
    assert!(!separation_ok([0.3, 0.0, 0.0], [0.2, 0.0, 0.0], 1, &others, r));
}
