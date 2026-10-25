//! Career stats for the local player (`skate_core::career_stats`): fed from the fixed step, kept in
//! `settings/career-stats.json`, shown on the loading screen's Stats panel. Details:
//! docs/hails-additions/31-retail-menus.md "Career stats".
use bevy::prelude::*;
use skate_core::career_stats::{row, CareerStats, Format};
use std::collections::BTreeMap;

/// A position step longer than this in one fixed tick is a teleport or a map switch, not travel.
const MAX_STEP: f32 = 5.0;
/// How often changed totals are written.
const SAVE_SECONDS: f32 = 30.0;

/// The hit rows by ped entity key (0x826C0730 compares the entity key's hash64); any other ped counts as a bum.
const HIT_TYPES: [(&str, usize); 13] = [
    ("jock01", 1),
    ("teenage_female01", 2),
    ("teenage_male01", 3),
    ("adult_male01", 4),
    ("adult_female01", 5),
    ("business_male01", 6),
    ("business_female01", 7),
    ("skater_male01", 8),
    ("skater_female01", 9),
    ("tourist_male01", 10),
    ("tourist_female01", 11),
    ("security_street01", 12),
    ("security_nsz01", 12),
];
/// security_nsz02 also counts as a security guard.
const SECURITY_NSZ02: &str = "security_nsz02";

/// The ped type names by type (`ID_LIVINGWORLD_<TYPE>`), bum first.
const TYPE_NAMES: [&str; 14] = [
    "BUM", "JOCK", "TEENGIRL", "TEENGUY", "ADULTMALE", "ADULTFEMALE", "BUSINESSMAN", "BUSINESSWOMAN", "MALESKATER",
    "FEMALESKATER", "MALETOURIST", "FEMALETOURIST", "SECURITYGUARD", "GRANNY",
];

pub(crate) fn hit_type(entity: &str) -> usize {
    if entity == SECURITY_NSZ02 {
        return 12;
    }
    HIT_TYPES.iter().find(|(k, _)| *k == entity).map_or(0, |(_, t)| *t)
}

#[derive(Resource, Default)]
pub(crate) struct CareerStatsHost {
    pub stats: CareerStats,
    /// The table's labels and enabled flags (menu tables `career_stats`; empty before setup exported them).
    pub labels: Vec<String>,
    pub enabled: Vec<bool>,
    path: Option<std::path::PathBuf>,
    last: Option<[f32; 3]>,
    grind_run: Option<f32>,
    dirty: bool,
    since_save: f32,
}

impl CareerStatsHost {
    fn load(path: std::path::PathBuf, tables: Option<skate_data::menu_tables::MenuTables>) -> Self {
        let mut stats = CareerStats::default();
        if let Some(saved) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<BTreeMap<String, f32>>(&t).ok()) {
            for (k, v) in saved {
                if let Ok(i) = k.parse::<usize>() {
                    stats.record(i, v);
                }
            }
        }
        let rows = tables.map(|t| t.career_stats).unwrap_or_default();
        Self {
            stats,
            labels: rows.iter().map(|r| r.label.clone()).collect(),
            enabled: rows.iter().map(|r| r.enabled).collect(),
            path: Some(path),
            ..default()
        }
    }

    /// The totals as JSON (row index -> value, rows at 0 left out).
    fn snapshot(&self) -> Option<Vec<u8>> {
        let map: BTreeMap<String, f32> =
            self.stats.values.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(i, v)| (i.to_string(), *v)).collect();
        serde_json::to_vec_pretty(&map).ok()
    }

    fn save(&mut self) {
        if let (Some(path), Some(text)) = (self.path.clone(), self.snapshot()) {
            self.dirty = !write(&path, &text);
        }
    }

    /// Off the game thread: the snapshot is written on a short-lived thread (the exit save stays on this one).
    fn save_in_background(&mut self) {
        if let (Some(path), Some(text)) = (self.path.clone(), self.snapshot()) {
            self.dirty = false;
            if std::thread::Builder::new().name("career-stats-save".into()).spawn(move || write(&path, &text)).is_err() {
                self.dirty = true;
            }
        }
    }

    pub(crate) fn record(&mut self, index: usize, value: f32) {
        if value != 0.0 {
            self.stats.record(index, value);
            self.dirty = true;
        }
    }

    /// The Stats panel rows (label id, value text): the offline fetch's 16 at most, empty without the table.
    pub(crate) fn panel_rows(&self, language: &BTreeMap<String, String>, numbers: &crate::fe_language::NumberFormat, imperial: bool, rng: &mut dyn FnMut() -> u32) -> Vec<(String, String)> {
        let template = |id: &str| crate::fe_language::template(language, id).to_owned();
        let (metres, feet, seconds) = (template("ID_COMMON_METRIC_DISTANCE"), template("ID_COMMON_METRIC_NEW_FEET"), template("ID_COMMON_METRIC_TIME"));
        let int = |n: i64| numbers.int(n);
        // 0x8290E370: the whole part grouped like integers.
        let float = |x: f64, d: usize| {
            let text = numbers.float(x, d);
            let (whole, rest) = text.split_once(numbers.decimal).map_or((text.as_str(), None), |(w, r)| (w, Some(r)));
            let grouped = numbers.int(whole.parse::<i64>().unwrap_or(0));
            let grouped = if whole.starts_with('-') && !grouped.starts_with('-') { format!("-{grouped}") } else { grouped };
            rest.map_or(grouped.clone(), |r| format!("{grouped}{}{r}", numbers.decimal))
        };
        // NOT RETAIL YET: retail's locale currency format (0x8290DCB8); money rows are disabled in the table.
        let money = |x: f64| format!("${}", numbers.int(x as i64));
        let ped_type = |t: usize| TYPE_NAMES.get(t).map(|n| format!("ID_LIVINGWORLD_{n}")).unwrap_or_default();
        let format = Format { imperial, metres: &metres, feet: &feet, seconds: &seconds, int: &int, float: &float, money: &money, ped_type: &ped_type };
        self.stats
            .fetch(&self.enabled, rng)
            .into_iter()
            .filter_map(|i| Some((self.labels.get(i)?.clone(), self.stats.format(i, &format))))
            .collect()
    }
}

/// Atomic write (`.part`, then rename); false when it failed (logged).
fn write(path: &std::path::Path, text: &[u8]) -> bool {
    let part = path.with_extension("json.part");
    let written = path.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok())
        && std::fs::write(&part, text).is_ok()
        && std::fs::rename(&part, path).is_ok();
    if !written {
        warn!("CAREER_STATS could not write {}", path.display());
    }
    written
}

fn load(mut commands: Commands, config: Res<crate::config::Config>) {
    let root = config.asset_root.parent().unwrap_or(&config.asset_root);
    let tables = std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok());
    commands.insert_resource(CareerStatsHost::load(root.join("settings/career-stats.json"), tables));
}

/// One fixed step of the local player: distance on and off the board, time played, skitching, grind length (summed
/// while a grind is active and added when it ends; our grind runtime has no distance of its own).
fn track(
    host: Option<ResMut<CareerStatsHost>>,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    time: Res<Time<Fixed>>,
    transition: Res<crate::map_transition::MapTransition>,
) {
    let (Some(mut host), Some(s)) = (host, skater) else { return };
    let dt = time.delta_secs();
    let root = s.animated_skeleton.roots.animation_to_world;
    let at = [root[3][0], root[3][1], root[3][2]];
    if transition.busy() {
        host.last = None;
        return;
    }
    host.record(row::TIME_PLAYED, dt);
    let step = host.last.map(|l| ((at[0] - l[0]).powi(2) + (at[1] - l[1]).powi(2) + (at[2] - l[2]).powi(2)).sqrt()).filter(|d| d.is_finite() && *d <= MAX_STEP);
    host.last = Some(at);
    let Some(step) = step else { return };
    let on_board = s.player_input.physical.state.category_12 != 500;
    host.record(if on_board { row::DISTANCE } else { row::DISTANCE_OFFBOARD }, step);
    if s.skitch_state.car.is_some() {
        host.record(row::SKITCH_DISTANCE, step);
        host.record(row::SKITCH_TIME, dt);
    }
    match (s.grind.active_name().is_some(), host.grind_run) {
        (true, run) => host.grind_run = Some(run.unwrap_or(0.0) + step),
        (false, Some(run)) => {
            host.grind_run = None;
            host.record(row::GRIND_DISTANCE, run);
        }
        (false, None) => {}
    }
}

/// The local player's ped hits by type (0x826C0730).
fn hits(
    host: Option<ResMut<CareerStatsHost>>,
    mut events: MessageReader<crate::living_world::peds::PedEvent>,
    peds: Query<&crate::living_world::peds::Pedestrian>,
) {
    let Some(mut host) = host else {
        events.clear();
        return;
    };
    for event in events.read() {
        if let crate::living_world::peds::PedEvent::Hit { id, toucher, .. } = event {
            if *toucher != crate::living_world::peds::PLAYER_TARGET_BASE {
                continue;
            }
            let entity = peds.iter().find(|p| p.id == *id).map(|p| p.entity.as_str()).unwrap_or("");
            host.record(row::PED_HITS + hit_type(entity), 1.0);
        }
    }
}

fn save(host: Option<ResMut<CareerStatsHost>>, time: Res<Time<Real>>, mut exit: MessageReader<AppExit>) {
    let Some(mut host) = host else { return };
    host.since_save += time.delta_secs();
    let exiting = exit.read().count() > 0;
    if host.dirty && exiting {
        host.save();
    } else if host.dirty && host.since_save >= SAVE_SECONDS {
        host.since_save = 0.0;
        host.save_in_background();
    }
}

pub(crate) struct CareerStatsPlugin;

impl Plugin for CareerStatsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load)
            .add_systems(FixedUpdate, track.after(crate::app::SimulationSet::Physics))
            .add_systems(Update, hits)
            .add_systems(Last, save);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_keys_pick_the_hit_row_and_others_count_as_bums() {
        assert_eq!(hit_type("jock01"), 1);
        assert_eq!(hit_type("security_nsz02"), 12);
        assert_eq!(hit_type("jock02"), 0);
        assert_eq!(hit_type(""), 0);
    }

    #[test]
    fn totals_survive_a_save_and_load() {
        let dir = std::env::temp_dir().join(format!("career-stats-{}", std::process::id()));
        let path = dir.join("career-stats.json");
        let mut host = CareerStatsHost::load(path.clone(), None);
        host.record(row::DISTANCE, 12.5);
        host.record(row::PED_HITS + 1, 1.0);
        host.save();
        let again = CareerStatsHost::load(path, None);
        assert_eq!((again.stats.value(row::DISTANCE), again.stats.value(row::PED_HITS + 1)), (12.5, 1.0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn panel_rows_need_the_table_and_format_by_kind() {
        let mut host = CareerStatsHost::default();
        host.record(row::DISTANCE, 1234.5);
        let language = BTreeMap::from([("ID_COMMON_METRIC_DISTANCE".to_owned(), "m".to_owned())]);
        let numbers = crate::fe_language::NumberFormat::default();
        assert!(host.panel_rows(&language, &numbers, false, &mut || 0).is_empty(), "no table: no Stats panel");
        host.labels = (0..skate_core::career_stats::ROWS).map(|i| format!("ID_RECORD_{i}")).collect();
        host.enabled = vec![true; skate_core::career_stats::ROWS];
        assert_eq!(host.panel_rows(&language, &numbers, false, &mut || 0), [("ID_RECORD_7".to_owned(), "1,234.0 m".to_owned())]);
    }
}
