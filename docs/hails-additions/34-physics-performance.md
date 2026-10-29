# Physics performance: hardware FMA and parallel NPC skater steps (2026-10-15)

## Problem

With NPC Skater Physics on, the game hitched: about 110 frames a minute took 9 to 13 ms against a 3.7 ms median (user,
2026-10-15: "there are lots of hitches and fps drops during the session"). Every frame that runs a 60 Hz physics tick
cost about 5 ms more than the frames between ticks. Retail ran a player and three AI skaters at full physics on an Xbox
360, so the cost pointed at waste in our code, not at the amount of work.

## Root cause

A CPU profile (Windows Performance Recorder, muted 60 s run at the PCU Library, top functions kept in
`.local/research/profile-2026-10-15/top-functions.txt`) had `fmaf` at the top of our own code: the C runtime's
**software** fused multiply-add. The physics ports use `f32::mul_add` throughout to match the console's `fmadds` bit
for bit, and the build targeted baseline x86-64 without the FMA instruction set, so every `mul_add` was a function call
into a software routine. The solver kernels that call it (`apply_impulse_changes`, `drive::solve`, `joint::solve`)
came next.

Two smaller findings along the way:
- The per-tick sync of an NPC worker (below) deep-copied the prop collision layer's template triangles, about 0.5 ms
  per NPC per tick; the instance list is shared behind an `Arc` now.
- The mod bridge's per-frame JSON observation showed in the profile with the four dev mods loaded; separate todo
  `.claude/todo/mod-json-cost.md`.

## Change

1. **Hardware FMA** (`.cargo/config.toml`, and `+fma` next to `+crt-static` in `scripts/Build-Release.ps1`,
   `scripts/build-multiplayer-test.ps1`, `tools/build_verified_windows.ps1`, whose `RUSTFLAGS` variable overrides the
   config file): `-C target-feature=+fma`. Hardware FMA and `fmaf` are both exactly rounded, so results are
   identical; Rust never fuses `a * b + c` by itself, so no other arithmetic changes. The game needs an x86-64 CPU with
   FMA3 (Intel Haswell / AMD Piledriver, 2013, or newer); `main` checks at start and shows a message instead of an
   illegal-instruction crash.
2. **Parallel NPC skater steps** (`living_world::npc_sim::run_steps`): each simulated NPC's physics step runs on its
   own worker (`GamePhysics::npc_worker`) on the compute pool. The collision world, the prop world, the dynamic props,
   the grab scene and the data are shared behind `Arc`s (`physics/world_handle.rs`: the world plus per-worker query
   scratch, `BoardWorld::query_primitives_in` in skate-core); settings and proxies are copied per tick
   (`sync_npc_worker`); workers are released after the phase so the player's step can change the shared parts.
3. **Order fixes found on the way** (needed for the parallel step, and real bugs in the serial one): the per-skater
   collision settings (wheel / truck / deck materials, truck collisions, deck child enables) were not swapped with an
   NPC's context, so an NPC's slide or bail materials leaked into the next NPC and the player's board
   (`SkaterSettings`); and each NPC solved against proxy bodies the previous NPC's solve had moved (now all against the
   post-player state). The grab scene is refreshed once per NPC phase (each NPC rebuilt the same one), and an NPC skips
   the prop carry (it returns before touching the props with nothing held and no grab).

## Verification

- `physics::skater_context_tests::npc_skaters_stepped_in_parallel_match_the_serial_steps_bit_for_bit` (data-gated):
  two NPCs and the player, 240 ticks, every deck's position and velocity bit-equal between the serial order and the
  parallel workers. `the_player_is_bit_identical_with_a_simulated_npc_skater_in_the_same_world` still passes.
- skate-game physics + living-world suites (258 passed), skate-core board world (23 passed).
- Muted runs at the PCU Library (`.local/poc-tests/npcsim.sh`, `MAP=University TELEPORT=dist_university_library`,
  radius 150 m, four dev mods), new `FRAME_HITCH` phases `solve_queries / solve_contacts / solve_solver / solve_rest /
  solve_npc` and `NPC_SIM_STEPS`:

| | morning (serial) | parallel, software FMA | parallel + hardware FMA |
|---|---|---|---|
| hitch frames per minute | 110 | 111 | 80 |
| typical hitch frame | 9.0 ms | 9.8 ms | 6.5 ms |
| median frame | 3.7 ms | 3.8 ms | 3.1 ms |
| fixed physics step | 5.5 ms | 6.2 ms | 3.3 ms |
| solve (all skaters) | 2.9 ms | 3.7 ms | 1.1 ms |
| solver | 2.3 ms | 3.1 ms | 0.7 ms |

## Open

- Frame blending for simulated NPCs (their pose is drawn between the previous and current tick, like the player's
  presentation) is in the same change set; not yet seen by the user.
- Remaining per-frame costs outside physics (audio ~1 ms, mod JSON) and `tracing` span filtering in hot code.

## Code reference: physics/world_handle.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/world_handle.rs`; each comment there links its section here.

### World handle

The static collision world as the physics step sees it: the geometry shared behind an `Arc`, plus this step's own query scratch. The player's `GamePhysics` and every simulated NPC skater's worker hold a handle to the same world, so their steps can run on separate threads (a query only reads the world). Reads go through `Deref`; the few writes (map load, prop streaming, tests) need the world not to be shared at that moment.

## NPC and replay per-frame allocations, first pass (2026-10-28, research b620)
Behaviour-identical by construction, each change with a reference test or no arithmetic moved:
- `npc_skaters.rs` advance: the Mirror decider borrows `branches` / `tricks` instead of cloning the whole history each
  tick; `others` / `in_use` built once per NPC per tick, not per step (the loop only moves this NPC's own entry,
  which `others` leaves out); `npc_trick_profile` returns a `Cow` (borrowed unless a mod override applies; still read
  every tick, so overrides stay live).
- `puppet_layers`: `sequence_ends_in` replaces `ends_with(format!("+{b}"))` (test `sequence_ends_in_matches_the_format_reference`).
- `ReplayBuffer::record`: `encode_into` writes into a reused scratch, no `moved` / `mask` Vecs; a key is cloned only
  when it is new (test `encode_into_matches_reference`, 10 000 generated poses, byte equality with the old encoder).
- `replay/world.rs` `refresh_joints`: compares joint lists via iterators, collects only on a change.
Not done (b620 items 1b..1f, 2, 3d): the present_pose scratch buffers and the fakie tree cache (needs the golden
digest test first). No timings measured.
