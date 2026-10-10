//! The CoreMenu script object the pause menu movie (core_menu.apt) reads (doc 31, Milestone 6).
//!
//! Retail registers 21 native members on CoreMenu (registrar sub_825B68E0, TU3); all are read-only views over the
//! crossbar screen (navigation state, skate_core::crossbar) and the current mode's menu (our menu registry), except
//! SetOnlineTeamSubOptionDone. Research: `.local/research/coremenu-host.md`.
use crate::apt_vm::{to_integer, Value, Vm};
use skate_core::{
    crossbar::{CrossbarState, CrossbarView, RegistryView, OPTION_LEVEL},
    menus::{Icon, MenuContext, MenuRegistry},
};

/// Retail GetMaxSubOptions (825B7238): the width of the sub-option tables.
pub const MAX_SUB_OPTIONS: usize = 6;

/// Context facts the progression panel reads (GetProgressionData, 825B73D8); absent = retail's "no career data"
/// value -1 in slot 0, which hides the panel.
pub mod progression_facts {
    pub const MILESTONE: &str = "progression_milestone";
    pub const COUNT: &str = "progression_count";
    pub const START: &str = "progression_start";
    pub const GOAL: &str = "progression_goal";
    pub const BOARD_SALES: &str = "progression_board_sales";
    pub const EXTRA: &str = "progression_extra";
}

pub struct CoreMenu<'a> {
    pub registry: &'a MenuRegistry,
    pub context: &'a MenuContext,
    pub view: &'a RegistryView,
    pub state: &'a mut CrossbarState,
}

fn icon(icon: &Icon) -> Value {
    match icon {
        Icon::Retail(label) | Icon::Image(label) => Value::Text(label.clone()),
    }
}

impl CoreMenu<'_> {
    fn arg(args: &[Value], i: usize) -> i32 {
        args.get(i).map_or(0, to_integer)
    }
    fn item_id(&self, menu: i32, option: i32) -> Option<&str> {
        if menu < 0 || option < 0 {
            return None;
        }
        self.view.item(menu as usize, option as usize)
    }

    /// One CoreMenu member call. Unknown members are an error (the movie only calls the 21 retail ones).
    pub fn call(&mut self, vm: &mut Vm, method: &str, args: &[Value]) -> Result<Value, String> {
        let registry = self.registry;
        let data = registry.data();
        let n = |v: usize| Value::Number(v as f64);
        let i = |v: i32| Value::Number(v as f64);
        Ok(match method {
            "GetNumMenus" => n(self.view.tab_count()),
            "GetMenuName" | "GetMenuIconName" => {
                let menu = Self::arg(args, 0);
                let category = (menu >= 0)
                    .then(|| self.view.tabs.get(menu as usize))
                    .flatten()
                    .and_then(|id| data.category(id));
                match category {
                    Some(c) if method == "GetMenuName" => Value::Text(c.label.clone()),
                    Some(c) => icon(&c.icon),
                    None => Value::Undefined,
                }
            }
            "GetNumOptions" => n(self.view.row_count(Self::arg(args, 0).max(0) as usize)),
            "GetOptionName" | "GetOptionIconName" => {
                let item = self.item_id(Self::arg(args, 0), Self::arg(args, 1)).and_then(|id| data.item(id));
                match item {
                    // NOT PORTED YET: retail's label overrides (SignIn -> sign out when signed in, ReplayEditor in
                    // instant replay, PartyPlay per-language literal; sub_8261DCF0).
                    Some(item) if method == "GetOptionName" => Value::Text(item.label.clone()),
                    Some(item) => icon(&item.icon),
                    None => Value::Undefined,
                }
            }
            "GetNumSubOptions" => {
                let (menu, option) = (Self::arg(args, 0), Self::arg(args, 1));
                if menu < 0 || option < 0 {
                    i(0)
                } else {
                    n(self.view.sub_option_count(menu as usize, option as usize))
                }
            }
            // NOT PORTED YET: the sub-option label table (0x83027958) is not extracted (doc 31 Milestone 6, open).
            "GetSubOptionName" => Value::Text("#".into()),
            "GetCurrentMenuIndex" => i(self.state.menu),
            "GetCurrentOptionIndex" => i(self.state.option),
            "GetCurrentSubOptionIndex" => i(self.state.sub),
            "GetCurrentMenuLevel" => i(self.state.level),
            "IsItemEnabled" => {
                let (menu, option) = (Self::arg(args, 0), Self::arg(args, 1));
                Value::Bool(menu >= 0 && option >= 0 && self.view.row_enabled(menu as usize, option as usize))
            }
            "IsDLCEnabled" => Value::Bool(self.state.dlc && self.view.extra_available()),
            "GetMaxSubOptions" => n(MAX_SUB_OPTIONS),
            "GetCurrentModeTitle" => {
                data.mode(&self.context.mode).map_or(Value::Undefined, |m| Value::Text(m.title.clone()))
            }
            "GetOptionHelperDescription" => {
                // sub_8261E270: the item's helper at the option level, the sub-option helper table at level 2
                // (not extracted yet), else the empty literal.
                let help = (self.state.level == OPTION_LEVEL)
                    .then(|| self.item_id(self.state.menu, self.state.option))
                    .flatten()
                    .and_then(|id| data.item(id))
                    .map(|item| item.help.clone());
                Value::Text(help.unwrap_or_else(|| "#".into()))
            }
            "GetCurrentDLCBannerIndex" => i(self.state.dlc_banner),
            "GetProgressionData" => {
                use progression_facts::*;
                let fact = |name: &str, default: f64| self.context.facts.get(name).copied().unwrap_or(default);
                let values = vec![
                    Value::Number(fact(MILESTONE, -1.0)),
                    Value::Number(fact(COUNT, 0.0)),
                    Value::Number(fact(START, 0.0)),
                    Value::Number(fact(GOAL, 0.0)),
                    Value::Number(fact(BOARD_SALES, 0.0)),
                    Value::Number(fact(EXTRA, 0.0)),
                ];
                Value::Object(vm.array(values)?)
            }
            "GetIsDemo" | "GetIsMarketingDemo" => Value::Bool(false),
            "SetOnlineTeamSubOptionDone" => {
                self.state.online_team_pending = false;
                Value::Undefined
            }
            _ => return Err(format!("CoreMenu has no member {method}")),
        })
    }
}

/// The front-end host objects on the stage (retail sub_825A2190 creates them): CoreMenu answered from the
/// registry and crossbar state; Game's screen callbacks are recorded for the screen stack (next milestone).
pub struct FrontEnd<'a> {
    pub registry: &'a MenuRegistry,
    pub context: &'a MenuContext,
    pub view: RegistryView,
    pub state: CrossbarState,
    /// The language table (label id -> text) and number separators for FELanguage.
    pub language: &'a std::collections::BTreeMap<String, String>,
    pub numbers: crate::fe_language::NumberFormat,
    /// Game / Audio / FELanguage calls not handled yet, in order (diagnostics, tests).
    pub calls: Vec<String>,
}

impl<'a> FrontEnd<'a> {
    pub fn new(
        registry: &'a MenuRegistry,
        context: &'a MenuContext,
        language: &'a std::collections::BTreeMap<String, String>,
    ) -> Self {
        let view = RegistryView::new(registry, context, Default::default());
        let state = CrossbarState::open(&view);
        Self { registry, context, view, state, language, numbers: Default::default(), calls: Vec::new() }
    }
}

impl crate::apt_stage::StageNatives for FrontEnd<'_> {
    fn objects(&self) -> Vec<String> {
        ["Game", "CoreMenu", "Audio", "FELanguage"].map(String::from).to_vec()
    }
    fn call(&mut self, vm: &mut Vm, object: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
        if object == "CoreMenu" {
            let mut core = CoreMenu { registry: self.registry, context: self.context, view: &self.view, state: &mut self.state };
            return core.call(vm, method, &args);
        }
        if object == "FELanguage" {
            use crate::fe_language::*;
            let arg = |v: &Value| match v {
                Value::Number(n) => Arg::Number(*n),
                Value::Bool(b) => Arg::Bool(*b),
                Value::Text(t) => Arg::Text(t.clone()),
                _ => Arg::Undefined,
            };
            let text = |i: usize| args.get(i).map(Value::text).unwrap_or_default();
            match method {
                "GetLocalizedFormatNumberString" => {
                    let decimals = args.get(1).map(|d| crate::apt_vm::to_integer(d).max(0) as usize);
                    let value = args.first().map_or(Arg::Undefined, arg);
                    return Ok(Value::Text(number_string(&self.numbers, &value, decimals)));
                }
                "GetLocalizedFormatString" => {
                    // (id, count, args...)
                    let id = text(0);
                    let count = args.get(1).map_or(0, |c| crate::apt_vm::to_integer(c).max(0) as usize);
                    let values: Vec<Arg> = args.iter().skip(2).take(count.min(6)).map(arg).collect();
                    return Ok(Value::Text(format_string(&self.numbers, template(self.language, &id), &values)));
                }
                "GetLocalizedFormatStringCustom" => {
                    let id = text(0);
                    return Ok(Value::Text(format_string_custom(&self.numbers, template(self.language, &id), &text(1))));
                }
                _ => {}
            }
        }
        self.calls.push(format!("{object}.{method}({})", args.iter().map(Value::text).collect::<Vec<_>>().join(",")));
        Ok(Value::Undefined)
    }
}

#[cfg(test)]
pub(crate) fn test_registry() -> MenuRegistry {
    tests::registry()
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::menus::{Category, Item, MenuData, Mode, Tab};

    pub(super) fn registry() -> MenuRegistry {
        let item = |id: &str, kind: i32| Item {
            id: id.into(),
            label: format!("ID_{id}"),
            icon: Icon::Retail(id.to_lowercase()),
            help: format!("ID_{id}_HELP"),
            sub_option_kind: kind,
        };
        MenuRegistry::new(MenuData {
            categories: vec![
                Category { id: "SinglePlayer".into(), label: "ID_CROSSBAR_SINGLE_PLAYER".into(), icon: Icon::Retail("singleplayer".into()) },
                Category { id: "Options".into(), label: "ID_CROSSBAR_OPTIONS".into(), icon: Icon::Retail("options".into()) },
            ],
            items: vec![item("ChallengeMap", -1), item("SkateReel", 3), item("GameSettings", -1)],
            settings: vec![],
            screens: vec![],
            modes: vec![Mode {
                key: "FreePlay".into(),
                title: "ID_CROSSBAR_FREEPLAY".into(),
                slots: 8,
                tabs: vec![
                    Tab { category: "SinglePlayer".into(), items: vec!["ChallengeMap".into(), "SkateReel".into()] },
                    Tab { category: "Options".into(), items: vec!["GameSettings".into()] },
                ],
            }],
        })
    }

    #[test]
    fn core_menu_members_read_the_registry_and_state() {
        let registry = registry();
        let context = MenuContext::new("FreePlay");
        let view = RegistryView::new(&registry, &context, [(3, 2)].into_iter().collect());
        let mut state = CrossbarState::open(&view);
        let mut vm = Vm::new();
        let mut m = CoreMenu { registry: &registry, context: &context, view: &view, state: &mut state };
        let num = |v: f64| Value::Number(v);
        assert_eq!(m.call(&mut vm, "GetNumMenus", &[]), Ok(num(2.0)));
        assert_eq!(m.call(&mut vm, "GetMenuName", &[num(1.0)]), Ok(Value::Text("ID_CROSSBAR_OPTIONS".into())));
        assert_eq!(m.call(&mut vm, "GetMenuIconName", &[num(0.0)]), Ok(Value::Text("singleplayer".into())));
        assert_eq!(m.call(&mut vm, "GetNumOptions", &[num(0.0)]), Ok(num(2.0)));
        assert_eq!(m.call(&mut vm, "GetOptionName", &[num(0.0), num(1.0)]), Ok(Value::Text("ID_SkateReel".into())));
        assert_eq!(m.call(&mut vm, "GetNumSubOptions", &[num(0.0), num(1.0)]), Ok(num(2.0)));
        assert_eq!(m.call(&mut vm, "GetMaxSubOptions", &[]), Ok(num(6.0)));
        assert_eq!(m.call(&mut vm, "GetCurrentMenuLevel", &[]), Ok(num(1.0)));
        assert_eq!(m.call(&mut vm, "GetCurrentModeTitle", &[]), Ok(Value::Text("ID_CROSSBAR_FREEPLAY".into())));
        // Greyed until a handler enables it (the registry's "greyed until built" rule).
        assert_eq!(m.call(&mut vm, "IsItemEnabled", &[num(0.0), num(0.0)]), Ok(Value::Bool(false)));
        let Ok(Value::Object(p)) = m.call(&mut vm, "GetProgressionData", &[]) else { panic!() };
        assert_eq!((vm.get(p, "0"), vm.get(p, "length")), (num(-1.0), num(6.0)));
        assert!(m.call(&mut vm, "Nope", &[]).is_err());
        state.online_team_pending = true;
        let mut m = CoreMenu { registry: &registry, context: &context, view: &view, state: &mut state };
        m.call(&mut vm, "SetOnlineTeamSubOptionDone", &[]).unwrap();
        assert!(!state.online_team_pending);
    }
}
