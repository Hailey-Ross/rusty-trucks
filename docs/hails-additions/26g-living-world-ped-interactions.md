# 26g: Living world: Pedestrians: props and skater contact

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

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
- Open (2026-10-07): user report "Moving objects does not update the collision for peds, untested on skater
  npc's". The sessions had no obstacle lines, so logging came first: `PED_OBSTACLE` (a prop more than 0.1 m from
  where it was first seen: spawn, now, cut and cut centre, speed, moving, carried, footprint, version; on every
  change), `PED_OBSTACLES` (inputs, props, cut, moving, carried, moved; every 2 s with a moved prop, else 10 s) and
  `PED_BLOCKED` (a refused ped step into a body, or within 3 m of a moved prop's spawn spot; once per ped per
  second). Core helper `NavObstacles::blocker_at`. Session 2026-10-07 22:57 answered it: 7 of 8 moved props fell
  through the floor once released (13 m to 8-10 m at 7-9 m/s) and never rested, so they were never cut; the one
  that stayed on the floor was re-cut at its new spot (moved 1.12 m, speed 0) and the user saw "the peds stopped
  and then pathed around it". The ped side works; the cause is props sinking (open, prop dynamics).
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

## Peds walking through a held prop, 2026-10-08

**Problem.** User, verbatim: "Moving objects does not update the collision for peds, untested on skater npc's",
then (2026-10-08) "I wasn't recreating the issue last night, that may have just been while I was holding the
object, which is still wrong". So peds walk through a prop while the player holds it with Move Object.

**Cause.** The port treated "held" as retail's obstacle-off gate (the state word below), so a held prop was
neither cut into the walkable area nor solid for a ped's step. The retail code says otherwise.

**Evidence (retail, [code], TU3 recompilation as reference).**
- The obstacle-off gate is DynamicObject slot +88 `sub_82C48648`: off while the component at `+144` reports
  `+4252 == 1` (component vtable `0x82322ED8` slot 31 `sub_82C56AE8`). That word is set to 1 only by component
  slot 32 `sub_82C56B00`, which also moves every collision element of the object to collision group 13; slots 33
  `sub_82C56BA0` / 34 `sub_82C56C70` set it back to 0 (group 12 or 14 by a per-type value). No direct caller of
  slot 32 was found; what state 1 means is still not decoded.
- The Move Object hold does not touch it. Each held tick the state calls interface slot 10 (keep-alive, type 2
  record), flushed to the DMO handler's slot 6 `sub_82C4C450`, which calls component slot 30 `sub_82C485D0` with 1:
  that only sets the held bit `DMO+4464 & 0x20` (getter slot 29 `sub_82C485C0`). The obstacle update
  (`sub_82595298` -> `sub_82C477B0`) never reads that bit; its "force moving" input is DMO slot +104 = 0.
- So in retail a held prop stays an obstacle: while it moves faster than 0.4 m/s (`0x82181B90`) its cut is
  removed and NavPower's moving avoider takes over (`sub_82E99998`: an 88-byte record in the planner's obstacle
  database with position, velocity and a radius of 0.35 x a planner-wide value, independent of the box; moved every
  tick by `sub_82E998C8`, removed by `sub_82E99BB0`); held still (or slower) it is cut where it lies, and re-cut
  after it moved more than 0.25 x its smallest half extent. After release the same rule cuts it where it rests.
- How NavPower's bots steer round a moving avoider is middleware internals (the database is only reached through
  generic query functions in `0x82EA8000..0x82EAC000` from NavPower code, no Skate-side reader); not decoded.

**Change.**
- `skate-core::living_world::peds::obstacles`: `ObstacleInput::held` (separate from `inactive`, which stays the
  retail off word), `ObstacleState::held`, `ObstacleParams::held_is_obstacle` (retail true: a held prop or an
  attached mod body stays an obstacle; false = the earlier "held is ignored" rule, mod option).
- `ObstacleParams::moving_solid` (default true), NOT RETAIL YET: stands in for the NavPower moving avoider. A
  moving object is solid for a ped's step (`step_ok` / `resolve_step`: slide along its face or stay, then re-plan),
  it does not bend paths. False = moving objects do not block a ped's step; this is the switch a decoded avoider
  port replaces.
- `skate-game::living_world::peds`: `prop_obstacle_inputs` (props: held = `PropDynamics::held`, never inactive);
  mod bodies: the attached body is held, not inactive.
- Logging: `PED_OBSTACLE` now prints `held`, `off` (the retail off word), `role` (`cut`, `solid`, `none`, `off`)
  and `cut_off` (distance from the cut centre to the body centre, -1 without a cut), on every change of cut /
  moving / held / off, on a re-cut, and once a second while held, so a dragged prop's body pose shows next to its
  cut pose. `PED_OBSTACLES` adds `off=` next to `held=`. `PED_BLOCKED by=` names a held prop that refused a step.
- Moddable: `ped_obstacles {held_is_obstacle, moving_solid}` in the `living_world` tuning (Lua
  `sdk.world.set_tuning`), readable via `world_tuning:living_world`, reset on mod disable.
- Multiplayer: plain data by stable id; the held flag comes from the authority's carry state.

**NPC skaters and moved props.** Retail AI skaters are full skaters: their board and body hit a DynamicObject
by the same rigid-body contact as the player (fix 19 ported this: our puppets push props, a moved prop is pushed
where it lies now). Their `AIController` also runs an `ObstacleAvoider` (controller +80) with four gatherers;
2026-10-08 they read three pools of one world container (globals `0x8308549C` = container +16 for the 8 m
gatherer `sub_82463C08`, `0x830854A0` = +2704 for the 20 m `sub_82464000`, `0x830854A8` = +22416 for the 16 m
`sub_82464448`; filled by `sub_826BBC40`); which object kinds those pools hold and what the avoider's modes do is
NOT decoded, so a moved prop does not change an NPC skater's line in our port (NOT RETAIL YET; no speculative
slow-down added).

**Files.** `crates/skate-core/src/living_world/peds/{obstacles,obstacle_tests}.rs`,
`crates/skate-game/src/living_world/{peds,peds_tests}.rs`, `crates/skate-game/src/physics/prop_dynamics.rs`
(test), `crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`, `sdk/skate.lua`.

**Verification.**
- New test `physics::prop_dynamics::tests::peds_do_not_walk_through_a_held_prop_and_see_it_where_it_rests`
  (headless, real Move Object carry path on the street fixture): a bin (half 0.35 x 0.5 x 0.35) is dragged
  toward a ped walking head-on along its line for 1 s, held still 3 s, released and left 3 s; obstacles come from
  `obstacle_boxes` through the game's input mapping each tick, the ped steps with `resolve_step`. Retail rule:
  0 ped steps deeper into the prop (past the 0.25 x 0.35 m re-cut tolerance); control (`held_is_obstacle = false`,
  the earlier rule): 8. Held and moving faster than 0.4 m/s for 65 ticks (solid), held and cut for 173 ticks.
  After release the bin rests 2.94 m from its spawn, cut at its new spot (cut centre 0.022 m from the body,
  inside the 0.0875 m re-cut tolerance), obstacle version 3; the new spot blocks and a path leg through it hits the
  cut, the old spot is free and a leg through it does not.
- New core test `living_world::peds::obstacle_tests::held_prop_stays_an_obstacle` (held still = cut, dragged at
  1 m/s = no cut but solid, `moving_solid = false` and `held_is_obstacle = false` switches).
- `living_world_ped_obstacle_inputs_and_mod_tuning`: the attached mod body is held (not off); the Lua patch sets
  `held_is_obstacle` / `moving_solid` and mod disable restores the defaults.
- Suites (`--release --locked -j 4`): skate-game `living_world physics::prop modding::world_tuning` 108 passed,
  3 ignored (asset tests); skate-core `living_world` 128 passed; skate-mods `world_tuning` 3 passed. Not run in
  game.

**Open questions.** NavPower's moving avoider (how bots steer round it) is not ported; ours blocks the ped's step
instead. The meaning of the off word (state 1, collision group 13) is not decoded. The NPC skater
`ObstacleAvoider` pools and modes are not decoded. Not seen in game yet: drag a prop into a ped's walk in DownTown
and check the `PED_OBSTACLE` lines (`held=true role=solid` while dragging, `role=cut` held still and after it rests).

## Skater hits peds: knock-down or stumble (2026-10-08)

**Problem.** Skating into a ped did nothing. Retail peds fall over (animated knock-down, then get up) or stumble.

**What retail does** [code, TU3; recomp disassembly as reference; `.local/research/peds/skater-ped-contact.md`]: the ped
contact callback `sub_82E38FB8`, kind 5 (an `IActor` toucher such as the skater):
- inputs: the length of the ped body's linear and angular velocity after the contact solve, gated by entity
  `C04236FB548697D0` / `797AA1D5F828819B` (0 for every stock entity);
- knock-down when the ped's animation set allows it (field `5B92564B352A9FAA`, off for the four marquee sets) and a
  speed is above `541FFA2E9D81C947` / `7C5E39ECE5A5572E` (3.0 / 3.0); otherwise a standing stumble unless the brain's
  `+3277` bit 0x10 makes it immune. The 6.0 thresholds and the "no reaction" cases belong to a ped touching a ped
  (toucher cast to `IPedestrian`, `sub_826C2C70`);
- direction: the contact normal (flat) against the ped's forward, `acos`, negated when `cross(forward, normal).y > 0`,
  in degrees: FromBack within 35, FromLeft 35..145, FromRight -145..-35, FromFront beyond (constants `0x8206D148`,
  `0x822F9428..30`; names from the table `0x830218B0`);
- reaction [data, `motiongraph_collision.xml`, `template/knockdown.xml`]: a stumble plays CollisionBack / Fwd /
  LeftStanding (FromLeft mirrored, 0.3 s blend); a knock-down plays WipeoutBack / Fwd / LeftFall (0.4 s), the ground
  cycle until the AI's `Recover`, then the get-up (0.1 s). Ground time: animation-set field `AD3C483F0C9DAD67` (1.5 s,
  security 0.5 s), read by `sub_8269A990` in `PedestrianColliding`'s update.

**Change.** `skate_core::living_world::peds::skater_contact` (decision, direction, reaction steps);
`PedAnimPlayer::react` and the `Reaction` locomotion state (steps in order, the ground cycle for the ground time, then
idle; no navigation meanwhile); `PedAnimSet.collision` filled from the export; `advance_peds` checks every observer
against every ped, logs `PED_SKATER_CONTACT` and sends `PedEvent::Hit` (mods: `on_event` `ped_hit`).
NOT RETAIL YET: the skater is a 0.35 m cylinder at the observer (2 m tall) and the ped body's post-contact speed is
the skater's speed into the ped (no Havok ped body); the normal is taken as pointing into the ped; where the ground
timer starts is not pinned (here: on reaching the ground cycle); the ped is not pushed aside.

**Verification.** skate-core: 3 `skater_contact` tests (buckets, 3.0 rule, marquee, immune, steps) and
`a_knock_down_falls_lies_for_the_ground_time_gets_up_and_walks_on` (90 frames on the ground at 60 Hz). Data-gated on
the user's export: default 3.0 / 3.0, allowed, 1.5 s; marquee not allowed; security 0.5 s; all 12 reaction animations
resolve to clips in the ped bank. skate-game `living_world_a_skater_knocks_a_ped_down_or_makes_it_stumble` (6 m/s
knock-down, 1 m/s stumble, one reaction, back to locomotion), `living_world_messages_become_mod_events` (`ped_hit`).
Not play-tested yet.
