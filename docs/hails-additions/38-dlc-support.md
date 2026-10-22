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
  b419), so `environment.world_environment` does the same and those maps get their skies. Hawaiian Dream's sky model has
  six meshes (research b427 / b428): the dome (`sky.default`) and five scenery meshes (trees, rocks, mountains) drawn
  with the world's environment effects. Retail's title update swaps the pack's model for a copy built into its
  executable (0x828A1460; not on the disc), identical except material 2 = `environmentsimple.alphatest` without its
  specular. Setup rebuilds that from the player's pack: materials bound per mesh by GUID (the backdrop binder), the
  dome picked by its `sky.` shader, the override row in `tools/asset_pipeline/sky_overrides.json` (keyed by model name
  like retail's check; a mod or a later find can add rows), and the scenery written as the map's render-only backdrop
  package (`backdrop.write_model`, factored out of the district backdrops). The disc's 48 sky files and the three
  district backdrops stay byte-identical (the Industrial proxy file differs between two runs of the unchanged code:
  a separate, older nondeterminism). Verified on Hawaiian Dream: blue sky with the mountains behind the park
  (SKATE_BACKDROP triangles=2161 materials=5, all families supported). Open: retail's draw order for the scenery
  (b428). Tests `test_sky`.
- Map names (`map_names.py`, wired in `install.py`): every map in `maps.json` gets `display` (its Skate 3 name) and
  `name_id`; the engine's lists show `display` (`map_library.rs`). Rule (ours, no retail screen lists maps by
  stream): the `map_category` row keyed like the world row, else the world row's own name, resolved in the disc's
  English table and the packs' own; text with font glyphs or none falls back to the label in words. Per the retail
  data: MegaPark = "Stadium", StartPark = "PCU Training", IndustrialSkatePark = "Team HQ", AGPark = "The Art
  Gallery", SanitariumDLC = "The Sanatorium" (the DLC category rows drop the world row's `dist_`: `sanitariumdlc`;
  the user's video of Skate 3's challenge map with the packs shows "The Sanatorium"). Test `test_map_names`.
- Audio (`audio_export.py`): the DLC rows join the disc's, the packs' `.ems` (7 files; Maloof's places the 7 disc loop
  banks b414 listed) and every DLC district's region layers keyed by label; the disc's 23 `.ems` are unchanged.
  DLC beds and banks (research b432; user 2026-10-21 asked for them first): the bed table is built from data as retail
  builds it (824D4630: `ambienceresident.big`'s index, bed number = the stream name's leading number, the first entry
  per number wins, which keeps `19_reclaimed_b_fix` and adds the index's `23_Press_start_screen`; 824D3990: a pack's
  loose `data/audio/<N>_dlc_ambience.snr` / `.sns` for a number the disc lacks, at most 16, never replacing a disc
  bed) in `audio_export.bed_table`; zones resolve through it and keep `bed_index`. After Dark's zone plays
  `24_dlc_ambience`, Downtown Night's `25_dlc_ambience`, Back Lot's the disc's `06_dt_open` (b418 had Back Lot wrong).
  After Dark's 7 loose `.abk` banks (its random set `e_dlc_sanitariumdlc`) decode with the disc's, a disc bank keeping
  its name, and reach the AEMS runtime (`aems_files` extra). Tests `test_audio_beds`.
  A pack's `.ems` (`DLC_sanitarium`, `DLC_Maloof`, `dlc_zenplaza`, ...) is its map's own emitter file, the disc's
  `sfx_<district>` counterpart, so its banks now count as placed: After Dark's `cage_rattle_1`, `human_moaning_1` and
  `electric_shock_1` plus 14 disc banks only the packs' emitters use (air units, birds, frogs, rivers, a club, music)
  decode; before, those emitters had no bank and stayed silent (ours: the disc rule is a file-name prefix).
- Map pictures (b417, `prepare_menu_movies.py` `dlc_world_maps`): each DLC world row's map name (`tMapInfo`) is read
  against its own vault `.bin` and the packs' BIGs holding `data/fe/source/images/map/` join the dynamic image export,
  so `world_maps` gets `dist_maloofmoneycupdlc` -> `DLC_Maloof_Map_512` and that picture is extracted. Other DLC
  worlds keep their retail names on disc pictures (`dist_sanitariumdlc` -> `map_afterdark`, `dist_artgallerydlc` ->
  `map_partypack`, `dlc_dw_megacompound` -> `map_danny`). The Challenge Map (`skate-data` `location_maps`) falls back
  to a location's World field when its District names no world (Maloof's names the `map_category` row): NOT RETAIL
  YET, which field retail reads there is unverified. Test
  `a_dlc_location_whose_district_is_no_world_takes_its_world_field`; verified in the dev install after a `hud`
  refresh (2026-10-21). Not play-tested.

## Challenge Map Locations: the DLC folders and skate.Park (research b447, 2026-10-21)

- Retail [data]: each folder is a `map_listing` row under `progression_locations`, sorted by its order field; each
  pack's vault adds its own (7 Maloof Money Cup 2010 NYC, 8 Danny Way's Hawaiian Dream, 9 The Sanatorium, 10 Art
  Gallery: the user's retail video order). A location folder's filter tests the `fe_locations` row's World, not its
  District (the disc Maloof filter names `dist_maloofmoneycup`, its rows' World; their District is `dist_maloof`; the
  Art Gallery row has no District). skate.Park's filter (`skatepark`) lists `world` rows with `IsSkatePark` and
  `AllowInSkateParksTab`: 5 disc park worlds plus Back Lot, Downtown Night and Zen Plaza Full with the packs = the 8 of
  the video, labelled by the world's own text.
- Ours (`skate-data` `challenge_map.rs`): the rules match on World (inherited through the row's parents) and include the
  four DLC folders; the skate.Park folder lists the park worlds as `World` leaves; `retail_menus.rs` gives each park
  world the teleport of the destination that names that world (`megapark_full` for `dist_megapark_full`, which starts
  at the layout's start, doc 39). Tests: `the_locations_tab_lists_each_districts_locations_in_its_folder` (by World),
  `the_dlc_folders_list_by_world_and_the_skate_park_folder_lists_park_worlds`, the real-data tree (every DLC folder
  filled, skate.Park lists World leaves).
- DLC park worlds (2026-10-21): the teleport data (`teleports.py`) now reads the packs' rows too (each against its own
  vault's `.bin`, the packs' locators and language, park layouts from the pack BIGs; `asset_exports.py` passes the
  vaults), so the packs' fe rows become destinations. The three DLC Full worlds (`dist_backlotpark_full`,
  `dist_downtownparknight_full`, `dist_zenplazapark_full`) are entered through their packs' rows `dist_backlotpark`,
  `dist_downtownparknight`, `dist_zenplazapark` (World = the Full world; with artwork, so they keep their locator, the
  same rule as `startpark_progression`). Before this the three stayed greyed. Checked on the dev install
  (2026-10-21 refresh): 66 destinations, all with a matrix, incl. the 3 DLC park worlds, their Empty rows and the
  4 DLC maps.
- Unlock rule (research b450) [code]: `Unlock.IsLocationUnlocked` (`82839C38`) lists a row when its bool
  `Hash_90E678B6961730E1` (inherited) is set, otherwise when the unlock manager (`*(0x83067074)`, `82503AE0`) holds
  `all_locations_unlock` or the row's name text (`Hash_942AB8AEE4B414ED`), or its unlock-all byte (+0x24) is set.
  [data] The flag is 0 on `gamestart` (it overrides its parent `dist_university_library`), `dist_skateschool`, both
  Maloof rows, `dist_blackboxpark` and the park rows; 1 on the others. Ours: `LocationUnlocks` (`challenge_map.rs`)
  over `load`; NOT RETAIL YET: no unlock store, so every name counts as unlocked except `gamestart` and
  `dist_skateschool`, the start rows progression never unlocks. Result: University 10, skate.School 1, as in the
  user's retail video (Maloof and Black Box Park show there, so that save holds their unlocks). Test
  `a_location_without_the_shown_flag_needs_its_unlock`, the real-data tree (University 10, skate.School 1).
- Order inside a folder (research b451) [code]: the folder build (`82D0D728` -> `82D0C890` -> `82D0C7A8`) sorts each
  list with `82647EA0` (std::sort, not stable) and comparator `82647B80`: the category order first (the row's
  `Hash_50D9293B818507EC` map_category, its Int32 `Hash_1349BB6EC1D1E751`; skate.Park: Downtown 100, PCU Training 200,
  Team HQ 300, Stadium 400, Back Lot 600, Downtown Night 700, Zen Plaza 20000), then items in a runtime list (+0x60)
  first, then the label's text compared by position in the language's sort string (`8290F0A8`: a character not in it
  after all listed ones, a prefix compares equal). Ours: `load` sorts each folder's leaves by category order and
  label text (`collate`, `COLLATION`: our case-interleaved letters then digits), with the English names teleports.json
  carries per label (`label_key`); ties keep data order. Test `a_folder_sorts_by_category_order_then_label_text`.
  NOT RETAIL YET: the language's own sort string.
- Category headers (research b454) [code + movie data]: after the sort retail inserts a map_category row before each
  category run; the screen draws it as a picker header (`UpdateStuffMenuItems` -> `AddSpecificItem(label,
  mHeaderItem)`, list [3] of `GetStuffMenuItems` `8264AF28`), labelled by the category's `Hash_465698DD3CC73CA2`
  (`82644430`), not greyed (`82646D18`), skipped by the cursor (`82D0D100`, the movie's picker) and by A (`826475B8`),
  and not counted; normal rows indent 16 px when a list has one. A folder whose inherited `Hash_EAFE7F250C154184` is
  set and that holds one category shows no header (University, Downtown and the other location folders). The
  comparator's middle key is cProgressionManager's challenge status "new" list (`*(*(0x830CFD94)+0xE0)+0x44`, +0x60):
  new challenges first; it never matches a location or park key. Ours: `load` inserts the header nodes
  (EntryKind::MapCategory; the screen host already sends list [3] and skips them), `select` ignores a header, rows
  never grey a header. Result on the dev install: skate.Park = 15 rows, 8 parks under 7 headers (Zen Plaza's
  category is the San Van pack's, labelled `ID_WORLD_ARTGALLERYDLC` in its data). Tests:
  `a_folder_sorts_by_category_order_then_label_text` (headers, the flag with one and two categories), the real tree.
  Open: retail's header label for leaves without a category; who fills the "new" list. Not play-tested; b447 noted
  the video's skate.Park count as "(8)", so the user's check decides whether the headers show.
- Unlock grants (research b458) [code + data]: the manager's list A (mgr+0x28) is the profile save (`82505150` /
  `82505000`); a fresh save (`82503AD0` -> `825046B0`) holds 7 characters and 37 gestures, no locations. List B is
  rebuilt from the content packages' `data\unlocks\*.unlock` lines (`KIND,name`, `82505220`) and never saved; the
  location test asks both. In the user's packages (main checked): `presell.unlock` (Black Box Distribution Skate
  Park) `LOCATIONS,dist_blackboxpark`, `unlckall_attrib.unlock` (Time is Money) `LOCATIONS,dist_maloof_street` /
  `dist_maloof_vert`: that is why both show in the user's video. `all_locations_unlock` is only cheat id 2, which no
  TU3 cheat code has. The hash is FNV-1 everywhere (`82503BD8` multiplies, then xors; recomp checked), not FNV-1a.
  Ours: setup writes `private/dlc-unlocks.json` (every package's unlock lines, `dlc.py` `unlocks`, read from the
  package without copying its BIGs; `BigArchive.decode`); `LocationUnlocks::load` takes its LOCATIONS names as
  grants. Until the profile store exists everything else stays unlocked except `gamestart` / `dist_skateschool` (the
  user's 2026-10-19 rule for the unlock hooks). Test `a_location_without_the_shown_flag_needs_its_unlock`.
- Open: the profile unlock store (list A, per profile) and a mod API to grant / lock locations; retail's Lua /
  challenge rewards that grant locations.

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
  "Enter Stadium skate.Park" in the retail font with the logo glyph.
- Arrival (research b455, 2026-10-21) [code; user fact 2026-10-21: "It did, i can confirm I landed from the teleport
  on my board"]: the skater arrives on the board and at rest. The teleporter path `82D55280` passes IsOnBoard = 1
  (`li r6,1` into the vt+36 call, recomp checked) through `828646E0` -> `82864038` -> skater vt+0x70, and the arrival
  state is 100 (on board); the player teleport `82DB8998` -> `82C0D680` zeroes every body's linear and angular
  velocity and nothing sets one afterwards. The Challenge Map menu passes IsOnBoard 0 (arrives on foot). Ours:
  `teleporters.rs` `on_board = true` (was the skater's own board state, a b421 misread); `travel_ex` already gets no
  velocity. Far teleport (research b457) [code]: the flow handler `82709740` case 3 stores the request
  (`82706F50`: matrix, slot, IsOnBoard), the load chain `82707BE0` -> `82707CB0` -> `82707D70` -> `82707F08` ->
  `827080A0` streams the exit area, then `827070C0` posts cMsgTeleport (0x8FCDC5E2 {matrix, IsOnBoard,
  IsSessionMarkerTeleport}), the same event as a same-world teleport; no velocity anywhere on the path. Ours already
  matches (`challenge_map.rs` `travel_ex(request.transform, None, request.on_board)` after the map load). Open: who
  turns cMsgTeleport into the skater move (likely retail Lua); whether our placement can run before the exit area's
  collision exists; whether our `travel_ex` resets twice (`skater.rs:219-228`).
  Moddable next: per exit `on_board` / arrival velocity on `sdk.triggers.teleporter` (`TeleporterOptions`, vm.rs:77).
- D-pad Up glyph (research b452, 2026-10-21): the Teleport state sets no button type; the movie's own data does.
  [data] `mSignup.mButtonItem` (a `button_item2`) has a Construct clip action setting `mButtonImage = "DPad_Up"`, and
  `button_item2`'s constructor calls `SetButtonType(mButtonImage)`, which writes its child `mButtonRender._test`; that
  child's own Construct sets `_test = "Unknown"`. [code] Retail runs a placed clip's initialize / construct
  (`82E7AF18`), then `82E5A9E0`: the prototype, the first tick `82E5B640` (which places the children, so their
  construct runs), then the class constructor (`82E6A208`), so the constructor's `DPad_Up` wins. Ours queued the
  constructor before placing the children, so the child's "Unknown" won and no glyph drew. Fix: `apt_movie.rs` queues
  the constructor after the first-frame `seek` (it still runs before the clip's own frame actions). This applies to
  every placed registered class, so other buttons whose constructor sets a glyph now show it too (the pause menu's
  screen test now draws a `prompt/X`: the DLC banner's button, which retail sets to X too; research b467 + main check:
  it is drawn at y -34..-2, above the stage, until InitializeMenu hides the banner, so it is never on screen, as in
  retail's video; the test asserts that and its texture check skips the engine's `prompt/` glyphs). Test
  `the_teleport_prompt_shows_its_dpad_up_glyph` (state 2 draws `prompt/DPad_Up`); apt_stage tests 15 pass. Not
  play-tested. Open: retail runs the clip's own load before the constructor, ours after; `DAT_83037550` (skips the
  first tick). Moddable next: an optional `button` on `sdk.triggers.teleporter` through the movie's `SetButtonType`.
- NOT RETAIL YET: the arrival speed is the same reset as the session marker's (b423: whether retail zeroes speed is
  open).

Verification (2026-10-20, verify switch `SKATE_VERIFY_TELEPORTER=<id>` with `--verify`: starts the skater in that
teleporter's volume and presses once, no popup): University `tele_world_to_stadium` armed 4 teleporters, loaded
MegaPark and landed at (67.2, 0.03, -134.3) on its exit locator (67.3, 0.2, -134.5), riding, with MegaPark's own
"Leave Stadium skate.Park" prompt up; Hawaiian Dream `teleport_dwmc_01` landed at (-8.0, 39.4, 64.3) on the Fun Track
(exit (-8.0, 40.3, 64.6)). The confirm popup and the D-pad Up press itself are not play-tested yet. Tests: setup 251,
skate-game 862, skate-mods 106, skate-data pass.

## Importing DLC: the start-up check sees new packs (2026-10-21, laptop session)

### Problem
On the laptop (hails-additions aba07209), Tools > Import DLC copied the 8 packs and Tools > Refresh game files right
after said "GAME_FILES current: nothing to refresh": nothing was converted. Only a manual setup run with
`SKATE3_DLC_DIR` set converted them, and that run removed the installation the launcher's own copy linked to.

### Root cause
The game's check (`asset_check.rs`), which both Refresh game files and Extras > Refresh Game Files go through, never
looked at the DLC folder. Setup knew (the DLC identity joins the hud, maps, environment and audio versions), but it
only runs when the check already found something out of date.

### Change
- Setup records what the DLC folder held: `installation.json` `dlc_files` (every file as `relative/path|size`,
  `tools/asset_pipeline/dlc.py` `listing`).
- The game's check resolves the DLC folder by setup's rules (`SKATE3_DLC_DIR`, the folder setup last used, a `dlc`
  folder next to the game folder setup read, and new: a `dlc` folder next to this game copy) and compares its listing
  with `dlc_files`. A pack added or removed reports the hud group (`ASSET_CHECK hud: DLC packages changed since
  setup`), so Refresh game files and the in-game Extras > Refresh Game Files run setup, which rebuilds every group
  whose DLC identity changed. An installation from before the listing that already read a DLC folder stays current;
  one that never read one is reported once a folder has files.
- Release ZIP: put the packages in a `dlc` folder next to `skate3rust.exe`; the next start offers the refresh. No
  variable needed.
- Launcher (hails-additions): Import DLC converts the new packs at once (it runs Refresh game files itself), and
  `launcher.ps1 tool-dlc -Path <folder or file>` imports without the dialogs (scripts, the second PC). Files that are
  not Skate 3 DLC packages, such as a title update, are skipped and named as such.

### Verification
skate-game `dlc_packs_added_after_setup_offer_the_refresh` (added, unchanged, removed, older installs, a `dlc` folder
next to the game) and the other asset check tests (6); setup `test_dlc` (7, new
`test_a_dlc_folder_next_to_the_game_copy_and_its_listing`). The launcher's non-interactive import was run on a temp
folder with one real pack and one other file (1 added and 1 skipped, then 1 already there; a missing path refused).
Not yet run end to end: the launcher's import followed by its automatic refresh on a fresh copy.

## DLC location names in the menus (2026-10-21, laptop session)

- **Problem:** the Challenge Map's Locations list showed `ID_WORLD_MALOOFMONEYCUP_NYC`, `ID_WORLD_DW_MEGACOMPOUND`,
  `ID_LOCATION_SANITARIUMDLC` and `ID_LOCATION_ARTGALLERYDLC` instead of the names (the user: "the DLC map names are
  not correct and are usimg their internal names").
- **Cause [code]:** the menu export (`tools/prepare_menu_movies.py`) added DLC text only from the packs whose menu
  movies it exports (the Create pack, `dlc_archives`); the map packs carry no menu movie, so their text tables never
  reached the menus' language. The map lists read them by another path (`map_names.py`), so MAPS was right.
- **Change:** `dlc_text_archives` reads the text of every pack with front-end language tables; movies still come only
  from the exported packs, and a disc label keeps its text. The menu movie set's version goes from 11 to 12
  (`tools/prepare_menu_movies.py` and `retail_menu_movies::VERSION`), so the asset check reports the hud group as
  out of date and Refresh Game Files rebuilds it.
- **Why the version bump [laptop, 2026-10-21 18:45]:** without it the laptop's copy at f21c445b reported
  "ASSET_CHECK done in 228 ms: 0 problem(s)" and "GAME_FILES current: nothing to refresh". The script is in the hud
  group's fingerprint (`tools/asset_pipeline/versions.py`), but Refresh Game Files (`setup.rs`
  `update_game_files_now`) decides only from the asset check, which reads the manifest version, not the
  fingerprints. Open question: whether a script-only converter change should reach Refresh Game Files by
  fingerprint as well.
- **Verification [data]:** the user's 8 packs give 786 labels with all four names ("Maloof Money Cup 2010 NYC",
  "Danny Way's Hawaiian Dream", "The Sanatorium", "Art Gallery"); test
  `test_a_map_pack_names_its_location_in_the_menu_text` (data-gated on the Maloof pack). Not play-tested.

## One move per teleport (2026-10-21, research b471 / b473)

- **Problem [code]:** every teleport (teleporters, Challenge Map, teleport menu, debug camera, mods, the session marker
  return) moved the skater TWICE: `travel_ex` queued the host teleport (reset on the next tick) and the actor reply
  (702, reset the tick after). `placements()` rose by 2; a mod's arrival velocity was zeroed by the second reset.
- **Retail [code, b471]:** one move, the actor reply `825926F8` -> `82D431F0` -> `82D43280` -> `82DB88C8` ->
  `82DB8998`; the marker return is the same single reply (b473).
- **Change:** `travel_ex` asks the reply only (`request_manual`) and refuses while a teleport is in flight
  (`SkaterRuntime::teleport_pending`: a host request, a reply not yet published, or a 702 reset not yet applied). A
  mod's velocity (ours) is applied after the single reset (`set_arrival_velocity`). The session marker return,
  climbing's drop check and the mod attachment dismount use the same predicate and single move.
- **Tests:** `travel_is_one_placement_and_refuses_a_second_request`, `travel_keeps_the_mods_arrival_velocity`
  (asset-gated, pass on the dev assets); regression: boundary, air timeout, checkpoint reply pass;
  `marker_reply_restores_on_foot` fails as before (pre-existing, gotchas). Not play-tested.

## Teleport arrivals fit onto the ground (2026-10-21, research b475)

- **Retail [code, b475]:** `82598428` casts one straight ray down from 0.75 m above the arrival point, 500 m long; on a
  hit only the translation becomes hit + 0.15 m (rotation untouched, a miss keeps the matrix). The reset then adds its
  0.1 m, so a teleport lands at ground + 0.25 m at rest whatever height the locator had. Ours had no fit-up: a
  locator h above the ground dropped the skater h (b476: 45 arrivals sit too high).
- **Change:** `skate_core::physics::teleport_ground_fit` (`GroundFit` with retail's values as defaults, pure
  function). `travel_ex` marks its move; the single reset fits the target against the static world
  (`BoardWorld::query_thin_line`). Checkpoint replies, NPC placements and the session marker are unchanged (retail's
  fit-up is in the teleport message only; whether the marker return also goes through it is open, b473). Mods:
  `sdk.player.teleport{..., ground_fit = false}` places exactly (default true), carried on the multiplayer teleport
  wire. Open: retail's query mask 7 (whether props count; ours: static world only).
- **Tests:** `fit_arrival_to_ground` F1..F7 (skate-core), `travel_fits_the_arrival_onto_the_ground` (asset-gated: a
  target 3 m up lands near the ground, no fall). Likely fixes most of b476's high arrivals: not play-tested.
- **Automated check (2026-10-21 night):** `SKATE_VERIFY_TELEPORTER=teleport_dwmc_01` on DLC_DW_MegaCompund: the exit
  asked y 40.30 and landed at 39.52 (b476's ground 39.372 + 0.15), riding on the road in the capture. Log line
  `TELEPORT_ARRIVAL asked=[x, y, z] fitted_y=... ground_fit=...` per fitted arrival.
- **skate.School "Back to the top" exits (tele_sksc_01..04) [recomp, 2026-10-21]:** retail sits under the world there
  too. A scripted recomp run teleported to each exit 3 m above its locator (`SKATE3_TP_OVERRIDE` on the skate.School
  Locations entry, row 3) and the skater fell through empty space to y -1000 at all four (screenshots). So our data
  and collision match retail; the exits only make sense while their challenge runs (their volumes sit at the same
  depth, unreachable in free skate). Nothing to fix.
