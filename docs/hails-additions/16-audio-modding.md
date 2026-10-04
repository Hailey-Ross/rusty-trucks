# Audio moddability pass (PR #32 final pass): design

Status: DESIGN (2026-10-03; nothing implemented yet). This is the plan for the audio modding PR. Builds on doc 15 /
`audio-specs/world-audio-hookin-spec.md` §8 (the world-object surface, done)
and `aems-evaluator.md` "Modular and moddable" (the 2026-10-02 goals). Standing rule (memory audio-moddable):
every audio feature must be moddable with the rest of the engine (Lua SDK, content layer), engine-hookable and
data-driven.

Line numbers are from the working tree on 2026-10-03, while `game_audio` was being edited; they drift, so
each reference also names the item.

---

## 0. Design in one paragraph

All retail audio data already flows through one place: `Library` (`game_audio/library.rs`), which reads
`audio_manifest.json` (banks, samples, AEMS projects / banks / MixMap / Splice, grains, wheels, emitters, location
sets, zones, crossfades, regions, player / grain / bus / world tuning, speech). Most of the moddability pass is
therefore **one content overlay at the `Library` level**: mods ship manifest-shaped fragments (`audio.json`) plus
files, merged mod first and then the install, by retail identity. With no audio mod active the overlay is empty and
`Library` is the same value it is today, so retail stays the default and byte-identical. Everything hard-coded
that is *data* (map → `.ems` files, beds, crossfade banks, bank lists, keep-list) moves into the manifest (setup
export) or into a code table that the overlay can extend. Behaviour that data can't express gets a small **runtime
API**, the same for Rust engine systems and Lua: retail posts / globals / MixMap reads, tuning overrides, emitters
and reverb zones as world-audio objects, mod WAVs routed through the native mixer, and audio events (observe in Lua,
suppress / replace through declarative native rules). The API follows the SDK conventions: commands validated
before allocation, keys and overrides owned by the calling mod, first owner wins, `nil` restores, per-mod and
global limits, capabilities, and cleanup on disable / reload / failure.

---

## 1. Inventory

Columns: **today** = what a mod can do now; **hard-coded** = what blocks it (file:line); **wanted** = what modders
would plausibly want.

Mod surface today (all features):
- `sdk.audio.preload / play / update / stop / stop_all`: the mod's own PCM16 WAVs as **Bevy voices outside the
  native mixer** (`skate-game/src/modding/audio.rs:1–40`). Limits 32 voices / 32 clips / 32 MiB per mod, 128 / 128 /
  128 MiB in all (`audio.rs:10–15`). They get no reverb, buses, ducking or retail distance curves; master volume and
  `--mute` apply via `GlobalVolume`.
- `sdk.world_audio.spawn / update / event / remove / read / info` (capability `world_audio` = 1,
  `skate-mods/src/world_audio.rs`, `modding/world_audio.rs`): traffic, peds, skaters (lite or ghost), horn / alarm /
  speech events. 48 objects per mod, 128 in all.
- **Gap found:** `sdk/skate.lua` (the language-server declarations) has **no `sdk.audio.*` entries**; they exist
  only in `skate-mods/src/api.lua:266–288`. `sdk.audio` also has no capability entry (only `sdk.audio.version = 1`).
  `sdk/AGENTS.md`, `GENERAL_API.md` and `ENGINE_API.md` don't mention audio.
- No mod can replace or add retail content, post retail sounds, read or write globals or the MixMap, change tuning,
  or see audio events.

### 1.1 Player audio

| feature | today | hard-coded | wanted |
|---|---|---|---|
| Player components (grind, sense of speed, foot drag, skid, squeaks, seams, rolling layers, rattle, board slide, tricks, treatment, footsteps, clothing, body slide) | nothing | bank lists `player_audio.rs:559–572` (`BANKS` = `components::BANKS` `skate-audio/src/player/components.rs:67`, `ROLLING_/RATTLE_/SLIDE_/TRICKS_/TREATMENT_BANKS`, `SPLICE_BANKS`, `WHEEL_STREAMS`); class names are `&'static str` constants (`rolling.rs:27–29`, `seams.rs:46`, `treatment.rs:20–21`, `footsteps.rs:44–45`, `Command::Post { class: &'static str }` `components.rs:61`); banks bound once in `Native::load_player_banks` / `load_optional_player_banks` (`native.rs:369–459`) | replace a grind / pop / landing / trick sound (sample or bank); change grind surfaces or per-material sounds; silence or layer a component |
| Splice (pops, landings, touchdowns, foot taps, scuffs, collisions) | nothing | `SPLICE_BANKS` (`player_audio.rs:570`); Splice trees only from `manifest.aems.splice` (`library.rs:1035`) | replace members (samples) or a whole patch tree; custom pop sound |
| Player tuning (surface table, jitter, seams, grind surfaces, landing materials, audio tricks, collision materials, rolling, tricks, treatment; contacts / wheels / footsteps / clothing tuning) | nothing | read **once** at start: `PlayerAudio::new(library.player_tuning(), true)` (`native.rs:~284`), `contacts_tuning`, `footstep_materials` (`native.rs:~413–416`); `PlayerTuning` has no serde (`skate-audio/src/player/tuning.rs:98–99`) | tweak per-material sounds, grind surface levels, trick sound ids; read values for HUD / debug mods |
| Wheel spins (SFXObj_Wheels) | nothing | `WHEEL_STREAMS` (`player_audio.rs:572`), loaded once | replace the spin recordings |
| Grain bed (granular rolling) | nothing | member lists `grain_bed.rs:52–70` (`MEMBERS`, `SOFT_MEMBERS`, `ROCKET`); tag → member `grain_for` is a code `match` (`grain_bed.rs:~76–97`) although the manifest has `grain_player.surface_map` (`library.rs:994`); bed built once (`native.rs:~282`) | replace a surface's recording (e.g. custom wood); map a new material to a member; tweak grain tuning |
| Trick → audio trick ids | nothing | `PlayerTuning::audio_tricks / _2` (setup export) | give a mod trick its own sound |

### 1.2 Mixer, buses, MixMap

| feature | today | hard-coded | wanted |
|---|---|---|---|
| Buses / reverb (env network presets, eEQChain, FlangeSub, FootStep SubMix) | nothing | presets / EQ / flange read once from `manifest.bus_tuning` in `Native::start_with` (`native.rs:~305–322`); SubMix always on | change reverb presets (a cave mod), EQ; route mod sounds through them |
| Reverb zones (`.ems` eVolumeType 5) | nothing | `emitters.rs:132` `zone_records` from `ems_files(map)` (`emitters.rs:55–74`) | add / move reverb zones (custom maps, mod buildings) |
| MixMap (curves, controllers, instance layout) | nothing | the `.mxb` from the install only (`native.rs:~263`); layout `RETAIL_INSTANCES` (`skate-audio/src/mixmap/mod.rs:31`), `WorldInstances::RETAIL / MORE_AUDIBLE` (`native.rs:227–240`), `PLAYER_INSTANCES` / `AUDIO_RADIUS` (`world/skaters.rs:75–78`), `TRAFFIC_ / PEDESTRIAN_INSTANCES` (`world/keys.rs:9–10`) | read outputs (level / distance of a car) for HUDs; replace the whole `.mxb` (expert); more instances (already a user setting) |
| Volume groups / categories | menu only | `AudioSettings` (`game_audio/mod.rs`), `GROUP_WORLD / GROUP_PLAYER` (`skate-audio/src/mixer.rs:293–294`) | duck the world or player group (music / cinematic mods) |
| Host clock | n/a | `MIX_STEP`, `MAX_STEPS_PER_FRAME = 4` (`native.rs:42, 50`) | **nothing; engine-only by design** (the console cadence) |

### 1.3 World: emitters, location sets, ambience

| feature | today | hard-coded | wanted |
|---|---|---|---|
| World emitters (`.ems` type 1 via `c_emitter`) | nothing (mods use Bevy voices instead) | map → files `ems_file` / `ems_files` (`emitters.rs:35–74`, the 10 retail maps only; a custom map gets **no** emitters); records only from `manifest.emitters`; `MAX_ACTIVE = 5` (`emitters.rs:32`) = the MixMap Emitter instances (`native.rs:94` `EMITTER_STATES`) | add / move / remove emitters; emitters for custom maps; a positional retail sound (siren, fountain) from a mod |
| Location sets (random distant one-shots) | nothing | `LOADED = 2`, `PAN_DISTANCE` (`random_sets.rs:24–26`); measured layers per bank `random_programs.rs:7` (interim, Bevy voices; a bank without a row is **silent**, `random_sets.rs:41–43`); district = map stem for the region lookup; `SKATE_AUDIO_SET` env (`random_sets.rs:126`) | change a set's sounds / weights / intervals; new sets for custom maps; force a set from a mod (instead of the env var) |
| Zone ambience + crossfades | nothing | `BEDS` map → bed fallback (`ambience.rs:27–43`), `crossfade_bank` district table (`ambience.rs:45–52`), `BED_BASE` (`ambience.rs:22`), measured crossfade layers `crossfade_groups.rs:7` (interim, Bevy voices) | replace beds; new zones / beds for custom maps; change fades |
| Regions (world-painter `audio_emitters` / `audio_ambience` tiles) | nothing | only from `manifest.regions[district]` (`library.rs:931`) | regions for custom maps |
| Bank unload on map change | n/a | keep-list in `Native::unload_map_banks` (`native.rs:471–482`: `emitter_utility`, `Common`, the player banks) | a mod bank that must survive a map change |

### 1.4 World / NPC sources and speech

| feature | today | hard-coded | wanted |
|---|---|---|---|
| Traffic (engine, horn, skid, alarm) | spawn / update / event (`world_audio` 1) | banks `TRAFFIC_BANKS` (`skate-audio/src/world/mod.rs:69–81`); classes `traffic.rs:28–31`; list radius `TRAFFIC_LIST_RADIUS = 40` (`world_sources.rs:339`) | a custom engine sound (new bank bound to `TRAFFIC_CAR` with its own patch); tweak `aud_traffic_engine` records |
| Peds (footsteps, body fall, tazer) | spawn / update | `PED_BANKS` (`world/mod.rs:82`), classes `peds.rs:27–31` (`livingword_footstep`, `sk8_foley` step ids), `PED_LIST_RADIUS = 50` (`world_sources.rs:342`), `FOOTSTEP_PEDS = 3`, `PED_FAR_THRESHOLD = 20` (`world_bridge.rs:33–35`); ped footstep tuning read once at the first owner (`world_sources.rs:523`) | new shoe sounds; tweak footstep tuning |
| NPC skaters | spawn lite / ghost | 1 instance within 30 m (retail); the more-audible setting (start-time only) | (covered by world_audio) |
| Speech (ped lines, NPC bail grunt) | `event(key,'speech')` triggers lines | index / rules / takes from setup only (`library.speech`, `world_speech.rs:190, 361`); manager tuning `SpeechManager::new(library.world_tuning().speech_tuning())` (`world_speech.rs:360`); `STREAMS = 2`, `QUEUE = 16` (`speech_player.rs:44–46`); `BAIL_GRUNT_EVENT = 8206`, `KEEP_TAKES = 24` (`world_speech.rs:39–41`) | add lines / takes to a voice and event; new voices; tweak event probability / timers |
| World RNG | n/a | `Lcg(0x5EED)` / `Lcg(0x5EEC)` (`world_sources.rs:308`, `world_speech.rs:113`) | seed for reproducible mod tests (minor) |
| Env-only switches | n/a | `SKATE_AEMS_WORLD`, `SKATE_AEMS_WORLD_PREFETCH` (`world_sources.rs:214–221`), `SKATE_AEMS_NPC_SKATERS` (`npc_skaters.rs:50`), `SKATE_AUDIO_MORE_AUDIBLE` (`mod.rs`), `SKATE_AUDIO_SET`; dev: `SKATE_AUDIO_TRACE / TIMING / STATE_LOG(S)` | `SKATE_AUDIO_SET` as an API (debug mods); the rest stay dev switches (not mod-facing) |

### 1.5 Cross-cutting

- **No event access:** posts happen at four choke points a hook can tap without touching retail logic:
  `PlayerAudio::apply` (`player_audio.rs:~259–292`, every component post / redeliver / release with its `Slot`
  and class), `SpliceAccess::start` (`skate-audio/src/runtime.rs:267`, every Splice sound: bank + id), the
  world host's `WorldCommand` apply (`world/mod.rs:60–63`, owner + slot + class), and the speech host
  (`world_speech.rs`). Retail edges (pop, land, grind on / off, bail) are detected inside the components, so
  hooks should be keyed on these retail identities, not re-detected from physics.
- **No retail post / global access:** `Runtime::post / redeliver / release`, `Evaluator::class_id / global_id /
  set_global / global` exist (`runtime.rs:92–109, 249–257`; `eval/mod.rs:297, 458, 471`) but nothing outside
  `game_audio` reaches them.
- **Mod WAVs bypass the native mixer**, although the mixer can already open direct voices on a bank's samples
  with a route (`Mixer::add_bank`, `open_direct`, `open_routed`, `set_bank_sample` `mixer.rs:326–469`).
- **Read once at start:** AEMS projects, MixMap, bus tuning, player tuning, grain bed, player banks, Splice,
  wheel streams (`Native::start_with`). Content that changes after start needs a reload path.
- **Data path:** `Library::load` (`library.rs:861`) and `Library::bank_source` (`library.rs:1051`) read only the
  install. This is the single choke point for a content overlay.

---

## 2. Proposed surface

### 2.1 Content overlay (capability `audio_content` = 1)

**Where:** a mod ships `audio.json` at its root (not a `mod.json` key: `Manifest` is `deny_unknown_fields`
(`skate-mods/src/schema.rs`), so a new key would make the mod fail on older engines; a separate file is ignored
by them). Files are mod-relative, read with `read_bounded` like everything else.

**Shape:** sections with the same names and row shapes as `audio_manifest.json`, so the overlay is "a manifest
fragment" and setup's own exporters double as authoring tools:

```jsonc
{
  "version": 1,
  "replace": {
    "samples":   { "Skate_Collisions": { "1187": "audio/splash.wav" } },          // bank + sample index
    "banks":     { "C04_taxi01": { "abk": "audio/C04_taxi01.abk", "samples": ["audio/taxi_0.wav", "..."] } },
    "splice":    { "Skate_Collisions": "audio/Skate_Collisions.splc" },           // whole patch tree (expert)
    "grains":    { "wood_ramp_hard": { "file": "audio/wood.wav", "grain": "audio/wood.grn" } },
    "wheels":    { "Whls_spins_Jump_1": "audio/spin.wav" },
    "ambience":  { "04_dt_main": "audio/dt_main.wav" },
    "speech":    { "livingworld": { "takes": { "501_59_busm1_Warn_n": { "3": "audio/warn3.wav" } } } },
    "mixmap":    "audio/MixMapSK8.mxb"                                            // whole file (expert)
  },
  "add": {
    "banks":     { "MOD_siren": { "abk": "audio/MOD_siren.abk", "samples": ["audio/s0.wav"] } },
    "speech":    { "livingworld": { "lines": [ { "voice": 59, "event": "Warn", "takes": ["audio/w.wav"] } ] } },
    "random_sets": { "e_dwtn_spillway_brewery": { "entries": [ { "bank": "MOD_siren", "weight": 3 } ] } },
    "location_programs": { "MOD_siren": [ { "delay": 0, "sample": "shuffle", "level": 1 } ] }
  },
  "tuning": {                                    // field merges on setup's tuning sections; validated, typed
    "player":  { "grind": { "3": { "level": 0.8 } } },
    "world":   { "traffic_engine": { "c04_taxi01": { "...": 0 } }, "speech_tuning": { "1": { "53": { "probability": 0.5 } } } },
    "bus":     { "reverb": { "<preset key>": { "...": 0 } } }
  },
  "maps": { "MyCustomMap": { "...": "see 2.2" } }
}
```

Rules:
- **Resolution by retail identity, mod first, then the install.** Identities: bank stem; bank + sample index; Splice
  tree stem; grain member; wheel stream; ambience bed; speech archive + clip + take; set / zone key (hex) or name;
  emitter file + record index; tuning section + record name + field.
- **Order between mods:** packages in mod-id order (the manager's `BTreeMap`), **first owner wins**, and a conflict
  is reported in the mod menu diagnostics (the same rule as graph gates, rig parts and input overrides:
  "owned by another mod"). User decision Q4.
- **Validation before allocation** (`check_mod` too): schema with `deny_unknown_fields`; paths via
  `valid_audio_path`-style checks; WAVs through `canonical_pcm_wav` (PCM16, 1–2 ch, 8–48 kHz). The duration cap is
  30 s for samples, and longer only for `ambience` / `speech` takes, under the byte budget. `.abk` through
  `formats::Bank` parse (it already rejects opcodes ≥ 40 and bad block walks); `.splc` / `.mxb` / grains through their
  parsers. Budget per mod: 64 MiB of decoded PCM, 256 MiB in all; bounded counts (e.g. 512 sample replacements,
  64 banks per mod). A bad entry is skipped with a diagnostic and the retail sound stays.
- **A new bank needs a class to post to.** Phase R binds mod banks to an **existing** retail class (a new engine
  sound bound to `TRAFFIC_CAR` with its own patch number, a location-set bank posted by the set scheduler, an
  emitter bank posted through `c_emitter`). New Csis classes (a mod `.csi` project) come later (L4).
- **Interim location / crossfade programs:** a mod bank in a location set has no measured row in
  `random_programs.rs`, so `add.location_programs` supplies one. Without that row a mod bank gets a single shuffle
  layer at level 1. Retail banks are unchanged. These tables go away when the sets move onto the native evaluator
  (a parity item, not this pass).
- **When it applies (Q1):** at mod enable / disable / reload the overlay set changes. The `Library` is rebuilt
  (install manifest + overlays), and the native runtime **restarts at the next safe point**: `Native` is dropped and
  `start_with` runs again, the world hosts take the map-epoch reset, and the location / zone state is rebuilt. This
  is a short sound cut at enable / disable only. A bank-level hot swap (unload + `ensure_bank` + epoch, the map-change
  path) is an optimisation for later (L1). With no audio-content mod the rebuild never happens.

**Rust (engine side):** `AudioContent` resource: `register(owner, Overlay)` / `unregister(owner)`, with
`Overlay` the same parsed type the mod file becomes. `Library::load_with(asset_root, &[Overlay])` merges it, and
`Library::source(identity) -> (root, file)` replaces `root.join(file)` (a `FileRef { root: Install | Mod(id), path }`).
A future DLC or Skate 2 content pack (todos `dlc-support.md`, `skate2-support.md`) can use the same overlay.

### 2.2 Data-driven map tables (custom maps get audio)

Move the per-map code tables into data:

| today (code) | new home |
|---|---|
| `ems_file` / `ems_files` (`emitters.rs:35–74`) | manifest `maps.<stem>.ems` from setup: retail's own list, the map database entry `F4917ACACAFAF913` field `65FA976EF23A314E`, which the code comment already names. The code table stays only as a test oracle (`every_map_has_its_emitter_file`). |
| `BEDS` fallback (`ambience.rs:27`) | `maps.<stem>.fallback_bed`. It is not retail data, so it stays only for installs without zones. |
| `crossfade_bank` (`ambience.rs:45`) | `maps.<stem>.crossfade_bank` (setup export) |
| district = map stem for regions | `maps.<stem>.district` (default: the stem) |

A map's audio = `MapAudio { ems: [files], emitters: [records], reverb_zones, regions, zones, random_sets,
crossfade_bank, fallback_bed }`, assembled in this order:
1. the install (retail maps);
2. a **sidecar** `<map>.audio.json` next to a custom `.skate` file (map authors). Q7;
3. mod overlays `maps.<stem>` (first owner wins per section).

`CurrentMap` changes already rebuild emitter / zone / set state; they now read `MapAudio` instead of the code
tables. **Rust:** a `MapAudio` resource, inserted by the map loader. An engine map importer (Skate 2, DLC) fills it
directly.

### 2.3 Runtime API (Lua and Rust)

All new Lua functions go into `api.lua` (wrappers that `submit{kind=…}`) and `sdk/skate.lua` (declarations), with
validated `Command` variants in `vm.rs` and handlers in `skate-game/src/modding/audio_*.rs`. Each has a Rust
equivalent, and the mod command handler calls it, as `world_audio` does.

**(a) Retail posts, globals, catalog** (capability `audio` = 2)
```lua
local h = sdk.audio.post('emit1', 'c_emitter', {words})  -- key-scoped handle; words: ≤ 32 i32
sdk.audio.redeliver('emit1', {words})
sdk.audio.release('emit1')
sdk.audio.set_global('g_name', value)    -- nil restores the value seen before the first write
local v = sdk.audio.global('g_name')     -- snapshot read
local o = sdk.audio.mixmap(slot_key, output) -- read-only MixMap output (level, filter Hz, pitch) of the last pass
sdk.engine.inspect('audio_catalog', 'audio') -- classes, functions, globals, loaded banks, map ems files, sets, zones, tuning names
```
- Limits: 32 held handles per mod, 16 posts per frame per mod, 128 handles in all. Unknown class / global →
  command error (so `sdk.commands.request` reports it). Handles are released on remove / disable / reload / failure
  and on map change (the epoch reset: the handle reads `released` and the mod re-posts).
- Globals: first owner wins; originals are restored on disable.
- **Parity note:** a mod post runs the bank's program, which draws from the evaluator's shared RNG (as every retail
  post does). With a mod posting, the retail random sequence differs from the no-mod run. This is expected and
  documented; with no mod nothing changes.
- **Rust:** `AudioPosts` system param (`post(owner, class, &words) -> AudioHandle`, `redeliver`, `release`),
  `AudioGlobals`, `MixMapReadback` resource (the last pass's outputs per key), `AudioCatalog`.

**(b) Tuning read / write** (capability `audio_tuning` = 1)
```lua
local t = sdk.audio.tuning('world.traffic_engine', 'c04_taxi01')    -- table or nil
sdk.audio.set_tuning('world.traffic_engine', 'c04_taxi01', {field=v}) -- merge; nil restores retail
```
- Domains, each a typed serde struct with `deny_unknown_fields` and finite / range checks:
  - `player.surface`, `player.grind`, `player.rolling`, `player.tricks`, `player.treatment`, `player.collision`,
    `player.contacts`, `player.wheels`, `player.footsteps`, `player.clothing`;
  - `grain.surface`;
  - `world.traffic_engine`, `world.ped_footsteps`, `world.speech_event`, `world.ped_model`, `world.traffic_model`;
  - `bus.reverb`, `bus.eq`, `bus.flange`.
- Applied between passes (no restart): `PlayerAudio.tuning` and the sub-tunings are fields today. The world host's
  `ped_tuning` is reset to re-read, the speech manager is rebuilt, and bus presets are swapped under the runtime lock.
- **Not tunable:** the MixMap's shape (instance layout, 30 m / 40 m / 50 m pools, 5 emitter states). That needs a
  restart and a deliberate non-retail option (as "more audible" is). Q3 covers the emitter pool.
- Static tuning changes can also ship in `audio.json` `tuning` (2.1). The runtime call is for dynamic mods.
- **Rust:** `AudioTuning` resource: `get(domain, name)`, `set_override(owner, domain, name, patch)`,
  `clear(owner)`.

**(c) Emitters and reverb zones as world-audio objects** (`world_audio` = 2)

These are two new kinds in the existing lifecycle, so the keys, limits, parking, `read` and cleanup come for free:
```lua
sdk.world_audio.spawn('fountain', 'emitter', {bank='Water_fountain', patch=3, position=p,
                       extent={6,6,6}, core=1.5, volume=0.8, falloff='squared'|'linear'|'flat', body=nil})
sdk.world_audio.spawn('cave', 'reverb_zone', {preset='<aud_reverb key or name>', position=p, extent=e, forward=f})
```
- An `emitter` is an `.ems` type-1 record added to the map's live list: the same reach test (sphere / ellipsoid,
  inner core), retail falloff curve, `c_emitter` post with MixMap Emitter payload, release on leave
  (`emitters.rs` `update`). It goes through the reverb, buses and ducking like retail emitters. A `reverb_zone` joins
  `ReverbZones` in reach order (eVolumeType 5).
- Emitter pool (Q3): **default = share retail's 5 Emitter states** (retail's rule: first reached is served first).
  The option is extra MixMap Emitter instances only while a mod has emitters; instances 0–4 are unchanged.
- **Rust:** components `WorldEmitter { bank, patch, extent, core, volume, falloff }` and `ReverbZoneVolume
  { preset, extent }` on an entity with a `GlobalTransform`. The map's own records are spawned the same way at map
  load (or kept as data; the bridge merges both lists).

**(d) Mod WAVs through the native mixer** (`audio` = 2)

This keeps the existing commands working. New optional `audio_play` fields (additive; an older engine rejects them
because the options are `deny_unknown_fields`, hence the capability bump):
```lua
sdk.audio.play('beep', {path='a.wav', position=p, native=true, group='world'|'player',
                        falloff={radius=30, curve='squared'}, reverb=true})
```
- A mod's preloaded clips become one **mod bank** per mod in the runtime (`Mixer::add_bank` with headers built from
  the WAV). Voices open with `open_routed` on the SFX route with the env tap (reverb by the current zone), are
  panned by the listener azimuth (`native::azimuth`) and get the chosen retail falloff curve. Pitch is the
  resampler's (≤ 4×, as validated already). Master / category volume and `--mute` apply as for retail sounds.
- Default stays the Bevy voice (`native=false`), so existing mods sound unchanged. Q2: flip the default later?
- The same limits as today (voices / clips / bytes per mod and total) are counted across both paths.
- **Rust:** `ModVoices` on the runtime: `open(owner, clip, params) / set / stop`. Engine UI sounds can use it too.

**(e) Audio events and rules** (capability `audio_events` = 1)

- **Observe (Lua, one frame late):** the choke points (§1.5) append compact rows to a per-frame buffer, **built only
  while some mod subscribes**:
  - fields: `kind` (`post`, `release`, `splice`, `speech`, `zone`, `emitter_enter` / `leave`, `world_claim` /
    `release`), `source` (`player` / `world` / `npc`), `class` or `bank`, `id` / `patch`, `slot`, `owner` (world key
    if mod-owned), `position` if known;
  - **named tags** from a small native table on retail identities: `pop`, `land`, `grind_start`, `grind_end`,
    `bail`, `splash`, `footstep`, `seam`, `trick`, `horn`, `alarm`, `speech_request`, `zone_change`.
  - Delivered as `sdk.snapshot.audio_events` (array, ≤ 256 per frame, truncation flagged) plus an optional
    `on_audio_event(event)` callback per row (≤ 64 per frame per mod).
  - `sdk.audio.subscribe{tags={...}}` limits what a mod pays for.
- **Suppress / replace / layer (native, same frame):** Lua cannot run inside the audio pass, so changes are
  **declarative rules**, evaluated at the choke points:
  ```lua
  sdk.audio.rule('quiet_pop', {match={tag='pop'}, action='suppress'})
  sdk.audio.rule('my_pop',    {match={tag='pop'}, action='replace', play={path='pop.wav', native=true}})
  sdk.audio.rule('layer',     {match={class='TRAFFIC_HORN'}, action='layer', post={class='c_emitter', words={...}}})
  sdk.audio.rule('quiet_pop', nil)  -- remove
  ```
  32 rules per mod. First owner wins per match key. Rules are removed on disable. With no rules the cost is one
  `is_empty()` branch per choke point.
- **Rust:** Bevy messages `AudioEvent` (same rows; engine systems read them with a `MessageReader`) and an
  `AudioRules` resource (`insert(owner, key, Rule)`, `clear(owner)`).

**(f) Small additions**
- `sdk.audio.set_group_volume('world'|'player'|'ambience', 0..1)`: a multiplier on top of the menu values, first owner
  wins, restored on disable (ducking for music / cinematic mods). Rust: `AudioDucking`.
- `sdk.audio.force_location_set(name|nil)`: replaces the `SKATE_AUDIO_SET` env var as an API; the env var stays a
  dev override.
- `sdk.audio.info()`: native on / off, map epoch, loaded banks, overlay owners and conflicts, limits, layout.
- `sdk/skate.lua`: declare the existing `sdk.audio.*` (the gap in §1).

### 2.4 Capabilities, limits, cleanup (summary)

| capability | covers |
|---|---|
| `audio` = 2 | `sdk.audio.*`: v1 commands unchanged, plus native routing, post / release / global / mixmap, ducking, info |
| `audio_content` = 1 | `audio.json` overlay, map sidecars |
| `audio_tuning` = 1 | tuning read / write |
| `audio_events` = 1 | event rows, `on_audio_event`, rules |
| `world_audio` = 2 | adds `emitter` and `reverb_zone` kinds |

Cleanup on disable / reload / failure goes through the existing retire path (`modding/mod.rs:797–906`, beside
`audio::stop_owner` / `world_audio::clear_owner`): release handles, restore globals, drop tuning overrides and
rules, despawn emitters and zones, unregister the overlay (→ restart if it had content), release the mod bank.
Commands from a failed callback are dropped (existing rule). 128 commands per callback (existing).

---

## 3. Phases

Effort is in focused working days, including tests and docs. The ordering keeps the no-mod path identical at every
step.

### "Ready" for PR #32 (proposed; the user decides the cut, Q5)

| phase | content | effort | key risks |
|---|---|---|---|
| **R0 baseline + docs gap** | declare `sdk.audio.*` in `sdk/skate.lua`; `sdk.audio.info()`; `audio` catalog for `sdk.engine.inspect`; capture the e2e baseline (13 scenarios + whole sessions, row / fps300) for the byte-identical proofs | 0.5 | none |
| **R1 content overlay** | `audio.json` schema + validation (`check_mod` too); `Library::load_with` + `FileRef` roots; replace / add for samples, banks, Splice, grains, wheels, ambience, speech takes and lines, sets, zones, emitters, MixMap file, tuning sections; restart-on-change; mod banks kept across map changes (keep-list → data: banks owned by an overlay are kept); conflicts in diagnostics | 3 | the restart path (stream handover, the prefetch worker, the world epoch); speech index merge; budgets |
| **R2 map tables** | setup export of `maps.<stem>` (retail's map → `.ems` list, crossfade bank, district); `MapAudio` resource; sidecar `<map>.audio.json`; the code tables become test oracles | 1.5 | needs a setup refresh (audio group); must reproduce the code tables exactly (test) |
| **R3 posts / globals / mixmap / tuning** | 2.3 (a) + (b) + (f) | 2.5 | the shared-RNG note; tuning applied mid-session must not tear a pass (apply between passes) |
| **R4 emitters, reverb zones, native mod WAVs** | 2.3 (c) + (d) | 2.5 | the emitter pool decision; mod bank lifetime vs runtime restart; pan / falloff matching retail emitters |
| **R5 events + rules** | 2.3 (e) | 2.5 | choke-point cost (must be 0 without subscribers, proven); tag table correctness (each tag tested against a real post) |
| **R6 examples, docs, proofs** | example mods (below); doc 16 "Audio modding" (+ doc 11 / 15 / PULL-REQUESTS rows); `make-mod` skill; SDK docs | 1.5 | — |

Total ≈ 14 days. A smaller cut (Q5) is **R0 + R1 + R2 + R3(a) + R5 (observe only) + R6** (≈ 9 days): every
feature's *data* is moddable, retail sounds can be posted, and events can be seen. Tuning writes at run time, mod
emitters, native WAV routing and rules would follow.

### Later

- **L1** bank-level hot swap instead of the runtime restart.
- **L2** writable MixMap inputs (mod ducking through retail's controllers rather than a group multiplier).
- **L3** extra MixMap instances for mod objects beyond the more-audible setting.
- **L4** mod Csis projects (new classes, functions, globals).
- **L5** a seedable world RNG for reproducible mod tests.
- **L6** flip `native=true` as the default for mod WAVs (Q2).
- **L7** hot reload of `audio.json` while the mod runs (the manager already fingerprints packages; changes then
  trigger R1's rebuild).

### Tests and proofs

- **No-mod identity:**
  - `Library::load_with(root, &[])` equals `Library::load(root)` (field-wise, data-gated);
  - the e2e bench byte-identical before / after each phase (R0 baseline vs each phase, 13 scenarios + sessions, row
    and fps300);
  - the oracle tests (evaluator, MixMap, DSP, grain, splice) unchanged;
  - R2: the exported `maps` table equals the old code tables for all 10 maps.
- **Choke-point cost:** with no subscribers / rules, the bench's per-pass timings match within noise and no
  allocation happens in the hook (skill optimisation §3b review).
- **Per feature, one mod-driven test** (headless, data-gated where needed):
  - sample replace → the voice reads the mod PCM;
  - bank replace / add → class binding and the patch plays;
  - Splice member replace; grain member replace; speech take add → the manager picks it;
  - set / zone / emitter overlay → the scheduler uses it;
  - custom-map sidecar → emitters and zones on a test map;
  - post / global / mixmap read; tuning set and restore (the output changes, and after `nil` it matches the
    baseline again);
  - emitter object audible within reach, released on leave; reverb zone raises the env send;
  - native WAV voice routed with the env tap;
  - each event tag fires on its real post; rule suppress / replace / layer.
- **Lifecycle:**
  - disable / reload / failing mod → the overlay is gone, the runtime is restarted, and the e2e output equals the
    no-mod baseline (the "removed mod restores retail" proof);
  - map change with mod handles (epoch);
  - two mods on one identity → first wins with a diagnostic.
- **Validation:** `check_mod` rejects bad `audio.json` (unknown field, path escape, bad WAV, oversize, unknown
  identity reported as a warning). `vm.rs` serde-boundary tests for every new command (valid / invalid / unknown
  field), as for `world_audio`.
- **Example mods** (dev-only under `mods/` unless the user says otherwise, Q8):
  - `audio-custom-pop`: sample replace + a rule (replace pop) + an event-driven HUD line;
  - `audio-louder-horns`: tuning + a bank add bound to `TRAFFIC_CAR`;
  - `audio-siren-emitter`: an emitter object + a reverb zone + `sdk.audio.post`;
  - a custom-map sidecar sample in the docs.

### Risks (overall)

- **Retail parity stays the default:** the overlay and rules are empty without mods, and every new path is gated on
  "some mod uses it". The byte-identical e2e proof is required per phase (memory: optimise / change only with proof).
- **The runtime restart** is the riskiest new mechanism. It reuses the map-change epoch reset (already tested) but
  also restarts the stream and the bed; R1 needs a test that a restart with an empty overlay gives byte-identical
  output from that point on.
- **Mod posts shift the shared RNG:** documented, mod-only.
- **Content licensing:** overlays reference retail identities, not retail data. Mods ship their own audio, and
  nothing copied from the game goes into the repo (memory: no copied game content). Example mods use self-made
  sounds.
- **Concurrent edits:** the files are the same as the ongoing world / speech work and the optimisation pass, so this
  runs sequentially after them (pr32-ready order).

## Decisions (2026-10-03)

- **Scope of this PR:** the smaller cut: R0 + R1 (content overlay) + R2 (map audio as data) + R3(a) (posts, globals, a read-only MixMap view) + R5 observe-only (audio events) + R6 (docs, example mods). Tuning writes, mod emitters and reverb zones, mod WAVs in the native mixer and mute / replace rules follow later.
- **Turning an audio mod on or off** restarts the native audio (a short cut, as on a map change).
- **Mod WAVs through the native mixer:** opt-in per sound (`native = true`); existing mods behave as before.
- **Two mods replacing the same sound:** the first by mod id wins, with a warning in the mod menu.
- **Open:** mod emitters' slots, mute / replace rules, a `<map>.audio.json` sidecar for custom maps (and the one audio setup refresh the retail map list then needs), whether the example mods ship.
