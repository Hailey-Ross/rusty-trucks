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

## Skitching: research and the tow spring (2026-10-09, groundwork)

**Retail [code] (`.local/research/npc/b26-skitching.md`, `b27-skitch-state.md`, `b30-skitch-pull-release.md`; main
checked the state id, the vault keys of the spring, its constants and the update gate).** Cars carry grab splines;
the grab query (`GrabSplineQueryManager`) publishes grab records at ProcessedPhysIn +1888 / +2176 (ours:
`player/post_input.rs:180` `publish_candidates_82d740f8`); with the hand flag (+2476 bit 21, not the animation's
GrabWorld bit 22) and a ready record the selector enters Skitching (104; ours: `player/selector/ground.rs:48`). The
state's update (`sub_82D477C0`, vtable `0x82327398`) runs only while +2480 bit 22 is set and holds the skater to
the car with a velocity-target spring (`sub_82D4B500`, board force tag 6), not a pin; release is the selector's
(hand flag lost -> 100, vehicle contact > 9.0 -> 300, off the ground -> 100). The car side receives a per-frame
"held" message (`sub_82C361E8`: +20 % speed cap, a latch for the player).

**Change (groundwork only).** skate-core `riding::skitching`: `SkitchSpringSettings` (vault
`physics_state_skitching/default` retail values) and `tow_spring` (`sub_82D4B500`); `SkitchSubMode` (`sub_82D49580`,
`b31-skitch-submode-hold.md`, main checked its vault getters): modes 0 settle, 1 inside the grab range, 2 at the
edge, 3 stepping off, 4 let go (readings inferred), with the 0.5 s settle timers, the 0.4 m edge band, the stick
toward / outward tests and the outward car-acceleration push time; the riding skater's skitch query
(`sub_82D39D98`, `b33-skitch-wiring.md`, main checked our publication of the hand flag): time to a candidate spline
`max(0, (ahead - 1.0) / max(closing, 0.5))`, latch below 0.1 s, the re-grab cooldown that skips the last car
(`SkitchQuerySettings`, `choose_skitch`). The hand flag (2476 bit 21) is already published from the ground state's
latch byte (`player/input_phase/publication.rs:262`, `riding/grounded/state/output.rs:159`); nothing sets the latch
yet. The car grab splines are authored on the disc (`b34`, `b35`: RW4 GRABDATA `0x00EB001F` in the
vehicle model-part `.rx2` files of `livingworld.big`; main ran the parser over the disc: 17 car models, one rear-edge
Bezier spline each, 12 to 24 control points, direction (0, 0, -1), z -1.93 to -3.12 m in the model frame; grab
record type 1): `tools/asset_pipeline/grab_data.py` parses them (not yet called by the vehicle exporter). Not wired: no state-104 handler,
no hand-flag producer, no car grab splines yet.

**Verification.** skate-core `the_tow_spring_follows_the_speed_curve_and_is_capped`,
`the_sub_mode_tracks_the_grab_range`, `the_skitch_query_latches_the_first_spline_within_reach`; pipeline
`test_grab_data` (synthetic RX2).

**Open.** The skitch frame (`sub_82D48148`, research b32 running) and the state fields' meanings, the along-car hold
chain (`82D48C98`: car acceleration + stick + rest servo -> offset target; b31 decoded the arithmetic), the car grab splines (research b34), the latch wiring in the ground update (`ground_runtime/update.rs:117`,
gated by owner +12836 bit 0x40, open) and the car-side hook.

## Cars knock the skater down, and brake when hit (V5 start, 2026-10-09)

**Retail [code] (`.local/research/npc/b37-traffic-v5-car-hits.md`; main checked the contact debug string and our
vehicle group).** The skeleton contact pass (`sub_82BD4A30`) keeps the largest relative normal speed against
vehicle-group (8) bodies; the Ground / Air wipeout checks (`sub_82D90C98`) bail the skater (reason 7, WipeoutGround)
above `Wipeout_GroundVehicleContact` (9.0; `Wipeout_GroundSkitchingContact` while skitching), and touching a vehicle
shrinks the other wipeout limits (`Wipeout_GroundVehicleScalar` 0.35). The car side (`sub_82C3C150`): an actor (the
skater; peds are not actors) hitting the car ahead of it latches `+4401` bit 0x20 and the planner brakes hard
(accel = -speed) until the car stands; a parked car's alarm needs `alarm_impulse` (0.1).

**Change.** The traffic car proxy was in contact group 0 (static world), so the already-ported car-hit bail never
fired: it is now `VEHICLE_GROUP` (8). The skeleton contact collector also returns the vehicle-group solids the body
touched (`SkaterRuntime::vehicle_hits`); `apply_vehicle_hits` maps them to the follower car and latches
`Car::hit_brake` when the contact is ahead of the car (ours: along its velocity; retail's axis, the car's vtable
+24, is inferred forward); `follow::step` brakes at -speed and clears the latch at a stop. Log `VEHICLE_HIT`.

**Verification.** skate-game data-gated `a_traffic_car_knocks_the_skater_down_above_the_contact_limit` (DownTown,
a kinematic car box: 12 m/s group 8 bails at tick 6 (group 0: tick 7 through the generic limits); 4 m/s group 8
bails at tick 22 because that box never brakes, group 0 never); `living_world_traffic_proxy_is_the_model_box`
asserts the group; skate-core `a_car_hit_by_the_skater_ahead_brakes_to_a_stop_then_drives_on`. Not play-tested.

**Open.** Roofs (no roof code found; the car is a moving surface in group 8 now), the parked-car alarm (no parked
cars yet), the hit-by mask's reader, a board-only car contact.

## Traffic: obstacles ahead (V4 look-ahead, 2026-10-09)

**Retail [code] (`.local/research/npc/b36-traffic-v4-driver.md`, `b41-v4-obstacles-corridor.md`,
`b42-v4-leftovers.md`; main checked the characteristics / driver values, 0.68 and 0.025).** Every frame the traffic
manager refills two obstacle lists (`sub_826B2EE0`): skaters (radius 0, soft) and world actors (peds radius 0.5 x
body extents z, soft; movable props radius half their smallest extent, hard; traffic cars add nothing). Each car's
look-ahead quad (`sub_82C400A8`) runs from its centre to the bumper plus speed + standoff, flared on the turning side
(`02A6` 2.0 x speed ratio); a record touching its side / front edges or inside it is in the way
(`sub_82C41BD0`, `sub_82C41A90`); its free distance is `|pos - car| - radius - 0.68 x speed`; soft records count
below 20 km/h only for opted-in drivers (all stock drivers). The planner brakes for the nearest one:
`-v^2 / (2 (d - standoff) + 0.001)` within one second of travel plus the standoff, `-v` inside it.

**Change.** skate-core `living_world::traffic::obstacles` (`Obstacle`, `CarFrame`, `look_ahead_quad`, `in_quad`,
`in_the_way`, `nearest`, `obstacle_accel`); `Car::obstacle` (the free distance) feeds `follow::step`; skate-game
`look_ahead` builds the lists from the observers, the peds and the active DMO footprints (`PedObstacles`) and sets
each car's nearest obstacle once per frame before the ticks.

**Engine choices.** The ped radius is our fallback ped radius (0.35; retail's per-ped extents are collision data),
props carry no corner points, a bailing skater's four extra points are not added, the turn widening is off (the
turn side `+3748` is open), the standoff is the car's `min_gap`, and the lists use last frame's poses.

**Verification.** skate-core `the_look_ahead_reaches_speed_plus_standoff_beyond_the_bumper`,
`the_nearest_obstacle_brakes_the_car`; skate-game living_world tests (69). Not play-tested.

**Open.** The turn side, the per-ped radius, the skater-behind zone (quad B, 40 m). The horn: next section.

## Traffic: the horn, and peds running from it (V4, 2026-10-09)

**Retail [code] (`.local/research/npc/b36-traffic-v4-driver.md` sections 2, 4, 5, 8, `b41-v4-obstacles-corridor.md`
section 3, `b44-horn-honker.md`, `.local/research/peds/b45-runfromhonker-flee-timeout.md`; main re-read the honk
receiver `sub_82E3C3D0` and the shape of `826A1358`).**
- Limiter kind `+4392`: each driving-state update sets 0 (free) or 2 (stop point), the lead check 1 (behind a lead
  whose own kind is 1 or 5) or 3 (any other lead), the look-ahead 4 (obstacle); the nearest limit wins. The junction
  answer goes into the same field (1 signal, 2 approach, 3 yield, 4 blocked, 5 a yield to a flagged car), so a car
  queued behind one waiting at a red light counts as waiting itself.
- Timers: blocked `+3704` grows while inside the standoff behind a kind-3 lead and slower than
  `honk_approach_speed_kmh` (else 0 or held); obstacle `+3708` grows while the limiter is the obstacle, not inside a
  lead's standoff, and slow (no obstacle: 0).
- Horn decider `sub_82C40660`, every frame, first match: horn disabled (driver bit 0x01) 0; junction wait (5) kind 3;
  blocked > `honk_blocked_time` kind 4 (driver bit 0x02) or 5; an obstacle with record flag 0 (skater / ped):
  obstacle timer > `honk_obstacle_time` kind 2 plus the honked-at notify to the record's handle, else under 2 s to it
  and faster than the approach speed kind 1. The horn sounds while the decider returns a kind; the sound per kind is
  the `Traffic_Horn` AEMS program's (our native evaluator runs it).
- Driver bits (`sub_82C42348`): percent rolls `rand() % 100 + 1 <= chance x 100` on the driver record
  (`Hash_B5C60C1D43899F74` enabled: 1.0, taxi 0.2; `Hash_7C6B48BD9ADF8E6E` long: 1.0, fast 0.0, reckless 0.5).
- The notify `sub_82E3C3D0` only writes the car id into the ped brain's honker (`+3232`; peds only, the skater's and
  cars' records carry no handle). `IsBeingHonkedAt` takes Wander (off the road / at intersections) and WanderFollow
  into RunFromHonker: Begin motion intent 5; Update every frame while the car exists: goal 10 m sideways of the car's
  line on the ped's side (strict `dot > 0`, a tie goes to the minus side), speed 6.0 (3.0 within 2 m). The op's
  `timeout` 30 is never read; only Wander's Begin clears the honker.

**Change.** skate-core `traffic::horn` (`HornParams`, `DriverBits::roll`, `HornTimers::update`, `decide`, limiter
kinds), `Car` gets `driver`, `limiter`, `horn_timers`, `horn`, `honk_target`, and `obstacle` is now an `ObstacleHit`
(distance, soft, ped id); `follow::step` sets the limiter kind (junction answer, lead, obstacle), runs the timers
and the decider. `FollowParams.horn` comes from the entity's driver record (`livingworld_vehicle_drivers` in
tables.json). skate-core `peds::honk::run_goal`, brain op `RunFromHonker` (intent 5). skate-game: the look-ahead
records carry ped ids; the driver bits are rolled from the spawn seed; `TrafficAudio.horn` gets the horn state every
frame (a mod's `VehicleHorn` still plays on top; the car alarm is untouched); `TrafficEvent::Horn` on a change and
`TrafficEvent::HonkedAt` every frame of kind 2; `think_peds` sets the honker and runs the ped to the goal (run gait,
log `PED_HONKED`). Mod values: `living_world.traffic_horn {[<driver record> or all] = {blocked_time, obstacle_time,
approach_speed_kmh, approach_seconds, enabled_chance, blocked_long_chance}}` (read at spawn) and
`ped_brain.run_from_honker_distance` / `run_from_honker_speed`.

**Engine choices.** "Inside the standoff behind a lead" is `gap <= min_gap + stop_margin` (our follower settles at
`min_gap`, retail stops exactly at the standoff); the junction answer is applied before the lead and obstacle
checks (retail's order between FollowingLane and the limiter step is not traced); RunFromHonker uses the car's
forward axis (retail reads `[car+164]+144`, velocity or an axis, open), sets the route every frame like retail's
path request, and skips the navmesh cast the flee legs use.

**Verification.** skate-core horn tests (`a_car_stuck_behind_a_lead_honks_after_the_blocked_time`,
`an_obstacle_gets_the_approach_horn_then_the_long_one_with_a_notify`, `a_disabled_horn_and_the_junction_wait`,
`the_percent_roll_matches_retail_bounds`), follower test
`a_car_stuck_behind_a_standing_car_honks_but_a_red_light_queue_does_not`, `the_ped_runs_ten_metres_sideways_on_its_own_side`,
`run_from_honker_posts_its_intent_and_keeps_the_honker`; skate-mods `traffic_horn` validation. Not heard in game yet.

**Open.** What packs `+3420` into the audio list entry (`sub_82C485A8` caller); the car vector RunFromHonker reads;
the steering type's speed quantisation (6 stays 6, 3 becomes 2 in type 0); `IsOnRoad` / `IsOnIntersection`
(absent, answer false, so Wander's on-road test never holds the honk back); the driver bits' other uses (0x04, 0x10).

## Skitching step 1: car grab splines in the vehicle data (2026-10-09)

**Retail [code + data] (`.local/research/npc/b34-car-grab-splines.md`, `b35-car-definition-resource.md`; main ran the
parser over the disc).** Car grab splines are authored per model: RW4 GRABDATA (`0x00EB001F`) in the first part arena
of the recipe that has one (`82C2A8C8`), registered as grab records of type 1. On the disc 17 of the 18 car recipes
have exactly one rear-edge spline (Bezier chains of 12 to 24 control points at z -1.7 to -3.1, direction (0, 0, -1));
`reda_car` has none.

**Change.** `tools/asset_pipeline/living_world_vehicles.py` `recipe_grab_splines` writes `vehicles.json
models.<record>.grab_splines = [{points, direction, bounds, flags}]` (model space, the GLB's frame; exporter VERSION
2); skate-game `VehicleModel.grab_splines` (`CarGrabSpline`), a spline that is not whole Bezier segments is dropped.

**Verification.** Python `GrabSplines.test_the_first_part_arena_with_grabdata_gives_the_splines` (+ the grab_data
tests); skate-game `grab_splines_and_driver_horn_values_load`. The dev install's vehicles.json got the field by hand
(same function) until the next setup refresh.

**Open (next steps, `.local/research/npc/b46-skitch-port-map.md`, `b47-skitch-frame-transforms.md`).** The grab
query's provider slots: `82760508` treats the mode as a bitmask (0x02 the provider at scene `+4088`, 0x04 the one at
`+4084`; our offboard port is the 0x04 one); which provider enumerates cars and its gates are not decoded yet, nor
whether a car record carries an assembly. Then the riding skitch query, state 104 (registry, dispatch, a no-op exit
like retail's `82B61BB8`, its own publication) and the car side.

## Skitching step 4a: the state-104 frame step, and the GRABDATA header fix (2026-10-09)

**Retail [code] (`.local/research/npc/b32-skitch-frame.md` section 1, `b47-skitch-frame-transforms.md` section 1).**
`sub_82D48148` re-orients the grab record to the board, keeps a three-frame history of the grab edge (direction,
up, side, midpoint), measures the skater's along-edge coordinate in the previous frame and builds the axis point
from it (b47 corrects b32: no relative transform there), maps the current grab point into the previous pose with
`inv(current) * previous`, and derives the car velocity at the grab location (two-frame difference), the tow speed,
the distances and rates the tow spring reads, the side target (0.5 m along the side axis) and the "tows fast" gate
(`Hash_1F85F500908C5E17`, 3.0).

**Change.** skate-core `riding::skitching::frame` (`FrameInput`, `FrameState`, `FrameOutput`, `FrameSettings`,
`step`; `to_world` / `to_local`); the car-motion frame (256 = 448) is kept for the hand targets, 384 (identity in
retail) is left out. `tools/asset_pipeline/grab_data.py`: GRABDATA header +12 is the count of enabled entries (byte
+70 non-zero, the length of the direction array), not the entry count again; 3 parkassets props with disabled
entries failed before (main re-ran all 102 with GRABDATA: all parse).

**Engine choices.** A zero vector normalises to zero (retail's epsilon vector at `0x830BD350` is not read).

**Verification.** skate-core `a_standing_car_gives_the_axis_point_and_no_tow`,
`a_car_pulling_away_tows_and_the_grab_point_is_compared_in_the_previous_pose`,
`a_board_facing_the_other_way_swaps_the_endpoints`; Python grab_data tests (disabled-entry header case added).

**Open.** Not wired: the state-104 handler (pre-step gate, sub-mode, along chain, forces, publication) and the car
provider. The grab providers are now known from `82857FB0` (b49, in progress): scene `+4084` (mode bit 0x04) is the
type-2 world-object provider (or the DMO manager on the alternate path), `+4088` (bit 0x02) the vehicle provider
(vtable `0x82322514`: `82C36068` single, `82C35B98` box, `82C35840` radius), so cars answer only mode 255 queries.

## Skitching step 2: cars in the grab scene (2026-10-09)

**Retail [code] (`.local/research/npc/b49-grab-providers.md`; main checked the provider vtable against the image and
the gates in `82C35B98`).** `82857FB0` fills the grab scene's two provider slots: `+4084` (query mode bit 0x04) gets
the type-2 world-object provider (or the DMO manager on an alternate path), `+4088` (bit 0x02) the vehicle provider
(vtable `0x82322514`: `82C36068` single record, `82C35B98` box query, `82C35840` radius query). `82760508` treats the
query mode as that bitmask, so only mode 255 (the riding skitch query) reaches cars. The vehicle box query gates each
car by a sphere (query radius + 15 m around the car's origin) only, walks the car's spline list (`car+200`, count
`+212`), needs byte +68 > 1 and +70 != 0, a segment hit (`82ADD910`, segments of 1e-5 or less skipped) and the car's
assembly (`car+172` chain), then builds a type-1 record (`82585F58`) with the car matrix and velocity and keeps it if
CanGrabSpline passes.

**Change.** skate-core `grab_scene`: `Provider::Vehicle`, `query` takes the mode as a provider bitmask (cars first,
then the existing world-object loop), the registry insists car records are type 1. skate-game: `Registry::set_cars` /
`GamePhysics::set_grab_cars`, `living_world::vehicles::push_vehicle_grab_splines` (each fixed tick before physics:
every car's splines with its current transform and velocity; ids `CAR_GRAB_TAG | serial`, spline ids
`CAR_GRAB_TAG | serial << 3 | index`), `CarGrabSpline.flags` into the record's word 60 [inferred].

**Engine choices.** NOT RETAIL YET: the car's assembly is a stand-in carrying the car's id (the `car+172` object is
not identified); spline ids are stable per car instead of retail's global counter; the box query and CanGrabSpline
are applied as in the world-object loop (whether the skitch query takes the box or the radius path is open).

**Verification.** skate-core `cars_answer_only_the_vehicle_bit_and_world_objects_only_the_other`,
`a_car_needs_its_assembly_and_must_be_within_the_sphere`; skate-game
`a_car_enters_the_grab_scene_with_its_rear_spline_in_world_space` (115 living_world / offboard tests pass).

**Open.** The riding skitch query (`82D39BB8`) that submits mode 255 and latches a car, state 104's handler, the car
side (held flag, tow reaction), flag `0x83082929` (which maps take the DMO path).

## Skitching step 3: the riding skitch query (2026-10-09)

**Retail [code] (`.local/research/npc/b33-skitch-wiring.md` section 2; main read the ground update branch, the box
flag of `82D2E250` and the query constants).**
- The ground update `82D37C88` branches on Processed `+2476` bit 22 (the grab input): set, it runs the skitch query
  `82D39BB8`; clear, it invalidates the grab owner (`82D749D0`). Our ground update always invalidated.
- The query box is `82D2E250(frame +192, (0, 0.86, 0), (0.48, 0.45, 2.0), out, true)`: with the flag set the box
  keeps the frame's own forward (row 2, not flattened), right = row 0 flattened and normalised, up = forward x right,
  placed at the origin + forward x (offset z + extent z) + up x offset y (so it reaches 2 m ahead).
- When the owner's validated results are ready (`+12836` bit 0x40) the latch `82D39D98` walks them: orients the
  endpoints, CanGrabSpline, the re-grab cooldown (Processed `+2848` / `+2592`), the time to the spline
  `max(0, (dot(P - pos, fwd) - 1.0) / max(dot(vSkater - vCar, fwd), 0.5))`, a bind per candidate, and the latch
  (`+2729`, `+2560` type, `+2564` id) on the first one under 0.1 s.
- It always submits `82D74200(owner, 255, pos, box, ...)` with margin 0.25 (`0x820991A0`) and the angles 60 / 30
  degrees (`0x8209919C` / `0x82099198` x the degree constant `0x8206D110`), cap 5. Mode 255 reaches the cars.

**Change.** skate-core `ground_sync::skitch_bounds` (the flag-true box), `SkitchQuerySettings` gains the limits and
cap, candidate ids are u32 (+192). skate-game `ground_runtime::skitch` (`query_shape`, `latch`, `query`), the ground
update calls it instead of the invalidate while bit 22 is set, `GroundSettings.skitch` reads the reach and latch time
from `physics_state_skitching` (retail values as defaults), the grab owner exposes its validated records.

**Engine choices.** The latch is computed but NOT written yet (`skitch::LATCH_ENABLED = false`): it would make the
selector request state 104, which the registry still refuses (the transition returns an error). NOT RETAIL YET:
type-2 world objects are not latched (their box `82D2CF68` is not decoded), the spline point is the closest point of
the endpoint chord, `+2668` stays 0 and `82D91298(state+28, 2)` is not called.

**Verification.** skate-game `the_box_reaches_two_metres_ahead_along_the_frame_forward`,
`a_close_car_latches_and_a_farther_one_only_binds` (time 0.5 s binds, 0.05 s latches, the cooldown skips the last
car only), `world_objects_are_not_latched_yet`. Not run in game (the riding grab now submits a query each frame;
regression check pending).

## Skitching step 4b: the hold target and the release impulses (2026-10-09)

**Retail [code + data] (`.local/research/npc/b52-skitch-target-impulses.md` sections 1-2; main checked the impulse
vault keys).** `82D4B8C0` builds the hold target (the side-shifted previous frame at the hand's along coordinate),
its horizontal direction (the tow spring's next axis), the yaw between the grab point and the target, and the lean
yaw (`940`); `82D4BCF8` turns the skater's facing toward the target at up to 180 degrees per second, scaled by the
larger of a turn-in curve over the skitch time and a gain over the tow speed. On release `82D4B1D0` pulls in
(rate 856 - 2.0, never outward, never faster than the tow speed) and `82D4B378` pushes off (856 + 2.5), one frame of
force tag 6.

**Change.** skate-core `riding::skitching::target` (`step`, `pull_in_force`, `push_off_force`, `TargetSettings` with
the vault values, `wrap`, `yaw`).

**Engine choices.** The signed angle `8286CD88` is the standard angle a -> b about the axis (internals not decoded);
`6FB7A3D992163663` (the speed gain) is not in the collections by hash, the stored graph between the other two keys
(`HeadingAdjustVsSpeed`) is used for it (unverified).

**Verification.** skate-core `facing_the_target_needs_no_yaw`, `a_target_to_the_side_turns_at_most_the_rate_cap`,
`release_impulses_pull_in_never_outward_and_push_off`. Not wired: state 104's handler is next.

## Skitching step 4c: the hold step and the release (2026-10-09)

**Retail [code + data] (`.local/research/npc/b53-skitch-hold-step.md`, b51 section 2; main checked the 1344 masks in
`82D49580`: the per-frame clear is 0x60, b31's 0x18 was wrong).** `82D49D70` keeps the hand's along target (904):
while 1344 bit 0x08 is set it follows the clamped position, or ratchets with a stick past +-0.5; the bit clears when
the skater rests near the posed hand and sets again when the hand is 0.2 m off or after 2 s; the posed hand (908)
chases the target with a step of 0.25 x the error, changing by at most 0.03 per tick. `82D49580` sets the release
bits per sub-mode (1 pull in, 2 / 3 push off; 2 -> 4 clears them, 3 -> 4 keeps push off). The tail `82D4AFB8` lets go
(sub-mode 4) without the grab input or, in sub-modes 1 / 2, with both hands off once the 0.5 s hold timer ran out;
in sub-mode 4 it queues the impulse on the first frame, ends the state after three, and refills the 0.5 s re-grab
block.

**Change.** skate-core `riding::skitching::hold` (`HoldState::{reset, step, mode_bits, tail, released}`,
`HoldSettings` with the eight vault values and the release frame count, `Impulse`).

**Verification.** skate-core `the_posed_hand_eases_toward_the_target_in_capped_steps`,
`the_stick_ratchets_the_target_and_rest_clears_the_bit`, `release_queues_one_impulse_then_ends_after_three_frames`.
Not wired: state 104's handler (registry, dispatch, enter / exit, forces, publication) is next.

**Open.** The meaning of 2476 bit 22 in the tail (the grab input, read as such), why reset skips 872, the vector at
1168 in the tail.

## Skitching step 4d: the along-the-bumper hand chain (2026-10-09)

**Retail [code + data] (`.local/research/npc/b30-skitch-pull-release.md` section 4, `b31-skitch-submode-hold.md`
section 2).** `82D48C98` low-passes the car's acceleration along the grab edge (0.9 / 0.1), turns it into a hard event
past 40 and an excess over a dead-band curve, adds the stick's push (10) or, without stick or event, a velocity servo to
rest, integrates the hand's velocity (capped at 1, or the stick gain against an opposing event; unbounded in the
event's direction) and moves the hand target along the edge, clamped to the range and snapped onto the latched edge
point when it crosses it.

**Change.** skate-core `riding::skitching::shimmy` (`ShimmyState::step`, `ShimmySettings` with the five vault values).

**Open.** The car acceleration `a` is an input: its derivation from the frame history (b30's reading predates b47's
frame layout) is being re-read (b55), as are the tow spring's body-height term and the stick axis 924.

**Verification.** skate-core `the_stick_moves_the_hand_along_the_bumper_at_most_one_unit_per_second`,
`a_hard_car_acceleration_is_an_event_and_the_gate_resets`.

