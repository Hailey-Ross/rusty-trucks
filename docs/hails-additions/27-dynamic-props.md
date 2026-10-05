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
- **Phase 3, grab and carry** (`physics/prop_carry.rs`): A on foot toggles grab of the nearest prop.
- **Phase 4, placement and layouts** (`prop_carry.rs`, `prop_layout.rs`, `prop_carry_hud.rs`): B while carrying
  enters placement (right stick distance / yaw, DPad height, A confirms); poses persist to
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

## Open questions

- Retail parity: every DMO is dynamic and box-approximated; retail drives DMOs through `LWDynamicObjectMan` with
  per-type characteristics (230 `livingworld_dynamicobject_characteristics` records), priorities, census spawn /
  cull rings and safety areas. Planned as milestones D0 onward (dmo-plan).
- Grind probes see only the static world (props not grindable yet), multiplayer is host-local (#15 known
  limitations); memory on DownTown is heavy (#15 notes).
- The grab button (A) and placement (B) are #15's choices; retail's ObjectMove input is to be confirmed.
