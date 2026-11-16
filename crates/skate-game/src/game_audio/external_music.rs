//! Spotify passthrough (ask 679): while the Spotify app plays on this PC, the game's music is silenced and Spotify's
//! title and artist show in the music caption. No account or web API: a worker thread looks at the local app once a
//! second (Windows: the Spotify window's title, "Artist - Title" while playing; Linux: its MPRIS player over
//! `dbus-send`; macOS: `osascript`, only when Spotify already runs). Off with `SKATE_SPOTIFY=0` or
//! [`ExternalMusic::enabled`] (the mod entry point); the music comes back when Spotify pauses or closes.
//! docs/hails-additions/11-audio.md#spotify-passthrough
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;

use super::music::{Music, MusicRequest, MusicTrackStarted};

/// The music owner name the silence is requested under.
pub(crate) const OWNER: &str = "spotify";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NowPlaying {
    pub title: String,
    pub artist: String,
}

#[derive(Resource)]
pub(crate) struct ExternalMusic {
    /// Silence the game's music and caption Spotify's songs while it plays (default on).
    pub enabled: bool,
    /// A mod's on / off while it runs (`sdk.music.spotify`), cleared when it stops.
    pub mod_override: Option<(String, bool)>,
    /// What the watcher saw last (None: Spotify paused, closed or not installed).
    latest: Arc<Mutex<Option<NowPlaying>>>,
    /// What the game acted on.
    shown: Option<NowPlaying>,
    silenced: bool,
}

impl Default for ExternalMusic {
    fn default() -> Self {
        let enabled = std::env::var("SKATE_SPOTIFY").map_or(true, |v| v != "0");
        let latest = Arc::new(Mutex::new(None));
        if enabled {
            let shared = latest.clone();
            let _ = std::thread::Builder::new().name("spotify-watch".into()).spawn(move || loop {
                let now = now_playing();
                if let Ok(mut g) = shared.lock() {
                    *g = now;
                }
                std::thread::sleep(Duration::from_secs(1));
            });
        }
        Self { enabled, mod_override: None, latest, shown: None, silenced: false }
    }
}

impl ExternalMusic {
    pub(crate) fn silenced(&self) -> bool {
        self.silenced
    }

    /// On unless the engine setting or a running mod turned it off.
    pub(crate) fn active(&self) -> bool {
        self.mod_override.as_ref().map_or(self.enabled, |o| o.1)
    }

    /// Spotify's song while it takes over (for the mods' snapshot).
    pub(crate) fn now(&self) -> Option<&NowPlaying> {
        self.shown.as_ref()
    }

    /// The caption for `t` is Spotify's current song.
    pub(crate) fn is_shown(&self, t: &MusicTrackStarted) -> bool {
        self.shown.as_ref().is_some_and(|s| s.title == t.title && s.artist == t.artist)
    }
}

pub(super) fn apply(mut ext: ResMut<ExternalMusic>, mut music: ResMut<Music>, mut started: MessageWriter<MusicTrackStarted>) {
    let now = if ext.active() { ext.latest.lock().ok().and_then(|g| g.clone()) } else { None };
    match (&now, ext.silenced) {
        (Some(_), false) => {
            music.requests.push(MusicRequest::Volume { owner: Some(OWNER.into()), level: Some(0.0) });
            ext.silenced = true;
        }
        (None, true) => {
            music.requests.push(MusicRequest::ClearOwner(OWNER.into()));
            ext.silenced = false;
        }
        _ => {}
    }
    if now != ext.shown {
        if let Some(n) = &now {
            info!("SPOTIFY now playing: {} / {}", n.title, n.artist);
            started.write(MusicTrackStarted { title: n.title.clone(), artist: n.artist.clone(), custom: true });
        }
        ext.shown = now;
    }
}

/// "Artist - Title" (the Spotify window while playing); the app's own name or an empty title is not a song.
fn split_window_title(t: &str) -> Option<NowPlaying> {
    let t = t.trim();
    if t.is_empty() || t.starts_with("Spotify") {
        return None;
    }
    let (artist, title) = t.split_once(" - ")?;
    Some(NowPlaying { title: title.trim().into(), artist: artist.trim().into() })
}

#[cfg(windows)]
fn now_playing() -> Option<NowPlaying> {
    use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: plain Win32 queries on a window handle EnumWindows gave us; `found` points at the caller's Option,
        // which outlives the enumeration.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return BOOL(1) };
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let named = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(process);
            if !named || !String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase().ends_with("\\spotify.exe") {
                return BOOL(1);
            }
            let mut title = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut title);
            if let Some(song) = split_window_title(&String::from_utf16_lossy(&title[..n.max(0) as usize])) {
                *(found.0 as *mut Option<NowPlaying>) = Some(song);
                return BOOL(0);
            }
            BOOL(1)
        }
    }
    let mut found: Option<NowPlaying> = None;
    // SAFETY: enumerates top-level windows with a callback that only reads them and writes the local Option.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut Option<NowPlaying> as isize));
    }
    found
}

#[cfg(target_os = "linux")]
fn now_playing() -> Option<NowPlaying> {
    let get = |prop: &str| {
        let out = std::process::Command::new("dbus-send")
            .args(["--print-reply", "--session", "--dest=org.mpris.MediaPlayer2.spotify", "/org/mpris/MediaPlayer2",
                "org.freedesktop.DBus.Properties.Get", "string:org.mpris.MediaPlayer2.Player", &format!("string:{prop}")])
            .stderr(std::process::Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    if !get("PlaybackStatus")?.contains("\"Playing\"") {
        return None;
    }
    let meta = get("Metadata")?;
    let after = |key: &str| {
        let i = meta.find(key)?;
        let rest = &meta[i + key.len()..];
        let q = rest.find("string \"")? + 8;
        Some(rest[q..q + rest[q..].find('"')?].to_owned())
    };
    Some(NowPlaying { title: after("\"xesam:title\"")?, artist: after("\"xesam:artist\"").unwrap_or_default() })
}

#[cfg(target_os = "macos")]
fn now_playing() -> Option<NowPlaying> {
    // `tell application "Spotify"` alone would start Spotify: only ask when it already runs.
    let script = r#"if application "Spotify" is running then
tell application "Spotify"
if player state is playing then return (name of current track) & linefeed & (artist of current track)
end tell
end if
return """#;
    let out = std::process::Command::new("osascript").args(["-e", script]).stderr(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut lines = text.lines();
    let title = lines.next()?.trim().to_owned();
    (!title.is_empty()).then(|| NowPlaying { title, artist: lines.next().unwrap_or("").trim().to_owned() })
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn now_playing() -> Option<NowPlaying> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spotify_window_title_gives_artist_and_title() {
        assert_eq!(split_window_title("Daft Punk - One More Time"), Some(NowPlaying { title: "One More Time".into(), artist: "Daft Punk".into() }));
        assert_eq!(split_window_title("Artist - Song - Remastered 2011").map(|n| n.title), Some("Song - Remastered 2011".into()));
        for idle in ["Spotify", "Spotify Premium", "Spotify Free", "", "  "] {
            assert_eq!(split_window_title(idle), None, "{idle:?}");
        }
    }

    #[test]
    fn spotify_silences_the_music_and_captions_each_song_then_gives_it_back() {
        let mut app = App::new();
        app.add_message::<MusicTrackStarted>().init_resource::<Music>()
            .insert_resource(ExternalMusic { enabled: true, mod_override: None, latest: Arc::new(Mutex::new(None)), shown: None, silenced: false })
            .add_systems(Update, apply);
        let set = |app: &mut App, n: Option<(&str, &str)>| {
            let ext = app.world().resource::<ExternalMusic>();
            *ext.latest.lock().unwrap() = n.map(|(a, t)| NowPlaying { title: t.into(), artist: a.into() });
        };
        set(&mut app, Some(("Band", "Song")));
        app.update();
        assert!(app.world().resource::<ExternalMusic>().silenced);
        assert!(matches!(app.world().resource::<Music>().requests.last(), Some(MusicRequest::Volume { level: Some(0.0), .. })));
        let captions = app.world().resource::<Messages<MusicTrackStarted>>();
        assert_eq!(captions.iter_current_update_messages().count(), 1);
        app.update();
        assert_eq!(app.world().resource::<Messages<MusicTrackStarted>>().iter_current_update_messages().count(), 0, "same song: no new caption");
        set(&mut app, None);
        app.update();
        assert!(!app.world().resource::<ExternalMusic>().silenced);
        assert!(matches!(app.world().resource::<Music>().requests.last(), Some(MusicRequest::ClearOwner(o)) if o == OWNER));
        // A mod turns it off: Spotify playing no longer takes over until the mod's override ends.
        app.world_mut().resource_mut::<ExternalMusic>().mod_override = Some(("quiet".into(), false));
        set(&mut app, Some(("Band", "Song 2")));
        app.update();
        assert!(!app.world().resource::<ExternalMusic>().silenced);
        app.world_mut().resource_mut::<ExternalMusic>().mod_override = None;
        app.update();
        assert!(app.world().resource::<ExternalMusic>().silenced);
    }
}
