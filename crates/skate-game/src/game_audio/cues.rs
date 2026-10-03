//! Which decoded retail sounds play for which gameplay cue. Retail picks them
//! through compiled AEMS event scripts we do not run. Since 2026-10-01 the
//! impact/contact cues use the retail patches (SPLC records/containers) that a
//! local recompiled build was measured playing for the same moments, at levels
//! in retail's measured ratios (docs/hails-additions/11-audio.md, "Measured
//! retail mapping"). The rest are our own picks, made by ear. Bank names and
//! indices refer to assets/private/audio/audio_manifest.json.
//!
//! **Native runtime (`SKATE_AEMS=1`):** these tables are the interim path only. With the native
//! player components running, the retired ones never play (`skate_events::play` gates them):
//! - retired, user-confirmed (19:19 build): `POP`, `POP_TAIL`, `LAND`, `LAND_NORMAL`,
//!   `LAND_HOLLOW`, `LAND_KIND_CHANCE`, `land_tier`, `land_scale`, `LAND_CLOTH`, `POWERSLIDE*`;
//! - retired, native replacement ready but not user-confirmed yet: `GRIND_START`, `GRIND_PIECES`,
//!   `GRIND_METAL*`, `AIR_WHEELS`, `BOARD_DOWN_KNOCK`, `BOARD_DOWN_TOUCH`, `CATCH`, `FLIP`,
//!   `FOOT_DRAG*`, and the `BED_CUES` / `BED_RECORDS` entries the native components play
//!   (sense_of_speed, Seams_Bank, Rolling_Rattles, PatchBank_Rolling_Surfaces, the knocks
//!   1112 / 1115 / 1119 and the scuffs `sk8_foley` 94 / 95).
//! They stay for the default path (native runtime off) until that path is retired as a whole.
//!
//! Levels are linear and deliberately low: many raw samples peak at 0 dBFS
//! (retail mixes them down at runtime). Final gain = level x effects x master.
//! Retail-measured cues use level = measured retail level x RETAIL_SCALE,
//! calibrated so rolling (retail p90 0.065) lands on our tested ROLL_LEVEL.

/// A set of interchangeable samples: one is picked at random per play.
pub(super) struct Cue {
    pub bank: &'static str,
    pub samples: &'static [usize],
    pub level: f32,
}

/// A retail patch to play as its data describes (layers, probabilities, gain
/// ranges): record `id` of an SPLC bank, or a container (id >= record count),
/// which picks one of its records each time.
pub(super) struct Record {
    pub bank: &'static str,
    pub id: usize,
    pub level: f32,
    /// Seconds after the event (retail spreads a pop's layers over ~60 ms).
    pub delay: f32,
    /// Retail's gain over time for this layer (see the *_ENV curves).
    pub envelope: Option<&'static [(f32, f32)]>,
}
impl Record {
    const fn after(self, delay: f32) -> Self {
        Self { delay, ..self }
    }
    const fn shaped(self, envelope: &'static [(f32, f32)]) -> Self {
        Self { envelope: Some(envelope), ..self }
    }
}

// Retail's per-layer gain over time (relative to the layer's peak), measured
// from the recomp's gain modules for ~2,300 pop/landing voices in a clean
// session (music and speech off): long samples are faded down within ~100 ms,
// which is what makes retail's impacts short and punchy instead of ringing.
/// Deck knocks and truck/wheel contacts: full for 50 ms, ~0.35 by 300 ms.
pub(super) const KNOCK_ENV: &[(f32, f32)] =
    &[(0.0, 1.0), (0.05, 1.0), (0.1, 0.88), (0.15, 0.58), (0.2, 0.45), (0.3, 0.35), (0.5, 0.3)];
/// Tail impacts and the hollow (deep) sets: held longer.
pub(super) const TAIL_ENV: &[(f32, f32)] = &[(0.0, 1.0), (0.1, 1.0), (0.15, 0.84), (0.2, 0.75), (0.3, 0.75), (0.5, 0.6)];
/// The body set: a 50 ms swell, gone by 150 ms.
pub(super) const BODY_ENV: &[(f32, f32)] = &[(0.0, 0.4), (0.05, 1.0), (0.1, 0.2), (0.15, 0.04)];
/// Wheels-down (~1 s samples): cut to a fifth within 100 ms.
pub(super) const WHEELS_ENV: &[(f32, f32)] = &[(0.0, 1.0), (0.02, 0.6), (0.05, 0.5), (0.1, 0.2), (0.15, 0.18)];
/// The pop crack: a click, down to 14 % after 20 ms.
pub(super) const CRACK_ENV: &[(f32, f32)] = &[(0.0, 1.0), (0.02, 0.14), (0.1, 0.08)];

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

/// Measured retail level -> our level (ROLL_LEVEL 0.15 / retail rolling p90 0.065, rounded down).
pub(super) const RETAIL_SCALE: f32 = 2.0;
const fn retail(bank: &'static str, id: usize, measured: f32) -> Record {
    Record { bank, id, level: measured * RETAIL_SCALE, delay: 0.0, envelope: None }
}
const SC: &str = "Skate_Collisions";

// Measured sequence for an ollie/kickflip (scripted recomp run, times from the
// flick): +0.24 s pop = containers 1096 + 1098/1099 with the flip object;
// +0.3-0.6 s board knocks 1112/1115/1118 (feet catching the board); +0.95 s
// landing = 1095 + one of the 1052/1055/1058 impact families + wheel skid.

// Pop and landing recipes from two recomp sessions (57 pops, 83 landings) and
// the real game sound captured with them (.local/recomp/compare_events.py):
// which retail sets play at each moment, and their median level there.
// (Earlier versions picked sets by their loudest moments, then by per-layer
// medians without the low thuds; both "did not sound close" to the user —
// the captured retail mix showed the missing low end.)

/// Pop, all at once: deck knock (1112, 548-552, ~0.42, the click), body
/// (1111, 173-182, ~0.16), low thuds (876: 199-201 ~0.18, 878: 193-195 ~0.08,
/// 891: 936/937 ~0.16), the pop crack (1096: 259/260 + 623-626, ~0.10) and one
/// of three tail impacts (1097/1098/1099, ~0.16 each).
/// Delays are retail's typical offsets from the knock (traced pops): ground
/// thud 193-195 at once, pop crack and tail impact ~+20 ms, body ~+30 ms,
/// low ground thud 199-201 ~+40 ms.
pub(super) const POP: &[Record] = &[
    retail(SC, 1112, 0.42).shaped(KNOCK_ENV), retail(SC, 878, 0.08), retail(SC, 891, 0.16),
    retail(SC, 1096, 0.10).after(0.02).shaped(CRACK_ENV), retail(SC, 1111, 0.16).after(0.03).shaped(BODY_ENV),
    retail(SC, 876, 0.18).after(0.04),
];
pub(super) const POP_TAIL: &[Record] =
    &[
        retail(SC, 1097, 0.16).after(0.02).shaped(TAIL_ENV),
        retail(SC, 1098, 0.16).after(0.02).shaped(TAIL_ENV),
        retail(SC, 1099, 0.16).after(0.02).shaped(TAIL_ENV),
    ];
/// Feet catching the board (1115: 548-552 + 530-535, ~0.43): retail plays it
/// ~0.2-0.3 s after the pop (scripted ollie: pop +0.24 s, knocks +0.42/+0.52 s).
pub(super) const CATCH: Record = retail(SC, 1115, 0.40).after(0.22).shaped(KNOCK_ENV);
/// Flip/spin whoosh when a trick registers: retail's Class_Flips plays
/// Sk8_Air_Flip_Tricks 1-3 (measured ~0.15-0.3).
pub(super) const FLIP: Cue = Cue { bank: "Sk8_Air_Flip_Tricks", samples: range!(1..=3), level: 0.45 };
/// Landing: the wheels-down set (1095, 396-404, ~0.09) always, plus retail's
/// board contact sets, ported from the game's own rule (recomp traces
/// 2026-10-01, 340 picks; read as reference only):
/// - a table per tier: tier 2 when the surface under the board is hollow
///   (observed: Wood_1 park ramps; the game reads a per-material flag, we take
///   the wood materials), tier 0 otherwise (tier 1 = during a bail);
/// - per contact kind (which wheels/part touched) three variants (light /
///   medium / hard). At a landing retail fires ~1.5 kinds: kind 0 44 %,
///   kind 1 47 %, kind 2 16 %, kind 3 46 % of landings;
/// - variant: retail uses the heaviest per-wheel impact class (not decoded);
///   ours follows the impact speed (`land_tier`).
/// Levels: the set's median level at landings in a clean session (music and
/// speech off, 118 landings; 1052 carries 22 % of the energy) x RETAIL_SCALE.
pub(super) const LAND: Record = retail(SC, 1095, 0.17).shaped(WHEELS_ENV);
/// [kind][variant] for tier 0 (normal) and tier 2 (hollow wood).
pub(super) const LAND_NORMAL: [[Record; 3]; 4] = [
    [retail(SC, 1051, 0.32).shaped(KNOCK_ENV), retail(SC, 1052, 0.48).shaped(KNOCK_ENV), retail(SC, 1053, 0.35).shaped(KNOCK_ENV)],
    [retail(SC, 1054, 0.18).shaped(KNOCK_ENV), retail(SC, 1055, 0.41).shaped(KNOCK_ENV), retail(SC, 1056, 0.31).shaped(KNOCK_ENV)],
    [retail(SC, 1057, 0.19).shaped(KNOCK_ENV), retail(SC, 1058, 0.33).shaped(KNOCK_ENV), retail(SC, 1059, 0.30).shaped(KNOCK_ENV)],
    [retail(SC, 1060, 0.22).shaped(KNOCK_ENV), retail(SC, 1061, 0.42).shaped(KNOCK_ENV), retail(SC, 1127, 0.30).shaped(KNOCK_ENV)],
];
pub(super) const LAND_HOLLOW: [[Record; 3]; 4] = [
    [retail(SC, 1062, 0.26).shaped(TAIL_ENV), retail(SC, 1063, 0.47).shaped(TAIL_ENV), retail(SC, 1053, 0.35).shaped(TAIL_ENV)],
    [retail(SC, 1065, 0.16).shaped(TAIL_ENV), retail(SC, 1066, 0.25).shaped(TAIL_ENV), retail(SC, 1056, 0.31).shaped(TAIL_ENV)],
    [retail(SC, 1068, 0.19).shaped(TAIL_ENV), retail(SC, 1069, 0.25).shaped(TAIL_ENV), retail(SC, 1070, 0.25).shaped(TAIL_ENV)],
    [retail(SC, 1071, 0.19).shaped(TAIL_ENV), retail(SC, 1131, 0.30).shaped(TAIL_ENV), retail(SC, 1132, 0.30).shaped(TAIL_ENV)],
];
/// Chance each contact kind sounds at a landing (retail, 100 landings).
pub(super) const LAND_KIND_CHANCE: [f32; 4] = [0.44, 0.47, 0.16, 0.46];
/// Cloth with the landing (sk8_foley 44-57, retail ~0.11).
pub(super) const LAND_CLOTH: Record = retail("sk8_foley", 95, 0.11).after(0.04);
/// Variant: light < 3, medium < 6, hard from 6 m/s impact (ours).
pub(super) fn land_tier(impact: f32) -> usize {
    match impact {
        i if i >= 6.0 => 2,
        i if i >= 3.0 => 1,
        _ => 0,
    }
}
/// Retail contact loudness rises gently with board speed (median 0.19-0.23
/// slow, ~0.33 above 11 m/s): 0.7 standing to 1.0 from 11 m/s.
pub(super) fn land_scale(speed: f32) -> f32 {
    let speed = if speed.is_finite() { speed.abs() } else { 0.0 };
    0.7 + 0.3 * (speed / 11.0).min(1.0)
}
/// Hollow surfaces for the tier-2 contact table: the wood materials
/// (Wood_Ramp 6, Plywood 7, Wood_1-4 41-46, Wood_5 90 in our audio ids;
/// retail's numbering is ours - 1).
pub(super) fn hollow(audio_surface: u32) -> bool {
    matches!(audio_surface, 6 | 7 | 41..=46 | 90)
}
/// Putting the board down / stepping on (measured when getting on the board):
/// the foot-on-deck set (1126: 422-426, 0.23-0.76) with a deck knock (1119) and
/// a light wheel touch (1054); jumping on (caveman) adds the landing set 1095.
pub(super) const BOARD_DOWN: Record = retail(SC, 1126, 0.30);
pub(super) const BOARD_DOWN_KNOCK: Record = retail(SC, 1119, 0.27).shaped(KNOCK_ENV);
pub(super) const BOARD_DOWN_TOUCH: Record = retail(SC, 1054, 0.14);
/// Grind start on ledges and other non-metal surfaces (954: 880-882) ...
pub(super) const GRIND_START: Record = retail(SC, 954, 0.25);
/// ... then retail re-fires short scrape pieces (955: 202-206, 0.04-0.24 s)
/// about every 63 ms while grinding (measured median gap), instead of a loop.
pub(super) const GRIND_PIECES: Record = retail(SC, 955, 0.15);
pub(super) const GRIND_PIECE_INTERVAL: f32 = 0.063;
/// Metal rails: the rail object's start (1146: 953-959, measured 0.49) ...
pub(super) const GRIND_METAL_START: Record = retail(SC, 1146, 0.40);
/// ... over the GRINDS metal loop (retail's Class_grind plays GRINDS quietly,
/// <= 0.13). 54-56 are the most tonal, brightest loops in the bank.
pub(super) const GRIND_METAL: Cue = Cue { bank: "GRINDS", samples: range!(54..=56), level: 0.26 };

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
/// Bail: retail's body hits (1074: records 667-671, samples 215-219 + 861-874,
/// measured <= 0.26) ...
pub(super) const BAIL_HIT: Record = retail(SC, 1074, 0.26);
/// ... with the Bodyslide layer under them (retail <= 0.19: an addition to the
/// fall, as heard in play). 0-15 come in three blocks of five — one longer
/// slide, four hits — taken as soft/medium/hard tiers; 15 joins the hard tier.
pub(super) const BAIL: Cue = Cue { bank: "Bodyslide", samples: range!(0..=15), level: 0.25 };
pub(super) const BAIL_TIERS: [&[usize]; 3] = [range!(0..=4), range!(5..=9), range!(10..=15)];
/// Body speed (m/s) at the start of a bail for the medium and hard tiers.
pub(super) fn bail_tier(speed: f32) -> usize {
    if speed >= 7.0 { 2 } else if speed >= 3.5 { 1 } else { 0 }
}

/// Falling into water. The user found Skate_Collisions 476 by ear; the bank's
/// patch tree puts 476-478 in record 869 with two more layers: 169 (played 75 %
/// of the time) and one of 171/172 — retail's full splash. Level follows the
/// fall speed. (Rejected: water_misc 0/4, Skate_Collisions 741-762, 473-475.)
pub(super) const SPLASH: Record = Record { bank: SC, id: 869, level: 0.5, delay: 0.0, envelope: None };
/// Pushing: foot hitting the ground.
pub(super) const PUSH: Cue = Cue { bank: "fstep_skateshoe1_sm", samples: range!(74..=84), level: 0.6 };
/// On-foot steps (walk) and heavier steps (run, landing on foot).
// These recordings peak ~11 dB below full scale and average ~28 dB below, so
// they are raised (the voice pool caps each clip at its own full-scale peak).
pub(super) const STEP: Cue = Cue { bank: "fstep_skateshoe1_sm", samples: range!(74..=84), level: 0.85 };
pub(super) const RUN_STEP: Cue = Cue { bank: "fstep_skateshoe1_sm", samples: range!(6..=17), level: 0.68 };

/// The riding bed: what retail plays continuously while rolling, away from
/// pops and landings (clean recomp session, 618 s; rates are starts per second
/// at typical riding speed, scaled by `bed_rate`; levels = median x
/// RETAIL_SCALE). Without it impacts sat on silence ("just a sound playing").
/// Surface collision pieces (SFXObj_Collision; ~2.1/s), cloth/body foley
/// (sk8_foley, ~2.9/s) and the odd deck knock.
pub(super) const BED_RECORDS: &[(Record, f32)] = &[
    (retail(SC, 955, 0.09), 0.50), (retail(SC, 954, 0.13), 0.45), (retail(SC, 956, 0.15), 0.42),
    (retail(SC, 958, 0.07), 0.23), (retail(SC, 947, 0.11), 0.19), (retail(SC, 952, 0.14), 0.17),
    (retail(SC, 968, 0.24), 0.17), (retail(SC, 948, 0.15), 0.11),
    (retail("sk8_foley", 95, 0.06), 0.78), (retail("sk8_foley", 94, 0.06), 0.52), (retail("sk8_foley", 74, 0.26), 0.32),
    (retail("sk8_foley", 73, 0.21), 0.12), (retail("sk8_foley", 62, 0.20), 0.33), (retail("sk8_foley", 84, 0.18), 0.17),
    (retail("sk8_foley", 88, 0.16), 0.12), (retail("sk8_foley", 85, 0.23), 0.17), (retail("sk8_foley", 89, 0.07), 0.12),
    (retail(SC, 1112, 0.19).shaped(KNOCK_ENV), 0.11), (retail(SC, 1115, 0.26).shaped(KNOCK_ENV), 0.08),
    (retail(SC, 1119, 0.32).shaped(KNOCK_ENV), 0.02),
];
/// Bed records the native contacts play from the retail mechanism (`player::contacts` foot taps
/// 1112..1120 and shoe scuffs `sk8_foley` 94 / 95): silent in the interim bed when those run.
pub(super) fn bed_record_native(record: &Record) -> bool {
    matches!((record.bank, record.id), (SC, 1112 | 1115 | 1119) | ("sk8_foley", 94 | 95))
}
/// Object-bank layers of the bed (retail's most used samples): truck rattles,
/// seams/cracks, speed wind and rolling-surface patches.
pub(super) const BED_CUES: &[(Cue, f32)] = &[
    (Cue { bank: "Rolling_Rattles", samples: &[11, 8, 7, 14, 9, 13], level: 0.44 }, 0.31),
    (Cue { bank: "Seams_Bank", samples: &[44, 144, 46, 40, 156, 140, 47, 161, 159, 153, 42, 36], level: 0.09 }, 0.31),
    (Cue { bank: "sense_of_speed", samples: &[4, 3, 0, 16, 1], level: 0.08 }, 0.5),
    (Cue { bank: "PatchBank_Rolling_Surfaces", samples: &[11, 1, 15, 13, 3], level: 0.14 }, 0.14),
];
/// Bed rate multiplier for board speed (m/s): none below 0.5, 1.0 at 6 m/s.
pub(super) fn bed_rate(speed: f32) -> f32 {
    let speed = if speed.is_finite() { speed.abs() } else { 0.0 };
    if speed < 0.5 { 0.0 } else { (speed / 6.0).min(1.5) }
}

/// Every one-shot/loop sample cue, for preloading.
pub(super) const ALL: &[&Cue] = &[&FLIP, &GRIND_METAL, &POWERSLIDE, &FOOT_DRAG, &BAIL, &PUSH, &STEP, &RUN_STEP];
/// Every retail patch cue, for preloading (all samples they can pick).
pub(super) const RECORDS: &[&Record] = &[
    &POP[0], &POP[1], &POP[2], &POP[3], &POP[4], &POP[5], &POP_TAIL[0], &POP_TAIL[1], &POP_TAIL[2],
    &CATCH, &LAND, &LAND_CLOTH,
    &LAND_NORMAL[0][0], &LAND_NORMAL[0][1], &LAND_NORMAL[0][2], &LAND_NORMAL[1][0], &LAND_NORMAL[1][1], &LAND_NORMAL[1][2],
    &LAND_NORMAL[2][0], &LAND_NORMAL[2][1], &LAND_NORMAL[2][2], &LAND_NORMAL[3][0], &LAND_NORMAL[3][1], &LAND_NORMAL[3][2],
    &LAND_HOLLOW[0][0], &LAND_HOLLOW[0][1], &LAND_HOLLOW[1][0], &LAND_HOLLOW[1][1], &LAND_HOLLOW[2][0], &LAND_HOLLOW[2][1],
    &LAND_HOLLOW[2][2], &LAND_HOLLOW[3][0], &LAND_HOLLOW[3][1], &LAND_HOLLOW[3][2],
    &BOARD_DOWN, &BOARD_DOWN_KNOCK, &BOARD_DOWN_TOUCH, &GRIND_START, &GRIND_PIECES, &GRIND_METAL_START,
    &BAIL_HIT, &SPLASH,
];

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
/// surface tag; names from the map tooling's audio surface table). Retail was
/// measured playing these "hard" grains too.
/// Rolling grain for a wheel's 7-bit audio surface tag, as retail maps it: tag → material (tag − 1) →
/// the vault's `Sk8::AudioSurfaceMap` → rolling surface 1–14 → grain member
/// (.claude/notes/grain-player-spec.md §1.4; corrected 2026-10-02, it used to be an own guess).
/// Only the hard-wheel members play: what selects the soft ones is not decoded yet.
pub(super) fn grain_for(audio_surface: u32) -> &'static str {
    match audio_surface & 0x7F {
        2 => "asphalt_rough_hard",                                                    // surface 1
        4 | 66 => "concrete_rough_hard",                                              // surface 2
        3 | 51..=54 | 57 | 60 | 63..=65 | 92 | 93 => "concrete_smooth_hard",          // surface 4
        6 | 7 | 41..=50 | 58 | 59 | 94 => "wood_ramp_hard",                           // surface 5
        5 => "concrete_aggregate_hard",                                               // surface 6
        9 | 11..=36 | 38..=40 | 84 | 85 | 89 | 91 => "metal_smooth_hard",             // surface 9
        // INTERIM stand-ins. Retail plays no grain on these: rolling surfaces 7 (tags 10, 70),
        // 8 (tag 8), 10 (67), 12 (68, 69) and 13 (37) post a per-surface `Class_rolling` patch
        // (selectors 1, 2, 10, 11, 9) that this engine cannot play until the AEMS evaluator exists.
        8 | 10 | 70 => "concrete_aggregate_hard",
        37 | 67..=69 => "metal_smooth_hard",
        // UNCERTAIN: tag 90 maps to rolling surface 0, which has no grain member; kept as before.
        90 => "wood_ramp_hard",
        // Surface 3 asphalt_smooth: tags 1, 55, 56, 61, 62, 71–83, 86–88, ≥ 95 and 0 (no material).
        _ => "asphalt_smooth_hard",
    }
}

/// Whether retail plays a grain on this tag (false: a `Class_rolling` patch, or uncertain).
#[cfg(test)]
fn retail_plays_grain(audio_surface: u32) -> bool {
    !matches!(audio_surface & 0x7F, 8 | 10 | 70 | 37 | 67..=69 | 90)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_grains_follow_the_retail_surface_map() {
        // Spot checks against the vault's AudioSurfaceMap (grain-player-spec.md §1.4).
        for (tag, grain) in [
            (2, "asphalt_rough_hard"), (66, "concrete_rough_hard"), (1, "asphalt_smooth_hard"),
            (55, "asphalt_smooth_hard"), (0, "asphalt_smooth_hard"), (100, "asphalt_smooth_hard"),
            (3, "concrete_smooth_hard"), (93, "concrete_smooth_hard"), (47, "wood_ramp_hard"),
            (94, "wood_ramp_hard"), (5, "concrete_aggregate_hard"), (84, "metal_smooth_hard"),
            (36, "metal_smooth_hard"), (130, "asphalt_rough_hard"),
        ] {
            assert_eq!(grain_for(tag), grain, "tag {tag}");
        }
        for tag in [8, 10, 70, 37, 67, 68, 69, 90] {
            assert!(!retail_plays_grain(tag), "tag {tag} is a Class_rolling / uncertain surface");
        }
        assert!(retail_plays_grain(1) && retail_plays_grain(9));
    }

    #[test]
    fn cue_ranges_and_levels_are_sane() {
        assert_eq!(FLIP.samples, &[1, 2, 3]);
        assert_eq!(STEP.samples.len(), 11);
        for cue in ALL {
            assert!(!cue.samples.is_empty());
            assert!(cue.level > 0.0 && cue.level <= 4.0, "{} out of range", cue.bank);
        }
        for record in RECORDS {
            assert!(record.level > 0.0 && record.level <= 1.0, "{}:{} out of range", record.bank, record.id);
        }
    }

    #[test]
    fn tiers_rise_with_impact_and_speed() {
        let all: Vec<usize> = BAIL_TIERS.iter().flat_map(|t| t.iter().copied()).collect();
        assert_eq!(all, BAIL.samples);
        assert_eq!((bail_tier(1.0), bail_tier(4.0), bail_tier(9.0)), (0, 1, 2));
        assert_eq!((land_tier(1.0), land_tier(4.0), land_tier(8.0)), (0, 1, 2));
        assert!(land_scale(0.0) == 0.7 && land_scale(11.0) == 1.0 && land_scale(f32::NAN) == 0.7);
        assert!(hollow(41) && hollow(6) && !hollow(3) && !hollow(66));
        assert!(bed_rate(0.2) == 0.0 && bed_rate(6.0) == 1.0 && bed_rate(30.0) == 1.5);
    }

    #[test]
    fn every_surface_has_a_grain() {
        assert_eq!(grain_for(3), "concrete_smooth_hard");
        assert_eq!(grain_for(6), "wood_ramp_hard");
        assert_eq!(grain_for(31), "metal_smooth_hard");
        // Retail: tags ≥ 95 map to asphalt_smooth (was concrete_smooth before the 2026-10-02 correction).
        assert_eq!(grain_for(127), "asphalt_smooth_hard");
    }
}
