//! Ped hand prop release (b25, b87, b90): the throw at a trash bin, the drop, the release frame and the unlink.
//!
//! Retail [code]: `sub_82E3E648` starts a throw (holding required; pending + aimed bits, target, launch speed, release
//! time, the clip by the angle to the target), the per-ped update `sub_82E3ED50` releases the held object when the
//! release time runs out, aimed with the launch velocity of `sub_82E16AB0` and timer 35 = the flight time, through
//! `sub_82E3EBE0` (holding cleared, the object goes into physics); a released object still linked to the ped is
//! unlinked by `sub_82E3FAE0` once it is outside the unlink box around the ped. The ped never destroys it (b90 §3).

use super::super::Vec3;
use super::brain::PedBrain;

/// Timer 34 ThrowHandPropTimer and timer 35 ThrowHandPropReactionTimer (`brain.rs` `timers::NAMES`).
pub const THROW_TIMER: i32 = 34;
pub const THROW_REACTION_TIMER: i32 = 35;

/// Release values with retail defaults (mod-overridable).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPropSettings {
    /// The light throw (ThrowHandPropAtTrashBin, `82E3E648` attack 0): launch speed (`0x821F1790` 5.0) and the time
    /// to the release (`0x82063BE0` 0.91667 s).
    pub light_speed: f32,
    pub light_release_seconds: f32,
    /// The attack throw (ThrowHandPropAtWantTarget): 10.0 (`0x82063BF0`) and 0.8333 s (`0x82063BE4`).
    pub attack_speed: f32,
    pub attack_release_seconds: f32,
    /// Half the launch solve's gravity (`sub_82E16AB0`, `0x822F8FA4` 4.9).
    pub half_gravity: f32,
    /// The unlink box half sizes around the ped, metres x / y (up) / z (`82E3F090`: 0.5, 2.0, 0.5).
    pub unlink_box: [f32; 3],
    /// The throw clips' blend times (ped vfunc +240, 0.2).
    pub clip_blend: f32,
    /// The dynamic-object pool: the manager's create (`826B8830`) refuses at 49 live objects, so a ped's hand prop
    /// is then not created [code, b91].
    pub max_live: usize,
    /// A released prop is culled beyond this distance from the census observer (`livingworld_census_ranges`
    /// `dynamicobjects` cull 100 m [data, dmo-plan]; retail's cull `826BAD98` for a DMO without a placement is not
    /// read, b91).
    pub cull_distance: f32,
}

impl Default for HandPropSettings {
    fn default() -> Self {
        Self {
            light_speed: 5.0,
            light_release_seconds: 0.916_67,
            attack_speed: 10.0,
            attack_release_seconds: 0.833_3,
            half_gravity: 4.9,
            unlink_box: [0.5, 2.0, 0.5],
            clip_blend: 0.2,
            max_live: 49,
            cull_distance: 100.0,
        }
    }
}

/// A started throw (`brain+3152` target, `+3252` speed; `3279` bit 0x40 aimed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPropThrow {
    pub target: Vec3,
    pub speed: f32,
}

/// The launch solve `sub_82E16AB0` [code]: the horizontal speed is `speed`, the flight time is the flat distance over
/// it, the vertical speed reaches `target` at that time under `2 * half_gravity`. `None` (retail: zero velocity, time
/// -1) when the speed is not positive or the target is straight above or below (|dx| and |dz| <= 1.19e-7).
pub fn launch(origin: Vec3, target: Vec3, speed: f32, half_gravity: f32) -> Option<(Vec3, f32)> {
    if speed <= 0.0 {
        return None;
    }
    let d = [target[0] - origin[0], target[1] - origin[1], target[2] - origin[2]];
    if d[0].abs() <= f32::EPSILON && d[2].abs() <= f32::EPSILON {
        return None;
    }
    let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    let time = flat / speed;
    let vy = d[1] / time + half_gravity * time;
    Some(([d[0] / flat * speed, vy, d[2] / flat * speed], time))
}

/// The flat angle from the ped's facing to `point`, radians in [0, 2pi), counter-clockwise seen from above (toward the
/// ped's left with `heading` 0 = +z). Retail `sub_8296EC98` [code]; its sign convention is [inferred] from the clip
/// names (small angles = L).
pub fn flat_angle(heading: f32, from: Vec3, point: Vec3) -> f32 {
    let yaw = (point[0] - from[0]).atan2(point[2] - from[2]);
    (yaw - heading).rem_euclid(std::f32::consts::TAU)
}

/// The light throw's clip by angle (`82E3E648`, thresholds `0x822F9144..0x822F9158`) [code].
pub fn light_throw_clip(angle: f32) -> &'static str {
    match angle {
        a if !(0.6854..=5.5978).contains(&a) => "HandPropThrowLightForward",
        a if a < 0.8854 => "HandPropThrowLightL45",
        a if a < 1.6708 => "HandPropThrowLightL90",
        a if a < 4.6124 => "HandPropThrowLightR180",
        a if a < 4.8124 => "HandPropThrowLightR90",
        _ => "HandPropThrowLightR45",
    }
}

impl PedBrain {
    /// `sub_82E3E648(ped, point, attack 0)`: start the light throw at `target` when the ped holds its prop. Returns the
    /// clip to play.
    pub fn start_light_throw(&mut self, settings: &HandPropSettings, heading: f32, position: Vec3, target: Vec3) -> Option<&'static str> {
        if !self.hand_prop.holding {
            return None;
        }
        self.hand_prop.linked = true;
        self.hand_prop.throw = Some(HandPropThrow { target, speed: settings.light_speed });
        self.set_timer(THROW_TIMER, settings.light_release_seconds);
        Some(light_throw_clip(flat_angle(heading, position, target)))
    }

    /// `sub_82E3EBE0(ped, 0, zero)` (DropHandProp): release in place. Retail's release vfunc +136 (`82C56C70`) writes
    /// no velocity for flag 0, so the body keeps the kinematic hand's velocity [code, b91]; ours: zero (the hand's
    /// velocity is not tracked). Returns the release velocity.
    pub fn drop_hand_prop(&mut self) -> Option<Vec3> {
        if !self.hand_prop.holding {
            return None;
        }
        self.hand_prop.holding = false;
        self.hand_prop.linked = true;
        self.hand_prop.throw = None;
        Some([0.0; 3])
    }

    /// The per-ped release step (`sub_82E3ED50`): a pending throw releases when timer 34 has run out (ours; retail
    /// compares a clip time query against the same value, b90 §2 [inferred equal]) with the launch velocity from the
    /// prop's hand position; timer 35 gets the flight time. Returns the release velocity on the release frame (the
    /// host releases the object at once; DropHandProp posts [`super::brain::ChaseRequest::HandPropReleased`]).
    pub fn update_hand_prop_release(&mut self, settings: &HandPropSettings, prop_position: Vec3) -> Option<Vec3> {
        let throw = self.hand_prop.throw?;
        if !self.hand_prop.holding || self.timer(THROW_TIMER) > 0.0 {
            return None;
        }
        let (velocity, time) = launch(prop_position, throw.target, throw.speed, settings.half_gravity).unwrap_or(([0.0; 3], -1.0));
        self.set_timer(THROW_REACTION_TIMER, time);
        self.hand_prop.holding = false;
        self.hand_prop.throw = None;
        Some(velocity)
    }

    /// `82E3F090` -> `sub_82E3FAE0`: a released prop still linked to the ped is unlinked once it leaves the box around
    /// the ped. Returns true on the unlink (the host forgets the link; the object stays in the world).
    pub fn update_hand_prop_link(&mut self, settings: &HandPropSettings, prop_position: Vec3, ped_position: Vec3) -> bool {
        if self.hand_prop.holding || !self.hand_prop.linked {
            return false;
        }
        let b = settings.unlink_box;
        let d = (0..3).map(|i| (prop_position[i] - ped_position[i]).abs()).collect::<Vec<_>>();
        if d[0] > b[0] || d[1] > b[1] || d[2] > b[2] {
            self.hand_prop.clear();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sub_82E16AB0`: flat speed = `speed`, time = flat distance / speed, and the arc under 2 x 4.9 reaches the
    /// target at that time; no solution straight up or with no speed (retail: zero velocity, time -1).
    #[test]
    fn launch_reaches_the_target() {
        let (origin, target) = ([1.0, 1.5, 2.0], [4.0, 0.8, 6.0]);
        let (v, t) = launch(origin, target, 5.0, 4.9).unwrap();
        assert!((t - 1.0).abs() < 1e-6, "5 m flat at 5 m/s: {t}");
        assert!(((v[0] * v[0] + v[2] * v[2]).sqrt() - 5.0).abs() < 1e-5);
        let y = origin[1] + v[1] * t - 4.9 * t * t;
        assert!((y - target[1]).abs() < 1e-5, "lands at the target height: {y}");
        assert_eq!(launch(origin, [1.0, 9.0, 2.0], 5.0, 4.9), None);
        assert_eq!(launch(origin, target, 0.0, 4.9), None);
    }

    /// The light throw clips by the angle to the target (`82E3E648`).
    #[test]
    fn light_throw_clip_by_angle() {
        let deg = |d: f32| d.to_radians();
        assert_eq!(light_throw_clip(deg(0.0)), "HandPropThrowLightForward");
        assert_eq!(light_throw_clip(deg(330.0)), "HandPropThrowLightForward");
        assert_eq!(light_throw_clip(deg(45.0)), "HandPropThrowLightL45");
        assert_eq!(light_throw_clip(deg(90.0)), "HandPropThrowLightL90");
        assert_eq!(light_throw_clip(deg(180.0)), "HandPropThrowLightR180");
        assert_eq!(light_throw_clip(deg(270.0)), "HandPropThrowLightR90");
        assert_eq!(light_throw_clip(deg(300.0)), "HandPropThrowLightR45");
        // Heading 0 faces +z; a target on +x is 90 deg counter-clockwise (left).
        assert!((flat_angle(0.0, [0.0; 3], [1.0, 0.0, 0.0]) - deg(90.0)).abs() < 1e-5);
    }

    /// Throw at a bin: nothing until the holding ped starts it, release when timer 34 runs out with the launch
    /// velocity (timer 35 = flight time, HasHandProp false from then), unlink once the prop leaves the box.
    #[test]
    fn bin_throw_releases_then_unlinks() {
        let s = HandPropSettings::default();
        let mut b = PedBrain::default();
        assert_eq!(b.start_light_throw(&s, 0.0, [0.0; 3], [0.0, 0.0, 2.0]), None, "nothing held");
        b.hand_prop.request("pop");
        b.hand_prop.requested = false;
        b.hand_prop.holding = true;
        assert_eq!(b.start_light_throw(&s, 0.0, [0.0; 3], [0.0, 0.5, 2.0]), Some("HandPropThrowLightForward"));
        let hand = [0.2, 1.2, 0.3];
        b.tick_timers(0.5);
        assert_eq!(b.update_hand_prop_release(&s, hand), None, "before the release time");
        b.tick_timers(0.5);
        let v = b.update_hand_prop_release(&s, hand).expect("released");
        let (expected, time) = launch(hand, [0.0, 0.5, 2.0], 5.0, 4.9).unwrap();
        assert_eq!(v, expected);
        assert!((b.timer(THROW_REACTION_TIMER) - time).abs() < 1e-6);
        assert!(!b.hand_prop.has() && b.hand_prop.linked);
        assert!(!b.update_hand_prop_link(&s, [0.3, 1.0, 0.4], [0.0; 3]), "still inside the box");
        assert!(b.update_hand_prop_link(&s, [0.0, 0.5, 2.0], [0.0; 3]));
        assert_eq!(b.hand_prop, Default::default());
    }

    /// DropHandProp releases in place with zero velocity.
    #[test]
    fn drop_releases_with_zero_velocity() {
        let mut b = PedBrain::default();
        assert_eq!(b.drop_hand_prop(), None);
        b.hand_prop.holding = true;
        assert_eq!(b.drop_hand_prop(), Some([0.0; 3]));
        assert!(!b.hand_prop.has() && b.hand_prop.linked);
    }
}
