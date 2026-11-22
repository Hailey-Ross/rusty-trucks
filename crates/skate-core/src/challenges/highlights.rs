//! A challenge's highlights and its highlight reel (research b1050, b933): each completed objective with a
//! HighlightDefinition keeps one replay window; after a film the reel plays the windows of the result's tier, each
//! as a replay NIS scene. Plain data, engine free; the frames are the replay's (1/60 s).
//! Details: docs/hails-additions/42-challenges.md#highlight-reel

/// One half of a `tHighlightDefinition` (0x40 bytes at +0x10 owned / +0x50 killed): the NIS scene the clip plays
/// and, when `located`, where the scene sits in the world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneDef {
    pub scene: String,
    pub located: bool,
    pub position: [f32; 3],
    pub direction: [f32; 3],
}

/// An objective's `tHighlightDefinition`: seconds before the sequence began and after the completion, and the scene
/// of each tier.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HighlightDefinition {
    pub pre: f32,
    pub post: f32,
    pub owned: SceneDef,
    pub killed: SceneDef,
}

/// One kept window: replay frames `start..=end` and the scene that shows them. `killed` is the tier (the graph's
/// OwnedHighlightIDs / KilledItHighlightIDs, by the completed objective's group), apart from the scene half.
#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub killed: bool,
    pub start: u32,
    pub end: u32,
    pub scene: SceneDef,
}

/// The values a mod may change; retail's by default.
#[derive(Clone, Debug, PartialEq)]
pub struct Rules {
    /// The replay tracker's protected ranges (82BCDAA8: 4 slots).
    pub slots: usize,
    /// Seconds to replay frames (the constant 60.0 at 821FF080).
    pub frames_per_second: f32,
}

impl Default for Rules {
    fn default() -> Self {
        Self { slots: 4, frames_per_second: 60.0 }
    }
}

/// The challenge's highlight group (cHighlightGroup).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Highlights {
    pub rules: Rules,
    list: Vec<Highlight>,
}

impl Highlights {
    pub fn new(rules: Rules) -> Self {
        Self { rules, list: Vec::new() }
    }

    /// AddHighlightUsingDefinition (82BCCEB0 -> 82BCDAA8): the window from the sequence's first frame minus `pre`
    /// seconds to the completion frame plus `post` seconds (an override above 0 replaces the definition's), on the
    /// definition's +0x50 half when `second_half` (Lua arg 3), else +0x10, kept in tier `killed`. With every slot
    /// taken, `drop_oldest` makes room, else nothing is added. True when added.
    #[allow(clippy::too_many_arguments)]
    pub fn add(&mut self, def: &HighlightDefinition, killed: bool, second_half: bool, begin: u32, completed: u32,
        drop_oldest: bool, pre_override: f32, post_override: f32) -> bool {
        if self.rules.slots == 0 {
            return false;
        }
        if self.list.len() >= self.rules.slots {
            if !drop_oldest {
                return false;
            }
            self.list.remove(0);
        }
        let pre = if pre_override > 0.0 { pre_override } else { def.pre };
        let post = if post_override > 0.0 { post_override } else { def.post };
        let frames = |s: f32| (s.max(0.0) * self.rules.frames_per_second) as u32;
        let start = begin.min(completed).saturating_sub(frames(pre));
        let end = completed.saturating_add(frames(post));
        let scene = if second_half { def.killed.clone() } else { def.owned.clone() };
        self.list.push(Highlight { killed, start, end, scene });
        true
    }

    /// RemoveAllHighlights.
    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn list(&self) -> &[Highlight] {
        &self.list
    }

    /// The HighlightReel state's enter (objectives_shared highlight_reel.stateenter@0-168, single player): the other
    /// tier's highlights go and the rest play in order (empty: the reel completes at once). The dummy highlight from
    /// the first objective's definition is SimulPlay only (@26-72, @99-144; not ported).
    pub fn reel(&mut self, killed: bool) -> Vec<Highlight> {
        self.list.retain(|h| h.killed == killed);
        self.list.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(pre: f32, post: f32) -> HighlightDefinition {
        HighlightDefinition {
            pre,
            post,
            owned: SceneDef { scene: "highlight_film".into(), ..Default::default() },
            killed: SceneDef { scene: "highlight_film_killed".into(), ..Default::default() },
        }
    }

    #[test]
    fn a_window_runs_from_pre_before_the_sequence_to_post_after_the_completion() {
        let mut h = Highlights::default();
        assert!(h.add(&def(3.0, 0.5), false, false, 1000, 1300, false, 0.0, 0.0));
        assert_eq!((h.list()[0].start, h.list()[0].end, h.list()[0].scene.scene.as_str()), (820, 1330, "highlight_film"));
        assert!(h.add(&def(3.0, 0.5), true, true, 100, 200, false, 1.0, 2.0), "overrides above 0 replace pre / post");
        assert_eq!((h.list()[1].start, h.list()[1].end, h.list()[1].scene.scene.as_str()), (40, 320, "highlight_film_killed"));
        assert!(h.add(&def(10.0, 0.0), false, false, 60, 90, false, 0.0, 0.0));
        assert_eq!(h.list()[2].start, 0, "never before frame 0");
    }

    #[test]
    fn four_slots_then_the_oldest_goes_only_when_asked() {
        let mut h = Highlights::default();
        for i in 0..4 {
            assert!(h.add(&def(0.0, 0.0), false, false, i * 10, i * 10, false, 0.0, 0.0));
        }
        assert!(!h.add(&def(0.0, 0.0), false, false, 50, 50, false, 0.0, 0.0), "full, no drop");
        assert!(h.add(&def(0.0, 0.0), false, false, 50, 50, true, 0.0, 0.0));
        assert_eq!(h.list().iter().map(|x| x.start).collect::<Vec<_>>(), [10, 20, 30, 50]);
    }

    #[test]
    fn the_reel_keeps_its_tier_and_adds_no_dummy_in_single_player() {
        let mut h = Highlights::default();
        h.add(&def(1.0, 0.0), false, false, 600, 900, false, 0.0, 0.0);
        h.add(&def(1.0, 0.0), true, false, 1200, 1500, false, 0.0, 0.0);
        let reel = h.reel(true);
        assert_eq!(reel.len(), 1);
        assert!(reel[0].killed, "the tier is the objective's group");
        assert_eq!(reel[0].scene.scene, "highlight_film", "the scene half is Lua arg 3, apart from the tier");
        assert!(Highlights::default().reel(false).is_empty(), "nothing to play: the reel completes at once");
    }
}
