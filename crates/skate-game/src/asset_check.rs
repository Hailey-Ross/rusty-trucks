//! Launch-time asset staleness check. It reads the converted files themselves on every start, not a stamp:
//! every output setup recorded must still be there at its recorded size, and each group's files must hold what
//! this build reads (format versions and content). A failing group is logged (`ASSET_CHECK <group>: <reason>`) and
//! marked stale (`setup::mark_stale`), so the PC overlay offers Extras > Refresh Game Files, which makes setup
//! rebuild exactly those groups. Runs on a background thread once the startup loading screen is gone, so its
//! thousands of file reads do not compete with the start's own loading on an HDD (the first start of the audio
//! runtime waited ~20 s on them, 2026-10-15).
use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// One failed check: the setup group to rebuild and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub group: String,
    pub reason: String,
}

fn finding(group: &str, reason: impl Into<String>) -> Finding {
    Finding { group: group.into(), reason: reason.into() }
}

/// The installation behind an asset root (a release copy's `data/installations/<id>/assets`, or the dev `assets`
/// junction to one) and its published marker, when there is one.
fn installation(assets: &Path) -> Option<(PathBuf, serde_json::Value)> {
    let assets = assets.canonicalize().ok()?;
    let install_root = assets.parent()?.to_path_buf();
    let base = install_root.parent()?.parent()?;
    let marker: serde_json::Value = serde_json::from_slice(&std::fs::read(base.join("installation.json")).ok()?).ok()?;
    let published = base.join(marker["directory"].as_str()?).canonicalize().ok()?;
    (published == install_root).then_some((install_root, marker))
}

/// Every output each group recorded at setup is still present at its size. The reason names the first few files
/// that differ, so a log shows what changed (a hand re-export that skipped setup, a deleted file, ...).
fn receipts(install_root: &Path, marker: &serde_json::Value) -> Vec<Finding> {
    let Some(groups) = marker["outputs"].as_object() else { return Vec::new() };
    groups
        .iter()
        .filter(|(_, files)| !crate::setup::outputs_present(install_root, files))
        .map(|(group, files)| {
            let differ = differing_outputs(install_root, files);
            let shown = differ.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let more = differ.len().saturating_sub(3);
            let more = if more > 0 { format!(" and {more} more") } else { String::new() };
            finding(group, format!("converted files missing or changed size ({} of {}: {shown}{more})", differ.len(), files.as_object().map_or(0, |f| f.len())))
        })
        .collect()
}

/// The recorded outputs that are missing or not at their recorded size, as `path (missing)` /
/// `path (N bytes, recorded M)`.
fn differing_outputs(install_root: &Path, files: &serde_json::Value) -> Vec<String> {
    let Some(files) = files.as_object() else { return vec!["no outputs recorded".into()] };
    files
        .iter()
        .filter_map(|(name, entry)| {
            let recorded = entry["size"].as_u64();
            match std::fs::metadata(install_root.join(name)) {
                Ok(m) if m.is_file() && Some(m.len()) == recorded => None,
                Ok(m) if m.is_file() => Some(format!("{name} ({} bytes, recorded {})", m.len(), recorded.map_or("none".into(), |r| r.to_string()))),
                _ => Some(format!("{name} (missing)")),
            }
        })
        .collect()
}

/// HUD group: the menu tables and movies this build's retail menus read.
fn hud(assets: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    match std::fs::read_to_string(assets.join(crate::retail_menus::TABLES)) {
        Err(_) => out.push(finding("hud", "menu tables missing")),
        Ok(text) => match skate_data::menu_tables::MenuTables::from_json(&text) {
            Err(e) => out.push(finding("hud", format!("menu tables unreadable by this build ({e})"))),
            Ok(t) => {
                for (empty, what) in [
                    (t.screens.is_empty(), "FE screen records"),
                    (t.settings_titles.is_empty(), "settings titles"),
                    (t.sub_options.is_empty(), "sub-option tables"),
                    (t.templates.is_empty(), "screen templates"),
                ] {
                    if empty {
                        out.push(finding("hud", format!("menu tables lack the {what}")));
                    }
                }
            }
        },
    }
    let movies = assets.join("private/menu-movies");
    match std::fs::read(movies.join("manifest.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
        None => out.push(finding("hud", "menu movie manifest missing")),
        Some(m) => {
            if m["format"] != crate::retail_menu_movies::FORMAT || m["version"] != crate::retail_menu_movies::VERSION {
                out.push(finding("hud", format!("menu movies are version {} (this build reads {})", m["version"], crate::retail_menu_movies::VERSION)));
            } else if let Some(files) = m["files"].as_object() {
                if let Some(name) = files.keys().find(|name| !movies.join(name.as_str()).is_file()) {
                    out.push(finding("hud", format!("menu movie file missing: {name}")));
                }
            }
        }
    }
    // The central message HUD (chase "ZAP!" / "BAM!" lines) came after the scoring HUD: offered, not forced.
    let hud = assets.join("private/hud/runtime");
    if hud.join("trickdisplay.json").is_file() && !hud.join("hudcentral.json").is_file() {
        out.push(finding("hud", "central message HUD missing"));
    }
    out
}

/// Maps group: the ped hand props carry the grip points this build reads (`attach`, setup since b758f0cc). A file
/// whose props have none was exported before them. With the living world tables installed, a missing or unreadable
/// file, or one where no prop has a model (`file`), means every hand prop is drawn as nothing (laptop 2026-10-18:
/// "PED_HAND_PROPS 23 records, 0 with models" after a refresh, no finding); without the tables there is no living
/// world data and nothing to check.
fn hand_props(assets: &Path) -> Vec<Finding> {
    let living = assets.join("private/living_world");
    if !living.join("tables.json").is_file() {
        return Vec::new();
    }
    let missing = || vec![finding("maps", "hand prop models missing")];
    let Some(v) = std::fs::read(living.join("hand_props.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
        return missing();
    };
    match v["props"].as_object() {
        Some(props) if !props.values().any(|p| p.get("file").is_some_and(serde_json::Value::is_string)) => missing(),
        Some(props) if props.values().all(|p| p.get("attach").is_none()) => vec![finding("maps", "hand props lack their grip points (attach)")],
        Some(_) => Vec::new(),
        None => missing(),
    }
}

/// All checks for an asset root; one finding per group and reason.
pub(crate) fn check(assets: &Path) -> Vec<Finding> {
    let mut out = installation(assets).map_or(Vec::new(), |(root, marker)| receipts(&root, &marker));
    out.extend(hud(assets));
    out.extend(hand_props(assets));
    out.dedup();
    out
}

pub(crate) struct AssetCheckPlugin;

impl Plugin for AssetCheckPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, start.run_if(loading_done.and(bevy::ecs::schedule::common_conditions::run_once)))
            .add_systems(Update, force_menu_refresh);
    }
}

/// Set by the check when the retail menu movies or tables are older than this build reads (user, 2026-10-18: "force a
/// refresh if the menus are out of date for the new menu work"): the game restarts into setup's refresh at once
/// instead of only offering Extras > Refresh Game Files.
static FORCE_MENU_REFRESH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A finding that forces the refresh: the menu movies or tables (the `hud` group's menu files).
fn forces_refresh(f: &Finding) -> bool {
    f.group == "hud" && (f.reason.starts_with("menu movie") || f.reason.starts_with("menu tables"))
}

/// Restart into the refresh once. Never from a run the refresh itself started (`--refresh-assets`), so a refresh that
/// could not fix the files falls back to the menu's Refresh Game Files row instead of looping.
fn force_menu_refresh(mut exit: MessageWriter<AppExit>, mut done: Local<bool>) {
    if *done || !FORCE_MENU_REFRESH.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    *done = true;
    if std::env::args().any(|a| a == "--refresh-assets") {
        warn!("ASSET_CHECK menus still out of date after a refresh: not forcing another (Extras > Refresh Game Files)");
        return;
    }
    match crate::setup::restart_with_refresh() {
        Ok(()) => {
            warn!("ASSET_CHECK menus out of date: restarting into Refresh Game Files");
            exit.write(AppExit::Success);
        }
        Err(e) => warn!("ASSET_CHECK menus out of date, the forced refresh could not start: {e}"),
    }
}

fn loading_done(loading: Option<Res<crate::startup_loading::StartupLoading>>) -> bool {
    loading.is_none_or(|l| l.done())
}

fn start(config: Res<crate::config::Config>) {
    let assets = config.asset_root.clone();
    let _ = std::thread::Builder::new().name("asset-check".into()).spawn(move || {
        let started = std::time::Instant::now();
        let findings = check(&assets);
        for f in &findings {
            warn!("ASSET_CHECK {}: {}", f.group, f.reason);
            crate::setup::mark_stale(&f.group);
        }
        if findings.iter().any(forces_refresh) {
            FORCE_MENU_REFRESH.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        info!("ASSET_CHECK done in {} ms: {}", started.elapsed().as_millis(),
            if findings.is_empty() { "game files current".to_owned() } else { format!("{} problem(s), refresh offered", findings.len()) });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_date_menus_force_the_refresh_and_nothing_else_does() {
        assert!(forces_refresh(&finding("hud", "menu movies are version 5 (this build reads 6)")));
        assert!(forces_refresh(&finding("hud", "menu tables lack the FE screens")));
        assert!(forces_refresh(&finding("hud", "menu movie manifest missing")));
        assert!(!forces_refresh(&finding("hud", "hud textures missing")), "other hud files only offer the refresh");
        assert!(!forces_refresh(&finding("audio", "menu movies")), "other groups do not force it");
    }

    fn tables(screens: bool) -> String {
        let mut t = serde_json::json!({
            "schema": skate_data::menu_tables::SCHEMA, "source_sha256": "", "settings_rows": [], "settings_screens": [],
            "categories": [], "items": [], "keys": [], "modes": [], "addresses": {},
            "templates": [{"name": "CrossBar", "path": "x", "kind": 0}],
            "settings_titles": ["ID_GAMESETTINGS_TITLE"],
            "sub_options": [{"labels": [], "helpers": []}],
        });
        if screens {
            t["screens"] = serde_json::json!([{"id": "a", "key": "a", "blur": false, "suspends_gameplay": false, "chyron": false, "hud": 0, "input_to_movie": false}]);
        }
        t.to_string()
    }

    #[test]
    fn a_missing_or_resized_output_flags_its_group() {
        let base = std::env::temp_dir().join(format!("skate3rust-receipts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let id = "0123456789abcdef0123456789abcdef";
        let root = base.join("installations").join(id);
        std::fs::create_dir_all(root.join("assets/private")).unwrap();
        std::fs::write(root.join("assets/private/a.bin"), b"1234").unwrap();
        std::fs::write(root.join("assets/private/b.bin"), b"12").unwrap();
        let marker = serde_json::json!({"version": 1, "directory": format!("installations/{id}"), "outputs": {
            "core": {"assets/private/a.bin": {"size": 4}},
            "maps": {"assets/private/b.bin": {"size": 3}},
            "audio": {"assets/private/c.bin": {"size": 1}},
        }});
        std::fs::write(base.join("installation.json"), marker.to_string()).unwrap();
        let (found_root, found) = installation(&root.join("assets")).expect("the published installation");
        let findings = receipts(&found_root, &found);
        let groups: Vec<&str> = findings.iter().map(|f| f.group.as_str()).collect();
        assert_eq!(groups, ["audio", "maps"], "missing c.bin, b.bin's size changed; core is fine");
        // The log names the files.
        assert!(findings[0].reason.ends_with("(1 of 1: assets/private/c.bin (missing))"), "{}", findings[0].reason);
        assert!(findings[1].reason.ends_with("(1 of 1: assets/private/b.bin (2 bytes, recorded 3))"), "{}", findings[1].reason);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hand_props_without_grip_points_are_stale() {
        let dir = std::env::temp_dir().join(format!("skate-asset-check-hand-props-{}", std::process::id()));
        let file = dir.join("private/living_world/hand_props.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        assert!(hand_props(&dir).is_empty(), "no living world data: nothing to check");
        std::fs::write(dir.join("private/living_world/tables.json"), "{}").unwrap();
        let missing = vec![finding("maps", "hand prop models missing")];
        assert_eq!(hand_props(&dir), missing, "tables but no hand prop file");
        std::fs::write(&file, r#"{"props": {"orange": {"model": "orange_lw", "error": "no template"}}}"#).unwrap();
        assert_eq!(hand_props(&dir), missing, "no prop has a model");
        std::fs::write(&file, r#"{"props": {"pop": {"file": "hand_props/pop.glb"}}}"#).unwrap();
        assert_eq!(hand_props(&dir), vec![finding("maps", "hand props lack their grip points (attach)")]);
        std::fs::write(&file, r#"{"props": {"pop": {"file": "x", "attach": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}, "orange": {"model": "orange_lw", "error": "no template"}}}"#).unwrap();
        assert!(hand_props(&dir).is_empty(), "current export (a prop without a template may lack it)");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hud_check_reads_the_files_not_a_stamp() {
        let dir = std::env::temp_dir().join(format!("skate3rust-asset-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let private = dir.join("private");
        std::fs::create_dir_all(private.join("menu-movies")).unwrap();
        // Nothing there: both files reported.
        let reasons: Vec<String> = hud(&dir).into_iter().map(|f| f.reason).collect();
        assert_eq!(reasons, ["menu tables missing", "menu movie manifest missing"]);
        // Tables from before the FE screen records, movies one version old.
        std::fs::write(private.join("menu-tables.json"), tables(false)).unwrap();
        std::fs::write(private.join("menu-movies/manifest.json"),
            serde_json::json!({"format": crate::retail_menu_movies::FORMAT, "version": crate::retail_menu_movies::VERSION - 1, "files": {}}).to_string()).unwrap();
        let reasons: Vec<String> = hud(&dir).into_iter().map(|f| f.reason).collect();
        assert!(reasons.iter().any(|r| r.contains("FE screen records")), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.contains("menu movies are version")), "{reasons:?}");
        // Current files: clean, and a listed movie that is gone is caught.
        std::fs::write(private.join("menu-tables.json"), tables(true)).unwrap();
        std::fs::write(private.join("menu-movies/manifest.json"),
            serde_json::json!({"format": crate::retail_menu_movies::FORMAT, "version": crate::retail_menu_movies::VERSION, "files": {"a.json": {}}}).to_string()).unwrap();
        assert_eq!(hud(&dir).into_iter().map(|f| f.reason).collect::<Vec<_>>(), ["menu movie file missing: a.json"]);
        std::fs::write(private.join("menu-movies/a.json"), "{}").unwrap();
        assert!(hud(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
