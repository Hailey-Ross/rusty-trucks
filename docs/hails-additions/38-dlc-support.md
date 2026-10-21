# DLC support: the player's own Skate 3 DLC packages

Part of upstream PR #52 (user, 2026-10-20: "dlc support is going into PR 52 with everything else"). Branch
`world/replay`.

## Problem

Setup and the engine only knew the base disc. Retail Skate 3 has downloadable content: maps (Maloof Money Cup 2010
NYC, Danny Way's Hawaiian Dream, the Sanatorium, the Art Gallery and Zen parks, the Create pack lots), clothing, and
the Skate.Create Upgrade Pack's replay editor with the look channels (brightness, contrast, saturation, tint). Players
who own DLC have it as Xbox 360 content packages; none of it may be committed, so setup has to read it from the
player's own files, like the disc.

## What retail does

- The packages are STFS (`LIVE`, content type 0x2 marketplace, title 0x454108E6), each holding one EB BIG v3 archive
  in the base game's formats [data, research b374].
- The replay editor checks product `CCF0007` (the Skate.Create Upgrade Pack) at about 25 call sites through
  `82518AD0` (string at 0x822FD708); with it, the editor opens template 71 `replay2/replay_editor3.swf` instead of
  72 `replay2/replay_editor2.swf` (`826102A8`) [code, b410 / b411; main counted 7 of 32 call sites loading that
  string]. The Create package carries `DLC_PRODUCT,CCF0007` (`creator0_product00000000.unlock`), the movies
  `data/fe/source/screens/replay2/replay_editor3.*` and `replay_help2.*`, and its language tables with the channel
  labels [data, main checked in the package].
- Maps: see "Next: DLC maps" below (b374, b412).

## Change (so far)

- `tools/asset_pipeline/stfs.py`: a small read-only STFS reader (public format; Xenia's `stfs_container_device` as
  the reference): header, volume descriptor, file table, block offsets past the interleaved hash tables, copying or
  reading a range of a consecutive file. Chained (non-consecutive) files are refused; none of Skate 3's packages
  need them.
- `tools/asset_pipeline/dlc.py`: the optional DLC folder (`SKATE3_DLC_DIR`, else the folder the last setup used,
  else a `dlc` folder next to the game folder as in the recomp's layout), a short identity of the packages found,
  copying out only the BIGs a step wants (decided from the BIG's index, read inside the package), and the DLC
  language pairs. The DLC labels table ends with one NUL where the disc's end with two, another pack's has one
  more; empty padding is evened out, real extra text is refused.
- Setup (`install.py`): the DLC identity joins the hud group's version, so adding or removing a pack refreshes the
  menus; with no packages every version stays exactly as before. The folder is saved in `installation.json` and
  passed to the hud stage (`asset_exports.hud` -> `prepare_runtime_huds.py --dlc`).
- Menu movies (`prepare_menu_movies.py`): `DLC_ROOTS` (replay_editor3, replay_help2) are exported when a package
  carries them; only BIGs holding one of those movies are used, so other packs' front-end files never replace the
  disc's. The vendored `extract_project` takes `extra_archives` (after the disc's, so they win) and checks them in
  its cache test. DLC text adds to the disc's; a disc label keeps its text.
- Moddability: the DLC roots and the per-pack content are data; a mod's own movies keep their own routes.

## Verification

- The reader on the user's Create Park Upgrade package: header "Skate.Create Upgrade Pack", the 95,990,656-byte
  `creator_00000000.big` copies out at its file-table size and parses (1262 entries, the count b374 found;
  `creator0.ver` = `695423:sk8build`).
- The menu export with the disc and the recomp's DLC folder (9 packs): 43 movies including
  `source/screens/replay2/replay_editor3` and `replay_help2`; the language has `ID_REPLAYEDITOR_CHANNEL_BRIGHTNESS` =
  "Brightness", `ID_REPLAYEDITOR_CHANNEL_TINT` = "Tint"; only the Create pack's BIG is copied (96 MB; first runs
  copied every pack's BIG, about 1 GB, before the index check).
- Unit tests: `tools/asset_pipeline/test_dlc.py` (a synthetic LIVE package read across a hash table, refused titles
  and chained files, the DLC folder rules, the language padding, and a data-gated check of the user's Create pack).
- Full setup refresh on the dev install with the user's original DLC folder (2026-10-20): only the hud group rebuilt;
  the installed menu set has replay_editor3 / replay_help2 and the creator text; `--check-assets` SKATE_ASSETS_READY.
  (A first attempt failed: `prepare_runtime_huds.py` runs with only `tools/` on its path; fixed by adding the repo
  root like `prepare_trick_guide_scene.py`.)
- Game run on University (`SKATE_VERIFY_REPLAY=1 SKATE_VERIFY_REPLAY_AT=12 SKATE_VERIFY_REPLAY_EDITOR=look
  SKATE_VERIFY_AT=15`, `--map <install>/maps/University.skate --verify <png>`): `RETAIL_MENUS open Replay3`, the
  Create pack's Tint panel (Red / Green / Blue / Alpha Tint rows, Blue at 5 notches, Alpha at 4) over a frame tinted
  blue under the panel and the HUD. Not play-tested by the user yet.

## The replay editor on replay_editor3 (engine)

- `retail_menu_stage.rs` `REPLAY_SCREENS`: the Create pack's screen (Replay3 / ReplayHelp2, `channels`) first, the
  base screen second; the first whose movie was exported and whose template is registered is opened. Tables read
  from the disc's own executable (129 templates; TU3 has 133) lack Replay3 / ReplayHelp2, so those two are
  registered after the table's own (NOT RETAIL order; the ids only need to be unique). The stage sets
  `ReplayLookScreen` while the Create screen is shown.
- `replay/editor.rs`: items 9..12 (brightness, contrast, saturation, tint), `CHANNELS` (label ids and items per
  channel, data), `channel` / `tint`, `cycle_channel` (Y), `move_row` (base screen: the 9 rows wrap; Create screen:
  the camera rows wrap, the tint cursor wraps R .. A, other channels hold one row), `mode_value`, `mode_slider`,
  `tint_value`; Left / Right edit the channel's lane through the look lanes (doc 37 "M3 step 2").
- `replay_menu_host.rs`: the natives GetMode, GetModeText, GetModeValue, GetModeSliderValue, GetCurrentTint,
  GetCurrentTintValue, GetCurrentSliderTintValue, Get{Red,Green,Blue,Alpha}TintValue; states 23 (channel changed)
  and 24 (tint cursor moved); look value changes post 6. Y is K on the keyboard.
- Tests: `create_pack_channels_edit_the_look_lanes`, `create_pack_natives_and_states`.
- NOT RETAIL YET / open: the movie's own state constants (23 / 24 by elimination), the tint overlay's blend, our
  own fallback overlay for players without the Create pack, a game run on the Create screen.

## Earlier plan: the replay editor on replay_editor3

The host drives replay_editor3 when the manifest has it (b411): Y cycles the channel (state 23), the Tint channel's
Up / Down move the component cursor (state 24), Left / Right post 6 with the retail steps; natives GetModeText,
GetMode, GetModeValue, GetModeSliderValue, GetCurrentTint, GetCurrentTintValue and the four tint getters. CCF0007
counts as owned on PC (user: look rows always on). Without the package: our own one-line channel / value overlay.

## Next: DLC maps (b412, Maloof Money Cup 2010 NYC first)

Research `.local/research/b412-maloof-nyc-map-job.md` [data, from the user's package; main checked the
`map_starts.py` claim]: the Maloof NYC BIG has an authored start (`global_locators/DLC_BAM/DIST_MaloofMoneyCupDLC`,
`Z_MaloofMCNYC_Start`) and a vault with a `world` row (WorldStream `DIST_MaloofMoneyCupDLC`, skybox
`DIST_skyboxMaloofNYC`) and an `fe_locations` row. Changes: DLC vaults converted with the base schema into
`private/stock/dlc/`, map jobs as `(archive, district, label, source)`, `map_starts.py` resolving each row against
its own vault's `.bin` (today it always uses the disc's `skatercollections.bin`), label `MaloofMoneyCupDLC`, a
`source: dlc:<pack>` catalog field, the engine's collections merging `private/stock/dlc/*.json`.

## DLC map conversion (b415, Maloof NYC)

[data, parsed in memory with the functions setup uses; code lines checked by main] The DLC district stream converts as
it is: `DIST_MaloofMoneyCupDLC` loads every Sim / Pres / Tex record (160 / 266 / 59), 261 textures, 55 models (675
meshes with material binding), 348 grind splines, 52 collision meshes, 160 arenas. `cSim_Global.xsf` is not needed
(setup uses `load_district_stream`; the base BlackBoxPark has none either). What breaks: the maps group globs only
`data/content/worldDIST_*.big` (`install.py` job list) and derives the district from the file stem
(`convert_map`), so the DLC BIG is never offered and would fail with "Missing district stream". Changes: DLC jobs
from `dlc.extract_bigs` with an explicit district and label, `map_job.py` `--district` / `--label` (defaults keep base
jobs identical), results keyed by label, the DLC identity in the maps group's version. Open: one real conversion run,
the DLC sky (`world/models/DIST_skyboxMaloofNYC*`), the authored start (b412).

## DLC maps in the map lists (b416, Maloof NYC)

[data, from the Maloof vault with our readers; main checked `map_library.rs` and `challenge_map.rs`] The DLC only adds
rows: a `map_filter` `maloofmoneycupdlc` (title `ID_WORLD_MALOOFMONEYCUP_NYC`, Lua keeping `fe_locations` items in
world `dist_maloofmoneycupdlc`), a `map_category`, three `map_listing` rows (Locations order 7 after the 7 disc
folders), one `fe_locations` row (`ID_LOCATION_MALOOFMONEYCUP_NYC`, locator `Z_MaloofMCNYC_Start`) and the world's map
picture `DLC_Maloof_Map_512`; both names read "Maloof Money Cup 2010 NYC" in the pack's own language tables. Unlike
the disc, the location's district key (`maloofmoneycupdlc`) differs from its World (`dist_maloofmoneycupdlc`). Ours
today: the PC overlay labels maps by file stem, the Challenge Map reads only the disc collections and fixed
`LocationRules`, `teleports.json` only disc rows. Changes: merge DLC collections in the engine, resolve the DLC `.bin`
pointers in setup, match Challenge Map leaves on the World field with rules built from each filter (today's pairs as
the fallback), merge every pack's language tables (not only the movie-carrying one), DLC rows in `teleports.py`,
`maps.json` entries with source / world / name id for the overlay label (stem stays the map id), the DLC map picture.

## DLC sky and map picture (b417, Maloof NYC)

[data, sky.py's own functions run in memory on the merged rows; main checked `sky.py`] Sky: the DLC world row
resolves `sky_model` = `world/models/DIST_skyboxMaloofNYC` (+ `_Textures`), which sit in the Maloof BIG; the parsers
work unchanged (2048x256 dome, 512x16 sun). `sky.py` today reads skies only from `miscload.big` and offers only worlds
with a disc `worldDIST_*.big`. Changes: DLC vault rows into its collections, worlds from the map job list, each
`.rx2` looked up in the pack's BIG first (Danny Way / San Van have no sky model of their own: miscload fallback), the
DLC identity in the environment group's version. Map picture: the world row's map name resolves to
`DLC_Maloof_Map_512` only against the DLC's own `.bin` (the disc `.bin` at that offset reads another name); the image
is `data/fe/source/images/map/DLC_Maloof_Map_512.rx2` in the Maloof BIG. Changes: resolve per vault, add the DLC
BIGs' map images to the image export, and let the Challenge Map fall back to a location's World field.

## DLC map audio (b414, Maloof NYC)

[data, from the user's package and disc with our readers; main checked the `ambience.rs` / `map_audio.rs` lines]
Maloof NYC adds no sound banks and no ambience bed: `data/audio/DLC_Maloof.ems` (45 records, 14 sound ids, 11 already
in base .ems files, one new type-6 PA row that is not played), its MAP_CLASS row lists only `DLC_Maloof.ems`, and its
53 cSim cells' region layers point at base rows (ambience zone `skate_school`, emitter set `e_univ_chan_center`).
Changes: the audio group takes the DLC folder (+dlc version like hud), `audio_export` reads the DLC vault rows, the
DLC `.ems` and the DLC district's region layers, the engine merges the DLC collections, and the map file is named
`MaloofMoneyCupDLC` so `map_audio::build` finds its regions. `ambience.rs` `BEDS` is only a fallback without zone
data. Open: no recomp trace confirms retail plays the SkateSchool bed there.

## Open questions

- Whether DLC maps stay or go when the DLC folder is removed (user decision).
- The DLC maps' display names (the DLC language tables), trigger volumes and `.ems` audio.
- The movie's own constants for kUpdateMode / kRefreshTint / kReplayMode_* (from our export of replay_editor3).

## DLC maps, part 1: conversion, starts, skies, audio (2026-10-20)

- Jobs (`dlc.map_jobs`, `install.py`): one map job per DLC district stream that holds a whole map (`<D>_Sim.xst` and
  Sim cells; the Art Gallery proxy has neither and is left out), label = the stream folder without `DIST_` /
  `Dist_`, the pack as `source` in `maps.json`; `map_job.py --district --label --source`, `convert_map` extracts only
  that stream from the pack BIG; disc jobs unchanged. Only packs carrying maps change the maps / environment / audio
  groups' versions. The user's packs give 7 maps: MaloofMoneyCupDLC, DLC_DW_MegaCompund (Hawaiian Dream),
  SanitariumDLC (After Dark), AGPark and Zen (San Van), BackLotParkDLC and DownTownSkateParkNightDLC (Create pack);
  all seven converted (2 to 18 s each) and passed the game's `--check-assets` in a test stage.
- Vaults (`dlc.vaults` / `dlc.load`): each pack's collection vaults (the `.vlt` / `.bin` pairs directly in `data/db/`)
  converted with the disc's schema, rows tagged with their vault, plus the pack's global locators and, for names
  those lack, each stream's own `<D>_Sim.loc` (XML LocationTable). Written to `private/stock/dlc/` for the engine,
  which merges them after the disc's rows (`skate-data` `Collections::load`, test
  `dlc_rows_add_to_the_disc_rows_and_inherit_from_them`).
- Starts (`map_starts.py`): DLC rows resolve their text against their own `.bin`; every DLC map gets its authored
  start (Maloof `Z_MaloofMCNYC_Start` at (0.56, 0.16, -43.42) facing -pi, as b412 read; Zen `Fn_Start_Zen`,
  DownTown night `Z_SP_DT_Start`, both from the streams' `_Sim.loc`); the disc's ten starts are unchanged. Each
  district now tries its candidates in order (world start locator, then the fe_locations rows) and skips a locator
  still at the identity matrix (never placed): Art Gallery's world start `z_ArtGallery` is such an entry in
  `DIST_AGPark_Sim.loc` (no floor below; the map check warned), so it starts at its fe location
  `freeskate_artgallery_locator` (-24.4, 51.2, -46.0) from the pack's global locators (ours: which of the two retail
  uses for a fresh visit is not traced; test `test_an_unplaced_world_start_falls_through_to_the_fe_location`).
- Skies (`sky.py`): DLC worlds offered, each model read from its own pack first; MaloofMoneyCupDLC, AGPark and Zen
  get skies; the disc's 30 sky files are byte-identical. Sanatorium and the Create pack lots name render locations
  no vault holds: retail's lookup (TU3 0x82B6DE10) returns nothing and 0x827950A8 then selects `default` (research
  b419), so `environment.world_environment` does the same and those maps get their skies. NOT RETAIL YET: Hawaiian
  Dream's "sky" is no sky material (retail swaps in a multi-mesh copy built into the executable); it gets none yet.
- Map names (`map_names.py`, wired in `install.py`): every map in `maps.json` gets `display` (its Skate 3 name) and
  `name_id`; the engine's lists show `display` (`map_library.rs`). Rule (ours, no retail screen lists maps by
  stream): the `map_category` row keyed like the world row, else the world row's own name, resolved in the disc's
  English table and the packs' own; text with font glyphs or none falls back to the label in words. Per the retail
  data: MegaPark = "Stadium", StartPark = "PCU Training", IndustrialSkatePark = "Team HQ", AGPark = "The Art
  Gallery", SanitariumDLC = "The Sanatorium" (the DLC category rows drop the world row's `dist_`: `sanitariumdlc`;
  the user's video of Skate 3's challenge map with the packs shows "The Sanatorium"). Test `test_map_names`.
- Audio (`audio_export.py`): the DLC rows join the disc's, the packs' `.ems` (7 files; Maloof's places the 7 disc loop
  banks b414 listed) and every DLC district's region layers keyed by label; the disc's 23 `.ems` are unchanged.
  Not yet: the new DLC beds and banks (After Dark `24_dlc_ambience`, the Create pack `25_dlc_ambience`, b418).

## Teleporters (disc and DLC maps)

Problem: the DLC maps' teleporters (user: "there are several in the dlc maps that make it not only easier in some
cases but also possible in others to reach the top of ramps") and the disc's (University to the Stadium and the other
parks; user: include them) did nothing; the converter found no volumes for most of them.

Root cause (research b420, main-verified on the files): a retail teleporter is a challenge of the `teleports` group,
not part of the map's world stream. Its bank row (`data/db/challengebanks/*`) has the title; its local data vault
(`data/db/challenge_local_data/<challenge>`) has TeleporterTrigger ("DISTRICT|volume|0xid" plus the volume's id) and
TeleportExitLocation (target `world` row plus locator name); the volume sits in the challenge's hull
(`data/content/missions/<challenge>/cSim_Global.xsf`) or, for a few, in the world stream. Maloof Money Cup 2010 NYC has
none; Hawaiian Dream has 4 (01 to 04; 04 lands on 01's exit, the top of the mega ramp).

What retail does (b421 / b423, stategraph `challenge_stategraph/teleport`; button D-pad Up is the user's fact): every
teleporter of the loaded district is armed; entering its volume shows the prompt with the title, leaving hides it;
D-pad Up asks `Popup.ShowYesNoPopup(Title, "ID_PAUSEMENU_CONFIRM")` (options ID_COMMON_NO / ID_COMMON_YES, read at
`828556D8`) for one exit, a list for several; Yes takes exit 0. Returning to waiting resets the volume: no cooldown
(user). An exit in the loaded world is placed at once (`82864038` -> skater `825926F8`, the actor-reset chain of
`request_manual`); another world is the far path with its loading screen (`828646E0` compares the target world-file
id 0x830B7B18 with the current one 0x830B7B10, main-verified in the recomp code).

Change:
- Setup (`tools/asset_pipeline/teleporters.py`, maps group): reads every challenge vault of the disc (`db.big`,
  `missions.big`) and the packs, keeps the rows whose trigger names a volume (a pack's vault also carries its group
  template with an empty trigger), resolves the volume (hull or stream), the exits (`map_starts.all_locators`) and the
  title text, and writes `private/teleporters.json`. Result on the disc and the user's packs: 20 teleporters on 9 maps
  (University 4, SkateSchool 5 incl. the four "Back to the top", StartPark 2, Hawaiian Dream 4, one each on DownTown,
  DownTownSkatePark, Industrial, IndustrialSkatePark, MegaPark); `teleport_dwgh_01` / `02` point at a Danny Way map no
  pack ships and are left out. Tests `test_teleporters`.
- Engine (`crates/skate-game/src/teleporters.rs`): arms the loaded map's teleporters (hull volumes join the map's
  trigger volumes), shows the prompt while the local skater is inside, D-pad Up raises retail's confirm popup (No /
  Yes; a list for several exits), Yes posts a serialisable `TeleporterUsed` and moves the skater: `travel_ex` on the
  same map (the skater's own board state), the map transition plus the Challenge Map arrival for another map
  (multiplayer refuses a map switch, as the Challenge Map does). Tests in the module.
- Mods (`sdk.triggers.teleporter`, `sdk/ENGINE_API.md`): add, replace (same id as a retail teleporter) or remove
  teleporters; event `teleporter_used`; cleared when the mod stops.
- The prompt is Skate 3's own action overlay (research b426): `actionoverlay2` (template 59 SignUpHUD) joins the
  menu movie set, opens once on the stage's popup layer, and the teleporter sends state 2 (the C++ value of
  82665E88; the `kActionOverlayState_*` names are set on `_global` 0..6 because no movie defines them) with the title
  through `SignUpPoster.GetTeleportTitle`, state 0 when it goes. A set exported before the movie keeps our text line
  (no set version bump, so no forced refresh), and without the stage the confirm is skipped. Verified on University:
  "Enter Stadium skate.Park" in the retail font with the logo glyph. OPEN: no D-pad Up glyph shows beside it (the
  Teleport state sets no button type; where retail's glyph comes from is not found).
- NOT RETAIL YET: the arrival speed is the same reset as the session marker's (b423: whether retail zeroes speed is
  open).

Verification (2026-10-20, verify switch `SKATE_VERIFY_TELEPORTER=<id>` with `--verify`: starts the skater in that
teleporter's volume and presses once, no popup): University `tele_world_to_stadium` armed 4 teleporters, loaded
MegaPark and landed at (67.2, 0.03, -134.3) on its exit locator (67.3, 0.2, -134.5), riding, with MegaPark's own
"Leave Stadium skate.Park" prompt up; Hawaiian Dream `teleport_dwmc_01` landed at (-8.0, 39.4, 64.3) on the Fun Track
(exit (-8.0, 40.3, 64.6)). The confirm popup and the D-pad Up press itself are not play-tested yet. Tests: setup 251,
skate-game 862, skate-mods 106, skate-data pass.
