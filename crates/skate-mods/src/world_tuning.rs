//! World tuning writes (capability `world_tuning` = 1): typed patches a mod applies to the living
//! world, dynamic props and prop carrying while it runs (`sdk.world.set_tuning(domain, patch)`).
//!
//! Every field is optional; an absent field keeps the game's value (retail by default). Patches are
//! held per mod; when several mods patch the same field the first writer wins, and a mod that stops
//! (or sends `nil`) gives its fields back. The schema lives here so the command is validated at the
//! serde boundary; the engine (`skate-game` `world_tuning`) applies it.
//!
//! Domains:
//! - `living_world`: `npc_draw_distance`, `skater_fade {fade_in_seconds, fade_seconds,
//!   despawn_alpha}`, `skater_line_chain {radius, max_candidates, blend_seconds, keep_facing, facing_rule,
//!   steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds,
//!   fakie_spawn_seconds}` (NPC skater line end: retail
//!   4 m / 16; root blend onto the new line after a branch or chain, engine default 0.2 s, 0 = cut), `ped_fade {distance = {near, far}, fade_in_seconds, enabled}`,
//!   `skater_clips {[<phase> or <phase>.<style>] = <stock clip name>}` (NPC skater puppet clip per
//!   replay phase; phases in [`NPC_SKATER_PHASES`]; a clip whose name holds `_CYC` loops),
//!   `skater_blend_seconds {[<phase> or default] = seconds}` (NPC skater crossfade into a phase's
//!   clip; stock graph default 0.2 s, 0 = cut; also `trick_takeoff` 0.05 s / `trick_air` 0.1 s),
//!   `skater_clips["trick.<scorable name>"]` = a trick animation base (`<base>_G` / `<base>_A`),
//!   `skater_clips["fakie_channel"]` = the stock tree overlaid while riding fakie (`B_FAKIE_CHANNEL`),
//!   `ped_obstacles {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin,
//!   step_height, held_is_obstacle, moving_solid}` (props and mod bodies as ped navigation
//!   obstacles; retail on / 0.2 / 0.4 / 0.25 / held props stay obstacles; `moving_solid` is our
//!   stand-in for NavPower's moving avoider, default on),
//!   `npc_skater_props {enabled}` (NPC skaters push dynamic props like the player; retail on),
//!   `ped_vehicle_contact {enabled, push}` (traffic cars touching peds; retail on / on: the ped is
//!   pushed out of the car, no knock-down).
//! - `props`: `default` and `by_template[<MOBJ template name>]`, each a [`PropTuningPatch`].
//! - `carry`: `grab_bit`, `placement_bit`, `grab_range`, and the Move Object tuning while
//!   holding a prop (retail defaults from attribute class 3EDA5B140604613D): `push_speed`,
//!   `pull_speed`, `side_speed` (target m/s at full left stick, retail 3.0 / 2.0 / 2.5),
//!   `turn_rate` (constant yaw gain replacing the retail inertia curve), `grip_reach` (m between
//!   the grab edge and the skater), `linear_clamp` / `yaw_clamp` (command clamps, 20 / 6),
//!   `relatch` (rad, 0.1), `slew_per_tick` (4), `yaw_rate_feedback` (60: the per-tick facing
//!   change times this is the measured yaw rate the yaw controller tracks; 0 = off),
//!   `linear_controller` / `yaw_controller`
//!   (`[p, filtered, d, filter]`, 20 / 0 / 40 / 0.1), the curves `lever_rotation`, `lever_yaw`,
//!   `mass_speed`, `inertia_yaw_gain` (`[[8 x], [8 y]]`), `let_go_distance` (m, 0 = off = retail), `drop_board`
//!   (grabbing a prop drops a carried board, retail true), `follow_step` (0.1 m), `hold_angle_limit` /
//!   `hold_max_angle_to_horizontal` (80 / 50 deg), `hold_box_extents` ([0.9, 0.8, 1.01]),
//!   `record_272_speed_scale` (2.0) and per template `record_272`. `grip_reach` sets the retail follow
//!   reach (0.65 m). Contact material blocks: `commanded_material` ([0.03, 0.02] static / dynamic
//!   friction), `upright_cos` (0.65) and per template `material_held`, `material_free`,
//!   `material_free_upright`, `upright_pair`, `restitution`, `linear_drag`, `angular_drag` (per
//!   second, retail DMO data +308 / +336 of the type), `mass` (kg, +304), `maximum_linear_velocity` /
//!   `maximum_angular_velocity` (+292 / +296) and `inertia_scale` / `inertia_offset` (+16 / +32).
//! - `shadows`: `world_floor = {r, g, b}`, the lightest a dynamic object's shadow can make the baked
//!   world (each 0..=1, in the shader's squared lightmap space). Retail {0.05, 0.09, 0.13}: the
//!   constant every retail world receiver shader adds to its shadow-map visibility before taking
//!   the minimum with the baked lightmap.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DOMAINS: [&str; 4] = ["living_world", "props", "carry", "shadows"];
/// Upper bound for every number (keeps a typo from building a 1e30 m fade range).
pub const MAX_NUMBER: f32 = 100_000.0;
/// Stable NPC skater replay phase ids (`skate_core::living_world::replay::ReplayPhase::name`).
pub const NPC_SKATER_PHASES: [&str; 6] = ["rolling", "crouched", "air", "air_trick", "ground_trick", "off_board"];
/// `skater_clips` key group for a recorded trick's animation: `trick.<EScorableID name>` (e.g.
/// `trick.kickflip`) = an animation base the NPC plays as `<base>_G` / `<base>_A` (fix 21).
pub const NPC_SKATER_TRICK_GROUP: &str = "trick";
/// `skater_clips` key for the stock tree the NPC overlays while riding fakie (retail
/// `B_FAKIE_CHANNEL`, `FakieHeadChannel82BAC778`).
pub const NPC_SKATER_FAKIE_CHANNEL: &str = "fakie_channel";
/// `skater_line_chain.facing_rule` values (`skate_core::living_world::replay::FacingRule::NAMES`).
pub const NPC_SKATER_FACING_RULES: [&str; 2] = ["riding_entry", "per_node"];
/// Extra `skater_blend_seconds` keys: into a trick's ground clip (retail 0.05 s) and into its air
/// clip when no ground clip ran before it (retail 0.1 s).
pub const NPC_SKATER_TRICK_BLENDS: [&str; 2] = ["trick_takeoff", "trick_air"];
/// Longest NPC skater crossfade a mod may set (s).
pub const MAX_BLEND_SECONDS: f32 = 10.0;
/// Per-template entries one patch may carry.
pub const MAX_TEMPLATES: usize = 256;

fn finite(v: Option<f32>) -> bool {
    v.is_none_or(|v| v.is_finite() && v.abs() <= MAX_NUMBER)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkaterFadePatch {
    pub fade_in_seconds: Option<f32>,
    pub fade_seconds: Option<f32>,
    pub despawn_alpha: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedFadePatch {
    /// Fallback camera distance pair (near, far); a model record's own pair still wins.
    pub distance: Option<[f32; 2]>,
    pub fade_in_seconds: Option<f32>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivingWorldPatch {
    pub npc_draw_distance: Option<f32>,
    pub skater_fade: Option<SkaterFadePatch>,
    pub skater_line_chain: Option<SkaterLineChainPatch>,
    pub ped_fade: Option<PedFadePatch>,
    /// NPC skater clip per phase id, or per `<phase>.<style>` (style = the pro's animation style).
    pub skater_clips: Option<BTreeMap<String, String>>,
    /// NPC skater crossfade time (s) into a phase's clip, per phase id or `default`.
    pub skater_blend_seconds: Option<BTreeMap<String, f32>>,
    /// Props and mod bodies as ped navigation obstacles (fix 11).
    pub ped_obstacles: Option<PedObstaclesPatch>,
    /// NPC skaters pushing dynamic props (fix 19).
    pub npc_skater_props: Option<NpcSkaterPropsPatch>,
    /// Traffic cars touching peds (`skate_core::living_world::peds::VehicleContactParams`).
    pub ped_vehicle_contact: Option<PedVehicleContactPatch>,
}

/// Traffic cars touching peds: `enabled` (detection, the event and the log; retail on), `push`
/// (the ped is shoved out of the car's box; retail on, the only response retail has).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedVehicleContactPatch {
    pub enabled: Option<bool>,
    pub push: Option<bool>,
}

/// NPC skaters against dynamic props: their board and body push a prop by the prop's own push
/// tuning (`props` domain), like the player's (retail: NPC skaters are full skaters; on).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcSkaterPropsPatch {
    pub enabled: Option<bool>,
}

/// Ped obstacle rules (`skate_core::living_world::peds::ObstacleParams`; retail: on, 0.2 m minimum
/// half extent, no cut above 0.4 m/s, re-cut after 0.25 x the smallest half extent; ours: 0.1 m
/// detour margin, 0 m step height). `held_is_obstacle`: a prop held by Move Object (or an attached
/// mod body) stays an obstacle (retail true). `moving_solid`: a moving object blocks a ped's step
/// (NOT RETAIL YET stand-in for NavPower's moving avoider; default true).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedObstaclesPatch {
    pub enabled: Option<bool>,
    pub min_half_extent: Option<f32>,
    pub moving_speed: Option<f32>,
    pub recut_fraction: Option<f32>,
    pub detour_margin: Option<f32>,
    pub step_height: Option<f32>,
    pub held_is_obstacle: Option<bool>,
    pub moving_solid: Option<bool>,
}

/// NPC skater line end (`skate_core::living_world::replay::ChainConfig`): continue on an unused
/// line whose start node is within `radius` m (retail 4.0; 0 = fade out at every line end), among
/// at most `max_candidates` (retail 16); `blend_seconds`: the drawn root moves onto the new line
/// over this time after a branch or chain (engine default 0.2 s, 0 = cut, at most 10 s);
/// `keep_facing`: mod option, not retail (default false): the skater keeps the way it faces
/// (forward or fakie) across a branch or chain by riding the new line turned round (fix 16).
/// `facing_rule`: one of [`NPC_SKATER_FACING_RULES`]: `riding_entry` (default, retail: the recorded
/// skater frame, turned while a flip latched on entering riding is set, held across switches; a
/// body against its travel on the ground is drawn riding fakie with the stock fakie channel) or
/// `per_node` (not retail, the fix 23 rule: each node folded onto the board's riding direction). `steer_dead_zone_deg` / `steer_full_deg`: retail AI
/// steer ramp (`ai_skater` 2 / 10 deg), data for the simulated tier (the replay tier does not steer).
/// `fakie_high_speed` / `fakie_low_speed` (m/s), `fakie_slow_seconds`, `fakie_spawn_seconds` (s):
/// retail's riding-fakie rule (stock motion graph `UpdateRidingFakie`: 1.0 / 0.5 / 0.2 / 1.0): the
/// NPC is drawn riding fakie (the stock fakie channel over its riding clip) when it rolls against
/// its board's forward above the high speed, or above the low speed for longer than the slow time,
/// never in the first spawn seconds. A very high speed turns the fakie drawing off.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkaterLineChainPatch {
    pub radius: Option<f32>,
    pub max_candidates: Option<u32>,
    pub blend_seconds: Option<f32>,
    pub keep_facing: Option<bool>,
    pub facing_rule: Option<String>,
    pub steer_dead_zone_deg: Option<f32>,
    pub steer_full_deg: Option<f32>,
    pub fakie_high_speed: Option<f32>,
    pub fakie_low_speed: Option<f32>,
    pub fakie_slow_seconds: Option<f32>,
    pub fakie_spawn_seconds: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropBoxPatch {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropTuningPatch {
    pub contact_padding: Option<f32>,
    pub penetration_slop: Option<f32>,
    pub penetration_correction: Option<f32>,
    pub max_depenetration_per_tick: Option<f32>,
    pub restitution_threshold: Option<f32>,
    pub skater_push_mass: Option<f32>,
    pub push_transfer: Option<f32>,
    pub body_push_speed: Option<f32>,
    pub board_push_speed: Option<f32>,
    pub penetration_push_speed: Option<f32>,
    pub stuck_release_ticks: Option<u32>,
    pub collision_box: Option<PropBoxPatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropsPatch {
    pub default: Option<PropTuningPatch>,
    #[serde(default)]
    pub by_template: BTreeMap<String, PropTuningPatch>,
    /// Island settings shared by every prop (retail DMO simulation block).
    pub solver: Option<PropSolverPatch>,
    /// Self-righting window of Upright (retail cMsgUprightDMO, 82C4B8C0 / 82C56780 / 82C573D0).
    pub upright: Option<PropUprightPatch>,
}

/// Upright self-righting (doc 27, Upright); retail defaults: `window_seconds` 2.0,
/// `tick_seconds` 1/60, `stop_angle_deg` 10, `max_angle_deg` 70, `dead_band_deg` 5, `gain_min` 3,
/// `gain_max` 5, `gain_blend_start` 1.1, `off_axis_spin` 0.1, `command_rate` 60,
/// `fallback_angle_deg` 120, `block_yaw` true. Numbers finite and >= 0; the window and tick > 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropUprightPatch {
    pub window_seconds: Option<f32>,
    pub tick_seconds: Option<f32>,
    pub stop_angle_deg: Option<f32>,
    pub max_angle_deg: Option<f32>,
    pub dead_band_deg: Option<f32>,
    pub gain_min: Option<f32>,
    pub gain_max: Option<f32>,
    pub gain_blend_start: Option<f32>,
    pub off_axis_spin: Option<f32>,
    pub command_rate: Option<f32>,
    pub fallback_angle_deg: Option<f32>,
    pub block_yaw: Option<bool>,
}

/// Prop contact solver and sleep rule (retail DMO simulation, 8275DCC8 ->
/// 82DC2840): `row_solver` true = retail row solver (false = the engine's older
/// impulse pass); `iterations` (retail 25, 1..=256); `sleep_energy` (retail 1e-5);
/// `sleep_frames` (retail 2, 1..=10000); `max_sleeps_per_step` (retail 100);
/// `rest_snap` (engine snap, not retail, default false).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSolverPatch {
    pub row_solver: Option<bool>,
    pub iterations: Option<u32>,
    pub sleep_energy: Option<f32>,
    pub sleep_frames: Option<u32>,
    pub max_sleeps_per_step: Option<u32>,
    pub rest_snap: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarryPatch {
    /// Packed controller button bit (0..=31) held to grab and carry (retail 28, RB).
    pub grab_bit: Option<u32>,
    /// Bit whose rising edge toggles placement (retail 20, B).
    pub placement_bit: Option<u32>,
    /// Pickup reach in metres to the prop's surface (engine default 2.0).
    pub grab_range: Option<f32>,
    /// Move Object speed pushing forward, m/s at full stick (engine default 1.4).
    pub push_speed: Option<f32>,
    /// Move Object speed pulling back, m/s at full stick (engine default 1.0).
    pub pull_speed: Option<f32>,
    /// Move Object side step, m/s at full stick (engine default 0.8).
    pub side_speed: Option<f32>,
    /// Move Object turn, rad/s at full right stick X (engine default 1.6).
    pub turn_rate: Option<f32>,
    /// Gap in metres between the grab edge and the skater (engine default 0.35).
    pub grip_reach: Option<f32>,
    /// Linear command clamp (retail 20).
    pub linear_clamp: Option<f32>,
    /// Yaw command clamp (retail 6).
    pub yaw_clamp: Option<f32>,
    /// Heading re-latch threshold in rad (retail 0.1).
    pub relatch: Option<f32>,
    /// Max change of the linear command per tick (retail 4).
    pub slew_per_tick: Option<f32>,
    /// Yaw-rate feedback factor (retail 60 = 1/dt, 0x822F860C): facing change per
    /// tick x this = measured yaw rate subtracted from the yaw target; 0 = off.
    pub yaw_rate_feedback: Option<f32>,
    /// Linear controller `[p, filtered, d, filter]` (retail 20, 0, 40, 0.1).
    pub linear_controller: Option<[f32; 4]>,
    /// Yaw controller `[p, filtered, d, filter]` (retail 20, 0, 40, 0.1).
    pub yaw_controller: Option<[f32; 4]>,
    /// Curve |lever| -> rotation demand, `[[8 x], [8 y]]`.
    pub lever_rotation: Option<[[f32; 8]; 2]>,
    /// Curve |lever| -> yaw-rate factor.
    pub lever_yaw: Option<[[f32; 8]; 2]>,
    /// Curve mass -> speed scale.
    pub mass_speed: Option<[[f32; 8]; 2]>,
    /// Curve yaw inertia -> yaw gain.
    pub inertia_yaw_gain: Option<[[f32; 8]; 2]>,
    /// Metres the skater may fall behind its grab point before letting go (engine rule for mods; default 0 = off, retail lets go by record qualification).
    pub let_go_distance: Option<f32>,
    /// Grabbing a prop drops a carried board (retail true, 82D442D0 -> LetGoOfSkateboard 82D75440).
    pub drop_board: Option<bool>,
    /// Max change of the skater follow step per tick in m (retail 0.1, 82BD41B0 in 82D44A10).
    pub follow_step: Option<f32>,
    /// Hold qualification approach angle limit in degrees (retail GrabSplineAngleLimitGrabbing 80).
    pub hold_angle_limit: Option<f32>,
    /// Hold qualification edge slope limit in degrees (retail GrabSplineMaxAngleToHorizontalGrabbing 50).
    pub hold_max_angle_to_horizontal: Option<f32>,
    /// Hold qualification grab box half extents (retail GrabBoxSizeGrabbing 0.9, 0.8, 1.01).
    pub hold_box_extents: Option<[f32; 3]>,
    /// Target speed scale for prop types with record+272 set (retail 2.0).
    pub record_272_speed_scale: Option<f32>,
    /// Friction pair `[static, dynamic]` every held (commanded) prop switches to (retail [0.03, 0.02],
    /// 82C53EF8); combined with the other side by max / max / min (82763078).
    pub commanded_material: Option<[f32; 2]>,
    /// Upright test on the prop's up axis y for the upright free pair (retail 0.65, 82C54B00; -1..1).
    pub upright_cos: Option<f32>,
    /// Linear command at the centre of mass (retail true; false = at the grip point, lever torque).
    pub apply_at_com: Option<bool>,
    /// Yaw command replaces the prop's angular accumulator (retail true; false = added).
    pub yaw_replaces_torque: Option<bool>,
    /// Vertical command dropped (retail true).
    pub ignore_vertical: Option<bool>,
    /// Every command wakes the prop (retail true; false = only a non-zero command).
    pub wake_on_command: Option<bool>,
    /// Per prop type held / free parameter blocks, keyed by the MOBJ template name or by the
    /// type's vault record name (`livingworld_dynamicobject_characteristics`, e.g.
    /// `dt_garbagebin`, logged as `type=` in HELD_PROP); the template name entry wins. Each
    /// field overrides the type's retail value; unset fields keep it.
    #[serde(default)]
    pub by_template: BTreeMap<String, CarryMaterialPatch>,
}

/// Per prop type contact material blocks of `carry.by_template` (friction pairs `[static, dynamic]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarryMaterialPatch {
    /// Friction pair while held (default: `commanded_material`).
    pub material_held: Option<[f32; 2]>,
    /// Free friction pair (retail DMO data +320 / +328 of the type; the prop's authored friction
    /// only when its type data is missing).
    pub material_free: Option<[f32; 2]>,
    /// Free friction pair while upright (retail DMO data +316 / +324; default: `material_free`),
    /// used only when `upright_pair` is set.
    pub material_free_upright: Option<[f32; 2]>,
    /// The free pair depends on the upright test (retail DMO data +312 of the type).
    pub upright_pair: Option<bool>,
    /// Restitution of this type's blocks (retail DMO data +272 of the type; the authored
    /// restitution only when its type data is missing).
    pub restitution: Option<f32>,
    /// Record+272 for this prop type: Move Object target speeds x `record_272_speed_scale`
    /// (retail: set when the type's DMO data +312 is set, 82C4B960).
    pub record_272: Option<bool>,
    /// Linear drag of this prop type's body, per second (retail DMO data +308 `LinearDrag`; the
    /// integrator keeps `1 - drag * dt` of the velocity each fixed step, 60 or more stops it).
    pub linear_drag: Option<f32>,
    /// Angular drag, per second (retail DMO data +336 `AngularDrag`, same rule).
    pub angular_drag: Option<f32>,
    /// Body mass in kg (retail DMO data +304 of the type; the box inertia follows it).
    pub mass: Option<f32>,
    /// Linear speed cap in m/s (retail DMO data +292; the integrator shortens faster velocities).
    pub maximum_linear_velocity: Option<f32>,
    /// Angular speed cap in rad/s (retail DMO data +296, same rule).
    pub maximum_angular_velocity: Option<f32>,
    /// Box inertia shape: the body's half extents x `inertia_scale` + `inertia_offset` (retail DMO
    /// data +16 / +32 of the type; class default 1.2 / 0).
    pub inertia_scale: Option<[f32; 3]>,
    pub inertia_offset: Option<[f32; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowsPatch {
    /// Dynamic shadow floor on the baked world, RGB 0..=1 (retail 0.05, 0.09, 0.13).
    pub world_floor: Option<[f32; 3]>,
}

/// Field-wise "first writer wins": `self` keeps its fields, `later` fills the gaps.
pub trait Merge {
    fn merge(&mut self, later: &Self);
}

macro_rules! merge_opts {
    ($a:ident, $b:ident; $($f:ident),*) => { $( if $a.$f.is_none() { $a.$f = $b.$f.clone(); } )* };
}

impl Merge for SkaterFadePatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; fade_in_seconds, fade_seconds, despawn_alpha);
    }
}
impl Merge for PedObstaclesPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, step_height, held_is_obstacle, moving_solid);
    }
}
impl Merge for PedVehicleContactPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, push);
    }
}
impl Merge for NpcSkaterPropsPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled);
    }
}
impl Merge for SkaterLineChainPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; radius, max_candidates, blend_seconds, keep_facing, facing_rule, steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds, fakie_spawn_seconds);
    }
}
impl Merge for PedFadePatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; distance, fade_in_seconds, enabled);
    }
}
fn merge_nested<T: Merge + Clone>(a: &mut Option<T>, b: &Option<T>) {
    match (a.as_mut(), b) {
        (Some(a), Some(b)) => a.merge(b),
        (None, Some(b)) => *a = Some(b.clone()),
        _ => {}
    }
}
impl Merge for LivingWorldPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; npc_draw_distance);
        merge_nested(&mut self.skater_fade, &b.skater_fade);
        merge_nested(&mut self.skater_line_chain, &b.skater_line_chain);
        merge_nested(&mut self.ped_fade, &b.ped_fade);
        merge_nested(&mut self.ped_obstacles, &b.ped_obstacles);
        merge_nested(&mut self.npc_skater_props, &b.npc_skater_props);
        merge_nested(&mut self.ped_vehicle_contact, &b.ped_vehicle_contact);
        match (self.skater_clips.as_mut(), &b.skater_clips) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert_with(|| v.clone());
            }),
            (None, Some(b)) => self.skater_clips = Some(b.clone()),
            _ => {}
        }
        match (self.skater_blend_seconds.as_mut(), &b.skater_blend_seconds) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert(*v);
            }),
            (None, Some(b)) => self.skater_blend_seconds = Some(b.clone()),
            _ => {}
        }
    }
}
impl Merge for PropTuningPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; contact_padding, penetration_slop, penetration_correction, max_depenetration_per_tick,
            restitution_threshold, skater_push_mass, push_transfer, body_push_speed, board_push_speed,
            penetration_push_speed, stuck_release_ticks, collision_box);
    }
}
impl Merge for PropSolverPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; row_solver, iterations, sleep_energy, sleep_frames, max_sleeps_per_step, rest_snap);
    }
}
impl Merge for PropUprightPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; window_seconds, tick_seconds, stop_angle_deg, max_angle_deg, dead_band_deg, gain_min, gain_max,
            gain_blend_start, off_axis_spin, command_rate, fallback_angle_deg, block_yaw);
    }
}
impl Merge for PropsPatch {
    fn merge(&mut self, b: &Self) {
        merge_nested(&mut self.default, &b.default);
        merge_nested(&mut self.solver, &b.solver);
        merge_nested(&mut self.upright, &b.upright);
        for (k, v) in &b.by_template {
            self.by_template.entry(k.clone()).and_modify(|a| a.merge(v)).or_insert_with(|| v.clone());
        }
    }
}
impl Merge for CarryPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; grab_bit, placement_bit, grab_range, push_speed, pull_speed, side_speed, turn_rate, grip_reach,
            linear_clamp, yaw_clamp, relatch, slew_per_tick, yaw_rate_feedback, linear_controller, yaw_controller, lever_rotation, lever_yaw,
            mass_speed, inertia_yaw_gain, let_go_distance, drop_board, follow_step, hold_angle_limit, hold_max_angle_to_horizontal,
            hold_box_extents, record_272_speed_scale, commanded_material, upright_cos, apply_at_com, yaw_replaces_torque,
            ignore_vertical, wake_on_command);
        for (k, v) in &b.by_template {
            self.by_template.entry(k.clone()).and_modify(|a| { merge_opts!(a, v; material_held, material_free, material_free_upright, upright_pair, restitution, record_272, linear_drag, angular_drag, mass, maximum_linear_velocity, maximum_angular_velocity, inertia_scale, inertia_offset); }).or_insert_with(|| v.clone());
        }
    }
}

impl Merge for ShadowsPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; world_floor);
    }
}

impl LivingWorldPatch {
    pub fn validate(&self) -> bool {
        finite(self.npc_draw_distance)
            && self.skater_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && finite(f.fade_seconds) && finite(f.despawn_alpha))
            && self.skater_line_chain.as_ref().is_none_or(|c| finite(c.radius) && c.max_candidates.is_none_or(|n| n <= 256) && c.blend_seconds.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v))
                && c.facing_rule.as_deref().is_none_or(|r| NPC_SKATER_FACING_RULES.contains(&r))
                && [c.steer_dead_zone_deg, c.steer_full_deg].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=180.0).contains(&v)))
                && [c.fakie_high_speed, c.fakie_low_speed, c.fakie_slow_seconds, c.fakie_spawn_seconds].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(&v))))
            && self.ped_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && f.distance.is_none_or(|d| d.iter().all(|v| finite(Some(*v)))))
            && self.ped_obstacles.as_ref().is_none_or(|o| {
                [o.min_half_extent, o.moving_speed, o.recut_fraction, o.detour_margin, o.step_height].into_iter().all(finite)
            })
            && self.skater_clips.as_ref().is_none_or(|m| {
                m.len() <= MAX_TEMPLATES
                    && m.iter().all(|(k, v)| {
                        let phase = k.split_once('.').map_or(k.as_str(), |(p, style)| if style.is_empty() || style.len() > 64 { "" } else { p });
                        (NPC_SKATER_PHASES.contains(&phase) || k == NPC_SKATER_FAKIE_CHANNEL || (phase == NPC_SKATER_TRICK_GROUP && k.contains('.'))) && !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| b.is_ascii_graphic())
                    })
            })
            && self.skater_blend_seconds.as_ref().is_none_or(|m| {
                m.iter().all(|(k, v)| (k == "default" || NPC_SKATER_PHASES.contains(&k.as_str()) || NPC_SKATER_TRICK_BLENDS.contains(&k.as_str())) && v.is_finite() && (0.0..=MAX_BLEND_SECONDS).contains(v))
            })
    }
}
impl PropTuningPatch {
    pub fn validate(&self) -> bool {
        [self.contact_padding, self.penetration_slop, self.penetration_correction, self.max_depenetration_per_tick,
            self.restitution_threshold, self.skater_push_mass, self.push_transfer, self.body_push_speed,
            self.board_push_speed, self.penetration_push_speed]
            .into_iter()
            .all(finite)
            && self.collision_box.is_none_or(|b| {
                b.center.iter().all(|v| finite(Some(*v))) && b.half_extents.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= MAX_NUMBER)
            })
    }
}
impl PropSolverPatch {
    pub fn validate(&self) -> bool {
        self.iterations.is_none_or(|n| (1..=256).contains(&n))
            && self.sleep_energy.is_none_or(|e| e.is_finite() && (0.0..=MAX_NUMBER).contains(&e))
            && self.sleep_frames.is_none_or(|n| (1..=10_000).contains(&n))
            && self.max_sleeps_per_step.is_none_or(|n| n >= 1)
    }
}
impl PropUprightPatch {
    pub fn validate(&self) -> bool {
        let ok = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(&v));
        let positive = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && v > 0.0 && v <= MAX_NUMBER);
        positive(self.window_seconds)
            && positive(self.tick_seconds)
            && [self.stop_angle_deg, self.max_angle_deg, self.dead_band_deg, self.gain_min, self.gain_max,
                self.gain_blend_start, self.off_axis_spin, self.command_rate, self.fallback_angle_deg]
                .into_iter()
                .all(ok)
    }
}
impl PropsPatch {
    pub fn validate(&self) -> bool {
        self.default.as_ref().is_none_or(PropTuningPatch::validate)
            && self.solver.as_ref().is_none_or(PropSolverPatch::validate)
            && self.upright.as_ref().is_none_or(PropUprightPatch::validate)
            && self.by_template.len() <= MAX_TEMPLATES
            && self.by_template.iter().all(|(k, v)| !k.is_empty() && k.len() <= 128 && v.validate())
    }
}
impl CarryPatch {
    pub fn validate(&self) -> bool {
        self.grab_bit.is_none_or(|b| b < 32)
            && self.placement_bit.is_none_or(|b| b < 32)
            && finite(self.grab_range)
            && [self.push_speed, self.pull_speed, self.side_speed, self.turn_rate, self.grip_reach,
                self.linear_clamp, self.yaw_clamp, self.relatch, self.slew_per_tick, self.yaw_rate_feedback, self.let_go_distance,
                self.follow_step, self.hold_angle_limit, self.hold_max_angle_to_horizontal, self.record_272_speed_scale]
                .into_iter()
                .all(|v| finite(v) && v.is_none_or(|v| v >= 0.0))
            && [self.linear_controller, self.yaw_controller]
                .into_iter()
                .flatten()
                .all(|g| g.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&g[3]))
            && [self.lever_rotation, self.lever_yaw, self.mass_speed, self.inertia_yaw_gain]
                .into_iter()
                .flatten()
                .all(|c| c.iter().flatten().all(|v| v.is_finite()) && c[0].windows(2).all(|p| p[0] <= p[1]))
            && self.hold_box_extents.is_none_or(|e| e.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= MAX_NUMBER))
            && self.commanded_material.is_none_or(material_block)
            && self.upright_cos.is_none_or(|v| (-1.0..=1.0).contains(&v))
            && self.by_template.len() <= MAX_TEMPLATES
            && self.by_template.iter().all(|(k, v)| {
                !k.is_empty()
                    && k.len() <= 128
                    && [v.material_held, v.material_free, v.material_free_upright].into_iter().flatten().all(material_block)
                    && [v.restitution, v.linear_drag, v.angular_drag, v.maximum_linear_velocity, v.maximum_angular_velocity].into_iter().flatten().all(|r| r.is_finite() && (0.0..=MAX_NUMBER).contains(&r))
                    && v.mass.is_none_or(|m| m.is_finite() && m > 0.0 && m <= MAX_NUMBER)
                    && [v.inertia_scale, v.inertia_offset].into_iter().flatten().flatten().all(|x| x.is_finite() && x.abs() <= MAX_NUMBER)
            })
    }
}

/// A friction pair: two finite, non-negative values.
fn material_block(b: [f32; 2]) -> bool {
    b.iter().all(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(v))
}

impl ShadowsPatch {
    pub fn validate(&self) -> bool {
        self.world_floor.is_none_or(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
    }
}

/// A parsed patch of one domain.
#[derive(Clone, Debug, PartialEq)]
pub enum Patch {
    LivingWorld(LivingWorldPatch),
    Props(PropsPatch),
    Carry(CarryPatch),
    Shadows(ShadowsPatch),
}

/// Parse and validate a patch for `domain` (`None` = unknown domain, unknown field or bad value).
pub fn parse(domain: &str, patch: &Value) -> Option<Patch> {
    let p = match domain {
        "living_world" => Patch::LivingWorld(serde_json::from_value(patch.clone()).ok()?),
        "props" => Patch::Props(serde_json::from_value(patch.clone()).ok()?),
        "carry" => Patch::Carry(serde_json::from_value(patch.clone()).ok()?),
        "shadows" => Patch::Shadows(serde_json::from_value(patch.clone()).ok()?),
        _ => return None,
    };
    let ok = match &p {
        Patch::LivingWorld(p) => p.validate(),
        Patch::Props(p) => p.validate(),
        Patch::Carry(p) => p.validate(),
        Patch::Shadows(p) => p.validate(),
    };
    ok.then_some(p)
}

pub fn valid_patch(domain: &str, patch: &Value) -> bool {
    parse(domain, patch).is_some()
}

/// `sdk.engine.inspect(key, "world_tuning:<domain>")`.
pub fn valid_inspect(system: &str) -> bool {
    system.strip_prefix("world_tuning:").is_some_and(|d| DOMAINS.contains(&d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patches_parse_validate_and_reject_unknown_fields() {
        assert!(valid_patch("living_world", &json!({"npc_draw_distance": 2.0, "skater_fade": {"fade_seconds": 3.0}, "ped_fade": {"distance": [80, 100], "enabled": false}})));
        assert!(valid_patch("props", &json!({"default": {"push_transfer": 0.5}, "by_template": {"bench01": {"collision_box": {"center": [0, 0.4, 0], "half_extents": [1, 0.4, 0.3]}}}})));
        assert!(valid_patch("carry", &json!({"grab_bit": 21, "grab_range": 3.5})));
        assert!(valid_patch("living_world", &json!({"skater_clips": {"rolling": "R_IDLE_RIDE_N_0_CYC", "rolling.Aggressive": "X"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"flying": "X"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"air": ""}})));
        assert!(valid_patch("living_world", &json!({"skater_blend_seconds": {"default": 0.3, "air": 0.0}})));
        assert!(!valid_patch("living_world", &json!({"skater_blend_seconds": {"flying": 0.2}})));
        assert!(!valid_patch("living_world", &json!({"skater_blend_seconds": {"air": -0.1}})));
        // Fix 21: trick animations per recorded trick and the trick transition times.
        assert!(valid_patch("living_world", &json!({"skater_clips": {"trick.kickflip": "B_HEELFLIP_IN"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"trick": "B_OLLIE"}})));
        assert!(valid_patch("living_world", &json!({"skater_blend_seconds": {"trick_takeoff": 0.1, "trick_air": 0.0}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"blend_seconds": 0.5}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"blend_seconds": -0.5}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"blend_seconds": 11.0}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"keep_facing": false}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"keep_facing": 1}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"facing_rule": "per_node", "steer_dead_zone_deg": 2.0, "steer_full_deg": 10.0}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"facing_rule": "backwards"}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"steer_full_deg": -1.0}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"fakie_high_speed": 2.0, "fakie_low_speed": 1.0, "fakie_slow_seconds": 0.5, "fakie_spawn_seconds": 0.0}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"fakie_low_speed": -1.0}})));
        assert!(valid_patch("living_world", &json!({"skater_clips": {"fakie_channel": "B_FAKIE_CHANNEL"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"fakie_channel": ""}})));
        assert!(!valid_patch("living_world", &json!({"draw": 2.0})));
        assert!(!valid_patch("living_world", &json!({"skater_fade": {"fade_seconds": 1e9}})));
        assert!(!valid_patch("props", &json!({"by_template": {"b": {"collision_box": {"center": [0, 0, 0], "half_extents": [0, 1, 1]}}}})));
        assert!(!valid_patch("carry", &json!({"grab_bit": 32})));
        assert!(valid_patch("carry", &json!({"push_speed": 2.0, "turn_rate": 0.5})));
        assert!(!valid_patch("carry", &json!({"pull_speed": -1.0})));
        assert!(valid_patch("carry", &json!({"grip_reach": 0.5})));
        assert!(!valid_patch("carry", &json!({"grip_reach": -0.1})));
        assert!(valid_patch("carry", &json!({"commanded_material": [0.1, 0.02], "apply_at_com": false, "wake_on_command": true})));
        assert!(!valid_patch("carry", &json!({"commanded_material": [-0.1, 0.02]})));
        assert!(valid_patch("carry", &json!({"upright_cos": 0.65, "by_template": {"t": {"material_free_upright": [0.9, 0.7], "upright_pair": true, "restitution": 0.2}}})));
        assert!(!valid_patch("carry", &json!({"upright_cos": 1.5})));
        assert!(!valid_patch("carry", &json!({"by_template": {"t": {"material_free_upright": [0.9, -0.7]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"t": {"restitution": -0.1}}})));
        assert!(valid_patch("carry", &json!({"by_template": {"bin": {"material_held": [0.2, 0.0], "material_free": [0.5, 0.1]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"bin": {"material_held": [0.2]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"bin": {"friction": 1.0}}})));
        assert!(!valid_patch("carry", &json!({"apply_at_com": 1})));
        assert!(valid_patch("shadows", &json!({"world_floor": [0.05, 0.09, 0.13]})));
        assert!(!valid_patch("shadows", &json!({"world_floor": [0.05, 0.09]})));
        assert!(!valid_patch("shadows", &json!({"world_floor": [0.05, 0.09, 1.5]})));
        assert!(!valid_patch("shadows", &json!({"floor": [0.0, 0.0, 0.0]})));
        assert!(!valid_patch("roads", &json!({})));
        assert!(valid_inspect("world_tuning:carry") && !valid_inspect("world_tuning:x"));
    }

    #[test]
    fn first_writer_wins_per_field() {
        let Some(Patch::LivingWorld(mut a)) = parse("living_world", &json!({"skater_fade": {"fade_seconds": 2.0}})) else { panic!() };
        let Some(Patch::LivingWorld(b)) = parse("living_world", &json!({"npc_draw_distance": 3.0, "skater_fade": {"fade_seconds": 5.0, "despawn_alpha": 0.1}})) else { panic!() };
        a.merge(&b);
        assert_eq!(a.npc_draw_distance, Some(3.0));
        let f = a.skater_fade.unwrap();
        assert_eq!((f.fade_seconds, f.despawn_alpha, f.fade_in_seconds), (Some(2.0), Some(0.1), None));
    }
}
