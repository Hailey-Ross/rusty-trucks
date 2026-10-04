//! Declarative mute / replace / layer rules on the game's audio event sites (capability
//! `audio_events` = 2; doc 16 "Rules"). Lua cannot run inside the audio pass, so a mod declares
//! what should happen and the engine applies it at the post site, the same frame:
//!
//! ```json
//! { "match": { "tag": "pop" }, "action": "replace", "play": { "path": "audio/pop.wav", "volume": 0.8 } }
//! ```
//!
//! - `match`: every given field must hold (at least one): `tag` (the event tags: `pop`, `land`,
//!   `grind_start`, `grind_end`, `footstep`, `horn`, `alarm`, `tazer`, `body_fall`, `emitter`),
//!   `kind` (`post`, `splice`, `emitter_start`), `source` (`player`, `world`, `npc`, `emitter`),
//!   `class` (a retail class for posts, a bank for Splice starts and emitters), `slot` (the poster's
//!   slot name: `grind`, `wind`, `footstep`, `horn`, …) and `id` (the slot index for posts, the
//!   sound id for Splice starts, the patch for emitters).
//! - `action`: `mute` (the request is dropped: a post is not made, so its later updates and its
//!   release do nothing; a Splice sound does not start; an emitter keeps its state but posts
//!   nothing), `replace` (mute + `play`), `layer` (the game's sound and `play`).
//! - `play`: the mod's own WAV through the native mixer, non-positional (centred, the retail
//!   non-positional emitter outputs), `volume` 0..1, `pitch` 0.25..4, `reverb` (default true),
//!   `group` `player` (default) or `world`; at most once per `min_interval` seconds (default 0.05)
//!   per rule.
//! - Rules apply while the mod runs (runtime `sdk.audio.rule(key, rule|nil)`, or `audio.json`
//!   `rules`). The first matching rule decides (mods in mod-id order, then rule keys). Event rows
//!   are the game's requests: a muted request is still reported to subscribers.
use serde::{Deserialize, Serialize};

/// Rules one mod may hold (runtime and `audio.json` together) and in all.
pub const MAX_RULES_PER_MOD: usize = 32;
pub const MAX_RULES: usize = 64;
/// The tags a rule can match (the engine's event tags without `zone_change` / `speech`, which
/// are not post sites).
pub const TAGS: [&str; 10] = ["pop", "land", "grind_start", "grind_end", "footstep", "horn", "alarm", "tazer", "body_fall", "emitter"];
pub const KINDS: [&str; 3] = ["post", "splice", "emitter_start"];
pub const SOURCES: [&str; 4] = ["player", "world", "npc", "emitter"];

fn min_interval() -> f32 {
    0.05
}
fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleMatch {
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub id: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Mute,
    Replace,
    Layer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulePlay {
    /// Mod-relative PCM16 WAV (the `sdk.audio.play` rules: 30 s, 8 MiB).
    pub path: String,
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default = "one")]
    pub pitch: f32,
    #[serde(default)]
    pub reverb: Option<bool>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(rename = "match")]
    pub on: RuleMatch,
    pub action: RuleAction,
    #[serde(default)]
    pub play: Option<RulePlay>,
    #[serde(default = "min_interval")]
    pub min_interval: f32,
}

fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.') && !s.contains("..")
}

impl RuleMatch {
    pub fn validate(&self) -> bool {
        let any = self.tag.is_some() || self.kind.is_some() || self.source.is_some() || self.class.is_some() || self.slot.is_some() || self.id.is_some();
        any && self.tag.as_deref().is_none_or(|t| TAGS.contains(&t))
            && self.kind.as_deref().is_none_or(|k| KINDS.contains(&k))
            && self.source.as_deref().is_none_or(|s| SOURCES.contains(&s))
            && self.class.as_deref().is_none_or(word)
            && self.slot.as_deref().is_none_or(word)
    }
}

impl RulePlay {
    pub fn validate(&self) -> bool {
        crate::audio::valid_audio_path(&self.path)
            && self.volume.is_finite() && (0.0..=1.0).contains(&self.volume)
            && self.pitch.is_finite() && (0.25..=4.0).contains(&self.pitch)
            && self.group.as_deref().is_none_or(|g| crate::audio::NATIVE_GROUPS.contains(&g))
    }
}

impl Rule {
    /// The shape: a non-empty match of known names; `play` exactly for `replace` / `layer`.
    pub fn validate(&self) -> bool {
        self.on.validate()
            && self.min_interval.is_finite()
            && (0.0..=10.0).contains(&self.min_interval)
            && match self.action {
                RuleAction::Mute => self.play.is_none(),
                RuleAction::Replace | RuleAction::Layer => self.play.as_ref().is_some_and(RulePlay::validate),
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rules_parse_and_validate() {
        for ok in [
            json!({"match": {"tag": "pop"}, "action": "mute"}),
            json!({"match": {"tag": "pop"}, "action": "replace", "play": {"path": "audio/pop.wav", "volume": 0.8}}),
            json!({"match": {"source": "world", "class": "TRAFFIC_HORN"}, "action": "layer", "play": {"path": "a.wav", "pitch": 1.5, "reverb": false, "group": "world"}, "min_interval": 0.5}),
            json!({"match": {"kind": "splice", "class": "Skate_Collisions", "id": 12}, "action": "mute"}),
            json!({"match": {"kind": "emitter_start", "class": "Baby_Cry_1"}, "action": "mute"}),
        ] {
            let r: Rule = serde_json::from_value(ok.clone()).unwrap();
            assert!(r.validate(), "{ok}");
        }
        for bad in [
            json!({"match": {}, "action": "mute"}),
            json!({"match": {"tag": "zone_change"}, "action": "mute"}),
            json!({"match": {"tag": "pop"}, "action": "mute", "play": {"path": "a.wav"}}),
            json!({"match": {"tag": "pop"}, "action": "replace"}),
            json!({"match": {"tag": "pop"}, "action": "layer", "play": {"path": "../a.wav"}}),
            json!({"match": {"tag": "pop"}, "action": "layer", "play": {"path": "a.wav", "volume": 2}}),
            json!({"match": {"source": "skater"}, "action": "mute"}),
            json!({"match": {"class": "a b"}, "action": "mute"}),
            json!({"match": {"tag": "pop"}, "action": "mute", "min_interval": 20}),
            json!({"match": {"tag": "pop"}, "action": "layer", "play": {"path": "a.wav", "group": "music"}}),
        ] {
            let r: Rule = serde_json::from_value(bad.clone()).unwrap();
            assert!(!r.validate(), "accepted {bad}");
        }
        for typo in [json!({"match": {"tag": "pop"}, "action": "silence"}), json!({"match": {"tags": "pop"}, "action": "mute"}), json!({"match": {"tag": "pop"}, "action": "mute", "when": 1})] {
            assert!(serde_json::from_value::<Rule>(typo.clone()).is_err(), "{typo}");
        }
    }
}
