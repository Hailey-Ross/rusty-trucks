//! Stream-safe mode (Tools): music that streams and videos may use, in place of the soundtrack. Turning it on downloads
//! the playlists' tracks (tools/stream-safe/playlists.json, built in, plus an optional local file next to the launcher
//! state), packs one music bank per playlist with the selected version's game, writes CREDITS.txt and turns on the
//! stream-safe-music mod; every launch then writes that version's playlist and the mod's on / off state.
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::Launcher;

const PUBLIC: &str = include_str!("../../stream-safe/playlists.json");
pub const MOD_ID: &str = "stream-safe-music";
const CUSTOM_MUSIC: &str = "custom-music";

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Manifest {
    #[serde(default)]
    pub playlists: Vec<Playlist>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub source_url: String,
    pub license: String,
    #[serde(default)]
    pub license_url: String,
    /// "fma": the source only lets signed-in accounts download; the player is asked for their own login.
    #[serde(default)]
    pub login: String,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub url: String,
    #[serde(default)]
    pub page: String,
    /// The full credit; built from title, artist, source and license when empty.
    #[serde(default)]
    pub credit: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub sha256: String,
}

impl Track {
    pub fn credit(&self, p: &Playlist) -> String {
        if !self.credit.trim().is_empty() {
            return self.credit.trim().to_owned();
        }
        let license = if self.license.is_empty() { &p.license } else { &self.license };
        let link = if self.page.is_empty() { &p.source_url } else { &self.page };
        let mut lines = vec![format!("\"{}\" by {}", self.title, self.artist), format!("Source: {}{}", p.source, if link.is_empty() { String::new() } else { format!(" ({link})") })];
        lines.push(format!("License: {license}{}", if p.license_url.is_empty() { String::new() } else { format!(" ({})", p.license_url) }));
        lines.join("\n")
    }

    /// `Artist - Title.ext`: the packer's name fallback, safe on every OS.
    pub fn file_name(&self) -> String {
        let ext = self.url.rsplit('.').next().filter(|e| e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("mp3");
        let clean = |s: &str| s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect::<String>();
        format!("{} - {}.{}", clean(&self.artist), clean(&self.title), ext.to_ascii_lowercase())
    }
}

pub fn dir(l: &Launcher) -> PathBuf {
    l.state_dir.join("stream-safe")
}

fn local_manifest(l: &Launcher) -> PathBuf {
    dir(l).join("local-playlists.json")
}

/// The built-in playlists, then the local file's (a local playlist with a built-in id replaces it).
pub fn manifest(l: &Launcher) -> Result<Manifest, String> {
    let mut m: Manifest = serde_json::from_str(PUBLIC.trim_start_matches('\u{feff}')).map_err(|e| format!("built-in playlists: {e}"))?;
    if let Ok(text) = std::fs::read_to_string(local_manifest(l)) {
        let local: Manifest = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", local_manifest(l).display()))?;
        for p in local.playlists {
            m.playlists.retain(|q| q.id != p.id);
            m.playlists.push(p);
        }
    }
    m.playlists.retain(|p| !p.tracks.is_empty() && safe_id(&p.id));
    Ok(m)
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn enabled(l: &Launcher) -> bool {
    dir(l).join("enabled").exists()
}

fn banks(l: &Launcher) -> Vec<PathBuf> {
    let mut b: Vec<PathBuf> = std::fs::read_dir(dir(l).join("banks")).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "bank")).collect();
    b.sort();
    b
}

/// `stream_safe` status: "on N" (tracks), "off", or "off none" when no playlist has tracks yet.
pub fn status(l: &Launcher) -> String {
    let tracks: usize = manifest(l).map(|m| m.playlists.iter().map(|p| p.tracks.len()).sum()).unwrap_or(0);
    match (enabled(l), tracks) {
        (true, n) => format!("on {n}"),
        (false, 0) => "off none".into(),
        (false, _) => "off".into(),
    }
}

/// The selected version's game, for packing.
fn game_exe(l: &Launcher, v: &Value) -> PathBuf {
    let d = l.version_dir(v);
    if l.use_release(v) { crate::ensure::release_exe(&d) } else { d.join("bin").join(crate::cli::GAME_EXE) }
}

fn sha256_file(p: &Path) -> Option<String> {
    use sha2::Digest;
    let bytes = std::fs::read(p).ok()?;
    Some(sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// `stream-safe-on`: download, pack, credits, mod on. Prints progress lines for the launcher window.
pub fn on(l: &Launcher) -> Result<Vec<String>, String> {
    let m = manifest(l)?;
    if m.playlists.is_empty() {
        return Err("No stream-safe playlists have tracks yet. Nothing was changed.".into());
    }
    let exe = game_exe(l, l.selected());
    if !exe.is_file() {
        return Err(format!("The selected version has no game yet ({}). Play it once, then turn this on.", exe.display()));
    }
    let root = dir(l);
    std::fs::create_dir_all(root.join("banks")).map_err(|e| e.to_string())?;
    let mut credits = vec!["Music in this stream or video:".to_owned(), String::new()];
    let mut packed = 0;
    let mut fma: Option<Result<crate::fma::Session, String>> = None;
    for p in &m.playlists {
        println!("Playlist: {} ({}, {})", p.name, p.source, p.license);
        let folder = root.join("music").join(&p.id);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let have = |t: &Track| {
            let file = folder.join(t.file_name());
            file.is_file() && (t.sha256.is_empty() || sha256_file(&file).is_some_and(|h| h.eq_ignore_ascii_case(&t.sha256)))
        };
        let session = if p.login == "fma" && !p.tracks.iter().all(have) {
            match fma.get_or_insert_with(|| crate::fma::sign_in(&root, &p.name)) {
                Ok(s) => Some(&*s),
                Err(e) => {
                    println!("  Skipped {}: {e}. Turn stream-safe mode on again to sign in.", p.name);
                    continue;
                }
            }
        } else {
            None
        };
        let mut names = serde_json::Map::new();
        for (i, t) in p.tracks.iter().enumerate() {
            let file = folder.join(t.file_name());
            if !have(t) {
                println!("  Downloading {}/{}: {} by {}", i + 1, p.tracks.len(), t.title, t.artist);
                let part = file.with_extension("part");
                let ok = match session {
                    Some(s) => crate::fma::download(s, &t.url, &part),
                    None => crate::proc::cmd("curl").args(["-fsSL", "--retry", "2", "--max-time", "600", "-o"]).arg(&part).arg(&t.url).status().is_ok_and(|s| s.success()),
                };
                if !ok {
                    let _ = std::fs::remove_file(&part);
                    println!("  Skipped {}: the download did not work.", t.title);
                    continue;
                }
                if !t.sha256.is_empty() && !sha256_file(&part).is_some_and(|h| h.eq_ignore_ascii_case(&t.sha256)) {
                    let _ = std::fs::remove_file(&part);
                    println!("  Skipped {}: the file is not the one the playlist lists (checksum).", t.title);
                    continue;
                }
                std::fs::rename(&part, &file).map_err(|e| e.to_string())?;
            }
            names.insert(t.file_name(), json!({"title": t.title, "artist": t.artist}));
            credits.push(t.credit(p));
            credits.push(String::new());
        }
        std::fs::write(folder.join("names.json"), serde_json::to_string_pretty(&Value::Object(names)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("  Packing {} into a music bank...", p.name);
        let bank = root.join("banks").join(format!("{}.bank", p.id));
        let ok = crate::proc::cmd(&exe).arg("--pack-music-folder").arg(&folder).arg(&bank).status().is_ok_and(|s| s.success());
        if ok { packed += 1 } else { println!("  Packing {} did not work.", p.name) }
    }
    if packed == 0 {
        return Err("No playlist could be packed. Stream-safe mode stays off.".into());
    }
    std::fs::write(root.join("CREDITS.txt"), credits.join("\r\n")).map_err(|e| e.to_string())?;
    std::fs::write(root.join("enabled"), "1").map_err(|e| e.to_string())?;
    let custom_was_on = mod_enabled(l, CUSTOM_MUSIC);
    if custom_was_on == Some(true) {
        let _ = std::fs::write(root.join("custom-music-was-on"), "1");
    }
    prepare(l, l.selected());
    Ok(vec![
        format!("Stream-safe mode is ON: {packed} playlist(s). The soundtrack is replaced; each song shows its title and artist."),
        format!("Credits for your stream or video description: {}", root.join("CREDITS.txt").display()),
    ])
}

/// `stream-safe-off`: mod off, Custom Music back on if it was; downloads kept.
pub fn off(l: &Launcher) -> Vec<String> {
    let root = dir(l);
    let _ = std::fs::remove_file(root.join("enabled"));
    set_mod_enabled(l, MOD_ID, false);
    if root.join("custom-music-was-on").exists() {
        set_mod_enabled(l, CUSTOM_MUSIC, true);
        let _ = std::fs::remove_file(root.join("custom-music-was-on"));
    }
    vec!["Stream-safe mode is OFF: the game's soundtrack plays again. The downloaded songs stay (Tools > Remove stream-safe songs).".into()]
}

/// `stream-safe-remove`: off, then the downloads, banks and credits go.
pub fn remove(l: &Launcher) -> Vec<String> {
    let mut lines = off(l);
    let root = dir(l);
    for sub in ["music", "banks"] {
        let _ = std::fs::remove_dir_all(root.join(sub));
    }
    let _ = std::fs::remove_file(root.join("CREDITS.txt"));
    lines.push("The downloaded stream-safe songs were removed.".into());
    lines
}

fn mod_prefs(l: &Launcher, id: &str) -> PathBuf {
    let assets = crate::status::current_assets(l);
    assets.parent().map_or_else(|| PathBuf::from("settings"), |p| p.join("settings")).join("mods").join(format!("{id}.json"))
}

fn mod_enabled(l: &Launcher, id: &str) -> Option<bool> {
    let text = std::fs::read_to_string(mod_prefs(l, id)).ok()?;
    serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok()?.get("enabled")?.as_bool()
}

/// The game's per-mod preference file (`{enabled, values}`), its values kept.
fn set_mod_enabled(l: &Launcher, id: &str, on: bool) {
    let path = mod_prefs(l, id);
    let mut o = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .filter(Value::is_object).unwrap_or_else(|| json!({"values": {}}));
    o["enabled"] = json!(on);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&o).unwrap_or_default());
}

/// Before a game starts: with the mode on, the version's mod gets the playlist of the packed banks and is turned on
/// (Custom Music off meanwhile); with it off, the mod is off.
pub fn prepare(l: &Launcher, v: &Value) {
    if !enabled(l) {
        if mod_enabled(l, MOD_ID) == Some(true) {
            set_mod_enabled(l, MOD_ID, false);
        }
        return;
    }
    let exe = game_exe(l, v);
    // A release has mods next to its game, a checkout in its root (Launch.ps1 / PLAY.sh set SKATE3_MODS there).
    let folder = [exe.parent().map(|d| d.join("mods")), Some(l.version_dir(v).join("mods"))].into_iter().flatten()
        .map(|m| m.join(MOD_ID)).find(|f| f.is_dir());
    let banks = banks(l);
    let Some(folder) = folder else {
        println!("  Stream-safe mode: this version has no {MOD_ID} mod; the soundtrack plays.");
        return;
    };
    if banks.is_empty() {
        println!("  Stream-safe mode: no music banks yet (turn it off and on again in Tools).");
        return;
    }
    let list = folder.join("playlists").join("stream-safe.json");
    let _ = std::fs::create_dir_all(folder.join("playlists"));
    let ok = crate::proc::cmd(&exe).arg("--music-playlist").arg(&list).arg("Stream-safe").arg("--shuffle").args(&banks)
        .stdout(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    if !ok {
        println!("  Stream-safe mode: the playlist could not be written; the soundtrack plays.");
        return;
    }
    set_mod_enabled(l, MOD_ID, true);
    if mod_enabled(l, CUSTOM_MUSIC) == Some(true) {
        let _ = std::fs::write(dir(l).join("custom-music-was-on"), "1");
        set_mod_enabled(l, CUSTOM_MUSIC, false);
    }
    println!("  Stream-safe mode: on ({} music bank(s)).", banks.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_playlists_parse_and_every_track_is_credited() {
        assert!(!PUBLIC.starts_with('\u{feff}'), "write playlists.json without a byte order mark");
        let m: Manifest = serde_json::from_str(PUBLIC).unwrap();
        for p in &m.playlists {
            assert!(safe_id(&p.id), "{}", p.id);
            assert!(!p.license.is_empty() && !p.source.is_empty(), "{}: source and license", p.id);
            for t in &p.tracks {
                assert!(!t.title.is_empty() && !t.artist.is_empty() && t.url.starts_with("https://"), "{}: {}", p.id, t.title);
            }
        }
    }

    #[test]
    fn credits_name_the_artist_source_and_license() {
        let p = Playlist { id: "a".into(), name: "A".into(), source: "Free Music Archive".into(), license: "CC0 1.0".into(),
            license_url: "https://creativecommons.org/publicdomain/zero/1.0/".into(), ..Default::default() };
        let t = Track { title: "Song".into(), artist: "Band".into(), url: "https://x/y/Song.MP3".into(), page: "https://x/song".into(), ..Default::default() };
        let c = t.credit(&p);
        assert!(c.contains("\"Song\" by Band") && c.contains("Free Music Archive (https://x/song)") && c.contains("CC0 1.0"), "{c}");
        assert_eq!(t.file_name(), "Band - Song.mp3");
        let odd = Track { title: "A/B: C?".into(), artist: "D".into(), url: "https://x/z".into(), ..Default::default() };
        assert_eq!(odd.file_name(), "D - A_B_ C_.mp3");
        let own = Track { credit: "Custom line".into(), ..t };
        assert_eq!(own.credit(&p), "Custom line");
    }
}
