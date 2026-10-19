//! The retail Challenge Map tree (FE screen 17) from the vault collections (skatercollections), as menu map nodes.
//!
//! [data] (research b244 / b255): `map_listing` rows form the tree by parent (the root is the mode key, `progression`
//! for career and free skate, `online` online; its children are the tabs `progression_locations` / `_active` /
//! `_completed` / `_all`). Each row names its folder's filter (`Hash_E0762EA3F90C9EEB`, a RefSpec of class + key, a
//! `map_filter` or a `map_category`) and a sort order (`Hash_982BD1EAE2334504`, u16 in the high half). Retail fills a
//! folder by running its filter's Lua over the item classes; the location filters (`downtown` ...) keep the
//! `fe_locations` rows whose district (`Hash_50D9293B818507EC`, a `map_category` RefSpec) is the filter's district
//! (`LocationFilterHelpers.IsLocationInWorld(item, "dist_downtown")`). Those are ported as data
//! ([`LocationRules`], retail defaults, mod-overridable); other filters (challenges, skate parks) are not ported yet,
//! so their folders have no leaves and stay greyed. Retail's unlock filter (`Unlock.IsLocationUnlocked`) is not
//! applied: every location is listed.
use crate::attrib_hash::{hash, numeric_name};
use crate::collections::Collection;
use skate_core::menus::{EntryKind, EntryRef, MapNode};
use std::collections::BTreeMap;

const LISTING_FILTER: &str = "Hash_E0762EA3F90C9EEB";
const LISTING_ORDER: &str = "Hash_982BD1EAE2334504";
const FILTER_TITLE: &str = "Hash_6766EFD84D74F4F0";
const LOCATION_LABEL: &str = "Hash_174B301910A902C9";
const LOCATION_DISTRICT: &str = "Hash_50D9293B818507EC";

/// Which `map_category` district each location filter lists (retail's filter Lua, as data). Mods may change it.
#[derive(Clone, Debug, PartialEq)]
pub struct LocationRules(pub BTreeMap<String, String>);

impl Default for LocationRules {
    fn default() -> Self {
        let pairs = [
            ("downtown", "dist_downtown"),
            ("industrial", "dist_industrial"),
            ("university", "dist_university"),
            ("skate_school", "dist_skateschool"),
            ("maloof", "dist_maloof"),
            ("blackbox_park", "dist_blackboxpark"),
        ];
        Self(pairs.into_iter().map(|(a, b)| (a.to_owned(), b.to_owned())).collect())
    }
}

/// Known `map_listing` names, so the tabs' hook ids read as words (other rows keep their vault key, `Hash_...`).
const KNOWN_LISTINGS: [&str; 10] = [
    "progression", "progression_locations", "progression_active", "progression_completed", "progression_all",
    "online", "online_competitive", "online_team", "freeskate", "location",
];

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
/// `world`'s tMapInfo (b257 / b259): word 0 = the map picture's name (`map_skateschool`); inherited from the parent.
const WORLD_MAP_INFO: &str = "Hash_20BBC594CD2FAEF8";

/// Each location's map picture name (FE `82649FF8`: the location's district names its `world` record, whose
/// tMapInfo names the picture), by location key. Records without a name are left out (the default map).
pub fn location_maps(collections: &crate::collections::Collections) -> BTreeMap<String, String> {
    let entries = collections.entries();
    let worlds: BTreeMap<u64, &str> =
        entries.iter().filter(|e| class_is(e, WORLD_CLASS)).map(|e| (key_hash(&e.key), e.key.as_str())).collect();
    entries
        .iter()
        .filter(|e| class_is(e, "fe_locations"))
        .filter_map(|loc| {
            let world = worlds.get(&typed_ref_key(loc, LOCATION_DISTRICT)?)?;
            let name = collections.field(WORLD_CLASS, world, WORLD_MAP_INFO).ok()?.text.clone()?;
            (!name.is_empty()).then(|| (loc.key.clone(), name))
        })
        .collect()
}

/// A `Hash_XXXXXXXXXXXXXXXX` key, or `hash(name)` for a readable one.
fn key_hash(key: &str) -> u64 {
    key.strip_prefix("Hash_").and_then(|h| u64::from_str_radix(h, 16).ok()).unwrap_or_else(|| hash(key))
}

/// The tree under `root` (a mode key, `progression`), as map nodes in data order: the root and its `map_listing`
/// tabs / folders (kind map_listing, id the readable name when known, else the vault key) and the location leaves.
pub fn load(entries: &[Collection], root: &str, rules: &LocationRules) -> Vec<MapNode> {
    let names: BTreeMap<u64, &str> = KNOWN_LISTINGS.iter().map(|n| (hash(n), *n)).collect();
    let id_of = |key: &str| names.get(&key_hash(key)).map_or_else(|| key.to_owned(), |n| (*n).to_owned());
    let filters: BTreeMap<u64, &Collection> = entries.iter().filter(|e| class_is(e, "map_filter")).map(|e| (key_hash(&e.key), e)).collect();
    let listings: Vec<&Collection> = entries.iter().filter(|e| class_is(e, "map_listing")).collect();
    let locations: Vec<&Collection> = entries.iter().filter(|e| class_is(e, "fe_locations")).collect();
    let root_entry = EntryRef::map(EntryKind::MapListing, root);
    let mut nodes = vec![MapNode { entry: root_entry.clone(), parent: None, label: root.to_owned(), order: 0 }];
    let mut queue = vec![(key_hash(root), root_entry)];
    while let Some((parent_hash, parent)) = queue.pop() {
        for row in listings.iter().filter(|r| key_hash(&r.parent) == parent_hash) {
            let entry = EntryRef::map(EntryKind::MapListing, &id_of(&row.key));
            let order = text(row, LISTING_ORDER).and_then(|d| d.get(0..4)).and_then(|d| i32::from_str_radix(d, 16).ok()).unwrap_or(0);
            let filter = ref_spec(row, LISTING_FILTER).filter(|(class, _)| *class == hash("map_filter")).and_then(|(_, key)| filters.get(&key));
            let label = filter.and_then(|f| text(f, FILTER_TITLE)).unwrap_or(&entry.id).to_owned();
            nodes.push(MapNode { entry: entry.clone(), parent: Some(parent.clone()), label, order });
            // The location leaves of a location filter.
            if let Some(district) = filter.and_then(|f| rules.0.get(&f.key)) {
                let district = hash(district);
                for (i, loc) in locations.iter().enumerate() {
                    if typed_ref_key(loc, LOCATION_DISTRICT) == Some(district) {
                        let label = text(loc, LOCATION_LABEL).unwrap_or(&loc.key).to_owned();
                        nodes.push(MapNode { entry: EntryRef::map(EntryKind::FeLocation, &loc.key), parent: Some(entry.clone()), label, order: i as i32 });
                    }
                }
            }
            queue.push((key_hash(&row.key), entry));
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::Field;

    fn field(data: &str) -> Field {
        Field { type_name: String::new(), data: data.to_owned(), array: None, text: None }
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
    fn the_locations_tab_lists_each_districts_locations_in_its_folder() {
        let tab = format!("Hash_{:016X}", hash("progression_locations"));
        let folder = "Hash_00000000000000AA";
        let entries = vec![
            row("map_listing", &tab, "progression", &[(LISTING_FILTER, refspec("map_filter", "location")), (LISTING_ORDER, "00030000".into())]),
            row("map_listing", folder, &tab, &[(LISTING_FILTER, refspec("map_filter", "downtown")), (LISTING_ORDER, "00010000".into())]),
            row("map_filter", "downtown", "location", &[(FILTER_TITLE, "ID_DISTRICT_DOWNTOWN".into())]),
            row("fe_locations", "dist_downtown_aletown", "skate3", &[(LOCATION_LABEL, "ID_LOT_DT_01C".into()), (LOCATION_DISTRICT, typed("dist_downtown"))]),
            row("fe_locations", "dist_university_x", "skate3", &[(LOCATION_DISTRICT, typed("dist_university"))]),
        ];
        let nodes = load(&entries, "progression", &LocationRules::default());
        let ids: Vec<String> = nodes.iter().map(|n| n.entry.hook_id()).collect();
        assert_eq!(ids, ["map_listing:progression", "map_listing:progression_locations", "map_listing:Hash_00000000000000AA", "fe_locations:dist_downtown_aletown"]);
        let aletown = nodes.last().unwrap();
        assert_eq!((aletown.parent.as_ref().map(|p| p.id.as_str()), aletown.label.as_str()), (Some(folder), "ID_LOT_DT_01C"));
        assert_eq!((nodes[2].label.as_str(), nodes[2].order, nodes[1].order), ("ID_DISTRICT_DOWNTOWN", 1, 3));
        // A mod rule moves the university location under the folder instead.
        let mut rules = LocationRules::default();
        rules.0.insert("downtown".into(), "dist_university".into());
        assert_eq!(load(&entries, "progression", &rules).last().unwrap().entry.id, "dist_university_x");
    }

    /// The real tree: four tabs, the seven location folders in retail order, Downtown's 14 locations.
    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_retail_progression_tree() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"));
        let collections = crate::collections::Collections::load(&root).unwrap();
        let data = skate_core::menus::MenuData { map_nodes: load(collections.entries(), "progression", &LocationRules::default()), ..Default::default() };
        let root_entry = EntryRef::map(EntryKind::MapListing, "progression");
        let tabs: Vec<&str> = data.map_children(&root_entry).iter().map(|n| n.entry.id.as_str()).collect();
        assert_eq!(tabs, ["progression_active", "progression_completed", "progression_locations", "progression_all"]);
        let folders = data.map_children(&EntryRef::map(EntryKind::MapListing, "progression_locations"));
        let labels: Vec<&str> = folders.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels.len(), 7, "{labels:?}");
        assert_eq!(labels[0], "ID_DISTRICT_DOWNTOWN");
        let downtown = data.map_children(&folders[0].entry);
        assert_eq!(downtown.len(), 14);
        assert!(downtown.iter().all(|n| n.entry.kind == EntryKind::FeLocation));
        assert!(downtown.iter().any(|n| n.entry.id == "dist_downtown_aletown"));
    }
}
