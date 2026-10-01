//! Skateboarding sounds. `observe` runs after every physics tick and turns
//! state changes into cues (some flags, like the ollie launch, only last one
//! tick); `play` turns them into voices each frame and keeps the continuous
//! loops (rolling, grind, powerslide, foot drag, wheel spin) following the board.
//! Every one-shot cue is logged as `AUDIO_CUE` so play sessions can be checked.
use super::{Library, Play, Voices, cues, library::Clip, voices::VoiceId};
use crate::physics::{GamePhysics, SkaterRuntime};
use bevy::prelude::*;
use skate_core::{
    physics::{board::BodyId, filtered_state::FilteredCategory},
    player::state::PhysicalStateId,
};

/// Board ground speed (m/s) mapped to the fastest rolling band.
const FULL_ROLL_SPEED: f32 = 10.0;
/// Landing impact speed (m/s) for full landing level.
const FULL_IMPACT: f32 = 12.0;
/// Horizontal speed (m/s) above which on-foot steps use the running set.
const RUN_SPEED: f32 = 3.0;
/// Downward speed (m/s) a body needs for a water-entry splash, and the
/// shortest time between two splashes (floating bodies touch water every tick).
const SPLASH_SPEED: f32 = 1.0;
const SPLASH_COOLDOWN: f32 = 1.5;
/// Rolling: speed smoothing time constant, shortest time on one band, and how
/// long a new surface must stay under the wheels before the sound changes.
const ROLL_SPEED_SMOOTHING: f32 = 0.25;
const BAND_HOLD: f32 = 1.0;
const SURFACE_HOLD: f32 = 0.25;
/// Seconds off the ground after which the rolling loop stops (restarting
/// cleanly on the surface it lands on).
const ROLL_OFF_STOP: f32 = 0.15;
/// Foot drag: the board must move this fast (m/s) to make a sound. The
/// foot-down brake event repeats every tick while the foot brakes (then
/// foot-up repeats while it lifts), so braking ends this long (s) after the
/// last foot-down.
const DRAG_MIN_SPEED: f32 = 0.5;
const BRAKE_TIMEOUT: f32 = 0.1;

#[derive(Clone, Copy, Debug)]
pub(super) enum Event {
    Pop(Vec3),
    Flip(Vec3),
    Land { at: Vec3, impact: f32 },
    /// Wheels touch down after stepping onto the board (an Air phase that began on foot).
    BoardDown { at: Vec3, impact: f32 },
    Bail { at: Vec3, speed: f32 },
    Push(Vec3),
    /// `level` 0..1 from the animation's AudibleFootStepStrength (walk ~0.6, run 1.0).
    Step { at: Vec3, run: bool, level: f32 },
    Splash { at: Vec3, speed: f32 },
}

impl Event {
    fn name(&self) -> &'static str {
        match self {
            Event::Pop(_) => "pop",
            Event::Flip(_) => "flip",
            Event::Land { .. } => "land",
            Event::BoardDown { .. } => "board_down",
            Event::Bail { .. } => "bail",
            Event::Push(_) => "push",
            Event::Step { run: false, .. } => "step",
            Event::Step { run: true, .. } => "run_step",
            Event::Splash { .. } => "splash",
        }
    }
}

/// Continuous state sampled at the last physics tick.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Riding {
    pub board: Vec3,
    pub speed: f32,
    pub rolling: bool,
    pub surface: u32,
    pub airborne: bool,
    pub grinding: bool,
    /// Audio surface of the grind (`grinds.audio_surface_216`).
    pub grind_surface: u32,
    pub sliding: bool,
    pub braking: bool,
}

#[derive(Resource, Default)]
pub(super) struct Cues {
    pub events: Vec<Event>,
    pub riding: Riding,
}

#[derive(Default)]
pub(super) struct Seen {
    started: bool,
    launched: bool,
    filtered: u32,
    state: u32,
    trick_seq: u32,
    in_water: bool,
    splash_cooldown: f32,
    push: bool,
    feet: [FootStrike; 2],
    /// The current Air phase began on foot (stepping onto the board).
    air_from_foot: bool,
    /// Seconds since the skater was last on foot (BipedGround/BipedAir).
    since_foot: f32,
    /// Foot braking, time since the last foot-down event, and the last event (+1/-1/0).
    braking: bool,
    brake_time: f32,
    brake_event: i32,
    on_rail: bool,
    /// Seconds since the last splash (a wipeout right after one is the water entry).
    since_splash: f32,
    /// Last trick announced in the current air phase.
    air_trick: Option<String>,
    trace: Option<bool>,
}

/// Foot-strike detector on one foot's height above the ground under it. The
/// resting height (ankle above sole) is learned: it follows the lowest height
/// and drifts back up slowly. A strike is the foot coming back down to rest
/// after having lifted clearly above it.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FootStrike {
    rest: Option<f32>,
    lifted: bool,
}
impl FootStrike {
    const LIFT: f32 = 0.05;
    const STRIKE: f32 = 0.02;
    const DRIFT: f32 = 2.0;

    /// Feed this tick's height (None: no ground found); true on a strike.
    pub(super) fn update(&mut self, height: Option<f32>, dt: f32) -> bool {
        let Some(height) = height.filter(|h| h.is_finite()) else {
            self.lifted = false;
            return false;
        };
        let rest = match self.rest {
            Some(rest) if height >= rest => rest + (height - rest) * (1.0 - (-dt / Self::DRIFT).exp()),
            _ => height,
        };
        self.rest = Some(rest);
        if height > rest + Self::LIFT {
            self.lifted = true;
        } else if self.lifted && height < rest + Self::STRIKE {
            self.lifted = false;
            return true;
        }
        false
    }
}

fn vec(v: skate_core::math::Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn grinding(state: u32) -> bool {
    (PhysicalStateId::GrindBoardslide as u32..=PhysicalStateId::GrindDarkslide as u32).contains(&state)
}

/// Majority audio surface under the wheels that touch the ground.
fn wheel_surface(physics: &GamePhysics) -> (bool, u32) {
    let ground = &physics.riding.ground;
    let lines = &physics.riding.wheel_lines;
    let touching: Vec<u32> = (0..4).filter(|&i| ground.parts[i].in_contact).map(|i| lines.audio_surfaces[i]).collect();
    let surface = touching.iter().copied().max_by_key(|s| touching.iter().filter(|t| *t == s).count());
    (!touching.is_empty(), surface.unwrap_or(0))
}

pub(super) fn observe(
    physics: Res<GamePhysics>,
    mut skater: ResMut<SkaterRuntime>,
    mut cues: ResMut<Cues>,
    mut seen: Local<Seen>,
    time: Res<Time>,
) {
    // Animation events latched during the tick (physics never reads these).
    let audio = std::mem::take(&mut skater.animation_input.audio);
    let p = &skater.player_input.physical;
    let filtered = p.filtered_state_0;
    let state = skater.player_state.current() as u32;
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates;
    let root = skater.skeleton.bodies()[0].rates;
    let (board, body) = (vec(deck.position), vec(root.position));
    let launched = skater.ground_animation.launched;
    let trick_seq = skater.scoring.trick_seq();
    let in_water = skater.collision_feedback.flags.material_12;
    let air = FilteredCategory::Air as u32;
    let ground = FilteredCategory::Ground as u32;
    let wipeout = PhysicalStateId::WipeoutGround as u32;
    let mut cooldown = (seen.splash_cooldown - time.delta_secs()).max(0.0);
    let mut since_splash = seen.since_splash + time.delta_secs();
    // The physical state leaves BipedGround a few ticks before the filtered
    // category reaches Air, so "began on foot" means on foot very recently.
    let since_foot = if matches!(state, 500 | 501) { 0.0 } else { seen.since_foot + time.delta_secs() };
    let air_from_foot = if filtered == air && seen.filtered != air { seen.since_foot < 0.5 } else { seen.air_from_foot };
    let trace = *seen.trace.get_or_insert_with(|| std::env::var_os("SKATE_AUDIO_TRACE").is_some_and(|v| v != "0"));
    if trace && (audio.footstep > 0.0 || audio.push || audio.brake_down || audio.brake_up) {
        info!("AUDIO_TRACE footstep={:.3} push={} brake_down={} brake_up={} state={state}",
            audio.footstep, audio.push, audio.brake_down, audio.brake_up);
    }
    // Foot braking lasts from the foot-down brake event to the foot-up one,
    // only while riding on the ground (a safety timeout covers a missed foot-up).
    let speed = physics.riding.motion.ground_speed.abs();
    // Logged on change only (the events repeat every tick).
    let brake_event = if audio.brake_down { 1 } else if audio.brake_up { -1 } else { 0 };
    if brake_event != 0 && brake_event != seen.brake_event {
        info!("AUDIO_EVENT brake {} speed={speed:.2} state={state}", if brake_event > 0 { "down" } else { "up" });
    }
    if audio.push && !seen.push {
        info!("AUDIO_EVENT push speed={speed:.2} state={state}");
    }
    let mut brake_time = if audio.brake_down { 0.0 } else { seen.brake_time + time.delta_secs() };
    let mut braking = (seen.braking || audio.brake_down) && !audio.brake_up;
    if !matches!(state, 100 | 101) || filtered != ground || brake_time > BRAKE_TIMEOUT {
        braking = false;
        brake_time = BRAKE_TIMEOUT;
    }
    if seen.started {
        let events = &mut cues.events;
        if launched && !seen.launched {
            events.push(Event::Pop(board));
        }
        if seen.filtered == air && filtered == ground {
            let impact = physics.riding.ground.maximum_closing_speed;
            events.push(if air_from_foot {
                Event::BoardDown { at: board, impact }
            } else {
                Event::Land { at: board, impact }
            });
        }
        // The scoring re-announces a held grab (trick_seq keeps rising), so the
        // whoosh plays only for a different trick than the last in this air.
        if trick_seq != seen.trick_seq && filtered == air {
            let name = skater.scoring.trick_name();
            if seen.air_trick.as_deref() != Some(name) {
                events.push(Event::Flip(board));
                seen.air_trick = Some(name.to_owned());
            }
        }
        // A water entry is also a wipeout; the splash covers it (no body slide).
        let watery = in_water || seen.since_splash < 0.5;
        // Wading in and falling (feet already wet, slow drop) never gives a
        // fresh fast contact below, so a wipeout in water splashes by itself.
        if state == wipeout && seen.state != wipeout && in_water && seen.since_splash > 1.0 {
            events.push(Event::Splash { at: body, speed: vec(root.linear_velocity).length().max(2.0) });
            since_splash = 0.0;
            cooldown = SPLASH_COOLDOWN;
        }
        if state == wipeout && seen.state != wipeout && !watery {
            events.push(Event::Bail { at: body, speed: vec(root.linear_velocity).length() });
        }
        if audio.push && !seen.push {
            events.push(Event::Push(board));
        }
        // AudibleFootStepStrength is a held loudness level (2.5-4), not a
        // step event; steps come from each foot coming down to the ground.
        if matches!(state, 500 | 501) {
            let horizontal = Vec2::new(root.linear_velocity.x, root.linear_velocity.z).length();
            let clearance = crate::physics::foot_clearance(&skater);
            if trace {
                let h = clearance.map(|c| c.map_or(f32::NAN, |(height, _)| height));
                info!("AUDIO_TRACE feet left={:.3} right={:.3} speed={horizontal:.2}", h[0], h[1]);
            }
            for (foot, clearance) in seen.feet.iter_mut().zip(clearance) {
                if foot.update(clearance.map(|(height, _)| height), time.delta_secs()) && horizontal > 0.3 {
                    let level = if audio.footstep > 0.0 { (audio.footstep / 4.0).clamp(0.3, 1.0) } else { 0.7 };
                    events.push(Event::Step { at: body, run: horizontal > RUN_SPEED, level });
                }
            }
        } else {
            seen.feet = Default::default();
        }
        if in_water && !seen.in_water && -root.linear_velocity.y > SPLASH_SPEED && cooldown == 0.0 {
            events.push(Event::Splash { at: body, speed: -root.linear_velocity.y });
            cooldown = SPLASH_COOLDOWN;
            since_splash = 0.0;
        }
    }
    // Grind: the selector's grinding flag can lead the named grind state, and
    // `leaving` marks coming off before the state changes.
    let grinds = &skater.player_input.physical.grinds;
    let on_rail = (grinds.grinding_316 != 0 || grinding(state)) && grinds.leaving_317 == 0;
    if on_rail != seen.on_rail {
        info!("AUDIO_EVENT grind {} surface={} ledge={} flag={} leaving={} state={state}", if on_rail { "start" } else { "stop" },
            grinds.audio_surface_216, grinds.is_ledge_320, grinds.grinding_316, grinds.leaving_317);
    }
    let (rolling, surface) = wheel_surface(&physics);
    cues.riding = Riding {
        board,
        speed: physics.riding.motion.ground_speed,
        rolling,
        surface,
        airborne: filtered == air,
        grinding: on_rail,
        grind_surface: grinds.audio_surface_216,
        sliding: state == PhysicalStateId::SlideGround as u32,
        braking: braking && speed > DRAG_MIN_SPEED,
    };
    *seen = Seen {
        started: true, launched, filtered, state, trick_seq, in_water, splash_cooldown: cooldown,
        push: audio.push, feet: seen.feet, air_from_foot, since_foot, braking, brake_time, brake_event, on_rail, since_splash, trace: Some(trace),
        air_trick: if filtered == air { seen.air_trick.take() } else { None },
    };
}

/// A looping voice and the clip it plays.
#[derive(Default)]
struct Loop {
    voice: Option<(VoiceId, Clip)>,
}
impl Loop {
    fn stop(&mut self, voices: &mut Voices, fade: f32) {
        if let Some((id, _)) = self.voice.take() {
            voices.stop(id, fade);
        }
    }
    /// Forget a voice that ended on its own (or was refused).
    fn prune(&mut self, voices: &Voices) {
        if self.voice.as_ref().is_some_and(|(id, _)| !voices.playing(*id)) {
            self.voice = None;
        }
    }
}

#[derive(Default)]
pub(super) struct Loops {
    roll: Loop,
    /// Grain and band the rolling loop plays, and how long it has played.
    roll_band: Option<(&'static str, usize)>,
    roll_held: f32,
    /// Board speed low-passed for band choice (landings and pushes jump it).
    roll_speed: f32,
    /// Surface grain under the wheels, switched to only once stable (seams,
    /// curbs and tile edges flicker between surfaces for a few ticks).
    roll_grain: Option<&'static str>,
    roll_candidate: Option<(&'static str, f32)>,
    /// Seconds the wheels have been off the ground (or the board stopped).
    roll_off: f32,
    grind: Loop,
    /// Countdown to the next shuffled powerslide / foot-drag piece.
    slide_next: f32,
    drag_next: f32,
    wheels: Loop,
    was_airborne: bool,
    rng: u32,
    /// Last member picked per (bank, record, layer), for sequential/shuffled layers.
    patch_order: std::collections::HashMap<(&'static str, usize, usize), usize>,
    /// Last start time per one-shot cue, for `cues::min_gap`.
    last: std::collections::HashMap<&'static str, f64>,
}

fn random(rng: &mut u32) -> f32 {
    if *rng == 0 {
        *rng = 0x2545_f491;
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    (*rng >> 8) as f32 / (1u32 << 24) as f32
}

/// (soft sample scale, landing-layer scale) for a board touch-down at `impact` m/s.
fn board_down_levels(impact: f32) -> (f32, f32) {
    let impact = if impact.is_finite() { impact.max(0.0) } else { 0.0 };
    (0.3 + 0.7 * (impact / 5.0).min(1.0), ((impact - 2.5) / 4.0).clamp(0.0, 1.0))
}

/// Speed band for `fraction` (0..1) with hysteresis around the current band.
fn band_for(fraction: f32, bands: usize, current: Option<usize>) -> usize {
    let scaled = fraction.clamp(0.0, 1.0) * (bands - 1) as f32;
    match current {
        // Switch only a full band away: on ramps speed changes constantly, and
        // every switch cross-fades two different recordings ("plays twice").
        Some(band) if (scaled - band as f32).abs() < 1.0 => band,
        _ => (scaled.round() as usize).min(bands - 1),
    }
}

struct Player<'a, 'w, 's> {
    commands: Commands<'w, 's>,
    library: &'a mut Library,
    voices: &'a mut Voices,
    assets: &'a mut Assets<AudioSource>,
    now: f64,
}
impl Player<'_, '_, '_> {
    fn pick(&mut self, cue: &cues::Cue, rng: &mut u32) -> Option<(usize, Clip)> {
        let index = cue.samples[(random(rng) * cue.samples.len() as f32) as usize % cue.samples.len()];
        self.library.sample(self.assets, cue.bank, index).map(|clip| (index, clip))
    }
    /// One-shot with a small random pitch spread (+-4%).
    fn once(&mut self, name: &str, cue: &cues::Cue, scale: f32, at: Vec3, rng: &mut u32) {
        self.once_pitched(name, cue, scale, 1.0, at, rng);
    }
    /// Play retail patch `id` of `bank` the way its data describes: an id past
    /// the records is a container that picks one record; each of the record's
    /// groups is a layer, one member each, chosen by the group mode (0 random,
    /// 1 in sequence, 2 shuffled without an immediate repeat), played if its
    /// probability allows, at its gain +- its random range and a pitch between
    /// 1/s and s for a pitch spread s > 1. (Semantics as documented in
    /// upstream PRs #1/#4; implemented here independently.)
    fn play_record(&mut self, name: &str, record: &cues::Record, scale: f32, at: Vec3, rng: &mut u32,
                   order: &mut std::collections::HashMap<(&'static str, usize, usize), usize>) {
        let Some(patches) = self.library.patches(record.bank) else { return };
        let records = patches.records.len();
        let id = if record.id < records {
            record.id
        } else {
            let Some(choices) = patches.containers.get(record.id - records).filter(|c| !c.is_empty()) else { return };
            choices[(random(rng) * choices.len() as f32) as usize % choices.len()]
        };
        let Some(groups) = patches.records.get(id) else { return };
        let mut layers = Vec::new();
        for (index, group) in groups.iter().enumerate() {
            let count = group.members.len();
            if count == 0 {
                continue;
            }
            let state = order.entry((record.bank, id, index)).or_insert(usize::MAX);
            let pick = match (count, group.mode) {
                (1, _) => 0,
                (_, 0) => (random(rng) * count as f32) as usize % count,
                (_, 2) => {
                    // Shuffle: never the same member twice in a row.
                    let offset = 1 + (random(rng) * (count - 1) as f32) as usize % (count - 1);
                    if *state < count { (*state + offset) % count } else { (random(rng) * count as f32) as usize % count }
                }
                _ => if *state < count { (*state + 1) % count } else { 0 },
            };
            *state = pick;
            let (sample, gain, gain_range, spread, probability) = group.members[pick];
            if random(rng) > probability {
                continue;
            }
            let gain = (gain + gain_range * (2.0 * random(rng) - 1.0)).max(0.0);
            let pitch = if spread > 1.0 && spread.is_finite() {
                let (low, high) = (1.0 / spread, spread);
                low + (high - low) * random(rng)
            } else {
                1.0
            };
            layers.push((sample, gain, pitch));
        }
        let mut played = Vec::new();
        for (sample, gain, pitch) in layers {
            let Some(clip) = self.library.sample(self.assets, record.bank, sample) else { continue };
            let volume = record.level * scale.clamp(0.0, 1.0) * gain;
            let mut play = Play::effect(volume, at);
            play.pitch = pitch;
            if self.voices.play(&mut self.commands, &clip, play, self.now).is_some() {
                played.push(sample);
            }
        }
        info!("AUDIO_CUE {name} {}:record{id} samples={played:?}", record.bank);
    }

    /// One-shot at `pitch` (playback speed), with the same small random spread.
    fn once_pitched(&mut self, name: &str, cue: &cues::Cue, scale: f32, pitch: f32, at: Vec3, rng: &mut u32) {
        let Some((index, clip)) = self.pick(cue, rng) else { return };
        let volume = cue.level * scale.clamp(0.0, 1.0);
        let mut play = Play::effect(volume, at);
        play.pitch = pitch * (0.96 + 0.08 * random(rng));
        let played = self.voices.play(&mut self.commands, &clip, play, self.now).is_some();
        if !name.is_empty() {
            info!("AUDIO_CUE {name} {}:{index} volume={volume:.2}{}", cue.bank, if played { "" } else { " (voice limit)" });
        }
    }
    /// While `active`, a random piece of `cue` every `interval` s, each fading in
    /// over `fade` so overlapping pieces blend into one continuous sound.
    #[allow(clippy::too_many_arguments)]
    fn shuffle(&mut self, name: &str, next: &mut f32, active: bool, dt: f32, cue: &cues::Cue, (interval, fade): (f32, f32), scale: f32, at: Vec3, rng: &mut u32) {
        if !active {
            *next = 0.0;
            return;
        }
        if *next == 0.0 {
            info!("AUDIO_LOOP start {name} ({})", cue.bank);
        }
        *next -= dt;
        if *next <= 0.0 {
            if let Some((_, clip)) = self.pick(cue, rng) {
                let mut play = Play::effect(cue.level * scale.clamp(0.0, 1.0), at);
                play.pitch = 0.96 + 0.08 * random(rng);
                play.fade_in = fade;
                self.voices.play(&mut self.commands, &clip, play, self.now);
            }
            *next = interval * (0.8 + 0.4 * random(rng));
        }
    }
    fn start_loop(&mut self, slot: &mut Loop, clip: Clip, volume: f32, pitch: f32, at: Vec3, fade: f32) {
        let mut play = Play::effect(volume, at);
        play.looping = true;
        play.pitch = pitch;
        play.fade_in = fade;
        if let Some(id) = self.voices.play(&mut self.commands, &clip, play, self.now) {
            info!("AUDIO_LOOP start {}", clip.key);
            slot.voice = Some((id, clip));
        }
    }
    /// Keep `slot` playing one of `cue`'s samples while `active`.
    fn hold(&mut self, slot: &mut Loop, active: bool, cue: &cues::Cue, volume: f32, at: Vec3, rng: &mut u32) {
        slot.prune(self.voices);
        if !active {
            slot.stop(self.voices, 0.06);
        } else if let Some((id, _)) = &slot.voice {
            self.voices.set(*id, volume, 1.0, Some(at));
        } else if let Some((_, clip)) = self.pick(cue, rng) {
            self.start_loop(slot, clip, volume, 1.0, at, 0.03);
        }
    }
}

pub(super) fn play(
    commands: Commands,
    mut cues: ResMut<Cues>,
    library: Option<ResMut<Library>>,
    mut voices: ResMut<Voices>,
    mut assets: ResMut<Assets<AudioSource>>,
    mut loops: Local<Loops>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
) {
    let events = std::mem::take(&mut cues.events);
    let Some(mut library) = library else { return };
    // Cues raised while silenced are dropped, never played late in a burst.
    if super::silenced(menu.as_deref(), &replay) {
        return;
    }
    let loops = &mut *loops;
    let mut rng = loops.rng;
    let dt = time.delta_secs().clamp(0.0, 0.25);
    let now = time.elapsed_secs_f64();
    let mut player = Player { commands, library: &mut library, voices: &mut voices, assets: &mut assets, now };
    for event in events {
        // Safety net: a cue never repeats faster than its minimum gap.
        let name = event.name();
        if loops.last.get(name).is_some_and(|t| now - t < f64::from(cues::min_gap(name))) {
            continue;
        }
        loops.last.insert(name, now);
        match event {
            Event::Pop(at) => player.once("pop", &cues::POP, 1.0, at, &mut rng),
            Event::Flip(at) => player.once("flip", &cues::FLIP, 1.0, at, &mut rng),
            Event::Land { at, impact } => {
                let scale = 0.3 + 0.7 * (impact / FULL_IMPACT).clamp(0.0, 1.0);
                player.once("land", &cues::LAND, scale, at, &mut rng);
                // Weight layer: from 1.5 m/s, louder and deeper (down to -20 %
                // pitch) as the impact grows.
                let heavy = ((impact - 1.5) / 7.5).clamp(0.0, 1.0);
                if heavy > 0.0 {
                    player.once_pitched("land_heavy", &cues::LAND_HEAVY, 0.4 + 0.6 * heavy, 1.0 - 0.2 * heavy, at, &mut rng);
                }
            }
            Event::BoardDown { at, impact } => {
                // Set down gently: the soft sample, quietly. Jumped on (caveman):
                // louder, plus a landing impact layer that grows with the impact.
                let (soft, heavy) = board_down_levels(impact);
                player.once("board_down", &cues::BOARD_DOWN, soft, at, &mut rng);
                if heavy > 0.0 {
                    player.once("board_down_heavy", &cues::LAND_HEAVY, heavy, at, &mut rng);
                }
            }
            Event::Bail { at, speed } => {
                let tier = cues::bail_tier(speed);
                let cue = cues::Cue { samples: cues::BAIL_TIERS[tier], ..cues::BAIL };
                player.once(["bail_soft", "bail_medium", "bail_hard"][tier], &cue, 1.0, at, &mut rng);
            }
            Event::Push(at) => player.once("push", &cues::PUSH, 1.0, at, &mut rng),
            Event::Step { at, run, level } => {
                player.once(if run { "run_step" } else { "step" }, if run { &cues::RUN_STEP } else { &cues::STEP }, level, at, &mut rng)
            }
            Event::Splash { at, speed } => {
                let scale = 0.4 + 0.6 * (speed / 8.0).min(1.0);
                player.play_record("splash", &cues::SPLASH, scale, at, &mut rng, &mut loops.patch_order);
            }
        }
    }

    let r = cues.riding;
    let fraction = (r.speed.abs() / FULL_ROLL_SPEED).clamp(0.0, 1.0);

    // Rolling: the grain band recorded at about this speed. The recordings get
    // louder with speed by themselves, so the level only fades in from a stop.
    let rolling = r.rolling && !r.grinding && r.speed.abs() > 0.3;
    loops.roll_speed += (r.speed.abs() - loops.roll_speed) * (1.0 - (-dt / ROLL_SPEED_SMOOTHING).exp());
    let touching = cues::grain_for(r.surface);
    // Off the ground for a moment (air, coping, ramp transition): stop the
    // loop, and on touching down start straight on the surface underneath —
    // the stability wait below is only for surface changes while rolling.
    loops.roll_off = if rolling { 0.0 } else { loops.roll_off + dt };
    if loops.roll_off > ROLL_OFF_STOP && loops.roll.voice.is_some() {
        loops.roll.stop(player.voices, 0.1);
        loops.roll_band = None;
        loops.roll_grain = None;
        loops.roll_candidate = None;
    }
    let grain = match (loops.roll_grain, loops.roll_candidate) {
        _ if loops.roll.voice.is_none() => touching,
        (None, _) => touching,
        (Some(current), _) if current == touching => current,
        (Some(current), Some((candidate, held))) if candidate == touching => {
            if held + dt >= SURFACE_HOLD { touching } else { loops.roll_candidate = Some((candidate, held + dt)); current }
        }
        (Some(current), _) => { loops.roll_candidate = Some((touching, 0.0)); current }
    };
    if Some(grain) != loops.roll_grain || grain == touching {
        loops.roll_candidate = None;
    }
    loops.roll_grain = Some(grain);
    let bands = player.library.grain_bands(grain);
    loops.roll.prune(player.voices);
    loops.roll_held += dt;
    if bands > 0 {
        let current = loops.roll_band.filter(|(g, _)| *g == grain).map(|(_, b)| b);
        let wanted = band_for((loops.roll_speed / FULL_ROLL_SPEED).clamp(0.0, 1.0), bands, current);
        // A band plays for at least BAND_HOLD before the next speed change.
        let band = match current {
            Some(band) if loops.roll_held < BAND_HOLD && loops.roll.voice.is_some() => band,
            _ => wanted,
        };
        let volume = if rolling { cues::ROLL_LEVEL * (r.speed.abs() / 1.5).min(1.0) } else { 0.0 };
        // Within a band, follow speed with a slight pitch change (+-6%).
        let scaled = (loops.roll_speed / FULL_ROLL_SPEED).clamp(0.0, 1.0) * (bands - 1) as f32;
        let pitch = 1.0 + 0.06 * (scaled - band as f32).clamp(-1.0, 1.0);
        if rolling && loops.roll_band != Some((grain, band)) {
            loops.roll_held = 0.0;
            loops.roll.stop(player.voices, 0.15);
            if let Some(clip) = player.library.grain(player.assets, grain, band) {
                info!("AUDIO_EVENT rolling surface={} grain={grain} band={band}", r.surface);
                player.start_loop(&mut loops.roll, clip, volume, pitch, r.board, 0.15);
                loops.roll_band = Some((grain, band));
            }
        } else if let Some((id, _)) = &loops.roll.voice {
            player.voices.set(*id, volume, pitch, Some(r.board));
        }
    }

    // Grinds, powerslides and foot drag loop while the state lasts. Braking is
    // reported in pulses, so it is held briefly.
    let dragging = r.braking && !r.airborne && !r.grinding;
    let grind = if cues::metal(r.grind_surface) { &cues::GRIND_METAL } else { &cues::GRIND };
    player.hold(&mut loops.grind, r.grinding, grind, grind.level * (0.5 + 0.5 * fraction), r.board, &mut rng);
    let sliding = r.sliding && !r.grinding;
    player.shuffle("powerslide", &mut loops.slide_next, sliding, dt, &cues::POWERSLIDE, cues::POWERSLIDE_SHUFFLE, fraction, r.board, &mut rng);
    player.shuffle("foot_drag", &mut loops.drag_next, dragging, dt, &cues::FOOT_DRAG, cues::FOOT_DRAG_SHUFFLE, 0.3 + 0.7 * fraction, r.board, &mut rng);

    // Wheels spin down after take-off; cut when they touch again.
    loops.wheels.prune(player.voices);
    if r.airborne && !loops.was_airborne && fraction > 0.1 {
        loops.wheels.stop(player.voices, 0.05);
        if let Some(clip) = player.library.wheels(player.assets, cues::AIR_WHEELS.0) {
            let mut play = Play::effect(cues::AIR_WHEELS.1 * fraction, r.board);
            play.fade_in = 0.05;
            if let Some(id) = player.voices.play(&mut player.commands, &clip, play, player.now) {
                loops.wheels.voice = Some((id, clip));
            }
        }
    } else if !r.airborne {
        loops.wheels.stop(player.voices, 0.05);
    } else if let Some((id, _)) = &loops.wheels.voice {
        player.voices.set(*id, cues::AIR_WHEELS.1 * fraction, 1.0, Some(r.board));
    }
    loops.was_airborne = r.airborne;
    loops.rng = rng;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_stays_in_unit_range() {
        let mut rng = 0;
        for _ in 0..10_000 {
            let x = random(&mut rng);
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn speed_bands_have_hysteresis() {
        assert_eq!(band_for(0.0, 6, None), 0);
        assert_eq!(band_for(1.0, 6, None), 5);
        assert_eq!(band_for(2.0, 6, None), 5);
        // 0.5 of 6 bands = 2.5: stays on the current band near the boundary.
        assert_eq!(band_for(0.5, 6, Some(2)), 2);
        assert_eq!(band_for(0.5, 6, Some(3)), 3);
        assert_eq!(band_for(0.75, 6, Some(2)), 4);
    }

    #[test]
    fn one_strike_per_step_with_a_learned_resting_height() {
        let mut foot = FootStrike::default();
        let dt = 1.0 / 60.0;
        let mut strikes = 0;
        // Ankle rests 0.09 m above the ground; each step lifts it ~0.12 m.
        for step in 0..4 {
            for tick in 0..30 {
                let phase = tick as f32 / 30.0;
                let lift = if phase < 0.5 { (phase * std::f32::consts::TAU).sin().abs() * 0.12 } else { 0.0 };
                strikes += foot.update(Some(0.09 + lift + step as f32 * 0.001), dt) as u32;
            }
        }
        assert_eq!(strikes, 4);
        // Standing still or losing the ground never strikes.
        let mut still = FootStrike::default();
        assert!((0..120).all(|_| !still.update(Some(0.09), dt)));
        assert!(!still.update(None, dt));
    }

    #[test]
    fn board_down_follows_the_impact() {
        assert_eq!(board_down_levels(0.0), (0.3, 0.0));
        let (soft, heavy) = board_down_levels(2.0);
        assert!(soft > 0.3 && soft < 1.0 && heavy == 0.0);
        let (soft, heavy) = board_down_levels(6.5);
        assert_eq!((soft, heavy), (1.0, 1.0));
        assert_eq!(board_down_levels(f32::NAN), (0.3, 0.0));
    }

    #[test]
    fn grind_states_are_the_six_grinds() {
        assert!(grinding(PhysicalStateId::GrindBoardslide as u32));
        assert!(grinding(PhysicalStateId::GrindDarkslide as u32));
        assert!(!grinding(PhysicalStateId::WipeoutGround as u32));
        assert!(!grinding(PhysicalStateId::SlideGround as u32));
    }
}
