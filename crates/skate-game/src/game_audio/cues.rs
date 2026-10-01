//! Which decoded retail samples play for which gameplay cue. Retail picks them
//! through compiled AEMS event scripts we do not run; these picks are our own,
//! made from the decoded banks by duration/attack/brightness and then by ear
//! (docs/hails-additions/11-audio.md lists the candidates and how to audition
//! them). Bank names and indices refer to assets/private/audio/audio_manifest.json.
//!
//! Levels are linear and deliberately low: many raw samples peak at 0 dBFS
//! (retail mixes them down at runtime). Final gain = level x effects x master.

/// A set of interchangeable samples: one is picked at random per play.
pub(super) struct Cue {
    pub bank: &'static str,
    pub samples: &'static [usize],
    pub level: f32,
}

macro_rules! range {
    ($a:literal..=$b:literal) => {{
        const N: usize = $b - $a + 1;
        const R: [usize; N] = {
            let mut out = [0; N];
            let mut i = 0;
            while i < N {
                out[i] = $a + i;
                i += 1;
            }
            out
        };
        &R
    }};
}

/// Skate_Collisions 1066-1092 were picked out by ear as "heard a lot while
/// skating": short knocks with most energy at 400-2500 Hz (the wooden deck),
/// unlike the low thumps on either side (1060-1065, 1093-1100).
/// Ollie / nollie pop (tail strike): the sharpest, shortest of them.
pub(super) const POP: Cue = Cue { bank: "Skate_Collisions", samples: range!(1074..=1078), level: 0.45 };
/// Board flip/spin whoosh when a trick registers in the air.
pub(super) const FLIP: Cue = Cue { bank: "Sk8_Air_Flip_Tricks", samples: range!(10..=13), level: 0.3 };
/// Landing (scaled by impact speed): the longer deck slaps of that family.
/// (Rejected before: 35-40 harsh; 0-2 is kept for setting the board down.)
pub(super) const LAND: Cue = Cue { bank: "Skate_Collisions", samples: range!(1088..=1092), level: 0.9 };
/// Extra weight for hard landings and jumping onto the board: the low thumps
/// right after the deck family (trucks/wheels), replacing the guessed 18-20.
pub(super) const LAND_HEAVY: Cue = Cue { bank: "Skate_Collisions", samples: range!(1097..=1100), level: 1.0 };
/// Grind/slide loop on rails and ledges.
/// GRINDS holds every material's grind loops (all 1.4-2.6 s). 9 and 11 (noisy,
/// low tonality) sounded like wood on a metal rail in play: they stay for
/// ledges and other non-metal surfaces.
pub(super) const GRIND: Cue = Cue { bank: "GRINDS", samples: &[9, 11], level: 0.3 };
/// Metal rails: 54-56 are the most tonal, brightest (4-5.6 kHz) loops in the
/// bank — a ringing metal scrape. Picked by measurement; not yet confirmed by ear.
pub(super) const GRIND_METAL: Cue = Cue { bank: "GRINDS", samples: range!(54..=56), level: 0.3 };

/// Whether a grind's audio surface (`grinds.audio_surface_216`) is metal:
/// Metal (9), the solid/hollow/square/complex metal shapes (11-40), manhole
/// and grates (67-69), DMORail (85), Metal_Rail_4 (89), Metal_Ramp (91).
pub(super) fn metal(audio_surface: u32) -> bool {
    matches!(audio_surface, 9 | 11..=40 | 67..=69 | 85 | 89 | 91)
}
/// Powerslide wheel skid: short pieces played back to back ("shuffle").
pub(super) const POWERSLIDE: Cue = Cue { bank: "WHEEL_SKID_BANK", samples: range!(0..=7), level: 0.2 };
/// Foot dragging to brake: ~0.2 s scuffs (0.1 s ones sounded choppy), overlapped.
pub(super) const FOOT_DRAG: Cue = Cue { bank: "FOOT_DRAG", samples: &[36, 37, 38, 39, 44, 45, 46, 47, 60, 61, 62, 63, 64], level: 0.25 };
/// (seconds between shuffled pieces, fade-in of each piece).
pub(super) const POWERSLIDE_SHUFFLE: (f32, f32) = (0.08, 0.01);
pub(super) const FOOT_DRAG_SHUFFLE: (f32, f32) = (0.11, 0.05);
/// Putting the board down: wheels touching after stepping on (softer, shorter impacts).
pub(super) const BOARD_DOWN: Cue = Cue { bank: "Skate_Collisions", samples: range!(0..=2), level: 0.3 };
/// Bail: body sliding/hitting the ground.
/// Bodyslide 0-15 comes in three blocks of five — one longer slide, four
/// hits — taken as soft/medium/hard tiers; 15 joins the hard tier. In play
/// they sound like additions to a fall, not the fall itself, so they are a
/// quieter layer until a main body-impact sound is picked
/// (`.local/audio-audition/bail.html`). `bail_tier` picks by impact speed.
pub(super) const BAIL: Cue = Cue { bank: "Bodyslide", samples: range!(0..=15), level: 0.25 };
pub(super) const BAIL_TIERS: [&[usize]; 3] = [range!(0..=4), range!(5..=9), range!(10..=15)];
/// Body speed (m/s) at the start of a bail for the medium and hard tiers.
pub(super) fn bail_tier(speed: f32) -> usize {
    if speed >= 7.0 { 2 } else if speed >= 3.5 { 1 } else { 0 }
}
/// A retail patch to play as its data describes (layers, probabilities, gain
/// ranges): record `id` of an SPLC bank, or a container (id >= record count).
pub(super) struct Record {
    pub bank: &'static str,
    pub id: usize,
    pub level: f32,
}

/// Falling into water. The user found Skate_Collisions 476 by ear; the bank's
/// patch tree puts 476-478 in record 869 with two more layers: 169 (played 75 %
/// of the time) and one of 171/172 — retail's full splash. Level follows the
/// fall speed. (Rejected: water_misc 0/4, Skate_Collisions 741-762, 473-475.)
pub(super) const SPLASH: Record = Record { bank: "Skate_Collisions", id: 869, level: 0.5 };
/// Samples of SPLASH, for preloading.
pub(super) const SPLASH_SAMPLES: Cue = Cue { bank: "Skate_Collisions", samples: &[169, 171, 172, 476, 477, 478], level: 0.5 };
/// Pushing: foot hitting the ground.
pub(super) const PUSH: Cue = Cue { bank: "fstep_skateshoe1_sm", samples: range!(74..=84), level: 0.6 };
/// On-foot steps (walk) and heavier steps (run, landing on foot).
// These recordings peak ~11 dB below full scale and average ~28 dB below, so
// they are raised (the voice pool caps each clip at its own full-scale peak).
pub(super) const STEP: Cue = Cue { bank: "fstep_skateshoe1_sm", samples: range!(74..=84), level: 0.85 };
pub(super) const RUN_STEP: Cue = Cue { bank: "fstep_skateshoe1_sm", samples: range!(6..=17), level: 0.68 };

/// Every one-shot/loop cue, for preloading.
pub(super) const ALL: &[&Cue] = &[&POP, &FLIP, &LAND, &LAND_HEAVY, &BOARD_DOWN, &GRIND, &GRIND_METAL, &POWERSLIDE, &FOOT_DRAG, &BAIL, &SPLASH_SAMPLES, &PUSH, &STEP, &RUN_STEP];

/// Shortest time between two plays of one cue (s): a safety net against
/// state flags that stay set for many ticks.
pub(super) fn min_gap(cue: &str) -> f32 {
    match cue {
        "step" => 0.2,
        "run_step" => 0.15,
        "push" => 0.3,
        "bail" => 0.5,
        "splash" => 1.0,
        _ => 0.12,
    }
}

/// Rolling loop level at full speed.
pub(super) const ROLL_LEVEL: f32 = 0.15;
/// Wheel spin after leaving the ground.
pub(super) const AIR_WHEELS: (&str, f32) = ("Whls_spins_Jump_1", 0.15);

/// Rolling grain for an audio material ID (low 7 bits of the collision
/// surface tag; names from the map tooling's audio surface table). Only the
/// "hard" grains exist for every material, so those are used.
pub(super) fn grain_for(audio_surface: u32) -> &'static str {
    match audio_surface {
        1 => "asphalt_smooth_hard",
        2 => "asphalt_rough_hard",
        4 | 66 => "concrete_rough_hard",                       // Concrete_Rough, Brick_Coarse
        5 | 8 | 10 | 55 | 56 => "concrete_aggregate_hard",    // Aggregate, Dirt, Grass, Leaves, Bush
        6 | 7 | 41..=46 | 90 => "wood_ramp_hard",             // Wood_Ramp, Plywood, Wood_*
        9 | 11..=40 | 67..=69 | 85 | 89 | 91 => "metal_smooth_hard", // Metal_*, grates, rails, Metal_Ramp
        // Concrete_Polished (the common default), curbs, benches, tile, marble,
        // smooth brick and anything unlisted.
        _ => "concrete_smooth_hard",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_ranges_and_levels_are_sane() {
        assert_eq!(POP.samples, &[1074, 1075, 1076, 1077, 1078]);
        assert_eq!(STEP.samples.len(), 11);
        for cue in [&POP, &FLIP, &LAND, &LAND_HEAVY, &BOARD_DOWN, &GRIND, &GRIND_METAL, &POWERSLIDE, &FOOT_DRAG, &BAIL, &SPLASH_SAMPLES, &PUSH, &STEP, &RUN_STEP] {
            assert!(!cue.samples.is_empty());
            assert!(cue.level > 0.0 && cue.level <= 4.0, "{} out of range", cue.bank);
        }
    }

    #[test]
    fn bail_tiers_cover_the_bank_and_rise_with_speed() {
        let all: Vec<usize> = BAIL_TIERS.iter().flat_map(|t| t.iter().copied()).collect();
        assert_eq!(all, BAIL.samples);
        assert_eq!((bail_tier(1.0), bail_tier(4.0), bail_tier(9.0)), (0, 1, 2));
    }

    #[test]
    fn every_surface_has_a_grain() {
        assert_eq!(grain_for(3), "concrete_smooth_hard");
        assert_eq!(grain_for(6), "wood_ramp_hard");
        assert_eq!(grain_for(31), "metal_smooth_hard");
        assert_eq!(grain_for(127), "concrete_smooth_hard");
    }
}
