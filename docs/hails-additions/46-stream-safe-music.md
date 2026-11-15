# 46. Stream-safe music

**Problem.** Skate 3's soundtrack is licensed music: streams, VODs and videos of the game get muted, claimed or taken
down. Players who stream or record need music they may use, credited, without hunting for it.

**Change.**
- Launcher (`tools/skate-launcher/src/streamsafe.rs`): **Tools > Stream-safe mode**. On: downloads every playlist's
  tracks with `curl` into `<launcher state>/stream-safe/music/<playlist>/` (sha256 checked when the playlist lists
  one), writes `names.json` with each track's exact title and artist, packs one music bank per playlist with the
  selected version's game (`--pack-music-folder`), writes `CREDITS.txt` (every track, full credit) and turns the
  `stream-safe-music` mod on; Custom Music is paused meanwhile and comes back when the mode goes off. Before every game
  start (`play::launch`) the launcher writes that version's `mods/stream-safe-music/playlists/stream-safe.json` from the
  banks and the mod's on / off preference (`settings/mods/<id>.json`). Off keeps the downloads; **Remove stream-safe
  songs** deletes them. Commands: `stream-safe`, `stream-safe-on`, `stream-safe-off`, `stream-safe-remove`; status key
  `stream_safe`.
- Playlists: `tools/stream-safe/playlists.json` is built into the launcher; `local-playlists.json` in the launcher's
  `stream-safe` folder adds or replaces playlists for local use. Each playlist names its source and license; each track
  its title, artist, download URL, page, optional full `credit` text and `sha256`.
- Game: `--pack-music-folder` reads an optional `names.json` (`{"file": {"title", "artist"}}`) in the music folder, so
  the on-screen caption shows the credited names, not whatever the file's tags say.
- Mod `mods/stream-safe-music`: plays the playlist in replace mode (the soundtrack never plays), shuffle and volume
  settings, caption always on.

**Sources and rules (2026-11-15).**
- Every track is credited in full (title, artist, source, license), even when its license does not require credit.
- Public playlists: only CC0 or CC BY tracks whose artist or label states the license on their own page, from Free
  Music Archive, Internet Archive (artist and netlabel uploads only: uploader-set licenses there are often wrong; a
  search for CC0 audio returned well-known commercial albums labeled CC0) and OpenGameArt. Each one is checked on its
  own page before it goes in.
- Built in (2026-11-15): 27 OpenGameArt tracks in two playlists, "Lofi and Funk" (19) and "Fast" (8). Each entry's
  page shows CC0 under the author's own upload; descriptions were read for remixes, samples or covers (one entry
  dropped: vocal sweeps of unclear origin and a file name that did not match the title). Every file's sha256 is
  pinned, and the page's own "Copyright/Attribution Notice" is kept in the credit. Free Music Archive's pages are
  built by script and Internet Archive needs per-uploader checks: both are next.
- Not used: Jamendo (its pages point Twitch / YouTube use to the paid licensing plan), StreamBeats (its license
  requires separate licensing for games), FreePD (closed in 2025). Incompetech (Kevin MacLeod, CC BY 4.0) is used
  locally only until he answers our question about apps downloading his files and whitelisting.

**Verification.** Launcher unit tests: the built-in playlists parse and every track has a title, artist and https URL;
credits name the artist, source and license; file names are safe on every OS. A local run with the Incompetech
playlists (27 tracks): see the PR tests table.

**Open questions.**
- Menu, loading screen and replay editor music: check whether the disc soundtrack plays anywhere the mod's playlist
  does not replace it.
- The public playlists' tracks.
