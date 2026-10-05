# 26: Living world: NPC skaters, pedestrians and traffic

Branch: `world/living-world` (built on all the audio work, #32, which it publishes sounds into). One upstream PR
for all of it (user, 2026-10-04: "this work shall be one PR"; traffic added the same day: "add this to the living
world work"). Details: [`living-world/`](living-world/) (designs and the data formats of milestone 1).

## Problem

Skate 3's free skate is full of life: AI skaters riding lines around the player, pedestrians walking and reacting,
traffic on the roads. The engine has none of it. The audio side already waits for it: #32 ports the retail traffic,
ped and NPC-skater sounds and exposes `TrafficAudio` / `PedAudio` / `NpcSkaterAudio`, but nothing publishes them.

## What retail does

Reference: the TU3 build through skate3recomp (by @mchughalex, built on the rexglue SDK; Xenia), the disc data read
in place, guarded hook runs of the recomp and the user's own play sessions. The retail code is the source of truth
(user: "the recomp doesnt seem to have the peds and npc skaters 100% right so use the retail code as the source of
truth"); the recomp only validates. Addresses are TU3; no game code or data is copied.

- **NPC skaters.** The manager `sub_8245BA28` (60-tick cycle) keeps 3 ambient skaters (5 AI in all, 7 skater slots
  shared with online players), spawns at the start of an unused recorded human line 60 to 90 m away and culls
  beyond 120 m. Each NPC is a full skater driven by an `AIController` (skater + 1828). The lines are 60 Hz
  recordings: 1,691 unique lines (3,891 per-tile copies; DownTown 760, University 508, Industrial 423; 208.7 km),
  none in the parks. The pool (`characters_marquee` byte +9) is the 33 pros, recruited teammates and community
  skaters, not `ambient_skater_01..09`. Details: [`living-world/npc-skaters-design.md`](living-world/npc-skaters-design.md).
- **Pedestrians.** The census `sub_826B71F0` caps the population from painted census areas (15 per sub-area, 20 per
  district; pool 31), spawns in a 50 to 60 m ring (at 45 km/h; 50 / 80 / 90 m with offset 20 at 80 km/h) and culls at
  70 m. 51 ped models and 26 hand props in `livingworld.big`, one animation bank (`PedestrianSkeletonPres.abin`, 462
  clips, 50 bones), behaviour from state graphs. Knock-down above a contact of 3.0 (6.0 flagged); get-up 2.0 /
  3.167 s; no ragdoll. Per-entity plugin odds (trash bin 0.5 to 0.65) and hand-prop odds are data. Details:
  [`living-world/peds-design.md`](living-world/peds-design.md).
- **Traffic.** The same census drives vehicles (spawn `sub_826B9B90`, cull `sub_826BAAB8`; default caps DownTown 30,
  Industrial 25, University 10). Cars follow the lanes of the road network (`0x00EB0013`) through a state graph
  (FollowingLane, PassingIntersection, Impatience, ChangingLane, PullingOver, StayingParked) with a speed planner
  (`sub_82C3FA08`) and a manoeuvre decider (`sub_82C41CD0`). Lights: green 7 or 8 s, amber 1 s, all-red 0.5 s per
  direction. In the recomp: median 5.8 m/s, p90 14.9 m/s; cars stop and queue for a skater in the lane; a skitched
  car speeds up to about 16.8 m/s. Cars are kinematic (the AI integrates speed along a lane: `+3408` commanded
  acceleration, `+3412` speed, `sub_82C3FF38`); they spawn in an 80 to 100 m ring (2 tries, at most 1 spawn per
  tick) and cull at 110 m; no traffic spawns in zombie mode or online; there are no patrol cars in the Skate 3
  districts. The horn decider `sub_82C40660` writes the horn state the audio reads. Skitching is a per-frame latch on
  the car (`+4403` bits 0x80 / 0x40, `sub_82C34648`, `sub_82C34CD0`); our `skate-core` already has the skater's
  Skitching state (104). 18 car recipes with colour palettes in `livingworld_models`. Details:
  [`living-world/vehicles-design.md`](living-world/vehicles-design.md).
- **Online:** nothing ambient spawns (census spawn pass and AI count gated by the online flags; culling runs).
- **Free Play** (mode 3, `sub_82706B40`): Traffic / Pedestrians / A.I. Skaters options scale the census caps
  (`sub_826B7010`, `sub_826B8A28`) and switch the AI skaters (`sub_8245C548`); career free roam has no switch.

## Change (milestone 1: data)

- **Setup group `livingworld`** (optional; own fingerprint; the existing groups' fingerprints are unchanged, so an
  existing install only runs the new group). On a content error it removes `living_world/`, writes
  `living_world-availability.json` and the world runs empty; setup reports it. One vault conversion feeds both
  exporters (about 40 s on the disc, 53 MB):
  - `tables.json` (29 living-world vault classes, inheritance applied, readable names where known plus a
    `field_names` map to the `Hash_*` names), census grids per district (4 m cells) and `census.json`,
    `roads.json` / `roads.bin` (segments; road objects kept verbatim; milestone V0 decodes the rest and corrects the count to 76),
    `waypoints.json` (74 groups, 394 waypoints), `navmesh.json` (inventory only), `models.json` and
    `models/<recipe>.glb` (all 77 recipes from a binary `.recipe` reader; each GLB with the model's skeleton, both
    LODs, all parts and textures). Format details: [`living-world/peds-data.md`](living-world/peds-data.md).
  - `skater_paths/<District>.bin` (the retail line blobs unchanged in a small container, deduplicated by line id like
    retail's path manager), `skater_paths/index.json`, `skater_profiles.json` (193 profiles, 87 characters, the
    42-character pool). Format details: [`living-world/skaters-data.md`](living-world/skaters-data.md).
- **`skate-data::aipath`**: the line format, the pack container and the per-district dedupe.
- **`native_roster`**: the NPC pool (`npc_pool`) and teammate binding (`bind_teammates`): the disc has no look for
  `teammate_NN` (retail reads it from the save), so a recruited teammate binds to a customiser look in
  `settings/living_world_teammates.json`; unbound = not recruited = never spawns.

Files: `crates/skate-data/src/aipath.rs`, `crates/skate-data/tests/aipath_data.rs`,
`tools/asset_pipeline/{living_world,living_world_models,living_world_skaters}.py` (+ tests), `versions.py`,
`install.py`, `group_receipts.py`, `asset_exports.py`, `native_roster.py`, `tools/test_setup_assets.py`.

## Change (milestone 2: population core)

One census engine for every ambient kind, shared by NPC skaters, pedestrians and vehicles (props and dynamic
objects later). Pure rules in `skate-core::living_world` (no ECS, no I/O), readers in `skate-data::living_world`,
a minimal Bevy plugin in `skate-game::living_world` that emits spawn / despawn decisions as messages. No bodies or
rendering yet (later milestones consume the messages).

What retail does, checked in the code for this milestone (tags [code] / [data]; addresses TU3):
- **Census tick** `sub_826B71F0` runs one living-world type per console tick in rotation (peds 0, vehicles 1, DMOs
  2, props 3), each a cull pass then a spawn pass [code]. So each census kind gets a pass every 4 ticks.
- **Census circle**: the `livingworld_census_ranges` sets lerped by the player's speed `|v| x 3.6` km/h
  (`sub_826B7D60`, `0x822F8628`); peds 50-60 m ring / 70 m cull at 45 km/h, 50-80 / 90 with offset 20 at 80 km/h;
  vehicles 80-100 / 110 [data]. Cull = 3-D squared distance to the circle centre (`sub_826BA8B0`) [code].
- **Spawn pass** `sub_826B9940` (peds) / `sub_826B9B90` (vehicles): gated by the manager byte, `0x83082929` and
  `IsOnline`; 2 attempts / at most 1 spawn; initial populate 6000 attempts / 600 spawns in an 8-80 m ring
  (`0x82099250`, `0x820E5748`) [code]. Ring point (`sub_82E17508`): direction from two uniform draws in
  [-0.5, 0.5) normalised, radius uniform in [inner, outer] (linear), heading = u32 x 2pi / 2^32 (`0x822F9598`) [code].
- **Cap at the point** `sub_826B8A28`: the record painted in the census layer at the spawn point, max population x
  density (Free Play scale, truncated; skipped in zombie mode). **An unpainted point has no record and cap 0** for
  peds and vehicles (the lookup only resolves a record when the layer query hits) [code]; this corrects the
  milestone-1 note "retail falls back to `default`", which only holds for the other census types.
- **Budget and category** `sub_826B8B88`: cap 0 = no spawn (even in zombie mode); count >= cap = no spawn unless the
  zombie cheat is on; category roll `rand() % 100 + 1` against the cumulative weight x 100 (`0x820ED57C`), no hit =
  no spawn (DownTown traffic weights sum to 0.875) [code, data]. Ped pool 31 [code].
- **Free Play** (mode 3): `sub_826B7010` clamps `+336` / `+332` to 0..1 as the ped / vehicle density; below 1.19e-7
  the whole kind is culled at once [code]. A.I. Skaters `+340` off: no spawns (`sub_8245C548`) and the per-skater
  checks despawn every ambient NPC (`sub_8245A9B8`, the `r25` gate) [code].
- **Ambient skaters** `sub_8245BA28`: 60-tick cycle (cull 0, pool 15, spawn 30, checks 3-12 / 18-27 / 33-57),
  desired 3 (0 online), AI cap 5, 7 slots shared with players, spawn at the start node of an unused line 60-90 m away
  (`0x821FF080`), cull 120 m 3-D or 1000 m height (`0x82256FE0`, `0x82256FE8`) or excess [code]. Line scorer
  `sub_8245C018` (lowest wins): reject when any skater is within 5 m (`d^2 < 25`, `0x8209994C`); nearest skater
  closer than 10 m (`d^2 < 100`): 600 below 0.5 m, else `(10 - d) x 300 x 600` (`0x821963E4`, `0x822F92C0` =
  0x4395FFFF, `0x820994A8`); plus `rand() % 400`; online both terms are skipped [code]. Character scorer
  `sub_8245B068`: 500 if it fits a nearby line + 5 per fitting line + `rand() % 160`, highest wins [code].

Change:
- `skate-core::living_world`: `LivingWorld` (one session: config, seed, console clock, three kinds),
  `PopulationConfig` (code constants as defaults, `config::retail` with labels and addresses; census ranges and
  caps only from data), `census` (circle lerp, grid lookup, ring point, cap, category roll, cull), `skaters`
  (`SkaterWorld` trait, line and character scorers, pool, slots), `population` (rosters, `Scorer` trait,
  `pick_lowest` / `pick_highest`, the census pass), `clock` (retail's fixed 60 Hz world step from any engine step; first written as 30 Hz, corrected in
  milestone V2: the census and the ambient skater manager run on the 60 Hz world step, `sub_8285C928` ->
  `sub_82859E70` bit 0 -> `sub_8245A7E8` / `sub_826BDB50`, fixed 1/60 s at `0x820849C8` [code]; the lights in the
  same update took 480 ticks for 8 s [trace]; the 60-tick skater cycle is 1 s, each census type passes 15 times
  per second, the replay tier advances one 60 Hz recording frame per tick), `rng`
  (seeded PCG32, derived sub-seeds; no global state).
- `skate-data::living_world`: `parse_census_grid`, `LivingWorldTables` (census records resolved through the
  category groups, ranges, `apply_to` the config), `skater_characters` (free-roam pool; unrecruited teammates
  left out), `skater_lines` (ambient lines, start node, heading).
- `skate-game::living_world` (`LivingWorldPlugin`): `LivingWorldSettings` (per kind enabled / density, ambient
  skater count, Free Play options, zombie, net role, seed, debug log; defaults = retail), `PopulationState`,
  `LivingWorldObservers`; `FixedUpdate` after physics: load the district's data on map change, gather the local
  skater's deck position / velocity and the multiplayer state, run the due console ticks, write
  `LivingWorldSpawn` / `LivingWorldDespawn`. `SKATE_LIVING_WORLD=0` turns it off, `SKATE_LIVING_WORLD_DEBUG=1`
  logs a summary every 5 s.

Multiplayer seams (user, 2026-10-04: "build everything with multiplayer support, just dont add the multiplayer
yet, we can have it ready for that to be added however."): a decision is a pure function of (config, seed, tick,
observers, slot budget); it runs in one place (`NetRole` Standalone / Host; a Client never decides and mirrors
records with `PopulationState::apply_records`). The core takes a list of observers (culls use all of them; the
spawn pass rotates through them; one observer = retail). Every entity has a stable `LivingWorldId` (kind + serial,
never reused in a session); records carry kind, id, tick, position, heading, the entity's own seed and the choice
(census record + category, or line id + character + slot) and serialise as `WireRecord`. Rosters are `BTreeMap`s;
no frame-time dependence. No transport or replication code. Retail default stays: nothing ambient spawns online.

Moddability: every rule value is a public config field (code defaults, data ranges), settings are one resource,
ids are stable, decisions are messages any system (or a mod bridge) can read. `sdk.living_world` population
surface (planned, not built in this milestone): `set_density(kind, scale)`, `set_enabled(kind, bool)`,
`set_ambient_skaters(n)`, census record / range patches through `living_world.json` (content overlay), events
`spawned` / `despawned` {kind, id, reason, record, category | line, character}; on mod disable the settings return
to `LivingWorldSettings::default()` and mod-spawned entities are despawned (`DespawnReason::External`).

Files: `crates/skate-core/src/living_world/{mod,census,clock,config,population,rng,skaters,tests}.rs`,
`crates/skate-core/src/lib.rs`, `crates/skate-data/src/living_world.rs`, `crates/skate-data/src/lib.rs`,
`crates/skate-data/tests/living_world_data.rs`, `crates/skate-game/src/living_world/{mod,tests}.rs`,
`crates/skate-game/src/{main,app}.rs`.

Verification (milestone 2):
- `cargo test -p skate-core --release --locked living_world`: 24 tests (circle lerp, ring, caps, category roll,
  initial populate, one spawn per pass in rotation, cull radii, unpainted = none, vehicles 80-100 / 110 / 30, Free
  Play scale and zero removes all at once, online spawns nothing but culls, zombie, skaters 3 / phase 30 /
  60-90 m / 5 m rule / 120 m cull / Free Play off / online excess / shared slots, same inputs same stream, ids
  unique and a client mirror equals the host).
- `cargo test -p skate-data --release --locked --lib living_world` (2) and, with `SKATE3_ASSET_ROOT`,
  `--test living_world_data` (4): caps and ranges exact on the export, the three grids parse, a DownTown run keeps
  15 peds max and 30 cars with every spawn in its ring, 38 pool characters and 1,691 lines.
- `cargo test -p skate-game --release --locked --bin skate3rust -- living_world::` (4): a fake player drives a
  path in a headless app; counts, radii, determinism at 60 and 144 Hz, wire round trip, client role, online,
  settings.


## Change (milestone 3: NPC skaters, replay tier)

Ambient NPC skaters are visible in free roam: the population's skater spawn records become skaters that ride the
retail recorded lines kinematically (the "replay tier"; the simulated tier with the full skater physics follows).

What retail does, checked in the code for this milestone (TU3):
- An NPC is a full skater steered along its line by the `PathController` (ctor `sub_824685F0`). At a node that
  carries a branch group the controller picks where to continue (`sub_8246BEE0`): stay on the line, or a branch
  target that exists, is not in use and is valid. The lowest score of `sub_8246C230` wins, the first on ties; there
  is no random draw and the branch record's f32 is not read [code]. A candidate is rejected when the line has no
  node after it, when the next node lies 50 deg or more off the skater's forward (`0x822F91B0`), or when an airborne
  or event node lies within one node (`sub_8246C4F8`). Score = angle x 572.958 + (offline) 1024 per other AI skater
  on the same line within 5 nodes (`sub_82456A38`) + min(30 x the player's distance to the line, 1500)
  (`sub_8246C5D8`) + 1000 for lines with flag bits 0..2 all set + a skill term. A taken branch starts at the target
  node nearest the skater among the 3 before it (`sub_82455BB0`) [code].
- Line nodes carry the skater and board orientation as quaternions x, y, z, w with +Z forward [data: 81 % of
  moving nodes]; branch groups never sit on a line's last node [data].

Change:
- `skate-core::living_world::replay`: the line cursor (node index + frame in the segment at the 60 Hz recording
  rate; any engine rate gives the same cursor), pose / velocity / heading / orientations / node flags and events,
  the phase (rolling, crouched, air, air trick, ground trick, off board), the retail branch choice, and a mirror mode
  that replays recorded branch decisions. `skate-data::living_world::replay_line` converts a decoded line.
- `skate-game::living_world::npc_skaters`: one entity per skater spawn record (`NpcSkater` with the stable
  `LivingWorldId`, character, slot, seed, voice; `NpcReplay` with the cursor). The cursor stays at
  2 x (population tick - spawn tick) recording frames, so an NPC's state follows from its spawn record, the tick and
  its branch records. Its position goes back into the population (culls, the 5 m rule). Collision: a kinematic
  capsule and board box join the skater solve through the network proxies (like a mod's solid), so the player bumps
  into NPCs. Look: the character's native roster GLB (the customiser's and online players' files), a mod override
  (`NpcSkaterLooks`), else the stock skater; bound to the stock skeleton like a remote player. Puppet animation: one
  stock clip per phase from the player's evaluator; the root takes the recorded position and skater orientation.
  Audio: `NpcSkaterAudio` with a lite state (speed, air, ground trick as a grind) and the character's voice, so
  #32's NPC board sounds and speech play. Events: `NpcSkaterEvent` (spawned, despawned, node event, branch, line
  end). Debug: `SKATE_LIVING_WORLD_DEBUG=1` logs the NPC count and the nearest NPC (distance, line id, node, phase,
  speed) every 5 s.

Replay-tier simplifications until the simulated tier (documented, not retail): positions and timing come from the
recording, not from steering (`AIPhysicsInput`); the branch is evaluated once when the node is reached; the
obstacle-list rejection is not modelled; NPCs do not react to the player (the proxy is infinite mass, no bails);
tricks show one clip per phase (a grind, slide or manual is one 50-50 clip; the recording does not say which), the
stock graphs do not run; the end of a line with no branch taken despawns the NPC (retail behaviour not decoded).

Files: `crates/skate-core/src/living_world/{replay,replay_tests,mod,clock,population}.rs`,
`crates/skate-data/src/living_world.rs`, `crates/skate-data/tests/living_world_data.rs`,
`crates/skate-game/src/living_world/{npc_skaters,npc_tests,mod,tests}.rs`.

Verification (milestone 3):
- `cargo test -p skate-core --release --locked living_world::replay`: 7 tests (60 Hz timing and interpolation,
  frame-rate independence at 30 / 60 / 64 / 144 / 240 Hz, phases from flags and trick events, line end, every term
  of the branch score against the code constants, branch taken / stay / in-use target, client mirror equals host,
  orientation order).
- `SKATE3_ASSET_ROOT=<export> cargo test -p skate-data --release --locked --test living_world_data`: every one of
  the 1,691 lines rides end to end with the retail branch choice (1,331 branches taken, average ride 14.6 s; recomp
  NPC median life 13 to 28 s), every branch target resolves, no frame moves more than 3 m, deterministic.
- `cargo test -p skate-game --release --locked --bin skate3rust -- living_world`: 9 tests; new: NPCs ride their
  lines and each position equals a cursor rebuilt from the spawn record and the tick, 3 NPCs max, line ends despawn,
  audio velocity and voice published, same result at 60 and 144 Hz, NPC entities go with the population, proxy /
  audio state / clip table; with `SKATE3_ASSET_ROOT` every puppet clip evaluates on the stock banks.

Multiplayer: an NPC is reproducible from its spawn record, the console tick and its branch records
(`Decider::Mirror`); a client never decides (`NetRole::Client` mirrors). No transport.

Moddability: lines are keyed by retail id in a shared map a content overlay can extend or patch; looks by character
key (`NpcSkaterLooks`); NPC events are messages; spawn / despawn go through the population (stable ids). The
`sdk.living_world` NPC surface is designed below (open items) and comes with the mod milestone.
```

## Change: peds milestone M2, the ped body

Pedestrian spawn records become visible, animated peds with footstep audio.

What retail does, checked in the code and data for this milestone (TU3):
- The entity inside the rolled category: `sub_826B8B88` draws `sub_826BB058` (world RNG u32 x 2^-32 as f32,
  `0x822F88F4`) and takes `trunc(draw x 100) % count` (`0x820ED57C` = 100) of the category's `entities` array
  (`sub_8269B040`); an empty list spawns nothing [code]. The model's tint pair comes from one rand `r`:
  `tints_a[r % na]`, `tints_b[r % nb]` (`sub_827B4170`) [code].
- `PedestrianSkeletonPres.abin`: 462 VBR clips, a 50-bone rig with trajectory and 10 parts; the clips carry the
  first 6 parts (bones 0..=26), additive over the `PEDESTRIAN_RIG_TPOSE` pose record; fingers, face and twist
  helpers have no clip data [data]. Clip attributes `LEFTTOEDOWN` / `RIGHTTOEDOWN` / `LEFTHEELDOWN` /
  `RIGHTHEELDOWN` (phase windows) and `BODYFALLTYPE` (values) are the foot plants and body falls the ped audio
  reads [data]. The walk clip's trajectory moves 1.325 m/s (recomp walking median 1.30).
- `livingworld_entity_animation` names its clips by vault hashes of the motion graph's logical names
  (`FwdWalkCyc` = `Hash_4AA12E0083F10739`); `tAnimAttributes.anim` is a byte offset into
  `skatercollections.bin`'s string pool [data]. The motion graph gives the blends and exits: idle 0.1 (cycle
  swap 0.5), Stand2Walk 0.25 (exit 0.03 s before its end), walk 0.1, Walk2Stand and turns 0.15 (exit 0.1 s
  before the end; Walk2Stand inside a walk branch window), left turns mirror the right clip [data].
- The 51 ped GLBs' 39 joints are all rig bones by name; their bind skeletons match the animation reference to
  1 cm [data].

Change:
- Setup: `living_world_anim.py` adds `anim_name` / `anim_b_name` beside those offsets (tables otherwise
  identical; only the `livingworld` fingerprint changes).
- `skate-core::living_world::peds`: `choice` (catalog, the retail entity index, tints, overrides, `PedLook`),
  `anim` (clips, rig, the player: remap, blend, mirror, root motion, foot channels, the locomotion states,
  `TestPath`), `match_bones`.
- `skate-data::ped_anim`: `PedBank` (rig, reference pose, partial-part clip decode with attributes),
  `PedTables` (categories, entities, models, animation sets).
- `skate-game::living_world::peds`: one entity per pedestrian record (`Pedestrian`, `PedBody`, `PedAudio`), the
  GLB bound to the rig by name, bones without clip data following their parent with the bind offset, ground snap,
  root motion, `PedAudio.feet_down` / `body_fall` / voice, `PedLooks` overrides, `PedEvent`, debug readout with
  `SKATE_LIVING_WORLD_DEBUG=1`.

Simplifications until later milestones (documented, not retail): no navigation (M3): a ped idles, walks a few
metres straight, stops and turns round (`TestPath`); the motion graph is not run (M4): the locomotion subset is
hand-wired from its data, the crossfade is linear over `blendTime`, the idle cycle swap is a uniform draw per
wrap; LOD switches LOD0 / LOD1 at the model's 45 / 55 m pair (meaning unconfirmed, `sub_827C1188` not read),
animation runs every tick for every ped; tints are kept but not drawn (the shader mask is not decoded); the group
model child is a seeded uniform pick (code not found); the `granny` set names `GRAN_WNDR_*` clips no shipped
bank holds, such peds use the `default` set.

Files: `crates/skate-core/src/living_world/peds/{mod,choice,anim,tests}.rs`, `crates/skate-data/src/ped_anim.rs`,
`crates/skate-data/tests/ped_anim_data.rs`, `crates/skate-game/src/living_world/{peds,peds_tests}.rs`,
`tools/asset_pipeline/{living_world_anim,test_living_world_anim}.py`, registration lines in the three
`mod.rs` / `lib.rs`, `living_world.py`, `versions.py`.

Verification (peds M2):
- `cargo test -p skate-core --release --locked living_world::peds`: 11 tests (retail index formula, tints,
  seeded looks, overrides, bone matching, idle / start / walk / stop, walk speed at 30 / 60 / 144 Hz, mirrored
  turns, determinism, reference add, idle swap, test path).
- `SKATE3_ASSET_ROOT=<roots> cargo test -p skate-data --release --locked --test ped_anim_data`: every clip
  decodes, rig and reference, walk 1.325 m/s, turn -3.07 rad, every census entity resolves a look and its
  clips, all 51 GLBs match the rig (bind vs reference 1 cm).
- `cargo test -p skate-game --release --locked --bin skate3rust -- living_world::peds_tests`: 5 tests (seeded
  looks, despawn, rejection, state = f(record, tick) at 60 / 144 Hz, foot plants, overrides, LOD, followers,
  data-gated load).
- Headless render: a mid-walk pose skinned onto `male_jock_2` (`render_glb.py --pose`) shows a correct stride.

Multiplayer: the look is a function of the spawn record; the body steps once per world tick from the spawn
tick; a client rebuilds the same ped from the same record. Moddability: `PedLooks` (category entity lists,
entity model / animation set, recipe GLB), `PedEvent`; restoring `PedLooks::default()` undoes a mod for new
spawns. `sdk.living_world` ped calls come with the mod milestone.
```

Plan line for doc 26 (milestone table): "peds M2 ped body: done (looks, animation player, foot plants; TestPath
until M3)". Open-questions additions: the 4 parked items below.
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
## Verification

- `cargo test -p skate-data --lib --tests --locked`: all pass (line format unit tests on synthetic blobs).
- Data-gated (`SKATE3_ASSET_ROOT` or `SKATE3_AIPATH_BLOBS`): every line decodes; 3,891 copies / 1,691 unique,
  bounding boxes, 60 Hz ratio, every branch resolves, the pack dedupe.
- Python: every `tools/asset_pipeline/test_*.py` plus `tools.test_setup_assets`: 176 OK, 2 skipped (incl. 16 new
  ped / table / census / road tests and 14 skater export tests; with `SKATE3_DISC_ROOT` the disc test checks 3,891 /
  1,691 / 87 / 193 / 42 and 0 warnings). Two export runs are byte-identical.
- Setup fingerprints: `core`, `hud`, `character`, `environment`, `maps`, `audio` identical before and after.
- Rendered two converted peds (`male_jock_1`, `female_adult_1`): textured correctly.

## Plan (milestones inside the one PR)

Shared core: population engine (`skate-core::living_world`, seeded, Free Play scaling, slots shared with online
players; **done, milestone 2**), crowd renderer and kinematic proxies, `sdk.living_world`. NPC skaters: replay tier, AI, simulated tier.
Pedestrians: body and animation, navigation, behaviour runtime, skater interaction, plugins and hand props.
Traffic (V0 to V8): data (vehicle tables, 18 car GLBs and tints, lanes / junctions / signals), road graph and signal
clock, vehicle census, cars on screen with `TrafficAudio`, the driver (planner, queues, horns, skids, manoeuvres,
parking, alarm), car colliders in the skater solve (roof landings, bails), **skitching** (grab conditions, the car's
skitch state, the skater's side, mod hooks, fixture tests from two recorded sessions), the vehicle mod surface. Then Free Play, zombie mode and the
standing pros, multiplayer (retail default: nothing online; opt-in host-authoritative). The PR description keeps the
checklist.

## Modding

Designed in, not bolted on: retail values are setup data a mod can override by key (tables, lines, profiles,
roads), stable ids, and every system gets a mod-facing entry point next to the engine one with cleanup when the mod
stops. Extends engine modding; there is no retail to match.

## Credits

skate3recomp by @mchughalex (rexglue SDK, Xenia), the reference for how the retail code is used; DumbadsSkate3ModdingTools by Ethanw05 (credits to SunJay,
Dumbad, RenderWareGavin and Tuukkas) for the AIPATH field names, NavPower constants and trigger types, used as a
format reference, no code copied; @andrewnakas' `mx/vehicle` fork as prior work on a (player-driven) vehicle Lua API,
described, not copied.

## Open questions

- Population core (milestone 2): retail reads the census count once per spawn pass (`r23` in `sub_826B9940`), so
  during the initial populate the cap would not bind and only the factory (pool 31) would; the recomp sessions show
  at most 15 peds in 15-cap areas, so we re-read the count per spawn (identical outside the initial populate). Also
  open: the vehicle pool size; the character pool's release rule (`sub_8245B400`; we release the oldest unused
  entry that fits no nearby line, and load at once instead of streaming); the line / profile capability bits
  (profile `+144..+146` unnamed; fit = allowed-skater bit or a matching flag bit); the per-skater stuck despawn
  (`skater+1804` vfunc 76 < 0.2) and the requested-character queue; mode 2 (45 m / 20 m cull, 4-30 m ring); the
  forward offset uses the horizontal velocity direction; spawn points are not snapped to the nav mesh / ground yet.
- Teammate looks at runtime: what writes the binding (recruit menu, save importer or mod).
- AIPATH: branch weight meaning, node flag bit 4, orientation order, `m_ID` bytes 6 to 15; the 38 `ai_skater`
  tunables.
- Roads: decoded in V0 except the meaning of a few raw fields (junction `flag_04`, connector `f32_50`, segment
  `word_56`, quad tags); crosswalks for peds (navigation milestone); NavPower; DMO plugin anchors (benches, ATMs,
  fountains are placed objects, not waypoint streams).
- Ped rig: the converted models carry 39 bones, the animation bank 50; matched by name in the ped-body milestone.
- How retail picks among shared-look entities (`sub_826B8B88`).
- Traffic: lane snapping at spawn, the census +144 reader, which of the 4 signal controllers and which light group
  each junction approach uses (V1), the chassis / secondary tint rule of the `vehicle_chassis` shader (V3), the skid
  flag writer, the skitch grab / attach / release numbers, the vehicle bail thresholds.
  New recomp hooks (skitch, lights, connectors, vehicle bails) and a few short play sessions will measure them.
