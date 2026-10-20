//! A rolling presentation recording, never a second simulation or a rewind of
//! live state. Root, skin (including the board), and camera share one playhead.
use crate::{
    app::SimulationSet,
    presentation::{Snapshot, blend},
};
use bevy::prelude::*;
use std::collections::VecDeque;

/// Retail records one frame per 60 Hz sim step, so its 5400-frame cap is a 90 s window (b345, REPLAYREC count run).
const RECORD_HZ: f64 = 60.0;
const SELECT: u16 = 0x0020;
const B: u16 = 0x2000;
const RB: u16 = 0x0200;
const DPAD_LEFT: u16 = 0x0004;
const DPAD_RIGHT: u16 = 0x0008;
/// The quit warning's callback id (engine popup channel, `popup_host::PopupMods`).
const QUIT_POPUP: &str = "engine:replay.quit";
/// Retail opens the replay only with more than 149 frames recorded (`8250B8F8`); read as frames of the editor's 30 fps
/// timeline, that is 5 s (the unit is inferred, research b343 / b389).
pub(crate) const MIN_OPEN_SECONDS: f64 = 150.0 / 30.0;

/// Retail's quit warning (research b387: ID_COMMON_WARNING_TITLE / ID_REPLAY_EDITOR_QUIT, No / Yes).
fn quit_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_COMMON_WARNING_TITLE".into(),
        description: "ID_REPLAY_EDITOR_QUIT".into(),
        options: vec!["ID_COMMON_NO".into(), "ID_COMMON_YES".into()],
        callback: QUIT_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
}

struct Frame {
    time: f64,
    pose: Snapshot,
    cut: bool,
    /// The player's audio step of this frame (`Cues::raw`): playback publishes it again, as retail's audio runs on
    /// the recorded skater state (provider id 4, b375).
    riding: Option<crate::game_audio::skate_events::Riding>,
}

/// At most this many recorded audio steps go to the host in one frame (a fast scrub merges the rest's pulses into
/// these, like the host's own step cap).
const MAX_FED_STEPS: usize = 8;

#[derive(Resource, Default)]
pub(crate) struct Replay {
    frames: VecDeque<Frame>,
    pub active: bool,
    pub free_camera: Option<Transform>,
    cursor: f64,
    playing: bool,
    previous_buttons: u16,
    release_controls: bool,
    /// Every other drawn actor ([`world`]), on the same clock as `frames`.
    pub(crate) world: skate_core::replay_buffer::ReplayBuffer,
    world_time: Option<f64>,
    /// Mod markers on the recording clock (retail provider id 10 holds up to 50).
    markers: VecDeque<Marker>,
    /// The mod whose `sdk.replay.set_limits` is in force (last writer wins).
    limits_owner: Option<String>,
    /// The player's Extended Replays length, s (PC overlay, not retail); None = retail's window. A mod's limits win.
    user_seconds: Option<u32>,
    /// The cursor the audio feed last published up to, and whether it moved forward this frame.
    fed: Option<f64>,
    audible: bool,
    /// How far the free camera may go from the player (m); None = retail's [`FREE_CAMERA_LEASH`]. Data, mod-settable.
    pub(crate) free_camera_leash: Option<f32>,
    /// The quit warning is up (waiting for its answer).
    quit_pending: bool,
    /// B / Back asked for the quit warning; it opens when the button is released.
    quit_on_release: bool,
    /// Shortest recording the replay opens on (s); None = retail's [`MIN_OPEN_SECONDS`]. Data, mod-settable.
    pub(crate) min_open_seconds: Option<f64>,
    /// The trigger scrub (retail rates as data) and its eased rate.
    pub(crate) scrub_spec: skate_core::replay_edit::ScrubSpec,
    scrub: skate_core::replay_edit::ScrubFilter,
    /// The cursor jumped (entered, rewound, or skipped more than [`MAX_FED_STEPS`]): the host's camera velocity resets.
    jumped: bool,
}

struct Marker {
    time: f64,
    label: String,
    owner: String,
}

/// Retail's marker cap (provider id 10: up to 50 x 48 B, b363).
const MAX_MARKERS: usize = 50;

impl Replay {
    pub fn record(&mut self, pose: Snapshot, dt: f64, cut: bool) {
        if self.active || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let time = self.frames.back().map_or(0.0, |f| f.time + dt);
        self.frames.push_back(Frame { time, pose, cut, riding: None });
        // Keep one bracketing sample at the start for fractional-time scrubs.
        let window = self.window();
        while self.frames.len() > 2 && self.frames[1].time <= time - window {
            self.frames.pop_front();
        }
        let start = self.bounds().0;
        while self.markers.front().is_some_and(|m| m.time < start) {
            self.markers.pop_front();
        }
    }

    fn window(&self) -> f64 {
        f64::from(self.world.limits.max_frames) / RECORD_HZ
    }

    /// The scrubbable span: the player's frames, cut to the actors' window when the byte budget evicted it first.
    fn bounds(&self) -> (f64, f64) {
        let end = self.frames.back().map_or(0.0, |f| f.time);
        let world_start = self.world.bounds().map_or(f64::MIN, |(start, _)| start);
        (
            self.frames
                .front()
                .map_or(0.0, |f| f.time)
                .max(end - self.window())
                .max(world_start.min(end)),
            end,
        )
    }

    /// The player's audio step for the newest frame (once per frame, after the capture).
    pub(crate) fn attach_riding(&mut self, riding: crate::game_audio::skate_events::Riding) {
        if let Some(frame) = self.frames.back_mut().filter(|f| f.riding.is_none()) {
            frame.riding = Some(riding);
        }
    }

    /// The recorded audio steps the cursor crossed going forward since the last call, oldest first; a jump back
    /// gives the step at the cursor without its one-step pulses (a reverse scrub replays no push). Nothing while the
    /// cursor stands still: then [`Replay::audible`] is false and the game sound is muted (retail mutes a paused
    /// replay).
    pub(crate) fn audio_steps(&mut self) -> Vec<crate::game_audio::skate_events::Riding> {
        if !self.active {
            self.fed = None;
            self.audible = false;
            return Vec::new();
        }
        let cursor = self.cursor;
        let previous = self.fed.replace(cursor);
        self.audible = previous.is_some_and(|p| cursor > p);
        self.jumped = previous.is_none_or(|p| cursor < p);
        let Some(previous) = previous.filter(|p| *p != cursor) else { return Vec::new() };
        if cursor < previous {
            let at = self.frames.partition_point(|f| f.time <= cursor).saturating_sub(1);
            return self.frames.get(at).and_then(|f| f.riding).map(|mut r| {
                r.audio.push_trigger = false;
                vec![r]
            }).unwrap_or_default();
        }
        let from = self.frames.partition_point(|f| f.time <= previous);
        let to = self.frames.partition_point(|f| f.time <= cursor);
        let steps: Vec<_> = self.frames.range(from..to).filter_map(|f| f.riding).collect();
        if steps.len() <= MAX_FED_STEPS {
            return steps;
        }
        // Keep the newest steps; the skipped ones' pushes land on the first one kept.
        self.jumped = true;
        let (skipped, kept) = steps.split_at(steps.len() - MAX_FED_STEPS);
        let mut kept = kept.to_vec();
        kept[0].audio.push_trigger |= skipped.iter().any(|r| r.audio.push_trigger);
        kept
    }

    pub(crate) fn jumped(&self) -> bool {
        self.jumped
    }

    /// The replay moved forward this frame (its sound plays); false while paused or rewinding.
    pub(crate) fn audible(&self) -> bool {
        self.audible
    }

    /// `sdk.replay.marker`: a marker at the newest frame; refused when nothing is recorded or the cap is reached.
    pub(crate) fn add_marker(&mut self, owner: &str, label: String) -> Result<(), String> {
        let time = self.frames.back().ok_or("replay_marker: nothing recorded yet")?.time;
        if self.markers.len() >= MAX_MARKERS {
            return Err(format!("replay_marker: {MAX_MARKERS} markers reached"));
        }
        self.markers.push_back(Marker { time, label, owner: owner.to_string() });
        Ok(())
    }

    /// The limits without a mod: retail's, or the player's Extended Replays length with the byte budget scaled by
    /// the same factor (90 s of a busy world filled ~125 MB of the 128 MB budget, laptop session 2026-10-20).
    fn base_limits(&self) -> skate_core::replay_buffer::ReplayLimits {
        let retail = skate_core::replay_buffer::ReplayLimits::default();
        match self.user_seconds {
            Some(seconds) => {
                let factor = f64::from(seconds) * RECORD_HZ / f64::from(retail.max_frames);
                skate_core::replay_buffer::ReplayLimits {
                    max_frames: (f64::from(seconds) * RECORD_HZ) as u32,
                    byte_budget: (retail.byte_budget as f64 * factor) as usize,
                }
            }
            None => retail,
        }
    }

    /// Extended Replays: the player's length (None = retail). Applies at once unless a mod's limits are in force.
    pub(crate) fn set_user_length(&mut self, seconds: Option<u32>) {
        if self.user_seconds == seconds {
            return;
        }
        self.user_seconds = seconds;
        if self.limits_owner.is_none() {
            self.world.limits = self.base_limits();
        }
        info!(
            "REPLAY_LENGTH user={seconds:?} max_frames={} byte_budget={} mod={:?}",
            self.world.limits.max_frames, self.world.limits.byte_budget, self.limits_owner
        );
    }

    /// `sdk.replay.set_limits`: both absent restores the defaults (retail, or the player's Extended Replays).
    pub(crate) fn set_limits(&mut self, owner: &str, max_frames: Option<u32>, byte_budget: Option<u64>) {
        let defaults = self.base_limits();
        if max_frames.is_none() && byte_budget.is_none() {
            if self.limits_owner.as_deref() == Some(owner) {
                self.world.limits = defaults;
                self.limits_owner = None;
            }
            return;
        }
        self.world.limits = skate_core::replay_buffer::ReplayLimits {
            max_frames: max_frames.unwrap_or(defaults.max_frames),
            byte_budget: byte_budget.map_or(defaults.byte_budget, |b| b as usize),
        };
        self.limits_owner = Some(owner.to_string());
    }

    /// A mod stopped: its markers and its limits go.
    pub(crate) fn clear_owner(&mut self, owner: &str) {
        self.markers.retain(|m| m.owner != owner);
        self.set_limits(owner, None, None);
    }

    /// Every mod stopped.
    pub(crate) fn clear_mods(&mut self) {
        self.markers.clear();
        self.world.limits = self.base_limits();
        self.limits_owner = None;
    }

    /// Read-only state for the mod snapshot (`replay_state`).
    pub(crate) fn state(&self) -> serde_json::Value {
        let (start, end) = self.bounds();
        serde_json::json!({
            "active": self.active,
            "cursor": if self.active { self.cursor } else { end },
            "start": start,
            "end": end,
            "frames": self.world.frame_count(),
            "bytes": self.world.bytes(),
            "max_frames": self.world.limits.max_frames,
            "byte_budget": self.world.limits.byte_budget,
            "markers": self.markers.iter()
                .map(|m| serde_json::json!({"time": m.time, "label": m.label, "owner": m.owner}))
                .collect::<Vec<_>>(),
        })
    }

    /// Retail's open check for Back and `sdk.replay.open`: enough recorded (`8250B8F8`).
    pub(crate) fn can_open(&self) -> bool {
        let (start, end) = self.bounds();
        !self.frames.is_empty() && end - start >= self.min_open_seconds.unwrap_or(MIN_OPEN_SECONDS)
    }

    pub(crate) fn enter(&mut self) {
        if self.frames.is_empty() {
            return;
        }
        self.active = true;
        self.playing = false;
        self.free_camera = None;
        self.cursor = self.bounds().1;
        // Logs must show when a reported replay happened and what it held.
        let (start, end) = self.bounds();
        info!(
            "REPLAY_OPEN span={:.1}s frames={} actors={} bytes={}",
            end - start,
            self.world.frame_count(),
            self.world.ids().count(),
            self.world.bytes()
        );
    }

    fn exit(&mut self) {
        if self.active {
            let (start, _) = self.bounds();
            info!("REPLAY_CLOSE at={:.1}s", self.cursor - start);
        }
        self.active = false;
        self.quit_pending = false;
        self.quit_on_release = false;
        self.playing = false;
        self.free_camera = None;
        self.release_controls = true;
    }

    fn seek(&mut self, time: f64) {
        let (start, end) = self.bounds();
        self.cursor = time.clamp(start, end);
    }

    /// One recorded frame back or forward (retail's editor frame step, mode 0xD; the editor binds it).
    #[cfg_attr(not(test), allow(dead_code))]
    fn step(&mut self, direction: i32) {
        self.playing = false;
        let next = if direction > 0 {
            self.frames.iter().find(|f| f.time > self.cursor + 1e-6)
        } else {
            self.frames
                .iter()
                .rev()
                .find(|f| f.time < self.cursor - 1e-6)
        };
        if let Some(next) = next {
            self.seek(next.time);
        }
    }

    pub fn sample(&self) -> Option<(&Snapshot, &Snapshot, f32)> {
        if !self.active {
            return None;
        }
        let upper = self.frames.partition_point(|f| f.time <= self.cursor);
        let a = self.frames.get(upper.saturating_sub(1))?;
        let b = self.frames.get(upper).unwrap_or(a);
        // Teleports, camera cuts and cadence changes remain discrete.
        if b.cut || a.pose.bones.len() != b.pose.bones.len() {
            return Some((&a.pose, &a.pose, 0.0));
        }
        let alpha = if b.time > a.time {
            ((self.cursor - a.time) / (b.time - a.time)).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        Some((&a.pose, &b.pose, alpha))
    }

    fn toggle_camera(&mut self) {
        self.free_camera = if self.free_camera.is_some() {
            None
        } else {
            self.sample().map(|(a, b, t)| blend(a.camera, b.camera, t))
        };
    }
}

pub(crate) struct ReplayPlugin;
impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        world::plugin(app);
        // The world actors' audio state follows the recording (b378 channels A1..A3).
        components::track::<crate::world_audio::TrafficAudio>(app);
        components::track::<crate::world_audio::PedAudio>(app);
        components::track::<crate::world_audio::NpcSkaterAudio>(app);
        components::track::<crate::world_audio::AudioVelocity>(app);
        // The NPC skaters' and peds' fade (spawn fade-in, leave fade, camera-distance fade).
        components::track::<crate::living_world::npc_skaters::NpcFade>(app);
        // Their one-shot sounds (speech stays out: retail blocks speech in replay).
        messages::track::<crate::world_audio::VehicleHorn>(app);
        messages::track::<crate::world_audio::VehicleAlarm>(app);
        messages::track::<crate::world_audio::PedTazerEvent>(app);
        messages::track::<crate::world_audio::PedBodyFallEvent>(app);
        app.init_resource::<Replay>()
            .add_systems(Update, apply_user_length)
            .configure_sets(
                FixedUpdate,
                (
                    SimulationSet::Input,
                    SimulationSet::Controls,
                    SimulationSet::Physics,
                )
                    .run_if(live),
            )
            .add_systems(
                PreUpdate,
                controls
                    .after(crate::input::poll_controllers)
                    .after(bevy::input::InputSystems),
            )
            .add_systems(Startup, setup_ui)
            .init_resource::<ReplayFeed>()
            .init_resource::<RetailReplayUi>()
            .add_systems(Update, (publish_feed, update_ui));
    }
}

fn live(replay: Res<Replay>) -> bool {
    !replay.active
}

/// Run condition for the living world's fixed steps: frozen during a replay, as retail plays recorded data. Without
/// it the population kept stepping: peds walked away from the frozen skater and despawned by distance while the
/// replay was open (desktop session 2026-10-20: 26 PED_DESPAWN lines during one replay, the hit ped among them).
/// Apps without a replay (unit tests) always run.
impl Replay {
    /// The playback cursor (s on the recording's clock).
    pub(crate) fn cursor(&self) -> f64 {
        self.cursor
    }
}

/// The PC overlay's Extended Replays setting into the recorder.
fn apply_user_length(menu: Option<Res<crate::graphics_menu::Menu>>, mut replay: ResMut<Replay>) {
    if let Some(menu) = menu.filter(|m| m.is_changed()) {
        replay.set_user_length(menu.replay_seconds());
    }
}

pub(crate) fn world_live(replay: Option<Res<Replay>>) -> bool {
    replay.is_none_or(|r| !r.active)
}

fn axis(value: f32) -> f32 {
    value.signum() * ((value.abs() - 0.18) / 0.82).clamp(0.0, 1.0)
}

pub(crate) fn controls(
    mut replay: ResMut<Replay>,
    mut input: ResMut<crate::input::ControllerInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    net: Option<Res<crate::multiplayer::Multiplayer>>,
    mut popups: ResMut<crate::popup_host::PopupMods>,
) {
    if net.is_some_and(|n| n.active()) {
        if replay.active {replay.exit();}
        return;
    }
    let pad = input.raw_input();
    let pressed = pad.buttons & !replay.previous_buttons;
    replay.previous_buttons = pad.buttons;
    let menu_open = !crate::graphics_menu::gameplay_active(menu);
    // The quit warning's answer (No / Yes, Yes = 1).
    for (callback, choice) in std::mem::take(&mut popups.engine_choices) {
        if callback == QUIT_POPUP && replay.quit_pending {
            replay.quit_pending = false;
            if choice == 1 {
                replay.exit();
            }
        }
    }
    if !menu_open {
        let back = pressed & SELECT != 0 || keys.just_pressed(KeyCode::F6);
        if !replay.active {
            // Retail: Back during play opens the instant replay (`825EB998`), only with enough recorded
            // (`8250B8F8`); a refusal is silent.
            if back && replay.can_open() {
                replay.enter();
            }
        } else if replay.quit_pending {
            // OURS (safety until play-tested): Back again while the warning is up leaves at once.
            if back {
                replay.quit_pending = false;
                replay.exit();
            }
        } else if back || pressed & B != 0 {
            replay.quit_on_release = true;
        }
        // Retail: B (Finished) or Back opens the Finished menu; with nothing to save or upload that is the quit
        // warning (ID_REPLAY_EDITOR_QUIT, No / Yes; research b387 / b389). Raised once the button is up: the press
        // that asked for it otherwise reached the popup the next frame and cancelled it at once (desktop session
        // 2026-10-20, log: popup 0 shown, choice -1 48 ms later). Without the retail menu stage: leave.
        if replay.quit_on_release && pad.buttons & (B | SELECT) == 0 && !keys.pressed(KeyCode::F6) {
            replay.quit_on_release = false;
            if popups.stage_ready {
                popups.engine_requests.push(quit_popup());
                replay.quit_pending = true;
            } else {
                replay.exit();
            }
        }
    }
    // Continue reading the UI controller while fixed gameplay ticks are gated.
    // On exit, require release so a held scrub/flight control cannot become a
    // grab, push or flick in the preserved live simulation.
    if replay.active || replay.release_controls {
        input.discard_gameplay();
        if !replay.active
            && pad.buttons == 0
            && pad.triggers.iter().all(|v| *v < 0.05)
            && pad.left.iter().chain(&pad.right).all(|v| v.abs() < 0.18)
        {
            replay.release_controls = false;
        }
    }
    if !replay.active || menu_open || replay.quit_pending {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    // OURS (PC only): C toggles a free camera. Retail's instant replay has none (its cameras are the editor's);
    // retail Y cycles the edit channel with the film DLC only, A opens the editor (not built yet).
    if keys.just_pressed(KeyCode::KeyC) {
        replay.toggle_camera();
    }
    // Retail: RB starts / stops playback (`82C7F0A8` stops); stick or D-pad input resumes a stopped replay
    // (`82C7FFB8`, research b386 / b387). Space is the PC key for RB.
    let stick = pad.left.iter().chain(&pad.right).any(|v| v.abs() >= 0.18);
    let dpad = pressed & (DPAD_LEFT | DPAD_RIGHT | 0x0001 | 0x0002) != 0;
    if pressed & RB != 0 || keys.just_pressed(KeyCode::Space) {
        if !replay.playing && replay.cursor >= replay.bounds().1 {
            let start = replay.bounds().0;
            replay.seek(start);
        }
        replay.playing = !replay.playing;
    } else if !replay.playing && replay.free_camera.is_none() && (stick || dpad) && replay.cursor < replay.bounds().1 {
        replay.playing = true;
    }
    // Retail: D-pad Left / Right pressed jump to the start / end (mode 5); comma / period on the keyboard.
    if pressed & DPAD_LEFT != 0 || keys.just_pressed(KeyCode::Comma) {
        let start = replay.bounds().0;
        replay.seek(start);
    }
    if pressed & DPAD_RIGHT != 0 || keys.just_pressed(KeyCode::Period) {
        let end = replay.bounds().1;
        replay.seek(end);
        replay.playing = false;
    }
    let trigger = |v: f32| ((v - 0.05) / 0.95).clamp(0.0, 1.0);
    let scrub = trigger(pad.triggers[1]) - trigger(pad.triggers[0])
        + f32::from(keys.pressed(KeyCode::ArrowRight))
        - f32::from(keys.pressed(KeyCode::ArrowLeft));
    // Retail's scrub: eased, at most 0.005 of the recording per console frame (`ScrubSpec`, research b387 / b388);
    // a moving scrub stops playback, and it eases out after the triggers are released.
    let spec = replay.scrub_spec;
    let moved = replay.scrub.step(&spec, scrub.clamp(-1.0, 1.0), dt);
    if moved != 0.0 {
        replay.playing = false;
        let (start, end) = replay.bounds();
        let cursor = replay.cursor + f64::from(moved) * (end - start);
        replay.seek(cursor);
    } else if replay.playing {
        let cursor = replay.cursor + f64::from(dt);
        replay.seek(cursor);
        if replay.cursor >= replay.bounds().1 {
            replay.playing = false;
        }
    }
    let player = replay.sample().map(|(a, b, t)| blend(a.root, b.root, t).translation);
    let leash = replay.free_camera_leash.unwrap_or(FREE_CAMERA_LEASH);
    if let Some(camera) = replay.free_camera.as_mut() {
        let (mut yaw, mut pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        yaw -= axis(pad.right[0]) * dt * 2.0;
        pitch += axis(pad.right[1]) * dt * 2.0;
        if mouse.pressed(MouseButton::Right) {
            yaw -= motion.delta.x * 0.003;
            pitch -= motion.delta.y * 0.003;
        }
        pitch = pitch.clamp(-1.5, 1.5);
        camera.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        let x = axis(pad.left[0]) + f32::from(keys.pressed(KeyCode::KeyD))
            - f32::from(keys.pressed(KeyCode::KeyA));
        let z = axis(pad.left[1]) + f32::from(keys.pressed(KeyCode::KeyW))
            - f32::from(keys.pressed(KeyCode::KeyS));
        let y = f32::from(pad.buttons & 0x0200 != 0 || keys.pressed(KeyCode::KeyE))
            - f32::from(pad.buttons & 0x0100 != 0 || keys.pressed(KeyCode::KeyQ));
        let heading = Quat::from_rotation_y(yaw);
        let speed = if pad.buttons & 0x0040 != 0 || keys.pressed(KeyCode::ShiftLeft) {
            12.0
        } else {
            3.0
        };
        camera.translation += (heading * Vec3::new(x, 0.0, -z) + Vec3::Y * y) * dt * speed;
        if let Some(player) = player {
            camera.translation = leashed(camera.translation, player, leash);
        }
    }
}

/// Retail's replay free camera is pulled back when it is farther than 40 m from the player (`82683680` ->
/// `82682CB0`, research b381).
pub(crate) const FREE_CAMERA_LEASH: f32 = 40.0;

/// `camera` pulled onto the sphere of `radius` around `player` when it is outside it.
fn leashed(camera: Vec3, player: Vec3, radius: f32) -> Vec3 {
    let offset = camera - player;
    if offset.length() > radius { player + offset.normalize_or_zero() * radius } else { camera }
}

#[derive(Component)]
struct ReplayUi;
#[derive(Component)]
struct ReplayLabel;
#[derive(Component)]
struct Playhead;

fn setup_ui(mut commands: Commands) {
    commands.spawn((ReplayUi, GlobalZIndex(5), Node {
        display: Display::None, position_type: PositionType::Absolute,
        bottom: px(24), left: percent(8), width: percent(84),
        padding: UiRect::all(px(16)), flex_direction: FlexDirection::Column,
        row_gap: px(10), ..default()
    }, BackgroundColor(Color::srgba(0.02, 0.035, 0.05, 0.92)))).with_children(|panel| {
        panel.spawn((ReplayLabel, Text::new("REPLAY"), TextFont { font_size: 20., ..default() }));
        panel.spawn((Node { width: percent(100), height: px(6), ..default() },
            BackgroundColor(Color::srgb(0.18, 0.22, 0.26)))).with_children(|bar| {
            bar.spawn((Playhead, Node { width: percent(100), height: percent(100), ..default() },
                BackgroundColor(Color::srgb(0.35, 0.9, 0.7))));
        });
        // Pad hints as the connected pad's glyphs (prompt_glyphs, SDL labels); keyboard hints as text.
        let hint = Color::srgb(0.75, 0.82, 0.86);
        // Retail's instant replay controls (research b386 / b389).
        crate::prompt_glyphs::hint_row(panel, &[(&["L_Trigger", "R_Trigger"], "", "Scrub"), (&["DPad_Left", "DPad_Right"], "", "Start / end"),
            (&["R_Bumper"], "", "Play / stop"), (&["Back", "B"], "", "Finished")], 14., hint);
        panel.spawn((Text::new("Keyboard: F6  Replay / Finished   |   Arrows  Scrub   |   , / .  Start / end   |   Space  Play   |   C  Free camera (WASD / Q / E move, RMB drag look, Shift fast)"),
            TextFont { font_size: 14., ..default() }, TextColor(hint)));
    });
}

/// The replay as the retail replay screen reads it (`replay_menu_host`), published every frame.
#[derive(Resource, Default)]
pub(crate) struct ReplayFeed(pub crate::replay_menu_host::ReplayScreenFeed);

/// The retail replay screen is shown (set by `retail_menu_stage`); our Bevy panel is the fallback without it.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct RetailReplayUi(pub bool);

fn publish_feed(replay: Res<Replay>, mut feed: ResMut<ReplayFeed>) {
    let (start, end) = replay.bounds();
    let next = crate::replay_menu_host::ReplayScreenFeed {
        instant: replay.active,
        position: if end > start { (replay.cursor - start) / (end - start) } else { 1.0 },
        playing: replay.playing,
        panel: true,
        length_seconds: (end - start).max(0.0).round() as u32,
    };
    if feed.0 != next {
        feed.0 = next;
    }
}

fn update_ui(
    replay: Res<Replay>,
    retail: Res<RetailReplayUi>,
    mut roots: Query<&mut Node, With<ReplayUi>>,
    mut labels: Query<&mut Text, With<ReplayLabel>>,
    mut bars: Query<&mut Node, (With<Playhead>, Without<ReplayUi>)>,
) {
    for mut node in &mut roots {
        node.display = if replay.active && !retail.0 {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !replay.active {
        return;
    }
    let (start, end) = replay.bounds();
    for mut text in &mut labels {
        **text = format!(
            "REPLAY  {:.2} / {:.2} s   |   {}   |   {}",
            replay.cursor - start,
            end - start,
            if replay.playing { "PLAYING" } else { "PAUSED" },
            if replay.free_camera.is_some() {
                "FREE CAMERA"
            } else {
                "RECORDED CAMERA"
            }
        );
    }
    let fraction = if end > start {
        (replay.cursor - start) / (end - start)
    } else {
        1.0
    };
    for mut node in &mut bars {
        node.width = percent(fraction as f32 * 100.0);
    }
}

mod components;
mod messages;
mod world;
pub(crate) use world::retire;

#[cfg(test)]
mod tests;
