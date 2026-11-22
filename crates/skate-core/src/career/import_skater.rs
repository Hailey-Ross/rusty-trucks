//! FE 68 ImportSkater, the preset list (template 114 `TeamSkater_Selection/import_skater`; research b986, Ghidra
//! vtable 82305678): the movie's list shows the skater groups as headers and rows; a pick asks to confirm, Yes loads
//! the row's look into the screen's slot and the screen closes. Details:
//! docs/hails-additions/47-movie-player.md#preset-skaters

/// The boot flow step's mode (FE 67's Presets row: slot 0, mode 2).
pub const MODE_FIRST_TEAM_MEMBER: u32 = 2;
/// The movie's states (its SetState constants).
pub const STATE_UPDATE_SKATERS: i32 = 10;
pub const STATE_UPDATE_PREVIEW_PHOTO: i32 = 11;
pub const STATE_FIRST_TEAMMATE_BACKGROUND: i32 = 20;
pub const STATE_SHOW_GAMERCARD: i32 = 30;
pub const STATE_HIDE_GAMERCARD: i32 = 31;
/// Popup 0x123 (mode 0) and 0x125 (modes 1 and 2) of the FE popup table: title, text, buttons (row 1 = Yes).
pub const CONFIRM_TITLE: &str = "ID_COMMON_WARNING_TITLE";
pub const CONFIRM_IMPORT_TEXT: &str = "ID_IMPORT_SKATER_CONFIRM_IMPORT";
pub const CONFIRM_PRESET_TEXT: &str = "ID_CONFIRM_USE_PRESET";
pub const CONFIRM_BUTTONS: [&str; 2] = ["ID_COMMON_NO", "ID_COMMON_YES"];
pub const CONFIRM_YES: i64 = 1;
/// GetSkaters' rows when every group is empty (825C3E20): a header and one row.
pub const NO_SKATERS: &str = "#NO SKATERS";
pub const NOT_A_SKATER: &str = "#NOT A SKATER";

/// One group of the resource manager's skater lists (82558438), in group order: its title text id and its rows'
/// names (`localized`: the name is a text id; else it is shown as typed, `#name`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub title: String,
    pub rows: Vec<(String, bool)>,
}

/// GetSkaters (825C3E20): [count, texts, header flags]: each group with rows gives its title (a header) and then its
/// rows; nothing at all gives `NO_SKATERS` / `NOT_A_SKATER`.
pub fn rows(groups: &[Group]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for g in groups.iter().filter(|g| !g.rows.is_empty()) {
        out.push((g.title.clone(), true));
        out.extend(g.rows.iter().map(|(name, localized)| (if *localized { name.clone() } else { format!("#{name}") }, false)));
    }
    if out.is_empty() {
        out = vec![(NO_SKATERS.into(), true), (NOT_A_SKATER.into(), false)];
    }
    out
}

/// A list index to (group, row) (82654F10): the index counts each non-empty group's header; a header gives no row.
pub fn locate(groups: &[Group], index: usize) -> Option<(usize, usize)> {
    let mut start = 0;
    for (n, g) in groups.iter().enumerate().filter(|(_, g)| !g.rows.is_empty()) {
        let end = start + g.rows.len() + 1;
        if index < end {
            return index.checked_sub(start + 1).map(|row| (n, row));
        }
        start = end;
    }
    None
}

/// Where the screen is (+0x2F0): idle (0), the pick confirmed and loading (1..0xB), closing (0xC).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Idle,
    Applying,
    Closing,
}

/// The screen's slot (+0x2E8), mode (+0x310), state and pick (+0x2EC).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportSkater {
    pub slot: usize,
    pub mode: u32,
    pub stage: Stage,
    pub pick: Option<usize>,
    /// The confirm popup is up (+0x328).
    pub confirming: bool,
}

impl ImportSkater {
    pub fn new(slot: usize, mode: u32) -> Self {
        Self { slot, mode, ..Default::default() }
    }

    /// A row picked (826536D0): while idle it is kept and the confirm popup's text returned (0x123 in mode 0, 0x125 in
    /// modes 1 and 2, none above).
    pub fn select(&mut self, index: usize) -> Option<&'static str> {
        if self.stage != Stage::Idle || self.confirming {
            return None;
        }
        let text = match self.mode {
            0 => CONFIRM_IMPORT_TEXT,
            1 | 2 => CONFIRM_PRESET_TEXT,
            _ => return None,
        };
        self.pick = Some(index);
        self.confirming = true;
        Some(text)
    }

    /// The popup's answer (82654EA8): Yes starts loading the pick; the popup is gone either way.
    pub fn answer(&mut self, choice: i64) -> bool {
        self.confirming = false;
        if choice == CONFIRM_YES && self.stage == Stage::Idle {
            self.stage = Stage::Applying;
            return true;
        }
        false
    }

    /// The look is in the slot (states 2..0xB): the screen closes.
    pub fn applied(&mut self) {
        self.stage = Stage::Closing;
    }

    /// B (82654270 button 3): pops the screen while idle with no popup.
    pub fn back(&mut self) -> bool {
        if self.stage == Stage::Idle && !self.confirming {
            self.stage = Stage::Closing;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presets() -> Vec<Group> {
        vec![
            Group { title: "ID_CIC_COMMUNITY_SKATERS".into(), rows: vec![] },
            Group { title: "ID_PRESET_LISTING_TITLE".into(), rows: vec![("ID_TEAMMATE_PRESET_01".into(), true), ("Mine".into(), false)] },
        ]
    }

    #[test]
    fn the_list_has_a_header_per_group_with_rows_and_the_index_counts_it() {
        let g = presets();
        assert_eq!(rows(&g), [("ID_PRESET_LISTING_TITLE".into(), true), ("ID_TEAMMATE_PRESET_01".into(), false), ("#Mine".into(), false)]);
        assert_eq!(locate(&g, 0), None, "the header");
        assert_eq!((locate(&g, 1), locate(&g, 2), locate(&g, 3)), (Some((1, 0)), Some((1, 1)), None));
        assert_eq!(rows(&[]), [(NO_SKATERS.into(), true), (NOT_A_SKATER.into(), false)]);
    }

    #[test]
    fn a_pick_asks_first_and_only_yes_applies_it() {
        let mut s = ImportSkater::new(0, MODE_FIRST_TEAM_MEMBER);
        assert_eq!(s.select(3), Some(CONFIRM_PRESET_TEXT));
        assert_eq!(s.select(4), None, "one popup at a time");
        assert!(!s.back(), "B waits for the popup");
        assert!(!s.answer(0));
        assert_eq!(s.stage, Stage::Idle);
        s.select(2);
        assert!(s.answer(CONFIRM_YES));
        assert_eq!((s.stage, s.pick), (Stage::Applying, Some(2)));
        assert!(!s.back(), "busy");
        s.applied();
        assert_eq!(s.stage, Stage::Closing);
        assert_eq!(ImportSkater::new(0, 0).select(1), Some(CONFIRM_IMPORT_TEXT));
        assert_eq!(ImportSkater::new(0, 3).select(1), None);
        assert!(ImportSkater::new(0, 2).back());
    }
}
