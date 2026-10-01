# 11 — Game audio (ambience, rolling, trick cues, footsteps, water)

**Branch:** `gameplay/audio` (from `gameplay/water` 477e9de), uncommitted. **Status:** implemented and tuned
by ear over ~15 play sessions with the user (2026-10-01). Most cues are confirmed in play; the rest are marked
**not yet confirmed** below. All sample choices are project choices, not retail event data.

## Problem

The engine made no sound of its own. Only Lua mods could play audio
(`crates/skate-game/src/modding/audio.rs`, PCM WAV through Bevy's `bevy_audio`).
Skate 3's sounds are on the owned disc but nothing extracted or decoded them.

## What is on the disc

Everything audible is under `data/audio/`. All archives except `music/overlays.big` are EB BIG v3
archives readable with `tools/owned_game/big.py`.

| Archive | Contents |
|---|---|
| `ambience.big` + `ambienceresident.big` | 24 per-area ambience beds (`.sns` bodies, `.snr` headers), e.g. `04_dt_main`, `09_univ_campus`; 5-channel 48 kHz, ~2–2.5 min loops |
| `audiofiles.big` | 376 ABKC `.abk` banks, 20 SPLC `.bnk` banks, 23 per-map `.ems` emitter files, 9 MOIR `.csi` AEMS event scripts |
| `grains.big` | 14 rolling "grains", 7 surfaces × soft/hard wheels (`asphalt_smooth`, `asphalt_rough`, `concrete_smooth`, `concrete_rough`, `concrete_aggregate`, `wood_ramp`, `metal_smooth` hard only) + `x_jet_rolling` |
| `wheels.big` | `Whls_spins_Jump_1`, `Whls_spins_Man_1` (+ `.sek` seek tables) |
| `post.big`, `nis.big`, `english/…`, `music/…` | stingers, speech, cutscenes, licensed music — not used |

### Formats (worked out from the archives; `tools/asset_pipeline/audio_formats.py`)

- **Codec:** every sample is an EA SNR (EAAC) stream with codec 3 = **EA-XMA** (Xbox 360 XMA2),
  mostly mono 48 kHz (also 44.1, 36, 24 and 22.05 kHz).
- **SNR header:** `u32 version(4)|codec(4)|channels-1(6)|rate(18)`, `u32 type(2)|loop(1)|samples(29)`,
  optional `u32 loop start`. RAM streams are followed by blocks of `u8 flag (0x80 = last)`, `u24 size`,
  `u32 samples`.
- **SPLC `.bnk`:** stream count at 0x18, 36-byte parameter records, then that many SNR streams back to
  back at **unaligned** offsets. The scanner finds exactly the declared count in all 20 banks (2,266 streams).
- **ABKC `.abk`:** embedded SNR streams; the scanner's count matches vgmstream's subsong count for all
  376 banks (5,168 streams).
- **`.grain`:** `u32` offset of the SNR stream, `f32` duration (matches samples/rate exactly), a
  `.sek`-style seek table, then one SNR stream. Each recording is a **slow-to-fast rolling sweep**: over
  17–22 s it gets ~10 dB louder and brighter (e.g. concrete_smooth_hard −24 → −14 dBFS, centroid
  0.9 → 2.0 kHz), so looping the whole file sounds wrong.
- **5-channel ambience** order is L, C, R, Ls, Rs (from inter-channel correlation).
- **`.csi` (MOIR)** event scripts name events, classes and variables (`play_grind_start`, `Class_Flips`,
  `Ollie_Rattles`, `surface_type`, `playercharacter_footstep`, `body_imp_Torso`, …) in 16-byte records
  (name offset, 16-bit id, default value). The banks hold no per-sample names; linking events to samples
  would mean re-implementing EA's AEMS runtime, which this change does not do.
- Retail mixes at runtime: many raw samples peak at 0 dBFS with RMS up to −6 dBFS, so playback levels
  must be low; footstep recordings are the opposite (peak ≈ −11 dBFS, RMS ≈ −28 dBFS).

## Change — current design

### Setup: new `audio` asset group

- `tools/asset_pipeline/audio_formats.py` — SNR scanner/validator (block-chain check), SPLC and grain
  readers, standalone `.snr` cut-out, gain-free 5→2 downmix (weights per output sum to 1), loopable
  band cutting for grains.
- `tools/asset_pipeline/audio_export.py` — decodes only what the engine plays to PCM16 WAV in
  `assets/private/audio/` with `audio_manifest.json` (version 2): 22 ambience beds (stereo), 14 grains
  cut into **6 loopable speed bands** each (84 files, 0.08 s loop cross-fade), 2 wheel spins and 23 sample
  banks (2,718 samples). No normalisation or gain. ~40 s, ~615 MB.
- **Decoder: vgmstream r2117** (`vgmstream-win64.zip`, SHA-256 `6c4a8a38…dc6c`, matching GitHub's
  published digest), downloaded and verified by the existing `install.dependency()` into
  `data/tools/vgmstream-cli/`. It bundles FFmpeg's XMA2 decoder (LGPL DLLs). vgmstream reads SNR/SNS and
  ABKC itself but not SPLC or grain files, so every stream is cut out to a standalone `.snr` first and
  decoded in batches of 256 (one vgmstream run stops accepting input files between 600 and 1,100 arguments).
- Registered as group `audio` (`versions.py` `GROUPS`/`SOURCES`, `group_receipts.py` `ROOTS`,
  `asset_exports.audio`, `install.py`). **Optional content:** a failed download or decode records
  `assets/private/audio-availability.json` and setup still succeeds; the game then runs silent. Existing
  installs refresh only this group.
- `.gitignore`: retail audio extensions and `*.wav` (except the Skyline Drive mod's own WAVs).

### Engine: `crates/skate-game/src/game_audio/`

| File | Role |
|---|---|
| `mod.rs` | `AudioSettings` (master **25 % by default**, ambience 100 %, effects 100 %, 5 % steps) saved to `settings/audio.json`; menu rows in GRAPHICS; `--mute`; `GlobalVolume` follows the master so mod audio obeys it; the single `SpatialListener` (moved out of `modding/audio.rs`) follows the gameplay camera; preloads every cue sample, grain band, wheel spin and water piece at startup (~207 clips, ~35 ms) so riding never reads the disk. |
| `library.rs` | Loads the manifest; reads WAVs into `Assets<AudioSource>`, measuring each clip's peak; rejects paths outside the audio folder; warns once per missing file. |
| `voices.rs` | Voice pool and **loudness rules** (below). Everything pauses while the menu is open or a replay runs. |
| `cues.rs` | The sample table, per-cue minimum gaps, metal-surface test, surface → grain table. |
| `skate_events.rs` | `observe` (FixedUpdate, after each physics tick) turns state changes into cues; `play` (Update) plays them and drives the loops (rolling, grind, powerslide, foot drag, wheel spin). |
| `ambience.rs` | One bed per map, level 0.6, 2 s cross-fade on map change. |
| `water.rs` | Water emitters from the map's water collision. |

Outside the module (observation only; physics never reads any of it):

- `physics/animation_input.rs` — `AudioEvents` latch. `push_contact` (flags bit 27), `brake_contact`
  foot-down/up (bits 28 + 30 / 23) and `AudibleFootStepStrength` are cleared within the tick
  (`finish_output_publication`, `ScalarAttributeInputs::reset`), so they are latched while the animation
  attributes are processed and taken by `observe`.
- `physics.rs` `foot_clearance` — each animated foot's height above its ground line test (via
  `biped_ground::services::feet_input`).
- `skate-core` `WheelLineState.audio_surfaces` — audio surface (`surface_tag & 0x7F`) per wheel, next to the
  existing `physics_surfaces`. Additive output only.

### Loudness rules (`voices.rs`)

- A requested volume may raise a quiet clip only until the clip's own peak reaches full scale, and never
  more than ×4; category and master volumes (both ≤ 1) then scale it down. No clip plays hotter than
  full scale × master.
- Sounds start silent and fade in (≥ 10 ms); at most 32 voices, 3 of one sound, same sound again only
  after 40 ms; each cue also has a minimum gap (`cues::min_gap`: step 0.2 s, run step 0.15, push 0.3,
  bail 0.5, splash 1.0, others 0.12) as a safety net against flags that stay set for many ticks.
- One-shots get a random ±4 % pitch. Spatial attenuation scale 0.1 (Bevy's attenuation never amplifies).

### Cues (as in `cues.rs` / `skate_events.rs` now)

"Level" is the cue level in `cues.rs`; the scale column multiplies it. Fraction = board speed / 10 m/s.

| Cue | Trigger (per physics tick) | Samples | Level × scale | In play |
|---|---|---|---|---|
| Pop | `ground_animation.launched` rising | `Skate_Collisions` 1074–1078 | 0.45 | **Confirmed** |
| Flip / grab whoosh | new trick name while airborne (a held grab re-announced by scoring stays silent) | `Sk8_Air_Flip_Tricks` 10–13 | 0.3 | Behaviour **confirmed**; samples never auditioned separately |
| Land | filtered category Air → Ground, air not begun on foot; impact = `riding.ground.maximum_closing_speed` | `Skate_Collisions` 1088–1092 | 0.9 × (0.3 + 0.7·impact/12) | **Confirmed** |
| Land weight layer | same, impact > 1.5 m/s | `Skate_Collisions` 1097–1100 | 1.0 × (0.4–1.0 over 1.5–9 m/s), pitch 1.0 → 0.8 | Not yet confirmed ("not thumpy enough" before the last change) |
| Board down | Air → Ground when the air phase began on foot (on foot within the last 0.5 s) | `Skate_Collisions` 0–2 | 0.3 × (0.3–1.0 over 0–5 m/s) | Gentle set-down **confirmed** |
| Board down heavy | same, impact > 2.5 m/s ("caveman" jump-on) | `Skate_Collisions` 1097–1100 | 1.0 × (impact − 2.5)/4 | Not yet confirmed |
| Grind (loop) | `grinds.grinding_316` or a grind state, and not `leaving_317`; stop fade 0.06 s | `GRINDS` 54–56 on metal (`grinds.audio_surface_216` ∈ 9, 11–40, 67–69, 85, 89, 91), else 9/11 | 0.3 × (0.5 + 0.5·fraction) | Timing "mostly right"; metal pick (by measurement) not yet confirmed |
| Powerslide | state 101 SlideGround: random piece every ~0.08 s (±20 %), 10 ms fade-in | `WHEEL_SKID_BANK` 0–7 | 0.2 × fraction | Not yet confirmed |
| Foot drag | between `brake_contact` foot-down ticks (ends at foot-up or 0.1 s after the last foot-down), riding on the ground above 0.5 m/s; piece every ~0.11 s, 50 ms fade-in | `FOOT_DRAG` 36–39, 44–47, 60–64 | 0.25 × (0.3 + 0.7·fraction) | **Confirmed** |
| Push | `push_contact` rising edge | `fstep_skateshoe1_sm` 74–84 | 0.6 | Plays at a sane rate (no flood); sound not judged separately |
| Step / run step | foot strike (below) while on foot (states 500/501) and moving > 0.3 m/s; run set above 3 m/s | `fstep_skateshoe1_sm` 74–84 / 6–17 | 0.85 / 0.68 × (`AudibleFootStepStrength`/4, 0.3–1.0) | **Confirmed** ("almost perfect") |
| Bail | entering WipeoutGround (300), not in water and no splash in the last 0.5 s; tier by body speed (≥ 3.5 / ≥ 7 m/s) | `Bodyslide` 0–4 / 5–9 / 10–15 | 0.25 (sweetener layer) | Tiers heard as right; main body-impact sound still open |
| Splash | body water contact (`collision_feedback.flags.material_12`) rising while falling > 1 m/s (1.5 s cooldown); or entering a wipeout in water with no splash in the last 1 s (speed ≥ 2) | `Skate_Collisions` 476–478 | 0.5 × (0.4 + 0.6·speed/8) | **Confirmed** (found by the user); the wading-wipeout path not yet confirmed |
| Rolling (loop) | wheels in contact, not grinding, > 0.3 m/s; grain by majority wheel audio surface; speed band | `grains/<surface>_hard/<band>` | 0.15 × min(speed/1.5, 1) | **Confirmed** |
| Wheel spin | take-off above 1 m/s, cut on landing (0.05 s) | `Whls_spins_Jump_1` | 0.15 × fraction | Not yet confirmed |

**Rolling bands:** board speed is low-passed (0.25 s) and mapped over 0–10 m/s to the 6 bands; the band
changes only when the speed is a full band away and after ≥ 1 s on the current one; changes cross-fade
over 0.15 s, and within a band the pitch follows speed by ±6 %. A new surface must stay under the wheels
0.25 s before the grain changes; after 0.15 s off the ground the loop stops and restarts directly on the
surface it lands on.

**Footsteps:** `FootStrike` (in `skate_events.rs`) learns each foot's resting clearance (follows the
lowest value, drifts back up over ~2 s) and fires when a foot that lifted > 5 cm above rest comes back
within 2 cm. `AudibleFootStepStrength` only scales the volume.

**Surface → grain** (`cues::grain_for`, audio surface names from the map tooling's `_AUDIO_NAMES`,
`tools/vendor/university/…/owned_world_material_addon/__init__.py`): 1 asphalt_smooth; 2 asphalt_rough;
4, 66 concrete_rough; 5, 8, 10, 55, 56 concrete_aggregate (aggregate, dirt, grass, leaves, bush);
6, 7, 41–46, 90 wood_ramp; metal IDs → metal_smooth; everything else (Concrete_Polished = 3, curbs, tile,
marble, unlisted) concrete_smooth. Only the "hard" grains exist for every surface, so those are used.

### Water emitters (`water.rs`)

On map load, water collision triangles (type 12) are grouped into bodies by shared vertices and sampled on
a 6 m grid; each body is classed by area. The nearest point of each body is the emitter, placed ≤ 8 m from
the listener in its direction (panned, Bevy attenuation stays 1) with its own distance fade
((1 − d/reach)^1.5). At most one body per kind and three in all; Ambience volume; random pieces with a
0.4 s fade-in.

| Kind | Area | Bank (pieces 0–9) | Level, reach | Interval |
|---|---|---|---|---|
| Fountain | < 400 m² | `water_fountain` (moving/splashing water) | 0.45, 25 m | 0.6–1.1 s |
| Canal | < 30,000 m² | `water_lapping` | 0.4, 40 m | 0.7–1.3 s |
| Lake | larger | `water_lapping_pond` | 0.45, 60 m | 0.7–1.3 s |

University: 7 bodies (6 fountains, 1 lake); DownTown: 25 (12 fountains, 13 canals). The fountain bank
replaced a still-water lapping loop after the user's comment; **not yet confirmed in play**.

### Ambience per map (project choice by bed name; retail switches beds by zone via `.ems` files)

| Map | Bed |
|---|---|
| University | `09_univ_campus` |
| StartPark | `10_univ_housing` |
| DownTown | `04_dt_main` |
| DownTownSkatePark, MegaPark | `05_dt_parks` |
| Industrial | `11_indu_shipyard` |
| IndustrialSkatePark | `20_indu_old_factory` |
| SkateSchool | `18_skate_school` |
| MaloofMoneyCup | `21_interior_arena_amb` |
| BlackBoxPark | `22_interior_tunnel_amb` |
| Test world | none |

"Ambience good so far" (user, University); other maps not yet judged.

### Logging (always on)

- `AUDIO_CUE <cue> <bank>:<index> volume=…` for every one-shot (`(voice limit)` if refused).
- `AUDIO_LOOP start <file>` for rolling/grind/water loops, `AUDIO_LOOP start <name> (<bank>)` for shuffles.
- `AUDIO_EVENT brake down|up`, `push`, `grind start|stop surface= ledge= flag= leaving=`,
  `rolling surface= grain= band=` (on change only).
- Startup: `Game audio: 22 ambience beds, 14 rolling grains, 23 sample banks`, `Game audio: preloaded N clips`,
  `Ambience: <bed> for map …`, `Water audio: N bodies (…)`.
- `SKATE_AUDIO_TRACE=1` adds per-tick `AUDIO_TRACE` lines (footstep strength, push/brake flags, foot heights).

### Dev tools

- `py -3.13 tools/audio_audition.py` → `.local/audio-audition/index.html`: every cue's current samples
  (parsed from `cues.rs`), each bank in full grouped into runs of similar clips, grains and beds; players at
  25 % volume; links the installed WAVs, copies nothing.
- `cargo run -p skate-data --release --example audio_surfaces -- <map.skate>` lists audio surface IDs per
  map; with `AT=x,y,z[,r]` it lists the collision surfaces (audio ID, physics type, slope) within r m
  (default 3) of a point, e.g. a HUD position the user reports.

### Retail patch records (SPLC) and prior work upstream
- **Prior work:** upstream PRs #4 ("Audio/retail exact player sound") and #1 (an earlier snapshot of it,
  plus a procedural vehicle-engine synth) run retail's AEMS sound routing through a function-by-function
  transcription of recompiled game code and a dumped TU3 memory image. That runtime conflicts with this
  fork's rule (re-implement; disassembly is reference only; no game code/data), so it is not used. Their
  format notes were used as *knowledge*, re-derived and verified here.
- **SPLC patch tree** (`audio_formats.splc_patches`): 60-byte header (+8 sample-table offset from byte 60,
  +12 records, +16 containers, +20 extras = 0, +24 samples); 36-byte records (+4 id, +7 group count); 72-byte
  containers (+4 record ids, +68 count, +69 mode); per record its groups (12-byte header: +8 member count,
  +9 mode) of 72-byte members (+0 sample index, +8 gain, +44 pitch spread, +48 gain range, +64 probability;
  +20/+52 delay, not used yet). Verified on all 20 SPLC banks: the walk ends exactly at the sample table,
  every member names a valid sample (5,973 members → 2,266 samples), and sample-table offsets sit at a
  constant distance from our SNR scan — **SPLC sample index n is our stream n**.
- **Semantics** (per PRs #1/#4, implemented independently in `skate_events.rs` `play_record`): a record's
  groups are layers played together, one member each; group mode 0 random, 1 in sequence, 2 shuffled
  without an immediate repeat; a member plays unless rand > probability; gain ± range; pitch random between
  1/s and s for spread s > 1; ids ≥ the record count are containers that pick one record.
- **Validation of the ear-picked samples:** every group the user picked is exactly one retail record in
  `Skate_Collisions`: pop 1074–1078 = record 818, landing 1088–1092 = records 824/825, heavy 1097–1100 =
  records 832/833, board-down 0/1/2 = records 125/126/127, splash 476–478 = part of record 869.
- **Splash now plays record 869** (three layers: 169 at 75 % probability + one of 476–478 + one of 171/172).
- Setup exports each SPLC bank's patch tree in `audio_manifest.json` (`patches`, manifest v3).
- PR #4's own pop/landing ids are different, layered patches (e.g. container 1097 → records 745–750, each
  5 single-member layers from 260–267 / 635–657 / 1000–1015); they come from game tuning values we do not
  read. Not adopted; the user's picks stay.

## How it was tuned (condensed session history)

Each step came from the user's report of an in-play session plus the `AUDIO_*` log lines of that session.

1. **First pass:** push, foot drag, footsteps silent — the animation events are cleared inside the tick
   before any ECS system can see them → `AudioEvents` latch. Bails silent (`board_wiping_out` is reset by the
   ground phase) → WipeoutGround state. Grinds silent (raw `category_12`) → `player_state.current()`.
2. **Flood:** the next log showed **push fired 1,723 times** and steps ~60/s: push contact and
   `AudibleFootStepStrength` stay set for many ticks. That flood caused scratchiness, odd pick-up sounds and
   likely the hitches (now fixed). → rising edges, per-cue minimum gaps, preloading.
3. **Footsteps:** a `SKATE_AUDIO_TRACE=1` run showed `AudibleFootStepStrength` is a **held level**
   (2.5–4, higher when running), **not a step event**; a peak detector fired only when the level changed.
   → foot strikes from foot clearance (planted 0.02–0.04 m, swing up to 0.4–0.55 m; 56 steps over ~16 s at
   ~6 m/s). Footstep clips are ~10 dB quieter than impacts → the headroom rule replaced "volume ≤ 1".
4. **Foot drag:** `brake_contact` foot-down **repeats every tick** while braking (47 and 139 ticks seen),
   then foot-up repeats while lifting (24–28 ticks); bit 28 is set by both, so the first latch also dragged
   on foot-up. → drag only between foot-down ticks, above 0.5 m/s, overlapping 0.2 s scuffs.
5. **Rolling:** grain files are speed sweeps → 6 bands. Then 53 restarts in one session (band jumps right
   after landings, concrete seams flickering) → smoothing, holds. On University's plywood ramp the sound
   "played twice": `AT=` confirmed the ramp is audio 41 (Wood_1) throughout, so the mapping was right; 36 of
   38 quick restarts were band changes → switch only a full band away, ≥ 1 s per band. Remaining doubling at
   low speed: on returning to the ramp the loop **restarted on the debounced old surface** (concrete) and
   switched to wood 0.25 s later → stop after 0.15 s off the ground and restart on the surface underneath.
   User: "much better".
6. **Board down:** every touch-down after stepping on played `land`, because the physical state leaves
   BipedGround a few ticks before the filtered category reaches Air → "began on foot" = on foot within 0.5 s;
   level by impact (0.18–0.45 m/s seen for gentle set-downs).
7. **Deck family (found by the user):** the user picked out `Skate_Collisions` 1066–1092 as "noises heard a
   lot while skating". Measured: 0.13–0.37 s knocks, most energy at 400–2500 Hz (the deck), unlike the low
   thumps either side (1060–1065, 1093–1100; 70–90 % of energy under 400 Hz). Pop = sharpest
   (1074–1078, 5–20 ms attack); land = 1088–1092; weight = 1097–1100. User: pop and landing "10000% better".
   Earlier rejected landings: 35–40 (harsh), 18–20.
8. **Splash (found by the user):** a full decode of all 7,434 streams scored for splash shape found no
   long splash; candidates (`water_misc`, `Skate_Collisions` 741–762) were rejected ("not water related at
   all"). The user found **`Skate_Collisions` 476** ("literally a splashing noise") on a candidate page;
   477–478 are its variations (same length 1.6–2.0 s and spectrum). 473–475 (shorter) were tried for small
   falls and rejected → always 476–478. Each water entry also played a bail ~20 ms later → bail suppressed
   in water. Wading in then falling gave no fresh fast contact → a wipeout in water splashes by itself.
9. **Grinds:** sound followed the named state, which lags the selector → `grinding_316` / `leaving_317`.
   A University rail sounded like wood: `Skate_Metal` has no clip ≥ 1 s, so `GRINDS` (123 clips,
   1.4–2.6 s) holds every material. **By tonality** (energy share of the strongest 1 % of spectral bins)
   9 and 11 are among the least tonal (0.30–0.36), **54–56 the most tonal and brightest** (0.65–0.85,
   4–5.6 kHz) → 54–56 on metal.
10. **Bail:** `Bodyslide` 0–15 is three blocks of five (one longer slide + four hits), heard as rising
    impact → tiers. In play they sound like **additions to a fall, not the body impact** → kept as a quiet
    sweetener layer (0.25) until a main impact is chosen (`.local/audio-audition/bail.html`; EA's scripts
    name body impacts per body part: `body_imp_Torso`, `_leg`, `_Head`, `_foot`).
11. **Grab whoosh** repeated while holding a grab (3 in 0.8 s): scoring re-announces a held grab with the
    same name → whoosh only for a new name in the air phase. Confirmed.
12. A game window that closed with no report was Windows Terminal crashing (the game was attached to its
    console via `PLAY.bat`), not the game.

## Files

- New: `tools/asset_pipeline/audio_formats.py`, `audio_export.py`, `test_audio_formats.py`,
  `tools/audio_audition.py`, `crates/skate-game/src/game_audio/{mod,library,voices,ambience,cues,skate_events,water}.rs`,
  `crates/skate-data/examples/audio_surfaces.rs`.
- Changed: `tools/asset_pipeline/{asset_exports,install,versions,group_receipts}.py`,
  `tools/asset_pipeline/test_versions.py` and `tools/test_setup_assets.py` (hand-written group lists),
  `crates/skate-core/src/physics/board_ground.rs` (+ test), `crates/skate-game/src/{main,app,config,graphics_menu,physics}.rs`,
  `physics/animation_input.rs` (audio event latch), `modding/audio.rs` (listener moved),
  `retail_character.rs` and `tests/map_transition.rs` (`Config.mute`), `.gitignore`.

## Verification

- `test_audio_formats` (synthetic streams only); setup tests 42/42; all tracked tools tests identical to the
  clean tree (the same 3 pre-existing import errors).
- Every SPLC bank: scanner count = declared count; every ABKC bank: scanner count = vgmstream subsongs.
- `cargo test`: `game_audio` (cue table sanity, bail tiers, band hysteresis, foot strikes, board-down levels,
  water bodies and fade, voice limits, headroom), `graphics_menu`, `modding::audio`, `map_transition`,
  `skate-core board_ground`.
- Export into the dev install: 22 beds, 14 grains (84 bands), 2 wheel spins, 23 banks; `--check-assets` →
  `SKATE_ASSETS_READY`.
- Muted smoke runs (`--mute`): the startup lines above, no panics, errors or missing-sink warnings.
- ~15 in-play sessions by the user (University, DownTown); confirmed items are marked in the cue table.
- **Not yet done:** full `regression-check` pass and a setup refresh through the installer path (only the
  `audio` group should rebuild).

## Open questions

- Main bail body-impact sound (Bodyslide is only a layer); landing weight layer level; metal grind pick
  (54–56) by ear; powerslide, wheel spin, push sound, fountain bank, caveman jump-on — not yet confirmed.
- Some surface → grain choices may be off; soft vs hard grains are probably wheel hardness (only hard used).
- Retail plays rolling granularly; this plays banded loops.
- Zone ambience from `.ems` emitters (beds per map are a name-based guess); manual wheel spin
  (`Whls_spins_Man_1`); music.
- ~615 MB of PCM (mostly ambience): a compressed format would need an encoder at setup.
- Retail's own sample choices could be measured from a recompiled build that logs which bank data the
  game decodes (local research only, not part of this change).
- `AUDIO_*` logging is verbose for a release build.
