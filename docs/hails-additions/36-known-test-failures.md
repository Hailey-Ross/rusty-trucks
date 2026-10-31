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

Two traps found on the way:

- **A dead `assets` junction** in a worktree (it pointed at a deleted install) raised the count to 100, because many tests read `assets/` directly and ignore `SKATE3_ASSET_ROOT`. Check the junction first. That run also ended once in `0xc0000409` (`STATUS_STACK_BUFFER_OVERRUN`); it did not come back after repointing, and the cause was not found.
- **A leaked process-wide flag:** `carry_direction_tests` turned `PROP_GRAB` on and left it on, so `downtown_dragged_props_rest_on_the_floor` failed whenever it ran later in the same process and passed alone. Tests now set it through `prop_grab_for_test`, which serialises them and restores it.

## Open questions

- Whether retail's on-foot ground filter really ignores the normal (`ignore_ground`, offboard threshold 0.3) was not
  checked against the retail code here; the code is upstream's port and stays as it is.
