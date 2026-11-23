# 47. Movie player

**Problem.** Skate 3 plays its boot and career movies (EA intro, the new career intro, Coach Frank, the park flyby,
the camera previews) through its FE 24 movie player. The engine had no movie support: the retail startup and the
career welcome flow (PR 68 "Planned") need it.

**Retail (research b898, b921, b924, b926, b931, b955).**
- The disc's movies are EA MV containers (`data/movies/<name>_english_ntsc.vp6`): `MVhd` (`VP60`, width, height, frame
  count, largest frame, rate / scale, e.g. 299699 / 10000 = 29.97 fps), per language `SH` / `SC`, then per frame one
  `MV0K` / `MV0F` (VP6) and one `SD<lang>` audio block per language (EA-XA v2, 6 channels, 48 kHz) [data b924].
- Frame pick: target frame = (offset + clock ms) x fps x 0.001; late frames are dropped, the newest due frame shows;
  no new frame for over 0.25 s counts as stalled [code 82A92DC8, b955 §3].
- Skip: A or Start shows the prompt, A again skips, B hides it; skippable rows after 5000 ms; ea_intro and a few rows
  never [code 825E0680 / 8260B240, b955 §1].

**Change (step 1, 2026-11-15).**
- `vendor/nihav-vp6`: the VP6 decoder (ruffle-rs/nihav-vp6 12d0bdf1, MIT, pure Rust, every OS) with the two fixes that
  make all 11 disc movies byte-identical to FFmpeg (`vendor/README.md` "nihav-vp6").
- `crates/skate-movie`: the decode side, no rendering or audio. `container::Reader` streams the EA chunks (one chunk in
  memory), `FrameDecoder` is the replaceable decoder trait (a mod or platform can bring its own), `Vp6Decoder` returns
  top-down YUV 4:2:0 planes.

**Change (step 2, 2026-11-16).** `crates/skate-game/src/movie_player.rs` + `movie.wgsl`: a worker thread decodes up to 6
frames ahead; the frame clock is retail's (target frame = elapsed ms x fps x 0.001, late frames dropped, the newest due
frame shown, no slow-down); the three YUV planes are written straight into persistent R8 textures in the render world
(as `video_screens.rs` copies its feed); a UI material converts BT.601 limited range to RGB, letterboxed on black above
everything. Dev entry `SKATE_MOVIE=<file.vp6>` (Escape ends it). Checked 2026-11-16: intro_movie plays in a muted
desktop run (`--verify` capture at 15 s: the right picture, upright, natural color).

**Change (step 3, 2026-11-16).** Sound: the decode worker also decodes the language's `SD` blocks (EA-XA v2, 6
channels, `skate-movie::audio`), folds them to stereo with retail's output table (Lo = 0.707 L + 0.5 C + 0.5 Ls, Ro
mirrored, LFE dropped, 1/32767 scale; skate-audio `output_stereo`, b955 2b) and feeds its own Bevy audio source (not
the game's native stream, which pauses under suspending FE screens, b926 §3). While the movie has sound, the frames the
device played are the movie clock (retail uses its audio clock when it has one, 82A92DC8); without sound (the camera
previews) the wall clock. Language: EN (`SKATE_MOVIE_LANG` for tests). Checked 2026-11-16: EA_Blackbox, muted desktop
run: 428034 audio frames = 8.92 s for its 268 frames at 29.97 fps (8.94 s); 248 shown, 20 dropped while the map
loaded underneath (late frames skip, retail's rule). NOT RETAIL YET: the movie gain (row volume x MixMap Master out4,
b955 2a), the game language word for the track.

**Change (step 4, 2026-11-16).** Movies by row: `skate_data::movie_rows` reads the 10 playable `fe_movie` rows
(vault class 0x18B275357481FB52, inherited from `default`, DLC rows through `default_dlc`): file, folder, volume,
skippable `82D9`, skip delay `0AC1` (ms), back_exits `9A07`, stays_after_end `6FC6`, subtitles `A3A0` / `2914`, listed
`07B8`, title `AA75`. Every caller plays a row key (`movie_player::play_row`), never a file; the row's volume is the
movie's (NOT RETAIL YET: times the MixMap Master out4 gain). Setup: the core step copies the disc's `data/movies` into
`private/stock/data/movies` (hard links on the same drive, else copies, about 1 GB; needs Refresh Game Files). Retail
skip rules (`skip_step`, FE 24 input 8260B240): nothing before the row's delay or on rows with 82D9 = 0 (ea_intro,
create_*, filmer_end_outro); A or Start shows the prompt, A again skips, B hides it and leaves only on back_exits rows
(credits). The prompt is retail's fmv_popup chyron on `_level3`: its frame 0 runs `new fmv_popup(this)`, whose
constructor keeps the level in the static `fmv_popup.mScreen`, sets `mcChyron.artist.text` to ID_MOVIESKIP and
defines `_global.ShowMovieSkip` / `HideMovieSkip` [data, fmv_popup export bytecode]. Show plays the level's own
`start` label (frames 2..18 fade mcChyron in from alpha 0) and moves the level to `_y` -168 when
`Audio.HasSubtitles()`; Hide plays `end`. The movie player raises the APT composite above the video while it plays
and calls those two functions; a plain label stands in only when no menu stage runs. Its `<B:A>` / `<B:B>` tags draw
as the pad's glyphs (doc 31 "Inline button tags"); the panel is a fixed 364-unit shape (fmv_popup char 3, no script
resizes it). Boot (826FDEF0): `ea_intro` plays over the loading screen at startup, behind the Career
and Challenges opt-in with the rest of the retail startup (off in automated runs, `SKATE_BOOT_MOVIE=0`).
`SKATE_MOVIE=<row key | file.vp6>`. Checked 2026-11-16: `SKATE_MOVIE=ea_intro` resolves through the row to the
install's file and plays to its end with sound (428035 audio frames, 8.92 s); the rows test reads the disc's rows.

### The skip prompt stays after Hide (2026-11-18)

**Problem.** After B (or the prompt timing out) the skip prompt stayed on screen.

**Root cause / retail.** fmv_popup's timeline: frame 2 `start` (`this._visible = true`, fade in), frame 19 `end`
(alpha 255, `stop()`), frame 20 (`this._visible = false; stop()`) [data, fmv_popup export bytecode]. Show stops on
frame 19, so Hide's `gotoAndPlay("end")` targets the frame the clip already shows. In Flash a goto to the current
frame only sets the play state and the frame's actions do not run again; the clip then plays on to frame 20 and
hides. Our APT VM re-ran frame 19's `stop()`, so the clip stayed on the visible frame.

**Change.** `apt_movie.rs` `method`: `gotoAndPlay` / `gotoAndStop` to the current frame skip the seek and only set
`playing`. Test `apt_goto_current_frame_runs_no_actions`.

**Verification.** `movie-run.ps1 -Row intro_movie -PromptFor 25 -At 35` (diagnostic build, since removed): before the
fix fmv_popup stayed on frame 19 after HideMovieSkip; after it, frame 20, and the 35 s frame shows no prompt. APT, menu
and HUD tests pass, as does the full skate-game suite.

`--mute` silences the movie's sound too (2026-11-16: it plays through its own Bevy audio player, not the native mix,
so muted test runs were audible); `MOVIE_START` logs the gain.

### Subtitles

**Problem.** The movies with spoken lines (intro_movie, coach_frank_intro, the pro challenge and story scenes) had no
subtitles, and Game Settings > Subtitles was grayed (no effect).

**Root cause / retail.** The disc's `data/fe/subtitles/subtitles.big` holds 186 `SubTitles_English/<name>.sub` cue
files: little-endian 8-byte records `{u16 time in tenths, u16 pad, u32 string hash}` up to time 0xFFFF (loader
8263E368, b955 §5). The hash is the language table's own key: the labels file (`language_labels_global_skate3ng.bin`)
is a 28-byte header, then one `{u32 hash, u32 pool offset}` record per string sorted by hash, then the string pool
[data: `0x4CA6AF68` -> `SUBTITLE_NIS_99_6328_PRO-PHIL-NIS_1_10` -> "Yeah, Cardiel in the house!"]. Every file ends on
`0x001A20BA`, which no string has: the lookup fails and the line clears (825E0E90). At the movie's start (825E0790)
subtitles are forced on `2914` rows in French / German, shown when the option byte +157 or forced is set, and a
Subtitler runs only on `A3A0` rows while shown; it fires a cue when `time x 100 ms <= FE time` (8263E6B0). The line
goes to the subtitle movie (`gSubtitle`): HUD command 7 calls `ScreenManager.RefreshSubtitle`, which reads
`Game.GetMovieSubtitleString` (the line, or `#` for none; 825A55D8) and bottom-anchors up to three lines over its
dimmer; only the text up to a `[` page break shows for timeline cues (825E0F60).

**Change.** Setup (`tools/asset_pipeline/subtitles.py`, core step) writes `private/stock/subtitles.json`, every cue
resolved to its string label (`null` = clear). `skate_data::subtitles` loads it (language folder, else English; names
without case) and plays a cue list (`Subtitler`, `first_page`). The movie player decides at the start (after the
stored settings are in) and publishes the line as `MovieSubtitle`; the menu stage answers
`Game.GetMovieSubtitleString` / `Audio.HasSubtitles` from it and calls `RefreshSubtitle` on each new line while
shown. The Subtitles row is no longer grayed. Log: `MOVIE_SUBTITLES <file>: row / option / forced / cues / running`
and `MOVIE_SUBTITLE <s> <label>` per cue. Mods: cues are labels, so a mod's strings replace the text; a mod cue file
or Lua control is not wired yet. NOT RETAIL YET: the game language word (the movie language stands in, as for the
audio).

**Verification.** `skate_data::subtitles` tests (cue timing, English fallback, first page) and
`tools/asset_pipeline/test_subtitles.py` (record order, hash to label, clears). Release run
(`.local/poc-tests/movie-run.ps1 -Subtitles`): intro_movie logs 56 cues, `SUBTITLE_INTRO_VIDEO_10` at 3.5 s, the
two-line text shows over the dimmer, and with the prompt up the chyron sits above the subtitle bar.

### Career movies

**Problem.** A new career started straight in the world: retail's intro_movie and coach_frank_intro never played.

**Retail.** A profile with "new career pending" (+0x68) runs the boot flow before its first world load (research b868
§1, Ghidra 82704038..827052C0): FE 9 welcome (difficulty, camera), FMV intro_movie (82704C30), FE 10, FE 46 team name
keyboard, FE 63 team logo, FE 67 FirstTeamMember (Create-a-Skater; entering it sets progress bit 1, and a career quit
after that resumes there), FMV coach_frank_intro while the first save runs (827051A0), FE 11 Coach Frank's question,
then the world load and the first challenge sign-up (827CF960). The PS3 new-profile run (2026-11-16,
`.local/research/rpcs3-new-profile-2026-11-16/review.md`) shows the same order: welcome, intro_movie, team and skater
creation, coach_frank_intro, then skate.School.

**Change.** `skate_core::career::boot_flow` holds the steps as data in retail order (`BootFlow`, `start` = the resume
rule). `career.rs` `run_boot_flow` decides once per session when the career has loaded: with the pending flag set it
runs the steps, each movie through `movie_player::play_row` and waiting for the movie before it (ea_intro at boot
included); the first challenge sign-up (`queue_first_challenge`) waits until the flow is done. NOT RETAIL YET: the
screens (FE 9, 10, 46, 63, 67, 11) are not ported and are skipped with a log line, so the two movies play back to back;
the progress bit is still set at FE 67. Off in automated runs (`SKATE_BACKGROUND_WINDOW=1`) and with
`SKATE_CAREER_SKIP_START=1`; `SKATE_BOOT_FLOW=1` / `0` forces it on / off. Log: `SKATE_CAREER boot flow ...`. Mods:
the steps are a resource (`BootFlowRun.flow`); a Lua entry point is not wired yet.

**Verification.** `boot_flow` tests (retail order, the resume rule). Release runs with a fresh career
(`.local/poc-tests/career-boot-run.ps1`): the flow starts at step 0 and plays intro_movie; with progress bit 1 saved it
starts at FE 67 and plays coach_frank_intro.

### Welcome screen

**Problem.** A new career had no welcome screen: no difficulty or camera choice before intro_movie.

**Retail.** FE 9 WelcomeFlow (template 9, `bootflow/welcomescreen`, only in fedynamic.big; research b1007): the movie
shows a 3-row difficulty menu (Normal preselected) and asks `TeamManagement.GetNewTeamState()` (0 here, 10 for the
FE 10 instance). Native side: the menu pick (82609D38, rows 0..2 only, no back) stores the difficulty (+0xB4) and shows
the Low camera page; on the camera pages (82609BA0) Left / Right toggle Low / High and A stores the camera (+0xBC) and
the screen closes (826099C0); every other key is taken there, while the difficulty and closing states pass keys on to
the movie. Sounds `0x373D38D7EA167A71` (choice) and `0xAD8BF9A0B4997B49` (toggle). The PS3 run shows the same panel
(difficulty, then "Choose your camera" with Low / High).

**Change.** Setup exports `bootflow/welcomescreen` and `bootflow/coachfrank` with the fedynamic movies
(`tools/prepare_menu_movies.py`). `skate_core::career::welcome` is the native state machine; `welcome_host.rs` answers
the movie's natives and holds the request / result (`WelcomeScreen`); the stage opens it when the boot flow asks
(FE 9 step), shows it on its load, routes keys as retail and pushes the movie states. On close the boot flow writes
the difficulty and camera through the Game Settings bindings (`retail_menus::write_setting`: applied and saved). An
install without the movie skips the step. NOT RETAIL YET: the camera previews (cameraLow / cameraHigh FMVs drawn into
the page's image boxes) are black, and the result is not yet copied into a career record (b1000).

FE 10 (after intro_movie) is the same movie on its team page: `GetNewTeamState()` answers 10, the movie shows
`ID_BOOTFLOW_PICK_TEAM_NAME` ("In Skate 3 you're starting a board company... pick a name and a logo!"), A closes it
(8260A198 / 8260A218: every key taken, no back) and nothing is written.

FE 46 (after FE 10) asks for the team name with the existing keyboard (b1004 / b1010, step 82704DD8): title
`ID_WELCOME_NEW_TEAM_HEADER` "Enter New Team Name", default `ID_WELCOME_NEW_TEAM_NAME` "New Team", 16 characters,
mode 1, empty passed on (`TextEntryRequest::team_name`). OK writes the text into the career's team name, cancel keeps
the default (82705470 / 82705528); the career is saved. The boot flow asks through `BootTextEntry` and the stage
pushes FE 46. An empty name (82705470 tests the byte length) shows popup 0x12E (record 8302C260, dumped 2026-11-21:
`ID_COMMON_WARNING_TITLE`, `ID_TEAM_MANAGEMENT_EMPTY_TEAM_NAME`, one `ID_COMMON_OK`), whose OK (827055E8) opens the
same keyboard again (`BootTextEntry::retry`, `EMPTY_TEAM_NAME_POPUP` in `retail_menu_stage.rs`).

While the flow's screens are up (not during the customizer FE 67 opens) a black layer sits between the world and the
raised APT composite and the gameplay HUD hides (`career::boot_flow_covers`, `movie_player::boot_backdrop`): retail
runs the flow before the world loads, so its screens and the keyboard sit on black. Our world is already loaded:
since 2026-11-21 it is held while the flow covers the screen (`movie_player::boot_backdrop`, a named hold on the
pause manager), so nothing simulates under the screens, as in retail. Status: written while the desktop was busy, not built yet.

### First team member

FE 67 FirstTeamMember ("Start your team!", template 113, `TeamSkater_Selection/team_management_boot`, now in the menu
export; research b1004 §2): the movie lists Customize Skater / Preset Skaters / Play Skate 3! and asks
`TeamManagement.GetTeamData()` ([count, team name, logo asset, member photos]) and `Game.GetProfileName()`. C++ takes no
keys (8274CA90 returns 0); the menu's pick (82650EC0) first asks for the profile name when the profile has none
(keyboard "Enter your name:", default "The Legend", 16 characters; OK sets the name and profile bit 2, cancel stays),
then 826513E8 runs the row: Customize opens FE 15 on the player, Presets FE 68, Play posts 0x8F (the step is done).
Ours (`skate_core::career::first_team_member`, `welcome_host.rs`, the boot flow): the profile name is our player name
(`Multiplayer::set_player_name`, the name others see); Customize opens our customizer and the step ends when it closes
(retail re-opens FE 67, which closes itself and posts 0x8F); Presets opens FE 68 (below) and the step ends when it
closes, the same way; Play ends the step. NOT RETAIL YET: GetTeamData has no logo or photo assets (FE 63 logos and the
player photo build are not ported); the menu shows at once (no photo build). Status: written while the desktop was busy,
not built or run yet.

### Preset skaters

FE 68 ImportSkater (template 114, `TeamSkater_Selection/import_skater`, in the menu export since 2026-11-22; research
b986; Ghidra vtable 82305678, decoded 2026-11-22) [code + data]:
- **Opening (82653460).** FE 67's Presets row opens it on slot 0, mode 2. It loads group 5 of the resource manager's
  skater lists (`progression_preset_teammate`, sorted by the localized name; group 2, community skaters, too when
  online).
- **Show (82653648).** The movie gets SetState 10 (the list), 11 (the preview photo) and, in mode 2, 20 (the first
  teammate background).
- **Rows (`ImportSkaters.GetSkaters`, 825C3E20).** [count, texts, header flags]: each group with rows gives its title as
  a header (`ID_PRESET_LISTING_TITLE`), then its rows' names. With no rows at all: `#NO SKATERS` / `#NOT A SKATER`.
  The list index counts the headers (82654F10), so preset N is row N + 1.
- **Highlight (82653750).** SetState 31 (no gamercard; 30 only for community skaters), then 11.
- **Pick (826536D0).** While idle it keeps the row and asks popup 0x125 (`ID_COMMON_WARNING_TITLE`,
  `ID_CONFIRM_USE_PRESET`, No / Yes; mode 0 asks 0x123 `ID_IMPORT_SKATER_CONFIRM_IMPORT`). Yes (82654EA8) starts
  loading.
- **Apply (82653960).** It loads the row's preset (8255A518 -> `SavedRecipe<id>.bin`) and copies it into the slot
  (8257DC48). It rebuilds the skater and the slot's photos, then pops the screen. Only mode 0 posts message
  0x478AA499 on the way out.
- **B (82654270, button 3).** It pops the screen while idle (`core_b_button`).

Ours (`skate_core::career::import_skater`; `ImportSkaterScreen` and the natives in `welcome_host.rs`; the stage in
`retail_menu_stage.rs`): the same rows, order, popup, B and states. Yes puts the preset's profile on the player through
the customizer (`apply_player_preset` in `career.rs`: resolved, worn and saved as the outfit, like a Create-a-Skater
save). Mods reach the rows through the teammate preset data (the vault rows and recipe files; `sdk.career` lists
them).

**Preview photos and the retake** [code]: a highlight (82653750) looks the row up in the screen's photo map (+0x3C).
A row with no photo is photographed when the skater builder is idle; while it is busy, the latest highlight waits
(+0x324) and 82653960 state 0 takes it later. The photo uses the slot's card setup (collection FEFE76B65E8F6197, the
same `team_profile_0N` row as the player's card photo). `IsPreviewPhotoLoaded` (825C4258) is "the row is in the map",
and `GetPreviewPhotoAssetId` (825C42D0) is the row's asset id. The highlight then sends state 11 again. On Yes, states
2..8 apply the preset to the slot, wait for the skater rebuild and request the slot's photos again (5C628A8FA0C3FAEE,
AB0A7B6A454D6B99 mode 1, FEFE76B65E8F6197 mode 2). Ours (`skater_photo.rs` `Previews`): the highlighted preset is
worn on the player for its photo (FE 68 covers the world) once the outfit is on, in the card photo's setup; the
finished photo is resident as `skater_preview_<key>`, state 11 is sent again, and B puts the player's own look back.
Yes keeps the picked look, takes the player's card photo again after the outfit rebuild
(`SkaterPhoto::retake_after_pick`) and pops the screen once that photo is ready. With a custom model (no outfit to
swap) rows read as loaded with no picture. The movie highlights its first row while state 10 builds the list, so the
first preset is photographed when the screen opens.

Fixed on the way (2026-11-22): `ImportSkaters` was not registered as a native object on the menu stage, so the movie
got no rows and the preview wheel spun forever. A photo waits until the worn outfit has matched the look for two
frames: the swap lands after the frame's visibility pass, and a photo one frame earlier showed the previous look.

NOT RETAIL YET:
- The photos have the card photo's own gaps (camera vectors and pose clip, see doc 43 "The skater photo").
- Retail's photo map drops its oldest photos (the +0x1E0 list ordered by +0x308); ours keeps every photo until the
  screen closes. The limit is not decoded.
- The banner photo (AB0A7B6A454D6B99) and the packed skater blob are not taken again after a pick (no banner yet).
- Community skaters (group 2, online) are not listed.

**Verification.** `import_skater` tests (rows and headers, index to row, the confirm and B rules);
`welcome_host::the_preset_list_answers_its_rows_while_open`, `welcome_host::preview_photos_answer_by_row`;
`skater_photo` tests (a pick keeps its look and retakes the photo; loaded when taken, missing or unavailable). Needs a
Refresh Game Files for the movie. Release runs (`career-boot-run.ps1 -From 5`, FE 67 Presets): the list shows Skater
01..08 under Preset Skaters; Skater 01's photo shows on open, Skater 02's after Down, each posed with the board in
hand on the teal backdrop; every photo's drawn parts match the look (`SKATER_PHOTO start ... parts`, `look`); Yes
(Up, A on the popup) applies `preset01`, retakes the player's photo, then closes the list and the flow goes on to
Coach Frank. Not run in game
yet.

FE 11 Coach Frank (after coach_frank_intro; b1010): `bootflow/coachfrank` (template 10) asks "Would you like to stay
here and learn the basics with Coach Frank?" with a Yes / No picker; every key goes to the picker and its
`Game.OnMenuSelect` row closes the screen (8260A288: row 0 Yes -> `schoolstart`, any other row -> `gamestart`). Ours:
`StartLocations` (data) maps the answer to our teleport destinations with the same labels (`dist_skateschool`,
`dist_university_library`), the boot flow travels there through `ChallengeMapTravel`, waits until that map is current
(20 s fallback) and the first challenge is picked by the destination's world (827CF960 reads the destination's world
key, not the loaded map). Note: the map-based fallback rule `skate_school` does not match the `SkateSchool` map file.

**Verification.** `welcome` tests (pick, pages, close, no back, ignored cancel, the FE 10 page); `welcome_host` test;
`text_entry` team name test; `boot_flow` start-location test. Release runs: FE 11 shows Coach Frank's card with Yes /
No as retail; A picks Yes and travels to `dist_skateschool`. Pending (written, not built or run yet): the first challenge waiting
for the school map and picking `tut_sksc_01` (the first run picked `ftl_univ_02` too early, before this fix). Release runs with
injected keys (`career-boot-run.ps1 -Keys`): the difficulty panel draws as retail; Down, A, Right shows the High page;
A closes it, logs difficulty 2 camera 1, writes both settings and starts intro_movie. From step 2
(`SKATE_BOOT_FLOW_FROM`, a test aid): FE 10 shows the team page, B does nothing, A closes it; from step 3 the keyboard
shows "Enter New Team Name" / "New Team"; B keeps "New Team", A on "q" then Start saves "New Teamq" in `career.json`.

**Planned.** A streaming 6-channel audio source folded to stereo with the retail output table, its sample count as the
movie clock (b926 §3); the FE 24 screen with the retail skip rules, subtitles (`.sub`, b955 §5) and the fe_movie rows
as data; Lua `sdk.movies.play(key, on_finished)` / `skip()`; the boot (ea_intro) and career hooks (b926 §1).

**Verification.** `skate-movie` tests: the container reads its header and chunks in order, rewinds, and refuses a file
without `VP60` or with a cut chunk; a non-VP6 packet is a header error; data-gated `a_disc_movie_decodes_like_ffmpeg`
(`SKATE_MOVIE_CHECK=<file.vp6>;<sha256>`) compares a disc movie's frames with the FFmpeg hash.

**Open questions.** The 4th audio subsong (b898); the two decoder fixes are offered upstream as ruffle-rs/nihav-vp6#4 (2026-11-15); drop the vendored copy once they land.
