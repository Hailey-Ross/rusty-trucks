//! FrequencyShiftSsb (`FSS0`, process `sub_82B22898`, constructor `sub_82B22770`, size
//! `sub_82B22738`; spec `.claude/notes/aems-grain-chain-spec.md` §1): a single-sideband frequency
//! shifter, mono.
//!
//! Per 256-frame block:
//! 1. Two cascades of two allpass biquads (image tables at `0x82FCE2B0` / `0x82FCE2C4` → I and
//!    `0x82FCE2D8` / `0x82FCE2EC` → Q; fixed constants, never recomputed) form a Hilbert pair, run
//!    through the shared biquad kernel `sub_82B43AF8` ([`super::biquad::kernel`]).
//! 2. A phase φ (state `+260`) advances by Δ = f32(f32(shift / rate) · 2π) per sample; the lanes of
//!    each group of four are {φ, φ + Δ, φ + 2Δ, φ + 3Δ} and the group advances by 4Δ (`vaddfp`).
//! 3. out = I · cos φ − Q · sin φ, with the XNA-style polynomial `XMVectorSin` / `XMVectorCos`
//!    (`sub_824531C8` / `sub_82473930`: round(x / 2π) reduction, 11 odd / even Taylor terms).
//! 4. Block end: φ' = (φ + 256·Δ) − trunc((φ + 256·Δ) / 2π) · 2π.
//!
//! The shift (parameter 0, `+52`, range ±96000 Hz) applies from the next block; there is no
//! smoothing and no bypass: at 0 Hz the output is the I path, a phase-shifted (allpass) copy.
//! The class's optional 64-tap band-limiting FIR (`sub_82B42510` / `sub_82B41D58`, active when the
//! construction parameter is 1.0) is off in the board chain (`sub_824C8878` passes 0.0), so it is
//! not ported.
use super::biquad::{Coefficients, TWO_PI, kernel};

/// The four allpass sections {a1, a2, b0, b1, b2} (I = 0, 1; Q = 2, 3), image `0x82FCE2B0`.
pub const SECTIONS: [Coefficients; 4] = [
    allpass(0xBFF6_20C5, 0x3F6C_7469),
    allpass(0xBECD_B811, 0xBE5A_43B1),
    allpass(0xBFFB_DFCD, 0x3F77_C5D9),
    allpass(0xBFA1_A207, 0x3EB1_E001),
];

const fn allpass(a1: u32, a2: u32) -> Coefficients {
    let (a1, a2) = (f32::from_bits(a1), f32::from_bits(a2));
    Coefficients { a1, a2, b0: a2, b1: a1, b2: 1.0 }
}

/// 1/2π as the image holds it (`0x822F8904` for the wrap, lane 3 of `0x822F9850` for the trig).
pub const INV_TWO_PI: f32 = f32::from_bits(0x3E22_F983);

/// `XMVectorSin` coefficients for V³ … V²³ (`0x822F97C4` … `0x822F97EC`).
const SIN: [u32; 11] = [
    0xBE2A_AAAB, 0x3C08_8889, 0xB950_0D01, 0x3638_EF1D, 0xB2D7_322B, 0x2F30_9231, 0xAB57_3F9F, 0x274A_963C, 0xA317_A4DA, 0x1EB8_DC78,
    0x9A3B_0DA1,
];
/// `XMVectorCos` coefficients for V² … V²² (`0x822F97F4` … `0x822F981C`).
const COS: [u32; 11] = [
    0xBF00_0000, 0x3D2A_AAAB, 0xBAB6_0B61, 0x37D0_0D01, 0xB493_F27E, 0x310F_76C8, 0xAD49_CBA5, 0x2957_3F9F, 0xA534_13C3, 0x20F2_A15D,
    0x9C86_71CB,
];

fn c(table: &[u32; 11], k: usize) -> f32 {
    f32::from_bits(table[k])
}

/// x − 2π · round(x / 2π) (`vrfin` = nearest, ties to even; fused `vnmsubfp`).
fn reduce(x: f32) -> f32 {
    let n = (x * INV_TWO_PI).round_ties_even();
    (-TWO_PI).mul_add(n, x)
}

/// `XMVectorSin` per lane: powers by repeated ×V², terms accumulated with fused multiply-adds in
/// ascending order. Retail's VMX flushes denormals; here a power that underflows stays denormal,
/// which can only matter for |V| < 1e-30.
pub fn sin(x: f32) -> f32 {
    let v = reduce(x);
    let v2 = v * v;
    let mut p = v2 * v;
    let mut r = c(&SIN, 0).mul_add(p, v);
    for k in 1..11 {
        p *= v2;
        r = c(&SIN, k).mul_add(p, r);
    }
    r
}

/// `XMVectorCos` per lane. The powers come in retail's pairing (V⁴ = V²·V², V⁶ = V⁴·V², V⁸ = V⁴·V⁴,
/// V¹⁰ = V⁶·V⁴, V¹² = V⁶·V⁶, V¹⁴ = V⁸·V⁶, V¹⁶ = V⁸·V⁸, V¹⁸ = V¹⁰·V⁸, V²⁰ = V¹⁰·V¹⁰, V²² = V¹²·V¹⁰);
/// the terms are summed from 1 upwards with fused multiply-adds.
pub fn cos(x: f32) -> f32 {
    let v = reduce(x);
    let v2 = v * v;
    let v4 = v2 * v2;
    let v6 = v4 * v2;
    let v8 = v4 * v4;
    let v10 = v6 * v4;
    let v12 = v6 * v6;
    let v14 = v8 * v6;
    let v16 = v8 * v8;
    let v18 = v10 * v8;
    let v20 = v10 * v10;
    let v22 = v12 * v10;
    let powers = [v2, v4, v6, v8, v10, v12, v14, v16, v18, v20, v22];
    let mut r = 1.0f32;
    for (k, p) in powers.into_iter().enumerate() {
        r = c(&COS, k).mul_add(p, r);
    }
    r
}

#[derive(Clone, Debug)]
pub struct FrequencyShift {
    /// Parameter 0: the shift in Hz (positive = up).
    pub shift_hz: f32,
    /// Oscillator phase (`+260`, radians, kept in (−2π, 2π) by the block-end wrap).
    phase: f32,
    /// Allpass histories {x1, x2, y1, y2} (`+56`, `+72`, `+88`, `+104`).
    history: [[f32; 4]; 4],
}

impl Default for FrequencyShift {
    fn default() -> Self {
        Self { shift_hz: 0.0, phase: 0.0, history: [[0.0; 4]; 4] }
    }
}

impl FrequencyShift {
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Process one block in place (mono; `samples.len()` a multiple of 4, retail's is 256).
    pub fn process(&mut self, samples: &mut [f32], rate: f32) {
        let n = samples.len();
        let mut i = [0.0f32; crate::BLOCK];
        let mut q = [0.0f32; crate::BLOCK];
        let (i, q) = (&mut i[..n], &mut q[..n]);
        i.copy_from_slice(samples);
        q.copy_from_slice(samples);
        let [h0, h1, h2, h3] = &mut self.history;
        kernel(&SECTIONS[0], h0, i);
        kernel(&SECTIONS[1], h1, i);
        kernel(&SECTIONS[2], h2, q);
        kernel(&SECTIONS[3], h3, q);
        let delta = (self.shift_hz / rate) * TWO_PI;
        let phi = self.phase;
        let mut lanes = [phi, phi + delta, delta.mul_add(2.0, phi), delta.mul_add(3.0, phi)];
        let step = delta * 4.0;
        for (g, out) in samples.chunks_exact_mut(4).enumerate() {
            for (k, s) in out.iter_mut().enumerate() {
                let at = 4 * g + k;
                *s = i[at] * cos(lanes[k]) - q[at] * sin(lanes[k]);
            }
            for l in &mut lanes {
                *l += step;
            }
        }
        let end = delta.mul_add(n as f32, phi);
        let turns = ((end * INV_TWO_PI) as i32) as f32;
        self.phase = (-turns).mul_add(TWO_PI, end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48000.0;

    fn tone(f: f32, n: usize) -> Vec<f32> {
        (0..n).map(|k| ((std::f64::consts::TAU * f as f64 * k as f64 / RATE as f64).sin() * 0.5) as f32).collect()
    }

    /// Amplitude of the component at `f` (single-bin DFT, f64).
    fn amplitude(x: &[f32], f: f32) -> f64 {
        let w = std::f64::consts::TAU * f as f64 / RATE as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (k, &s) in x.iter().enumerate() {
            re += s as f64 * (w * k as f64).cos();
            im -= s as f64 * (w * k as f64).sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f64
    }

    fn shift(input: &[f32], hz: f32) -> Vec<f32> {
        let mut fss = FrequencyShift { shift_hz: hz, ..Default::default() };
        let mut x = input.to_vec();
        for block in x.chunks_mut(crate::BLOCK) {
            fss.process(block, RATE);
        }
        x
    }

    #[test]
    fn coefficients_are_the_image_allpasses() {
        assert_eq!(SECTIONS[0].a1, -1.922_875_0);
        assert_eq!(SECTIONS[0].a2, 0.923_651_3);
        assert_eq!(SECTIONS[1].a2, -0.213_148_85);
        assert_eq!(SECTIONS[3].a1, -1.262_757_2);
        for s in SECTIONS {
            assert_eq!((s.b0, s.b1, s.b2), (s.a2, s.a1, 1.0));
        }
    }

    #[test]
    fn polynomial_trig_tracks_libm() {
        let mut worst = (0.0f32, 0.0f32);
        for k in -20000..=20000 {
            let x = k as f32 * 0.000_7;
            let es = (sin(x) - (x as f64).sin() as f32).abs();
            let ec = (cos(x) - (x as f64).cos() as f32).abs();
            worst = (worst.0.max(es), worst.1.max(ec));
        }
        assert!(worst.0 < 1e-6 && worst.1 < 1e-6, "{worst:?}");
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(sin(0.0), 0.0);
    }

    #[test]
    fn each_path_is_an_allpass_and_the_pair_is_in_quadrature() {
        // Steady state of a tone through I and Q alone: unit magnitude; the phase difference is a
        // quarter turn across the band the bed uses.
        for f in [100.0f32, 300.0, 1000.0, 3000.0, 8000.0, 15000.0] {
            let x = tone(f, 48000);
            let mut i = x.clone();
            let mut q = x.clone();
            let mut h = [[0.0f32; 4]; 4];
            for (bi, bq) in i.chunks_mut(256).zip(q.chunks_mut(256)) {
                kernel(&SECTIONS[0], &mut h[0], bi);
                kernel(&SECTIONS[1], &mut h[1], bi);
                kernel(&SECTIONS[2], &mut h[2], bq);
                kernel(&SECTIONS[3], &mut h[3], bq);
            }
            let (ai, aq) = (amplitude(&i[24000..], f), amplitude(&q[24000..], f));
            assert!((ai - 0.5).abs() < 2e-3 && (aq - 0.5).abs() < 2e-3, "{f} Hz: |I| {ai}, |Q| {aq}");
            // Quadrature: I² + Q² is flat (a 90° pair) — its ripple measures the phase error.
            let env: Vec<f64> = i[24000..].iter().zip(&q[24000..]).map(|(&a, &b)| (a as f64).hypot(b as f64)).collect();
            let (lo, hi) = env.iter().fold((f64::MAX, 0.0f64), |(l, h), &e| (l.min(e), h.max(e)));
            let error_deg = ((hi - lo) / (hi + lo)).asin().to_degrees() * 2.0;
            // The image pair's design (f64 response of the sections): within 0.6° from 100 Hz to
            // 1 kHz, 6.1° at 8 kHz, 10° at 15 kHz; −47 / −74 / −25 dB image at 100 Hz / 1 kHz / 8 kHz.
            let allowed = if f <= 1000.0 { 1.0 } else if f <= 8000.0 { 7.0 } else { 12.0 };
            assert!(error_deg < allowed, "{f} Hz: quadrature error ≈ {error_deg:.2}°");
        }
    }

    #[test]
    fn a_tone_moves_by_the_shift() {
        let x = tone(1000.0, 48000);
        // Shifts on whole cycles of the 0.5 s analysis window (no DFT leakage between the bins).
        for hz in [150.0f32, -52.0, 38.0] {
            let y = shift(&x, hz);
            let wanted = amplitude(&y[24000..], 1000.0 + hz);
            let image = amplitude(&y[24000..], 1000.0 - hz);
            let left = amplitude(&y[24000..], 1000.0);
            assert!((wanted - 0.5).abs() < 5e-3, "{hz}: shifted tone {wanted}");
            // −60 dB: the pair's −74 dB image at 1 kHz plus the oscillator's f32 phase noise.
            assert!(image < 0.5e-3 && left < 0.5e-3, "{hz}: image {image}, carrier {left}");
        }
    }

    #[test]
    fn zero_hz_is_the_allpass_not_a_bypass() {
        let x = tone(440.0, 4096);
        let y = shift(&x, 0.0);
        assert_ne!(x, y, "never a bypass");
        let mut i = x.clone();
        let mut h = [[0.0f32; 4]; 2];
        for b in i.chunks_mut(256) {
            kernel(&SECTIONS[0], &mut h[0], b);
            kernel(&SECTIONS[1], &mut h[1], b);
        }
        // cos 0 = 1, sin 0 = 0: exactly the I path.
        assert_eq!(y, i);
    }

    #[test]
    fn phase_wraps_per_block_and_stays_bounded() {
        let mut fss = FrequencyShift { shift_hz: 1234.5, ..Default::default() };
        let mut x = [0.0f32; crate::BLOCK];
        for _ in 0..10_000 {
            fss.process(&mut x, RATE);
            assert!(fss.phase().abs() < TWO_PI);
        }
        let mut fss = FrequencyShift { shift_hz: -150.0, ..Default::default() };
        fss.process(&mut x, RATE);
        let d = (-150.0f32 / RATE) * TWO_PI;
        let end = d.mul_add(256.0, 0.0);
        assert_eq!(fss.phase(), end, "less than a turn: no wrap, sign kept");
    }
}
