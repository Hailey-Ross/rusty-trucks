# Hooking up world audio (traffic, pedestrians, NPC skaters)

Part of PR #32 (branch `gameplay/audio`; doc [11](11-audio.md) has the ported audio itself).

## Problem

Skate 3's living world has its own sounds: traffic engines with Doppler, horns, skids and car
alarms; pedestrians' footsteps and speech; and the board sounds of an AI skater near the camera. The
native audio port (`crates/skate-audio`, `skate_audio::world`) already plays all of them the way
retail does. But the engine has no traffic, pedestrian or AI-skater system yet. Nothing upstream or
in any fork has one, and no issue asks for one (prior-work check, 2026-10-03). The sounds therefore
had no way in.

The goal (the user, 2026-10-03): "build out everything that can then easily be hooked to by the game
engine once they are added", in this PR, and moddable like the rest of the engine.

## The change in one paragraph

A future engine system adds **components** to its own entities: `TrafficAudio`, `PedAudio`,
`NpcSkaterAudio`, plus an optional `AudioVelocity`. It sends a few **messages**: `PedSpeechEvent`,
`VehicleHorn` and `VehicleAlarm`. A **bridge** turns these into the audio hosts' inputs every frame.
The hosts apply **retail's limits** to decide who is audible, and the bridge marks the audible
entities with `WorldAudioInstance`. Lua mods reach the same path through `sdk.world_audio`. A dev
mod, `mods/world-audio-test`, makes it all audible now.

## Engine surface (`crates/skate-game/src/world_audio.rs`)

Position and heading come from the entity's `GlobalTransform`: its +Z axis is the forward
direction, the game's convention, and retail's vehicle `+112` is likewise the world matrix's forward
row (recomp gap run G1). Velocity comes from the transform's change per frame unless the entity has
an `AudioVelocity`. Give one to anything that teleports or is a kinematic proxy, because Doppler and
the 3-D rates read it.

**Lifetime = the entity.** To publish, insert the component. To release, despawn the entity or
remove the component. Owner ids are `Entity::to_bits()`, so a reused index counts as a new owner.

### `TrafficAudio` (retail `SFXObj_TrafficEngine` / `TrafficHorn` / `TrafficSkids`)

| field | retail | meaning | who fills it | default |
|---|---|---|---|---|
| `engine` | vehicle record `+168` | the `aud_traffic_engine` record by name, **or the living-world model** (`taxi01`, `sedan02`, `suv`, `minivan01`, …), which setup maps through retail's attribute chain (entity → vehicle spec → record, G1; export `world_tuning.traffic_models`): sedans / hatchback → `c01_family01`, sports / muscle → `c03_sports01`, taxi / patrol → `c04_taxi01`, SUV / pickup / minivan → `c05_truck01`. The engine's patch override picks c06 / c07 / c08 itself. | the vehicle system, from the model | — (an unknown name is logged once and stays silent) |
| `speed` | `+148` | m/s | the vehicle system | the length of the velocity |
| `load` | `+144` | the driver's signed acceleration in m/s². G1 measured −15.6 in a hard stop, up to +3 pulling away, and 0 while cruising. It goes into the engine and skid words × 3000. | the driving AI | derived from the speed change |
| `horn` | `+156` | `None`, `Honk(1..=5)` or `Alarm` | the AI (honk decisions) | `None` |
| `skidding` | `+160` | the tyres skid | the AI (hard braking or a swerve) | false |

### `PedAudio` (retail `SFXObj_PedestrianSFX` / `PedestrianSpeech`, ped state S, G2)

| field | retail | meaning | default |
|---|---|---|---|
| `voice` | `S+84` | the model = the speech voice id 1–96 (41–96 living-world peds; a pro 1–29 or the special cast 30–38 speaks on the main-cast channel). Its `aud_characteristics` record (setup export `world_tuning.ped_models`) gives the shoe class, the security kind, the speech type / variant / gender words and the far threshold (G2) | none (no speech, the defaults below) |
| `shoe_class` | `S+132` | 1–5 (`None` = the model's). Most models are 2; no model uses 1, which is silent. | the model's, else 2 |
| `weight` | `[obj+28]+144` | 1–5. It changes per ped in retail (1 beyond about 12 m, 2–5 nearer) and its meaning is open. | 1 |
| `close_range` | `S+96 == 64` | the security guards' close-range footstep levels and their speech levels (`None` = the model's type bit) | the model's, else false |
| `feet_down` | `S+74` / `S+73` | the walk animation's foot plants (A, B) | up |
| `foot_materials` | `S+140` / `S+144` | the audio surface materials under the feet | 0 (retail's pavements read 0 in every line) |
| `footsteps_on` | `S+68` | footsteps on | retail's rule: the 3 nearest peds in the list |
| `speech_distance` | `S+148` / `S+156` | distance to the listener / the model's far threshold. Beyond the threshold the far `_f` lines are used. | the 3-D distance / the model's threshold (20 m for regular peds, 30 for pros) |
| `speech_value` | `S+136` | the state graph's speech value | 0. Send `PedSpeechEvent` rather than writing it. |
| `tazing` | `S+80` | the ped tazes (retail: its state graph's `TazeEntity` state): SFXObj_Tazer holds `c_tazer`, the zap burst | false. `PedTazerEvent` holds it for a time. |
| `body_fall` | `S+76` | the ped animation's `BodyFallType` channel: each change to a non-zero value starts one PedBodyFall sound (8, 9, other) | 0. `PedBodyFallEvent` sends one key. |

### `NpcSkaterAudio` (the MixMap Player slot's second instance)

| field | meaning | default |
|---|---|---|
| `list_order` | the skater list position (retail walks its list in order) | spawn order |
| `state` | this frame's `AudioState`. A skater simulated with the player's physics uses `game_audio::skate_events::skater_audio_state(physics, skater, &mut memory, dt)`, the same builder the local player uses (proved identical, below). Anything else uses `AudioState::rolling(&LiteSkater { .. })`: speed, wheels, materials, grind / air flags. With that fill, rolling, surfaces, seams, grinds and landings sound; tricks and foot / body foley stay silent. | none (not published) |
| `remote` | a remote multiplayer player (see below) | false |
| `voice` | the skater's speech voice: the AI skaters' models 89–96 (living-world channel) or a pro 1–29 / the special cast 30–38 (main-cast channel). Its bail grunt and its reactions say lines of it | none |
| `reactions` | the skater record's reaction bytes this frame (`SkaterReactions`: a slam seen and by whom, a second slam reaction, a trick seen and by whom, its own crash, the chase flag): its speech process (`skate_audio::world::skater_speech`) says `101_pos` / `104_slam` / `150_pro_pos` / `151_pro_slam` (pros) or `101_pos` / `404`–`405` / `105_spec_chase` (AI skaters), and `906_aislm` / `131_collide_object` on its crash | none. `NpcSkaterReactionEvent` raises one for a console frame. |
| `loose_board` | the conditioner's loose-board state (`+780`: 0, 1 upside down, 2 on its side; `rolling::loose_board`): the board slide holds while set | 0 (ghosts: from their log) |

### Messages, resources and the read-back

- `PedSpeechEvent { ped, value: SpeechValue }`: sets the ped's speech value, so PedestrianSpeech sees
  the change and requests a line. A repeat of the current value goes through 0 for one frame, so it
  speaks again. `SpeechValue::from_name` accepts the state-graph names (`DoWarning`, `LongCheer`, …),
  the short names `warn` / `cheer` / `slam` / `flee` / `knockdown` / `nearby`, and numbers.
  **`warn` is 53** (`SpeechValue::WARN`): the code's warn, which the manager maps to `501_warn`. The
  state graph's `DoWarning` (11) entry is commented out in retail ("moved to the code"), and 11 is
  also `LostInterestEndChase`, which maps to `605_chase_terminate`.
- `VehicleHorn { vehicle, kind, seconds }`: holds horn kind `kind` for the caller's time. The length
  is the AI's choice, not retail data.
- `VehicleAlarm { vehicle }`: retail's alarm, horn state 6 for **8 s**.
- `PedTazerEvent { ped, seconds }`: the ped zaps: `tazing` held for `seconds` (None = the state graph's
  `TazerCycTime`, setup `world_tuning.ped_objects.tazer_seconds`, 2.0 s).
- `PedBodyFallEvent { ped, kind }`: one `BodyFallType` key of a knock-down animation. Keys queue; each is on for
  one console frame with a frame of 0 after it, so repeated keys sound.
- `NpcSkaterReactionEvent { skater, reaction, by }`: an NPC skater reacts (`SkaterReaction::{Slam, SlamB, Trick,
  Crash, Chase}`, `by` = the other skater's model, 0 = the player); held for one console frame.
- `LivingWorldAudio { expected, photo_flag }`: set `expected` at map load when the system will publish (the
  world banks then decode on the prefetch worker). `photo_flag` is the game flag of the photographer's repeat
  (a ped holding speech value 29 repeats it every second; retail system byte `+912`, meaning not traced).
- `WorldAudioInstance { slot, instance }` (read-back): the bridge inserts it on the entities that
  hold a MixMap instance. Only those are audible. An engine system can use it to skip per-frame
  audio work for the others, such as an NPC's `AudioState`.
- `WorldAudioStats`: published and audible counts, the instance layout, and whether the
  "more audible" setting is on.

### Who is audible (retail's limits, applied by the hosts)

| pool | instances | rule | source |
|---|---|---|---|
| Traffic | 4 | the 4 nearest within **40 m**, measured horizontally | G1: every holder was among the 4 nearest in 311 of 311 holder-seconds; the list is cut at 39.994 m |
| Pedestrians | 15 | the 15 nearest within **50 m**; footsteps for the **3** nearest | G2: manager `sub_824F2890`; `+68 == (index < 3)` in 2,803 of 2,803 lines |
| NPC skater | 1 | the first in list order within **30 m** of the camera, held until it is 30 m away or more | `sub_824F1FB0` / `sub_824F8EF8` |

**Opt-in "more audible" setting (not retail; user decision 2026-10-03):** `settings/audio.json`
`"more_audible_world": true`, or `SKATE_AUDIO_MORE_AUDIBLE=1` for one run. The MixMap is then built
with 8 traffic, 24 pedestrian and 4 Player instances (3 NPC / remote skaters). The extra objects
get retail's lookups, curves and posts; only their number is not retail. Instance 0 of every slot
is unchanged. The setting is read at start. The runtime has one NPC grain bed, so only NPC instance
1 has granular rolling. Default: off (retail).

**Remote multiplayer players (not retail; user decision 2026-10-03):** another real player nearby
takes the NPC skater instance, ahead of the NPCs in the list order. Retail's online behaviour is
not traced. The remote state carries only the body and pose, so the bridge gives remote players a
lite state from their root transform. The material under them comes from the same stock line query
the wheel lines use.

**No traffic-light or crossing sounds:** Skate 3 has none (the user, 2026-10-03).

### Example: an engine system

```rust
use crate::world_audio::*;

// A traffic system spawning a taxi (heading = the entity's +Z):
commands.spawn((Transform::from_xyz(10.0, 0.0, 4.0), TrafficAudio::new("c04_taxi01")));

// Its driving AI each frame:
fn drive(mut cars: Query<(&mut Transform, &mut TrafficAudio, &Car)>) {
    for (mut t, mut audio, car) in &mut cars {
        t.translation = car.position;
        audio.load = Some(car.acceleration); // m/s², or None to derive it
        audio.skidding = car.acceleration < -8.0;
    }
}

// The AI honks at a skater in the lane, a car is hit:
honks.write(VehicleHorn { vehicle, kind: 2, seconds: 0.8 });
alarms.write(VehicleAlarm { vehicle });

// A ped system: walk animation foot plants, reactions from its state graph:
commands.spawn((transform, PedAudio { voice: Some(59), ..Default::default() })); // shoe class, kind, words: the model's
speech.write(PedSpeechEvent { ped, value: SpeechValue::WARN });

// An AI skater simulated with the player's physics, near the camera:
npc.state = Some(skate_events::skater_audio_state(&physics, &skater, &mut memory, dt));
```

## Mod surface (`sdk.world_audio`, API 2, capability `world_audio` = 1)

Mods publish through the same components: each key becomes an entity. The rules follow the existing
mod rules. Keys belong to the calling mod. The limits are **48 objects per mod and 128 in all** (raised from 16 / 64 the same day: the
test mod publishes 37 objects; retail's pools pick the audible few anyway, so the limits only
bound the per-frame work, a distance per object and one sort).
Options are validated before anything is allocated, and a field of another kind is an error. An
object that is not updated for **0.5 s is parked** (speed 0, feet up, horn off). Disabling,
reloading or a failing mod removes everything it published. The existing `sdk.audio.*` (the mod's
own WAVs) is unchanged.

```lua
sdk.world_audio.spawn('car1', 'traffic', {engine='c04_taxi01', position=p, heading=h})
sdk.world_audio.update('car1', {position=p, velocity=v, speed=12, load=-3, skidding=false})
sdk.world_audio.event('car1', 'horn', {kind=3, seconds=1.2})   -- or 'alarm' (8 s)
sdk.world_audio.spawn('taxi', 'traffic', {engine='c04_taxi01', body='chassis'})  -- a mod car opts in to a retail engine sound
sdk.world_audio.spawn('ped1', 'ped', {voice=59, shoe_class=3, position=p})
sdk.world_audio.update('ped1', {position=p, feet={true,false}})
sdk.world_audio.event('ped1', 'speech', {value='warn'})        -- name or number (49: a phone call)
sdk.world_audio.event('ped1', 'tazer')                         -- the zap burst (seconds = 2 by default)
sdk.world_audio.event('ped1', 'body_fall', {kind=9})           -- one knock-down key (8, 9, other)
sdk.world_audio.update('ped1', {tazing=true, photo_flag=true}) -- held tazing; the photographer's game flag
sdk.world_audio.update('npc1', {loose_board=1})                -- lite skater: the board slide
sdk.world_audio.event('npc1', 'reaction', {value='trick', by=0}) -- it saw the player's trick (slam, slam_b, trick, crash, chase)
sdk.world_audio.update('npc1', {voice=24})                      -- a pro's voice: the main-cast channel
sdk.world_audio.spawn('npc1', 'skater', {position=p, speed=6})  -- lite: rolling from these fields
sdk.world_audio.spawn('ghost', 'skater', {source='state_log:state_20261003_143434', from=30, seconds=20, position=p})
local r = sdk.world_audio.read('car1')   -- {kind='traffic', audible=true, instance=2, parked=false}
local info = sdk.world_audio.info()      -- {more_audible=false, instances={traffic=4, peds=15, skaters=1}, ...}
sdk.world_audio.remove('car1')
```

`body=` makes an object follow one of the mod's physics bodies (position, rotation, velocity). This
is how a mod car opts in to the retail traffic engine sound; that choice is the user's
(2026-10-03) and is not retail. A **ghost** replays one of the user's recorded audio state logs
(`SKATE_AUDIO_STATE_LOG` writes them) at full fidelity. It reads `logs/<name>.tsv` in the mod
folder, or `<name>.tsv` in the folder named by `SKATE_AUDIO_STATE_LOGS`. A log with any malformed
line is refused.

## The dev test publisher (`mods/world-audio-test/`)

The mod is dev-only: it should not ship upstream unless the user asks. It drives everything around
the spot where the mod starts:

- **16 cars** on four rectangular lanes, cycling through the nine engine records `c00`–`c08`. Each
  car accelerates at +2.5 m/s², cruises at 8–15 m/s for 3–8 s, then brakes: hard (−12 m/s², with
  skids) or soft (−4). One car honks every 10–15 s with kind 1–5. One taxi is parked, and **F8**
  fires its alarm.
- **20 peds** on back-and-forth lines: 16 walk (1.3 m/s), 3 jog (4) and 1 runs (8), so the three
  `sk8_foley` step ids play. A gait clock alternates the feet; it is dev-only, not retail. Each ped is
  one of retail's 35 living-world models (its voice), so its shoe class, kind and speech words are the
  model's. Speech (it plays when the speech decode is installed):
  - warn (53, `501_warn`) when you pass within 1.5 m at more than 3 m/s;
  - cheer (23, `101_pos`) on a landed trick within 10 m;
  - slam (25, `104_spec_slam`) on a bail within 10 m;
  - every 6–10 s the nearest ped shouts (2, `1901_shout`; dev-only chatter, not retail). The
    manager's own tuning still gates every line (the shout: 50 % and 30 s per voice), and nothing
    speaks in the first 30 s after the game starts (retail's timers start at 0).
- **A ghost NPC skater** replays the 20 s window of a recorded state log, 5 m beside the start, with
  voice 91 (an AI skater's): its Wheels streams, Clothing foley and bail grunt play as an NPC's.
  Settings: `ghost_log`, `ghost_from`.
- **Off by default** (`"enabled_by_default": false`, a new optional `mod.json` field): enable it in the
  mod menu, or run with `SKATE3_MODS_ENABLE=dev-world-audio-test`. While it publishes, the game logs one
  `WORLD_AUDIO cars N/audible M, peds N/M (footsteps K), skaters N/M, posts +P (npc +Q), speech lines +S`
  line per second.
- Debug **boxes**: green = holds an instance (audible), grey = not. An on-screen line shows the
  audible counts, the limits and the ghost's status. **F9** re-centres everything on the skater.

## Proofs and tests

- **The map-change bug (P0).** `unload_map_banks` (a map change) destroyed every instance of the 13
  world banks. `WorldHost` kept its dead nodes and "already posted" objects, so an owner that
  survived the change redelivered to a dead node and stayed silent. The fix:
  - `Native::map_epoch` is bumped by `unload_map_banks`;
  - on a change both hosts release every node, clear their pools, deactivate the 3DObjPos blocks,
    stop the ped Splice steps and the NPC bed, and drop their objects;
  - the evaluation count is `saturating_sub`;
  - the evaluation `dt` follows the MixMap cadence: 1/30 per console evaluation. The old 60 Hz
    mode, where `CONSOLE_DT` doubled it, was removed with the A/B switches the same day (doc 11).

  Test `world_owners_post_again_after_a_map_change` (data-gated): a taxi and a ped publish, the C04
  engine sounds, the map changes, and the same ids post afresh and sound again. Without the epoch
  reset the test fails ("the same owner sounds again after the map change"). The ghost test below
  checks the NPC host's reset the same way.
- **The per-skater builder** (`skater_audio_state`, `SkaterAudioMemory`) is a pure move of
  `observe`'s code. Test `audio_state_capture` (data-gated) drives 1,500 production physics steps
  headless (push, carves, two ollies, a powerslide, a roll-out) and runs `observe` after each. Its
  published samples are **identical line for line** to a capture taken before the refactor. On
  every step the builder, run on its own memory, also gives the same `AudioState` as `observe`.
- **The local player's output:** the e2e bench (13 scenarios and 4 whole sessions, row and fps300)
  is **byte-identical** before and after this work (`wa_base` vs `wa_new`). The state-log replay
  that e2e uses was moved, unchanged, into `game_audio/state_replay.rs` so the ghost can use it.
- Bridge unit tests (a minimal Bevy app):
  - components become owners;
  - the ped footsteps-on and far-threshold rules hold, and remote players come first;
  - the alarm, horn and speech messages work;
  - the read-back inserts and removes `WorldAudioInstance`;
  - despawning releases, and nothing happens without components.
- `a_ghost_claims_instance_1_releases_and_survives_a_map_change` (data-gated): a real user log as a
  ghost holds Player instance 1 within 30 m and sounds. It is released at 400 m, and after a map
  change it claims instance 1 and sounds again.
- skate-mods:
  - command validation per kind, and the event options;
  - the Lua wrappers cross the serde boundary;
  - the bundled test mod runs 400 frames with no Lua error or invalid command, stays within 128
    commands per callback, and its F9 re-centre spawns everything again.

  `check_mod` validates the mod.

## Files

- New: `crates/skate-game/src/world_audio.rs`, `game_audio/world_bridge.rs`,
  `game_audio/state_replay.rs`, `crates/skate-game/src/modding/world_audio.rs`,
  `crates/skate-mods/src/world_audio.rs`, `crates/skate-game/src/tests/audio_state_capture.rs`,
  `mods/world-audio-test/{mod.json,main.lua}`.
- Changed:
  - `game_audio/{world_sources,npc_skaters,native,skate_events,e2e,mod}.rs`;
  - `skate_audio::{player::state (LiteSkater, AudioState::rolling), world::skaters (Slots::with_records)}`;
  - `skate-mods` `vm.rs`, `api.lua`, `lib.rs`;
  - `modding/mod.rs`;
  - `sdk/skate.lua`;
  - `.gitignore` (`mods/world-audio-test/logs/`).

## P3–P5: speech, the NPC instance, traffic, per-model data, retail's order (2026-10-03, headless)

Everything below was built and checked headless against the existing recomp recordings. Neither the game
nor the recomp was launched, and no new recording was made. "The recomp" numbers come from the recordings,
not from a console.

### Ped speech plays (`skate_audio::world::speech_player`, `game_audio/world_speech.rs`)

**How it works.** A ped's speech value change becomes a PedestrianSpeech request (ported before). The speech
manager maps it to an event and gates it (also ported before). The library then picks the record and its takes.
New in this pass:

- **The line plays on one of the living world's two streams.** The interrupt `sub_824A73F0` indexes the
  channel's stream records as `channel × 2 + k`, and the recomp's living-world lines play on two stream
  players.
- **When both streams are busy, the event's tuning decides.** With `+13` it stops a playing line of lower
  priority; with `+14` it does the same when the channel is full. Otherwise the request waits in the
  16-request queue until the event's queue timeout runs out.
- **The stream follows its speaker every console frame.** This was read from the code, not fitted: the
  PedestrianSpeech update `sub_824D9370` copies its outputs into the block that the line's stream reads.
  - Main level: out2. A clip whose name ends in `_f` uses out3 instead (`sub_824A89E8` compares the name with
    `"_f"`).
  - Reverb send: out15.
  - Azimuth: raw out0. Pitch: out1.
  - High pass: out13. Low pass: out14.
  - Security guards, the guard radio and the conversation states use other outputs (spec note
    `world-speech.md`).
- **The cut.** A speaker whose level stays at or below 200 for more than 60 frames has its line stopped
  (vault `6995C510258C9AF6` / `3D8CD05C962FF399`). A speaker that loses its MixMap instance stops at once.
- **The NPC bail grunt.** It goes through the same manager and streams. Its level comes from the skater's
  PlayerSpeech instance (`sub_824DA300`: out2 / out3, send out10, filters out8 / out9).
- **The manager's clock is console time since the start.** Retail's per-speaker timers start at 0, so nothing
  speaks in the first 30 s: the warn's not-follow list holds it, and the shout has its own 30 s.

**`SpeechValue::WARN` is now 53**, the code's warn, which the manager maps to `501_warn`. The state graph's
`DoWarning` (11) entry is commented out in retail. 11 is also `LostInterestEndChase` → `605_chase_terminate`.

**Data and failure modes.**
- The index and rules are part of the normal setup.
- The takes need the opt-in decode (`SKATE_SETUP_SPEECH=1`, about 2.4 GB). The dev install now has all 40
  free-roam events: 13,322 takes, 2.3 GB.
- A take is read from disk when its line starts, as retail streams it. The last 24 stay loaded.
- Without the index, speech is off and the log says so once.
- Without the decode, lines are still chosen and logged but stay silent, and the log says so once.

**Checked against the recomp.**
- *End to end* (`a_ped_warns_through_a_stream_at_its_owner_levels`, data-gated): a business man (voice 59)
  warns 3.6 m from the camera and gets `501_59_busm1_Warn_n`; its stream plays at out2 / 32767. At 30 m the
  next warn is `Warn_f` at out3.
- *Levels* (`speech_levels_follow_the_recomp`, from `recomp-research/tools/speech_levels.py` on 163809 /
  164620 / 180430): 39 lines with a joined speaker. Each one was rebuilt at its recorded geometry (ped, camera,
  player).
  - The recomp's stream filters sit at exactly our out14 / out13: 24956 / 77 Hz near a speaker, 3489 / 379 Hz
    far away.
  - First LPF / HPF within 10 % in 4 of the 5 lines that log both.
  - First GAIN: recomp / ours median 1.005 (p10 0.55, p90 1.43, n 34).
  - `_f` lines at 30 / 40 m: 0.092 / 0.044 against our 0.085 / 0.056. out2 alone would be 0.030 / 0.001.
  - SEND within 0.01 in only 10 of 37 lines (open).
- *Overlap*: the recomp shows two persistent stream players (163809: 40 + 6 lines; 164620: 95 + 48; 180430:
  28 + 17).

### The NPC skater's instance: Wheels, Clothing, the bail grunt

Recomp gap run G3 showed which components run for the NPC instance. Now ported:

- **Wheels:** the spin-down streams on layers 0 / 1. Layer 2 is local only.
- **Clothing:** push / plant foley, body slide, cloth falls, with its non-local start block.
- **The bail grunt:** the body poster's first message of a bail (`+422`, once per bail) → event 8206 for the
  skater's voice (`NpcSkaterAudio::voice`, the AI skaters' 89–96).

Tricks, Treatment and the OffBoard steps stay off for NPCs, as in retail. Board slide was reached by neither
instance in the runs and is not run.

Data test `the_npc_instance_runs_wheels_clothing_and_the_bail_grunt`: a 476 s user log replayed as an NPC.
- Wheel streams: 0.21 starts/s (recomp NPC 0.24 per held second, other motion).
- Clothing: 0.39 Splice starts/s (recomp 0.27).
- 8 grunts for 8 bails.
- The local player's components posted nothing.

### Pedestrians: per-model data
- **Setup export `world_tuning.ped_models`** (`world_audio.ped_models`, from `aud_characteristics`, keyed by the
  voice): variant, kind, gender, shoe class, far threshold, and a per-voice float. 82 models.
- **The bridge fills these from `PedAudio::voice`:** the shoe class (which sets the footstep samples), the
  security kind (footstep and speech levels), the speech words and the far threshold.
- **Checked** (`recomp-research/tools/ped_models_check.py`) against every PEDAUD line of the gap runs: 20 models,
  3146 of 3146 lines agree on all five fields.
- **Footsteps** still play only for list index < 3 (already in the bridge).

### Traffic
- **Model → record:** setup export `world_tuning.traffic_models` (27 entities). `TrafficAudio::engine` accepts
  `taxi01`, `sedan02` and the like.
- **`TrafficCarPhysics.in0`:** the record writer's relative speed, min(|heading × speed − v_listener|, 35),
  slewed by 100 /s, × 32767 / 35. It opens A11.
- **3DObjPos blocks 1 / 2 / 3** follow the body and the front / rear points at ±1 m along the heading. The
  binding is inferred: block 2 feeds the engine layer, block 3 the exhaust.
- **Checked** (`traffic_rpm_and_relative_speed_follow_the_recomp`, gapg1 VEHAUD):
  - the RPM model lands within 60 RPM in 97.3 % of 451 live sample pairs (p50 0.7 RPM);
  - `+176` lands within 0.5 m/s in 99.2 % while the listener stands. While it moves, 29 of 59 land within 2 m/s;
    the listener's own velocity is not logged.
  - 696 pairs were frozen "stale" records of cars that had left the 40 m list. They were left out.

### Retail's order: process before the tick, update after
- **The world and NPC hosts now run inside the pass:** `native::mixmap_frame` (inputs, the local player's
  process) → `world_sources::frame` (pre) → `npc_skaters::frame_pre` → `native::mixmap_tick` (ticks, the
  local update) → … → the hosts' update → `world_speech::frame`.
- **Before, both ran after the ticks**, so the inputs a tick saw were one console frame old.
- **The local player's sequence is unchanged** (inputs → process → ticks → update), and the hosts touch nothing
  without owners.
- **Proof (`process_before_the_tick_shifts_the_outputs_by_one_evaluation`):** a car driving past the camera through the real MixMap, rendered in the old order (tick, update,
  process) and the new (process, tick, update), with the same start-up history (one empty first tick). The
  TrafficEngine / TrafficSkids / TrafficHorn outputs (level, raw, pitch and filter of 11 outputs each) satisfy
  new[k] == old[k + 1] on 89 of 89 evaluations. So the move is a pure one-evaluation shift: the tick now sees
  this frame's position.
- **The NPC bed now always steps after the local bed's step.** It draws from the local bed's generator, and
  before this the order of those two systems was not fixed.

### Instance managers (checked, unchanged)
The hosts match the measured rules:
- Traffic: the 4 nearest within 40 m, horizontal.
- Peds: the 15 nearest within 50 m (3-D, nearest-first), footsteps for the 3 nearest.
- NPC skater: the first in list order within 30 m, held until 30 m.

### PedBodyFall and Tazer
Gaps at the end of P3; closed below ("Gaps closed").

### Mod surface
**Done in this pass (cheap):**
- `voice` for skaters (the bail grunt);
- traffic model names in `engine`;
- the model-driven ped defaults;
- `info().speech_lines`;
- the mod limits raised to 48 per mod / 128 in all;
- `"enabled_by_default"` in `mod.json` and `SKATE3_MODS_ENABLE`.

**Left for the final moddability pass:** spec §8.2–§8.4 (the content overlay for banks, samples, programs and
speech lines; mod emitters on the native voice graph; tuning read / write; `sdk.audio.post`; audio event hooks)
and §8.6.

### Proofs and tests (this pass)
- **Local player byte-identical:** the e2e bench (13 scenarios + 4 whole sessions, `row` and `fps300`) from a worktree of
  `a4ec831` (`p3_base`, equal to the earlier `sw_new`) against this tree (`p3_new`): all four sets IDENTICAL (26 +
  26 + 8 + 8 outputs). The e2e harness drives the local player's pass itself, so the native split was proved by the
  shift test and by its unchanged local sequence.
- **New data tests** (all pass, `--ignored`):
  - the speech end-to-end;
  - the speech levels against the recomp;
  - the NPC components;
  - traffic RPM / `+176` against the recomp;
  - the one-evaluation shift.
- **New unit tests:**
  - `speech_player` (level ids, far clip, streams, interrupt, queue timeout, cut);
  - the traffic relative speed and points;
  - the opt-in mod;
  - the test mod's objects within the limit.

### Files (this pass)
- **New:**
  - `crates/skate-audio/src/world/speech_player.rs`;
  - `crates/skate-game/src/game_audio/world_speech.rs`;
  - the local tools `speech_levels.py` and `ped_models_check.py` (not published).
- **Changed:**
  - `skate-audio`: `mixer.rs` (`set_direct_dsp`, `set_bank_sample`), `player/contacts.rs` (the grunt flag),
    `world/{keys,mod,peds,skaters,speech_manager,traffic}.rs`;
  - `skate-game`: `game_audio/{library,mod,native,npc_skaters,player_audio,world_bridge,world_sources}.rs`,
    `modding/world_audio.rs`, `world_audio.rs`;
  - `skate-mods`: `schema.rs`, `lib.rs`, `world_audio.rs`, `vm.rs`, `api.lua`;
  - `sdk/skate.lua`, `sdk/AGENTS.md`, `mods/world-audio-test/*`, `tools/asset_pipeline/world_audio.py`.

### Open (after P3)
- **Speech:**
  - the stream's PEAK filter;
  - the reverb SEND for most lines;
  - the `Obj:Speech` inputs a playing line sets (F45 / F19 / F42), probably the ~+100 mB the recomp's near
    lines carry over ours;
  - which of the two streams a request targets;
  - the queue timeout's unit;
  - values 49 (a Splice ring, `sub_824D9C70`) and 29 (the photographer's timer);
  - the main-cast path for pros.
- **Tazer** (`SFXObj_Tazer`) and **PedBodyFall**: the objects are not decoded.
- **Traffic:** the 3DObjPos binding's writer; the horn kinds per model (the AI's); the frozen record of a held
  car (not ported).
- **NPC:** board slide; the walking / jump voices of an NPC on foot; the NPC grain bed against 180430's NPC GREC
  rows (not compared in this pass).

## Gaps closed: Tazer, PedBodyFall, speech details, the NPC board slide and bed (2026-10-03)

Built from the P3–P5 state and checked headless against the existing recordings, plus one scripted background
recomp run for the NPC board slide (user-approved gap runs; muted, one game at a time). All numbers are "the
recomp". Reference only: the TU3 recompilation (skate3recomp / rexglue / Xenia); our own code.

### Tazer (`skate_audio::world::peds::PedTazer`)
- **Decoded:** `SFXObj_Tazer` (Pedestrian object 3, factory `sub_824F12D0`). Its process posts the 9-word
  `c_tazer` packet while the ped audio state's byte `S+80` is set (the manager's bit 17 of the ped entry) and
  releases it when it clears; its update writes raw 0, pitch 1, levels 3 / 2. The program (op 38, ported earlier)
  owns a `c_tazer_grn_play` child post and plays the burst while the packet is held.
- **`S+80` = tazing:** each post came 22 ms after the state graph's `TazeEntity` entry (`TazerCycTime` 2.0 s).
- **Checked** (`tazer_bursts_follow_the_recomp`, session 164620: three zaps, 49 Tazer starts): per 2 s hold ours
  start 19 sounds (recomp 10 / 20 / 19; its first burst stopped after 1.33 s, cause not traced); gaps
  192 / 192 / 128 / 96 ms against 190 / 190 / 130 / 90–100 (the 11th gap 128 against 129 / 131); the order 8, 7,
  then shuffles of 0–6 in both; the first start's gain recomp / ours 0.85 / 0.93 / 1.00 at each zap's geometry.

### PedBodyFall (`PedBodyFall`)
- **Decoded:** `SFXObj_PedBodyFall` (object 2). Its trigger `S+76` is the ped animation's `BodyFallType` channel.
  Each change to a non-zero value starts one Skate_Collisions sound through the collision Splice object: type 8 →
  1184, 9 → 948, any other → 1183 (vault record `923CCB46EF5BF5BA` / `DFEFC9212E0CBD2C`); 8 and 9 round-robin two
  slots. Each type follows its own volume / pitch outputs (8: out1 / out2, 9: out3 / out4, other: out5 / out6).
- **The recordings had it after all:** the starts are Splice starts, not POSTs (75 in six sessions). Retail's
  knock-downs key 9, other, 9, 9 (0.10 / 0.48 / 0.16 s apart) or 9, 8, 8, other, 9.
- **Checked** (`body_falls_follow_the_recomp`, 73 starts): container by type 73 / 73; voice gain recomp / ours
  p50 0.79 (p10 0.25, p90 1.25, n 40; the knocked-down ped is not logged, the ped nearest the player is taken).

### Speech details
- **PEAK filter:** a head shadow by azimuth. The owner's raw azimuth, folded front / back, goes through three
  curves of the speech record (centre 600 → 4000 Hz at the side → 600, gain 0.4 → 0.1, Q 3; setup export
  `world_tuning.speech_voice`). Every recomp PEAK (6 / 6) lies on the curves at one azimuth; ours at the estimated
  geometry 3 / 6 within 10 %. The mixer plays speech on the stream graph (PEAK, Send A, gain, then the filters).
- **Two sends, and the echo submix (ported):** the stream system (`sub_82C5CEF0`) multiplies the gain and both
  sends by the speaker's per-voice float (`S+152`, `aud_characteristics` `2087A3290483BB4F`, 0.8–1.4). The
  **pre-gain** send (`desc+16`, recomp module `+0x570`) carries PedestrianSpeech **out21** (PlayerSpeech out13)
  and feeds the stream slot's **echo submix** (`skate_audio::bus::speech_echo`, built per stream slot by
  `sub_82C5E2D8`: high pass = owner filter 22 (skater 14), delay = camera distance × the record's factor / 344 m/s
  up to 0.15 s (recomputed every 4 console frames, posted only on change), low pass = filter 23 (skater 15), then
  the environment bus). The **post-filter** send (`desc+32`, `+0x7D0`) carries out15 (skater out10) straight to
  the environment bus. Correction to the first decode: it had out15 feeding the echo; the code says the pre-gain
  send does. Recomp: the pre-gain send = out21 in 17 of 29 lines within 25 % (out15: 10); the post-filter send =
  out15 in 19 of 27. Modelled mono: the graph's two-channel Pn21 / Sen0 routing is a unity tap (its gains are not
  traced).
- **Value 49 = a phone call:** the ped's phone rings (CellPhone_Rings container 5, now part of setup) and when the
  ring ends the ped asks for value 64 (`4402_cell_greet`). Recomp 164620: both rings answered 1.00 / 1.19 s
  later; the ring records last 0.84–1.13 s. Without the bank (an install from before this setup) the answer
  follows at once (logged).
- **Value 29 (the photographer):** repeats each second while the game flag is set (`LivingWorldAudio::photo_flag`).
- **`Obj:Speech` inputs:** in0 / in1 / in4 are raised while a playing line's speaker is 37–38 / 75–77 (the guards)
  / 1–29 (the pros); in2 / in3 are not traced. They don't touch regular peds, so the recomp near lines' extra
  ~0.6 dB stays open.
- **Which stream:** the first free one (k0 when both are free: 95 of 109 new lines).
- **Queue timeout unit (settled from the code):** the library's clock `[[0x830CFD94]+16]` is the function behind the
  `GetVisualGameTick` Lua binding (`0x8283C2D8`): visual game ticks, one per rendered frame. The console renders
  at its ~30 fps cadence, so the port's console-frame count is the same unit.
- **Main-cast channel (ported):** the pros' and the special cast's speech (`maincastspeech.big`, speech-manager
  bank 0, channel 0, its own tuning `speech_tuning["0"]`, timers, two streams and mixer bank). Who speaks there:
  a model with no living-world type bit and a cast bit / word (`aud_characteristics` `6F2933E977CF40DD` /
  `14FD437D190677C8`, plus `D6EA428C2B43E23A` for the pro-on-pro lines; setup `world_tuning.ped_models`). Request
  words per event: `sub_824AC898` (`speech_manager::main_cast::request_words`); the main cast's near / far flag is
  1 near, 2 far. Senders:
  - a pro **ped** (`sub_824AC438`): value 29 → `1014_bored` (141), 28 → `601_ai_greet` (11, outside a challenge),
    30 / 51 → 115 or 6, 53 / 54 → 77;
  - an NPC **skater's** own process (`SFXObj_PlayerSpeech` non-local, `sub_824DA1B0`;
    `skate_audio::world::skater_speech`): pros (`sub_824DA768`) say `101_pos` / `150_pro_pos` on a seen trick and
    `104_slam` / `903_race_slam_pc` / `151_pro_slam` on a seen slam, every frame the byte holds; AI skaters
    (`sub_824DAAC0`) `101_pos`, `404` / `405` / `903` / `104_spec_slam`, `105_spec_chase`, on change; its crash
    (`sub_824DB688`, latched) the message pair `131_collide_object` / `906_aislm`;
  - the skater messages by speaker kind (`sub_824DAC00`): bail grunt 8206 / 115, crash 8229 / 125, impact
    8233 / 6, hit reaction 8309 / 253, race punch 8310 / 250, gesture 8291 / 247 (the senders of the last four are
    engine systems the game does not have yet).
- **Main-cast decode:** setup indexes it always (`speech/maincast.json`, 1283 clips) and decodes its free-roam
  events with the living world's (`SKATE_SETUP_SPEECH=1`): **6596 takes, 3.0 h, 922 MB** of 44.1 kHz PCM16.
- **Checked:** `main_cast_lines_are_reachable`: all **82** main-cast lines the recomp streamed in 11 sessions
  (906 `AiSlam` ×45, 101 `pos`, 150 `pro_pos`, 104 `Slam`, 130 `col`, 201 `grunt`) are reachable through our
  words for their speaker (76 of them of an event the port sends; 130 `_col` has no ported sender).
  `a_pro_skater_says_a_main_cast_line`: Ryan Smith's model as an NPC skater sees the player's trick and the main
  cast streams `101_24_Smit_pos` at its PlayerSpeech level.
- **Re-verified** (`speech_levels_follow_the_recomp`, 39 lines, now with the voice float): filters 4 of 5, gain
  recomp / ours median 0.988 (p10 0.55, p90 1.36), pre-gain send = out21 17 / 29, post-filter send = out15 19 / 27,
  PEAK on the curves 6 / 6.

### NPC skater: the board slide and the bed
- **Board slide ported for the NPC instance:** retail's slide code has no local test and its input (the
  loose-board state) is computed per skater, so `NpcSkater` runs it from `NpcSkaterAudio::loose_board` (ghosts:
  per row of their log). A scripted background run (Mega-Park, 4 min standing, the PLAYERPOST hook, 0 malformed
  lines): instance 1 was held 78 s in 12 holds, no NPC bailed, no slide post by either instance: still unobserved.
- **NPC grain bed vs session 180430's NPC rows** (the GREC rows of the NPC object, filtered by owner, joined with
  the board's distance; `npc_bed_follows_the_recomp_rows`, 289 straight-roll rows): truck A gain recomp / ours
  0–10 m 0.097 / 0.108, 10–20 m 0.045 / 0.065, 20–30 m 0.005 / 0.014; pitch 0.93–0.97 / 0.96. The far bands are
  louder in ours: the lookups measure the distance to the local skater, which the rows don't give.

### Mod surface and data (moddability)
- Engine: `PedAudio::{tazing, body_fall}`, `PedTazerEvent`, `PedBodyFallEvent`, `NpcSkaterAudio::{loose_board,
  reactions}`, `NpcSkaterReactionEvent`, `LivingWorldAudio::photo_flag`; ped / skater `voice` 1–96 (the pros and
  the special cast speak on the main cast).
- Mods: ped options `tazing`, `photo_flag`; lite-skater option `loose_board`; events `tazer` (seconds) and
  `body_fall` (kind), skater event `reaction` (value `slam` / `slam_b` / `trick` / `crash` / `chase`, `by`); speech
  value 49 works through `speech`; `voice` accepts 1–96. The test mod: F7 tazer, F6 a retail knock-down's keys, F5
  a phone call, F4 the photographer, F3 the ghost sees your trick, F2 the ghost switches between its AI voice and
  a pro's (24) and crashes.
- Setup data (`world_tuning`): `ped_objects` (fall containers, eq bus, ring container, the tazer hold from the ped
  state graph) and `speech_voice` (the PEAK curves, the echo delay factor / per-metre / cap / refresh frames);
  `ped_models` gained `cast_bit` / `cast_word` / `cast_word2`; `speech_tuning["0"]` (the main cast's) is now used;
  `speech/maincast.json` + the opt-in decode; `CellPhone_Rings.bnk` decoded with its patch tree;
  `world_audio.py` joined the audio group's fingerprint. Every field defaults to the retail value and the world
  tuning overrides it.

### Proofs and tests
- Local player byte-identical: the e2e bench (13 scenarios + 4 sessions, `row` and `fps300`) from this pass's
  starting point (`wg_base`) against the result (`wg_new`, and again after the echo submix and the main cast:
  `wg_new2`): all four sets IDENTICAL (26 + 26 + 8 + 8 outputs) both times.
- Main-cast / echo tests: `main_cast_lines_are_reachable` (82 / 82), `a_pro_skater_says_a_main_cast_line`; units
  `skater_speech` (pro / living-world choices, crash latch), the echo delay and the echo graph; the mod event
  `reaction` and the test mod's F3 / F2.
- New data tests: `tazer_bursts_follow_the_recomp`, `body_falls_follow_the_recomp`,
  `npc_bed_follows_the_recomp_rows`, `the_phone_ring_lasts_until_the_recomp_answers`; re-verified
  `speech_levels_follow_the_recomp`. New unit tests: peds (tazer, falls, ring, photographer), the PEAK curves, the
  mod options / events, the test mod's new keys, the setup export.

### Still open
- Speech: Obj:Speech in2 / in3; the near lines' ~0.6 dB; the echo graph's two-channel routing gains; the
  main-cast speaker-slot repeat times for slots 30 / 31 (record `+52` / `+56`); values 30 / 51 also stopping the
  speaker's playing line; the game modes the skater choices test (19 / 20 / 55: free skate is none of them, the
  port passes 0); the cameraman line the crash can request; senders of the hit-reaction / race-punch / gesture /
  skater-collision lines (no such engine systems yet).
- NPC: the board slide is ported but unobserved; the walking / jump voices.
- Traffic: the 3DObjPos binding's writer; horn kinds per model.

## Open questions before P3 (kept for the record)

- **Speech playback** in the host: the manager, the library and the streams. The level and pan
  mapping is still open.
- **The NPC instance's components.** G3 (2026-10-03) settled which ones run for an NPC:
  - Wheels runs (spin streams on layers 0 / 1; layer 2 and the post-start parameter writes are
    local only);
  - Clothing runs (`sk8_foley` 73 / 74; body slide / cloth falls on bails; the start-block float is
    0 for non-local; the eq-chain pick takes `local72`);
  - Tricks and Treatment are local only;
  - OffBoard creates its footstep packets but plays no steps for NPCs;
  - the bail grunt `sub_824BF5F8` posts speech.

  Instance 1 changes hands often (12 times in 74 s), so claims and releases must stay cheap.
- **PedBodyFall** (decode the object), **Tazer** (needs AEMS op 38).
- **Traffic bindings:**
  - the `TrafficCarPhysics.in0` writer (|v_car − v_listener| clamped to 35, slewed by 100 /s,
    × 32767 / 35; opens the A11 near boost);
  - the 3DObjPos 4.1 / 4.2 points (front and rear ±1 m along the heading, R+80 / R+96, likely);
  - retail's frozen record when a held car leaves the 40 m list (recorded behaviour, not ported).
- The per-model tables from `aud_characteristics` (shoe class, kind, far threshold 20 / 30 m) as a
  setup export, so the engine names a model and gets the rest. The horn kinds per model are open.
- Moving process / update into `mixmap_frame` (retail's split; the inputs are one console frame
  old today).
- The wider mod surface (P4): content overrides (banks, samples, programs, speech lines), mod
  emitters on the native voice graph, tuning read / write, `sdk.audio.post`, and audio event hooks.
