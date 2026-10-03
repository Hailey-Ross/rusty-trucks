//! Host for the native AEMS runtime (`crates/skate-audio`): retail's own patch programs and voice
//! graph instead of our measured tables. Off by default; `SKATE_AEMS=1` or `"native": true` in
//! `settings/audio.json` turns it on (docs/hails-additions/11-audio.md, "Native AEMS runtime").
//!
//! - At startup every Csis project is installed and `emitter_utility.abk` is loaded and posted
//!   (retail posts `c_emitter_utility` once at boot; it feeds the `*_snd` / `random_*_gbl` globals).
//! - Banks are loaded on first use, with their decoded WAVs as PCM, and unloaded on map change.
//! - One device-paced stream: rodio pulls 48 kHz stereo; each 256-frame block renders the voices
//!   and ticks the evaluator, so programs run on the audio clock exactly like retail. Game code
//!   posts, redelivers and releases between blocks under the runtime's lock.
//! - The stream follows the master volume (the AEMS voices × ambience, the rolling bed × effects)
//!   and pauses while the menu or a replay runs.
//! - The MixMap mixer (`MixMapSK8.mxb`, `skate_audio::mixmap`) runs on the game thread once per
//!   60 Hz frame (fixed steps): [`mixmap_frame`] writes the inputs we can supply (category gains,
//!   pause, the local player's physics and 3-D position, the emitter states' positions), ticks,
//!   and the systems read its outputs (the `c_emitter` words; the rolling bed's levels, pitch,
//!   filters and pan).
//!
//! Which systems use it so far: the `.ems` world emitters (`emitters.rs`) and, when the install
//! has the whole grain recordings, the granular rolling bed (`grain_bed.rs`). Location sets, zone
//! beds, crossfades and the other skate cues still use their measured tables.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::{AddAudioSource, AudioPlayer, Decodable, PlaybackSettings, Source, Volume};
use bevy::prelude::*;
use skate_audio::eval::NodeId;
use skate_audio::formats::Project;
use skate_audio::mixmap::{MixMap, keys};
use skate_audio::runtime::Runtime;

use super::Library;

pub(crate) mod prefetch;

/// The host's step: inputs, the components' process and update run per 60 Hz physics step; the
/// MixMap evaluates on every second one (the console's 30 Hz, `skate_audio::mixmap::cadence`), or
/// on each with `SKATE_AEMS_MIX_CONSOLE=0`.
const MIX_STEP: f32 = 1.0 / 60.0;
/// CSTATEMGR_Emitter's pool = the MixMap's Emitter instances.
pub(crate) const EMITTER_STATES: usize = 5;

/// The runtime, shared with the audio thread.
pub(crate) type Shared = Arc<Mutex<Runtime>>;

/// The asset rodio plays: an endless 48 kHz stereo stream pulled from the runtime.
#[derive(Asset, TypePath, Clone)]
pub(crate) struct NativeStream {
    shared: Shared,
}

pub(crate) struct NativeDecoder {
    shared: Shared,
    buffer: Vec<f32>,
    at: usize,
}

impl Iterator for NativeDecoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.at >= self.buffer.len() {
            match super::timing::lock(&self.shared, &super::timing::AUDIO_LOCK) {
                Ok(mut runtime) => {
                    let _render = super::timing::scope(&super::timing::RENDER);
                    runtime.fill_stereo(&mut self.buffer);
                }
                Err(_) => self.buffer.fill(0.0),
            }
            self.at = 0;
        }
        let v = self.buffer[self.at];
        self.at += 1;
        Some(v)
    }
}

impl Source for NativeDecoder {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        skate_audio::MIX_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for NativeStream {
    type DecoderItem = f32;
    type Decoder = NativeDecoder;
    fn decoder(&self) -> NativeDecoder {
        // One block of stereo per lock.
        NativeDecoder { shared: self.shared.clone(), buffer: vec![0.0; 2 * skate_audio::BLOCK], at: usize::MAX }
    }
}

/// Marks the stream's player entity.
#[derive(Component)]
struct NativeOutput;

/// The running native runtime and what the game has loaded into it.
#[derive(Resource)]
pub(crate) struct Native {
    pub(crate) shared: Shared,
    /// Bank stem → runtime bank id.
    banks: HashMap<String, usize>,
    emitter_class: Option<usize>,
    /// The MixMap, when the install has `MixMapSK8.mxb`.
    pub(crate) mixmap: Option<MixMap>,
    mix_clock: f32,
    /// MixMap frames run (the eEQChain clear runs on every second one; old 60 Hz cadence only).
    mix_frames: u64,
    /// The console's 30 Hz evaluation grid over the 60 Hz steps ([`mix_console_requested`]).
    cadence: skate_audio::mixmap::cadence::Cadence,
    /// The flag inputs are held between evaluations (set once under the console cadence).
    holds: bool,
    /// The local player's MixMap inputs and components (None without a MixMap).
    pub(crate) player: Option<super::player_audio::PlayerAudio>,
    /// Which emitter states (MixMap Emitter instances 0..4) are taken.
    emitter_states: [bool; EMITTER_STATES],
    /// The granular rolling bed's game-side state (None: the interim rolling loop plays).
    pub(crate) bed: Option<super::grain_bed::Bed>,
    /// World emitter banks read and decoded ahead of need on a worker thread ([`prefetch`]).
    pub(crate) prefetch: prefetch::Prefetch,
}

impl Native {
    pub(super) fn start(library: &Library) -> Result<Self, String> {
        let files = library.aems();
        if files.projects.is_empty() {
            return Err("this install has no AEMS banks (run setup to refresh the audio)".into());
        }
        let mut runtime = Runtime::new();
        for file in &files.projects {
            let bytes = library.read(file).map_err(|e| format!("{file}: {e}"))?;
            let project = Project::parse(file, &bytes).map_err(|e| e.to_string())?;
            runtime.install_project(&project);
        }
        let mixmap = match &files.mixmap {
            Some(file) => {
                let bytes = library.read(file).map_err(|e| format!("{file}: {e}"))?;
                let m = MixMap::from_bytes(&bytes).map_err(|e| e.to_string())?;
                info!("Game audio: MixMap {} controllers ({} output blocks)", m.controller_count(), m.output_blocks());
                Some(m)
            }
            None => {
                warn!("Game audio: this install has no MixMapSK8.mxb (run setup to refresh the audio); emitter words use defaults");
                None
            }
        };
        let bed = if mixmap.is_some() { super::grain_bed::Bed::new(library) } else { None };
        let player = mixmap.as_ref().map(|_| {
            super::player_audio::PlayerAudio::new(library.player_tuning(), super::player_audio::components_requested())
        });
        if bed.is_none() {
            info!("Game audio: no MixMap, whole grain recordings or grain tuning in this install; the interim rolling loop plays");
        }
        let mut native = Self {
            shared: Arc::new(Mutex::new(runtime)),
            banks: HashMap::new(),
            emitter_class: None,
            mixmap,
            mix_clock: 0.0,
            mix_frames: 0,
            cadence: Default::default(),
            holds: false,
            player,
            emitter_states: [false; EMITTER_STATES],
            bed,
            prefetch: Default::default(),
        };
        // The environment (reverb) network and the eEQChain buses (optional install data).
        let (presets, eq) = library.bus_tuning();
        if !presets.is_empty() || !eq.is_empty() {
            let mut runtime = native.shared.lock().map_err(|_| "audio lock poisoned")?;
            info!("Game audio: native buses on ({} reverb presets, {} eEQChain buses)", presets.len(), eq.len());
            runtime.mixer.buses.env.presets = presets;
            runtime.mixer.buses.eq.set_records(&eq);
        }
        // The FlangeSub effect returns (GRINDS and the skid send into return A).
        if let Some([a, b]) = library.flange_presets() {
            let mut runtime = native.shared.lock().map_err(|_| "audio lock poisoned")?;
            info!("Game audio: native FlangeSub returns on");
            runtime.mixer.buses.flange.set_presets(a, b);
        }
        // The FootStep SubMix graphs (`sub_82494188`: each foot sound through its EQ record, env
        // send and panner); `SKATE_AEMS_FOOTSTEP_SUBMIX=0` = the voices straight into SFX Master.
        if let Ok(mut runtime) = native.shared.lock() {
            runtime.mixer.buses.submix.enabled = !std::env::var("SKATE_AEMS_FOOTSTEP_SUBMIX").is_ok_and(|v| v == "0");
        }
        native.ensure_bank(library, "emitter_utility")?;
        native.load_player_banks(library);
        let mut runtime = native.shared.lock().map_err(|_| "audio lock poisoned")?;
        let utility = runtime.eval.class_id("c_emitter_utility").ok_or("no c_emitter_utility class")?;
        runtime.post(utility, &[]);
        drop(runtime);
        // Retail's second boot utility (Start_up_Play_ctl, Common.abk): the Seams program's sample
        // shuffles. Older installs without Common.abk keep the fixed seam samples.
        match native.ensure_bank(library, skate_audio::player::seams::UTILITY_BANK) {
            Ok(_) => {
                let mut runtime = native.shared.lock().map_err(|_| "audio lock poisoned")?;
                if let Some(class) = runtime.eval.class_id(skate_audio::player::seams::UTILITY) {
                    runtime.post(class, &[]);
                }
            }
            Err(e) => warn!("Game audio: {e}; seam hits play fixed samples (rerun setup)"),
        }
        // Then the foley utility (the cloth_trick programs call it): retail's boot order
        // c_emitter_utility → Start_up_Play_ctl → c_foley_utility, which matters because every
        // program shares one random generator.
        if native.player.as_ref().is_some_and(|p| p.components && p.tricks_on) {
            let mut runtime = native.shared.lock().map_err(|_| "audio lock poisoned")?;
            if let Some(id) = runtime.eval.class_id(skate_audio::player::tricks::FOLEY_UTILITY) {
                runtime.post(id, &[]);
            }
        }
        let runtime = native.shared.lock().map_err(|_| "audio lock poisoned")?;
        native.emitter_class = runtime.eval.class_id("c_emitter");
        drop(runtime);
        Ok(native)
    }

    /// Load a bank (and its samples) unless it is loaded already. A bank the prefetch worker has
    /// read and decoded is taken from it (waiting if it is mid-decode); otherwise it is read and
    /// decoded here, by the same `BankSource::load`. Either way `load_bank` runs now.
    pub(crate) fn ensure_bank(&mut self, library: &Library, stem: &str) -> Result<usize, String> {
        if let Some(&id) = self.banks.get(stem) {
            return Ok(id);
        }
        let (bank, pcm) = match self.prefetch.take(stem) {
            Some(loaded) => loaded,
            None => library.bank_source(stem)?.load()?,
        };
        let id = self.shared.lock().map_err(|_| "audio lock poisoned")?.load_bank(bank, pcm);
        self.banks.insert(stem.to_owned(), id);
        Ok(id)
    }

    /// The player components' banks, in the player volume group; without them the components
    /// stay off and their interim cues play.
    fn load_player_banks(&mut self, library: &Library) {
        if !self.player.as_ref().is_some_and(|p| p.components) {
            return;
        }
        for stem in super::player_audio::BANKS {
            match self.ensure_bank(library, stem) {
                Ok(id) => {
                    if let Ok(mut runtime) = self.shared.lock() {
                        runtime.mixer.set_bank_group(id, skate_audio::mixer::GROUP_PLAYER);
                    }
                }
                Err(e) => {
                    warn!("Game audio: native player sounds off ({e}); the measured cues play");
                    if let Some(p) = &mut self.player {
                        p.components = false;
                    }
                    return;
                }
            }
        }
        info!("Game audio: native player components on (Class_grind, SenseOfSpeed, Class_foot_drag)");
        self.load_optional_player_banks(library);
        // The Splice banks (pops, landings, touchdowns), decoded up front so a sample's first
        // trigger sounds like every later one.
        let mut first = false;
        for (i, stem) in super::player_audio::SPLICE_BANKS.iter().enumerate() {
            match library.splice_bank(stem) {
                Some((bank, pcm)) => {
                    if let Ok(mut runtime) = self.shared.lock() {
                        let rt = &mut *runtime;
                        rt.splice.load_bank(stem, bank, pcm, &mut rt.mixer);
                        first |= i == 0;
                    }
                }
                None => warn!("Game audio: {stem} has no patch tree in this install (run setup to refresh the audio); its native sounds stay off"),
            }
        }
        // SFXObj_Wheels' spin-down recordings, decoded now (first trigger = later triggers).
        let streams: Vec<_> = super::player_audio::WHEEL_STREAMS.iter().map(|n| library.wheels_pcm(n)).collect();
        let wheels = streams.iter().all(Option::is_some);
        if let Ok(mut runtime) = self.shared.lock() {
            runtime.load_streams(streams);
        }
        if let Some(p) = &mut self.player {
            p.contacts_on = first;
            p.set_footstep_materials(library.footstep_materials());
            p.footsteps_on = first && !std::env::var("SKATE_AEMS_FOOTSTEPS").is_ok_and(|v| v == "0");
            p.contact_tuning = library.contacts_tuning();
            p.wheels_on = wheels;
            if p.contacts_on {
                info!("Game audio: native board contacts on (Splice: pops, landings, touchdowns, foot taps, scuffs; collision pairs: {} materials)", p.tuning.collision.materials.len());
            }
            if wheels {
                info!("Game audio: native wheel spin on (SFXObj_Wheels)");
            }
        }
    }

    /// The optional components' banks (rolling layers, rattle, board slide, tricks, treatment):
    /// each component runs when its first bank is in the install; missing later banks only drop
    /// their layers (a Class_rolling post reaches every bank bound to the class).
    fn load_optional_player_banks(&mut self, library: &Library) {
        use super::player_audio::{RATTLE_BANKS, ROLLING_BANKS, SLIDE_BANKS, TREATMENT_BANKS, TRICKS_BANKS};
        let load = |me: &mut Self, banks: &[&str]| -> bool {
            let mut first = false;
            for (i, stem) in banks.iter().enumerate() {
                match me.ensure_bank(library, stem) {
                    Ok(id) => {
                        if let Ok(mut runtime) = me.shared.lock() {
                            runtime.mixer.set_bank_group(id, skate_audio::mixer::GROUP_PLAYER);
                        }
                        first |= i == 0;
                    }
                    Err(e) => warn!("Game audio: {e}; its native layers stay off (run setup to refresh the audio)"),
                }
            }
            first
        };
        let rolling = load(self, ROLLING_BANKS);
        let rattle = load(self, RATTLE_BANKS);
        let slide = load(self, SLIDE_BANKS);
        let tricks = load(self, TRICKS_BANKS);
        let treatment = load(self, TREATMENT_BANKS);
        // With the tricks the foley utility is posted at boot, after Start_up_Play_ctl (`start`).
        if let Some(p) = &mut self.player {
            (p.rolling_on, p.rattle_on, p.slide_on, p.tricks_on, p.treatment_on) = (rolling, rattle, slide, tricks, treatment);
            info!("Game audio: native rolling layers {rolling}, rattle {rattle}, board slide {slide}, tricks {tricks}, treatment {treatment}");
        }
    }

    /// Whether the runtime holds the bank.
    pub(crate) fn bank_loaded(&self, stem: &str) -> bool {
        self.banks.contains_key(stem)
    }

    /// Unload every bank but the utility and the player's (map change); forget the prefetched ones.
    pub(crate) fn unload_map_banks(&mut self) {
        self.prefetch.clear();
        let Ok(mut runtime) = self.shared.lock() else { return };
        self.banks.retain(|stem, id| {
            let keep = stem == "emitter_utility" || stem == skate_audio::player::seams::UTILITY_BANK || super::player_audio::BANKS.contains(&stem.as_str()) || super::player_audio::OPTIONAL_BANKS.iter().any(|b| b.contains(&stem.as_str()));
            if !keep {
                runtime.unload_bank(*id);
            }
            keep
        });
    }

    pub(crate) fn has_bank(&self, library: &Library, stem: &str) -> bool {
        library.aems().banks.contains_key(stem)
    }

    /// Post to `c_emitter` (payload words: level, dry, send, azimuth, pitch, low-pass, high-pass,
    /// unused, selector).
    pub(crate) fn post_emitter(&self, payload: &[i32; 9]) -> Option<NodeId> {
        let class = self.emitter_class?;
        Some(self.shared.lock().ok()?.post(class, payload))
    }

    /// Take a free emitter state (0..4), if any.
    pub(crate) fn claim_emitter_state(&mut self) -> Option<usize> {
        let g = self.emitter_states.iter().position(|used| !used)?;
        self.emitter_states[g] = true;
        Some(g)
    }

    /// Free an emitter state; its 3-D input goes inactive.
    pub(crate) fn release_emitter_state(&mut self, g: usize) {
        if let Some(used) = self.emitter_states.get_mut(g) {
            *used = false;
        }
        if let Some(m) = &mut self.mixmap {
            m.set_input(keys::emitter_pos(g as u32), keys::pos::FLAGS, 0);
        }
    }

    /// The emitter state's SFXCTL 3-D input (what B0 reads: camera distance and azimuth). Who
    /// writes it in retail is not traced (mixmap-spec §10); we follow the 3DObjPos layout (§7.3).
    pub(crate) fn set_emitter_position(&mut self, g: usize, listener: &GlobalTransform, skater: Vec3, source: Vec3) {
        let Some(m) = &mut self.mixmap else { return };
        write_position(m, keys::emitter_pos(g as u32), listener, skater, source);
    }

    /// The positional `c_emitter` payload from the MixMap (mixmap-spec §6.3, `sub_824DCF08`):
    /// w1 dry = out4 × level, w2 send = out8 × level, w3 pan = out0 (raw azimuth of B0), w4 pitch
    /// = out5 through the pitch reader, w5 low-pass = out6; w8 = the attribute patch. Without a
    /// MixMap (or a state) the old defaults apply: dry = level, send 0, pitch 4096, 25 kHz.
    pub(crate) fn emitter_payload(&self, g: Option<usize>, level: f32, azimuth: i32, patch: i32) -> [i32; 9] {
        let level = level.clamp(0.0, 1.0);
        let patch = patch.clamp(0, 500);
        match (&self.mixmap, g) {
            (Some(m), Some(g)) => {
                let key = keys::emitter(g as u32);
                let scaled = |id: usize| (m.level(key, id) as f32 * level) as i32;
                [32767, scaled(4), scaled(8), m.raw(key, 0), m.pitch_4096(key, 5), m.filter_hz(key, 6), 0, 0, patch]
            }
            _ => [32767, (level * 32767.0).round() as i32, 0, azimuth, 4096, 25000, 0, 0, patch],
        }
    }

    pub(crate) fn redeliver(&self, node: NodeId, payload: &[i32]) {
        if let Ok(mut runtime) = self.shared.lock() {
            runtime.redeliver(node, payload);
        }
    }

    pub(crate) fn release(&self, node: NodeId) {
        if let Ok(mut runtime) = self.shared.lock() {
            runtime.release(node);
        }
    }
}

/// Whether to run the native runtime: on by default; `SKATE_AEMS=0` (or `"interim": true` in
/// `settings/audio.json`) keeps the interim tables.
pub(crate) fn requested(settings: &super::AudioSettings) -> bool {
    match std::env::var("SKATE_AEMS") {
        Ok(v) => !(v == "0" || v.eq_ignore_ascii_case("false")),
        Err(_) => settings.native(),
    }
}

pub(crate) fn register(app: &mut App) {
    app.add_audio_source::<NativeStream>()
        .add_systems(PostStartup, start)
        .add_systems(PostUpdate, follow_volume);
}

/// Write an emitter state's 3DObjPos-style input block (mixmap-spec §7.3): in0 = f32 distance
/// from the skater, in1 = f32 distance from the camera itself, in2 / in3 = azimuths (u16 scale,
/// both in the camera frame here: who writes the emitter blocks in retail is not traced, §10),
/// in15 bit 0 = active. The emitters are static: relative speeds (in13/14) stay 0.
fn write_position(m: &mut MixMap, key: u32, listener: &GlobalTransform, skater: Vec3, source: Vec3) {
    let az = azimuth(listener, source);
    m.set_input_f32(key, keys::pos::DIST_SKATER, skater.distance(source));
    m.set_input_f32(key, keys::pos::DIST_CAMERA, listener.translation().distance(source));
    m.set_input(key, keys::pos::AZ_SKATER, az);
    m.set_input(key, keys::pos::AZ_CAMERA, az);
    m.set_input(key, keys::pos::FLAGS, 1);
}

/// The inputs we can supply for this frame, then the fixed-step MixMap ticks (spec §1: inputs →
/// components' process → tick → components' update). Written here: Master.in1–4, Music.in1/2/5,
/// Reverb.in5 (free-skate values, §7.4) and Pause.in0 while the menu is open; through
/// `player_audio`: PlayerPhysics 0–14, the two 3DObjPos blocks (skater COM and board, with
/// relative speeds and the sign-flip bits), Jitter, Contacts 1/2/6, Rail 0/1, OffBoard 0; the
/// board owner inputs (`grain_bed.rs`). Left at their free-skate 0: VU (no output meter; only the
/// ambience reads it, not native yet), Menu / NIS / HOM / Challenge / Speech flags (no such modes
/// in free skate), HandGrabs, Music.in3/6 (combo emphasis, not wired).
/// `SKATE_AEMS_REVERB_ZONES=0`: the reverb preset from the region layer only.
fn reverb_zones_requested() -> bool {
    !std::env::var("SKATE_AEMS_REVERB_ZONES").is_ok_and(|v| v == "0")
}

/// `SKATE_AEMS_REVERB_INPUTS=0` keeps the fixed Reverb.in5 = 32767 (no env scale).
fn reverb_inputs_requested() -> bool {
    !std::env::var("SKATE_AEMS_REVERB_INPUTS").is_ok_and(|v| v == "0")
}

/// `SKATE_AEMS_MIX_CONSOLE=0` turns off the MixMap's console cadence: the old evaluation per 60 Hz
/// step with dt 1/60, the Jitter stepped and the eEQChain cleared per step / every second step.
pub(crate) fn mix_console_requested() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("SKATE_AEMS_MIX_CONSOLE").is_ok_and(|v| v == "0"))
}

/// The one-frame flag inputs the console's writers set for a whole evaluation, held between our 60
/// Hz writes and the 30 Hz evaluations ([`MixMap::hold_input`]): Contacts 1 / 6 (the landing
/// swell and the landing-material flag), Rail 1 (the grind ended), SkateBoard 0 / 4 (the surface
/// change, the push plant), Cracks 0 (a seam hit). Everything else the host writes is a level that
/// holds between writes.
pub(crate) fn hold_flag_inputs(m: &mut MixMap) {
    for (key, id) in [(keys::contacts(0), 1), (keys::contacts(0), 6), (keys::rail(0), 1), (keys::skateboard(0), 0), (keys::skateboard(0), 4), (keys::cracks(0), 0)] {
        m.hold_input(key, id);
    }
}

/// `SKATE_AEMS_SEAM_PULSE=0` turns off Class_Seams' console cadence (Listening test 9).
fn seam_pulse_requested() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("SKATE_AEMS_SEAM_PULSE").is_ok_and(|v| v == "0"))
}

pub(super) fn mixmap_frame(
    native: Option<ResMut<Native>>,
    cues: Res<super::skate_events::Cues>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    time: Res<Time<Real>>,
    fixed: Res<Time<Fixed>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
) {
    let _timing = super::timing::scope(&super::timing::MIXMAP_FRAME);
    let Some(mut native) = native else { return };
    let native = &mut *native;
    let Some(m) = &mut native.mixmap else { return };
    for id in 1..=4 {
        m.set_input(keys::MASTER, id, 32767);
    }
    for id in [1, 2, 5] {
        m.set_input(keys::MUSIC, id, 32767);
    }
    // SFXObj_Reverb's first step (`sub_824DF468`): Reverb.in0..6 from the number of the preset
    // being faded to (in5 = reverb11/12/15/16/22, which ducks the global env scale out4 by 4 dB).
    // `SKATE_AEMS_REVERB_INPUTS=0`: the old fixed in5 = 32767 and no scale.
    let reverb = reverb_inputs_requested().then(|| super::timing::lock(&native.shared, &super::timing::GAME_LOCK).ok().map(|r| r.mixer.buses.env.reverb_inputs())).flatten();
    match reverb {
        Some(v) => {
            for (id, x) in v.into_iter().enumerate() {
                m.set_input(keys::REVERB, id, x);
            }
        }
        None => m.set_input(keys::REVERB, 5, 32767),
    }
    let silenced = super::silenced(menu.as_deref(), &replay);
    m.set_input(keys::PAUSE, 0, if silenced { 32767 } else { 0 });

    // Inputs, components and the tick run only on frames that tick (retail: once per 60 Hz frame),
    // so one-frame pulses (landing, surface change, push) always reach an evaluation.
    native.mix_clock = (native.mix_clock + time.delta_secs()).min(4.0 * MIX_STEP);
    let ticks = (native.mix_clock / MIX_STEP) as usize;
    // Class_Seams on the console's 30 fps process cadence at the rendered board's wheels, on every
    // rendered frame (Listening test 9). `SKATE_AEMS_SEAM_PULSE=0`: its whole process per 60 Hz tick
    // at the physics positions, as before.
    if let Some(player) = &mut native.player {
        let console = seam_pulse_requested();
        player.seam_console = console;
        player.seam_alpha = console.then(|| fixed.overstep_fraction());
        if console {
            if let Ok(mut runtime) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK) {
                player.seam_frame(m, &cues.riding.audio, time.delta_secs(), &mut runtime);
            }
        }
    }
    if ticks == 0 {
        return;
    }
    native.mix_clock -= ticks as f32 * MIX_STEP;
    // The console cadence (`skate_audio::mixmap::cadence`): retail's audio manager evaluates the
    // MixMap, steps the Jitter and clears the eEQChain buses once per 1/30 s console frame with dt
    // 1/30; here every second 60 Hz step, the flag inputs held in between. Off: per 60 Hz step.
    let console = mix_console_requested();
    let calls = if console { native.cadence.advance(ticks) } else { 0 };
    if console && !native.holds {
        hold_flag_inputs(m);
        native.holds = true;
    }
    // sub_82491180 in half 1 of the audio manager: every console frame (both halves run when the
    // frame is longer than 20 ms); the old cadence: every second 60 Hz frame. The jittered eEQChain
    // buses take the walk's values and every bus may re-roll again.
    native.mix_frames += 1;
    if if console { calls > 0 } else { native.mix_frames % 2 == 0 || ticks > 1 } {
        let jitter = native.player.as_ref().and_then(|p| p.eq_jitter());
        if let Ok(mut runtime) = native.shared.lock() {
            runtime.mixer.buses.eq.clear(jitter);
        }
    }
    let s = cues.riding.audio;
    if let Some(player) = &mut native.player {
        let dt = ticks as f32 * MIX_STEP;
        let l = listener.single().ok().map(|t| player.listener(t.translation().to_array(), t.forward().as_vec3().to_array(), dt, &s));
        // SFXObj_Jitter's walk steps once per console evaluation (half 1's process).
        player.jitter_steps = console.then_some(calls);
        player.write_inputs(m, &s, l.as_ref());
    }
    // With the native rolling layers the owner's surface routing (player::rolling) writes
    // SkateBoard inputs 0 / 6 and drives the bed's binds; otherwise the bed routes itself.
    let routing = native.player.as_ref().is_some_and(|p| p.components && p.rolling_on);
    if let Some(bed) = &mut native.bed {
        bed.write_inputs(m, &s, !routing);
    }
    let speed_scale = native.bed.as_ref().and_then(|b| b.push_scale());
    let loose = super::player_audio::PlayerAudio::loose_board(&s, &cues.riding);
    if let (Some(player), Ok(mut runtime)) = (&mut native.player, super::timing::lock(&native.shared, &super::timing::GAME_LOCK)) {
        player.process(m, &s, &mut runtime, speed_scale, loose);
    }
    if console {
        for _ in 0..calls {
            m.tick(skate_audio::mixmap::cadence::CONSOLE_DT);
        }
    } else {
        for _ in 0..ticks {
            m.tick(MIX_STEP);
        }
    }
    if let (Some(player), Ok(mut runtime)) = (&mut native.player, super::timing::lock(&native.shared, &super::timing::GAME_LOCK)) {
        player.update(m, &s, &mut runtime, speed_scale, loose);
    }
    // SFXObj_Reverb's update (`sub_824DF220`): the FlangeSub returns' levels and the global env
    // scale (manager +104 = out4) from the Reverb owner.
    if let Ok(mut runtime) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK) {
        runtime.mixer.buses.flange.frame(std::array::from_fn(|i| m.level(keys::REVERB, i)));
        if reverb.is_some() {
            runtime.mixer.buses.env.scale_frame(m.level(keys::REVERB, 4));
        }
    }
}

fn start(
    mut commands: Commands,
    settings: Option<Res<super::AudioSettings>>,
    library: Option<Res<Library>>,
    mut streams: ResMut<Assets<NativeStream>>,
) {
    let (Some(settings), Some(library)) = (settings, library) else { return };
    if !requested(&settings) {
        return;
    }
    // Which compiled copy of the DSP loops runs (hardware FMA or plain; same output bits): chosen
    // once, here, before the first render (doc 11 "Hardware FMA dispatch").
    info!("AUDIO_DSP {}", skate_audio::dsp::init_fma());
    match Native::start(&library) {
        Ok(native) => {
            info!("Game audio: native AEMS runtime on ({} projects)", library.aems().projects.len());
            let handle = streams.add(NativeStream { shared: native.shared.clone() });
            // ONCE, not LOOP: the stream never ends, and Bevy's LOOP wraps it in rodio's
            // `repeat_infinite`, i.e. `Buffered`, which renders 32768 samples (64 blocks, 341 ms)
            // at a time inside the device callback and keeps every chunk forever: a lock burst of
            // 64 renders every 341 ms (game-thread stalls = the 19:01 stutter), up to 341 ms of
            // event-to-sound latency and ~23 MB/min of memory growth.
            commands.spawn((NativeOutput, AudioPlayer(handle), PlaybackSettings::ONCE.with_volume(Volume::Linear(0.0))));
            commands.insert_resource(native);
        }
        Err(error) => warn!("Game audio: native AEMS runtime unavailable, using the measured tables: {error}"),
    }
}

/// Master volume on the stream; inside it the AEMS voices (world emitters: ambience) and the
/// rolling bed (effects) take their category volumes. Pause while silenced.
fn follow_volume(
    settings: Option<Res<super::AudioSettings>>,
    native: Option<Res<Native>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
    mut sinks: Query<&mut AudioSink, With<NativeOutput>>,
    voices: Option<ResMut<super::Voices>>,
    mut listeners: Query<&mut SpatialListener, With<super::GameAudioListener>>,
) {
    let Some(settings) = settings else { return };
    // rodio 0.20's spatial source gives the ear FARTHER from the source the larger factor
    // (((d_left - d_right) / gap + 1) / 4 + 0.5 on the left channel), so with the ears where Bevy
    // puts them every interim positional sound is mirrored. With the native host on, swap the
    // ears so interim sounds come from the side native voices (Pan2D1) put them on.
    for mut listener in &mut listeners {
        let gap = listener.left_ear_offset.distance(listener.right_ear_offset);
        let want = if native.is_some() { Vec3::X * gap / 2.0 } else { Vec3::X * gap / -2.0 };
        if listener.left_ear_offset != want {
            listener.left_ear_offset = want;
            listener.right_ear_offset = -want;
        }
    }
    let silenced = super::silenced(menu.as_deref(), &replay);
    let volume = settings.master().clamp(0.0, 1.0);
    // The interim cues are measured retail level × RETAIL_SCALE (anchored to the interim rolling
    // loop); with the native player sounds (retail level) they drop back to the measured level.
    let native_player = native.as_deref().is_some_and(|n| n.player.as_ref().is_some_and(|p| p.components));
    if let Some(mut voices) = voices {
        let scale = if native_player { 1.0 / super::cues::RETAIL_SCALE } else { 1.0 };
        if voices.scale != scale {
            voices.scale = scale;
        }
        // Interim and native voices share the ears: fold the interim ones like native voices.
        let fold = native.is_some();
        if voices.native_fold != fold {
            voices.native_fold = fold;
        }
    }
    if let Some(native) = native {
        if let Ok(mut runtime) = native.shared.lock() {
            runtime.aems_gain = settings.category(super::Category::Ambience).clamp(0.0, 1.0);
            runtime.player_gain = settings.category(super::Category::Effects).clamp(0.0, 1.0);
            runtime.grains.gain = settings.category(super::Category::Effects).clamp(0.0, 1.0);
        }
    }
    for mut sink in &mut sinks {
        sink.set_volume(Volume::Linear(volume));
        if silenced && !sink.is_paused() {
            sink.pause();
        } else if !silenced && sink.is_paused() {
            sink.play();
        }
    }
}

/// The reverb preset (`SFXObj_Reverb`, `sub_824DE548`): the district's `audio_reverb` region at the
/// skater's x, z (reverb01 when none), crossfaded over 1 s of game time; per frame.
pub(super) fn reverb_frame(
    native: Option<Res<Native>>,
    library: Option<Res<Library>>,
    map: Res<crate::map_transition::CurrentMap>,
    cues: Res<super::skate_events::Cues>,
    time: Res<Time<Real>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    zones: Res<super::emitters::ReverbZones>,
) {
    let (Some(native), Some(library)) = (native, library) else { return };
    let district = map.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("");
    let at = cues.riding.board;
    let key = library.region_key(district, "audio_reverb", at.x, at.z).unwrap_or(skate_audio::bus::env::DEFAULT_PRESET);
    let camera = listener.single().ok().map(|t| skate_audio::bus::zones::Camera {
        position: t.translation().to_array(),
        forward: t.forward().as_vec3().to_array(),
    });
    if let Ok(mut runtime) = native.shared.lock() {
        let env = &mut runtime.mixer.buses.env;
        if env.enabled() {
            // SFXObj_Reverb's update (`sub_824DE548`) in retail's order, with the reverb-zone
            // emitters the listener is in (`emitters::reverb_zones`; `SKATE_AEMS_REVERB_ZONES=0`: none).
            let zones = if reverb_zones_requested() { &zones.zones[..] } else { &[] };
            env.update(time.delta_secs().clamp(0.0, 0.25), key, zones, camera.as_ref());
        }
    }
}

/// The `c_emitter` azimuth word for a source seen from the listener: 0 = straight ahead,
/// increasing clockwise (to the right), 65536 = 360°.
pub(crate) fn azimuth(listener: &GlobalTransform, source: Vec3) -> i32 {
    let local = listener.affine().inverse().transform_point3(source);
    // Bevy cameras look down −Z with +X to the right.
    let degrees = local.x.atan2(-local.z).to_degrees();
    ((degrees / 360.0 * 65536.0).round() as i32).rem_euclid(65536)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azimuth_is_clockwise_from_the_view_direction() {
        let listener = GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0));
        assert_eq!(azimuth(&listener, Vec3::new(0.0, 0.0, -5.0)), 0);
        assert_eq!(azimuth(&listener, Vec3::new(5.0, 0.0, 0.0)), 16384);
        assert_eq!(azimuth(&listener, Vec3::new(-5.0, 0.0, 0.0)), 49152);
        assert_eq!(azimuth(&listener, Vec3::new(0.0, 0.0, 5.0)), 32768);
        // Turned to face +X, a source at +X is ahead.
        let turned = GlobalTransform::from(Transform::from_xyz(1.0, 0.0, 0.0).looking_at(Vec3::new(10.0, 0.0, 0.0), Vec3::Y));
        assert_eq!(azimuth(&turned, Vec3::new(10.0, 0.0, 0.0)), 0);
    }

    fn native(mixmap: Option<MixMap>) -> Native {
        Native {
            shared: Arc::new(Mutex::new(Runtime::new())),
            banks: HashMap::new(),
            emitter_class: None,
            mixmap,
            mix_clock: 0.0,
            mix_frames: 0,
            cadence: Default::default(),
            holds: false,
            player: None,
            emitter_states: [false; EMITTER_STATES],
            bed: None,
            prefetch: Default::default(),
        }
    }

    #[test]
    fn emitter_words_without_a_mixmap_keep_the_old_defaults() {
        let n = native(None);
        assert_eq!(n.emitter_payload(Some(0), 0.5, 16384, 81), [32767, 16384, 0, 16384, 4096, 25000, 0, 0, 81]);
        assert_eq!(n.emitter_payload(None, 3.0, 0, 900)[1], 32767);
        assert_eq!(n.emitter_payload(None, -1.0, 0, -4)[..2], [32767, 0]);
        assert_eq!(n.emitter_payload(None, 0.0, 0, -4)[8], 0);
    }

    #[test]
    fn emitter_states_are_the_five_mixmap_instances() {
        let mut n = native(None);
        let got: Vec<_> = (0..6).map(|_| n.claim_emitter_state()).collect();
        assert_eq!(got, vec![Some(0), Some(1), Some(2), Some(3), Some(4), None]);
        n.release_emitter_state(2);
        assert_eq!(n.claim_emitter_state(), Some(2));
    }

    /// With the retail MixMap (when the private install has it): a positional emitter's dry word is
    /// out4 (−600 mB + the Global ducks' rest) × level, the pan is the B0 azimuth we wrote, the
    /// send rolls off with camera distance (4 → 70 m) and the filter stays open.
    #[test]
    fn emitter_words_from_the_retail_mixmap() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/private/audio/aems/MixMapSK8.mxb");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipped: no {path}");
            return;
        };
        let mut n = native(Some(MixMap::from_bytes(&bytes).unwrap()));
        let g = n.claim_emitter_state().unwrap();
        let listener = GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0));
        let source = Vec3::new(10.0, 0.0, 0.0);
        n.set_emitter_position(g, &listener, Vec3::ZERO, source);
        let m = n.mixmap.as_mut().unwrap();
        for id in 1..=4 {
            m.set_input(keys::MASTER, id, 32767);
        }
        for _ in 0..3 {
            m.tick(MIX_STEP);
        }
        let near = n.emitter_payload(Some(g), 1.0, 0, 81);
        assert_eq!(near[1], 16365);
        assert_eq!(near[3], 16384, "pan = the azimuth written");
        assert_eq!((near[4], near[5], near[8]), (4096, 24971, 81));
        assert!(near[2] > 0);
        assert_eq!(n.emitter_payload(Some(g), 0.5, 0, 81)[1], 16365 / 2);
        n.set_emitter_position(g, &listener, Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0));
        n.mixmap.as_mut().unwrap().tick(MIX_STEP);
        let far = n.emitter_payload(Some(g), 1.0, 0, 81);
        assert_eq!(far[2], 0, "beyond 70 m the send is silent");
        assert_eq!(far[1], near[1], "the dry level has no distance roll-off");
    }

    /// The stutter of the 19:01 build: a looped (`repeat_infinite`) stream renders 64 blocks on
    /// its first pull; played once it renders one block per 256 frames.
    #[test]
    fn the_stream_renders_one_block_per_pull_unless_looped() {
        let looped = NativeStream { shared: Arc::new(Mutex::new(Runtime::new())) };
        let mut source = looped.decoder().repeat_infinite();
        source.next();
        assert_eq!(looped.shared.lock().unwrap().blocks, 64, "Buffered pulls 32768 samples");
        let once = NativeStream { shared: Arc::new(Mutex::new(Runtime::new())) };
        let mut source = once.decoder();
        source.next();
        assert_eq!(once.shared.lock().unwrap().blocks, 1);
    }

    #[test]
    fn the_stream_pulls_blocks_from_the_runtime() {
        let stream = NativeStream { shared: Arc::new(Mutex::new(Runtime::new())) };
        let mut decoder = stream.decoder();
        let samples: Vec<f32> = (&mut decoder).take(4 * skate_audio::BLOCK).collect();
        assert!(samples.iter().all(|&s| s == 0.0));
        assert_eq!(stream.shared.lock().unwrap().blocks, 2);
        assert_eq!((decoder.channels(), decoder.sample_rate()), (2, 48000));
    }

    /// Native is the default: an install that lacks some of its data must still start (or fall
    /// back to the measured tables with a warning), never panic. Each case loads a copy of the
    /// dev install's manifest with parts removed (the data folders are linked, not copied).
    /// Data-gated: skipped without the install.
    #[test]
    fn missing_install_parts_fall_back_without_breaking() {
        let real = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/private/audio"));
        let Ok(text) = std::fs::read_to_string(real.join("audio_manifest.json")) else { return eprintln!("skipped: no audio install") };
        let full: serde_json::Value = serde_json::from_str(&text).unwrap();
        if full["aems"]["projects"].as_array().is_none_or(|p| p.is_empty()) {
            return eprintln!("skipped: the install has no AEMS banks");
        }
        let dir = std::env::temp_dir().join(format!("skate-audio-fallback-{}", std::process::id()));
        let audio = dir.join("private/audio");
        std::fs::create_dir_all(&audio).unwrap();
        let mut links = Vec::new();
        for sub in ["aems", "banks", "grains", "wheels", "ambience"] {
            let (link, target) = (audio.join(sub), real.join(sub));
            if !target.is_dir() {
                continue;
            }
            // mklink does not take the `\\?\` form canonicalize returns.
            let target = target.canonicalize().unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_owned();
            let out = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link.to_string_lossy().replace('/', "\\")).arg(&target).output();
            if !out.as_ref().is_ok_and(|o| o.status.success()) {
                let _ = std::fs::remove_dir_all(&dir);
                let why = out.map_or_else(|e| e.to_string(), |o| String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr));
                return eprintln!("skipped: could not link {sub} ({})", why.trim());
            }
            links.push(link);
        }
        let start = |name: &str, edit: &dyn Fn(&mut serde_json::Value)| -> Result<Native, String> {
            let mut m = full.clone();
            edit(&mut m);
            std::fs::write(audio.join("audio_manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
            let library = super::super::Library::load(&dir).unwrap_or_else(|e| panic!("{name}: {e}"));
            let r = Native::start(&library);
            println!("{name}: {}", match &r {
                Ok(n) => format!("native on, player {:?}", n.player.as_ref().map(|p| (p.components, p.tricks_on, p.treatment_on, p.contacts_on, p.footsteps_on))),
                Err(e) => format!("fallback to the tables: {e}"),
            });
            r
        };
        let remove_bank = |stem: &'static str| move |m: &mut serde_json::Value| {
            m["aems"]["banks"].as_object_mut().unwrap().remove(stem);
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let n = start("full install", &|_| {}).expect("the full install starts");
            let p = n.player.as_ref().expect("player components with a MixMap");
            assert!(p.components && p.tricks_on && p.treatment_on);
            assert!(start("no AEMS projects", &|m| m["aems"]["projects"] = serde_json::json!([])).is_err());
            assert!(start("no emitter_utility bank", &remove_bank("emitter_utility")).is_err());
            let n = start("no MixMap", &|m| m["aems"]["mixmap"] = serde_json::Value::Null).expect("starts without a MixMap");
            assert!(n.player.is_none() && n.bed.is_none());
            let n = start("no GRINDS bank", &remove_bank("GRINDS")).expect("starts without the player banks");
            assert!(!n.player.as_ref().unwrap().components, "the components hand back to the measured cues");
            let n = start("no Treatments bank", &remove_bank("Treatments")).expect("starts without Treatments");
            let p = n.player.as_ref().unwrap();
            assert!(p.components && p.tricks_on && !p.treatment_on);
            let n = start("Treatments listed, file missing", &|m| m["aems"]["banks"]["Treatments"] = serde_json::json!("aems/missing/Treatments.abk"))
                .expect("starts with a missing bank file");
            assert!(!n.player.as_ref().unwrap().treatment_on);
            start("no Common bank (seam utility)", &remove_bank(skate_audio::player::seams::UTILITY_BANK)).expect("starts without Common");
            start("no Splice trees", &|m| m["aems"]["splice"] = serde_json::json!({})).expect("starts without the Splice trees");
            start("no bus tuning", &|m| {
                m.as_object_mut().unwrap().remove("bus_tuning");
            })
            .expect("starts without the bus tuning");
        }));
        for link in &links {
            let _ = std::fs::remove_dir(link);
        }
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }
}
