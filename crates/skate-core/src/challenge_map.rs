//! The retail Challenge Map screen's navigation (FE screen 17, research b255 / b256), over the menu registry's map
//! nodes: the tab (`mgr+0x198`, clamped, no wrap), the folder stack (`mgr+0xE5B4`), the highlighted row (`+0xD8`) and
//! the selected one (`mgr+0xE5E8`). Engine-independent: the host (skate-game `challenge_map_host`) turns the movie's
//! natives and picker callbacks into these calls and posts the returned screen states (`SetState(n)`).
use crate::menus::{EntryKind, EntryRef, MenuContext, MenuRegistry};

/// The movie's `SetState` codes the C++ posts (b256 "Movie refresh") [code + data].
pub mod state {
    pub const RELOAD_LIST: i32 = 10;
    pub const LIST_PANEL: i32 = 21;
    pub const DEFAULT_MAP: i32 = 33;
    pub const MAP_TEXTURE: i32 = 34;
    pub const LIST_TITLE_OFF: i32 = 41;
    pub const LIST_TITLE: i32 = 80;
    pub const DISTRICT_TITLE: i32 = 90;
    pub const SELECT_TAB: i32 = 100;
    pub const CURSOR: i32 = 110;
    pub const LEGEND_ON: i32 = 120;
    pub const LEGEND_OFF: i32 = 130;
}

/// One row of the current list (GetStuffMenuItems' per-row values).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub entry: EntryRef,
    /// The label string id (the host localizes it).
    pub label: String,
    /// Greyed unless something handles it (ours: the registry's rule; retail's `!vt+0x28` is "not a header").
    pub enabled: bool,
}

/// What a select or back press asks the host to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Nothing (a greyed row, an empty folder, a refresh pending).
    None,
    /// The list changed (folder pushed / popped, tab switched): post these states.
    Refresh(Vec<i32>),
    /// A leaf was selected: dispatch it through the registry (its handler decides: a teleport, a mod).
    Select(EntryRef),
    /// B at the root: close the screen (`sub_82D0AFA0(FE, 0x38, -1)`).
    Close,
}

/// The screen's navigation state (plain data: a host can snapshot it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChallengeMapState {
    /// The mode root (`progression` offline, `online` online).
    pub root: Option<EntryRef>,
    pub tab: usize,
    pub stack: Vec<EntryRef>,
    pub hilight: usize,
    pub legend: bool,
}

const REFRESH: [i32; 4] = [state::RELOAD_LIST, state::LIST_TITLE, state::SELECT_TAB, state::CURSOR];

impl ChallengeMapState {
    /// Screen open (`sub_826457E0`): the mode root, the preferred tab (ours: the first visible one; retail's
    /// "initial tab" Lua is not ported), and the open states (10, 90, 21, 33, 41).
    pub fn open(registry: &MenuRegistry, ctx: &MenuContext, root: &str) -> (Self, Vec<i32>) {
        let mut s = Self { root: Some(EntryRef::map(EntryKind::MapListing, root)), ..Default::default() };
        if let Some(i) = s.tabs(registry, ctx).iter().position(|t| t.id == "progression_locations") {
            s.tab = i;
        }
        (s, vec![state::RELOAD_LIST, state::DISTRICT_TITLE, state::LIST_PANEL, state::DEFAULT_MAP, state::LIST_TITLE_OFF])
    }

    /// The tabs (GetSuperCategories): the root's visible children.
    pub fn tabs(&self, registry: &MenuRegistry, ctx: &MenuContext) -> Vec<EntryRef> {
        let Some(root) = &self.root else { return Vec::new() };
        registry.data().map_children(root).into_iter().map(|n| n.entry.clone()).filter(|e| registry.visible(e, ctx)).collect()
    }

    /// The folder whose children are listed: the top of the stack, else the current tab.
    pub fn node(&self, registry: &MenuRegistry, ctx: &MenuContext) -> Option<EntryRef> {
        self.stack.last().cloned().or_else(|| self.tabs(registry, ctx).get(self.tab).cloned())
    }

    /// The current list (GetStuffMenuItems).
    pub fn rows(&self, registry: &MenuRegistry, ctx: &MenuContext) -> Vec<Row> {
        let Some(node) = self.node(registry, ctx) else { return Vec::new() };
        registry
            .data()
            .map_children(&node)
            .into_iter()
            .filter(|n| registry.visible(&n.entry, ctx))
            .map(|n| Row { entry: n.entry.clone(), label: n.label.clone(), enabled: registry.enabled(&n.entry, ctx) })
            .collect()
    }

    /// The list title (GetStuffTitleText): the folder's label.
    pub fn title(&self, registry: &MenuRegistry, ctx: &MenuContext) -> String {
        self.node(registry, ctx).and_then(|n| registry.data().map_node(&n).map(|m| m.label.clone())).unwrap_or_default()
    }

    /// OnMenuHilight (`826477D8`): +0xD8 = index; the map cursor follows.
    pub fn hilight(&mut self, index: usize) -> Vec<i32> {
        self.hilight = index;
        vec![state::CURSOR]
    }

    /// OnMenuSelect (`826475B8`): a folder with something in it is pushed; a leaf is selected; a greyed row or an
    /// empty folder does nothing.
    pub fn select(&mut self, registry: &MenuRegistry, ctx: &MenuContext, index: usize) -> Action {
        let rows = self.rows(registry, ctx);
        let Some(row) = rows.get(index) else { return Action::None };
        if !row.enabled {
            return Action::None;
        }
        if row.entry.kind.is_map_folder() {
            if registry.data().map_children(&row.entry).is_empty() {
                return Action::None;
            }
            self.stack.push(row.entry.clone());
            self.hilight = 0;
            return Action::Refresh(REFRESH.to_vec());
        }
        Action::Select(row.entry.clone())
    }

    /// B (`82646B28`): hide the legend, else pop a folder, else close at the root.
    pub fn back(&mut self) -> Action {
        if self.legend {
            self.legend = false;
            return Action::Refresh(vec![state::LEGEND_OFF]);
        }
        if self.stack.pop().is_some() {
            self.hilight = 0;
            return Action::Refresh(REFRESH.to_vec());
        }
        Action::Close
    }

    /// Y: show the legend (Y again does nothing; B hides it).
    pub fn legend_on(&mut self) -> Action {
        if self.legend {
            return Action::None;
        }
        self.legend = true;
        Action::Refresh(vec![state::LEGEND_ON])
    }

    /// Next / previous tab (triggers, d-pad or stick left / right): clamped, no wrap; the folder stack resets.
    pub fn step_tab(&mut self, registry: &MenuRegistry, ctx: &MenuContext, forward: bool) -> Action {
        let count = self.tabs(registry, ctx).len();
        let next = if forward { (self.tab + 1).min(count.saturating_sub(1)) } else { self.tab.saturating_sub(1) };
        if next == self.tab || self.legend {
            return Action::None;
        }
        self.tab = next;
        self.stack.clear();
        self.hilight = 0;
        Action::Refresh(REFRESH.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menus::{Handler, MapNode, MenuData, Owner};

    fn registry() -> MenuRegistry {
        let node = |kind, key: &str, parent: Option<(EntryKind, &str)>, order| MapNode {
            entry: EntryRef::map(kind, key),
            parent: parent.map(|(k, p)| EntryRef::map(k, p)),
            label: format!("L_{key}"),
            order,
        };
        use EntryKind::*;
        let data = MenuData {
            map_nodes: vec![
                node(MapListing, "progression", None, 0),
                node(MapListing, "progression_active", Some((MapListing, "progression")), 0),
                node(MapListing, "progression_locations", Some((MapListing, "progression")), 3),
                node(MapListing, "downtown", Some((MapListing, "progression_locations")), 0),
                node(MapListing, "empty", Some((MapListing, "progression_locations")), 1),
                node(FeLocation, "dist_downtown_aletown", Some((MapListing, "downtown")), 0),
                node(Challenge, "c1", Some((MapListing, "progression_active")), 0),
            ],
            ..Default::default()
        };
        let mut r = MenuRegistry::new(data);
        let owner = Owner::Engine("teleport:dist_downtown_aletown".into());
        r.register_handler(EntryRef::map(FeLocation, "dist_downtown_aletown"), Handler { owner, select: true, highlight: false, callback: None });
        r
    }

    #[test]
    fn the_map_opens_on_locations_pushes_folders_and_selects_a_teleport() {
        let r = registry();
        let ctx = MenuContext::new("Career");
        let (mut s, open) = ChallengeMapState::open(&r, &ctx, "progression");
        assert_eq!(open, [10, 90, 21, 33, 41]);
        assert_eq!(s.tabs(&r, &ctx).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["progression_active", "progression_locations"]);
        assert_eq!(s.tab, 1, "opens on the locations tab");
        let rows = s.rows(&r, &ctx);
        assert_eq!(rows.iter().map(|r| (r.entry.id.as_str(), r.enabled)).collect::<Vec<_>>(), [("downtown", true), ("empty", false)]);
        assert_eq!(s.title(&r, &ctx), "L_progression_locations");
        assert_eq!(s.select(&r, &ctx, 1), Action::None, "an empty, greyed folder does nothing");
        assert_eq!(s.select(&r, &ctx, 0), Action::Refresh(vec![10, 80, 100, 110]));
        assert_eq!(s.title(&r, &ctx), "L_downtown");
        assert_eq!(s.select(&r, &ctx, 0), Action::Select(EntryRef::map(EntryKind::FeLocation, "dist_downtown_aletown")));
        assert_eq!(s.hilight(0), [110]);
        // Back pops, then closes at the root.
        assert!(matches!(s.back(), Action::Refresh(_)));
        assert_eq!(s.back(), Action::Close);
        // Tabs clamp, no wrap; the challenges tab's challenge has no handler, so it is greyed.
        assert_eq!(s.step_tab(&r, &ctx, true), Action::None);
        assert!(matches!(s.step_tab(&r, &ctx, false), Action::Refresh(_)));
        assert_eq!(s.rows(&r, &ctx)[0].enabled, false);
        assert_eq!(s.step_tab(&r, &ctx, false), Action::None);
        // The legend: Y shows it, B hides it before anything else.
        assert!(matches!(s.legend_on(), Action::Refresh(_)));
        assert_eq!(s.legend_on(), Action::None);
        assert_eq!(s.back(), Action::Refresh(vec![130]));
    }
}
