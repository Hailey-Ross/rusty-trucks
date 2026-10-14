//! Retail's AI path projection (research `.local/research/npc/b110-ai-path-projection.md`): a physics AI skater's
//! place on its recorded line follows the skater, not the recording's clock. Per tick (`sub_8246D560`):
//! 1. the nearest node in a forward window from the search start (`sub_8246D3C0` -> `sub_82455BB0`, start..=start+4,
//!    squared 3D distance; the start only grows);
//! 2. the projection onto the two segments around it (`sub_824563D8`), kept or replaced against cursor B with a
//!    0.5 hysteresis, and clamped forward to A + 0.25 of A->B when it falls back (`sub_8246C930`): cursor A;
//! 3. distance to A and the recorded lane test (`sub_8246CCC8`, node widths x 1/50);
//! 4. off path: the rejoin commit within 0.5 m or inside the lane (`sub_8246E2B8`);
//! 5. cursor B = A + the speed-shape ratio in recorded frames (`sub_8246E3B8` -> `sub_82456628`);
//! 6. on path is dropped beyond 1.0 m outside the lane (`sub_8246DB50`).
//!
//! Positions are (node, t): t in [0, 1] along the segment node -> node + 1. Pure and deterministic (no clock).
//! NOT RETAIL YET: the "lost" hand-over to the NavMesh (`sub_82468DD8` / `sub_8246FE38`), the node-change handler
//! (`sub_8246BEE0`), the progress tracker (`sub_82468240`); which body point retail projects (`sub_82467FC8`) is open
//! (ours: the deck).

use super::replay::{ReplayLine, WIDTH_SCALE};
use super::Vec3;

/// The projection's values, retail defaults ([code] addresses in b110; the two `tunable` ones are live tunables).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathTuning {
    /// Forward search window from the search start, nodes (`sub_82455BB0`: start..=start+4).
    pub window: u32,
    /// Keep B unless the projection is closer by this much (squared distances, `sub_8246C930`).
    pub hysteresis: f32,
    /// A fallen-back candidate is clamped to A + this fraction of A->B (`sub_8246C930`).
    pub clamp_fraction: f32,
    /// ... when the clamped point is within this horizontal / vertical distance of the skater.
    pub clamp_horizontal: f32,
    pub clamp_vertical: f32,
    /// Rejoin commit distance (tunable `84FD`, 0.5 m).
    pub commit: f32,
    /// On-path drop distance (tunable `996C`, 1.0 m).
    pub drop: f32,
    /// The start gate latches once A is past this node (`pc+937`).
    pub start_node: u32,
}

impl Default for PathTuning {
    fn default() -> Self {
        Self { window: 4, hysteresis: 0.5, clamp_fraction: 0.25, clamp_horizontal: 1.0, clamp_vertical: 5.0, commit: 0.5, drop: 1.0, start_node: 3 }
    }
}

/// One AI skater's projection state (`pc+608..+944`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathFollow {
    /// `pc+816`: the nearest-node search start; only grows.
    pub search_start: u32,
    /// Cursor A (the skater's projection) and cursor B (A plus one shaped recorded tick).
    pub a: (u32, f32),
    pub b: (u32, f32),
    /// `pc+764/768`: the last raw projection.
    pub last: (u32, f32),
    /// `pc+920`: on path. `pc+937`: past the start nodes.
    pub on_path: bool,
    pub past_start: bool,
    /// `pc+828`: distance skater -> A; `pc+932`: inside the recorded lane.
    pub dist: f32,
    pub inside_lane: bool,
    /// `pc+820`: the B node of the previous tick (events up to it are handled).
    pub handled: u32,
}

impl PathFollow {
    /// A skater placed on `node` (spawn or respawn): on the path, A = B = the node.
    pub fn start(node: u32, tuning: &PathTuning) -> Self {
        Self { search_start: node, a: (node, 0.0), b: (node, 0.0), last: (node, 0.0), on_path: true, past_start: node > tuning.start_node, dist: 0.0, inside_lane: true, handled: node }
    }

    /// One tick at skater position `pos`; `ratio` = shaped speed / recorded speed at A (1.0 unshaped).
    pub fn tick(&mut self, line: &ReplayLine, pos: Vec3, ratio: f32, tuning: &PathTuning) {
        if line.nodes.len() < 2 {
            return;
        }
        let n = nearest_forward(line, pos, self.search_start, tuning.window);
        self.search_start = n;
        // Projection -> A.
        let proj = project(line, n, pos);
        let p = point(line, proj);
        let mut cand = proj;
        if dist2(point(line, self.b), pos) < dist2(p, pos) + tuning.hysteresis {
            cand = self.b;
        }
        let b_ahead = self.b.0 > cand.0 || (self.b.0 == cand.0 && self.b.1 > cand.1);
        if b_ahead && (self.a.0 > 0 || self.a.1 > 0.1) {
            let dab = along(line, self.a, self.b);
            if dab > 0.0 && along(line, self.a, cand) < tuning.clamp_fraction * dab {
                let q = advance(line, self.a, tuning.clamp_fraction * dab);
                let qp = point(line, q);
                let horizontal = (qp[0] - pos[0]).powi(2) + (qp[2] - pos[2]).powi(2);
                if horizontal < tuning.clamp_horizontal * tuning.clamp_horizontal && (qp[1] - pos[1]).abs() < tuning.clamp_vertical {
                    cand = q;
                }
            }
        }
        self.last = proj;
        self.a = cand;
        self.search_start = self.search_start.max(self.a.0);
        // Distance and lane.
        let ap = point(line, self.a);
        let offset = [pos[0] - ap[0], pos[1] - ap[1], pos[2] - ap[2]];
        self.dist = dist2(ap, pos).sqrt();
        self.inside_lane = if self.dist <= 0.0001 {
            true
        } else {
            let next = line.nodes.get(self.a.0 as usize + 1).unwrap_or(&line.nodes[self.a.0 as usize]);
            let dir = next.step;
            // Right of the direction (dir x up); width[0] = left, width[1] = right.
            let right = [dir[2], 0.0, -dir[0]];
            let side = usize::from(offset[0] * right[0] + offset[2] * right[2] >= 0.0);
            let w = f32::from(line.nodes[self.a.0 as usize].width[side]) * WIDTH_SCALE;
            self.dist < w
        };
        // Off path: rejoin commit.
        if !self.on_path && (self.a.0 > 0 || self.a.1 > 0.0001) && (self.inside_lane || self.dist < tuning.commit) {
            self.b = self.a;
            self.on_path = true;
        }
        if self.on_path && !self.past_start {
            self.past_start = self.a.0 > tuning.start_node || (self.a.0 == tuning.start_node && self.a.1 > 0.0001);
        }
        // Cursor B.
        self.handled = self.b.0;
        self.b = if ratio > 1e-5 { advance(line, self.a, ratio) } else { self.a };
        // On-path drop.
        if self.on_path && !self.inside_lane && self.dist > tuning.drop {
            self.on_path = false;
        }
    }

    /// Recorded frames from cursor (`node`, `frame_in_segment`) forward to B (0 when B is not ahead).
    pub fn frames_to_b(&self, line: &ReplayLine, node: u32, frame_in_segment: u32) -> u64 {
        let seg = |i: u32| line.segment_frames(i) as f32;
        let from = (node, if seg(node) > 0.0 { frame_in_segment as f32 / seg(node) } else { 0.0 });
        along(line, from, self.b).max(0.0).floor() as u64
    }
}

/// Point on the line at (node, t).
pub fn point(line: &ReplayLine, c: (u32, f32)) -> Vec3 {
    let i = (c.0 as usize).min(line.nodes.len() - 1);
    let a = line.nodes[i].position;
    match line.nodes.get(i + 1) {
        Some(b) => std::array::from_fn(|k| a[k] + (b.position[k] - a[k]) * c.1),
        None => a,
    }
}

/// Recorded frames from `from` to `to` along the line (negative when `to` is behind).
pub fn along(line: &ReplayLine, from: (u32, f32), to: (u32, f32)) -> f32 {
    let pos = |c: (u32, f32)| -> f32 { (0..c.0).map(|i| line.segment_frames(i) as f32).sum::<f32>() + c.1 * line.segment_frames(c.0) as f32 };
    pos(to) - pos(from)
}

/// `sub_82456628`: `amount` recorded frames ahead of `c`, clamped at the last node.
pub fn advance(line: &ReplayLine, c: (u32, f32), amount: f32) -> (u32, f32) {
    let last = line.nodes.len().saturating_sub(1) as u32;
    let (mut i, mut t) = c;
    let mut left = amount.max(0.0);
    for _ in 0..4096 {
        if i >= last {
            return (last, 0.0);
        }
        let seg = line.segment_frames(i) as f32;
        let rest = (1.0 - t) * seg;
        if seg > 0.0 && left < rest {
            return (i, t + left / seg);
        }
        left -= rest.max(0.0);
        i += 1;
        t = 0.0;
        if left <= 0.0 {
            return (i, 0.0);
        }
    }
    (i, t)
}

/// `sub_82455BB0`: the nearest node in start..=start+window (squared 3D distance; forward only).
pub fn nearest_forward(line: &ReplayLine, pos: Vec3, start: u32, window: u32) -> u32 {
    let last = line.nodes.len().saturating_sub(1) as u32;
    let start = start.min(last);
    (start..=(start + window).min(last)).min_by(|&a, &b| dist2(line.nodes[a as usize].position, pos).total_cmp(&dist2(line.nodes[b as usize].position, pos))).unwrap_or(start)
}

/// `sub_824563D8`: project onto the segments around node `n` (n-1 and n; segment 0 at the start, n-1 at the end).
pub fn project(line: &ReplayLine, n: u32, pos: Vec3) -> (u32, f32) {
    let last = line.nodes.len().saturating_sub(1) as u32;
    let seg_t = |i: u32| -> f32 {
        let a = line.nodes[i as usize].position;
        let b = line.nodes[i as usize + 1].position;
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if l2 <= 1e-12 {
            return 1.0;
        }
        ((pos[0] - a[0]) * d[0] + (pos[1] - a[1]) * d[1] + (pos[2] - a[2]) * d[2]) / l2
    };
    if n == 0 {
        return (0, seg_t(0).clamp(0.0, 1.0));
    }
    if n >= last {
        return (last - 1, seg_t(last - 1).clamp(0.0, 1.0));
    }
    let (tp, tn) = (seg_t(n - 1), seg_t(n));
    let inside = |t: f32| (0.0..=1.0).contains(&t);
    match (inside(tp), inside(tn)) {
        (true, true) => {
            let (pp, pn) = (point(line, (n - 1, tp)), point(line, (n, tn)));
            if dist2(pp, pos) <= dist2(pn, pos) { (n - 1, tp) } else { (n, tn) }
        }
        (true, false) => (n - 1, tp),
        (false, true) => (n, tn),
        (false, false) if tp < 0.0 => (n - 1, 0.0),
        (false, false) if tn > 1.0 => (n, 1.0),
        _ => (n, 0.0),
    }
}

fn dist2(a: Vec3, b: Vec3) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::living_world::replay::ReplayNode;

    /// A straight line along +z: nodes every 1 m, 10 frames per segment (0.1 m per frame).
    fn line(n: usize) -> ReplayLine {
        let node = |i: usize| ReplayNode { position: [0.0, 0.0, i as f32], step: [0.0, 0.0, 0.1], board: [128; 4], skater: [128; 4], frames: if i == 0 { 0 } else { 10 }, event: 0, flags: 0, jump: None, width: [25, 25] };
        ReplayLine { id: [0; 16], flags: 0, skill: -1, nodes: (0..n).map(node).collect(), jumps: Vec::new(), groups: Vec::new() }
    }

    #[test]
    fn b_follows_the_skater_not_a_clock() {
        let l = line(40);
        let t = PathTuning::default();
        let mut f = PathFollow::start(0, &t);
        // The skater rolls at half the recorded speed (0.05 m per tick): A stays beside it (the 0.5 hysteresis
        // lets it lead by at most ~0.7 m), B one recorded frame ahead of A; a clock would run away from it.
        for k in 0..200 {
            let z = 0.05 * k as f32;
            f.tick(&l, [0.1, 0.0, z], 1.0, &t);
            let a = point(&l, f.a)[2];
            assert!(a >= z - 0.05 && a <= z + 0.75, "tick {k}: A at {a}, skater at {z}");
            assert!((along(&l, f.a, f.b) - 1.0).abs() < 1e-3);
        }
        assert!(f.on_path);
    }

    #[test]
    fn off_the_lane_beyond_1_m_drops_the_path_and_rejoins_within_0_5_m() {
        let l = line(40);
        let t = PathTuning::default();
        let mut f = PathFollow::start(0, &t);
        let mut z = 0.0;
        let mut step = |f: &mut PathFollow, x: f32| {
            z += 0.1;
            f.tick(&l, [x, 0.0, z], 1.0, &t);
        };
        for _ in 0..30 {
            step(&mut f, 0.0);
        }
        // Lane 0.5 m each side (25 / 50): 1.2 m to the side is outside and beyond 1 m.
        step(&mut f, 1.2);
        assert!(!f.on_path);
        step(&mut f, 0.8);
        assert!(!f.on_path, "0.8 m: outside the lane and over the 0.5 m commit");
        for _ in 0..3 {
            step(&mut f, 0.2);
        }
        assert!(f.on_path, "back inside the lane: rejoined ({f:?})");
    }

    #[test]
    fn the_search_only_moves_forward() {
        let l = line(20);
        let t = PathTuning::default();
        let mut f = PathFollow::start(10, &t);
        f.tick(&l, [0.0, 0.0, 2.0], 1.0, &t);
        assert!(f.search_start >= 10 && f.a.0 >= 9, "{:?}", f);
    }

    #[test]
    fn frames_to_b_counts_recorded_frames_from_the_cursor() {
        let l = line(20);
        let t = PathTuning::default();
        let mut f = PathFollow::start(0, &t);
        f.tick(&l, [0.0, 0.0, 3.0], 1.0, &t);
        // Cursor at node 1 frame 0: B at about 3.1 m = 21 frames on.
        assert_eq!(f.frames_to_b(&l, 1, 0), 21);
        assert_eq!(f.frames_to_b(&l, 5, 0), 0, "B behind the cursor");
    }
}
