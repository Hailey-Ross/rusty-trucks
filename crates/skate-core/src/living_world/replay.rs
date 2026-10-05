//! NPC skaters, replay tier: the line cursor (doc 26, milestone 3).
//!
//! Retail runs every NPC skater as a full skater steered along a recorded human line
//! (`AIController` + `PathController`, ctor `sub_824685F0`, vtable `0x822FBA1C`). The replay tier
//! is the engine's cheap stand-in until the simulated tier exists: the NPC follows its line
//! kinematically, node by node, at the recording rate (60 Hz, [data]: node distance / frames =
//! per-tick displacement over 389k nodes). What is ported 1:1 from the code is the **branch
//! choice** at a node that carries a branch group ([code] `sub_8246BEE0`, chooser `sub_8246C1C8`,
//! candidate score `sub_8246C230`):
//!
//! - candidates: index 0 = stay on the current line at the current node, then every branch target
//!   line that exists, is not in use by another skater (`sub_82458860`) and is valid
//!   (`sub_82456970`);
//! - each candidate is rejected (score -1, never chosen) when the line has no node after the
//!   candidate node (`sub_8246C3C0`), when the direction from the skater to that next node is 50 deg
//!   or more off the skater's forward (0.872665 rad, `0x822F91B0`), or when a node within one of
//!   the candidate node is airborne (flag 0x04) or carries an event (`sub_8246C4F8`; skipped at
//!   node 0); otherwise
//! - score = |angle| x 572.958 (`0x822F9354`, tenths of a degree) + 400 x speed (`0x822F95F0` =
//!   -400, the same for every candidate) + offline: 1024 x the other AI skaters on the same line
//!   within 5 nodes (`sub_82456A38`) + min(30 x the nearest distance from the player to every
//!   second node from the candidate node on, 1500) (`sub_8246C5D8`, `0x820D4924` = 30) + 1000 when
//!   the line's flags have bits 0, 1 and 2 all set + the skill term (|path skill - preferred| x 250 +
//!   100 when both are set and differ);
//! - the lowest score wins, the first on ties; when every candidate is rejected or the stay wins,
//!   nothing changes. No random draw is involved. The branch record's f32 is not read there.
//! - a taken branch starts at the target node nearest to the skater among `target - 3 ..= target`
//!   (`sub_82455BB0`).
//!
//! Replay-tier simplifications (documented in doc 26; the simulated tier replaces them):
//! positions, orientation and timing come straight from the recording instead of the steering
//! (`AIPhysicsInput`); the branch is evaluated once when the cursor reaches the group's node
//! (retail evaluates while the controller sits on it); the obstacle-list rejection of
//! `sub_8246C4F8` is not modelled (no obstacles yet); what retail does at the very end of a line
//! with no branch taken is not decoded (parked): the cursor reports [`CursorEvent::Finished`] and
//! the engine despawns the NPC (the per-skater check `sub_8245A9B8` also despawns NPCs whose
//! controller reports a finished state).
//!
//! Multiplayer: between branches the cursor is a pure function of (line, start node, frames);
//! a branch decision is a record ([`BranchRecord`]) a client mirrors ([`LineCursor::step`] with a
//! mirroring decider), so a client reproduces the host's NPC from its spawn record, the frame
//! count and the branch records alone.

use super::Vec3;
use std::collections::BTreeMap;

/// Recording rate of the lines [data].
pub const RECORDING_HZ: f64 = 60.0;

/// Node flag bits (`m_IsBoardFlipped`, `m_IsCrouched`, `m_IsAirborne`, `m_IsOffBoard`; bit order
/// as in `skate-data::aipath::node_flags`).
pub mod node_flags {
    pub const BOARD_FLIPPED: u8 = 1 << 0;
    pub const CROUCHED: u8 = 1 << 1;
    pub const AIRBORNE: u8 = 1 << 2;
    pub const OFF_BOARD: u8 = 1 << 3;
}

/// Node events (`m_EventType`).
pub mod node_events {
    pub const NONE: u8 = 0;
    pub const START_TRICK: u8 = 1;
    pub const END_TRICK: u8 = 2;
    pub const INCIDENTAL_AIR: u8 = 4;
}

/// Retail constants of the branch choice [code].
pub mod retail {
    /// `sub_8246C3C0`: reject at or above this angle (rad), `0x822F91B0`.
    pub const BRANCH_MAX_ANGLE: f32 = 0.872_665;
    /// Angle to score (tenths of a degree), `0x822F9354`.
    pub const BRANCH_ANGLE_SCALE: f32 = 572.958;
    /// Speed factor, `0x822F95F0` (-400, subtracted).
    pub const BRANCH_SPEED_SCALE: f32 = -400.0;
    /// Per other AI skater on the same line within [`BRANCH_CROWD_NODES`] (`rlwinm r29,r3,10`).
    pub const BRANCH_CROWD: i64 = 1024;
    pub const BRANCH_CROWD_NODES: u32 = 5;
    /// Player distance term: x 30 (`0x820D4924`), capped at 1500.
    pub const BRANCH_NEAR_SCALE: f32 = 30.0;
    pub const BRANCH_NEAR_CAP: i64 = 1500;
    /// Lines with flags bits 0..2 all set.
    pub const BRANCH_ALL_TYPES: i64 = 1000;
    /// Skill difference: x 250 + 100.
    pub const BRANCH_SKILL_SCALE: i64 = 250;
    pub const BRANCH_SKILL_BASE: i64 = 100;
    /// A taken branch searches `target - 3 ..= target` for the nearest node.
    pub const BRANCH_REJOIN_BACK: u32 = 3;
}

/// One recorded node (`tAIPathNode`, 44 bytes on the disc).
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayNode {
    pub position: Vec3,
    /// Board and skater orientation, 4 biased bytes each (`(b - 128) / 127`, x y z w).
    pub board: [u8; 4],
    pub skater: [u8; 4],
    /// 60 Hz frames since the previous node.
    pub frames: u8,
    pub event: u8,
    pub flags: u8,
    /// Index into [`ReplayLine::jumps`].
    pub jump: Option<u32>,
}

/// A recorded jump / trick slot (`tAIPathNodeExtData`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayJump {
    pub start_position: Vec3,
    pub start_velocity: Vec3,
    pub offset: Vec3,
    pub trick: i16,
    pub spins: i8,
    pub flags: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayBranch {
    pub target: [u8; 16],
    pub target_node: u32,
    /// The disc's f32 (0..1); not read by the branch choice [code].
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplayBranchGroup {
    pub node: u32,
    pub branches: Vec<ReplayBranch>,
}

/// One recorded line as the cursor needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayLine {
    pub id: [u8; 16],
    pub flags: u32,
    /// `m_SkillLevel` (path +80); -1 = none.
    pub skill: i32,
    pub nodes: Vec<ReplayNode>,
    pub jumps: Vec<ReplayJump>,
    pub groups: Vec<ReplayBranchGroup>,
}

impl ReplayLine {
    pub fn group_at(&self, node: u32) -> Option<&ReplayBranchGroup> {
        self.groups.iter().find(|g| g.node == node)
    }
    /// 60 Hz frames from node `i` to node `i + 1`.
    pub fn segment_frames(&self, i: u32) -> u32 {
        self.nodes.get(i as usize + 1).map_or(0, |n| u32::from(n.frames))
    }
    pub fn duration_frames(&self) -> u64 {
        self.nodes.iter().skip(1).map(|n| u64::from(n.frames)).sum()
    }
    /// Whether a trick span (START_TRICK without its END_TRICK yet) is open at `node`.
    pub fn trick_open_at(&self, node: u32) -> bool {
        self.nodes[..=(node as usize).min(self.nodes.len().saturating_sub(1))]
            .iter()
            .rev()
            .find_map(|n| match n.event {
                node_events::START_TRICK => Some(true),
                node_events::END_TRICK => Some(false),
                _ => None,
            })
            .unwrap_or(false)
    }
}

/// Where the cursor finds lines by id (the loaded district, a mod's lines).
pub trait LineSource {
    fn line(&self, id: &[u8; 16]) -> Option<&ReplayLine>;
}

impl LineSource for BTreeMap<[u8; 16], ReplayLine> {
    fn line(&self, id: &[u8; 16]) -> Option<&ReplayLine> {
        self.get(id)
    }
}

/// Decode a node orientation: x, y, z, w with +Z = forward ([data]: the skater quaternion turns
/// +Z onto the travel direction within 25 deg on 81 % of moving nodes in this order; other
/// orders and axes score far lower).
pub fn decode_orientation(raw: [u8; 4]) -> [f32; 4] {
    let q = raw.map(|b| (f32::from(b) - 128.0) / 127.0);
    let n = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    if n > 1e-6 {
        q.map(|c| c / n)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Rotate `v` by the unit quaternion `q` (x, y, z, w).
pub fn rotate(q: [f32; 4], v: Vec3) -> Vec3 {
    let [x, y, z, w] = q;
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}

fn nlerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    let s = if dot < 0.0 { -1.0 } else { 1.0 };
    let q: [f32; 4] = std::array::from_fn(|i| a[i] + (s * b[i] - a[i]) * t);
    let n = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    if n > 1e-6 {
        q.map(|c| c / n)
    } else {
        a
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// What the NPC is doing at a node (puppet animation, audio).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReplayPhase {
    Rolling,
    Crouched,
    Air,
    /// Airborne inside a trick span (a flip / grab / spin slot).
    AirTrick,
    /// On the ground inside a trick span: a grind, slide or manual (the recording does not say
    /// which; open until the simulated tier performs the trick).
    GroundTrick,
    OffBoard,
}

impl ReplayPhase {
    pub fn of(flags: u8, trick_open: bool) -> Self {
        if flags & node_flags::OFF_BOARD != 0 {
            ReplayPhase::OffBoard
        } else if flags & node_flags::AIRBORNE != 0 {
            if trick_open {
                ReplayPhase::AirTrick
            } else {
                ReplayPhase::Air
            }
        } else if trick_open {
            ReplayPhase::GroundTrick
        } else if flags & node_flags::CROUCHED != 0 {
            ReplayPhase::Crouched
        } else {
            ReplayPhase::Rolling
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ReplayPhase::Rolling => "rolling",
            ReplayPhase::Crouched => "crouched",
            ReplayPhase::Air => "air",
            ReplayPhase::AirTrick => "air_trick",
            ReplayPhase::GroundTrick => "ground_trick",
            ReplayPhase::OffBoard => "off_board",
        }
    }
}

/// The NPC's state at one instant.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplaySample {
    pub line: [u8; 16],
    pub node: u32,
    pub position: Vec3,
    /// m/s, from the current segment.
    pub velocity: Vec3,
    /// Yaw about +Y (0 = +Z, the engine's forward).
    pub heading: f32,
    pub board: [f32; 4],
    pub skater: [f32; 4],
    pub flags: u8,
    pub phase: ReplayPhase,
    /// The recorded jump of the current node, if any.
    pub jump: Option<u32>,
    /// Frames spent in the current phase (60 Hz), for clip time.
    pub phase_frames: u64,
}

/// A branch decision (host side) or the record a client mirrors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchRecord {
    /// Cursor frame (60 Hz frames since spawn) of the decision.
    pub frame: u64,
    pub from_line: [u8; 16],
    pub from_node: u32,
    pub to_line: [u8; 16],
    pub to_node: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CursorEvent {
    /// The cursor reached a node (event and flags of that node; mods and speech read these).
    Node { line: [u8; 16], node: u32, event: u8, flags: u8 },
    Branch(BranchRecord),
    /// End of the line with no branch taken (parked: retail behaviour not decoded).
    Finished,
}

/// What the branch score reads besides the lines [code `sub_8246C230`].
#[derive(Clone, Copy, Debug)]
pub struct BranchContext<'a> {
    /// The skater's position and forward (the NPC itself).
    pub position: Vec3,
    pub forward: Vec3,
    pub speed: f32,
    /// The players (the 1500 term uses the nearest; one observer = retail's local player).
    pub players: &'a [Vec3],
    /// Other AI skaters' (line, node) (the 1024 term).
    pub others: &'a [([u8; 16], u32)],
    /// Lines in use by other skaters (`sub_82458860`).
    pub in_use: &'a [[u8; 16]],
    /// The controller's preferred skill level (+164, -1 = none).
    pub preferred_skill: i32,
    /// Online (mgr+608): the crowd and player terms are skipped.
    pub online: bool,
}

/// Signed angle from `a` to `b` about +Y (radians), like `sub_824536C8` with the up axis.
fn yaw_angle(a: Vec3, b: Vec3) -> f32 {
    let (ax, az, bx, bz) = (a[0], a[2], b[0], b[2]);
    let cross = az * bx - ax * bz;
    let dot = ax * bx + az * bz;
    cross.atan2(dot)
}

/// Score one (line, node) candidate; `None` = rejected. Lowest wins.
pub fn branch_score(line: &ReplayLine, node: u32, ctx: &BranchContext) -> Option<i64> {
    let count = line.nodes.len() as u32;
    // sub_8246C4F8: airborne or event nodes within one of the candidate node (skipped at node 0).
    if node > 0 {
        for i in node - 1..=node + 1 {
            if let Some(n) = line.nodes.get(i as usize) {
                if n.flags & node_flags::AIRBORNE != 0 || n.event != 0 {
                    return None;
                }
            }
        }
    }
    // sub_8246C3C0: the next node must exist and lie within 50 deg of the forward.
    let next = line.nodes.get(node as usize + 1)?;
    let angle = yaw_angle(ctx.forward, sub(next.position, ctx.position)).abs();
    if !(angle < retail::BRANCH_MAX_ANGLE) {
        return None;
    }
    let mut score = (angle * retail::BRANCH_ANGLE_SCALE) as i64;
    score -= (ctx.speed * retail::BRANCH_SPEED_SCALE) as i64;
    if !ctx.online {
        let crowd = ctx.others.iter().filter(|(l, n)| *l == line.id && n.abs_diff(node) <= retail::BRANCH_CROWD_NODES).count() as i64;
        score += crowd * retail::BRANCH_CROWD;
        // sub_8246C5D8: every second node from the candidate node on.
        let mut best = f32::MAX;
        let mut i = node;
        while i < count {
            let p = line.nodes[i as usize].position;
            for r in ctx.players {
                let d = sub(p, *r);
                best = best.min(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            }
            i += 2;
        }
        let near = if best == f32::MAX { retail::BRANCH_NEAR_CAP } else { ((best.sqrt() * retail::BRANCH_NEAR_SCALE) as i64).min(retail::BRANCH_NEAR_CAP) };
        score += near;
    }
    if line.flags & 7 == 7 {
        score += retail::BRANCH_ALL_TYPES;
    }
    if ctx.preferred_skill != -1 && line.skill != -1 && line.skill != ctx.preferred_skill {
        score += i64::from((line.skill - ctx.preferred_skill).abs()) * retail::BRANCH_SKILL_SCALE + retail::BRANCH_SKILL_BASE;
    }
    Some(score)
}

/// The nearest node to `position` among `target - 3 ..= target` (`sub_82455BB0`).
pub fn rejoin_node(line: &ReplayLine, target: u32, position: Vec3) -> u32 {
    let last = line.nodes.len().saturating_sub(1) as u32;
    let start = target.saturating_sub(retail::BRANCH_REJOIN_BACK).min(last);
    let end = target.min(last);
    if end <= start {
        return start;
    }
    let mut best = (start, f32::MAX);
    for i in start..=end {
        let d = sub(line.nodes[i as usize].position, position);
        let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if d2 < best.1 {
            best = (i, d2);
        }
    }
    best.0
}

/// The branch choice at a group node (`sub_8246BEE0`): `Some((line, node))` when a branch target
/// wins, `None` when the stay wins or every candidate is rejected.
pub fn choose_branch(lines: &dyn LineSource, current: &ReplayLine, node: u32, group: &ReplayBranchGroup, ctx: &BranchContext) -> Option<([u8; 16], u32)> {
    let mut best: Option<(usize, i64)> = None;
    let mut candidates: Vec<(&ReplayLine, u32)> = vec![(current, node)];
    for b in &group.branches {
        if b.target == current.id || ctx.in_use.contains(&b.target) {
            continue;
        }
        if let Some(l) = lines.line(&b.target) {
            if (b.target_node as usize) < l.nodes.len() {
                candidates.push((l, b.target_node));
            }
        }
    }
    for (i, (l, n)) in candidates.iter().enumerate() {
        if let Some(s) = branch_score(l, *n, ctx) {
            if best.is_none_or(|(_, b)| s < b) {
                best = Some((i, s));
            }
        }
    }
    match best {
        Some((i, _)) if i > 0 => {
            let (l, n) = candidates[i];
            Some((l.id, rejoin_node(l, n, ctx.position)))
        }
        _ => None,
    }
}

/// Follows one line at the recording rate. Copyable state, no references: a client rebuilds it
/// from the spawn record (line, node 0) and the frame count.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCursor {
    pub line: [u8; 16],
    pub node: u32,
    /// 60 Hz frames into the segment `node -> node + 1`.
    pub frame_in_segment: u32,
    /// 60 Hz frames since spawn.
    pub frames: u64,
    pub finished: bool,
    trick_open: bool,
    phase: Option<ReplayPhase>,
    phase_since: u64,
}

/// How a cursor takes branches: the host decides, a client mirrors records.
pub enum Decider<'a> {
    /// Retail branch choice with this context; decisions are appended to the cursor events.
    Decide(BranchContext<'a>),
    /// Apply recorded decisions (matched by frame and from-node); never decides.
    Mirror(&'a [BranchRecord]),
    /// Never branch (tests, a mod that pins a line).
    Stay,
}

impl LineCursor {
    pub fn new(line: [u8; 16], node: u32) -> Self {
        Self { line, node, frame_in_segment: 0, frames: 0, finished: false, trick_open: false, phase: None, phase_since: 0 }
    }

    /// Spawn on a line at a node (retail spawns at node 0, `sub_8245DA78`).
    pub fn spawn(lines: &dyn LineSource, line: [u8; 16], node: u32) -> Self {
        let mut c = Self::new(line, node);
        if let Some(l) = lines.line(&line) {
            c.trick_open = l.trick_open_at(node);
            c.phase = l.nodes.get(node as usize).map(|n| ReplayPhase::of(n.flags, c.trick_open));
        } else {
            c.finished = true;
        }
        c
    }

    /// Advance one 60 Hz frame.
    pub fn step(&mut self, lines: &dyn LineSource, decider: &mut Decider, out: &mut Vec<CursorEvent>) {
        if self.finished {
            return;
        }
        let Some(mut line) = lines.line(&self.line) else {
            self.finished = true;
            out.push(CursorEvent::Finished);
            return;
        };
        if self.node as usize + 1 >= line.nodes.len() {
            self.finished = true;
            out.push(CursorEvent::Finished);
            return;
        }
        self.frames += 1;
        self.frame_in_segment += 1;
        // Bounded: zero-frame segments are crossed at once, a branch may land on one.
        for _ in 0..256 {
            if self.node as usize + 1 >= line.nodes.len() {
                break;
            }
            let seg = line.segment_frames(self.node);
            if self.frame_in_segment < seg {
                break;
            }
            self.frame_in_segment -= seg;
            self.node += 1;
            let n = &line.nodes[self.node as usize];
            match n.event {
                node_events::START_TRICK => self.trick_open = true,
                node_events::END_TRICK => self.trick_open = false,
                _ => {}
            }
            out.push(CursorEvent::Node { line: self.line, node: self.node, event: n.event, flags: n.flags });
            if let Some(group) = line.group_at(self.node) {
                let choice = match decider {
                    Decider::Decide(ctx) => {
                        // The replay skater stands on the node, moving along the segment it
                        // just rode.
                        let mut here = *ctx;
                        here.position = line.nodes[self.node as usize].position;
                        let v = sub(here.position, line.nodes[self.node as usize - 1].position);
                        if v[0].hypot(v[2]) > 1e-4 {
                            here.forward = v;
                        }
                        choose_branch(lines, line, self.node, group, &here)
                    }
                    Decider::Mirror(records) => records.iter().find(|r| r.frame == self.frames && r.from_line == self.line && r.from_node == self.node).map(|r| (r.to_line, r.to_node)),
                    Decider::Stay => None,
                };
                if let Some((to_line, to_node)) = choice {
                    if let Some(next) = lines.line(&to_line) {
                        out.push(CursorEvent::Branch(BranchRecord { frame: self.frames, from_line: self.line, from_node: self.node, to_line, to_node }));
                        self.line = to_line;
                        self.node = to_node;
                        self.frame_in_segment = 0;
                        self.trick_open = next.trick_open_at(to_node);
                        line = next;
                    }
                }
            }
        }
        if let Some(n) = line.nodes.get(self.node as usize) {
            let phase = ReplayPhase::of(n.flags, self.trick_open);
            if self.phase != Some(phase) {
                self.phase = Some(phase);
                self.phase_since = self.frames;
            }
        }
    }

    /// Advance `frames` 60 Hz frames.
    pub fn advance(&mut self, frames: u32, lines: &dyn LineSource, decider: &mut Decider, out: &mut Vec<CursorEvent>) {
        for _ in 0..frames {
            self.step(lines, decider, out);
        }
    }

    /// The state now, `alpha` (0..1) of the way to the next 60 Hz frame (render interpolation).
    pub fn sample(&self, lines: &dyn LineSource, alpha: f32) -> Option<ReplaySample> {
        let line = lines.line(&self.line)?;
        let i = self.node as usize;
        let a = line.nodes.get(i)?;
        let (b, seg) = match line.nodes.get(i + 1) {
            Some(b) if !self.finished => (b, line.segment_frames(self.node)),
            _ => (a, 0),
        };
        let t = if seg > 0 { ((self.frame_in_segment as f32 + alpha.clamp(0.0, 1.0)) / seg as f32).min(1.0) } else { 0.0 };
        let position = std::array::from_fn(|k| a.position[k] + (b.position[k] - a.position[k]) * t);
        let velocity = segment_velocity(line, self.node);
        let skater = nlerp(decode_orientation(a.skater), decode_orientation(b.skater), t);
        let board = nlerp(decode_orientation(a.board), decode_orientation(b.board), t);
        let heading = if velocity[0].hypot(velocity[2]) > 0.05 {
            velocity[0].atan2(velocity[2])
        } else {
            let f = rotate(skater, [0.0, 0.0, 1.0]);
            f[0].atan2(f[2])
        };
        let phase = self.phase.unwrap_or_else(|| ReplayPhase::of(a.flags, self.trick_open));
        Some(ReplaySample {
            line: self.line,
            node: self.node,
            position,
            velocity,
            heading,
            board,
            skater,
            flags: a.flags,
            phase,
            jump: a.jump,
            phase_frames: self.frames - self.phase_since.min(self.frames),
        })
    }
}

/// Velocity of the segment from `node` (or the last one before it when it has no duration), m/s.
pub fn segment_velocity(line: &ReplayLine, node: u32) -> Vec3 {
    let mut i = node as usize;
    loop {
        if i + 1 < line.nodes.len() {
            let f = line.nodes[i + 1].frames;
            if f > 0 {
                let d = sub(line.nodes[i + 1].position, line.nodes[i].position);
                let s = RECORDING_HZ as f32 / f32::from(f);
                return d.map(|c| c * s);
            }
        }
        if i == 0 {
            return [0.0; 3];
        }
        i -= 1;
    }
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
