//! Line cursor tests on synthetic lines (no data). The data-gated test on the export lives in
//! `skate-data/tests/living_world_data.rs`.

use super::*;
use crate::living_world::clock::ConsoleClock;

const IDENTITY: [u8; 4] = [128, 128, 128, 255];

fn id(n: u8) -> [u8; 16] {
    let mut i = [0u8; 16];
    i[0] = n;
    i
}

/// A straight line along +Z from `origin`: `count` nodes, `frames` 60 Hz frames apart, `step` m
/// apart.
fn straight(n: u8, origin: Vec3, count: u32, frames: u8, step: f32) -> ReplayLine {
    let nodes = (0..count)
        .map(|i| ReplayNode {
            position: [origin[0], origin[1], origin[2] + i as f32 * step],
            board: IDENTITY,
            skater: IDENTITY,
            frames: if i == 0 { 0 } else { frames },
            event: 0,
            flags: 0,
            jump: None,
        })
        .collect();
    ReplayLine { id: id(n), flags: 4, skill: 0, nodes, jumps: vec![], groups: vec![] }
}

fn lines(v: Vec<ReplayLine>) -> BTreeMap<[u8; 16], ReplayLine> {
    v.into_iter().map(|l| (l.id, l)).collect()
}

fn ctx<'a>(position: Vec3, players: &'a [Vec3]) -> BranchContext<'a> {
    BranchContext { position, forward: [0.0, 0.0, 1.0], speed: 8.0, players, others: &[], in_use: &[], preferred_skill: -1, online: false }
}

#[test]
fn replay_cursor_follows_the_line_at_60_hz() {
    // 4 frames per node, 0.5 m per node = 7.5 m/s.
    let ls = lines(vec![straight(1, [10.0, 2.0, 0.0], 50, 4, 0.5)]);
    let mut c = LineCursor::spawn(&ls, id(1), 0);
    let mut ev = Vec::new();
    c.advance(6, &ls, &mut Decider::Stay, &mut ev);
    // 6 frames = node 1 + 2 frames into the next segment.
    assert_eq!((c.node, c.frame_in_segment), (1, 2));
    let s = c.sample(&ls, 0.0).unwrap();
    assert!((s.position[2] - 0.75).abs() < 1e-5, "{:?}", s.position);
    assert!((s.velocity[2] - 7.5).abs() < 1e-4);
    assert!(s.heading.abs() < 1e-5);
    // Half a frame later: 1/8 of a node further.
    let h = c.sample(&ls, 0.5).unwrap();
    assert!((h.position[2] - 0.8125).abs() < 1e-5);
    assert_eq!(ev.iter().filter(|e| matches!(e, CursorEvent::Node { .. })).count(), 1);
    assert_eq!(s.phase, ReplayPhase::Rolling);
}

#[test]
fn replay_cursor_is_frame_rate_independent() {
    // The engine converts game time into 60 Hz frames; any engine rate gives the same cursor.
    let ls = lines(vec![straight(1, [0.0; 3], 400, 3, 0.4)]);
    let mut results = Vec::new();
    for engine_hz in [30.0, 60.0, 64.0, 144.0, 240.0] {
        let mut clock = ConsoleClock::new(RECORDING_HZ);
        let mut c = LineCursor::spawn(&ls, id(1), 0);
        let mut ev = Vec::new();
        for _ in 0..(engine_hz * 10.0) as u32 {
            let due = clock.advance(1.0 / engine_hz);
            c.advance(due, &ls, &mut Decider::Stay, &mut ev);
        }
        results.push((engine_hz, c.frames, c.node, c.frame_in_segment));
    }
    for r in &results {
        assert!((599..=600).contains(&r.1), "{r:?}");
    }
    let at600: Vec<_> = results.iter().filter(|r| r.1 == 600).map(|r| (r.2, r.3)).collect();
    assert!(at600.windows(2).all(|w| w[0] == w[1]), "{results:?}");
}

#[test]
fn replay_cursor_phases_follow_flags_and_trick_events() {
    let mut l = straight(1, [0.0; 3], 20, 2, 0.3);
    l.nodes[3].flags = node_flags::CROUCHED;
    l.nodes[5].flags = node_flags::AIRBORNE;
    l.nodes[6].flags = node_flags::AIRBORNE;
    l.nodes[6].event = node_events::START_TRICK;
    l.nodes[7].flags = node_flags::AIRBORNE;
    l.nodes[8].event = node_events::END_TRICK;
    l.nodes[10].event = node_events::START_TRICK;
    l.nodes[12].event = node_events::END_TRICK;
    l.nodes[14].flags = node_flags::OFF_BOARD;
    let ls = lines(vec![l]);
    let mut c = LineCursor::spawn(&ls, id(1), 0);
    let mut seen = Vec::new();
    let mut ev = Vec::new();
    for _ in 0..40 {
        c.step(&ls, &mut Decider::Stay, &mut ev);
        if let Some(s) = c.sample(&ls, 0.0) {
            if seen.last() != Some(&(s.phase)) {
                seen.push(s.phase);
            }
        }
    }
    use ReplayPhase::*;
    assert_eq!(seen, vec![Rolling, Crouched, Rolling, Air, AirTrick, Rolling, GroundTrick, Rolling, OffBoard, Rolling]);
    assert!(ev.iter().any(|e| matches!(e, CursorEvent::Node { event: node_events::START_TRICK, .. })));
}

#[test]
fn replay_cursor_finishes_at_the_line_end() {
    let ls = lines(vec![straight(1, [0.0; 3], 5, 2, 0.3)]);
    let mut c = LineCursor::spawn(&ls, id(1), 0);
    let mut ev = Vec::new();
    c.advance(8, &ls, &mut Decider::Stay, &mut ev);
    assert_eq!(c.node, 4);
    assert!(!c.finished);
    c.step(&ls, &mut Decider::Stay, &mut ev);
    assert!(c.finished);
    assert_eq!(ev.last(), Some(&CursorEvent::Finished));
    // The last sample stays at the end node.
    assert!((c.sample(&ls, 0.0).unwrap().position[2] - 1.2).abs() < 1e-5);
    // Unknown line: finished at once.
    assert!(LineCursor::spawn(&ls, id(9), 0).finished);
}

/// Line 1 runs along +Z; at node 10 a branch to line 2 at node 4. Line 2 runs beside it (x 0.05)
/// and turns 45 deg towards +x after its node 6.
fn branching() -> BTreeMap<[u8; 16], ReplayLine> {
    let mut a = straight(1, [0.0; 3], 40, 2, 0.3);
    a.groups.push(ReplayBranchGroup { node: 10, branches: vec![ReplayBranch { target: id(2), target_node: 4, weight: 0.5 }] });
    let mut b = straight(2, [0.05, 0.0, 1.8], 40, 2, 0.3);
    for (i, n) in b.nodes.iter_mut().enumerate().skip(7) {
        n.position[0] = 0.05 + 0.3 * (i as f32 - 6.0);
    }
    lines(vec![a, b])
}

#[test]
fn replay_branch_score_terms_match_the_code() {
    let ls = branching();
    let l = &ls[&id(1)];
    let players = [[0.0, 0.0, 3.0]];
    // At node 10 (z 3.0), forward +Z, next node straight ahead: angle 0; 400 x 8 = 3200;
    // the player stands on node 10: distance 0.
    let c = ctx([0.0, 0.0, 3.0], &players);
    assert_eq!(branch_score(l, 10, &c), Some(3200));
    // Online: only angle and speed.
    assert_eq!(branch_score(l, 10, &BranchContext { online: true, ..c }), Some(3200));
    // Player 20 m to the side of every node: 30 x 20 = 600.
    let far = [[20.0, 0.0, 3.0]];
    let s = branch_score(l, 10, &ctx([0.0, 0.0, 3.0], &far)).unwrap();
    assert!((3200 + 599..=3200 + 600).contains(&s), "{s}");
    // Beyond 50 m: capped at 1500.
    let very_far = [[500.0, 0.0, 0.0]];
    assert_eq!(branch_score(l, 10, &ctx([0.0, 0.0, 3.0], &very_far)), Some(3200 + 1500));
    // Two other AI skaters within 5 nodes of node 10 on this line: + 2 x 1024.
    let others = [(id(1), 6u32), (id(1), 15), (id(1), 16), (id(2), 10)];
    assert_eq!(branch_score(l, 10, &BranchContext { others: &others, ..c }), Some(3200 + 2048));
    // Flags 7: + 1000. Skill 2 against preferred 0: + 2 x 250 + 100.
    let mut l7 = l.clone();
    l7.flags = 7;
    l7.skill = 2;
    assert_eq!(branch_score(&l7, 10, &BranchContext { preferred_skill: 0, ..c }), Some(3200 + 1000 + 600));
    // 60 deg off the forward: rejected; 30 deg: 300 tenths.
    let side = BranchContext { forward: [1.0, 0.0, 0.0], ..c };
    assert_eq!(branch_score(l, 10, &side), None);
    let f30 = BranchContext { forward: [0.5, 0.0, 0.866_025], ..c };
    let s30 = branch_score(l, 10, &f30).unwrap();
    assert!((3200 + 299..=3200 + 300).contains(&s30), "{s30}");
    // Airborne or event node within one: rejected; at the last node (no next): rejected.
    let mut la = l.clone();
    la.nodes[11].flags = node_flags::AIRBORNE;
    assert_eq!(branch_score(&la, 10, &c), None);
    la.nodes[11].flags = 0;
    la.nodes[9].event = node_events::END_TRICK;
    assert_eq!(branch_score(&la, 10, &c), None);
    assert_eq!(branch_score(l, 39, &c), None);
}

#[test]
fn replay_branch_taken_only_when_it_scores_lower_and_mirrors_on_a_client() {
    let ls = branching();
    // Player on line 2's node 22 (4.85, 8.4): line 2 scores 95 (angle 9.5 deg to its next node)
    // + 0; the stay scores 0 + 30 x 4.85 = 145 -> branch.
    let players = [[4.85, 0.0, 8.4]];
    let run = |players: &[Vec3], in_use: &[[u8; 16]]| {
        let mut c = LineCursor::spawn(&ls, id(1), 0);
        let mut ev = Vec::new();
        for _ in 0..30 {
            let s = c.sample(&ls, 0.0).unwrap();
            let ctx = BranchContext { position: s.position, forward: [0.0, 0.0, 1.0], speed: 9.0, players, others: &[], in_use, preferred_skill: -1, online: false };
            c.step(&ls, &mut Decider::Decide(ctx), &mut ev);
        }
        (c, ev)
    };
    let (c, ev) = run(&players, &[]);
    let branches: Vec<_> = ev.iter().filter_map(|e| if let CursorEvent::Branch(b) = e { Some(b.clone()) } else { None }).collect();
    assert_eq!(branches.len(), 1);
    let b = &branches[0];
    assert_eq!((b.from_line, b.from_node, b.to_line), (id(1), 10, id(2)));
    // Rejoin: nearest of target nodes 1..=4 to (0, 0, 3.0): line 2 node 4 is z 3.0.
    assert_eq!(b.to_node, 4);
    assert_eq!(c.line, id(2));
    // Player beside line 1: the stay wins (ties also keep the stay).
    let (c1, ev1) = run(&[[0.0, 0.0, 9.0]], &[]);
    assert_eq!(c1.line, id(1));
    assert!(!ev1.iter().any(|e| matches!(e, CursorEvent::Branch(_))));
    // Target in use by another skater: skipped.
    let (c2, _) = run(&players, &[id(2)]);
    assert_eq!(c2.line, id(1));
    // A client mirrors the host's records without scoring and ends in the same state.
    let mut m = LineCursor::spawn(&ls, id(1), 0);
    let mut mev = Vec::new();
    m.advance(30, &ls, &mut Decider::Mirror(&branches), &mut mev);
    assert_eq!(m, c);
    assert_eq!(m.sample(&ls, 0.3), c.sample(&ls, 0.3));
}

#[test]
fn replay_orientation_decodes_x_y_z_w() {
    assert_eq!(decode_orientation(IDENTITY), [0.0, 0.0, 0.0, 1.0]);
    // 90 deg about +Y: (0, sin 45, 0, cos 45) turns +Z onto +X.
    let b = (0.707_107f32 * 127.0 + 128.0).round() as u8;
    let q = decode_orientation([128, b, 128, b]);
    let f = rotate(q, [0.0, 0.0, 1.0]);
    assert!((f[0] - 1.0).abs() < 0.01 && f[2].abs() < 0.01, "{f:?}");
    assert_eq!(decode_orientation([128; 4]), [0.0, 0.0, 0.0, 1.0]);
}
