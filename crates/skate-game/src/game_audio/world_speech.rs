//! Living-world speech playback on the native runtime: the peds' PedestrianSpeech requests
//! (`world_sources`) and the NPC skaters' bail grunts (`npc_skaters`) go through the speech
//! manager (`skate_audio::world::speech_manager`: value → event, the vault tuning gate, the
//! request words), the speech library (`speech_rules`: the `.evt` record and its takes) and the
//! living world's two streams (`speech_player`: interrupts, the queue, the cut), and play as
//! stream voices whose level, reverb send, pitch, pan and filters follow the speaker's MixMap
//! owner every console frame: a ped's PedestrianSpeech instance, a skater's PlayerSpeech
//! instance (spec `audio-specs/world-audio-hookin-spec.md`, doc 15 "Speech").
//!
//! **Data.** The index and rules (`speech/livingworld.json`) are a normal setup export; the takes
//! are the opt-in decode (`SKATE_SETUP_SPEECH=1`, ~2.4 GB). Without the index or the decode the
//! speech stays silent and says so once in the log (`AUDIO_WORLD speech off: …`); requests are
//! still gated and logged. A take is read from disk when its line starts (as retail streams it)
//! and dropped after use.
//!
//! **Inert** while nothing requests a line and nothing plays.
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use serde::Deserialize;
use skate_audio::formats::SampleHeader;
use skate_audio::mixer::Mixer;
use skate_audio::player::objpos::{Listener, ObjPos};
use skate_audio::world::keys;
use skate_audio::world::owners::Pool;
use skate_audio::world::peds::SpeechRequest;
use skate_audio::world::speech::{Clip, Line, SPEECH_BANK, SpeechIndex, SpeechSlots, Take};
use skate_audio::world::speech_manager::{GateInputs, Speaker, SpeechManager, kind};
use skate_audio::world::speech_player::{self, Event, Outcome, PedLevelSelect, Request, SpeechPlayer, SpeechVoices, VoiceParams};
use skate_audio::world::speech_rules::{ClipHeader, ClipRef, EventTable, Library as SpeechLibrary, Record};
use skate_audio::world::traffic::OutputsSnapshot;
use skate_audio::world::Lcg;

use super::native::Native;

/// The NPC bail grunt's event (`201_grunt`, message 8206 / 115 of `sub_824BF5F8`).
pub(crate) const BAIL_GRUNT_EVENT: u16 = 8206;
/// Decoded takes kept loaded after their line (the rest are read again when picked).
const KEEP_TAKES: usize = 24;

/// A ped's request with what the speech host needs of the ped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PedRequest {
    pub(crate) request: SpeechRequest,
    pub(crate) voice: u32,
    pub(crate) speaker: Speaker,
    pub(crate) level: PedLevelSelect,
}

/// A published NPC skater with a speech voice (its PlayerSpeech position).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SkaterSpeaker {
    pub(crate) id: u64,
    pub(crate) voice: u32,
    pub(crate) position: [f32; 3],
    pub(crate) velocity: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Who {
    Ped(PedLevelSelect),
    Skater(u32),
}

/// The loaded speech export.
struct Data {
    index: SpeechIndex,
    table: EventTable,
    library: SpeechLibrary,
    slots: SpeechSlots,
    audio: Option<PathBuf>,
}

#[derive(Resource)]
pub(crate) struct WorldSpeech {
    /// This frame's requests (the hosts append).
    pub(crate) peds: Vec<PedRequest>,
    pub(crate) grunts: Vec<u64>,
    /// The published NPC skaters with a voice (the NPC host rewrites it).
    pub(crate) skaters: Vec<SkaterSpeaker>,
    tried: bool,
    data: Option<Data>,
    manager: SpeechManager,
    player: SpeechPlayer,
    rng: Lcg,
    /// The manager clock (s): console time since the runtime started (`MixMap::ticks`).
    clock: f64,
    last_tick: u64,
    who: HashMap<u64, Who>,
    /// PlayerSpeech instances 1.. for speaking skaters (0 = the local player's record).
    skater_slots: Pool,
    skater_pos: HashMap<u64, ObjPos>,
    loaded: VecDeque<u16>,
    missing_logged: bool,
    epoch: Option<u64>,
    last_camera: Option<([f32; 3], u64)>,
    /// Lines started (the summary log).
    pub(crate) lines: u64,
}

impl Default for WorldSpeech {
    fn default() -> Self {
        Self {
            peds: Vec::new(),
            grunts: Vec::new(),
            skaters: Vec::new(),
            tried: false,
            data: None,
            manager: SpeechManager::default(),
            player: SpeechPlayer::default(),
            rng: Lcg(0x5EEC),
            clock: 0.0,
            last_tick: 0,
            who: HashMap::new(),
            skater_slots: Pool::new(keys::PLAYER_SPEECH_INSTANCES - 1),
            skater_pos: HashMap::new(),
            loaded: VecDeque::new(),
            missing_logged: false,
            epoch: None,
            last_camera: None,
            lines: 0,
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<WorldSpeech>()
        .add_systems(Update, frame.after(super::world_sources::frame).after(super::npc_skaters::frame));
}

// ---- the export (speech/livingworld.json) ----

#[derive(Deserialize)]
struct IndexJson {
    clips: Vec<ClipJson>,
    rules: RulesJson,
}

#[derive(Deserialize)]
struct ClipJson {
    name: String,
    #[serde(default)]
    id: Option<u16>,
    #[serde(default)]
    history: u8,
    takes: Vec<TakeJson>,
}

#[derive(Deserialize)]
struct TakeJson {
    offset: u32,
    size: u32,
    rate: u32,
    samples: u32,
}

#[derive(Deserialize)]
struct RulesJson {
    bank: u8,
    sub_bank: u8,
    events: Vec<EventJson>,
}

#[derive(Deserialize)]
struct EventJson {
    id: u16,
    name: String,
    queue_timeout: u16,
    priority: u16,
    conditions: u8,
    flags: u8,
    probability: u8,
    flags2: u8,
    fields: Vec<u8>,
    records: Vec<RecordJson>,
}

#[derive(Deserialize)]
struct RecordJson {
    weight: u8,
    probability: u8,
    mode: u8,
    locals: u8,
    values: Vec<u32>,
    clips: Vec<u16>,
}

fn load(index_path: &std::path::Path, audio: Option<PathBuf>) -> Result<Data, String> {
    let text = std::fs::read_to_string(index_path).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let json: IndexJson = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let mut clips = Vec::with_capacity(json.clips.len());
    let mut ids = Vec::with_capacity(json.clips.len());
    let mut headers = Vec::new();
    for c in json.clips {
        let Some((event, voice, voice_name, line)) = skate_audio::world::speech::parse_name(&c.name) else { continue };
        if let Some(id) = c.id {
            headers.push(ClipHeader { id, takes: c.takes.len().min(255) as u8, history: c.history, flags: 0 });
        }
        ids.push(c.id);
        let takes = c.takes.iter().map(|t| Take { offset: t.offset, size: t.size, rate: t.rate, samples: t.samples }).collect();
        clips.push(Clip { name: c.name, event, voice, voice_name, line, takes });
    }
    let mut index = SpeechIndex::new(clips);
    index.set_ids(&ids);
    let events = json
        .rules
        .events
        .into_iter()
        .map(|e| skate_audio::world::speech_rules::Event {
            id: e.id,
            name: e.name,
            queue_timeout: e.queue_timeout,
            priority: e.priority,
            conditions: e.conditions,
            flags: e.flags,
            probability: e.probability,
            flags2: e.flags2,
            fields: e.fields,
            records: e
                .records
                .into_iter()
                .map(|r| Record {
                    weight_code: r.weight,
                    probability: r.probability,
                    mode: r.mode,
                    locals: r.locals,
                    values: r.values,
                    clips: r.clips.into_iter().map(|id| ClipRef { id, lookup: 0, params: 0 }).collect(),
                })
                .collect(),
        })
        .collect();
    let table = EventTable { bank: json.rules.bank, sub_bank: json.rules.sub_bank, events };
    let slots = SpeechSlots::new(&index);
    Ok(Data { index, table, library: SpeechLibrary::new(headers), slots, audio })
}

// ---- the stream voices ----

struct Voices<'a> {
    mixer: &'a mut Mixer,
    data: &'a Data,
    loaded: &'a mut VecDeque<u16>,
    missing: &'a mut bool,
}

impl Voices<'_> {
    fn ensure(&mut self, line: Line) -> Option<u16> {
        let slot = self.data.slots.slot(line)?;
        if self.loaded.contains(&slot) {
            return Some(slot);
        }
        let audio = self.data.audio.as_ref()?;
        let clip = self.data.index.clips.get(line.clip)?;
        let stem = clip.name.strip_suffix(".dat").unwrap_or(&clip.name);
        let path = audio.join(stem).join(format!("{:02}.wav", line.take));
        let pcm = match std::fs::read(&path).ok().and_then(|b| super::library::wav_pcm(&b)) {
            Some(p) => p,
            None => {
                if !*self.missing {
                    *self.missing = true;
                    warn!("AUDIO_WORLD speech: {} is not decoded (the speech decode covers the free-roam events: SKATE_SETUP_SPEECH=1)", path.display());
                }
                return None;
            }
        };
        let frames = pcm.channels.first().map_or(0, Vec::len) as u32;
        let header = SampleHeader { codec: 0, channels: pcm.channels.len().clamp(1, 8) as u8, rate: pcm.rate, frames, loop_start: None };
        self.mixer.set_bank_sample(SPEECH_BANK, slot, header, Arc::new(pcm));
        self.loaded.push_back(slot);
        while self.loaded.len() > KEEP_TAKES {
            if let Some(old) = self.loaded.pop_front() {
                // A playing voice keeps its own reference to the PCM.
                self.mixer.clear_bank_sample(SPEECH_BANK, old);
            }
        }
        Some(slot)
    }
}

impl SpeechVoices for Voices<'_> {
    fn open(&mut self, line: Line, p: &VoiceParams) -> Option<u32> {
        let slot = self.ensure(line)?;
        let v = self.mixer.open_direct(SPEECH_BANK, slot, 0.0, p.pitch, p.gain, Some(p.azimuth))?;
        self.mixer.set_direct_dsp(v, p.hpf, p.lpf, p.send);
        Some(v)
    }
    fn set(&mut self, voice: u32, p: &VoiceParams) {
        self.mixer.set_direct(voice, p.pitch, p.gain, Some(p.azimuth));
        self.mixer.set_direct_dsp(voice, p.hpf, p.lpf, p.send);
    }
    fn alive(&self, voice: u32) -> bool {
        self.mixer.direct_alive(voice)
    }
    fn stop(&mut self, voice: u32) {
        skate_audio::eval::VoiceHost::release(self.mixer, voice);
    }
}

/// The speaker words of a skater's voice (the `aud_characteristics` model of the AI skaters):
/// type / variant bits from the export, else from the speech rules.
fn skater_speaker(library: &super::Library, data: &Data, voice: u32) -> Speaker {
    let tuning = library.world_tuning();
    match tuning.ped_model(voice) {
        Some(m) => Speaker { index: voice, kind: m.kind, variant: m.variant, partner: 0, word5: 0, word6: m.gender },
        None => {
            let (kind, variant) = skate_audio::world::speech_manager::speaker_bits(&data.table, &data.index, voice).unwrap_or((kind::SKATER_MALE, 1));
            Speaker { index: voice, kind, variant, partner: 0, word5: 0, word6: 0 }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    native: Option<ResMut<Native>>,
    mut speech: ResMut<WorldSpeech>,
    mut held: ResMut<super::world_sources::WorldHeld>,
    library: Option<Res<super::Library>>,
    cues: Res<super::skate_events::Cues>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
) {
    let speech = &mut *speech;
    if speech.peds.is_empty() && speech.grunts.is_empty() && speech.player.busy() == 0 && speech.player.queued() == 0 && speech.skater_slots.holders().next().is_none() {
        return;
    }
    let (Some(mut native), Some(library)) = (native, library) else {
        speech.peds.clear();
        speech.grunts.clear();
        return;
    };
    let camera = listener.single().ok().map(|t| (t.translation().to_array(), t.forward().as_vec3().to_array()));
    run(speech, &held.peds, &mut native, &library, camera, &cues.riding.audio);
    if held.speech_lines != speech.lines {
        held.speech_lines = speech.lines;
    }
}

/// One frame of the speech host (see the module docs).
pub(crate) fn run(speech: &mut WorldSpeech, peds: &[(u64, u32)], native: &mut Native, library: &super::Library, camera: Option<([f32; 3], [f32; 3])>, local: &skate_audio::player::AudioState) {
    // A map change unloaded nothing of ours, but the speakers are gone: stop and forget.
    if speech.epoch != Some(native.map_epoch) {
        speech.epoch = Some(native.map_epoch);
        if let Ok(mut rt) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK)
            && let Some(data) = speech.data.as_ref()
        {
            let mut missing = speech.missing_logged;
            let mut v = Voices { mixer: &mut rt.mixer, data, loaded: &mut speech.loaded, missing: &mut missing };
            speech.player.clear(&mut v);
            speech.missing_logged = missing;
        }
        speech.who.clear();
        speech.skater_slots.clear();
        speech.skater_pos.clear();
        speech.last_camera = None;
    }
    if !speech.tried {
        speech.tried = true;
        speech.manager = SpeechManager::new(library.world_tuning().speech_tuning());
        match library.speech("livingworld") {
            None => info!("AUDIO_WORLD speech off: the install has no speech index (rerun setup)"),
            Some((index, audio)) => match load(&index, audio.clone()) {
                Ok(data) => {
                    if audio.is_none() {
                        info!("AUDIO_WORLD speech: lines are chosen and logged but silent: the takes are not decoded (setup with SKATE_SETUP_SPEECH=1)");
                    } else {
                        info!("AUDIO_WORLD speech on: {} clips, {} events", data.index.clips.len(), data.table.events.len());
                    }
                    speech.data = Some(data);
                }
                Err(e) => warn!("AUDIO_WORLD speech off: {e}"),
            },
        }
    }
    let Some(data) = speech.data.as_mut() else {
        speech.peds.clear();
        speech.grunts.clear();
        return;
    };
    let Native { mixmap, shared, cuts, .. } = native;
    let Some(m) = mixmap.as_mut() else { return };
    let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) else { return };
    let rt = &mut *runtime;
    // The manager clock: console time since the runtime started (every MixMap evaluation), so the
    // per-speaker timers run while nothing speaks, as retail's (they start at 0 at boot).
    speech.clock = m.ticks as f64 * f64::from(super::world_sources::evaluation_dt());
    let inputs = GateInputs { now: speech.clock, player_speed: local.ground_speed.abs(), ..Default::default() };
    let mut missing = speech.missing_logged;
    // The requests.
    let mut new: Vec<(u64, Who, Result<skate_audio::world::speech_manager::Line, skate_audio::world::speech_manager::Refusal>, i32)> = Vec::new();
    for r in std::mem::take(&mut speech.peds) {
        if r.voice == 0 {
            continue;
        }
        let res = speech.manager.request(&mut data.library, &data.table, r.request.value, r.request.flag, &r.speaker, &inputs, &mut speech.rng);
        new.push((r.request.owner, Who::Ped(r.level), res, r.request.value));
    }
    let camera_pos = camera.map(|c| c.0);
    for id in std::mem::take(&mut speech.grunts) {
        let Some(sk) = speech.skaters.iter().find(|s| s.id == id).copied() else { continue };
        let words = skater_speaker(library, &*data, sk.voice);
        let far = library.world_tuning().ped_model(sk.voice).map_or(20.0, |m| m.far);
        let distance = camera_pos.map_or(0.0, |c| ((sk.position[0] - c[0]).powi(2) + (sk.position[1] - c[1]).powi(2) + (sk.position[2] - c[2]).powi(2)).sqrt());
        // `sub_824DAC00`: flag 1 when the skater's distance exceeds the model's far threshold.
        let flag = if distance > far { skate_audio::world::speech_manager::FAR } else { skate_audio::world::speech_manager::NEAR };
        let res = speech.manager.request_event(&mut data.library, &data.table, BAIL_GRUNT_EVENT, flag, &words, &inputs, &mut speech.rng);
        new.push((id, Who::Skater(sk.voice), res, -1));
    }
    for (speaker, who, res, value) in new {
        match res {
            Ok(line) => {
                let Some(lines) = data.index.picks_to_lines(&line.picks, u32::from(line.event)) else { continue };
                let tuning = speech.manager.tuning.get(&line.event).cloned().unwrap_or_default();
                let timeout = data.table.event(line.event).map_or(60, |e| u32::from(e.queue_timeout));
                let names: Vec<&str> = lines.iter().filter_map(|l| data.index.clips.get(l.clip).map(|c| c.name.as_str())).collect();
                let req = Request { speaker, event: line.event, priority: tuning.priority, interrupt: tuning.interrupt, interrupt_when_full: tuning.interrupt_when_full, lines, timeout };
                let mut v = Voices { mixer: &mut rt.mixer, data: &*data, loaded: &mut speech.loaded, missing: &mut missing };
                let outcome = speech.player.request(req, &mut v);
                info!("AUDIO_WORLD speech owner={speaker} value={value} event={} line={} -> {outcome:?}", line.event, names.join("+"));
                #[cfg(test)]
                eprintln!("AUDIO_WORLD speech owner={speaker} value={value} event={} line={} -> {outcome:?}", line.event, names.join("+"));
                if !matches!(outcome, Outcome::Dropped) {
                    speech.who.insert(speaker, who);
                }
            }
            Err(refusal) => {
                #[cfg(test)]
                eprintln!("AUDIO_WORLD speech owner={speaker} value={value} refused: {refusal:?}");
                debug!("AUDIO_WORLD speech owner={speaker} value={value} refused: {refusal:?}");
            }
        }
    }
    // Per console evaluation: the skaters' PlayerSpeech instances and positions, then the streams.
    if m.ticks == speech.last_tick {
        return;
    }
    let evaluations = m.ticks.saturating_sub(speech.last_tick).max(1);
    speech.last_tick = m.ticks;
    let dt = super::world_sources::evaluation_dt() * evaluations.min(4) as f32;
    let skater_ids: Vec<(u64, f32)> = speech.who.iter().filter(|(_, w)| matches!(w, Who::Skater(_))).map(|(id, _)| (*id, 0.0)).collect();
    let assignment = speech.skater_slots.assign(&skater_ids);
    let Some((cam, view)) = camera else { return };
    let cam_velocity = speech.last_camera.filter(|l| l.1 == *cuts).map_or([0.0; 3], |(last, _)| std::array::from_fn(|i| (cam[i] - last[i]) / dt));
    speech.last_camera = Some((cam, *cuts));
    let l = Listener { camera: cam, view, camera_velocity: cam_velocity, followed: local.com_position, facing: local.com_velocity, followed_velocity: local.com_velocity };
    for (id, g) in assignment.released {
        if let Some(mut pos) = speech.skater_pos.remove(&id) {
            pos.write(m, keys::player_speech_pos(g as u32 + 1), &l, None);
        }
    }
    for (g, id) in speech.skater_slots.holders().collect::<Vec<_>>() {
        let point = speech.skaters.iter().find(|s| s.id == id).map(|s| (s.position, s.velocity));
        speech.skater_pos.entry(id).or_default().write(m, keys::player_speech_pos(g as u32 + 1), &l, point);
    }
    let who = &speech.who;
    let skater_slots = &speech.skater_slots;
    let m_ref: &skate_audio::mixmap::MixMap = m;
    let mut params = |speaker: u64, far: bool, event: u16| -> Option<VoiceParams> {
        match *who.get(&speaker)? {
            Who::Ped(sel) => {
                let g = peds.iter().find(|(o, _)| *o == speaker)?.1;
                let out = OutputsSnapshot::take(m_ref, keys::ped_speech(g), &speech_player::PED_FILTERS);
                Some(speech_player::ped_outputs(&out, sel, event, far))
            }
            Who::Skater(voice) => {
                let g = skater_slots.instance(speaker)? as u32 + 1;
                let out = OutputsSnapshot::take(m_ref, keys::player_speech(g), &speech_player::SKATER_FILTERS);
                Some(speech_player::skater_outputs(&out, voice, far))
            }
        }
    };
    let data = &*data;
    let mut v = Voices { mixer: &mut rt.mixer, data, loaded: &mut speech.loaded, missing: &mut missing };
    let events = speech.player.frame(&data.index, &mut params, &mut v);
    for e in &events {
        match *e {
            Event::Started { speaker, line, .. } => {
                speech.lines += 1;
                debug!("AUDIO_WORLD speech start owner={speaker} {} take {}", data.index.clips.get(line.clip).map_or("?", |c| c.name.as_str()), line.take);
            }
            Event::Cut { speaker, .. } => debug!("AUDIO_WORLD speech cut owner={speaker} (level at or below 200 for 2 s)"),
            _ => {}
        }
    }
    speech.missing_logged = missing;
    // Forget speakers that no longer play or wait.
    let active: Vec<u64> = speech.player.speakers().collect();
    let waiting = speech.player.queued() > 0;
    if !waiting {
        speech.who.retain(|id, _| active.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_audio::world_sources::{WorldHost, WorldOwners};
    use skate_audio::mixmap::cadence::CONSOLE_DT;
    use skate_audio::world::peds::PedState;

    /// As `native::mixmap_frame`: the category gains.
    fn globals(m: &mut skate_audio::mixmap::MixMap) {
        for id in 1..=4 {
            m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
        }
        for id in [1, 2, 5] {
            m.set_input(skate_audio::mixmap::keys::MUSIC, id, 32767);
        }
        m.set_input(skate_audio::mixmap::keys::REVERB, 5, 32767);
    }

    /// The category gains, then the tick.
    fn tick(m: &mut skate_audio::mixmap::MixMap) {
        globals(m);
        m.tick(CONSOLE_DT);
    }

    /// Ped speech end to end through the install (data-gated): a business man (voice 59) warns
    /// (value 53) 3.6 m from the camera: the manager picks a `501_59_busm1_Warn_n` take, a stream
    /// voice plays it at PedestrianSpeech out2 / 32767 with out15 as its reverb send and out13 / out14
    /// as its filters (the recomp's 24956 / 77 Hz near a speaker); 30 m away the next warn is a
    /// `_f` line at out3. Without the decode the line is chosen and nothing plays.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_ped_warns_through_a_stream_at_its_owner_levels() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = crate::game_audio::Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        if library.speech("livingworld").and_then(|s| s.1).is_none() {
            panic!("missing private data: the speech takes are not decoded (SKATE_SETUP_SPEECH=1)");
        }
        let model = library.world_tuning().ped_model(59).expect("ped models export");
        let mut host = WorldHost::default();
        let mut owners = WorldOwners::default();
        let mut speech = WorldSpeech::default();
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        let ped = 9u64;
        let speaker = Speaker { index: 59, kind: model.kind, variant: model.variant, partner: 0, word5: 0, word6: model.gender };
        let state = |z: f32, value: i32| PedState {
            position: [0.0, 1.5, z],
            speed: 0.0,
            class: 5,
            voice: 59,
            speaker,
            speech_measure: z,
            speech_limit: model.far,
            speech_value: value,
            ..Default::default()
        };
        // Past the warn's repeat (15 s) and not-follow (30 s) times: the timers start at 0.
        native.mixmap.as_mut().unwrap().ticks = 40 * 30;
        let mut seen: Vec<(u32, f32, f32, f32)> = Vec::new();
        let run_frames = |native: &mut Native, host: &mut WorldHost, owners: &mut WorldOwners, speech: &mut WorldSpeech, z: f32, frames: usize, seen: &mut Vec<(u32, f32, f32, f32)>| {
            for f in 0..frames {
                owners.peds.insert(ped, state(z, if f >= 3 { 53 } else { 0 }));
                globals(native.mixmap.as_mut().unwrap());
                crate::game_audio::world_sources::run(host, owners, native, &library, camera, &local);
                speech.peds.append(&mut host.speech_requests);
                let held = host.held().1;
                run(speech, &held, native, &library, camera, &local);
                let bank = SPEECH_BANK;
                let m = native.mixmap.as_ref().unwrap();
                let (out2, out3) = (m.level(keys::ped_speech(0), 2), m.level(keys::ped_speech(0), 3));
                let mut rt = native.shared.lock().unwrap();
                for _ in 0..7 {
                    rt.render_block();
                }
                for v in rt.mixer.snapshot().iter().filter(|v| v.bank == bank) {
                    seen.push((v.id, v.gain, out2 as f32 / 32767.0, out3 as f32 / 32767.0));
                }
            }
        };
        run_frames(&mut native, &mut host, &mut owners, &mut speech, 3.6, 40, &mut seen);
        assert!(speech.lines >= 1, "a line started");
        assert!(!seen.is_empty(), "a speech voice sounded");
        let (_, gain, out2, _) = seen[seen.len() / 2];
        assert!((gain - out2).abs() < 1e-3, "near: the stream follows out2 ({gain} vs {out2})");
        // Far: a new warn (another value first), 30 m away → a `_f` line at out3.
        owners.peds.insert(ped, state(30.0, 0));
        seen.clear();
        // Past the repeat time again.
        native.mixmap.as_mut().unwrap().ticks += 40 * 30;
        run_frames(&mut native, &mut host, &mut owners, &mut speech, 30.0, 40, &mut seen);
        let far_line = speech.data.as_ref().unwrap().index.clips.iter().any(|c| c.name.ends_with("Warn_f.dat"));
        assert!(far_line);
        assert!(!seen.is_empty(), "the far line sounded");
        // The newest voice (the near line may still be playing on the other stream).
        let newest = seen.iter().map(|s| s.0).max().unwrap();
        let far: Vec<_> = seen.iter().filter(|s| s.0 == newest).collect();
        let (_, gain, out2, out3) = *far[far.len() / 2];
        assert!((gain - out3).abs() < 1e-3 && (gain - out2).abs() > 1e-3, "far: the stream follows out3 ({gain} vs out3 {out3}, out2 {out2})");
        eprintln!("speech: {} lines; near / far gains match out2 / out3", speech.lines);
    }

    #[derive(Deserialize)]
    struct LevelRow {
        ms: f64,
        clip: Option<String>,
        geometry: Option<Geometry>,
        first: First,
    }
    #[derive(Deserialize)]
    struct Geometry {
        ped: [f32; 3],
        camera: [f32; 3],
        player: [f32; 3],
    }
    #[derive(Deserialize)]
    struct First {
        gain: Option<f32>,
        send: Option<f32>,
        lpf: Option<f32>,
        hpf: Option<f32>,
    }

    /// Our PedestrianSpeech values against the recomp's speech voices (data-gated: the install's
    /// MixMap and `$SKATE_SPEECH_LEVELS/levels_*.json` from the local tool `speech_levels.py`
    /// on sessions 163809 / 164620 / 180430). Per line with a joined speaker: the ped, the camera
    /// (WPPOS) and the player (PEDSEE's target) at its start drive one ped's 3DObjPos (the camera
    /// looking at the player); the stream values our port derives from the outputs (`_f` clips
    /// out3, else out2; send out15; filters out13 / out14) are compared with the recomp's first
    /// GAIN / SEND / LPF / HPF targets of the voice. Prints every row and the agreement.
    #[test]
    #[ignore = "needs the private install data and the recomp level export"]
    fn speech_levels_follow_the_recomp() {
        let base = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let Ok(bytes) = std::fs::read(base.join("assets/private/audio/aems/MixMapSK8.mxb")) else { panic!("missing private data: no MixMap") };
        let mut rows = Vec::new();
        let levels = std::env::var_os("SKATE_SPEECH_LEVELS").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        for s in ["163809", "164620", "180430"] {
            let Some(Ok(text)) = levels.as_ref().map(|d| std::fs::read_to_string(d.join(format!("levels_{s}.json")))) else { continue };
            let list: Vec<LevelRow> = serde_json::from_str(&text).unwrap();
            rows.extend(list.into_iter().filter(|r| r.geometry.is_some() && r.first.gain.is_some()).map(|r| (s, r)));
        }
        if rows.is_empty() {
            panic!("missing private data: no speech level export (SKATE_SPEECH_LEVELS, speech_levels.py --json)");
        }
        let (mut filt_ok, mut filt_n, mut send_ok, mut send_n) = (0, 0, 0, 0);
        let mut ratios = Vec::new();
        for (session, r) in &rows {
            let g = r.geometry.as_ref().unwrap();
            let mut m = skate_audio::mixmap::MixMap::from_bytes(&bytes).unwrap();
            let view = {
                let d = [g.player[0] - g.camera[0], 0.0, g.player[2] - g.camera[2]];
                let n = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-3);
                [d[0] / n, 0.0, d[2] / n]
            };
            let l = Listener { camera: g.camera, view, camera_velocity: [0.0; 3], followed: g.player, facing: view, followed_velocity: [0.0; 3] };
            let mut pos = ObjPos::default();
            for _ in 0..40 {
                pos.write(&mut m, keys::ped_pos(0), &l, Some((g.ped, [0.0; 3])));
                tick(&mut m);
            }
            let far = r.clip.as_deref().is_some_and(speech_player::far_clip);
            let out = OutputsSnapshot::take(&m, keys::ped_speech(0), &speech_player::PED_FILTERS);
            let p = speech_player::ped_outputs(&out, PedLevelSelect::default(), 0, far);
            let gain = r.first.gain.unwrap();
            if p.gain > 1e-3 && gain > 1e-3 {
                ratios.push(gain / p.gain);
            }
            if let (Some(lpf), Some(hpf)) = (r.first.lpf, r.first.hpf) {
                filt_n += 1;
                filt_ok += usize::from((lpf - p.lpf).abs() <= 0.1 * lpf.max(1.0) && (hpf - p.hpf).abs() <= 0.1 * hpf.max(10.0));
            }
            if let Some(send) = r.first.send {
                send_n += 1;
                send_ok += usize::from((send - p.send).abs() <= 0.01);
            }
            let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            eprintln!(
                "{session} {:7.1}s cam {:5.1} m skater {:5.1} m {:>32}: gain recomp {gain:.3} ours {:.3} | send {:?} {:.3} | lpf {:?} {:.0} hpf {:?} {:.0}",
                r.ms / 1000.0,
                d(g.ped, g.camera),
                d(g.ped, g.player),
                r.clip.as_deref().unwrap_or("?"),
                p.gain,
                r.first.send,
                p.send,
                r.first.lpf,
                p.lpf,
                r.first.hpf,
                p.hpf
            );
        }
        ratios.sort_by(f32::total_cmp);
        let median = ratios.get(ratios.len() / 2).copied().unwrap_or(0.0);
        eprintln!("{} lines: filters within 10 % in {filt_ok} of {filt_n}, send within 0.01 in {send_ok} of {send_n}, gain recomp / ours median {median:.3} (p10 {:.3} p90 {:.3}, n {})", rows.len(), ratios.get(ratios.len() / 10).copied().unwrap_or(0.0), ratios.get(ratios.len() * 9 / 10).copied().unwrap_or(0.0), ratios.len());
        assert!(filt_n == 0 || filt_ok * 10 >= filt_n * 6, "the filters follow out13 / out14");
    }
}
