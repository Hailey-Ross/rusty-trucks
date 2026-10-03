//! PeakingIir2 (PI20, `sub_82B2C658`; spec `aems-voice-graph-spec.md` §4.10): an RBJ peaking EQ.
//! Parameters: 0 = centre (Hz), 1 = linear gain, 2 = Q. Class defaults 96000 Hz / 1.0 / 3.0. The
//! module bypasses (pass-through, history cleared once) only when the gain is exactly 1.0; ω is
//! clamped to [FLOOR, CEIL] and Q to 0.2..20 for the coefficients. Same Direct Form I kernel as
//! the high/low-pass ([`super::biquad::kernel`]). Not bit-exact against retail (no replay vectors:
//! the coefficient arithmetic is RBJ in f32 with libm trig, UNCERTAIN to the ulp).
use super::biquad::{CEIL, Coefficients, FLOOR, kernel, omega};

#[derive(Clone, Debug)]
pub struct PeakingIir2 {
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    cached: Option<[u32; 3]>,
    coefficients: Coefficients,
    history: [[f32; 4]; 8],
    filtering: bool,
}

impl Default for PeakingIir2 {
    fn default() -> Self {
        Self { freq: 96_000.0, gain: 1.0, q: 3.0, cached: None, coefficients: Coefficients::default(), history: [[0.0; 4]; 8], filtering: false }
    }
}

/// RBJ peaking coefficients (A = √gain, α = sin ω / 2Q), normalised by a0, in f32.
pub fn coefficients(freq: f32, gain: f32, q: f32, rate: f32) -> Coefficients {
    let w = omega(freq, rate);
    let w = if w.is_nan() { CEIL } else { w.clamp(FLOOR, CEIL) };
    let q = q.clamp(0.2, 20.0);
    let a = gain.max(0.0).sqrt();
    let s = (w as f64).sin() as f32;
    let c = (w as f64).cos() as f32;
    let alpha = s / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    let inv = 1.0 / a0;
    Coefficients {
        b0: (1.0 + alpha * a) * inv,
        b1: (-2.0 * c) * inv,
        b2: (1.0 - alpha * a) * inv,
        a1: (-2.0 * c) * inv,
        a2: (1.0 - alpha / a) * inv,
    }
}

impl PeakingIir2 {
    pub fn bypassed(&self) -> bool {
        self.gain == 1.0
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]], rate: f32) {
        if self.gain == 1.0 {
            if self.filtering {
                self.history = [[0.0; 4]; 8];
            }
            self.filtering = false;
            return;
        }
        let key = [self.freq.to_bits(), self.gain.to_bits(), self.q.to_bits()];
        if self.cached != Some(key) {
            self.coefficients = coefficients(self.freq, self.gain, self.q, rate);
            self.cached = Some(key);
        }
        self.filtering = true;
        for (ch, samples) in channels.iter_mut().enumerate().take(8) {
            kernel(&self.coefficients, &mut self.history[ch], samples);
        }
    }
}

/// DCl0 (`sub_82B22678`): hard clip to ±level; bypass when level ≥ 100 or NaN.
pub fn clip(channels: &mut [&mut [f32]], level: f32) {
    if level.is_nan() || level >= 100.0 {
        return;
    }
    for ch in channels.iter_mut() {
        for s in ch.iter_mut() {
            *s = s.clamp(-level, level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_gain(eq: &mut PeakingIir2, f: f32) -> f32 {
        let mut x: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * f * i as f32 / 48000.0).sin()).collect();
        for chunk in x.chunks_mut(256) {
            eq.process(&mut [chunk], 48000.0);
        }
        let tail = &x[24000..];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt() * std::f32::consts::SQRT_2
    }

    #[test]
    fn peak_gain_at_the_centre_and_flat_far_away() {
        let mut eq = PeakingIir2 { freq: 1000.0, gain: 2.0, q: 1.0, ..Default::default() };
        assert!((tone_gain(&mut eq, 1000.0) - 2.0).abs() < 0.02);
        let mut eq = PeakingIir2 { freq: 1000.0, gain: 0.25, q: 1.0, ..Default::default() };
        assert!((tone_gain(&mut eq, 1000.0) - 0.25).abs() < 0.01);
        assert!((tone_gain(&mut eq, 20.0) - 1.0).abs() < 0.03);
    }

    #[test]
    fn gain_one_is_a_bit_exact_bypass() {
        let mut eq = PeakingIir2 { freq: 500.0, ..Default::default() };
        let mut x: Vec<f32> = (0..256).map(|i| (i as f32 * 0.3).sin()).collect();
        let before = x.clone();
        eq.process(&mut [&mut x[..]], 48000.0);
        assert_eq!(x, before);
        let mut c = vec![0.5f32, -0.2, 0.09];
        clip(&mut [&mut c[..]], 0.1);
        assert_eq!(c, vec![0.1, -0.1, 0.09]);
        clip(&mut [&mut c[..]], 100.0);
    }
}
