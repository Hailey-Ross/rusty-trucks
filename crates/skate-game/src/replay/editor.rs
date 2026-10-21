//! The replay editor on the recording (FE 12 timeline, mode 6, and its sub modes; research b386 / b387): one clip of
//! skate-core's model over the recording, in retail's 30 fps timeline frames, with the session's mode. A from the
//! instant replay opens it (`82613988`: leave instant, mode 6, SetState 21, 9).
use skate_core::replay_edit::{
    session::{EditorMode, EditorSession},
    EditError, EditLimits, Lane, ReplayEdit,
};

/// The row of the game camera (key type 3 -> row 2 in `8250A8E8`, b398), the clip's camera until another is picked.
pub(crate) const GAME_CAMERA_ROW: usize = 2;
/// The effects menu rows without the DLC (research b398, input `82612038`, item table `8302CCE0`): rows 0..6 pick the
/// key's camera (A: 0..3 the basic cameras, `camera_key`; the advanced 4..6 are not ported), row 7 speed and row 8
/// shake take Left / Right (`8250ADF8` / `8250AB28`). Data: a mod may reorder or extend it.
pub(crate) const EFFECT_ROWS: [Option<Lane>; 9] = [None, None, None, None, None, None, None, Some(Lane::Speed), Some(Lane::Shake)];

pub(crate) struct Editor {
    pub session: EditorSession,
    pub edit: ReplayEdit,
    pub clip: usize,
    /// The effects menu's row (`GetEditIndex`).
    pub row: usize,
    /// Recording time of timeline frame 0, and the timeline's frames per second.
    base: f64,
    fps: f64,
    /// The recording's length in timeline frames, and the trim moves' fractional frames (start, end).
    frames: u32,
    trim_carry: [f64; 2],
    trimming_end: bool,
    /// The camera keys (sorted by frame; none = the recorded camera), the key being edited, and the camera limits /
    /// rates (data).
    pub cameras: Vec<CamKey>,
    pub editing: Option<usize>,
    pub camera_rules: super::camera_key::CameraRules,
}

/// The two marker kinds retail's delete asks about (b407, `82508D88`): 1 a clip marker (a hard key), 2 a key marker
/// (a soft key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkerKind {
    Clip,
    Key,
}

/// One camera key on the timeline (b405): inserted at the playhead when a camera row is picked; a basic camera's key is
/// hard (the pose holds and cuts at the next key), an advanced camera's soft (blends into the next soft key).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CamKey {
    pub frame: u32,
    pub camera: super::camera_key::CameraKey,
    pub lens: f32,
    pub soft: bool,
}

/// Retail's camera key cap (b381 / b405: 100 markers, `82613500`).
pub(crate) const MAX_CAMERA_KEYS: usize = 100;

/// Retail's trim rate: at full stick, 0.005 of the timeline per console frame (the scrub filter's clamp, `82610F18`
/// mode 2, b387), at the console's 30 frames per second.
pub(crate) const TRIM_RATE: f64 = 0.005;
pub(crate) const CONSOLE_HZ: f64 = 30.0;

impl Editor {
    /// The editor over the recording `start..end` (s): None while it holds fewer than the limits' frames (retail
    /// 150, `8250B8F8`).
    pub fn open(start: f64, end: f64, limits: EditLimits) -> Option<Self> {
        let fps = f64::from(limits.timeline_fps);
        let frames = ((end - start).max(0.0) * fps).floor() as u32;
        let mut edit = ReplayEdit::new(limits);
        if !edit.can_open(frames) {
            return None;
        }
        let clip = edit.add_clip(0, frames).ok()?;
        let mut session = EditorSession::instant();
        session.mode = EditorMode::Timeline;
        Some(Self { session, edit, clip, row: 0, base: start, fps, frames, trim_carry: [0.0; 2], trimming_end: false, cameras: Vec::new(), editing: None, camera_rules: Default::default() })
    }

    pub fn frame(&self, time: f64) -> u32 {
        ((time - self.base) * self.fps).round().max(0.0) as u32
    }

    pub fn time(&self, frame: u32) -> f64 {
        self.base + f64::from(frame) / self.fps
    }

    /// The clip's in / out points as recording times.
    pub fn heads(&self) -> (f64, f64) {
        let c = &self.edit.clips[self.clip];
        (self.time(c.start), self.time(c.end))
    }

    /// Playback rate at `time` (the speed lane: 1.0 = real time; retail slows only, 10 % steps).
    /// The clip's look at `time` over the world grade `base` (the look lanes, b408), or None outside a clip.
    pub fn look(&self, time: f64, base: &skate_core::colour_matrix::ColourMatrixRecord) -> Option<skate_core::replay_edit::ReplayLook> {
        self.edit.look_at(self.clip, self.frame(time), base)
    }

    pub fn speed(&self, time: f64) -> f64 {
        f64::from(self.edit.effect_at(self.clip, Lane::Speed, self.frame(time)).unwrap_or(1.0))
    }

    /// Move the in / out points by whole timeline frames (LB held: the sticks, `82610F18` mode 2). A move that
    /// would empty the clip or leave the recording stops at the limit.
    pub fn trim_by(&mut self, start: i64, end: i64, frames: u32) {
        let c = &self.edit.clips[self.clip];
        let s = (i64::from(c.start) + start).clamp(0, i64::from(frames)) as u32;
        let e = (i64::from(c.end) + end).clamp(0, i64::from(frames)) as u32;
        if e > s {
            let _ = self.edit.trim(self.clip, s, e);
        }
    }

    /// LB held (mode 2): the left stick's x moves the in point, the right stick's x the out point.
    pub fn trim_sticks(&mut self, left: f32, right: f32, dt: f32) {
        let per_second = TRIM_RATE * f64::from(self.frames) * CONSOLE_HZ;
        let mut whole = [0i64; 2];
        for (i, axis) in [left, right].into_iter().enumerate() {
            self.trim_carry[i] += f64::from(axis) * per_second * f64::from(dt);
            whole[i] = self.trim_carry[i].trunc() as i64;
            self.trim_carry[i] -= whole[i] as f64;
        }
        let frames = self.frames;
        self.trim_by(whole[0], whole[1], frames);
        if whole[0] != 0 {
            self.trimming_end = false;
        } else if whole[1] != 0 {
            self.trimming_end = true;
        }
    }

    /// Where the playhead sits while trimming: on the head that moved last (the movie draws the head at the
    /// playhead, `GetCursorOffset`, b398).
    pub fn trim_cursor(&self) -> f64 {
        let (start, end) = self.heads();
        if self.trimming_end { end } else { start }
    }

    /// The camera keys as fractions of the recording (the timeline strip's markers, x / 817, b403).
    pub fn key_fractions(&self) -> Vec<f32> {
        self.cameras.iter().map(|k| k.frame as f32 / self.frames.max(1) as f32).collect()
    }

    /// The timeline band's segments: (start fraction, camera type), the recorded camera before the first key.
    pub fn segments(&self) -> Vec<(f32, u8)> {
        let mut out = Vec::new();
        if self.cameras.first().is_none_or(|k| k.frame > 0) {
            out.push((0.0, 3));
        }
        out.extend(self.cameras.iter().map(|k| (k.frame as f32 / self.frames.max(1) as f32, k.camera.kind())));
        out
    }

    /// The key at the playhead: within the marker spacing of the clip (at least one frame; `825084C8`, b382).
    pub fn key_near(&self, time: f64) -> Option<usize> {
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        let near = ((self.edit.limits.marker_spacing * (c.end - c.start) as f32).floor() as u32).max(1);
        self.cameras.iter().position(|k| k.frame.abs_diff(frame) <= near)
    }

    /// The key in effect at `time` (the last at or before it).
    fn active(&self, time: f64) -> Option<usize> {
        let frame = self.frame(time);
        self.cameras.iter().rposition(|k| k.frame <= frame)
    }

    /// The menu row of the camera in effect (advanced rows for soft keys; the game camera without keys).
    pub fn active_row(&self, time: f64) -> usize {
        self.active(time).map_or(GAME_CAMERA_ROW, |i| {
            let k = &self.cameras[i];
            match (k.camera.row(), k.soft) {
                (0, true) => 4,
                (1, true) => 5,
                (3, true) => 6,
                (r, _) => r,
            }
        })
    }

    /// A on a camera row (b405, `82C7FFB8`): a key of that camera at the playhead from the current view (a key already
    /// there takes the new camera), then its edit mode. None when refused (the cap) or the row is not a camera.
    pub fn pick_camera(&mut self, row: usize, time: f64, view: &bevy::prelude::Transform, root: &bevy::prelude::Transform) -> Option<u8> {
        let (mode, _) = super::camera_key::CameraKey::edit_mode_and_state(row)?;
        let (basic, soft) = super::camera_key::CameraKey::basic_row(row);
        let from = self.pose(time, root).0.unwrap_or(*view);
        let camera = super::camera_key::key_from_view(basic, &from, root)?;
        let lens = self.active(time).map_or(self.camera_rules.lens_default, |i| self.cameras[i].lens);
        let index = match self.key_near(time) {
            Some(i) => {
                self.cameras[i].camera = camera;
                self.cameras[i].soft = soft;
                i
            }
            None if self.cameras.len() >= MAX_CAMERA_KEYS => return None,
            None => {
                let frame = self.frame(time);
                let i = self.cameras.partition_point(|k| k.frame < frame);
                self.cameras.insert(i, CamKey { frame, camera, lens, soft });
                i
            }
        };
        self.editing = Some(index);
        Some(mode)
    }

    /// A on a camera key in the timeline (b405, `82612C28`): that key's edit mode, None when no key is there.
    pub fn reopen_at(&mut self, time: f64) -> Option<u8> {
        let i = self.key_near(time)?;
        self.editing = Some(i);
        let k = &self.cameras[i];
        let row = if k.soft { [4, 5, 2, 6][k.camera.row().min(3)] } else { k.camera.row() };
        super::camera_key::CameraKey::edit_mode_and_state(row).map(|(mode, _)| mode)
    }

    /// The key edit controls on the key being edited.
    pub fn edit_key(&mut self, input: super::camera_key::CameraInput, dt: f32) {
        let rules = self.camera_rules;
        if let Some(k) = self.editing.and_then(|i| self.cameras.get_mut(i)) {
            super::camera_key::edit(&mut k.camera, &mut k.lens, input, &rules, dt);
        }
    }

    /// The camera at `time` on the skater root: the key in effect (a soft key blends into the next soft key of the
    /// same camera), and its lens; (None, None) = the recorded camera at its own lens.
    pub fn pose(&self, time: f64, root: &bevy::prelude::Transform) -> (Option<bevy::prelude::Transform>, Option<f32>) {
        let Some(i) = self.active(time) else { return (None, None) };
        let (mut camera, mut lens) = (self.cameras[i].camera, self.cameras[i].lens);
        if let Some(next) = self.cameras.get(i + 1).filter(|n| self.cameras[i].soft && n.soft) {
            let a = self.cameras[i].frame as f64;
            let u = ((self.frame(time) as f64 - a) / (next.frame as f64 - a).max(1.0)).clamp(0.0, 1.0) as f32;
            if let Some(blend) = camera.lerp(&next.camera, u) {
                camera = blend;
                lens += (next.lens - lens) * u;
            }
        }
        let fov = (lens != self.camera_rules.lens_default || camera != super::camera_key::CameraKey::Game)
            .then(|| super::camera_key::lens_fov(lens));
        (super::camera_key::pose(&camera, root), fov)
    }

    /// D-pad Left / Right released: the previous / next marker (the screen's "Skip to previous / next marker"),
    /// else the in / out point (b386).
    pub fn skip(&self, time: f64, forward: bool) -> f64 {
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        let markers = self.cameras.iter().map(|k| k.frame);
        let target = if forward {
            markers.filter(|&f| f > frame).min().unwrap_or(c.end)
        } else {
            markers.filter(|&f| f < frame).max().unwrap_or(c.start)
        };
        self.time(target)
    }

    /// D-pad Down released: a camera key at the cursor holding the camera in effect there (a hard key: the marker the
    /// timeline draws, b403 / b405); refused at a key, at the clip's ends (the marker spacing) or at the cap (retail's
    /// error sound / popup, `82613500` / `82613598`).
    pub fn drop_marker(&mut self, time: f64) -> Result<(), EditError> {
        if self.cameras.len() >= MAX_CAMERA_KEYS {
            return Err(EditError::NoMoreMarkers);
        }
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        let at = (frame as f32 - c.start as f32) / (c.end - c.start).max(1) as f32;
        let spacing = self.edit.limits.marker_spacing;
        if self.key_near(time).is_some() || at <= spacing || at >= 1.0 - spacing {
            return Err(EditError::TooClose);
        }
        let (camera, lens) = self.active(time).map_or((super::camera_key::CameraKey::Game, self.camera_rules.lens_default), |i| (self.cameras[i].camera, self.cameras[i].lens));
        let i = self.cameras.partition_point(|k| k.frame < frame);
        self.cameras.insert(i, CamKey { frame, camera, lens, soft: false });
        Ok(())
    }

    /// What D-pad Up asks about before removing the key at the cursor (b407, `82613650` via `82508D88`): a hard key is
    /// a clip marker, a soft key a key marker; None = no key there (retail's error sound).
    pub fn marker_kind(&self, time: f64) -> Option<MarkerKind> {
        self.key_near(time).map(|i| if self.cameras[i].soft { MarkerKind::Key } else { MarkerKind::Clip })
    }

    /// D-pad Down's confirm (b407, `82508828`): retail asks before a clip marker lands inside a run of soft keys (the
    /// key in effect is soft); not at the cap, whose refusal comes first (`82613500`).
    pub fn drop_needs_confirm(&self, time: f64) -> bool {
        self.cameras.len() < MAX_CAMERA_KEYS && self.active(time).is_some_and(|i| self.cameras[i].soft)
    }

    /// D-pad Up released, answered Yes (b407, `82613910` -> `82C7FD60`): remove the key at the cursor. A clip marker
    /// also takes every soft key in the clips on both sides, up to the next hard key each way (`82C7FB08`; retail's
    /// own text, ID_REPLAY_EDITOR_POPUP_CLIPMARKER). NOT RETAIL YET: retail also resets the merged clip's speed and
    /// shake (channel 0); our speed / shake lanes keep their values.
    pub fn remove_marker(&mut self, time: f64) -> bool {
        let Some(i) = self.key_near(time) else { return false };
        if self.cameras[i].soft {
            self.cameras.remove(i);
        } else {
            let lo = self.cameras[..i].iter().rposition(|k| !k.soft).map_or(0, |p| p + 1);
            let hi = self.cameras[i + 1..].iter().position(|k| !k.soft).map_or(self.cameras.len(), |p| i + 1 + p);
            self.cameras.drain(lo..hi);
        }
        self.editing = None;
        true
    }

    /// A lane's editor value at the cursor (`GetSimpleSpeedSetting` speed %, `GetShakeCamType` shake 0..3).
    pub fn lane_value(&self, lane: Lane, time: f64) -> i32 {
        self.edit.value_at(self.clip, lane, self.frame(time)).unwrap_or(0)
    }

    /// The in / out points in timeline frames from the recording's start (`GetStartHeadPosition` /
    /// `GetEndHeadPosition`: whole 30 fps frames, b398).
    pub fn head_frames(&self) -> (u32, u32) {
        let c = &self.edit.clips[self.clip];
        (c.start, c.end)
    }

    /// Left / Right in the effects menu: step the row's value at the cursor (a marker there, or the clip's value
    /// before the first marker / near the ends). Returns whether it changed.
    pub fn adjust_row(&mut self, time: f64, direction: i32) -> bool {
        let Some(lane) = EFFECT_ROWS.get(self.row).copied().flatten() else { return false };
        let frame = self.frame(time);
        let before = self.edit.value_at(self.clip, lane, frame);
        let spec = self.edit.limits.lanes[lane.index()];
        let next = spec.adjust(before.unwrap_or(spec.default), direction);
        if self.edit.set_marker(self.clip, lane, frame, next).is_err() {
            let _ = self.edit.adjust(self.clip, lane, direction);
        }
        self.edit.value_at(self.clip, lane, frame) != before
    }

    /// The retail editor mode value (`+0x30`) the screen is in.
    pub fn mode(&self) -> u8 {
        self.session.mode as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_over_the_recording_trims_marks_and_slows() {
        assert!(Editor::open(0.0, 4.0, EditLimits::default()).is_none(), "under 150 frames (5 s)");
        let mut e = Editor::open(10.0, 30.0, EditLimits::default()).unwrap();
        assert_eq!(e.heads(), (10.0, 30.0));
        assert_eq!(e.mode(), 6);
        e.trim_by(30, -60, 600);
        assert_eq!(e.heads(), (11.0, 28.0));
        // A full left stick for one second at 30 Hz: 0.005 x 600 frames x 30 = 90 frames = 3 s later in point.
        for _ in 0..60 {
            e.trim_sticks(1.0, 0.0, 1.0 / 60.0);
        }
        assert_eq!(e.heads(), (14.0, 28.0));
        e.trim_by(-90, 0, 600);
        // Speed 100 % until a lowered value at the cursor slows it from there.
        assert_eq!(e.speed(20.0), 1.0);
        e.drop_marker(20.0).unwrap();
        assert_eq!(e.cameras.len(), 1, "a camera key (the timeline's marker)");
        e.row = 7;
        assert!(e.adjust_row(20.0, -1));
        assert!((e.speed(21.0) - 0.9).abs() < 1e-6 && e.speed(15.0) == 1.0);
        assert_eq!(e.lane_value(Lane::Speed, 20.0), 90);
        // Camera rows take no Left / Right; shake wraps 0..3.
        e.row = 0;
        assert!(!e.adjust_row(20.0, 1));
        e.row = 8;
        assert!(e.adjust_row(20.0, -1));
        assert_eq!(e.lane_value(Lane::Shake, 21.0), 3);
        assert_eq!((e.skip(15.0, true), e.skip(25.0, false), e.skip(25.0, true)), (20.0, 20.0, 28.0));
        assert!(e.remove_marker(20.0));
        assert!(e.cameras.is_empty(), "the camera key went; the speed lane keeps its value");
        assert!((e.speed(21.0) - 0.9).abs() < 1e-6);
    }

    /// Camera keys (b405): picking a basic camera inserts a hard key at the playhead (the pose holds and cuts), an
    /// advanced camera a soft key (blends into the next soft key of the same camera); A on a key re-edits it.
    #[test]
    fn camera_keys_hold_cut_and_blend() {
        use bevy::prelude::{Transform, Vec3};
        let mut e = Editor::open(0.0, 20.0, EditLimits::default()).unwrap();
        let root = Transform::IDENTITY;
        let view = Transform::from_xyz(0.0, 2.0, 5.0);
        // No keys: the recorded camera.
        assert!(e.pose(5.0, &root).0.is_none());
        // Tripod (basic) at 5 s: holds from there.
        assert_eq!(e.pick_camera(1, 5.0, &view, &root), Some(8));
        assert!(e.pose(4.0, &root).0.is_none() && e.pose(9.0, &root).0.is_some());
        assert_eq!(e.active_row(9.0), 1);
        // Advanced Free keys at 10 s and 14 s: soft, blending between them.
        // (A new key starts from the camera in effect at the playhead; the eyes are then edited, set here directly.)
        assert_eq!(e.pick_camera(6, 10.0, &view, &root), Some(10));
        assert_eq!(e.pick_camera(6, 14.0, &view, &root), Some(10));
        for (i, x) in [(1, 0.0), (2, 8.0)] {
            e.cameras[i].camera = super::super::camera_key::CameraKey::Free { eye: Vec3::new(x, 2.0, 0.0), yaw: 0.0, pitch: 0.0 };
        }
        let mid = e.pose(12.0, &root).0.unwrap().translation;
        assert!((mid - Vec3::new(4.0, 2.0, 0.0)).length() < 1e-3, "{mid}");
        assert_eq!(e.active_row(12.0), 6);
        assert_eq!(e.key_fractions().len(), 3);
        assert_eq!(e.segments().iter().map(|s| s.1).collect::<Vec<_>>(), vec![3, 1, 2, 2]);
        // A on the Tripod key re-edits it (mode 8); not on a key: None.
        assert_eq!(e.reopen_at(5.0), Some(8));
        assert_eq!(e.reopen_at(7.0), None);
    }

    /// Deleting (b407): a soft key goes alone; a clip marker takes the soft keys in the clips on both sides, up to
    /// the next hard key each way. D-pad Down asks first only inside a run of soft keys.
    #[test]
    fn deleting_a_clip_marker_takes_the_key_markers_on_both_sides() {
        use bevy::prelude::Transform;
        let mut e = Editor::open(0.0, 30.0, EditLimits::default()).unwrap();
        let (root, view) = (Transform::IDENTITY, Transform::from_xyz(0.0, 2.0, 5.0));
        // Keys: hard 4, soft 8, soft 10, hard 14, soft 18, soft 20, hard 24.
        for (t, row) in [(4.0, 1), (8.0, 6), (10.0, 6), (14.0, 0), (18.0, 4), (20.0, 4), (24.0, 2)] {
            assert!(e.pick_camera(row, t, &view, &root).is_some(), "{t}");
        }
        assert_eq!(e.marker_kind(14.0), Some(MarkerKind::Clip));
        assert_eq!(e.marker_kind(18.0), Some(MarkerKind::Key));
        assert_eq!(e.marker_kind(16.0), None);
        assert!(e.drop_needs_confirm(9.0) && !e.drop_needs_confirm(16.0));
        // A key marker goes alone.
        assert!(e.remove_marker(20.0));
        assert_eq!(e.cameras.len(), 6);
        // The clip marker at 14 takes 8, 10 and 18; the hard keys at 4 and 24 stay.
        assert!(e.remove_marker(14.0));
        assert_eq!(e.cameras.iter().map(|k| (k.frame, k.soft)).collect::<Vec<_>>(), vec![(e.frame(4.0), false), (e.frame(24.0), false)]);
        assert!(!e.remove_marker(14.0));
    }
}
