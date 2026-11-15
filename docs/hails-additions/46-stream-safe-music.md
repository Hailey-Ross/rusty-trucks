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
- Public playlists: CC0, CC BY, CC BY-NC, CC BY-SA or CC BY-NC-SA tracks (the engine is always free to use, so
  non-commercial licenses fit; each track's exact license is in its entry and in `CREDITS.txt`) whose artist or label
  states the license on their own page, from Free
  Music Archive, Internet Archive (artist and netlabel uploads only: uploader-set licenses there are often wrong; a
  search for CC0 audio returned well-known commercial albums labeled CC0) and OpenGameArt. Each one is checked on its
  own page before it goes in.
- Built in (2026-11-15): 27 OpenGameArt tracks in two playlists, "Lofi and Funk" (19) and "Fast" (8). Each entry's
  page shows CC0 under the author's own upload; descriptions were read for remixes, samples or covers (one entry
  dropped: vocal sweeps of unclear origin and a file name that did not match the title). Every file's sha256 is
  pinned, and the page's own "Copyright/Attribution Notice" is kept in the credit.
- Built in (2026-11-15): 28 Internet Archive netlabel tracks in two playlists, "Chill" (20) and "Electronic" (8).
  Every item is in the label's own collection, uploaded by the label, and the label's or artist's own site states the
  license: Silent Running Scandal (22 tracks by Artificial.Music, Ashiera, Ash Electric, Caturday & Babasmas,
  epi- chord; each artist's Bandcamp links CC BY 4.0), Toucan Music (3; the item says CC BY 2.5, the label's
  licensing page says free for non-commercial use, so credited as CC BY-NC 2.5 with the link to toucanmusic.com the
  label asks for) and BuckramBeats (3 by Mylar Man; the item says CC BY 3.0, the label's site stated CC BY-NC-SA 2.5
  for all its music, so credited as that). Releases were read for covers, parodies, sample packs, mixes, famous
  featured artists, samples, unreadable lyrics and explicit titles; about 18 releases were dropped. Built by
  `.local/stream-safe/build_ia_playlists.ps1` from the vetted list.
- Built in (2026-11-15): "Beats (blocSonic)" (23 instrumentals: C da 76er, Moki Mcfly, aitänna77, Cutside and the
  funk of Pimpstrut Local #13; each
  release page on blocsonic.com states CC BY-NC-SA 4.0 per track; the label's BY-NC-ND releases, its sample packs, rap
  with vocals and releases with explicit titles were left out) and "Disco, Dance and Metal (Loyalty Freak Music)" (19;
  the artist's album pages and FAQ on loyaltyfreakmusic.com state CC0 and allow monetized use; the files come from
  the Internet Archive copies since the artist's site offers zips only).
- Free Music Archive: "Surf, Garage and Funk" (Blue Wave Theory's instrumental surf rock, Crowander, Mr Smith,
  Lemon Knife's instrumentals, Speed Limit 35, Heavenless' funk). Each track page shows the license the artist set
  (CC BY, BY-SA or BY-NC 4.0; picks from `.local/stream-safe/fma_scan.ps1`). FMA lets only signed-in accounts
  download, so the playlist has `"login": "fma"`: when its songs are missing, the launcher asks for the player's own
  FMA email and password in the system's sign-in dialog (Windows' credential prompt, macOS dialogs, zenity or
  kdialog; `tools/skate-launcher/src/fma.rs`), signs in once, downloads with that session and deletes the session
  file; the password goes to FMA over https through curl's stdin and is never saved. Canceled or refused: only that
  playlist is skipped. The playlist links FMA's download endpoints and track pages, never mp3 files. FMA's terms of use
  ask for no deep links to its mp3 files and no automated requests or data mining: one player's launcher downloading
  the playlist's tracks once with their own account is the same as them pressing download. Built by
  `.local/stream-safe/build_fma_playlist.ps1` (same sign-in, sha256 pinned).
- Not used: Jamendo (its pages point Twitch / YouTube use to the paid licensing plan), StreamBeats (its license
  requires separate licensing for games), FreePD (closed in 2025). Incompetech (Kevin MacLeod, CC BY 4.0) is used
  locally only until he answers our question about apps downloading his files and whitelisting.

**Verification.** Launcher unit tests: the built-in playlists parse and every track has a title, artist and https URL;
credits name the artist, source and license; file names are safe on every OS; the FMA sign-in reads the form token,
encodes the form body and tells a returned web page from a song. A local run with the Incompetech
playlists (27 tracks): see the PR tests table.

**Open questions.**
- Menu, loading screen and replay editor music: check whether the disc soundtrack plays anywhere the mod's playlist
  does not replace it.
- More punk and funk: almost none passed on the Internet Archive; Free Music Archive and blocSonic next.
