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
    /// Wheels in contact (0..4) and pushes since start (the native rolling bed reads these).
    pub wheels: u32,
    pub pushes: u32,
    /// The retail audio-state view of this tick for the native runtime (`skate_audio::player`).
    pub audio: skate_audio::player::AudioState,
    /// The loose-board state's inputs (`PlayerAudio::loose_board`, conditioner `sub_827A1B78`):
    /// `up_dot` = SkateboardReckoning+80 (the deck's physical up) · Ground+80 (the retained
    /// wheel-contact normal, Board Fill82C03318); the deck contact Collision+3475 and its material
    /// Collision+12 − 1 (read raw: the riderless-board gate of the record's wheel fields does not
    /// apply to these).
    pub deck_up: f32,
    pub deck_contact: bool,
    pub deck_material: u32,
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
    /// Seconds in the current air phase (audio state `+236`).
    air_time: f32,
    /// Latched grind family / material (`+192` / `+692`).
    grind_family: Option<i32>,
    grind_material: Option<u32>,
    /// `+468` (written while Air440 holds).
    jump_velocity: f32,
    /// `+228` the last positive grind impact, and the conditioner's 4-frame deck-impact ring
    /// (`+668`).
    grind_impact: f32,
    deck_ring: [f32; 4],
    deck_at: usize,
    /// The conditioner's step code (`+740`, `sub_827729B8`).
    step_code: skate_audio::player::footsteps::StepCode,
    /// Rows written to the audio state log.
    log_frame: u64,
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
/// The skater is off the board or wiping out (Wipeout 300, Offboard 500 / 501 / 502): the riding
/// board's contacts are not the player's. Retail's audio record reports no wheels (material 143)
/// through such stretches — clean GREC sessions show it moving at speed with wheels 0, air 0 and
/// material 143 for whole stretches, never flickering — while our riding ground state keeps
/// reporting the riderless board's touches (the 19:29 listening test: a bed and skid burst when
/// the dropped board touched down after a trick). UNCERTAIN: the record writer itself (+152 bits
/// 20–22) is not located; this is the measured behaviour.
pub(super) fn board_unridden(state: u32) -> bool {
    matches!(state, 300 | 500..=502)
}

fn wheel_surface(physics: &GamePhysics) -> (u32, u32) {
    let ground = &physics.riding.ground;
    let lines = &physics.riding.wheel_lines;
    let touching: Vec<u32> = (0..4).filter(|&i| ground.parts[i].in_contact).map(|i| lines.audio_surfaces[i]).collect();
    let surface = touching.iter().copied().max_by_key(|s| touching.iter().filter(|t| *t == s).count());
    (touching.len() as u32, surface.unwrap_or(0))
}

pub(super) fn observe(
    physics: Res<GamePhysics>,
    mut skater: ResMut<SkaterRuntime>,
    mut cues: ResMut<Cues>,
    mut seen: Local<Seen>,
    time: Res<Time>,
) {
    let _timing = super::timing::scope(&super::timing::OBSERVE);
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
    let unridden = board_unridden(state);
    let (wheels, surface) = if unridden { (0, 0) } else { wheel_surface(&physics) };
    let rolling = wheels > 0;
    let memory: &mut Seen = &mut seen;
    let audio_state = audio_state(&physics, &skater, AudioFrame {
        dt: time.delta_secs(),
        state,
        unridden,
        wheels,
        board: (board, vec(deck.linear_velocity)),
        on_rail,
        push: audio.push,
        push_trigger: memory.started && audio.push && !memory.push,
        air_time: &mut memory.air_time,
        grind_family: &mut memory.grind_family,
        grind_material: &mut memory.grind_material,
        jump_velocity: &mut memory.jump_velocity,
        grind_impact: &mut memory.grind_impact,
        deck_ring: &mut memory.deck_ring,
        deck_at: &mut memory.deck_at,
        step_code: &mut memory.step_code,
        slip: if wheels > 0 {
            let ri = physics.board.part_transforms()[BodyId::Deck.index()].basis.columns[0];
            skate_audio::player::state::slip(vec(deck.linear_velocity).dot(Vec3::from_array(ri)))
        } else {
            0.0
        },
        deck_spin: {
            let at = physics.board.part_transforms()[BodyId::Deck.index()].basis.columns[2];
            vec(deck.angular_velocity).dot(Vec3::from_array(at))
        },
        deck_spin_xy: {
            let basis = physics.board.part_transforms()[BodyId::Deck.index()].basis.columns;
            let w = vec(deck.angular_velocity);
            [w.dot(Vec3::from_array(basis[0])), w.dot(Vec3::from_array(basis[1]))]
        },
    });
    if super::state_log::enabled() {
        let p = &skater.player_input.physical;
        let s = &audio_state;
        let v = vec(deck.linear_velocity);
        let lateral = Vec3::from_array(physics.board.part_transforms()[BodyId::Deck.index()].basis.columns[0]);
        let air440 = skater.player_input.processed.flags_2468 & 0x0040_0000 != 0;
        let [jx, jy, jz] = raw3(p.air.jump_velocity_delta_112);
        super::state_log::write(&super::state_log::Row {
            ms: time.elapsed_secs_f64() * 1000.0,
            frame: seen.log_frame,
            speed: s.ground_speed,
            turn: s.turn,
            wheels: (0..4).filter(|&i| s.wheel_contact[i]).map(|i| 1u32 << i).sum(),
            tag: surface,
            air: s.airborne,
            air_time: s.air_time,
            to_land: p.air.scalar_184,
            jump_height: p.air.jump_height_200,
            jv: if air440 { (jx * jx + jy * jy + jz * jz).sqrt() } else { 0.0 },
            grinding: s.grinding,
            family: if s.grinding { s.grind_family } else { 0 },
            grind_tag: if s.grinding { s.grind_material } else { 0 },
            brake: s.brake,
            manual: s.manual_brake,
            balance: s.balance,
            scorable: s.scorable,
            push: s.push_trigger,
            slope: s.slope,
            tilt: skater.ground.steering.deck_tilt,
            slip: if v.length() > 1e-3 { v.dot(lateral) / v.length() } else { 0.0 },
            feet: u32::from(s.feet_in_deck_box[0]) | (u32::from(s.feet_in_deck_box[1]) << 1),
            state,
            board: board.to_array(),
            com: s.com_position,
            grind_impact: s.grind_impact,
            deck_impact: s.deck_impact,
            deck_tag: physics.riding.ground.part_audio_surfaces[2],
            foot_y: s.foot_speed_y,
            foot_xz: s.foot_speed_xz,
            seam: [s.seam_pattern[0], s.seam_pattern[3]],
            wheel: [s.wheel_position[0][0], s.wheel_position[0][2]],
            heading: {
                let at = physics.board.part_transforms()[BodyId::Deck.index()].basis.columns[2];
                at[0].atan2(at[2])
            },
            foot_down: u32::from(s.foot_down[0]) | (u32::from(s.foot_down[1]) << 1),
            foot_material: s.foot_material,
            hands: u32::from(s.hands_on_deck[0]) | (u32::from(s.hands_on_deck[1]) << 1),
            strength: s.footstep_strength,
            foot_vy: s.foot_vertical_speed,
            step: s.step_code,
            body: s.body_speed,
            limb: s.limb_speed,
            slide: s.body_slide.iter().copied().fold(0.0, f32::max),
            deck_up: deck_up(&physics, &skater),
            deck_contact: p.collision.flag_3475 != 0,
            lines: if unridden { 0 } else { (0..4).filter(|&i| physics.riding.wheel_lines.audio_surfaces[i] != 0).map(|i| 1u32 << i).sum() },
        });
    }
    let pushes = cues.riding.pushes.wrapping_add(u32::from(seen.started && audio.push && !seen.push));
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
        wheels,
        pushes,
        audio: audio_state,
        deck_up: deck_up(&physics, &skater),
        deck_contact: skater.player_input.physical.collision.flag_3475 != 0,
        deck_material: skate_audio::player::state::material_of_tag(physics.riding.ground.part_audio_surfaces[2]),
    };
    *seen = Seen {
        started: true, launched, filtered, state, trick_seq, in_water, splash_cooldown: cooldown,
        push: audio.push, feet: seen.feet, air_from_foot, since_foot, braking, brake_time, brake_event, on_rail, since_splash, trace: Some(trace),
        air_trick: if filtered == air { seen.air_trick.take() } else { None },
        air_time: seen.air_time,
        grind_family: seen.grind_family,
        grind_material: seen.grind_material,
        jump_velocity: seen.jump_velocity,
        grind_impact: seen.grind_impact,
        deck_ring: seen.deck_ring,
        step_code: seen.step_code,
        deck_at: seen.deck_at,
        log_frame: seen.log_frame + u64::from(super::state_log::enabled()),
    };
}

/// The audio state's `+236` (Air+176, the time in the air *state*) after this tick. It counts
/// while the air state (200..300) holds, so it keeps counting on the landing tick (wheels down,
/// `+332` already clear, the state still air) and drops to 0 with the state — the recomp's TREAT
/// capture (session all_20261002_223306: last air tick 0.750 s, landing tick 0.767 s, then 0; the
/// same one-tick hold as `+240`). `follow_state` off: the count before 2026-10-02 (air flag only,
/// 0 on the landing tick).
pub(super) fn air_time_236(previous: f32, state: u32, airborne: bool, dt: f32, follow_state: bool) -> f32 {
    let counting = if follow_state { (200..300).contains(&state) } else { airborne };
    if counting { previous + dt } else { 0.0 }
}

/// `SKATE_AEMS_AIR_TIME_STATE=0` restores the audio state's `+236` count from before 2026-10-02
/// (reset on the landing tick instead of with the air state).
fn air_time_follows_state() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("SKATE_AEMS_AIR_TIME_STATE").is_ok_and(|v| v == "0"))
}

/// What [`audio_state`] needs beyond the physics resources.
struct AudioFrame<'a> {
    dt: f32,
    state: u32,
    /// [`board_unridden`]: no board contacts reach the record.
    unridden: bool,
    wheels: u32,
    board: (Vec3, Vec3),
    on_rail: bool,
    push: bool,
    push_trigger: bool,
    air_time: &'a mut f32,
    grind_family: &'a mut Option<i32>,
    grind_material: &'a mut Option<u32>,
    jump_velocity: &'a mut f32,
    grind_impact: &'a mut f32,
    deck_ring: &'a mut [f32; 4],
    deck_at: &'a mut usize,
    step_code: &'a mut skate_audio::player::footsteps::StepCode,
    /// `+232` from the deck's lateral speed (0 with no wheel down).
    slip: f32,
    /// `+488` the deck's angular velocity about its At axis.
    deck_spin: f32,
    /// `+480` / `+484` the same about its Ri and Up axes.
    deck_spin_xy: [f32; 2],
}

fn raw3(v: [u32; 4]) -> [f32; 3] {
    [f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2])]
}

/// The loose-board `up_dot` (`sub_827A1B78` at 0x827A2A60): SkateboardReckoning+80 (Y of
/// GetEffectiveTransform82C01BF8 = the physical deck's up; the stance flip only negates X / Z) ·
/// Ground+80 (the retained wheel-contact normal), three lanes.
fn deck_up(physics: &GamePhysics, skater: &SkaterRuntime) -> f32 {
    let up = physics.board.part_transforms()[BodyId::Deck.index()].basis.columns[1];
    let n = raw3(skater.player_input.physical.ground.vector_80);
    up[0] * n[0] + up[1] * n[1] + up[2] * n[2]
}

/// |v| over three lanes (retail: x · the refined reciprocal square root, 0 for x = 0).
fn length3(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The retail audio-state record (`skate_audio::player::state`) from this tick's physics: what the
/// bridge `sub_824B0DA8` reads, mapped to the engine's equivalents (field docs there).
/// - `+332` in the air = state 200..300 with no wheel down (ProcessOutput's KnownAir test); our
///   engine rarely selects KnownAir itself, so `+236` is our own time in that air phase.
/// - `+343` / `+372`: the score packet's scorable id (bit 24 or 25 of its flags, else none);
///   hippy jump = id 234 (`hippyjump`) while in the air.
/// - `+192` / `+692` latch the grind family / material while grinding (−1 / 143 before the first).
fn audio_state(physics: &GamePhysics, skater: &SkaterRuntime, f: AudioFrame) -> skate_audio::player::AudioState {
    use skate_audio::player::state::{AudioState, material_of_tag};
    let p = &skater.player_input.physical;
    let flags = &skater.player_state.state_flags;
    let flag = |byte: usize| flags.get(byte - 52).copied().unwrap_or(false);
    let ground = &physics.riding.ground;
    let lines = &physics.riding.wheel_lines;
    let wheel_contact: [bool; 4] = std::array::from_fn(|i| !f.unridden && ground.parts[i].in_contact);
    let airborne = (200..300).contains(&f.state) && f.wheels == 0;
    *f.air_time = air_time_236(*f.air_time, f.state, airborne, f.dt, air_time_follows_state());
    let grinding = f.on_rail;
    if p.grinds.grinding_316 != 0 || grinding {
        *f.grind_family = Some(p.grinds.words_136_140[0] as i32);
        *f.grind_material = Some(p.grinds.audio_surface_216.min(143));
    }
    let score = &skater.animation.motion.score_packet;
    let scorable = if score.flags & 0x0300_0000 != 0 {
        score.trick_names.first.and_then(|name| {
            skater.scoring.data.definitions.iter().find(|d| d.encoded_name == name).map(|d| d.metadata.id as i64)
        })
    } else {
        None
    };
    // +468: the conditioner writes |Air+112| / 2.65 while Air440 (Processed2468 bit 22) holds.
    if skater.player_input.processed.flags_2468 & 0x0040_0000 != 0 {
        let [x, y, z] = raw3(p.air.jump_velocity_delta_112);
        *f.jump_velocity = skate_audio::player::state::jump_velocity((x * x + y * y + z * z).sqrt());
    }
    let (board, board_velocity) = f.board;
    // +228: B40+32 = Grinds+128 when positive, else the last one (`sub_827731C8`).
    if p.grinds.impact_speed_128 > 0.0 {
        *f.grind_impact = p.grinds.impact_speed_128;
    }
    // +668: the conditioner's max of the last 4 Collision+24 = clamp01(|deck acceleration · deck
    // contact normal| × 0.00125) with the deck in contact (`sub_82C02A80`, `sub_82772748`).
    let deck = BodyId::Deck.index();
    let deck_contact = !f.unridden && ground.parts[deck].in_contact;
    let impact = if deck_contact {
        let (a, n) = (ground.accelerations[deck], ground.parts[deck].normal);
        let d = (a.x * n.x + a.y * n.y + a.z * n.z).abs() * f32::from_bits(0x3AA3_D70A);
        skate_audio::player::clamp01(d)
    } else {
        0.0
    };
    f.deck_ring[*f.deck_at % 4] = impact;
    *f.deck_at = (*f.deck_at + 1) % 4;
    let deck_impact = f.deck_ring.iter().copied().fold(0.0f32, f32::max);
    // +268..+280: the toes' local velocities in the physical board's frame (Skeleton+192..+216).
    let toes = skater.foot_physical.output.local_velocity;
    // Off-board inputs (`player::footsteps` / `clothing` / `step_on`; spec
    // `.claude/notes/aems-offboard-clothing-spec.md` §3). Foot A = toe part 19 (OffBoard side 1),
    // foot B = part 15.
    let world_toes = skater.foot_physical.output.world_velocity;
    let push_planted = flag(55);
    // Skeleton+602/+603: the IK's hand targets (limbs 2 / 3) relative to the animated board inside
    // the deck box padded by physics_skeletonik `A28E50D30B0506A4`.
    let (half_width, half_length) = skater.foot_physical.deck_box();
    let hands = [2usize, 3].map(|limb| {
        let t = skater.foot_ik.state.frames[limb].board[3];
        skate_audio::player::step_on::hand_in_deck_box([t[0], t[1], t[2]], half_width, half_length, skate_audio::player::step_on::HAND_PADDING)
    });
    // Skeleton::FillPhysOut (`sub_82BE1AE8`) publishes, from the physical record and the
    // skeleton assembly's bodies (assembly +76 + 96·part):
    // - +144 / +160 = the physical pose translations of toe parts 15 / 19 (the step code's plant
    //   heights);
    // - +288 / +304 / +320 = the angular velocity (body +48) of parts 23 / 20 / 16, which the record
    //   writer turns into +328 = |+288| (the body speed) and +296 / +292 = |y| of +304 / +320 (foot
    //   A / B "vertical" speeds);
    // - +560..+572 = |record velocity − Skeleton16176 (the physical COM velocity)| of parts 17, 21,
    //   4, 8, averaged into +672 = 0.25 · (572 + 568 + 564 + 560).
    let record = &skater.skeleton.record;
    let bodies = skater.skeleton.bodies();
    let spin = |part: usize| {
        let w = bodies[part].rates.angular_velocity;
        [w.x, w.y, w.z]
    };
    let com16176 = raw3(p.reckoning.vector_16);
    let limb = |part: usize| {
        let v = record.velocities[part];
        length3([v[0] - com16176[0], v[1] - com16176[1], v[2] - com16176[2]])
    };
    let limb_speed = (((limb(8) + limb(4)) + limb(21)) + limb(17)) * 0.25;
    let step_code = f.step_code.update(
        [p.off_board.flags_306_307[0] != 0, p.off_board.flags_306_307[1] != 0],
        [skater.player_input.processed.left_surface_2596, skater.player_input.processed.right_surface_2600],
        [record.pose[15][3][1], record.pose[19][3][1]],
    );
    // The ragdoll's body regions (SkeletonCollision, published by `sub_82BD60C8` into
    // Collision+80..+195 and copied to the audio state +496..+611): per region with a contact
    // part its tangential (slide) speed (+944 + 4i → +528..+548) and surface tag (+976 + 4i →
    // +560..+580; the per-frame PhysOut reset leaves 0 without one), and the face point's contact
    // (specific point 1, byte 4009 → +593).
    let fb = &skater.collision_feedback;
    let xz = |v: [f32; 4]| if v[0].abs() - v[2].abs() >= 0.0 { v[0].abs() } else { v[2].abs() };
    AudioState {
        dt: f.dt,
        ground_speed: physics.riding.motion.ground_speed,
        com_velocity: raw3(p.reckoning.vector_16),
        com_position: raw3(p.reckoning.vector_64),
        board_position: board.to_array(),
        board_velocity: board_velocity.to_array(),
        wheel_count: f.wheels,
        wheel_contact,
        // `+620..+632` / `+636..+648` come from the wheel lines (82C079E0: each wheel's 0.2 m ray), not
        // from contact: retail keeps wheel 0's material on 100 % of 3-wheel and 96 % of 2-wheel frames
        // and changes it 0.9 times a second while rolling (GREC, `aems-port/tools/grec_material.py`).
        // Gating them by contact made every contact flicker a material change, i.e. a Class_Seams
        // transition hit (5–9 changes/s on wheel 0 in the 20:08 / 20:13 sessions).
        wheel_material: std::array::from_fn(|i| if f.unridden { 143 } else { material_of_tag(lines.audio_surfaces[i]) }),
        seam_pattern: std::array::from_fn(|i| if f.unridden { 0 } else { lines.seam_patterns[i] }),
        wheel_position: {
            let parts = physics.board.part_transforms();
            std::array::from_fn(|i| {
                let p = parts[i].translation;
                [p.x, p.y, p.z]
            })
        },
        turn: skater.animation_input.fields.turn,
        slope: skater.ground.pumping.absorption,
        airborne,
        air_time: *f.air_time,
        brake: flag(52),
        manual_brake: flag(54),
        balance: flag(60),
        grinding,
        trick_active: scorable.is_some(),
        hippy_jump: airborne && scorable == Some(234),
        bail: flag(59),
        bail_end: p.skeleton.over_599 != 0,
        on_foot: f.state == 500,
        soft_wheels: skater.player_input.processed.scalar_2764 < 0.5,
        push_planted: f.push,
        push_trigger: f.push_trigger,
        feet_in_deck_box: [p.skeleton.flag_600 != 0, p.skeleton.flag_601 != 0],
        grind_family: f.grind_family.unwrap_or(-1),
        grind_material: f.grind_material.unwrap_or(143),
        local: true,
        jump_velocity: *f.jump_velocity,
        audio_trick: -1,
        scorable: scorable.map_or(-1, |id| id as i32),
        slip: f.slip,
        // +690: RevertGround (102) with State+66 set.
        revert: f.state == 102 && flag(66),
        // `+308` = OffBoard 311 (the board held in hand).
        offboard_308: p.off_board.flag_311 != 0,
        deck_tilt: skater.ground.steering.deck_tilt,
        deck_spin: f.deck_spin,
        grind_impact: *f.grind_impact,
        deck_impact,
        deck_material: if f.unridden { 143 } else { material_of_tag(ground.part_audio_surfaces[2]) },
        foot_speed_y: [toes[0][1].abs(), toes[1][1].abs()],
        foot_speed_xz: [xz(toes[0]), xz(toes[1])],
        jump_bucket: 0,
        // `+352`: the host resolves it from the scorable (`PlayerTuning::audio_trick_2`).
        audio_trick_2: -1,
        offboard_310: false,
        deck_spin_xy: f.deck_spin_xy,
        // `+240` / `+260`: KnownAir's predicted time until landing and jump height (Air+184 / +200).
        air_until_landing: p.air.scalar_184,
        jump_height: p.air.jump_height_200,
        // `+220` the game time scale and `+224` (0 in free skate).
        time_scale: 1.0,
        global_224: false,
        foot_down: [
            p.off_board.flags_306_307[1] != 0 || p.air.footplant_right_450 != 0 || (push_planted && flag(57)) || flag(52),
            p.off_board.flags_306_307[0] != 0 || p.air.footplant_left_449 != 0 || (push_planted && !flag(57) && flag(56)),
        ],
        foot_material: [
            material_of_tag(skater.player_input.processed.right_surface_2600 & 0x7F),
            material_of_tag(skater.player_input.processed.left_surface_2596 & 0x7F),
        ],
        foot_xz_speed: [xz(world_toes[1]), xz(world_toes[0])],
        foot_vertical_speed: [spin(20)[1].abs(), spin(16)[1].abs()],
        step_code,
        footstep_strength: skater.animation_input.extra.footstep_strength,
        // `+300`: the host's LandingBucket (it needs the resolved audio trick).
        landing_bucket: 1,
        footplant: p.air.flag_448 != 0,
        offboard_air: p.filtered_state_0 == 7,
        push_stroke: flag(56),
        body_speed: length3(spin(23)),
        limb_speed,
        body_slide: std::array::from_fn(|i| if fb.regions[i].part.is_some() { fb.regions[i].tangent_speed } else { 0.0 }),
        body_tag: std::array::from_fn(|i| if fb.regions[i].part.is_some() { fb.regions[i].material_flags } else { 0 }),
        body_slide_flag: fb.specific[1].current,
        hands_on_deck: skate_audio::player::step_on::hands_on_deck(hands, f.state == 500, p.off_board.flag_311 != 0),
    }
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
    /// Grinding last tick (start patch on the rising edge), and the countdown
    /// to the next re-fired scrape piece on non-metal grinds.
    was_grinding: bool,
    grind_piece_next: f32,
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
    /// Delayed retail layers: (due time, name, record, scale, position).
    delayed: Vec<(f64, &'static str, &'static cues::Record, f32, Vec3)>,
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

/// Play a retail layer now, or queue it for its retail offset (`Record::delay`).
#[allow(clippy::too_many_arguments)]
fn layer(player: &mut Player, loops: &mut Loops, now: f64, name: &'static str, record: &'static cues::Record,
         scale: f32, at: Vec3, rng: &mut u32) {
    if record.delay > 0.0 {
        loops.delayed.push((now + f64::from(record.delay), name, record, scale, at));
    } else {
        player.play_record(name, record, scale, at, rng, &mut loops.patch_order);
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
            play.envelope = record.envelope;
            if self.voices.play(&mut self.commands, &clip, play, self.now).is_some() {
                played.push(sample);
            }
        }
        if !name.is_empty() {
            info!("AUDIO_CUE {name} {}:record{id} samples={played:?}", record.bank);
        }
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
    native: Option<Res<super::native::Native>>,
) {
    let _timing = super::timing::scope(&super::timing::PLAY);
    let events = std::mem::take(&mut cues.events);
    // The native granular bed (grain_bed.rs) replaces the interim speed-band loop when it runs.
    let native_bed = native.as_deref().is_some_and(|n| n.bed.is_some());
    // The native player components (player_audio.rs) replace the metal GRINDS loop, the
    // sense_of_speed bed cue and the foot-drag pieces when they run.
    let native_player = native.as_deref().is_some_and(|n| n.player.as_ref().is_some_and(|p| p.components));
    // The native board contacts (Splice) replace the interim pop layers and landing impacts.
    let native_contacts = native.as_deref().is_some_and(|n| n.player.as_ref().is_some_and(|p| p.components && p.contacts_on));
    // SFXObj_Wheels (player_audio.rs) replaces the interim air wheel spin.
    let native_wheels = native.as_deref().is_some_and(|n| n.player.as_ref().is_some_and(|p| p.components && p.wheels_on));
    // The native Tricks component (Class_Flips) replaces the interim flip whoosh; the native
    // rolling layers / rattle replace the interim bed's PatchBank_Rolling_Surfaces / Rolling_Rattles.
    let native_part = |f: fn(&super::player_audio::PlayerAudio) -> bool| native.as_deref().is_some_and(|n| n.player.as_ref().is_some_and(|p| p.components && f(p)));
    let native_tricks = native_part(|p| p.tricks_on);
    // SFXObj_OffBoard's footsteps (on foot, and the push plants on the board) replace the interim
    // steps and push; the Contacts' hands-on-deck sounds (1124 / 1125 / 1126) replace the interim
    // "foot on deck" 1126 at a board put-down.
    let native_footsteps = native_part(|p| p.footsteps_on);
    let native_rolling = native_part(|p| p.rolling_on);
    let native_rattle = native_part(|p| p.rattle_on);
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
    // Layers whose retail offset has come due (frame-granular, <= one frame late).
    let due: Vec<_> = loops.delayed.iter().filter(|d| d.0 <= now).cloned().collect();
    loops.delayed.retain(|d| d.0 > now);
    for (_, name, record, scale, at) in due {
        player.play_record(name, record, scale, at, &mut rng, &mut loops.patch_order);
    }
    for event in events {
        // Safety net: a cue never repeats faster than its minimum gap.
        let name = event.name();
        if loops.last.get(name).is_some_and(|t| now - t < f64::from(cues::min_gap(name))) {
            continue;
        }
        loops.last.insert(name, now);
        match event {
            // Retail's measured layers per moment (cues.rs): pop crack + tail
            // impact + deck knock; flip whoosh + feet catching the board; the
            // wheels-down set + an impact family on landing.
            Event::Pop(_) if native_contacts => {}
            Event::Pop(at) => {
                for record in cues::POP {
                    layer(&mut player, loops, now, "pop", record, 1.0, at, &mut rng);
                }
                let tail = &cues::POP_TAIL[(random(&mut rng) * 3.0) as usize % 3];
                layer(&mut player, loops, now, "pop_tail", tail, 1.0, at, &mut rng);
            }
            Event::Flip(at) => {
                if !native_tricks {
                    player.once("flip", &cues::FLIP, 1.0, at, &mut rng);
                }
                // The feet catching the board are the native foot taps (`player::contacts`,
                // 1112..1120) when the native contacts run.
                if !native_contacts {
                    layer(&mut player, loops, now, "catch", &cues::CATCH, 1.0, at, &mut rng);
                }
            }
            // RETIRED from the native path (user-confirmed 19:19 build): the native contacts play the
            // landing impact pair, touchdowns and the shoe scuffs (`sk8_foley` 94 / 95, which the
            // interim LAND_CLOTH was); nothing interim plays at a landing then.
            Event::Land { .. } if native_contacts => {}
            Event::Land { at, impact } => {
                // Retail's board contact rule (cues.rs): hollow (wood) table or
                // normal, each contact kind with its retail chance, variant by
                // impact, gentle loudness rise with board speed.
                let riding = cues.riding;
                let hollow = cues::hollow(riding.surface);
                let table = if hollow { &cues::LAND_HOLLOW } else { &cues::LAND_NORMAL };
                let variant = cues::land_tier(impact);
                let scale = cues::land_scale(riding.speed);
                let mut kinds: Vec<usize> = (0..4).filter(|&k| random(&mut rng) < cues::LAND_KIND_CHANCE[k]).collect();
                if kinds.is_empty() {
                    kinds.push(1);
                }
                info!("AUDIO_EVENT land impact={impact:.2} variant={variant} hollow={hollow} kinds={kinds:?} scale={scale:.2}");
                for kind in kinds {
                    layer(&mut player, loops, now, "land_impact", &table[kind][variant], scale, at, &mut rng);
                }
                layer(&mut player, loops, now, "land_cloth", &cues::LAND_CLOTH, scale, at, &mut rng);
                player.play_record("land", &cues::LAND, scale, at, &mut rng, &mut loops.patch_order);
            }
            Event::BoardDown { at, impact } => {
                // Set down gently: foot on deck, knock, light wheel touch.
                // Jumped on (caveman): louder, plus the landing set.
                let (soft, heavy) = board_down_levels(impact);
                if !native_contacts {
                    player.play_record("board_down", &cues::BOARD_DOWN, soft, at, &mut rng, &mut loops.patch_order);
                }
                // With the native contacts the wheels' touchdowns (Contacts sets) and the feet's
                // deck taps (1112..1120) play from the retail mechanism; the interim knock and
                // touch would double them. (The foot-on-deck set stays interim: retail's step-on
                // `sub_824B85B0` needs Skeleton+602/+603, which the engine does not publish.)
                if !native_contacts {
                    player.play_record("board_down_knock", &cues::BOARD_DOWN_KNOCK, soft, at, &mut rng, &mut loops.patch_order);
                    player.play_record("board_down_touch", &cues::BOARD_DOWN_TOUCH, soft, at, &mut rng, &mut loops.patch_order);
                }
                if heavy > 0.0 && !native_contacts {
                    player.play_record("board_down_heavy", &cues::LAND, heavy, at, &mut rng, &mut loops.patch_order);
                }
            }
            Event::Bail { at, speed } => {
                let tier = cues::bail_tier(speed);
                let scale = [0.6, 0.8, 1.0][tier];
                player.play_record("bail_hit", &cues::BAIL_HIT, scale, at, &mut rng, &mut loops.patch_order);
                let cue = cues::Cue { samples: cues::BAIL_TIERS[tier], ..cues::BAIL };
                player.once(["bail_soft", "bail_medium", "bail_hard"][tier], &cue, 1.0, at, &mut rng);
            }
            Event::Push(_) | Event::Step { .. } if native_footsteps => {}
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
    if native_bed && loops.roll.voice.is_some() {
        loops.roll.stop(player.voices, 0.1);
        loops.roll_band = None;
    }
    if bands > 0 && !native_bed {
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
    // Riding bed: retail's continuous small sounds while rolling (cues.rs),
    // each source a random process at its measured rate x board speed.
    let bed = cues::bed_rate(r.speed) * if r.rolling && !r.grinding && !r.airborne { 1.0 } else { 0.0 };
    if bed > 0.0 {
        for (record, rate) in cues::BED_RECORDS {
            if native_contacts && cues::bed_record_native(record) {
                continue;
            }
            if random(&mut rng) < rate * bed * dt {
                player.play_record("", record, 1.0, r.board, &mut rng, &mut loops.patch_order);
            }
        }
        for (cue, rate) in cues::BED_CUES {
            // sense_of_speed: SenseOfSpeed rattle / wind; Seams_Bank: Class_Seams; Rolling_Rattles /
            // PatchBank_Rolling_Surfaces: the native rattle (posted on pushes) and Class_rolling's
            // continuous layers replace these random one-shots.
            if native_player && matches!(cue.bank, "sense_of_speed" | "Seams_Bank")
                || native_rolling && cue.bank == "PatchBank_Rolling_Surfaces"
                || native_rattle && cue.bank == "Rolling_Rattles"
            {
                continue;
            }
            if random(&mut rng) < rate * bed * dt {
                player.once_pitched("", cue, 1.0, 1.0, r.board, &mut rng);
            }
        }
    }

    // Grinds: retail's start patch, then on metal the GRINDS loop; on ledges
    // and other surfaces short scrape pieces re-fired every ~63 ms.
    let metal = cues::metal(r.grind_surface);
    // With the native contacts the collision manager plays the grind start (board / truck against
    // the grind material); the interim start and scrape pieces (Skate_Collisions 954 / 955 are the
    // skin / denim body-impact sounds, not grind layers) stay silent.
    if r.grinding && !loops.was_grinding && !native_contacts {
        let start = if metal { &cues::GRIND_METAL_START } else { &cues::GRIND_START };
        player.play_record("grind_start", start, 0.5 + 0.5 * fraction, r.board, &mut rng, &mut loops.patch_order);
        loops.grind_piece_next = cues::GRIND_PIECE_INTERVAL;
    }
    loops.was_grinding = r.grinding;
    player.hold(&mut loops.grind, r.grinding && metal && !native_player, &cues::GRIND_METAL, cues::GRIND_METAL.level * (0.5 + 0.5 * fraction), r.board, &mut rng);
    if r.grinding && !metal && !native_contacts {
        loops.grind_piece_next -= dt;
        if loops.grind_piece_next <= 0.0 {
            loops.grind_piece_next += cues::GRIND_PIECE_INTERVAL;
            player.play_record("", &cues::GRIND_PIECES, 0.5 + 0.5 * fraction, r.board, &mut rng, &mut loops.patch_order);
        }
    }
    let sliding = r.sliding && !r.grinding;
    // The native Class_wheels_skid / Class_Squeaks (player_audio.rs) replace the interim pieces.
    player.shuffle("powerslide", &mut loops.slide_next, sliding && !native_player, dt, &cues::POWERSLIDE, cues::POWERSLIDE_SHUFFLE, fraction, r.board, &mut rng);
    player.shuffle("foot_drag", &mut loops.drag_next, dragging && !native_player, dt, &cues::FOOT_DRAG, cues::FOOT_DRAG_SHUFFLE, 0.3 + 0.7 * fraction, r.board, &mut rng);

    // Wheels spin down after take-off; cut when they touch again.
    loops.wheels.prune(player.voices);
    if r.airborne && !loops.was_airborne && fraction > 0.1 && !native_wheels {
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

    /// `+236` across an ollie as the recomp's TREAT capture has it (session all_20261002_223306,
    /// 60 Hz ticks): 0 on the ground, 1/60 on the first air tick, still counting on the landing
    /// tick (state 201, wheels down), 0 once the state leaves the air. Off: 0 on the landing tick.
    #[test]
    fn air_time_holds_through_the_landing_tick() {
        let dt = 1.0 / 60.0;
        // (state, wheels down) per tick: rolling, takeoff, 3 air ticks, landing tick, rolling.
        let ticks = [(103, true), (201, false), (201, false), (201, false), (201, false), (201, true), (100, true)];
        let run = |follow: bool| {
            let mut t = 0.0f32;
            ticks.iter().map(|&(state, down)| {
                t = air_time_236(t, state, (200..300).contains(&state) && !down, dt, follow);
                (t * 60.0).round() as i32
            }).collect::<Vec<_>>()
        };
        assert_eq!(run(true), [0, 1, 2, 3, 4, 5, 0]);
        assert_eq!(run(false), [0, 1, 2, 3, 4, 0, 0]);
    }

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
