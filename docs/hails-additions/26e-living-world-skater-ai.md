# 26e: Living world: NPC skaters: AI and simulated tier

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## NPC skater steering: the AI board path (M5 port, 2026-10-08)

**Problem.** Retail's NPC skaters are full physics skaters. Their AI (the PathController) writes a steering
record every tick, and the physics nudges the board toward it. Our engine had the record's path into the physics
input (`ProcessedPhysicsInput.external_physics_1616`) but not the step that acts on it, so a physics-bodied NPC
could not be steered. This change ports that step; NPCs still use the replay tier until they get a physics body
(the simulated tier, next).

**What retail does** [code, TU3; recomp disassembly as reference]:
- The record goes from the AIPhysicsInput component into the skater's physics state (`82593640`, +10512, "use
  external physics" +10688) and from there into the physics frame (`82DB4048` -> +1616, flags +1776; already
  ported as `publish_external_physics`).
- `UpdatePostPhysics 82D387A8` of `PHYSICS_STATE_PHYSICS_GROUND` (100) and `PHYSICS_STATE_SLIDE_GROUND` (101) runs
  the ground wipeout check `82D8F9E0`, then, while record flag bit 31 is set, the board path `82C05EC0`:
  - bit 30, position `82C056B0`: `d = (target - deck) * gain`; the deck (part 6 alone, `82D9C8C8` -> `82BD4318`,
    waking a frozen body with `82ADF7B8`) moves by `min(|d|, max_step)` along `d` when `|d| > 1e-6`
    (`0x830BD350`, initialised to 1e-6 by `82F826F8`);
  - bit 28, velocity `82C05868`: the deck body's linear velocity approaches the target velocity the same way;
  - bit 29, facing `82C05988`: the target forward in the ground frame (`controller+292` -> `+752`), y dropped,
    normalised, its signed angle from +Z about +Y (`8296EC98`) wrapped to [-pi, pi], times the gain, clamped to the
    maximum (degrees, `0x8206D110` = pi/180); the deck rotation becomes `D * F^T * Ry * F` and the whole board is set
    (`82C0B2C8`).
- Gains: the `physics_ai` vault record (`*(*(0x830CFDA4)+272)+4`, holder `8289D5C8` -> `8289D2E8`, class
  `527C93F55CFC663D`), `default`: velocity max change 0.2 m/s and gain 0.5 (+0 / +4), position max step 0.02 m and
  gain 1 (+8 / +12), facing max 2 degrees and gain 0.5 (+16 / +20) per tick. An `unstreamed` record (10 m, 1000 m/s,
  0.2) also exists; what selects it is not found yet.
- `PHYSICS_STATE_FOLLOW_PATH` (105) is a separate kinematic state (`82D43118` -> `82C05D78`: velocity from the
  position error, 40 m/s cap, 5 m/s change per tick); not ported yet. Jumps along the recorded trajectory:
  `82D682E8` / `82D67B50` / `82D67A00`, not ported yet.

**Change.**
- `skate_core::riding::grounded::state::board_path`: `PhysicsAiTuning`, `SteerTarget`, `approach`,
  `facing_step`, `rotate_about_ground_up`, `update_board_path` (retail order position, velocity, facing).
- `BoardRuntime::set_single_part_transform` (`82D9C8C8`: one part alone, frozen parts woken).
- `skate-game` `physics/board_path.rs`: runs after the ground wipeout check in PhysicsGround / SlideGround when the
  record's bit 31 is set; `physics_ai` loaded from the setup collections into `PhysicsSettings`
  (`SKATE_PHYSICS_AI` warning and the `default` record's values if the class is missing).
- The player's record never has bit 31 (its "use external physics" byte is 0), so the player's physics is
  unchanged.

**Files.** `crates/skate-core/src/riding/grounded/state/{board_path.rs, board_path_tests.rs, mod.rs}`,
`crates/skate-core/src/physics/board_runtime.rs`, `crates/skate-game/src/physics/{board_path.rs,
board_path_tests.rs, settings.rs}`, `crates/skate-game/src/physics.rs`.

**Verification.** skate-core `board_path` 4 tests (position and velocity steps and caps, the 1e-6 gate, facing
sign / wrap / clamp / gain / ground frame, the rotation about the ground up keeping the position, retail order and
per-flag gating). Data-gated skate-game test on DownTown (`SKATE3_ASSET_ROOT`, `SKATE3_MAP`): `physics_ai` loads from
the setup collections and equals the `default` record; 240 riding ticks with the player's record never steering;
a steering record on the riding board moves the deck 2 cm and its velocity 0.2 m/s toward the target, the wheels
untouched; without bit 31 nothing runs.

**Open questions.** The `unstreamed` switch; `physics_ai` +24 (0.95); FOLLOW_PATH and the recorded-trajectory jump.

## NPC skater trick choice (M5 port, 2026-10-08)

**Problem.** The replay tier always did the trick recorded on the line. Retail's ambient NPC skaters do not: in
their default behaviour an ollie or flip slot is re-picked from the character's AI profile, so the same line shows
different flips from different skaters and from one pass to the next. Not play-tested yet.

**What retail does** [code, TU3; read in the recomp disassembly, reference only]:
- The PathController's dispatcher `sub_8246A2E0` runs on every start-trick node (event 1) the skater passes. The
  trick's category comes from the static trick table `0x820862A8` (+16). Ollie / nollie (1) and flip (2) slots go to
  `sub_8246A080`; every other category (grinds, grabs, manuals, slides, reverts, plants) does the recorded trick.
- A higher chain level (heelflip2 / 3 / 4, kickflip2 / 3 / 4 and the nollie forms: table +8 base is another trick and
  not the ollie 128) takes no action: the base trick already running covers it. The late flips have the ollie as
  base and are not chain levels.
- Behaviour mode (behaviour context +60; `sub_824733A0` creates the ambient behaviour with mode 1):
  - 0: the recorded trick. `sub_82469FA8` walks the next start nodes within 120 frames whose trick's previous level
    (+4) is the current id, but it assigns that previous level each time, which is the id it already holds, so it
    always returns the recorded trick (an earlier research note read this as "takes the highest level"; the code
    does not).
  - 1 (ambient default): when the gate passes, a weighted pick from the profile, else the recorded trick.
  - 2: a scripted list (not ported, see Open questions). 3 and up: no ollie / flip.
- Gate `sub_82469C60`: walk forward from the slot summing each node's frames (+0x24) until 300. A start node first:
  pass only when it is an ollie / flip (jump table `0x82469D5C`: categories 1 and 2) whose previous level is the
  recorded trick, else fail. A landing (the airborne flag seen, then a node without it): fail when the next node is a
  start node, else pass only when more than 50 frames passed (the landing node's frames included; `subfc` / `eqv` /
  `addze` = signed `frames > 50`). The window end or the line end passes.
- Pick `sub_82469A28` -> `sub_8245FE58`: the nollie table (runtime profile +8) when the recorded trick's name
  starts with `n` / `N` (`sub_82469048`), else the regular table (+0). `u = rand32 / 2^32`; walk the 8-byte entries
  (weight, trick) adding weights (normalised to sum 1 at load, `sub_824720C8`) and take the first with `u < sum`,
  else entry 0. `sub_82469A28` also keeps the recorded trick when the trick's attribute record has no family entries
  in the per-category list; those records are not decoded, every recorded ollie / flip is taken to have them
  [inferred].

**Data.** The profile tables are already in the `livingworld` export (`skater_profiles.json`,
`ai_skater_profiles.*.fields`: `Hash_E580B6284639E03F` regular, `Hash_BB901D68361E9833` nollie, inheritance
resolved by `tools/asset_pipeline/living_world_skaters.py`) with each character's `aiprofile`; no setup re-run is
needed. The trick table's chain links (+4 previous level, +8 base) are 18 numeric rows in
`skate_core::scoring::catalog::LINKS`, read from the TU3 image and checked against the catalog (+0 = index and the
+12 / +16 columns match all 332 rows).

**Change.**
- `skate_core::living_world::npc_tricks`: `TrickMode` (recorded / profile / none), `TrickParams` (gate window 300,
  more than 50 frames), `TrickProfile`, `gate`, `pick_weighted`, `choose`.
- The line cursor asks `choose` at every start-trick node (`BranchContext.tricks`), keeps the running trick at a
  chain level, and emits `CursorEvent::Trick(TrickRecord {frame, line, node, recorded, chosen})`. `Decider::Mirror`
  takes the trick records next to the branch records, so a client (and the render look-ahead) applies the host's
  choices and never decides.
- Determinism choice (not retail): retail draws `rand32` from the skater's random source in tick order. Ours is
  `derive(npc seed, [line, node, frame])`: the same distribution, but a pure function of the NPC's seed and the slot.
- `skate-data`: `skater_trick_profiles` (tables by profile name, profile by character; `default` when a character
  has none).
- `skate-game`: each NPC's tables are its `aiprofile`'s, with mod tables applied; `NPC_SKATER_TRICK #id character
  node frame recorded chosen mode` in the log for every slot, `NpcSkaterEvent::Trick` for engine systems and the
  planned `sdk.living_world` events, `NpcReplay.tricks` holds the records a host would send. The puppet reads
  the cursor's trick (`resolve_trick_anim`), so the chosen flip's clip plays.
- Mod surface (`sdk.world.set_tuning("living_world", ...)`): `npc_tricks {mode, gate_window, min_air_frames}` and
  `skater_trick_profiles {[character key or profile name] = {regular, nollie}}` (lists of `{trick, weight}`; a key
  wins over a profile name per table; an absent table keeps the disc's). Cleared when the mod stops.

**Files.** `crates/skate-core/src/living_world/{npc_tricks.rs, npc_tricks_tests.rs, replay.rs}`,
`crates/skate-core/src/scoring/catalog.rs`, `crates/skate-data/src/living_world.rs`,
`crates/skate-game/src/living_world/{mod.rs, npc_skaters.rs, npc_tests.rs}`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/{world_tuning.rs, vm.rs, api.lua}`,
`sdk/skate.lua`.

**Verification.**
- skate-core `living_world::npc_tricks` 9 tests: the pick at the cumulative boundaries and the entry-0 fallback,
  the gate at 40 / 50 / 60 frames and with a mod window, a start node after the landing, the 300-frame window and
  the line end, chain continuation, nollie table only for `n*` tricks, other categories and modes, catalog links,
  the cursor emitting one record per slot (none at the chain level) and a mirrored client showing the same tricks,
  and the spread over 4,000 seeds (about 3 in 4 for a 1 : 3 table).
- skate-data: a unit test on synthetic JSON; data-gated on the user's export: all 193 profiles, every table entry is
  an ollie / flip (category 1 or 2) with a weight of at least 0, every pool character resolves to a non-empty table.
- skate-game: `living_world_npc_skaters_pick_tricks_from_their_profile_and_clients_mirror_them` (an ollie slot
  with 80 frames of air on every fixture line: picks only from the profile, both entries come up, a client cursor
  rebuilt from the records shows the host's trick; mode `recorded` keeps the ollie; the reset restores retail) and
  `npc_skater_trick_choice_set_merge_and_reset` (domain read, first writer wins, key over profile, invalid mode or
  trick id rejected, reset). skate-mods `world_tuning_commands_deserialize_and_validate` gains 4 cases.
- In game (muted DownTown run, 2026-10-08): 12 slots in the first NPC seconds; `360popshuvit` -> `kickflip` and
  `ollie` -> `fs360popshuvit` re-picked; grinds, grabs, manuals and short-air ollies kept the recorded trick.
- Full workspace run (`--lib --bins --tests`): only the 4 known upstream failures.

**Open questions.**
1. Mode 2 (scripted list, `sub_82469E10`) and who sets modes 0 / 2 (challenge or scripted AI skaters).
2. The trick attribute records' family entries (`sub_8245F120`, list at `*(0x830CFE64) + (category + 583) * 16`).
3. Profile byte +51 (grab / fingerflip / boneless allowed; one of the two unnamed bools) and the gesture-start
   percent (+36): only matter once the simulated tier sends ActionGraph signals.
4. Skater vfunc +36: whether retail's draw is a per-skater stream or the global generator.

## Simulated NPC skaters: per-skater physics context (M7 step 1, 2026-10-08)

**Problem.** Retail runs every ambient NPC skater as a full physics skater (the same physics states and board as
the player, steered by its AI record; see "NPC skater steering"). Our physics (`GamePhysics`) held exactly one
skater: its board, riding outputs and clock sat next to the shared world. About 800 places read those fields, so
splitting them out would be a large change with a regression risk for the player.

**Change.** A simulated skater's own parts are a `SkaterPhysicsContext` (board, riding outputs, clock, exchange,
tick count, carry, wipeout flags). An NPC skater keeps one and swaps it into `GamePhysics` around its own tick
(`swap_skater_context`, a plain swap of those fields); the collision world, props, grind world, settings and network
proxies stay shared, so moved props and every map change reach every skater. `new_skater_context(spawn)` builds one
from the same setup collections (kept in `GamePhysics`). Only the local player steps the dynamic props
(`owns_props`); NPC skaters push props through `actor_prop_volumes` as before, so props still step once per tick.
The world's query buffers are cleared on every query (no state carried between skaters).

Not yet: spawning simulated NPC skaters from the population, their AI record and pad, drawing them from their
simulated pose, the distance switch between replay and simulated, skater-to-skater collision.

**Files.** `crates/skate-game/src/physics.rs`, `crates/skate-game/src/physics/skater_context_tests.rs`.

**Verification.** Data-gated `the_player_is_bit_identical_with_a_simulated_npc_skater_in_the_same_world` on
DownTown: 300 ticks of the player pushing off, alone and again with a second skater (own context and runtime, 6 m
to the side, same input) ticking after the player every tick: the player's deck position and velocity bits are
identical on every tick; the second skater rode more than 1 m on its own board without falling through.

## Simulated NPC skaters: the AI record drives the physics (M7 step 2, 2026-10-08)

**Change.** A simulated skater gets its AI physics record through `SkaterRuntime.ai_physics` (`None` for the
player): the animation packet publishes it like retail `82593640` (record copied with the high flag bits only,
`82592810`; "use external physics" = the AI's fresh bit, "externally controlled" set), and the existing ported
publication puts it into `ProcessedPhysicsInput.external_physics_1616`. The record is built from the recorded line
(`skate_core::living_world::ai_record`, retail `sub_8246DB50` / `sub_8246DE38`): target = the path frame
(`path_frame`, board orientation, turned on board-flipped nodes) and position along the current segment
(`LineCursor::line_target`, no drawing blends), target velocity = the node's per-frame displacement x 60 (new
`ReplayNode::step`, node `+0x0C`; measured on the DownTown export: |step| x frames = segment length, median ratio
1.000 over 47,087 segments) clamped to 99.9 m/s, the frame's Ri / At negated when the skater faces the other way,
and the flags: bit 25 = on-board steering (seeded by `8246DB50` from `pc+922`), bits 31..28 from the state bytes as
decoded. Bit 25 matters: the ported state selector sends a skater whose record steers without it to
`PHYSICS_STATE_FOLLOW_PATH` (105), which is not ported yet; with it the skater stays in `PhysicsGround` and the
board path steers it.

**Verification.** skate-core `ai_record` 2 tests (flag rules incl. bit 25, target pose and speed, facing flip,
clamp). Data-gated `a_simulated_skater_rides_a_recorded_line_from_its_ai_record` on DownTown: a skater with only the
record (neutral pad) spawned on the first ground NPC line stays in `PhysicsGround` for 300 ticks and rides more than
5 m along the line, tracking it (median under 0.5 m, max under 1 m on the default line). The player identity test
still passes. skate-core `ai_record` also tests the spawn push (scale, kind 1, the node radius).

**Spawn push (retail, ported).** The first run fell behind the recorded speed (tracking error median 9.5 m, and the
slow board hit geometry). Retail starts a fresh NPC near line speed: `sub_824701F8` (hold branch) sets every board
part's velocity (`82C04168`) to the node's per-frame displacement x 60 x 0.75 (`0x821814A0`; x 0.5 `0x8209975C` for AI
kind 1 outside motion states 19 / 20) while the skater is within 0.01 m (`0x820D71E8`; 0.05 m `0x82165A00` past node
0) of its node in the ground plane (`ai_record::spawn_push`). With it, on the first seven DownTown ground lines the
simulated skater holds the recorded line over 5 s on its own physics: median error 0.06 to 0.2 m, max under 0.6 m, at
the recorded speed (about 8 m/s), except line 5.

**Open.** Line 5: around (-147.9, 10.18, 436.2) the board's contacts climb from 22 to 92, it rides about 3 cm up and
nearly stops, then recovers (max error 3.7 m) while the recording rolls through on the ground (no airborne node):
a ground-physics or collision difference at that spot (a curb, a prop, or our wheel solve), not the AI. Then:
FOLLOW_PATH (105), the recorded-trajectory jump (`82D682E8` / `82D67A00`), spawning simulated NPCs in the game and
drawing them from their physics.

## Simulated NPC skaters in the game (M7 step 3, 2026-10-08)

**Change.** `living_world::npc_sim`: an NPC skater within 40 m of the player, on the ground and not in a trick,
becomes a simulated skater (`NpcSim`: its own board context, `SkaterRuntime`, controls and camera runtime, loaded
from the setup data at the switch); beyond 44 m (1.1 x), at the end of its line or on a physics error it goes back to
the replay tier (only while on the ground). Each tick after the cursors advance: the AI record from the cursor, the
retail spawn push while on its node, then `GamePhysics::advance_npc_skater` (the player's frame with a neutral pad,
in its own context). It is drawn from its simulated pose like the player (`render_pose`, puppet root at the origin);
its population position and audio still follow the cursor. At most 3 at once; only the authority simulates.
Engine choices (retail simulates every ambient skater and never hands over): the distance switch and the handover at
the line's speed. Off by default until play-tested: `SKATE_NPC_SIM=1`, or the mod value `npc_simulated {enabled,
radius, max}`. Log: `NPC_SKATER_SIM` on every switch and every 2 s per replay-tier NPC with the reason it waits.

**Verification.** Muted DownTown run with `SKATE_NPC_SIM=1` (40 s): andrew_reynolds switched at 39.9 m and stayed
simulated for the rest of the run (about 30 s, through its tricks) without a physics error; the fixed physics step
went from about 2.3 ms to 4.4 ms with one simulated skater. The others waited "too far" (51 to 122 m). Not seen on
screen yet and not play-tested. skate-mods validation gains 2 cases; player identity and line tests still pass.

**Open.** Its recorded ollies and tricks are not performed yet (no trajectory launch, no ActionGraph trick signals:
a simulated NPC rolls through its jumps); skater-to-skater collision; how the board and look appear when drawn from
the simulated pose; render interpolation (drawn at the 60 Hz tick).
