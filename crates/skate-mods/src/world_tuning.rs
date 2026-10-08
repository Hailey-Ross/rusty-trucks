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
//!   despawn_alpha}`, `skater_line_chain {radius, max_candidates, blend_seconds, keep_facing}` (NPC skater line end: retail
//!   4 m / 16; root blend onto the new line after a branch or chain, engine default 0.2 s, 0 = cut), `ped_fade {distance = {near, far}, fade_in_seconds, enabled}`,
//!   `skater_clips {[<phase> or <phase>.<style>] = <stock clip name>}` (NPC skater puppet clip per
//!   replay phase; phases in [`NPC_SKATER_PHASES`]; a clip whose name holds `_CYC` loops),
//!   `skater_blend_seconds {[<phase> or default] = seconds}` (NPC skater crossfade into a phase's
//!   clip; stock graph default 0.2 s, 0 = cut; also `trick_takeoff` 0.05 s / `trick_air` 0.1 s),
//!   `skater_clips["trick.<scorable name>"]` = a trick animation base (`<base>_G` / `<base>_A`),
//!   `ped_obstacles {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin,
//!   step_height}` (props and mod bodies as ped navigation obstacles; retail on / 0.2 / 0.4 / 0.25),
//!   `npc_skater_props {enabled}` (NPC skaters push dynamic props like the player; retail on).
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
//!   `mass_speed`, `inertia_yaw_gain` (`[[8 x], [8 y]]`) and `let_go_distance` (m).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DOMAINS: [&str; 3] = ["living_world", "props", "carry"];
/// Upper bound for every number (keeps a typo from building a 1e30 m fade range).
pub const MAX_NUMBER: f32 = 100_000.0;
/// Stable NPC skater replay phase ids (`skate_core::living_world::replay::ReplayPhase::name`).
pub const NPC_SKATER_PHASES: [&str; 6] = ["rolling", "crouched", "air", "air_trick", "ground_trick", "off_board"];
/// `skater_clips` key group for a recorded trick's animation: `trick.<EScorableID name>` (e.g.
/// `trick.kickflip`) = an animation base the NPC plays as `<base>_G` / `<base>_A` (fix 21).
pub const NPC_SKATER_TRICK_GROUP: &str = "trick";
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
/// detour margin, 0 m step height).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedObstaclesPatch {
    pub enabled: Option<bool>,
    pub min_half_extent: Option<f32>,
    pub moving_speed: Option<f32>,
    pub recut_fraction: Option<f32>,
    pub detour_margin: Option<f32>,
    pub step_height: Option<f32>,
}

/// NPC skater line end (`skate_core::living_world::replay::ChainConfig`): continue on an unused
/// line whose start node is within `radius` m (retail 4.0; 0 = fade out at every line end), among
/// at most `max_candidates` (retail 16); `blend_seconds`: the drawn root moves onto the new line
/// over this time after a branch or chain (engine default 0.2 s, 0 = cut, at most 10 s);
/// `keep_facing`: the skater keeps the way it faces (stance side, forward or fakie) across a
/// branch or chain (retail true); false takes the new line's recorded facing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkaterLineChainPatch {
    pub radius: Option<f32>,
    pub max_candidates: Option<u32>,
    pub blend_seconds: Option<f32>,
    pub keep_facing: Option<bool>,
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
    /// Metres the skater may fall behind its grab point before letting go (engine default 1.0).
    pub let_go_distance: Option<f32>,
    /// Parameter block `[a, b]` every held (commanded) prop switches to (retail [0.03, 0.02]).
    pub commanded_material: Option<[f32; 2]>,
    /// Linear command at the centre of mass (retail true; false = at the grip point, lever torque).
    pub apply_at_com: Option<bool>,
    /// Yaw command replaces the prop's angular accumulator (retail true; false = added).
    pub yaw_replaces_torque: Option<bool>,
    /// Vertical command dropped (retail true).
    pub ignore_vertical: Option<bool>,
    /// Every command wakes the prop (retail true; false = only a non-zero command).
    pub wake_on_command: Option<bool>,
    /// Per prop type (MOBJ template name) held / free parameter blocks.
    #[serde(default)]
    pub by_template: BTreeMap<String, CarryMaterialPatch>,
}

/// Per prop type parameter blocks `[a, b]` of `carry.by_template`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarryMaterialPatch {
    /// Block while held (default: `commanded_material`).
    pub material_held: Option<[f32; 2]>,
    /// Block when let go (default: the prop's authored material).
    pub material_free: Option<[f32; 2]>,
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
        merge_opts!(self, b; enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, step_height);
    }
}
impl Merge for NpcSkaterPropsPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled);
    }
}
impl Merge for SkaterLineChainPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; radius, max_candidates, blend_seconds, keep_facing);
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
impl Merge for PropsPatch {
    fn merge(&mut self, b: &Self) {
        merge_nested(&mut self.default, &b.default);
        for (k, v) in &b.by_template {
            self.by_template.entry(k.clone()).and_modify(|a| a.merge(v)).or_insert_with(|| v.clone());
        }
    }
}
impl Merge for CarryPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; grab_bit, placement_bit, grab_range, push_speed, pull_speed, side_speed, turn_rate, grip_reach,
            linear_clamp, yaw_clamp, relatch, slew_per_tick, yaw_rate_feedback, linear_controller, yaw_controller, lever_rotation, lever_yaw,
            mass_speed, inertia_yaw_gain, let_go_distance, commanded_material, apply_at_com, yaw_replaces_torque,
            ignore_vertical, wake_on_command);
        for (k, v) in &b.by_template {
            self.by_template.entry(k.clone()).and_modify(|a| { merge_opts!(a, v; material_held, material_free); }).or_insert_with(|| v.clone());
        }
    }
}

impl LivingWorldPatch {
    pub fn validate(&self) -> bool {
        finite(self.npc_draw_distance)
            && self.skater_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && finite(f.fade_seconds) && finite(f.despawn_alpha))
            && self.skater_line_chain.as_ref().is_none_or(|c| finite(c.radius) && c.max_candidates.is_none_or(|n| n <= 256) && c.blend_seconds.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v)))
            && self.ped_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && f.distance.is_none_or(|d| d.iter().all(|v| finite(Some(*v)))))
            && self.ped_obstacles.as_ref().is_none_or(|o| {
                [o.min_half_extent, o.moving_speed, o.recut_fraction, o.detour_margin, o.step_height].into_iter().all(finite)
            })
            && self.skater_clips.as_ref().is_none_or(|m| {
                m.len() <= MAX_TEMPLATES
                    && m.iter().all(|(k, v)| {
                        let phase = k.split_once('.').map_or(k.as_str(), |(p, style)| if style.is_empty() || style.len() > 64 { "" } else { p });
                        (NPC_SKATER_PHASES.contains(&phase) || (phase == NPC_SKATER_TRICK_GROUP && k.contains('.'))) && !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| b.is_ascii_graphic())
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
impl PropsPatch {
    pub fn validate(&self) -> bool {
        self.default.as_ref().is_none_or(PropTuningPatch::validate)
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
                self.linear_clamp, self.yaw_clamp, self.relatch, self.slew_per_tick, self.yaw_rate_feedback, self.let_go_distance]
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
            && self.commanded_material.is_none_or(material_block)
            && self.by_template.len() <= MAX_TEMPLATES
            && self.by_template.iter().all(|(k, v)| {
                !k.is_empty() && k.len() <= 128 && v.material_held.is_none_or(material_block) && v.material_free.is_none_or(material_block)
            })
    }
}

/// A parameter block: two finite, non-negative values.
fn material_block(b: [f32; 2]) -> bool {
    b.iter().all(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(v))
}

/// A parsed patch of one domain.
#[derive(Clone, Debug, PartialEq)]
pub enum Patch {
    LivingWorld(LivingWorldPatch),
    Props(PropsPatch),
    Carry(CarryPatch),
}

/// Parse and validate a patch for `domain` (`None` = unknown domain, unknown field or bad value).
pub fn parse(domain: &str, patch: &Value) -> Option<Patch> {
    let p = match domain {
        "living_world" => Patch::LivingWorld(serde_json::from_value(patch.clone()).ok()?),
        "props" => Patch::Props(serde_json::from_value(patch.clone()).ok()?),
        "carry" => Patch::Carry(serde_json::from_value(patch.clone()).ok()?),
        _ => return None,
    };
    let ok = match &p {
        Patch::LivingWorld(p) => p.validate(),
        Patch::Props(p) => p.validate(),
        Patch::Carry(p) => p.validate(),
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
        assert!(valid_patch("carry", &json!({"by_template": {"bin": {"material_held": [0.2, 0.0], "material_free": [0.5, 0.1]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"bin": {"material_held": [0.2]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"bin": {"friction": 1.0}}})));
        assert!(!valid_patch("carry", &json!({"apply_at_com": 1})));
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
