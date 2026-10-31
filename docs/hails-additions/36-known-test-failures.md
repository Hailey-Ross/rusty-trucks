# 35. The two known upstream test failures (2026-10-13)

Branch `fix/known-test-failures` (from `main` 2e166977). Both tests failed on upstream `main` and therefore on every
branch built on it, so every PR listed them as "known failures". Both are test fixtures that contradict the code's
own rule; no engine behaviour changes.

## Problem

- skate-core `physics::skeleton_body::collision_feedback_tests::a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`:
  `assertion failed: collision.flags.group_8 && collision.flags.has_impulse`.
- skate-game `setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`:
  `assertion failed: pipelines_acceptable(&marker, &expected, None, &root)`.

## Root cause

- **Group 8 impact:** the test runs once on the board and once on foot. On foot, `ignore_ground`
  (`collision_update.rs`) drops every contact within 0.3 m of the ground plane, whatever its normal (those are the
  feet's ground contacts); on the board it drops only contacts near the plane whose normal faces it. The test's
  contact point was the origin, on the plane, so on foot the car's contact was filtered before any force was
  recorded (flags printed: `any` set, `group_8` / `has_impulse` / `nonboard` not). The rule predates the test
  (a47da5bb); the fixture never matched it.
- **Pipeline fingerprints:** `pipelines_acceptable` reuses a group whose fingerprint changed only when that group's
  recorded outputs are on disk with their recorded sizes (`group_outputs_valid`). The fixture changed the `core`
  fingerprint but recorded outputs only for `maps`, so `core` was correctly refused. Accepting it would skip a
  setup run the new build needs. Test and function arrived together (821f523b); the fixture missed `core`.

## Change

- `crates/skate-core/src/physics/skeleton_body/tests/collision_feedback.rs`: the car meets the body at hip height
  (contact point y = 1.0), with a comment on the on-foot ground filter.
- `crates/skate-game/src/setup.rs` (tests only): `core` gets a recorded output (`private/game.json`, written by the
  test); the test now also checks the refusals: a changed group without recorded outputs, and a recorded output
  whose size no longer matches.

## Verification

- `cargo test -p skate-core --lib` and `cargo test -p skate-game --bin skate3rust` on the branch (results in the
  commit message).

## The full data-gated run (2026-10-30)

`cargo test -p skate-game -- --ignored` with `SKATE3_ASSET_ROOT` and `SKATE3_MAP` (DownTown) set: 198 passed, 37 failed of 235. None of the 37 is a failure of the branch's code as far as checked:

- 8 fail with the same messages on upstream `main` 2e166977: three `air_tests` and `stock_skater_startup` (no authored query metadata on the course terrain), three `climbing::tests` (`climbing/tests.rs:184`), `powerslide_tests` (the slide latch never starts). Not investigated further.
- 1 needs another map: `skater_dropped_onto_the_industrial_sea_floor` passes with `SKATE3_MAP` = Industrial.
- 1 needs a connected controller: `xinput_identity_of_connected_pad`.
- 27 stop at a setup check: an unset variable or export (audio state logs, recomp sessions, `SKATE3_GRIND_MAP`, `SKATE_BUDGET_MAP` and so on) or a missing local file. They were not run with those supplied.

### Fixed and re-run (2026-10-30 evening)

Same run with the map and audio inputs supplied (`SKATE3_MAP` = `<install>/maps/DownTown.skate`, `SKATE_BUDGET_MAP`, `SKATE_MAP_TEST_PATH`, `SKATE_TRANSITION_TEST_ASSETS`, `SKATE3_GRIND_MAP` = University, `SKATE3_WATER_MAP` = Industrial, the `.local` audio exports listed in doc 11): 223 passed, 12 failed of 235.

The earlier 8 upstream failures and one more:

- **Query metadata (`air_tests` x2, `stock_skater_startup`):** the tests rebuilt the world with `BoardWorld::new`, which has none. They now use `ground::with_authored_metadata` / `ground::flat_world`. The startup test's old 20 m floor was also too small for its braking run.
- **Tailwalk (`airborne_tailwalk...`):** the test pressed the trigger and the right stick on the same tick, which enters `Grabbing`. `ActionGraph_OnBoard` only reaches `BoardAdjusting.Up.TailGrab` with the stick already held, so the test now holds it first.
- **Powerslide:** the test pushed the stick forward; retail's start window is back and to the side (`riding_intentions.rs`). It also expected `Slide` unchanged, but retail's handler negates it (`*(iVar20 + 0xaac) = -*pfVar23`, 82BDA0D0).
- **`static_draw_budget`:** the test world never registered `Assets<SkyMaterial>`.
- **Climbing, an engine bug:** with clips installed the camera failed every attached tick, because `frame::advance` had reset the output snapshot and the tick was counted after the camera. `climbing::advance` now counts the tick and calls `frame::publish_output` first. No install has the clips, so players never reached this.

Climbing tests now build their clips from the stock skeleton (`install_fixture_clips`) because `private/custom/climbing.json` is written by no setup. The dropped board is also moved off the run-up: the skater ran over it and was thrown 5 m up, which was not investigated further.

The 12 still failing:

- **2 show a real bug, left failing:** `hybrid_climb...` and `climbing_with_a_dropped_board...`. After the mantle the motion graph is still in its paused jump state, so `flags_2484` bit 16 selects `BipedAir` on the first tick, the skater jumps 0.37 m and wipes out. The fixture clips could contribute, but the stale graph state does not depend on them. Fixing it needs a graph reset at the handoff, which does not exist yet.
- **1 needs its map:** `skater_dropped_onto_the_industrial_sea_floor` fails on DownTown and passes with `SKATE3_MAP` = Industrial.
- **9 need files that are not here:** `SKATE_TEST_LIBRARY` / `SKATE_TEST_ASSETS`, `SKATE_TEST_CHARACTER`, `SKATE_CAC_TEST_LIBRARY`, `SKATE_ONLINE_TEST_GLB`, `SKATE3_MOTION_INVENTORY`, `SKATE3_INPUT_RECORDING`, and `private/custom/crouch-treflip.json` (three tests). These have not run.

`xinput_identity_of_connected_pad` needs a connected controller; it passed in this run.

Two traps found on the way:

- **A dead `assets` junction** in a worktree (it pointed at a deleted install) raised the count to 100, because many tests read `assets/` directly and ignore `SKATE3_ASSET_ROOT`. Check the junction first. That run also ended once in `0xc0000409` (`STATUS_STACK_BUFFER_OVERRUN`); it did not come back after repointing, and the cause was not found.
- **A leaked process-wide flag:** `carry_direction_tests` turned `PROP_GRAB` on and left it on, so `downtown_dragged_props_rest_on_the_floor` failed whenever it ran later in the same process and passed alone. Tests now set it through `prop_grab_for_test`, which serialises them and restores it.

## Open questions

- Whether retail's on-foot ground filter really ignores the normal (`ignore_ground`, offboard threshold 0.3) was not
  checked against the retail code here; the code is upstream's port and stays as it is.
