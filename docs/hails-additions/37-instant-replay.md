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
- Any entity a mod gives a `ReplayActor` (and optionally `ReplayJoints`) is recorded and played back; limits are a
  plain data struct. Mod API (`sdk.replay.*`: start / stop / marker / register_provider, cleanup on disable) is the
  next M1 step.
- Ids are the stable network ids (`LivingWorldId::to_u64`, prop instance ids), keys are plain bytes with counts, so
  a recording is serialisable. Replay stays refused in multiplayer for now (unchanged).

### Files
`crates/skate-core/src/replay_buffer.rs`, `crates/skate-core/src/replay_buffer/tests.rs`, `crates/skate-core/src/lib.rs`,
`crates/skate-game/src/replay.rs`, `crates/skate-game/src/replay/world.rs`, `crates/skate-game/src/replay/tests.rs`,
accessors in `animation.rs`, `living_world/npc_skaters.rs`, `living_world/peds.rs`, `living_world/vehicles.rs`.

### Verification
- skate-core: 7 tests (quantise round trip within 2e-4 rad, keys only on change and holds across gaps, hidden before
  spawn and after despawn, nlerp midpoint, eviction at the frame cap keeping the key that holds at the window start,
  eviction by bytes with exact byte accounting, clock reset clears).
- skate-game: the 90 s window test, and a world playback test (an actor plays back its recorded root and joint
  mid-frame, an actor that appeared later is hidden, exit restores the live transforms and visibility).
- Not play-tested yet.

### Open
- Actors that despawned inside the window are not drawn (their entities are gone); they need puppets spawned from the
  recorded look (character, model, recipe, tints, paints).
- Ped / NPC fade alpha, held hand props and the camera's full `CameraFrame` (shake, opacity, blur) are not recorded
  yet.
- Playback audio: retail regenerates audio from the recorded state with "in replay" gates (research b375); ours still
  mutes during replay.
- The player is still recorded as full `Mat4` skin rows (about 45 MB at 90 s); moving it into the quantised buffer
  would cut that to about a fifth.
