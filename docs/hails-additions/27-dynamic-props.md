# 27. Dynamic props (movable DMOs) from upstream PR #15

Status: cherry-picked onto `world/dynamic-props` (from `world/living-world` 5127ae6), builds and tests green on
Windows in both link configurations; not yet checked in game. Planned to ride with the living world (upstream draft
#52) as its dynamic-object part; the retail DMO system is planned in `.local/research/npc/dmo-plan.md`.

## Credit

All prop gameplay here is **laaledesiempre's** work: upstream PR
[#15](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/15) "[Feature] Dynamic props: collide, grab/drag,
place (retail ObjectMove)" (draft, issue #13). Their 12 commits are cherry-picked with `git cherry-pick -x`, so each
keeps their authorship and records the original hash. #15 is based on their Linux/macOS port (#9); none of #9 is
brought in. The upstream DMO placement exporter (`tools/asset_pipeline/dynamic_props.py`) that #15 extends is
upstream's.

## Problem

Retail districts are full of movable objects (benches, trash bins, dumpsters, ramps, vending machines; 226
placements in DownTown). Upstream main exports their placements but, since `e2b85b6` replaced the renderer, no
longer loads `private/native-props` at all: props are neither drawn nor collidable. #15 makes them live instances
that collide, can be pushed, grabbed, dragged and placed, with layouts saved per map.

## What #15 implements (their phases)

- **Phase 0, live instances:** the exporter writes template geometry once per DMO template and one MOBJ schema 4
  record per placement (schema 3 fields plus a 12-float row-vector affine). Parser and validation in
  `skate-data::skate_map` (`StaticObject.transform`, later `ObjectPhysics`).
- **Phase 1, collision:** each instance's render triangles, baked to world space, form a second `BoardWorld`
  (`skate_world::build_prop_layer`, `GamePhysics::prop_world`), queried beside the map world by board, skeleton
  and wheel queries. Surface tags use the same `audio | physics<<7 | pattern<<12` mapping as the map.
- **Phase 2, rigid bodies** (`physics/prop_dynamics.rs`): TU3 integrator, retail rounded-box mass properties,
  GP pair queries for box vs world / box / skater volumes, a compact impulse pass; moved instances rebake their
  triangle range (`BoardWorld::replace_triangles`, new in skate-core).
- **Phase 3, grab and carry** (`physics/prop_carry.rs`): A on foot toggles grab of the nearest prop
  (#15; since 2026-10-05 a held RB, the retail GrabWorld button, because A is retail sprint, see doc 26 "Props
  pulled toward the player").
- **Phase 4, placement and layouts** (`prop_carry.rs`, `prop_layout.rs`, `prop_carry_hud.rs`): B while carrying
  enters placement (right stick distance / yaw, DPad height; now releasing RB confirms); poses persist to
  `settings/prop-layouts/<map>.json` and reload on map load; a small HUD diamond.
- **Later commits:** authored MOBJ physics (density, friction, restitution, damping, sleep flags), Skate 3 style
  drag carry, surface-distance and omnidirectional grab, momentum-style skater push, body-bump speed cap
  (1.2 m/s), retail MovingObject presentation (grab flag, OffBoardPushing 502, MovingObjectNew subtree with the
  MVOBJ_* animations), MovingObjectNew condition leaves, walking from the ObjectMv stick.

Their own design notes (phases 0 to 4) were in `docs/dynamic-props-and-grime.md` on their branch; upstream no
longer tracks `docs/*.md`, so the key points are summarised here.

## Adaptations to current main (our commits and conflict notes)

1. **Renderer (our commit "Props: adapt #15 to main's renderer").** Phase 0's runtime half (the
   `retail_backdrop.rs` loader, `skate_world::spawn_instances` on the material-batched renderer,
   `SceneEntity::spawn_child`) targets code upstream replaced in `e2b85b6`. Re-implemented on the current renderer:
   `skate_world::load_prop_package` (the old loader's render-only checks), `spawn_instances` (one root entity per
   MOBJ record with `PropInstance { id, template, name }` and the record's affine; the template range is merged per
   (render class, slab) in template space with the props package's own `MaterialTable`, shared by every instance of
   that range), `SceneCommands::spawn_with_children` (only the root carries `MapEntity`, children go with it), and
   `PreparedScene::prepare` spawning props for retail districts as before.
2. **Phase 1:** upstream moved the portable collision tests out of `skate_world.rs`; their prop collision test now
   sits at the end of the current test module with its own `material()` helper.
3. **Phase 2:** `BoardWorld::replace_triangles` is kept next to main's water surface queries.
4. **Phases 3 and 4:** main's `solve::advance` wraps `advance_inner` (mod part physics); `grab_rising`, later
   `carry_tick`, is passed through the wrapper. Main's water board drag call before the solve stays.
5. **Old packages (our follow-up commit):** a props package written before MOBJ schema 4 has no placements; the
   loader now says so ("re-run setup (maps)") instead of silently drawing nothing. A test covers the instance spawn
   (two placements of one template: one mesh, two placed roots, children sharing it, only roots marked).

No other changes to their code.

## Setup impact

`tools/asset_pipeline/dynamic_props.py` and `map_writer.py` change, so the **`maps` and `environment` setup
fingerprints change**: existing installs re-run those two groups once (the customiser and other groups are not
affected). Until then the old props package has no placements and the game logs the hint above; nothing else
changes.

## Verification (2026-10-05)

- Builds: dev (`cargo build -p skate-game --bin skate3rust --release --locked`) and release / CI static
  (`+crt-static`, `--no-default-features`, `--target x86_64-pc-windows-msvc`).
- skate-game: 422 pass (incl. the new instance spawn test), 1 known failure `pipelines_accept_valid_group_outputs_when_fingerprint_changes`.
  skate-core 622 pass, 2 known failures. skate-data `--lib --tests` green. skate-mods green except the Skyline
  GLB test (gitignored model). Python: 177 setup tests OK (2 skipped).
- Not yet: in-game checks (see open questions); `--validate-maps` needs installed assets and a refreshed setup.

## Grabbing spins the player; dragging does not work (2026-10-05)

**Problem.** User, verbatim: "when grabbing objects the player spun and had trouble with dragging them around as
they should be able to". Video `2026-10-05 11-43-41.mp4`: 0:06 to 0:11.5 the skater holds a shopping cart (HUD
diamond cyan), bent over it in the MovingObjectNew pose, and skater and cart circle round each other on the spot
while the camera swings round; 0:43 to 1:14 the same with a bin.

**Root cause [code].** In state 502 (OffBoardPushing) with a held prop, `biped_ground::update` rotated the carried
OB_ObjectMv stick by the skater's current ground frame into a world direction and fed it to the walking
controller (`ground_input::calculate`). That controller only walks forward and turns toward its stick
(TurnVsStickAngle). A stick that is not straight ahead therefore always sits at the same angle from the facing:
the skater turns, the target turns with them, and the turn never ends. Pulling back asks for a 180 degree turn
every tick. #15's prop follow kept the prop on a world bearing, so the prop slid round the turning skater.

**Retail [code][data].** The object-move inputs come from 8259C4B0, called by Fill825999F0 with the current
RawControllerInput (raw left stick X/Y, right stick X):
- angle a = atan2(x, -y) wrapped to (-pi, pi], curve key |a| / pi (constant 822F8610 = 1/pi);
- OB_ObjectMvX = x * curve 1A1A7AC37A72DF87, OB_ObjectMvZ = y * curve 05BA8B52C23B3481 (inputlistener,
  PointNegGraphData8; X gain is 1 everywhere, Z dips to about 0.54 on diagonals);
- OB_ObjectMvRot = clamp(right X + sign(a) * curve 9ADFC2E222938C1E (16 points) * s, -1, 1) (clamp constants
  8216DEE0 = -1, 8231A844 = 1). Every Y value of 9ADFC2E222938C1E is 0, so the left stick never turns the
  skater and object; only the right stick does. The scale s (caller f21) is not resolved; it only multiplies
  that zero curve.
The stock MovingObjectNew graph picks MVOBJ push / pull / left / right clips from the OB_ObjectMv angle.
Retail's physics state 502 (string `PhysState_OffBoardPushing`, 0x82080660) is not decoded, so how far retail
moves the pair per second (likely the MVOBJ clips' root motion) is not known.

**Change.**
- `skate-core` `produce_object_move` ports 8259C4B0 with the three shipped curves (`ObjectMoveCurves`, loaded by
  `PhysicsSettings` from the inputlistener collection) and publishes OB_ObjectMvRot from the right stick (was 0).
- `biped_ground::update` (502 with a held prop): the walking stick stays idle (no turn toward the stick). The
  pair's planar velocity comes from `prop_carry::object_move_motion` (left stick, skater frame: push, pull, side
  step) through the controller's velocity override, so contacts and obstacle rejection still apply; the pair
  turns only by OB_ObjectMvRot.
- `PropCarry::follow`: the prop keeps its grab offset fixed in the carrier's frame (pulled in to 0.9 m) and turns
  at the carrier's yaw rate (`PropDynamics::set_yaw_rate`), so it stays in front of the skater while turning.
- Speeds are engine values until state 502 is decoded: push 1.4 m/s, pull 1.0 m/s, side 0.8 m/s, turn 1.6 rad/s
  (`CarryLocomotion`). Mods: `sdk.world.set_tuning('carry', {push_speed, pull_speed, side_speed, turn_rate})`
  next to `grab_bit`, `placement_bit`, `grab_range`; mod disable restores the defaults. All values are per tick
  and deterministic (no wall clock, no randomness), ready for a later network authority.

**Files.** `crates/skate-core/src/input/offboard_intentions.rs` (+ tests), `crates/skate-game/src/physics/`
`biped_ground.rs`, `prop_carry.rs`, `prop_dynamics.rs` (set_yaw_rate, tests), `settings.rs`,
`offboard/settings.rs`, `animation_phase.rs`; `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-mods/src/api.lua`, `sdk/skate.lua`, `sdk/GENERAL_API.md`.

**Verification.**
- `move_object_left_stick_never_turns_the_pair`: 16 left-stick directions for 240 ticks each, yaw rate exactly 0
  and travel on a straight line in the stick's direction; right stick turn bounded by `turn_rate`.
- `dragged_prop_follows_a_straight_push`: 180 ticks of a straight push, the prop stays within 5 cm of the push line
  and 0.9 m ahead.
- `turning_carrier_swings_the_held_prop_with_it`: a 90 degree turn leaves the prop in front (bearing 90 +/- 10
  degrees); #15's world-bearing follow kept the prop at its old bearing (0 degrees).
- `object_move_maps_left_stick_and_right_stick_rotation`, `object_move_z_gain_follows_the_stick_angle_curve`
  (skate-core), `carry_move_object_speeds_set_and_reset` (Lua path and reset) and the skate-mods carry validation.
- `cargo test --locked -p skate-game --bin skate3rust`: 482 pass, 1 known failure
  (`pipelines_accept_valid_group_outputs_when_fingerprint_changes`). The asset-backed
  `raw_x_offboard_jump_connects_input_ground_launch_air_and_landing` passes with the stock data (loads the new
  curves; on-foot walking unchanged).
- Not yet checked in game: hold RB next to the cart or a bin, push, pull, side step with the left stick, turn with
  the right stick.

**Open.** Decode state 502 (PhysState_OffBoardPushing) and the MVOBJ clip root motion to replace the engine
speeds; the prop's retail grip point (hands on the handle) and whether heavy DMOs move slower.

## Open questions

- Retail parity: every DMO is dynamic and box-approximated; retail drives DMOs through `LWDynamicObjectMan` with
  per-type characteristics (230 `livingworld_dynamicobject_characteristics` records), priorities, census spawn /
  cull rings and safety areas. Planned as milestones D0 onward (dmo-plan).
- Grind probes see only the static world (props not grindable yet), multiplayer is host-local (#15 known
  limitations); memory on DownTown is heavy (#15 notes).
- Board stuck inside a prop (bench near the Aletown spawn, frame drop): #15's push nudged a prop every tick while a
  skater volume sat inside its render-AABB box, so a bench never slept and rebaked (and rebuilt the prop layer's
  query index) every tick. Fixed 2026-10-05 with a bounded, data-driven push and depenetration (`PropTuning`,
  per prop type overrides, resource `PropTuningSettings`); see doc 26 "Board stuck inside a prop". Still open: the
  render-AABB box is solid under seats where the mesh is open; retail's collision for these DMOs is not recovered.
- Grab button: A (#15's choice) was the retail sprint button and grabbed props while running; it is now a held RB
  (GrabWorld, the gate of retail 82D324B0), release drops. Placement (B) is still #15's choice. See doc 26.
- Move Object speeds (push / pull / side / turn) are engine values until retail state 502 is decoded; see
  "Grabbing spins the player" above.
