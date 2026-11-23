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
    /// The team's logo row key (the header shows it).
    pub logo: Option<String>,
    /// A row picked on the movie, not handled yet.
    pub picked: Option<i64>,
    /// The choice waiting for the profile name keyboard.
    pub pending: Option<skate_core::career::first_team_member::Choice>,
}

/// FE 68 ImportSkater, opened by FE 67's Presets row (slot 0, mode 2): the preset rows (key, name text id) the stage
/// lists in retail's order, the screen's logic, and the preset Yes picked for the boot flow to apply.
#[derive(Resource, Default)]
pub(crate) struct ImportSkaterScreen {
    pub open: bool,
    pub logic: skate_core::career::import_skater::ImportSkater,
    pub presets: Vec<(String, String)>,
    pub apply: Option<String>,
    /// The highlighted preset's key (its preview photo is taken next, skater_photo::Previews).
    pub highlight: Option<String>,
    /// A preset was applied: the screen pops once the player's photo is taken again.
    pub retaking: bool,
}

impl ImportSkaterScreen {
    pub(crate) fn start(&mut self, slot: usize, mode: u32, presets: Vec<(String, String)>) {
        *self = Self { open: true, logic: skate_core::career::import_skater::ImportSkater::new(slot, mode), presets, ..default() };
    }
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
    /// The team name's request again after popup 0x12E (empty name): its OK reopens the keyboard (827055E8).
    pub retry: Option<skate_core::text_entry::TextEntryRequest>,
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
    /// FE 67 is open: its team name, profile name and the team logo's asset name.
    pub first_team_member: Option<(String, String, String)>,
    /// The player's photo asset name (empty until it is taken).
    pub photo: String,
    /// Rows picked this update (`Game.OnMenuSelect`).
    pub picks: Vec<i64>,
    /// Rows highlighted this update (`Game.OnMenuHilight`).
    pub hilights: Vec<i64>,
    /// FE 68 is open: the list `ImportSkaters.GetSkaters` gives (text, header).
    pub import_rows: Option<Vec<(String, bool)>>,
    /// FE 68's preview photos by list row: None = not taken yet, Some("") = loaded with no picture, else the asset.
    pub import_photos: Vec<Option<String>>,
}

impl WelcomeHost {
    pub(crate) fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        if (object, method) == ("TeamManagement", "GetNewTeamState") {
            return Ok(Some(Value::Number(f64::from(self.team_state))));
        }
        if let Some((team, profile, logo)) = &self.first_team_member {
            match (object, method) {
                // [count, team name, logo asset, member photo assets...]: the player alone, with their photo.
                ("TeamManagement", "GetTeamData") => {
                    let items = vec![Value::Number(1.0), Value::Text(format!("#{team}")), Value::Text(logo.clone()), Value::Text(self.photo.clone())];
                    return Ok(Some(Value::Object(vm.array(items)?)));
                }
                ("Game", "GetProfileName") => return Ok(Some(Value::Text(format!("#{profile}")))),
                _ => {}
            }
        }
        if let Some(rows) = &self.import_rows {
            match (object, method) {
                // 825C3E20: [count, texts, header flags].
                ("ImportSkaters", "GetSkaters") => {
                    let texts = rows.iter().map(|(t, _)| Value::Text(t.clone())).collect();
                    let headers = rows.iter().map(|(_, h)| Value::Bool(*h)).collect();
                    let items = vec![Value::Number(rows.len() as f64), Value::Object(vm.array(texts)?), Value::Object(vm.array(headers)?)];
                    return Ok(Some(Value::Object(vm.array(items)?)));
                }
                // 825C4258 / 825C42D0: the row is in the photo map, and its asset ("0" when there is none). Header
                // rows have no photo and read as loaded.
                ("ImportSkaters", "IsPreviewPhotoLoaded") | ("ImportSkaters", "GetPreviewPhotoAssetId") => {
                    let row = args.first().and_then(|v| usize::try_from(i64::from(crate::apt_vm::to_integer(v))).ok());
                    let photo = row.and_then(|r| self.import_photos.get(r)).map_or(Some(""), |p| p.as_deref());
                    return Ok(Some(if method == "IsPreviewPhotoLoaded" {
                        Value::Bool(photo.is_some())
                    } else {
                        Value::Text(photo.filter(|p| !p.is_empty()).unwrap_or("0").into())
                    }));
                }
                _ => {}
            }
        }
        if !self.open {
            return Ok(None);
        }
        Ok(Some(match (object, method) {
            ("Game", "OnMenuGetItemEnabled") => Value::Bool(true),
            ("Game", "OnMenuHilight") => {
                self.hilights.push(args.get(1).map_or(-1, |v| i64::from(crate::apt_vm::to_integer(v))));
                Value::Undefined
            }
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
        h.first_team_member = Some(("mtnbound".into(), "The Legend".into(), "logo_object_team_logo_cat_skate_mat".into()));
        assert_eq!(h.call(&mut vm, "Game", "GetProfileName", &[]).unwrap(), Some(Value::Text("#The Legend".into())));
        let Some(Value::Object(list)) = h.call(&mut vm, "TeamManagement", "GetTeamData", &[]).unwrap() else { panic!("array") };
        assert_eq!(vm.get(list, "1"), Value::Text("#mtnbound".into()));
        assert_eq!(vm.get(list, "2"), Value::Text("logo_object_team_logo_cat_skate_mat".into()));
    }

    #[test]
    fn the_preset_list_answers_its_rows_while_open() {
        let mut vm = Vm::new();
        let mut h = WelcomeHost::default();
        assert_eq!(h.call(&mut vm, "ImportSkaters", "GetSkaters", &[]).unwrap(), None);
        h.import_rows = Some(vec![("ID_PRESET_LISTING_TITLE".into(), true), ("ID_TEAMMATE_PRESET_01".into(), false)]);
        let Some(Value::Object(list)) = h.call(&mut vm, "ImportSkaters", "GetSkaters", &[]).unwrap() else { panic!("array") };
        assert_eq!(vm.get(list, "0"), Value::Number(2.0));
        let Value::Object(texts) = vm.get(list, "1") else { panic!("texts") };
        let Value::Object(headers) = vm.get(list, "2") else { panic!("headers") };
        assert_eq!((vm.get(texts, "1"), vm.get(headers, "0"), vm.get(headers, "1")),
            (Value::Text("ID_TEAMMATE_PRESET_01".into()), Value::Bool(true), Value::Bool(false)));
        assert_eq!(h.call(&mut vm, "ImportSkaters", "IsPreviewPhotoLoaded", &[Value::Number(1.0)]).unwrap(), Some(Value::Bool(true)));
    }

    /// 825C4258 / 825C42D0: a row is loaded once its photo is in the map; the asset is "0" until then and for a header.
    #[test]
    fn preview_photos_answer_by_row() {
        let mut vm = Vm::new();
        let mut h = WelcomeHost {
            import_rows: Some(vec![("ID_PRESET_LISTING_TITLE".into(), true), ("A".into(), false), ("B".into(), false)]),
            import_photos: vec![Some(String::new()), None, Some("skater_preview_b".into())],
            ..Default::default()
        };
        let mut ask = |method: &str, row: f64| h.call(&mut vm, "ImportSkaters", method, &[Value::Number(row)]).unwrap();
        assert_eq!(ask("IsPreviewPhotoLoaded", 0.0), Some(Value::Bool(true)));
        assert_eq!(ask("IsPreviewPhotoLoaded", 1.0), Some(Value::Bool(false)));
        assert_eq!(ask("GetPreviewPhotoAssetId", 1.0), Some(Value::Text("0".into())));
        assert_eq!(ask("IsPreviewPhotoLoaded", 2.0), Some(Value::Bool(true)));
        assert_eq!(ask("GetPreviewPhotoAssetId", 2.0), Some(Value::Text("skater_preview_b".into())));
        assert_eq!(ask("GetPreviewPhotoAssetId", 0.0), Some(Value::Text("0".into())));
    }
}
