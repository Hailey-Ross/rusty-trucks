//! A new career's boot flow (research b868 §1, confirmed by the PS3 new-profile run 2026-11-16): the ordered steps
//! between Press Start and the first world load, as data a mod can replace. Details: docs/hails-additions/47-movie-player.md#career-movies

/// One step: an FE screen by id, or a movie by its `fe_movie` row key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Screen(u16),
    Movie(String),
}

/// FE 46, the keyboard: here the team name (82704DD8).
pub const TEXT_ENTRY: u16 = 46;
/// FE 11 Coach Frank's question (template 10 `bootflow/coachfrank`; select 8260A288).
pub const COACH_FRANK: u16 = 11;

/// Where Coach Frank's answer starts the career (8260A288: row 0 Yes -> `schoolstart`, else `gamestart`), as our
/// teleport destination ids: retail's rows carry the same labels (`ID_LOCATION_SKATESCHOOL` / `ID_LOT_UN_07`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartLocations {
    pub yes: String,
    pub no: String,
}

impl Default for StartLocations {
    fn default() -> Self {
        Self { yes: "dist_skateschool".into(), no: "dist_university_library".into() }
    }
}

impl StartLocations {
    /// The destination for a picked row (any row but 0 is No, as retail; B's cancel row included).
    pub fn pick(&self, row: i64) -> &str {
        if row == 0 { &self.yes } else { &self.no }
    }
}
/// FE 67 FirstTeamMember: entering it sets boot progress bit 1, and a career quit after it resumes there (82704FD8).
pub const FIRST_TEAM_MEMBER: u16 = 67;
pub const PROGRESS_FIRST_TEAM_MEMBER: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootFlow {
    pub steps: Vec<Step>,
}

impl Default for BootFlow {
    /// Retail: FE 9 welcome (difficulty, camera), intro_movie, FE 10, FE 46 team name keyboard, FE 63 team logo,
    /// FE 67 FirstTeamMember (Create-a-Skater), coach_frank_intro (the career's first save runs meanwhile), FE 11
    /// Coach Frank's question (82704BA8 .. 827052C0).
    fn default() -> Self {
        let movie = |k: &str| Step::Movie(k.to_owned());
        Self {
            steps: vec![
                Step::Screen(9),
                movie("intro_movie"),
                Step::Screen(10),
                Step::Screen(TEXT_ENTRY),
                Step::Screen(63),
                Step::Screen(FIRST_TEAM_MEMBER),
                movie("coach_frank_intro"),
                Step::Screen(COACH_FRANK),
            ],
        }
    }
}

impl BootFlow {
    /// Where a new career's flow starts: at FirstTeamMember when the profile already reached it, else at the top.
    pub fn start(&self, progress: u32) -> usize {
        if progress & PROGRESS_FIRST_TEAM_MEMBER != 0 {
            self.steps.iter().position(|s| *s == Step::Screen(FIRST_TEAM_MEMBER)).unwrap_or(0)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retail_order_plays_intro_movie_after_the_welcome_and_coach_frank_after_creation() {
        let flow = BootFlow::default();
        let at = |s: Step| flow.steps.iter().position(|x| *x == s).unwrap();
        assert!(at(Step::Screen(9)) < at(Step::Movie("intro_movie".into())));
        assert!(at(Step::Screen(FIRST_TEAM_MEMBER)) < at(Step::Movie("coach_frank_intro".into())));
        assert!(at(Step::Movie("coach_frank_intro".into())) < at(Step::Screen(11)));
    }

    #[test]
    fn coach_franks_yes_starts_at_the_school_and_anything_else_at_the_library() {
        let s = StartLocations::default();
        assert_eq!(s.pick(0), "dist_skateschool");
        assert_eq!(s.pick(1), "dist_university_library");
        assert_eq!(s.pick(255), "dist_university_library");
    }

    #[test]
    fn a_career_quit_after_the_first_team_member_resumes_there() {
        let flow = BootFlow::default();
        assert_eq!(flow.start(0), 0);
        assert_eq!(flow.steps[flow.start(PROGRESS_FIRST_TEAM_MEMBER)], Step::Screen(FIRST_TEAM_MEMBER));
    }
}
