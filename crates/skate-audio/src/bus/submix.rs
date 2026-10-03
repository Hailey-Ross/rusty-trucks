//! The FootStep SubMix (`sub_82494188`, spec `.claude/notes/aems-offboard-clothing-spec.md` §2.5):
//! one mono graph per foot sound slot of `SFXObj_OffBoard` (two feet × four slots), built once
//! for the owner's life (`sub_82494A68`):
//!
//! `Sub0 → HI20 → LI20 → PI20 → Sen0 (→ the env bus input) → Pn21 (1 → 6) → Sen0 (→ SFX Master)`.
//!
//! The slot's Splice voices play into Sub0 (their 6-channel final Send sums L, C, R, Ls, Rs into
//! the mono input, LFE dropped, as the Collision SubMix's). At each start of the slot's sound
//! `sub_82494550` posts HI20 = slot +4, LI20 = slot +8, PI20 centre / gain / Q = slot +12 / +16 /
//! +20, Sen0 #1 = the holder's env level (OffBoard level(8) / 32767 as of the last update) and Pn21
//! p0 = the holder's azimuth × 360/65536: the graph keeps those values (and its filter history,
//! send ramps and panner matrix) until the slot's next start.
//!
//! UNCERTAIN (spec §8): Sen0 #2's level is never posted (constructor default 1.0); Pn21 keeps its
//! class defaults except the azimuth (distance 1, on the speaker circle).
use super::env::Level;
use crate::dsp::biquad::{Iir2, Kind};
use crate::dsp::pan::{self, Pan2D};
use crate::dsp::peaking::PeakingIir2;
use crate::dsp::routes::to_six;
use crate::dsp::send::{Mode, Send};
use crate::{BLOCK, MIX_RATE};

/// The graphs of one owner: foot A slots 0..3, foot B slots 4..7.
pub const GRAPHS: usize = 8;

/// What `sub_82494550` posts at a slot's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubmixParams {
    /// HI20 / LI20 cutoffs (Hz).
    pub high_pass: f32,
    pub low_pass: f32,
    /// PI20 centre (Hz), linear gain and Q.
    pub peak_freq: f32,
    pub peak_gain: f32,
    pub peak_q: f32,
    /// Sen0 #1 (→ env bus) level.
    pub env: f32,
    /// Pn21 azimuth (degrees).
    pub azimuth: f32,
}

/// One built graph.
#[derive(Clone, Debug)]
pub struct FootSubmix {
    /// Sub0: the voices' Send adds into channel 1 (centre) of this 6-channel buffer (the mono
    /// route of [`super::Route::mono`]); the other channels stay 0.
    pub input: Box<[[f32; BLOCK]; 6]>,
    hpf: Iir2,
    lpf: Iir2,
    peak: PeakingIir2,
    env: Level,
    pan: Pan2D,
    send: Send,
}

impl Default for FootSubmix {
    fn default() -> Self {
        Self {
            input: Box::new([[0.0; BLOCK]; 6]),
            hpf: Iir2::new(Kind::HighPass),
            lpf: Iir2::new(Kind::LowPass),
            peak: PeakingIir2::default(),
            // Sen0 posts 0 at build; `sub_82494550` sets it before the first sound.
            env: Level::new(0.0),
            pan: Pan2D::new(1),
            send: Send::default(),
        }
    }
}

impl FootSubmix {
    fn set(&mut self, p: &SubmixParams) {
        self.hpf.cutoff = p.high_pass;
        self.lpf.cutoff = p.low_pass;
        self.peak.freq = p.peak_freq;
        self.peak.gain = p.peak_gain;
        self.peak.q = p.peak_q;
        self.env.target = p.env;
        self.pan.params[pan::ANGLE] = p.azimuth;
    }

    /// One block: filter the mono input, send it to the env bus, pan it into SFX Master; the
    /// input is cleared for the next block.
    fn render(&mut self, env_in: &mut [f32; BLOCK], master: &mut [[f32; BLOCK]; 6]) {
        let mut mono = self.input[1];
        self.input[1] = [0.0; BLOCK];
        {
            let mut planes: [&mut [f32]; 1] = [&mut mono[..]];
            self.hpf.process(&mut planes, MIX_RATE as f32);
            self.lpf.process(&mut planes, MIX_RATE as f32);
            self.peak.process(&mut planes, MIX_RATE as f32);
        }
        self.env.add(&mono, &mut env_in[..]);
        let mut six = [[0.0f32; BLOCK]; 6];
        self.pan.process(&[&mono[..]], &mut six);
        let outs: Vec<&[f32]> = six.iter().map(|c| &c[..]).collect();
        self.send.process(&outs, to_six(6), master, Mode::Normal);
    }
}

/// The FootStep SubMix graphs (built on first use, rendered every block from then on).
#[derive(Clone, Debug, Default)]
pub struct FootSubmixes {
    /// Off: hosts keep the old direct route (SFX Master with the owner env tap).
    pub enabled: bool,
    pub graphs: Vec<Option<FootSubmix>>,
}

impl FootSubmixes {
    /// `sub_82494A68` (build when missing) + `sub_82494550` (post the start's parameters).
    pub fn set(&mut self, graph: u8, p: &SubmixParams) {
        let i = usize::from(graph);
        if i >= GRAPHS {
            return;
        }
        if self.graphs.len() < GRAPHS {
            self.graphs.resize_with(GRAPHS, || None);
        }
        self.graphs[i].get_or_insert_with(FootSubmix::default).set(p);
    }

    /// The input a voice routed to `graph` adds into (None when the graph was never built).
    pub fn input(&mut self, graph: u8) -> Option<&mut [[f32; BLOCK]; 6]> {
        self.graphs.get_mut(usize::from(graph))?.as_mut().map(|g| &mut *g.input)
    }

    pub fn render(&mut self, env_in: &mut [f32; BLOCK], master: &mut [[f32; BLOCK]; 6]) {
        for g in self.graphs.iter_mut().flatten() {
            g.render(env_in, master);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f: f32, k0: usize) -> [f32; BLOCK] {
        std::array::from_fn(|k| (std::f32::consts::TAU * f * (k0 + k) as f32 / MIX_RATE as f32).sin() * 0.5)
    }

    const OPEN: SubmixParams =
        SubmixParams { high_pass: 0.0, low_pass: 96_000.0, peak_freq: 96_000.0, peak_gain: 1.0, peak_q: 3.0, env: 0.1, azimuth: 0.0 };

    #[test]
    fn an_open_graph_passes_the_mono_input_to_the_centre_and_taps_the_env_send() {
        let mut s = FootSubmixes::default();
        s.set(2, &OPEN);
        assert!(s.input(0).is_none(), "only built graphs take input");
        let x = tone(440.0, 0);
        s.input(2).unwrap()[1] = x;
        let (mut env, mut master) = ([0.0; BLOCK], [[0.0; BLOCK]; 6]);
        s.render(&mut env, &mut master);
        for k in 0..BLOCK {
            assert!((master[1][k] - x[k]).abs() < 1e-6, "centre carries the input at unity");
            assert!((env[k] - 0.1 * x[k]).abs() < 1e-7, "env send at the posted level");
        }
        assert!([0, 2, 3, 4, 5].iter().all(|&c| master[c].iter().all(|&v| v == 0.0)));
        // The input was consumed.
        let (mut env, mut master) = ([0.0; BLOCK], [[0.0; BLOCK]; 6]);
        s.render(&mut env, &mut master);
        assert!(master.iter().all(|c| c.iter().all(|&v| v == 0.0)));
    }

    #[test]
    fn the_eq_record_filters_and_the_azimuth_pans() {
        let mut s = FootSubmixes::default();
        // Footstep id 959's record: HPF 200, LPF 7000, PI20 5200 Hz × 0.1, Q 0.5.
        let p = SubmixParams { high_pass: 200.0, low_pass: 7000.0, peak_freq: 5200.0, peak_gain: 0.1, peak_q: 0.5, env: 0.0, azimuth: 90.0 };
        s.set(0, &p);
        let rms = |f: f32| {
            let mut s2 = s.clone();
            let mut acc = 0.0f32;
            for b in 0..40 {
                s2.input(0).unwrap()[1] = tone(f, b * BLOCK);
                let (mut env, mut master) = ([0.0; BLOCK], [[0.0; BLOCK]; 6]);
                s2.render(&mut env, &mut master);
                if b >= 20 {
                    acc += master.iter().flat_map(|c| c.iter()).map(|v| v * v).sum::<f32>();
                    assert!(env.iter().all(|&v| v == 0.0));
                    assert!(master[1].iter().all(|v| v.abs() < 1e-3), "panned right of centre");
                }
            }
            acc.sqrt()
        };
        let (low, mid, peak) = (rms(50.0), rms(1000.0), rms(5200.0));
        assert!(low < 0.2 * mid, "high-passed: {low} vs {mid}");
        assert!(peak < 0.3 * mid, "the PI20 cut at 5.2 kHz: {peak} vs {mid}");
    }
}
