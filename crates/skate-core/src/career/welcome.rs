//! The new career's welcome screen, FE 9 WelcomeFlow (research b1007; Ghidra 82609BA0 input, 82609D38 menu pick,
//! 826099C0 close): the difficulty menu, then the Low / High camera pages, then the screen closes. The movie's own
//! menu moves the difficulty row; the camera pages are native. There is no way back.
//! Details: docs/hails-additions/47-movie-player.md#welcome-screen

/// The boot flow step that shows it.
pub const FE_SCREEN: u16 = 9;
/// FE 10: the same movie on its team page (`GetNewTeamState` = 10, "pick a name and a logo"; init 8260A198, input
/// 8260A218: A closes, every key is taken).
pub const FE_TEAM_PROMPT: u16 = 10;

/// Apt key codes the native handler compares (82609BA0): A, Left, Right.
pub const KEY_A: u32 = 2;
pub const KEY_LEFT: u32 = 14;
pub const KEY_RIGHT: u32 = 15;

/// UI sounds the native handler posts (825DFAF0): a choice made, a camera page toggled.
pub const SOUND_SELECT: u64 = 0x373D_38D7_EA16_7A71;
pub const SOUND_TOGGLE: u64 = 0xAD8B_F9A0_B499_7B49;

/// Native state (+0x48): 0 difficulty menu, 1 camera Low, 2 camera High, 3 waiting to close.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Difficulty,
    Low,
    High,
    Closing,
    /// FE 10's team page.
    TeamPrompt,
}

impl State {
    /// The movie state `SetState(n)` shows for it.
    pub fn movie_state(self) -> i32 {
        match self {
            Self::Difficulty => 0,
            Self::Low => 1,
            Self::High => 2,
            Self::Closing => 3,
            Self::TeamPrompt => 10,
        }
    }
}

/// What the host does after an input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effect {
    /// Push this movie state (`SetState`).
    pub movie_state: Option<i32>,
    pub sound: Option<u64>,
    /// The screen closes (retail: state 3 with both camera previews released).
    pub close: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Welcome {
    pub state: State,
    /// Profile +0xB4: 0 Easy, 1 Normal, 2 Hardcore.
    pub difficulty: Option<u8>,
    /// Profile +0xBC: 0 Low, 1 High.
    pub camera: Option<u8>,
}

impl Welcome {
    /// FE 10: the team page only.
    pub fn team_prompt() -> Self {
        Self { state: State::TeamPrompt, ..Self::default() }
    }

    /// `TeamManagement.GetNewTeamState()` for the movie (825C3D08): 10 on the team page, else 0.
    pub fn team_state(&self) -> i32 {
        if self.state == State::TeamPrompt { 10 } else { 0 }
    }

    /// The difficulty menu's pick (82609D38): only in the difficulty state; rows past 2 (the menu's cancel, 255) are
    /// ignored. Shows the Low camera page first.
    pub fn pick(&mut self, row: i64) -> Option<Effect> {
        if self.state != State::Difficulty || !(0..=2).contains(&row) {
            return None;
        }
        self.difficulty = Some(row as u8);
        self.state = State::Low;
        Some(Effect { movie_state: Some(1), sound: Some(SOUND_SELECT), close: false })
    }

    /// A key (82609BA0): `None` = the key goes on to the movie (difficulty and closing states); `Some` = taken by the
    /// camera pages (every key there, most with no effect).
    pub fn key(&mut self, key: u32) -> Option<Effect> {
        let page = match self.state {
            State::Low => 0,
            State::High => 1,
            State::TeamPrompt => {
                return Some(if key == KEY_A {
                    self.state = State::Closing;
                    Effect { movie_state: None, sound: Some(SOUND_SELECT), close: true }
                } else {
                    Effect::default()
                });
            }
            State::Difficulty | State::Closing => return None,
        };
        Some(match key {
            KEY_A => {
                self.camera = Some(page);
                self.state = State::Closing;
                Effect { movie_state: None, sound: Some(SOUND_SELECT), close: true }
            }
            KEY_LEFT | KEY_RIGHT => {
                self.state = if page == 0 { State::High } else { State::Low };
                Effect { movie_state: Some(self.state.movie_state()), sound: Some(SOUND_TOGGLE), close: false }
            }
            _ => Effect::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_then_the_camera_pages_then_a_closes_with_both_choices() {
        let mut w = Welcome::default();
        assert_eq!(w.key(KEY_A), None, "the difficulty menu takes its own keys");
        assert_eq!(w.pick(1).unwrap().movie_state, Some(1));
        assert_eq!(w.key(KEY_RIGHT).unwrap().movie_state, Some(2));
        assert_eq!(w.key(KEY_LEFT).unwrap().movie_state, Some(1));
        assert_eq!(w.key(KEY_RIGHT).unwrap().sound, Some(SOUND_TOGGLE));
        let done = w.key(KEY_A).unwrap();
        assert!(done.close);
        assert_eq!((w.difficulty, w.camera), (Some(1), Some(1)));
        assert_eq!(w.key(KEY_A), None, "closing: keys go to the movie");
    }

    #[test]
    fn the_team_page_answers_10_takes_every_key_and_closes_on_a_without_choices() {
        let mut w = Welcome::team_prompt();
        assert_eq!(w.team_state(), 10);
        assert_eq!(w.pick(1), None);
        assert_eq!(w.key(3), Some(Effect::default()), "B is taken and does nothing");
        assert!(w.key(KEY_A).unwrap().close);
        assert_eq!((w.difficulty, w.camera), (None, None));
        assert_eq!(Welcome::default().team_state(), 0);
    }

    #[test]
    fn there_is_no_way_back_and_cancel_rows_are_ignored() {
        let mut w = Welcome::default();
        assert_eq!(w.pick(255), None);
        assert_eq!(w.state, State::Difficulty);
        w.pick(0);
        assert_eq!(w.pick(2), None, "a second pick in the camera pages is ignored");
        assert_eq!(w.key(3), Some(Effect::default()), "B on a camera page does nothing");
        assert_eq!(w.state, State::Low);
    }
}
