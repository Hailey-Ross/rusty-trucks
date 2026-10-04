# Audio modding

Status: IMPLEMENTED (2026-10-04) for the scope chosen on 2026-10-03: the content overlay (R1), map audio as data (R2),
retail posts / globals / read-only MixMap (R3a), observe-only audio events (R5) and docs / examples (R6), on the audio
modding PR (draft upstream #36), which builds on #32 (the native audio engine) and doc 15 (the world-object surface).
Speech is included. Without audio mods the game sounds exactly as before: every phase was checked byte for byte
against the headless end-to-end renders (below). Later (not in this PR): tuning writes at run time, mod emitters and
reverb zones as world-audio objects, mod WAVs through the native mixer, mute / replace rules.

The guide (sections A–G) says what a mod can do and how. The design and research it was built from follow (sections
0–4, unchanged apart from status notes).

---

## A. What a mod can change, at a glance

| want | how | capability |
|---|---|---|
| replace or add retail sounds (sample slots, whole banks, Splice trees, grain members, wheel streams, ambience beds, speech takes), sets, zones, crossfades, emitter records, tuning fields | `audio.json` (data, no Lua needed) | `audio_content` = 1 |
| give a custom map audio (emitters, reverb zones, zone ambience, location sets, crossfades) | `<map>.audio.json` next to the `.skate`, a `.skate` `AUDO` extension, or a mod's `maps` section | `audio_content` = 1 |
| play a retail sound (post to a retail class), set a retail global, read MixMap outputs | `sdk.audio.post / redeliver / release / set_global / watch / handle / global / mixmap / info` | `audio` = 2 |
| react to the game's sounds (pop, land, grind, horn, …) | `sdk.audio.subscribe / events` | `audio_events` = 1 |
| play the mod's own WAVs (outside the native mixer, as before) | `sdk.audio.preload / play / update / stop / stop_all` | `audio` ≥ 1 |
| publish cars, peds, skaters to the world audio | `sdk.world_audio.*` (doc 15) | `world_audio` = 1 |

Test `(sdk.capabilities.audio or 0) >= 2` (and so on) before relying on a feature; an older engine lacks the keys.

## B. The content overlay: `audio.json`

A mod ships `audio.json` at its root, next to `mod.json`; files are mod-relative paths (`/`-separated, no `..`).
It applies **only while the mod runs**: when the mod starts, stops, is reloaded or its script fails, the game's sound
restarts once with the new set of overlays (a short cut in the sound). Overlays merge in mod-id order over the
install, by retail identity; **the first mod to change an identity wins** and later claims are listed in the mod menu
("Audio conflict") and the log. An entry the install does not have (an unknown bank, slot, set…) is skipped with a
warning, never an error. A whole overlay that fails its checks is not applied, the mod still runs, and the menu says
why.

```jsonc
{
  "version": 1,
  "replace": {
    "samples":   { "Skate_Collisions": { "3": "audio/pop.wav", "4": { "file": "audio/loop.wav", "loop_start": 1200 } } },
    "banks":     { "C04_taxi01": { "abk": "audio/C04_taxi01.abk", "samples": ["audio/t0.wav"], "group": "world" } },
    "splice":    { "Skate_Collisions": "audio/Skate_Collisions.splc" },
    "grains":    { "wood_ramp_hard": { "file": "audio/wood.wav", "grain": "audio/wood.grain" } },
    "wheels":    { "Whls_spins_Jump_1": "audio/spin.wav" },
    "ambience":  { "04_dt_main": "audio/dt_main.wav" },
    "mixmap":    "audio/MixMapSK8.mxb",
    "emitters":  { "sfx_downtown": { "9": { "position": [0, 0, 0], "extent": [18, 18, 18], "bank": "Baby_Cry_1", "patch": 22 } } },
    "random_sets": { "e_dwtn_spillway_brewery": { "sounds": [ { "bank": "Siren_city_8", "weight": 3 } ] } },
    "zones":     { "int_tunnel": { "bed": "22_interior_tunnel_amb", "volume": 0.5 } },
    "crossfades": [ { "from": "1F741D87EB58E84F", "to": "B6CE4BDEB63B6639", "group": 2 } ],
    "speech":    { "livingworld": { "501_41_adtm1_Warn_n.dat": { "0": "audio/warn.wav" } } }
  },
  "add": {
    "banks":     { "MOD_siren": { "samples": ["audio/siren.wav"], "preload": false, "group": "world" } },
    "emitters":  { "sfx_mymap": [ { "position": [1, 2, 3], "extent": [6, 6, 6], "bank": "MOD_siren" },
                                  { "position": [1, 2, 3], "extent": [12, 6, 12], "kind": 5, "reverb": "BEEFC8E3DE04FBAE" } ] },
    "random_sets": { "00000000000DE501": { "name": "my_sirens", "sounds": [ { "bank": "MOD_siren" } ], "min_interval": 5, "max_interval": 10 } },
    "zones":     { "00000000000DE502": { "name": "my_zone", "bed": "04_dt_main" } },
    "crossfades": [ { "from": "00000000000DE502", "to": "DB21C7A69325F3DF", "group": 1 } ],
    "speech":    { "livingworld": { "501_41_adtm1_Warn_n": ["audio/warn_extra.wav"] } },
    "location_programs": { "MOD_siren": [ { "sample": "shuffle", "level": 0.8, "pan_sweep": 30 } ] }
  },
  "tuning": { "world": { "traffic_engine": { "c04_taxi01": { "idle_rpm": 1200 } } } },
  "maps": { "MyMap": { "district": "DownTown", "ems": ["sfx_mymap"],
                       "regions": { "audio_emitters": [ { "box": [0, 0, 50, 50], "key": "my_sirens" } ] } } }
}
```

Identities and what each section does:

| section | identity | notes |
|---|---|---|
| `replace.samples` | bank stem + S10A slot | an AEMS bank's slot gets a sample header built from the WAV (rate, length, channels); a slot that looped loops from frame 0 unless `loop_start` is given. Programs with fixed timers may cut a longer replacement. Sample banks played by Bevy voices (location sets, crossfades) and Splice banks (pops, landings, foley: Splice builds headers from the PCM) take it as is. |
| `replace.banks` / `add.banks` | bank stem | the program (`abk`, optional for a replace) and / or the WAVs by slot. A new AEMS bank binds to the retail classes its exports name (e.g. a new engine bank bound to `TRAFFIC_CAR` with its own patch). `preload: true` loads it when the audio starts and keeps it across map changes; `group` = `player` (Effects volume) or `world` (Ambience volume). |
| `replace.splice` | bank stem | the whole Splice patch tree (expert). |
| `replace.grains` | grain member | the whole recording and its `.grain` member. |
| `replace.wheels` / `replace.ambience` | stream / bed name | beds may be up to 600 s. |
| `replace.mixmap` | the MixMap | the whole `.mxb` (expert). |
| `replace.emitters` / `add.emitters` | `.ems` file + record index | sound emitters (`kind` 1, a bank and patch) and reverb zones (`kind` 5, a reverb preset key). Added records join a file (a new file name makes a new file a map can list). |
| `replace.random_sets` / `zones` | 16-hex key or name | the whole record; a replacement keeps the record's name unless it gives one. `add` takes a new 16-hex key. |
| `replace.crossfades` / `add.crossfades` | zone pair (either order) | |
| `replace.speech` / `add.speech` | archive (`livingworld` = peds and NPC skaters, `maincast` = the pros and the special cast) + clip (with or without `.dat`) + take | added takes join the clip after its own, so the speech manager can pick them. |
| `add.location_programs` | bank | the layers a location-set post of the bank plays (the interim Bevy-voice player); a mod bank without a row plays one shuffle layer at level 1, a retail bank without one stays silent (as retail). |
| `tuning.player / world / bus / grain` | section + field path | field merges onto the install's tuning sections (e.g. `world.ped_objects` body-fall ids / tazer time, `world.speech_voice` peak filter / echo delay, `world.speech_tuning`); only fields the install has; arrays take decimal index keys; numbers replace numbers. An overlay whose merged tuning does not read back is left out whole. |
| `maps.<stem>` | map stem + field | see C. |

Files are checked when the mod starts (and by `check_mod`): WAVs must be PCM16, 1–2 channels, 8–48 kHz, at most 30 s
(beds 600 s, wheel / grain streams 60 s); `.abk`, `.splc`, `.mxb` and `.grain` files must parse. Limits: 64 MiB of PCM
per mod and 256 MiB for all running audio mods, 512 sample replacements, 64 banks, 1024 files and 4096 records per
mod. Licence note: overlays reference retail identities (names, keys, slot numbers); mods ship their own audio.

## C. Map audio (custom maps)

A map's audio is assembled when the map loads, in this order (later sources override the fields they set and append
their records and boxes):

1. **retail**: the map's database entry, read at run time from the stock collections every install has (the `world`
   row whose `WorldStream` is `DIST_<stem>` → its map entry: the `.ems` list and the crossfade bank). No setup refresh.
2. **the map's own definition**: the `.skate` `AUDO` extension (schema 1, UTF-8 JSON), else a `<map>.audio.json` file
   next to the `.skate`.
3. **mods**: `maps.<stem>` in `audio.json` (first mod by id per field).

All three use the same shape:

```json
{
  "district": "DownTown",
  "ems": ["sfx_downtown"],
  "crossfade_bank": "Main_Ambience_Crossfade_DT",
  "fallback_bed": "04_dt_main",
  "emitters": [ { "position": [0, 0, 0], "extent": [6, 6, 6], "bank": "Baby_Cry_1", "patch": 22 },
                { "position": [40, 0, 10], "extent": [20, 8, 20], "kind": 5, "reverb": "BEEFC8E3DE04FBAE" } ],
  "regions": {
    "audio_ambience": [ { "box": [0, 0, 64, 64], "key": "int_tunnel" } ],
    "audio_emitters": [ { "box": [0, 0, 128, 128], "key": "e_dwtn_spillway_brewery" } ],
    "audio_reverb":   [ { "box": [0, 0, 32, 32], "key": "BEEFC8E3DE04FBAE" } ]
  }
}
```

- `district`: whose retail world-painter regions apply (zone ambience, location sets, reverb). Default: the map's stem.
- `emitters`: extra `.ems`-style records after the files' (sound emitters and reverb zones, reached and released as
  retail's).
- `regions`: axis-aligned boxes `[centre x, centre z, half x, half z]` per layer, checked before the district's tiles.
  `key` is a 16-hex key or a zone / location-set name (reverb boxes take a preset key).
- `fallback_bed`: a bed for installs without zone data (not retail).

Mod files are not referenced from a map definition: a custom map uses retail identities, and new sounds come from an
audio mod (`add.banks`, `add.random_sets`, …).

## D. Runtime API (Lua and Rust)

`game_audio::mod_audio::AudioApi` is the same API for engine systems and mods; the mod commands call it.

```lua
sdk.audio.post('fountain', 'c_emitter', {32767, 30000, 0, 0, 4096, 25000, 0, 0, 22})
sdk.audio.redeliver('fountain', {32767, 20000, 0, 0, 4096, 25000, 0, 0, 22})
sdk.audio.release('fountain')
sdk.audio.set_global('babycry_1_sel_snd', 1)     -- nil restores
sdk.audio.watch{globals={'babycry_1_sel_snd'}, mixmap={{slot='emitter', object=0, instance=0, output=4}}}
local h = sdk.audio.handle('fountain')           -- {live=true, class='c_emitter'}
local v = sdk.audio.global('babycry_1_sel_snd')
local o = sdk.audio.mixmap('emitter', 0, 0, 4)   -- {level=…, raw=…, pitch=…, half=…}
local info = sdk.audio.info()                    -- native, generation, restarts, overlays, conflicts, map, limits, tags
sdk.engine.inspect('catalog', 'audio_catalog')   -- classes, functions, globals (values), loaded banks, map audio, sets, zones
```

- Posts are queued and applied at one fixed point, the start of the next audio pass (before the local player's
  process), so their order against the game's own posts never changes. A key's post replaces its last one. An unknown
  class or global is a command error (use `sdk.commands.request` to receive it as a result instead of failing the
  mod). `c_emitter`'s words are expert-level: the game's own emitters derive them from the MixMap (dry, send, pan,
  pitch, low-pass, …, patch).
- A handle dies at a map change and when the game's sound restarts for an audio content change: it reads `live =
  false` and the mod posts again (on `world_changed`). Dead ids are never released into the new runtime.
- Globals: the first mod to set one owns it; `nil`, the mod stopping and a map change restore the value seen before
  its first write; after a restart the override is applied again.
- Limits: 32 handles per mod, 128 in all, 16 posts per frame per mod, 32 payload words, 16 watched globals and 16
  watched MixMap outputs.
- Parity: a post runs the bank's program, which draws from the evaluator's one random generator as every retail post
  does, so with a mod posting the retail random sequence differs from a run without it. Without mod posts nothing
  changes.

## E. Audio events (observe only)

```lua
sdk.audio.subscribe{tags = {'pop', 'land'}}     -- {tags = {}} = every row; subscribe(nil) stops
for _, e in ipairs(sdk.audio.events()) do        -- last frame's rows, each frame once
  if e.tag == 'pop' then ... end                 -- e.kind, e.source, e.class, e.slot, e.id, e.owner, e.tag
end
```

Rows come from the post sites themselves: the local player's component posts / releases and Splice starts, the world
and NPC hosts (posts, releases, ped Splice steps, ped body falls and phone rings, the ped tazer, the NPC loose-board slide),
world emitter start / stop, zone ambience changes and speech line starts (class `speech` for the living world, `maincast`
for the main cast). Tags: `pop`, `land` (the board contacts' Splice starts with the Contacts tuning's pop and landing ids),
`grind_start`, `grind_end` (the grind slot's post / release), `footstep`, `horn`, `alarm`, `tazer` (a ped's `c_tazer`
post), `body_fall` (a ped's body-fall Splice start), `emitter`, `zone_change`, `speech`. Rows are one frame late; at most 256 a frame (`truncated` says when more happened). Nothing is recorded while
no mod subscribes. Muting or replacing a retail sound from an event is not possible yet (later: declarative rules).

## F. Tooling, lifecycle and the menu

- `cargo run -p skate-mods --example check_mod -- <package>` checks `mod.json`, the Lua syntax and `audio.json` in
  depth and prints a summary; `--install <assets folder>` also merges it over the install as the game does and lists
  unknown identities, `--with <package>` adds other audio mods to report conflicts.
- The game's native audio starts after the mod manager's first scan, so an audio mod enabled at boot costs no second
  start.
- The mod menu shows, for the selected mod, its `audio.json` load error, conflicts (the first by mod id wins) and
  skipped entries; with no mod selected, a count of audio problems.
- Example: `sdk/examples/audio-example` (self-made chimes as a location set on the format-demo map, a click layered on
  pops from the events). A larger dev test mod (off by default) is `mods/audio-content-test`.

## G. Implementation (for the upstream PR)

Problem: before this PR a mod could only play its own WAVs outside the native mixer and publish world objects; no
retail content could be replaced or added, custom maps had no emitters / zones / sets, and nothing could post a
retail sound or observe one. Root cause: retail audio data reached the engine through one install-only manifest
reader, the map → `.ems` lists and crossfade banks were code tables for the ten retail maps, and no runtime entry
existed outside `game_audio`.

Changes:
- `crates/skate-mods/src/audio_content.rs`: the `audio.json` schema (`deny_unknown_fields`), checks, per-kind WAV
  limits, deep file checks through the skate-audio parsers (skate-mods now depends on skate-audio, pure Rust).
  `audio_merge.rs`: the pure JSON merge by identity (first owner wins, conflicts, warnings); `check_mod`.
- `crates/skate-game/src/game_audio/library.rs`: `Library::load_with` (mod files as `mod:<id>/<path>`, per-file bank
  sources, sample headers rebuilt for mod WAVs in AEMS banks, speech take overrides, location programs, resident
  banks). With no overlays it reads the manifest exactly as before.
- `content.rs`: `AudioContent` (the running set, load errors, the merge report, the content generation), the native
  start after the first scan, the restart (new runtime continuing the old post ids with `map_epoch` + 1, fresh world /
  NPC / speech hosts: old handles forgotten, never released; stream respawned; the prefetch worker ends with the old
  runtime). Emitters, reverb zones, zone ambience and location sets key on the content generation.
- `map_audio.rs`: `MapAudio` (retail lookup from the stock collections, `AUDO` tag / sidecar, mod sections, box
  regions); `skate-data` `Field.array`; `CurrentMap.audio_tag`. The old code tables remain only as the test oracle.
- `mod_audio.rs`: `AudioApi` (posts, globals, watches, catalog, info) and the events (an observing Splice host
  wrapper, per-site buffers that exist only while a mod subscribes, a double buffer at the start of the pass).
- `skate-audio`: `Evaluator::next_node / continue_nodes`. `skate-mods` `vm.rs` / `api.lua`: the new commands and
  wrappers, capabilities `audio` = 2, `audio_content` = 1, `audio_events` = 1. Mod menu lines; `sdk/skate.lua`.

Verification:
- No-mod identity: the headless e2e bench (13 scripted scenarios and whole play sessions, the 60 Hz host and 300 fps;
  84 renders and voice logs) is byte-identical to the pre-PR build after every phase (R1, R2, R3, R5); the merge of the
  #32 work into this branch did not change it either, nor did the second merge (#32 at `cb71483` with
  optimisation pass 2 and the world gaps — tazer, ped body falls, speech echo, main-cast speech, NPC loose-board
  slide — plus upstream `main` `4488651`): the 84 outputs equal #32's own bench run of that tree. A data-gated test shows the merge path with an empty overlay
  reproduces the real install's manifest field for field.
- R1: a restart mid-scenario plays bit for bit like a fresh runtime from that point; a mod replacing an emitter bank's
  samples changes the output and removing it restores retail exactly; preloaded mod banks survive map changes;
  replaced AEMS samples get their own headers; Splice / grain / speech / set / zone / emitter overlays reach their
  readers; conflicts and rejected overlays are reported.
- R2: the run-time retail lookup reproduces the old tables for all ten maps (files, order, crossfade banks) and the
  reverb-zone records keep their ids; sidecar, tag and mod sections stack as described.
- R3: post limits, dead handles (map change, restart), global restore, watches; a mod's `c_emitter` post plays.
- R5: tags fire on real posts (pop, land, grind start / end through the real banks; horn, alarm, ped footsteps through
  the world host); nothing is recorded without a subscriber; frame timings equal within noise.
- World gaps (after the second merge): main-cast takes, the ped one-shot tuning, the speech echo delay and the
  `Tazer` bank's samples come from an overlay (`world_gaps_data_comes_from_the_overlay`, data-gated); ped ring /
  body-fall / foot-plant Splice starts, the tazer post (tag `tazer`, slot `ped_tazer`), body falls (tag `body_fall`)
  and main-cast line starts (speech rows with class `maincast`) are recorded for subscribers
  (`world_event_tags_fire_on_real_posts` sees `tazer` and both `body_fall` starts of a tazing, falling ped).
- Pre-existing failures unrelated to this PR: `setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`
  (listed in PULL-REQUESTS.md).

Open questions / later: tuning writes at run time (`sdk.audio.set_tuning`), mod emitters and reverb zones as world-audio
objects (emitter slots: retail's 5 or extra), mod WAVs through the native mixer (`native=true`, opt-in per sound),
declarative mute / replace / layer rules, crossfade layouts for mod crossfade banks (the interim table only knows
retail's three), a bank-level hot swap instead of the restart.

---

# Design and research (2026-10-03 / 04)

The plan this was built from. Sections 0–3 are the design (2026-10-03); section 4 holds the research for the chosen
scope: prior art, the engine facts each phase needed (file:line on the branch at a4ec831), the open questions and the
refined plan. Line numbers drift; each reference also names the item.

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
- **Gap found (fixed in R0, 2026-10-04):** `sdk/skate.lua` (the language-server declarations) had **no `sdk.audio.*` entries**; they exist
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
  "some mod uses it". The byte-identical e2e proof is required per phase.
- **The runtime restart** is the riskiest new mechanism. It reuses the map-change epoch reset (already tested) but
  also restarts the stream and the bed; R1 needs a test that a restart with an empty overlay gives byte-identical
  output from that point on.
- **Mod posts shift the shared RNG:** documented, mod-only.
- **Content licensing:** overlays reference retail identities, not retail data. Mods ship their own audio, and
  nothing copied from the game goes into the repo. Example mods use self-made
  sounds.
- **Concurrent edits:** the files are the same as the ongoing world / speech work and the optimisation pass, so this
  runs after them (#32 first).

## 4. Research for this PR (2026-10-04)

File:line references are this branch (code base a4ec831).

### 4.1 Prior art

- **Upstream and forks:** there is no earlier mod content-override or audio-modding work upstream (PRs and issues up
  to #36) or in the active forks. The closest precedent in the engine is `custom_difficulty.rs`, which overlays
  user values onto collections by hash identity.
- **Other games** (design ideas only, from their public documentation):

| project | mechanism | what this design takes from it |
|---|---|---|
| Minecraft resource packs | `sounds.json`: packs merge per sound event; `"replace": true` drops lower packs' entries | `replace` / `add` sections merged per identity |
| Source engine | `maps/<map>_level_sounds.txt` overrides sound scripts for one map | a per-map sidecar for custom maps |
| Garry's Mod | the `EntityEmitSound` hook sees every sound; it can suppress or modify it | observe in Lua now; suppress / replace later, as declarative native rules (Lua can't run inside the audio pass) |
| Factorio | data stage vs control stage; changing mods needs a restart | data overlay ≠ run-time API; a restart on a content change is accepted practice |
| Bethesda games, GZDoom | records by stable id or logical name, load order decides, conflict reports | stable identities; the winner and the loser are reported |
| RimWorld | patch operations on single fields of XML defs | field-level tuning merges |
| FMOD / Wwise mods, FiveM | extra banks registered beside the game's; data by hashed names | mod banks bound to an existing class; 16-hex keys |

### 4.2 Engine facts per phase

**R1 content overlay**
- *Shape:* `Library` (`game_audio/library.rs`) holds the install manifest (`Manifest` :298–333: ambience, grains,
  wheels, banks = WAVs by S10A slot, emitters, random_sets, zones, crossfades, regions, `aems` (projects, banks,
  mixmap, splice), grain / player / bus / world tuning). Unknown sections are ignored. Versions 3..=5 are accepted
  (:15).
- *Readers:*
  - every file is read through `Library::load` (:837), `clip` (:860, Bevy voices, cache keyed by relative path),
    `read` (:1005), `wheels_pcm` (:832), `splice_bank` (:1011), `bank_pcm` (:1020) and `bank_source` (:1027 →
    `BankSource`, one root, decoded on the prefetch worker);
  - their consumers are `native::start_with` (native.rs:232–333), `ensure_bank` (:338), emitters, reverb zones,
    ambience, location sets, the grain bed and the world / NPC hosts.
- *Merge point:* `Library::load_with(asset_root, overlays)`: the install manifest, then the overlays in mod-id order,
  with the first owner of an identity winning. Each file becomes an absolute path from its root (install or mod).
  This touches three places:
  - `BankSource` needs per-file paths, since a retail bank with one replaced WAV mixes roots;
  - the clip cache key must include the root;
  - replaced AEMS samples need new S10A headers. Voices take frames, rate, channels and loop start from the `.abk`
    header (mixer.rs:129–174, 385–391; `Runtime::load_bank` runtime.rs:80–85), so a replacement WAV of another
    length or rate would play wrong. Splice banks and the wheel streams already build their headers from the PCM
    (splice/mod.rs:209–221).
- *Validation:*
  - `deny_unknown_fields` schema;
  - mod paths through `read_bounded` (skate-mods archive.rs:150);
  - WAVs through `canonical_pcm_wav` (skate-mods audio.rs:76–139: PCM16, 1–2 channels, 8–48 kHz, ≤ 30 s, ≤ 8 MiB).
    Ambience beds need a larger per-kind cap;
  - `.abk` / `.splc` / `.mxb` through the skate-audio parsers;
  - unknown identities are a warning, never an error.
- *Conflicts:*
  - The existing "owned by another mod" cases are run-time claims that fail the second mod (engine_access.rs:71,
    modding/mod.rs:130 / 943, player_physics.rs:26 / 567, attachment.rs:19).
  - Static overlay conflicts need a new persistent list that the mod menu shows. `Manager.diagnostics` is cleared on
    every package scan (skate-mods lib.rs:106).
- *Lifecycle:*
  - The overlay is package state: active while the mod runs.
  - It changes on start, disable, reload (a package fingerprint change also reloads, so editing a file hot-reloads)
    and failure (the `retired` list, modding/mod.rs:797ff).
  - It survives map changes: `clear_runtime` (mod.rs:734) clears only run-time state.
  - Several changes in one frame cause one restart.
- *Boot order:* `Library` loads at Startup and the native runtime at PostStartup, but mods are first scanned on frame
  1. Starting the native runtime after the first scan avoids a second start when an audio mod is on at boot.
- *Restart* (on a change of the active overlay set):
  - rebuild `Library`;
  - despawn the stream's player (the sink holds the runtime);
  - start a new `Native` and spawn a new stream.
  - Two details are required for correctness:
    - The new evaluator continues the old node counter (`next_node` starts at 1, eval/mod.rs:174), and `map_epoch`
      becomes old + 1 (a new `Native` starts at 0). The world and NPC hosts release their held nodes when the epoch
      changes (world_sources.rs:309, npc_skaters.rs:105). With a restarted counter those stale ids would release
      the new runtime's live posts (node 1 is the `c_emitter_utility` boot post). Without the epoch bump, hosts
      that saw epoch 0 would never reset.
    - The emitters, reverb zones, ambience and location sets rebuild only when `(map name, generation)` changes
      (emitters.rs:113 / 316, ambience.rs:108), so an audio-content generation joins that identity. Nodes held
      from the old runtime are forgotten, not released.
- *check_mod:* today it validates only `mod.json` and the Lua syntax (`validate_package`, archive.rs:124). It gains an
  `audio.json` pass (schema, paths, WAV / format parsing, budgets) and a summary. With an install path, it also
  reports unknown identities and conflicts with other mods.
- *Speech:* the speech index and takes are not on this branch yet (they come with #32's world speech). The speech
  overlay and the speech event site follow when that lands here.

**R2 map audio as data**
- Retail keeps it in the stock collections (`skater-collections.json`, present in every install):
  - class `F4917ACACAFAF913`: 11 district records plus `default`;
    - field `65FA976EF23A314E` = the `.ems` files, e.g. dist_university: music_ / sfx_ / reverb_ / speakers_ /
      crowds_university; each park one `sfx_` file; skateschool `skateschool.ems`;
    - field `33526BC9D1C36B4A` = the crossfade bank (only downtown, industrial, university).
  - The `world` rows point at that record through field `99D6E51C9E20A663`. Park variants (full / empty / tutorial)
    share their park's record. DLC rows point at the empty `default`.
  - A map file `<stem>.skate` comes from `DIST_<stem>` (install.py:263–284). World rows carry `WorldStream =
    DIST_<stem>`, and setup's spawn stage already joins them that way (map_starts.py:58–75).
- Two ways in:
  - (A) a setup export: a `maps` table in the audio manifest (`audio_export.convert`). This takes one audio-group
    refresh. Leave the manifest version at 5: the table is optional, and a version bump would make older engines
    refuse the whole manifest.
  - (B) a run-time lookup in the collections the game already loads (`skate_data::collections::Collections`). No
    refresh.
  - Either way the current code tables (`ems_file` / `ems_files` emitters.rs:35–74, `crossfade_bank`
    ambience.rs:45) stay as the test oracle for the 10 maps.
- Custom maps today:
  - `.skate` files in `<install>/maps` and `maps/private` (map_library.rs:10–29).
  - Audio keys everything on the file stem, so a custom map gets no emitters, zones, location sets, regions or
    crossfades. Surface audio (rolling, footsteps) works, since materials carry an audio surface id.
  - SKATE v12+ files have tagged extensions (`{tag, schema, payload}`, skate_map.rs:119; `WMET`, `RWCM` in use),
    so an embedded audio extension is an alternative to a sidecar.
  - Mods cannot ship maps.

**R3(a) posts, globals, MixMap read**
- *API:*
  - `Runtime::post / redeliver / release` (runtime.rs:92–114);
  - `Evaluator::class_id / function_id / global_id` (eval/mod.rs:249–262);
  - `post` always succeeds, even with no bound bank; `release` is harmless on a dead node;
  - `set_global` notifies subscribers only on a change (:458); `global` (:471).
- *Threading:*
  - The runtime is `Arc<Mutex<Runtime>>`. The audio thread renders one block per lock and the evaluator ticks
    there. The game thread posts between blocks.
  - The MixMap is game-thread state in `Native`, without a lock. Reads: `level / filter_hz / raw / pitch_4096`
    (mixmap/mod.rs:426–461).
  - Mod commands apply in `modding::apply`, which is unordered against `native::mixmap_frame`. Posts are therefore
    queued and drained at a fixed point of the host pass.
- *Shared RNG:* every program draws from one generator (eval/mod.rs:143). A mod post that draws shifts every later
  retail draw. This is expected and only happens while a mod posts.
- *`c_emitter`:* its payload is MixMap-derived and needs an emitter state (`Native::emitter_payload` native.rs:507).
  A raw post is possible but unpositioned; positional mod emitters are the later `world_audio` kinds.
- *Limits and cleanup:*
  - handles per mod;
  - released on retire (beside `world_audio::clear_owner`, modding/mod.rs:805 / 878 / 904) and on a map change
    (`clear_runtime`);
  - a handle is dead after a map change or a restart and is never released by its stale id;
  - globals: original saved at the first write, restored on retire.
- *MixMap view:* the snapshot copies only the controllers mods asked for. A world object's MixMap instance comes from
  `sdk.world_audio.read(key).instance`.

**R5 observe-only events**
- The post sites are:
  1. `PlayerAudio::apply` (player_audio.rs:261, under the runtime lock);
  2. Splice starts through `rt.splice_host()` (player_audio.rs:207 / 397, npc_skaters.rs:241 / 268). A game-side
     wrapper keeps skate-audio unchanged;
  3. the world host's apply (world_sources.rs:256);
  4. the NPC host's own apply (npc_skaters.rs:74);
  5. the world emitters' `c_emitter` post (emitters.rs:407), plus zone / set changes;
  6. speech, later (see R1).
- *Zero cost:* an optional sink that exists only while a running mod subscribes; each site checks it once. Proof: the
  e2e hashes and bench timings.
- *Delivery:* the mod snapshot is built once a frame (`snapshot_ro`, modding/mod.rs:458), unordered against the audio
  pass, so the rows are double-buffered and arrive one frame late. They are filtered per mod by its subscription,
  ≤ 256 a frame with a truncation flag.

**R6 examples and docs**
- The upstream Skyline mod commits self-made WAVs together with the `synthesize.py` that makes them. The examples
  can do the same.
- `*.abk` is gitignored repo-wide, so examples prefer sample replacement and binding to existing classes.

### 4.3 Open questions (options)

1. **Mod emitters vs retail's 5 emitter slots** (a later phase):
   - share them (retail's first-reached rule);
   - or add extra MixMap Emitter instances only while a mod has emitters (instances 0–4 unchanged).
2. **Retail map list:** a setup export (one audio refresh) or the run-time collections lookup (no refresh). Both
   reproduce today's tables.
3. **Custom-map audio:**
   - Options: a sidecar `<map>.audio.json` next to the `.skate`, an audio extension inside the `.skate`, or both.
   - Content either way: retail identities only (`.ems` files by name, emitter and reverb-zone records, simple box
     regions for zone ambience / location sets / reverb, crossfade bank). WAVs come from mods.
4. **Example mods:** dev-only, or one small example shipped under `sdk/examples/` with synthesized sounds.
5. **When an overlay is active:** while the mod runs (a Lua failure removes it), or while it is enabled.
6. **Boot order:** start the native audio after the first mod scan, or accept one restart at boot.
7. **Speech:** ship the overlay without speech lines first, and add them when #32's speech lands.
8. **check_mod depth:** skate-mods may depend on skate-audio (pure Rust) so `check_mod` parses banks, Splice trees and
   the MixMap.
9. **Mute / replace rules:** later; declarative native rules.

### 4.4 Refined plan

| step | content |
|---|---|
| R0 | `sdk.audio.*` declared in `sdk/skate.lua`, the `audio` capability, `GENERAL_API.md`. e2e identity reference: the branch's headless e2e bench (13 scenarios + 8 whole sessions, row and 300 fps modes) is byte-identical to the base commit's |
| R1a | overlay types and validation (skate-mods), `check_mod` pass |
| R1b | `Library::load_with`, per-file roots, clip cache key, header rebuild for replaced AEMS samples; test `load_with(root, &[]) == load(root)` |
| R1c | active-overlay tracking, conflict list in the mod menu |
| R1d | restart: continued node counter, epoch + 1, content generation in the map identities, stream respawn, prefetch shutdown, native start after the first scan; test: a restart with an empty overlay renders like a fresh run |
| R1e | overlay-owned banks survive map changes (the keep-list in `Native::unload_map_banks`) |
| R2 | `MapAudio` per map (collections or manifest), sidecar, code tables as the oracle |
| R3(a) | queued posts, epoch / restart-safe handles, globals restore, catalog, MixMap watch list |
| R5 | optional sink at the five sites, double buffer, tags tested against real posts |
| R6 | doc 16, SDK docs, `make-mod`, examples |

Every step keeps the no-mod e2e output byte-identical to the R0 reference.

## Decisions (2026-10-03)

- **Scope of this PR:** the smaller cut: R0 + R1 (content overlay) + R2 (map audio as data) + R3(a) (posts, globals, a read-only MixMap view) + R5 observe-only (audio events) + R6 (docs, example mods). Tuning writes, mod emitters and reverb zones, mod WAVs in the native mixer and mute / replace rules follow later.
- **Turning an audio mod on or off** restarts the native audio (a short cut, as on a map change).
- **Mod WAVs through the native mixer:** opt-in per sound (`native = true`); existing mods behave as before.
- **Two mods replacing the same sound:** the first by mod id wins, with a warning in the mod menu.
- **Open:** see §4.3 (mod emitters' slots, the retail map list's source, the custom-map audio format, example mods, when an overlay is active, boot order, speech timing, check_mod depth, mute / replace rules).
