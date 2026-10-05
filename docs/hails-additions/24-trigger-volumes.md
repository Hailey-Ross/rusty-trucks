# 24 — Named trigger volumes: exported, tracked, moddable

Branch: `feature/trigger-volumes` (from `main` 4488651). Related: PR #25 / doc 05 (drops the same boxes from solid
collision). This change does not touch collision, so it neither needs nor conflicts with #25.

## Problem

Retail collision archives hold clustered meshes with no surface on any triangle: 12-triangle boxes. PR #25 found
they were invisible walls and drops them from solid collision, but that only throws them away. They are the
collision shapes of the maps' **named trigger volumes**: SkateSchool's tutorial areas and map-wide reset box,
the MegaPark and Maloof teleport prompts, DownTown's session spots. The engine had no notion of trigger volumes at
all, so nothing (engine or mod) could react to the player entering one.

## What retail does

Reference: the TU3 build through skate3recomp (by @mchughalex, built on the rexglue SDK), the converted disc data
read in place, and a guarded hook run of the recomp. Addresses are TU3; no game code or data is copied.

**Data.** Each volume is an item of a `0x00EB0019` volume-set record in an RW4 simulation arena (ATOC processor
`0xAB329A6A`) of the district's `cSim_*.xsf` streams (fixup `82962538`, stream-in `82C9AFD8`). Item (240 bytes):
matrix +0, bounds +64/+80, **link GUID** +176, **instance id** +184 (the id challenge scripts address,
`0x2C701706xxxxxxxx`), +220 the index of a `0x00EB000A` link record, +224 the name (`<short>_0x…:0x…::[…]_HighLOD`).
The link record names an RW collision volume of **type 4 (box)**: 3×3 rotation rows (the box's axes in world
space), centre, half extents, fatness. That box is the real shape; the bounds only enclose it (SkateSchool's
`inthehub_vol_02` and `wrongway_vol_01` are 45° squares). A second link entry names the clustered mesh — the
surfaceless box PR #25 drops.

**Runtime** (`cTriggerVolumeMessageGroup`). The trigger manager (`82DD7AE0`) owns three groups: **Challenge**,
**Stairs**, **Camera**. Each sim tick (`82DD70E8`), for every tracked entity (Challenge: up to 9):

1. Its query shape is rebuilt (`82DD80B8` → `82ADA4E0`): a **cylinder** (RW volume type 5) of **radius 0.34 m**
   whose axis runs along `top − direction`, half-height **`0.5·|length − top| + 0.05`**, centred
   **`half-height − 0.02`** below `top`, where top / length / direction are three points the entity reports
   (entity vtable +12/+16/+20). So it spans from 2 cm above the top point down past the length point.
2. An AABB tree of the volumes' bounds gives candidates (`82DD90E0` → `82476158`), and each candidate is tested
   against the volume's **box** (`82DD8BC8` → `82DD8978` → `82DD8498` → RW volume-volume overlap `82AD3CD8`,
   tolerance 0.0). The bounds contain the box, so the narrow test decides.
3. The new set is diffed with last tick's (`82557600`): **entered volumes are posted first, then exited ones**, as
   `cMsgTriggerEnterCollision` (0x5EAD1B62) / `cMsgTriggerExitCollision` (0xE961529A), each **twice** — once with
   the link GUID, once with the instance id — plus the entity id.

Removing a volume (`82DD7018`) drops it from every entity's set **without** an exit message; removing an entity
(`82DD6D20`) posts exits for every volume it was in. In the recomp trace all 19 AddVolume calls went to the
Challenge group (world `tele_*`, session spots, own-the-spot hulls); Stairs and Camera got none on those routes.

**Where they live.** Only 11 volumes are in the world streams (= PR #25's boxes): SkateSchool `coach_frank_sksc`,
`ws_sksc_coachfrank_instance_01`, `tut_sksc_inthehub_vol_01`, `tut_sksc_inthehub_vol_02`, `tut_sksc_reset_vol01`
(the whole map, Y −73…79), `tut_sksc_wrongway_vol_01`; MegaPark `tele_stadium_to_world_volume_a`; Maloof
`tele_mega_ramp_up_volume_01`, `tele_mmcp_to_dwtn_volume_01`; DownTown `dwtn_sessionspot_01_kubetower_volume_01`,
`dwtn_sessionspot_02_spillway_volume_02`. The other 2,549 are in 337 challenge packages (`missions.big`), several of
which stream in during free skate (University's `tele_world_to_stadium_volume_01` among them) — not exported yet.

## Root cause

The converter never read the `0x00EB0019` records, and the engine had no trigger system to give them to.

## Change

### Setup (group `maps`, one refresh)
- `tools/asset_pipeline/map_volumes.py`: reads every simulation arena of a district stream once (only processor
  `0xAB329A6A` arenas are decoded), parses each volume set, follows item → link record → box volume, composes the
  item matrix, checks the box against the retail bounds (≤ 0.25 m; retail's rotated bounds are a few cm off) and
  writes **`maps/<Map>.triggers`** (JSON, `format: "skate3rust-trigger-volumes"`, `version: 1`): id (= instance
  id), short and full name, instance id, link GUID, group, oriented box, retail bounds, stream and arena. Maps with
  no volumes get an empty list (so "none" and "not converted" differ). Standalone staging without a conversion:
  `python -m tools.asset_pipeline.map_volumes <worldDIST_*.big…> --output <dir>`.
- `install.convert_map` writes it beside the `.skate` (like `.irradiance`); a failure is an optional-content note
  (`map-status/<Map>-triggers-availability.json`), not a failed map.
- A sidecar, not a new `.skate` section: an older engine refuses unknown `.skate` extensions
  (`validate_runtime`), so an extension would break every other build sharing the install; the sidecar is ignored
  by builds without this change.

### Engine
- `skate-core::triggers` (pure): `OrientedBox`, `Cylinder`, `QueryShape` (retail constants as data:
  `QueryShape::RETAIL` = 0.34 / 0.5 / 0.05 / 0.02, and `cylinder(top, length, direction)`), the narrow test
  `overlaps` (GJK distance with exact box and flat-capped cylinder support, f64 inside), and `Tracker` (per-body
  inside sets; enters then exits per body in order; removed volumes forgotten silently; removed bodies exit).
- `skate-data::trigger_volumes`: the format (serde), validation (unique ids, 64-bit hex ids, orthonormal axes,
  finite values, ≤ 4096 volumes), and `load_for_map`: `<Map>.triggers` next to the package wins over an embedded
  **`TVOL`** extension (schema 1, same JSON) so custom maps can ship volumes either way; neither = no volumes.
  `skate_world::validate_runtime` accepts and validates `TVOL`.
- `skate-game::trigger_volumes`: resources `TriggerVolumes` (map volumes in file order = retail registration
  order, mod volumes, mod switches), `TriggerBodies` (engine hook: any system can register a body's query
  cylinder by id), `TriggerState`; messages **`TriggerEntered` / `TriggerExited`** `{body, volume}`; system set
  `TriggerSet` (FixedUpdate, after physics, gameplay only). The player is body `"player"`, tracked first like
  retail slot 0. Its three points: the head body (top) and the toes' midpoint (length and direction) — the retail
  entity's points are not identified yet (open question). A map transition loads the new map's volumes on the
  loader thread and replaces the list at commit; insides are forgotten without events (retail unload is silent).
  Log lines `SKATE_TRIGGERS map=… volumes=… source=…` and `SKATE_TRIGGER enter|exit body=… volume=… name=… group=…`.
- `--check-assets` loads the map's trigger data and fails on a broken sidecar / `TVOL`
  (`SKATE_TRIGGERS_READY volumes=N`), so setup catches exporter errors; at runtime a broken sidecar is a warning
  and the map has no volumes.

### Modding (Lua API 2, `sdk.capabilities.triggers` = 1)
- Read: `sdk.triggers.list()` / `get(id_or_name)` / `inside(body)` (snapshot `triggers`), every field including
  retail ids, box, rotation, bounds, enabled, who is inside.
- Events: `on_event {name="trigger_entered"|"trigger_exited", body, volume, volume_name, group, instance_id,
  link_guid}`, delivered before `on_fixed_update` of the same tick (one event per transition; both retail ids are
  in it instead of retail's two messages).
- Write: `sdk.triggers.box(key, {center, half_extents, rotation, name, group})` (id `mod:<mod>:<key>`, ≤ 64),
  `remove`; `set_enabled(map_id, false)` switches a map volume off; `track(body_key, {radius, length})` follows a
  mod physics body (≤ 16; positions after the dynamics step, one tick late); `configure({...})` changes the
  query-shape constants (one mod at a time).
- Cleanup: everything a mod set goes when it stops, fails or the runtime resets; mod volumes / switches / tracked
  bodies are world-scoped (cleared on world change, like `sdk.volumes`).
- SDK docs: `sdk/ENGINE_API.md` (Trigger volumes), `sdk/skate.lua`, `sdk/GENERAL_API.md`.

### SkateSchool's reset box
`tut_sksc_reset_vol01` is exported and fires enter/exit like the others, but **no reset behaviour is ported**: the
research shows what the volume is (map-wide bounds, Y −73…79), not what retail does on exit (its consumer is the
tutorial's challenge script, not traced). It is exposed as data so a mod or a later port can act on it.

## Files
- `tools/asset_pipeline/map_volumes.py`, `tools/asset_pipeline/test_map_volumes.py`, `tools/asset_pipeline/install.py`
- `crates/skate-core/src/triggers.rs`, `crates/skate-core/src/lib.rs`, `crates/skate-core/tests/trigger_volumes.rs`
- `crates/skate-data/src/trigger_volumes.rs`, `crates/skate-data/src/lib.rs`, `crates/skate-data/tests/trigger_volumes.rs`
- `crates/skate-game/src/trigger_volumes.rs`, `crates/skate-game/src/tests/trigger_volumes.rs`, `main.rs`, `app.rs`,
  `map_transition.rs`, `skate_world.rs`, `modding/triggers.rs`, `modding/mod.rs`
- `crates/skate-mods/src/vm.rs`, `crates/skate-mods/src/lib.rs`, `crates/skate-mods/src/api.lua`
- `sdk/ENGINE_API.md`, `sdk/GENERAL_API.md`, `sdk/skate.lua`

## Verification
- Counts per map (exporter on the owned disc, data-gated tests in Python and Rust): SkateSchool 6, MegaPark 1,
  MaloofMoneyCup 2, DownTown 2, University / Industrial / StartPark / BlackBoxPark / DownTownSkatePark /
  IndustrialSkatePark 0 — **11**, names identical to the research and to PR #25's surfaceless boxes; ids, link
  GUIDs and bounds match `volumes.json` from the research tools.
- Tests: Python `test_map_volumes` (synthetic + data-gated on the disc); skate-core `tests/trigger_volumes.rs` (9:
  retail cylinder, analytic and sampled GJK checks, rotated boxes, tracker ordering and removal rules); skate-data
  `tests/trigger_volumes.rs` (5, incl. data-gated on converted maps via `SKATE3_MAPS_DIR`); skate-game
  `trigger_volumes::tests` (5, incl. the SkateSchool authored start entering `inthehub_vol_02` + `reset_vol01`) and
  `modding::triggers::tests` (3); skate-mods `trigger_api_crosses_the_lua_serde_boundary`. Full suites (release):
  only the known pre-existing failures (skate-game 2, skate-core 2, skate-mods skyline).
- End to end: the real `install.convert_map` (MegaPark, SkateSchool, scratch stage) writes the sidecars and its
  `--check-assets` step passes; `--check-assets` on all ten maps reports `SKATE_TRIGGERS_READY volumes=` matching
  the counts above.
- Fingerprints: only the `maps` group changes (core / hud / character / environment and the customiser unchanged).
- Collision: untouched — no collision code or data changed; `check_maps` / `--validate-maps` results are the same
  (the sidecar is not collision).

## Open questions
- Which three points the retail entity interface reports for the skater (entity vtable +12/+16/+20) — needs a hook
  on `82DD80B8` reading them; we use head / toes.
- Which entities fill the 9 Challenge slots besides the player (peds? AI skaters? remote players?). We track the
  player and registered bodies; remote multiplayer skaters are not tracked yet.
- The challenge-package volumes (2,549 in `missions.big`), their streaming by distance, and the consumers
  (teleport prompts, session spots, challenge scripts). The Stairs and Camera groups' users.
- What SkateSchool's reset and wrong-way volumes do on exit/enter in retail.
- PR #25 doc error (signed vertex deltas): fixed in doc 05 on `hails-additions` / the #25 description already
  (2026-10-02). The converter's `map_writer.spawn_point` fallback and `prepare_hawaiian_dream` bounds still use the
  vendored signed decoder — a separate change (it can move fallback spawns), not part of this one.
