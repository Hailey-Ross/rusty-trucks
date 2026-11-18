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

/// The exit confirmation's callback (82603290 -> 82604748).
pub(crate) const EXIT_POPUP: &str = "engine:cas.exit";

/// B on the top menu (82603290, research b1033): Yes (0) / No (1).
pub(crate) fn exit_popup() -> skate_core::fe_popup::PopupRequest {
    skate_core::fe_popup::PopupRequest {
        title: "ID_COMMON_WARNING_TITLE".into(),
        description: "ID_CAC_EXIT_CAC_CONFIRMATION".into(),
        options: vec!["ID_COMMON_YES".into(), "ID_COMMON_NO".into()],
        callback: EXIT_POPUP.into(),
        cancelable: true,
        ..Default::default()
    }
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
    /// S+0x177C, set on menu entry (82600AF8: menu 15 = 2, 17 = 1, 1 = 0): the banner and info panel kind.
    pub shop: u32,
    /// Truck tightness and wheel stiffness, 0..1 (look record +0x2004 / +0x2008; research b1033). NOT RETAIL YET:
    /// not read from or saved to the profile; retail's defaults not read (0.5 meanwhile).
    pub trucks: f32,
    pub wheels: f32,
}

/// Option ids of the top menu's sliders.
const TRUCKS: i32 = 47;
const WHEELS: i32 = 48;
/// Left / Right step and the label thresholds (825FC7E8, 825FFE08; data a mod can change).
const SLIDER_STEP: f32 = 0.1;
const SLIDER_LOW: f32 = 0.35;
const SLIDER_HIGH: f32 = 0.75;

impl CasHost {
    pub(crate) fn new(menus: Vec<CasMenu>, options: Vec<skate_data::menu_tables::CasOptionRow>) -> Self {
        let indices = menus.iter().map(|m| (m.index, m.window)).collect();
        let options = options.into_iter().map(|o| CasOption {
            text: o.text,
            description: o.description,
            target: usize::try_from(o.target).ok(),
            kind: o.kind,
        }).collect();
        Self { menus, options, indices, trucks: 0.5, wheels: 0.5, ..Default::default() }
    }

    /// Entering a menu (82600AF8): the shop mode it sets.
    fn enter(&mut self, menu: usize) {
        self.shop = match menu {
            TOP_MENU => 2,
            17 => 1,
            1 => 0,
            _ => self.shop,
        };
    }

    /// The highlighted row's option id.
    fn highlighted(&self) -> Option<i32> {
        let m = self.current()?;
        self.option(self.indices[m].0 as usize).map(|(id, _)| id)
    }

    /// Left / Right (825FC7E8, C++): the top menu's sliders move by 0.1 within 0..1. True when a value changed
    /// (the movie then refreshes it, SetScreenState state 50). NOT RETAIL YET: selectors, stance, part cycles,
    /// colors and morphs.
    pub(crate) fn left_right(&mut self, right: bool) -> bool {
        let step = if right { SLIDER_STEP } else { -SLIDER_STEP };
        let value = match self.highlighted() {
            Some(TRUCKS) => &mut self.trucks,
            Some(WHEELS) => &mut self.wheels,
            _ => return false,
        };
        let next = ((*value + step) * 10.0).round() / 10.0;
        let next = next.clamp(0.0, 1.0);
        let changed = next != *value;
        *value = next;
        changed
    }

    fn slider(&self, id: i32) -> Option<f32> {
        match id {
            TRUCKS => Some(self.trucks),
            WHEELS => Some(self.wheels),
            _ => None,
        }
    }

    /// The movie's A (Game.OnMenuSelect -> 825FE3C0): a row with a target menu pushes it (82600780: its highlight and
    /// window start at 0); a leaf (target -1: sliders, selectors, colors, morphs) does nothing. Returns the pushed
    /// menu. NOT RETAIL YET: the menu / leaf branches of 825FE3C0 (menus 4, 5, 6, 31..33, options 56, 96..114) and
    /// the part list setup before menu 10.
    pub(crate) fn select(&mut self, row: usize) -> Option<usize> {
        let (_, option) = self.option(row)?;
        let target = option?.target.filter(|t| *t < self.menus.len())?;
        self.stack.push(target);
        self.indices[target] = (0, 0);
        self.enter(target);
        Some(target)
    }

    /// B (825FC768): pops one menu while the stack holds more than one; false on the top menu (retail opens the exit
    /// confirmation there).
    pub(crate) fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            if let Some(m) = self.current() {
                self.enter(m);
            }
            true
        } else {
            false
        }
    }

    /// The current menu.
    pub(crate) fn menu(&self) -> Option<usize> {
        self.current()
    }

    /// The current menu's panel kind (M+8: the movie's SetState 0 list, 1 thumbnails, 2 style, 3 morph, 4 scale,
    /// 5 tattoo, 6 custom thumbnails; research b1032).
    pub(crate) fn panel(&self) -> Option<u32> {
        self.current().map(|m| self.menus[m].panel)
    }

    /// Opens on the top menu with the table's initial highlights.
    pub(crate) fn begin(&mut self) {
        self.open = true;
        self.stack = vec![TOP_MENU];
        self.indices = self.menus.iter().map(|m| (m.index, m.window)).collect();
        self.enter(TOP_MENU);
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
        if !self.open || object != "Game" || !(method.starts_with("CAC_") || method == "OnMenuSelect") {
            return Ok(None);
        }
        let Some(m) = self.current() else { return Ok(None) };
        if method == "OnMenuSelect" {
            // (screen id, row).
            let row = args.get(1).map_or(-1, |v| crate::apt_vm::to_integer(v));
            if let Ok(row) = usize::try_from(row) {
                self.select(row);
            }
            return Ok(Some(Value::Undefined));
        }
        let row = args.first().map(|v| v.number().max(0.0) as usize).unwrap_or(0);
        let text = |s: &str| Value::Text(if s.is_empty() { "#".into() } else { s.into() });
        Ok(Some(match method {
            "CAC_GetTitleText" => text(&self.menus[m].title),
            "CAC_GetNumItems" => Value::Number(self.menus[m].options.len() as f64),
            // A table without the option records (extracted before them) shows the option id.
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
            // 825A7988 / 825A74F0 by the shop mode (S+0x177C). NOT RETAIL YET: the part thumbnails' own panel types.
            "CAC_GetBannerType" => Value::Text(match self.shop { 0 => "slappys", 1 => "petes", _ => "none" }.into()),
            "CAC_GetInfoPanelType" => Value::Text(match self.shop { 1 => "petes", 2 => "blurb", _ => "noblurb" }.into()),
            // 825FFB18 / 825FFE08 for the sliders: round(value * 10); LOOSE / MEDIUM / TIGHT, SOFT / MEDIUM / HARD.
            "CAC_GetIntegerValue" => match self.option(row).and_then(|(id, _)| self.slider(id)) {
                Some(v) => Value::Number((v * 10.0).round() as f64),
                None => Value::Number(0.0),
            },
            "CAC_GetStringValue" => match self.option(row).and_then(|(id, _)| self.slider(id).map(|v| (id, v))) {
                Some((id, v)) => {
                    let level = if v < SLIDER_LOW { 0 } else if v > SLIDER_HIGH { 2 } else { 1 };
                    let labels = if id == TRUCKS {
                        ["ID_CAC_EDIT_TRUCKS_LOOSE", "ID_CAC_EDIT_TRUCKS_MEDIUM", "ID_CAC_EDIT_TRUCKS_TIGHT"]
                    } else {
                        ["ID_CAC_EDIT_WHEELS_SOFT", "ID_CAC_EDIT_WHEELS_MEDIUM", "ID_CAC_EDIT_WHEELS_HARD"]
                    };
                    Value::Text(labels[level].into())
                }
                None => text(""),
            },
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
        let mut h = CasHost::new(menus, vec![skate_data::menu_tables::CasOptionRow { text: "ID_BODY".into(),
            description: String::new(), target: 17, kind: "option".into() }]);
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
    fn a_pushes_the_rows_target_and_b_pops_it() {
        let mut menus: Vec<CasMenu> = (0..18).map(|i| menu(&format!("m{i}"), "", (0, 0), &[])).collect();
        menus[TOP_MENU] = menu("EditSkaterMain", "ID_CAC_EDITSKATER_TITLE", (1, 1), &[0]);
        menus[17] = menu("BackAlleyPete", "ID_CAC_BODY_MODIFICATION_TITLE", (1, 2), &[]);
        let mut h = CasHost::new(menus, vec![skate_data::menu_tables::CasOptionRow { text: "ID_BODY".into(),
            description: String::new(), target: 17, kind: "option".into() }]);
        h.begin();
        assert_eq!(h.shop, 2);
        let mut vm = Vm::new();
        h.call(&mut vm, "Game", "OnMenuSelect", &[Value::Number(688.0), Value::Number(0.0)]).unwrap();
        assert_eq!((h.menu(), h.shop), (Some(17), 1));
        assert!(h.back());
        assert_eq!((h.menu(), h.shop), (Some(TOP_MENU), 2));
        assert!(!h.back());
    }

    #[test]
    fn the_sliders_step_by_a_tenth_and_name_their_level() {
        let mut h = host();
        let mut vm = Vm::new();
        h.indices[TOP_MENU] = (3, 0);
        assert_eq!(h.call(&mut vm, "Game", "CAC_GetIntegerValue", &[Value::Number(3.0)]).unwrap(), Some(Value::Number(5.0)));
        assert!(h.left_right(true));
        assert!(h.left_right(true));
        assert!(h.left_right(true));
        assert_eq!(h.call(&mut vm, "Game", "CAC_GetStringValue", &[Value::Number(3.0)]).unwrap(),
            Some(Value::Text("ID_CAC_EDIT_TRUCKS_TIGHT".into())));
        for _ in 0..12 {
            h.left_right(false);
        }
        assert_eq!(h.trucks, 0.0);
        assert!(!h.left_right(false));
        assert_eq!(h.call(&mut vm, "Game", "CAC_GetStringValue", &[Value::Number(4.0)]).unwrap(),
            Some(Value::Text("ID_CAC_EDIT_WHEELS_MEDIUM".into())));
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
