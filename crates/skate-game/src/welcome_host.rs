//! The new career's boot flow screens on the retail menu stage: FE 9 / 10 WelcomeFlow (research b1007), FE 11 Coach
//! Frank (b1010), FE 67 FirstTeamMember (b1004) and the FE 46 keyboards they ask for. The boot flow (career.rs) opens
//! them through these resources; the stage shows the movies and routes keys; the rules are in `skate_core::career`.
//! Details: docs/hails-additions/47-movie-player.md#welcome-screen
use crate::apt_vm::{Value, Vm};
use bevy::prelude::*;
use skate_core::career::welcome::Welcome;

/// The welcome screen's request and result, shared by the boot flow and the stage.
#[derive(Resource, Default)]
pub(crate) struct WelcomeScreen {
    /// The boot flow wants it open.
    pub open: bool,
    pub logic: Welcome,
    /// Difficulty and camera once the screen closed (the boot flow takes it).
    pub done: Option<(u8, u8)>,
}

impl WelcomeScreen {
    pub(crate) fn start(&mut self) {
        *self = Self { open: true, ..default() };
    }

    /// FE 10: the same movie on its team page.
    pub(crate) fn start_team_prompt(&mut self) {
        *self = Self { open: true, logic: Welcome::team_prompt(), done: None };
    }
}

/// FE 11 Coach Frank's question (`bootflow/coachfrank`): its Yes / No picker reaches us through `Game.OnMenuSelect`
/// (8260A288); the boot flow takes the row and travels to that start.
#[derive(Resource, Default)]
pub(crate) struct CoachFrankScreen {
    pub open: bool,
    /// The picked row once answered (0 = Yes).
    pub answer: Option<i64>,
}

/// FE 67 FirstTeamMember: the boot flow opens it with the team and profile names the movie shows; the stage hands
/// back each picked row (the boot flow applies 82650EC0's rules).
#[derive(Resource, Default)]
pub(crate) struct FirstTeamMemberScreen {
    pub open: bool,
    pub team_name: String,
    pub profile_name: String,
    /// A row picked on the movie, not handled yet.
    pub picked: Option<i64>,
    /// The choice waiting for the profile name keyboard.
    pub pending: Option<skate_core::career::first_team_member::Choice>,
}

/// A keyboard (FE 46) the boot flow asks the stage for, and its answer.
#[derive(Resource, Default)]
pub(crate) struct BootTextEntry {
    /// Waiting for the stage to open it.
    pub request: Option<skate_core::text_entry::TextEntryRequest>,
    /// The request's callback id (which answer this is).
    pub callback: String,
    /// Asked and not answered yet.
    pub waiting: bool,
    /// The localized default the stage filled in (what cancel keeps).
    pub default_text: String,
    /// OK with this text, or `None` = cancel.
    pub result: Option<Option<String>>,
}

impl BootTextEntry {
    pub(crate) fn ask(&mut self, request: skate_core::text_entry::TextEntryRequest) {
        *self = Self { callback: request.callback.clone(), request: Some(request), waiting: true, ..default() };
    }
}

/// The boot screens' natives: `TeamManagement.GetNewTeamState` (825C3D08: 0 for FE 9, 10 for FE 10), FE 67's
/// `TeamManagement.GetTeamData` (825C36B0) and `Game.GetProfileName`, and the shared menu callbacks of their menus.
#[derive(Clone, Debug, Default)]
pub(crate) struct WelcomeHost {
    pub open: bool,
    pub team_state: i32,
    /// FE 67 is open: its names.
    pub first_team_member: Option<(String, String)>,
    /// Rows picked this update (`Game.OnMenuSelect`).
    pub picks: Vec<i64>,
}

impl WelcomeHost {
    pub(crate) fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        if (object, method) == ("TeamManagement", "GetNewTeamState") {
            return Ok(Some(Value::Number(f64::from(self.team_state))));
        }
        if let Some((team, profile)) = &self.first_team_member {
            match (object, method) {
                // [count, team name, logo asset, member photo assets...]: the player alone; no logo or photo assets
                // yet (NOT RETAIL YET: logos FE 63 and the player photo build are not ported).
                ("TeamManagement", "GetTeamData") => {
                    let items = vec![Value::Number(1.0), Value::Text(format!("#{team}")), Value::Text(String::new()), Value::Text(String::new())];
                    return Ok(Some(Value::Object(vm.array(items)?)));
                }
                ("Game", "GetProfileName") => return Ok(Some(Value::Text(format!("#{profile}")))),
                _ => {}
            }
        }
        if !self.open {
            return Ok(None);
        }
        Ok(Some(match (object, method) {
            ("Game", "OnMenuGetItemEnabled") => Value::Bool(true),
            ("Game", "OnMenuHilight") => Value::Undefined,
            ("Game", "OnMenuSelect") => {
                self.picks.push(args.get(1).map_or(-1, |v| i64::from(crate::apt_vm::to_integer(v))));
                Value::Undefined
            }
            _ => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_pick_is_taken_only_while_open_and_the_team_state_always_answers() {
        let mut vm = Vm::new();
        let mut h = WelcomeHost::default();
        assert_eq!(h.call(&mut vm, "Game", "OnMenuSelect", &[Value::Number(0.0), Value::Number(1.0)]).unwrap(), None);
        assert_eq!(h.call(&mut vm, "TeamManagement", "GetNewTeamState", &[]).unwrap(), Some(Value::Number(0.0)));
        h.open = true;
        h.call(&mut vm, "Game", "OnMenuSelect", &[Value::Number(0.0), Value::Number(2.0)]).unwrap();
        assert_eq!(h.picks, vec![2]);
    }

    #[test]
    fn the_first_team_member_names_answer_only_while_it_is_open() {
        let mut vm = Vm::new();
        let mut h = WelcomeHost::default();
        assert_eq!(h.call(&mut vm, "Game", "GetProfileName", &[]).unwrap(), None);
        h.first_team_member = Some(("mtnbound".into(), "The Legend".into()));
        assert_eq!(h.call(&mut vm, "Game", "GetProfileName", &[]).unwrap(), Some(Value::Text("#The Legend".into())));
        let Some(Value::Object(list)) = h.call(&mut vm, "TeamManagement", "GetTeamData", &[]).unwrap() else { panic!("array") };
        assert_eq!(vm.get(list, "1"), Value::Text("#mtnbound".into()));
    }
}
