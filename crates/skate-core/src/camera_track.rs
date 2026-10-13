//! ICE camera tracks (the cutscene Scene system's cameras, e.g. the Trick Guide demo's 21 ViewScene tracks in
//! scene.big `data/scene/ice_000.bin`; parsed by `skate_data::ice_camera`). A track keys the camera elements of
//! key group 0: EYE and LOOK (scene space, Z-up), DUTCH (roll), TANGENT_EYE / TANGENT_LOOK and LENS_LENGTH (mm).
//!
//! Sampling ports the TU3 segment setup sub_828F0BB0 and the tangent sub_828F0990: each element is a cubic Hermite
//! per segment, `((a*u + b)*u + c)*u + d` with u the segment fraction, a = m0 + m1 - 2D, b = 3D - m1 - 2*m0, c = m0,
//! d = p0, D = p1 - p0. The tangent at a segment end is the non-uniform Catmull-Rom tangent in the segment's own
//! parameter, `D*dt_n/(dt+dt_n) + D_n*(dt/dt_n)*dt/(dt+dt_n)` with the neighbour segment n (previous for m0, next
//! for m1; none at the track ends: D), scaled by the element's TANGENT value at that key.
//! NOT RETAIL YET: the spline switch (descriptor +60 = element 87) is assumed on for EYE / LOOK; LENS uses its
//! spline with tangent 1 (TANGENT_LENS sits in key group 1, unread) and DUTCH is linear (its descriptor's tangent
//! link is unread). Data-driven: tracks come from the disc; a mod can replace or add tracks by index.

/// sub_828F1E80: the film half-width (mm, 0x822F91F4) the lens is measured against.
pub const FILM_HALF_WIDTH_MM: f32 = 15.96;
/// LENS_LENGTH range (element 9): clamped before the angle.
pub const LENS_MIN_MM: f32 = 10.0;
pub const LENS_MAX_MM: f32 = 500.0;
/// LENS_LENGTH default (no key).
pub const LENS_DEFAULT_MM: f32 = 24.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraKey {
    /// Seconds from the track start.
    pub time: f32,
    pub eye: [f32; 3],
    pub look: [f32; 3],
    pub dutch: f32,
    pub tangent_eye: f32,
    pub tangent_look: f32,
    pub lens_mm: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraTrack {
    /// Track length in seconds (record +12); retail fits it to the playing clip ("Fixing Camera Group length").
    pub length: f32,
    pub keys: Vec<CameraKey>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraSample {
    pub eye: [f32; 3],
    pub look: [f32; 3],
    pub dutch: f32,
    pub lens_mm: f32,
}

impl CameraSample {
    /// sub_828F1E80: `2*atan(15.96 / lens)`, lens clamped to [10, 500] mm.
    pub fn fov_radians(&self) -> f32 {
        2.0 * (FILM_HALF_WIDTH_MM / self.lens_mm.clamp(LENS_MIN_MM, LENS_MAX_MM)).atan()
    }
}

/// One element's Hermite over segment `s` of `keys` (value by key, tangent scale by key or None = 1).
fn hermite(times: &[f32], value: impl Fn(usize) -> f32, scale: impl Fn(usize) -> f32, s: usize, u: f32) -> f32 {
    let (p0, p1) = (value(s), value(s + 1));
    let d = p1 - p0;
    let dt = times[s + 1] - times[s];
    // sub_828F0990: neighbour segment n, its delta and duration.
    let tangent = |n: Option<usize>| -> f32 {
        let Some(n) = n.filter(|n| n + 1 < times.len()) else { return d };
        let (dn, dtn) = (value(n + 1) - value(n), times[n + 1] - times[n]);
        let (mut own, mut other) = (1.0, 0.0);
        if dt + dtn > 0.0 {
            own = dtn / (dt + dtn);
            other = 1.0 - own;
            if dtn > 0.0 {
                other *= dt / dtn;
            }
        }
        dn * other + d * own
    };
    let m0 = tangent(s.checked_sub(1)) * scale(s);
    let m1 = tangent(Some(s + 1)) * scale(s + 1);
    let (a, b, c) = (m0 + m1 - 2.0 * d, 3.0 * d - m1 - 2.0 * m0, m0);
    ((a * u + b) * u + c) * u + p0
}

impl CameraTrack {
    /// The camera at `time` seconds into the track (clamped to the keys).
    pub fn sample(&self, time: f32) -> Option<CameraSample> {
        let keys = &self.keys;
        let first = keys.first()?;
        if keys.len() == 1 || time <= first.time {
            return Some(CameraSample { eye: first.eye, look: first.look, dutch: first.dutch, lens_mm: first.lens_mm });
        }
        let s = keys.windows(2).position(|w| time < w[1].time).unwrap_or(keys.len() - 2);
        let span = keys[s + 1].time - keys[s].time;
        let u = if span > 0.0 { ((time - keys[s].time) / span).clamp(0.0, 1.0) } else { 1.0 };
        let times: Vec<f32> = keys.iter().map(|k| k.time).collect();
        let axis = |pick: fn(&CameraKey) -> [f32; 3], tangent: fn(&CameraKey) -> f32| {
            [0, 1, 2].map(|i| hermite(&times, |k| pick(&keys[k])[i], |k| tangent(&keys[k]), s, u))
        };
        Some(CameraSample {
            eye: axis(|k| k.eye, |k| k.tangent_eye),
            look: axis(|k| k.look, |k| k.tangent_look),
            dutch: keys[s].dutch + (keys[s + 1].dutch - keys[s].dutch) * u,
            lens_mm: hermite(&times, |k| keys[k].lens_mm, |_| 1.0, s, u),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(time: f32, x: f32, tangent: f32) -> CameraKey {
        CameraKey { time, eye: [x, 0.0, 0.0], look: [x, 1.0, 0.0], tangent_eye: tangent, tangent_look: tangent,
            lens_mm: LENS_DEFAULT_MM, ..Default::default() }
    }

    #[test]
    fn keys_are_hit_and_two_keys_interpolate_linearly() {
        let track = CameraTrack { length: 2.0, keys: vec![key(0.0, 0.0, 1.0), key(2.0, 4.0, 1.0)] };
        assert_eq!(track.sample(0.0).unwrap().eye[0], 0.0);
        assert!((track.sample(2.0).unwrap().eye[0] - 4.0).abs() < 1e-5);
        // Both tangents are D with no neighbours: the Hermite is the straight line.
        assert!((track.sample(0.5).unwrap().eye[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn zero_tangent_eases_in_and_out() {
        let track = CameraTrack { length: 1.0, keys: vec![key(0.0, 0.0, 0.0), key(1.0, 1.0, 0.0)] };
        let v = track.sample(0.25).unwrap().eye[0];
        assert!((v - 0.15625).abs() < 1e-5, "{v}");
    }

    #[test]
    fn interior_key_takes_the_catmull_rom_tangent() {
        // Evenly spaced collinear keys: the spline stays on the line.
        let track = CameraTrack { length: 2.0, keys: vec![key(0.0, 0.0, 1.0), key(1.0, 1.0, 1.0), key(2.0, 2.0, 1.0)] };
        for t in [0.25, 0.5, 1.3, 1.9] {
            assert!((track.sample(t).unwrap().eye[0] - t).abs() < 1e-5, "{t}");
        }
    }

    #[test]
    fn lens_gives_the_retail_angle() {
        let s = CameraSample { eye: [0.0; 3], look: [0.0; 3], dutch: 0.0, lens_mm: LENS_DEFAULT_MM };
        assert!((s.fov_radians().to_degrees() - 67.24).abs() < 0.05);
    }
}
