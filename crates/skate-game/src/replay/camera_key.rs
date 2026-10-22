//! The replay editor's basic cameras (Follow, Tripod, Free, Game; b401 / b402, b404, b406); limits are data.
//! Details and gaps: docs/hails-additions/37-instant-replay.md#replay-basic-cameras
use bevy::prelude::*;

/// Which basic camera a clip uses (retail's camera type: 0 Follow, 1 Tripod, 2 Free, 3 Game; `82508CE8`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CameraKey {
    /// The recorded gameplay camera.
    Game,
    /// Orbit about the skater, in its frame: yaw / pitch (degrees), distance (m), look height over the root (m).
    Follow { yaw: f32, pitch: f32, distance: f32, look_height: f32 },
    /// A world eye looking at the skater's root plus the look height.
    Tripod { eye: Vec3, look_height: f32 },
    /// A world pose: eye, yaw / pitch (degrees).
    Free { eye: Vec3, yaw: f32, pitch: f32 },
}

/// The menu rows of the basic cameras (screen order Follow, Tripod, Game, Free; item table `8302CCE0`, b401).
impl CameraKey {
    pub fn row(&self) -> usize {
        match self {
            CameraKey::Follow { .. } => 0,
            CameraKey::Tripod { .. } => 1,
            CameraKey::Game => 2,
            CameraKey::Free { .. } => 3,
        }
    }

    /// The edit mode the row's A opens (b401: Follow 7, Tripod 8, Game 9, Free 10; the advanced rows 4..6 the same
    /// modes, b405) and the movie state the C++ posts for it (kFollowEditMode 1, kTripodEditMode 2, kDefaultCameraMode
    /// 10, kFreeCameraMode 7; advanced 12 / 13 / 14).
    pub fn edit_mode_and_state(row: usize) -> Option<(u8, i32)> {
        match row {
            0 => Some((7, 1)),
            1 => Some((8, 2)),
            2 => Some((9, 10)),
            3 => Some((10, 7)),
            4 => Some((7, 12)),
            5 => Some((8, 13)),
            6 => Some((10, 14)),
            _ => None,
        }
    }

    /// The basic row of a menu row (the advanced rows 4..6 are Follow / Tripod / Free) and whether it is advanced.
    pub fn basic_row(row: usize) -> (usize, bool) {
        match row {
            4 => (0, true),
            5 => (1, true),
            6 => (3, true),
            r => (r, false),
        }
    }

    /// Retail's camera type (`82508CE8`: 0 Follow, 1 Tripod, 2 Free, 3 Game), the timeline band's colour index.
    pub fn kind(&self) -> u8 {
        match self {
            CameraKey::Follow { .. } => 0,
            CameraKey::Tripod { .. } => 1,
            CameraKey::Free { .. } => 2,
            CameraKey::Game => 3,
        }
    }

    /// Between two keys of the same camera (`u` 0..1): the parameters blended (OURS: linear for every camera; retail's
    /// Free camera eye is cubic, `curve`); None across different cameras (a cut).
    pub fn lerp(&self, other: &CameraKey, u: f32) -> Option<CameraKey> {
        let l = |a: f32, b: f32| a + (b - a) * u;
        Some(match (*self, *other) {
            (CameraKey::Game, CameraKey::Game) => CameraKey::Game,
            (CameraKey::Follow { yaw, pitch, distance, look_height }, CameraKey::Follow { yaw: y2, pitch: p2, distance: d2, look_height: h2 }) => {
                CameraKey::Follow { yaw: l(yaw, y2), pitch: l(pitch, p2), distance: l(distance, d2), look_height: l(look_height, h2) }
            }
            (CameraKey::Tripod { eye, look_height }, CameraKey::Tripod { eye: e2, look_height: h2 }) => CameraKey::Tripod { eye: eye.lerp(e2, u), look_height: l(look_height, h2) },
            (CameraKey::Free { eye, yaw, pitch }, CameraKey::Free { eye: e2, yaw: y2, pitch: p2 }) => CameraKey::Free { eye: eye.lerp(e2, u), yaw: l(yaw, y2), pitch: l(pitch, p2) },
            _ => return None,
        })
    }
}

/// Limits and per-console-frame rates (b402), data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraRules {
    /// Orbit: degrees per stick unit and the per-frame cap; pitch limit; distance cap per frame and range.
    pub orbit_deg_per_unit: f32,
    pub orbit_deg_cap: f32,
    pub pitch_limit: f32,
    pub distance_step: f32,
    pub distance_range: (f32, f32),
    /// Orbit look height: per frame and range.
    pub look_step: f32,
    pub look_range: (f32, f32),
    /// Free: turn cap (degrees) and move cap (m) per frame, height range.
    pub free_deg_cap: f32,
    pub free_move: f32,
    pub free_height_range: f32,
    /// Lens (mm): per frame step and the editor's clamp, default.
    pub lens_step: f32,
    pub lens_range: (f32, f32),
    pub lens_default: f32,
    pub console_hz: f32,
    /// Height over the skater root the Follow / Tripod cameras look at, m (data; the key's own D-pad lift adds on top).
    /// Retail 0: its look object 4 is the skater root at foot / deck level and a new key's lift starts at 0 (b406:
    /// `826824F8`, recomp trace CAMTGT; `82C7FFB8` resets LOOK_Z).
    pub look_target_height: f32,
}

impl Default for CameraRules {
    fn default() -> Self {
        Self {
            orbit_deg_per_unit: 2.5,
            orbit_deg_cap: 3.0,
            pitch_limit: 85.0,
            distance_step: 0.2,
            distance_range: (0.5, 1000.0),
            look_step: 0.01,
            look_range: (-1.0, 1.0),
            free_deg_cap: 1.6,
            free_move: 0.2,
            free_height_range: 10.0,
            lens_step: 0.5,
            lens_range: (24.0, 75.0),
            lens_default: 24.0,
            console_hz: 30.0,
            look_target_height: 0.0,
        }
    }
}

/// The pad input of one frame for the key edit modes (the screen's prompts): right stick rotates, left stick y =
/// distance (orbit) / move (free, both axes), d-pad up / down = height, left / right (the arrows) = lens.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CameraInput {
    pub turn: Vec2,
    pub move_: Vec2,
    pub height: f32,
    pub zoom: f32,
}

/// A new key of `row`'s camera from the current view (`view`) and the skater root (`root`).
pub(crate) fn key_from_view(row: usize, view: &Transform, root: &Transform) -> Option<CameraKey> {
    let (yaw, pitch, _) = view.rotation.to_euler(EulerRot::YXZ);
    Some(match row {
        0 => {
            let local = root.rotation.inverse() * (view.translation - root.translation);
            let distance = local.length().max(0.5);
            CameraKey::Follow {
                yaw: local.x.atan2(local.z).to_degrees(),
                pitch: (local.y / distance).clamp(-1.0, 1.0).asin().to_degrees(),
                distance,
                look_height: 0.0,
            }
        }
        1 => CameraKey::Tripod { eye: view.translation, look_height: 0.0 },
        2 => CameraKey::Game,
        3 => CameraKey::Free { eye: view.translation, yaw: yaw.to_degrees(), pitch: pitch.to_degrees() },
        _ => return None,
    })
}

/// One console frame of the key edit controls, scaled by `dt` x the console rate (retail steps per frame, no dt).
pub(crate) fn edit(key: &mut CameraKey, lens: &mut f32, input: CameraInput, rules: &CameraRules, dt: f32) {
    let frames = dt * rules.console_hz;
    let cap = |v: f32, c: f32| v.clamp(-c, c);
    *lens = (*lens + input.zoom * rules.lens_step * frames).clamp(rules.lens_range.0, rules.lens_range.1);
    match key {
        CameraKey::Follow { yaw, pitch, distance, look_height } => {
            *yaw += cap(input.turn.x * rules.orbit_deg_per_unit, rules.orbit_deg_cap) * frames;
            *pitch = (*pitch + cap(input.turn.y * rules.orbit_deg_per_unit, rules.orbit_deg_cap) * frames)
                .clamp(-rules.pitch_limit, rules.pitch_limit);
            *distance = (*distance - cap(input.move_.y * rules.distance_step, rules.distance_step) * frames)
                .clamp(rules.distance_range.0, rules.distance_range.1);
            *look_height = (*look_height + input.height * rules.look_step * frames).clamp(rules.look_range.0, rules.look_range.1);
        }
        CameraKey::Tripod { look_height, .. } => {
            *look_height = (*look_height + input.height * rules.look_step * frames).clamp(rules.look_range.0, rules.look_range.1);
        }
        CameraKey::Free { eye, yaw, pitch } => {
            *yaw -= cap(input.turn.x * rules.orbit_deg_per_unit, rules.free_deg_cap) * frames;
            *pitch = (*pitch + cap(input.turn.y * rules.orbit_deg_per_unit, rules.free_deg_cap) * frames)
                .clamp(-rules.pitch_limit, rules.pitch_limit);
            let heading = Quat::from_rotation_y(yaw.to_radians());
            let step = heading * Vec3::new(input.move_.x, 0.0, -input.move_.y) * rules.free_move * frames;
            *eye += step + Vec3::Y * input.height * rules.free_move * frames;
        }
        CameraKey::Game => {}
    }
}

/// The camera pose of `key` for the skater root `root` (None: the recorded camera), looking at the root raised by the
/// rules' look target height.
pub(crate) fn pose(key: &CameraKey, root: &Transform) -> Option<Transform> {
    let root = Transform { translation: root.translation + Vec3::Y * CameraRules::default().look_target_height, ..*root };
    let root = &root;
    let look_at = |eye: Vec3, look: Vec3| Transform::from_translation(eye).looking_at(look, Vec3::Y);
    match *key {
        CameraKey::Game => None,
        CameraKey::Follow { yaw, pitch, distance, look_height } => {
            let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
            let local = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * distance;
            let look = root.translation + Vec3::Y * look_height;
            Some(look_at(root.translation + root.rotation * local, look))
        }
        CameraKey::Tripod { eye, look_height } => Some(look_at(eye, root.translation + Vec3::Y * look_height)),
        CameraKey::Free { eye, yaw, pitch } => Some(Transform::from_translation(eye).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            yaw.to_radians(),
            pitch.to_radians(),
            0.0,
        ))),
    }
}

/// The point a Follow / Tripod `key` looks at for `root` (the same point [`pose`] aims at); None for Free and Game.
pub(crate) fn look_point(key: &CameraKey, root: &Transform) -> Option<Vec3> {
    let base = root.translation + Vec3::Y * CameraRules::default().look_target_height;
    match *key {
        CameraKey::Follow { look_height, .. } | CameraKey::Tripod { look_height, .. } => Some(base + Vec3::Y * look_height),
        CameraKey::Game | CameraKey::Free { .. } => None,
    }
}

/// Vertical field of view (radians) of a lens in mm. NOT RETAIL YET: a 24 mm high gate (retail's mm -> FOV is open).
pub(crate) fn lens_fov(mm: f32) -> f32 {
    2.0 * (12.0 / mm.max(1.0)).atan()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_orbits_in_the_skater_frame_with_retail_caps() {
        let root = Transform::from_xyz(10.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let view = Transform::from_xyz(10.0, 0.0, 0.0) * Transform::from_xyz(0.0, 0.0, 4.0);
        let mut key = key_from_view(0, &Transform::from_translation(root.transform_point(Vec3::new(0.0, 0.0, 4.0))), &root).unwrap();
        let p = pose(&key, &root).unwrap();
        // The orbit centre is the look target, the root raised by the rules' height.
        let expected = root.transform_point(Vec3::new(0.0, 0.0, 4.0)) + Vec3::Y * CameraRules::default().look_target_height;
        assert!(p.translation.distance(expected) < 1e-4, "{p:?}");
        let _ = view;
        // A full stick for one second: capped at 3 degrees per console frame -> 90 degrees.
        let mut lens = 24.0;
        let rules = CameraRules::default();
        for _ in 0..30 {
            edit(&mut key, &mut lens, CameraInput { turn: Vec2::new(10.0, 0.0), ..default() }, &rules, 1.0 / 30.0);
        }
        let CameraKey::Follow { yaw, .. } = key else { panic!() };
        assert!((yaw - 90.0).abs() < 1e-3, "{yaw}");
        // Pitch stops at 85, the lens at 75 mm.
        for _ in 0..120 {
            edit(&mut key, &mut lens, CameraInput { turn: Vec2::new(0.0, 1.0), zoom: 1.0, ..default() }, &rules, 1.0 / 30.0);
        }
        let CameraKey::Follow { pitch, .. } = key else { panic!() };
        assert_eq!((pitch, lens), (85.0, 75.0));
        assert_eq!(CameraKey::edit_mode_and_state(2), Some((9, 10)));
        assert!(pose(&CameraKey::Game, &root).is_none());
    }
}
