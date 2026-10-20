//! The instant replay screen's natives (FE 12, template Replay2, `replay2/replay_editor2`; research b385 / b390 /
//! b391): `FEReplayMenu.*` answered from a per-frame snapshot of the replay ([`ReplayScreenFeed`], published by
//! `replay.rs`), and the screen states the C++ posts back (the facade's SetScreenState, applied by
//! `retail_menu_stage` after the call returns), as the Trick Guide host does.
use crate::apt_vm::{Value, Vm};

/// Movie states the C++ posts (b386 / b390): cursor moved, play start / stop, panel hide / show.
pub mod movie_state {
    pub const CURSOR: i32 = 999;
    pub const PLAY: i32 = 18;
    pub const STOP: i32 = 19;
    pub const HIDE: i32 = 1002;
    pub const SHOW: i32 = 1003;
}

/// What the screen reads from the replay each frame (local player only; a serialisable snapshot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayScreenFeed {
    /// The replay is open in instant mode (retail FE 12 byte +0x82, `IsInstantReplayMode` 825ADFC0).
    pub instant: bool,
    /// Cursor and playhead as a fraction of the recording, 0..1.
    pub position: f64,
    pub playing: bool,
    /// The panel (prompts and timeline) is shown.
    pub panel: bool,
    /// The recording's length, s (`GetEncodedLength` 825AE488 reads it as a whole number).
    pub length_seconds: u32,
}

/// Retail `IsEncodedLengthValid` (825ADF88): valid while the length is under 0x3D (61) seconds.
pub const SHARE_LIMIT_SECONDS: u32 = 60;

#[derive(Clone, Debug, Default)]
pub struct ReplayMenuHost {
    /// The screen is open (its natives are ours).
    pub open: bool,
    pub feed: ReplayScreenFeed,
    /// The feed last posted to the movie (edges become states).
    posted: Option<ReplayScreenFeed>,
    /// Screen states to post to the movie, in order.
    pub states: Vec<i32>,
    /// The movie called `FEReplayMenu.Ready`.
    pub ready: bool,
    /// The natives the movie called since the screen opened (diagnostics, tests).
    pub seen: Vec<String>,
}

impl ReplayMenuHost {
    pub fn reset(&mut self) {
        let feed = std::mem::take(&mut self.feed);
        *self = Self { feed, ..Default::default() };
    }

    /// Queue the states for what changed since the last post: play / stop and panel edges (never on the first post:
    /// the movie opens paused with its panel shown; a first-frame panel state hid it, verify run 2026-10-20), then
    /// the cursor. NOT RETAIL YET: which of 1002 / 1003 hides is not proven, the panel toggle (L3) is not ported.
    pub fn post_changes(&mut self) {
        let last = self.posted.clone();
        let feed = &self.feed;
        if last.as_ref().is_some_and(|l| l.playing != feed.playing) || (last.is_none() && feed.playing) {
            self.states.push(if feed.playing { movie_state::PLAY } else { movie_state::STOP });
        }
        if last.as_ref().is_some_and(|l| l.panel != feed.panel) {
            self.states.push(if feed.panel { movie_state::SHOW } else { movie_state::HIDE });
        }
        if last.as_ref().is_none_or(|l| l.position != feed.position || l.length_seconds != feed.length_seconds) {
            self.states.push(movie_state::CURSOR);
        }
        self.posted = Some(self.feed.clone());
    }

    /// The length text: `ID_REPLAY_EDITOR_LENGTH` with the length. NOT RETAIL YET: the `{0:time}` formatter is not
    /// decoded, ours passes m:ss.
    fn length_text(&self, language: &std::collections::BTreeMap<String, String>) -> String {
        let s = self.feed.length_seconds;
        let template = crate::fe_language::template(language, "ID_REPLAY_EDITOR_LENGTH");
        let value = format!("{}:{:02}", s / 60, s % 60);
        template.replacen("{0:time}", &value, 1)
    }

    /// One native call; None = not ours.
    pub fn call(&mut self, _vm: &mut Vm, language: &std::collections::BTreeMap<String, String>, object: &str, method: &str) -> Option<Value> {
        if object != "FEReplayMenu" || !self.open {
            return None;
        }
        if !self.seen.iter().any(|m| m == method) {
            self.seen.push(method.to_owned());
        }
        let feed = &self.feed;
        Some(match method {
            "Ready" => {
                self.ready = true;
                Value::Undefined
            }
            "IsInstantReplayMode" => Value::Bool(feed.instant),
            "RefreshCursor" => Value::Undefined,
            "GetCursorPosition" | "GetPlayheadPosition" => Value::Number(feed.position.clamp(0.0, 1.0)),
            "GetEncodedLength" => Value::Text(self.length_text(language)),
            "IsEncodedLengthValid" => Value::Bool(feed.length_seconds <= SHARE_LIMIT_SECONDS),
            // Ours on PC: the photo item shows (b390 table 2).
            "CanSaveMedia" => Value::Bool(true),
            // NOT RETAIL YET: true in instant mode (b390 table 2, guess).
            "CanDisplayShowHide" => Value::Bool(feed.instant),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natives_answer_from_the_feed_and_edges_become_states() {
        let mut vm = Vm::new();
        let language = std::collections::BTreeMap::from([("ID_REPLAY_EDITOR_LENGTH".to_owned(), "Estimated Video Length: {0:time}".to_owned())]);
        let mut host = ReplayMenuHost { open: true, ..Default::default() };
        host.feed = ReplayScreenFeed { instant: true, position: 0.5, playing: false, panel: true, length_seconds: 61 };
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "IsInstantReplayMode"), Some(Value::Bool(true)));
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "GetCursorPosition"), Some(Value::Number(0.5)));
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "GetEncodedLength"), Some(Value::Text("Estimated Video Length: 1:01".into())));
        // 825ADF88: valid under 61 s.
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "IsEncodedLengthValid"), Some(Value::Bool(false)));
        host.feed.length_seconds = 60;
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "IsEncodedLengthValid"), Some(Value::Bool(true)));
        assert_eq!(host.call(&mut vm, &language, "Game", "IsInstantReplayMode"), None);
        host.post_changes();
        assert_eq!(std::mem::take(&mut host.states), vec![movie_state::CURSOR], "opens paused with its panel shown");
        host.post_changes();
        assert!(host.states.is_empty(), "nothing changed");
        host.feed.playing = true;
        host.feed.position = 0.6;
        host.post_changes();
        assert_eq!(host.states, vec![movie_state::PLAY, movie_state::CURSOR]);
        host.open = false;
        assert_eq!(host.call(&mut vm, &language, "FEReplayMenu", "Ready"), None, "closed: not ours");
    }
}
