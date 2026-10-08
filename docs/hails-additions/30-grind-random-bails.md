# 30. Grinds: random bails at some rails (investigation, not fixed yet)

Branch `fix/grind-random-bails` (from `main` b3c9679). Status: **cause chain found and reproduced headlessly;
the rule that differs from retail is not identified yet, so no gameplay code is changed.** The second pass
(below) finds no support for candidates 2 and 3 in the static code and narrows it to the spline end
(candidate 1). This document holds the evidence so the next step starts from it.

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

## Second pass: static TU3 reads and contact reports (2026-10-08)

### Candidate 3 (rail mesh filtered for the board while grinding): not supported by the code

- The rail triangles are ordinary units of the same clustered mesh as the ground around them (University mesh
  `cSim_250_-450_high.xsf#0`, unit group 0, surface `0x90`: surface type 1, audio surface 16, the "grind tag 16"
  of the session log). Nothing in the data marks them as grind-only.
- The triangle dispatch `8277BC58` [code] skips a moving volume only by its byte `+216`, its kind 6, a (volume
  group x object group) bit table (job `+40`) and a (volume group x surface type) bit table (job `+44`; the
  stock initializer `82767D60` [code] sets only (7,12) and (16,12)). With one mesh and unit group 0 there is no
  rail-specific entry.
- The board's group is 4 both while grinding and outside: the grind material switch `82D89DC0` [code] (taken
  when 2512 is 400 or 2508 is 701) and the standard switch `82C090C0` [code] both write 4 to the assembly `+24`
  and to every part's `+92`; they differ only in the materials (`+8316/+8328/+8340` against
  `+8280/+8292/+8304`).
- Grind Enter `82D3F318` [code] (animation drive off, `16505`, feet volumes `82D91330`, deck angular drag),
  Exit `82D3F430` [code] (feet volumes, `82C091F8`, drive off, wheel spin `82D3F7D0`) and the Nonspecific
  Enter `82D42D80` [code] change no board volume enable and no query setting.
- The closing-velocity loop of `82C07D20` (`82C07E68..82C07F30` [code]) runs over every report of all seven
  parts, wheels included, with the old part velocity: the port's `board_ground.rs` matches.

So, statically, retail queries the rail mesh with all board parts while grinding, as the port does.

### Candidate 2 (ride height offset): ruled out as an offset

The boardslide leaves are forces plus an angular-only hook drive (`82C05658` [code]); none holds a height. The
deck rests on the tube: tube top 74.914 (the spline height), deck centre 74.92 = tube top + the deck's half
thickness (rounded box half extent 0.00075 + radius 0.00675). The ride height is a contact result, so a retail
difference there would have to come from the contacts, not from an authored offset.

### What the board actually hits (headless, per-tick board contact reports)

Corner, spline 14 (`0x656`), boardslide 8 m/s, default lead 1.5 m (grind 70 ticks, wipeout tick 118):

| Tick | Reports |
|---|---|
| up to 115 | deck only, normal (0, 1, 0) on the tube top (y 74.914) |
| 116 | **left front wheel**, normal (0.91, -0.04, -0.42) at (262.891, 74.867, -430.370): the leading wheel meets the side of the cross tube (spline 28, `0x694`) below its top; the deck centre is still 0.17 m before the corner and the deck edge about 5 cm before the cross spline |
| 117 | BipedGround (run-out); deck now also against the cross tube's upper facets (0.6, 0.75, -0.27) |
| 118 | WipeoutGround |

So with this approach the first impact is a real wheel collision, not a predictive ghost: in a boardslide the
wheels hang below the rail top on both sides, and at a 90 degree corner the cross tube lies across the
leading wheels' path. (The first pass, from another start position, saw the deck reach the cross tube first; either
way the board runs into the continuing geometry at the end of the spline.)

Handrail, spline 26 (`0x688`), downhill, lead 3, boardslide (wipeout tick 101): ticks 96 and 97 the front
wheels touch the rail side (normal (-0.45, 0.38, -0.81)) while the deck rides the top; tick 98 the deck reaches
the rail end where it bends into the post (deck report (-0.17, -0.29, -0.94)); tick 99 the post triangle
report (0.96, -0.26, -0.14) (the vertex-recovery normal of the first pass) and the deck's speed drops from 8.2
to 0.9 m/s in one tick; 100 BipedGround; 101 WipeoutGround.

### Conclusion of the second pass

Both bails start when the board reaches the **end of the grind spline where the geometry continues** (a cross
tube, a post). Every function checked along the physics chain matches TU3, and candidates 2 and 3 are not
supported by the code. That leaves candidate 1: what retail's grind does at the end of a spline. The admission
`82D886B8` [code] allows StayInGrind up to 90 degrees, so a perpendicular linked primitive can be taken over,
but in the port the leading wheels reach the cross tube before the deck's contact query reaches the linked
spline. Whether retail hands over (and turns the board) earlier, ends the grind before the end, or lifts the
board, was not found in this pass: the manager `82D8A828` [code], the investigator `82D875A8` [code] and
`82D89150` [code] were not re-read instruction by instruction against the port at the spline end. That read,
or a recomp trace, is the next step.

Two points that bound candidate 1 at the corner:

- The boardslide deck contact (`82D889A8` [code] admission, port `boardslide_candidate` and the deck rectangle
  query before it) intersects the grind segments with a vertical rectangle along the deck's long axis through
  its centre. In a boardslide the deck lies across the rail, so the cross spline of a 90 degree corner runs
  parallel to that rectangle and is never found: the port cannot hand over at such a corner, and the same
  query in retail cannot either. A retail hand-over would have to come from another path.
- The leading wheels hang about 5 cm below the deck (wheel centre 74.867 against the tube top 74.914), so at a
  90 degree corner of a rail at one height they cross the cross tube's line geometrically. Whether retail's
  board really slides through that corner, or also stops there, should be asked: the user's report names the
  PCU Library rails as bailing in this engine; that retail boardslides through this particular corner is not
  confirmed yet.

The handrail case is different: the deck slides off the rail end that bends down into the post, and the
contact that stops it is with a post triangle 17 cm below the deck (a predictive contact with the vertex
recovery normal of the first pass). That looks like the stronger case of an engine-side difference.

### Recomp trace plan (not run)

Hooks (research-hooks, guarded reads, category `grind`), logged only while 2508 is 400..405 or 701, plus the
two ticks after leaving:

1. `GRMGR` at the end of the manager pre-update `82D8A828`: 2508, 2512, selected owner `+1296`, primitive start
   and end `+1264/+1280`, point `+1120`, family `+1248`, entry kind `+1252`, flags `+1516`, deck frame.
2. `GRCAND` at entry and exit of `82D89150`: r3 to r8, the two truck contact primitives and the return value.
3. `BRDREP` at `82C07ED0` in the `82C07D20` report loop: part index (`+64`), normal, position and surface of
   every report.
4. `BRDPOSE` once per tick: the seven board part positions (the wheel heights against the tube top).
5. `GRPOST` at `82D90AB8`: closing velocity `+656`, its animation-space xz length and the request.

Action: following the standing rules, gameplay data comes from the user's play, not scripted skating. Verify in
one background run that the hooks fire, then ask the user for one short session: boardslide the flat PCU
Library rail into its 90 degree corner and the stair handrail off its bottom bend, three times each. Compare per
tick with the headless traces above: does retail switch owner `0x656` to `0x694` (or leave 400) before the
corner, where are the wheels relative to the tube top, and does any wheel or deck report exceed 6 m/s.

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

- Second pass: no code changed. The contact reports and the RWCM unit groups were read with temporary
  prints in the diagnostic test (removed again); the prints did not change the outcome (corner, lead 1.5:
  wipeout tick 118 with and without them; handrail boardslide, lead 3: 101 as in the first pass).

## Open questions

- Which rule retail applies at the spline end (second pass: candidates 2 and 3 are not supported by the static
  code; candidate 1 remains). Needs the instruction-level read of `82D8A828` / `82D875A8` at a spline end or
  the recomp trace planned above (the recomp is reference, not a console measurement).
- Does retail boardslide through the PCU Library flat rail's 90 degree corner, or does the board also stop
  there? (To ask the user before more research on the corner.)
- Handrail bottom: retail's contacts for the deck sliding off the bent rail end (the post triangle 17 cm below
  the deck); the BRDREP hook of the trace plan answers this.
- Stair-edge spline 23 (`0x677`) bails after 49 grind ticks; not analysed yet (which request, where).

## Moddability

Nothing to expose yet. When the rule is found, its values (ride height, link hand-over) come from the
setup data as retail defaults, like the existing grind settings.
