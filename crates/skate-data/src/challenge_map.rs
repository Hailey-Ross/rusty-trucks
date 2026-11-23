//! The retail Challenge Map tree (FE screen 17) from the vault collections, as menu map nodes.
//! Data rules (b244, b255, b447, b450): docs/hails-additions/31-retail-menus.md#challenge-map-tree
use crate::attrib_hash::{hash, numeric_name};
use crate::collections::Collection;
use skate_core::menus::{EntryKind, EntryRef, MapNode};
use std::collections::BTreeMap;

const LISTING_FILTER: &str = "Hash_E0762EA3F90C9EEB";
const LISTING_ORDER: &str = "Hash_982BD1EAE2334504";
const FILTER_TITLE: &str = "Hash_6766EFD84D74F4F0";
const LOCATION_LABEL: &str = "Hash_174B301910A902C9";
const LOCATION_DISTRICT: &str = "Hash_50D9293B818507EC";

/// Which `world` each location filter lists (retail's filter Lua, as data; the DLC folders come with the packs' rows,
/// research b447). Mods may change it.
#[derive(Clone, Debug, PartialEq)]
pub struct LocationRules(pub BTreeMap<String, String>);

impl Default for LocationRules {
    fn default() -> Self {
        let pairs = [
            ("downtown", "dist_downtown"),
            ("industrial", "dist_industrial"),
            ("university", "dist_university"),
            ("skate_school", "dist_skateschool"),
            ("maloof", "dist_maloofmoneycup"),
            ("blackbox_park", "dist_blackboxpark"),
            ("maloofmoneycupdlc", "dist_maloofmoneycupdlc"),
            ("dwmegacompounddlc", "dlc_dw_megacompound"),
            ("sanitariumdlc", "dist_sanitariumdlc"),
            ("artgallerydlc", "dist_artgallerydlc"),
        ];
        Self(pairs.into_iter().map(|(a, b)| (a.to_owned(), b.to_owned())).collect())
    }
}

/// A location row's "always shown" flag and name text (retail `Unlock.IsLocationUnlocked`, `82839C38`, research b450).
const LOCATION_SHOWN: &str = "Hash_90E678B6961730E1";
const LOCATION_NAME: &str = "Hash_942AB8AEE4B414ED";

/// Which locations count as unlocked (b450 / b458; NOT RETAIL YET until the unlock store exists): docs/hails-additions/31-retail-menus.md#location-unlocks
#[derive(Clone, Debug, PartialEq)]
pub struct LocationUnlocks {
    /// Retail's `all_locations_unlock` (cheat id 2, which no TU3 code triggers) and the manager's unlock-all byte.
    pub all: bool,
    pub granted: std::collections::BTreeSet<String>,
    pub locked: std::collections::BTreeSet<String>,
}

impl Default for LocationUnlocks {
    fn default() -> Self {
        Self { all: false, granted: Default::default(), locked: ["gamestart", "dist_skateschool"].map(str::to_owned).into() }
    }
}

impl LocationUnlocks {
    pub fn unlocked(&self, name: &str) -> bool {
        self.all || self.granted.contains(name) || !self.locked.contains(name)
    }

    /// The `LOCATIONS` lines of setup's `private/dlc-unlocks.json` (the player's DLC unlock files, b458); a fresh
    /// save's view when the file is missing.
    pub fn load(asset_root: &std::path::Path) -> Self {
        let path = asset_root.join("private/dlc-unlocks.json");
        let Ok(bytes) = std::fs::read(&path) else { return Self::default() };
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let granted = value["unlocks"].as_array().into_iter().flatten()
            .filter(|u| u["kind"].as_str() == Some("LOCATIONS"))
            .filter_map(|u| u["name"].as_str().map(str::to_owned))
            .collect();
        Self { granted, ..Self::default() }
    }
}

/// Known `map_listing` names, so the tabs' hook ids read as words (other rows keep their vault key, `Hash_...`).
const KNOWN_LISTINGS: [&str; 10] = [
    "progression", "progression_locations", "progression_active", "progression_completed", "progression_all",
    "online", "online_competitive", "online_team", "freeskate", "location",
];

/// `map_listing` field `IsDebug` (research b479: retail's tab and folder filter 82D0C0E0 skips a row with it set).
const LISTING_IS_DEBUG: &str = "Hash_755772463A459824";

/// The `map_listing` ids flagged `IsDebug` (TU3: only `progression_all`, the "debug_list_all" tab), named as `load`
/// names them.
pub fn debug_listings(entries: &[Collection]) -> std::collections::BTreeSet<String> {
    let names: BTreeMap<u64, &str> = KNOWN_LISTINGS.iter().map(|n| (hash(n), *n)).collect();
    entries
        .iter()
        .filter(|e| class_is(e, "map_listing"))
        .filter(|e| text(e, LISTING_IS_DEBUG).is_some_and(|d| d.trim_start_matches('0').len() > 0))
        .map(|e| names.get(&key_hash(&e.key)).map_or_else(|| e.key.clone(), |n| (*n).to_owned()))
        .collect()
}

fn class_is(entry: &Collection, class: &str) -> bool {
    numeric_name(&entry.class_name) == numeric_name(class)
}

fn text<'a>(entry: &'a Collection, field: &str) -> Option<&'a str> {
    entry.fields.get(field).map(|f| f.data.as_str())
}

/// A RefSpec field's (class hash, key hash) from its hex (`CCCCCCCCCCCCCCCC KKKKKKKKKKKKKKKK`).
fn ref_spec(entry: &Collection, field: &str) -> Option<(u64, u64)> {
    let data = text(entry, field)?;
    let class = u64::from_str_radix(data.get(0..16)?, 16).ok()?;
    let key = u64::from_str_radix(data.get(16..32)?, 16).ok()?;
    Some((class, key))
}

/// A typed RefSpec field's key (`ssRefSpec_<class>`: the class is the type, the data is `KKKKKKKKKKKKKKKK` + padding).
fn typed_ref_key(entry: &Collection, field: &str) -> Option<u64> {
    u64::from_str_radix(text(entry, field)?.get(0..16)?, 16).ok()
}

const WORLD_CLASS: &str = "world";

/// Each location's map picture name (FE `82649FF8`: the location's district names its `world` record, whose
/// tMapInfo names the picture), by location key. `world_maps` = the world records that set their own name (the menu
/// set's `world_maps`, resolved at export, b259); a record without one inherits its parent's.
pub fn location_maps(entries: &[Collection], world_maps: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let worlds: Vec<&Collection> = entries.iter().filter(|e| class_is(e, WORLD_CLASS)).collect();
    let by_hash: BTreeMap<u64, &str> = worlds.iter().copied().map(|e| (key_hash(&e.key), e.key.as_str())).collect();
    let parent: BTreeMap<&str, &str> = worlds.iter().copied().map(|e| (e.key.as_str(), e.parent.as_str())).collect();
    fn name_of<'a>(mut world: &'a str, parent: &BTreeMap<&'a str, &'a str>, maps: &BTreeMap<String, String>) -> Option<String> {
        for _ in 0..16 {
            if let Some(name) = maps.get(world) {
                return Some(name.clone());
            }
            world = parent.get(world)?;
        }
        None
    }
    entries
        .iter()
        .filter(|e| class_is(e, "fe_locations"))
        .filter_map(|loc| {
            // NOT RETAIL YET (b417): the DLC locations' District names a map_category (Maloof NYC), not a world, so
            // their World field picks the record; which field retail reads there is unverified.
            let world = [LOCATION_DISTRICT, LOCATION_WORLD].iter().find_map(|f| by_hash.get(&typed_ref_key(loc, f)?))?;
            Some((loc.key.clone(), name_of(world, &parent, world_maps)?))
        })
        .filter(|(_, name)| !name.is_empty())
        .collect()
}

const LOCATION_ICON: &str = "Hash_786255B8FF426A3D";
const LOCATION_WORLD: &str = "Hash_7565CD2A80E93A5E";
pub(crate) const ICON_CLASS: &str = "Hash_F788504A8D922CED";
pub(crate) const ICON_NAME: &str = "Hash_FCFE787CCF2F775E";
const WORLD_OFFSET: &str = "Hash_B88A4034D63E905E";
const WORLD_MAP_INFO: &str = "Hash_20BBC594CD2FAEF8";

const ICON_MAX_DISTANCE: &str = "Hash_2CF25B33B439DDCD";

/// A fe_map_icons record (minimap and Challenge Map icons, b565 / b571).
#[derive(Clone, Debug, PartialEq)]
pub struct MapIcon {
    /// Its texture in the icon pack (`race_gate` -> `mini_gate`).
    pub texture: String,
    /// Beyond this the minimap does not draw it; 0 = the minimap's default (1000).
    pub max_distance: f32,
}

/// Every fe_map_icons record by name.
pub fn map_icons(collections: &crate::collections::Collections) -> BTreeMap<String, MapIcon> {
    collections
        .entries()
        .iter()
        .filter(|e| class_is(e, ICON_CLASS))
        .filter_map(|e| {
            let texture = collections.field(ICON_CLASS, &e.key, ICON_NAME).ok()?.data.clone();
            let max_distance = collections
                .field(ICON_CLASS, &e.key, ICON_MAX_DISTANCE)
                .ok()
                .and_then(|f| u32::from_str_radix(f.data.get(0..8)?, 16).ok())
                .map_or(0.0, f32::from_bits);
            Some((e.key.clone(), MapIcon { texture, max_distance }))
        })
        .collect()
}

/// A record by name: most keys are stored hashed, so `name` also matches its hash (retail's script lookup, b571).
pub fn map_icon<'a>(icons: &'a BTreeMap<String, MapIcon>, name: &str) -> Option<&'a MapIcon> {
    icons.get(name).or_else(|| icons.get(&format!("Hash_{:016X}", hash(name))))
}

/// What a location's Challenge Map marker needs besides its locator (research b264 / b265).
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerSource {
    /// The icon texture name: the location's `Icon` (fe_map_icons RefSpec, inherited) record's name text.
    pub icon: String,
    /// The location's `World` record's offset (Vector2, inherited; added to the locator's x / z).
    pub offset: [f32; 2],
    /// That world's tMapInfo b1..b6 (inherited): the projection bounds and map size.
    pub bounds: [i32; 6],
}

/// The marker data per fe_locations key; a location whose World or icon cannot be read is left out.
pub fn location_markers(collections: &crate::collections::Collections) -> BTreeMap<String, MarkerSource> {
    let entries = collections.entries();
    let icons: BTreeMap<u64, &str> =
        entries.iter().filter(|e| class_is(e, ICON_CLASS)).map(|e| (key_hash(&e.key), e.key.as_str())).collect();
    let worlds: BTreeMap<u64, &str> =
        entries.iter().filter(|e| class_is(e, WORLD_CLASS)).map(|e| (key_hash(&e.key), e.key.as_str())).collect();
    let hex = |data: &str, at: usize| data.get(at..at + 8).and_then(|h| u32::from_str_radix(h, 16).ok());
    entries
        .iter()
        .filter(|e| class_is(e, "fe_locations"))
        .filter_map(|loc| {
            let field = |name: &str| collections.field("fe_locations", &loc.key, name).ok().map(|f| f.data.as_str());
            // Icon: Attrib::RefSpec = class, key; World: ClassRefSpec = key first.
            let icon_key = u64::from_str_radix(field(LOCATION_ICON)?.get(16..32)?, 16).ok()?;
            let world_key = u64::from_str_radix(field(LOCATION_WORLD)?.get(0..16)?, 16).ok()?;
            let icon = collections.field(ICON_CLASS, icons.get(&icon_key)?, ICON_NAME).ok()?.data.clone();
            let world = worlds.get(&world_key)?;
            let offset = collections.field(WORLD_CLASS, world, WORLD_OFFSET).ok().map_or([0.0; 2], |f| {
                [0, 8].map(|at| hex(&f.data, at).map_or(0.0, f32::from_bits))
            });
            let info = collections.field(WORLD_CLASS, world, WORLD_MAP_INFO).ok()?;
            let mut bounds = [0i32; 6];
            for (i, b) in bounds.iter_mut().enumerate() {
                *b = hex(&info.data, 8 + 8 * i)? as i32;
            }
            Some((loc.key.clone(), MarkerSource { icon, offset, bounds }))
        })
        .collect()
}

/// A `Hash_XXXXXXXXXXXXXXXX` key, or `hash(name)` for a readable one.
pub(crate) fn key_hash(key: &str) -> u64 {
    key.strip_prefix("Hash_").and_then(|h| u64::from_str_radix(h, 16).ok()).unwrap_or_else(|| hash(key))
}

/// The skate.Park folder's filter and the `world` fields it reads (research b447).
const SKATEPARK_FILTER: &str = "skatepark";
const PARK_FLAG: &str = "IsSkatePark";
const PARK_TAB_FLAG: &str = "AllowInSkateParksTab";
/// A world row's own display text id (`ID_WORLD_SKATEPARK_MEGAPARK_FULL`).
const WORLD_LABEL: &str = "Hash_095E1D9778F0C6F4";

/// A leaf's category class and the category's sort order (Int32; research b451).
const CATEGORY_CLASS: &str = "map_category";
const CATEGORY_ORDER: &str = "Hash_1349BB6EC1D1E751";
/// A category header's label id (`82644430`, b454) and the folder flag that drops a lone category's header.
const CATEGORY_LABEL: &str = "Hash_465698DD3CC73CA2";
const FOLDER_LONE_HEADERLESS: &str = "Hash_EAFE7F250C154184";

/// The label collation order (b451: `8290F0A8` ranks each character by its position in the language's sort string;
/// upper and lower case interleaved, then the digits). Ours, written to that rule; NOT RETAIL YET: the language's own
/// string (likely `ID_COMMON_SORTING_FORMAT`, with accented letters) is not exported, so a mod language cannot set it.
pub const COLLATION: &str = "AaBbCcDdEeFfGgHhIiJjKkLlMmNnOoPpQqRrSsTtUuVvWwXxYyZz0123456789";

/// `8290F0A8`'s comparison: character by character by position in `order`, a character missing from it after every
/// listed one; a string that is a prefix of the other compares equal.
pub fn collate(a: &str, b: &str, order: &str) -> std::cmp::Ordering {
    let rank = |c: char| order.chars().position(|o| o == c).unwrap_or(usize::MAX);
    a.chars().zip(b.chars()).map(|(x, y)| rank(x).cmp(&rank(y))).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal)
}

/// Field lookup through a row's parents in its class (the vault's inheritance), over a plain row list.
struct Inherited<'a> {
    rows: BTreeMap<(u64, u64), &'a Collection>,
}

impl<'a> Inherited<'a> {
    fn new(entries: &'a [Collection]) -> Self {
        Self { rows: entries.iter().map(|e| ((key_hash(&numeric_name(&e.class_name)), key_hash(&e.key)), e)).collect() }
    }

    fn get(&self, row: &'a Collection, field: &str) -> Option<&'a str> {
        let wanted = numeric_name(field);
        let class = key_hash(&numeric_name(&row.class_name));
        let mut at = row;
        for _ in 0..16 {
            if let Some(f) = at.fields.get(field).or_else(|| at.fields.get(&wanted)) {
                return Some(f.data.as_str());
            }
            if at.parent.is_empty() {
                return None;
            }
            at = self.rows.get(&(class, key_hash(&at.parent)))?;
        }
        None
    }
}
/// The tree under `root` as map nodes, folders in retail's sort order (b451).
pub fn load(entries: &[Collection], root: &str, rules: &LocationRules, unlocks: &LocationUnlocks,
    label_text: &dyn Fn(&str) -> Option<String>) -> Vec<MapNode> {
    let categories: BTreeMap<u64, &Collection> =
        entries.iter().filter(|e| class_is(e, CATEGORY_CLASS)).map(|e| (key_hash(&e.key), e)).collect();
    let names: BTreeMap<u64, &str> = KNOWN_LISTINGS.iter().map(|n| (hash(n), *n)).collect();
    let id_of = |key: &str| names.get(&key_hash(key)).map_or_else(|| key.to_owned(), |n| (*n).to_owned());
    let filters: BTreeMap<u64, &Collection> = entries.iter().filter(|e| class_is(e, "map_filter")).map(|e| (key_hash(&e.key), e)).collect();
    let listings: Vec<&Collection> = entries.iter().filter(|e| class_is(e, "map_listing")).collect();
    let locations: Vec<&Collection> = entries.iter().filter(|e| class_is(e, "fe_locations")).collect();
    let worlds: Vec<&Collection> = entries.iter().filter(|e| class_is(e, WORLD_CLASS)).collect();
    let inherited = Inherited::new(entries);
    // A leaf's category (`Hash_50D9293B818507EC`, a map_category ref, inherited; `82644228`) and that category's order.
    let category_of = |row: &Collection| -> Option<&Collection> {
        inherited.get(row, LOCATION_DISTRICT).and_then(|d| u64::from_str_radix(d.get(0..16)?, 16).ok())
            .and_then(|key| categories.get(&key)).copied()
    };
    let category_order = |c: Option<&Collection>| -> i32 {
        c.and_then(|c| inherited.get(c, CATEGORY_ORDER)).and_then(|d| u32::from_str_radix(d.get(0..8)?, 16).ok()).map_or(0, |v| v as i32)
    };
    let root_entry = EntryRef::map(EntryKind::MapListing, root);
    let mut nodes = vec![MapNode { entry: root_entry.clone(), parent: None, label: root.to_owned(), order: 0 }];
    let mut queue = vec![(key_hash(root), root_entry)];
    while let Some((parent_hash, parent)) = queue.pop() {
        for row in listings.iter().filter(|r| key_hash(&r.parent) == parent_hash) {
            let entry = EntryRef::map(EntryKind::MapListing, &id_of(&row.key));
            let order = text(row, LISTING_ORDER).and_then(|d| d.get(0..4)).and_then(|d| i32::from_str_radix(d, 16).ok()).unwrap_or(0);
            let folder = ref_spec(row, LISTING_FILTER);
            let filter = folder.filter(|(class, _)| *class == hash("map_filter")).and_then(|(_, key)| filters.get(&key));
            // A folder may point at a map_category instead (the debug tree's `progression_all_team_spacer`): its label
            // then comes from the category, empty for a spacer row.
            let category = folder.filter(|(class, _)| *class == hash("map_category")).and_then(|(_, key)| categories.get(&key));
            let label = filter.and_then(|f| text(f, FILTER_TITLE))
                .or_else(|| category.map(|c| text(c, CATEGORY_LABEL).unwrap_or("")))
                .unwrap_or(&entry.id).to_owned();
            nodes.push(MapNode { entry: entry.clone(), parent: Some(parent.clone()), label, order });
            // A folder's leaves: (category order, category, label, leaf).
            let mut leaves: Vec<(i32, Option<&Collection>, String, MapNode)> = Vec::new();
            let mut leaf = |row: &Collection, kind: EntryKind, label: &str| {
                let node = MapNode { entry: EntryRef::map(kind, &row.key), parent: Some(entry.clone()), label: label.to_owned(), order: 0 };
                let category = category_of(row);
                leaves.push((category_order(category), category, label.to_owned(), node));
            };
            // The location leaves of a location filter.
            if let Some(world) = filter.and_then(|f| rules.0.get(&f.key)) {
                let world = hash(world);
                for loc in &locations {
                    let in_world = inherited.get(loc, LOCATION_WORLD).and_then(|d| u64::from_str_radix(d.get(0..16)?, 16).ok()) == Some(world);
                    let shown = inherited.get(loc, LOCATION_SHOWN).is_some_and(|d| d.trim_start_matches('0') != "")
                        || unlocks.unlocked(inherited.get(loc, LOCATION_NAME).unwrap_or(&loc.key));
                    if in_world && shown {
                        leaf(loc, EntryKind::FeLocation, text(loc, LOCATION_LABEL).unwrap_or(&loc.key));
                    }
                }
            }
            // The skate.Park folder: its park worlds (b447).
            if filter.is_some_and(|f| f.key == SKATEPARK_FILTER) {
                let on = |w: &Collection, field: &str| inherited.get(w, field).is_some_and(|d| d.starts_with("01"));
                for world in worlds.iter().filter(|w| on(w, PARK_FLAG) && on(w, PARK_TAB_FLAG)) {
                    leaf(world, EntryKind::World, inherited.get(world, WORLD_LABEL).unwrap_or(&world.key));
                }
            }
            // Retail folder sort (82647EA0, comparator 82647B80, b451): category order, then label text; ours stable.
            let display = |label: &str| label_text(label).unwrap_or_else(|| label.to_owned());
            leaves.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| collate(&display(&a.2), &display(&b.2), COLLATION)));
            // A map_category header row before each category run (b454); uncategorized leaves get none (NOT RETAIL YET).
            let mut kinds: Vec<&str> = leaves.iter().filter_map(|l| l.1.map(|c| c.key.as_str())).collect();
            kinds.dedup();
            let lone = inherited.get(row, FOLDER_LONE_HEADERLESS).is_some_and(|d| d.trim_start_matches('0') != "") && kinds.len() == 1;
            let mut previous: Option<&str> = None;
            let mut rank = 0;
            for (_, category, _, mut node) in leaves {
                if let Some(c) = category.filter(|c| !lone && previous != Some(c.key.as_str())) {
                    let label = inherited.get(c, CATEGORY_LABEL).unwrap_or(&c.key).to_owned();
                    nodes.push(MapNode { entry: EntryRef::map(EntryKind::MapCategory, &c.key), parent: Some(entry.clone()), label, order: rank });
                    rank += 1;
                }
                previous = category.map(|c| c.key.as_str());
                node.order = rank;
                rank += 1;
                nodes.push(node);
            }
            queue.push((key_hash(&row.key), entry));
        }
    }
    nodes
}

/// What the challenge folders test per challenge (research b1054 §3: the GlobalData row's fields), from setup's
/// `challenges.json` `map_items`.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub struct ChallengeItem {
    pub id: String,
    pub challenge_type: u32,
    #[serde(default)]
    pub title_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub teams: bool,
    #[serde(default)]
    pub debug_only: bool,
    /// The map name and the sign-up spot a Sign Up teleports to (Location, b911 §2).
    #[serde(default)]
    pub map: Option<String>,
    #[serde(default)]
    pub location: Option<ItemLocation>,
    /// In the map's item pool (a skatercollections `challenges` row; the bank's family rows are not).
    #[serde(default)]
    pub pool: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub struct ItemLocation {
    pub matrix: [[f32; 4]; 4],
}

/// The tab filters (b1054 §1): `active` (activated, not passed), `completed` (Owned or Killed), `all`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabRule {
    Active,
    Passed,
    All,
}

/// One challenge folder filter as data (b1054 §1 / §5): the types it takes, a tag it needs, tags it refuses, whether
/// a team challenge also passes, whether DebugOnly rows are refused. Mods can add or retarget a rule by filter key.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FolderRule {
    pub types: Vec<u32>,
    pub require_tag: Option<String>,
    pub exclude_tags: Vec<String>,
    pub teams: bool,
    pub exclude_debug: bool,
}

impl FolderRule {
    pub fn accepts(&self, item: &ChallengeItem) -> bool {
        let typed = self.types.contains(&item.challenge_type) || (self.teams && item.teams);
        let typed = typed || (self.types.is_empty() && self.require_tag.is_some());
        typed
            && self.require_tag.as_ref().is_none_or(|t| item.tags.contains(t))
            && !self.exclude_tags.iter().any(|t| item.tags.contains(t))
            && !(self.exclude_debug && item.debug_only)
    }
}

/// The retail rules (b1054 §1; types: Tutorial 0, Film 7, Street 8, OwnTheSpot 9, Vert 15, DeathRace 19, Follow 23,
/// HallOfMeat 31, OneUp 32, AnywherePhoto 37, SpotPhoto 38, AnywhereFilm 39, OwnTheLot 52). `skateschool`'s body is
/// commented out in retail: it never accepts, so its folder is always dropped.
pub fn retail_folder_rules() -> BTreeMap<String, FolderRule> {
    let pro = || vec!["Pro".to_owned()];
    let pro_gs = || vec!["Pro".to_owned(), "GettingStarted".to_owned()];
    let rule = |types: &[u32], exclude: Vec<String>| FolderRule { types: types.to_vec(), exclude_tags: exclude, ..Default::default() };
    let mut rules = BTreeMap::new();
    rules.insert("sp_team".into(), FolderRule { types: vec![37, 39], teams: true, ..Default::default() });
    rules.insert("1up".into(), rule(&[32], pro()));
    rules.insert("deathrace".into(), rule(&[19], pro()));
    rules.insert("film".into(), rule(&[7], pro_gs()));
    rules.insert("followtheleader".into(), rule(&[23], pro_gs()));
    rules.insert("gettingstarted".into(), FolderRule { require_tag: Some("GettingStarted".into()), ..Default::default() });
    rules.insert("hallofmeat".into(), rule(&[31], pro()));
    rules.insert("otl".into(), FolderRule { types: vec![52], exclude_debug: true, ..Default::default() });
    rules.insert("ots".into(), rule(&[9], Vec::new()));
    rules.insert("photo".into(), rule(&[38], pro_gs()));
    rules.insert("pro".into(), FolderRule { require_tag: Some("Pro".into()), ..Default::default() });
    rules.insert("promotional".into(), rule(&[37, 39], Vec::new()));
    rules.insert("skateschool".into(), FolderRule::default());
    rules.insert("street".into(), rule(&[8], pro()));
    rules.insert("training".into(), rule(&[0], pro()));
    rules.insert("vert".into(), rule(&[15], pro()));
    rules
}

pub fn retail_tab_rules() -> BTreeMap<String, TabRule> {
    [("active", TabRule::Active), ("completed", TabRule::Passed), ("all", TabRule::All)].into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// The Challenge Map's challenge pool: the `challenges` rows (class 0x8876E0B556740E36) in vault order (b1054 §3).
pub fn pool(entries: &[Collection]) -> Vec<String> {
    entries.iter().filter(|e| class_is(e, "challenges")).map(|e| e.key.clone()).collect()
}

/// A challenge folder of the tree: its node, its tab's rule and its own filter key.
#[derive(Clone, Debug, PartialEq)]
pub struct ChallengeFolder {
    pub folder: EntryRef,
    pub tab: EntryRef,
    pub tab_rule: TabRule,
    pub filter: String,
}

/// The listing rows under `root` whose filter is a challenge folder rule, under a tab whose filter is a tab rule.
pub fn challenge_folders(entries: &[Collection], root: &str, folders: &BTreeMap<String, FolderRule>,
    tabs: &BTreeMap<String, TabRule>) -> Vec<ChallengeFolder> {
    let names: BTreeMap<u64, &str> = KNOWN_LISTINGS.iter().map(|n| (hash(n), *n)).collect();
    let id_of = |key: &str| names.get(&key_hash(key)).map_or_else(|| key.to_owned(), |n| (*n).to_owned());
    let filters: BTreeMap<u64, &Collection> = entries.iter().filter(|e| class_is(e, "map_filter")).map(|e| (key_hash(&e.key), e)).collect();
    let listings: Vec<&Collection> = entries.iter().filter(|e| class_is(e, "map_listing")).collect();
    let filter_of = |row: &Collection| ref_spec(row, LISTING_FILTER).filter(|(c, _)| *c == hash("map_filter"))
        .and_then(|(_, k)| filters.get(&k)).map(|f| f.key.clone());
    let mut out = Vec::new();
    for tab in listings.iter().filter(|r| key_hash(&r.parent) == key_hash(root)) {
        let Some(rule) = filter_of(tab).and_then(|f| tabs.get(&f).copied()) else { continue };
        let tab_entry = EntryRef::map(EntryKind::MapListing, &id_of(&tab.key));
        for folder in listings.iter().filter(|r| key_hash(&r.parent) == key_hash(&tab.key)) {
            if let Some(filter) = filter_of(folder).filter(|f| folders.contains_key(f)) {
                out.push(ChallengeFolder { folder: EntryRef::map(EntryKind::MapListing, &id_of(&folder.key)), tab: tab_entry.clone(),
                    tab_rule: rule, filter });
            }
        }
    }
    out
}

/// The challenge leaves of every folder for the player's status (82D0C1C0 tab pool, then 82D0C890 the folder rule,
/// b1054 §2): an item passes its tab's rule and the folder's. `pool` keeps the map's item pool order (the
/// skatercollections `challenges` rows, DebugOnly ones left out); leaves sort by their label's text like the other
/// folders (82647EA0). Returns the leaves and the folders left empty (retail drops those, b1054 §2).
pub fn challenge_leaves(folders: &[ChallengeFolder], rules: &BTreeMap<String, FolderRule>, pool: &[&ChallengeItem],
    active: &dyn Fn(&str) -> bool, passed: &dyn Fn(&str) -> bool, label_text: &dyn Fn(&str) -> Option<String>)
    -> (Vec<MapNode>, Vec<EntryRef>) {
    let mut nodes = Vec::new();
    let mut empty = Vec::new();
    for f in folders {
        let Some(rule) = rules.get(&f.filter) else { continue };
        let mut leaves: Vec<(String, &ChallengeItem)> = pool.iter().copied()
            .filter(|i| match f.tab_rule { TabRule::Active => active(&i.id), TabRule::Passed => passed(&i.id), TabRule::All => true })
            .filter(|i| rule.accepts(i))
            .map(|i| {
                let label = i.title_id.clone().unwrap_or_else(|| i.id.clone());
                (label_text(&label).unwrap_or_else(|| label.clone()), i)
            })
            .collect();
        if leaves.is_empty() {
            empty.push(f.folder.clone());
            continue;
        }
        leaves.sort_by(|a, b| collate(&a.0, &b.0, COLLATION));
        for (order, (_, i)) in leaves.into_iter().enumerate() {
            nodes.push(MapNode { entry: EntryRef::map(EntryKind::Challenge, &i.id), parent: Some(f.folder.clone()),
                label: i.title_id.clone().unwrap_or_else(|| i.id.clone()), order: order as i32 });
        }
    }
    (nodes, empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::Field;

    fn field(data: &str) -> Field {
        Field { type_name: String::new(), data: data.to_owned(), array: None }
    }
    fn row(class: &str, key: &str, parent: &str, fields: &[(&str, String)]) -> Collection {
        Collection {
            class_name: numeric_name(class),
            key: key.to_owned(),
            parent: parent.to_owned(),
            fields: fields.iter().map(|(k, v)| (k.to_string(), field(v))).collect(),
            source: String::new(),
            sha256: String::new(),
        }
    }
    fn refspec(class: &str, key: &str) -> String {
        format!("{:016X}{:016X}", hash(class), hash(key))
    }
    /// A typed RefSpec (`ssRefSpec_map_category`): the key, then padding.
    fn typed(key: &str) -> String {
        format!("{:016X}0000000000000000", hash(key))
    }

    #[test]
    fn a_location_shows_its_districts_world_map_inherited_up_the_world_records() {
        let rows = [
            row("world", "default", "", &[]),
            row("world", "default_skate3", "default", &[]),
            row("world", "dist_skateschool", "default_skate3", &[]),
            row("world", "dist_university", "default_skate3", &[]),
            row("fe_locations", "school_spot", "skate3", &[(LOCATION_DISTRICT, typed("dist_skateschool"))]),
            row("fe_locations", "uni_spot", "skate3", &[(LOCATION_DISTRICT, typed("dist_university"))]),
            row("fe_locations", "nowhere", "skate3", &[]),
        ];
        let named = |pairs: &[(&str, &str)]| pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeMap<_, _>>();
        let worlds = named(&[("default", "map1024"), ("default_skate3", "map_portcarverton"), ("dist_skateschool", "map_skateschool")]);
        let maps = location_maps(&rows, &worlds);
        assert_eq!(maps, named(&[("school_spot", "map_skateschool"), ("uni_spot", "map_portcarverton")]));
        assert!(location_maps(&rows, &BTreeMap::new()).is_empty(), "an older menu set names no maps");
    }

    #[test]
    fn a_dlc_location_whose_district_is_no_world_takes_its_world_field() {
        let rows = [
            row("world", "default", "", &[]),
            row("world", "dist_maloofmoneycupdlc", "default", &[]),
            row("fe_locations", "dist_maloofmoneycupdlc", "skate3", &[(LOCATION_DISTRICT, typed("maloofmoneycupdlc")),
                (LOCATION_WORLD, typed("dist_maloofmoneycupdlc"))]),
        ];
        let worlds: BTreeMap<String, String> = [("default".to_string(), "map1024".to_string()),
            ("dist_maloofmoneycupdlc".to_string(), "DLC_Maloof_Map_512".to_string())].into();
        assert_eq!(location_maps(&rows, &worlds).get("dist_maloofmoneycupdlc").map(String::as_str), Some("DLC_Maloof_Map_512"));
    }

    #[test]
    fn the_locations_tab_lists_each_districts_locations_in_its_folder() {
        let tab = format!("Hash_{:016X}", hash("progression_locations"));
        let folder = "Hash_00000000000000AA";
        let entries = vec![
            row("map_listing", &tab, "progression", &[(LISTING_FILTER, refspec("map_filter", "location")), (LISTING_ORDER, "00030000".into())]),
            row("map_listing", folder, &tab, &[(LISTING_FILTER, refspec("map_filter", "downtown")), (LISTING_ORDER, "00010000".into())]),
            row("map_filter", "downtown", "location", &[(FILTER_TITLE, "ID_DISTRICT_DOWNTOWN".into())]),
            // Retail lists by World (b447), not District: a District naming another place does not matter.
            row("fe_locations", "dist_downtown_aletown", "skate3", &[(LOCATION_LABEL, "ID_LOT_DT_01C".into()),
                (LOCATION_DISTRICT, typed("dist_maloof")), (LOCATION_WORLD, typed("dist_downtown")), (LOCATION_SHOWN, "01000000".into())]),
            row("fe_locations", "dist_university_x", "skate3", &[(LOCATION_WORLD, typed("dist_university")), (LOCATION_SHOWN, "01000000".into())]),
        ];
        let nodes = load(&entries, "progression", &LocationRules::default(), &LocationUnlocks::default(), &|_| None);
        let ids: Vec<String> = nodes.iter().map(|n| n.entry.hook_id()).collect();
        assert_eq!(ids, ["map_listing:progression", "map_listing:progression_locations", "map_listing:Hash_00000000000000AA", "fe_locations:dist_downtown_aletown"]);
        let aletown = nodes.last().unwrap();
        assert_eq!((aletown.parent.as_ref().map(|p| p.id.as_str()), aletown.label.as_str()), (Some(folder), "ID_LOT_DT_01C"));
        assert_eq!((nodes[2].label.as_str(), nodes[2].order, nodes[1].order), ("ID_DISTRICT_DOWNTOWN", 1, 3));
        // A mod rule moves the university location under the folder instead.
        let mut rules = LocationRules::default();
        rules.0.insert("downtown".into(), "dist_university".into());
        assert_eq!(load(&entries, "progression", &rules, &LocationUnlocks::default(), &|_| None).last().unwrap().entry.id, "dist_university_x");
    }

    #[test]
    fn the_dlc_folders_list_by_world_and_the_skate_park_folder_lists_park_worlds() {
        let tab = format!("Hash_{:016X}", hash("progression_locations"));
        let entries = vec![
            row("map_listing", &tab, "progression", &[(LISTING_FILTER, refspec("map_filter", "location"))]),
            row("map_listing", "Hash_00000000000000AA", &tab, &[(LISTING_FILTER, refspec("map_filter", "maloofmoneycupdlc"))]),
            row("map_listing", "Hash_00000000000000BB", &tab, &[(LISTING_FILTER, refspec("map_filter", "skatepark"))]),
            row("map_filter", "maloofmoneycupdlc", "location", &[]),
            row("map_filter", "skatepark", "location", &[]),
            row("fe_locations", "skate3", "", &[]),
            row("fe_locations", "dist_maloofmoneycupdlc", "skate3", &[(LOCATION_WORLD, typed("dist_maloofmoneycupdlc")), (LOCATION_SHOWN, "01000000".into())]),
            row("world", "default_skateparks", "", &[("IsSkatePark", "01".into())]),
            row("world", "dist_megapark_full", "default_skateparks", &[(WORLD_LABEL, "ID_WORLD_SKATEPARK_MEGAPARK_FULL".into()),
                ("AllowInSkateParksTab", "01".into())]),
            row("world", "dist_megapark", "default_skateparks", &[]),
            row("world", "dist_downtown", "", &[]),
        ];
        let nodes = load(&entries, "progression", &LocationRules::default(), &LocationUnlocks::default(), &|_| None);
        let under = |folder: &str| -> Vec<String> {
            nodes.iter().filter(|n| n.parent.as_ref().is_some_and(|p| p.id == folder)).map(|n| n.entry.hook_id()).collect()
        };
        assert_eq!(under("Hash_00000000000000AA"), ["fe_locations:dist_maloofmoneycupdlc"], "a DLC folder lists its pack's location by World");
        assert_eq!(under("Hash_00000000000000BB"), ["world:dist_megapark_full"], "both flags, inherited; the Empty row is off the tab");
        let park = nodes.iter().find(|n| n.entry.id == "dist_megapark_full").unwrap();
        assert_eq!(park.label, "ID_WORLD_SKATEPARK_MEGAPARK_FULL");
    }

    #[test]
    fn a_folder_sorts_by_category_order_then_label_text() {
        let tab = format!("Hash_{:016X}", hash("progression_locations"));
        let entries = vec![
            row("map_listing", &tab, "progression", &[(LISTING_FILTER, refspec("map_filter", "location"))]),
            row("map_listing", "Hash_00000000000000BB", &tab, &[(LISTING_FILTER, refspec("map_filter", "skatepark"))]),
            row("map_filter", "skatepark", "location", &[]),
            row("map_category", "late", "", &[(CATEGORY_ORDER, "00000258".into())]),
            row("map_category", "early", "", &[(CATEGORY_ORDER, "00000064".into())]),
            row("world", "parks", "", &[("IsSkatePark", "01".into())]),
            row("world", "z_first_in_data", "parks", &[(WORLD_LABEL, "ID_ZEN".into()), (LOCATION_DISTRICT, typed("late")), ("AllowInSkateParksTab", "01".into())]),
            row("world", "b_full", "parks", &[(WORLD_LABEL, "ID_B_FULL".into()), (LOCATION_DISTRICT, typed("early")), ("AllowInSkateParksTab", "01".into())]),
            row("world", "b_basic", "parks", &[(WORLD_LABEL, "ID_B_BASIC".into()), (LOCATION_DISTRICT, typed("early")), ("AllowInSkateParksTab", "01".into())]),
        ];
        let texts = |id: &str| match id { "ID_B_FULL" => Some("PCU Training - Full".to_owned()),
            "ID_B_BASIC" => Some("PCU Training - Basic".to_owned()), _ => None };
        let nodes = load(&entries, "progression", &LocationRules::default(), &LocationUnlocks::default(), &texts);
        let parks: Vec<(&str, i32)> = nodes.iter().filter(|n| n.entry.kind == EntryKind::World).map(|n| (n.entry.id.as_str(), n.order)).collect();
        assert_eq!(parks, [("b_basic", 1), ("b_full", 2), ("z_first_in_data", 4)], "category 100 before 600, then Basic before Full");
        // b454: a header row before each category run.
        let folder: Vec<(EntryKind, &str)> = nodes.iter().filter(|n| n.parent.as_ref().is_some_and(|p| p.id == "Hash_00000000000000BB"))
            .map(|n| (n.entry.kind, n.entry.id.as_str())).collect();
        assert_eq!(folder, [(EntryKind::MapCategory, "early"), (EntryKind::World, "b_basic"), (EntryKind::World, "b_full"),
            (EntryKind::MapCategory, "late"), (EntryKind::World, "z_first_in_data")]);
        // The folder flag drops the header only when the folder holds one category.
        let mut flagged = entries.clone();
        flagged[1].fields.insert(FOLDER_LONE_HEADERLESS.into(), field("01"));
        let headers = |e: &[Collection]| load(e, "progression", &LocationRules::default(), &LocationUnlocks::default(), &texts)
            .iter().filter(|n| n.entry.kind == EntryKind::MapCategory).count();
        assert_eq!(headers(&flagged), 2, "two categories keep their headers");
        flagged.retain(|r| r.key != "z_first_in_data");
        assert_eq!(headers(&flagged), 0, "a lone category under the flag has none");
        assert_eq!(collate("a", "B", COLLATION), std::cmp::Ordering::Less, "a before B: case interleaved");
        assert_eq!(collate("A", "a", COLLATION), std::cmp::Ordering::Less);
        assert_eq!(collate("ab", "abc", COLLATION), std::cmp::Ordering::Equal, "a prefix compares equal");
        assert_eq!(collate("z", "~", COLLATION), std::cmp::Ordering::Less, "unlisted characters sort last");
    }

    #[test]
    fn a_location_without_the_shown_flag_needs_its_unlock() {
        let tab = format!("Hash_{:016X}", hash("progression_locations"));
        let entries = vec![
            row("map_listing", &tab, "progression", &[(LISTING_FILTER, refspec("map_filter", "location"))]),
            row("map_listing", "Hash_00000000000000AA", &tab, &[(LISTING_FILTER, refspec("map_filter", "university"))]),
            row("map_filter", "university", "location", &[]),
            row("fe_locations", "uni", "", &[(LOCATION_WORLD, typed("dist_university")), (LOCATION_SHOWN, "01000000".into())]),
            row("fe_locations", "dist_university_library", "uni", &[(LOCATION_NAME, "dist_university_library".into())]),
            // b450: `gamestart` clears its parent's flag, so its name must be unlocked.
            row("fe_locations", "gamestart", "dist_university_library", &[(LOCATION_NAME, "gamestart".into()), (LOCATION_SHOWN, "00000000".into())]),
            row("fe_locations", "late_spot", "uni", &[(LOCATION_NAME, "late_spot".into()), (LOCATION_SHOWN, "00000000".into())]),
        ];
        let leaves = |unlocks: &LocationUnlocks| -> Vec<String> {
            load(&entries, "progression", &LocationRules::default(), unlocks, &|_| None).iter()
                .filter(|n| n.entry.kind == EntryKind::FeLocation).map(|n| n.entry.id.clone()).collect()
        };
        assert_eq!(leaves(&LocationUnlocks::default()), ["dist_university_library", "late_spot", "uni"], "inherited flag; gamestart locked");
        let mut unlocks = LocationUnlocks::default();
        unlocks.locked.insert("late_spot".into());
        assert_eq!(leaves(&unlocks), ["dist_university_library", "uni"], "a locked name without a grant is hidden");
        unlocks.granted.insert("late_spot".into());
        assert_eq!(leaves(&unlocks).len(), 3, "a grant (DLC line, mod) beats the lock");
        unlocks.all = true;
        assert_eq!(leaves(&unlocks).len(), 4, "all_locations_unlock");
        // setup's dlc-unlocks.json: only LOCATIONS lines grant.
        let root = std::env::temp_dir().join(format!("skate-unlocks-{}", std::process::id()));
        std::fs::create_dir_all(root.join("private")).unwrap();
        std::fs::write(root.join("private/dlc-unlocks.json"), r#"{"version":1,"unlocks":[
            {"source":"Time is Money","file":"a.unlock","kind":"LOCATIONS","name":"late_spot"},
            {"source":"x","file":"b.unlock","kind":"CHARACTERS","name":"gamestart"}]}"#).unwrap();
        let loaded = LocationUnlocks::load(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(loaded.granted, ["late_spot".to_owned()].into());
        assert_eq!(LocationUnlocks::load(std::path::Path::new("missing")), LocationUnlocks::default());
    }
    /// The real tree: four tabs, the seven location folders in retail order, Downtown's 14 locations.
    #[test]
    #[ignore = "requires the converted vault and challenges.json (SKATE3_ASSET_ROOT)"]
    fn the_retail_challenge_folders_fill_from_the_installed_items() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let collections = crate::collections::Collections::load(&root).unwrap();
        let folders = challenge_folders(collections.entries(), "progression", &retail_folder_rules(), &retail_tab_rules());
        #[derive(serde::Deserialize)]
        struct File { map_items: Vec<ChallengeItem> }
        let file: File = serde_json::from_slice(&std::fs::read(root.join("private/challenges.json")).unwrap()).unwrap();
        let pool: Vec<&ChallengeItem> = file.map_items.iter().filter(|i| i.pool && !i.debug_only).collect();
        eprintln!("folders {} pool {} items {}", folders.len(), pool.len(), file.map_items.len());
        assert!(folders.len() >= 30, "the challenge folders of the Active, Completed and All tabs");
        let (leaves, _) = challenge_leaves(&folders, &retail_folder_rules(), &pool, &|id| id == "tut_sksc_01" || id == "ftl_univ_02", &|_| false, &|_| None);
        let active: Vec<(&str, &str)> = leaves.iter().filter(|n| folders.iter().any(|f| Some(&f.folder) == n.parent.as_ref() && f.tab_rule == TabRule::Active))
            .map(|n| (n.parent.as_ref().unwrap().id.as_str(), n.entry.id.as_str())).collect();
        eprintln!("active leaves {active:?}");
        let base = load(collections.entries(), "progression", &LocationRules::default(), &LocationUnlocks::default(), &|_| None);
        let tab = EntryRef::map(EntryKind::MapListing, "progression_active");
        for n in base.iter().filter(|n| n.parent.as_ref() == Some(&tab)) {
            let kids = base.iter().filter(|k| k.parent.as_ref() == Some(&n.entry)).count();
            let rule = folders.iter().find(|f| f.folder == n.entry).map(|f| f.filter.as_str());
            eprintln!("tab child {:?} {:?} label {:?} order {} base kids {kids} rule {rule:?}", n.entry.kind, n.entry.id, n.label, n.order);
        }
        assert!(active.iter().any(|(_, id)| *id == "tut_sksc_01") && active.iter().any(|(_, id)| *id == "ftl_univ_02"));
    }

    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_retail_progression_tree() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let collections = crate::collections::Collections::load(&root).unwrap();
        let data = skate_core::menus::MenuData { map_nodes: load(collections.entries(), "progression", &LocationRules::default(), &LocationUnlocks::default(), &|_| None), ..Default::default() };
        let root_entry = EntryRef::map(EntryKind::MapListing, "progression");
        let tabs: Vec<&str> = data.map_children(&root_entry).iter().map(|n| n.entry.id.as_str()).collect();
        assert_eq!(tabs, ["progression_active", "progression_completed", "progression_locations", "progression_all"]);
        let folders = data.map_children(&EntryRef::map(EntryKind::MapListing, "progression_locations"));
        let labels: Vec<&str> = folders.iter().map(|n| n.label.as_str()).collect();
        // The disc's seven folders, then one per DLC pack installed (b447: Maloof NYC, Hawaiian Dream, The Sanatorium,
        // Art Gallery with the user's packs).
        assert!(labels.len() >= 7, "{labels:?}");
        assert_eq!(labels[0], "ID_DISTRICT_DOWNTOWN");
        let downtown = data.map_children(&folders[0].entry);
        assert_eq!(downtown.len(), 14);
        // b450 / b458: University lists 10 (gamestart's flag is 0 and no package grants it), skate.School 1 (no
        // dist_skateschool), as in the retail video.
        assert_eq!(data.map_children(&folders[2].entry).len(), 10);
        assert_eq!(data.map_children(&folders[3].entry).len(), 1);
        // skate.Park (folder 4): the park worlds with both tab flags; 5 on the disc (+3 with the Create / San Van packs).
        // b454: a map_category header before each park's category run (8 parks + 7 headers with the packs).
        let parks = data.map_children(&folders[4].entry);
        let worlds = parks.iter().filter(|n| n.entry.kind == EntryKind::World).count();
        let headers = parks.iter().filter(|n| n.entry.kind == EntryKind::MapCategory).count();
        eprintln!("SKATE_PARK rows {:?}", parks.iter().map(|n| n.label.as_str()).collect::<Vec<_>>());
        assert!(worlds >= 5 && worlds + headers == parks.len() && parks[0].entry.kind == EntryKind::MapCategory, "{parks:?}");
        assert!(parks.iter().any(|n| n.entry.id == "dist_megapark_full"));
        // Every DLC folder lists its pack's location.
        assert!(folders[7..].iter().all(|f| !data.map_children(&f.entry).is_empty()), "{labels:?}");
        assert!(downtown.iter().all(|n| n.entry.kind == EntryKind::FeLocation));
        assert!(downtown.iter().any(|n| n.entry.id == "dist_downtown_aletown"));
        // Markers (b264 / b265): every career location's icon is mini_teleport; Downtown's world offset and bounds.
        let markers = location_markers(&collections);
        let aletown = &markers["dist_downtown_aletown"];
        assert_eq!(aletown.icon, "mini_teleport");
        assert_eq!(aletown.offset, [225.0, -175.0]);
        assert_eq!(aletown.bounds, [2050, -850, 1050, -1250, 512, 512]);
        assert!(downtown.iter().all(|n| markers.get(&n.entry.id).is_some_and(|m| m.icon == "mini_teleport")));
    }

    /// fe_map_icons (b571): the script default `race_gate`, the called skater's `skater`, the teleport's 500 m.
    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_retail_map_icons() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let icons = map_icons(&crate::collections::Collections::load(&root).unwrap());
        eprintln!("MAP_ICONS {}: {:?}", icons.len(), icons.iter().map(|(k, v)| (k.as_str(), v.texture.as_str(), v.max_distance)).collect::<Vec<_>>());
        assert_eq!(icons["race_gate"].texture, "mini_gate");
        assert_eq!(icons["skater"].texture, "mini_skater");
        assert_eq!(icons["teleport"].max_distance, 500.0);
        // The friend-film key script 82849DE8 uses, by name.
        assert_eq!(map_icon(&icons, "challenge_friendfilm_visualindicator").map(|i| i.texture.as_str()), Some("mini_teleport"));
    }

    #[test]
    fn challenge_folders_keep_what_passes_the_tab_and_the_folder_rule_and_drop_empty_ones() {
        let item = |id: &str, t: u32, tags: &[&str]| ChallengeItem { id: id.into(), challenge_type: t, title_id: Some(id.into()),
            tags: tags.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        let items = [item("photo_a", 38, &[]), item("1up_a", 32, &["Pro"]), item("film_anyw_a", 39, &[]), item("film_gs", 7, &["GettingStarted"]),
            item("photo_done", 38, &[])];
        let pool: Vec<&ChallengeItem> = items.iter().collect();
        let tab = EntryRef::map(EntryKind::MapListing, "progression_active");
        let folder = |f: &str| ChallengeFolder { folder: EntryRef::map(EntryKind::MapListing, f), tab: tab.clone(), tab_rule: TabRule::Active, filter: f.into() };
        let folders: Vec<ChallengeFolder> = ["photo", "1up", "pro", "promotional", "sp_team", "gettingstarted", "film"].iter().map(|f| folder(f)).collect();
        let rules = retail_folder_rules();
        let (leaves, empty) = challenge_leaves(&folders, &rules, &pool, &|id| id != "photo_done", &|id| id == "photo_done", &|_| None);
        let at = |f: &str| -> Vec<&str> { leaves.iter().filter(|n| n.parent.as_ref().is_some_and(|p| p.id == f)).map(|n| n.entry.id.as_str()).collect() };
        assert_eq!(at("photo"), ["photo_a"], "a passed challenge is not active");
        assert_eq!(at("pro"), ["1up_a"], "the 1-Ups are Pro challenges");
        assert_eq!(at("promotional"), ["film_anyw_a"]);
        assert_eq!(at("sp_team"), ["film_anyw_a"], "anywhere types also sit in the team folder");
        assert_eq!(at("gettingstarted"), ["film_gs"]);
        assert!(at("film").is_empty(), "a GettingStarted film stays out of the film folder");
        let empty: Vec<&str> = empty.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(empty, ["1up", "film"], "empty folders are dropped (b1054 §2)");
        assert!(leaves.iter().all(|n| n.entry.kind == EntryKind::Challenge));
    }
}
