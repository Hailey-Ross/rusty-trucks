# 26c: Living world: NPC skaters: replay tier and animation

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Change (milestone 3: NPC skaters, replay tier)

Ambient NPC skaters are visible in free roam: the population's skater spawn records become skaters that ride the
retail recorded lines kinematically (the "replay tier"; the simulated tier with the full skater physics follows).

What retail does, checked in the code for this milestone (TU3):
- An NPC is a full skater steered along its line by the `PathController` (ctor `sub_824685F0`). At a node that
  carries a branch group the controller picks where to continue (`sub_8246BEE0`): stay on the line, or a branch
  target that exists, is not in use and is valid. The lowest score of `sub_8246C230` wins, the first on ties; there
  is no random draw and the branch record's f32 is not read [code]. A candidate is rejected when the line has no
  node after it, when the next node lies 50 deg or more off the skater's forward (`0x822F91B0`), or when an airborne
  or event node lies within one node (`sub_8246C4F8`). Score = angle x 572.958 + (offline) 1024 per other AI skater
  on the same line within 5 nodes (`sub_82456A38`) + min(30 x the player's distance to the line, 1500)
  (`sub_8246C5D8`) + 1000 for lines with flag bits 0..2 all set + a skill term. A taken branch starts at the target
  node nearest the skater among the 3 before it (`sub_82455BB0`) [code].
- Line nodes carry the skater and board orientation as quaternions x, y, z, w with +Z forward [data: 81 % of
  moving nodes]; branch groups never sit on a line's last node [data].

Change:
- `skate-core::living_world::replay`: the line cursor (node index + frame in the segment at the 60 Hz recording
  rate; any engine rate gives the same cursor), pose / velocity / heading / orientations / node flags and events,
  the phase (rolling, crouched, air, air trick, ground trick, off board), the retail branch choice, and a mirror mode
  that replays recorded branch decisions. `skate-data::living_world::replay_line` converts a decoded line.
- `skate-game::living_world::npc_skaters`: one entity per skater spawn record (`NpcSkater` with the stable
  `LivingWorldId`, character, slot, seed, voice; `NpcReplay` with the cursor). The cursor stays at
  2 x (population tick - spawn tick) recording frames, so an NPC's state follows from its spawn record, the tick and
  its branch records. Its position goes back into the population (culls, the 5 m rule). Collision: a kinematic
  capsule and board box join the skater solve through the network proxies (like a mod's solid), so the player bumps
  into NPCs. Look: the character's native roster GLB (the customiser's and online players' files), a mod override
  (`NpcSkaterLooks`), else the stock skater; bound to the stock skeleton like a remote player. Puppet animation: one
  stock clip per phase from the player's evaluator; the root takes the recorded position and skater orientation.
  The clip time is the time since the phase began; a looping clip (`_CYC` in its name) wraps by its length,
  `(frames - 1) / fps` like the player's `ClipClock`, others hold their last frame. Fix (2026-10-05): the time used
  to run past the clip end unwrapped and the sampler clamps there, so after about one clip length every NPC froze on
  the clip's last frame ("Skate npc animations are not playing"). Each NPC carries `NpcPuppetClip` (phase, clip,
  time, posed). Mod entry: `sdk.world.set_tuning('living_world', {skater_clips = {rolling = '...',
  ['rolling.Aggressive'] = '...'}})` keyed by the stable phase ids (`ReplayPhase::name`); an override that does not
  evaluate falls back to the shipped pick; cleared on mod disable (`reset_mod_overrides`). Tests:
  `living_world_npc_puppet_clip_attached_and_playing_per_phase` (headless), `..._looping_clips_wrap_and_others_hold`,
  data-gated `..._puppet_clips_play_past_their_first_loop`, `npc_skater_clips_set_merge_and_reset`.
  Audio: `NpcSkaterAudio` with a lite state (speed, air, ground trick as a grind) and the character's voice, so
  #32's NPC board sounds and speech play. Events: `NpcSkaterEvent` (spawned, despawned, node event, branch, line
  end). Debug: `SKATE_LIVING_WORLD_DEBUG=1` logs the NPC count and the nearest NPC (distance, line id, node, phase,
  speed) every 5 s.

Replay-tier simplifications until the simulated tier (documented, not retail): positions and timing come from the
recording, not from steering (`AIPhysicsInput`); the branch is evaluated once when the node is reached; the
obstacle-list rejection is not modelled; NPCs do not react to the player (the proxy is infinite mass, no bails);
tricks show one clip per phase (a grind, slide or manual is one 50-50 clip; the recording does not say which), the
stock graphs do not run; the end of a line with no branch taken despawns the NPC (retail behaviour not decoded).

Files: `crates/skate-core/src/living_world/{replay,replay_tests,mod,clock,population}.rs`,
`crates/skate-data/src/living_world.rs`, `crates/skate-data/tests/living_world_data.rs`,
`crates/skate-game/src/living_world/{npc_skaters,npc_tests,mod,tests}.rs`.

Verification (milestone 3):
- `cargo test -p skate-core --release --locked living_world::replay`: 7 tests (60 Hz timing and interpolation,
  frame-rate independence at 30 / 60 / 64 / 144 / 240 Hz, phases from flags and trick events, line end, every term
  of the branch score against the code constants, branch taken / stay / in-use target, client mirror equals host,
  orientation order).
- `SKATE3_ASSET_ROOT=<export> cargo test -p skate-data --release --locked --test living_world_data`: every one of
  the 1,691 lines rides end to end with the retail branch choice (1,331 branches taken, average ride 14.6 s; recomp
  NPC median life 13 to 28 s), every branch target resolves, no frame moves more than 3 m, deterministic.
- `cargo test -p skate-game --release --locked --bin skate3rust -- living_world`: 9 tests; new: NPCs ride their
  lines and each position equals a cursor rebuilt from the spawn record and the tick, 3 NPCs max, line ends despawn,
  audio velocity and voice published, same result at 60 and 144 Hz, NPC entities go with the population, proxy /
  audio state / clip table; with `SKATE3_ASSET_ROOT` every puppet clip evaluates on the stock banks.

Multiplayer: an NPC is reproducible from its spawn record, the console tick and its branch records
(`Decider::Mirror`); a client never decides (`NetRole::Client` mirrors). No transport.

Moddability: lines are keyed by retail id in a shared map a content overlay can extend or patch; looks by character
key (`NpcSkaterLooks`); NPC events are messages; spawn / despawn go through the population (stable ids). The
`sdk.living_world` NPC surface is designed below (open items) and comes with the mod milestone.

## NPC skaters fading out near the player (line end), 2026-10-05

**Problem.** User: "npc skaters are still fading out way too fast, and way too close to the player. I think its
measuing the distance from its spawn point, not the players distance from them.." and "They should continue skating
around".

**Cause.** The replay tier faded every skater 1 s before the end of its recorded line when no branch group was
ahead, wherever that was. [data] No line on the disc has a branch group on its last node (760 of 760 DownTown lines,
all 1,691), 426 DownTown lines have none at all, and the median line lasts 9 s (DownTown p10/50/90 4.3 / 9.0 /
18.7 s), so most skaters vanished after a few seconds, often close to the player. The fade itself is retail, but it
belongs to a timed controller state entered on a skater component flag (`sub_8246EF78`, byte `+59`; leaving it
cancels the fade), not to the line end.

**Evidence (retail code, TU3 recompilation).**
- [code] `sub_8246D3C0` (PathController node update): when the node index reaches the last node it calls
  `sub_8246C7F8` with the end position; otherwise a changed node goes to the branch choice `sub_8246BEE0`.
- [code] `sub_8246C7F8`: `sub_82458968(path manager, position, ..., 16 entries, mode 1)` lists lines; one result
  is taken as it is, several go through the branch chooser `sub_8246C1C8` (score `sub_8246C230`, lowest wins,
  index 0 when all are rejected); the controller takes the line id, node and path (`+592`, `+816`, `+800`).
- [code] `sub_82458968` mode 1: walks every loaded line (path hash map), skips lines in use (`sub_82458860`),
  takes the **start node** (node 0) and keeps lines whose start is within the radius; with none, it falls back to
  the nearest valid start at any distance (a nearer invalid one wins only beyond 6 m, `0x822F94F4` = 36).
- [data] Radius: `ai_skater` `default` field `Hash_4F87E7A70DA11691` = 4.0 m (read by `sub_8246C7F8`).
- [code] Ambient skaters otherwise leave by the population cull `sub_8245D520` (3-D distance to the player > 120 m,
  height > 1000 m) and the per-skater check `sub_8245A9B8` (fading state below opacity 0.2; character wanted
  elsewhere while over budget). [recomp] measured lives 13-28 s median, despawn distance median 115-123 m.
- [data] 742 of 760 DownTown lines (406 / 423 Industrial, 484 / 508 University) have another line's start within
  4 m of their end; median end-to-start distance 0.1 m (tool `.claude/skills/living-world/tools/line_ends.py`).

**Change.** At the last node the cursor continues on the next line exactly like that: unused lines whose start
lies within `ChainConfig::radius` (retail 4 m) of the end, at most 16 in id order (deterministic; retail walks a
hash map), one taken directly, several scored by the branch chooser, start at node 0. The decision is a
`BranchRecord` from the last node, so clients mirror it. The leave fade now starts only at a dead end (no start
within the radius), which the replay tier cannot ride past (retail's full skater would steer to the far start);
the fade in, the 1 s fade and the 0.2 removal are unchanged. So skaters keep riding until the 120 m cull.
Moddable: `SkaterConfig::line_chain`, `LivingWorldSettings::skater_line_chain`,
`sdk.world.set_tuning('living_world', {skater_line_chain = {radius, max_candidates}})` (radius 0 = fade at every
line end), reset on mod disable. The debug readout of the NPC skaters now uses `report_due` (it went silent after
a map reload).

**Files.** `crates/skate-core/src/living_world/replay.rs` (`ChainConfig`, `choose_next_line`,
`LineSource::for_each_line`, `LineCursor::chain`), `leave_fade.rs` (fade only at a dead end), `config.rs`
(`line_chain`), `replay_tests.rs`; `crates/skate-game/src/living_world/npc_skaters.rs`, `mod.rs` (settings),
`npc_tests.rs`; `crates/skate-game/src/modding/world_tuning.rs`; `crates/skate-mods/src/world_tuning.rs`,
`api.lua`; `sdk/skate.lua`; `crates/skate-data/tests/living_world_data.rs`.

**Verification.**
- skate-core: `npc_skater_chains_to_a_line_starting_within_4_m_like_sub_8246c7f8` (chains on the frame the end is
  reached, onto node 0, mirrored by a client; in-use and out-of-radius lines skipped; radius 8 / 0 from data),
  `line_end_choice_scores_like_the_branch_chooser_and_falls_back_to_the_first`,
  `npc_skater_keeps_riding_a_seeded_line_network_for_minutes` (seeds 1, 7, 1234: 5 minutes, never ends, two runs
  identical), `npc_fades_out_only_at_a_dead_end_and_goes_below_alpha_0_2`.
- skate-game: `living_world_npc_skaters_chain_lines_and_fade_only_at_a_dead_end` (no fade while a next line
  exists, chains at the line end, leaves by the distance cull; radius 0 gives the retail fade at every end),
  `..._ride_their_lines_from_the_spawn_record` (cursor rebuilt from the host's records), tuning test extended.
- skate-data (data-gated, exported lines): 1,691 rides, 19,948 branches and chains, 440 dead ends within 2 minutes
  (before: more than 1,600 ended within their first line).

**Open.** Line validity per character (`sub_82456970`) is not applied to branches or chains. Retail's fallback to a
far start (the full skater steers there) is not possible in the replay tier: 440 of 1,691 solo rides still meet a
dead end within 2 minutes and fade there. What the timed state on byte `+59` is (likely a bail) is unconfirmed.
Not seen in game yet.

## NPC skaters snapping between animations, 2026-10-05

**Problem.** User: "npc skaters look better, but they still snap between animations, there is no interpolation
between them."

**Cause.** The replay tier shows one stock clip per replay phase and swapped clips on the frame the phase changed
(rolling to air, air to rolling, ground trick, off board, also when a branch or line chain changes the phase). The
cursor only knew the current phase, so there was nothing to blend from.

**Evidence (retail).** Ambient skaters are full skaters on the same motion graph as the player (design doc 26), so
a clip change is the graph's transition: [code] Blend82B96058 (`playback_transition`, already ported for the
player) blends the outgoing tree into the incoming one with weight `elapsed / time` clamped to 0..1 and eased
`3w^2 - 2w^3` when `time` is 0.05 s (`0x3d4ccccd`) or more, linear below; the outgoing tree keeps playing
(matching mode 0). [data] `time` comes from each `PlayAnimation`; without one it is the literal at `0x82099280`,
0.2 s. The looping idle states the puppet clips stand for enter with 0.2 s (`air.xml` `B_AIR_CYC`,
`offboard.xml` stand `BLENDSEC`, `T_handplantair.xml` `IA_IDLE_N_N_0_CYC`). Across the stock motion graph the
times are 0.1 (119), 0.2 (89 plus 80 without a time), 0.05 (39), 0.3 (26), 0.15 (10, landings), up to 0.85 s.
No separate animation path for far AI skaters was found (no LOD animation strings besides `PhysicalPlayerHiLOD`).

**Change.**
- `skate-core::animation::playback_transition::transition_weight(elapsed, seconds)`: the player's transition
  weight, moved out of `PlaybackTransition::weight` unchanged so the NPC puppet uses the same code.
- `skate-core::living_world::replay`: the cursor keeps the phase it left and when it began; `ReplaySample` carries
  `previous_phase` and `previous_phase_frames`. Both follow from the cursor alone (a client mirrors them).
- `npc_skaters::present_pose`: while `transition_weight(phase_frames / 60, seconds) < 1` the pose is
  `[outgoing clip at its own time, incoming clip, Blend {weight}, RIG_TPOSE, Add]` (the player's SQT blend);
  same clip on both sides = no blend. A mod clip that does not evaluate falls back to the shipped pick on both
  sides. `NpcPuppetClip::blend` shows the crossfade in effect.
- Moddable: `LivingWorldSettings::skater_blend_seconds` (phase id or `default` -> s, empty = 0.2 s), Lua
  `sdk.world.set_tuning('living_world', {skater_blend_seconds = {default = 0.3, air = 0.1}})`, validated
  (known phase or `default`, 0..10 s), first writer wins per key, readable via `world_tuning`, cleared by
  `reset_mod_overrides()` on mod disable. Mod clips blend the same way.

**Files.** `crates/skate-core/src/animation/playback_transition.rs`, `crates/skate-core/src/living_world/replay.rs`,
`replay_tests.rs`, `crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `mod.rs`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`, `api.lua`, `sdk/skate.lua`.

**Verification.** skate-core `living_world`: 105 passed (new: the cursor keeps the previous phase; the weight
curve is 0 / 0.15625 / 0.5 / 0.84375 / 1 at 0 / 0.05 / 0.1 / 0.15 / 0.2 s, linear below 0.05 s); skate-core
`animation::` 44 passed (player transitions unchanged). skate-game: `..._puppet_blend_follows_the_transition_curve`,
`npc_skater_blend_seconds_set_merge_and_reset`, and data-gated `..._puppet_clip_change_has_no_pose_jump` (every
pair of puppet clips: the biggest per-frame joint move across the change stays within the clips' own motion plus
one curve step of the gap; worst old snap 0.94, worst blended frame step 0.13). Not seen in game yet.

**Open.** Per-transition times: the replay tier does not run the graph, so every phase change uses the stock
default 0.2 s (retail landings use 0.15 s, grind entries 0.3 s, many `INTO` clips 0.1 s; the simulated tier gets
them from the graph). A phase change during a running blend starts a new blend from the previous phase's clip
alone (retail nests the running transition). Jitter between 60 Hz ticks is a separate cause (fix 12 report).

## NPC skaters jittery while skating, 2026-10-05

**Problem.** User: "The skater npcs look decent but a little jittery when they skate."

**Cause.** Three presentation faults of the replay tier, none in the line data:
1. The root was placed between ticks, but the clip time and the crossfade weight advanced in whole 60 Hz frames
   (`phase_frames / 60`), so at 144 Hz the body pose repeated frames under a smoothly moving root.
2. The render extrapolated past the current tick with a look-ahead cursor that never branches
   (`Decider::Stay`), while the player is drawn from the previous to the current fixed state. At a branch or chain
   the look-ahead rode the old line, then the next tick corrected it.
3. A branch or chain moved the line under the skater at once: on the exported lines (212 seeded rides, 881
   switches) the gap is p50 0.46 m, p90 2.8 m, p99 6.7 m, max 16 m (branch targets far from the branch node).

**Evidence (retail).** Retail has no snap and no blend time here: [code] at a line end `sub_8246D3C0` ->
`sub_8246C7F8` only stores the new line id and node (`+592/+600`, `+816`, fix 9), and the branch choice
`sub_8246BEE0` likewise only picks a line and node; the full skater keeps its physical position and its
`AIPhysicsInput` steers onto the new line (not traced in the recomp for this fix). The orientation inside a segment is
already nlerped between the two nodes' decoded quaternions (continuous at nodes), unchanged.

**Change.**
- `skate-core::living_world::replay`: `LineSwitch` (frame, drawn offset, drawn skater and board orientation at
  the switch, including a blend still running); the drawn root decays from it onto the new line with the graph
  transition curve (`transition_weight`) over `ChainConfig::blend_seconds` (default `SWITCH_BLEND_SECONDS` = the
  stock graph's default transition time 0.2 s, an engine stand-in for retail's steering; 0 = cut).
  `ReplaySample::sub_frame` (render fraction). `LineCursor::render_sample(lines, records, frames_ahead)`: steps
  whole frames with the recorded branch decisions (`Decider::Mirror`), never guessing, then samples the fraction.
  All of it follows from the cursor, the branch records and the fraction (a client draws the same pose).
- `npc_skaters`: `NpcReplay::previous` (cursor one world tick back, taken before the tick's last step);
  `present_pose` draws `previous.render_sample(branches, fraction)` like the player's previous-to-current
  interpolation; clip time and crossfade use `(frames + sub_frame) / 60` (`puppet_blend_at`). The cursor's
  `switch_blend_seconds` is set from the tuning each step.
- Moddable: `sdk.world.set_tuning('living_world', {skater_line_chain = {blend_seconds = 0.5}})` (0..10 s,
  validated, first writer wins, read back, reset on mod disable with the rest of `skater_line_chain`).

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`,
`crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-data/tests/living_world_data.rs`, `sdk/skate.lua`.

**Verification.** skate-core `living_world` 107 passed (new: `npc_skater_render_is_smooth_across_branches_and_chains`
renders a ride with a 0.8 m / 30 deg branch and a 1.5 m chain at 144 Hz: every root step stays below the line speed
plus one smoothstep step of the gap, the turn likewise, clip time grows by exactly 1/144 s, the cut ride jumps,
identical when mirrored; `npc_skater_render_is_smooth_on_a_seeded_line_network`, seeds 1 / 7 / 1234). skate-game
`living_world world_tuning` 50 passed (new: `living_world_npc_skaters_render_smoothly_between_ticks_and_across_chains`,
40 s at 144 Hz, worst root step below 0.075 m, clip time on the render clock; tuning test covers `blend_seconds`).
skate-mods `world_tuning` 3 passed. Data-gated `npc_skater_render_is_smooth_on_the_exported_lines` (pm3 export):
worst render step while a switch blends 0.75 m (the 16 m gap), 15.9 m cut; elsewhere 0.36 m. Not seen in game yet.

**Open.** Large branch gaps (p99 6.7 m) still slide fast over 0.2 s; retail would steer (and hand over to the nav
mesh beyond its 4.5 / 6 m corridor). A gap-scaled blend time or the simulated tier would cover it; mods can raise
`blend_seconds` now. The 8-bit node quaternions (about 0.45 deg steps) are interpolated as recorded, not smoothed.

## NPC skaters switching the side they stand on, 2026-10-05

**Problem.** User: "NPC skaters look much better and seem to skate much smoother. They still have weirdness with
switching sides they are standing on their boards randomly". In the user's capture an NPC rolling on flat ground
turns its whole body round in under 0.1 s while the board keeps rolling the same way.

**Root cause.** The replay tier draws the recorded skater orientation as the puppet root. The lines are recorded
human runs, and some recorders rode fakie at places (the skater frame faces against the travel direction): DownTown
34 of 760 lines start fakie and 40 end fakie, and branch targets also land inside fakie stretches. At a branch or a
chain the NPC took the new line's recorded facing, so the 0.2 s switch blend turned it 180 deg. On the exported
lines (212 seeded 40 s rides) 130 of 882 switches spun the skater round, about one per ride, which reads as random.

**Evidence (retail).** [code] A switch stores only the new line and node (`sub_8246D3C0` -> `sub_8246C7F8`
`+816`, `sub_8246BEE0`; fix 9 and 14); the full skater keeps its own body, so its facing and stance carry onto the
new line and only a performed trick or revert turns it. [code] The path frame retail builds from a node is the board
orientation (node `+0x18`, `sub_82453970`) turned 180 deg about its up axis when the node's `m_IsBoardFlipped` bit
(flags `+0x28` bit 0) is set: `sub_82453A58` and the interpolating `sub_824734A8` (called from the controller update
`sub_8246D560` into controller `+272`) negate the frame's X and Z rows. [data] With that turn the board frame matches
the recorded skater frame (flag clear: 20,195 of 21,191 moving nodes within 22 deg; flag set: 11,679 of 12,653 at
180 deg). Stance (regular or goofy) is not stored per node.

**Change.** `skate-core::living_world::replay`: `LineCursor::facing_flipped`; at a branch or chain, when the new
line's recorded skater faces more than 90 deg away (about +Y) from the drawn skater, the cursor toggles it and rides
the line turned 180 deg about the skater's own up axis (`turn_about_up`, `q * (0, 1, 0, 0)`, retail's board-flip
turn), applied to the skater and the board orientation in `sample` and in the switch blend. Recorded turns on a line
(180s, reverts) still play, from the facing the skater has. A pure function of the lines and the branch records (a
client mirroring the records derives the same flip). Moddable: `ChainConfig::keep_facing` (retail `true`),
`sdk.world.set_tuning('living_world', {skater_line_chain = {keep_facing = false}})` takes the recorded facing
(validated, first writer wins, read back, reset on mod disable); the cursor copies it each step like the blend time.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`,
`crates/skate-game/src/living_world/npc_skaters.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-data/tests/living_world_data.rs`, `sdk/skate.lua`.

**Verification.** skate-core `living_world` 108 passed (new `npc_skater_keeps_its_facing_across_a_line_switch`: a
chain onto a line recorded fakie keeps facing +Z on every frame, the line's recorded revert still turns it,
mirrored client identical, `keep_facing = false` spins as before). skate-game `living_world world_tuning` 50
passed (tuning test covers `keep_facing`), skate-mods `world_tuning` 3 passed. Data-gated
`npc_skater_keeps_its_facing_across_switches_on_the_exported_lines`: 212 rides, 882 switches, 130 kept by a flip,
0 spins (130 without `keep_facing`), deterministic. Not seen in game yet.

**Superseded (fix 23, 2026-10-08):** the flip is now a mod option, off by default; see "NPC skaters riding
backwards" below.

**Open.** The replay puppet plays one forward clip per phase; retail plays fakie riding clips while fakie and the
character's stance (regular or goofy) mirrors them. Recorded in-line facing changes on the ground without air (about
12 in DownTown) still turn the root within a node, where retail performs a revert. Both belong to the simulated tier.

## NPC skaters popping between animations, tricks not playing, 2026-10-05

**Problem.** User (after fixes 12 to 19): "NPC skaters still seem to pop between animations and trick animations
appear to be missing or not playing." / "they pop between the animations, one ends and suddenly the next is already
partially going, their limbs jump to each position between making them appear to pop between animations" / "When
they jump nothing plays, i can hear the sounds of a tick, but the board doesn't do the trick and their limbs do not
animate like they are doing the trick."

**Root cause.**
1. The puppet crossfade (fix 12) only knew ONE previous phase. On the shipped lines 13,183 of 27,464 phase changes
   come before a 0.2 s blend could finish [data] (crouch phases are 0 to 2 frames, a takeoff about 10, an air trick
   about 13). Each such change restarted the blend from the previous clip alone, dropping the running blend: the body
   jumped to that clip's pose in one frame (measured up to 0.83 m per joint on a jump).
2. Every recorded jump opens its trick span on the ground (most common sequence rolling > ground trick > air trick >
   air [data]), and the ground trick phase showed the 50-50 grind clip: the takeoff flashed a grind pose.
3. The node's trick slot was never read for animation: the air trick phase played an air idle clip
   (`IA_IDLE_LO_N_0_CYC`, the board turns 3 deg), so no flip, no board trick.

**Retail.**
- [code] The trick slot is the 40 B extended record's i16 at +0x24, an `EScorableID`: `sub_8246A2E0` (0x8246A4A0)
  loads `*(node+0x20)`, then `lhz 36(ext)`, `extsh`, and treats `> -1 && < 332` as a trick (332 = the scorable
  table 0x820862A8, `skate_core::scoring::catalog`). On the shipped lines: ollie 1925, gnd_bsgrab 829, kickflip 734,
  bsgrab 409, fsgrab 365, nollie 322, n_heelflip 322, heelflip 264, then grinds [data].
- [code] Ambient skaters are full skaters on the player's motion graph (doc 26 section 1); a graph transition keeps
  the running transition as its outgoing tree (Blend82B96058, fix 12), so blends nest.
- [data] `MotionGraphIncludes/Tricks/Tricks.xml` pairs each `TRICK_NAME` with an `ANIM_NAME`; `T_Trick.xml` /
  `T_Ollie.xml` / `T_Kickflip.xml` play `$ANIM_NAME$_G` on takeoff (`time="0.05"`), then `$ANIM_NAME$_A` with
  `transType="sequence"` (follows the ground clip, no blend; `time="0.1"` when entered from the air); the kickflip
  family continues with `$ANIM_CYC_NAME$1..3` and `$ANIM_OUT_NAME$n`. The `B_*` names are authored trees in the stock
  banks (phase blends over `TRICKHEIGHT`, selectors over `PROSKATER`), e.g. `B_KICKFLIP_IN_A` = phase blend of
  `KICKFLIP_IN_LOW_A` / `KICKFLIP_IN_HIGH_A`. The board is the rig's `Skateboard_Root`, so the trick clip moves body
  and board together (kickflip air sequence: board turns 169 deg).
- [video] Retail (RPCS3, `2026-10-05 13-01-52.mp4` 3:23.6, a varial heelflip from the same trick trees): the board
  turns under a continuous body over about 0.4 s (`.local/research/npc/fix21/retail-varial-heelflip-board.jpg`).
  NPC skaters in that recording are too far from the camera to read limb detail.

**Change.**
- `skate-core` `living_world/replay.rs`: `ReplayLine::{open_trick_at, node_trick}`, the cursor keeps the span's trick
  id and a 6-entry `PhaseEntry` history (phase, start frame, trick), `LineCursor::phase_history()`,
  `render_cursor()` (the cursor `render_sample` samples). `ReplaySample` unchanged.
- `skate-game` `living_world/npc_skaters.rs`: `puppet_layers` builds the pose from every phase whose blend still runs
  (oldest first, each blended over the result, stops at the first fully blended one; consecutive phases on one clip,
  or a clip continuing a `<ground>+<air>` sequence, are one layer from the older start); `puppet_layers_pose`
  evaluates them (`PoseCommand::Blend` chain); `puppet_layer_clip` picks per phase: a trick span with a trick
  animation plays `<base>_G+<air sequence>` from the ground (0.05 s in) or the air sequence alone from the air (0.1 s
  in), the air after an air trick continues it, everything else the phase clip as before; `retail_trick_anim`
  (EScorableID -> `B_<NAME>` per Tricks.xml; kickflip / heelflip and nollie forms `B_<NAME>_IN`; numbered, late,
  underflip, dark-catch variants their base flip or the ollie; grabs and other air tricks the ollie takeoff; grinds,
  slides, manuals, powerslides, reverts and handplants none), `trick_air_sequence`, `stock_tree_leaf` (selector
  default, phase blend first child = low `TRICKHEIGHT`), `sequence_part` (`+` parts back to back).
- Moddable: `skater_clips["trick.<scorable name>"]` = an animation base (or a plain clip), `skater_blend_seconds`
  keys `trick_takeoff` / `trick_air` (`skate-mods` validation, `sdk/skate.lua`); reset on mod disable with the rest
  of `LivingWorldSettings`. Multiplayer: the pose is a function of the cursor (lines, branch records, tick), no new
  records.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`; `crates/skate-game/src/living_world/npc_skaters.rs`,
`npc_tests.rs`; `crates/skate-mods/src/world_tuning.rs`; `crates/skate-data/tests/living_world_data.rs`; `sdk/skate.lua`.

**Verification.** skate-core `--lib living_world` 114 passed (new `replay_cursor_keeps_a_phase_history_with_the_span_trick`),
`--lib animation::` 44 passed; skate-game `--bin skate3rust -- living_world world_tuning` 56 passed (new
`living_world_npc_puppet_layers_nest_running_blends`, `..._trick_slots_pick_the_stock_trick_animation`, data-gated
`..._trick_clips_exist_and_flip_the_board`: every mapped tree resolves to a stock clip, kickflip board 169 deg vs 3 deg,
and `..._trick_jump_has_no_pose_pop`: on recorded jump timelines the pose at a phase change leaves the showing pose by
0.008 m, the one-previous-clip blend by 0.83 m); skate-mods `world_tuning` 3 passed; skate-data `living_world_data`
8 passed (new `npc_skater_trick_slots_and_phase_changes_on_the_exported_lines`). Not seen in game yet.

**Open.** Trick height: retail blends low/high by `TRICKHEIGHT`; the NPC plays the low clip (the line's jump record
has a launch velocity that could give the height). Grabs show the ollie (retail adds the grab from the action graph,
`T_Grab.xml`); grind / slide / manual spans still show one 50-50 clip (the slot names the grind: next step); spins
(`spins` byte) and landing clips (`B_LAND_*`, 0.15 s) are not played; the per-skater `PROSKATER` selector takes the
default branch.

## Silent landings (NPC skater sounds filled the voice cap), 2026-10-07

**Problem.** User: "landing was quiet randomly", "At the carvetron. had two silent landings", "there were several",
and after the next session "last session had a silent landing near the end of the session".

**Root cause.** The landing logic plays every touchdown (a headless replay of the last 95 s of the user's 46-minute
session starts the touchdown voices on every landing). The in-game mixer, however, was full: it sat at its 96-voice cap
(`Mixer::max_voices`) and refused new opens without an error, so a touchdown whose voices were all refused was silent.
The voices leaked from NPC skaters: when an NPC skater lost its audio instance the host dropped its `NpcSkater` without
releasing its Splice sounds (`world::skaters::NpcSkater::deactivate` assumed "the Splice one-shots end by
themselves"). A Splice sound is only stepped, and its mixer voice only released, by its owner's update or release, so
the dropped skater's touchdowns, pop, roll, taps, scuffs, hand-on-deck, grind on / off and clothing sounds kept their
`SplicePlayer` slots and mixer voices for good. The mixer kept finished voices until released and counted them against
the cap.

**Evidence.** Session log of 2026-10-07 (AUDIO_TIMING `block_voices`): max 96 in 790 of 2789 s, from about 9 minutes
in; 96 every second in the last ~105 s. The per-minute voice floor rose from 17 to 90, only while NPC skaters were
released (58 after 63 releases, flat while none were released, 90 after 143), while the world at the end held
`skaters 2/0`. 143 of the 621 landings fell in seconds at the cap. Full report:
`.local/research/audio/silent-landings-2026-10-07.md` (local, not in the repo).

**Change.**
1. Retail parity (retail's release stops every layer): `NpcSkater::release` releases every Splice sound the skater
   holds: `Contacts::release_all` (pop, roll, ollie, landing, touchdowns, second voice, manual landing, taps, scuffs,
   plant / lift, and `StepOn::release_all` for the hands on deck), `Grind::release_sounds` (grind on / off, queued
   starts dropped) and `Clothing::release_all` (stroke / plant foley). The NPC host calls it next to `stop_wheels`
   when a skater loses its instance and for every skater on a map change (`NpcHost::reset`). Release is per owner id
   and deterministic (multiplayer-ready). Peds (`PedObjects::release`) and mod voices (`ModVoices`: ended one-shots
   are forgotten, `stop_owner` on mod disable) already released everything; checked, unchanged.
2. Safety net in the mixer: at the cap, voices whose sample has ended (`done`; every owner query already reads them as
   gone) are freed before the open is refused (`Mixer::make_room`, the same de-click fold as a release). Below the cap
   nothing changes, so what plays is identical until the cap is reached. Retail does not keep finished voices
   allocated (aems-voice-graph-spec 6.6).
3. Diagnostics: `Mixer::refused_cap` and `Mixer::evicted`; the `AUDIO_TIMING` line shows them per second as
   `voices_refused=` / `voices_evicted=` (only when not 0), so a future session shows refusals directly.

**Moddability.** No new mod surface: mod-owned voices are already released on mod disable (`stop_owner`), mods' mute
rules (`Observed`) see the same starts as before, and a released NPC frees its sounds whatever mod content its banks
hold. The cap stays a plain `max_voices` field.

**Files.** `crates/skate-audio/src/world/skaters.rs`, `crates/skate-audio/src/player/{contacts,step_on,clothing,components}.rs`,
`crates/skate-audio/src/mixer.rs`, `crates/skate-audio/src/splice/mod.rs` (`sound_count`, diagnostics),
`crates/skate-game/src/game_audio/npc_skaters.rs`, `crates/skate-game/src/game_audio/timing.rs`.

**Verification.**
- New tests: `releasing_an_npc_skater_frees_its_splice_sounds_and_mixer_voices` (an NPC ollie and landing holds its
  sounds; after its release the Splice sound count and mixer voice count return to the start),
  `two_hundred_claim_release_cycles_keep_the_voice_count_bounded` (0 / 0 after every cycle, no refused open),
  `finished_voices_do_not_block_a_new_open_at_the_cap` (below the cap a finished voice stays as before; live voices
  still refuse at the cap; finished ones make room).
- `cargo test -p skate-audio --locked`: all pass. `cargo test -p skate-game --bin skate3rust --locked -- game_audio
  living_world`: 124 passed.
- Behaviour identity for the player: the e2e bench (`tools/audio-e2e/scenarios.py`, 12 scenarios, `E2E_FPS=60`)
  rendered before and after the change: all 48 outputs (audio, voices, body, deck) byte-identical.

**Open.** Confirm in the user's next session that `block_voices` stays flat and `voices_refused` stays absent. "No
landing noise when landing in manual" is a separate report (by design the kind-2 touch is skipped while in a manual);
check it against `sub_824BB330` if the user still hears it.

## NPC Skater Physics: hitches, skaters vanishing up close, path projection (2026-10-13)

### Problem
User (2026-10-13): "I can only see skaters in the distance, anytime I get close they just dissapear." / "yea npc
skaters just dissapear when they get close and I get massive hitches" / "FIX IT, IM NOT SWITCHING IT OFF".
Sessions `hails-additions/logs/game-20261013-183803` and `-204749` (NPC Skater Physics opt-in on): 59 / 63 switches
replay -> physics, 26 / 43 bails; 35 of 39 switches hitched over 150 ms.

### Causes and changes
- **Hitches** (measured, `NPC_SKATER_SIM_LOAD`): every switch loaded a skater from disk: runtime 149 ms, controls
  31 ms, camera 37 ms. Now `NpcSimPool` keeps loaded skaters with their own physics context (filled up to the sim
  limit on the first enabled tick, refilled by hand-backs and despawns); a switch or a bail respawn places one with the
  player's teleport (`SkaterRuntime::travel`; retail places its AI skater object again). Its own context is kept (a
  fresh context restarted the clock and the camera refused the older subject tick). Earlier the same day: a blocked
  skater no longer restarts the sim every frame beyond the radius.
- **Start state** (research `.local/research/npc/b109-npc-sim-bails.md`): sims started between nodes, from rest
  (the teleport reset zeroed the context velocity) and 0.1 m up. Now a sim starts only on a node with retail's spawn
  push (x 0.75, `82C04168`) carried through the teleport.
- **Path projection** (research `.local/research/npc/b110-ai-path-projection.md`, new skate-core
  `living_world/path_follow.rs`): the line cursor of a simulated skater followed the recording's clock, so the
  target and the trick events ran away from the skater (137 of 144 trick dispatches were more than 1 m off the line).
  Ported `sub_8246D560`: nearest node in a forward window (`sub_82455BB0`), two-segment projection with the 0.5
  hysteresis and the 0.25 clamp (`sub_824563D8` / `sub_8246C930`), lane test (widths / 50, `sub_8246CCC8`), rejoin
  commit within 0.5 m (`sub_8246E2B8`), cursor B one shaped recorded tick ahead (`sub_8246E3B8`), on path dropped
  beyond 1.0 m outside the lane (`sub_8246DB50`; no recorded trick then). The cursor now goes to B
  (`NpcReplay::follow_target`); replay resumes from there on hand-back. Values: `SimulatedTierSettings::path`
  (retail defaults, mod-overridable). Log `NPC_SKATER_PATH` (left / rejoined the path).
- Test knob `SKATE_NPC_SIM_RADIUS`; per-system living-world timing in `FRAME_HITCH` (`living_world=[...]`).

### Verification
- skate-core `living_world::path_follow::tests` (4); skate-core living_world 285 passed; skate-game living_world 87.
- Muted DownTown runs, radius 150 m, 150 s, up to 3 sims:

| build | disk loads | hitches over 100 ms | bails |
|---|---|---|---|
| before (user session 204749, radius 40 m) | one per switch | 82 | 43 |
| pool, node start, trick gate | 27 | 14 | 29 |
| + own context kept | 4 | 3 | 26 |
| + path projection | 3 | 2 (both at load) | 17 |

### Open (next steps)
- Remaining bails are mostly ground wipeouts while rolling (`PhysicsGround -> WipeoutGround`) and many path leaves
  (48 left / 31 rejoined in 150 s). Not ported: retail's steering-target gate off path and before node 3
  (`sub_8246DE38` is skipped there; ours still writes the record from B), the "lost" hand-over to the NavMesh
  (`sub_82468DD8` / `sub_8246FE38`), the node-change handler `sub_8246BEE0`, the progress tracker `sub_82468240`,
  which body point retail projects (`sub_82467FC8`; ours the deck).
- A bail still hides the skater for 5 s (retail respawn delay); what the user sees as "vanishing".

## NPC Skater Physics: bails (2026-10-13 night)

### Problem
User (2026-10-13): "FIX IT, IM NOT SWITCHING IT OFF" (NPC Skater Physics skaters bail and vanish up close). Muted
DownTown baseline (radius 150 m, 150 s, player idle): 17 bails, 79 on/off-path switches.

### Causes and changes
- **Steering record off the path** (`sub_8246DB50`, main checked 0x8246DB50..0x8246DD40): retail zeroes the record,
  seeds the flag word with `pc+922 << 25` and clears bits 31..26, and only builds the steering target
  (`sub_8246DE38`) when the skater is on the path and past the start nodes (`pc+920`, `pc+937`), or always without
  on-board steering. Ours wrote the steering target every tick, pulling a skater that had left its line back toward a
  point metres away. Now `ai_record::steers` / `ai_record::without_steering`: off the path the skater coasts on its
  own physics until the rejoin commit. 17 -> 4 bails.
- **Respawn counted twice:** the placing teleport is queued until the next physics step, so the old wipeout bit was
  still set on the next tick and started a second 5 s bail (2 of those 4). `NpcSim::placed` ignores it until the bit
  clears.
- **Line kept running during a bail:** the projection kept moving the cursor while the skater lay on the ground
  (node 6 -> 14, a trick dispatched mid-bail). During the respawn wait there is no projection now (the line waits, as
  the comment always said).
- **Stale first tick:** the first tick after a start or respawn projected the body's position from before the queued
  teleport (a pooled body: anywhere), moving the cursor nodes ahead ("left the path at node 1 (A 5+1.00, 79.25 m)").
  `NpcSim::teleport_pending` skips the projection until the teleport has been applied.
- **Respawn node** (`sub_8246ED68`, decoded and main checked 0x8246ED68..0x8246EE24, picked when the bail starts by
  `sub_8246EE30`): the projection's search node `pc+816` plus 2, a remembered prop node ahead within 20 instead,
  clamped to the last node, then the first safe node (`sub_82455728`, our `ai_controller::safe_node`). Ours placed the
  skater where it fell, on a take-off: one skater respawned 24 times in a row at node 17, wiping out right after each
  placement. Now `ai_controller::respawn_node`. Not ported: game modes 19 / 20 (`sub_824681D8`) use
  `sub_824553D8(line, cursor A)`.
- **Lost hand-over** (research `.local/research/npc/b115-lost-handover.md`; main checked the test's structure in
  `sub_82468DD8`): on a tick its search node did not advance, a skater that was off the path, more than 6.0 m from
  the search node and from both lane edge points, and slower than 14 m/s (`pc+808`) is lost; that raises the same
  reposition request (`ctrl+941`) mode 7 raises, so controller B walks it back (goal `pc+816` + 2, safe node). On B's
  hand-back retail's A activate (`sub_824689C0`) resets the controller off the path with the search at the node
  nearest the skater over the whole line (`sub_82455108`); the normal rejoin commit puts it back on. Ours re-attached
  on the path at B's goal node. Now `path_follow::lost`, `PathFollow::reattach`, `path_follow::nearest_node`; the
  reposition counts from `pc+816`. Log `NPC_SKATER_LOST`. Values in `PathTuning` (`lost_radius` 6.0,
  `lost_max_speed` 14.0; retail defaults, mod-overridable).
- **Projected point** (research `.local/research/npc/b116-path-follow-inputs.md`, main checked the state 500 /
  wipeout tests in `sub_82467FC8`): off the board (BipedGround) or wiping out retail projects the animation root
  (Skeleton +416), not the board; the speed `pc+808` (`sub_8246BCF8`) is that point's displacement, negative only
  above 0.01 m/s against the heading (the skeleton's forward off the board). Ours projected the deck always, so a
  skater walking back was measured where its board lay. Now `npc_sim::projected_point`; the hand-back's nearest node
  uses the skater too.
- **Getting back on the board** (research `.local/research/npc/b117-ai-reboarding.md`, main checked the press in
  `sub_8246FA30`): retail's line controller A, not controller B, re-boards a skater: every path tick it presses the
  off-board toggle (`824712E0`: `NewToggleOffBoardState` + `ToggleOffBoardState` while off the board and not
  toggling, every 5 ticks) and then sets mode 2 ("get on") for the next tick; A's activate clears it. Ours had no
  toggle in A, so a skater handed back by controller B stood off its board for good. Now
  `ai_controller::OffBoardToggle` in the line tick (period from the controller B settings, retail 4).
- **Line ends and switches** (research `.local/research/npc/b118-node-change-line-end.md`): retail has no
  "finished" line for an AI skater: at the end it looks for the next line every tick (`sub_8246C7F8`), and every
  switch runs the reset `sub_82468AC8`, which leaves the skater off the path (rejoin commit again) but keeps the start
  gate `pc+937`. Ours handed a simulated skater at a line end back to the replay tier, which faded it out, and
  restarted the projection on the path after a switch. Now `LineCursor::retry_chain` each tick for simulated skaters
  (log `NPC_SKATER_CHAIN`), no line-end hand-back, and `PathFollow::switch_to` on a switch and on controller B's
  hand-back. The retry runs whenever the cursor sits on the last node (the projection holds it there, so it never
  became "finished" and never retried at first).
- **Line-end fallback** (research `.local/research/npc/b122-line-candidates.md`, `sub_82458968` mode 1): with no
  line start within 4 m, retail takes the nearest free start at any distance (a valid one over a nearer invalid one
  below 36 m^2; per-character validity is not modelled, so every line counts as valid). Ours ended the line, and the
  skater rode off, got lost, bailed and was respawned at the line end again: two skaters made 70 of 72 bails in a
  300 s run that way (each placement there launched them at 25 to 100 m/s, cause not found; trace
  `NPC_SKATER_PLACED` at debug level). Now `ChainConfig::fallback` (on for the physics tier only; the replay tier
  cannot ride the gap). Not ported yet: retail's end point (B past 0.8 of the second-to-last segment), the 4.5 m
  lost test after a re-pick, junction scoring from the physics skater.
- **Ground steer** (research `.local/research/npc/b121-ai-ground-steer.md`): retail also steers an AI skater with
  stick input every path tick on the ground, on or off the path (`sub_8246ACF0` -> `sub_8246AA00` /
  `sub_82471070`): the heading error to a point 45 ticks ahead on the line, moving toward the point 2 ticks ahead
  when the skater is 0.4 to 3 m off it, written to `Turn`, `BodySpin` and `KickTurn` (full at 18 degrees). Off the
  path this is what turns a skater back to its line. Ours had no stick input at all. Now skate-core
  `living_world::ground_steer` (tuning `SimulatedTierSettings::ground_steer`, retail defaults); `PathFollow` keeps
  the lateral ratio `pc+868`. Not mapped: the pending trick's category 9 gate.
- **Respawned cursor ran ahead:** the re-created cursor counts its own frames, but the follow target still held the
  old count, so the cursor ran up to 38 nodes ahead right after the respawn (then "lost" 50 m from the path). Now the
  target is reset with the cursor.
- **Airborne** (research `.local/research/npc/b119-state-byte-438.md`, main checked the writer in `sub_82DB6EC0`):
  the skater state byte `+438` that `sub_8246DB50` and the junction switch read is "an air state (200..299) with no
  wheel touching", not "riding". While airborne retail writes no steering target and does not drop the path. Now the
  sim passes it to `ai_record::steers` and `PathFollow::tick`. The replay tier's flip latch reads the same byte as
  "riding" (replay.rs `node_riding`); left as is tonight (research b120 first: it touches the facing rule behind the
  earlier backwards-riding fix).
- **Air steer** (research `.local/research/npc/b123-ai-air-steer.md`): in the air retail steers the character toward
  the recorded skater forward at the near look-ahead (`sub_8246B358`, reversed when the takeoff latch `pc+927` says
  the character faced against the node's recorded skater, `sub_8246A700`), with a 2 degree dead zone and full stick
  at 10 (`sub_82471188`), on the same three keys. Now skate-core `living_world::air_steer` (tuning
  `SimulatedTierSettings::air_steer`). Measured at takeoff: the character forward (animation root row 2) lines up with
  the deck forward (dot 0.9 to 1.0), so the input axis is right. **Off by default for now:** muted A/B runs (240 s,
  same build, `SKATE_NPC_AIR_STEER=1` to turn it on) had more bails with it: radius 150 m 48 vs 19, radius 110 m 33
  vs 24. Candidates: the board adjust write it also makes (`BoardAdjustAngle` / `BoardAdjustMag`, `sub_82471008`, not
  ported), whether retail's AI input map keeps or clears an unwritten key between ticks, the slerp fraction
  (`sub_82459F68`). Ported anyway so the next step is a switch, not a rewrite.
- `NPC_SKATER_BAIL` now says what the skater was doing: previous state, speed, node, cursor A, on path, distance to
  the path and position (logs must diagnose).

### Verification
- skate-core: `ai_record` (steering gate), `ai_controller` (respawn node), `path_follow` (lost radius / lane edges /
  speed gate / node advance / reattach / nearest node); living_world 290 passed.
- Muted DownTown runs (`.local/poc-tests/npcsim.sh`, radius 150 m, up to 3 sims, player idle):

| build | length | bails | hitches over 100 ms (excl. the two load frames) |
|---|---|---|---|
| before (8489da5e) | 150 s | 17 | 1 |
| steering gate | 150 s | 4 (2 double counts) | 1 |
| + respawn / teleport fixes, retail respawn node | 150 s | 2 | 2 |
| + lost hand-over | 300 s | 4 | 9 |
| + projected point | 300 s | 4 | 15 |
| + line controller re-boards (skaters ride again instead of standing) | 300 s | 55 | 16 |
| + line-end retry, switch reset, airborne gate, ground steer | 300 s | 72 (70 at two line ends) | 14 |
| + line-end fallback, retry on the last node | 300 s | 19 | 12 |
| + air steer (A/B, 240 s, radius 150 / 110) | 240 s | 48 / 33 on, 19 / 24 off | 7 / 4 on, 7 / 8 off |

The long frames left (100 to 165 ms) show no physics steps and nothing in the instrumented stages or the living
world systems: not the NPCs; open.

### Open
- `pc+922` (on-board steering) comes from an undecoded skater field (`[[rec]+32]+320`, `sub_824660A0`); ours stays
  set. A's second toggle writer `sub_8246FFF8` (node flag 0x10 while state 500) is not ported.
- On the board retail projects the board rigid body's position (SkateboardReckoning +144); ours the deck part (b116,
  field not mapped yet). `pc+808`'s scale `[0x8303EE08]` (taken as 1/dt = 60). `+1904` bit `0x04000000` is a scripted
  AI flag (UnpinAI / ClearBehavior), never set for ambient skaters; `G+608` and motion state 104 not read.
- The 4.5 m lost test after a line-end re-pick; the node-change handler `sub_8246BEE0`.

## NPC Skater Physics: bails right after a placement (2026-10-15)

**Problem.** Simulated NPC skaters wiped out about 0.45 s after every placement (switch to physics and every respawn), so
the user rarely saw one ride past (user: "the skaters seemed to crash as soon as I saw them").

**Evidence.**
- New log `WIPEOUT_REASONS` (`physics/player_state/selection.rs`): placement bails come from PhysicsGround with ground
  request 3 (deck angle error) and 18 (squash). They fire when the 0.4 s teleport wipeout cooldown runs out, so the bad
  state is there from the first tick. A review of 225 bails in 13 user sessions found the same in older builds at
  0.2 to 0.3 s, matching their cooldown (`.local/research/npc-sim-bail-log-review.md`).
- New debug log `NPC_SKATER_STEER` (deck, line and velocity yaw per tick after a placement): on ticks 3 to 5 the deck
  tumbles (up y 0.82 to -0.26), then the skater is left 50 to 60 deg off its line at 3 m/s and takes off crooked
  (`NPC_SKATER_TAKEOFF char.deck 0.72`, `deck.travel -0.41`). Placement frames themselves are upright
  (`NPC_SKATER_PLACE_FRAME` up y 0.99), and the steer sign is right (flipped, it diverged faster).
- Every placement ran two resets: `SkaterRuntime::travel` queues the host teleport with the spawn push and the actor
  reply, which resets again one tick later and snaps the rolling board back (two `player teleport spawn` lines per
  placement). Retail places once (`825926F8`) and runs its hold branch (`824701F8`: no steer while the placed skater
  settles) (`.local/research/npc-sim-placement-bails.md`, code reading, not traced).

**Change.**
- `place` uses one reset, the host teleport with the spawn push (`SkaterRuntime::teleport_with_velocity`).
- A hold after each placement: no ground steer and no trick dispatch for `npc_simulated.placement_hold_seconds`
  (default 0.5 s; mod key, 0..=10).
- Crash fix (same day): `NpcSim::respawn_node` keeps its line; a walk-back hand back could chain the cursor to the next
  line during the respawn wait, and the old line's node index ran past the new line (`npc_avoid.rs:204`, index 103 of 70).

**Verification.** Muted 90 to 150 s runs at the PCU Library (`.local/poc-tests/npcsim.sh`, `MAP=University
TELEPORT=dist_university_library`, radius 150 m):

| placement | bails | within 1 s of a placement |
|---|---|---|
| two resets (before) | 16 in 90 s (21 placements) | 4 |
| actor reply only | 9 in 90 s (15) | 4 |
| host teleport only | 6 in 90 s (10) | 1 |
| host teleport + 0.5 s hold (shipped) | 8 in 150 s (17) | 0 |
| actor reply + 0.5 s hold | 12 in 150 s (17) | 4 |

**Second pass (same day): one respawn path, and a hold with no record target.**
- Every respawn wiped out again within 0.2 s (8 of 8 in a muted run). Two respawn systems ran per NPC: our 5 s timer
  placed it with a teleport while its wipeout was still running, and the skater's own wipeout-end respawn (the player's
  checkpoint respawn, `physics/respawn.rs` `request`) later threw it to an old checkpoint (41 m off its line, then a
  walk-back). Now the bail sets the chosen node as the skater's respawn destination
  (`SkaterRuntime::set_respawn_destination`); the wipeout-end teleport places it there through the actor reply (one reset,
  like the player); `npc_sim` only follows with the line state. A timer 10 s past the respawn delay stays as a logged
  fallback.
- During the hold the record carries no target (retail's hold branch: no steering), so the record no longer pulls the
  body while it settles (pose displacement, request 1, fired at 0.45 s before).
- Muted PCU runs, 180 s each (real wipeouts counted from `WIPEOUT_REASONS`, which include ones the old bail log hid):

| | wipeouts | checkpoint teleports | walk-backs | placements |
|---|---|---|---|---|
| host teleport + hold (first pass) | 29 | 14 | 12 | 20 |
| one respawn path | 15 | 2 | 2 | 16 |
| + no record target during the hold | 4 | 1 | 1 | 8 |

**Retail riding mode (recomp trace, 2026-10-15).** Hook `AIPC922` (category `aiskater`, `824660A0`, research-hooks
branch, uncommitted): the path controller's `pc+922` (the hips ground test, which keeps the record's physics-riding bit
25) is 0 only on a controller's first sample and at its end, 1 for its whole ride (24 controllers, 360 samples,
`.local/research/aipc922/`). Retail's ambient AI skaters ride with full physics, like ours; FollowPath (105) is not their
riding mode. `824660A0` writes `pc+922` every call (not a one-way latch).

**Third pass (same day, after the user's play-tests).**
- Vanishing: the user saw skaters "vanish instantly, board and all, never come back" at the switch to physics. The
  render pose of a simulated skater is relative to its animation root (joints within ~2 m of 0, new log
  `NPC_SKATER_DRAWN`), but the draw set the puppet root to the world origin, so every simulated skater was drawn at the
  map origin. The puppet root now sits at the animation root (`npc_skaters.rs`, `NpcSim::animation_root`).
- Placement: one actor reply (retail `825926F8`, `SkaterRuntime::place_by_reply`) for the switch from replay too; the
  host teleport alone moved the board but not the animation root.
- Placement guard: the reply lands a tick or two later; the guard lifted after one tick, so the first projection used
  the pooled body (71 m off the line) and the skater was "lost" and walked off within 0.1 s (user: "they often bail and
  then run off to a new line"). It now lifts when the placement count changes (logged 30-tick limit).
- Ghost proxy: `push_proxies` built collision solids from every NPC's replay sample, simulated ones included, so a
  simulated skater rode with an invisible solid copy of itself on its line (contact bails, flings) and the player hit that
  copy. Simulated NPCs get no replay proxy now. Open: a proxy from the simulated body for the player to hit (it must not
  collide with its own skater).

**Open.**
- NOT RETAIL YET: retail places with the actor reply only and pushes from the hold branch; our reply-only variant
  bailed more. Needs a recomp trace of the branch `824701F8` takes after `8246E558` (pc+888 / pc+924 on the first ticks).
- Remaining bails are mostly in the air (KnownAir / PhysicsAir request 1, pose displacement) and spread over lines and
  nodes; walk-backs stood still for ~10 s before the stuck monitor wiped them out (new log `NPC_WALK_PROGRESS`).
- Logs added: `WIPEOUT_REASONS`, `GRIND_WIPEOUT`, `PED_DESPAWN at= focus= distance=`, `NPC_AVOID` (bad cursor),
  `NPC_SKATER_PLACE_FRAME`, `NPC_SKATER_STEER` (debug), `NPC_WALK_PROGRESS`.

## Simulated skater bodies share one animation source (load time, 2026-10-16)
- **Problem:** each simulated skater body read and decoded the animation banks from disk (`NPC_SKATER_SIM_LOAD`
  ~222 ms each in the 2026-10-16 sessions; the pool loads 3 at world start, ~670 ms inside the load hitch).
- **Change:** `NpcSimPool::source` keeps the first body's `AnimationSource`; later bodies are built on it
  (`GamePhysics::load_skater_in_context(.., source)`, `SkaterRuntime::load_for_world` also takes the source's banks
  for the metadata). `SkaterAnimation::load` is `from_source` over a fresh `AnimationSource::load`, the source is
  immutable (no interior mutability in `animation_pose.rs`), so a shared body is the same runtime.
- **Verification (optimisation rule: identical behaviour first):** data-gated
  `a_body_on_a_shared_animation_source_rides_like_a_fresh_load` (DownTown): a fresh and a shared body ride the same
  push for 240 ticks, deck transform, skeleton root and physical state equal every tick (bit for bit, `Debug`
  strings); load 161-166 ms from disk vs 64-66 ms on the shared source (2 runs). The bump test still gives NPC wipeout
  tick 26 and player tick 36. The log line now says `(shared animation source: true/false)`.
- **Open:** the remaining ~65 ms (collections, skeleton, controls and camera loads); the first body could use the
  player's source too (left out: a mod could change the player's clips).

## Simulated skaters send the full audio record (2026-10-16)
- **Problem (research b161):** both NPC tiers sent a cut-down audio state (`lite_state` through
  `AudioState::rolling`, about 16 of ~80 fields), so a simulated skater's bails, pushes, plants, pop strength, deck and
  body impacts, skids and seams never sounded, and the bail grunt (armed on the record's bail) never fired.
- **Retail [code]:** the bridge `sub_824B0DA8` fills an AI skater's record from its skater entry the same way as the
  player's; the player's sound objects then run on it (our `skate-audio/src/world/skaters.rs` already matches that).
- **Change:** after each simulated step `npc_sim::run_steps` swaps the NPC's context into its worker and calls
  `skate_events::skater_audio_state` (the hook the audio port left for this) with the NPC's runtime and its own
  `SkaterAudioMemory`; `NpcSim::audio_state` holds the record and `npc_skaters` sends it (the lite state only before
  the first step after a placement). A placement resets the memory.
- **Verification:** data-gated `a_simulated_npc_skaters_audio_record_is_its_own` (DownTown: the record's board is the
  NPC's deck, 4 wheels down, not the player's board); skate-game tests pass (known failure only). Not listened to yet.
- **Open (b161):** the replay tier still sends the lite state (its lines record no contacts); the engine never sets
  the reaction bits (slam, missed trick, positive, chase, own crash: entry +388 / +392 / +396, writer not found);
  the loose-board state is never sent for NPCs; the 30 m gate's position.
