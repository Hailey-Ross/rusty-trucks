//! The NPC skater controller's sub-modes for the obstacle avoider's modes 6 and 7 (retail TU3, evidence only;
//! re-implemented; `.local/research/npc/b66-npc-modes-6-7.md`, main checked `sub_8246F818` cases 3 / 5):
//! - mode 6 (a low prop within 1 s): `sub_8246D560` sets controller sub-mode 4 (`ctrl+568`) when below 4; while 4
//!   the recorded node action is dropped, the speed shape is skipped and no grab / trick is posted;
//! - mode 7 (blocked by a prop within 1.5 s, cap under 0.1): `sub_8246F938` (not airborne) sets sub-mode 3 and a
//!   one-shot reposition request (`ctrl+941`) on the first tick; a second tick still in mode 7 sets sub-mode 5,
//!   remembers the prop's path node (`ctrl+756`) and posts `WipeOutRequest` 1.0 once (key 0x830BECC4); later ticks
//!   post nothing;
//! - `sub_8246FE38` at the end of every controller tick: a pending request (offline only) re-places the skater on
//!   its line, at most once per 60 clock units: in sub-mode 3 at cursor + 5 (or the prop's node when further in
//!   mode 7), else at the cursor; then + 2; a remembered prop node + 2 within 1..19 nodes ahead wins; clamped to the
//!   line. A successful move resets the controller (sub-mode 0). The request is cleared either way.
//!
//! Multiplayer: [`ControllerState`] is plain per-NPC data; the host runs it.

/// Retail numbers; every field is data a mod can override.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerSettings {
    /// Nodes ahead of the cursor for the sub-mode 3 reposition (5).
    pub reposition_ahead: u32,
    /// Nodes past the chosen node (2).
    pub reposition_past: u32,
    /// A remembered prop node wins when it lies 1..this nodes past the choice (20, exclusive).
    pub remembered_window: u32,
    /// Clock units between two repositions (60; the clock unit is not decoded, b66).
    pub reposition_interval: u64,
    /// The `WipeOutRequest` value (1.0, `0x8231A844`).
    pub wipe_out: f32,
}

impl Default for ControllerSettings {
    fn default() -> Self {
        Self { reposition_ahead: 5, reposition_past: 2, remembered_window: 20, reposition_interval: 60, wipe_out: 1.0 }
    }
}

/// `ctrl+568` values used here.
pub mod sub_mode {
    pub const NORMAL: u8 = 0;
    pub const STEP_OFF: u8 = 3;
    pub const LOW_PROP: u8 = 4;
    pub const STEP_OFF_BAIL: u8 = 5;
}

/// The controller fields this logic owns (`+568`, `+941`, `+943` / `+756`, `+912`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControllerState {
    pub sub_mode: u8,
    pub reposition_request: bool,
    pub remembered_node: Option<u32>,
    pub last_reposition: Option<u64>,
}

/// `sub_8246D560` for mode 6: enter sub-mode 4 (its exit is not decoded yet, b66).
pub fn enter_low_prop(state: &mut ControllerState) {
    if state.sub_mode < sub_mode::LOW_PROP {
        state.sub_mode = sub_mode::LOW_PROP;
    }
}

/// `sub_8246F938` for one tick; returns the `WipeOutRequest` value to post, if any. `online` is the flag at
/// `0x830CFE5C`+608 (meaning open; offline 0 runs the reposition step first).
pub fn step_off(state: &mut ControllerState, s: &ControllerSettings, mode_7: bool, airborne: bool, online: bool, prop_node: u32) -> Option<f32> {
    if !mode_7 || airborne {
        return None;
    }
    if !online && state.sub_mode < sub_mode::STEP_OFF {
        state.sub_mode = sub_mode::STEP_OFF;
        state.reposition_request = true;
        return None;
    }
    if state.sub_mode < sub_mode::STEP_OFF_BAIL {
        state.sub_mode = sub_mode::STEP_OFF_BAIL;
        state.remembered_node = Some(prop_node);
        return Some(s.wipe_out);
    }
    None
}

/// `sub_8246FE38`'s node choice.
pub fn reposition_node(state: &ControllerState, s: &ControllerSettings, mode_7: bool, cursor: u32, prop_node: u32, node_count: u32) -> u32 {
    let mut node = cursor;
    if state.sub_mode == sub_mode::STEP_OFF {
        let mut d = s.reposition_ahead;
        if mode_7 && prop_node > cursor && prop_node - cursor > d {
            d = prop_node - cursor;
        }
        node = cursor + d;
    }
    node += s.reposition_past;
    if let Some(r) = state.remembered_node {
        let r = r + s.reposition_past;
        if r > node && r - node < s.remembered_window {
            node = r;
        }
    }
    node.min(node_count.saturating_sub(1))
}

/// `sub_8246FE38`: the node to move to when the one-shot request may run now (offline, throttled); the request is
/// cleared either way. The caller reports success with [`reposition_done`].
pub fn take_reposition(state: &mut ControllerState, s: &ControllerSettings, online: bool, now: u64, mode_7: bool, cursor: u32, prop_node: u32, node_count: u32) -> Option<u32> {
    let pending = std::mem::take(&mut state.reposition_request);
    if !pending || online || state.last_reposition.is_some_and(|last| now < last + s.reposition_interval) {
        return None;
    }
    let node = reposition_node(state, s, mode_7, cursor, prop_node, node_count);
    state.last_reposition = Some(now);
    Some(node)
}

/// A successful move resets the controller (sub-mode 0; the throttle time is kept).
pub fn reposition_done(state: &mut ControllerState) {
    *state = ControllerState { last_reposition: state.last_reposition, ..Default::default() };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_7_requests_a_reposition_then_wipes_out_once() {
        let s = ControllerSettings::default();
        let mut c = ControllerState::default();
        // Airborne: nothing.
        assert_eq!(step_off(&mut c, &s, true, true, false, 40), None);
        assert_eq!(c, ControllerState::default());
        assert_eq!(step_off(&mut c, &s, true, false, false, 40), None);
        assert!(c.reposition_request && c.sub_mode == sub_mode::STEP_OFF);
        assert_eq!(step_off(&mut c, &s, true, false, false, 40), Some(1.0));
        assert_eq!((c.sub_mode, c.remembered_node), (sub_mode::STEP_OFF_BAIL, Some(40)));
        assert_eq!(step_off(&mut c, &s, true, false, false, 40), None);
        // Online: straight to the wipe out.
        let mut c = ControllerState::default();
        assert_eq!(step_off(&mut c, &s, true, false, true, 40), Some(1.0));
    }

    #[test]
    fn the_reposition_node_follows_the_retail_choice() {
        let s = ControllerSettings::default();
        let step3 = ControllerState { sub_mode: sub_mode::STEP_OFF, ..Default::default() };
        assert_eq!(reposition_node(&step3, &s, true, 10, 12, 100), 17);
        assert_eq!(reposition_node(&step3, &s, true, 10, 30, 100), 32);
        assert_eq!(reposition_node(&step3, &s, true, 10, 30, 20), 19);
        let other = ControllerState { sub_mode: sub_mode::NORMAL, remembered_node: Some(20), ..Default::default() };
        assert_eq!(reposition_node(&other, &s, false, 10, 0, 100), 22);
        let far = ControllerState { remembered_node: Some(40), ..other };
        assert_eq!(reposition_node(&far, &s, false, 10, 0, 100), 12);
    }

    #[test]
    fn the_reposition_is_one_shot_and_throttled() {
        let s = ControllerSettings::default();
        let mut c = ControllerState { sub_mode: sub_mode::STEP_OFF, reposition_request: true, ..Default::default() };
        assert_eq!(take_reposition(&mut c, &s, false, 100, true, 10, 12, 100), Some(17));
        reposition_done(&mut c);
        assert_eq!((c.sub_mode, c.last_reposition), (sub_mode::NORMAL, Some(100)));
        c.reposition_request = true;
        assert_eq!(take_reposition(&mut c, &s, false, 130, false, 10, 12, 100), None);
        assert!(!c.reposition_request);
        c.reposition_request = true;
        assert_eq!(take_reposition(&mut c, &s, false, 160, false, 10, 12, 100), Some(12));
        // Online: never.
        c.reposition_request = true;
        assert_eq!(take_reposition(&mut c, &s, true, 1000, false, 10, 12, 100), None);
    }

    #[test]
    fn mode_6_enters_low_prop_without_lowering_a_higher_sub_mode() {
        let mut c = ControllerState::default();
        enter_low_prop(&mut c);
        assert_eq!(c.sub_mode, sub_mode::LOW_PROP);
        let mut c = ControllerState { sub_mode: sub_mode::STEP_OFF_BAIL, ..Default::default() };
        enter_low_prop(&mut c);
        assert_eq!(c.sub_mode, sub_mode::STEP_OFF_BAIL);
    }
}
