//! Carrying and placing dynamic props while offboard (Phases 3-4 glue).
//!
//! Buttons (raw controller flag bits, see `CarryButtons`): holding the retail
//! GrabWorld button (RB) grabs the nearest prop in reach and keeps it while
//! held; releasing drops it. **B** toggles placement mode while carrying, and
//! releasing the grab button in placement mode confirms the ghost pose.
//!
//! The grab used to be a rising edge of **A**, but A is the retail sprint
//! button: the derived controller's held timer for action 80 (slot 20) is what
//! publishes `OB_Sprint` (8259AA8C, `offboard_intentions.rs`), so every sprint
//! press near a prop grabbed it and dragged it along ("props magnetize to the
//! player as they run by"). Retail gates the offboard grab-object decision
//! (82D324B0, `biped_ground/grab.rs`) on Processed2476 bit 22, GrabWorld,
//! which the input listener emits while raw flag bit 28 (action 73, RB) is
//! held (8259AF68): a held button, not a toggle.
//!
//! Carry is a Skate 3 style drag, not a floating hold: the prop stays on the
//! ground at the offset it was grabbed at, fixed in the carrier's frame, and
//! is pulled along (`PropDynamics::drag_to`) and turned with the carrier
//! (`PropDynamics::set_yaw_rate`), so it slides with its authored friction,
//! bumps over curbs and never lifts off or swings through the carrier by
//! itself. The held prop neither receives skater pushes nor pushes the
//! skater back: `PropDynamics::set_held` exempts it from volume pushes and
//! its collision-layer triangles stay parked at `HELD_PARK` until the drop
//! rebakes them.
//!
//! Placement mode (Phase 4): the prop keeps following a target pose relative
//! to the carrier, frozen mid-air by the per-tick velocity overwrite, while
//! the right stick adjusts distance (Y) and yaw (X) and DPad up/down adjusts
//! height. Releasing the grab button confirms: the prop is dropped at the
//! ghost pose and the layout sidecar is rewritten. B cancels back to plain
//! carry. Yaw applies a direct
//! orientation snap; position still moves by velocity so contacts and the
//! rebaked triangle layer keep working.
//!
//! Layout persistence (`prop_layout.rs`): confirming a placement records
//! `id → (origin, basis)` and saves `settings/prop-layouts/<map>.json` next
//! to the asset root (same convention as `settings/gameplay.json`). Loading
//! a map teleports saved bodies to their stored poses before the first sync.
//!
//! Restrictions: grabbing requires `BipedGround`; a single prop at a time;
//! auto-drop beyond `MAX_HOLD_DISTANCE` or when leaving the on-foot states
//! (`BipedGround`/`OffBoardPushing`; never saves). While held, the retail
//! grab-object byte (`OffBoard304`, published in `player_state/publication.rs`)
//! keeps the selector in `OffBoardPushing` and the MotionGraph in
//! MovingObjectNew, so carrying must not treat state 502 as leaving the ground. Drop keeps the current velocity, so releasing while moving throws
//! gently; re-sleep is the natural cool-down.
//!
//! Locomotion while holding (Move Object, state 502): retail's producer
//! 8259C4B0 turns the raw left stick into OB_ObjectMvX/Z in the skater's
//! frame and the right stick X into OB_ObjectMvRot; the shipped curve for a
//! left-stick share of the rotation is all zero. So the left stick moves the
//! pair (push, pull, side step) without turning it and only the right stick
//! turns it. `biped_ground::update` applies that through [`object_move_motion`]
//! with the speeds of [`CarryLocomotion`]. Before this, the carried stick was
//! fed to the walking controller as a world direction rebuilt from the
//! current facing, so any stick not straight ahead turned the skater, which
//! turned the target with it: the skater and prop spun in place.
//!
//! Multiplayer: prop bodies and layouts are host-local; skate-net would need
//! to replicate held id, body poses/velocities and layout writes. Not
//! implemented.
use skate_core::{math::Vector3, player::state::PhysicalStateId};
use bevy::prelude::warn;
use std::collections::BTreeMap;

use super::prop_dynamics::PropDynamics;

/// Pickup reach from the carrier's root, measured to the prop's surface.
/// Omnidirectional: retail grabbing does not require facing the prop.
const GRAB_RADIUS: f32 = 2.0;
/// Drag hold distance: the prop keeps the side it was grabbed from, this far
/// from the carrier.
const DRAG_HOLD: f32 = 0.9;
/// Drag speed cap; above this the prop lags behind instead of snapping.
/// Roughly a fast walk, so sprinting leaves a heavy prop trailing.
const MAX_DRAG_SPEED: f32 = 4.0;
/// Placement follow cap; above this the prop lags instead of snapping.
const MAX_CARRY_SPEED: f32 = 6.0;
/// Auto-drop distance: the prop is stuck or was left behind.
const MAX_HOLD_DISTANCE: f32 = 3.5;
/// Placement adjust rates and clamps.
const PLACE_YAW_RATE: f32 = 2.5;
const PLACE_DISTANCE_RATE: f32 = 2.0;
const PLACE_HEIGHT_RATE: f32 = 1.5;
const PLACE_DISTANCE: std::ops::Range<f32> = 0.3..4.0;
const PLACE_HEIGHT: std::ops::Range<f32> = 0.0..2.5;

/// Speeds of the skater and held prop moving as one pair (Move Object,
/// state 502). Retail moves the pair with the MovingObjectNew animations from
/// the OB_ObjectMv inputs; that physics state (PhysState_OffBoardPushing) is
/// not decoded yet, so these are engine values a mod may change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CarryLocomotion {
    /// m/s at full stick pushing forward (OB_ObjectMvZ > 0).
    pub push_speed: f32,
    /// m/s at full stick pulling back (OB_ObjectMvZ < 0).
    pub pull_speed: f32,
    /// m/s at full stick sideways (OB_ObjectMvX).
    pub side_speed: f32,
    /// rad/s at full OB_ObjectMvRot (right stick X).
    pub turn_rate: f32,
    /// m kept between the skater's root and the prop's near face while
    /// dragging (fix20): the hold distance is never shorter than the prop's
    /// extent toward the skater plus this, so a long prop (bench) is held
    /// by its edge instead of pulling its centre into the skater. NOT RETAIL
    /// YET: engine value. Retail's grab offset probably comes from the DMO
    /// characteristics (per-object grab data) and the MovingObjectNew /
    /// MVOBJ clips' hand positions (UpdateObjectGrabbing 0x821BCDB4,
    /// IsGrabbingDMO 0x820B757C), not decoded.
    pub grip_reach: f32,
}

impl Default for CarryLocomotion {
    fn default() -> Self {
        Self { push_speed: 1.4, pull_speed: 1.0, side_speed: 0.8, turn_rate: 1.6, grip_reach: 0.35 }
    }
}

impl CarryLocomotion {
    /// Non-finite or negative values fall back to the default per field.
    pub(crate) fn sanitized(self) -> Self {
        let d = Self::default();
        let ok = |v: f32, d: f32| if v.is_finite() && v >= 0.0 { v } else { d };
        Self {
            push_speed: ok(self.push_speed, d.push_speed),
            pull_speed: ok(self.pull_speed, d.pull_speed),
            side_speed: ok(self.side_speed, d.side_speed),
            turn_rate: ok(self.turn_rate, d.turn_rate),
            grip_reach: ok(self.grip_reach, d.grip_reach),
        }
    }
}

/// One tick of Move Object locomotion from the OB_ObjectMv inputs: the
/// world velocity of the pair and its yaw rate (rad/s, positive turns the
/// facing toward the frame's right). `right`/`forward` are the skater's
/// ground-frame rows. The left stick never contributes to the yaw rate
/// (retail curve 9ADFC2E222938C1E is all zero), which is what keeps the
/// pair from chasing its own stick direction.
pub(crate) fn object_move_motion(
    right: [f32; 4],
    forward: [f32; 4],
    move_x: f32,
    move_z: f32,
    rotation: f32,
    locomotion: CarryLocomotion,
) -> ([f32; 4], f32) {
    let flat = |v: [f32; 4]| {
        let l = (v[0] * v[0] + v[2] * v[2]).sqrt();
        if l > 1e-6 { [v[0] / l, v[2] / l] } else { [0.0, 0.0] }
    };
    let (r, f) = (flat(right), flat(forward));
    let finite = |v: f32| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
    let (x, z, rot) = (finite(move_x), finite(move_z), finite(rotation));
    let along = z * if z >= 0.0 { locomotion.push_speed } else { locomotion.pull_speed };
    let side = x * locomotion.side_speed;
    let velocity = [r[0] * side + f[0] * along, 0.0, r[1] * side + f[1] * along, 0.0];
    (velocity, rot * locomotion.turn_rate)
}

/// Rotate a ground-frame row about world +Y by `angle` (positive turns +Z
/// toward +X, the same sense as [`object_move_motion`]'s yaw rate).
pub(crate) fn yaw_row(v: [f32; 4], angle: f32) -> [f32; 4] {
    let (sin, cos) = angle.sin_cos();
    [v[0] * cos + v[2] * sin, v[1], -v[0] * sin + v[2] * cos, v[3]]
}

/// Which raw controller flag bits drive carrying. The bits index the packed
/// button word of `DerivedControllerInput` (word 6 previous, word 13 current;
/// bit = 101 - action for actions 74..81, bits 28..31 for actions 73..66).
/// Defaults are the retail buttons; a host setting or mod may rebind them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CarryButtons {
    /// Held to grab and carry: bit 28, action 73, RB (retail GrabWorld).
    pub grab_bit: u32,
    /// Rising edge toggles placement: bit 20, action 81, B.
    pub placement_bit: u32,
}

impl Default for CarryButtons {
    fn default() -> Self {
        Self {
            grab_bit: 28,
            placement_bit: 20,
        }
    }
}

/// One tick of carry/placement input, sampled in `frame.rs`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Tick {
    /// Grab button held (level): grabs, keeps the carry, release drops or
    /// confirms placement.
    pub grab: bool,
    /// B rising edge: enter placement or cancel back to carry.
    pub placement: bool,
    /// Right stick X/Y and DPad up-down, already scaled to [-1, 1].
    pub yaw_axis: f32,
    pub distance_axis: f32,
    pub height_axis: f32,
}

impl Tick {
    /// Buttons from the derived controller words (`DerivedControllerInput::words`).
    pub(crate) fn from_controller(
        words: &[u32; 26],
        buttons: CarryButtons,
        yaw_axis: f32,
        distance_axis: f32,
        height_axis: f32,
    ) -> Self {
        let held = |bit: u32| bit < 32 && words[13] & (1 << bit) != 0;
        let was_held = |bit: u32| bit < 32 && words[6] & (1 << bit) != 0;
        Self {
            grab: held(buttons.grab_bit),
            placement: held(buttons.placement_bit) && !was_held(buttons.placement_bit),
            yaw_axis,
            distance_axis,
            height_axis,
        }
    }
}

/// Per-tick carrier observation from the skater runtime.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Carrier {
    pub state: PhysicalStateId,
    pub position: Vector3,
    /// Horizontal facing direction, normalized.
    pub forward: Vector3,
    pub time_step: f32,
}

impl Carrier {
    /// Rotation about +Y mapping (0,0,1) onto `forward`.
    fn facing_yaw(&self) -> f32 {
        f32::atan2(self.forward.x, self.forward.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Carry,
    /// Ghost pose offsets relative to the carrier's root and facing.
    Placement { distance: f32, height: f32, yaw: f32 },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PropCarry {
    held: Option<u32>,
    mode: Mode,
    /// Confirmed placements this session, seeded from the layout sidecar so
    /// re-saving never drops props placed in earlier sessions.
    layout: BTreeMap<u32, super::prop_layout::PropPose>,
    layout_path: Option<std::path::PathBuf>,
    buttons: CarryButtons,
    /// Pickup reach (`GRAB_RADIUS` unless [`CarrySettings`] changes it).
    grab_range: Option<f32>,
    /// Move Object speeds (host setting or mod).
    locomotion: CarryLocomotion,
    /// Held prop's offset in the carrier's frame (right, forward), fixed at
    /// grab time so the pair moves and turns as one, and the box's extent
    /// toward the carrier (added to `MAX_HOLD_DISTANCE`).
    grip: Option<(f32, f32, f32)>,
    /// Carrier facing yaw on the previous carry tick, for the prop's turn.
    last_yaw: Option<f32>,
}

/// Carry settings a host setting or a mod (`sdk.world.set_tuning('carry', ...)`) changes:
/// the buttons and the pickup reach. `default()` = shipped values (mod disable). Pushed into the
/// live [`PropCarry`] before each physics tick, so a map load (new `PropCarry`) keeps them.
#[derive(bevy::prelude::Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct CarrySettings {
    pub buttons: CarryButtons,
    pub grab_range: f32,
    pub locomotion: CarryLocomotion,
}

impl Default for CarrySettings {
    fn default() -> Self {
        Self { buttons: CarryButtons::default(), grab_range: GRAB_RADIUS, locomotion: CarryLocomotion::default() }
    }
}

impl CarrySettings {
    /// Push into the live carry state (non-finite or non-positive reach = default).
    pub(crate) fn apply_to(&self, carry: &mut PropCarry) {
        carry.buttons = self.buttons;
        carry.grab_range = (self.grab_range.is_finite() && self.grab_range > 0.0 && self.grab_range != GRAB_RADIUS).then_some(self.grab_range);
        carry.locomotion = self.locomotion.sanitized();
    }
}

pub(crate) fn apply_carry_settings(settings: bevy::prelude::Res<CarrySettings>, mut physics: bevy::prelude::ResMut<super::GamePhysics>) {
    if physics.prop_carry.buttons != settings.buttons
        || physics.prop_carry.grab_range() != settings.grab_range
        || physics.prop_carry.locomotion != settings.locomotion.sanitized()
    {
        settings.apply_to(&mut physics.prop_carry);
    }
}

impl Default for Mode {
    fn default() -> Self {
        Self::Carry
    }
}

impl PropCarry {
    pub fn held(&self) -> Option<u32> {
        self.held
    }

    pub(crate) fn buttons(&self) -> CarryButtons {
        self.buttons
    }

    /// Move Object speeds in effect.
    pub(crate) fn locomotion(&self) -> CarryLocomotion {
        self.locomotion
    }

    /// Pickup reach in effect.
    pub(crate) fn grab_range(&self) -> f32 {
        self.grab_range.unwrap_or(GRAB_RADIUS)
    }

    /// Rebind the carry buttons (host setting or mod override).
    #[allow(dead_code)]
    pub(crate) fn set_buttons(&mut self, buttons: CarryButtons) {
        self.buttons = buttons;
    }

    pub fn placing(&self) -> bool {
        matches!(self.mode, Mode::Placement { .. })
    }

    /// Session state after loading a map: saved poses plus the sidecar path
    /// (None keeps placement working without persistence).
    pub(crate) fn with_layout(
        layout: BTreeMap<u32, super::prop_layout::PropPose>,
        layout_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            held: None,
            mode: Mode::Carry,
            layout,
            layout_path,
            buttons: CarryButtons::default(),
            grab_range: None,
            locomotion: CarryLocomotion::default(),
            grip: None,
            last_yaw: None,
        }
    }

    /// Saved pose overrides, for load-time application.
    pub(crate) fn layout(&self) -> &BTreeMap<u32, super::prop_layout::PropPose> {
        &self.layout
    }

    /// The prop currently grabbable by this carrier, if any. Shared by the
    /// grab path and the HUD indicator. Reach is measured to the prop's
    /// surface and works in any direction.
    pub(crate) fn candidate(&self, dynamics: &PropDynamics, carrier: Carrier) -> Option<u32> {
        if self.held.is_some() || carrier.state != PhysicalStateId::BipedGround {
            return None;
        }
        dynamics.nearest_body(carrier.position, self.grab_range()).map(|(id, _)| id)
    }

    pub(crate) fn update(&mut self, dynamics: &mut PropDynamics, tick: Tick, carrier: Carrier) {
        let on_foot = matches!(
            carrier.state,
            PhysicalStateId::BipedGround | PhysicalStateId::OffBoardPushing
        );
        let Some(id) = self.held else {
            if tick.grab {
                if let Some(id) = self.candidate(dynamics, carrier) {
                    self.held = Some(id);
                    self.grip = None;
                    self.last_yaw = None;
                    self.follow(dynamics, id, carrier);
                }
            }
            return;
        };
        let position = dynamics.position_of(id);
        // Measured from the near face like the grab reach (fix20): a long
        // prop grabbed by its end has its centre beyond MAX_HOLD_DISTANCE,
        // and a centre test dropped it the tick after every grab (the
        // BipedGround <-> OffBoardPushing flip each tick in the 14:46 log).
        let limit = MAX_HOLD_DISTANCE + self.grip.map_or(0.0, |g| g.2);
        let too_far = position.is_some_and(|p| {
            let d = sub(p, carrier.position);
            dot(d, d) > limit * limit
        });
        if !on_foot || position.is_none() || too_far {
            // Auto-drop: never a confirmed placement, so nothing is saved.
            self.held = None;
            self.mode = Mode::Carry;
            self.grip = None;
            return;
        }
        match self.mode {
            Mode::Carry => {
                if !tick.grab {
                    // Velocity is kept: releasing while moving throws gently.
                    self.held = None;
                    self.grip = None;
                    return;
                }
                if tick.placement {
                    // Enter placement at the prop's current relative pose so
                    // the ghost starts where the drag left it.
                    let position = dynamics.position_of(id).unwrap_or(carrier.position);
                    let offset = sub(position, carrier.position);
                    let flat = Vector3::new(offset.x, 0.0, offset.z);
                    self.mode = Mode::Placement {
                        distance: dot(flat, flat)
                            .sqrt()
                            .clamp(PLACE_DISTANCE.start, PLACE_DISTANCE.end),
                        height: offset.y.clamp(PLACE_HEIGHT.start, PLACE_HEIGHT.end),
                        yaw: 0.0,
                    };
                    return;
                }
                self.follow(dynamics, id, carrier);
            }
            Mode::Placement {
                mut distance,
                mut height,
                mut yaw,
            } => {
                if !tick.grab {
                    self.confirm(dynamics, id);
                    self.held = None;
                    self.grip = None;
                    self.mode = Mode::Carry;
                    return;
                }
                if tick.placement {
                    self.mode = Mode::Carry;
                    self.grip = None;
                    self.last_yaw = None;
                    return;
                }
                let dt = carrier.time_step;
                yaw += tick.yaw_axis * PLACE_YAW_RATE * dt;
                distance = (distance + tick.distance_axis * PLACE_DISTANCE_RATE * dt)
                    .clamp(PLACE_DISTANCE.start, PLACE_DISTANCE.end);
                height = (height + tick.height_axis * PLACE_HEIGHT_RATE * dt)
                    .clamp(PLACE_HEIGHT.start, PLACE_HEIGHT.end);
                self.mode = Mode::Placement {
                    distance,
                    height,
                    yaw,
                };
                let (target, basis) = ghost_pose(carrier, distance, height, yaw);
                dynamics.carry_to_pose(id, target, basis, MAX_CARRY_SPEED, dt);
            }
        }
    }

    /// Plain carry: drag the prop along the ground at the offset it was
    /// grabbed at, fixed in the carrier's frame (right, forward), so walking
    /// pulls or pushes it and turning swings it round with the carrier
    /// instead of leaving it on a world bearing. The grab distance is pulled
    /// in to `DRAG_HOLD` (at least 0.5 m). Height and vertical velocity stay
    /// physical; the prop turns at the carrier's yaw rate.
    fn follow(&mut self, dynamics: &mut PropDynamics, id: u32, carrier: Carrier) {
        let Some(position) = dynamics.position_of(id) else {
            return;
        };
        let forward = carrier.forward;
        // Right of the facing: (0,0,1) -> (1,0,0), as in the ground frame.
        let right = Vector3::new(forward.z, 0.0, -forward.x);
        let reach = self.locomotion.grip_reach;
        let (grip_right, grip_forward, _) = *self.grip.get_or_insert_with(|| {
            let offset = sub(position, carrier.position);
            let flat = Vector3::new(offset.x, 0.0, offset.z);
            let distance = dot(flat, flat).sqrt();
            let bearing = if distance > 1e-3 { scale(flat, 1.0 / distance) } else { forward };
            // Hold by the near face: never closer than the box's extent
            // toward the skater plus the grip reach (a bench's centre at
            // DRAG_HOLD put the skater inside it).
            let extent = dynamics
                .obstacle_boxes()
                .into_iter()
                .find(|b| b.0 == id)
                .map_or(0.0, |(_, _, basis, half, _, _)| box_extent(basis, half, bearing));
            let hold = distance.clamp(0.5, DRAG_HOLD).max(extent + reach);
            (dot(bearing, right) * hold, dot(bearing, forward) * hold, extent)
        });
        let target = Vector3::new(
            carrier.position.x + right.x * grip_right + forward.x * grip_forward,
            position.y,
            carrier.position.z + right.z * grip_right + forward.z * grip_forward,
        );
        dynamics.drag_to(id, target, MAX_DRAG_SPEED, carrier.time_step);
        let yaw = carrier.facing_yaw();
        let rate = self.last_yaw.map_or(0.0, |last| {
            let pi = std::f32::consts::PI;
            let delta = (yaw - last + pi).rem_euclid(std::f32::consts::TAU) - pi;
            delta / carrier.time_step.max(1e-4)
        });
        self.last_yaw = Some(yaw);
        dynamics.set_yaw_rate(id, rate);
    }

    /// Record the confirmed pose and rewrite the layout sidecar.
    fn confirm(&mut self, dynamics: &mut PropDynamics, id: u32) {
        dynamics.release_still(id);
        let Some((origin, basis)) = dynamics.pose(id) else {
            return;
        };
        self.layout.insert(
            id,
            super::prop_layout::PropPose {
                id,
                origin: [origin.x, origin.y, origin.z],
                basis: basis.columns,
            },
        );
        if let Some(path) = &self.layout_path {
            if let Err(error) = super::prop_layout::save(path, &self.layout) {
                warn!("SKATE_PROP_LAYOUT: {}: {error}", path.display());
            }
        }
    }
}

/// Half-length of a box (rows of `basis` are its axes, as in
/// `PropDynamics::nearest_body`) along the flat direction `bearing`.
fn box_extent(basis: skate_core::math::Basis3, half: Vector3, bearing: Vector3) -> f32 {
    let b = basis.columns;
    let along = |i: usize| (bearing.x * b[i][0] + bearing.y * b[i][1] + bearing.z * b[i][2]).abs();
    along(0) * half.x + along(1) * half.y + along(2) * half.z
}

/// Ghost pose: `distance` ahead of the carrier along facing+yaw, `height`
/// above the root, rotated `yaw` from the carrier's facing.
fn ghost_pose(
    carrier: Carrier,
    distance: f32,
    height: f32,
    yaw: f32,
) -> (Vector3, skate_core::math::Basis3) {
    let angle = carrier.facing_yaw() + yaw;
    let (sin, cos) = angle.sin_cos();
    let target = add(
        carrier.position,
        Vector3::new(sin * distance, height, cos * distance),
    );
    let basis = skate_core::math::Basis3 {
        columns: [[cos, 0.0, -sin], [0.0, 1.0, 0.0], [sin, 0.0, cos]],
    };
    (target, basis)
}

fn add(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn sub(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn scale(v: Vector3, s: f32) -> Vector3 {
    Vector3::new(v.x * s, v.y * s, v.z * s)
}

fn dot(a: Vector3, b: Vector3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
