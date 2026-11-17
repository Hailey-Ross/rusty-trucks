//! FE 67 FirstTeamMember, "Start your team!" (research b1004 §2; Ghidra 82650EC0 menu select, 826513E8 act): three rows,
//! Customize Skater / Preset Skaters / Play Skate 3!. The first pick of a profile without a name asks for it on the
//! keyboard (`ID_KEYBOARD_PROFILE_NAME_HEADER`, default `ID_DEFAULT_PLAYER_NAME`, 16 characters; OK sets profile
//! +0x6C bit 2, cancel stays on the screen). Details: docs/hails-additions/47-movie-player.md#first-team-member

/// The boot flow step that shows it.
pub const FE_SCREEN: u16 = 67;
/// Profile progress bit 2: the player has entered a profile name (82500BC8).
pub const PROGRESS_PROFILE_NAMED: u32 = 4;
pub const PROFILE_NAME_TITLE: &str = "ID_KEYBOARD_PROFILE_NAME_HEADER";
pub const PROFILE_NAME_DEFAULT: &str = "ID_DEFAULT_PLAYER_NAME";
pub const PROFILE_NAME_CALLBACK: &str = "engine:profile_name";

/// The movie's rows in order (its constant pool: `ID_FIRST_TEAM_MEMBER_CUSTOMIZE`, `_PRESETS`, `ID_PLAY_SKATE_3`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// FE 15, the customizer, on slot 0 (the player).
    Customize,
    /// FE 68, the preset list, slot 0, mode 2.
    Presets,
    /// Keep the current skater (posts 0x8F: the step is done).
    Play,
}

impl Choice {
    pub fn from_row(row: i64) -> Option<Self> {
        Some(match row {
            0 => Self::Customize,
            1 => Self::Presets,
            2 => Self::Play,
            _ => return None,
        })
    }
}

/// What a pick leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Ask for the profile name first; the choice runs after OK.
    AskProfileName(Choice),
    Run(Choice),
}

/// A row picked (82650EC0): unknown rows (the menu's cancel) do nothing; without a profile name the keyboard comes
/// first.
pub fn select(row: i64, progress: u32) -> Option<Next> {
    let choice = Choice::from_row(row)?;
    Some(if progress & PROGRESS_PROFILE_NAMED == 0 { Next::AskProfileName(choice) } else { Next::Run(choice) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_pick_asks_for_the_profile_name_and_later_ones_run_at_once() {
        assert_eq!(select(2, 0), Some(Next::AskProfileName(Choice::Play)));
        assert_eq!(select(0, PROGRESS_PROFILE_NAMED), Some(Next::Run(Choice::Customize)));
        assert_eq!(select(1, PROGRESS_PROFILE_NAMED | 2), Some(Next::Run(Choice::Presets)));
        assert_eq!(select(255, 0), None);
    }
}
