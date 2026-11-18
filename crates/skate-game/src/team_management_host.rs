//! FE 63 TeamManagement (vtable 82305898, template 108; research b1011): the new career's forced logo pick (mode 2).
//! The team view and the pause menu's logo picker (modes 0 / 1) are not hosted yet.
//! Details: docs/hails-additions/43-career.md#change-the-team-logo-fe-63
use crate::apt_vm::{to_integer, Value, Vm};
use crate::retail_menu_movies::TeamLogo;
use bevy::prelude::*;

/// FE 63's forced logo pick: the boot flow opens it with the team; the stage reports the row picked with A.
#[derive(Resource, Default)]
pub(crate) struct TeamLogoScreen {
    pub open: bool,
    pub team_name: String,
    /// The team's logo row key (None = the data's fallback, which has no picture).
    pub logo: Option<String>,
    /// The row picked with A, not handled yet.
    pub chosen: Option<String>,
}

/// The `TeamManagement` natives while FE 63 is open (registrar 825C3498).
#[derive(Clone, Debug, Default)]
pub(crate) struct TeamManagementHost {
    pub open: bool,
    pub logos: Vec<TeamLogo>,
    pub team_name: String,
    pub logo: Option<String>,
    /// The hovered row in the forced pick (`SelectLogo(i, true)`), which the header shows.
    pub preview: Option<usize>,
    /// SetState codes to push to the movie after the call (0x15 = refresh the header).
    pub states: Vec<i32>,
    /// `SelectLogo(i, false)`: the row the player took.
    pub chosen: Option<String>,
}

/// The asset name a logo row gives the movie: its texture's name (`logo_object_team_logo_<x>_mat`); the menu images
/// resolve it by name (`retail_menu_movies::MenuMovies::image`).
pub(crate) fn logo_asset(logo: &TeamLogo) -> String {
    logo.image.rsplit('/').next().unwrap_or(&logo.image).to_owned()
}

/// The asset name of a team's logo row key; unknown or none = no picture (retail's `default` row has none).
pub(crate) fn team_logo_asset(logos: &[TeamLogo], key: Option<&str>) -> String {
    key.and_then(|k| logos.iter().find(|l| l.key == k)).map(logo_asset).unwrap_or_default()
}

impl TeamManagementHost {
    pub(crate) fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        if !self.open || object != "TeamManagement" {
            return Ok(None);
        }
        let index = |i: usize| args.get(i).map(|v| i64::from(to_integer(v)));
        Ok(Some(match method {
            // [members, name, logo asset, member photos...]: the player alone (photos: NOT RETAIL YET, none).
            "GetTeamData" => {
                let logo = match self.preview.and_then(|i| self.logos.get(i)) {
                    Some(row) => logo_asset(row),
                    None => team_logo_asset(&self.logos, self.logo.as_deref()),
                };
                // The movie adds the literal text marker itself (`'#' + d[1]`).
                let items = vec![Value::Number(1.0), Value::Text(self.team_name.clone()), Value::Text(logo), Value::Text(String::new())];
                Value::Object(vm.array(items)?)
            }
            "GetNumTeamLogos" => Value::Number(self.logos.len() as f64),
            // Out of range: the fallback row, which has no picture.
            "GetLogoAssetID" => Value::Text(
                index(0).and_then(|i| usize::try_from(i).ok()).and_then(|i| self.logos.get(i)).map(logo_asset).unwrap_or_default(),
            ),
            // 826524B8 in mode 2: flag true = preview (SetState 0x15), false = take the row and close.
            "SelectLogo" => {
                let row = index(0).and_then(|i| usize::try_from(i).ok()).filter(|&i| i < self.logos.len());
                let preview = matches!(args.get(1), Some(Value::Bool(true)));
                match (row, preview) {
                    (Some(i), true) => {
                        self.preview = Some(i);
                        self.states.push(0x15);
                    }
                    (Some(i), false) => self.chosen = Some(self.logos[i].key.clone()),
                    (None, _) => {}
                }
                Value::Undefined
            }
            "GetSelectedTeammateData" => Value::Object(vm.array(vec![Value::Number(0.0), Value::Text(" ".into())])?),
            "GetEditSkaterOptions" => Value::Object(vm.array(vec![Value::Number(0.0)])?),
            _ => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logos() -> Vec<TeamLogo> {
        ["mongo", "team_blue_1", "checker_bones"]
            .map(|n| TeamLogo { key: format!("logo_{n}"), text: None, image: format!("fe/cas/logos/logo_object_team_logo_{n}_mat") })
            .to_vec()
    }

    fn host() -> TeamManagementHost {
        TeamManagementHost { open: true, logos: logos(), team_name: "mtnbound".into(), ..Default::default() }
    }

    fn text(v: Option<Value>) -> String {
        match v {
            Some(Value::Text(t)) => t,
            other => panic!("text expected: {other:?}"),
        }
    }

    #[test]
    fn logos_are_listed_by_index_with_their_texture_names() {
        let mut vm = Vm::new();
        let mut h = host();
        assert_eq!(h.call(&mut vm, "TeamManagement", "GetNumTeamLogos", &[]).unwrap(), Some(Value::Number(3.0)));
        assert_eq!(text(h.call(&mut vm, "TeamManagement", "GetLogoAssetID", &[Value::Number(1.0)]).unwrap()), "logo_object_team_logo_team_blue_1_mat");
        assert_eq!(text(h.call(&mut vm, "TeamManagement", "GetLogoAssetID", &[Value::Number(9.0)]).unwrap()), "");
    }

    #[test]
    fn the_forced_pick_previews_the_hovered_logo_and_takes_the_one_picked_with_a() {
        let mut vm = Vm::new();
        let mut h = host();
        h.call(&mut vm, "TeamManagement", "SelectLogo", &[Value::Number(2.0), Value::Bool(true)]).unwrap();
        assert_eq!((h.preview, h.states.as_slice(), h.chosen.as_deref()), (Some(2), &[0x15][..], None));
        h.call(&mut vm, "TeamManagement", "SelectLogo", &[Value::Number(0.0), Value::Bool(false)]).unwrap();
        assert_eq!(h.chosen.as_deref(), Some("logo_mongo"));
    }

    #[test]
    fn a_closed_screen_answers_nothing() {
        let mut vm = Vm::new();
        let mut h = TeamManagementHost::default();
        assert_eq!(h.call(&mut vm, "TeamManagement", "GetNumTeamLogos", &[]).unwrap(), None);
    }

    #[test]
    fn an_unknown_team_logo_has_no_picture() {
        assert_eq!(team_logo_asset(&logos(), Some("logo_gone")), "");
        assert_eq!(team_logo_asset(&logos(), None), "");
        assert_eq!(team_logo_asset(&logos(), Some("logo_mongo")), "logo_object_team_logo_mongo_mat");
    }
}
