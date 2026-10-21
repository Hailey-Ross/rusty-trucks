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

## Fixes from the first play test (desktop session 2026-10-20)

User: "Ped held props get wonky and lose their attachpoint or just for some reason seem to float offset from the ped
during the replay, only properly returning to their hand once you reach the end of the replay. They also slightly
wobble in place even when the replay is paused at the end." / "the ped has dissapeared from the replay entirely and
now i just ride over and bail on nothing".
- **The living world kept running during the replay.** The log showed fixed steps and 26 `PED_DESPAWN reason=Distance`
  lines while the replay was open: peds walked away from the frozen skater and despawned at 70 m, the hit ped (#3)
  at 15:01:50 among them. Retail plays recorded data while the world waits. The living world's fixed chains
  (population, peds, traffic, NPC skaters, ped / vehicle contacts) now run only outside a replay
  (`replay::world_live`).
- **Ghosts.** The cap is 256 (traffic churn could pass 64 in a 90 s window), and every ghost made or refused is logged
  (`REPLAY_GHOST id=... joints=... ghosts=...` / `REPLAY_GHOST refused id=... reason=cap | not recorded | no joints |
  scene not under the actor`), so a vanished actor can be traced in the log.
- **Held hand props** were drawn every frame from the ped's live (frozen) hand while the ped played back from the
  recording, so they floated off and wobbled with the presenter's interpolation. A held prop is now a recorded actor
  of its own (id `5 << 56 | ped serial`, one track per ped whichever prop it holds) and plays back with its ped.
  Still open: a held prop goes with its ped when the ped despawns, so a ghost ped has empty hands.
Tests: the living world freezes while a replay is open (and runs in apps without a replay).

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
  while the warning is up leaves at once. Fix 2026-10-20 (desktop session: "the close confirmation dialog [is] closed
  instantly"; log: popup 0 shown at 14:32:45.341, choice -1 at .389): the B press that asked for the warning reached
  the popup the next frame and cancelled it. The warning now opens when B / Back is released.
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

## Laptop session 2026-10-20 (water clock, HUD message log)
- Problem: Better Water kept moving while the replay was paused. Cause: the water shaders' clock (`retail_render.rs`
  `advance_frame_state`) read real time. Change: `WaterClock` follows the replay cursor going forward only (paused or
  rewound playback holds the water; forward-only because the ocean frame counter never steps back), real time in
  play. Test `water_clock_holds_while_a_replay_is_paused`.
- Open: after watching a takedown in the replay, a later chase message ("&@%#!!!", 16:38:09) was posted and hidden on
  time by the central HUD runtime but never seen (user: "No, nothing showed"). Not the FE HUD flag reset, the PC
  overlay, the replay message track or the cameras (read, not proven in game). New log line on a message's first
  frame: `HUD_CENTRAL draw draws= hud_draws= fe_hud= overlay= hud_off= failed=` (`scoring_hud.rs`).

## Extended Replays (PC overlay, not retail; user 2026-10-20)
- Ask: "Perhaps we can add an extra option for extended length with a slider from 2 mins to 4 mins? "Extended Replays"".
- Change: PC overlay Extras > Replay: "Extended Replays" (Retail 1:30 / On) and "Replay Length" (2:00 to 4:00, 30 s
  steps; `graphics_menu.rs` `REPLAY_LENGTHS`, saved in the overlay settings file). `replay.rs` `set_user_length`: the
  recorder's base limits become the player's length (frames at 60 Hz, byte budget scaled by the same factor from
  retail's 90 s / 128 MB). A mod's `sdk.replay.set_limits` still wins; clearing it returns to the player's length.
  Log `REPLAY_LENGTH user= max_frames= byte_budget= mod=`. Test `extended_replays_scale_the_window_and_a_mod_still_wins`.
- Memory: 90 s of a busy world filled ~125 MB (laptop session 2026-10-20), so 4 min is ~335 MB.

## Laptop session 2026-10-20 11:35 (still props, peds at a distance, ped alerts)
- User: "During replay the bench and ped both dissapeared at the beginning of the replay but reappear when they become
  relevant again" / "The bench kind pops back into existance when I come back during the replay and get within like
  5-10m maybe." / "the warning over a ped is just always there during replays. it probably should be hidden."
- Bench: a still prop records a key only on a change; its first key came from its `Added` tick, which most render
  frames skip (no new 60 Hz replay frame), so the prop had no key until it next moved and played back hidden. Change:
  `replay/world.rs` records any actor without a track. Test `a_still_prop_added_between_replay_frames_still_plays_back`.
- Ped (#3, alive the whole window): `present_ped_pose` kept running its draw distance fade and LOD against the
  camera's distance to the ped's frozen live body, so a replayed ped only drew near its live spot. Change: no
  distance fade or LOD drop while the replay is open (every recorded ped draws at full detail).
- Ped alerts: hidden while the replay is open (`ped_alerts.rs`), like the rest of the gameplay HUD.

## Retail instant replay screen, step 1 (FE 12 Replay2 on the APT stage; b390 / b391; user 2026-10-20)
- Ask: "then finish with the retail ui parity for the replay".
- Setup: `tools/prepare_menu_movies.py` exports `replay2/replay_editor2` and `replay2/replay_help` (menu set version
  11; this engine still reads 5..11, older engines refuse an 11 set, so an install refreshed with this build needs
  the other launcher versions updated too).
- Host `replay_menu_host.rs` answers `FEReplayMenu.*` from a per-frame `ReplayFeed` (`replay.rs`): IsInstantReplayMode,
  Get Cursor / Playhead Position (0..1), GetEncodedLength (`ID_REPLAY_EDITOR_LENGTH` "Estimated Video Length: {0:time}";
  NOT RETAIL YET: our m:ss for the undecoded `{0:time}`), IsEncodedLengthValid (825ADF88: under 61 s), CanSaveMedia
  (true on PC), CanDisplayShowHide (instant), Ready.
- Stage (`retail_menu_stage.rs`): opens Replay2 when the replay opens (the movie in the set and the template in the
  tables, else our Bevy panel stays), shows it after its IntroComplete with ScreenManager.ShowScreen (b391: retail never
  calls InstantDone), posts SetState 999 on cursor moves and 18 / 19 on play / stop edges, closes it with the replay.
  The Bevy panel shows only without the retail screen (`RetailReplayUi`).
- Found by the verify run: a first-frame panel state (1003) hid the panel ("Show" prompt), so panel / play states go
  only on changes; which of 1002 / 1003 hides is open (the L3 toggle is not ported).
- Verified: stage test `retail_replay_editor_opens` (loads, intro, the six natives answered, none left), host test
  `natives_answer_from_the_feed_and_edges_become_states`, a verify screenshot with the screen drawn (timeline,
  transport row, Instant replay bar, Estimated Video Length, A / X / B / Start prompts, Hide).
- Next: Start = help (FE 13 ReplayHelp), L3 hide / show, the prompts' actions (A edit timeline = the editor, X photo,
  B Finished popup with a "Save to PC" row), the `{0:time}` format, the L3 icon missing next to Hide.

## Retail instant replay screen, step 2: help (Start) and hide / show (L3) (2026-10-20)
- The replay movie's own SetState (read from its actions with `.claude/skills/retail-menus/tools/apt_dump.py`)
  names its states [data]: 999 kMovePlayHead, 1000 / 1001 kMoveStartMarker / kMoveEndMarker, 1002 / 1003
  kShowEditor / kHideEditor (the whole panel; the movie then shows its "Show" prompt), 18 / 19
  kStartPlaybackReplay / kStopPlaybackReplay, 20 kOnlineInstantReplayMode, 21 / 22 kShowToggle / kHideToggle (only
  the Hide / Show prompt, `ForceShowHideToggle`). Step 1 had 1002 / 1003 the wrong way round.
- L3 (H on the keyboard) hides / shows the panel: kHideEditor / kShowEditor on the edge. With our free camera on, L3
  stays its speed modifier. The Bevy fallback panel hides too.
- Start opens the help (FE 13, template ReplayHelp, `replay2/replay_help`, exported since menu set 11): it asks
  `FEReplayMenu.GetHelpMode` (its kInstantReplayMode = 0, two pages) and turns the page on SetState(0)
  (`NextHelpPage`); A turns the page, B or Start closes it, nothing else reaches the replay meanwhile. Only with the
  retail screen.
- Verified: stage tests `retail_replay_editor_opens`, `retail_replay_help_opens` (GetHelpMode answered, page turn
  leaves nothing unanswered), host test (L3 states), a verify screenshot of "Help - Instant Replay" over the replay
  (`SKATE_VERIFY_REPLAY_HELP=1`). Not play-tested yet.

## Retail instant replay screen, step 3: Take Photo (X) (2026-10-20)
- Retail X (`826147C8`, b386): stops playback, asks with a confirm popup (callback `82614940`) and saves the photo.
  Ours: the same with retail's strings (Take Photo / "Save Photo?" No / Yes, then "Your photograph has been saved
  successfully." OK; NOT RETAIL YET: the popup title / options are ours). The photo is the world image the game renders
  into (`graphics_menu::SceneTarget`, captured with `Screenshot::image`), so it holds no UI; saved as a PNG to
  `Pictures\Skate 3 Rust` (`SKATE_PHOTO_DIR` overrides; retail saves to My Media). P on the keyboard. Log
  `REPLAY_PHOTO saving <path> at <cursor>`.
- B stays retail: without sign-in / storage retail's Finished shows the quit warning (b387), which is ours; the
  Finished menu with our "Save to PC" row comes with the video export.
- Verified: `retail_screen_keys_help_panel_and_photo` (L3, Start / A / B help, X photo flow), a verify run with
  `SKATE_VERIFY_REPLAY_PHOTO=1` wrote a 1280 x 800 PNG of the world without UI. Not play-tested yet.

## Retail instant replay screen, step 4: prompt icons and the length text (2026-10-20)
- The screen's prompts ask for HUD-style button names (`L3_Click_hud` for Hide, `EnableShowHide`; the transport row
  likewise); `prompt_glyphs::resolve` now reads `<name>_hud` as `<name>`, so Hide shows its L3 glyph and the transport
  buttons their RB / LT / RT / D-pad glyphs (all were blank). Test `every_retail_button_has_an_equivalent_on_every_family`.
- Length text (research b397, `.local/research/b397-language-time-format.md`; main checked `ID_COMMON_METRIC_TIME` =
  "s" and `sub_824FE900` in the recomp): retail's `{0:time}` (handler 824FE900) is the number grouped by 3
  (`8290E370`) + " " + `ID_COMMON_METRIC_TIME`, not a clock: "Estimated Video Length: 42 s". NOT RETAIL YET: the
  default decimals (0 assumed). The general `{0:time}` / distance / speed types in `fe_language.rs` are a follow-up.

## M2 step 3: the editor timeline and the Pro Edit Menu on the retail screen (2026-10-20)
- A (panel shown, retail screen, at least 150 timeline frames) leaves the instant replay for the editor's timeline
  (`82613988`: mode 6, SetState 21, 9; b386). `replay/editor.rs` puts skate-core's model on the recording: one clip
  over it in 30 fps timeline frames, the session's mode, and the effects row.
- Timeline (mode 6): RB plays the clip from the cursor at its speed lane and stops at the out point (from the in
  point when at the end); LB held (mode 2, SetState 1005 / 21) trims: left stick x the in point, right stick x the out
  point, at retail's rate (0.005 of the timeline per 30 Hz console frame at full stick, b387), the playhead on the head
  that moved (the movie draws the heads at the playhead, b398); D-pad Down / Up released drop / remove a marker at the
  cursor; D-pad Left / Right released skip to the previous / next marker, else the in / out point (the screen's
  "Skip to previous / next marker"); B Finished, X photo, Start help (help mode 1), L3 hide. Keyboard: Space, L,
  M / N, comma / period, Enter.
- A opens the Pro Edit Menu (mode 0xB, SetState 22, 4): 9 rows (b398, item table `8302CCE0`): 0..6 cameras (A selects;
  NOT PORTED YET, Left / Right do nothing there), 7 speed (10..100 % step 10, `8250ADF8`), 8 shake (0..3 wrapping,
  `8250AB28`); Up / Down move the row (SetState 5), Left / Right change the value at the cursor (SetState 6), B back
  to the timeline (SetState 21, 9). It opens on the game camera row (2), the only camera our keys have.
- Natives (b398, AS integers): GetEditIndex, GetSimpleSpeedSetting, GetShakeCamType, GetCurrentCameraEditIndex (2),
  GetStartHeadPosition / GetEndHeadPosition (30 fps frames); GetCameraZoomString empty (NOT RETAIL YET).
- Verified: editor test `opens_over_the_recording_trims_marks_and_slows`, host test (mode states), verify screenshots
  of "Edit timeline" (marker and in / out point prompts, "Estimated Video Length: 10 s") and the Pro Edit Menu
  (`SKATE_VERIFY_REPLAY_AT=12 SKATE_VERIFY_REPLAY_EDITOR=timeline|effects`). Not play-tested yet.
- Open: the camera keys (follow / tripod / free and the advanced ones, b376), shake applied to the camera, the look
  lanes into the grade (M3), the markers drawn on the timeline (retail draws them from C++, the drawer not found yet,
  b398), the Finished menu with "Save to PC" (M4).

## M2 step 4: camera shake from retail's recorded clips (2026-10-20)
- Retail [code, b376 / b399 / b400]: the Shake row's levels play short recorded handheld-camera clips from
  `data/scene/ICELite.bin` (in `scene.big`; 14 ICELite sub-entries, index 0 empty), 30 keys per second, six channels
  (SHAKE_QUAT_X/Y/Z, SHAKE_POS_X/Y/Z, 16-bit, -1..1). Low = clip 3 ("Jog") at 0.35, Medium = clip 3 at 1.0, High =
  clip 4 ("RunFast") at 1.0 (amplitude = frequency). The evaluator `82C8AF50` builds a rotation (key i to i + 1,
  per-component lerp, `w = sqrt(1 - |xyz|)`) and a translation (key i only) and multiplies them into the camera
  matrix.
- Setup: `tools/prepare_replay_shake.py` (in the hud stage, `prepare_runtime_huds.py`) writes
  `private/hud/replay-shake/replay_shake.json`; the asset check offers "replay camera shake" until a refresh adds it.
- Engine: `skate_core::replay_edit::shake` (clips, levels as data, `sample`), `Replay::shake` (the shake lane at the
  cursor, timed from the in point), applied in `camera.rs` (camera space). NOT RETAIL YET: retail's gain is
  `2 x amp x sin(R)` with R a global angle whose writer is not found; ours holds `2 sin(R)` at 1, and normalises the
  quaternion.
- Verified: `off_and_empty_clips_do_nothing_and_keys_step_at_the_clip_rate`, the asset check test, the exporter on the
  owned disc (13 of 14 clips with keys; Jog 119, RunFast 110), the game logs `REPLAY_SHAKE 13 clips`. Not play-tested.

## M2 step 5: the basic cameras (Follow, Tripod, Game, Free) (2026-10-20)
- Retail [code, b401 / b402]: A on a camera row of the Pro Edit Menu opens the camera's key edit mode: Follow
  `82614658` (mode 7, movie state 1), Tripod `82614A18` (8, state 2), Game `82614B70` (9, kDefaultCameraMode 10), Free
  `82614C78` (10, state 7); A commits (21, 9). Follow: eye and look in the skater object's frame (space 0); Tripod: a
  world eye looking at the skater; Free: all world; Game: the recorded camera, lens only. Per console frame (no dt):
  orbit 2.5 degrees per stick unit capped at 3, pitch +-85, distance 0.2 m cap within 0.5..1000 m, look height 0.01 m
  within +-1 m; Free turn cap 1.6 degrees, move 0.2 m; lens 0.5 mm within the editor's 24..75 mm (default 24).
- Ours: `replay/camera_key.rs` (CameraKey, CameraRules as data, `key_from_view`, `edit`, `pose`, `lens_fov`), one key
  per clip in `replay/editor.rs`, the edit modes in `replay.rs` `editor_controls`, the pose and lens applied in
  `camera.rs` (before the shake), `GetCurrentCameraEditIndex` from the clip's camera, the edit states posted by the
  host. Rates are normalised to 30 Hz so any fps gives the same motion.
- One pose per clip is retail for the basic cameras (b404: their setup clears the channel's keys and the sampler
  `82C89A50` holds the value without keys). NOT RETAIL YET: the skater root for retail's objects 2 / 4, the pad mapping
  (left stick turns, right stick distance / move, D-pad up / down height, left / right lens; retail's axis table
  unread), the lens to FOV (24 mm high gate). The advanced cameras (rows 4..6, keyed poses on the cubic curve below)
  came later: see "M2 step 7: camera keys".
- Retail's key curve (b404, for the advanced cameras): keys at u16 fractions of the clip, cubic Hermite per segment
  `v(u) = ((a u + b) u + m0) u + p0` (a = m0 + m1 - 2d, b = 3d - 2 m0 - m1), tangents linear (m = d) unless the
  element's CUBIC flag is set, then the non-uniform Catmull-Rom (Bessel) tangent across a soft key, times the key's
  TANGENT (0..8, default 1); hard keys cut; some elements are stepped.
- Verified: `follow_orbits_in_the_skater_frame_with_retail_caps` (orbit pose, 90 degrees in one second at full stick,
  pitch and lens clamps), full bin 850 passed. No screenshot (the user is gaming: no windows). Not play-tested.

## M2 step 6: the timeline strip (markers on the timeline) (2026-10-20)
- Retail [code, b403]: the movie's mCustomRender clip has `_type` "CustomTimeline"; the APT custom render dispatcher
  `825D4828` calls `82509B80`, which draws in the clip's space (x = normalised time x 817) every frame: `82508E08` one
  untextured band per camera segment from y 3 to the clip height, coloured by the camera type (normal / selected,
  alpha 0.65 / 0.85; selected = no key under the playhead, not instant mode), and `82509298` one 13 x 51 quad per key
  after the first with `key_default` / `key_glow` (under the playhead within 0.00889) or `marker_default` /
  `marker_glow` (hard markers); in the camera edit modes the key under the playhead is not drawn.
- Ours: `apt_scene.rs` emits a `REPLAY_TIMELINE` placeholder with the clip transform; `retail_menu_stage.rs`
  `timeline_draws` expands it from the replay feed (one band for the clip's camera, the markers as keys) with
  `TimelineLook` (retail's colours, sizes, snap; data). The textures are retail's, already in the menu set.
- NOT RETAIL YET: the band bottom (clip height 30 assumed), our markers live on the speed lane (retail's keys are camera
  channel keys carrying the values), no hard markers (Advanced cameras), one segment per clip.
- Verified: `timeline_draws_the_camera_band_and_the_keys`, full bin 850 before the test was added. No screenshot (the
  user is gaming). Not play-tested.

## The opening sound (2026-10-20)
- Retail plays UI sound 0x394015B68783BE9E as Back opens the instant replay (`825EB998`, b389); in our front-end sound
  table that key is the fe record `editor_select` (level 0.88). `replay.rs` sends `FrontendSound::named("editor_select")`
  on open (`OPEN_SOUND`, data). Test: `retail_quit_warning_and_minimum_length` checks the message. Not heard yet.

## M4 step 1: the export encoder (Save to PC) (2026-10-20)
- User: "Add `windows` (Recommended)" (for "Save to PC": Windows' own encoder). `skate-game` now depends on `windows`
  =0.58.0 on Windows only (the version already in the lockfile; one added lockfile line), features MediaFoundation,
  Com, LibraryLoader, Shell (for the save dialog next).
- `replay/export.rs`: `ExportSettings` (size, 30 fps, bitrates, 48 kHz; data), `VideoSink` with two backends chosen at
  open: Media Foundation (H.264 + AAC in an .mp4 through the sink writer; its functions loaded at run time from
  mfplat.dll / mfreadwrite.dll, research b383, so the exe still starts on N editions without them; top-down BGRA frames,
  interleaved 16-bit stereo sound, hardware encoders allowed) and the fallback, PNG frames + a WAV in `<name>_frames`.
  Log `REPLAY_EXPORT` when the fallback is taken.
- Verified: `exports_frames_and_sound_to_a_file` wrote a real 2 s .mp4 (1.19 MB) with Windows' encoder on this PC, no
  window. Not yet built in the release (/MT, static) link setup.
- Next: render the clip frame by frame (replay cursor at the export rate, the world image read back from
  `SceneTarget`), the sound rendered offline from the replay audio, the save dialog, and the Finished menu with a
  "Save to PC" row (needs the game window to test: not while the user plays).

## M4 step 2: Save to PC (the Finished menu, the save dialog, lock-step rendering) (2026-10-20)
- B / Back in the replay (instant or editor) now opens our Finished menu with the retail strings: title
  `ID_REPLAYEDITOR_TITLE`, rows "Save to PC" (ours; data) and `ID_REPLAYEDITOR_EXIT` (retail's Exit = the quit, b387);
  B cancels. Retail's Upload / Save Replay rows need sign-in and storage and are not offered. Without the exporter
  resource the quit warning stays.
- Save to PC: the Windows save dialog (`IFileSaveDialog`, .mp4, its own thread), then `replay/export_run.rs` renders
  the clip (the editor's in / out points, else the recording) lock-step at 30 fps: per output frame the replay cursor
  is set, a one-shot `Readback` of `SceneTarget` brings back that frame's world image (rows unpadded from 256 bytes,
  RGBA -> BGRA, cut to an even size), and exactly 48000 / 30 samples of sound are pulled from the audio runtime the
  replay feeds (256-sample blocks, the fraction carried); the audio device plays silence meanwhile
  (`DEVICE_MUTED`), so the two never take the same blocks. Frames and sound go over a bounded channel to the encoder
  thread (`export::open_sink`: Media Foundation .mp4, else PNG + WAV). The controls are off while it runs; a popup
  says where the file went (or why it failed). Logs `REPLAY_EXPORT start / done / failed`.
- Verified: `the_finished_menu_offers_save_to_pc_and_exit`, `readback_rows_are_unpadded_and_swapped_to_bgra`, the
  encoder test; full bin 854 passed. Verify switch: `SKATE_VERIFY_REPLAY_EXPORT=<file.mp4>`.
- First game run (2026-10-20 evening, the user paused gaming): wgpu refused the readback (the scene image had no
  COPY_SRC usage); fixed in `graphics_menu.rs` (a usage flag only, rendering unchanged). Then a 10.5 s replay exported
  in about 9.4 s to a 16 MB .mp4 (1280 x 800, 30 fps, H.264 + AAC). Checked: the encoder test now decodes a frame back
  (Media Foundation's source reader) and finds a white top half on top (upright, colours right); the game export's first
  frame decoded to PNG shows the world without UI; its audio decodes to 10.5 s of 48 kHz stereo (quiet ambience, the
  skater standing still). The save dialog itself (B > Save to PC) is not clicked through yet; the user tests it.

## M2 step 7: camera keys, the key curve and the advanced cameras (2026-10-20)
- Retail [code, b405]: there is no separate advanced input mode. A camera row's setup `82C7FFB8(take, advanced,
  existing)` inserts a key at the playhead: basic rows (Follow / Tripod / Free) a **hard** key (both sides hard: the
  pose holds and cuts at the key), advanced rows a **soft** key (the camera blends through it on the channel curve).
  Then the same edit states as basic (7 / 8 / 10), but the movie gets states 12 / 13 / 14 instead of 1 / 2 / 3. An
  insert is refused within a minimum spacing of another key or the In / Out point (scaled by the clip length). In the
  timeline (mode 6, `82612C28`), A over a key marker seeks onto it and, on release, re-opens that key in the advanced
  state 12 / 13 / 14 without inserting; A elsewhere inside the clip opens the effects menu, where a camera row adds
  the key; D-pad Down release adds a marker (cap 100, `82613500`, popup `82613598`). b405 also corrects b404:
  `82C810A8` / `82C813B0` are effect lane setters 4 / 5, not key handlers; no pad path edits a key's CUBIC / TANGENT.
- Key curve (b404): `skate_core::replay_edit::curve`, retail's cubic Hermite per segment with linear tangents, or the
  Bessel (non-uniform Catmull-Rom) tangent across a soft key when the element is CUBIC, times the key's TANGENT; hard
  keys cut. Test `linear_by_default_cubic_across_soft_keys_and_cuts_at_hard_keys`.
- Ours: `replay/editor.rs` keeps a sorted camera key track (`CamKey`: frame, camera, hard / soft) instead of one key
  per clip. `pick_camera` inserts or replaces the key at the playhead, `reopen_at` re-edits the key under the playhead
  (edit states 12 / 13 / 14 posted by `replay_menu_host.rs`), `pose` returns the key in effect (a soft key blends into
  the next soft key of the same camera), `segments` feeds the timeline band (`retail_menu_stage.rs`: one band per
  segment, the recorded camera before the first key) and the markers come from the keys. D-pad Down drops a hard key
  holding the camera in effect (refused at a key, at the clip ends or at the cap). The advanced rows, the key kind
  and the pose blend are in `replay/camera_key.rs`. Cap, spacing and rates are data (`CameraRules`, editor limits),
  so a mod can override them.
- Fixes from the user's Follow edit screenshot: the right stick now orbits and the left stick moves in / out; the
  `DPad_UpDown` / `DPad_Arrows` prompts show the plain D-pad glyph (`prompt_glyphs.rs`); the Follow / Tripod cameras
  aim 1 m above the skater root (`CameraRules::look_target_height`).
- NOT RETAIL YET: D-pad Up removes the key at the cursor (retail's delete path was not found: b405 reads Up release as
  the same seek as A); the look target height (retail looks at an actor part, which one is open); the stick mapping
  (retail's axis table is unread); the look effects (DLC lanes) are not ported.
- Verified: `camera_keys_hold_cut_and_blend`, the curve test, `follow_orbits_in_the_skater_frame_with_retail_caps`
  (orbit centre now includes the look height); skate-core 1176 and full bin 855 passed. Verify switches
  `SKATE_VERIFY_REPLAY_EDITOR=markers` (two markers, the playhead glows on the second) and `=camera` (Follow picked at
  the playhead, then its edit mode). Not play-tested by the user yet.
