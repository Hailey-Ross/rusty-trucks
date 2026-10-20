# Instant replay: record every actor, 90 s window

Branch `world/replay` (from `world/living-world`). Plan and milestones: the replay todo (M1 record everything, M2
editor, M3 effects, M4 export). This doc covers what has landed.

## M1 step 1: every drawn actor is recorded and played back

### Problem
Upstream's replay (`crates/skate-game/src/replay.rs`, 88ec43e2) records only the player (root, skin, camera) in a
30 s window. During playback the rest of the world freezes: NPC skaters, peds, traffic and props stand where they were
when the replay opened, so a replay of a line past a crowd shows a still picture around a moving skater.

### Retail (evidence)
- Retail records presentation data, not a re-simulation: one DataTracker frame per sim step (60 per second, recomp
  count run 2026-10-19, hook REPLAYREC), at most 5400 frames (`li 5400`) in an 11,534,336 B byte ring that evicts the
  oldest frames (DataTracker `82726170` / `82DBBAF8`) [code]. 5400 frames at 60 per second = a 90 s window.
- Provider id 2 records the living-world census actors (peds, traffic cars, DMO props) and ids 3 / 4 the skaters
  (`sub_827BF090` reads the census containers +0x20 / +0xAA0) [code, research b364].
- Skater poses are LOCAL joint rotations as 4 x i16 quaternions (scale 32767), interpolated per joint with nlerp on
  playback (decoder `8278ABE0`, pose block `8278A4E0`) [code, research b365 to b367].

### Change
- `crates/skate-core/src/replay_buffer.rs` (new, engine independent): one key track per actor, keyed by a stable u64
  id. A key holds the root transform (f32 translation and scale, i16 quaternion) and every joint's local rotation as
  retail's 4 x i16 quaternion; joint translations are stored only where they left the actor's rest translation, scale
  only when not 1. A key is written only when the quantised pose changed (a resting prop costs one key for the whole
  window); after a hold the held pose is repeated at the previous frame so playback holds instead of blending across
  the gap. Playback blends translation / scale linearly and rotations with nlerp, like retail's decoder.
- Limits are data (`ReplayLimits`): `max_frames` defaults to retail's 5400. `byte_budget` defaults to 128 MiB (OURS):
  retail's 11 MB holds its compressed frames, ours are uncompressed i16 rows for every drawn joint, so the budget is
  sized to keep the retail 5400 frames in a busy scene. When the budget fills first, the oldest frames go first, as in
  retail.
- `crates/skate-game/src/replay/world.rs` (new): `ReplayActor { id }` tags NPC skaters, peds and traffic cars with
  `LivingWorldId::to_u64` and props with `3 << 56 | instance id`; `ReplayJoints` lists the joints from each kind's
  bindings (NPC puppet, ped puppet, car wheels). Recording runs in PostUpdate after every presenter, once per new
  player frame (so on the same clock as the player's recording). Playback writes the recorded root and joint
  transforms, hides actors that were not in the world at the playhead (not spawned yet, or already despawned), and
  puts every touched transform and visibility back on exit.
- The player's window follows the same limit: 5400 frames at 60 per second = 90 s (was 30 s).

### Moddability and multiplayer
- Any entity with a `ReplayActor` (and optionally `ReplayJoints`) is recorded and played back; mod props
  (`sdk.world.spawn_prop`) carry `PropInstance` and are recorded like map props.
- Mod API (`sdk.replay`, capability `replay` = 1, all in `crates/skate-mods/src/api.lua`):
  - `sdk.replay.open()` (`replay_open`): opens the replay at the newest frame; refused in multiplayer.
  - `sdk.replay.marker(label)` (`replay_marker`): a marker at the newest frame, label 1..64 characters; at most 50
    (retail provider id 10); markers leave with the window and the mod's markers go on disable.
  - `sdk.replay.set_limits{max_frames, byte_budget}` (`replay_set_limits`): 60..36,000 frames, 1 MiB..1 GiB; nil
    restores the defaults; last writer wins; the override goes when that mod is disabled.
  - Read-only `snapshot.replay_state`: active, cursor, start, end, frames, bytes, max_frames, byte_budget, markers
    (time, label, owner). `snapshot.replay` stays the old boolean.
  - Still to come: a mod-side provider (record per-frame mod values, read them back during playback).
- Ids are the stable network ids (`LivingWorldId::to_u64`, prop instance ids), keys are plain bytes with counts, so
  a recording is serialisable. Replay stays refused in multiplayer for now (unchanged).

### Files
`crates/skate-core/src/replay_buffer.rs`, `crates/skate-core/src/replay_buffer/tests.rs`, `crates/skate-core/src/lib.rs`,
`crates/skate-game/src/replay.rs`, `crates/skate-game/src/replay/world.rs`, `crates/skate-game/src/replay/tests.rs`,
`crates/skate-mods/src/api.lua`, `crates/skate-mods/src/vm.rs`, `crates/skate-game/src/modding/mod.rs`,
accessors in `animation.rs`, `living_world/npc_skaters.rs`, `living_world/peds.rs`, `living_world/vehicles.rs`.

### Verification
- skate-core: 7 tests (quantise round trip within 2e-4 rad, keys only on change and holds across gaps, hidden before
  spawn and after despawn, nlerp midpoint, eviction at the frame cap keeping the key that holds at the window start,
  eviction by bytes with exact byte accounting, clock reset clears).
- skate-mods: the replay commands parse and validate (bounds refused), the Lua functions submit them, the capability
  is set.
- skate-game: marker cap, marker eviction with the window, limits ownership and cleanup on disable; the 90 s window
  test, and a world playback test (an actor plays back its recorded root and joint
  mid-frame, an actor that appeared later is hidden, exit restores the live transforms and visibility).
- Not play-tested yet.

### Actors that left the world inside the window
NPC skaters, peds and traffic cars despawn through `crate::replay::retire` instead of `despawn()` (npc_skaters.rs
the two despawn sites, peds.rs the despawn system, vehicles.rs the despawn record and the external despawn). It
moves the actor's loaded scene (the child that holds its recorded joints) under a hidden "ghost" root with the same
replay id, then despawns the actor with its gameplay components. Playback draws the ghost from the track up to the
despawn frame and hides it after; the ghost is dropped once its track leaves the window, never during playback. At
most 64 ghosts (`MAX_GHOSTS`); past that, or when the actor was never recorded, it is simply despawned. The world
reset on a map change keeps a plain despawn (the replay resets with the map). Test: a retired ped plays back as a
ghost before its despawn, hides after it, stays hidden while live and goes with its track.

### Playback audio: the recorded skater state plays again
Retail records no audio and does not mute: its normal audio runs on the recorded presentation state, with "in
replay" gates (research b375, b378).
- Recording: every player frame keeps the audio step `observe` published that physics step (`Cues::raw`, the
  `Riding` record with the full retail `AudioState`; one choke point, every player audio system reads `Cues`).
- Playback: `observe` stops publishing the frozen live state; the feed (`feed_replay_audio`, PreUpdate after the
  replay controls) publishes the recorded steps the cursor crossed going forward, so the host hears rolling, grinds,
  pushes, landings and bails again, at the steps they happened (one-step pulses merge like a live multi-step frame).
  A fast scrub sends at most 8 steps per frame and keeps the skipped pushes; a rewind sends the step at the cursor
  without its pulses.
- Gates: a replay that stands still (paused) is silent, as retail's paused replay (Master.in6 at time scale 0);
  speech stays silent in any replay (retail blocks speech requests in replay); Master.in0 is written every pass, 32767
  in replay and 0 otherwise (checked in the recomp: `sub_824D5160` after the replay flag `sub_8279E180` writes
  input 0 with 32767 or 0), so the retail MixMap swaps the Hall of Meat slow-motion ducks for the replay duck.
- Seeks and rewinds count as camera cuts for the host (Doppler velocity reset).
- Not yet: the world actors' audio state (traffic engines, ped feet, NPC skaters' own audio) and their one-shots
  (horns, alarms, body falls) are not recorded, so those sources hold their last live state during playback.
  Replay speed below 100 % (M2) will scale `AudioState.time_scale`; whether retail's pitch follows is open.

### Open
- Ped / NPC fade alpha, held hand props and the camera's full `CameraFrame` (shake, opacity, blur) are not recorded
  yet.
- World actors' audio during playback (see above).
- Retail editor cameras (for M2, research b376): ICELITE keyed values (eye, look, dutch, lens mm), lens 10..500 mm,
  default 24; shake levels 0 / 0.35 / 1.0 / 1.0 for amplitude and frequency (`8250A6E8`).
- The player is still recorded as full `Mat4` skin rows (about 45 MB at 90 s); moving it into the quantised buffer
  would cut that to about a fifth.
