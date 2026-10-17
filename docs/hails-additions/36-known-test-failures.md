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

## Open questions

- Whether retail's on-foot ground filter really ignores the normal (`ignore_ground`, offboard threshold 0.3) was not
  checked against the retail code here; the code is upstream's port and stays as it is.
