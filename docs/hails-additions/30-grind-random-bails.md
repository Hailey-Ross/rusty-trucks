# 30. Grinds: random bails at some rails (investigation, not fixed yet)

Branch `fix/grind-random-bails` (from `main` b3c9679). Status: **cause chain found and reproduced headlessly;
the rule that differs from retail is not identified yet, so no gameplay code is changed.** This document holds
the evidence so the next step starts from it.

## Problem

The user: "some grinding locations will cause you to bail randomly. its an issue in main that needs fixed as
well" and "I know for sure that the rails at the PCU Library spawn do. all three or four sets of them"
(University, PCU Library spawn). It happens on upstream `main` too.

## Evidence from the user's sessions

The audio state log (`state_*.tsv`, one row per 60 Hz step) of the session of 2026-10-03 11:53 has two grind
bails at the rails next to the PCU Library spawn (spawn locator `Z_UN_TrainingExterior`, (308.7, 74.0, -439.0)):

| Frame | Grind | Speed | Where | States |
|---|---|---|---|---|
| 15243 | boardslide (400), grind tag 16 | 8.2 m/s | bottom end of the stair handrail, (264.5, 73.1, -426.3) | 400 -> 500 -> 300 |
| 16008 | boardslide (400), grind tag 16 | 7.3 m/s | flat rail at its 90 degree corner, (263.1, 74.9, -430.1) | 400 -> 500 -> 300 |

400 is `GrindBoardslide`, 500 `BipedGround` (the grind state selector takes it when the run-out animation
attribute `OB_Traj` sets processed bit 2476.15), 300 `WipeoutGround`.

## Reproduction (headless)

`crates/skate-game/src/tests/grind_bail.rs` (ignored, needs the private assets and the converted University
map) lists the grind splines near the library stairs from the map's grind provider, drops the skater onto one
moving along it at 8 m/s and traces the physical state:

```
set SKATE3_ASSET_ROOT=<repo>\assets
set SKATE3_GRIND_MAP=<repo>\data\installations\<id>\maps\University.skate
set SKATE3_GRIND_RAIL=26  & set SKATE3_GRIND_REVERSE=1 & set SKATE3_GRIND_SLIDE=1 & set SKATE3_GRIND_LEAD=3
cargo test --locked -p skate-game --release --bin skate3rust -- --ignored --nocapture grind_bail_trace
```

| Spline (index in the listing, owner) | Approach | Result |
|---|---|---|
| 26, `0x688`: stair handrail, (275.7, 75.1) down to (264.2, 73.0), bends down into a post at the bottom | boardslide, downhill | grinds 53 ticks, wipeout at the bottom bend (tick 101) |
| 26, `0x688` | 50-50, downhill | grinds 53 ticks, 400 -> 500 (run-out) -> 300 at the bottom bend (tick 103): the user's sequence |
| 14, `0x656`: flat rail at y 74.91, ends in a 90 degree corner into spline 28 (`0x694`) | boardslide | wipeout at the corner (tick 105) |
| 10, `0x63c`: flat straight rail, free end | boardslide | grinds the whole run (104 ticks), no bail |
| 28, `0x694`: the flat rail after the corner, ends in a second 90 degree corner into spline 10 | boardslide | grinds 42 ticks, wipeout at that corner (tick 92) |
| 26, `0x688`, uphill | boardslide | grinds the whole run (104 ticks), no bail |
| stair edges (21 splines, 7 to 8 m each, `SKATE3_GRIND_LEAD=1.5`) | boardslide | 17 grind 85 to 93 ticks without a bail; spline 23 (`0x677`) bails after 49 ticks (not analysed); 4 never reach a grind (the drop misses the edge) |

So the bails are reproducible and not random: they happen where the rail bends (the handrail's bottom bend, a
corner). A straight rail does not bail.

## Cause chain (measured with temporary traces, removed again)

All three bails are wipeout request **2** from grind Post: the board's closing velocity is above the limit.

1. Grind Post `82D43098` [code] tail-calls `82D90898` [code] (ported in
   `crates/skate-core/src/physics/grind_forces/post.rs`). Its closing check `82D90AB8` [code] requests reason 2
   when the board is in contact (`+868`) and the horizontal part of the closing velocity `+656` in animation
   space is above `xz_acceleration_204` (6.0 [data], physics_wipeout) or its vertical part above 100.
   Compared instruction by instruction with the port: same.
2. The closing velocity `+656` is `normal * dot(normal, old part velocity)` of the board contact report with
   the largest closing speed (`82C07ED0..7F2C` [code], `board_ground.rs`). Only solved rows with a positive
   normal impulse are reported (`82AE1668..168C` [code], `board_reports.rs`).
3. Handrail bottom (spline 26), tick 99: the **deck** gets a contact with a 5 cm triangle of the post below the
   rail's bend, (264.33, 72.92, -426.17), face normal (0.97, -0.24, -0.02), flags `0xF71` (one-sided, edge
   cosines, edges 0 and 1 convex, all three vertices disabled), edge cosines (0.923, 0.9997, 0.961).
   - The deck is 17 cm above that triangle (`separation` 0.168), but the predictive limit along the triangle's
     face normal is 0.178 (`world_separation_limit`: approach `-n . v / 60` = 0.128, plus padding 0.05), so the
     pair is kept.
   - The raw contact normal is mostly vertical (-0.35, -0.92, 0.15). The triangle fixup `82AD3130` [code]
     classifies it as a disabled vertex and takes the TU3 recovery in `82AD2E00` [code] (one convex and one
     non-convex edge with cosine 0.961 <= 0.97): the normal is bent perpendicular to the convex edge, which is
     nearly vertical, so the bent normal is horizontal, (0.956, -0.258, -0.142) toward the deck. `82AD2BF0`
     [code] reprojects the contact points along it: 2 cm apart.
   - The solver applies an impulse, the row is reported, and the closing speed is 7.95 m/s (the whole slide
     speed): reason 2.
   - The vertex argument mapping (edges `+64/+96`, cosines, convex bits 0x20/0x80) and the recovery's edge and
     cosine choice were compared with the TU3 code: same as the port.
4. Flat rail corner (spline 14), tick 104: the deck reaches the cross tube of spline 28 at the corner. The deck
   rides with its centre at the spline height (the spline is the rail's top line; deck centre 74.92, spline
   74.915), so the cross tube's upper facets meet the deck front: unbent contacts at 6 cm with normals
   (0.83, 0.39, -0.39), closing speed 6.30 m/s > 6.0: reason 2.
5. One request of reason 2 with an upright animation is a run-out (`82DB9188` [code], `requests_runout`), so the
   50-50 goes to BipedGround first; BipedGround Post then requests 31 or 32 (stumbling, `ground_lifecycle/post.rs`)
   and the skater wipes out.

## What is not known yet (the actual root cause)

Every function inspected along the chain matches the TU3 code. Retail does not bail on these rails (the user),
so retail must differ before the chain starts. Candidates, in the order to check:

1. **Linked splines at corners.** The grind provider keeps the native link bounds (`grind_world` tests
   `native_payload_and_inclusive_link_bounds_survive_conversion`), but `grind_contact.rs` says "linked/adjacent
   primitive arbitration belongs to the caller". If retail hands the grind over to the linked spline (and turns
   the board) before the deck reaches the cross tube, the corner bail cannot happen in retail. Check the
   manager's primitive change at a link in the TU3 code (`82D89150` and its callers) and with a recomp trace of a
   boardslide around a corner.
2. **Board ride height during a grind.** If retail holds the deck a few centimetres higher (for example the
   board target is offset from the spline by the truck or deck height), both bails disappear: the corner tube
   is passed over, and the post triangle leaves the predictive range (0.168 against a limit of 0.178, a 1 cm
   margin). Compare the deck height over the spline in a recomp trace of a boardslide.
3. **Predictive contacts against grind rails.** Whether retail runs the deck's predictive world query against
   the rail mesh while grinding at all (collision group 4 is set by `grind_materials.rs`; the stock exclusion
   table `82767D60` only has (7,12) and (16,12)).

No threshold was changed: the 6.0 m/s limit and the 0.97 bend cosine are retail values, and a fudge would
hide real impacts.

## Change

- New diagnostic test `crates/skate-game/src/tests/grind_bail.rs` (registered in `crates/skate-game/src/physics.rs`
  as `grind_bail_tests`), ignored and data-gated like `water_drop`. It lists the splines near a point
  (`SKATE3_GRIND_NEAR`), runs one approach per spline, prints the state per tick and the first wipeout tick,
  and dumps the collision triangles around a point (`SKATE3_GRIND_TRIS=x,y,z,r`) with their edge flags.
- No gameplay code changed.

## Verification

- The test reproduces the user's two locations (and a third approach, the 50-50) and shows a straight rail
  without a bail (table above).
- Existing tests, same before and after (no gameplay code changed): `cargo test -p skate-game --release --bin
  skate3rust -- grind` 52 passed, 4 ignored; `cargo test -p skate-core --release -- grind` 60 passed.
- The repro runs again without the temporary traces: handrail boardslide wipeout tick 101 (54 grind ticks),
  handrail 50-50 tick 103, corner tick 105, straight rail no bail.

## Open questions

- Which of the three candidates is retail's rule (needs a TU3 read of the manager's link handling and a recomp
  trace of the deck height while grinding; the recomp is reference, not a console measurement).
- Stair-edge spline 23 (`0x677`) bails after 49 grind ticks; not analysed yet (which request, where).

## Moddability

Nothing to expose yet. When the rule is found, its values (ride height, link hand-over) come from the
setup data as retail defaults, like the existing grind settings.
