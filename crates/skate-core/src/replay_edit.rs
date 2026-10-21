//! The replay editor's model (M2): clips trimmed out of the recording, each with its effect values and stepped
//! marker lanes, and retail's length rules. Pure data, engine independent; the game draws and plays it.
//!
//! Retail (FEReplayMenu host `823011D8`, editor state `DAT_83067084`; research b343, b344, b376 to b381):
//! - The editor opens once the recording holds at least 150 frames (`8250B8F8`, > 149).
//! - The timeline counts 30 fps frames (`825ADD90`, 30.0 at 0x820D4924).
//! - Encoded length = round(sum over clips of (end - start) / (speed% x 0.01) / 30 + 0.5) s; sharing needs <= 60 s
//!   (`825ADF88`, `cmplwi 60`).
//! - Effect values per clip (record +0x50 speed, +0x54.. the look): speed 10..100 % step 10 default 100 (slows only;
//!   `82506CD8` stores 100), shake 0..3 wrapping (`8250A6E8`), brightness -100..100 step 20, contrast 10..30 step 2,
//!   saturation 0..100 step 10, tint R / G / B / A 0..250 step 25 (`8250AB28`, `8250B218`).
//! - Effect lanes: per lane an array of markers; a lane's value steps at a marker's time (`8250CC10` writes the value
//!   at the marker and again 1 ms later, so no ramp).
//! - Lane values reach the screen as: brightness = the colour matrix add term, contrast, saturation (near and far
//!   bands), each replacing the world block while set; tint = an RGBA screen overlay (b380).
//!
//! Every limit is data ([`EditLimits`], [`LaneSpec`]) so a mod can change them (for example allow speeds over 100 %).

use crate::colour_matrix::ColourMatrixRecord;

/// The effect lanes, in retail's lane order after the camera lane (0). Stable ids: [`Lane::id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lane {
    Speed,
    Shake,
    Brightness,
    Contrast,
    Saturation,
    TintR,
    TintG,
    TintB,
    TintA,
}

pub const LANES: [Lane; 9] = [
    Lane::Speed,
    Lane::Shake,
    Lane::Brightness,
    Lane::Contrast,
    Lane::Saturation,
    Lane::TintR,
    Lane::TintG,
    Lane::TintB,
    Lane::TintA,
];

impl Lane {
    pub fn index(self) -> usize {
        self as usize
    }

    /// Stable string id (mod records, saved edits).
    pub fn id(self) -> &'static str {
        match self {
            Lane::Speed => "speed",
            Lane::Shake => "shake",
            Lane::Brightness => "brightness",
            Lane::Contrast => "contrast",
            Lane::Saturation => "saturation",
            Lane::TintR => "tint_r",
            Lane::TintG => "tint_g",
            Lane::TintB => "tint_b",
            Lane::TintA => "tint_a",
        }
    }

    pub fn from_id(id: &str) -> Option<Lane> {
        LANES.into_iter().find(|l| l.id() == id)
    }
}

/// One lane's range: integer editor values, `scale` turns them into the effect value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneSpec {
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: i32,
    /// Past the end wraps to the other end (retail's shake) instead of clamping.
    pub wraps: bool,
    pub scale: f32,
}

impl LaneSpec {
    /// Retail's ranges. Defaults: speed 100 [code]; the look defaults are the neutral values (brightness 0,
    /// contrast 10 = x1, saturation 100 = x1, tint 0 = overlay off) [ours: retail's look defaults not read yet].
    pub fn retail(lane: Lane) -> LaneSpec {
        let spec = |min, max, step, default, scale| LaneSpec { min, max, step, default, wraps: false, scale };
        match lane {
            Lane::Speed => spec(10, 100, 10, 100, 0.01),
            Lane::Shake => LaneSpec { wraps: true, ..spec(0, 3, 1, 0, 1.0) },
            Lane::Brightness => spec(-100, 100, 20, 0, 0.01),
            Lane::Contrast => spec(10, 30, 2, 10, 0.1),
            Lane::Saturation => spec(0, 100, 10, 100, 0.01),
            Lane::TintR | Lane::TintG | Lane::TintB | Lane::TintA => spec(0, 250, 25, 0, 1.0 / 255.0),
        }
    }

    /// One press up (+1) or down (-1).
    pub fn adjust(&self, value: i32, direction: i32) -> i32 {
        let next = value + self.step * direction.signum();
        if self.wraps {
            if next > self.max {
                self.min
            } else if next < self.min {
                self.max
            } else {
                next
            }
        } else {
            next.clamp(self.min, self.max)
        }
    }
}

/// The editor's limits, data driven.
#[derive(Clone, Debug, PartialEq)]
pub struct EditLimits {
    /// Frames the recording must hold before the editor opens (retail 150, at 30 fps = 5 s).
    pub min_frames_to_open: u32,
    /// The timeline's frame rate (retail 30).
    pub timeline_fps: f32,
    /// Longest encoded length that can be shared (retail 60 s).
    pub share_max_seconds: u32,
    /// Markers per lane (retail refuses at 100: `82613500`).
    pub max_markers_per_lane: usize,
    /// A key within this fraction of the clip from the cursor is "at the cursor", and no key goes this close to
    /// the clip's ends (retail 0.0089 of the normalised timeline, 0x822428EC / 0.9911 at 0x822F8F74, `825084C8`,
    /// research b382; the unit, the clip span, is inferred).
    pub marker_spacing: f32,
    pub lanes: [LaneSpec; 9],
}

impl Default for EditLimits {
    fn default() -> Self {
        Self {
            min_frames_to_open: 150,
            timeline_fps: 30.0,
            share_max_seconds: 60,
            max_markers_per_lane: 100,
            marker_spacing: 0.0089,
            lanes: LANES.map(LaneSpec::retail),
        }
    }
}

/// One clip: a span of the recording in timeline frames, its own effect values, and marker lanes.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub start: u32,
    pub end: u32,
    /// The clip's value per lane (used where the lane has no marker at or before the frame).
    pub values: [i32; 9],
    /// Per lane, markers (frame, value) sorted by frame.
    pub markers: [Vec<(u32, i32)>; 9],
}

/// Retail's trigger scrub, data driven. Per console frame (`frame_hz`, the 360's ~30 fps): target = (RT - LT) x
/// `max_rate`, the rate moves `smoothing` of the way to it, clamps to +-`max_rate`, and stops inside the dead band;
/// the playhead moves by the rate as a fraction of the whole recording (`82610F18` -> `825071B8` -> `82C7EF90`,
/// research b387 / b388).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrubSpec {
    pub max_rate: f32,
    pub smoothing: f32,
    pub dead_target: f32,
    pub dead_rate: f32,
    pub frame_hz: f32,
}

impl Default for ScrubSpec {
    fn default() -> Self {
        Self { max_rate: 0.005, smoothing: 0.25, dead_target: 1.0e-5, dead_rate: 1.0e-4, frame_hz: 30.0 }
    }
}

/// The scrub's state; [`ScrubFilter::step`] runs whole console frames, so any engine frame rate gives the same
/// movement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrubFilter {
    pub rate: f32,
    carry: f32,
}

impl ScrubFilter {
    /// Advance by `dt` seconds with the trigger axis `axis` (RT - LT, -1..1); returns how far the playhead moves, as
    /// a fraction of the recording.
    pub fn step(&mut self, spec: &ScrubSpec, axis: f32, dt: f32) -> f32 {
        self.carry += dt.clamp(0.0, 0.25) * spec.frame_hz;
        let target = axis.clamp(-1.0, 1.0) * spec.max_rate;
        let mut moved = 0.0;
        // A frame due up to float error counts now (144 steps of 30/144 sum to 29.9999...).
        while self.carry >= 1.0 - 1.0e-4 {
            self.carry -= 1.0;
            self.rate = (self.rate + (target - self.rate) * spec.smoothing).clamp(-spec.max_rate, spec.max_rate);
            if target.abs() < spec.dead_target && self.rate.abs() <= spec.dead_rate {
                self.rate = 0.0;
            }
            moved += self.rate;
        }
        moved
    }
}

/// A clip's look at one frame: the colour matrix record to build ([`crate::colour_matrix::build`]) and the RGBA
/// screen tint overlay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayLook {
    pub record: ColourMatrixRecord,
    pub tint: [f32; 4],
}

/// The edit: clips in play order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayEdit {
    pub limits: EditLimits,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    NoClip,
    EmptySpan,
    /// Retail's "no more markers" popup (`ID_REPLAY_EDITOR_NO_MORE_MARKERS`).
    NoMoreMarkers,
    OutsideClip,
    /// Too close to the clip's in / out point (retail's error sound, `82612C28`).
    TooClose,
}

impl ReplayEdit {
    pub fn new(limits: EditLimits) -> Self {
        Self { limits, clips: Vec::new() }
    }

    /// Whether the editor can open on a recording of `frames` timeline frames.
    pub fn can_open(&self, frames: u32) -> bool {
        frames >= self.limits.min_frames_to_open
    }

    /// A new clip over `start..end` (timeline frames) with the default values; returns its index.
    pub fn add_clip(&mut self, start: u32, end: u32) -> Result<usize, EditError> {
        if end <= start {
            return Err(EditError::EmptySpan);
        }
        let values = self.limits.lanes.map(|s| s.default);
        self.clips.push(Clip { start, end, values, markers: Default::default() });
        Ok(self.clips.len() - 1)
    }

    /// Move a clip's in / out points; markers outside the new span are dropped.
    pub fn trim(&mut self, clip: usize, start: u32, end: u32) -> Result<(), EditError> {
        if end <= start {
            return Err(EditError::EmptySpan);
        }
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        c.start = start;
        c.end = end;
        for lane in &mut c.markers {
            lane.retain(|(f, _)| (start..end).contains(f));
        }
        Ok(())
    }

    /// One press on a clip's value (speed, look): step and clamp (or wrap) by the lane's spec.
    pub fn adjust(&mut self, clip: usize, lane: Lane, direction: i32) -> Result<i32, EditError> {
        let spec = self.limits.lanes[lane.index()];
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        let v = &mut c.values[lane.index()];
        *v = spec.adjust(*v, direction);
        Ok(*v)
    }

    /// Set a lane's value from `frame` on. A marker within the spacing of `frame` is "at the cursor" and takes the
    /// value; a new marker needs room from the clip's ends and the lane below its cap (retail plays its error sound
    /// and adds nothing otherwise).
    pub fn set_marker(&mut self, clip: usize, lane: Lane, frame: u32, value: i32) -> Result<(), EditError> {
        let spec = self.limits.lanes[lane.index()];
        let cap = self.limits.max_markers_per_lane;
        let spacing = self.limits.marker_spacing;
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        if !(c.start..c.end).contains(&frame) {
            return Err(EditError::OutsideClip);
        }
        let span = (c.end - c.start) as f32;
        let near = (spacing * span).floor() as u32;
        let value = value.clamp(spec.min, spec.max);
        let markers = &mut c.markers[lane.index()];
        if let Some(m) = markers.iter_mut().find(|m| m.0.abs_diff(frame) <= near) {
            m.1 = value;
            return Ok(());
        }
        let at = (frame - c.start) as f32 / span;
        if at <= spacing || at >= 1.0 - spacing {
            return Err(EditError::TooClose);
        }
        if markers.len() >= cap {
            return Err(EditError::NoMoreMarkers);
        }
        let i = markers.partition_point(|m| m.0 < frame);
        markers.insert(i, (frame, value));
        Ok(())
    }

    pub fn remove_marker(&mut self, clip: usize, lane: Lane, frame: u32) -> Result<bool, EditError> {
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        let markers = &mut c.markers[lane.index()];
        Ok(markers.binary_search_by_key(&frame, |m| m.0).map(|i| markers.remove(i)).is_ok())
    }

    /// A lane's editor value at `frame`: the last marker at or before it, else the clip's value (stepped, no ramp).
    pub fn value_at(&self, clip: usize, lane: Lane, frame: u32) -> Option<i32> {
        let c = self.clips.get(clip)?;
        let markers = &c.markers[lane.index()];
        let i = markers.partition_point(|m| m.0 <= frame);
        Some(if i > 0 { markers[i - 1].1 } else { c.values[lane.index()] })
    }

    /// The effect value (editor value x the lane's scale), e.g. speed 50 -> 0.5.
    pub fn effect_at(&self, clip: usize, lane: Lane, frame: u32) -> Option<f32> {
        self.value_at(clip, lane, frame).map(|v| v as f32 * self.limits.lanes[lane.index()].scale)
    }

    /// The look at `frame`: the world grade `base` with the clip's look lanes replacing their terms (retail's
    /// consumers `82686DF0` / `82687FD8` / `826888B8` write an override copy of the world colour block, research b380):
    /// brightness = the add term on R, G, B, contrast, saturation, in both bands (retail's far band takes the near
    /// saturation). A lane at its neutral default leaves the world's term (OURS: when retail counts a lane as set is
    /// not read). Plus the tint lanes as an RGBA screen overlay (0 alpha = none).
    pub fn look_at(&self, clip: usize, frame: u32, base: &ColourMatrixRecord) -> Option<ReplayLook> {
        let set = |lane: Lane| {
            let v = self.value_at(clip, lane, frame)?;
            let spec = self.limits.lanes[lane.index()];
            (v != spec.default).then_some(v as f32 * spec.scale)
        };
        let mut record = *base;
        for band in [&mut record.near, &mut record.far] {
            if let Some(b) = set(Lane::Brightness) {
                band.add = [b; 3];
            }
            if let Some(c) = set(Lane::Contrast) {
                band.contrast = c;
            }
            if let Some(s) = set(Lane::Saturation) {
                band.saturation = s;
            }
        }
        let tint = [Lane::TintR, Lane::TintG, Lane::TintB, Lane::TintA]
            .map(|l| self.effect_at(clip, l, frame).unwrap_or(0.0));
        Some(ReplayLook { record, tint })
    }

    /// Retail's encoded length in seconds: each clip's span stretched by its speed, at the timeline rate, rounded.
    pub fn encoded_seconds(&self) -> u32 {
        let speed = self.limits.lanes[Lane::Speed.index()];
        let frames: f32 = self
            .clips
            .iter()
            .map(|c| (c.end - c.start) as f32 / (c.values[Lane::Speed.index()] as f32 * speed.scale).max(1.0e-3))
            .sum();
        (frames / self.limits.timeline_fps + 0.5) as u32
    }

    /// Retail's "surpassed the designated sharing length" check.
    pub fn can_share(&self) -> bool {
        self.encoded_seconds() <= self.limits.share_max_seconds
    }
}

pub mod session;
pub mod curve;
pub mod shake;

#[cfg(test)]
mod tests;
