# 5. Invisible collision volumes

## Problem

On SkateSchool the player hit an invisible wall in the middle of the spawn
area and could not skate around. Logs showed normal physics states (no
failures), so the obstruction was in the collision data.

## Root cause

Retail collision archives contain a small number of clustered meshes in which
**no unit carries a surface ID** (unit flag `0x80` unset; units read as flags
`0x21` instead of the usual `0xa1`). Every one of them, in every shipped
district, is a 12-triangle axis-aligned box: zone, trigger or level-bounds
volumes. The game's loader (`skate_world::retail_collision_world`, via
`skate_data::retail_collision::visit_clusters`) turned every triangle into
solid collision, so these volumes became invisible walls, floors and ceilings.

SkateSchool has seven, all in its global stream (`cSim_Global.xsf`):

| Box (X / Y / Z, metres) | Effect in game |
|---|---|
| −79…16.5 / −1.53…6.65 / −43…52 | covers the whole school area; the authored start (y 0.11) is inside it |
| −66…−62 / −1.53…6.65 / −3.6…12.7 | the invisible wall in the spawn area |
| −11…2.5 / 0…4.16 / −2.6…1.6 (three identical) | invisible block near the origin |
| level bounds, Y −73…79 | floor under the whole map |
| one more box | — |

Evidence that these are not solid in the original game: every authored
SkateSchool locator (Coach Frank, the hub, all tutorial stations) is at
y 0.0–0.4 on the real floor (surface 258), i.e. inside the big box; and in
DownTown, a surfaceless box (X 259–333, Y 48.9–57.7, Z −152…−71) encloses
the Kube Tower landing zone (`Z_DT_CubeTower`, y 55.9) and four authored
challenge start points.

Before the spawn change in doc 4, SkateSchool's spawn (y 7.63) stood on top
of the big box, which hid the problem.

## Survey of all districts

Classified per mesh by surface-ID presence:

| District | Fully surfaceless meshes | Notes |
|---|---|---|
| SkateSchool | 7 (all 12-triangle boxes) | global stream |
| MaloofMoneyCup | 2 (12-triangle boxes) | global stream |
| DownTown | 2 (12-triangle boxes) | city cell streams |
| MegaPark | 1 (12-triangle box) | global stream |
| others | 0 | |

**Mixed** meshes (some units with surface IDs, some without) are common in
real geometry: University cells (up to 31k triangles), Industrial, DownTown
(138 meshes), BlackBoxPark, and the main geometry of several parks. A
per-triangle filter would therefore remove real floors; the rule must be per
mesh.

## Change

`crates/skate-data/src/retail_collision.rs`:

- `RetailTriangle` gains `has_surface` (unit flag `0x80`).
- `mesh_clusters` decodes and validates a whole mesh (including the triangle
  count check) before visiting its clusters, and skips the mesh when no unit
  has a surface ID. `visit_clusters` returns the number of triangles visited.
  Memory: one decoded mesh at a time instead of one cluster.

The fix is in the game, so existing installations benefit without an asset
refresh.

## Verification

- New test `skips_meshes_without_any_surface_ids` (fixture mesh with its
  surface ID stripped is skipped; the others are visited intact). All
  `skate-data` library and integration tests pass, as do the game's
  collision/world tests.
- Collision triangles loaded per map (`SKATE_RWCM_READY`), before → after:
  SkateSchool 91,790 → 91,706 (−84 = 7 boxes), DownTown −24, MaloofMoneyCup
  −24, MegaPark −12, all other maps unchanged. Every map passes
  `--check-assets`.
- In play: SkateSchool (spawn area free to skate) and DownTown tested
  thoroughly; the Maloof and MegaPark box locations (below) were visited
  afterwards and are also clear.

Further evidence the volumes are triggers: MegaPark's only box
(X 61.4–73.2, Y −4.1…4.1, Z −140.8…−132.6) surrounds the stadium's arrival
point from the world (`tele_world_to_stadium_dest_locator_01`, 2 m from its
centre), and one of Maloof's two boxes lies 14 m from its arrival point from
DownTown (`tele_dwtn_to_mmcp_dest_locator_01`). The other Maloof box
(2.8 × 3.4 × 2.0 m) is 17 m from a street challenge start.

## Notes for upstream

- `cargo test -p skate-data` currently fails to compile three examples
  (`apt_data`, `hud_data`, `scoring_flow_data`) that reference game-only
  modules; this predates this change. `--lib --tests` runs the test suite.
- The mesh-level rule is empirical (all 12 such meshes in the shipped data are
  boxes, and authored player positions lie inside some). If retail code that
  consumes these volumes (zones/triggers) is ported later, they should be
  routed there instead of dropped.
