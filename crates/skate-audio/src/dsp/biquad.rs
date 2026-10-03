//! HighPassIir2 / LowPassIir2 (spec §4.4, §4.5): RBJ cookbook biquads at fixed Q = 1, single
//! precision, Direct Form I with a 1e-18 denormal bias. The cutoff is raw Hz; the filter bypasses
//! (bit-exact pass-through) outside 24 Hz … 0.999·Nyquist, so a raw 25000 Hz low-pass is "open"
//! because it lies past 0.999·Nyquist, not because of a special case.

/// 2π as f32 (6.2831855).
pub const TWO_PI: f32 = std::f32::consts::TAU;
/// ω floor π/1000 (≈ 24 Hz at 48 kHz) and ceiling 0.999π (≈ 23976 Hz), image constants.
pub const FLOOR: f32 = 0.003_141_593;
pub const CEIL: f32 = 3.138_451_1;
/// Denormal guard added to the feed-forward sum (cell 0x822F87B0).
pub const BIAS: f32 = 1e-18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    LowPass,
    HighPass,
}

/// Normalised coefficients {a1, a2, b0, b1, b2}.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coefficients {
    pub a1: f32,
    pub a2: f32,
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
}

/// RBJ low/high-pass at Q = 1 from ω, every step in f32 (sin/cos in f64, rounded once).
pub fn coefficients(kind: Kind, omega: f32) -> Coefficients {
    let s = (omega as f64).sin() as f32;
    let c = (omega as f64).cos() as f32;
    let alpha = 0.5 * s;
    let a0 = 1.0 + alpha;
    let inv = 1.0 / a0;
    let a1 = (-2.0 * c) * inv;
    let a2 = (1.0 - alpha) * inv;
    let (n, sign) = match kind {
        Kind::LowPass => (1.0 - c, 1.0f32),
        Kind::HighPass => (1.0 + c, -1.0f32),
    };
    let b0 = n / (2.0 * a0);
    Coefficients { a1, a2, b0, b1: sign * (n * inv), b2: b0 }
}

/// ω for a cutoff at the block rate: f32(f32(fc / fs) × 2π).
pub fn omega(cutoff: f32, rate: f32) -> f32 {
    (cutoff / rate) * TWO_PI
}

#[derive(Clone, Debug)]
pub struct Iir2 {
    pub kind: Kind,
    /// Parameter 0: cutoff in Hz.
    pub cutoff: f32,
    cached: f32,
    coefficients: Coefficients,
    /// Per channel {x1, x2, y1, y2}.
    history: [[f32; 4]; 8],
    filtering: bool,
}

impl Iir2 {
    /// Class defaults: HPF 0 Hz, LPF 96000 Hz (both bypass).
    pub fn new(kind: Kind) -> Self {
        let cutoff = match kind {
            Kind::LowPass => 96_000.0,
            Kind::HighPass => 0.0,
        };
        // The constructor caches the raw Hz, so the first filtering block always rebuilds.
        Self { kind, cutoff, cached: cutoff, coefficients: Coefficients::default(), history: [[0.0; 4]; 8], filtering: false }
    }

    /// True when this block will pass through untouched.
    pub fn bypassed(&self, rate: f32) -> bool {
        let w = omega(self.cutoff, rate);
        match self.kind {
            Kind::LowPass => w.is_nan() || w >= CEIL,
            Kind::HighPass => w.is_nan() || w <= FLOOR,
        }
    }

    /// Process one block in place (planar channels).
    pub fn process(&mut self, channels: &mut [&mut [f32]], rate: f32) {
        let w = omega(self.cutoff, rate);
        let bypass = match self.kind {
            Kind::LowPass => w.is_nan() || w >= CEIL,
            Kind::HighPass => w.is_nan() || w <= FLOOR,
        };
        if bypass {
            if self.filtering {
                self.history = [[0.0; 4]; 8];
            }
            self.filtering = false;
            self.cached = w;
            return;
        }
        let w = match self.kind {
            Kind::LowPass => w.max(FLOOR),
            Kind::HighPass => w.min(CEIL),
        };
        if w.to_bits() != self.cached.to_bits() || !self.filtering && self.coefficients == Coefficients::default() {
            self.coefficients = coefficients(self.kind, w);
            self.cached = w;
        }
        self.filtering = true;
        for (ch, samples) in channels.iter_mut().enumerate().take(8) {
            kernel(&self.coefficients, &mut self.history[ch], samples);
        }
    }
}

/// Direct Form I in place over one block of one channel, history {x1, x2, y1, y2}.
///
/// Feed-forward b0·x + b1·x1 + b2·x2 + 1e-18 and feedback y = (t − a1·y1) − a2·y2 in single
/// precision. Retail processes 8 samples at a time and associates the feed-forward sum by the
/// sample's position in its group of 8 (groups aligned to the block start). The order below was
/// fitted black-box against the PoC's replay-verified kernel (`tests/dsp_oracle.rs`
/// `fit_biquad_association`: every position 100 % bit-exact) — the innermost product is a plain
/// multiply plus the bias, the outer two are fused multiply-adds:
/// - position 0: b1·x1 + (b2·x2 + (b0·x + bias));
/// - position 1: b2·x2 + (b1·x1 + (b0·x + bias));
/// - positions 2..7: b0·x + (b2·x2 + (b1·x1 + bias));
/// - feedback: two fused negative multiply-subtracts, a1 first.
pub fn kernel(k: &Coefficients, history: &mut [f32; 4], samples: &mut [f32]) {
    let [mut x1, mut x2, mut y1, mut y2] = *history;
    for (n, s) in samples.iter_mut().enumerate() {
        let x = *s;
        let t = match n % 8 {
            0 => k.b1.mul_add(x1, k.b2.mul_add(x2, k.b0 * x + BIAS)),
            1 => k.b2.mul_add(x2, k.b1.mul_add(x1, k.b0 * x + BIAS)),
            _ => k.b0.mul_add(x, k.b2.mul_add(x2, k.b1 * x1 + BIAS)),
        };
        let y = (-k.a2).mul_add(y2, (-k.a1).mul_add(y1, t));
        x2 = x1;
        x1 = x;
        y2 = y1;
        y1 = y;
        *s = y;
    }
    *history = [x1, x2, y1, y2];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 2.0 * f32::EPSILON * b.abs().max(1.0)
    }

    #[test]
    fn worked_coefficients_from_the_spec() {
        for (kind, fc, want) in [
            (Kind::LowPass, 5000.0, [-1.2164445, 0.53329474, 0.079212569, 0.15842514]),
            (Kind::LowPass, 1000.0, [-1.8614084, 0.87747043, 0.0040154932, 0.0080309864]),
            (Kind::HighPass, 77.0, [-1.9898703, 0.98997140, 0.99496043, -1.9899209]),
        ] {
            let c = coefficients(kind, omega(fc, 48000.0));
            for (got, want) in [c.a1, c.a2, c.b0, c.b1].into_iter().zip(want) {
                assert!(close(got, want), "{kind:?} {fc}: {got} vs {want}");
            }
            assert_eq!(c.b0, c.b2);
        }
        assert_eq!(omega(5000.0, 48000.0), 0.654_498_46);
    }

    #[test]
    fn bypass_edges() {
        let mut lpf = Iir2::new(Kind::LowPass);
        for (fc, bypass) in [(25000.0, true), (23976.0, true), (23975.0, false), (96000.0, true)] {
            lpf.cutoff = fc;
            assert_eq!(lpf.bypassed(48000.0), bypass, "LPF {fc}");
        }
        let mut hpf = Iir2::new(Kind::HighPass);
        for (fc, bypass) in [(0.0, true), (24.0, true), (25.0, false), (77.0, false)] {
            hpf.cutoff = fc;
            assert_eq!(hpf.bypassed(48000.0), bypass, "HPF {fc}");
        }
        // Bypass is a bit-exact pass-through.
        let mut x: Vec<f32> = (0..256).map(|i| (i as f32 * 0.1).sin()).collect();
        let before = x.clone();
        lpf.cutoff = 25000.0;
        lpf.process(&mut [&mut x[..]], 48000.0);
        assert_eq!(x, before);
    }

    #[test]
    fn low_pass_attenuates_above_cutoff_and_passes_below() {
        let rms = |fc: f32, f: f32| {
            let mut lpf = Iir2::new(Kind::LowPass);
            lpf.cutoff = fc;
            let mut x: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * f * i as f32 / 48000.0).sin()).collect();
            for chunk in x.chunks_mut(256) {
                lpf.process(&mut [chunk], 48000.0);
            }
            let tail = &x[24000..];
            (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt() * std::f32::consts::SQRT_2
        };
        // 0 dB at fc for Q = 1 (RBJ), strong cut two octaves above, flat well below.
        assert!((rms(1000.0, 1000.0) - 1.0).abs() < 0.02);
        assert!(rms(1000.0, 4000.0) < 0.08);
        assert!((rms(1000.0, 100.0) - 1.0).abs() < 0.02);
    }

    #[test]
    fn history_clears_when_switching_to_bypass() {
        let mut lpf = Iir2::new(Kind::LowPass);
        lpf.cutoff = 1000.0;
        let mut x = vec![1.0f32; 256];
        lpf.process(&mut [&mut x[..]], 48000.0);
        lpf.cutoff = 25000.0;
        lpf.process(&mut [&mut x[..]], 48000.0);
        assert_eq!(lpf.history[0], [0.0; 4]);
    }
}
