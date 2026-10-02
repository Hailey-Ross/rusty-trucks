//! Retail world sound emitters: the map's `.ems` records (positions, shapes) with
//! their sound attributes from the skatercollections database, exported by
//! setup into the audio manifest (tools/asset_pipeline/audio_export.py).
//!
//! The game side follows TU3 (.claude/notes/ems-emitters-re.md; docs 11):
//! - a record is a sphere when its three extents are equal, otherwise an
//!   ellipsoid whose semi-axes are the extents along forward = scalars[1..4],
//!   up and side; the listener's normalised distance `d` must be below 1;
//! - scalars[0] is an inner core: inside it the level is full, outside `d` is
//!   rescaled over the rest;
//! - level = attribute volume x falloff curve ((1-d)^2, 1-d or flat);
//! - at most `MAX_ACTIVE` play, in the order they were reached; a record the
//!   listener leaves stops at once (retail releases it without a fade).
//!
//! What each bank then plays (relay of short pieces or one loop, and its slow
//! level/pitch movement) is retail's patch program for the bank. Until the
//! AEMS evaluator is ported, `PROFILES` holds those programs' measured
//! behaviour (PoC evaluator runs, notes ems-emitters-re.md); banks without a
//! profile are not played yet.
use super::{Category, Library, Play, Voices, library::Clip, voices::VoiceId};
use bevy::prelude::*;

/// CSTATEMGR_Emitter's pool size.
const MAX_ACTIVE: usize = 5;
/// Voices are placed at most this far from the listener towards the emitter,
/// so Bevy's distance attenuation stays 1 and the retail level is the only one.
const PAN_DISTANCE: f32 = 8.0;
/// Stop fade (s). Retail releases at once; this only avoids a click.
const STOP_FADE: f32 = 0.05;

/// How a bank's program plays.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pattern {
    /// Two voices take turns, each drawing from its own shuffle bag of the
    /// bank's samples; the next starts `overlap` seconds before the current ends.
    Relay { overlap: f32 },
    /// One voice loops the bank's first sample.
    Loop,
}

/// Measured behaviour of a bank's program: pattern, level and pitch ranges and
/// their modulation periods (seconds).
#[derive(Clone, Copy, Debug)]
struct Profile {
    bank: &'static str,
    pattern: Pattern,
    level: (f32, f32, f32),
    pitch: (f32, f32, f32),
}

const PROFILES: &[Profile] = &[
    Profile { bank: "water_fountain", pattern: Pattern::Relay { overlap: 0.17 }, level: (0.645, 0.942, 3.46), pitch: (0.818, 0.940, 4.0) },
    Profile { bank: "water_lapping", pattern: Pattern::Relay { overlap: 0.17 }, level: (0.466, 1.0, 2.75), pitch: (0.940, 1.062, 4.0) },
    Profile { bank: "water_lapping_pond", pattern: Pattern::Relay { overlap: 0.17 }, level: (0.466, 1.0, 3.52), pitch: (0.940, 1.062, 4.0) },
    Profile { bank: "ocean_wave_small", pattern: Pattern::Relay { overlap: 0.0 }, level: (0.479, 0.824, 4.0), pitch: (0.855, 1.001, 9.98) },
    Profile { bank: "water_dam_close", pattern: Pattern::Loop, level: (0.462, 0.759, 3.5), pitch: (0.952, 1.031, 4.0) },
    Profile { bank: "water_dam_far", pattern: Pattern::Loop, level: (0.797, 1.0, 3.52), pitch: (0.915, 1.062, 4.0) },
    Profile { bank: "fountains_waterlaps_left", pattern: Pattern::Loop, level: (1.0, 1.0, 1.0), pitch: (1.0, 1.0, 1.0) },
    Profile { bank: "fountains_waterlaps_right", pattern: Pattern::Loop, level: (1.0, 1.0, 1.0), pitch: (1.0, 1.0, 1.0) },
];

fn profile(bank: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.bank == bank)
}

/// The `.ems` file of a map (by its `.skate` file stem).
fn ems_file(map_stem: &str) -> Option<&'static str> {
    Some(match map_stem {
        "University" => "sfx_university",
        "DownTown" => "sfx_downtown",
        "Industrial" => "sfx_industrial",
        "DownTownSkatePark" => "sfx_dt_skatepark",
        "IndustrialSkatePark" => "sfx_ind_skatepark",
        "MegaPark" => "sfx_mega_skatepark",
        "MaloofMoneyCup" => "sfx_maloof_money_cup",
        "StartPark" => "sfx_startpark",
        "BlackBoxPark" => "sfx_blackbox_park",
        "SkateSchool" => "skateschool",
        _ => return None,
    })
}

/// A record's shape, in world space.
#[derive(Clone, Copy, Debug)]
struct Shape {
    position: Vec3,
    extent: Vec3,
    forward: Vec3,
    core: f32,
}

/// Normalised distance of `listener` in the shape (0 at the core, 1 at the
/// edge), or None outside.
fn reach(shape: &Shape, listener: Vec3) -> Option<f32> {
    let delta = listener - shape.position;
    let e = shape.extent;
    let d = if e.x == e.y && e.y == e.z {
        delta.length() / e.x
    } else {
        let side = Vec3::Y.cross(shape.forward).normalize_or_zero();
        let up = shape.forward.cross(side).normalize_or_zero();
        let (f, u, s) = (delta.dot(shape.forward) / e.x, delta.dot(up) / e.y, delta.dot(side) / e.z);
        (f * f + u * u + s * s).sqrt()
    };
    if !d.is_finite() || d >= 1.0 {
        return None;
    }
    let core = shape.core;
    Some(if core > 0.0 && d < core { 0.0 } else if core >= 1.0 { 0.0 } else { (d - core) / (1.0 - core) })
}

/// Retail falloff curve by `eVolumeFalloffType`.
fn falloff(kind: i32, d: f32) -> f32 {
    match kind {
        0 => (1.0 - d) * (1.0 - d),
        1 => 1.0 - d,
        _ => 1.0,
    }
}

struct Emitter {
    shape: Shape,
    volume: f32,
    falloff: i32,
    profile: &'static Profile,
}

/// A started emitter: its voices and program state.
struct Node {
    record: usize,
    started: bool,
    /// (voice, clip, end time) of the voice currently leading the relay or looping.
    voices: Vec<(VoiceId, Clip, f64)>,
    /// Which relay voice plays next, and each voice's shuffle bag.
    turn: usize,
    bags: [Vec<usize>; 2],
    phase: (f32, f32),
}

#[derive(Default)]
pub(super) struct State {
    map: Option<(String, u64)>,
    emitters: Vec<Emitter>,
    /// Reached records in discovery order (retail's node list).
    nodes: Vec<Node>,
    rng: u32,
}

fn random(rng: &mut u32) -> f32 {
    if *rng == 0 {
        *rng = 0x9e37_79b9;
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 17;
    *rng ^= *rng << 5;
    (*rng >> 8) as f32 / (1u32 << 24) as f32
}

/// Next sample from a shuffle bag over `count` samples (no repeat until empty).
fn draw(bag: &mut Vec<usize>, count: usize, rng: &mut u32) -> usize {
    if bag.is_empty() {
        *bag = (0..count).collect();
    }
    let at = (random(rng) * bag.len() as f32) as usize % bag.len();
    bag.swap_remove(at)
}

fn modulate((low, high, period): (f32, f32, f32), phase: f32, now: f64) -> f32 {
    let wave = (std::f64::consts::TAU * now / f64::from(period.max(0.1)) + f64::from(phase)).sin() as f32;
    low + (high - low) * 0.5 * (1.0 + wave)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut state: Local<State>,
    mut commands: Commands,
    map: Res<crate::map_transition::CurrentMap>,
    library: Option<ResMut<Library>>,
    mut voices: ResMut<Voices>,
    mut assets: ResMut<Assets<AudioSource>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    time: Res<Time<Real>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
) {
    let Some(mut library) = library else { return };
    let state = &mut *state;
    let identity = (map.name.clone(), map.generation);
    if state.map.as_ref() != Some(&identity) {
        for node in state.nodes.drain(..) {
            for (id, ..) in node.voices {
                voices.stop(id, STOP_FADE);
            }
        }
        let stem = map.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("");
        let records = ems_file(stem).map_or(&[][..], |file| library.emitters(file));
        state.emitters = records.iter().filter(|r| r.kind == 1 && r.flags == 0).filter_map(|r| {
            let profile = profile(r.bank.as_deref()?)?;
            let s = r.scalars;
            Some(Emitter {
                shape: Shape { position: Vec3::from(r.position), extent: Vec3::from(r.extent), forward: Vec3::new(s[1], s[2], s[3]), core: s[0] },
                volume: r.volume, falloff: r.falloff, profile,
            })
        }).collect();
        info!("World emitters: {} of {} records on {stem} have a played sound", state.emitters.len(), records.len());
        state.map = Some(identity);
    }
    let Ok(ear) = listener.single().map(GlobalTransform::translation) else { return };
    let silent = super::silenced(menu.as_deref(), &replay);
    let now = time.elapsed_secs_f64();
    let mut rng = state.rng;

    // Release nodes the listener left (or everything while silenced).
    let reached: Vec<Option<f32>> = state.emitters.iter().map(|e| if silent { None } else { reach(&e.shape, ear) }).collect();
    state.nodes.retain(|node| {
        let keep = reached[node.record].is_some();
        if !keep {
            if node.started {
                info!("AUDIO_EMITTER stop {} #{}", state.emitters[node.record].profile.bank, node.record);
            }
            for (id, ..) in &node.voices {
                voices.stop(*id, STOP_FADE);
            }
        }
        keep
    });
    // New hits join the node list in discovery order.
    for (index, hit) in reached.iter().enumerate() {
        if hit.is_some() && !state.nodes.iter().any(|n| n.record == index) {
            state.nodes.push(Node {
                record: index, started: false, voices: Vec::new(), turn: 0, bags: [Vec::new(), Vec::new()],
                phase: (random(&mut rng) * std::f32::consts::TAU, random(&mut rng) * std::f32::consts::TAU),
            });
        }
    }
    // Waiting nodes take free states in list order.
    let mut active = state.nodes.iter().filter(|n| n.started).count();
    for node in state.nodes.iter_mut().filter(|n| !n.started) {
        if active >= MAX_ACTIVE {
            break;
        }
        node.started = true;
        active += 1;
        let e = &state.emitters[node.record];
        info!("AUDIO_EMITTER start {} #{} at {:.1?} volume {:.2}", e.profile.bank, node.record, e.shape.position.to_array(), e.volume);
    }

    for node in state.nodes.iter_mut().filter(|n| n.started) {
        let emitter = &state.emitters[node.record];
        let Some(d) = reached[node.record] else { continue };
        let profile = emitter.profile;
        let level = emitter.volume * falloff(emitter.falloff, d) * modulate(profile.level, node.phase.0, now);
        let pitch = modulate(profile.pitch, node.phase.1, now);
        let towards = emitter.shape.position - ear;
        let at = ear + towards.normalize_or_zero() * towards.length().min(PAN_DISTANCE);
        node.voices.retain(|(id, ..)| voices.playing(*id));
        for (id, ..) in &node.voices {
            voices.set(*id, level, pitch, Some(at));
        }
        let samples = library.bank_len(profile.bank);
        if samples == 0 {
            continue;
        }
        let play = |looping| Play { category: Category::Ambience, volume: level, pitch, position: Some(at), looping, fade_in: 0.0, envelope: None };
        match profile.pattern {
            Pattern::Loop => {
                if node.voices.is_empty() {
                    if let Some(clip) = library.sample(&mut assets, profile.bank, 0) {
                        if let Some(id) = voices.play(&mut commands, &clip, play(true), now) {
                            node.voices.push((id, clip, f64::INFINITY));
                        }
                    }
                }
            }
            Pattern::Relay { overlap } => {
                let due = node.voices.iter().map(|v| v.2).fold(f64::NEG_INFINITY, f64::max) - f64::from(overlap);
                if node.voices.is_empty() || now >= due {
                    let turn = node.turn;
                    let index = draw(&mut node.bags[turn], samples, &mut rng);
                    if let Some(clip) = library.sample(&mut assets, profile.bank, index) {
                        let seconds = library.sample_seconds(profile.bank, index);
                        if let Some(id) = voices.play(&mut commands, &clip, play(false), now) {
                            node.voices.push((id, clip, now + f64::from(seconds / pitch.max(0.25))));
                            node.turn = 1 - turn;
                        }
                    }
                }
            }
        }
    }
    state.rng = rng;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(extent: Vec3, forward: Vec3, core: f32) -> Shape {
        Shape { position: Vec3::ZERO, extent, forward, core }
    }

    #[test]
    fn sphere_uses_the_first_extent_as_radius() {
        let s = shape(Vec3::splat(10.0), Vec3::X, 0.0);
        assert_eq!(reach(&s, Vec3::new(5.0, 0.0, 0.0)), Some(0.5));
        assert_eq!(reach(&s, Vec3::new(0.0, 0.0, 10.0)), None);
    }

    #[test]
    fn ellipsoid_axes_follow_forward_up_and_side() {
        // University fountain: 13 along forward (x), 7 up, 61 along the side (z).
        let s = shape(Vec3::new(13.0, 7.0, 61.0), Vec3::X, 0.0);
        assert!((reach(&s, Vec3::new(0.0, 0.0, 30.5)).unwrap() - 0.5).abs() < 1e-5);
        assert!((reach(&s, Vec3::new(6.5, 0.0, 0.0)).unwrap() - 0.5).abs() < 1e-5);
        assert!((reach(&s, Vec3::new(0.0, 3.5, 0.0)).unwrap() - 0.5).abs() < 1e-5);
        assert_eq!(reach(&s, Vec3::new(14.0, 0.0, 0.0)), None);
        // Rotated a quarter turn, the long axis lies along x instead.
        let turned = shape(Vec3::new(13.0, 7.0, 61.0), Vec3::Z, 0.0);
        assert!(reach(&turned, Vec3::new(30.0, 0.0, 0.0)).is_some());
    }

    #[test]
    fn inner_core_is_full_level_and_rescales_the_rest() {
        let s = shape(Vec3::splat(10.0), Vec3::X, 0.2);
        assert_eq!(reach(&s, Vec3::new(1.0, 0.0, 0.0)), Some(0.0));
        assert!((reach(&s, Vec3::new(6.0, 0.0, 0.0)).unwrap() - 0.5).abs() < 1e-5);
    }

    #[test]
    fn falloff_curves() {
        assert_eq!(falloff(0, 0.5), 0.25);
        assert_eq!(falloff(1, 0.5), 0.5);
        assert_eq!(falloff(7, 0.5), 1.0);
    }

    #[test]
    fn shuffle_bag_plays_every_sample_before_repeating() {
        let (mut bag, mut rng) = (Vec::new(), 1);
        let mut seen: Vec<usize> = (0..10).map(|_| draw(&mut bag, 10, &mut rng)).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn every_map_has_its_emitter_file() {
        for map in ["University", "DownTown", "Industrial", "SkateSchool", "MegaPark"] {
            assert!(ems_file(map).is_some(), "{map}");
        }
        assert!(profile("water_fountain").is_some() && profile("trees_rustle").is_none());
    }
}
