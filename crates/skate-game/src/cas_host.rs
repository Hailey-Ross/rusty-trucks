//! Create-a-Skater (FE 15, template CreateASkater, movie cas/createskater): the `Game.CAC_*` natives over the menu
//! table the setup extracts (`menu-tables.json` cas_menus; research b1030). Details: docs/hails-additions/43-career.md#create-a-skater
use crate::apt_vm::{Value, Vm};
use skate_data::menu_tables::CasMenu;

/// The top menu (EditSkaterMain).
pub(crate) const TOP_MENU: usize = 15;

/// Who wants FE 15 open (the boot flow's Customize row); `done` once it closed.
#[derive(bevy::prelude::Resource, Default, Debug)]
pub(crate) struct CasScreen {
    pub open: bool,
    pub done: bool,
}

/// NOT RETAIL YET: the retail screen opens only with `SKATE_RETAIL_CAS=1` until its navigation (menu push / pop, B,
/// the exit popup; research b1031) is ported; otherwise the boot flow keeps our customizer.
pub(crate) fn retail_enabled() -> bool {
    std::env::var("SKATE_RETAIL_CAS").is_ok_and(|v| v == "1")
}

/// One option (the constructor's option records, S+0x40 + id * 0x30): text id, description id, target menu (None =
/// a leaf), type. Empty until the setup extracts them (research b1031).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CasOption {
    pub text: String,
    pub description: String,
    pub target: Option<usize>,
    pub kind: String,
}

#[derive(Default)]
pub(crate) struct CasHost {
    pub open: bool,
    pub menus: Vec<CasMenu>,
    pub options: Vec<CasOption>,
    /// The menu stack (S+0x16D4); the current menu is the last.
    pub stack: Vec<usize>,
    /// Per menu: the highlighted row and the window start (M+0x18 / +0x1C; the movie writes them through
    /// CAC_UpdateMenuIndices).
    pub indices: Vec<(u32, u32)>,
}

impl CasHost {
    pub(crate) fn new(menus: Vec<CasMenu>) -> Self {
        let indices = menus.iter().map(|m| (m.index, m.window)).collect();
        Self { menus, indices, ..Default::default() }
    }

    /// Opens on the top menu with the table's initial highlights.
    pub(crate) fn begin(&mut self) {
        self.open = true;
        self.stack = vec![TOP_MENU];
        self.indices = self.menus.iter().map(|m| (m.index, m.window)).collect();
    }

    fn current(&self) -> Option<usize> {
        self.stack.last().copied().filter(|m| *m < self.menus.len())
    }

    fn option(&self, row: usize) -> Option<(i32, Option<&CasOption>)> {
        let menu = &self.menus[self.current()?];
        let id = *menu.options.get(row)?;
        Some((id, usize::try_from(id).ok().and_then(|i| self.options.get(i))))
    }

    pub(crate) fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        let _ = vm;
        if !self.open || object != "Game" || !method.starts_with("CAC_") {
            return Ok(None);
        }
        let Some(m) = self.current() else { return Ok(None) };
        let row = args.first().map(|v| v.number().max(0.0) as usize).unwrap_or(0);
        let text = |s: &str| Value::Text(if s.is_empty() { "#".into() } else { s.into() });
        Ok(Some(match method {
            "CAC_GetTitleText" => text(&self.menus[m].title),
            "CAC_GetNumItems" => Value::Number(self.menus[m].options.len() as f64),
            // NOT RETAIL YET: the option records are not extracted yet (b1031), so a row shows its option id.
            "CAC_GetItemText" => match self.option(row) {
                Some((_, Some(o))) => text(&o.text),
                Some((id, None)) => Value::Text(format!("#option {id}")),
                None => text(""),
            },
            "CAC_GetDescriptionText" => self.option(row).and_then(|(_, o)| o).map_or(text(""), |o| text(&o.description)),
            "CAC_GetOptionType" => self.option(row).and_then(|(_, o)| o).map_or(text(""), |o| text(&o.kind)),
            "CAC_GetCurrentMenuIndex" => Value::Number(self.indices[m].0 as f64),
            "CAC_GetCurrentMenuMinWindowIndex" => Value::Number(self.indices[m].1 as f64),
            "CAC_UpdateMenuIndices" => {
                let index = args.first().map_or(0.0, |v| v.number()).max(0.0) as u32;
                let window = args.get(1).map_or(0.0, |v| v.number()).max(0.0) as u32;
                self.indices[m] = (index, window);
                Value::Undefined
            }
            // 825A7A08: help A by kind (1 SELECT, except the top menu's two sliders; 2 ACCEPT).
            "CAC_GetButtonHelpTextA" => {
                let slider = m == TOP_MENU && matches!(self.option(self.indices[m].0 as usize), Some((47 | 48, _)));
                text(match self.menus[m].help_a {
                    1 if !slider => "ID_COMMON_SELECT",
                    2 => "ID_COMMON_ACCEPT",
                    _ => "",
                })
            }
            // 825A7B28: help B by kind.
            "CAC_GetButtonHelpTextB" => text(match self.menus[m].help_b {
                1 => "ID_CAC_EXIT",
                2 => "ID_COMMON_BACK",
                3 => "ID_CAC_CHANGE_GRAPHIC",
                _ => "",
            }),
            "CAC_GetFilterText" => text(""),
            "CAC_GetReceiptBalance" => Value::Text(String::new()),
            // NOT RETAIL YET (open in b1030): the shop banner / info panel follow S+0x177C, not ported.
            "CAC_GetBannerType" => Value::Text("none".into()),
            "CAC_GetInfoPanelType" => Value::Text("noblurb".into()),
            "CAC_IsPartNew" => Value::Bool(false),
            "CAC_GetThumbnailFileName" => Value::Text("no_image".into()),
            _ => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(name: &str, title: &str, help: (u32, u32), options: &[i32]) -> CasMenu {
        CasMenu { name: name.into(), title: title.into(), panel: 0, mode: 0, help_a: help.0, help_b: help.1, index: 0,
            window: 0, options: options.to_vec() }
    }

    fn host() -> CasHost {
        let mut menus: Vec<CasMenu> = (0..16).map(|i| menu(&format!("m{i}"), "", (0, 0), &[])).collect();
        menus[TOP_MENU] = menu("EditSkaterMain", "ID_CAC_EDITSKATER_TITLE", (1, 1), &[0, 1, 46, 47, 48]);
        let mut h = CasHost::new(menus);
        h.options = vec![CasOption { text: "ID_BODY".into(), ..Default::default() }];
        h.begin();
        h
    }

    #[test]
    fn the_top_menu_answers_from_the_table() {
        let mut h = host();
        let mut vm = Vm::new();
        let mut call = |h: &mut CasHost, m: &str, args: &[Value]| h.call(&mut vm, "Game", m, args).unwrap().unwrap();
        assert_eq!(call(&mut h, "CAC_GetTitleText", &[]), Value::Text("ID_CAC_EDITSKATER_TITLE".into()));
        assert_eq!(call(&mut h, "CAC_GetNumItems", &[]), Value::Number(5.0));
        assert_eq!(call(&mut h, "CAC_GetItemText", &[Value::Number(0.0)]), Value::Text("ID_BODY".into()));
        assert_eq!(call(&mut h, "CAC_GetButtonHelpTextA", &[]), Value::Text("ID_COMMON_SELECT".into()));
        assert_eq!(call(&mut h, "CAC_GetButtonHelpTextB", &[]), Value::Text("ID_CAC_EXIT".into()));
        // The sliders (options 47 / 48) show no A help.
        call(&mut h, "CAC_UpdateMenuIndices", &[Value::Number(3.0), Value::Number(0.0)]);
        assert_eq!(call(&mut h, "CAC_GetCurrentMenuIndex", &[]), Value::Number(3.0));
        assert_eq!(call(&mut h, "CAC_GetButtonHelpTextA", &[]), Value::Text("#".into()));
    }

    #[test]
    fn closed_or_other_objects_are_not_answered() {
        let mut h = host();
        let mut vm = Vm::new();
        assert!(h.call(&mut vm, "TeamManagement", "CAC_GetNumItems", &[]).unwrap().is_none());
        h.open = false;
        assert!(h.call(&mut vm, "Game", "CAC_GetNumItems", &[]).unwrap().is_none());
    }
}
