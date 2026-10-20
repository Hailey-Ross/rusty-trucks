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
    /// The clip's camera key and lens (mm), and the camera limits / rates (data).
    pub camera: super::camera_key::CameraKey,
    pub lens: f32,
    pub camera_rules: super::camera_key::CameraRules,
}

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
        Some(Self { session, edit, clip, row: 0, base: start, fps, frames, trim_carry: [0.0; 2], trimming_end: false, camera: super::camera_key::CameraKey::Game, lens: super::camera_key::CameraRules::default().lens_default, camera_rules: Default::default() })
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

    /// The markers as fractions of the recording (the timeline strip's x / 817, b403).
    pub fn key_fractions(&self) -> Vec<f32> {
        let c = &self.edit.clips[self.clip];
        c.markers[Lane::Speed.index()].iter().map(|m| m.0 as f32 / self.frames.max(1) as f32).collect()
    }

    /// D-pad Left / Right released: the previous / next marker (the screen's "Skip to previous / next marker"),
    /// else the in / out point (b386).
    pub fn skip(&self, time: f64, forward: bool) -> f64 {
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        let markers = c.markers[Lane::Speed.index()].iter().map(|m| m.0);
        let target = if forward {
            markers.filter(|&f| f > frame).min().unwrap_or(c.end)
        } else {
            markers.filter(|&f| f < frame).max().unwrap_or(c.start)
        };
        self.time(target)
    }

    /// D-pad Down released: a speed marker at the cursor with the value there (retail drops a key; refused near the
    /// ends or at the cap with its error sound / popup).
    pub fn drop_marker(&mut self, time: f64) -> Result<(), EditError> {
        let frame = self.frame(time);
        let value = self.edit.value_at(self.clip, Lane::Speed, frame).ok_or(EditError::NoClip)?;
        self.edit.set_marker(self.clip, Lane::Speed, frame, value)
    }

    /// D-pad Up released: remove the marker at the cursor (within the marker spacing).
    pub fn remove_marker(&mut self, time: f64) -> bool {
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        let near = (self.edit.limits.marker_spacing * (c.end - c.start) as f32).floor() as u32;
        let Some(&(at, _)) = c.markers[Lane::Speed.index()].iter().find(|m| m.0.abs_diff(frame) <= near) else {
            return false;
        };
        self.edit.remove_marker(self.clip, Lane::Speed, at).unwrap_or(false)
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
        // Speed 100 % until a marker slows it.
        assert_eq!(e.speed(20.0), 1.0);
        e.drop_marker(20.0).unwrap();
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
        assert_eq!(e.speed(21.0), 1.0);
    }
}
