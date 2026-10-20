//! The replay editor's camera shake (the Pro Edit Menu's Shake row; research b376, b399, b400). Retail plays short
//! recorded handheld-camera clips (`data/scene/ICELite.bin`, 30 keys per second, six channels: rotation x / y / z and
//! position x / y / z, each -1..1) through the evaluator `82C8AF50`, which builds a rotation + translation and
//! multiplies it into the camera matrix (camera space). Levels (`8250A6E8`): Off; Low = clip 3 ("Jog") at 0.35; Medium
//! = clip 3 at 1.0; High = clip 4 ("RunFast") at 1.0; the same value is the amplitude and the frequency. Every value
//! is data (mod-overridable).

/// The clips as setup exported them: per clip its keys `[qx, qy, qz, px, py, pz]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShakeClips {
    pub key_rate: f32,
    pub clips: Vec<Vec<[f32; 6]>>,
}

/// One shake level: which clip, its amplitude and frequency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShakeLevel {
    pub clip: usize,
    pub amplitude: f32,
    pub frequency: f32,
}

/// Retail's levels 0..3 (`8250A6E8`, b376): Off, Low, Medium, High.
pub fn retail_levels() -> [Option<ShakeLevel>; 4] {
    let level = |clip, v| Some(ShakeLevel { clip, amplitude: v, frequency: v });
    [None, level(3, 0.35), level(3, 1.0), level(4, 1.0)]
}

/// `2 x sin(R)` of retail's gain `g = 2 x amp x sin(R)`. NOT RETAIL YET: R is a global angle (setter `82674C08`) whose
/// writers are not found (b400), so ours holds 2 sin(R) at 1 (R = 30 degrees).
pub const GAIN_FACTOR: f32 = 1.0;

/// The shake at `t` seconds into the take: (rotation quaternion `[x, y, z, w]`, translation `[x, y, z]`) in camera
/// space, or None when off or the clip is empty. Key position `f = t x freq x key_rate`, keys `i = floor(f) mod n` and
/// `i + 1`; rotation per key `(qx g, qz g, -qy g)` with `w = sqrt(1 - |xyz|)` (retail's double square root, confirmed
/// in the recomp), lerped per component without renormalising; translation `(-pz g, -px g, py g)` from key `i` only
/// (retail reads key `i` twice) [code `82C8AF50`, b399].
pub fn sample(clips: &ShakeClips, level: Option<ShakeLevel>, t: f64) -> Option<([f32; 4], [f32; 3])> {
    let level = level?;
    let keys = clips.clips.get(level.clip).filter(|k| !k.is_empty())?;
    let n = keys.len();
    let f = t.max(0.0) * f64::from(level.frequency) * f64::from(clips.key_rate);
    let i = (f.floor() as usize) % n;
    let j = (i + 1) % n;
    let a = (f - f.floor()) as f32;
    let g = level.amplitude * GAIN_FACTOR;
    let rotation = |k: &[f32; 6]| {
        let (x, y, z) = (k[0] * g, k[2] * g, -k[1] * g);
        let w = (1.0 - (x * x + y * y + z * z).sqrt()).max(0.0).sqrt();
        [x, y, z, w]
    };
    let (qi, qj) = (rotation(&keys[i]), rotation(&keys[j]));
    let q = [0, 1, 2, 3].map(|c| qi[c] + (qj[c] - qi[c]) * a);
    let k = &keys[i];
    Some((q, [-k[5] * g, -k[3] * g, k[4] * g]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_and_empty_clips_do_nothing_and_keys_step_at_the_clip_rate() {
        let mut clips = ShakeClips { key_rate: 30.0, clips: vec![Vec::new(); 5] };
        clips.clips[3] = vec![[0.1, 0.0, 0.0, 0.2, 0.0, 0.0], [0.3, 0.0, 0.0, 0.4, 0.0, 0.0]];
        let levels = retail_levels();
        assert!(sample(&clips, levels[0], 1.0).is_none(), "Off");
        assert!(sample(&clips, levels[3], 1.0).is_none(), "empty clip");
        // Medium (amp = freq = 1): t = 0.5 / 30 s is halfway from key 0 to key 1.
        let (q, p) = sample(&clips, levels[2], 0.5 / 30.0).unwrap();
        assert!((q[0] - 0.2).abs() < 1e-6, "{q:?}");
        // Position steps: key 0's, not halfway; mapped to (-pz, -px, py).
        assert_eq!(p, [-0.0, -0.2, 0.0]);
        // Low (0.35) scales the rotation and runs the clip at 0.35 of the rate.
        let (q, _) = sample(&clips, levels[1], 0.0).unwrap();
        assert!((q[0] - 0.035).abs() < 1e-6);
        // Wraps after the last key.
        let (q, _) = sample(&clips, levels[2], 2.0 / 30.0).unwrap();
        assert!((q[0] - 0.1).abs() < 1e-6);
    }
}
