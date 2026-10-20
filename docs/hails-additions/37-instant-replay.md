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
reset on a map change keeps a plain despawn (the replay resets with the map). The actor's fade (`NpcFade`, spawn
fade-in, leave fade, the peds' camera-distance fade) is recorded like the audio components and moves to the ghost
with its fade materials, so a skater that faded out before leaving plays back at its recorded alpha instead of
staying invisible. Test: a retired ped plays back as a
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
- World actors: `replay/components.rs` records each actor's audio components with it, on change, keyed by the
  replay id: `TrafficAudio` (engine, speed, load, horn, skid), `PedAudio` (feet, speech state), `NpcSkaterAudio`
  (the NPC's full audio record) and `AudioVelocity`. During playback the value holding at the cursor is written back
  in PreUpdate, before the audio host reads it, and the live values return on exit. Any `Component + Clone +
  PartialEq` can be tracked the same way (`components::track`). Ghosts (actors that despawned inside the window) are
  drawn but stay silent: an audio component on a hidden ghost would make it audible during live play.
- One-shots: `replay/messages.rs` records the bridge's one-shot messages with their frame time (`VehicleHorn`,
  `VehicleAlarm`, `PedTazerEvent`, `PedBodyFallEvent`) and writes each again once when the playback cursor crosses
  its time going forward (not on a rewind). NPC reactions and ped speech stay out (speech, blocked in retail's
  replay). Any `Message + Clone` can be tracked the same way (`messages::track`).
- Not yet: vehicle impact sounds (`car_alarm.rs` contact path) are not recorded. Replay speed below 100 % (M2) will
  scale `AudioState.time_scale`; whether retail's pitch follows is open.

## The free camera stays within 40 m of the player

Retail's replay free camera is pulled back when it is farther than 40 m from the player (per-frame limits step
`82683680` -> `82682CB0`, constant at 0x821EB794; research b381). Ours flew anywhere. Now the free camera is projected
back onto the 40 m sphere around the player's recorded position at the cursor after each move
(`replay.rs` `leashed`, `FREE_CAMERA_LEASH`; `Replay::free_camera_leash` overrides it, data for a mod). Retail's
world collision for the free camera (a 0.1 m sphere, 0.8 m with a world flag, x 1.3, `82682D80`) is not ported yet.
Test: the leash keeps a camera inside 40 m and pulls a far one onto the sphere.

## Instant replay controls follow retail

Retail's instant replay (editor mode 5; research b386, b387, b389; main verified the open function `82610458`
setting mode 5 and the 150-frame check) replaces our own bindings (user 2026-10-20: "make the controls follow
retail"):
- **Back** (View / Select; F6 on the keyboard) opens the replay, only with at least 5 s recorded (retail's 150
  frames of the 30 fps timeline; the unit is inferred); a refusal is silent, as retail. `sdk.replay.open` uses the
  same check (`Replay::can_open`). It no longer closes the replay.
- **B** or **Back** in the replay: retail's Finished menu; with nothing to save or upload that is the quit warning
  (ID_COMMON_WARNING_TITLE / ID_REPLAY_EDITOR_QUIT, No / Yes), raised on the retail menu stage through a small engine
  popup channel (`PopupMods::engine_requests` / `engine_choices`). Yes leaves, No stays. Without the retail menu
  stage (old installs, `SKATE3_RETAIL_MENUS=0`) B / Back leave at once. OURS, a safety until play-tested: Back again
  while the warning is up leaves at once.
- **RB** plays / stops (was A); stick or D-pad input resumes a stopped replay (`82C7FFB8`).
- **D-pad Left / Right** jump to the start / end (was a one-frame step; the frame step stays for the editor).
- **A** (retail: open the full editor) and **Y** (retail: cycle the edit channel, film DLC only) do nothing until
  the editor exists. The free camera is PC only now (C, WASD / Q / E, right mouse; was Y on the pad): retail's
  instant replay has no free camera.
- Not ported: the open sound (retail UI sound 0x394015B68783BE9E), the online instant replay mode.
Tests: the controller test (open, scrub, jump, RB, stick resume, B), the quit warning (No stays, Yes leaves) and the
5 s minimum.

## Trigger scrub at retail's rate

Retail's scrub (research b387 / b388; main verified `82610F18` reads the mode word and stores the rate at +0x34, and
`825071B8` passes it to the playhead step `82C7EF90`): per console frame the target is (RT - LT) x 0.005, the rate
moves 0.25 of the way to it, clamps to +-0.005, and stops in a dead band (target < 1e-5 and rate <= 1e-4); the
playhead moves by the rate as a fraction of the WHOLE recording, so a full sweep takes about 6.7 s whatever the
length (at most 0.15 of the recording per second). Ours moved a flat 4 s of recording per second, with no easing.
Now `skate_core::replay_edit::ScrubFilter` / `ScrubSpec` (data) run whole 30 Hz console frames (the same movement at
any engine frame rate) and the replay controls use it: a moving scrub stops playback, and it eases out after the
triggers are released. The keyboard arrows count as full triggers. Tests: ease-in, cap, release to zero, 30 vs 144
fps give the same distance; the controller test now ticks whole console frames.

## M2 step 1: the editor model (skate-core)

`crates/skate-core/src/replay_edit.rs`: clips trimmed out of the recording (30 fps timeline frames), each with its
effect values and stepped marker lanes, and retail's length rules. Pure data, no UI yet.
- Opens at 150 frames or more (`8250B8F8`, > 149); timeline 30 fps (`825ADD90`).
- Encoded length = round(sum over clips of span / (speed% x 0.01) / 30 + 0.5) s, shareable at 60 s or less
  (`825ADF88`, `cmplwi 60`).
- Lanes with retail ranges (research b343 / b344 / b377): speed 10..100 % step 10 default 100 (slows only),
  shake 0..3 wrapping, brightness -100..100 step 20, contrast 10..30 step 2, saturation 0..100 step 10, tint R / G /
  B / A 0..250 step 25. The look defaults are the neutral values (OURS until retail's are read).
- A lane's value steps at a marker (no ramp: retail writes the value at the marker and 1 ms later, `8250CC10`); a
  marker on the same frame replaces; trimming drops markers outside the clip; at most 100 markers per lane (retail
  refuses at 100 with "no more markers", `82613500`, research b381). A marker within 0.0089 of the clip span from
  an existing one is "at the cursor" (setting a value edits that marker), and no marker goes within 0.0089 of the
  clip's in / out points (`825084C8`, constants 0x822428EC and 0.9911 at 0x822F8F74; research b382; that the
  normalised unit is the clip span is inferred).
- Data: `EditLimits` and `LaneSpec` hold every number; lanes have stable string ids (`speed`, `tint_r`, ...) for mod
  records and saved edits. A mod may allow speeds over 100 % (ours, not retail).
- Tests: 6 (open threshold, stepping / clamping / wrapping, stepped markers, marker cap, marker spacing, encoded
  length).

## M2 step 2: editor session and key bindings (skate-core)

`skate_core::replay_edit::session`: retail's editor modes (the values of `[DAT_83067084]+0x30`: 1 playback, 2 trim,
5 instant, 6 timeline, 7 / 8 / 9 / 10 camera key edit, 11 effects) and a binding table (mode, FE key index, action,
next mode) with retail's proven bindings as the default (research b386 / b387 / b389; `826127B8` input handler,
`82610458` opens mode 5 from Back during play): instant A -> timeline, D-pad Left / Right pressed jump to the ends;
timeline B Finished, X photo, RB play (back to the timeline at the clip's end), LB held trims until released, L3
panel, released D-pad Up / Down remove / drop a marker, Left / Right jump to the ends, Start help; effects Up / Down
item, Left / Right value, B commits. Unproven transitions (camera key edit entry, modes 0xC / 0xD) are left out. The
table is data a mod can replace. Not wired to the game yet: our current replay controls keep working until the
FE 12 host lands (retail differences listed in the plan: Back opens only, B opens Finished, D-pad jumps, 150-frame
minimum). Test: retail's mode transitions.

## M3 step 1: the look lanes as a colour matrix override (skate-core)

`ReplayEdit::look_at(clip, frame, base)` turns a clip's look lanes into what the screen uses: the world grade's
colour matrix record with the lanes' terms replaced, plus an RGBA tint overlay. Retail (research b380): the lane
consumers (`82686DF0` brightness, `82687FD8` contrast, `826888B8` saturation) write an override copy of the world
colour block that the post FX update uses instead of the base while set (main checked `82686DF0` stores the near and
far add terms at +0x7C00 / +0x7C40); brightness is the matrix add term on R, G, B; the values replace, never
multiply; lane 6 is a separate RGBA screen tint. Our `colour_matrix::build` takes the same roles as retail's builder
`827F13D8`, so the override record goes straight into it. A lane at its neutral default keeps the world's term
(OURS: when retail counts a lane as set is not read), so zombie mode's grade stays under an untouched replay. Not
wired to the renderer yet (M3 step 2, with the editor UI). Test: untouched lanes keep the grade; greyscale,
brightness and contrast replace both bands; a tint marker shows from its frame.

### Open
- Held hand props are not recorded. (The camera frame's opacity / blur / shake are not used by the renderer, so the
  recorded transform and FOV are all playback needs.)
- Ghosts' audio and vehicle impact sounds during playback (see above).
- The replay look lanes (M3, research b379 / b380): lane 3 brightness (colour matrix add term), lane 4 contrast,
  lane 5 saturation, all near and far, replacing the world colour block while set; lane 6 an RGBA screen-tint
  overlay. Our `colour_matrix::build` takes the same argument roles as retail's builder `827F13D8`.
- Retail editor cameras (for M2, research b376 / b377): ICELITE keyed values (eye, look, dutch, lens mm), lens
  10..500 mm, default 24; shake levels 0 / 0.35 / 1.0 / 1.0 for amplitude and frequency (`8250A6E8`); effect lanes
  step at marker times (speed +10 % up to 100 %, shake 0..3, brightness, contrast, saturation, tint).
- The player is still recorded as full `Mat4` skin rows (about 45 MB at 90 s); moving it into the quantised buffer
  would cut that to about a fifth.
