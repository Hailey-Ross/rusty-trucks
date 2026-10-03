//! The game side of the native granular rolling bed (`skate_audio::grain`, spec
//! `.claude/notes/grain-player-spec.md`), on when the native runtime runs with a MixMap and the
//! install has the whole grain recordings and their vault tuning (`audio_export.grain_whole` /
//! `grain_tuning`). Otherwise the interim speed-band loop in `skate_events.rs` plays.
//!
//! Per frame, after the MixMap tick (`native::mixmap_frame`):
//! - **Surface routing** (§1.4, one sounding truck): the wheels' majority surface → grain member
//!   (`cues::grain_for`, the retail AudioSurfaceMap); a change stops both players and binds the new
//!   member (pulsing SkateBoard input 0); grinding (surface 14) stops them. In the air the last
//!   member keeps playing and the MixMap's no-contact duck silences it (what retail does with the
//!   wheel material in the air is UNCERTAIN, spec §2.10).
//! - **Records** (§2.2): position A from the member's Bézier of speed (push-scaled), B 0.1 behind;
//!   gains from SkateBoard level(1)/(2), pitch from pitch(3); the push speed-scale envelope, the
//!   brake slew, the turn intensity (`sub_824C8588`: |COM v|·0.24 capped × the turn input, slewed),
//!   the manual / trick latches ("special": A × special gain, no turn layer), the downhill level D
//!   (`sub_824CA738`, owner inputs 2/3) and wheel 0's seam-pattern gain envelope (`sub_824CA448`).
//!   Player B is the turning / downhill layer.
//! - **Soft wheels** (`sub_824C8370`): the soft member of surfaces 1..6 while the board's wheel
//!   hardness is below 0.5 (audio state `+684` = Motion+200 < 0.5); metal has only a hard member.
//! - **Chains** (§3.2, `skate_audio::grain::chain`): high-/low-pass from level(12)/(11), pan from
//!   raw(0), the FrequencyShiftSsb values (the +150 Hz special shift, the push shift envelope, the
//!   slope terms), the graph-1 level ramp and gain wobbles, the graph-3 send by speed, the env send
//!   level(13) and the FlangeSub send level(21)/(22) (carried, not rendered).
//! - **Rocket** (§2.8): `x_jet_rolling` above the owner's start speed (35 km/h), into the default bus.
//! - **Routing:** with the native rolling layers (`PlayerAudio::rolling_on`) the owner's two-truck
//!   surface routing (`player::rolling`, `sub_824C5CA8`) decides the binds and stops ([`GrainEvent`]s)
//!   and writes SkateBoard inputs 0 / 6; otherwise the bed routes one truck itself (as before).
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use skate_audio::eval::rng::Rng;
use skate_audio::grain::board::{self, BoardInputs, Latches, PushEnvelope, RocketTuning, SeamEnvelope, SurfaceTuning, TurnIntensity};
use skate_audio::grain::chain::{self, ChainFrame, ChainState, ChainTuning, PushShift};
use skate_audio::grain::{GrainFile, GrainSource};
use skate_audio::player::rolling::{self, GrainEvent};
use skate_audio::mixmap::{MixMap, keys};

use super::Library;

/// Every member `cues::grain_for` can name, plus the rocket layer.
const MEMBERS: &[&str] = &[
    "asphalt_rough_hard",
    "concrete_rough_hard",
    "concrete_smooth_hard",
    "wood_ramp_hard",
    "concrete_aggregate_hard",
    "metal_smooth_hard",
    "asphalt_smooth_hard",
];
/// Soft-wheel members (surfaces 1..6), used when the install has them.
const SOFT_MEMBERS: &[&str] = &[
    "asphalt_rough_soft",
    "concrete_rough_soft",
    "concrete_smooth_soft",
    "wood_ramp_soft",
    "concrete_aggregate_soft",
    "asphalt_smooth_soft",
];
const ROCKET: &str = "x_jet_rolling";
/// The brake slew step per 60 Hz frame (§2.7).
const BRAKE_STEP: f32 = 0.05;

/// The `default` collection's tuning key in [`Bed`]'s map (rolling surface 0 binds
/// asphalt_rough_hard with it, `rolling::member`).
const DEFAULT_TUNING: &str = "default";

/// A bound truck: the recording and the collection whose tuning it plays with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bound {
    stem: &'static str,
    tuning: &'static str,
}

pub(crate) struct Bed {
    tunings: HashMap<&'static str, SurfaceTuning>,
    rocket: Option<RocketTuning>,
    chain_tuning: ChainTuning,
    tuned: bool,
    sources: HashMap<String, Arc<GrainSource>>,
    failed: HashSet<String>,
    /// Per truck (the bed's own routing uses truck 0 only).
    bound: [Option<Bound>; 2],
    /// The routing's primary truck (`+1500`).
    primary: usize,
    rocket_on: bool,
    /// SkateBoard owner inputs for the next tick: 0 surface-change pulse, 4 push plant.
    pulse_surface: bool,
    pulse_push: bool,
    pushes_seen: Option<u32>,
    push_scale: PushEnvelope,
    /// The push frequency-shift envelope (`+1036`).
    shift: PushShift,
    brake: f32,
    /// Send, level ramp, wobbles and the gains each chain holds.
    chain: ChainState,
    turn: TurnIntensity,
    latches: Latches,
    seam: SeamEnvelope,
    /// Our own instance of the title generator for the seam draws (retail's is shared and
    /// unreproducible anyway).
    rng: Rng,
    /// Downhill level D (owner `+1508`), computed with the owner inputs.
    downhill: f32,
}

impl Bed {
    pub(crate) fn new(library: &Library) -> Option<Self> {
        let mut tunings = HashMap::new();
        for &name in MEMBERS {
            library.grain_whole(name)?;
            tunings.insert(name, library.grain_tuning(name)?);
        }
        for &name in SOFT_MEMBERS {
            if let (Some(_), Some(t)) = (library.grain_whole(name), library.grain_tuning(name)) {
                tunings.insert(name, t);
            }
        }
        if let Some(t) = library.grain_default_tuning() {
            tunings.insert(DEFAULT_TUNING, t);
        }
        let rocket = library.rocket_tuning().filter(|_| library.grain_whole(ROCKET).is_some());
        let chain_tuning = library.chain_tuning();
        let mut bed = Self {
            tunings,
            rocket,
            chain_tuning,
            tuned: false,
            sources: HashMap::new(),
            failed: HashSet::new(),
            bound: [None; 2],
            primary: 0,
            rocket_on: false,
            pulse_surface: false,
            pulse_push: false,
            pushes_seen: None,
            push_scale: PushEnvelope::default(),
            shift: PushShift::default(),
            brake: 0.0,
            chain: ChainState::default(),
            turn: TurnIntensity::default(),
            latches: Latches::default(),
            seam: SeamEnvelope::default(),
            rng: Rng::new(skate_audio::grain::GrainBed::SEED),
            downhill: 0.0,
        };
        // Decode every recording now: a grain's first trigger must sound like the later ones
        // (no decode on the game thread at first use).
        let names: Vec<&'static str> = bed.tunings.keys().copied().filter(|n| *n != DEFAULT_TUNING).chain(bed.rocket.map(|_| ROCKET)).collect();
        for name in names {
            bed.source(library, name);
        }
        Some(bed)
    }

    /// The member for a wheel surface tag: the soft member while the wheels are soft and the
    /// install has it (`sub_824C8370`).
    fn member(&self, tag: u32, soft: bool) -> &'static str {
        let hard = super::cues::grain_for(tag);
        if soft {
            if let Some(&name) = SOFT_MEMBERS.iter().find(|n| n.strip_suffix("_soft") == hard.strip_suffix("_hard")) {
                if self.tunings.contains_key(name) {
                    return name;
                }
            }
        }
        hard
    }

    /// The primary truck's binding (or the other's when only that one runs).
    fn primary_bound(&self) -> Option<Bound> {
        self.bound[self.primary].or(self.bound[1 - self.primary])
    }

    /// The push speed-scale envelope while it runs (`player::rolling` scales its patch speeds).
    pub(crate) fn push_scale(&self) -> Option<f32> {
        self.push_scale.value()
    }

    /// The board owner's MixMap inputs (written before the tick): 0 = 32767 for the frame after a
    /// surface change, 4 = 32767 on a push plant, 6 = 32767 on metal, 2 / 3 = the downhill /
    /// uphill levels (`sub_824CA738`: the primary truck on a grain surface, slope `+712` over the
    /// member's divisors). Heading rate (5) and skid (1) reach no MixMap output and are not written.
    /// `own_routing`: the bed routes itself and writes 0 / 6 (else `player::rolling` does).
    pub(crate) fn write_inputs(&mut self, m: &mut MixMap, s: &skate_audio::player::AudioState, own_routing: bool) {
        let owner = keys::skateboard(0);
        let pulse = std::mem::take(&mut self.pulse_surface);
        if own_routing {
            m.set_input(owner, 0, if pulse { 32767 } else { 0 });
            m.set_input(owner, 6, if self.bound[0].is_some_and(|b| b.stem == "metal_smooth_hard") { 32767 } else { 0 });
        }
        m.set_input(owner, 4, if std::mem::take(&mut self.pulse_push) { 32767 } else { 0 });
        let tuning = self.primary_bound().and_then(|b| self.tunings.get(b.tuning));
        let (down, up) = match tuning {
            Some(t) => board::slope_levels(s.slope, true, t.slope_divisors.0, t.slope_divisors.1),
            None => (0.0, 0.0),
        };
        self.downhill = down;
        let word = |x: f32| ((x * 32767.0) as i32).clamp(0, 32767);
        m.set_input(owner, 2, word(down));
        m.set_input(owner, 3, word(up));
    }

    /// The decoded recording of a member (all are decoded in [`Bed::new`]).
    fn source(&mut self, library: &Library, name: &str) -> Option<Arc<GrainSource>> {
        if let Some(s) = self.sources.get(name) {
            return Some(s.clone());
        }
        if self.failed.contains(name) {
            return None;
        }
        let loaded = (|| -> Result<Arc<GrainSource>, String> {
            let (wav, raw) = library.grain_whole(name).ok_or("not in the install")?;
            let header = GrainFile::parse(&library.read(raw).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let pcm = super::library::wav_pcm(&library.read(wav).map_err(|e| e.to_string())?).ok_or("not a PCM16 WAV")?;
            if pcm.rate != header.stream.rate {
                return Err(format!("WAV rate {} ≠ stream rate {}", pcm.rate, header.stream.rate));
            }
            Ok(Arc::new(GrainSource { name: name.to_owned(), duration: header.duration, pcm: Arc::new(pcm) }))
        })();
        match loaded {
            Ok(s) => {
                self.sources.insert(name.to_owned(), s.clone());
                Some(s)
            }
            Err(e) => {
                warn!("Game audio: grain {name}: {e}");
                self.failed.insert(name.to_owned());
                None
            }
        }
    }
}

pub(super) fn update(
    native: Option<ResMut<super::native::Native>>,
    library: Option<Res<Library>>,
    cues: Res<super::skate_events::Cues>,
    time: Res<Time<Real>>,
) {
    let _timing = super::timing::scope(&super::timing::GRAIN_BED);
    let (Some(mut native), Some(library)) = (native, library) else { return };
    let native = &mut *native;
    // The seam wobbles live with the player tuning.
    let seams = skate_audio::player::tuning::PlayerTuning {
        seam_wobbles: native.player.as_ref().map(|p| p.tuning.seam_wobbles.clone()).unwrap_or_default(),
        ..Default::default()
    };
    // The owner's routing (native rolling layers): its binds / stops since the last frame.
    let routed = native.player.as_mut().filter(|p| p.components && p.rolling_on).map(|p| (std::mem::take(&mut p.routed.grains), p.routed.primary));
    let (Some(bed), Some(m)) = (&mut native.bed, &native.mixmap) else { return };
    step(bed, &library, m, &native.shared, &cues.riding, time.delta_secs(), &seams, routed);
}

/// One game frame of the bed (after the MixMap tick): [`update`]'s body, callable headless.
/// `routed`: the owner routing's grain events since the last frame and its primary truck (native
/// rolling layers), or `None` for the bed's own one-truck routing.
#[allow(clippy::too_many_arguments)]
pub(super) fn step(
    bed: &mut Bed,
    library: &Library,
    m: &MixMap,
    shared: &std::sync::Mutex<skate_audio::runtime::Runtime>,
    r: &super::skate_events::Riding,
    dt: f32,
    seams: &skate_audio::player::tuning::PlayerTuning,
    routed: Option<(Vec<GrainEvent>, usize)>,
) {
    let r = *r;
    let s = r.audio;
    let frames = (dt * 60.0).clamp(0.0, 6.0);
    let speed = r.speed.abs();

    // Surface routing: the wanted binding per truck (None = stopped).
    let mut wanted = bed.bound;
    match &routed {
        Some((events, primary)) => {
            bed.primary = *primary;
            for e in events {
                match *e {
                    GrainEvent::Bind { truck, surface, soft } => {
                        let m = rolling::member(surface, soft);
                        let tuning = if m.default_tuning { DEFAULT_TUNING } else { m.stem };
                        // Soft members the install lacks play their hard member (as before).
                        let stem = if bed.tunings.contains_key(m.stem) { m.stem } else { rolling::member(surface, false).stem };
                        let tuning = if bed.tunings.contains_key(tuning) { tuning } else { stem };
                        wanted[truck] = Some(Bound { stem, tuning });
                    }
                    GrainEvent::Stop { truck } => wanted[truck] = None,
                }
            }
        }
        None => {
            bed.primary = 0;
            wanted[0] = if r.grinding {
                None
            } else if r.wheels == 0 {
                bed.bound[0]
            } else {
                let stem = bed.member(r.surface, s.soft_wheels);
                Some(Bound { stem, tuning: stem })
            };
        }
    }
    let primary = wanted[bed.primary].or(wanted[1 - bed.primary]);
    let tuning = primary.and_then(|b| bed.tunings.get(b.tuning)).cloned();

    // Modulators.
    if bed.pushes_seen.is_some_and(|seen| seen != r.pushes) {
        bed.pulse_push = true;
        if let Some(t) = &tuning {
            let (scale, shift) = board::push_peaks(&t.push, speed);
            bed.push_scale.trigger(scale, 1.0, t.push.scale_ms);
            bed.shift.trigger(shift, t.push.shift_ms);
        }
    }
    bed.pushes_seen = Some(r.pushes);
    bed.push_scale.advance(dt);
    bed.shift.advance(dt);
    bed.brake = board::slew(bed.brake, if r.braking { 1.0 } else { 0.0 }, BRAKE_STEP * frames);
    let special = bed.latches.update(s.balance, s.wheel_count, s.hippy_jump, s.feet_in_deck_box);
    let turn = tuning.as_ref().map_or(0.0, |t| bed.turn.step(t, s.com_speed(), s.turn, special, frames));
    let rng = &mut bed.rng;
    bed.seam.update(s.seam_pattern[0], dt, |p| seams.seam_wobble(p), || rng.draw());
    // The chain modulators share the title generator, after the seam draws (retail's frame order).
    let rng = &mut bed.rng;
    bed.chain.frame(&bed.chain_tuning, speed, dt, [wanted[0].is_some(), wanted[1].is_some()], || rng.draw());

    let owner = keys::skateboard(0);
    let inputs = BoardInputs {
        speed,
        speed_scale: bed.push_scale.value(),
        level_a: m.level(owner, 1),
        level_b: m.level(owner, 2),
        pitch: m.pitch_4096(owner, 3),
        turn,
        brake: bed.brake,
        special,
        downhill: bed.downhill,
        seam: bed.seam.value(),
    };
    let truck_tuning = |b: Option<Bound>| b.and_then(|b| bed.tunings.get(b.tuning)).cloned();
    let tunings = [truck_tuning(wanted[0]), truck_tuning(wanted[1])];
    let records = tunings.clone().map(|t| t.map(|t| board::records(&t, &inputs)));
    let (push_shift, downhill) = (bed.shift.value(), bed.downhill);
    let frame = |t: &SurfaceTuning| ChainFrame {
        highpass_hz: m.filter_hz(owner, 12) as f32,
        lowpass_hz: m.filter_hz(owner, 11) as f32,
        pan_degrees: m.raw(owner, 0) as f32 * chain::DEGREES_PER_RAW,
        env_send: m.level(owner, 13) as f32 * chain::PER_LEVEL,
        flange_send: [m.level(owner, 21) as f32 * chain::PER_LEVEL, m.level(owner, 22) as f32 * chain::PER_LEVEL],
        fss_hz: chain::fss_shifts(t, tuning.as_ref().unwrap_or(t), special, push_shift, downhill),
    };
    let sos = keys::sense_of_speed(0);
    let rocket_record = bed.rocket.map(|t| board::rocket_record(&t, speed, m.level(sos, 5), m.pitch_4096(sos, 3)));
    let sources: [Option<Arc<GrainSource>>; 2] = std::array::from_fn(|t| {
        if wanted[t] != bed.bound[t] { wanted[t].and_then(|b| bed.source(library, b.stem)) } else { None }
    });
    let rocket_source = match (bed.rocket, bed.rocket_on) {
        (Some(t), false) if speed * 3.6 > t.start_kmh => bed.source(library, ROCKET),
        _ => None,
    };

    let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) else { return };
    let grains = &mut runtime.grains;
    if !bed.tuned {
        grains.chain_tuning = bed.chain_tuning;
        bed.tuned = true;
    }
    for t in 0..2 {
        if wanted[t] != bed.bound[t] {
            grains.stop_truck(t);
            if routed.is_none() {
                bed.pulse_surface = bed.bound[t].is_some();
            }
            bed.bound[t] = None;
            if let (Some(b), Some(source), Some(tu), Some(rec)) = (wanted[t], sources[t].clone(), &tunings[t], records[t]) {
                info!("AUDIO_EVENT grain bind truck {t} {} ({} tuning, surface tag {})", b.stem, b.tuning, r.surface);
                bed.chain.rebuilt(t);
                grains.bind_truck(t, source, tu.params, rec);
                bed.bound[t] = Some(b);
            }
        } else if let Some(rec) = records[t] {
            grains.set_records(t, rec);
        }
        if let Some(tu) = &tunings[t] {
            grains.set_chains(t, bed.chain.values(t, &frame(tu)));
        }
    }
    if let (Some(t), Some(rec)) = (bed.rocket, rocket_record) {
        if bed.rocket_on && speed * 3.6 <= t.start_kmh {
            grains.stop_rocket();
            bed.rocket_on = false;
        } else if let Some(source) = rocket_source {
            grains.start_rocket(source, t.params, rec);
            bed.rocket_on = true;
        } else if bed.rocket_on {
            grains.rocket.record = rec;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cues::grain_for` against the install's own `Sk8::AudioSurfaceMap` (when present): tag →
    /// material (tag − 1; 0 = none → 143 → surface 3) → rolling surface → grain member, for every
    /// surface that plays a grain (1–6, 9). And every member has its tuning and recording.
    #[test]
    fn grain_for_matches_the_vault_surface_map_and_the_bed_loads() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else {
            eprintln!("skipped: no audio install under {}", root.display());
            return;
        };
        let map = library.surface_map();
        if map.is_empty() {
            eprintln!("skipped: the install has no grain tuning (stage_grain_mixmap.py)");
            return;
        }
        assert_eq!(map.len(), 95);
        for tag in 0..128u32 {
            let material = if tag == 0 { 143 } else { tag - 1 };
            let surface = if material == 143 { 3 } else { map[material.min(94) as usize] };
            let want = match surface {
                1 => "asphalt_rough_hard",
                2 => "concrete_rough_hard",
                3 => "asphalt_smooth_hard",
                4 => "concrete_smooth_hard",
                5 => "wood_ramp_hard",
                6 => "concrete_aggregate_hard",
                9 => "metal_smooth_hard",
                _ => continue, // Class_rolling surfaces and surface 0: stand-ins
            };
            assert_eq!(super::super::cues::grain_for(tag), want, "tag {tag} (surface {surface})");
        }
        let bed = Bed::new(&library).expect("every member has its recording and tuning");
        assert_eq!(bed.tunings.len(), MEMBERS.len() + SOFT_MEMBERS.len() + 1, "hard and soft members and the `default` collection");
        assert!(bed.rocket.is_some_and(|r| r.start_kmh == 35.0 && r.gain_word == 22000));
        assert_eq!(bed.tunings["concrete_rough_hard"].max_kmh, 60.0);
        // Soft wheels pick the soft member where one exists; metal has only the hard one.
        assert_eq!(bed.member(4, true), "concrete_rough_soft");
        assert_eq!(bed.member(4, false), "concrete_rough_hard");
        assert_eq!(bed.member(9, true), "metal_smooth_hard");
    }
}
