# 26h: Living world: Traffic

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Change: milestone V0, vehicle data

What retail ships, read from the disc and the code for this milestone (details and formats:
[`living-world/vehicles-data.md`](living-world/vehicles-data.md)):
- **Cars** [data]: 18 vehicle recipes (`livingworld.big`), each a body (`Accessory`) and windows (`Equipment`) on an
  8-bone rig (root, chassis, six wheel bones), one LOD, materials `vehicle_chassis` / `vehicle_glass`; positions are
  half floats. The visible **drivers are modelled into the body mesh** (a dark low-poly figure in the left front
  seat); there is no driver model or texture, and no light material (no head / tail / brake lights).
- **Palettes** [data]: `livingworld_models` gives each car model 2-10 chassis colours and 1-10 secondary colours; the
  base records hold blue / red, the colours of the body atlas's paint mask.
- **Specs and drivers** [data + code]: `livingworld_vehicle_characteristics` (6 records) and
  `livingworld_vehicle_drivers` (5); fields with a reading code site are named (horn timers and speed, pull-over
  chance, parked time, alarm impulse / duration, the following rule's speeds), the rest keep `Hash_*` names.
- **Road network** [data, all 82 objects]: decoded completely: per district directed segments (lane runs go from
  `node_b` to `node_a`, so `node_a` is the destination), lane geometry as about 4 m Hermite pieces with edges and
  arc-length tables, junctions with 8 end records (the approaches and exits of node ends 0-3, each lane listing the
  turn connectors it may take) and the turn connectors (Hermite curves from an approach lane to an exit lane):
  DownTown 46 segments / 19 junctions / 88 connectors, Industrial 26 / 10 / 46, University 4 / 4 / 4. Milestone 1's
  77th segment was a phantom (the header's second count is the lane-run count, the third the segment count).
- **Traffic lights** [code `sub_826B1540`]: 4 signal controllers, each with two light groups and the cycle all-red
  0.5, green 7, amber 1, all-red 0.5, red 8 s (the other group's green + amber), odd controllers offset by half a
  cycle; values from the `livingworld.trafficlights` record. Which approach uses which controller and group is not in
  the road data (V1 reads the binding from the code).

Change:
- Setup group `livingworld` (same group, same vault conversion; only its fingerprint changes): `tables.json` gains the
  two vehicle classes, `vehicles/<recipe>.glb` (18 cars: rig, body / windows / wheels primitives tagged with their
  role and material kind, diffuse and normal maps), `vehicles.json` (models with palettes and stable palette ids
  `<model>/chassis/<i>`, entities with model / spec / driver, the vehicle census resolved to recipes), `roads.bin` v2
  (a little-endian road graph), `roads.json` v2 and `roads_raw.bin` (the old verbatim pack, unchanged). The export
  fails the group (world runs empty) if a painted vehicle census record does not resolve to built cars with a spec
  and a driver. Recipe reader: the LOD word is the material instance count (two on `reda_car`'s wheels); single-
  instance output is unchanged, so every ped file stays byte-identical.
- `skate-data::roads`: `RoadGraph::parse` / `to_bytes`, lookups by retail id (segment, junction), the connectors a
  lane may take at its destination, the segment a connector leads onto, Hermite evaluation and arc-length
  parameters. No I/O, no engine types.

Moddability: every value stays setup data a mod overrides by key (tables by class / record / field, palettes by
stable id, cars by recipe name with the same bone names and primitive tags, roads by retail id with
`RoadGraph::to_bytes` for custom maps); the mod surface itself is V8. Multiplayer: every car model, palette entry,
segment, junction and connector keeps a stable retail id, and the export is deterministic (two runs byte-identical).

Files: `tools/asset_pipeline/{living_world_vehicles,living_world_roads,test_living_world_vehicles}.py`,
`tools/asset_pipeline/{living_world,versions}.py`, `crates/skate-data/src/{roads,lib}.rs`,
`crates/skate-data/tests/roads_data.rs`, `docs/hails-additions/living-world/{vehicles-data,vehicles-design,peds-data}.md`.

Verification (milestone V0):
- `py -3.13 -m unittest tools.asset_pipeline.test_living_world_vehicles`: 13 OK; all pipeline tests (every
  `tools/asset_pipeline/test_*.py` + `tools.test_setup_assets`): 190 OK, 2 skipped.
- `cargo test -p skate-data --release --locked --lib --tests`: all pass; with `SKATE3_ASSET_ROOT` on the export,
  `--test roads_data` (3) and `--test living_world_data` (4) pass: shipped counts, continuous pieces ending at their
  junction, every connector in its lane lists and leading onto a segment, every lane into a junction has a way on.
- Export on the user's disc: 35 s, 18 / 18 cars, validation clean; ped files and GLBs byte-identical to milestone 1;
  `roads_raw.bin` byte-identical to milestone 1's `roads.bin`; fingerprints of `core`, `hud`, `character`,
  `environment`, `maps`, `audio` identical before and after.
- Rendered the taxi and sports_car_01 without their windows: the driver figure sits in the body mesh.


## Change: milestone V1, road graph, lane cursor, traffic signals and junction entry

**Problem.** Traffic needs the road network as a graph a car can drive on, retail's traffic lights and retail's rule for
when a car may enter a junction, before the census (V2) and the driver (V4) can use them.

**What retail does (code read, TU3 recomp as reference).**
- Road network object (global `0x830854C0`, vtable `0x8230C6A8`): `+8` vehicle by id, `+16` junction by node, `+24`
  lane run by id, `+32` segment by id, `+36` / `+40` signal controller by index (`sub_8269B2F0`, controllers 320 bytes
  apart) [code].
- **Signal binding** [code `sub_82E11E90`]: the controller an approach obeys is the connector's `from_end` (connector
  `+84`), so every signalled junction shares the 4 controllers: node ends 0 / 2 run together, 1 / 3 together, the whole
  city switches in step. The light is consulted only when the junction's `flag_04` (header `+0x244`) is set, so
  `flag_04` = "has traffic lights" (24 of 33 shipped junctions) [code + data].
- **Signal programmes** [code `sub_826B1540`, `sub_82E156D8`]: even controllers all-red 0.5, red 8 (= green + amber),
  all-red 0.5, green 7, amber 1; odd controllers all-red 0.5, green 7, amber 1, all-red 0.5, red 8. Walk list: a green
  splits into walk green `(1 - 0.4) x 7` = 4.2 and walk amber `7 x 0.4` = 2.8, every other phase is walk red. Jump
  targets `+304` / `+308` (all-red before red) and `+312` / `+316` (all-red before green).
- **Signal tick** [code `sub_82E158D8`]: a fixed 1/60 s per call (`0x820849C8`); below 0 the next phase starts and
  inherits the overshoot, except on the car list's wrap to phase 0 (overshoot dropped, walk list forced to phase 0).
  Called for all 4 controllers by the manager `sub_826B2C18` from the living-world update `sub_826BDB50` in the world
  tick `sub_82859E70`, which runs as a **fixed 60 Hz step** [trace: 60 ticks per second while the recomp renders at
  ~345 fps], so the cycle is 17 s of game time. The f32 carries make the odd controllers' walk light lead the car light
  by one tick (253 / 167 / 1 / 59 ticks), exactly as recorded.
- **Green wave** [code]: a priority vehicle (`sub_8269B328` sets it, `sub_8269B338` clears it; caller not found) within
  50 m (`0x8220E13C`) of a signalled junction, or inside one, calls `sub_826B3C88(end)`: controllers of the end's parity
  jump to their all-red before green, the others to their all-red before red, and the lights freeze (`+13382`) once
  the end's controller is green, until the priority is cleared.
- **Connector choice** [code `sub_82C376E8`]: when a car has no connector or changed segment it picks one from its
  approach lane's list and stores it at `+4388`. With vehicle flag `+4401` bit 0x02 (constructor `sub_82C3B7C8` writes
  0xCE, no code clears it, so every car) the connector whose exit lane has the smallest `load / exit length` wins (exit
  segment `+136`, a vec4 per lane; strict `<`, first wins a tie). The other branch (flag clear, unused) picks
  `trunc(u32 x 100 / 2^32) mod n` from the world RNG (`0x822F94B0`).
- **Junction entry** [code `sub_82E11E90`, run by FollowingLane once the stop line is within look-ahead + speed x 1 s;
  conflict scans `sub_82E11C78` / `sub_82E11980`]. Results (the VEHJUNC codes; stored as junction state `+4392`):
  - 0 go;
  - 1 signal: red or amber, or green with `speed x remaining green < distance to the line - length / 2`; a right turn
    (`to_end == from_end - 1`) skips the light (turn on red). A car stopped at the line therefore only goes on green when
    its front is within half its length of the line;
  - 2 approach: faster than the connector's entry speed (connector `+0x50`, V0's `f32_50`: the road speed on straight
    connectors, 1.4-2.7 m/s on turns) while still beyond its look-ahead; at an unsignalled junction the entry speed is
    0.1 m/s (`0x820641A8`), a stop sign;
  - 3 yield: another connector from my lane in use; a car inside the junction merging into my exit lane (left turns
    check lanes up to theirs, right turns and straight on the lanes from theirs outwards); or a crossing / oncoming flow.
    Cars still on their approach count only by distance (closer first, a tie goes to the lower id), at unsignalled
    junctions for every flow and at signalled ones for the oncoming flow of left turns and straight on. A blocker with
    mover flag `+96` (the skitch speed-cap bit) makes FollowingLane set state 5 (horn 3);
  - 4 blocked: no room on the exit lane (`my length > rear car distance + speed x 1 s - its length / 2`), the car ahead
    on my connector closer than my minimum gap, or a missing approach / exit segment.
  - The light check is skipped (`r7 = !sub_82C344D0`) while the player skitches the car (`+4403` bit 0x80) or for a car
    with `+4401` bit 0x10 faster than the driver's `Hash_F142ABBFBEDA71E2` km/h (taxi 40); no code read sets bit 0x10.
- **Lane leader** [code `sub_82E14EE0`]: each lane of a segment (`+104`) and each connector (`+8`) lists its cars newest
  first; the leader is the next older car, none for the front car, the rearmost car for one not on the lane.
- Lane geometry [data]: a piece's centre curve is the road middle; lane `k` of `n` sits at `(k + 0.5) / n` from the left
  edge (4 m lanes, lane 0 left); connectors start and end exactly on those lane points. Right turns leave from the right
  lane, left turns from the left lane.

**Evidence.** proof1 / proof2 VEHJUNC (89 result changes): straight on red / amber always 1, green 0 (1 when the green
cannot be cleared), right turns never 1, unsignalled right turns 2, light skipped with the flag off. TRAFLIGHT2: all
500 recorded phase changes reproduced tick for tick. TRAFPROG: the programme layout above.

**Change.** `skate-core::living_world::traffic`:
- `RoadNetwork::build(&RoadInput)`: segments / junctions / connectors sorted by retail id (stable dense indices on every
  machine), ends resolved to segments, references checked; `lane_frame` (fractional lanes for lane changes),
  `connector_frame`, `next_connectors`, `connector_exit` / `approach`, `adjacent_lanes`, `nearest_lane` (engine helper).
- `LaneCursor`: lane or connector plus distance; `advance` carries the overshoot across pieces, onto the chosen connector
  and onto its exit lane (choosing the next connector on entry, as retail); stops at dead ends; `set_lane`.
- `choose_connector` with `ConnectorChoice::{LeastLoaded (retail), Random}` and a load function.
- `SignalClock`: 4 `Controller`s built from `SignalTimings` (data), the f32 tick, a 60 Hz accumulator so any engine
  frame rate gives the same ticks, phase-change records, `controller_for_end` / `light_for` (binding), `walk_for_end`,
  `request_green` / `clear_priority` (green wave) and `priority_end`.
- `junction_entry(&EntryQuery)` returning `Entry::{Go, Signal, Approach, Yield, Blocked}` plus the flagged-blocker bit;
  `Occupancy` (newest-first lists, `leader`, `lane_load`); `VehicleSnapshot` (the mover values the query reads).
`skate-data::roads`: `RoadGraph::traffic_input()`, `signal_timings(&tables)`.

**Moddability.** Durations come from the `trafficlights` record (a content overlay changes the city's lights;
`SignalClock::set_timings` rebuilds live); `ticks_per_second` is a field; the connector choice takes a policy and a
load function; `request_green` gives a mod (or a scripted car) retail's green wave; `RoadInput` is plain records a mod
map can fill (or `RoadGraph::to_bytes` for the file format). Stable ids: `SegmentId`, `JunctionId`, `ConnectorId`
(junction + retail index), `LaneId`.

**Multiplayer readiness.** No hash-map iteration, no frame-time dependence; the light state is a pure function of the
tick count and timings (a client can rebuild it from the host's `SignalClock::ticks`); random draws take an explicit
seeded `Rng`.

**Tests.** 18 unit tests on synthetic graphs (graph build / rejects, lane frames, cursor continuity across pieces,
connectors and segments, connector choice, programme layout, recorded tick gaps, 30 / 60 / 144 / 240 / 29.97 Hz
identical, controller alternation, approach binding, green wave, junction results for lights, stop sign, yields,
blocks, distance and id priority, left turn vs oncoming, leader, priority distance); 1 skate-data unit test; 6
data-gated tests (shipped counts and 24 signalled junctions, connector endpoints on lanes, every lane reachable,
cursors over the whole network, 4 controllers with the shipped timings, recorded timelines from the traces).

**Open questions.** See the list below.

## Change: milestone V2, the vehicle census
Ported from the code: the census vehicle pass `sub_826B7760` / `sub_826B9B90` (ring 80-100 m, cull 110 m, 2 attempts /
1 spawn per pass, initial 6000 / 600 in 8-80 m, cap per district x Free Play traffic, none online / in zombie mode /
with the census enable bit clear), the entity roll (`sub_826B8B88`), the **vehicle limit of 15** (census `+148`,
`sub_826B83C8`: DownTown's cap of 30 never fills; the recomp never showed more than 15 cars), and the factory's road
placement (`sub_82C36300`): the car goes on the road surface under the ring point (point in a piece's road
triangles, `sub_826B3B18`), at that piece's start, at least 15 m from both segment ends, on a lane where the next car
ahead starts 15 m further and the car behind ends 7.5 m (plus 1 s of its speed) earlier (`sub_82E14928`), one fitting
lane picked at random, then a 20 m overlap check. Cars drive their segment's direction; no heading is drawn.
Spawn records (`SpawnChoice::Vehicle`) carry the census record, category, entity, model, palette indices
(`vehicles.json` `palette_ids`), the retail segment id, lane and distance: everything a client needs. Rules are data
(`PlacementRules`, `CensusKindConfig` limits) a mod can override; `LivingWorld::update_lane` is the V3 hook.
Files: `skate-core/src/living_world/{traffic/spawn.rs, census.rs, config.rs, population.rs, mod.rs}`,
`skate-data/src/living_world.rs` (`vehicle_catalog`), `skate-data/tests/vehicle_census_data.rs`,
`skate-game/src/living_world/mod.rs`. Open: palette pick (not in the code read; seeded per id), spawn speed (0),
the overlap extents formula (radius = half the larger side), the pool size behind the limit, the meaning of
`FFE5E258BD468196` (census `+144`).

## Change: milestone V3, cars on screen
Census cars become visible, moving traffic that obeys the lights and is heard through #32's engine audio.

- **Follower** (`skate-core::living_world::traffic::follow`): per 60 Hz world tick, every car (in key order) runs
  the V1 junction query once the stop line is within its look-ahead plus one second of speed, brakes to the line on
  Signal / Yield / Blocked (`-v^2 / (2 (d - f2) + 0.001)`, `sub_82C3FA08` [code]), slows to the connector's entry
  speed on Approach, keeps `min_gap` behind the car ahead on its path (same lane, its chosen connector, the exit lane),
  and integrates speed with retail's integrator (`sub_82C3FF38` [code]: speed += accel x dt, accel 0 above the cap;
  cap = lane speed limit, 14.167 / 13.889 m/s [data], 17.0 while skitched [trace, V7]). Pull-away accel ramps at
  0.8 m/s^3 [trace] up to the spec field `Hash_328B9F4685A14018` (2.0-3.1, [data], meaning a candidate); planning
  decel = `Hash_758229215579C6D1` (2.5-3.0, candidate); hardest braking 7.3 m/s^2 [trace]. Connectors by retail's
  least-loaded rule (V1). Cars at a dead end leave.
- **V3 simplifications until V4**: cars are the only obstacles (skater, NPCs, peds are V4 / V5); the look-ahead is the
  comfortable stopping distance (retail `+3516` not read); following is "brake to the lead's speed by `min_gap`
  (2 m, engine value)" instead of retail's lead-minus-20-km/h term (kept as `retail_follow_accel` for V4; its gap
  inputs `+3756` / `+3760` / `+3728` are not read); a car that got Go and can no longer stop comfortably commits
  (amber dilemma zone); no lane changes, overtakes, horns, parking, skids.
- **Engine-side safeguard, not retail**: two lanes of one approach merging into one exit lane. Retail's junction
  query never scans the car's own approach (`sub_82E11E90` passes only ends from_end + 1 / + 2 / + 3 to
  `sub_82E11C78` / `sub_82E11980` [code]); the spacing comes from the look-ahead (V4). Until then the follower treats a
  car inside the junction on another connector into the same exit lane, nearer the exit, as the car ahead.
- **Look** (`skate-game::living_world::vehicles`): the car GLB as a scene, glTF materials on render layers 0 / 28 (the
  mod-graphics path); `vehicle_chassis` gets a tinted copy of its base texture, `vehicle_glass` is drawn at 0.55
  opacity (engine value); wheel bones spin by distance / `wheel_hint` [data]. Drivers are in the body mesh, cars have
  no lights [data, V0].
- **Tint rule (shader not decoded, open)**: the atlases paint the body pure blue `(0, 0, b)` and the base palettes are
  chassis `(0, 0, 1)` / secondary `(1, 0, 0)` [data], so the chassis colour replaces the blue channel and the
  secondary the red channel, weighted by channel purity; identity for the base palette. The painted value is scaled by
  a gain of 2.0: an estimate from the data (paint blue sits at 0.5-0.56; only 2x makes palette white and taxi yellow
  read as such), not retail, overridable (`VehicleOverrides::paint_gain`).
- **Collision**: a kinematic box per car from the GLB bounds joins the skater's contact solve
  (`physics::network::Proxies`); solid only, bails and roofs are V5.
- **Audio**: `TrafficAudio` per car: engine = the entity's spec `engine_audio` record, speed = `+3412`, load = the
  commanded accel `+3408`; `AudioVelocity` for Doppler.
- **Multiplayer (no networking)**: a car's motion is a function of its spawn record, the world tick, the signal
  clock's tick count (one per world tick since the world loaded) and the other cars; the connector choice reads the
  occupancy, so a client runs the whole car set from the host's spawn / despawn records (plus the host's tick and
  signal tick count) rather than one car. Retail runs no traffic online [code]; that default stays.
- **Mod surface**: `VehicleOverrides` (model per entity, GLB per model, colour per palette id, follower numbers per
  entity, connector choice, tint gain) and `TrafficEvent` (spawned, despawned, junction answer, entered junction,
  entered lane); `sdk.living_world.vehicles` bindings are V8.
- **Tests**: 7 core follower tests, 7 headless engine tests, 1 data-gated DownTown run (30 cars, 120 s, 0 entries on
  red, no overlaps, moving median 6.4 m/s / p90 14.2 vs the recomp's 5.8 / 14.7).

## Cars flying off, population gone, 2026-10-05

**Problem.** First play video of the living-world build (DownTown): "At a certain point in the first map shown
the cars leave the ground and fly off and all peds and vehicles cease to exist." The session log is empty; the
video is the only record. User: "The cars appear to fly away when I jump as im going uphill, not clear if its
related."

**Evidence.**
- [video] Frames 21.5-24.5 s (4 fps, zoomed): a green and a red car hang about two storeys up in front of the
  building on the left of the uphill street, moving left, while other cars further away sit on the road. From
  about 28 s no cars and no peds are seen until the map is changed; the frame rate falls from ~290 to ~50.
- [data] The road graph itself is clean: no non-finite values, no height steps (worst lane height vs its own
  road edges 0.09 m, connector bulge 0.19 m), connectors meet their lanes within 0.1 mm.
- [data] Compared with the DownTown ped walk mesh (`navmesh.bin`), every DownTown lane point lies on the ground
  (2093 points, worst 1.11 m), but the other districts in the same `roads.bin` overlap DownTown in x / z at other
  heights: University roads 16-18 m **above** the DownTown ground (segment `6A581EC75CE57FC5`, 820 m long, and
  two more), Industrial roads 47-59 m below it. 1173 of 3284 lane points of the whole file are off by more than
  1.5 m. The districts are separate worlds sharing coordinates.
- [code, engine] The world built its road network from the whole file (`traffic_input()`), and the car
  placement (`road_under`, port of `sub_826B3B18`) takes the first piece in network order whose road
  triangles hold the ring point horizontally. A ring point over an overlapping University road (when that
  road comes first in id order, or no DownTown road is under the point) put the car on that road, 16-18 m in the air; cars could also be routed across
  junction ends into another district. Inference, not read in the code: retail loads the road
  object per district, so its `sub_826B3B18` only sees the loaded district's roads.
- Nothing ties a car's height to the player after placement (pose = lane frame only); the ring point takes the
  focus height, but `road_under` ignores height, so the jump is most likely a coincidence.

**Root cause.** Floating cars: other districts' roads in the DownTown network (high confidence). Population
gone: not proven from the video. Found and closed on the way: an infinite focus position (or speed) put every
NPC beyond the cull in one pass (`dist2 = inf`), emptying the whole population at once.

**Change.**
- `skate-data::roads::RoadGraph::district_traffic_input(name)`: segments and junctions of one district only;
  a junction end whose segment is in another district becomes a map edge. `load_data` builds the network from
  the loaded map's district (a map with no road district gets no roads, so no cars, as before without a census).
  A mod's own road graph still goes through `traffic_input()` / `RoadNetwork::build`.
- Guard (engine, not retail): `census_pass` drops observer circles with a non-finite centre, cull or spawn
  radius; with none left the pass neither culls nor spawns, so a broken focus can never empty the population.
  The game logs `LIVING_WORLD census focus is not finite ...` when it happens. A traffic car whose state goes
  non-finite is removed alone with a `LIVING_WORLD traffic: car #N has a non-finite state ...` warning before it
  reaches the census.

**Files.** `crates/skate-data/src/roads.rs`, `crates/skate-data/tests/road_districts_data.rs` (new),
`crates/skate-core/src/living_world/{population.rs, tests.rs}`,
`crates/skate-game/src/living_world/{mod.rs, vehicles.rs}`.

**Verification.** `district_traffic_input_keeps_only_that_districts_roads` (skate-data unit),
`downtown_world_gets_only_downtown_roads_and_they_lie_on_the_ground` (data-gated: DownTown 0 points off,
whole file 1173 off), `a_non_finite_focus_never_empties_the_population` (skate-core; fails without the guard:
the infinite focus culls everything, passes with it; a real teleport still culls). skate-core living_world
92/92, skate-game living_world 28/28, `cargo build --locked` (dev) ok. Not yet seen in game.

**Open.** Why the population vanished in the video is not proven: the census focus is the board's deck body
(`gather_observers`), not the walking player, and the fix only guards a non-finite focus; a finite but wrong
focus (the deck left behind or moved while off the board) would still cull everything by the retail rules.
The FPS drop at ~28 s is not explained. A debug-log session (`SKATE_LIVING_WORLD_DEBUG=1`) of the same street
would settle both.

## Car shadows from a bridge printed on the ground below, 2026-10-08

**Problem.** Session 2026-10-07 12:14 (DownTown, player about [42.6, 15.8, 353]). User: "I did find a spot where
vehicle shadows from the bridge overhead were appearing below".

**Root cause.** Dynamic objects (skaters, traffic cars on layer 28, mod graphics) cast into one dynamic shadow map
that the baked world receives (`retail_character.rs` "Dynamic object shadows onto baked world", upstream ddc3028).
The world receiver keeps the darker of the baked lightmap and `visibility + floor`. Our floor was an adapter: the
player's nearest irradiance probe `sh[0]` (eased over 0.35 s). Under that bridge the probe is (0.0157, 0.0196, 0.0275),
3 to 5 times darker than retail's constant, so a car on the bridge darkened the bridge's baked shade below it.

**Retail evidence.**
- [code, shader microcode] `data/big/shaders_final.big`, read with `.claude/skills/living-world/tools/eb_big_extract.py`
  and `xenos_disasm.py`. Every world receiver pixel shader samples one blurred shadow atlas (`shadowAtlasBlurred`,
  `CSM_Mat_Row0..2`, `g_CSMBlurBias`): `defaultenvironment_defaultPS`, `environmentdiffuse_defaultPS`,
  `baseterrain_defaultPS`, `baseenvironment*`, `decal*environment*`, `transparent*`, `building_*`, `advertisement`,
  `water_defaultPS`, `flowingwater_defaultPS`. Each computes visibility = saturate(depth step + 1 - blurred value),
  adds the literal set {0.05, 0.09, 0.13} per channel and takes the minimum with the squared lightmap (e.g.
  `environmentdiffuse_defaultPS` instructions 24 to 31, `defaultenvironment_defaultPS` 62 and 64). Following the
  register swizzles back to the lightmap fetch gives R 0.05, G 0.09, B 0.13 in every one of them, including the water
  shaders, whose lightmap sits in G,B,R registers so the literal pool reads 0.09, 0.13, 0.05.
- [code] No height cut-off, receiver depth window or per-caster range in the receiver: one constant for every caster
  and receiver. The city world shaders (`defaultenvironment`, `environmentdiffuse`, `baseterrain`, ...) have no
  `shadow` technique, so the world never occludes the dynamic map; casters are `vehicle*_shadowPS`, character, ped,
  `dynamicobject_shadowPS`, `environmentpark*_shadowPS` and `videoscreen_shadowPS`.
- [code] Peds and dynamic objects additionally read a static world shadow map (`shadowWorld`, `WorldShadow_MatRow`,
  drawn by `WorldShadow_defaultVS/PS`, TU3 strings "World Shadow generation" / "DrawWorldShadowCasterInstances" at
  0x821A02D4 / 0x821A02EC). That is a receiver map for objects, not an occluder for the world.
- So retail's answer to the bridge is the floor: a dynamic shadow falling into baked shade at or below
  (0.05, 0.09, 0.13) leaves no mark.

**Change.** The world shadow floor is the retail constant `RETAIL_WORLD_SHADOW_FLOOR` (0.05, 0.09, 0.13) for every
receiver (lightmapped families, flowing water and water), held as data in `WorldShadowSettings`. The probe adapter
and its easing are gone. The water floor (family 33) was the literal in register order (0.09, 0.13, 0.05) applied to
RGB; it now uses the same RGB floor. Character shading, the two directional lights, their cascades and biases, the
layer-28 caster set and the shader's visibility term are unchanged (the character shader never read this floor).

**Moddability.** `sdk.world.set_tuning('shadows', {world_floor = {r, g, b}})` (each 0..1; 0 = full-strength dynamic
shadows), first writer wins, rebuilt to retail when the mod stops; `sdk.world.tuning(key, 'shadows')` reads it. Engine
systems use `modding::world_tuning::set` with the same domain.

**Files.** `crates/skate-game/src/retail_render.rs` (constant, `WorldShadowSettings`, `enable_world_shadows`, tests),
`crates/skate-game/src/retail_character.rs`, `crates/skate-game/src/retail_world.wgsl`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`,
`crates/skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.**
- Tests: `world_shadow_floor_is_the_retail_constant`, `baked_shade_at_the_floor_hides_a_dynamic_shadow_and_sunlit_ground_takes_it`
  (the retail receiver expression: shade at the floor is untouched by a full shadow, the old probe floor darkened it,
  sunlit ground still darkens to the floor), `every_world_shadow_read_uses_the_shared_floor` (shader source),
  `world_shadow_floor_defaults_to_retail_set_and_reset` (mod patch, first writer, reset), schema cases in
  `skate-mods` `patches_parse_validate_and_reject_unknown_fields`; existing shader validation tests
  (`retail_shader_tests.rs`) still pass.
- To playtest (rendering not checked by eye): DownTown under the bridge at about [42.6, 15.8, 353] with traffic on the
  bridge: no car shadows on the shaded ground. Elsewhere: the skater's and cars' shadows on sunlit ground are now a
  little lighter and bluish (retail floor instead of the local probe), and in deep baked shade they fade out as in
  retail. Compare with the recomp at the same spot if they look off.

**Open questions.**
- The retail visibility term (depth step plus `1 - blurred` from an exponential-blurred atlas, two cascades) is still
  Bevy's PCF lookup in ours; only the floor is ported here. The sign convention of the scalar-constant subtract was
  read as constant minus register (the only reading that leaves unshadowed receivers at full light).
- Retail's cascade extent and which vehicles are submitted to the shadow pass (CPU side, "Shadow Map Cascade" /
  "DrawShadowCasterInstances") were not traced; ours keeps one 24 m cascade.

## Cars hit peds: retail reaction, 2026-10-08

**Problem.** Traffic cars drove straight through peds: nothing tested a car against a ped. The user remembered (from
long ago, "user's memory is old, confirm in code") that a ped hit by a car ragdolls, then fades out or gets up and
flees.

**Retail evidence** [code, TU3; addresses are evidence only, nothing copied].
- Every contact on a ped's collision body goes through the ped's contact callback `sub_82E38FB8` (ped vtable
  `0x8232BE80`). It first asks vtable slot +164, `sub_82E38400`, for a contact kind 0..5. That classifier takes the
  other body's owner (`[[contact+76]+32]`) and runs the interface cast `sub_82965630` with the type getter
  `0x82C34050`, which returns `0x823220B8`, the type record named `IVehicle` (string at `0x823220B8`, `vehicle` at
  `0x823220C4`). A vehicle owner returns kind **2** at once: no speed, angle or flag test.
- The callback's switch (`0x82E39230`) sends kinds 1 and 2 to one block (`0x82E3926C`): it reads the pose of the
  ped's collision body (`sub_82585CB0`), subtracts the body-to-root offset (`[ped+5756]+19872`) when the ped's slot
  +180 says so, keeps the root's own height (the `vrlimi` keeps y), skips the result if it is not finite or out of
  range (`0x822F88D4`), and writes it as the ped's root position. That is all: no reaction kind or direction
  (`ped+2496` / `+2500`), no `Collision` intent (`PedestrianColliding`), no knock-down speech, no brain flag (the
  `+3196` bit 0x80 at the top is set only for kinds 4 and 5).
- The knock-down / stumble path (3.0 / 6.0 thresholds, `Collision.Knockdown` motion graph, animated, not ragdoll) is
  kind **5**, an `IActor` owner (type getter `0x82586478` -> `0x823000F0`, `IActor`), i.e. the skater; kind 4 is an
  actor contact on body part 1 or 2 (acted on only while `[ped+5756]+140` is 7); kind 3 (the object at `ped+5916`) sets `+3278` bit 0x80.
- The car side, `sub_82C3C150` (the vehicle collision interface at `+136`): a parked car's alarm test on the contact
  impulse; for an `IActor` toucher a bit in the "hit by" mask `+4248` and, for a contact ahead of the car, `+4401` bit
  0x20. It does not stop, honk or post a sound there.
- Peds run from cars only through the horn: the horn decider `sub_82C40660` honks (kind 2) after an obstacle has been
  ahead for 2 s and notifies the obstacle (the honked-at input, `RunFromHonker`, at most 30 s); doc 26 V4, not ported.

**Verdict.** The user's memory is refuted for TU3 (confidence high for the ped side: the classifier and the switch
are read end to end; medium that no other system adds a reaction, no other `IVehicle` test was found in the ped
code). A car shoves a ped out of its way (the car is kinematic with infinite mass, the ped's body is pushed, its root
follows) and the ped walks on. No ragdoll, no knock-down, no fade, no flee from the contact itself. Not checked in a
recomp run (no hook placed; peds rarely stand in a lane).

**Change.**
- `skate-core::living_world::peds::vehicle_contact`: `RetailContactKind` and `retail_response` (the classifier's
  kinds and the callback's switch), `VehicleContactParams` (retail defaults `enabled = true`, `push = true`),
  `detect` (a ped cylinder against a car's oriented box: overlap depth, normal and the pushed feet position with the
  height kept), `closing_speed`.
- `skate-game::living_world::vehicle_contacts`: `ped_vehicle_contacts` (`FixedUpdate`, after `advance_peds` and
  `drive_traffic`) tests every ped against every car's box (`car_box`: the GLB bounds, the same box as the car's skater
  proxy), peds and cars in id order; a contact pushes the ped out and keeps it on its navmesh (`constrain_move`), moves
  the drawn ped in the ground plane only, publishes `VehicleContactEvent` (tick, car and ped `LivingWorldId::to_u64`,
  car speed, closing speed, position, normal, depth, reaction; `Serialize` / `Deserialize`) and logs
  `VEHICLE_CONTACT car=#.. ped=#.. speed= closing= at=[..] normal=[..] depth= reaction= tick=` once per car and ped per
  second.
- Multiplayer: one system decides; it is a pure function of the ped and car states, which already follow from the
  spawn records and the tick, so a host and a client compute the same pushes; the event is the record a host would
  send.

**Moddability.** `sdk.world.set_tuning('living_world', {ped_vehicle_contact = {enabled, push}})`: `enabled = false`
turns the detection, event and log off; `push = false` reports the contact (`reaction = reported`) without moving
the ped, so a mod can react itself. First writer wins per field; the domain is rebuilt to retail when the mod stops.
`VehicleContactEvent` is the hook the planned `sdk.living_world` events read.

**NOT RETAIL YET.** The ped body is a cylinder of the NavPower agent radius and height (0.35 / 1.6 m [data]); retail's
Havok ped shape (`sub_82E26430`) is not decoded. The push is the smallest separation in the ground plane; Havok's
penetration recovery is not decoded. The navmesh stands in for the world collision of the pushed body. The car side
(hit-by mask, the planner stopping for an obstacle ahead, the horn and the ped's `RunFromHonker`) is V4.

**Files.** `crates/skate-core/src/living_world/peds/vehicle_contact.rs`, `crates/skate-core/src/living_world/peds/mod.rs`,
`crates/skate-game/src/living_world/vehicle_contacts.rs`, `crates/skate-game/src/living_world/mod.rs`,
`crates/skate-game/src/living_world/peds_tests.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`.

**Verification.** skate-core: `retail_vehicle_contact_follows_the_body_and_never_knocks_down`,
`a_ped_in_front_of_the_bumper_is_pushed_forward`, `a_ped_beside_or_clear_of_the_car_is_not_touched`,
`a_ped_inside_the_box_leaves_through_the_nearest_side`, `a_turned_car_pushes_along_its_own_axes`. skate-game:
`living_world_cars_push_peds_out_of_the_way_and_report_the_contact` (push, event fields, determinism, serialisation,
the two mod options), `living_world_car_box_matches_the_car_proxy`,
`ped_vehicle_contact_is_mod_reachable_and_reset_on_disable`. Not playtested.

**Open questions.**
- Whether retail ped bodies are actually displaced by a kinematic car in the Havok solve (the callback only follows
  the body); a recomp hook on `sub_82E38FB8` with kind 2 would show it. Peds seldom stand in a lane, which is why the
  user may remember a different game.
- The skater's car-hit bail rules (vehicle contact term `0x820CFF14`, 9.0 limits) are V5.
