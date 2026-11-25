//! The photo challenges' shot picker, FE screen 2 PhotoSelect (research b1060, b1061): the player steps the photo
//! NIS's shots, adjusts brightness / contrast / saturation / depth of field from the edit tables and takes the photo.
//! Pure logic: the host feeds FE actions and update time, and carries out the outputs (the NIS shot, the edits on the
//! view, sounds, the movie state, the store, the end).

/// The edit tables (Attrib collection `5CFA2765DFA23B43` in skatercollections; the export's `photo_edits`). A mod may
/// replace any of them; each reset picks the entry equal to 0.0 (or the first).
#[derive(Clone, Debug, PartialEq)]
pub struct EditTables {
    /// Field `43FA37C2C8D671C4`.
    pub brightness: Vec<f32>,
    /// Field `CE69A39D6B442E64`.
    pub contrast: Vec<f32>,
    /// Field `B5CDB584810D2AF9`.
    pub saturation: Vec<f32>,
    /// Field `F35B9698B1AD85CB` (cycles).
    pub depth_of_field: Vec<f32>,
}

/// Timer lengths (b1060 section 1 / 4): the online pick timer in updates and the store delay after a take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub online_pick_updates: u32,
    pub store_countdown: f32,
}

impl Default for Timing {
    fn default() -> Self {
        Self { online_pick_updates: 1200, store_countdown: 400.0 }
    }
}

/// FE actions the screen answers (82617458; the FE pad map: L2 / R2 step, L1 / R1 saturation, d-pad left / right
/// brightness, up / down contrast, X / A takes; retail PS3 recording 2026-11-16 23:40).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Next,
    Previous,
    BrightnessUp,
    BrightnessDown,
    ContrastUp,
    ContrastDown,
    SaturationUp,
    SaturationDown,
    DepthOfField,
    Reset,
    Take,
}

impl Action {
    /// The retail FE action id.
    pub fn from_fe(id: u32) -> Option<Self> {
        Some(match id {
            7 => Self::Next,
            10 => Self::Previous,
            0x10 => Self::BrightnessUp,
            0x11 => Self::BrightnessDown,
            0x13 => Self::ContrastUp,
            0x12 => Self::ContrastDown,
            6 => Self::SaturationUp,
            9 => Self::SaturationDown,
            5 => Self::DepthOfField,
            4 => Self::Reset,
            2 => Self::Take,
            _ => return None,
        })
    }
}

/// FE sounds by hash (Audio_PlayFeSound).
pub mod sound {
    pub const STEP: u64 = 0x1687_7EBA_E9E4_1263;
    pub const BRIGHTNESS: u64 = 0x2D9A_071B_FF5F_F44A;
    pub const CONTRAST: u64 = 0x7CBA_47FC_E701_1694;
    pub const SATURATION: u64 = 0xFA25_AAE6_95D9_AC29;
    pub const DEPTH_OF_FIELD: u64 = 0xC405_6A53_2848_A417;
    pub const TAKE: u64 = 0x2255_1FF6_CAD4_B8E0;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// cMsgNISPhotoSelect: the NIS shows shot `index` (82829C70 seeks the n-th FREEZE_CMD 4 point).
    Select(u32),
    /// The movie's state (vt+0x14): -1 / -2 the next / previous arrow, -3 the take glow, -10 the fake outro, else the
    /// new shot index.
    MovieState(i32),
    Sound(u64),
    Brightness(f32),
    Contrast(f32),
    Saturation(f32),
    DepthOfField(f32),
    /// cMsgNISPhotoReset (the edits back to their 0.0 entries).
    Reset,
    /// The row has a photo layout: MagazinePromo (FE 69) opens; its close re-enters the picker (`reenter`).
    OpenMagazinePromo,
    /// The store (82618290): the career photo of the picked shot under the row's slot.
    Store,
    /// The finish (82618428): cMsgNISComplete and cMsgPresTeamBrandRefresh, then by mode: the end options follow
    /// ([`FinishMode::Normal`]), the challenge continues or restarts.
    Finish(FinishMode),
}

/// PhotoSelect +0x64, written by MagazinePromo (research b1067).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FinishMode {
    /// The end options (FE 4).
    #[default]
    Normal,
    /// MagazinePromo's Continue: cMsgHUDContinue, no end options.
    Continue,
    /// MagazinePromo's Retry (confirmed): cMsgHUDRetry and a restart; no photo is stored.
    Retry,
}

/// What closed MagazinePromo (8264EA88 / 8264E9E8 / 8264F1B0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagazineChoice {
    /// A: finish mode Continue, the store follows.
    Continue,
    /// Y: back to the picker to choose again.
    Edit,
    /// B, then Yes on the confirm popup: finish mode Retry.
    Retry,
}

#[derive(Clone, Debug)]
pub struct PhotoSelect {
    pub tables: EditTables,
    pub timing: Timing,
    shots: u32,
    index: u32,
    brightness: usize,
    contrast: usize,
    saturation: usize,
    depth_of_field: usize,
    busy: bool,
    online: bool,
    has_layout: bool,
    store_in: Option<f32>,
    online_left: u32,
    stored: bool,
    mode: FinishMode,
}

fn zero_index(values: &[f32]) -> usize {
    values.iter().position(|&v| v == 0.0).unwrap_or(0)
}

impl PhotoSelect {
    /// FrontEnd.OpenPhotoSelection(n) (828544E0) and the screen's first enter (82616EC0): shot 0, edits reset, the NIS
    /// told to show shot 0. `has_layout`: the row's photo layout field (a magazine cover / promo).
    pub fn open(shots: u32, tables: EditTables, timing: Timing, online: bool, has_layout: bool) -> (Self, Vec<Out>) {
        let mut s = Self {
            shots, index: 0, brightness: 0, contrast: 0, saturation: 0, depth_of_field: 0, busy: false, online, has_layout,
            store_in: None, online_left: timing.online_pick_updates, stored: false, mode: FinishMode::Normal, tables, timing,
        };
        s.reset_indices();
        (s, vec![Out::Select(0)])
    }

    pub fn index(&self) -> u32 {
        self.index
    }
    pub fn shots(&self) -> u32 {
        self.shots
    }
    /// More than one shot to step through (+0x58).
    pub fn selection_available(&self) -> bool {
        self.shots > 1
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
    /// The current edit values (brightness, contrast, saturation, depth of field).
    pub fn edits(&self) -> [f32; 4] {
        let at = |t: &[f32], i: usize| t.get(i).copied().unwrap_or(0.0);
        [at(&self.tables.brightness, self.brightness), at(&self.tables.contrast, self.contrast),
            at(&self.tables.saturation, self.saturation), at(&self.tables.depth_of_field, self.depth_of_field)]
    }

    fn reset_indices(&mut self) {
        self.brightness = zero_index(&self.tables.brightness);
        self.contrast = zero_index(&self.tables.contrast);
        self.saturation = zero_index(&self.tables.saturation);
        self.depth_of_field = zero_index(&self.tables.depth_of_field);
    }

    /// One FE action (82617458): ignored once the photo is taken.
    pub fn action(&mut self, action: Action) -> Vec<Out> {
        let mut out = Vec::new();
        if self.busy {
            return out;
        }
        let before = self.index;
        let up = |i: &mut usize, n: usize| *i = (*i + 1).min(n.saturating_sub(1));
        let down = |i: &mut usize| *i = i.saturating_sub(1);
        match action {
            Action::Next | Action::Previous if self.selection_available() => {
                self.index = if action == Action::Next {
                    if self.index + 1 > self.shots - 1 { 0 } else { self.index + 1 }
                } else if self.index == 0 {
                    self.shots - 1
                } else {
                    self.index - 1
                };
                out.push(Out::Select(self.index));
                out.push(Out::Sound(sound::STEP));
                out.push(Out::MovieState(if action == Action::Next { -1 } else { -2 }));
            }
            Action::Next | Action::Previous => {}
            Action::BrightnessUp | Action::BrightnessDown => {
                out.push(Out::Sound(sound::BRIGHTNESS));
                if action == Action::BrightnessUp { up(&mut self.brightness, self.tables.brightness.len()) } else { down(&mut self.brightness) }
                out.push(Out::Brightness(self.edits()[0]));
            }
            Action::ContrastUp | Action::ContrastDown => {
                out.push(Out::Sound(sound::CONTRAST));
                if action == Action::ContrastUp { up(&mut self.contrast, self.tables.contrast.len()) } else { down(&mut self.contrast) }
                out.push(Out::Contrast(self.edits()[1]));
            }
            Action::SaturationUp | Action::SaturationDown => {
                out.push(Out::Sound(sound::SATURATION));
                if action == Action::SaturationUp { up(&mut self.saturation, self.tables.saturation.len()) } else { down(&mut self.saturation) }
                out.push(Out::Saturation(self.edits()[2]));
            }
            Action::DepthOfField => {
                out.push(Out::Sound(sound::DEPTH_OF_FIELD));
                self.depth_of_field += 1;
                if self.depth_of_field >= self.tables.depth_of_field.len() {
                    self.depth_of_field = 0;
                }
                out.push(Out::DepthOfField(self.edits()[3]));
            }
            // 82617BD8: cMsgNISPhotoReset, then every index to its 0.0 entry (no sound).
            Action::Reset => {
                out.push(Out::Reset);
                self.reset_indices();
            }
            // 82617FE8; online the take button does nothing (only the pick timer takes).
            Action::Take if !self.online => self.take(&mut out),
            Action::Take => {}
        }
        if self.index != before {
            out.push(Out::MovieState(self.index as i32));
        }
        out
    }

    fn take(&mut self, out: &mut Vec<Out>) {
        out.push(Out::MovieState(-3));
        out.push(Out::MovieState(-10));
        out.push(Out::Sound(sound::TAKE));
        self.busy = true;
        if self.has_layout {
            out.push(Out::OpenMagazinePromo);
        } else {
            self.store_in = Some(self.timing.store_countdown);
        }
    }

    /// MagazinePromo closed: the picker's re-enter path (EnterSelect) starts the store countdown.
    pub fn reenter(&mut self) {
        if self.busy && self.store_in.is_none() && !self.stored {
            self.store_in = Some(self.timing.store_countdown);
        }
    }

    /// MagazinePromo's answer (research b1067): Continue and Retry set the finish mode and re-enter (the store
    /// countdown); Edit clears the "came back" flag, so the picker's first enter runs again: the picker is open on the
    /// current shot (cMsgNISPhotoSelect).
    pub fn magazine_closed(&mut self, choice: MagazineChoice) -> Vec<Out> {
        match choice {
            MagazineChoice::Continue | MagazineChoice::Retry => {
                self.mode = if choice == MagazineChoice::Continue { FinishMode::Continue } else { FinishMode::Retry };
                self.reenter();
                Vec::new()
            }
            MagazineChoice::Edit => {
                self.busy = false;
                vec![Out::Select(self.index), Out::MovieState(self.index as i32)]
            }
        }
    }

    pub fn mode(&self) -> FinishMode {
        self.mode
    }

    /// The per-frame update (82617100) with its argument `step`: online, the pick timer takes the photo at 0; once
    /// taken, the store countdown, then the store and the finish (82618290 -> callback 82618428).
    pub fn update(&mut self, step: f32) -> Vec<Out> {
        let mut out = Vec::new();
        if self.online && !self.busy {
            self.online_left = self.online_left.saturating_sub(1);
            if self.online_left == 0 {
                self.take(&mut out);
            }
        }
        if let Some(left) = self.store_in.as_mut() {
            *left -= step;
            if *left <= 0.0 {
                self.store_in = None;
                self.stored = true;
                // 82618290: no store for a retry.
                if self.mode != FinishMode::Retry {
                    out.push(Out::Store);
                }
                out.push(Out::Finish(self.mode));
            }
        }
        out
    }

    /// Online: the pick timer's seconds left (count / 60).
    pub fn online_seconds(&self) -> f32 {
        self.online_left as f32 / 60.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> EditTables {
        EditTables {
            brightness: vec![-0.2, -0.15, -0.1, -0.05, 0.0, 0.05, 0.1, 0.15, 0.2],
            contrast: vec![-0.5, -0.375, -0.25, -0.125, 0.0, 0.25, 0.5, 0.75, 1.0],
            saturation: vec![-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0],
            depth_of_field: vec![0.0, 0.05, 0.1, 0.15, 0.2],
        }
    }

    #[test]
    fn it_opens_on_shot_zero_with_neutral_edits_and_steps_with_wraparound() {
        let (mut p, out) = PhotoSelect::open(8, tables(), Timing::default(), false, false);
        assert_eq!(out, [Out::Select(0)]);
        assert_eq!(p.edits(), [0.0, 0.0, 0.0, 0.0]);
        let out = p.action(Action::Previous);
        assert_eq!(out, [Out::Select(7), Out::Sound(sound::STEP), Out::MovieState(-2), Out::MovieState(7)], "wraps to the last shot");
        p.action(Action::Next);
        assert_eq!(p.index(), 0, "and back past the end");
        let (mut one, _) = PhotoSelect::open(1, tables(), Timing::default(), false, false);
        assert!(one.action(Action::Next).is_empty(), "a single shot has nothing to step");
    }

    #[test]
    fn edits_clamp_except_depth_of_field_which_cycles_and_reset_restores_the_zero_entries() {
        let (mut p, _) = PhotoSelect::open(8, tables(), Timing::default(), false, false);
        for _ in 0..12 {
            p.action(Action::BrightnessUp);
            p.action(Action::ContrastDown);
            p.action(Action::SaturationDown);
        }
        assert_eq!(&p.edits()[..3], [0.2, -0.5, -1.0]);
        assert_eq!(p.action(Action::ContrastUp), [Out::Sound(sound::CONTRAST), Out::Contrast(-0.375)]);
        let steps: Vec<_> = (0..5).map(|_| p.action(Action::DepthOfField)[1].clone()).collect();
        assert_eq!(steps.last(), Some(&Out::DepthOfField(0.0)), "the fifth press wraps to the first value");
        assert_eq!(p.action(Action::Reset), [Out::Reset]);
        assert_eq!(p.edits(), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_take_locks_the_screen_then_stores_and_finishes_after_the_countdown() {
        let (mut p, _) = PhotoSelect::open(8, tables(), Timing::default(), false, false);
        let out = p.action(Action::Take);
        assert_eq!(out, [Out::MovieState(-3), Out::MovieState(-10), Out::Sound(sound::TAKE)]);
        assert!(p.busy() && p.action(Action::Next).is_empty(), "input is ignored once taken");
        assert!(p.update(399.0).is_empty());
        assert_eq!(p.update(1.0), [Out::Store, Out::Finish(FinishMode::Normal)]);
        assert!(p.update(1.0).is_empty(), "once");
        // A row with a photo layout opens MagazinePromo; its close starts the store.
        let (mut cover, _) = PhotoSelect::open(8, tables(), Timing::default(), false, true);
        assert_eq!(cover.action(Action::Take).last(), Some(&Out::OpenMagazinePromo));
        assert!(cover.update(1000.0).is_empty());
        cover.reenter();
        assert_eq!(cover.update(400.0), [Out::Store, Out::Finish(FinishMode::Normal)]);
    }

    #[test]
    fn online_the_take_button_is_off_and_the_pick_timer_takes_the_photo() {
        let (mut p, _) = PhotoSelect::open(8, tables(), Timing { online_pick_updates: 3, store_countdown: 1.0 }, true, false);
        assert!(p.action(Action::Take).is_empty());
        assert!(p.update(0.0).is_empty() && p.update(0.0).is_empty());
        assert_eq!(p.update(0.0)[..3], [Out::MovieState(-3), Out::MovieState(-10), Out::Sound(sound::TAKE)]);
        assert_eq!(p.update(1.0), [Out::Store, Out::Finish(FinishMode::Normal)]);
        assert_eq!(Action::from_fe(0x12), Some(Action::ContrastDown));
    }

    #[test]
    fn the_magazine_page_continues_retries_without_a_store_or_goes_back_to_the_picker() {
        let open = || {
            let (mut p, _) = PhotoSelect::open(8, tables(), Timing::default(), false, true);
            p.action(Action::Next);
            assert_eq!(p.action(Action::Take).last(), Some(&Out::OpenMagazinePromo));
            p
        };
        let mut c = open();
        assert!(c.magazine_closed(MagazineChoice::Continue).is_empty());
        assert_eq!(c.update(400.0), [Out::Store, Out::Finish(FinishMode::Continue)]);
        let mut r = open();
        r.magazine_closed(MagazineChoice::Retry);
        assert_eq!(r.update(400.0), [Out::Finish(FinishMode::Retry)], "82618290: a retry stores nothing");
        let mut e = open();
        assert_eq!(e.magazine_closed(MagazineChoice::Edit), [Out::Select(1), Out::MovieState(1)], "back on the same shot");
        assert!(!e.busy() && e.update(1000.0).is_empty(), "picking again, no store");
        assert_eq!(e.action(Action::Next)[0], Out::Select(2));
    }
}
