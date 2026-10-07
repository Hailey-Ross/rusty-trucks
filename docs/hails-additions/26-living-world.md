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
  The clip time is the time since the phase began; a looping clip (`_CYC` in its name) wraps by its length,
  `(frames - 1) / fps` like the player's `ClipClock`, others hold their last frame. Fix (2026-10-05): the time used
  to run past the clip end unwrapped and the sampler clamps there, so after about one clip length every NPC froze on
  the clip's last frame ("Skate npc animations are not playing"). Each NPC carries `NpcPuppetClip` (phase, clip,
  time, posed). Mod entry: `sdk.world.set_tuning('living_world', {skater_clips = {rolling = '...',
  ['rolling.Aggressive'] = '...'}})` keyed by the stable phase ids (`ReplayPhase::name`); an override that does not
  evaluate falls back to the shipped pick; cleared on mod disable (`reset_mod_overrides`). Tests:
  `living_world_npc_puppet_clip_attached_and_playing_per_phase` (headless), `..._looping_clips_wrap_and_others_hold`,
  data-gated `..._puppet_clips_play_past_their_first_loop`, `npc_skater_clips_set_merge_and_reset`.
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

## Change: peds milestone M3, navigation

**What retail does** (TU3 code read; addresses are evidence only):
- **Ambient peds never use the road branch.** `Pedestrian.xml`'s `Wander` takes `WanderMode.Road` (FollowRoad +
  `UseCrossWalk`) only while `HasRoadWanderTarget`, and `WanderOnRoad` on `IsOnRoad`; in
  `StateGraph::TheConditionFactory` (`sub_826C1730`) both are registered with the generic factory `sub_82BC3F68`
  (`sub_82F7A698`, `sub_82F79698`) whose evaluate (vtable `0x8231ED5C` + 48) returns 0 [code]. Every ped runs
  `WanderMode.NoRoad`: `CheckForRoadTarget` (no-ops) and `NoRoadWander` (`sub_826A2FB8`). The crosswalk states and
  `WalkSignSaysGo` (`sub_826AC190`) are unreachable for ambient peds; the working `IsOnRoad` / `IsNearCrossWalk`
  belong to the plugin transfer conditions (`usetrashbin.xml`).
- `NoRoadWander` puts the ped on its NavPower mover (`ped+2224`, vtable `0x8232C708`) with the wander goal
  (`ped+5632`, vtable `0x8232C9C8`, mover value 2.0). Target (`sub_82E30F58`): from the ped's forward, probe 5
  directions within 90 degrees at 40 m, else 9 within 270 degrees at 10 m (skipped to the second fan after a
  NavPower failure event 1-3), else step `max(3, 1.1 x 2.0)` m ahead. Probe order (`sub_82E311A8`): straight, then
  `ceil(i/2) x (spread/2)/(n/2)`, negative first. A direction fits when its end point snaps to the navmesh (0.5 m)
  and is reachable (`sub_82C464F0`, a connectivity bitset); NavPower plans the path and steers. On arrival (bot
  state 1, `sub_82E2CB08`) the goal picks the next target.
- **Navmesh** (`0x00EB0027`, all 678 objects) [data]: NavPower v23 graphs per tile: agent block 0.12 / 0.35 / 0.2 /
  1.6 (cell, radius, step, height; unverified), tile bounds, polygons (centroid, radius, flags; the area byte in
  bits 8-15 of the second flags word) with 24-byte edge records (neighbour offset relative to the graph base,
  vertex, edge flags). 63283 polygons, every one of the 148258 neighbour links checked. Areas: 0x11 pavement
  (91-92 % of recomp ped positions), 0xA1 road carriageway (2054 of 2147 DownTown road piece starts; 3-4 % of ped
  positions), 0xF1 never stood on [data + trace]. Tiles stop 0.14 m short of their borders; cross-tile neighbours
  are resolved at setup.

**Change.**
- Setup (`livingworld` group): `living_world_navmesh.py` writes `living_world/navmesh.bin` (polygons, areas,
  neighbours incl. cross-tile links) and `navmesh.json` (counts, area histogram).
- `skate-core::living_world::peds`: `nav` (`NavMesh`: point location with the 0.5 m snap, reachability components,
  A* + funnel paths; `NavRules`), `wander` (the retail target choice, `PedNav` per ped, avoidance, step checks,
  `CrosswalkRule`), `crosswalk` (walk lights from the shared `SignalClock` for the mod rule).
- `skate-data::ped_nav`: `navmesh.bin` reader / writer.
- `skate-game::living_world::peds`: the district navmesh in `PedData`, `PedNavSettings`, navigation-driven intents,
  every step kept on walkable polygons and 0.7 m from other peds; maps without a navmesh keep the test path.

**Simplifications (not retail, documented):** NavPower's path search, path following and local avoidance are not
decoded: paths are A* over polygon portals (equal cost) with funnel corners, a ped turns at 45 degrees per second
while walking (locomotion field `Hash_AD1EA18F819BF397` = 45 read as degrees per second, unverified) and pivots
when a corner is more than 60 degrees off; avoidance = yield to a lower id ahead, side-step a higher id, never
closer than twice the agent radius; 0xF1 polygons blocked (trace-backed); event 4 (`+33`, head back) has no
trigger yet; spawn points snap onto the navmesh (retail's spawn probe not read).

**Moddability.** `PedNavSettings` (wander fans / distances / fallback, turn and avoidance values, `NavRules`
blocked areas and area costs, crosswalk rule), `PedNav::set_route` (mod routes), `NavMeshInput` (a custom map's
walk areas, or `navmesh.bin` written with `skate_data::ped_nav::write`), `WalkSignals` (own crossing lights). The
crosswalk rule (`WalkSignal`: wait while the walk light is not green) is the unused retail `UseCrossWalk` logic,
off by default.

**Multiplayer readiness.** A ped's walk is a function of its spawn record, the world tick, the navmesh and the
other peds' positions (stepped in id order); no hash-map iteration, ties broken by polygon index.

**Tests.** skate-core 8 (fan order, location / reachability, paths, target choice, wander on walkable ground +
determinism, crosswalk waits for walk, clock mapping, avoidance radius); skate-data 1 + 3 data-gated (DownTown
counts and areas, road pieces on 0xA1, 15 peds x 90 s never off walkable ground and identical on rerun, crosswalk
rule: every road entry at a signalled arm on walk green); skate-game 2 (navmesh wander in the app, export load);
Python 4.

Credits: NavPower v23 constants cross-checked against DumbadsSkate3ModdingTools by Ethanw05 (credits there to
SunJay, Dumbad, RenderWareGavin, Tuukkas); recomp: skate3recomp / rexglue / Xenia (code reading and PEDXYZ traces).
```

Plan line for doc 26: "peds M3 navigation: done (NavPower navmesh decoded, retail NoRoad wander, avoidance;
crosswalk rule as mod option since retail never uses it)". Open-questions additions: items 1-5 below.

Player memory agrees with the code (user, 2026-10-05): "I don't remember them using crosswalks in the retail game".

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
## NPCs vanishing in view (leave fade, census centre), 2026-10-05

**Problem.** The user: "They also (peds and skaters) keep dissapearing randomly while still in sight, which i
don't remember happening often in retail."

**Root cause.** (1) Replay NPC skaters were removed on the spot at the end of a line with no branch, a
placeholder, often in plain view. (2) The census circle centre moved along the horizontal velocity; retail uses
the 3-D velocity, so on slopes our cull circle sat higher or lower than retail's.

**Evidence** [code, TU3 recompilation, reference only]:
- `sub_8245A9B8` (manager per-skater check) removes a skater whose controller has its fading byte (`+104` object,
  `+29`) set once `skater+1804` vfunc 76 is below 0.2 (constant at `0x82099280`).
- `skater+1804` is a sub-object at `+14960` of the object built by `sub_82B973C8` (vtable `0x8231E170`): vfunc
  76 `sub_82B97190` returns the opacity at `+244`, vfunc 68 sets it; default 1.0.
- `sub_82594488(skater, dt)` sets the opacity from two timers growing by dt: `+1868` (fade in, `clamp(t,0,1)`)
  and `+1872` (fade out, `1 - clamp(t,0,1)`), so a fade lasts 1 s.
- `sub_8246EA90` starts the fade out (`+1872 = 0`, byte `+29 = 1`) when the time in the controller's timed state
  passes `duration - 1 s`; `sub_8246EE30` sets that duration to `clamp(5 s + rest of the current line, 1.5 s,
  7.9 s)`; `sub_8246EF78` cancels the fade (`+1872 = FLT_MAX`) when the state ends. Retail skaters therefore
  fade for 0.8 s and are removed at opacity 0.2, never popping out at full opacity.
- Census centre: `sub_826B7530` normalises the 3-D velocity (see `.local/research/npc/fix1-cull.md`).

**Change.** New `skate_core::living_world::leave_fade` (`LeaveFadeConfig { fade_seconds: 1.0, despawn_alpha: 0.2
}` as data-driven retail defaults in `SkaterConfig::leave_fade`, `LeaveFade`, `frames_to_line_end`). The replay
NPC starts fading 1 s before a line end with no branch group ahead and is removed below 0.2; the opacity is
published as the `NpcFade` component (alpha) for rendering and mods. Deterministic (a function of the spawn
record and the tick), host decides the removal, clients mirror. `CensusRange::around` moves the centre along
the 3-D velocity.

**Files.** `crates/skate-core/src/living_world/{leave_fade.rs, mod.rs, config.rs, census.rs, tests.rs,
replay_tests.rs}`, `crates/skate-game/src/living_world/{npc_skaters.rs, npc_tests.rs, mod.rs}`.

**Verification.** New tests: `census_centre_follows_the_3d_velocity_like_sub_826b7530`,
`npc_fades_out_over_the_last_second_and_goes_below_alpha_0_2`,
`leave_fade_waits_while_a_branch_is_ahead_and_is_data_driven` (skate-core),
`living_world_npc_skaters_fade_out_before_their_line_ends` (skate-game). All living_world tests pass.

**Drawing the fade and the spawn fade in (follow-up, same day).** Evidence [code]: `sub_825926F8` (skater spawn
setup) sets `+1868 = 0`, so a new skater fades in from 0 to 1 over 1 s; `sub_82594488` checks the fade in first,
so it wins over a fade out. Change:
- `LeaveFadeConfig::fade_in_seconds` (retail 1.0) with `fade_in_alpha`; `LeaveFade::alpha(frames since spawn)`
  is the fade in while it runs, then the leave fade. Removal is only checked once the fade in is over (retail's
  timed state lasts at least 1.5 s, so its fade out cannot start before the fade in is at 0.5; our lines can be
  shorter, so the rule is explicit).
- Mod entry: `LivingWorldSettings::skater_fade` (retail default, applied into `SkaterConfig::leave_fade` every
  step; `LivingWorldSettings::default()` restores retail).
- `present_fade` (skate-game, `Update`): while `NpcFade::alpha < 1`, every mesh under the entity (body, board,
  anything parented to it) draws with a per-entity `AlphaMode::Blend` copy of its material at source alpha times
  the fade (`NpcFadeMaterials`, like the vehicle glass copies). At 1 the shared materials are put back and the
  component (the only owner of the copies) is removed, so a solid NPC costs what it did before; a despawn or mod
  disable drops the component and frees the copies. Generic over any entity with `NpcFade`, so peds can use it
  for their distance fade.

Verification: `npc_fades_in_over_its_first_second_like_sub_825926f8` (skate-core: curve, fade in wins, no removal
during it, data-driven) and `living_world_npc_skaters_fade_in_from_transparent_with_blended_copies` (skate-game:
alpha < 0.05 at spawn, every frame equals the core curve, 1.0 at 1 s, the mesh uses a blended copy at the fade's
alpha while fading and the shared material again at 1, copies freed). `cargo test --locked -p skate-game
living_world` 27/27, skate-core living_world 88/88, `cargo build --locked` (dev) ok. Not yet seen in game.

**Open.** Blending the whole character can show its own back faces through itself during the 1 s fades (no
depth write in `AlphaMode::Blend`), and alpha-masked source materials lose their cutout while fading; retail's
fade shader is not decoded. The 0.5 hold of the fade in (component byte `+71`) is not ported. What the controller's timed state is in retail
(entered on a skater component byte `+59`) and the early fade when a speed-like value is below 1.0 (byte `+604`,
float `+576`) are not decoded. Peds: the cull component's vfunc +4 and the census focus object (player or
camera) are still open; no camera test was found in the ped cull itself.

**Peds (follow-up, same day).** Problem: the same report for pedestrians. Root cause: retail peds fade out by
camera distance (opaque to 45 m, gone at 55 m) and are culled at 70 m (slow circle; 90 m with a 20 m forward
offset fast), so the cull is never seen. We read the model's 45 / 55 m pair as an LOD switch and drew every ped
solid up to the 70 m cull, which then popped it in view. Evidence [code, TU3 recompilation, reference only]:
- `sub_827C1188` (per render instance, every frame): pair `Hash_73B6874C7B46C7C6` of the model record (peds 45 /
  55 m [data]) against the camera distance gives opacity `1 - clamp((d - near) / (far - near), 0, 1)`; the pair
  `Hash_9FCFDBEA56BA4733` (65 / 75) is used only when its third float is larger (`sub_827C1870`), and both are 0
  for peds [data]. A spawn fade in `+576` grows by 1/30 per console frame (`0x8232BA4C`, about 1 s); the drawn
  opacity is the smaller one, and below 1 the instance draws blended (`+580`).
- `sub_826BA8B0` (ped cull) is a pure 3-D squared-distance test (`>=` for the ped pass). Entities flagged
  census-owned (`*(ent+2020)+240` bit 0x80, set at spawn from the spawn descriptor byte `+97`) are removed;
  the others only pass the "beyond" flag to their `ISecurityGuard` component (lookup key `sub_8269D780`, which
  returns the string `ISecurityGuard` at `0x82307BC0`; vfunc +4), so that branch is the security guards, not a
  visibility rule.
- The census focus (`sub_826BDB50` calls `sub_826B71F0` with its own vfuncs +124 / +128, `sub_826BE7D0` /
  `sub_826BE870`) is the focused skater from the player manager `*(0x83085480)` (= world `+196`) vfunc 16:
  position from its `+52` component, velocity from vfunc 24 (zeroed below |v|^2 = 1e-4, `0x8209BE90`). With no
  skater it falls back to a vector at world `+292` `+5312`. Not the camera.

Change: new `skate_core::living_world::peds::fade` (`PedFadeConfig { distance: [45, 55], fade_in_seconds: 1.0,
enabled }`, `draw_alpha`); a model record's own pair wins (mods set it per model), `LivingWorldSettings::ped_fade`
is the mod entry. Each ped gets the shared `NpcFade` (alpha 0 at spawn); `present_ped_pose` sets the alpha from
the camera distance and the time since the spawn tick (deterministic) and hides the ped's scene at 0; the generic
`present_fade` draws the blend. The cull radii and the LOD placeholder are unchanged.
Files: `crates/skate-core/src/living_world/peds/{fade.rs, mod.rs, choice.rs}`,
`crates/skate-game/src/living_world/{peds.rs, mod.rs, peds_tests.rs}`.
Verification: `peds_are_invisible_before_the_census_cull_can_remove_them`,
`ped_fade_follows_the_model_pair_like_sub_827c1188`,
`ped_spawn_fades_in_over_a_second_and_mods_can_change_or_disable_it` (skate-core) and
`living_world_peds_spawn_with_a_draw_fade_that_starts_transparent` (skate-game); skate-core living_world 91/91,
skate-game living_world 28/28. Not yet seen in game. Open: the `ISecurityGuard` vfunc +4 body (what a guard does
when beyond), the census gate on the focus skater (`+52` vfunc 4, `+1904` bit 0x8) and the near-range override
(`+612` / `+616`) in `sub_827C1188`; our observer is still the deck, retail's is the skater's `+52` position.

## Peds standing on their heads in the left turn, 2026-10-05

**Problem.** First play of the living world: the pedestrians "look like they hurt" (user). Twisted bodies.

**Root cause.** The left turn plays `StandTurnR180` mirrored. `PedAnimPlayer::pose` mirrored the bare clip
delta (before adding the `PEDESTRIAN_RIG_TPOSE` reference) with `pose_mirror::mirror` in trajectory mode 1. That
mode is a full-pose operation: for the children of the trajectory bone it multiplies in the literal 180 degree
quaternion (`0x8232F740`, part of `Mirror` `0x828CDAF8` [code]) that the rig's reference hips carry. On a delta
(near identity) it rotated the hips 180 degrees, so every ped stood on its head for the whole mirrored turn and
through the blends into and out of it. Crossfade blending was not the cause: `pose_blend::blend_sample` already
takes the shorter arc (dot < 0 check).

**Evidence.** The extended dump test `living_world_ped_pose_dump_for_a_render_check` writes 15 poses per set
(mid-blend into start / walk / stop / idle / both turns, mid walk, mid stop, idle, mid turns) for the `default`,
`female` and `jock` sets; rendered on `male_adult_1`, `female_adult_1` and `male_jock_2` with `render_glb.py
--pose`. Before: walk, blends, idle and the right turn are upright, all mirrored poses are upside down or lying
flat on all three models. The reference pose against the mirror on the export [data]: mode 0 leaves the hips 180
degrees off, mode 1 keeps every bone within 6.6 degrees (the authored hand asymmetry), so mode 1 belongs on full
poses. After: the mirrored turn is the upright mirror image of the right turn on all three models.

**Change.** Each layer becomes a full pose (delta added onto the reference) before it is mirrored, then the
layers blend. Blending full poses equals blending deltas and adding afterwards (the add is a left multiply /
affine map, which nlerp and lerp commute with), so unmirrored output is unchanged. No new values; nothing a mod
reaches changed (sets, clips and overrides as before).

**Files.** `crates/skate-core/src/living_world/peds/anim.rs` (`pose`, new `add_reference`),
`crates/skate-core/src/living_world/peds/tests.rs` (new test),
`crates/skate-game/src/living_world/peds_tests.rs` (dump test extended).

**Verification.** New `the_mirrored_turn_mirrors_the_full_pose_and_never_flips_the_hips` (skate-core): a rig whose
hips reference carries the mode 1 literal, a turn clip with a zero hips delta and an asymmetric leg; the hips
stay at the reference at every tick including mid-blend, and out of the blend the left turn equals the right
turn's full pose mirrored. Fails before the change (hips flipped from tick 1), passes after. skate-core
`living_world::peds` 20 passed; skate-game `living_world` 26 passed; release build of `skate3rust` succeeds.

**Open.** Retail's ped motion graph tree order (where `AddBindPose` sits against the mirror node) was not read;
the fix follows the data (the reference is symmetric only as a full pose). Walk and blend poses rendered upright,
but a subtler twist in game (limb roll on the GLB followers, bones without clip data) is not excluded until the
user looks again.

## Props pulled toward the player, 2026-10-05

**Problem.** First play video, second map, Aletown spawn (about 1:00 to 1:10): running past them on foot, props
"magnetize" to the player. A trash bin trails the runner at a fixed offset and a newspaper box slides along the wall
with them. Earlier notes said grabbing objects "doesn't appear to work yet".

**Root cause.** In #15's carry glue (`physics/prop_carry.rs`, wired in `physics/frame.rs`), a rising edge of **A**
toggles a grab of the nearest prop within 2 m in any direction. But A is the retail on-foot sprint button: the
derived controller's held timer for action 80 (slot 20) is what publishes `OB_Sprint` (8259AA8C,
`input/offboard_intentions.rs`) [code]. #15's comment says B sprints, but B (action 81, slot 21) only blocks sprint.
So every sprint press near a prop grabbed it, and the drag (0.9 m hold, up to 4 m/s, auto-drop beyond 3.5 m)
pulled it along until the runner outran it. The bug is in #15's original code; its author should hear about it.

**Evidence.** Frames of the video at 0:59 to 1:03 (4 fps): the carry HUD diamond turns white (prop in reach) and
then cyan (carrying) while the bin follows. The bin is left behind once the runner is faster than the drag cap.
[trace: video] Retail code: the on-foot grab-object decision (82D324B0, `physics/biped_ground/grab.rs`) only runs
while Processed2476 bit 22 (GrabWorld) is set. The input listener emits GrabWorld while raw flag bit 28 (action 73,
pad slot 9, RB) is held (8259AF68, `input/riding_intentions.rs`) [code]. So retail grabs with a held RB, not a
toggle.

**Change.** Grab is now a held button: hold the GrabWorld button (RB) to grab the nearest prop in reach and keep
carrying it; release it to drop. In placement mode, releasing it confirms the ghost pose. B still toggles placement
on a rising edge. The buttons live in `CarryButtons` (defaults bit 28 grab, bit 20 placement) on `PropCarry`, so a
host setting or mod can rebind them. `Tick::from_controller` builds the tick from the derived controller words.
The physics (push, drag, depenetration) is unchanged.

**Files.** `crates/skate-game/src/physics/prop_carry.rs`, `crates/skate-game/src/physics/frame.rs`, tests in
`crates/skate-game/src/physics/prop_dynamics.rs`.

**Verification.** New `sprinting_past_a_prop_does_not_grab_it`: A pressed then held for 90 ticks while the carrier
runs past a prop 0.8 m to the side at 5 m/s. No grab, and the prop moves less than 5 cm. It fails with the old A
binding ("sprint (A) produced a grab on tick 0") and passes now. New `grab_world_button_holds_and_release_drops`
covers hold, carry, release and the B edge. The existing carry and placement tests now use the held grab and pass:
16 prop tests pass. `cargo test --locked -p skate-game`: 454 pass, 1 known upstream failure (`pipelines_accept_…`).
`cargo build --locked -p skate-game` (dev) succeeds.

**Open.** Retail's grab qualification (scene query, angle limits 436/452, margin 444) is not ported. The host still
uses #15's 2 m any-direction nearest-prop rule. The bench FPS drop with the board stuck inside it is a separate
issue (prop depenetration has no cap) and is not addressed here. There is no Lua API for `CarryButtons` yet. In-game
check by the user: sprint past the Aletown bins (they should stay put), then hold RB next to one (it should drag).

## Moving a held prop: every stick direction goes forward, skater inside the prop, grab flicker, 2026-10-05

**Problem.** User sessions 13:20 and 14:46 (fix13 in the build): "no matter what direction on my stick I pushed I
went one direction", "No matter what direction I push or pull an object it just goes forward. that includes to the
sides", "Grabbing objects warps you inside of it instead of grabbing the edge of it". The 14:46 log also shows the
physical state flipping BipedGround <-> OffBoardPushing every tick for 6 ticks at 20:47:32Z.

**Root cause (direction).** fix13's Move Object path (`biped_ground::update`, state 502 with a held prop) drives the
pair through the walking controller's velocity override. The OB_ObjectMv inputs and the controller velocity
(Biped480) were right for every direction, but the controller's approach step (82D7F458..FDD0,
`ground_motion/approach.rs`) [code] steps toward the ground contact target projected onto the FACING line
(side axis = cross(up, forward) removed), with a budget of |velocity| * dt. The target sits ahead of the facing, so
a pull or side step came out as a forward step at the same speed. Walking never shows this because the walker
turns to face its velocity.

**Retail.** [code] State 502 is its own class: ctor 82D43B90 (1216 bytes, vtable 0x82317364), stored at
Player+1776 by the player constructor (82DB2DFC); BipedGround's object (ctor 82D305D8) sits at Player+1764. The
walking controller job 82D4E2F8 (the only caller of the ground controller 82D7C818) is submitted only from
BipedGround's update 82D30D30, so retail Move Object does not take the facing-line approach. Its own movement
(probably in 82D44A10 / 82D45D30, which call the shared foot and ground-sync helpers 82D773F8, 82D77558, 82D2E250)
is not decoded.

**Root cause (inside the prop).** `prop_carry::follow` pulled the prop's CENTRE in to 0.9 m (`DRAG_HOLD`). A bench
(box half extents 2.64 x 0.22 x 0.38 m) grabbed by its end then sits around the skater.

**Root cause (flicker).** The auto-drop test measured the prop's centre against `MAX_HOLD_DISTANCE` (3.5 m) while
the grab reach is measured to the box surface (2 m). A long prop grabbed by its end has its centre beyond 3.5 m, so
it was dropped the tick after every grab and re-grabbed the tick after that while RB stayed held.

**Change.**
- `biped_ground.rs`: for the Move Object job only (502, held prop, stick asking for motion), clear the job's
  target-contact bit (flags bit 2) so the approach steps by the requested velocity; the support contact (bit 1)
  still keeps the feet on the ground. Labelled NOT RETAIL YET in the code: it loses the 0.3 m step-up of the
  target contact while dragging.
- `prop_carry.rs`: the hold distance is at least the box's extent toward the skater plus `grip_reach` (new carry
  tuning, 0.35 m, NOT RETAIL YET: engine value; retail's grab offset probably comes from the DMO characteristics and
  the MovingObjectNew / MVOBJ hand positions, UpdateObjectGrabbing 0x821BCDB4, IsGrabbingDMO 0x820B757C). The
  auto-drop limit is `MAX_HOLD_DISTANCE` plus that extent, so it is measured from the near face like the grab.
- Mods: `sdk.world.set_tuning('carry', {grip_reach = ...})` (finite, >= 0; reset on mod disable like the other carry
  fields), shown by `sdk.engine.inspect(..., 'world_tuning:carry')`; `sdk/skate.lua`, `api.lua` updated.

**Files.** `crates/skate-game/src/physics/biped_ground.rs`, `crates/skate-game/src/physics/prop_carry.rs`,
`crates/skate-game/src/physics/carry_direction_tests.rs` (new), `crates/skate-game/src/physics.rs` (test module),
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`,
`crates/skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.** New asset-backed `move_object_follows_the_left_stick_in_the_skater_frame` (DownTown, stock graphs
and setup data, pad input through the whole frame): step off, put the nearest map prop (a 5.3 m bench-sized box)
end-on with its near face 1.5 m ahead, hold RB, then push the left stick in 8 directions for 45 ticks each. Before:
every direction moved the pair forward (stick right: local [0.01, 0.59] m; stick back: [0.00, +0.72] m). After:
forward [0, 1.05], right [0.60, 0], back [0, -0.75], left [-0.60, 0], diagonals cos >= 0.97 (speeds differ per
axis: push 1.4, side 0.8, pull 1.0 m/s); the prop follows; gap skater to near face 0.35 m throughout (before: centre
at 0.9 m); exactly one state change while grabbing (11 flips with the old centre limit). Prop height drift on the
flat spawn plaza is under 2 cm. Run: `SKATE3_ASSET_ROOT=<assets> SKATE3_MAP=<maps/DownTown.skate> cargo test
--locked --release -p skate-game --bin skate3rust carry_direction -- --ignored`. `cargo test -p skate-game --bin
skate3rust`: 489 pass, known `pipelines_accept_...` failure, and `living_world_npc_trick_jump_has_no_pose_pop`
failed once in the full run but passes alone (NPC work in progress, not this change). skate-core
`input::offboard` 11 pass; skate-mods 96 + 3 pass, known Skyline failure.

**Open.** Decode the 502 class's movement (82D44A10 / 82D45D30) and replace the bypass. The bench sinking
through the ground by the wide plaza stairs (14:46 video at about 10 s) is not reproduced on flat ground; the held
prop is moved by `drag_to` (horizontal velocity forced every tick, vertical left to the solver) while its own
triangles are parked, so a step riser or edge under a long box is the likely place; a repro on those stairs and a
look at the held-body depenetration in `prop_dynamics.rs` come next.

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

## Board stuck inside a prop, 2026-10-05

**Problem.** User's first play: "Interacting with this bench near aletown spawn broke the framerate, also its hitbox
seems a bit off ... The framerate returned when I called back my board, so it may have been stuck inside the bench".

**Root cause (confidence: high for the cost, medium for how the board got in).** The bug is in #15's original code
(`physics/prop_dynamics.rs`, `push_from_skater`). Each prop's contact box is the AABB of its template's render
triangles, while the board itself collides with the exact render triangles. Under a bench seat the mesh is open
but the box is solid, so the board can sit inside the box. #15 then pushes the prop with a 0.5 m/s floor every tick
any skater volume penetrates it, even when nothing is closing, and board pushes had no speed cap. For a bench-sized
prop (about 126 kg at the default density) that nudge is almost exactly what ground friction removes again, so the
bench creeps but never separates and never cools down to sleep. Every awake tick rebakes its triangles, and
`BoardWorld::replace_triangles` rebuilds the query index of the whole prop layer. Calling the board back removes
the overlapping volume, which ends it.

**Evidence.**
- `legacy_push_keeps_bench_awake_with_parked_deck` (deck capsule parked inside a 2.0 x 0.9 x 0.7 m box, #15's
  tuning): over the last 300 of 600 ticks the bench is awake 300/300, pushed 300 times, rebaked 300 times, travel
  0.135 m.
- `downtown_prop_rebake_cost` (ignored, private assets): DownTown has 786 props and 151885 prop-layer triangles; one
  rebake of the Aletown bench template (`ac_DMO_DT_benchAggregate_1001...`, 224 triangles, box half extents
  1.61 x 0.54 x 0.43 m) costs about 0.5 ms in the test build. That is paid every physics tick while the bench is
  awake, plus the per-tick pair queries.
- No retail address for DMO contact response or depenetration was found in this run; the values below are engine
  choices (#15's constants plus caps), not retail.

**Change.**
- All prop contact and push values are now one `PropTuning` (`physics/prop_dynamics.rs`), defaults = #15's
  constants plus: `max_depenetration_per_tick` 0.05 m (the positional correction of one body per tick is clamped),
  `board_push_speed` 6.0 m/s (board pushes are capped along the push direction like body bumps), and
  `stuck_release_ticks` 30 (after 30 ticks of a skater volume sitting inside a prop without closing on it, the
  overlap nudge stops and slow drift no longer pushes; a real hit faster than `penetration_push_speed` still does).
- Per prop type overrides by MOBJ template name (`PropTuningTable.by_template`), including an optional
  template-space `collision_box` that replaces the render-AABB box (the rendered pose stays put; mass unchanged).
- Bevy resource `PropTuningSettings` holds the table; `apply_prop_tuning` pushes it into the live props before each
  physics tick and after a map load. Setting it to `default()` (mod disable) restores the shipped values; only
  prop types whose tuning changed are woken.
- A prop whose pose did not change since its last rebake skips the rebake (bit-identical pose, so identical
  triangles; behaviour unchanged).

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs`, `crates/skate-game/src/physics.rs`.

**Verification.** `cargo test --locked -p skate-game prop_dynamics`: 18 pass (all earlier prop tests unchanged) plus
the ignored DownTown probe. New: `parked_deck_inside_bench_lets_it_sleep` (awake 0, rebakes 0, pushes 0 over the
last 300 ticks, travel 0.001 m), `released_bench_still_takes_a_real_hit`, `board_push_speed_is_capped`,
`depenetration_is_bounded_per_tick`, `tuning_overrides_per_template_and_resets`. Full skate-game: 461 pass, 2 fail:
the known `pipelines_accept_valid_group_outputs_when_fingerprint_changes` and
`graphics_menu::sections_expose_only_real_rows_and_all_maps` (graphics_menu.rs is being changed by the NPC draw
distance work, not touched here). Dev build OK. Not yet checked in game.

**Open.** The board itself still sits inside the open bench mesh until the player moves it; the frame cost is gone
because the bench sleeps. What retail uses for this DMO's collision (its own physics volume vs the render mesh) is
not recovered; a per-type `collision_box` can fit the bench once it is. Lua (API 2) bindings for `PropTuningSettings`
are a follow-up. `replace_triangles` still rebuilds the whole prop-layer index per moved prop (a refit would be
cheaper but needs proof that query order is unchanged).

## NPC draw distance (QoL, not retail)

**Problem.** User request, 2026-10-05: "Lets add an option in the settings to extend the distance of culling (non
retail QoL feature". Decisions: one option "NPC draw distance" in the graphics settings, Retail (default) / 1.5x /
2x / 3x, for peds, NPC skaters and cars together, keeping the density (caps scale with the covered area, so 2x
range = up to 4x NPCs). Retail has no such option; this is an engine QoL setting, not a retail claim.
User request, 2026-10-07: a step so no peds spawn; decision (user): a "None" step on this option, first in the
list, which turns off NPC skaters, peds and cars together (`DrawDistance::NONE` = 0; `LivingWorldSettings::
user_npcs_off` sets every kind's `enabled` false, so live NPCs despawn with reason Disabled; ranges stay retail;
kept across mod resets like the draw distance; saved with the other graphics settings). Test: the draw distance
test in `living_world/tests.rs` (config off, kept on mod reset, a running population spawns 0 / 0 / 0).

**Change.**
- `skate-core::living_world::draw_distance::DrawDistance` (new): one multiplier `m` on top of the retail
  data-driven values, never written into them. Distances x m: both census circles of peds and cars (spawn ring
  inner / outer, cull radius, forward offset; speed keys stay), the initial populate ring (8-80 m), the ambient
  skater spawn ring (60-90 m) and cull (120 m). Counts x m^2: census caps (through the density, so the cap at a
  point is `max_population x density x m^2`), the entity pools (peds 31, vehicles 15 and the initial 15), the
  attempts / spawns per pass and of the initial populate, the skater desired count, AI cap, character pool,
  candidate line count and the ambient skater slots (slot 0 stays the player's). Not scaled: speed keys, the
  5 m / 10 m skater spacing, the 1000 m height cull, all fades in time, the census rotation and cycle phases.
  Values that are not finite or not positive mean retail; the rest is clamped to 0.25..4.
- `PopulationConfig::draw_distance` (1.0 = retail). `LivingWorld::step` builds a scaled copy only when the
  multiplier is not 1; at 1x it runs `self.config` itself, so retail is the unchanged code path.
- Ped draw fade (`peds::ped_draw_alpha`): the model's 45 / 55 m pair (or the configured default) x m, so the
  fade still ends before the scaled census cull (70 m near edge x m). The ped LOD switch stays at the retail
  distances (far peds keep the cheaper LOD). NPC skaters fade by time (1 s), so only their ranges scale. Cars
  have no draw fade; their census cull scales like the peds'.
- Settings: `LivingWorldSettings::npc_draw_distance` (in effect; a mod reads and sets it) and
  `user_npc_draw_distance` (the player's menu choice). `apply` writes the sanitised value into the core config
  every step. `reset_mod_overrides()` (mod disabled) restores every mod-changeable field to retail and the draw
  distance to the player's own choice, keeping the session flags.
- Menu: GRAPHICS row "NPC draw distance" (Left / Right or Enter cycles Retail, 1.5x, 2x, 3x), saved in
  `settings/graphics.json` (`npc_draw_distance`, missing or unknown = Retail) and applied at start. The row
  reads "2x  (not retail)", and changing it shows: "NPC draw distance is a QoL option, not retail: peds, NPC
  skaters and cars appear farther out, with more of them to keep the density. Costs frame time."

**Multiplayer.** The population authority (standalone or a future host) owns the multiplier: it decides what
exists for everyone, and the records carry positions, not ranges. A per-client draw distance must stay out of
the authority's simulation: a client would only hide or fade what it was sent beyond its own range (render
side, like the ped fade), never feed its value into `PopulationConfig`. A Client role runs no rules today, so
its setting only changes its ped fade. No networking was added.

**Files.** `crates/skate-core/src/living_world/{draw_distance.rs (new), config.rs, mod.rs, population.rs,
tests.rs}`, `crates/skate-game/src/living_world/{mod.rs, peds.rs, tests.rs}`, `crates/skate-game/src/graphics_menu.rs`.
No vehicle file changed (cars read the scaled census circle in the core).

**Verification.**
- 1x behaviour-identical: the decision stream (every spawn / despawn record with position, heading, seed,
  choice, plus the final counts) of the core's seeded 100 s run (player circling at 8 m/s, skaters, peds, cars;
  seeds 1234, 99, 7) was dumped before the change and after it: byte-identical (421,816 bytes, same SHA-256).
  Kept as tests: `draw_distance_retail_runs_the_unchanged_config` (no scaled copy at 1x; explicit 1.0 gives the
  same stream as the default) and, in skate-game, the same through the settings path.
- `draw_distance_2x_doubles_every_range_and_quadruples_the_caps`: every radius 2x, density / pools / budgets
  4x, skaters 120 / 180 / 240 m with 12 desired; a still player gets 60 peds instead of 15, all spawns inside
  the scaled ring; the retail config is untouched.
- `draw_distance_keeps_the_fade_before_the_cull_at_every_step`: at 1x, 1.5x, 2x and 3x the scaled ped fade
  ends before the near edge of either cull circle (camera up to 10 m nearer), and every kind spawns inside its
  cull.
- skate-game: `living_world_npc_draw_distance_setting_scales_population_and_mods_reset_to_the_players_choice`,
  `living_world_ped_fade_scales_with_the_draw_distance`.
- Results: skate-core living_world 95/95, skate-game living_world 30/30, `cargo build --locked` (dev) ok with
  no new warnings. Not yet seen in game.

**Open.** The frame cost at 2x / 3x (up to 4x / 9x NPCs) is not measured. Retail pools (31 peds, 15 cars) are
memory limits on the console; scaled by area here, which is a choice of this option. Skater lines are only
loaded for the current district, so at 3x the skater ring (180-270 m) may find few lines.

## Startup freeze from the first-play fixes, 2026-10-05

**Problem.** A release build with the fixes above (snapshot of the uncommitted tree) froze at startup with no
rendering; Windows ended it as a hung window. The build without the fixes started.

**Cause: not the code.** The freeze run happened while another worktree compiled `skate-game` (three rustc
processes at about 4 GB each) on the same HDD that holds the repo, the assets and the target dirs; the disk sat at
100 %. Evidence: the map parse took 8165 ms in the frozen run against 2097 / 2460 ms in normal runs, audio init
frames were about 20 s apart, and the process used little CPU (waiting on I/O, not spinning). Reading the diff
found nothing that blocks before the living-world load (no locks, waits, ordering cycles; the new systems only
act once the population exists).

**Verification.** The same exe, run muted once with no build running: `LIVING_WORLD data DownTown` 28 s after the
first log line (baseline build 42 s), then traffic, peds and NPC skater lines, `tick 600` reached. `roads 46`
instead of 76 is the district road filter ("Cars flying off"). No code change. Lesson: do not judge a startup
while a build runs on the same disk.

## Population follows the board, not the player, 2026-10-05

**Problem.** In the user's session (log `game-20261005-114026`), at 17:45:02 every car and NPC skater disappeared
and all 15 peds respawned out of earshot. That lasted until 17:45:15, when the board was back in hand. The video
shows the player walking without the board. FPS fell from about 280 to about 40 over the same 13 s.

**Cause.** `gather_observers` built the census focus from the deck body. When the player walks off the board and
leaves it behind (or the board is far away), the census culled everything around the player and populated around
the board. Retail census focus is the focused skater: `sub_826BDB50` -> `sub_826BE7D0` (position, skater `+52`
component vfunc 20) and `sub_826BE870` (velocity, zeroed when |v|^2 <= 1e-4 at `0x8209BE90`) [code], see "NPCs
vanishing in view".

**Evidence.** WORLD_AUDIO lines 17:45:02 to 17:45:15: `cars 0`, `skaters 0`, `peds 15/0`. REPORT_META shows
`physical:BipedGround` the whole time. The jump from 9 cars to 0 within one second means the focus moved more than
the 110 m car cull radius in under a second, which walking cannot do.

**Change.** The focus is now the player's character: the skeleton's physical centre of mass and its velocity
(`board_frames.centre_of_mass` / `com_velocity`, the Skeleton16144/16176 values that `render_pose.rs` publishes
into the reckoning fields). These are valid on board, walking, in the air and in a bail. The deck is only the
fallback when no skater is loaded. Near-zero velocity is zeroed as retail does. The non-finite guard is unchanged.
The observer list stays one entry per player. Peds' draw fade and the NPC draw distance keep using the camera,
as in retail (`sub_827C1188` fades by camera distance).
Also fixed: after a map reload or respawn into a new generation, the world tick restarts at 0 but the debug
readouts kept the old world's last-report tick, so `LIVING_WORLD tick ..`, the peds readout and the traffic summary
never logged again. A shared `report_due` restarts with the world. The population line now also prints the focus
position and speed.

**FPS drop (not proven).** The fixed physics step kept 60 ticks/s through the drop (REPORT_META physics_tick
2513 -> 3306), so the cost went to frame time. The log has no deck position and no spawn counts, because the
summary was silenced by the reload bug above. Likely causes: (a) spawn churn, if the deck was moving fast (flung
or falling), so that peds spawned around it fell behind the cull every pass and respawned with fresh looks; or
(b) the deck itself costing physics time far from the player. With the focus on the player, (a) cannot happen
from a left board. The next debug session will show it (`focus` and `spawned/despawned` in the population line).

**Files.** `crates/skate-game/src/living_world/mod.rs` (`player_focus`, `local_focus`, `report_due`,
`gather_observers`, population readout), `peds.rs` and `vehicles.rs` (readout timing), `tests.rs` (3 tests).
`npc_skaters.rs` `log_readout` has the same reload bug and still needs `report_due` (left for the NPC skater work).

**Verification.** `cargo test --locked -p skate-game --bin skate3rust living_world`: 37 passed, including
`living_world_off_board_player_keeps_the_population_around_the_player_not_the_board` (a player walks away from a
deck 500 m off; every ped and car stays within cull range of the player and nothing ever spawns near the deck),
`living_world_focus_is_the_character_and_still_velocity_is_zero`, and
`living_world_debug_summaries_restart_with_a_new_world`. Not seen in game yet.

## Peds rendered warped (bone frames twisted 90 degrees), 2026-10-05

**Problem.** After the left-turn fix the user still saw warped peds: "Peds look bad still. they are warped and not
quite right. we need to relook at the retail code and see what we are doing wrong. Honestly the recomp doesn't do
the best job of rendering them either, but ours is considerably worse at the moment." / "they look terrible".
Offline renders showed the same on every model and in every pose, the reference pose included: pinched waist,
wide bowed hips, twisted shoulders and arms.

**Root cause.** The skin was `bone global x render_basis x inverse(GLB bind)`. `render_basis` (90 degrees about the
bone's own x axis) belongs to the skater GLBs, whose converter (`character_glb.py`) bakes the matching
`basis_transpose` into their bind matrices so the two cancel. The ped GLBs (`living_world_models.py`) keep the
retail model's bind matrices unchanged, so nothing cancelled it: every ped bone turned 90 degrees about its own
axis. Spine bones have x up (the torso turned -90 degrees about the vertical), leg bones have x down (the legs
turned +90 degrees), so torso and thighs were 180 degrees apart at the hips and linear blend skinning collapsed
the waist. The clip data, the reference add and the mirror were not at fault.

**Evidence.**
- [data] All 51 ped GLBs against the rig's `PEDESTRIAN_RIG_TPOSE` globals: the bind frames match within 0.56
  degrees and 1.0 cm with no basis; with the skater basis every animated bone is 89.7 to 90.3 degrees off.
- Split offline (no game), `render_glb.py`: the bind pose renders correctly; the rig reference pose and idle /
  walk / stop frames through our pose path render warped with the skater basis and correct without it (same
  poses, same GLBs). Per-bone skin rotation of `male_adult_1` in idle before the fix: hips and spine about -90
  degrees about the vertical, thighs about +90 degrees.
- [code] `sub_827BA100` (the living-world presentation manager setup) names the rig `FullPedestrianACS`
  (`0x821AA1BC`) and `PedestrianBindPoseSQTs` (`0x821AA1D0`), allocates `cLivingWorldPresEntityManager::
  InvBindPoseMats` (`0x821AA1E8`, bones x 64 bytes via `sub_828D8170`) and fills each entry with the identity
  matrix (`0x82139A10` to `0x82139A40`) before uploading it. No per-bone basis or bone remap matrix appears on
  that path; the ped skin is driven by the animation rig's own bind frames, which is what the [data] check shows
  the model binds equal. Where retail forms the final palette (bone global x inverse bind) was not read.
- Reference look: the user's recomp sessions show peds only small at a distance (upright, normal proportions);
  the large figure in those shots is the player's skater.

**Change.** `skate-game::living_world::peds`: the joint globals handed to the shared binding are
`global x inverse(render_basis) x basis`, so the skin is `global x basis x inverse(GLB bind)`. `basis` comes from
`ped_bone_basis`, a pure function of the model: of identity (retail ped GLBs) and the skater convention, the one
whose `reference x basis` matches the GLB bind frames. Every shipped ped resolves to identity; a mod GLB written
the skater way still renders correctly, and a mod GLB written the plain glTF way (bind frames = rig frames) needs
nothing. Followers (fingers, face) use the bind in the rig's frames. No new tuning values (it is a correctness
fix); no setup re-run (the GLBs were right). Deterministic: the basis depends only on the model file.

**Files.** `crates/skate-game/src/living_world/peds.rs` (`reference_globals`, `ped_bone_basis`,
`ped_joint_globals`, `PedPuppet::basis`, binding and pose), `crates/skate-game/src/living_world/peds_tests.rs`
(new test; the dump writes the reference pose first), `crates/skate-data/tests/ped_anim_data.rs` (new data test),
local `.claude/skills/living-world/tools/render_glb.py` (`--bone-basis auto|none|skater`, auto = engine rule).

**Verification.**
- `ped_skin_rests_in_the_reference_pose_and_never_twists_bones` (skate-game): rig frames with spine x up and leg
  x down; for a retail-style and a skater-style GLB the basis is detected, the reference pose skins to identity on
  every bone, a 30 degree bend on the leg turns only the leg (30 degrees, torso 0); the pre-fix path gives 90.
- `ped_glb_bind_frames_are_the_rig_reference_frames` (skate-data, data-gated): all ped GLBs, every animated bone,
  bind frame vs reference frame under 2 degrees; the skater basis is at least 45 degrees off.
- Renders `.local/research/npc/fix10-out/before.png` / `after.png` (reference, mid walk, into stop, idle on
  `male_adult_1`, `female_adult_1`, `male_jock_2`).

**Open.** The final palette multiply in retail's ped renderer was not traced to an address. Tints are still not
drawn (separate item). Clip quality vs the recomp should be judged by the user in game.

## NPC skaters fading out near the player (line end), 2026-10-05

**Problem.** User: "npc skaters are still fading out way too fast, and way too close to the player. I think its
measuing the distance from its spawn point, not the players distance from them.." and "They should continue skating
around".

**Cause.** The replay tier faded every skater 1 s before the end of its recorded line when no branch group was
ahead, wherever that was. [data] No line on the disc has a branch group on its last node (760 of 760 DownTown lines,
all 1,691), 426 DownTown lines have none at all, and the median line lasts 9 s (DownTown p10/50/90 4.3 / 9.0 /
18.7 s), so most skaters vanished after a few seconds, often close to the player. The fade itself is retail, but it
belongs to a timed controller state entered on a skater component flag (`sub_8246EF78`, byte `+59`; leaving it
cancels the fade), not to the line end.

**Evidence (retail code, TU3 recompilation).**
- [code] `sub_8246D3C0` (PathController node update): when the node index reaches the last node it calls
  `sub_8246C7F8` with the end position; otherwise a changed node goes to the branch choice `sub_8246BEE0`.
- [code] `sub_8246C7F8`: `sub_82458968(path manager, position, ..., 16 entries, mode 1)` lists lines; one result
  is taken as it is, several go through the branch chooser `sub_8246C1C8` (score `sub_8246C230`, lowest wins,
  index 0 when all are rejected); the controller takes the line id, node and path (`+592`, `+816`, `+800`).
- [code] `sub_82458968` mode 1: walks every loaded line (path hash map), skips lines in use (`sub_82458860`),
  takes the **start node** (node 0) and keeps lines whose start is within the radius; with none, it falls back to
  the nearest valid start at any distance (a nearer invalid one wins only beyond 6 m, `0x822F94F4` = 36).
- [data] Radius: `ai_skater` `default` field `Hash_4F87E7A70DA11691` = 4.0 m (read by `sub_8246C7F8`).
- [code] Ambient skaters otherwise leave by the population cull `sub_8245D520` (3-D distance to the player > 120 m,
  height > 1000 m) and the per-skater check `sub_8245A9B8` (fading state below opacity 0.2; character wanted
  elsewhere while over budget). [recomp] measured lives 13-28 s median, despawn distance median 115-123 m.
- [data] 742 of 760 DownTown lines (406 / 423 Industrial, 484 / 508 University) have another line's start within
  4 m of their end; median end-to-start distance 0.1 m (tool `.claude/skills/living-world/tools/line_ends.py`).

**Change.** At the last node the cursor continues on the next line exactly like that: unused lines whose start
lies within `ChainConfig::radius` (retail 4 m) of the end, at most 16 in id order (deterministic; retail walks a
hash map), one taken directly, several scored by the branch chooser, start at node 0. The decision is a
`BranchRecord` from the last node, so clients mirror it. The leave fade now starts only at a dead end (no start
within the radius), which the replay tier cannot ride past (retail's full skater would steer to the far start);
the fade in, the 1 s fade and the 0.2 removal are unchanged. So skaters keep riding until the 120 m cull.
Moddable: `SkaterConfig::line_chain`, `LivingWorldSettings::skater_line_chain`,
`sdk.world.set_tuning('living_world', {skater_line_chain = {radius, max_candidates}})` (radius 0 = fade at every
line end), reset on mod disable. The debug readout of the NPC skaters now uses `report_due` (it went silent after
a map reload).

**Files.** `crates/skate-core/src/living_world/replay.rs` (`ChainConfig`, `choose_next_line`,
`LineSource::for_each_line`, `LineCursor::chain`), `leave_fade.rs` (fade only at a dead end), `config.rs`
(`line_chain`), `replay_tests.rs`; `crates/skate-game/src/living_world/npc_skaters.rs`, `mod.rs` (settings),
`npc_tests.rs`; `crates/skate-game/src/modding/world_tuning.rs`; `crates/skate-mods/src/world_tuning.rs`,
`api.lua`; `sdk/skate.lua`; `crates/skate-data/tests/living_world_data.rs`.

**Verification.**
- skate-core: `npc_skater_chains_to_a_line_starting_within_4_m_like_sub_8246c7f8` (chains on the frame the end is
  reached, onto node 0, mirrored by a client; in-use and out-of-radius lines skipped; radius 8 / 0 from data),
  `line_end_choice_scores_like_the_branch_chooser_and_falls_back_to_the_first`,
  `npc_skater_keeps_riding_a_seeded_line_network_for_minutes` (seeds 1, 7, 1234: 5 minutes, never ends, two runs
  identical), `npc_fades_out_only_at_a_dead_end_and_goes_below_alpha_0_2`.
- skate-game: `living_world_npc_skaters_chain_lines_and_fade_only_at_a_dead_end` (no fade while a next line
  exists, chains at the line end, leaves by the distance cull; radius 0 gives the retail fade at every end),
  `..._ride_their_lines_from_the_spawn_record` (cursor rebuilt from the host's records), tuning test extended.
- skate-data (data-gated, exported lines): 1,691 rides, 19,948 branches and chains, 440 dead ends within 2 minutes
  (before: more than 1,600 ended within their first line).

**Open.** Line validity per character (`sub_82456970`) is not applied to branches or chains. Retail's fallback to a
far start (the full skater steers there) is not possible in the replay tier: 440 of 1,691 solo rides still meet a
dead end within 2 minutes and fade there. What the timed state on byte `+59` is (likely a bail) is unconfirmed.
Not seen in game yet.

## NPC skaters snapping between animations, 2026-10-05

**Problem.** User: "npc skaters look better, but they still snap between animations, there is no interpolation
between them."

**Cause.** The replay tier shows one stock clip per replay phase and swapped clips on the frame the phase changed
(rolling to air, air to rolling, ground trick, off board, also when a branch or line chain changes the phase). The
cursor only knew the current phase, so there was nothing to blend from.

**Evidence (retail).** Ambient skaters are full skaters on the same motion graph as the player (design doc 26), so
a clip change is the graph's transition: [code] Blend82B96058 (`playback_transition`, already ported for the
player) blends the outgoing tree into the incoming one with weight `elapsed / time` clamped to 0..1 and eased
`3w^2 - 2w^3` when `time` is 0.05 s (`0x3d4ccccd`) or more, linear below; the outgoing tree keeps playing
(matching mode 0). [data] `time` comes from each `PlayAnimation`; without one it is the literal at `0x82099280`,
0.2 s. The looping idle states the puppet clips stand for enter with 0.2 s (`air.xml` `B_AIR_CYC`,
`offboard.xml` stand `BLENDSEC`, `T_handplantair.xml` `IA_IDLE_N_N_0_CYC`). Across the stock motion graph the
times are 0.1 (119), 0.2 (89 plus 80 without a time), 0.05 (39), 0.3 (26), 0.15 (10, landings), up to 0.85 s.
No separate animation path for far AI skaters was found (no LOD animation strings besides `PhysicalPlayerHiLOD`).

**Change.**
- `skate-core::animation::playback_transition::transition_weight(elapsed, seconds)`: the player's transition
  weight, moved out of `PlaybackTransition::weight` unchanged so the NPC puppet uses the same code.
- `skate-core::living_world::replay`: the cursor keeps the phase it left and when it began; `ReplaySample` carries
  `previous_phase` and `previous_phase_frames`. Both follow from the cursor alone (a client mirrors them).
- `npc_skaters::present_pose`: while `transition_weight(phase_frames / 60, seconds) < 1` the pose is
  `[outgoing clip at its own time, incoming clip, Blend {weight}, RIG_TPOSE, Add]` (the player's SQT blend);
  same clip on both sides = no blend. A mod clip that does not evaluate falls back to the shipped pick on both
  sides. `NpcPuppetClip::blend` shows the crossfade in effect.
- Moddable: `LivingWorldSettings::skater_blend_seconds` (phase id or `default` -> s, empty = 0.2 s), Lua
  `sdk.world.set_tuning('living_world', {skater_blend_seconds = {default = 0.3, air = 0.1}})`, validated
  (known phase or `default`, 0..10 s), first writer wins per key, readable via `world_tuning`, cleared by
  `reset_mod_overrides()` on mod disable. Mod clips blend the same way.

**Files.** `crates/skate-core/src/animation/playback_transition.rs`, `crates/skate-core/src/living_world/replay.rs`,
`replay_tests.rs`, `crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `mod.rs`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`, `api.lua`, `sdk/skate.lua`.

**Verification.** skate-core `living_world`: 105 passed (new: the cursor keeps the previous phase; the weight
curve is 0 / 0.15625 / 0.5 / 0.84375 / 1 at 0 / 0.05 / 0.1 / 0.15 / 0.2 s, linear below 0.05 s); skate-core
`animation::` 44 passed (player transitions unchanged). skate-game: `..._puppet_blend_follows_the_transition_curve`,
`npc_skater_blend_seconds_set_merge_and_reset`, and data-gated `..._puppet_clip_change_has_no_pose_jump` (every
pair of puppet clips: the biggest per-frame joint move across the change stays within the clips' own motion plus
one curve step of the gap; worst old snap 0.94, worst blended frame step 0.13). Not seen in game yet.

**Open.** Per-transition times: the replay tier does not run the graph, so every phase change uses the stock
default 0.2 s (retail landings use 0.15 s, grind entries 0.3 s, many `INTO` clips 0.1 s; the simulated tier gets
them from the graph). A phase change during a running blend starts a new blend from the previous phase's clip
alone (retail nests the running transition). Jitter between 60 Hz ticks is a separate cause (fix 12 report).

## NPC skaters jittery while skating, 2026-10-05

**Problem.** User: "The skater npcs look decent but a little jittery when they skate."

**Cause.** Three presentation faults of the replay tier, none in the line data:
1. The root was placed between ticks, but the clip time and the crossfade weight advanced in whole 60 Hz frames
   (`phase_frames / 60`), so at 144 Hz the body pose repeated frames under a smoothly moving root.
2. The render extrapolated past the current tick with a look-ahead cursor that never branches
   (`Decider::Stay`), while the player is drawn from the previous to the current fixed state. At a branch or chain
   the look-ahead rode the old line, then the next tick corrected it.
3. A branch or chain moved the line under the skater at once: on the exported lines (212 seeded rides, 881
   switches) the gap is p50 0.46 m, p90 2.8 m, p99 6.7 m, max 16 m (branch targets far from the branch node).

**Evidence (retail).** Retail has no snap and no blend time here: [code] at a line end `sub_8246D3C0` ->
`sub_8246C7F8` only stores the new line id and node (`+592/+600`, `+816`, fix 9), and the branch choice
`sub_8246BEE0` likewise only picks a line and node; the full skater keeps its physical position and its
`AIPhysicsInput` steers onto the new line (not traced in the recomp for this fix). The orientation inside a segment is
already nlerped between the two nodes' decoded quaternions (continuous at nodes), unchanged.

**Change.**
- `skate-core::living_world::replay`: `LineSwitch` (frame, drawn offset, drawn skater and board orientation at
  the switch, including a blend still running); the drawn root decays from it onto the new line with the graph
  transition curve (`transition_weight`) over `ChainConfig::blend_seconds` (default `SWITCH_BLEND_SECONDS` = the
  stock graph's default transition time 0.2 s, an engine stand-in for retail's steering; 0 = cut).
  `ReplaySample::sub_frame` (render fraction). `LineCursor::render_sample(lines, records, frames_ahead)`: steps
  whole frames with the recorded branch decisions (`Decider::Mirror`), never guessing, then samples the fraction.
  All of it follows from the cursor, the branch records and the fraction (a client draws the same pose).
- `npc_skaters`: `NpcReplay::previous` (cursor one world tick back, taken before the tick's last step);
  `present_pose` draws `previous.render_sample(branches, fraction)` like the player's previous-to-current
  interpolation; clip time and crossfade use `(frames + sub_frame) / 60` (`puppet_blend_at`). The cursor's
  `switch_blend_seconds` is set from the tuning each step.
- Moddable: `sdk.world.set_tuning('living_world', {skater_line_chain = {blend_seconds = 0.5}})` (0..10 s,
  validated, first writer wins, read back, reset on mod disable with the rest of `skater_line_chain`).

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`,
`crates/skate-game/src/living_world/npc_skaters.rs`, `npc_tests.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-data/tests/living_world_data.rs`, `sdk/skate.lua`.

**Verification.** skate-core `living_world` 107 passed (new: `npc_skater_render_is_smooth_across_branches_and_chains`
renders a ride with a 0.8 m / 30 deg branch and a 1.5 m chain at 144 Hz: every root step stays below the line speed
plus one smoothstep step of the gap, the turn likewise, clip time grows by exactly 1/144 s, the cut ride jumps,
identical when mirrored; `npc_skater_render_is_smooth_on_a_seeded_line_network`, seeds 1 / 7 / 1234). skate-game
`living_world world_tuning` 50 passed (new: `living_world_npc_skaters_render_smoothly_between_ticks_and_across_chains`,
40 s at 144 Hz, worst root step below 0.075 m, clip time on the render clock; tuning test covers `blend_seconds`).
skate-mods `world_tuning` 3 passed. Data-gated `npc_skater_render_is_smooth_on_the_exported_lines` (pm3 export):
worst render step while a switch blends 0.75 m (the 16 m gap), 15.9 m cut; elsewhere 0.36 m. Not seen in game yet.

**Open.** Large branch gaps (p99 6.7 m) still slide fast over 0.2 s; retail would steer (and hand over to the nav
mesh beyond its 4.5 / 6 m corridor). A gap-scaled blend time or the simulated tier would cover it; mods can raise
`blend_seconds` now. The 8-bit node quaternions (about 0.45 deg steps) are interpolated as recorded, not smoothed.

## NPC skaters switching the side they stand on, 2026-10-05

**Problem.** User: "NPC skaters look much better and seem to skate much smoother. They still have weirdness with
switching sides they are standing on their boards randomly". In the user's capture an NPC rolling on flat ground
turns its whole body round in under 0.1 s while the board keeps rolling the same way.

**Root cause.** The replay tier draws the recorded skater orientation as the puppet root. The lines are recorded
human runs, and some recorders rode fakie at places (the skater frame faces against the travel direction): DownTown
34 of 760 lines start fakie and 40 end fakie, and branch targets also land inside fakie stretches. At a branch or a
chain the NPC took the new line's recorded facing, so the 0.2 s switch blend turned it 180 deg. On the exported
lines (212 seeded 40 s rides) 130 of 882 switches spun the skater round, about one per ride, which reads as random.

**Evidence (retail).** [code] A switch stores only the new line and node (`sub_8246D3C0` -> `sub_8246C7F8`
`+816`, `sub_8246BEE0`; fix 9 and 14); the full skater keeps its own body, so its facing and stance carry onto the
new line and only a performed trick or revert turns it. [code] The path frame retail builds from a node is the board
orientation (node `+0x18`, `sub_82453970`) turned 180 deg about its up axis when the node's `m_IsBoardFlipped` bit
(flags `+0x28` bit 0) is set: `sub_82453A58` and the interpolating `sub_824734A8` (called from the controller update
`sub_8246D560` into controller `+272`) negate the frame's X and Z rows. [data] With that turn the board frame matches
the recorded skater frame (flag clear: 20,195 of 21,191 moving nodes within 22 deg; flag set: 11,679 of 12,653 at
180 deg). Stance (regular or goofy) is not stored per node.

**Change.** `skate-core::living_world::replay`: `LineCursor::facing_flipped`; at a branch or chain, when the new
line's recorded skater faces more than 90 deg away (about +Y) from the drawn skater, the cursor toggles it and rides
the line turned 180 deg about the skater's own up axis (`turn_about_up`, `q * (0, 1, 0, 0)`, retail's board-flip
turn), applied to the skater and the board orientation in `sample` and in the switch blend. Recorded turns on a line
(180s, reverts) still play, from the facing the skater has. A pure function of the lines and the branch records (a
client mirroring the records derives the same flip). Moddable: `ChainConfig::keep_facing` (retail `true`),
`sdk.world.set_tuning('living_world', {skater_line_chain = {keep_facing = false}})` takes the recorded facing
(validated, first writer wins, read back, reset on mod disable); the cursor copies it each step like the blend time.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`,
`crates/skate-game/src/living_world/npc_skaters.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-data/tests/living_world_data.rs`, `sdk/skate.lua`.

**Verification.** skate-core `living_world` 108 passed (new `npc_skater_keeps_its_facing_across_a_line_switch`: a
chain onto a line recorded fakie keeps facing +Z on every frame, the line's recorded revert still turns it,
mirrored client identical, `keep_facing = false` spins as before). skate-game `living_world world_tuning` 50
passed (tuning test covers `keep_facing`), skate-mods `world_tuning` 3 passed. Data-gated
`npc_skater_keeps_its_facing_across_switches_on_the_exported_lines`: 212 rides, 882 switches, 130 kept by a flip,
0 spins (130 without `keep_facing`), deterministic. Not seen in game yet.

**Open.** The replay puppet plays one forward clip per phase; retail plays fakie riding clips while fakie and the
character's stance (regular or goofy) mirrors them. Recorded in-line facing changes on the ground without air (about
12 in DownTown) still turn the root within a node, where retail performs a revert. Both belong to the simulated tier.

## Frame drop with the board thrown away (hidden board scanned the whole map), 2026-10-05

- **Problem:** throwing the board and walking away dropped the frame rate to 4 to 20 FPS; calling the board back
  fixed it at once (user play test, 2026-10-05). The living world was not the cause: the log shows the population
  steady and physics at 60 ticks per second.
- **Root cause:** beyond `MaxDistance` (30 m) the board controller hides the board (state 3: parked 1000 m above the
  player, every board collision volume disabled). The solve keeps only enabled volumes and called
  `BoardWorld::query_primitives` with an empty list; with no volumes the search box was `None`, and
  `candidate_ranges(None)` means every triangle, so each tick walked all of the map's collision triangles for an
  empty result. The cost jumps once at the hide distance; it does not grow with distance.
- **Evidence:** a headless test on the real DownTown map (stock graphs, pad-driven: step off, drop, walk away)
  measured 2 to 3.5 ms per tick with the board dropped and 27 to 29 ms once hidden, 22 ms of it in that query.
- **Change:** `query_primitives` returns right after its buffer reset when the volume list is empty. Pure
  optimisation: the old loop gave the same empty result and the same buffer state. It covers every caller (board,
  skeleton, prop world). The bug is in shared physics and exists on `main`; it is fixed in this PR because walking
  away from the board is part of the living world's tests.
- **Files:** `crates/skate-core/src/physics/board_world.rs`, `crates/skate-core/src/physics/board_world/tests.rs`,
  `crates/skate-game/src/physics/board_away_tests.rs` (new), `crates/skate-game/src/physics.rs` (test module).
- **Verification:** `empty_volume_query_is_empty_and_resets_the_previous_result` passes with and without the early
  return (identical behaviour); the ignored data-gated `hidden_board_tick_costs_no_more_than_a_dropped_board`
  passes with the fix (2.73 ms hidden vs 2.66 ms dropped) and fails without it (29.3 ms). Not yet confirmed in game.
- **Open:** in one run a board lying 24 m away cost about 1 ms more per tick than at 0 to 10 m, which looked tied
  to where it landed rather than distance; not investigated.

## Peds walking through props, 2026-10-05

**Problem.** User: "Peds ... also completely ignore the props in the world and go right through them." (Same for
NPC skaters: "npc skaters ... also ignore props in the world and go right through them."; ped side fixed here,
NPC skater side planned below.)

**Cause.** Ped navigation (M3) only knew the static NavPower navmesh and the other peds. The props (#15's dynamic
props, the DMOs) live in the physics' prop layer and were never handed to the peds, so a path or a step could run
through a bin, bench or barrier.

**Evidence (retail, [code], TU3 recompilation as reference).**
- Every world object carries an obstacle interface: register `sub_82595140` (entered through the base slot
  `sub_82595010` at vtable + 4) and update `sub_82595298`. Slot +68 says "I am an obstacle": 1 for the
  DynamicObject class (vtable `0x82322D50`, next to the string `dynamicobject` at `0x82322D2C`), for the player
  skater (`0x82300910`) and one more class (`0x823220E0`); 0 for peds (`0x8232BE80`) and the other actors.
- A DynamicObject takes its record from `DynamicObject Obstacle MemStore` (slot +76 `sub_82C57268`, record
  `sub_82C46778`, vtable `0x82322D10`). The box is its collision body's (slot +96 `sub_82C486F8` -> body vfunc 160
  half extents; slot +92 `sub_82C48688` orientation). Slot +88 `sub_82C48648` turns the obstacle off while the
  object's state word (`+144` component, `+4252`) is 1 (meaning not decoded; we use "carried").
- `sub_82C477B0` (each update): half extents below 0.2 m become 0.2 (`0x82099280`); faster than 0.4 m/s
  (`0x82181B90`, slot +24 = velocity) the cut is removed and NavPower's moving avoider (`+64`, `sub_82E99998`)
  takes over; at rest the box is cut into the NavPower mesh (`sub_8293EF28` with `{-1, 15.0, type 4}`, then
  `sub_8293F340`) and re-cut only after it moved more than 0.25 (`0x820C6D98`) x its smallest half extent.
  NavPower plans every ped bot on the cut mesh (its planner reflects `obstacles` and `dynAreas`), so retail peds
  walk round resting props, a moved prop counts where it lies, a carried or rolling one is not in the mesh.
- The PEDCOLL session (`pedcoll_20261004_215239`) logs only ped vs skater reactions (6 lines); it holds no prop
  data, so it validates nothing here.

**Change.**
- `skate-core::living_world::peds::obstacles` (new): `ObstacleParams` (retail defaults: on, 0.2 m, 0.4 m/s,
  0.25; ours: detour margin 0.1 m, max 8 detour corners, step height 0), `ObstacleInput`, `Footprint` (the box
  projected onto the ground as an oriented rectangle plus its height span), `NavObstacles` (stable ids in a
  `BTreeMap`, retail's cut / re-cut / moving / carried rules in `update`, a 4 m grid rebuilt only when a cut
  changes, `version`; queries `blocked`, `first_hit`, `detour`, `step_ok`, `resolve_step`).
- `peds::wander`: `PedNav::step_avoiding` (the old `step` = no obstacles): a probed target inside a cut does not
  fit (retail: no snap on the cut mesh), each new path gets detour corners round the cuts it crosses (the
  cheapest free corner of the grown rectangle on the mesh), and when `version` changes the rest of the path is
  checked again. `probe_fan_clear` / `choose_target_clear`.
- `skate-game::living_world::peds`: resource `PedObstacles`, system `update_ped_obstacles` (FixedUpdate, before
  `advance_peds`) feeds every prop body (`PropDynamics::obstacle_boxes`: box centre, basis, contact half
  extents, velocity, carried) and every mod body (`modding::bridge::obstacle_solids`: world AABB, the attached
  body carried; ids above 2^40); `advance_peds` steps with `step_avoiding` and never steps into a body
  (`resolve_step` slides along its face or the ped stays, then re-plans after 3 s as before).
- Moddable: `LivingWorldSettings::ped_obstacles`, Lua `sdk.world.set_tuning('living_world', {ped_obstacles =
  {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, step_height}})`, readable via
  `world_tuning:living_world`, reset to retail on mod disable. Mod-spawned bodies are obstacles like props.
- Multiplayer: the obstacle state is plain data keyed by stable ids, updated once per population tick in id
  order; ped steps stay a function of records, tick, mesh and the obstacle list.

**Files.** `crates/skate-core/src/living_world/peds/{obstacles,obstacle_tests}.rs` (new), `peds/{mod,wander}.rs`,
`crates/skate-game/src/living_world/{peds,peds_tests,mod}.rs`, `crates/skate-game/src/physics/prop_dynamics.rs`
(`obstacle_boxes`), `crates/skate-game/src/modding/{bridge,world_tuning}.rs`, `crates/skate-mods/src/world_tuning.rs`.

**Verification.** skate-core `living_world::peds`: 29 pass (6 new: retail cut rules incl. 0.2 m minimum, 0.4 m/s,
0.04 m keeps / 0.06 m re-cuts, carried and removed, no new version while props rest; footprint orientation and
height; a ped walks round a bin and reaches its target, the same walk without obstacles crosses the bin; a prop
kicked onto the path at 2 s counts where it lies; 6 wandering peds among 12 props never inside one and identical
twice; step check lets a ped out of a prop pushed onto it). skate-game `living_world` + `physics::prop`: 62 pass
(new: 4 peds wander the plaza among 12 bins and a bench, never inside one, identical twice; obstacle inputs from
mod bodies and the Lua patch set / reject / reset). skate-mods `world_tuning`: 3 pass. Not seen in game yet.

**Open.** NavPower's moving avoider (`+64`) is not ported: a rolling prop is only solid for the step check. The
record's `-1` / 15.0 (all-movers blockage flags / penalty x15?) are not decoded; cuts block. The meaning of the
state word that switches the obstacle off is inferred. NavPower cuts polygons; we bend paths at rectangle corners.
The funnel can return a first corner behind a ped spawned within 1.5 m of the mesh edge (seen in a test, M3 code,
not changed here).

### NPC skaters riding through props, 2026-10-05 (fix 19)

**Problem.** User: "They also ignore props in the world and go right through them." (NPC skaters; peds were fixed
above.)

**Root cause.** Our NPC skaters are replay puppets. Their kinematic proxies (body capsule + board) joined the
player's contact solve, so the player bumps them, but not the dynamic prop step: `GamePhysics::step_props` got only
the local skater's board and skeleton volumes (`physics/solve.rs`), so no prop ever felt an NPC.

**Evidence (retail).** [code] NPC skaters are full skaters: an `AIController` drives the player's skater class
(AI skater vtable `0x82306320` has the player's slots, notes `npc-skaters-re.md` section 0), so their board and
body hit a DynamicObject (`0x82322D50`) like the player's. The AI controller also has an `ObstacleAvoider`
(controller `+80`, ctor `sub_824658C0`, gatherers `sub_82463C08` / `sub_82464000` / `sub_82464448` /
`sub_82464968` adding through `sub_82463A40`, mode pick `sub_82465578`, read by the PathController update
`sub_8246D560`); which object kinds it scans and what its modes do is not decoded, so it is not ported.

**Change.** The NPC's body capsule and board (as a capsule along the deck), at the recorded velocity, are push
volumes in the prop step, by the player's contact rule and each prop's own tuning (`props` domain: push mass,
transfer, caps). Both count as a board hit (the prop's `board_push_speed` cap): the lower body-bump cap is for the
player's walking body, which the prop's triangles stop; a puppet is not stopped, and a retail NPC rides into the
prop with its whole skater at riding speed. A bin on a line is knocked ahead of the NPC; a bench is shoved out of the
line (weight-scaled). The NPC itself does not slow down or bail (puppet; see open items). Each pushed prop records
the stable actor id that last pushed it (`PropDynamics::pushed_by`: 0 = local player, NPC = its proxy solid id),
the authority owner a future host uses; the push order is the local player first, then NPCs by id
(deterministic). Moddable: `sdk.world.set_tuning('living_world', {npc_skater_props = {enabled = false}})`,
retail on, restored on mod disable; push strength per prop type stays in the `props` domain.

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs` (`step_with_actors`, `LOCAL_PUSHER`, `pushed_by`),
`crates/skate-game/src/physics.rs` (`GamePhysics::actor_prop_volumes`, `step_props`),
`crates/skate-game/src/living_world/npc_skaters.rs` (`NpcSkaterPropContact`, `prop_volumes`, `push_proxies` fills
the volumes each tick), `living_world/mod.rs` (setting), `modding/world_tuning.rs` (apply + inspect),
`crates/skate-mods/src/world_tuning.rs` (`NpcSkaterPropsPatch`), `skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.** Seeded headless tests: `npc_skater_knocks_a_bin_on_its_line_out_of_the_way` (control run without
NPC volumes leaves the bin still; with them the bin ends more than 1 m ahead, never behind the board, owned by the
NPC), `npc_skater_bench_push_is_deterministic` (two runs identical bit for bit, bench pushed along),
`far_npc_leaves_props_alone_and_local_push_owns_it`, `living_world_npc_skater_prop_volumes_and_mod_switch`.
Not checked in game yet.

**Open.** (1) The ObstacleAvoider (slow down, stop or steer for an entry ahead) needs decoding first: trace hooks on
`sub_82463A40` (object added, its vtable = class) and `sub_82465578` (mode `+6000`, speed `+5996`), then an
avoidance input on the replay cursor. (2) Retail NPCs can bail on a heavy prop; our puppet bulldozes it instead.
Full report: `.local/research/npc/fix19-skater-props.md`.

## Peds walking in place at fixed spots, one standing on a wall top, 2026-10-05

**Problem.** User: "a bunch of them got stuck walking over some type of border and seem to still have pathing
issues." / "it just looks like a seam or perhaps an error in the navmesh at that spot? hard to tell, they just get
lined up and start walking in place in specific spots. Its shown in several videos" / "there is also a random woman
floating at the end of this one which also shows the people walking in place issue". Video: at the DownTown ramp
with the long blue rail (player near [-254, 41, 100]) a woman stands animating on the top edge of the tall wall for
8+ s.

**Root cause** (four ped navigation faults, one render feedback; tags as above):
1. **Tile seams.** The NavPower tiles stop short of their shared border, about 0.12 m each side [data]: at the
   user's spot polygon 7551 ends at z 99.9 and 6035 starts at z 100.14. Setup links the two sides, but a step was
   kept on the mesh by locating its end point alone; a point in the gap snaps to the closest edge, which for the
   first half of the gap is the edge the ped just left. A ped at a seam was put back each tick (counted as "on the
   mesh", so it never re-planned) and walked in place; peds heading the same way lined up behind it. 4449 seams in
   DownTown; the old rule gets across 188 of them.
2. **One-sided seam links.** Setup's stitch links the short polygons along one tile border to the long edge across
   it, but the long edge records at most one of them (6034's border edge links none; 7553 and 7554 link to 6034).
   Paths and steps from the long-edge side could not use those links.
3. **Corners left early.** Path corners are mesh boundary vertices (the mesh is already shrunk by the agent radius).
   A ped took the next corner as soon as it was 0.5 m from the current one; from the wrong side of the vertex the
   straight line to the next corner runs into the boundary edge head on, so the step slid along the edge by almost
   nothing. 415 walking-in-place episodes in a 30 ped x 120 s run round the ramp with 1 fixed, 64 with 1 to 3.
4. **Side-steps into walls.** A ped walking round another one side-stepped at 90 degrees even inside a 0.24 m wide
   strip (7626), into the boundary (31 episodes left before this was fixed).
5. **Floating.** The render ground probe (a line from 3 m above the ped down) wrote its hit back into the navigation
   position. DownTown has 661 separate navmesh components, many single wall-top or planter polygons over the ground
   (6092 at y 33.9 over 6104 at y 30.9); the probe could hit such a ledge, and the point query (any polygon under
   the point within 4 m, before any edge point) then put the ped on the unconnected wall-top layer, where it walked
   in place.

**Retail.** The ped's NavPower bot (`ped+5940`) moves over the bot's own polygon graph: the mover hands it the
destination (`sub_82C47378`) and reads its steering back (`sub_82C47198`) [code, pm3]; reachability is the graph's
connectivity bitset (`sub_82C464F0` -> `sub_82926BA8`) [code]. A bot does not leave its connected graph and the
runtime joins tiles across their borders. NavPower's path follow and steering internals are not decoded; the rules
below are ours, stated as such.

**Change.**
- `NavMesh::move_along` (new): a step moves over the polygons linked to the ped's own polygon (breadth first round
  the move): inside one of them, the step is taken at its height; in the strip across a linked edge (a seam gap,
  within the snap radius) the step is kept; otherwise it slides onto the closest edge. Never onto an unconnected
  layer. `NavMesh::point_on`, `NavMesh::clear_line` (straight walk over the surface).
- Links in both directions per edge (`edge_links`): a one-sided stitched link is added to the other polygon's
  closest edge; A* and the portals use them.
- `NavMesh::locate`: a polygon under the point more than the agent step height (0.2 [data], agent block) away in
  height competes with nearby edge points by vertical plus horizontal distance (a ground point in a seam gap stays
  on the ground, not the wall top above).
- `PedNav`: `poly` (the polygon the body stands on, stable per ped); a corner counts as passed within the corner
  radius only once the next corner is in straight reach over the surface; a side-step needs one agent diameter of
  room, else the ped yields (and re-plans after the patience, as before).
- `constrain_step` uses `move_along`; `constrain_move` takes the tracked polygon.
- Game (`living_world/peds.rs`): steps from the tracked polygon; a step that slid to under a quarter of the wanted
  distance counts as refused (re-plan after 3 s instead of walking in place); spawns go onto the navmesh at the
  record's height first (ground probe only without a navmesh); the ground probe is render only, in a window of one
  agent height (1.6 [data]) up and down round the navmesh height.

**Moddability / multiplayer.** No new tuning: the values come from the navmesh's agent block (step 0.2, height 1.6),
which a mod map's navmesh sets (`NavMeshInput.agent`); corner radius and patience stay in `WanderParams`. Pure
functions of the mesh and the ped's state, ties broken by polygon index; `PedNav::poly` is plain data.

**Files.** `crates/skate-core/src/living_world/peds/{nav,wander,nav_tests}.rs`,
`crates/skate-game/src/living_world/peds.rs`, `crates/skate-data/tests/ped_nav_data.rs`.

**Verification.** skate-core `living_world` 109 passed (new `steps_cross_tile_seams_and_never_jump_layers`, with the
old rule as a control). Data-gated on the user's DownTown export: `downtown_tile_seams_are_crossed_without_layer_jumps`
(4449 seams: crossed 4193, old rule 188; layer jumps 0, old rule 47; the rest are seams into slivers a few cm wide
that a straight walk leaves through their boundary), `downtown_peds_at_the_ramp_never_walk_in_place` (30 peds x
120 s round [-254, 41, 100]: 0 episodes of 4 s walking without getting 0.3 m, none leaves its connected surface);
the other ped_nav_data and living_world_data tests pass. skate-game `living_world` 43 passed. Not checked in game.

**Open.** Setup's stitch itself could record both sides (re-export); NavPower's path follow / steering and local
avoidance are not decoded; 1-polygon islands are still valid spawn points if a census record lies on one.

## Ped clothes drawn in their mask colours (looked like mixed outfits), 2026-10-05

**Problem.** User: "after further review some of the ped models are still not right." / "they aren't squished
anymore though" / "The three women at the start of this video are definitely not wearing the right clothes". The
women (log readout: `female_granny_1`, `female_teenager_3`, `female_skater_1`) wore a dark red / brown coat or top
with bright blue shorts, skirt or trousers; `male_teenager_1` and `female_business_3` (later in the same video) wore
the same flat red top and blue trousers. User on `female_business_3`: "Im pretty sure this npc isn't being rendered
corectly".

**Root cause.** Not mixed parts: every ped recipe holds one body (`Rostral`) and at most one `Hair` part, one mesh per
LOD [data, all 51 recipes], and each ped in the video matches its own GLB. The red and blue are the atlas's tint
masks, drawn raw:
1. The loader read model fields `tints_a` / `tints_b` that the export never had (the fields are
   `secondary_colours` / `chassis_colours`), so every ped got the white default pair.
2. Nothing drew the tints (M2 left the shader mask undecoded).

**What retail does.**
- `sub_827B4170` reads, with one rand `r`, `secondary_colours[r % n]` (`Hash_DF76D7D773857EDB`, stored at +80 of
  the ped's 112-byte presentation slot, `sub_827B3BF8`) and `chassis_colours[r % m]` (`Hash_12026E2EED18CC8D`, +96)
  [code].
- Ped body materials are type `pedestrian_high_stamp` (one LOD of one recipe `pedestrian_low`); hair and pro
  clothes are `marquee_hair` / `marquee_cloth` / `cac_alpha` [data, recipe XML]. The ped pixel shaders
  (`shaders_final.big`: `livingworld_stamp_defaultPS`, `defaultlivingworld_defaultPS`; read with an offline Xenos
  microcode decode of the `ucode.h` layout) both do [code, shader]: `lin = diffuse^2`; when `G^2 < 0.001225` (a
  literal) the texel becomes `R^2 x i_colorize_red + B^2 x i_colorize_blue`, else `lin`; lighting; output
  `sqrt`. Constant table: `i_colorize_red` c22 / c15, `i_colorize_blue` c23 / c16.
- Which tint feeds which constant [data]: the root `default` model record holds secondary `(1, 0, 0)` and chassis
  `(0, 0, 1)`; only secondary -> red, chassis -> blue makes that the identity. The CPU upload of the two constants
  was not traced (the parameter handles at `0x830B9C10` / `0x830B9BF4` are only referenced by their static
  initialisers, which register them by name through `sub_82531428`); the table values are taken as they are.

**Change.**
- `skate-core::living_world::peds::colorize`: the shader rule (`colorize_texel`, `colorize_rgba8`,
  `MASK_GREEN_SQ_MAX`, `colorized_shader`); `choice`: doc of the field mapping, `PedOverrides::model_tints` (a mod
  replaces a model record's palettes; same one-rand pick; `PedOverrides::default()` restores retail).
- `skate-data::ped_anim`: `tints_a` = `secondary_colours`, `tints_b` = `chassis_colours` (raw hash keys accepted).
- `skate-game::living_world::peds`: `present_ped_tints` (after `present_ped_looks`, before the pose): bakes the rule
  into a copy of the body diffuse per (material, tint pair), shared by peds with the same pair, freed with the last
  user (weak ids); `ped_material_colorized` picks materials by the export's material type, or the `Rostral_` body
  slot for GLBs exported before the type was written.
- Converter: `living_world.recipe_shaders` reads `<mat id type>` from each recipe's XML twin into `models.json`
  (`shaders`); `living_world_models.write_glb` writes it as material extras `{"shader": ...}`. A mod GLB opts a
  material in with the same extras.

Multiplayer: the pair follows from the spawn record's seed (unchanged draw order), the texture from the pair.
Moddability: palettes are table data (content overlay of `secondary_colours` / `chassis_colours` by model record),
`PedOverrides::model_tints` by model record id, material opt-in by extras; Lua surface with the `sdk.living_world`
ped calls (mod milestone).

**Evidence / verification.** Renders (`render_glb.py --ped-look MODEL:INDEX`, same rule) of the five models in the
video, untinted vs three palette entries each: coherent outfits (grey / white / tan tops, dark jeans, a brown or
green coat over a checked skirt). Tests: skate-core `living_world::peds` 34 passed (colorize rule, mod palette
override); skate-game `living_world` 45 passed (material selection); skate-data `ped_anim_data` 5 passed on the user's
tables (root pair is the identity, every ped look takes its model's palettes); asset_pipeline living-world unittest 32
OK. Not seen in game yet. Setup: old exports work through the `Rostral_` fallback; a `livingworld` refresh writes
the exact material types.

**Separate items, not this cause.** `female_business_3` has no `Hair` part (her ponytail is in the body mesh and
renders with it offline); her flat, washed-out head in the video is not a missing part (likely lighting on our standard material; not checked).
`male_teenager_1`'s grey shoulder patch is the sleeve trim, a non-mask region of his atlas (the same in the GLB);
his stiff arms-out walk is the animation side (open).

## NPC skaters popping between animations, tricks not playing, 2026-10-05

**Problem.** User (after fixes 12 to 19): "NPC skaters still seem to pop between animations and trick animations
appear to be missing or not playing." / "they pop between the animations, one ends and suddenly the next is already
partially going, their limbs jump to each position between making them appear to pop between animations" / "When
they jump nothing plays, i can hear the sounds of a tick, but the board doesn't do the trick and their limbs do not
animate like they are doing the trick."

**Root cause.**
1. The puppet crossfade (fix 12) only knew ONE previous phase. On the shipped lines 13,183 of 27,464 phase changes
   come before a 0.2 s blend could finish [data] (crouch phases are 0 to 2 frames, a takeoff about 10, an air trick
   about 13). Each such change restarted the blend from the previous clip alone, dropping the running blend: the body
   jumped to that clip's pose in one frame (measured up to 0.83 m per joint on a jump).
2. Every recorded jump opens its trick span on the ground (most common sequence rolling > ground trick > air trick >
   air [data]), and the ground trick phase showed the 50-50 grind clip: the takeoff flashed a grind pose.
3. The node's trick slot was never read for animation: the air trick phase played an air idle clip
   (`IA_IDLE_LO_N_0_CYC`, the board turns 3 deg), so no flip, no board trick.

**Retail.**
- [code] The trick slot is the 40 B extended record's i16 at +0x24, an `EScorableID`: `sub_8246A2E0` (0x8246A4A0)
  loads `*(node+0x20)`, then `lhz 36(ext)`, `extsh`, and treats `> -1 && < 332` as a trick (332 = the scorable
  table 0x820862A8, `skate_core::scoring::catalog`). On the shipped lines: ollie 1925, gnd_bsgrab 829, kickflip 734,
  bsgrab 409, fsgrab 365, nollie 322, n_heelflip 322, heelflip 264, then grinds [data].
- [code] Ambient skaters are full skaters on the player's motion graph (doc 26 section 1); a graph transition keeps
  the running transition as its outgoing tree (Blend82B96058, fix 12), so blends nest.
- [data] `MotionGraphIncludes/Tricks/Tricks.xml` pairs each `TRICK_NAME` with an `ANIM_NAME`; `T_Trick.xml` /
  `T_Ollie.xml` / `T_Kickflip.xml` play `$ANIM_NAME$_G` on takeoff (`time="0.05"`), then `$ANIM_NAME$_A` with
  `transType="sequence"` (follows the ground clip, no blend; `time="0.1"` when entered from the air); the kickflip
  family continues with `$ANIM_CYC_NAME$1..3` and `$ANIM_OUT_NAME$n`. The `B_*` names are authored trees in the stock
  banks (phase blends over `TRICKHEIGHT`, selectors over `PROSKATER`), e.g. `B_KICKFLIP_IN_A` = phase blend of
  `KICKFLIP_IN_LOW_A` / `KICKFLIP_IN_HIGH_A`. The board is the rig's `Skateboard_Root`, so the trick clip moves body
  and board together (kickflip air sequence: board turns 169 deg).
- [video] Retail (RPCS3, `2026-10-05 13-01-52.mp4` 3:23.6, a varial heelflip from the same trick trees): the board
  turns under a continuous body over about 0.4 s (`.local/research/npc/fix21/retail-varial-heelflip-board.jpg`).
  NPC skaters in that recording are too far from the camera to read limb detail.

**Change.**
- `skate-core` `living_world/replay.rs`: `ReplayLine::{open_trick_at, node_trick}`, the cursor keeps the span's trick
  id and a 6-entry `PhaseEntry` history (phase, start frame, trick), `LineCursor::phase_history()`,
  `render_cursor()` (the cursor `render_sample` samples). `ReplaySample` unchanged.
- `skate-game` `living_world/npc_skaters.rs`: `puppet_layers` builds the pose from every phase whose blend still runs
  (oldest first, each blended over the result, stops at the first fully blended one; consecutive phases on one clip,
  or a clip continuing a `<ground>+<air>` sequence, are one layer from the older start); `puppet_layers_pose`
  evaluates them (`PoseCommand::Blend` chain); `puppet_layer_clip` picks per phase: a trick span with a trick
  animation plays `<base>_G+<air sequence>` from the ground (0.05 s in) or the air sequence alone from the air (0.1 s
  in), the air after an air trick continues it, everything else the phase clip as before; `retail_trick_anim`
  (EScorableID -> `B_<NAME>` per Tricks.xml; kickflip / heelflip and nollie forms `B_<NAME>_IN`; numbered, late,
  underflip, dark-catch variants their base flip or the ollie; grabs and other air tricks the ollie takeoff; grinds,
  slides, manuals, powerslides, reverts and handplants none), `trick_air_sequence`, `stock_tree_leaf` (selector
  default, phase blend first child = low `TRICKHEIGHT`), `sequence_part` (`+` parts back to back).
- Moddable: `skater_clips["trick.<scorable name>"]` = an animation base (or a plain clip), `skater_blend_seconds`
  keys `trick_takeoff` / `trick_air` (`skate-mods` validation, `sdk/skate.lua`); reset on mod disable with the rest
  of `LivingWorldSettings`. Multiplayer: the pose is a function of the cursor (lines, branch records, tick), no new
  records.

**Files.** `crates/skate-core/src/living_world/replay.rs`, `replay_tests.rs`; `crates/skate-game/src/living_world/npc_skaters.rs`,
`npc_tests.rs`; `crates/skate-mods/src/world_tuning.rs`; `crates/skate-data/tests/living_world_data.rs`; `sdk/skate.lua`.

**Verification.** skate-core `--lib living_world` 114 passed (new `replay_cursor_keeps_a_phase_history_with_the_span_trick`),
`--lib animation::` 44 passed; skate-game `--bin skate3rust -- living_world world_tuning` 56 passed (new
`living_world_npc_puppet_layers_nest_running_blends`, `..._trick_slots_pick_the_stock_trick_animation`, data-gated
`..._trick_clips_exist_and_flip_the_board`: every mapped tree resolves to a stock clip, kickflip board 169 deg vs 3 deg,
and `..._trick_jump_has_no_pose_pop`: on recorded jump timelines the pose at a phase change leaves the showing pose by
0.008 m, the one-previous-clip blend by 0.83 m); skate-mods `world_tuning` 3 passed; skate-data `living_world_data`
8 passed (new `npc_skater_trick_slots_and_phase_changes_on_the_exported_lines`). Not seen in game yet.

**Open.** Trick height: retail blends low/high by `TRICKHEIGHT`; the NPC plays the low clip (the line's jump record
has a launch velocity that could give the height). Grabs show the ollie (retail adds the grab from the action graph,
`T_Grab.xml`); grind / slide / manual spans still show one 50-50 clip (the slot names the grind: next step); spins
(`spins` byte) and landing clips (`B_LAND_*`, 0.15 s) are not played; the per-skater `PROSKATER` selector takes the
default branch.

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

Lua (API 2, capability `world_tuning` = 1): `sdk.world.set_tuning(domain, patch)` patches a typed domain while
the mod runs (`nil` restores the mod's patch), `sdk.world.tuning(key, domain)` reads the values in effect. Every
field is optional and defaults to the shipped value; per field the first mod to write it wins. Patches are held
per mod in `WorldTuning` (serialisable JSON) and each change rebuilds the domain into its one authority resource.
When a mod stops, fails or reloads its patches go (`modding::world_tuning::clear_owner`; all mods gone:
`clear_all`).

| Domain | Fields (shipped value) | Resource |
|---|---|---|
| `living_world` | `npc_draw_distance` (1.0, 0.25..4), `skater_fade {fade_in_seconds 1, fade_seconds 1, despawn_alpha 0.2}`, `ped_fade {distance {45, 55}, fade_in_seconds 1, enabled true}` (a model record's own pair still wins), `skater_clips {[phase or phase.Style] = clip}` (empty = shipped picks), `skater_clips["trick.<scorable name>"] = trick animation base` (empty = Tricks.xml picks), `skater_blend_seconds {[phase or default or trick_takeoff or trick_air] = s}` (empty = 0.2 s; tricks 0.05 / 0.1 s), `skater_line_chain {radius 4, max_candidates 16, blend_seconds 0.2, keep_facing true}` (line end chaining; root blend onto the new line after a branch or chain, 0 = cut; keep the skater's facing across switches), `ped_obstacles {enabled true, min_half_extent 0.2, moving_speed 0.4, recut_fraction 0.25, detour_margin 0.1, step_height 0}` (props and mod bodies as ped obstacles), `npc_skater_props {enabled true}` (NPC skaters push dynamic props) | `LivingWorldSettings`, rebuilt via `reset_mod_overrides()` so the player's menu draw distance returns |
| `props` | `default` / `by_template[<MOBJ template>]`: every `PropTuning` field plus `collision_box {center, half_extents}`; a template entry starts from the patched default | `PropTuningSettings` |
| `carry` | `grab_bit` (28, RB), `placement_bit` (20, B), `grab_range` (2.0 m) | `CarrySettings`, pushed into `PropCarry` each tick (survives map loads) |

Not exposed yet: road district selection for mod maps (the loader picks the district by map name), census range
overrides (`data_config`), per-kind density / ambient skater count. The ped mirrored-animation fix has no values.

## Credits

skate3recomp by @mchughalex (rexglue SDK, Xenia), the reference for how the retail code is used; DumbadsSkate3ModdingTools by Ethanw05 (credits to SunJay,
Dumbad, RenderWareGavin and Tuukkas) for the AIPATH field names, NavPower constants and trigger types, used as a
format reference, no code copied; @andrewnakas' `mx/vehicle` fork as prior work on a (player-driven) vehicle Lua API,
described, not copied.

## Open questions

- Props look (todo, D9): all 40 prop materials use retail's `dynamicobject.default` / `dynamicobject.alphatest`
  shader, which the renderer does not support yet (log: "40 of 40 world materials use an unsupported shader family
  and render as family 1"). Retail draws an extra layer over the base texture (dents, grime, rust on dumpsters and
  trash bins, recomp comparison 2026-10-07). Port the retail shader program. Research and plan: doc 27,
  "D9 research".
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

### Peds floating in the air all over the map (2026-10-06)

User: "There were MANY floating peds during my last play session. Not on geometry that is visible, literally floating int he air." / "IT IS NOT JUST ON PROPS GOD DAMNIT, IT IS ALL OVER THE PLACE".

- **Cause:** the ped render height (fix 17) took the first hit of a line from one NavPower agent height (1.6 m) above the navmesh down to 1.6 m below. Any collision within 1.6 m overhead (awnings, ledges, signs, invisible collision) won, so the ped was drawn standing on it. Video 2026-10-06 10-00-44 at 49 s and 51 s.
- **Change:** the upward search is the NavPower step height (agent block [2], 0.2 m [data]); NavPower keeps its polygons within one step of the walkable floor. Downward stays one agent height. Not retail yet: retail's own ped render placement is not decoded.
- **Evidence:** data test `ped_render_ground_does_not_lift_onto_overhead_geometry` over all 36,443 DownTown polygon centres: drawn more than 0.3 m above the navmesh at 195 polygons before, 0 after.
- **Files:** `crates/skate-game/src/living_world/peds.rs` (`ground(up, down)`), `peds_tests.rs`.
- **Logging (always on):** `PED_FLOATING` (warn, every 2 s per ped, once per ped per 10 s) when a ped is drawn more than 0.3 m above the floor under it or over no floor: ped, model, drawn position, navmesh height, floor height, gap, polygon and area code. The ped readout (count, nearest ped, player position) now runs every 2.5 s without debug mode. User: "im tired of you saying you can't see the floating pedestrains". A data check found 167 walkable DownTown polygons (areas 17 and 161) more than 1.6 m above the collision floor or over none; the log names them when a ped walks there.
- **Peds spawned at the player's height (2026-10-06):** the logs showed nearly every floating ped had no navmesh polygon (32 of 34, then 44 of 44). The census ring point carries the observer's height (`census::ring_point`); where the navmesh has no floor within `locate_height` (4 m) of it, the ped kept that height and hung in the air or under the ground. With a navmesh, such a spawn is now released like an unresolved look and the census tries another point next pass. Not retail yet: retail's spawn validation is not decoded. Session after the fix (about 6.5 minutes): 0 `PED_FLOATING`, the ped count stayed at its cap (14 to 15).
