//! The NPC skater's AI physics record (AIPhysicsInput, 164 bytes) built from its recorded line
//! (doc 26, "Simulated NPC skaters"). The physics reads it in the ground states' board path
//! (`crate::riding::grounded::state::board_path`).
//!
//! Retail (TU3, evidence only; re-implemented): `sub_8246DB50` -> `sub_8246DE38` each tick:
//! - target frame = the path frame of the committed cursor node (`sub_82453AD0`: board orientation
//!   `+0x18`, turned about up on board-flipped nodes, `sub_82453A58`) at the node position; when its
//!   forward points away from the skater's (`dot < 0`), its Ri and At rows are negated (the line is
//!   ridden switch / fakie instead of turning round);
//! - target velocity = the node's per-frame displacement (`+0x0C`) x 60 (`0x8302EE08`), its length
//!   clamped to 99.9 (`0x822F94D4`); above 0.001 (`0x82063A48`) the speed shape `sub_82470830`
//!   may scale it (`0.3 * s * influence + 0.125 * s * speed factor`, applied when it changes the
//!   speed by more than 0.01; both inputs not decoded yet, 0 here = no change);
//! - flags `+160`: bit 25 = on-board steering (`pc+922`, seeded by `sub_8246DB50`); with it (`pc+922`) bit 31 always, bit 30 = A and C, bit 29 = B
//!   and C, bit 28 = B, where A = (state byte `14652` clear and `pc+933` clear), B = A and
//!   `pc+945` clear, C = `pc+944` clear or trick category (`pc+832`) 9; without it all of bits
//!   31..28 (holding the cached pose when the skater is more than 10 m off the path,
//!   `0x821963E4`, AI kind 0).
//! The record's other words (trajectory block +80..+144, bits 27..25) are not built yet.

use super::replay::rotate;
use super::Vec3;
use crate::animation::output::actor_packet::ExternalPhysicsInput;

/// `0x8302EE08`: recorded per-frame displacement to m/s.
pub const FRAMES_PER_SECOND: f32 = 60.0;
/// `0x822F94D4`.
pub const MAX_SPEED: f32 = 99.9;

/// The PathController state bytes the flags read (retail riding defaults: all clear, steering on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SteerState {
    /// `pc+922`: on-board steering.
    pub on_board_steering: bool,
    /// The skater component's byte `14652`.
    pub byte_14652: bool,
    pub byte_933: bool,
    pub byte_944: bool,
    pub byte_945: bool,
    /// `pc+832`: the current trick category.
    pub trick_category: i32,
}

impl Default for SteerState {
    fn default() -> Self {
        Self { on_board_steering: true, byte_14652: false, byte_933: false, byte_944: false, byte_945: false, trick_category: 0 }
    }
}

impl SteerState {
    /// The `+160` steering bits, and bit 25 = on-board steering (`sub_8246DB50` seeds the word
    /// with `pc+922 << 25`; the physics state selector sends a skater whose record steers without
    /// it to `PHYSICS_STATE_FOLLOW_PATH`).
    pub fn flags(&self) -> u32 {
        if !self.on_board_steering {
            return 0xF000_0000;
        }
        let on_board = 1 << 25;
        let a = !self.byte_14652 && !self.byte_933;
        let b = a && !self.byte_945;
        let c = !self.byte_944 || self.trick_category == 9;
        on_board | (1 << 31) | (u32::from(a && c) << 30) | (u32::from(b && c) << 29) | (u32::from(b) << 28)
    }
}

/// Where the line is now (the committed cursor node): position, path frame (unit quaternion
/// x y z w) and the recorded per-frame displacement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineTarget {
    pub position: Vec3,
    pub frame: [f32; 4],
    pub step: Vec3,
}

fn words(v: Vec3, w: f32) -> [u32; 4] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), w.to_bits()]
}

/// `sub_8246DE38`: the record for `target`, steered by a skater whose forward is `forward`.
pub fn build(target: &LineTarget, forward: Vec3, state: &SteerState) -> ExternalPhysicsInput {
    let mut ri = rotate(target.frame, [1.0, 0.0, 0.0]);
    let up = rotate(target.frame, [0.0, 1.0, 0.0]);
    let mut at = rotate(target.frame, [0.0, 0.0, 1.0]);
    if at[0] * forward[0] + at[1] * forward[1] + at[2] * forward[2] < 0.0 {
        ri = ri.map(|x| -x);
        at = at.map(|x| -x);
    }
    let mut v = target.step.map(|x| x * FRAMES_PER_SECOND);
    let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if speed > MAX_SPEED {
        v = v.map(|x| x / speed * MAX_SPEED);
    }
    let mut vectors = [[0u32; 4]; 10];
    vectors[0] = words(ri, 0.0);
    vectors[1] = words(up, 0.0);
    vectors[2] = words(at, 0.0);
    vectors[3] = words(target.position, 1.0);
    vectors[4] = words(v, 0.0);
    // +128: the gravity / trajectory scalar, -1.0 splat without a trajectory (`0x8216DEE0`).
    vectors[8] = [(-1.0f32).to_bits(); 4];
    ExternalPhysicsInput { vectors, flags: state.flags() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::riding::grounded::state::board_path::{flags, SteerTarget};

    const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    #[test]
    fn riding_defaults_steer_on_every_axis() {
        const ON_BOARD: u32 = 1 << 25;
        assert_eq!(SteerState::default().flags(), 0xF000_0000 | ON_BOARD);
        // 945 clears B (bits 29 and 28).
        let s = SteerState { byte_945: true, ..SteerState::default() };
        assert_eq!(s.flags(), ON_BOARD | (1 << 31) | (1 << 30));
        // 944 without category 9 clears bits 30 / 29; category 9 keeps them.
        let s = SteerState { byte_944: true, ..SteerState::default() };
        assert_eq!(s.flags(), ON_BOARD | (1 << 31) | (1 << 28));
        assert_eq!(SteerState { trick_category: 9, ..s }.flags(), 0xF000_0000 | ON_BOARD);
        assert_eq!(SteerState { byte_933: true, ..SteerState::default() }.flags(), ON_BOARD | 1 << 31);
        assert_eq!(SteerState { on_board_steering: false, byte_933: true, ..SteerState::default() }.flags(), 0xF000_0000);
    }

    #[test]
    fn the_record_targets_the_line_pose_and_speed() {
        let t = LineTarget { position: [1.0, 2.0, 3.0], frame: IDENTITY, step: [0.0, 0.0, 0.1] };
        let r = build(&t, [0.0, 0.0, 1.0], &SteerState::default());
        let s = SteerTarget::from_words(&r.vectors, r.flags);
        assert_eq!((s.position.x, s.position.y, s.position.z), (1.0, 2.0, 3.0));
        assert!((s.velocity.z - 6.0).abs() < 1e-5, "0.1 m per frame = 6 m/s");
        assert_eq!((s.forward.x, s.forward.z), (0.0, 1.0));
        assert_ne!(s.flags & flags::STEER, 0);
        // Facing away: the frame's Ri / At flip, the velocity keeps the line's direction.
        let r = build(&t, [0.0, 0.0, -1.0], &SteerState::default());
        let s = SteerTarget::from_words(&r.vectors, r.flags);
        assert_eq!(s.forward.z, -1.0);
        assert!(s.velocity.z > 0.0);
        // Clamped at 99.9 m/s.
        let r = build(&LineTarget { step: [0.0, 0.0, 10.0], ..t }, [0.0, 0.0, 1.0], &SteerState::default());
        assert!((SteerTarget::from_words(&r.vectors, r.flags).velocity.z - MAX_SPEED).abs() < 1e-3);
    }
}
