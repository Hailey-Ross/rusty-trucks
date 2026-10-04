//! The speech streams of one speech channel (the living world's): the lines the speech manager
//! starts ([`super::speech_manager`]), played as stream voices whose level, send, pitch, pan and
//! filters follow the speaker's MixMap owner every console frame. Read from the TU3 recompilation
//! (reference only; addresses are facts):
//!
//! - **Owner → stream values.** The speaker's speech owner (`SFXObj_PedestrianSpeech` update
//!   `sub_824D9370`; a skater's `SFXObj_PlayerSpeech` update `sub_824DA300`) copies its outputs into
//!   a parameter block the line's stream voice reads (`sub_824A84B8` finds it by the speaker's id
//!   when the line starts): the main level, a second level, the azimuth (raw 0), the pitch (1) and
//!   two filters. Which level depends on the speaker and on the clip: a clip whose name ends in
//!   `_f` (`sub_824A89E8`, the string `"_f"` at `0x8224EC48`) takes the next output (the far
//!   variant). For a regular ped that is PedestrianSpeech out2 / out3 with out15 as the second;
//!   the filters are out13 (high pass) and out14 (low pass) ([`ped_outputs`]). For a skater with
//!   a living-world voice (model ≥ 41) PlayerSpeech out2 / out3, second out10, filters out8 / out9
//!   ([`skater_outputs`]).
//! - **The recomp agrees** (sessions 163809 / 164620 / 180430, the local tool `speech_levels.py`,
//!   skate-game test `speech_levels_follow_the_recomp`, 39 lines rebuilt at their geometry): a
//!   speech stream's LPF / HPF sit at exactly 24956 / 77 Hz near the speaker and 3489 / 379 Hz far
//!   away, our out14 / out13; its first GAIN over ours has median 1.005 (p10 0.55, p90 1.43); `_f`
//!   lines at 30 / 40 m play at 0.092 / 0.044 against our out3's 0.085 / 0.056, where out2 has fallen
//!   to 0.030 / 0.001. The SEND matches out15 on some lines (0.035 at 3.6 m, 0.049 at 11 m) but only
//!   10 of 37 overall: open. Note: the level lookups measure the distance to the followed skater
//!   (3DObjPos input 0), the near / far flag the camera distance.
//! - **Two streams per channel** (`sub_824A73F0` indexes the channel's stream records as
//!   `channel × 2 + k`; the recomp's living-world lines play on two stream players). A request
//!   takes a free stream. When none is free, the event's tuning decides (`sub_824A73F0`): `+13`
//!   lets it stop a playing line of lower priority, `+14` the same when the channel is full;
//!   otherwise it waits in the library's 16-request queue until its event's queue timeout
//!   (`.evt` `+2`) runs out. Which of the two streams a request targets (`k`) is not traced: the
//!   lower-priority one is taken (provisional).
//! - **The cut** (`sub_824D9370`): while a line plays, a speaker whose main level stays at or below
//!   200 (vault `6995C510258C9AF6`) for more than 60 console frames (`3D8CD05C962FF399`) has its
//!   line stopped. A speaker that loses its MixMap instance stops its line too (deactivation
//!   `sub_824D92B0` / `sub_824DA110`).
//!
//! Not modelled: the speech voice's PEAK filter (≈3.1–4 kHz, gain 0.21–0.26, Q 3 in the recomp;
//! its writer is not found), the per-voice float `aud_characteristics` `2087A3290483BB4F` (0.8–1.15;
//! the recomp's pitch follows out1 alone), the `Obj:Speech` inputs a playing line sets (their
//! ducks), the "focus speaker" levels (out24 / out16 by a game global) and the event queue
//! timeout's unit (taken as console frames).
use std::collections::VecDeque;

use super::speech::{Line, SpeechIndex};
use crate::player::Outputs;

/// Streams of one channel.
pub const STREAMS: usize = 2;
/// The library's request queue.
pub const QUEUE: usize = 16;
/// The cut: a speaker level at or below this …
pub const CUT_LEVEL: i32 = 200;
/// … for more than this many console frames stops its line.
pub const CUT_FRAMES: u32 = 60;

const INV_32767: f32 = f32::from_bits(0x3800_0100);
const INV_4096: f32 = f32::from_bits(0x3980_0000);
/// 360 / 65535 (`0x822F8C64`): raw azimuth → degrees.
const DEGREES: f32 = f32::from_bits(0x3BB4_00B4);

/// The ped audio state words that pick PedestrianSpeech's level outputs (`sub_824D9370`). `S+100`,
/// `S+104` and `S+112` are not traced (the recomp's peds were not logged with them): 0 = a regular
/// ped, out2 / out3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PedLevelSelect {
    pub s100: i32,
    pub s104: i32,
    pub s112: i32,
    /// `S+96 == 64`: a security guard (out4 / out5; its radio event 8277 out6).
    pub security: bool,
}

/// PedestrianSpeech's (main level, second level) output ids for a line (`sub_824D9370`).
pub fn ped_level_ids(sel: PedLevelSelect, event: u16, far: bool) -> (usize, usize) {
    let b = usize::from(far);
    if sel.s100 != 0 {
        (9 + b, 19)
    } else if sel.s104 == 0 {
        if sel.s112 != 0 {
            (7 + b, 18)
        } else if sel.security {
            if event == 8277 { (6, 17) } else { (4 + b, 16) }
        } else {
            (2 + b, 15)
        }
    } else if matches!(sel.s104, 2 | 4) {
        (11 + b, 20)
    } else {
        (7 + b, 18)
    }
}

/// PlayerSpeech's (main level, second level) output ids (`sub_824DA300`): by the skater's model
/// (the pros below 30, 30–40, the living-world voices from 41).
pub fn skater_level_ids(model: u32, far: bool) -> (usize, usize) {
    let b = usize::from(far);
    if model < 30 {
        (4 + b, 12)
    } else if model < 41 {
        (6 + b, 11)
    } else {
        (2 + b, 10)
    }
}

/// The filter outputs (high pass, low pass) each owner kind copies (read with the MixMap's filter
/// reader).
pub const PED_FILTERS: [usize; 2] = [13, 14];
pub const SKATER_FILTERS: [usize; 2] = [8, 9];

/// One frame's stream values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VoiceParams {
    /// The owner's main level (raw, 0..32767: the cut reads it).
    pub level: i32,
    pub gain: f32,
    /// The environment (reverb) send.
    pub send: f32,
    pub pitch: f32,
    /// Degrees.
    pub azimuth: f32,
    pub hpf: f32,
    pub lpf: f32,
}

fn params(out: &dyn Outputs, ids: (usize, usize), filters: [usize; 2]) -> VoiceParams {
    let level = out.level(ids.0).clamp(0, 32767);
    VoiceParams {
        level,
        gain: level as f32 * INV_32767,
        send: out.level(ids.1).clamp(0, 32767) as f32 * INV_32767,
        pitch: out.pitch(1).max(1) as f32 * INV_4096,
        azimuth: out.raw(0) as f32 * DEGREES,
        hpf: out.level(filters[0]) as f32,
        lpf: out.level(filters[1]) as f32,
    }
}

/// A ped speaker's values (`out` = a PedestrianSpeech snapshot with [`PED_FILTERS`] read as filters).
pub fn ped_outputs(out: &dyn Outputs, sel: PedLevelSelect, event: u16, far: bool) -> VoiceParams {
    params(out, ped_level_ids(sel, event, far), PED_FILTERS)
}

/// A skater speaker's values (`out` = a PlayerSpeech snapshot with [`SKATER_FILTERS`] as filters).
pub fn skater_outputs(out: &dyn Outputs, model: u32, far: bool) -> VoiceParams {
    params(out, skater_level_ids(model, far), SKATER_FILTERS)
}

/// `sub_824A89E8`: the clip is a far line (its name ends in `_f`).
pub fn far_clip(name: &str) -> bool {
    name.strip_suffix(".dat").unwrap_or(name).ends_with("_f")
}

/// What plays the takes (the host's mixer and decoded speech).
pub trait SpeechVoices {
    /// Start `line`'s take with these values; None when it cannot play (no decoded take).
    fn open(&mut self, line: Line, p: &VoiceParams) -> Option<u32>;
    fn set(&mut self, voice: u32, p: &VoiceParams);
    fn alive(&self, voice: u32) -> bool;
    fn stop(&mut self, voice: u32);
}

/// A line the manager started for a speaker.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// The speaker (the owner id the host knows it by).
    pub speaker: u64,
    pub event: u16,
    /// The event's tuning `+16` and interrupt bytes `+13` / `+14`.
    pub priority: i32,
    pub interrupt: bool,
    pub interrupt_when_full: bool,
    /// The record's clips in order.
    pub lines: Vec<Line>,
    /// The `.evt` queue timeout (console frames, provisional).
    pub timeout: u32,
}

#[derive(Clone, Debug)]
struct Stream {
    req: Request,
    at: usize,
    voice: Option<u32>,
    low: u32,
}

/// What [`SpeechPlayer::request`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// On stream `k`.
    Playing(usize),
    /// On stream `k`, stopping the lower-priority line there.
    Interrupted(usize),
    Queued,
    /// The queue was full.
    Dropped,
}

/// Something that happened during [`SpeechPlayer::frame`] (logs, the summary counts, hooks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A take started on stream `k`.
    Started { k: usize, speaker: u64, line: Line },
    /// The line ended (its last take finished).
    Finished { k: usize, speaker: u64 },
    /// The cut stopped the line (the speaker's level stayed low).
    Cut { k: usize, speaker: u64 },
    /// The speaker went away (lost its instance).
    Gone { k: usize, speaker: u64 },
    /// A queued request timed out.
    Expired { speaker: u64 },
}

/// One speech channel's streams and queue.
#[derive(Clone, Debug, Default)]
pub struct SpeechPlayer {
    streams: [Option<Stream>; STREAMS],
    queue: VecDeque<(Request, u32)>,
    /// Counters (diagnostics).
    pub started: u64,
    pub interrupted: u64,
    pub dropped: u64,
    pub cut: u64,
}

impl SpeechPlayer {
    /// The speakers whose lines play now.
    pub fn speakers(&self) -> impl Iterator<Item = u64> + '_ {
        self.streams.iter().flatten().map(|s| s.req.speaker)
    }

    /// Whether `speaker` has a line playing (on any stream).
    pub fn speaking(&self, speaker: u64) -> bool {
        self.speakers().any(|s| s == speaker)
    }

    pub fn busy(&self) -> usize {
        self.streams.iter().flatten().count()
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// A line the manager started (see the module docs for the stream rule).
    pub fn request(&mut self, req: Request, voices: &mut dyn SpeechVoices) -> Outcome {
        if let Some(k) = self.streams.iter().position(Option::is_none) {
            self.streams[k] = Some(Stream { req, at: 0, voice: None, low: 0 });
            return Outcome::Playing(k);
        }
        // Full: the lower-priority playing line, if the event may interrupt it.
        let (k, lowest) = self
            .streams
            .iter()
            .enumerate()
            .filter_map(|(k, s)| s.as_ref().map(|s| (k, s.req.priority)))
            .min_by_key(|&(k, p)| (p, k))
            .expect("full");
        if (req.interrupt || req.interrupt_when_full) && lowest < req.priority {
            if let Some(old) = self.streams[k].take()
                && let Some(v) = old.voice
            {
                voices.stop(v);
            }
            self.streams[k] = Some(Stream { req, at: 0, voice: None, low: 0 });
            self.interrupted += 1;
            return Outcome::Interrupted(k);
        }
        if self.queue.len() >= QUEUE {
            self.dropped += 1;
            return Outcome::Dropped;
        }
        self.queue.push_back((req, 0));
        Outcome::Queued
    }

    /// Stop every line and forget the queue (a map change, the speech going off).
    pub fn clear(&mut self, voices: &mut dyn SpeechVoices) {
        for s in self.streams.iter_mut() {
            if let Some(v) = s.take().and_then(|s| s.voice) {
                voices.stop(v);
            }
        }
        self.queue.clear();
    }

    /// One console frame: the queue, then every stream: its speaker's values (`speaker(id, far,
    /// event)`, None = the speaker is gone), the cut, the next take of the line, the voice values.
    pub fn frame(&mut self, index: &SpeechIndex, speaker: &mut dyn FnMut(u64, bool, u16) -> Option<VoiceParams>, voices: &mut dyn SpeechVoices) -> Vec<Event> {
        let mut events = Vec::new();
        // The queue: age, expire, then fill free streams (highest priority first, then the newest).
        for q in self.queue.iter_mut() {
            q.1 += 1;
        }
        self.queue.retain(|(r, age)| {
            let keep = *age <= r.timeout;
            if !keep {
                events.push(Event::Expired { speaker: r.speaker });
            }
            keep
        });
        while let Some(k) = self.streams.iter().position(Option::is_none) {
            let Some(best) = self.queue.iter().enumerate().max_by_key(|(i, (r, _))| (r.priority, *i)).map(|(i, _)| i) else { break };
            let (req, _) = self.queue.remove(best).expect("index");
            self.streams[k] = Some(Stream { req, at: 0, voice: None, low: 0 });
        }
        for k in 0..STREAMS {
            let Some(mut s) = self.streams[k].take() else { continue };
            let line = s.req.lines[s.at];
            let far = index.clips.get(line.clip).is_some_and(|c| far_clip(&c.name));
            let Some(p) = speaker(s.req.speaker, far, s.req.event) else {
                if let Some(v) = s.voice {
                    voices.stop(v);
                }
                events.push(Event::Gone { k, speaker: s.req.speaker });
                continue;
            };
            // The cut.
            if p.level <= CUT_LEVEL {
                s.low += 1;
                if s.low > CUT_FRAMES {
                    if let Some(v) = s.voice {
                        voices.stop(v);
                    }
                    self.cut += 1;
                    events.push(Event::Cut { k, speaker: s.req.speaker });
                    continue;
                }
            } else {
                s.low = 0;
            }
            match s.voice {
                Some(v) if voices.alive(v) => voices.set(v, &p),
                Some(v) => {
                    voices.stop(v);
                    s.voice = None;
                    s.at += 1;
                    if s.at >= s.req.lines.len() {
                        events.push(Event::Finished { k, speaker: s.req.speaker });
                        continue;
                    }
                    let line = s.req.lines[s.at];
                    let far = index.clips.get(line.clip).is_some_and(|c| far_clip(&c.name));
                    let p = speaker(s.req.speaker, far, s.req.event).unwrap_or(p);
                    s.voice = voices.open(line, &p);
                    if s.voice.is_some() {
                        events.push(Event::Started { k, speaker: s.req.speaker, line });
                    }
                }
                None => {
                    s.voice = voices.open(line, &p);
                    match s.voice {
                        Some(_) => {
                            self.started += 1;
                            events.push(Event::Started { k, speaker: s.req.speaker, line });
                        }
                        None => {
                            // Not playable (the take is not decoded): the line ends.
                            events.push(Event::Finished { k, speaker: s.req.speaker });
                            continue;
                        }
                    }
                }
            }
            self.streams[k] = Some(s);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::speech::{Clip, Take};

    struct Out(i32);
    impl Outputs for Out {
        fn level(&self, id: usize) -> i32 {
            match id {
                2 => self.0,
                3 => 2903,
                13 => 77,
                14 => 24956,
                15 => 1196,
                _ => 100 * id as i32,
            }
        }
        fn raw(&self, _: usize) -> i32 {
            16384
        }
        fn pitch(&self, _: usize) -> i32 {
            4086
        }
    }

    #[derive(Default)]
    struct Voices {
        live: Vec<u32>,
        opened: Vec<Line>,
        next: u32,
        last: Option<VoiceParams>,
    }
    impl SpeechVoices for Voices {
        fn open(&mut self, line: Line, p: &VoiceParams) -> Option<u32> {
            self.next += 1;
            self.live.push(self.next);
            self.opened.push(line);
            self.last = Some(*p);
            Some(self.next)
        }
        fn set(&mut self, _: u32, p: &VoiceParams) {
            self.last = Some(*p);
        }
        fn alive(&self, v: u32) -> bool {
            self.live.contains(&v)
        }
        fn stop(&mut self, v: u32) {
            self.live.retain(|x| *x != v);
        }
    }

    fn index() -> SpeechIndex {
        let take = Take { offset: 0, size: 1, rate: 36000, samples: 1 };
        SpeechIndex::new(vec![
            Clip { name: "501_59_busm1_Warn_n.dat".into(), event: 501, voice: 59, voice_name: Some("busm1".into()), line: "Warn_n".into(), takes: vec![take; 2] },
            Clip { name: "501_59_busm1_Warn_f.dat".into(), event: 501, voice: 59, voice_name: Some("busm1".into()), line: "Warn_f".into(), takes: vec![take; 2] },
        ])
    }

    fn req(speaker: u64, clip: usize, priority: i32, interrupt: bool) -> Request {
        Request { speaker, event: 8210, priority, interrupt, interrupt_when_full: false, lines: vec![Line { clip, take: 0, event: 501 }], timeout: 3 }
    }

    #[test]
    fn the_level_outputs_follow_the_speaker_and_the_far_clip() {
        assert_eq!(ped_level_ids(PedLevelSelect::default(), 8210, false), (2, 15));
        assert_eq!(ped_level_ids(PedLevelSelect::default(), 8210, true), (3, 15));
        let guard = PedLevelSelect { security: true, ..Default::default() };
        assert_eq!(ped_level_ids(guard, 8210, true), (5, 16));
        assert_eq!(ped_level_ids(guard, 8277, false), (6, 17), "the guard's radio");
        assert_eq!(skater_level_ids(91, false), (2, 10));
        assert_eq!(skater_level_ids(12, true), (5, 12));
        assert!(far_clip("501_59_busm1_Warn_f.dat") && !far_clip("101_51_GenPos_Grn1_far.dat") && !far_clip("501_59_busm1_Warn_n.dat"));
        let p = ped_outputs(&Out(9804), PedLevelSelect::default(), 8210, false);
        assert_eq!((p.level, p.hpf, p.lpf), (9804, 77.0, 24956.0));
        assert!((p.send - 1196.0 / 32767.0).abs() < 1e-6 && (p.pitch - 4086.0 / 4096.0).abs() < 1e-6);
        assert!((p.azimuth - 16384.0 * 360.0 / 65535.0).abs() < 1e-2);
        assert_eq!(ped_outputs(&Out(9804), PedLevelSelect::default(), 8210, true).level, 2903);
    }

    #[test]
    fn two_streams_interrupts_queue_and_cut() {
        let ix = index();
        let mut p = SpeechPlayer::default();
        let mut v = Voices::default();
        assert_eq!(p.request(req(1, 0, 500, false), &mut v), Outcome::Playing(0));
        assert_eq!(p.request(req(2, 1, 520, false), &mut v), Outcome::Playing(1));
        assert_eq!(p.request(req(3, 0, 510, false), &mut v), Outcome::Queued, "no interrupt byte: it waits");
        assert_eq!(p.request(req(4, 0, 510, true), &mut v), Outcome::Interrupted(0), "beats the 500 line");
        let mut level = 9000;
        let ev = p.frame(&ix, &mut |_, _, _| Some(ped_outputs(&Out(level), PedLevelSelect::default(), 8210, false)), &mut v);
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::Started { .. })).count(), 2);
        assert_eq!(v.opened.len(), 2);
        // Speaker 2's line is far: out3.
        assert_eq!(p.speakers().collect::<Vec<_>>(), vec![4, 2]);
        // The queued request expires after its timeout (3 frames).
        for _ in 0..3 {
            p.frame(&ix, &mut |_, far, _| Some(ped_outputs(&Out(level), PedLevelSelect::default(), 8210, far)), &mut v);
        }
        assert_eq!(p.queued(), 0);
        // The cut: 61 frames at or below 200.
        level = 150;
        let mut cut = 0;
        for _ in 0..61 {
            let ev = p.frame(&ix, &mut |s, far, _| (s == 4).then(|| ped_outputs(&Out(level), PedLevelSelect::default(), 8210, far)), &mut v);
            cut += ev.iter().filter(|e| matches!(e, Event::Cut { .. })).count();
        }
        assert_eq!(cut, 1, "speaker 4 cut; speaker 2 went away at once");
        assert_eq!(p.busy(), 0);
        // A finished take ends the line.
        assert_eq!(p.request(req(5, 0, 500, false), &mut v), Outcome::Playing(0));
        p.frame(&ix, &mut |_, _, _| Some(ped_outputs(&Out(9000), PedLevelSelect::default(), 8210, false)), &mut v);
        v.live.clear();
        let ev = p.frame(&ix, &mut |_, _, _| Some(ped_outputs(&Out(9000), PedLevelSelect::default(), 8210, false)), &mut v);
        assert_eq!(ev, vec![Event::Finished { k: 0, speaker: 5 }]);
    }
}
