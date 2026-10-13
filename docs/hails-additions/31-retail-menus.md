# 31: Retail menus: menu data, hook registry, mod API (milestone 1) and settings value model (milestone 2)

Branch: `ui/retail-menus` (from `main` b3c96793). Own upstream PR when ready (not part of the audio or living-world
PRs). This milestone adds data, hooks and the mod API only; nothing is drawn yet and the current overlay menus
(`graphics_menu.rs`, `modding/menu.rs`) are unchanged.

## Problem

The engine's pause and settings menus are hand-built (`graphics_menu.rs` sections MAPS / SKATER / GRAPHICS /
MULTIPLAYER / EXTRAS). They do not look or behave like Skate 3's, and they have no structure that features or mods
can plug into: every new feature edits menu code. A real accident showed the cost: the FPS limit was changed by an
unintended press in the overlay. The goal of the menus work is retail menus (layout, navigation, input handling,
sounds) built on the retail data, moddable from the start. Milestone 1 is the foundation: the retail menu
structure as data, a registry with hooks for every entry, and the mod API over it.

## What retail does

Reference: the TU3 build through skate3recomp (by @mchughalex, built on the rexglue SDK) read statically (no game
run), the front-end movies in `data/big/fedata.big` read with upstream's APT tools (`tools/vendor/skate3_ui`, from
chazmm's APT/HUD port 1c768a3e). Addresses are TU3 and only used to verify the extractor; no game code or data is
copied. Full notes: `.claude/notes/fe-menus-re.md` (local).

**The movies ask the game for data.** The pause menu (`screens/main/core_menu.apt`, the "crossbar") and the
settings screen (`screens/options/options.apt`) are AS2 movies; they read the menu contents through host objects
(`CoreMenu`: GetNumMenus / GetOptionName / IsItemEnabled ...) and natives (`GameSettings_GetNumOptions`,
`OnMenuSelect`, `OnMenuGetItemEnabled`, ...; registrar `825A3070`). The contents are C++ tables:

- **Game Settings rows** `0x83026EC8`: 36 x (string id, widget type `option` / `slider` / `selector`); rows 0 and 23
  are blank. **Screens** `0x83026FE8`, right after: 13 x 16 row indices, 0-terminated (1 = main: Difficulty, Audio,
  Video, Controls, Online, Skate Feed; 3 = Audio: SFX / Dialog / Music volume, SFX pack; 4..8 = Video variants; 9
  Controls; 10 Online; 11 Skate Feed; 12 Audio without the SFX pack, unused).
- **Crossbar categories** `0x8302751C`: 12-byte records (name, label id, icon): Multiplayer, Options, SinglePlayer,
  Create, Learn. **Items** `0x83027558`: 20-byte records (name, label id, icon label, sub-option kind or -1, helper
  text id), 49 items from ReplayEditor (id 0) to ExitPartyPlay (id 48). Item ids are positions in this table.
- **Apt key table** `0x83027BF0`: 56 x 20-byte records (input action, name, Apt key code, released flag, kind):
  AptStart 0x12D, AptNext 0x12E, AptPrevious 0x12F, AptUp / AptDown / AptLeft / AptRight 0xE / 0xF / 1 / 2 and the
  `*Released` twins.
- **Per-mode menus** are static data reached through the crossbar mode classes. Each class's vtable (e.g. Career
  `0x82303D54`) holds, in order: destructor, a tab accessor `GetMenu(i)` (`lis; rlwinm r4,2; addi; lwzx; blr`, e.g.
  `825E8368` -> `0x822059E4`), a row accessor `GetItem(tab, slot)` (`825E8380`, rows `0x82205A68`, stride 8 via
  `rlwinm r4,3`; the park variants use `r4*8 + r4` = 9 slots), the item-kind getter, the tab count (`li r3,5`), the
  row counter (skips Project10 until unlocked, `8261F950`), IsItemEnabled, a constant, the mode title getter
  (`8261F940` -> `ID_CROSSBAR_SINGLE_PLAYER_MODE_TITLE`) and the select hook. Nine classes have static tables:
  Career, Career in a park, Free Play, Free Play in a park, a restricted free-skate menu (Main = ChallengeMap only),
  the Skate challenge, two lobby modes and the Party Play menu. Career Main = ChallengeMap, MyCareerTeam,
  SkateProfileOffline, SkateWith, SkateFeed, SoloFreeskate, PartyPlay (matches the retail screenshot row for row).
- **Rules** (from the code): the settings row count stops at the SFX pack row with <= 1 SFX pack
  (`GameSettings_GetNumOptions` 825A6F70) and IsItemEnabled (`8260D648`) greys it, greys Difficulty in online modes
  (GameMode 4 / 5) and Skate Feed settings when unavailable (`8245A2D0`). In a park (`8261FB18`) SkateReel and
  SkateWith are greyed, SavePark needs a park-state flag, RatePark / FlagSkatePark only in a community park. The park
  select hook (`8261F9D8`) asks `ID_SKATEPARK_DIRTY_TITLE` / `_DESC` (Yes / No) before ChallengeMap /
  OnlineChallengeMap leave a dirty park. Settings select (`8260D290`) links rows to screens, Video by front-end
  state, mode and session (screen 8 / 7 / 6 / 5 / 4). Retail's own greyed look is the row state `disabled` in
  core_menu's row sprite.

## Change

### 1. Extraction at setup (`skate_data::menu_tables`)
`skate3rust --extract-menu-tables <default.xex> <menu-tables.json>` (like `--extract-ocean-pca`) unpacks the user's
own executable with `XexImage` and writes `assets/private/menu-tables.json`; setup runs it in the environment stage
(`tools/asset_pipeline/menu_tables.py`, `asset_exports.py`; failures become an optional-content note). Every table is
found by content or code pattern, never by address, so any build works:

- settings rows: the pointer to the string `ID_GAMESETTINGS_SFXVOLUME` whose neighbour points to `slider`; the run of
  (string, widget) pairs around it; the screens follow while every block holds valid row indices;
- crossbar items: the pointer to `ReplayEditor` in a record of the item shape (three strings, a small kind, a
  string); categories are the 12-byte `ID_CROSSBAR_*` records right before;
- Apt keys: the pointer to `AptStart` in a record of the key shape;
- per-mode menus: every pair of adjacent vtable slots where a small evaluator (lis / addi / ori / rlwinm / add /
  mulli / lwzx / lwz / blr) shows the first function reads `table[r4]` and the second `table[r4 * n + r5]`; the
  following slots give the tab count (`li r3,n; blr`) and the title (a two-instruction getter returning a string).
  Item and category ids become names. Mode keys: `Career` / `CareerPark` (single-player title, 8 / 9 slots),
  `FreePlay` / `FreePlayPark` (offline free-skate title; FreePlay has ResumeCareer), `FreeSkateRestricted` (same
  title, no ResumeCareer), others by title (`SKATE_CHALLENGE`, `TEAM_VS_TEAM_LOBBY`, `RANKED_LOBBY`, `PartyPlay`).
  Retail has no class names for these menus, so the keys are ours, derived from the retail title and slot count.

The JSON (`schema` 1) also keeps the source image hash and the table addresses for diagnostics.

**The disc build differs from TU3**, which is why nothing is fixed by address or index: the user's disc
`default.xex` has 47 crossbar items (TU3 added PartyPlay and ExitPartyPlay), 35 settings rows (no
`ID_GAMESETTINGS_SKATEFEED_STATUS_MESSAGE`, so the Skate Feed settings row is index 34, not 35), its categories sit
right after the items instead of before them, and every table is elsewhere (settings rows `0x82FCB168`, items
`0x82FCB7A8`, categories `0x82FCBB54`, keys `0x82FCBE50`, Career vtable `0x822FE764`). Its per-mode menus are the
TU3 ones without the Party Play entries and without the Party Play menu. The registry keys settings rows by string
id, so screens stay correct in both builds.

### 2. Registry (`skate_core::menus`, resource `retail_menus::RetailMenus`)
Engine-independent registry over `MenuData` (categories, items, settings rows, screens, modes), built from the JSON
(`MenuTables::to_menu_data`; settings rows keyed by string id, blank rows `GAMESETTINGS_ROW_<n>`). Keys are the retail
internal names (`GameSettings`, `SkateFeed`, `SinglePlayer`, `ID_GAMESETTINGS_SFXVOLUME`).

- Hooks per entry: `visible` and `enabled` (data `Rule`s over published facts and the mode, or native predicates),
  `on_select` / `on_highlight` handlers, select interceptors (confirmation popups), value get/set bindings. Every
  hook list is a stack by owner (`Retail`, `Engine(name)`, `Mod(id)`); the last registered wins and removing an
  owner restores the one below.
- **Greyed until handled:** an entry is enabled only when a handler (or value binding) exists and its enabled rule
  holds; a settings link is enabled when its target screen has an enabled row. So every retail item exists from the
  start, greyed, and a feature enables it by registering a handler, with no menu code changes. Online / Xbox LIVE
  items stay greyed placeholders until the custom online feature registers.
- Retail rules registered as `Owner::Retail` data: SFX pack hidden and greyed at <= 1 pack (fact `sfx_packs`),
  Project10 hidden until `project10_unlocked`, Difficulty greyed when `game_mode` is 4 / 5, Skate Feed settings by
  `skate_feed_available`, park rules (SkateReel / SkateWith off in `CareerPark` / `FreePlayPark`, SavePark by
  `park_saveable`, RatePark / FlagSkatePark by `park_community`), the dirty-park confirmation on ChallengeMap /
  OnlineChallengeMap, and the settings links incl. the Video screen choice (`retail_link`).
- Actions are values with stable ids (`MenuAction::Select / Confirm / Highlight / SetValue`, entry kind + id, plus the
  mode): `dispatch` returns what the host does next (`Confirm(popup)`, `Handled`, `Forward` to a mod,
  `OpenScreen`, `Ignored`, `Declined`), so a menu UI or a remote peer can replay the same actions (multiplayer-ready,
  no networking).
- Structural edits (`Edit::Category / Item / Setting / Hide`) are owner-tagged layers over the retail base: add,
  hide (per mode), reorder, relabel, change icon. Removing an owner rebuilds the structure from the base plus the
  remaining layers, so disabling a mod reverts exactly its changes.

### 3. Mod API (`sdk.menus`, capability `retail_menus` = 1)
`sdk.menus.category / item / setting(id, options)` (add or change: label, retail `icon` label or mod `image`,
help, tab `category`, `modes`, `position`, `widget`, `screen`), `hide(kind, id, modes)`, `handle(kind, id, {select,
highlight, value, enabled, visible, confirm})`, `unhandle`, `set_value`, `value`, `info`. New entries must be named
`<mod id>.<name>`; 64 entries per mod. Handled entries send `on_event {name = "menu_select" | "menu_highlight" |
"menu_value", entry, id, mode, value}`. Rules from Lua: `true` / `false` or `{fact = "sfx_packs", gt = 1}`,
`{modes = {...}}`, `{all = {...}}`, `{any = {...}}`, `{["not"] = ...}`. `sdk.snapshot.menus` holds the mode, whether
the retail tables loaded and every bound value. Everything a mod did is removed when it stops, fails or reloads.
Mod images are a designed field only; drawing them comes with the menus (milestone 5).

## Milestone 2: settings value model (2026-10-08)

### Problem
Milestone 1 had value get/set hooks but no rule for how a row's value changes, and no engine setting was bound, so
every value row was greyed.

### What retail does
All from the TU3 settings input handler `sub_8260C258` (events 14 = Left, 15 = Right), GetIntegerValue
`sub_8260D7E8` and GetStringValue `sub_8260D968`; values live in the settings object `[[0x83067060] + 28]`.
- [code] Volume sliders (rows SFX / Dialog / Music, offsets +128 / +132 / +136, f32):
  `v = clamp(v + dir * 0.1, 0.0, 1.0)` with `fmadds` (step 0.1 at `0x820641A8`, min 0.0 at `0x82165A10`, max 1.0 at
  `0x8231A844`, Left -1.0 at `0x8216DEE0`, Right uses the 1.0 at `0x8231A844`), then `v < 0.05` (`0x82165A00`) -> 0.0.
  The slider shows `floor(v * 10.0 + 0.5)` bars (10.0 at `0x821963E4`, 0.5 at `0x8209975C`).
- [code] Selectors toggle `x = (x == 0)` on either direction; Left and Right only differ in their UI sound. Labels
  (sub_8260D968, strings at `0x82203990 - 668 ..`): Camera Angle +188 0 = `ID_GAMESETTINGS_CAMERA_LOW`, 1 = `_HIGH`
  (other values `#ERROR!!`); offboard Y / X axis +192 / +193 `_AXIS_NORMAL` / `_AXIS_INVERTED`; Subtitles +157,
  Minimap +158, HUD +164 (applied by `sub_824AD2F0` / `sub_825DA2A0`), Camera manual +165, Vibration +163
  (`sub_8260DF40`, plays a test rumble when switched on), Transparency +195, Skate Feed +196..+200:
  `ID_COMMON_OFF` / `_ON`; Units +168 `_UNITS_METRIC` / `_IMPERIAL` (also mirrored to `[0x830CFE34] + 4`).
- [code] Play Mode (+180, `sub_8260DE60`): `v + dir`, above 2 -> 0, below 0 -> 2; applied at once by
  `sub_827D8450` unless `sub_82743FC0` (online session). Labels `sub_825E7648`: 0..4 = `ID_GAMESETTINGS_DIFFICULTY_`
  `EASY` / `NORMAL` / `HARDCORE` / `MOTORIZED` / `TEST`, nothing above 4.
- [code] HOM mode (+184): `v + dir`, 2 or more -> 0, below 0 -> 1 (two values, `ID_COMMON_OFF` / `_ON`).
- [code] Not value rules: SFX pack (`sub_82486870` picks the next pack id), Auto sign-in (`sub_8260E068`, a popup);
  VSync and Sixaxis have no case (Left / Right do nothing). Every change refreshes all rows (`vtbl+20(item, 50)`).
- [code] Volume routing: the MixMap Master controller's step `sub_824D5160` (runs every audio pass, so a change is
  heard on the next pass) reads the three floats from the settings object and writes its own inputs through
  `[controller + 12]->vtbl+8(input, value)` with `value = trunc(v * 32767.0)` (`fmuls` by 32767.0 at `0x821747FC`,
  `fctiwz`), clamped to 0..32767: Master.in1 = Music (+136), in2 = SFX (+128), in3 = Dialog (+132). The MixMap data
  then scales the buses (mixmap-spec section 6.5: in1 -> Music, in2 -> nearly every SFX sum, in3 -> Announcer, NIS
  and CameraMan speech). Ambience and emitters hang on Master.in4, which is not an option volume (same function, from
  a game object's float, 32767 in free skate). Music +136 is also a gate elsewhere: `sub_8249A7E8` and
  `sub_824CF350` only start / advance a music track when it is above 0.0, `sub_8249C078` tests it for 0.0;
  `sub_825AD1F8` writes +136 from a system call result (`sub_82E5F430`).

### Change
- `skate_core::menu_values` (new): `ValueRule` {Slider, Toggle, Cycle} with the arithmetic above (f32,
  `mul_add` like `fmadds`, deterministic), `Display` (bars or a label id), `Direction` (serialisable by name), and
  `retail_rule(id)`: the retail rules keyed by string id (disc and TU3 tables agree), so ranges and steps are data.
- Registry: retail rules installed as `Owner::Retail`; `set_rule` stacks an owner's rule over it (last wins);
  `MenuAction::Step { entry, direction }` turns Left / Right into a `SetValue` through the rule in force;
  `rule`, `display`, `owner_value`, `set_owner_value`. Removing an owner drops its rules too.
- Engine bindings (`retail_menus.rs`, `Owner::Engine`), which enable these rows: SFX / Dialog / Music volume <->
  `game_audio::RetailVolumes` (0..1 floats, default 1.0, saved in `settings/audio.json` as `retail_volumes`), which
  the native pass (`native::mixmap_frame`) writes to Master.in2 / in3 / in1 exactly as `sub_824D5160` (it wrote
  32767 to all four before); our own Master / Ambience / Effects percent settings are separate engine settings and
  no longer tied to a retail row. Camera Angle <-> `CameraAngleSettings.selected` (0 Low / 1 High, saved),
  Play Mode <-> difficulty (physics mode, `Config`, `settings/gameplay.json`). A sync system applies a row change to
  the engine and saves it, and takes changes made elsewhere (current menu, settings files) into the row without an
  event. Motorized and Custom stay settings-file values (shown as their index label; the retail rule steps back into
  Easy..Hardcore). Engine-only settings stay in the settings files (decision above).
- Mods: `sdk.menus.handle('setting', id, {value = ..., rule = {kind = 'slider' | 'toggle' | 'cycle', ...}})` binds a
  value and / or overrides the rule over the engine binding; `set_value` and Left / Right reach the mod layer
  (`menu_value` event); disable / reload / failure removes both and the engine value shows again.
- Mod `set_value` never writes the user's real setting (follow-up 2026-10-08). On a row whose value in force is not
  the mod's own (an engine binding, another mod's value) the value goes into the mod's own value layer on top of the
  owner stack, exactly as `handle('setting', id, {value = ...})` does (`MenuRegistry::value_owner` tells which layer
  is in force). Last writer wins: a mod writing again moves its layer back on top. While a mod layer is on top of an
  engine-bound row, the sync system applies its value at runtime without saving: SFX / Dialog / Music through
  `AudioSettings` runtime volume overrides on the Master inputs (`settings/audio.json` keeps the user's volumes),
  Camera Angle as the camera's forced angle (owner `menus`; the player's `selected` angle is untouched), Play Mode
  as the physics mode and `Config` difficulty without writing `gameplay.json`. When the layer goes (disable, reload,
  failure) the next layer down is applied again, ending with the user's own setting. A mod's `handle` value
  override on an engine row is applied the same way (before, it was only shown). On a row whose top layer is the
  mod's own, `set_value` acts like a menu change (`menu_value` event to the mod), as before.

### Verification
- `cargo test -p skate-core menu_values`: slider steps, clamp, snap below 0.05, exact zero from 0.1, bars rounding;
  toggles on both directions and any non-zero; Play Mode and HOM wrap, labels incl. out-of-range; undecoded rows have
  no rule.
- `cargo test -p skate-core menus`: Step through the retail rule on an engine binding (0.75 -> 0.85, Hardcore ->
  Easy), engine-side change without an event, mod rule + value override and exact revert to the retail rule and the
  engine value.
- `cargo test -p skate-mods menus`: rule specs parse and validate.
- `cargo test -p skate-game mod_set_value_layers_over_engine_rows_and_reverts`: a mod's `set_value` on the SFX volume
  and Camera Angle rows is shown and applied (Master.in2, forced camera angle) while the user's volume and camera
  choice stay; a second mod on top wins, the first writing again wins back; removing each mod reverts only its own
  layer; after both, the engine value is back exactly, `audio.json` is byte-identical and `camera.json` was never
  written.
- `cargo test -p skate-game retail_volumes volume_rows`: default 1.0 -> 32767 on in1..3; 0.9 / 0.5 / 0.0 -> 29490 /
  16383 / 0 (truncation), clamp and NaN, clamping on load; each row stepped Left reaches only its own Master input
  (SFX in2, Dialog in3, Music in1), is saved, and an engine-side change shows in the row.

### Open
- Subtitles, Minimap, HUD, offboard axes, Camera manual, Vibration, Units, Transparency, HOM
  mode, Skate Feed rows: rules ported, no engine setting yet, so they stay greyed until a feature binds them.
- Music volume as a gate (no track started at 0.0, `sub_8249A7E8` / `sub_824CF350`): needs the music player.
- Retail's default volumes (settings constructor, inside the profile save) not read; 1.0 matches the recomp's
  free-skate capture (Master.in1..3 = 32767).
- SFX pack (`sub_82486870`) and Auto sign-in (`sub_8260E068`) rules not decoded.
- Retail defaults of the settings object (its constructor) not read; our bound settings keep their own defaults.
  Update 2026-10-11: read [trace] instead: the recomp booted through the demo path without a signed-in profile,
  `SKATE3_WATCH` on `0x83067060 +28 @ +128` (hex64) and `+192` (hex16), files in `.local/recomp/defaults/`
  (the zeroing function `sub_82DE3F38` clears the object first; the values come from later code, not decoded).
  Values: volumes 1.0, Subtitles 0, Minimap 1, Vibration 1, HUD 1, Camera manual 0, Units 0, Play Mode 1, HOM 0,
  Camera Angle 0, axes 0 / 0, Transparency 0, the five Skate Feed toggles 1. `skate_core::menu_values::retail_default`
  holds them; `MenuRegistry::value` falls back to it when a row has no value layer, so rows without an engine
  setting show retail's value instead of "ERROR!!" (they stay greyed: a default is not a binding). Video Settings
  before: Subtitles / Minimap / Cameraman Mode / HUD read "ERROR!!"; after: Off / On / Off / On. Test
  `every_rule_row_has_a_retail_default_that_displays`. The recomp is not retail: the values are its boot state
  with no save loaded.
- The current overlay (`graphics_menu.rs`) keeps its own difficulty copy; it is replaced in milestone 5.
- While a mod holds Play Mode, a difficulty change saved from the current overlay saves that overlay's choice; the
  mod's value itself is never saved. A mod already holding the camera through `sdk.camera` keeps it (the menus
  hold is refused with a warning).

## Milestone 3a: APT VM opcodes for the menu movies (2026-10-08)

### Problem
The 16 retail front-end movies (core_menu, options, highlight_selector / option / slider, screenmanager,
screentransitionmanager, menu_picker, tabs, button_item2, menu_part_text_hilite, panel, scrollbar, dimmer, FE_root,
main3dhud) use 6 action opcodes our APT VM (`crates/skate-game/src/apt_vm.rs`, HUD-only until now) rejected with
"Unsupported APT opcode": 0x26, 0x30, 0x42, 0x43, 0x4A, 0xA5.

### What retail does
Retail dispatches actions through a table of 256 handler pointers at `0x82FC9BF0` (TU3 image; entry 0x8E =
DefineFunction2 `82E73010`, 0x9B = DefineFunction `82E72EC0`, which confirms the base). Handlers:
- **0x26 trace** `82E6D958`: pops one value, converts it to a string and prints `AptTrace: %s` (`0x82063488`) to the
  debug output.
- **0x30 random** `82E6DDD0`: n = ToInteger(top); result = rand() % n (unsigned `divwu`; `twllei` traps on n = 0);
  pops and pushes the integer. rand is `82E82528`, a Mersenne-Twister-style generator (tempering, refill `82E823A8`).
- **0x42 initArray** `82E6E9C0`: n = ToInteger(pop); creates an array; element i = the i-th value from the top;
  length n; pops n, pushes the array (n <= 0: empty).
- **0x43 initObject** `82E6EB00`: n = ToInteger(pop); creates an object; for i = 0..n the pair is value = top, name =
  the value below it; pairs are set from the top pair down (so for a repeated name the pair deepest in the stack,
  i.e. written first in the source, wins); pops 2n, pushes the object.
- **0x4A toNumber** `82E702F0`: numbers stay; values the NaN test `82E67400` rejects become NaN; otherwise a string
  without '.' (search for `"."` at `0x820634A4`) becomes an integer via ToInteger, one with '.' a float.
- **0xA5** `82E73F50`: pushes its string operand, then tail-calls the getMember handler `82E705C0` (0x4E): pop name,
  pop object, push object[name] (EA's string-operand GetMember, like 0xA4 / 0xAF for variables / constants).
- **ToInteger** `82E5F2A8` (used by random, initArray, initObject, toNumber): int as is; float clamped to i32 and
  truncated; bool 0 / 1; string: "0x..." (length > 2) via strtol base 16 `82F53468`, else atoi `82F4DEC0`; values
  without the valid flag give 0; other types compare against the global value `[0x8307F3A8]` (0 if equal, else 1).
These match standard SWF / AVM1 Trace, RandomNumber, InitArray, InitObject, ToNumber and GetMember; retail's
handlers decide the details above (integer results, the '.' rule, atoi-style string parsing).

### Change
- `apt_vm.rs`: the six opcodes; `pub fn to_integer` (82E5F2A8) and `pub fn to_number` (82E702F0); `Host::trace`
  (default no-op, so the HUD host is unchanged) receives trace output; `Vm::seed_random` seeds the script random
  generator.
- Safety on untrusted (mod) movies: initializer count capped at 256 and checked against the stack before anything is
  popped; random with n = 0 gives 0 where retail traps; 0xA5 without a string operand is an error, not a panic;
  all errors go through the existing `Result` path and execution budget.
- Determinism: random uses a seeded xorshift64* owned by the VM (fixed default seed), so replays and a future
  multiplayer session get the same values from the same seed. Only the `rand % n` contract is retail's; retail's
  own generator state is runtime data and not reproduced.

### Verification
- `cargo test -p skate-game --bins -- apt hud`: 19 passed, 4 ignored (data-gated), including per-opcode unit tests
  (`apt_vm::tests`: trace pop + host output, random range / determinism / n = 0 / underflow, toNumber and ToInteger
  rules, initArray order / underflow / limit / negative count, initObject pairs and duplicates, 0xA5 member read)
  and the existing HUD tests unchanged.
- `apt_vm::tests::retail_menu_movies_use_no_unknown_opcode` (skipped when no local data): runs every action stream
  of the 16 decoded movies (183 streams) with a stub host and executes every opcode they contain in isolation; no
  "Unsupported APT opcode". Data: `python .claude/skills/recomp-research/tools/apt_actions_json.py
  <fedata.big> .local/research/fe-menus/actions <movie paths>` (local only, never committed); the test reads
  `SKATE3_FE_ACTIONS` or `.local/research/fe-menus/actions`.

### Open
- The exact string grammar of the retail NaN test `82E67400` (leading spaces, signs, exponents) is only partly read;
  ours accepts what Rust's float parser accepts plus "0x" hex.
- Frame labels: the decoded movies list 151 labels; reaching them needs the display side (imports, screen manager),
  milestone 3 later parts.

## Milestone 3b: cross-movie imports (2026-10-08)

### Problem
The menu movies are built from shared library movies: core_menu imports `Button Item` (button_item2),
`HighlightText` (menu_part_text_hilite), the picker (menu_picker) and `Image_Renderer_Component` (image_renderer);
options imports panel and highlight_option / selector / slider; the libraries import further libraries
(menu_picker -> menu_part_text_hilite -> menu_part_glow, panel -> button_menu -> button_item2). An imported character
id has no local definition, so our player (`apt_movie.rs`, HUD-only, whose movie is self-contained) fails with
"APT unknown character" as soon as a menu movie places one.

### What retail does
[data] The root character of a movie (type 9) has an import table (count / offset at body +32 / +36, 16-byte records:
source path, symbol name, local character id, runtime slot) and an export table (+40 / +44, 8-byte records: name,
character id). Source paths are relative to `data/fe/` (`source/controls/button_item2`).

[code, TU3]
- **When the source loads:** the APT load queue `sub_82E7E9F8` moves an entry from requested to loading by calling
  the host load callback stored in AptInitParms +36 (parms block `0x830CE9C8`, filled by `sub_825D3840`): that is
  `sub_82CA4138`, which prefixes `data\fe\` (`0x820AED74`) and opens the movie with `sub_82CA3428` (`/` turned into
  `\`, `big:%s`, `.apt` and `.const`). When a movie has loaded, `sub_82E7E8C0` walks its import list and asks
  `sub_82E7E6E0` for each source movie by name; the movie is linked only when every source is there, otherwise it
  stays waiting. Sources are therefore loaded on demand, before the importer is linked, and are themselves already
  linked (chained imports resolved) at that point.
- **Linking** `sub_82E76070` (argument: the movie body): for each import record in table order, scan the source
  movie's export list from the start and take the first export whose name equals the record's name byte for byte
  (case-sensitive compare loop at `82E760CC`); write the source movie's character-table entry for that export's id
  into this movie's character table at the record's character id, set the character's owner reference to the source
  movie (`sub_82CA44A0`, record +12) and count a use (16-bit, saturating). No export with that name: the slot is set to
  null. The second loop of the same function only turns local sprite / button child indices into pointers.
- **Use:** placements look the character up in the movie's character table (`sub_82E810B8`), so an imported id
  behaves like a local one from then on.

### Change
- New `crates/skate-game/src/apt_imports.rs`, engine-independent: `Import`, `Export`, `MovieDecl` (from the decoded
  JSON or the pipeline's movie JSON), `library_key` (stable key: `/` separators, no `data/fe/`, no `.apt`, like
  retail's prefix and extension handling), `MovieSource` (where movies come from: the user's assets, test data),
  `resolve(root, movie, source)`: loads each source movie once, on demand, and binds every import of the root and
  of every library it reaches with retail's rule (first export of exactly that name; a chained import is followed to
  the defining movie, which is what retail's pre-linked source table gives). Output: sorted `Binding`s
  (movie, character id -> defining movie, character id) and `Unresolved` entries with a `Failure`; serialisable and
  deterministic (BTreeMaps, sorted output).
- `Movie::link_imports(key, resolution, libraries)` in `apt_movie.rs`: gives each bound id a copy of the defining
  library's character plus everything it places, its action streams and its fonts, under free ids / action keys
  from a deterministic allocator; unresolved ids stay absent (retail's null slot). The HUD path does not call it,
  so the HUD is unchanged.
- **Moddable:** `OverrideTable` holds library movies supplied by mods, keyed by library key, per owner (newest owner
  wins); `table.over(&mut base)` is a source that consults the overrides before the user's assets, so a mod can
  replace a retail library (`source/controls/button_item2`) or add a new one that its own movies import;
  `clear_owner(id)` reverts it when the mod is disabled.
- **Safety on mod movies:** missing source movie, missing export, export of an undefined id, import cycles, chains
  longer than 16 and more than 256 movies are reported as failures, never panics or loops; the copy step has its
  own depth (64) and size (65 536 characters) limits and returns errors.
- **NOT RETAIL (layout):** retail shares the source character by pointer and keeps the source movie for its
  constants; ours copies, which behaves the same because character data is immutable and our action streams are
  already decoded (no constant-pool lookups at run time).
- **NOT RETAIL YET (not decoded):** what retail does when a source movie file does not exist (the queue entry never
  reaches the loaded state; whether it waits forever or gives up was not traced) and with import cycles (each movie
  would wait for the other). Ours reports both and links nothing for them; no retail movie has either. The limits
  above are ours (safety).
- Tool: `.claude/skills/recomp-research/tools/apt_actions_json.py` now also writes `imports`, `exports` and the
  local character ids; the 4 libraries reached only through imports (image_renderer, menu_part_arrow,
  menu_part_glow, button_menu) were decoded as well (20 movies, local only, never committed).

### Verification
- `cargo test -p skate-game --bins -- apt hud`: unit tests `apt_imports::tests` (chain resolve, `.apt` / `data/fe/`
  key forms, missing movie / export / undefined export, cycle and chain limit, determinism, mod override and revert)
  and `apt_movie::import_tests` (library subtree copied, child ids and action streams remapped, the imported clip
  instantiates, errors instead of panics); the existing HUD and VM tests unchanged.
- `apt_imports::tests::retail_menu_movies_resolve_every_import` (data-gated, `SKATE3_FE_ACTIONS` or
  `.local/research/fe-menus/actions`): loads the 20 decoded movies and resolves every import of every movie:
  "20 movies, 19 of 19 imports resolved", none unresolved. Run 2026-10-08: 25 passed, 4 ignored.

### Open
- Lua entry point: the `OverrideTable` is the mod-facing layer, but no `sdk` command feeds it yet (needs the menu
  movies in the user's assets first, part of the screen-manager step).
- Running the linked menu movies end to end needs the pipeline to export menu movies in the player's JSON format
  (characters, fonts, language) like the HUD prepare step; then `link_imports` is called after `Movie::load`.
- The missing-file and cycle behaviour of retail (above).

## Milestone 3c: menu movies exported at setup, loaded and linked (2026-10-08)

### Problem
The APT player and the import linker (3a, 3b) only ran on research decodes in `.local/`. The engine had no menu
movies from the user's disc, and mods had no way to reach the import override layer.

### Change
- **Setup export** `tools/prepare_menu_movies.py`, run inside the HUD stage (`prepare_runtime_huds.py`, same disc
  archives and fonts as the scoring HUD; `versions.py` fingerprints it in the `hud` group, so a re-run refreshes the
  set). Starts from 12 menu screens (`MENU_ROOTS`: FE_root, core_menu, dimmer, freeskate_options, options, main3dhud,
  screenmanager, screentransitionmanager, tabs, scrollbar, sk8popup, small_popup) and follows every import, as
  retail's load queue requests each import source by name before linking (sub_82E7E8C0, milestone 3b). Writes
  `assets/private/menu-movies/` (never committed): `manifest.json` (roots, movies, hashes), `shared.json` (language
  table, fonts, native font layout, once for all movies) and `movies/<key>.json` per movie in the HUD player format
  plus `imports` / `exports`; keys are the retail import paths (`apt_imports::library_key`). Install verifies every
  hash into a staging folder and swaps it in; a failed export keeps the previous set and writes
  `menu-movies-availability.json` like the HUD. On the user's disc: 23 movies (12 roots + 11 libraries), none
  missing, every font resolved, about 9 MB.
- **Engine** `crates/skate-game/src/retail_menu_movies.rs`: resource `RetailMenuMovies`, loaded at startup next to the
  menu tables (`retail_menus.rs` plugin). Each movie: `MovieDecl::from_json` + `apt_imports::resolve` (import
  tables resolve even for movies the player cannot load yet), `Movie::load`, `Movie::link_imports`. Log
  `RETAIL_MENUS movies=<linked> imports=<resolved>/<total>`, one warning per movie that failed with the reason.
  Nothing is drawn.
- **Moddable** `sdk.menus.movie(name, path)` (command `menu_movie`): a mod supplies or replaces a menu or library
  movie by its import name with a player-format JSON file from its own folder (path contained in the package,
  16 MB cap). It goes through `OverrideTable` (newest owner wins) and every movie is relinked, so menus importing a
  replaced library pick up the mod's version (a missing export leaves the slot empty, as retail's linker does).
  Removed on mod stop / disable (`retail_menus::clear_owner` / `clear_mods`). `sdk.menus.info().movies` lists linked
  movies, import counts, failures and each mod's movies. Multiplayer: keys are stable strings, state is per-owner and
  rebuilt deterministically (sorted keys).
- **Data-gated tests from a worktree**: `apt_imports::main_checkout()` follows a worktree's `.git` file back to the
  main checkout (override `SKATE3_MAIN_CHECKOUT`), so the 3a / 3b tests now find `.local/research/fe-menus/actions`
  from `.local/wt-menus` too. The Python test finds the disc the same way through `git rev-parse --git-common-dir`
  (overrides `SKATE3_DISC`, `SKATE3_COLLECTIONS`).
- NOT RETAIL (layout): the shared language / font file and the JSON format are ours; retail loads `.apt` / `.const`
  through `data\fe\` at run time. Same content.

### Verification
- `python -m unittest test_prepare_menu_movies` (in `tools/`): key rule matches the engine, hash-verified install
  replaces the old set and keeps it on failure, contained paths; data-gated export from the owned disc into a
  temporary folder: every root exported, none missing, no unresolved font, every import's export present in its
  source movie. 4 passed (13 s).
- `cargo test -p skate-game --bin skate3rust -- apt menu` with `SKATE3_MENU_MOVIES` at a scratch export: 42 passed,
  3 ignored. `retail_menu_movies::tests`: link + mod replace + revert, missing set; data-gated
  `retail_menu_movie_set_loads_links_and_runs`: `RETAIL_MENUS movies=13 of 23 imports=25/25`, no unknown opcode in
  any stream of the 23 movies. `cargo test -p skate-mods --lib -- menu`: 4 passed (Lua `menu_movie` command).

### Open (player gaps found by loading the real set)
- 5 movies use placement clip actions (place flag 0x80, `onClipEvent` handlers): `apt_display.rs` refuses them
  (button_item2, button_menu, image_renderer, tabs, core_menu). 4 more fail only because they import button_menu
  (panel, freeskate_options, options, sk8popup). Done in Milestone 3d.
- small_popup: the Futura Shadow foreground-font rule in `apt_text.rs` is HUD-specific (looks for a `futuraheavy`
  font in the same movie); retail's pairing rule for other movies is not decoded.
- The root list is ours (the menu screens the engine will use); more screens can be added without code changes in
  the engine.

## Milestone 3d: placement clip actions (2026-10-08)

### Problem
9 of the 23 exported movies did not load: 5 place clips with clip actions (place flag 0x80, `onClipEvent`
handlers), which `apt_display.rs` refused, and 4 import one of them (button_menu).

### What retail does [code, TU3]
- **Record.** Placement +60 (sub_82E7B340 reads it at record+4+56 when flag 0x80 is set; on a new placement and on
  a move) points at `{u32 count, u32 events}`; each event is 12 bytes `{u32 flags, u32 key_code, u32 actions}`
  (sub_82E5B158 walks it with stride 12 and queues record+8). sub_82E7B090 stores the table pointer at display
  object +24 only when one is given, so a move without 0x80 keeps the old table.
- **Flags** are the SWF clip-event values: Load 0x1, EnterFrame 0x2, Unload 0x4, MouseMove 0x8, MouseDown 0x10,
  MouseUp 0x20, KeyDown 0x40, KeyUp 0x80, Data 0x100, Initialize 0x200, Press 0x400, Release 0x800,
  ReleaseOutside 0x1000, RollOver 0x2000, RollOut 0x4000, DragOver 0x8000, DragOut 0x10000, KeyPress 0x20000,
  Construct 0x40000. Evidence: the dispatch sites below, and the dynamic-handler table 0x82FC9A3C (6 pairs of mask
  and name id) mapping 2 / 0x40 / 0x80 / 0x100 to onEnterFrame / onKeyDown / onKeyUp / onData (name table
  0x83044C60). The ids for 1 and 4 sit 10 apart like onLoad / onUnload (0xCF / 0xD9) but one entry off from the
  others, so that pair is medium confidence; the dispatch sites confirm Load = 1 and Unload = 4 by use.
- **Placement** sub_82E7AF18 (sprites only): ORs every event's flags << 8 into display flags +20 for the masks
  0x201C7 (load, enterFrame, unload, key down / up, data, keyPress), registers the clip as a key listener
  (sub_82E88050) when it has keyDown / keyUp / keyPress, then dispatches **Initialize (0x200) and Construct (0x40000)
  at once** (sub_82E5B158: an action object of priority 60 / 70 run synchronously through sub_82E6A208).
- **Tick** sub_82E5B640 (every clip, playing or not): frame advance, frame controls (sub_82E810B8: init actions,
  placements), frame do_actions queued at the back (sub_82E81438 -> sub_82E78168); then **EnterFrame** (2), except
  on the clip's first tick, pushed at the **front** of the action queue (sub_82E78240); on the first tick
  (flag 0x80 in +20) **Load** (1) at the back instead, then that flag is cleared; then the children
  (sub_82E7C658). So children's enterFrame handlers run before their parent's, and all of them before that tick's
  frame actions.
- **Unload** (4): removal sub_82E55DA8 queues it at the back before tearing down the children; it runs on the
  removed clip.
- **Scope**: the queue entry's target is the clip that owns the placement (sub_82E78168 stores r5 = the clip), so
  `this` is the placed clip, as in Flash.
- **Input events** (press / release / rollOver / key*) come from the host's input through the same dispatcher;
  no movie in the set uses them (all 9 tables hold only Construct). Out of scope here.
- **0xA6** (handler 82E74028): push string operand, then setVariable (0x1D, 82E6D248). Every construct handler in
  the set is `_type = "CustomButton"` / `mButtonImage = "A"` style component parameters written with it.

### Change
- `tools/prepare_menu_movies.py`: decodes the table into `clip_actions` on the placement (bounds-checked, at most
  64 events) and exports the handler streams; menu set version 2 (the engine refuses version 1, setup re-exports).
- `apt_display.rs`: `ClipAction`, `clip_event` flag constants, `Placement::clip_actions` (kept on a move without
  0x80, as retail); a 0x80 placement without a decoded table (old export) and more than 64 events are errors.
- `apt_movie.rs`: `Pending { object, offset, event }` queue; `create` queues Initialize then Construct in
  `immediate` (run before anything queued) before the clip's first frame, Load after its first frame's actions;
  `advance` pushes EnterFrame of every clip that existed before the tick to the queue front, in creation order (so
  children first); `remove` queues Unload; `next_action()` drains immediate first and skips handlers of removed
  clips except Unload. `link_imports` copies and renumbers clip-action streams. `hud_runtime.rs` drains through
  `next_action()` (HUD movies have no clip actions: unchanged).
- `apt_vm.rs`: opcode 0xA6.
- Mod movies use the same path (same JSON, same limits).
- NOT RETAIL (timing): retail runs Initialize / Construct inside the placing action; ours right after the running
  action, still before every other queued action. NOT RETAIL YET (order): retail queues a parent's frame actions
  before its new children tick (Load); our seek places children first (existing frame-action order).

### Verification
- `cargo test -p skate-game --bin skate3rust -- apt hud menu` with `SKATE3_MENU_MOVIES` at a scratch export:
  47 passed, 4 ignored (unrelated data-gated tests matched by the filter). New `apt_movie::clip_action_tests`:
  dispatch order (initialize, construct, the clip's frame action, load; next tick enterFrame; unload runs on the
  removed clip; nothing after removal), scope (construct sets `_type` on the placed clip, not the parent, through
  0xA6), unsafe tables (old export without a decoded table, more than 64 events) are errors and a move keeps the
  table. Data-gated `retail_menu_movie_set_loads_links_and_runs`: **`RETAIL_MENUS movies=22 of 23
  imports=25/25`** (was 13), no unknown opcode in any stream including the clip-action streams.
- HUD: `scoring_runtime::tests::landed_multiplier_reaches_hud_across_repeated_tricks` (with `SKATE3_ASSET_ROOT`)
  passes; the HUD drain only changed to `next_action()`.
- `python -m unittest test_prepare_menu_movies`: 5 passed (new: table layout and bounds); the real export decodes
  9 tables, all Construct only.

### Open
- small_popup (the last of the 23): the Futura Shadow foreground-font rule (done in Milestone 3e).
- Input-driven clip events (press, release, rollOver, key*) wait for the menu input milestone; the dispatcher
  shape (`queue_events`, flag constants) is ready for them.
- The onLoad / onUnload name ids in table 0x82FC9A3C are one entry off from the others (see above); behaviour
  does not depend on it.

## Milestone 3e: the Futura Shadow secondary font in every movie (2026-10-08)

### Problem
small_popup, the last of the 23 movies, failed with "Missing native Futura Shadow foreground font". Our rule
(apt_text.rs, written for the HUD) looked for the foreground font among the movie's own APT font characters;
small_popup places "Futura Shadow" but no character that uses futuraheavy.

### What retail does [code, TU3]
- Native font table: FontManager sub_82808AE8 builds up to 12 rows of 36 bytes at 0x830680D0 from the font records
  (class 0xFECFBCAF356518C4, the list picked by language mode): row +0 file name, +4 APT name, scale / offset
  floats, +32 the font (loaded lazily by sub_82809308 from `data\fe/fonts/<file name>.bmpFont`, sub_82807870).
- Lookup sub_82809208(name): case-insensitive compare (sub_82AE8A10) of `name` against each row's +4 name; on a
  miss, the row whose +0 name is "debug" (0x821A06E0); else row 0. It never fails.
- Text font setup sub_825D6B68: primary = lookup(APT font name); if that name equals "Futura Shadow" (0x8220BD44,
  sub_82AE89B0) the text also gets a secondary font = lookup("futuraheavy") (0x8220BD54) with X adjustment 1 and
  the other 0. sub_82CA1FD8 draws the primary black, then the secondary in the text colour. The secondary comes
  from the global native table: it does not depend on the movie using that font.

### Change
- `apt_text.rs`: `Font::foreground` is now the secondary native font itself (`Option<Box<Font>>`), not an APT
  character id. `RETAIL_FONT_PAIRS` holds retail's one pair; data may replace it with `font_pairs` (APT name ->
  native file name; the menu export writes retail's table into shared.json, an empty map turns pairs off). The
  partner is found in the whole native table (`font_mappings` by file name), case-insensitive. A missing partner
  (old export, a mod's data) leaves the text with its primary pass: no error, no panic.
- `apt_movie.rs` (import linking) copies the secondary with the font; `apt_scene.rs` draws it directly;
  `scoring_hud.rs` loads its texture too. HUD output unchanged (same fonts, same passes).
- `tools/prepare_menu_movies.py`: `FONT_PAIRS`; the export adds each pair's partner font to shared.json even when
  no movie places it, and writes `font_pairs`. Older installs still load (retail table default; without the
  partner asset the shadow text draws its primary pass only): re-running setup group `hud` adds it.
- Moddability: a mod's movie (same JSON path) gets the same rule; a mod font missing its partner falls back
  without error.
- NOT RETAIL (fallback): retail's lookup falls back to the "debug" row; we have no debug bank and draw the
  primary pass only.
- OPEN QUESTION (match field): the static data says row +4 is the APT name (the debug row's is "NOT FOR USE IN
  APT", the futuraheavy row's is "Futura Std Medium"), so lookup("futuraheavy") would miss and fall back to the
  debug font. We keep the HUD's established match by file name (futuraheavy), which the HUD port matched
  visually. A recomp trace of sub_82809208's return for "futuraheavy" would settle it.

### Verification
- `cargo test -p skate-game --bin skate3rust -- apt hud menu font` with `SKATE3_MENU_MOVIES` at a scratch export:
  51 passed, 4 ignored. New `apt_text::font_pair_tests` (4): Futura Shadow gets futuraheavy from the native table
  without a character using it; missing partner asset or table row gives the primary pass only; `font_pairs` data
  replaces the retail table (case-insensitive, empty map turns pairs off, unknown file falls back); other fonts get
  no partner. Data-gated `retail_menu_movie_set_loads_links_and_runs`: **`RETAIL_MENUS movies=23 of 23
  imports=25/25`**; the test now fails on any load failure.
- HUD unchanged: `scoring_runtime::tests::landed_multiplier_reaches_hud_across_repeated_tricks` (with
  `SKATE3_ASSET_ROOT`), `scoring_hud::tests` and the other scoring tests pass.
- `python -m unittest test_prepare_menu_movies`: 5 passed; the scratch export holds 23 movies, no missing movie,
  no unresolved font, `Futura Std Medium` in shared.json.

## Milestone 3f: placement clip depth (2026-10-08)

### Problem
A SWF placement can carry a clip depth: the placed shape is then a mask for the depths above it up to the clip
depth. Our display list parsed the field (`apt_display.rs`) but the drawing path (`apt_scene.rs`) ignored it, and it
was unknown whether retail APT draws such masks (stencil, scissor) and whether the menu movies use them.

### What retail does [code, TU3]
- **Read.** `sub_82E7B340` (placement record at r31 = control + 4: +0 flags, +4 depth, +8 character, +12 matrix,
  +36 colour transform, +44 ratio, +48 name, +52 clip depth, +56 clip-event table) loads +52 and passes it to
  `sub_82E7AE98` -> `sub_82E7B090` on every path that creates a display object: flag 2 (new or replaced character)
  and a move (flag 1) onto an empty depth. It does not test flag 0x40. A move of an existing object
  (`loc_82E7B5F8`) passes -1, and `sub_82E7B090` only hands the value to the create call `sub_82E7A950`, so a move
  never changes a stored clip depth.
- **Store.** `sub_82E7A950` writes it as a halfword: `sth r23,22(props)` (props = display object +32 -> +4; +20 is the
  depth halfword). Clones copy it (`sub_82E832C8`, `sub_82E83BC0`).
- **Use.** Over the whole recompiled image only two code sites read props +22: the getter `sub_82E48930` (and
  `sub_82E461A0` on props), which no code calls and no data table points at, and the bounds walk `sub_82E59C28`
  (called by `sub_82E59DA8`, the local-bounds function behind sizes and hit areas), which leaves out every child whose
  clip depth halfword is below 0x8000 (a mask). No other reader exists, the APT renderer (game side, 82CA range) has
  none either, and no load of the word at +20 extracts the low half. So **retail draws a clip-depth shape like any
  other shape and masks nothing**; the only effect is that masks do not count towards a clip's bounds. Script masks
  (`setMask`, warning string 0x82062250) are a separate mechanism, not used by these movies.
- **Data.** All 23 retail menu movies: 1434 placements, every clip depth -1, flag 0x40 never set. Both HUD movies:
  1233 placements, all -1. No retail movie we ship uses a clip depth.

### Change
- `apt_display.rs`: clip depth follows retail: taken from the record whenever a placement creates the object
  (flag 2) regardless of flag 0x40, stored as 16 bits (`as i16`), missing value = -1; a move keeps the stored
  value (flag 0x40 on a move no longer errors or changes it). Doc comment on `Placement::clip_depth`.
- Drawing (`apt_scene.rs`) unchanged on purpose: retail draws the shape and masks nothing, so no stencil or
  scissor. Bounds: our player has no sprite bounds yet (`_width` / `_height` exist only for text,
  `apt_movie.rs` text setup); when sprite bounds land they must skip children with clip depth >= 0
  (`sub_82E59C28`).
- Mods: a mod movie with clip depths draws exactly as retail would (no mask); values are clamped to 16 bits like
  retail, no panic path.

### Verification
- `cargo test -p skate-game --bin skate3rust -- apt hud menu font` with `SKATE3_MENU_MOVIES` at a scratch export:
  52 passed, 4 ignored. New `apt_movie::clip_action_tests::apt_clip_depth_follows_retail_placement` (stored without
  flag 0x40, kept on a move with flag 0x40, replaced with the character, 16-bit storage, missing = -1, removed with
  the object). The data-gated set test now counts clip depths: **`RETAIL_MENUS movies=23 of 23 imports=25/25
  placements=1434 clip_depth=0`** and fails if a retail menu movie ever carries one.
- HUD unchanged (all its clip depths are -1 and drawing does not read the field):
  `scoring_runtime::tests::landed_multiplier_reaches_hud_across_repeated_tricks` (with `SKATE3_ASSET_ROOT`) passes.

### Open
- Retail creates an object when a move (flag 1 only) targets an empty depth; our display list still refuses that
  ("APT move references empty depth"). Not hit by any retail movie.
- A recomp trace could confirm at run time that nothing reads props +22 while a menu draws (static evidence only).

## Milestone 4: bitmaps and textured fills (2026-10-08)

### Problem
The menu movies are mostly textured shapes (panels, glows, arrows, tab icons), but the player could only describe
shapes that always have a texture (the HUD case): solid units did not load, imported shapes lost their geometry
(`link_imports` gives them a new id in the importing movie) and nothing named a bitmap, so a mod could not replace
one.

### What retail does
- [data, the user's disc, 23 menu movies] 154 bitmap characters (type 7, `{texture_id}` only). None is ever placed:
  placements reference sprites (204), shapes (193) and text (25) only. Every bitmap is drawn as the texture of a
  shape unit. Shapes come pre-tessellated in each movie's `.geo` (no gradients or curves at run time): 231 units,
  207 `texture_clamped` (render type 2) and 24 `solid` (type 1); no `texture_wrapped` (3) or `line` (0). Every unit
  resolves; the 207 textured units use all 154 bitmaps (30 are used by more than one unit). The 2 HUD movies
  (trickdisplay and its fonts) have only textured units.
- [data] GEO unit layout (`tools/vendor/skate3_ui/geo.py`): +0x00 render type, +0x04 RGBA colour (floats), +0x14
  bitmap character id, +0x18 UV matrix (6 floats, applied to the vertex position, in texels), +0x30 triangle count,
  +0x34 triangles. The bitmap character's `texture_id` names the texture `<movie>_textures\<id>.Texture` in the
  movie's texture package (fetexture.big / fedata.big RX2), decoded to RGBA8 at setup (158 payloads incl. fonts).
- [code, TU3] The movie loader `sub_82CA3428` (AptInitParms +36 callback chain, doc "Milestone 3b") opens the three
  files of a movie from the `data\fe\` table at `0x820AED00` (`.apt`, `.const`, `.geo` at `0x820AECF8`) into
  {pointer, size} slots at record +532 / +540 / +548: the GEO blob is kept as authored. The texture name format
  `%s_textures\%i.Texture` is at `0x820AED9C`, with a fallback `missingtexture.Texture` at `0x820AEDB4`.
- [code, TU3] Draw chain (details in `.claude/notes/fe-menus-re.md` "APT shape renderer"): `sub_82CA4020` builds
  the movie resource, `sub_82CA30B8` relocates the GEO in place, `sub_82CA2A40` makes one record per shape and
  `sub_825D5DC8` one draw object per unit: type 1 -> solid object `sub_825D5800` (draw `825D5AE0`), types 2 / 3 ->
  textured object `sub_825D4FC0` (draw `825D5580`), any other type (line 0) -> nothing. The textured draw computes
  one colour = unit colour * colour-transform multiply + add, draws nothing when its alpha <= 0, and hands 6
  vertices (2 triangles), the UV rows from unit +24..+44, the texture and that colour to `TheSimpleDraw`
  (`sub_82805368`, shaders `simpledraw_*` in shaders_final.big).
- Not decoded yet: the sampler (filter, and which of types 2 / 3 clamps); see Open.

### Change
- **Setup** (`tools/prepare_menu_movies.py`, set version 3): each textured unit also carries `bitmap`, the GEO unit
  +0x14 id it was resolved through (`_with_bitmap`). Re-export on the next setup run (the `hud` group fingerprint
  covers the tool). No new files: the RGBA payloads were already exported.
- **Scene** (`crates/skate-game/src/apt_scene.rs`): `Fill` (GEO render type), `Shape.texture` optional (solid and
  line units), `Shape.bitmap`, `Vertex.uv` optional; `Draw.fill` tells the renderer clamp or wrap addressing (text
  draws are clamped); a solid draw has texture `""`. Line units are not drawn (retail `sub_825D5DC8`). Shapes come
  from a `ShapeSource`; for menus (`retail_unit_colour`) a unit draws with retail's single colour
  `unit colour * multiply + add` and is skipped at alpha <= 0 (`825D5580`). The HUD keeps its `Shapes` table and
  its earlier `texel * multiply + add` (output unchanged; see Open).
- **Imports** (`apt_movie.rs`): `Movie::shape_origins` records, for every shape `link_imports` copies, the library
  and id that define it (following nested imports to the defining movie).
- **Menu set** (`retail_menu_movies.rs`): GEO units per movie key (validated: textured units need a bitmap,
  finite values; a bad movie is a failure, not a panic); `scene(key, movie)` gives the `ShapeSource` for a running
  instance; `image(texture)` returns the retail payload (path in the set + size) or a mod image.
- **Stable id and mod entry point**: a bitmap is `<movie key>#<bitmap character id>` in the movie that defines the
  shape. `sdk.menus.bitmap(movie, bitmap, path)` (command `menu_bitmap`) replaces it with a PNG from the mod's
  folder (4 MB file, up to 2048 x 2048, 64 per mod), decoded to RGBA8; the image covers the same fill (same UVs,
  normalised), newest mod wins, removed on mod stop / disable with the rest of the mod's menu content
  (`clear_owner` / `clear_mods`). `sdk.menus.info().movies.bitmaps` lists every id (`ids`) and each mod's
  replacements (`mods`). A whole movie can still be replaced with `sdk.menus.movie`.
- **Fixed**: `menu_movie` validated its path as a `.glb` asset, so every Lua `sdk.menus.movie(..., 'x.json')` was
  rejected at submit; it now takes a `.json` content path.
- Deterministic and local: draws follow the display list order; ids are strings sorted in `BTreeMap`s.

### Verification
- `python -m unittest test_prepare_menu_movies` (in `tools/`): new `test_textured_units_get_their_bitmap_id`; the
  data-gated export checks that every shape resolves and every textured unit names a bitmap character whose texture
  id is the payload's. 6 passed.
- `retail_menu_movies::tests::menu_bitmaps_draw_through_imports_and_mods_replace_then_revert`: a menu importing a
  library sprite draws the library's solid and textured units (colour, UVs, placement), the retail payload resolves,
  a mod bitmap replaces it by stable id (invalid ids, sizes and movies refused), newest owner wins, stop reverts.
  `menu_units_follow_the_retail_colour_rule` (line unit not drawn, alpha 0 skipped, add folded into the colour),
  `menu_shapes_reject_textured_units_without_bitmap`. skate-mods: `menu_movie` / `menu_bitmap` validation.
- Data-gated `retail_menu_movie_set_loads_links_and_runs` now also walks every shape of every linked movie
  (local and imported) and draws each movie's first frame: **`RETAIL_MENUS movies=23 of 23 imports=25/25
  placements=1434 clip_depth=0 shape_units=528 textured=481 solid=47 bitmaps_used=154 bitmap_ids=154
  frame0_draws=863 frame0_bitmap_draws=835`** (unit counts are per linked movie, so library shapes count once per
  importer; every one of the 154 bitmaps is reached and its payload has the exported size).

### Open
- Sampler: the tfetch filter / address bits of the `simpledraw_*` pixel shaders are not decoded (disassembler fails
  on constant-less shaders; notes "APT shape renderer" OPEN), nor which of types 2 / 3 clamps. Ours names the
  addressing from the render type (`Draw.fill`); the screen-manager renderer must take filtering from that decode.
- The HUD movie has 233 placements with colour-transform add terms; retail folds add into the unit colour
  (`825D5580`), ours still adds after the texel for the HUD. Switching the HUD to `retail_unit_colour` is a one-line
  change that changes its look, so it waits for an in-game check. The solid draw `825D5AE0` is assumed to use the
  same colour rule (to read).
- `missingtexture.Texture` (`0x820AEDB4`): retail's fallback for a missing payload; ours fails the movie at setup
  instead (no retail movie misses one).
- Drawing the menus on screen comes with the screen manager milestone (the renderer maps `Draw.texture` through
  `RetailMenuMovies::image`).

## Milestone 5a: the front-end stage (2026-10-10)
Branch `ui/retail-menus-2` (after #59 merged). Retail's screen manager is a script (`helper/screenmanager.apt`);
the C++ only calls it by name and answers its callbacks. So the port runs the retail movies themselves on one stage.

### Problem
The player ran one movie per VM (the HUD). The menus need FE_root, the screen manager and each screen movie in ONE
script VM sharing `_global`, screens loaded into clips at runtime (MovieClipLoader), clips created by script
(createEmptyMovieClip), component classes bound by `Object.registerClass`, and each movie at its own frame rate.

### Evidence
- [code] C++ calls the AS by path string: names at 0x8302C608 + index * 4 (OpenScreen 6, CloseScreen 8,
  DestroyScreen 9, SetScreenState 10, UpdateInput 11, ShowScreen 12, RegisterTemplate 16), via sub_825D3B38 /
  sub_8263F320. Template table 0x8302F1A8 (133 records of 16 bytes: name, swf path, type, -1), e.g. 69 CrossBar
  `main/core_menu.swf`, 90 Options, 131 TipsLoader. OpenScreen (sub_82D09140) passes ("source/screens/" + path,
  layer, "0"/"1", template name). Callbacks OnScreenLoaded / IntroComplete / OutroComplete (825A4A70 / 825A4AF8 /
  825A4B70) index their slot with id - 600 (AS id = 600 + template index, the AS hands out depths from 600).
  Research notes: `.local/research/fe-screen-stack.md`.
- [code] Retail pop (opcode 0x17, handler 82E6D048) pops only above the stack base; on an empty stack it does
  nothing. screenmanager.apt relies on it (a pop after a setMember at 0x875).
- [data] Libraries declare their component classes in their ROOT init actions
  (`Object.registerClass("Menu_Picker_Component", menu_picker)` in menu_picker.apt); importers name the clip by
  that linkage name (core_menu imports character 166 as Menu_Picker_Component). Retail loads every library before
  linking (sub_82E7E8C0), so those init actions run.

### Change
- `apt_stage.rs` (new): `Stage` = one `Vm` + loaded movies (`Level`), `StageNatives` (the C++ host objects: Game,
  CoreMenu, ...), `MovieSource` (linked movies with mod overrides). MovieClipLoader (addListener, loadClip queued to
  the next update like retail's load queue, unloadClip, onLoadStart / Complete / Init), createEmptyMovieClip,
  removeMovieClip, getDepth, `Object.registerClass`, library root init actions once per stage, `call_path` for the
  C++ calls (`_global.ScreenManager.OpenScreen`), per-movie `milliseconds_per_frame` (header of character 0).
  Retail `.swf` template paths map to the packed `.apt` key.
- `apt_movie.rs`: `initialize_into` (a loaded movie's root reuses the target clip's object, so script references
  such as ScreenManager.mMovieClipList stay valid), `unload`, `milliseconds_per_frame`, `linkage`,
  `import_files`, `classes` + `CLASS_CONSTRUCTOR` (clip gets the class prototype; constructor runs after its
  initialize / construct handlers; the exact retail point is not decoded).
- `apt_vm.rs`: retail pop rule; String methods (length, indexOf, lastIndexOf, substring, substr, slice, charAt,
  charCodeAt, case) and Array methods (push, pop, shift, unshift, splice, slice, concat, reverse, join), standard
  AS2 semantics; `call_function`; `last_instruction` for error reports.

### Verification
`cargo test -p skate-game apt_` (23 passed, 1 ignored). `retail_screen_manager_opens_a_screen` (data-gated, the
exported set): FE_root traces "FE_ROOT SETUP!", screenmanager registers `_global.ScreenManager` (currentDepth 600),
RegisterTemplate("CrossBar"), OpenScreen with the retail CrossBar arguments, the next update loads core_menu
with its imported components, and the movie calls Game.OnScreenLoaded(600) then Game.OnScreenIntroComplete(600).
The only call left without a target is `screen_class.OnLoaded()` on core_menu's class, which defines none (an AS2
no-op).

### Open
- Root timeline loop: RESOLVED in Milestone 6 (below): loaded roots do not play.
- Library init order relative to the importer's own init actions, and the registerClass constructor point.
- `_root` of a loaded movie (ours: its own root; Flash: `_level0` unless locked).
- Drawing the stage (levels with their parent clip transforms) and the C++ host (CoreMenu, Game) are next.
- Input repeat timing (`.repeat` actions in input.cfg) is not statically findable; a guarded recomp hook is
  proposed in `.local/research/input-repeat.md`.

## Milestone 6: pause menu navigation and the CoreMenu object (2026-10-10, in progress)

### Evidence
- [code] Loaded movie roots do not play: load completion sub_82E7EF28 gives the root clip flag 0x80, not the
  playing flag 0x40 (set only by play(), action 0x06 handler 82E6BCC8, main-checked; stop 0x07 82E6BDB8 clears it).
  The tick 82E5B640 advances 0x80 clips only when the APT config byte [0x83037510+64] is set; Skate 3's init
  (sub_825D39F0 -> sub_82E654A0) leaves it 0. So screenmanager.apt's 2-frame root runs `new screenmanager(this)`
  once (research `.local/research/apt-root-loop.md`; how frame 0 is first entered is not traced).
- [code] Crossbar handler sub_8261B560 (research `.local/research/crossbar-nav.md`; main-checked: the eight fe sound
  keys against fe_callers.txt, the six-entry SetState registration in 8261B9C8, the row walk 8261F0E8 read in full):
  up / down move the row with wrap and walk to an enabled row (two passes, else row 0 and no sound / SetState),
  tab left / right wrap, reset the row and post RefreshMenu (0), select opens the sub column (SetState 3) for items
  with a sub-option kind or runs the item, back at level 1 closes the pause menu (FE id 56), at level 2 returns to
  level 1. Sounds crossbar_up / down / left / right, core_a / b / x_button.
- [code] CoreMenu: 21 native members (registrar sub_825B68E0; research `.local/research/coremenu-host.md`), all
  read-only views of the crossbar screen and the mode menu except SetOnlineTeamSubOptionDone.

### Change
- `apt_stage.rs`: loaded roots start stopped (above); `retail_screen_manager_closes_a_screen` now passes
  (OnScreenOutroComplete(600) after CloseScreen("600")).
- `skate-core/src/crossbar.rs` (new): `CrossbarState` (the screen fields, serialisable), `CrossbarView`,
  `NavEvent` (retail key indices), `CrossbarRules` (sounds and SetState numbers as data, retail defaults),
  `handle` (port of sub_8261B560 / 8261F0E8 / 8261D9F8 / 8261BC30 / 8261DA58), `RegistryView` (rows and enabled
  rules from the menu registry, so mods change the rows).
- `skate-game/src/crossbar_host.rs` (new): the 21 CoreMenu members over the registry and the crossbar state.

### Drawing and FELanguage (same milestone)
- `Stage::draw` gives each loaded movie's draws under the clip it was loaded into (ancestor matrices, visibility,
  alpha); `apt_scene::draw_from`. Font atlases join the menu image map (`RetailMenuMovies::image`). The open pause
  menu gives 24 draws (22 textured), all textures resolved (data-gated test).
- Text fields get `_height` (field height; core_menu centres its DLC label with it). NOT RETAIL YET: retail's
  autosize height rule is not decoded.
- `fe_language.rs` (new): FELanguage GetLocalizedFormatNumberString / FormatString / FormatStringCustom
  (registrar 825AA5E8; research `.local/research/felanguage.md`; main-checked the example strings
  `ID_CROSSBAR_BOARD_SALES_MILESTONE` "Board Sales Milestone {0:d}/{1:d}" and `ID_COMMON_PERCENT` "{0:d}%" in the
  exported language table). Placeholders `{N:type[.P]}`, `{{` literal, separators as data (`NumberFormat`,
  retail `. , 3`). NOT RETAIL YET: int vs float is taken from integrality (our VM has only f64); typed formats
  other than d / f are not decoded.
- `crossbar_host::FrontEnd`: the stage's host objects (CoreMenu, FELanguage; Game / Audio calls recorded).
- [data, main-checked] Sub-option label table 0x83027958 and helper table at +168 (7 kinds x 6 pointers; kind 0
  labels NEW_PARK .. TUTORIAL against helpers MY_PARKS .. PARK_EDITOR, retail pairs them by index); research
  `.local/research/menu-subtables.md` (extraction next).

### The retail boot and the first picture (same milestone)
- [code + data] The front end's `_level0` is `screens/main.apt` (sub_825D7E10 loads `source\screens\main` into
  `_level0`; research `.local/research/fe-main-root.md`). Its root defines Register*Enum, loads main3dhud /
  autosave / fmv_popup into `_level1..3` with getURL (opcode 0x83, handler 82E72590: "FSCommand:" or a ".swf"
  load) and fills layer clips with loadMovie: gScreenManager (screenmanager.swf), gChyron, gSubtitle,
  gFadeBackground. FE_root.apt is template 12 "Background" (main-checked), a screen, not the root.
- [code, main-checked] Crossbar IntroComplete (vtable 0x82303F90 +104 = 8261B1F0) -> mode selector 8261B9E8 ->
  SetState(InitialiseMenu) through the facade; the AS compares against FECrossBarScreenMode_* registered by C++
  through `_level0._root.RegisterChallengeEnum(name, "%d")` (path entry 34), which stores `_global[name]`.
- [code] Named DefineFunction2 stores into the current scope (82E73010 -> 82E68770): locals in a function, the
  timeline in a frame action (ours used `_global`).
- [data] Name lookup ignores case when no exact match: the only spelling in the image is "CoreMenu"
  (0x82300C68) while core_menu.apt reads `coremenu`.
- Export set version 4 adds `source/screens/main` and getURL operands (`tools/prepare_menu_movies.py`,
  `tools/vendor/skate3_ui/actions.py`); the user's install needs the hud setup group re-run (automatic).
- Stage: getURL `_levelN` loads, MovieClip.loadMovie, text changes re-localise (`property_changed`).
- Dev view (`retail_menu_stage.rs`, `SKATE3_RETAIL_MENUS=1`, F8; `SKATE3_MENU_MOVIES` points at a dev export):
  boots main.apt, registers the crossbar enums and the CrossBar template, opens core_menu with the retail
  arguments and posts InitialiseMenu on IntroComplete. In game (`.local/research/menus-m6/pause.png`): the retail
  Free Play pause menu, Port Carverton banner, tab icons, and the Free Play Main rows Challenge Map, Resume
  Career, Free Play, Call Skater, skate.Profile: Career (greyed: no handlers yet, the agreed rule).
- Missing frame labels leave the clip where it is (Flash behaviour; retail not traced).
- main.apt needs three more opcodes: 0x3F modulo (82E6E5F8: ToNumber both, C fmod, f32 result), 0x4B ToString
  (82E70458), 0x44 typeof (82E6ECC0; standard AS2 names, NOT RETAIL YET: retail's name table not read).
- Data-gated tests skip an installed set older than the engine reads (`apt_imports::test_menu_set`); pass
  `SKATE3_MENU_MOVIES=<export>/runtime` to run them against a fresh export.

### Open
- FIXED (title / category overlap): a clip whose `_x` / `_y` a script set no longer follows its timeline's
  placement (`Movie::script_moved`, stage only; Flash rule, NOT RETAIL YET: retail's flag is not traced). The
  header now reads "Free Play > Main" like retail.
- Selected-row highlight works (see "Row handlers"). The blur behind the pause menu: see "Pause blur" below.
- Boot registration ported (`retail_menu_stage::boot_registration`, on Game.ScreenManagerReady like retail
  825D9450; research `.local/research/boot-enums.md`, main-checked path indices 28 / 30 / 31 / 32 / 33 / 46): the
  56 Apt keys as RegisterAptInputEnum(name, key INDEX) (retail registers the index, not the record's key code),
  FE_SOUND_<NAME> = record name for all fe records (from the install's audio manifest), SetPlatform "0",
  SetScreenSize 1280 / 720, SetScreenEdgeOffset 136, UpdatePALMode false. Audio.PlaySound from the movies plays
  the fe record. Open: the 191 challenge enums, RefreshSubtitle.
- Screen templates extracted (`skate-data` menu_tables `templates`, located by the record whose path is
  "main/core_menu.swf", walked both ways): TU3 133 records at 0x8302F1A8 (0 Ignore, 12 Background = FE_root,
  69 CrossBar, 90 Options, 131 TipsLoader; test `tu3_image_matches_the_documented_tables`); the disc build's table
  is at 0x82FD33E8. The stage registers all of them in order, so the pause menu's AS id is 669 like retail (600
  with tables extracted before this; setup re-extracts because `asset_pipeline/menu_tables.py` now requires
  templates).
- Sub-option tables: done (see "Sub-options").
- Label overrides (SignIn / sign out, ReplayEditor instant replay, PartyPlay literal) and the online team pending
  path are not ported. GetProgressionData reads context facts (`progression_*`), -1 (panel hidden) by default.
- Hooking CoreMenu and the handler into the stage, the Game natives, drawing, input and the FE screen stack are next.

## Pause blur: the world behind FE screens (2026-10-10)

### Problem
Retail blurs and darkens the world behind the pause menu; our dev view drew the menu over a sharp world, and how
retail switches the blur on for the pause menu was not known (`pause-backdrop.md`, `pause-blur.md`).

### Evidence (TU3 recomp addresses; research `.local/research/pause-blur-link.md`, all main-checked)
- [code] Pause open (sub_825EB998) queues a push of FE id 56 on the FE screen stack. The stack's push / replace /
  pop handlers (sub_82D09B00 .. sub_82D09DE0) call vt+28 with byte +4 of the top screen's record in the info table
  at [stack+148]; pop applies the new top's byte, an empty stack applies 0.
- [code] vt+28 of the stack vtable 0x823048E0 (pointer at 0x823048FC) = sub_825EBED8: stores the flag at FE manager
  +461 and, on an off -> on change only, resets the tint (+476 / +480 / +484) to 255 and the strength (+468 / +472)
  to 0.0.
- [data] The info table: 0x83026088, 72 records of 24 bytes (stack init sub_825E8C40: `li r8,72; addi r7,r9,24712`,
  `stw r8,132(r25)`). Byte +4 is set for 32 screens, including 56 (the pause crossbar) and 69. The 64-bit value at
  +16 is the same per screen in every build (the disc build has 70 records, TU3 inserts two at ids 31 / 32; equal
  to TU3's key by key, test `disc_executable_gives_the_same_tables`), but four keys are shared (TU3 ids 0 / 32 / 49,
  5 / 6 / 7 / 8 / 22, 9 / 10, 12 / 13; research `.local/research/fe-stack-port.md`, main-checked in both builds).
  Our screen id is the key, plus `:n` (position among the records sharing it) when shared; a shared-key id can
  differ between builds (TU3 49 = `:2`, disc 47 = `:1`), setup always extracts the disc's.
- [code + data] FE manager tick sub_825D8E68: while on, tint -15 per tick down to (90, 85, 81) (a global flag at
  [0x83083C38]+32764 picks (30, 30, 30) instead) and strength +0.8 (0x8208824C) up to 8.0 (0x82099250); while off,
  tint +15 back to 255 and strength -0.8 to 0. A second byte (+462) gives a blur-only ramp while off (not used by
  the pause path, not ported).

### Change
- `skate-core/src/fe_blur.rs` (new): `FeBlur` (set_enabled = sub_825EBED8, tick = sub_825D8E68 with retail's
  clamps, `advance` at the console's 30 Hz cadence so any frame rate gives the same ramp), `FeBlurParams::RETAIL`.
- `skate-data` menu_tables: `screens` (`FeScreen { id, key, blur }`), located by the stack init's code pattern (works on
  both builds); setup re-extracts the tables because `menu_tables.py` now requires `screens`.
- `skate-game/src/fe_backdrop.rs` + `fe_backdrop.wgsl` (new): the FE stack's top screen key decides on / off
  (retail flag, then mod overrides, newest last); a two-pass blur + tint on the gameplay camera after the retail
  tone pass and before the UI pass, so the menu stays sharp and the exposure meter sees the undimmed world. The dev
  view pushes the pause screen (id `b4b7976b40b3e837`, TU3 id 56) while it is open.
- Mods: `sdk.menus.backdrop(screen_id, {blur, strength, step, tint={r,g,b}, tint_step})`, nil options drops the
  mod's override, removed when the mod stops; `sdk.menus.info().screens` lists every id with its retail flag.

### NOT RETAIL YET
- The blur kernel: retail's blur pass (sub_827F7270, renderer +3292 / pass bit 0x40) is not ported; ours is a
  17-tap gaussian whose radius is the strength in pixels at 720 lines (assumed). The tint multiplies the displayed
  colour (tint alpha is 0.0 in retail; assumed multiply).
- The tick rate of the FE manager is not measured (assumed 30 Hz: about 0.37 s to full tint).
- The alternative (30, 30, 30) floor and the +462 blur-only mode are not ported.

### Verification
- `cargo test -p skate-core fe_blur`: the ramp step by step against the clamps above, restart only on off -> on,
  the same result at 30 and 120 fps.
- `cargo test -p skate-data --test menu_tables -- --ignored`: the TU3 table address, the 32 blur ids, the pause key
  at id 56; the disc table equals TU3's minus ids 31 / 32.
- `cargo test -p skate-mods`, `cargo test -p skate-game fe_backdrop`: command validation, retail flag then the
  newest mod wins, cleanup per mod.

## Row handlers: retail pause rows open our features (2026-10-10)

### Problem
Every retail pause row was greyed: the registry enables a row only when someone handles it, and nothing did.

### Change
- `retail_menus.rs`: `OVERLAY_ITEMS` registers engine handlers (owner `overlay:<SECTION>`) for ChallengeMap -> MAPS,
  SkateProfileOffline -> SKATER, GameSettings -> GRAPHICS, Extras -> EXTRAS. Retail opens its own FE screens
  there (ChallengeMap 17, GameSettings 45, Extras 18); until those are ported, the overlay menu section with the
  same job opens. NOT RETAIL YET by design (a bridge).
- `retail_menu_stage.rs`: a crossbar select goes through `RetailMenus::act`; an `overlay:` outcome closes the
  retail pause screen and opens that section (`graphics_menu::Menu::open_section`); a mod's handler gets its
  `menu_select` event as before. Mods enable or take over any row with `sdk.menus.handle`.

### Verification
- `cargo test -p skate-game retail_menus`: ChallengeMap is enabled and forwards to `overlay:MAPS`.
- Screenshot `.local/research/menus-m6/rows.png`: Challenge Map enabled and selected with retail's highlight
  (large icon, help line "Access challenges and locations"); the highlight needed no extra port.

## Screen stack (2026-10-10)

### Problem
The dev view opened and closed the pause movie directly; retail runs every FE screen through a two-stack screen
manager whose per-screen records also suspend gameplay, show the chyron and set the HUD.

### Evidence (TU3 recomp; research `.local/research/fe-stack-port.md`)
- [code, main-checked] Push sub_82D09C58: closes the old top (sub_82D09848) or hides it (screen vt+28) by the
  request's keep-open byte, appends, screen vt+48, opens (sub_82D09998), screen vt+44(param), then the blur. The
  queue runner sub_82D0A2C0 waits while the top's state is not 0 / 3 and dispatches ops 1 pop, 2 push, 3 replace,
  4 pop to id, 5 close id.
- [code, agent, not re-read by main] Op 5 below the top unwinds and reopens the id; pop shows a kept-open screen
  at once; the overlay queue runs only when the main one ran nothing; input goes to the main top only, not while
  main requests are pending. Record fields: +0 suspends gameplay (context 3 pushed on open, removed on close),
  +1 chyron, +8 HUD mode (0 on, 1 off, 2 online HUDs only, 3 keep), +12 leftover input to the movie.
- [data, main-checked] Field counts in TU3: +0 set for 52 screens, +1 for 19, +12 for 62, +8 = 1 for 64; the
  disc build has the same records minus TU3's ids 31 / 32 (test `disc_executable_gives_the_same_tables`).

### Change
- `skate-core/src/fe_stack.rs` (new): `FeStack` (main + overlay stacks, FIFO queues, the five ops, the wait rule,
  `input_target`), `ScreenInfo`, `Effect` for the host (open / close / show, gameplay suspend, chyron, blur, HUD,
  focus). Serialisable plain requests, screens by this build's index.
- `menu_tables`: `FeScreen` carries all record fields (`suspends_gameplay`, `chyron`, `hud`, `input_to_movie`).
- `retail_menu_stage.rs`: F8 pushes the pause screen (retail Start, plain push) or closes it (op 5); the crossbar's
  back and an engine row close it through the stack; input only while the stack says the pause screen has it.
  Gameplay suspend sets `graphics_menu::Menu::fe_suspended`, which `gameplay_active`, the time pause and the
  game audio mute already follow (multiplayer sessions never pause, as with the overlay). The menu and the blur
  run on real time.
- NOT RETAIL YET: HUD modes and the chyron are not hooked up; only the pause screen has a movie (other screens log
  "no template yet"); retail's focus filter for screen 30 is not ported.

### Verification
- `cargo test -p skate-core fe_stack`: pause push / close, the plain push closing the screen below and the pop
  reopening it, kept-open hide / show, close below the top, the overlay queue order, input gating.
- `cargo test -p skate-data --test menu_tables -- --ignored`: record fields and counts, disc vs TU3.
- Screenshot `.local/research/menus-m6/stack.png`: the pause menu through the stack, the world blurred and tinted,
  gameplay suspended (physics 0.01 ms), the selected row's glow animating.

## Pad input (2026-10-10)

### Problem
The dev view read four keyboard keys; retail turns pad input into Apt key indices through its input map and hands
them to the top screen's C++ handler and then its movie.

### Evidence (research `.local/research/fe-input.md`)
- [data, main-checked] The install's stock `input.cfg` defines all 56 Apt keys of the extracted key table
  (`AptStart = Start.pressed;`, `AptUp = LStickU.repeat || DPadU.repeat;`, `AptUpReleased = ...released`).
- [code, agent] The FE poll sub_825DFDC8 evaluates the 56 keys in table order for every connected controller and
  calls the stack's input handler sub_825EC0E8 with the key index; that offers AptStart to the pause opener
  sub_825EB998, then the main top's C++ handler (crossbar sub_8261B560, which consumes only its close branch),
  then the movie's `ScreenManager.UpdateInput(id, key, controller)` when the screen's record +12 is set.

### Change
- `skate-core/src/fe_input.rs` (new): `parse_apt_keys` (Apt expressions over the 24 native slots, key-table
  order), `FeInput` (held / pressed / released / repeat edges per slot), `RepeatRule` (data), `fired`.
- `retail_menu_stage.rs`: every controller through `xbox::convert` (the gameplay conversion) plus the keyboard on
  controller 0's slots; AptStart opens / closes the pause screen; other keys go to the crossbar handler, then to
  UpdateInput unless consumed; input only while the stack gives the pause screen input.
  `SKATE3_RETAIL_MENUS_KEYS` injects key indices for screenshot checks. With the dev view on, pad Start no longer
  opens the overlay menu, and the overlay ignores Escape / Start while a retail screen holds gameplay.
- Repeat (updated the same day): retail's channel update sub_82966F30 (research `input-repeat.md` pass 2,
  constants main-checked): held above 0.5, 3 settle ticks after a change, repeat with the press, after 24 ticks,
  then every 12. NOT RETAIL YET: the tick rate is assumed to be the console's 30 Hz (0.8 s, then 0.4 s). Start
  closing the open pause menu is unverified.

### Verification
- `cargo test -p skate-core fe_input`: parsing in key order, unknown slots rejected, press / repeat / release,
  threshold, pressed keys do not repeat.
- Screenshot `.local/research/menus-m6/input.png` (`SKATE3_RETAIL_MENUS_KEYS=13`): one AptDown skips the greyed
  rows to skate.Profile ("View your career profile"); the movie's UpdateInput after the handler changes nothing
  else.

## Game Settings screen (2026-10-10, in progress)

### Problem
The crossbar's Game Settings row opened our overlay menu; retail pushes FE screen 45, whose init opens template 90
"Options" (options.apt) and answers its `Game.GameSettings_*` / `Game.OnMenu*` natives from the settings tables.
options.apt also needs APT features the pause menu never used (attachMovie, component classes, Math).

### Evidence
- [data, main-checked] Screen titles: 13 string ids at TU3 0x83026E90 (0 / 1 ID_GAMESETTINGS_TITLE, 2 _DIFFICULTY_,
  3 / 12 _AUDIO_, 4..8 _VIDEO_, 9 _CONTROL_, 10 _ONLINE_, 11 _SKATEFEED_SETTINGS); the disc build has the same.
  FE 45's info record key `0ca9bfe5c1ba4995` (unique); its record suspends gameplay, shows the chyron, blurs.
- [code, research `.local/research/settings-host-map.md`, `settings-screen-boot.md`] The natives, the loaded
  handler (SetScreenState 1), the begin-close handler (SetScreenState 2 outside FE state 2), Back (sub-screen ->
  main list, else close), Left / Right (sub_8260C258: step the row, SetState 50), select (sub_8260D290: links set
  the screen index and rebuild).
- [data, research `.local/research/apt-components.md`] main.apt's root defines RegisterComponentClasses and
  IncreaseScreenDepth as named functions on `_level0` and calls them by name (APT 0xb0); each control library
  registers its component class with `Object.registerClass("<Linkage>", class)`; highlight_option's constructor
  sets the default title "#TBD TITLE" and SetTitle forwards to its HighlightText child.

### Change
- `menu_tables`: `settings_titles` (located by two pointers to ID_GAMESETTINGS_TITLE).
- `settings_host.rs` (new): the GameSettings natives from the registry over a row snapshot taken at
  GetNumOptions; OnMenuSelect is queued for the host. Value descriptions: the Difficulty row only, from its value
  label (EASY -> DESCRIPTION_EASY ..., as retail's index table). NOT RETAIL (guard): when no row is enabled the
  first one counts as enabled, because options.apt's NextValueCalc recurses forever otherwise (mods can do that).
- `retail_menus.rs`: GameSettings now pushes FE screen 45 (`SCREEN_ITEMS`, owner `screen:<id>`) instead of the
  overlay section.
- `retail_menu_stage.rs`: opens / closes either ported screen by template (CrossBar 669 / Options 690 in TU3;
  the disc build numbers its 129 templates differently); settings input (Back, Left / Right) as sub_8260C258,
  steps and selects dispatched through the registry, then SetState(50) / SetState(1).
  NOT RETAIL YET: the per-row UI sounds (core_nav_left / right, core_a_button, core_b_button stand in).
- APT VM / stage (standard AS2 semantics, each marked NOT RETAIL YET in code): `attachMovie` (script depths above
  the timeline, kept across seeks, the subtree's class constructors run before it returns, nested clips first),
  `removeMovieClip` / `getDepth` / `getNextHighestDepth` for attached clips, call-by-name through the scope chain
  (locals, the timeline, then _global), `Math`, `TextFormat` (a record; setTextFormat not applied), `textHeight`,
  clip `_width` / `_height` from frame-0 bounds (clip-depth children left out like retail's bounds walk
  sub_82E59C28), library linkage names carried with imported copies (nested component classes).
- `SKATE3_RETAIL_MENUS_TRACE=1` logs the host calls the movies made (answered and unanswered).

### Verification
- `cargo test -p skate-game settings_host retail_options_screen_builds_rows` (the latter data-gated on the menu
  movie set): the natives in options.apt's order; the movie builds six rows from the registry and the first row
  reads ID_GAMESETTINGS_DIFFICULTY_SETTINGS.
- `cargo test -p skate-data --test menu_tables -- --ignored`: title table address and values, disc = TU3.
- Screenshot `.local/research/menus-m6/settings.png` (`SKATE3_RETAIL_MENUS_KEYS=15,15,15,2`): Game Settings with
  Difficulty / Audio / Video / Control / Online / skate.Feed Settings, the first row selected.

### Panel, sub-screens and colour (same day)
- [data, research `.local/research/menu-white-squares.md`, main-checked] The squares were not gradients: panel.apt
  stretches 16 x 16 colour swatches (panelTopFill, panelCoreFill, ...) into its bands with `_xscale` / `_yscale`
  in BuildPanel, which our APT player ignored; and button_item2's `mButtonRender` is a placeholder for a native
  component (`_type = "CustomButton"`, `_test` = the button), drawn by the player.
- APT: `_xscale` / `_yscale` / `_rotation` seeded from the placement, script writes (and `_width` / `_height` on
  non-text clips, through their bounds) replace the matrix's linear part (`apt_movie::clip_matrix`, both matrix
  builders). The front-end stage host applies it; the gameplay HUD host does not yet (unchanged).
- removeMovieClip on attached clips reaches the movie (DeleteOptions on a sub-screen rebuild); test: screen 2.
- Colour: the menus apply colour transforms in gamma space (hud_render.wgsl mode 1): the console front end works
  on display values, and a recomp shot shows a near-black panel body where ours was mid grey (solid 0.145
  encoded twice). The gameplay HUD uses the same path and the scale support (user: "follow retail"; one APT player
  in retail). Checked: the line HUD's ring at 2 s is identical before / after (textured, identity transform);
  tinted / solid HUD elements now show their display colours (to look at in play).
- In game (`SKATE3_RETAIL_MENUS_KEYS=15,15,15,2,2,15`): A on Difficulty Settings opens the Difficulty screen
  ("Play Mode < Normal >", "Classic Skate Gameplay" description); Right steps the difficulty through the engine
  binding.

### HUD mode and chyron (same day)
- HUD mode (+8): the stack's Hud effect sets `fe_backdrop::FeHud`; the scoring HUD draws nothing and the online
  roster hides while the top screen's mode is "off" (the pause menu, the settings screen); an empty stack means
  "on" (retail's gameplay screen, FE id 0 mode 0, sits at the bottom; ours has no screen there yet).
- Chyron (+1): main.apt's layer movies are now exported (menu movie set 5: chyron, subtitle, fmv_popup,
  fadebackground, autosave; setup re-exports); the Chyron effect calls `_global.Chyron_SetVisible` on a change.
  NOT RETAIL YET: its text (Chyron_Display from the C++) is not driven, so nothing shows yet; PlayOutro not used.

### Open

## Button prompts (2026-10-10)

### Problem
Retail draws button glyphs through a native "CustomButton" component (Xbox 360 art). On PC the prompt must show
the connected pad's own button (user: the glyphs "should be replaced by SDL's appropriate button replacement
prompt"; size and placement as retail).

### Change
- `input/controller_kind.rs`: `PromptLabel`; `ControllerKind::face_labels` = SDL's labels of the South / East /
  West / North buttons for the pad type (`SDL_GetGamepadButtonLabelForType`, `input/platform.rs`), else the
  family's prompt style (XInput, unknown types).
- `apt_scene.rs`: a CustomButton clip with a face button in `_test` draws one quad over its placeholder's bounds
  (retail's box and placement, laid out by button_item2's own script) with texture `prompt/<button>`; other
  native components draw nothing.
- `prompt_glyphs.rs` (new): every retail button name (retail's list, TU3 0x82204614: A / B / X / Y, Back, Start,
  L/R_Bumper, L/R_Trigger, L3/R3_Click, Stick_Left / Stick_Right with _Up / _Down / _L / _R, DPad with
  _Up / _Down / _Left / _Right) has an equivalent on every family (user: "every console should have equivalent
  buttons for every entry"):

  | retail | Xbox 360 | Xbox One / Series | PS3 | PS4 | PS5 | Nintendo |
  |---|---|---|---|---|---|---|
  | A / B / X / Y | SDL label | SDL label | SDL label | SDL label | SDL label | SDL label (South = B) |
  | L/R_Bumper | LB / RB | LB / RB | L1 / R1 | L1 / R1 | L1 / R1 | L / R |
  | L/R_Trigger | LT / RT | LT / RT | L2 / R2 | L2 / R2 | L2 / R2 | ZL / ZR |
  | Start | START | MENU | START | OPTIONS | OPTIONS | + |
  | Back | BACK | VIEW | SELECT | SHARE | CREATE | - |
  | L3/R3_Click | LS / RS | LS / RS | L3 / R3 | L3 / R3 | L3 / R3 | LS / RS |
  | Stick_*, DPad_* | stick / cross with the direction marked, every family | | | | | |

  Face looks: Xbox coloured balls (retail's), PlayStation dark buttons with coloured symbols, Nintendo dark with
  light letters; named buttons a dark pill with the name. Generated (a small stroke font), no console art; sized
  to retail's measured glyph (0.83 of the box; a recomp shot, `.local/research/menus-m6/retail/`). The menu
  renderer resolves `prompt/<button>` for the first connected pad (`resolve_for`), Xbox 360 without one.
  `cargo test -p skate-game prompt_sheet -- --ignored` with `SKATE3_PROMPT_SHEET=<folder>` writes a sheet.

### Verification
- `cargo test -p skate-game prompt_glyphs controller_kind`: glyph size / colours, SDL-less fallbacks (PS5 ->
  Cross..., Switch South -> B).
- Screenshot `.local/research/menus-m6/settings.png`: A / B prompts beside Select / Back like the retail shot.


## Sub-options (2026-10-10)

### Problem
GetSubOptionName answered "#" and the sub-option counts were empty, so no crossbar row could open its sub column
(Enter Skate Park, Skate Reel, On-Screen Help, ...).

### Evidence (research `.local/research/menu-subtables.md`, `coremenu-host.md`; main-checked the table contents)
- [data] Label table TU3 0x83027958 and helper table right after it (0x83027A00): 7 kinds x 6 string pointers.
- [code] GetNumSubOptions sub_8261DB48: non-null labels of the kind's row; park kind minus one unless global
  [0x830B7AE8] == 1; online kinds by online state. GetSubOptionName sub_8261DE88: the label by (kind, sub), with
  on-screen help naming the action (OFF when on). Level-2 helper text from the helper table (sub_8261E270).

### Change
- `menu_tables`: `sub_options` (`SubOptionRow { labels, helpers }`, located by the pointer to ID_CROSSBAR_NEW_PARK);
  setup re-extracts (menu_tables.py requires it).
- `crossbar_host.rs`: `sub_option_counts` (the count rule; facts `crossbar.park_tutorial`,
  `crossbar.trick_analyzer`, `crossbar.manual_meter` for the flags), GetSubOptionName, level-2 helpers; the stage
  builds the crossbar view with these counts. NOT PORTED: the online kinds' rules (offline counts: contacts 3,
  online team 3, party 1) and label swaps.

### Verification
- `cargo test -p skate-data --test menu_tables -- --ignored`: TU3 table address and rows, disc 7 rows.
- `cargo test -p skate-game crossbar_host`: counts (park tutorial flag), names, out-of-range "#", level-2 helper.

## PC overlay on the Guide button (2026-10-11)

### Decision
User: "maybe we should create a pc overlay like the recomp instead", "on the guide button press". PC-only features
(Mods, Updates, custom models, day & night, local testing, network debug, mod sections, custom maps, PC video /
audio / control settings, Quit) live in a PC overlay like the recomp's settings overlay (rexglue
`simple_settings_overlay`: a fullscreen console-style screen, category rail Video / Controls / Audio / Profile /
System, opened by its own binding). The retail menus stay retail: Start opens the retail pause menu; retail's
Extras keeps its own rows.

### Change
- `input/platform.rs`: SDL's Guide button reads as bit 0x0400 (XInputGetStateEx's place; retail's 24-slot
  conversion never reads it; not a bindable name for paddles).
- `graphics_menu.rs`: Guide, F1 or Escape toggle the overlay from anywhere, also over the retail menus; pad Start
  opens it only while the retail menus are off. `retail_menu_stage.rs`: no retail key handling while the overlay
  is open.
- Open: the XInput fallback backend has no Guide (only the SDL path); Windows Game Bar may take the Xbox Guide
  press first (its own setting).

### The recomp's look and navigation (2026-10-11)
User: "faithfully recreate the recomps guide menu and replace our guide menu". The overlay's view is now a port of
the recomp's settings overlay (skate3recomp's rexglue-sdk, `src/ui/overlay/simple_settings_overlay.cpp`,
`SimpleSettingsDialog::OnDraw` / `GatherInput`, at `third_party/rexglue-sdk` commit ae6ea8e); the rows stay ours.
- Look: the recomp's palette, metrics (authored for 1080p, `s = clamp(h / 1080, 0.6, 3) * 0.92`, legend at the
  base scale), column layout (300 s rail and description, content clamped to 300 to 800 s, the block centred, a
  whole number of 52 s row slots, at most 12), title "Settings" (sub-pages show their own name), lime section bars,
  white rows, magenta steppers that grey out at the ends, magenta sliders, the black focus fill with a 2 s lime ring
  and a 4 s black edge outside it, the teal rail with Close Game pinned under it, the Description panel, and the
  pad / keyboard legend that follows the last input. Font: Inter Regular / SemiBold (SIL OFL, subset to Latin,
  `crates/skate-game/src/fonts/`, from Inter 4.1), the recomp's typeface; sizes converted from ImGui line heights
  to Bevy ems (2048 / 2478).
- Motion: the focus fill slides at the recomp's rate (`min(1, dt * 22)`, snapping beyond 160 s), the list scroll
  locks to whole rows and slides at `dt * 16`, focused text styling waits until the fill mostly covers the row, held
  directions repeat after 0.42 s every 0.11 s, the wheel steps whole rows.
- Navigation: focus zones rail / rows; Up / Down on the rail switch category, Right or Select enters the rows;
  Left / Right step (clamped), Select cycles with wrap or runs the row; B / Escape back out one level (text edit,
  sub-page, rows, rail, closed); F1 / Guide close at once; LB / RB or Q / E (and Tab) change category; Y / R reset
  the row to its default; X / F apply pending custom tuning (the recomp's Apply & Restart slot; its chip reads
  "TUNING NOT APPLIED"). The mouse hovers rows into focus, clicks the rail, steppers, rows and slider tracks.
- Backdrop: the recomp blurs the frame (gaussian sigma 9 px at 1080p, eased with a 60 ms time constant) under a
  `(6, 9, 11, 133)` wash. Ours uses `fe_backdrop`'s pass at strength 12 (= sigma 9 at 1080p for that shader) with the
  same easing, and the wash as the overlay's background. The gameplay HUD is hidden while the overlay shows (the
  recomp's HUD is part of the blurred frame; ours draws above the blur pass).
- Deviations, not the recomp's: description text is left-aligned (the recomp justifies only when gaps stay under
  1.6 spaces; Bevy's justify has no such limit and opened wide gaps); value text clips instead of shrinking then
  ellipsizing; hover needs 2 px of movement since the last hover instead of per frame; Reset to Default shows in
  the legend only on rows that have a default; Close Game replaces the Extras "Quit game" row.
- NPC Draw Distance (Graphics > Living World; QoL, not retail): the living world's row (None, Retail, 1.5x, 2x,
  3x), saved in `graphics.json` as `npc_draw_distance`. Here it is a placeholder: the overlay keeps the choice in
  the `NpcDrawDistance` resource, which nothing reads until the living world (`world/living-world`) lands and
  applies it; its description says so. On `hails-additions` the row drives the living world directly.
- Moddability: mod menu sections still become rail categories (`custom_sections`), their entries rows.
- Tests: `cargo test -p skate-game graphics_menu` (keys, back steps, stepper ends, header placement, NPC draw distance setting, repeat timing,
  1080p layout, easing).
- Open: the user's play test of the motion.

## Swap-in: the retail menus are the default (2026-10-11)

### Change
- `retail_menu_stage.rs`: the stage runs unless `SKATE3_RETAIL_MENUS=0` (was: only with `=1`). Start (and F8) open
  the retail pause menu; Game Settings opens the retail settings screen; its sub-screens step the bound rows.
- `RetailPause` (resource): true while the stage loaded and the menu tables carry the FE screen records. When it is
  false (tables extracted before those records, missing movies, a failed stage, or the menus off), Start opens the
  PC overlay instead, the fallback, with one log line: `the menu tables have no FE screens yet (run setup); Start
  opens the PC overlay until then`. `graphics_menu::overlay_key` takes that flag instead of the env switch.
- The PC overlay keeps Guide / F1 / Escape in both cases.

### Verification
- Default (no env flags, fresh tables): `SKATE3_RETAIL_MENUS_OPEN=1` opens the retail pause menu ("Free Play >
  Main", Challenge Map selected).
- Fallback (the dev install's tables from before the FE screen records): the one warning above, no retail screen.
- Idle cost (gameplay, menu closed, 1280 x 800, frame counter at 14 s): on 2.74 / 2.10 / 2.08 ms, off 2.12 / 2.13
  / 2.08 ms; the first "on" run is the only outlier, the other two match "off" to 0.02 ms.
- `cargo test -p skate-game`: 508 passed, the known `pipelines_accept_valid_group_outputs_when_fingerprint_changes`
  failure.
- Open: the user's play test of the retail menus as the default.

## Stored settings: every rule row steps and saves (2026-10-11)

User decision: "Store, then features": first make every settings row work as a setting (steps, saves, readable),
then port each row's effect. Rows that save but have no effect yet are listed below.

### Change
- `retail_menus.rs`: `StoredSettings` (`settings/game-settings.json`, string id -> number). Every settings row with a
  retail rule and no engine binding (`BINDINGS`) gets an `Owner::Engine("settings")` value layer: the saved value,
  else retail's default. That enables the row; Left / Right step it through retail's rule; a change is saved. A
  mod's value layers on top and is never saved (same as the engine bindings). `retail_menus::setting(menus, id)`
  gives features the value in force.
- Effects so far (research `.local/research/settings-consumers.md`; main-checked the parts marked so):
  - HUD +164 [code]: retail `sub_825DA2A0` shows / hides the HUD clips trickdisplay, minimap, central and
    trickanalyzer (manualmeter and online_chyron in the HUD setup `sub_825D99C8`; homscoring always on). Our
    gameplay HUD draws only trickdisplay, so Off hides it (`scoring_hud.rs`); the other clips follow when they are
    ported.
  - Offboard X / Y axis +193 / +192 [code, main-checked]: `sub_8252FEB0` packs them into the camera config
    `[0x83085450]` +24 / +25 (Transparency +195 into +26); the offboard camera `82DF69C0` negates the look stick's
    x when +24 is set and y when +25 is set. Ours: `camera::angle::sync_look_invert` sets
    `CameraRuntime::invert_look`, which the existing publication applies to `look_552_556` (the right stick).
    Test `look_invert_follows_the_axis_rows`. Not play-tested.
- Vibration +163 (2026-10-11): rumble output ported (research `.local/research/rumble.md`; constants main-checked
  in the TU3 image, the tick and device curve main-checked in `sub_82695010` / `sub_8296D480`):
  - `skate_core::rumble`: per pad, channels {low, high, decay}; a start overwrites its channel with low / high
    clamped to 0..1 and decay = clamp(1 - rate / 60) (`sub_82694D38`, only it checks the vibration flag, so
    switching off lets running rumble fade); each tick sums the channels per motor, caps the sum at
    max + (1 - max) * 0.2, clamps to 0..1, then decays each channel and drops it once both motors are below 0.001
    (`sub_82695010`); the device curve (`sub_8296D480`): gamepad motor 0 = 1.2 l - 0.02, motor 1 = 1.05 l + 0.05,
    other devices 0.7 l - 0.02 / 0.7 l, clamp, exactly 0 stays 0, else trunc(v * 65535). As in retail, a gamepad's
    motor 1 at level 0 gets 3276 (0.05). Values in `RumbleTuning::RETAIL`.
  - `rumble.rs` (game): ticked at 60 Hz of real time whatever the frame rate; the Vibration row (value in force)
    gates starts; switching it on plays retail's test rumble (`sub_8260DF40`: channel 0, 0.5 / 0.5, rate 5.0) on
    the active pad; output through `input::platform::set_rumble` (SDL `SDL_RumbleGamepad`, re-sent every 0.5 s
    for a 1 s duration while non-zero; XInput `XInputSetState` on change). NOT RETAIL: Xbox-family pads get the
    gamepad curve, other pads the other-device curve (retail's known-device test is the 360 pad's caps; on a
    DualSense the 0.05 floor would be a constant buzz).
  - Mods: `sdk.input.rumble` / `stop_rumble` (capability `input_rumble`; per-mod channels, stopped with the mod).
  - NOT PORTED: retail's gameplay rumble events (the posters `sub_82BB8CF0`, 0.5 / 0.5 / rate 1.2, local player only,
    and `sub_8259E3D0`, per-item channels from `sub_8259DFB0`; which events they belong to is not proven; a recomp
    trace hook on `sub_82694D38` logging caller, channel and the three floats would settle it).
  - Tests: `cargo test -p skate-core rumble`, `cargo test -p skate-game rumble` (60 Hz at any frame rate, the
    switch-on test rumble, off blocks starts, mod channels separate and stopped with their mod). Not felt on a pad.
- Saved, no effect yet (what retail does, from the research, not main-checked): Minimap +158 (`sub_82665278`:
  hidden with the HUD off, forced on / off in some game states; we draw no minimap), Subtitles +157 (six readers
  gate subtitles on it or a forced flag; no subtitle text in the engine), Cameraman Mode +165 and HOM mode +184 (readers found, effect undecoded), Units,
  Transparency (no reader of the copied byte found), the Skate Feed toggles.
- Still greyed (no retail rule decoded): Video Calibration (opens a calibration screen), SFX Pack (hidden with one
  pack), Auto sign-in (an online popup).

### Verification
- `cargo test -p skate-game stored_rows_are_enabled_step_save_and_reload_and_mod_values_are_not_saved`: a stored row
  is enabled with retail's default; nothing is written until a change; Right saves Off; a mod's value shows and is
  not saved; a new session loads Off; engine-bound rows are not stored.
- In game (fresh tables, `SKATE3_RETAIL_MENUS_KEYS=15,15,15,2,13,13,2,13,13,13,13,15`): Video Settings, HUD selected
  and stepped to Off; log `ID_GAMESETTINGS_HUD_TOGGLE = Int(0) (saved)`, the file holds `{"ID_GAMESETTINGS_HUD_TOGGLE": 0}`.

## Session logging (2026-10-11)

User: "make sure that the new changes in living world and the retail menu have proper logging so you can review my
sessions/logs". Every line is `info!`, so `PLAY-WITH-LOG.bat` sessions carry them; grep by prefix:
- `RETAIL_MENUS key <AptKey> pad <n> screen <id>`: every key the retail menus took (retail's Apt key names);
  `RETAIL_MENUS open / close pause menu`, `open / close Game Settings` (FE ids), `gameplay suspended / resumed`,
  `settings back to the main list from screen <n>`, `select <item>: <outcome>`, `settings step <id> <dir>: <outcome>`,
  `<setting id> = <value> (saved)`, the fallback warning `no FE screens yet (run setup)`.
- `PC_OVERLAY open / close by <Guide | F1 | Escape | Start>`, `back by <Escape | B>`, `at <category> / <page>`,
  `<Step | Activate | Reset>(row) "<label>" was "<value>" (section, page)`, `now "<label>" = "<value>"`,
  `status: <message>` (saves and errors).
- `RUMBLE Vibration on / off`, `settings test rumble on pad <n>`, `mod <id> pad <n> channel <c>: low high rate`
  (and "not started" while Vibration is off), `mod <id> stopped`, `pad <n> rumbling / idle (<curve>)`.
- Already there: `Offboard look invert: x y`, `Camera angle: <label>`, `PC overlay toggled by the Guide button`.
- Checked in a scripted run (keys 15,15,15,2,...): Vibration on, AptStart, gameplay suspended, open pause menu,
  three AptRight, AptNext, select, close pause menu, gameplay resumed / suspended, open Game Settings.

## Out-of-date game files prompt (2026-10-11)

User: "add a prompt to the new guide overlay that prompts users to refresh their assets if they are out of date".

### Problem
A release lists the asset converter versions it expects (`release.json` `asset_pipelines`). When a converter changed
but the group's recorded outputs were still present, `setup::asset_root` accepted the old files and stamped the new
versions as installed, so nothing ever asked for a refresh. That is how an install kept menu tables from before the
FE screen records (Start then falls back to the PC overlay).

### Change
- `setup.rs`: `outdated_groups` lists the groups with no exact or equivalence match at startup (`stale_groups()`,
  logged as `SETUP: game files out of date for <groups>`); `stamp_pipelines` keeps those groups' old fingerprints, so
  every start asks again until a refresh. `--refresh-assets` makes a running copy go through setup's refresh;
  `restart_with_refresh` starts the exe again with it (needs `support/skate3setup.exe`, the release package's).
- PC overlay: while groups are stale, the title chip reads "GAME FILES OUT OF DATE", every description ends with
  "Your game files are out of date: Extras > Refresh Game Files updates them.", and Extras gets a "Game Files"
  section with **Refresh Game Files** (describes the stale groups by name; Select restarts into setup, logged
  `PC_OVERLAY refresh game files: restarting into setup`; without a setup helper the status says so).
- User: "it shouldnt trust a flag to tell if assets are stale, write a lightweight checker that runs on each launch
  to test asset staleness". `asset_check.rs` runs on a background thread at every start and reads the files
  themselves: every output in each group's setup receipt (`installation.json` `outputs`) must exist at its recorded
  size (no hashing), and the HUD group must hold what this build reads (menu tables parse in this build's reader and
  carry the FE screen records, settings titles, sub-option tables and screen templates; the menu movie manifest is
  this build's format version and every file it lists exists). Each failure is logged `ASSET_CHECK <group>: <reason>`
  and marks the group stale; `ASSET_CHECK done in <ms>: game files current` otherwise (178 ms on the refreshed dev
  install). Groups other than HUD are checked by their receipts; content checks for them (for example the living
  world's data) are added with the features that need them. Before restarting into setup, Refresh Game Files writes
  "stale" as the version of every stale group in `installation.json`, so setup's refresh rebuilds exactly those.
  Tests `a_missing_or_resized_output_flags_its_group`, `hud_check_reads_the_files_not_a_stamp`.
- Development roots (`--assets`, `SKATE3_ASSETS`) have no `release.json`, so the version check never fires there;
  `SKATE_VERIFY_MENU=stale` previews it for screenshots.

### Verification
- `cargo test -p skate-game changed_converters_are_reported_and_keep_their_old_fingerprint_when_stamped`.
- Screenshot (`SKATE_VERIFY_MENU=stale`): chip, Game Files section, the row and both description lines.
- Open: the restart into setup is not run end to end (it needs a release package's setup helper).

## Refresh Game Files in development copies, and which files differ (2026-10-12)

### Problem
A Steam launcher copy of hails-additions showed GAME FILES OUT OF DATE (living world), and Extras > Refresh Game Files
only said "This copy has no setup helper". The log said `ASSET_CHECK livingworld: converted files missing or changed
size` without naming a file.

### Root cause
- The 19 car models in the dev installation had been re-exported straight into it during the car shader work
  (2026-10-11, 18:47), after setup's last receipt (15:22): each file 108 bytes larger than recorded. The game data was
  current; the receipt was not. Release copies are not affected (the converter change shows as a normal refresh offer).
- The refresh restart only knew the release layout (`support/skate3setup.exe` and `data/` beside the exe). Development
  copies run `bin/skate3rust.exe --assets <link>`, so there was no helper and the wrong data folder.

### Change
- `setup.rs`: with `--assets`, Refresh Game Files finds the installation behind the link (`<base>/installations/<id>/assets`),
  clears the stale groups' fingerprints there and restarts; the restarted copy runs setup's refresh (the packaged helper
  when present, else the checkout's `tools/setup.py` through `py -3.13`, the source the helper is built from) and points
  the link at the installation setup publishes. Only a link is replaced, never a real folder.
- `asset_check.rs`: the receipt finding names the first three files that differ and the count, e.g.
  `converted files missing or changed size (19 of 114: .../suv_01.glb (710100 bytes, recorded 709992), ... and 16 more)`.
- Launcher (`.local/steam`, not in the repo): a broken version link is re-pointed at the installation in
  `data/installation.json`, not at the main checkout's link (which can be stale as well).

### Verification
- `dev_refresh_finds_the_base_and_repoints_the_link` (Windows): the base behind a junction, re-pointing to a path joined
  with '/' (mklink read it as a switch before the fix), the old target kept, a real folder refused.
- `a_missing_or_resized_output_flags_its_group` checks the new reasons.
- The dev installation refreshed with setup's `refresh=True` (only `livingworld` changed): new installation, every
  group's receipt matches (core 2033, hud 27, character 60, environment 38, maps 26, audio 4416, livingworld 115 files).
- Not yet run end to end from the overlay button in a development copy.

## Opt-in features in the PC overlay (2026-10-11)

User: "anything opt in should be added as a setting to the guide overlay that can be toggled on and off if they arent already".

### Problem
Two player-facing opt-ins on this branch had no overlay row: the non-retail "more audible" world layout (only
`settings/audio.json` `"more_audible_world": true` by hand, or `SKATE_AUDIO_MORE_AUDIBLE=1`) and setup's speech
decode (`SKATE_SETUP_SPEECH=1` in the environment of a setup run, about 3.4 GB for the world, main cast and
announcer speech). Without the decode, ped speech is chosen and logged but silent.

### Change
- PC overlay, Graphics: a new "Opt-In Features" group after Living World (header 913) with **More Audible World**
  (row 32: Retail / On, saved in `settings/audio.json`, takes effect at the next start because the runtime picks
  its instance counts at launch; Y / R resets it to Retail) and **Ped Speech** (row 33: Off / On, saved as
  `decode_speech` in `settings/audio.json`). The living world branch adds its opt-ins (zombie mode, NPC skater
  physics, skitching, prop grab) to the same group where both branches meet (doc 26b "Opt-in switches in the PC
  overlay").
- Ped Speech on without decoded takes (`private/audio/speech/livingworld` empty or missing) marks the `audio`
  group stale, so Extras shows **Refresh Game Files** and the title chip says GAME FILES OUT OF DATE; the same
  check runs at every start while the choice is on (a cancelled refresh asks again). `setup::asset_root` passes
  `SKATE_SETUP_SPEECH=1` to the setup helper when the install's saved choice is on (logged
  `SETUP: speech decode asked for`). Off keeps the decoded takes; setup skips the decode from the next refresh.
- `SKATE_VERIFY_MENU=optin` opens the overlay on the group for screenshots.

### Verification
- `cargo test -p skate-game --bin skate3rust -- graphics_menu game_audio::tests setup`: all pass except the known
  upstream failure `pipelines_accept_valid_group_outputs_when_fingerprint_changes`. New test
  `speech_decode_choice_reaches_setup`.
- Screenshot on DownTown (`SKATE_VERIFY_MENU=optin`): the group header, both rows and the Ped Speech description.
- Found while taking it: with `--test-world` the overlay capture (`SKATE_VERIFY_MENU=optin` and the older `npc`)
  panics in wgpu ("fe backdrop" pipeline Rgba16Float vs the Rgba8UnormSrgb pass); on a real map it renders. Not
  caused by this change; open.
- Open: the refresh with the decode is not run end to end (it needs a release package's setup helper).

## Pause menu froze the game on the third open (2026-10-12)

### Problem
A hails-additions session (Steam launcher, trace-all) froze with the pause menu: the game stayed suspended and Start did
nothing, so the game had to be closed.

### Root cause
- The front-end stage never collected its VM objects. Every pause open loads the crossbar movie (about 1300 objects);
  closing it unloaded the level but the objects stayed, so the third open crossed the VM's 4096-object guard
  (`RETAIL_MENUS stage stopped: APT execution/object budget exceeded`).
- `Vm::collect` existed but nothing called it; and when called, the stage's table of script-made clips
  (`createEmptyMovieClip`) kept every closed screen alive through the clips' parent links.
- A stopped stage left gameplay suspended (`fe_suspended`) and the pause backdrop up, and Start is ignored while
  suspended: the freeze.

### Evidence
Session log `game-20261012-072425.stderr.log`: pause opened three times (two Challenge Map -> PC overlay round trips in
between), the third open fails 5 ms after `open pause menu (FE 54)`, physics tick stays at 392 to the end.

### Change
- `apt_stage.rs`: `Stage::collect` runs at the end of an update once the slots reach twice what survived the last
  collection (at least 1024). Roots: `_global`, the stage's prototypes, classes and pending loads, and every object id
  the loaded movies hold (root, instances and children, queued actions, classes, script-moved / attached / scaled
  clips). The script-made clip table is weak: entries go when their clip is collected.
- `apt_vm.rs`: `collect` returns the free slots; the object guard is 16384 (a runaway guard, not a retail limit).
- `retail_menu_stage.rs`: a stopped stage releases the gameplay suspension, the pause backdrop and the HUD mode once
  (warning `stage stopped with the pause menu open: gameplay resumed`); Start then opens the PC overlay.
- NOT RETAIL (mechanism): retail's APT player refcounts its objects; the collection only keeps the slots bounded and
  changes no script-visible behaviour (only unreachable objects are freed).

### Verification
- `cargo test -p skate-game pause_menu_reopens_without_growing` (data-gated on the exported menu set): 8 open/close
  cycles of the real crossbar through the retail screen manager; live objects after each cycle
  `[908, 908, 908, 908, 908, 908, 908, 908]`. Before the change it failed on the third open with the session's error.

### Open questions
- Challenge Map still opens the PC overlay's Maps section (the bridge in "Row handlers"); retail's Challenge Map
  screen (FE 17) with its locations and teleports is not ported.

## Settings text colours and the glow (2026-10-12)

### Problem
Play test: on Difficulty Settings the current value ("< Normal >") was unreadable, the Audio "100%" values were very
dim, and the selected row's glow was far too bright while it flickered.

### Root cause
- [data, code] The rows' own scripts set `TextField.textColor` per state in SetState (highlight_selector,
  highlight_slider, highlight_option, menu_part_text_hilite, main: selected value 0x37A5FF, rows on 0x70BBFF, off
  0x385570, disabled 0x555555). Our text draw never read it and used only the field's authored `color_argb` (dark
  navy #0F4670 for the selector value, grey #3E485C for the slider value, #70BBFF for every row title).
- [code, medium-high] The HUD / menu layer blended in linear space (target `Rgba8UnormSrgb`, shader output converted
  to linear). Retail's APT draw state is straight SRC_ALPHA / INV_SRC_ALPHA (`sub_827E4AF0`, bound by the draw
  `sub_825D4490`) on a display-value image (the 2D pass's format is inferred from the recomp's tone chain and frame
  surface, not traced). Linear blending lifts the selected row's many soft low-alpha glow layers.
- Research: `.local/research/menus-m6/glow-and-value-text.md` (main verified the textColor finding and the draw path).

### Change
- `apt_scene.rs`: a text draw takes the RGB of the field's script-set `textColor` when set, the alpha from the field.
- `scoring_hud.rs`, `hud_render.wgsl`, `hud_composite.wgsl`: the layer target is `Rgba8Unorm` holding display values;
  both shader modes output display values (mode 1 applies the colour transform to the texel's display value); the
  composite undoes the coverage, converts to linear and premultiplies again for the frame. The gameplay HUD shares the
  layer (one APT player in retail).
- Moddability: the colours stay in the movie scripts (mod movies and scripts use the same path); no Rust constants.

### Verification
- `retail_options_screen_builds_rows` now also asserts the script colours (0x385570 / 0x37A5FF) reach the draws; the
  full skate-game suite passes apart from the known upstream `pipelines_accept_valid_group_outputs_when_fingerprint_changes`.
- In game (local test build): the menus load (a first build put a helper between `@fragment` and the entry point and
  the composite shader failed to compile; fixed). The user's visual check of the glow and the gameplay HUD is pending.

### Open questions
- Prove the 2D pass's colour format with a short recomp trace at `sub_825D4490`.
- `apt_scene.rs` ignores `blend_mode`; retail also has an additive ONE/ONE object (`[0x830680AC]`, user not traced).
- Trick Guide (Learn tab): ported, see "Trick Guide".

## Trick Guide (2026-10-12)

### Problem
Pause menu > Learn > Trick Guide was greyed: no handler and no retail screen (play test 2026-10-12).

### Root cause
Not ported yet. Retail runs it as FE screen 14 (template 29 TrickTutorial, movie `screens/tricks/trickguide`); the
C++ screen object answers the movie's FETrickTutorial natives and its picker callbacks from the trick records.

### Evidence (TU3; main re-read every handler named here)
- Open: crossbar case 0x8261BF78 starts the fade hold `sub_825E6B30`; its callback 0x8261CDD8 opens FE screen 14
  through `sub_8261AF30` (the same path as Game Settings / FE 45). Screen 14's vtable is 0x823038B8; init 0x82618A40
  opens template 29.
- Lists (`sub_82619368`): level 0 = the 8 category keys at 0x830274B0 (static); level 1 = the category's RefSpec array
  0x57035D975F71D506 (subcategories); a level with fewer than 2 entries is skipped straight to level 2; level 2 = every
  `scoring_trick` record with a demo animation (Text 0x3B71EA27D53D7C4F) whose subcategory ref (0x06513CC28A532524)
  matches, sorted by the Int32 at layout offset 128 (field 0xF9E5D2F69066E925; insertion / intro sort, ascending).
  S+128 = !Bool 0xA990472B974734C3 of the new list's first subcategory: leaf subcategories show their own slots.
- Natives (registrar `sub_825AE7E0`): GetMenuItemData 0x825AE8A8 = [level, has submenu (`sub_826192A0`: level 0 true,
  level 1 the subcategory's bool, level 2 false), description (vt+88 0x826191B0)]; GetMenuItemGesturesButtons
  0x825AE968 = [slots with a gesture or button set, then gesture name, button name, connector per slot] (gesture ids
  58 / 59 read ""; connectors 1 Plus, 2 Arrow, 3 Or); IsGoofy 0x825AEBF0 always false; GetCurrentMenuItem = S+116.
  Game.GetMenuItems 0x825A52A8 lists the current screen's vt+80 / vt+84 (level names: category / subcategory name,
  trick label). Game.OnMenuGetItemEnabled goes to vt+72 = 0x8281DD70 (always true).
- Picker callbacks: OnMenuHilight vt+68 0x82618E98 (row; SetState(10) on the upper levels, else fill the slots
  `sub_82619D38` from the trick or, with S+128, the subcategory record, then SetState(11)); OnMenuSelect vt+64
  0x82618D98 (save the row, open the next level, an empty level goes back, SetState(1)). Input vt+40 0x82618F38: Back
  closes at level 0 (fade, callback 0x82618EF0) or goes up a level, twice past a one-entry level, clears S+128,
  SetState(0), plays core_b_button; A on a row without a list is consumed.
- Slots: `Sk8::FE::tTrickGuideInputSlot` arrays (12 bytes: gesture id, button id, connector); name tables 0x830273C8
  (58 gesture names) and 0x83027350 (28 button names). Button remap `sub_82619FC0`: when exactly one of CAS byte +120
  (= HUDComponents.PlayerStance_IsRegular 0x826635A0) and a controls byte is set, the triggers swap, A shows X, X shows
  A or B; the movie swaps A / X and the triggers back for a regular stance.
- Fonts: the movie's title font "HelveticaPresto" has no FontManager row; `sub_82809208` then takes the first row
  whose FileName is not "debug" (`sub_82AE8A10` returns 0 on a match).
- Word wrap (the description field): `sub_82CA14D0` loop 0x82CA1BB0 breaks greedily at spaces only, `^` forces a
  break, a word wider than the field overhangs; lines advance by vfunc +72 (0x82806470: font+360 x scale) x font+364
  (FontManager row +24 = field 0x672E35A2AA8C4A00: futuraheavy 0.9, others 1.0) x height; per-line alignment
  0x82CA23B4 (1 right, 2 and 3 centred); no clipping.
- Research: `.local/research/menus-m6/trick-guide-open.md`, `trick-guide-gestures.md`, `apt-text-wrap.md`.

### Change
- `skate-core/src/trick_guide.rs` (new): the data model (records by Attrib key), the navigation (lists, select, back,
  highlight, slots, the button remap) and mod patches (`EntryPatch`, `TrickGuide::apply`).
- `skate-data/src/trick_guide.rs` (new): builds the guide from the installed collections; `menu_tables.rs` extracts
  `trick_guide` (category keys by the code that copies them, the two name tables by content); `collections.rs` keeps
  array elements (`items`).
- `skate-game/src/trick_guide_host.rs` (new): the natives and picker callbacks, `TrickGuideMods` (mod layers);
  `retail_menu_stage.rs`: FE 14 open / close, input, queued screen states, the TrickGuide item's engine handler (only
  with tables, movie and collections, else the row stays greyed); `crossbar_host.rs` routes the objects;
  `fe_backdrop.rs` `TRICK_GUIDE_SCREEN`; `skater_animation.rs` `regular_stance`.
- APT fixes found on this screen: `apt_movie.rs` `class_links` (every copy of a library symbol gets its registered
  class; the screen imports "Button Item" itself and again through panel > button_menu, so its button help had no
  SetButtonType and no prompts); `apt_text.rs` / `apt_scene.rs` word wrap, line advance and alignment 3 centred.
- Export: `prepare_menu_movies.py` adds the trickguide root and the FontManager fallback for APT names without a row;
  `prepare_hud.py` exports `LineSpacing`; `menu_tables.py` requires the trick guide tables (setup refreshes both).
- Moddability: `sdk.menus.trick_guide(entry, key, options)` adds or changes categories, subcategories and tricks by
  key (retail 16 hex digits or the mod's own id): label, description, up to 4 slots, animation, parent, order,
  has_tricks, position, hidden; layered in mod order, removed when the mod stops (`skate-mods` command
  `menu_trick_guide`, `api.lua`, `sdk/skate.lua`). Retail values are the defaults, all from setup data.
- Multiplayer: local UI state only.

### Verification
- `cargo test -p skate-core trick_guide` (navigation, skip rules, leaf slots, remap, patches); `cargo test -p
  skate-data --test menu_tables -- --ignored` (TU3 image and disc xex give the same tables: 8 categories, 58 gestures,
  28 buttons); `--test trick_guide -- --ignored` walks the real tree (8 categories, 217 listed tricks; Kickflip's slot is
  ollie_kickflip); `retail_trick_guide_screen_opens` (screen manager, picker callbacks, B prompt drawn); the apt_text
  wrap test; `cargo test -p skate-mods` menu commands; `python -m unittest tools.asset_pipeline.test_menu_tables`.
- In game (dev build, tables extracted into the install for the shots, then restored): Learn > Trick Guide opens; the
  categories, Flip Tricks > Ollie Flips, Kickflip with its gesture and the wrapped description, Other Tricks > No Comply
  with A, Arrow, stick, A / B prompts. Pause menu and Game Settings unchanged.
- Known unrelated failures: skate-core `a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`
  (doc 11), skate-mods `skyline_every_component_is_real_and_drives_through_ground_contact` (Skyline GLB not in a
  worktree).

### Open questions
- The 3D demo behind the screen: not ported; the world shows instead (FE 14 has no blur). Research
  `.local/research/menus-m6/trick-guide-demo.md` (agent, not main-checked yet): init loads
  `world/models/DIST_TrickGuide_Textures.rx2` + `DIST_TrickGuide.rx2` (miscload.big, a backdrop set) and
  `rendering/csm_trickguide` unless `sub_82743FC0` (online state 11); close vt+24 0x82618CA8 unloads; the update posts
  message 0xF00F5033 (trickbook mode) and 0x58B0BD5C {clip, camera track} to the trickbook object (handlers
  `sub_82821810`), which plays `scene/trickguide/<name>1.abin` (scene.big, 312 clips, not in our extract) through the
  cutscene Scene system ("ViewScene"); `trickbook_camera_tracks` = Int16 index (22 records).
- The fade before open / close: ported, see "Fade to black".
- Remap controls byte (`+224/+20`) and the X setting byte (`+260/+37`) not decoded (defaults: off, X reads A).
- Font+360 is LineHeight or VAdvanceYMax (equal in every retail font); the wrap flag is taken as `word_wrap`.
- `SetSkin` on the menu picker is unhandled (the menu_picker class has none; likely a no-op in retail too).
- Ties in the trick order follow record order; retail's intro sort could order ties differently in lists over 28.

## Fade to black (2026-10-12)

### Problem
Opening and closing the Trick Guide switched screens at once; retail fades to black first and back in after.

### Root cause / evidence (TU3, main-read)
- The FE fader `[[0x830CFE14]+304]`: +12 alpha (1 = black), +16 step, +20 hold bits, +24 actions waiting for black.
  Hold `sub_825E6B30(fader, bit, callback, instant)`: sets the bit; already black = run the callback now; else queue
  it and step 0.04 (0x82216FEC; instant 1.0). Tick `sub_825E69A0`: alpha += step, clamped; at 1 the queued callbacks
  run. Release `sub_825E6BF8(fader, bit, instant)`: clears the bit; with none left and the screen not clear, step
  -0.04 (0x822F88F8; instant -1).
- Trick Guide: the crossbar case 0x8261BF78 holds bit 0 with callback 0x8261CDD8 (push FE 14) and sets its closing
  flags; screen 14's update `sub_82619A20` state 4 counts 11 updates (S+336 > 10), then releases bit 0 and goes to
  state 2, the only state its input handler takes keys in. Back at level 0 holds bit 0 with callback 0x82618EF0
  (close). The crossbar's init (vtable 0x82303F90 slot 3, `sub_8261ACA8`) releases bit 0, so the pause menu that comes
  back fades in.

### Change
- `skate-core/src/fe_fade.rs` (new): the fader (hold / release / tick, actions at black), `FeFadeParams` (steps and
  tick rate as data, retail's the defaults), FE cadence like fe_blur.
- `retail_menu_stage.rs`: TrickGuide select holds the fade and pushes FE 14 at black; the guide counts 11 ticks after
  its intro and releases; its Back at level 0 holds and closes at black; the pause menu's open releases; no pause input
  while the fade is held, no guide input before the release; a black quad over the menu layer with the fader's alpha;
  `RETAIL_MENUS fade black / clear` log lines.
- NOT RETAIL YET: the 30 Hz FE tick rate (as the blur); what enters screen 14's state 4 (the count starts at the
  intro's end); Game Settings still opens without the fade (its crossbar case not checked). NOT RETAIL (guard): with
  no screen left on the stack the fade is released (retail always returns to the pause menu).

### Verification
- `cargo test -p skate-core fe_fade` (25 ticks to black, the action at black, release order, instant, cadence).
- In game (tables extracted for the run, then restored): select Trick Guide: `fade black`, the pause menu closes, the
  guide opens, `fade clear` after the count; B at level 0: `fade black`, the guide closes, the pause menu opens again,
  `fade clear`.

## Trick Guide demo (2026-10-12)

### Problem
Retail shows a 3D demo behind the Trick Guide (FE 14): the player's skater performs the highlighted trick in a small
set. Ours showed the gameplay world behind the menu.

### Root cause / evidence (TU3; research `.local/research/menus-m6/trick-guide-*.md`, `raw-codec.md`)
- [code, main-read] Screen init 0x82618A40 loads `DIST_TrickGuide_Textures.rx2` + `DIST_TrickGuide.rx2`
  (miscload.big) unless online (`sub_82743FC0`), copies the default clip `trickguide_fs_noseblunt_01` (0x82204B88)
  and attaches `rendering/csm_trickguide`; the clip path is built by `sub_82826488` as
  `"scene/trickguide/" + name + "1.abin"` (0x82198B78); `sub_82821740` stores the camera track and starts the clip.
- [data, main-read] scene_000.bin (miscboot.big): `ViewScene` = record 254 (type 22, 5.0 s), one character, name
  `current_player`, kind `marquee`, default clip `trickguide_bindpose`. So the demo shows the player's own skater.
- [agent, high] The highlight settles 60 ms (S+332 = 60.0 counted down by the FE update's milliseconds) before the
  clip changes; the scene restarts at time 0 only on a new clip.
- [code, main-checked anchors] The clips (scene.big, 312 files) use the RAW codec "DR". Its TU3 sampler
  `sub_82D1E658` reads the header bytes +27 / +28 and the frame size +24; packed rotations are an axis in two angles
  (A = bits 21..31 x 2pi/2047 - pi, M = bits 11..20 x pi/1023 - pi/2) times r = bits 0..10 / 2047, with
  w = sqrt(1 - r^2) (pi at 0x82060C44). The pinned community decoder (tools/vendor/skate3_anim) read the middle field
  as a length, which folded the skeleton; with the sampler's formula the file's T-pose decodes to the gameplay
  `RIG_TPOSE` to three decimals.
- [data, NOT RETAIL YET] The clip channels are deltas on the file's pose (limbs near identity where the pose has large
  rotations; the HIPS channel is the inverse of the bind trajectory). Each frame = retail's AddSQT (`pose_add::add`,
  0x828CCC08) of the delta onto the pose, root bones hang from the trajectory. Checked: the bind pose stands (hips
  0.92 m, head 1.58 m, feet 0.11 m, mirrored); 302 of 312 clips start and end upright, the other 10 are Hall of Meat
  bails that end on the ground.
- [data] One authored NaN (trickguide_bs_dark_slide1 frame 103, RIGHT_WHEELBACK translation), also in the
  reference decoder: the previous frame is held.

### Change
- `skate-core/src/animation/raw.rs` (new): the RAW decoder (stored components over a caller's base).
- `skate-data/src/scene_clip.rs` (new): `SceneClip::parse` (131-bone hierarchy, parts by header bits 0..5 / 2,
  deltas added onto the file's pose).
- Setup: `tools/asset_pipeline/scene_clips.py` + task `tools/prepare_trick_guide_scene.py` (hud group): the clips to
  `assets/private/hud/scene/trickguide/`, the set as a render-only `TrickGuide.skate` (`export_set`, the format of
  backdrop.py's global-model export, kept apart from backdrop.py so this branch and the living-world branch merge
  cleanly), and the set's sky dome as
  `TrickGuide.sky.*` with its `material_sky` parameters.
- `skate-game/src/trick_guide_scene.rs` (new): while FE 14 is on top the set and dome spawn on render layer 27 as
  children of the player root, the player's own rig plays the clip (`AnimationStatus::for_scene`), the meshes the
  gameplay camera draws join that layer (hidden rigs stay out: a hidden default skater under a custom model showed
  through on hails-additions), the gameplay camera draws only that layer; closing despawns them and restores layers. `retail_menu_stage.rs` publishes
  `TrickGuideDemo` (open, highlighted clip); `map_render.rs` `SceneCommands::apply_now` (the demo re-tags what it
  spawned: its own marker instead of `MapEntity`, parented to the player root, on the demo layer); `retail_sky.rs`
  `SkyPackage::load_scene`.
- Mods: a mod picks any clip per entry through the existing Trick Guide patches (`sdk.menus.trick_guide`, field
  `animation`). Mod clip folders are not wired yet.
- NOT RETAIL YET: looping (assumed), the delta add and trajectory parenting (from the data). The camera, fog and
  sun: see "Trick Guide camera, fog and sun" below.

### Verification
- `cargo test -p skate-core animation::raw`; ignored data tests `crates/skate-data/tests/scene_clip.rs` (all 312
  decode, T-pose stands, upright ends) and `tests/trick_guide_checklist.rs` (217 trick rows, all with a clip); script
  `.claude/skills/retail-menus/tools/check_trick_clips.sh`.
- In game (default skater and a custom model): the set, the dome and the skater playing the noseblunt
  (screenshots `.local/research/menus-m6/demo/`); gameplay after the change unchanged (Downtown screenshot).
  Closing the guide in a run was not checked (injected keys arrive before the guide takes input); play test pending.

### Open questions
- Whether the scene loops the clip (`TrickGuideLoop` = enum 9). Camera and environment: next section.
- 61 clip files and 9 clip names no listed row uses (8 tricks with no subcategory, the unlisted "Retrieve"
  subcategory): retail does not show them either.

## Trick Guide camera, fog and sun (2026-10-12)

### Problem
The demo used a fixed side view of the hips, no fog, and the current map's sun on the set's dome.

### Root cause / evidence (TU3 recomp code and the disc data; research `.local/research/menus-m6/trick-guide-camera.md`, `trick-guide-fog-csm.md`, `trick-guide-csm-fields.md`)
- Each Trick Guide entry names a camera track: RefSpec 0x3D3A79F16DCB54BB (a ClassRefSpec: record key in the first
  8 bytes) into `trickbook_camera_tracks`, whose Int16 is the track index; no ref reads 0 (`sub_826188B8`, S+200);
  the screen init sets track 1 with the default clip. [code + data] The installed data uses tracks 0, 1, 3, 18, 19, 20.
- The tracks are the ICE ViewScene block (scene hash 0x7F298808) in scene.big `data/scene/ice_000.bin`: 21 records
  (`sub_828F51E8`), each with 37 key groups; the camera elements (EYE, LOOK, DUTCH, TANGENT_EYE / TANGENT_LOOK,
  LENS_LENGTH) sit in group 0 (element table `sub_82903840`; EYE's descriptor names TANGENT_EYE as its tangent
  element, +64 = 7, and element 87 at +60). [code + data]
- Interpolation (main read the code): `sub_828F0BB0` sets each element up per segment as a cubic Hermite
  `((a*u + b)*u + c)*u + p0`, a = m0 + m1 - 2D, b = 3D - m1 - 2*m0; the tangent `sub_828F0990` is the non-uniform
  Catmull-Rom tangent in the segment's own parameter (neighbour segment weighted by the durations; none at the ends:
  D), scaled by the tangent element's value at that key. [code]
- FOV: `sub_828F1E80`, `2*atan(15.96 / lens_mm)`, lens clamped to 10..500 mm. [code]
- While the set is loaded, the renderer's environment is render location `trick_guide` (`sub_827950A8`: model list
  +17496 == 1 selects key 0x6568F34643066684, main checked): parents `fe`, `default`; sun (0, 0.8, 0.1), anchor 165,
  `fog_trickguide` (fog_default made black: near 10, far 200, power 1.9, max 0.8). [code + data]
- `csm_trickguide` differs from `cascade_shadowmap` in the per-cascade shader bias `g_CSMSelfBias` (0.005, 0.001,
  0.024 vs 0.0025, 0.005, 0.012), a Bool and a pair (-2, 4) stored next to each other with no reader found; both
  records share the cascade splits 1.8 / 7.2 / 28.8 m. [code + data, research agent; the bias name by its hash]

### Change
- `skate-core/src/camera_track.rs` (new): `CameraTrack::sample` (the Hermite and tangent above), `fov_radians`.
- `skate-data/src/ice_camera.rs` (new): `scene_tracks(bytes, VIEW_SCENE)` reads the block (little-endian; key times
  u16/65535 of the length, quantised elements MSB-first bit streams).
- `skate-data/src/trick_guide.rs`: `Entry.camera_track` from the ref; `skate-core` `TrickGuideState.camera_track`
  (S+200) and `EntryPatch.camera_track`; mods: `sdk.menus.trick_guide` option `camera_track` (track index).
- `trick_guide_scene.rs`: the playing clip's track is fitted to the clip length (retail "Fixing Camera Group
  length"), placed on the skater with the height zeroed, and drives the gameplay camera's transform, roll and FOV
  (restored on close); track -1 or no `ice_000.bin` keeps the side view.
- Setup `scene_clips.py`: exports `ice_000.bin` next to the clips, and the set's sky package carries the `trick_guide`
  location's sun, anchor height and fog frame (`scene_environment`); `retail_sky.rs` `load_scene` uses it (older
  exports: the map's sun, no fog), and the set's materials take its fog.
- NOT RETAIL YET: the locator and axis map (inferred: the tracks aim at a skater near the origin while clips travel
  up to 70 m, so the track follows the skater with height zeroed, "TrickbookPlayerAtZeroY"; track +y = the clips'
  run direction, Z up: scene = (y, z, x)), the lens angle as the vertical FOV, DUTCH as radians and linear, LENS with
  tangent 1, the spline switch (element 87) assumed on, key times and dequantisation (from the data), the record walk
  (by the constant +4 word: the element table past id 17 is not decoded). The location's exposure and post_fx refs
  stay the map's. `csm_trickguide` is not applied: our shadows do not port the base `cascade_shadowmap` record yet,
  and its own differences are a per-cascade bias Bevy has no slot for and fields with no known reader.

### Verification
- `cargo test -p skate-core camera_track` (key hits, linear two-key case, ease with tangent 0, Catmull-Rom on
  collinear keys, the 24 mm angle); ignored data tests `skate-data --test ice_camera` (`SKATE3_ICE`: 21 tracks,
  lengths, track 0 values, track 3's interior key, finite samples) and `--test trick_guide` (track indices 0..20 in
  use, 3 and 18 present).
- In game with the dev export: the default clip with track 1 frames the skater left of the menu, facing the camera;
  the fog is applied (faint at the set's distances, as its values give). Other tracks are not screenshot (injected
  keys arrive before the guide takes input). No retail picture of the demo to compare yet.

### Open questions
- A recomp trace (hooks being designed: `.local/research/menus-m6/trick-guide-hooks.md`) to settle the locator, the
  axis map, horizontal vs vertical FOV, DUTCH units, element 87 and the track time fitting.
- Who reads the csm Bool / pair; mapping `cascade_shadowmap` + `csm_trickguide` onto our cascades.

## Files
- Trick Guide camera, fog and sun: `crates/skate-core/src/camera_track.rs` (new), `crates/skate-data/src/ice_camera.rs`
  (new), `crates/skate-data/tests/ice_camera.rs` (new), `crates/skate-data/src/trick_guide.rs`,
  `crates/skate-core/src/trick_guide.rs`, `crates/skate-mods/src/menus.rs`, `crates/skate-game/src/trick_guide_host.rs`,
  `crates/skate-game/src/trick_guide_scene.rs`, `crates/skate-game/src/retail_menu_stage.rs`,
  `crates/skate-game/src/retail_sky.rs`, `tools/asset_pipeline/scene_clips.py`.
- Trick Guide demo: `crates/skate-core/src/animation/raw.rs` (new), `crates/skate-data/src/scene_clip.rs` (new),
  `tests/scene_clip.rs`, `tests/trick_guide_checklist.rs` (new); `crates/skate-game/src/trick_guide_scene.rs` (new),
  `retail_menu_stage.rs`, `map_render.rs`, `retail_sky.rs`, `app.rs`, `main.rs`; `tools/asset_pipeline/scene_clips.py`,
  `asset_exports.py`, `versions.py`, `tools/prepare_trick_guide_scene.py` (new).
- Trick Guide: `crates/skate-core/src/trick_guide.rs` (new), `crates/skate-data/src/trick_guide.rs` (new),
  `menu_tables.rs`, `collections.rs`, `tests/trick_guide.rs` (new); `crates/skate-game/src/trick_guide_host.rs` (new),
  `retail_menu_stage.rs`, `crossbar_host.rs`, `apt_movie.rs`, `apt_stage.rs`, `apt_text.rs`, `apt_scene.rs`,
  `fe_backdrop.rs`, `skater_animation.rs`, `retail_menus.rs`, `modding/mod.rs`, `main.rs`; `crates/skate-mods/src/menus.rs`,
  `vm.rs`, `api.lua`; `sdk/skate.lua`; `tools/prepare_menu_movies.py`, `prepare_hud.py`,
  `asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Out-of-date prompt: `crates/skate-game/src/asset_check.rs` (new), `setup.rs`, `graphics_menu.rs`, `graphics_menu/view.rs`
- Stored settings: `crates/skate-game/src/retail_menus.rs` (`StoredSettings`, `setting`), `scoring_hud.rs`
- Swap-in: `crates/skate-game/src/retail_menu_stage.rs` (`enabled`, `RetailPause`), `graphics_menu.rs` (`overlay_key`)
- PC overlay: `crates/skate-game/src/input/platform.rs`, `graphics_menu.rs`, `graphics_menu/view.rs`, `fonts/`
  (Inter + OFL), `fe_backdrop.rs` (overlay blur), `scoring_hud.rs` (HUD hidden), `customiser.rs`
  (`Navigation::held`), `game_audio/mod.rs` (volume row accessors), `custom_difficulty.rs`, `retail_menu_stage.rs`
- Sub-options: `crates/skate-data/src/menu_tables.rs` (`SubOptionRow`), `tests/menu_tables.rs`;
  `crates/skate-game/src/crossbar_host.rs`, `retail_menu_stage.rs`; `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Button prompts / panel: `crates/skate-game/src/prompt_glyphs.rs` (new), `input/controller_kind.rs`,
  `input/platform.rs`, `input/tests/controller_kind.rs`, `apt_scene.rs`, `apt_movie.rs` (`clip_matrix`,
  `transform_changed`), `apt_stage.rs`, `hud_render.wgsl`, `scoring_hud.rs` (`render_draws` gamma flag),
  `retail_menu_stage.rs`
- Game Settings screen: `crates/skate-game/src/settings_host.rs` (new), `crossbar_host.rs`, `retail_menu_stage.rs`,
  `retail_menus.rs`, `fe_backdrop.rs`, `apt_vm.rs`, `apt_movie.rs`, `apt_stage.rs` (test), `main.rs`;
  `crates/skate-core/src/fe_input.rs` (retail repeat ticks); `crates/skate-data/src/menu_tables.rs`
  (`settings_titles`), `tests/menu_tables.rs`; `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Pad input: `crates/skate-core/src/fe_input.rs` (new), `lib.rs`; `crates/skate-game/src/retail_menu_stage.rs`,
  `graphics_menu.rs` (Start / Escape gating)
- Screen stack: `crates/skate-core/src/fe_stack.rs` (new), `lib.rs`; `crates/skate-game/src/retail_menu_stage.rs`,
  `fe_backdrop.rs` (index, info), `graphics_menu.rs` (`fe_suspended`), `game_audio/mod.rs`;
  `crates/skate-data/src/menu_tables.rs` (record fields), `tests/menu_tables.rs`
- Row handlers: `crates/skate-game/src/retail_menus.rs` (`OVERLAY_ITEMS`), `retail_menu_stage.rs`,
  `graphics_menu.rs` (`open_section`)
- Pause blur: `crates/skate-core/src/fe_blur.rs` (new), `lib.rs`; `crates/skate-game/src/fe_backdrop.rs`,
  `fe_backdrop.wgsl` (new), `retail_menu_stage.rs`, `retail_exposure.rs` (label), `retail_menus.rs` (cleanup,
  snapshot), `modding/mod.rs`, `app.rs`, `main.rs`; `crates/skate-mods/src/menus.rs`, `vm.rs`, `api.lua`;
  `sdk/skate.lua`;
  `crates/skate-data/src/menu_tables.rs` (`FeScreen`, `screens`), `tests/menu_tables.rs`;
  `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Milestone 6: `crates/skate-core/src/crossbar.rs` (new), `lib.rs`; `crates/skate-game/src/crossbar_host.rs`
  (new), `fe_language.rs` (new), `retail_menu_stage.rs` (new), `apt_stage.rs`, `apt_scene.rs`, `apt_movie.rs`,
  `apt_vm.rs`, `retail_menu_movies.rs`, `scoring_hud.rs` (`render_draws`), `app.rs`, `main.rs`;
  `tools/prepare_menu_movies.py` (set v4), `tools/vendor/skate3_ui/actions.py` (getURL);
  `crates/skate-data/src/menu_tables.rs` (`ScreenTemplate`, `templates`), `crates/skate-data/tests/menu_tables.rs`,
  `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`
- Milestone 5a: `crates/skate-game/src/apt_stage.rs` (new), `apt_movie.rs`, `apt_vm.rs`, `main.rs` (module)
- `crates/skate-data/src/menu_tables.rs` (extractor, JSON, conversion), `crates/skate-data/tests/menu_tables.rs`
- `crates/skate-core/src/menus.rs`, `menus_tests.rs` (registry), `menu_values.rs` (value rules, milestone 2)
- `crates/skate-game/src/apt_vm.rs` (milestone 3a opcodes, tests)
- `crates/skate-game/src/apt_imports.rs` (milestone 3b import resolver, mod overrides, tests), `apt_movie.rs`
  (`link_imports`), `main.rs` (module)
- `crates/skate-game/src/apt_imports.rs` (milestone 3b import resolver, mod overrides, tests), `apt_movie.rs`
  (`link_imports`), `main.rs` (module)
- `crates/skate-game/src/retail_menus.rs` (resource, loader, mod commands, events, snapshot, engine value bindings),
  `game_audio/mod.rs` (`RetailVolumes`, saved), `game_audio/native.rs` (Master.in1..3), `main.rs`
  (`--extract-menu-tables`), `app.rs`, `modding/mod.rs` (commands, cleanup, events, snapshot)
- `crates/skate-mods/src/menus.rs`, `vm.rs` (commands, capability), `api.lua`; `sdk/skate.lua`
- `tools/asset_pipeline/menu_tables.py`, `test_menu_tables.py`, `asset_exports.py`, `versions.py`
- Milestone 3c: `tools/prepare_menu_movies.py`, `tools/test_prepare_menu_movies.py`, `tools/prepare_runtime_huds.py`;
  `crates/skate-game/src/retail_menu_movies.rs`, `retail_menus.rs` (startup, cleanup, snapshot), `modding/mod.rs`
  (`menu_movie`), `apt_imports.rs` (`main_checkout`, `OverrideTable::position`), `apt_vm.rs` (test path)
- Milestone 3d: `crates/skate-game/src/apt_display.rs` (`ClipAction`, `clip_event`), `apt_movie.rs` (`Pending`,
  dispatch, `next_action`, link), `hud_runtime.rs` (drain), `apt_vm.rs` (0xA6), `retail_menu_movies.rs` (set
  version 2, test); `tools/prepare_menu_movies.py` (`clip_actions`), `tools/test_prepare_menu_movies.py`
- Milestone 3f: `crates/skate-game/src/apt_display.rs` (clip-depth storage rule), `apt_movie.rs` (test),
  `retail_menu_movies.rs` (clip-depth count in the set test)
- Milestone 4: `crates/skate-game/src/apt_scene.rs` (`Fill`, `ShapeSource`), `apt_movie.rs` (`shape_origins`),
  `retail_menu_movies.rs` (shapes, `MenuScene`, `image`, mod bitmaps, tests), `scoring_hud.rs` (optional texture),
  `modding/mod.rs` (`menu_bitmap`); `crates/skate-mods/src/vm.rs`, `api.lua`; `sdk/skate.lua`;
  `tools/prepare_menu_movies.py` (set version 3), `tools/test_prepare_menu_movies.py`

## Verification
- `cargo test -p skate-data --test menu_tables -- --ignored` (local files, skipped when absent): the TU3 image gives
  exactly the documented tables and addresses (36 rows, 13 screens, 5 categories, 49 items, 56 keys, 9 modes,
  Career Main, park rows 9 slots, Free Play 4 tabs); the user's disc `default.xex` (a different build) gives the same
  content at its own addresses, minus TU3's additions (above), compared entry by entry against TU3.
- `cargo test -p skate-core menus`: greyed by default, a handler enables, dirty-park confirmation intercepts, hidden
  rules (Project10, SFX pack), links and the Video choice, mod add / hide / reorder / relabel and exact revert,
  owner stacking.
- `cargo test -p skate-mods menus`, `cargo test -p skate-game retail_menus`: options and rule validation, mod
  commands end to end, events JSON, revert.
- `python -m unittest tools.asset_pipeline.test_menu_tables`: the setup step through the real spawn path.

## Open questions
- Career IsItemEnabled (`8261F850`) greys SkateWith / SkateReel when `82511168` fails and EnterSkatePark / PartyPlay by
  other checks; those conditions are not decoded yet, so Career uses the default rule (enabled once handled).
- The Free Play park twin of IsItemEnabled (`826205C8`) is assumed to match the Career park one; to read.
- Mode keys are ours (retail has no names); the online / lobby menus are extracted but not decoded further (online
  is a custom implementation later).
- Retail stops the settings row count at a hidden row; the registry skips hidden rows instead (identical for the only
  retail case, the SFX pack row, which is last on its screen).
- Next milestones (settings model done, above): APT player gaps, host side (Game / CoreMenu objects, fe
  sounds, input repeat), then swapping in the retail menus with the current features as hooks.
