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

## Ped behaviour runtime: the stock ped AI graph on each ped (2026-10-09)

**Problem.** Peds only wandered and reacted to contact; retail peds also warn, flee, observe, chase and take
the skater down. That behaviour is mostly data: the stock ped AI graph and the mood tables.

**Retail [code] [data] (`.local/research/peds/b1-ped-behaviour-runtime.md` to `b5-ped-mood-events.md`; main
checked the key addresses).** Retail runs the ped AI graph on the same dynamic controller as the skater's graphs
(vtable `0x82321EA8`, ctor `82C12E88`, created through `82C12DD0`); every ped condition goes through the masked
activation gate `82C12D48`; behaviour slots 13 / 14 / 15 are Begin / Update / End. The shipped
`Pedestrian.stategraph` is already compiled (includes and templates expanded). The ped brain holds wants
(`brain+412+want*12`: target, flag 0x80 "needs addressing"), timers (`82E40940` / `82E40A80`), the honker
(`+3232`) and the scatter bit (`+3281` 0x80). Wants come from the mood system: per-ped mood records (presence of a
player within 35 m every 0.5 s, collisions, tricks; `sub_82E41060`, at most 10 records) and the producer
`sub_82E41B98` that rolls the ped type's moodresults (`rand() % 100 + 1` against the probability,
`sub_8269A588`).

**Change (step 1 of the runtime).**
- skate-core `living_world::peds::brain`: `PedBrain` (wants, timers, flags, outputs) and `BrainHost`, the
  graph host. Ported operations: `HasSpecificWantThatNeedsToBeAddressed`, `NeedsToBeginColliding`,
  `IsPedestrianColliding`, `ShouldScatter`, `IsBeingHonkedAt`, `IsZombieMode`, `HasPlugin`,
  `DistanceToWantTarget` (squared compare), `Wander` (`8269F380` / `8269F410`: motion intent 6, 2.0 m/s),
  `NoRoadWander` (`826A2FB8`), `SuggestVelocity`, `KnowAboutWantTarget` (30.0), `UnsetWant`, `UnsetWantOnEnd`,
  `ChannelWarnWantTarget` (`8269F7B0` / `8269F980`: warn timer 36 = 3.5 s, `0x82063AB8`, then the want goes),
  `Flee` (`826A3530` / `826A3760`: motion intent 4, the flee target), markers. Every other name is `Pending`:
  false / no effect, listed at load (`PED_GRAPH loaded: ... not ported yet: ...`).
- skate-core `living_world::peds::mood`: the mood record store (post / bump / at most 10 / tick / expiry) and the
  presence scan (35 m). Not wired yet: the field binding for magnitudes and lifetimes is still open.
- skate-game `living_world::ped_graph` loads the stock ped AI graph with the ped data; `think_peds` runs each
  ped's graph once per population tick before the bodies step (host only), logging `PED_BRAIN` on every state
  change. Mod values `ped_brain {enabled, wander_speed, warn_seconds, know_about_seconds}`.

**What changes in game now.** Nothing visible yet: no want is produced until the mood system is wired, so every
ped runs the graph into its wander state (`NoRoad`) and the body keeps its current navigation.

**Verification.** skate-core `brain` tests (4) and `mood` tests (3). Data-gated
`living_world_ped_ai_graph_runs_on_the_brain`: the stock graph loads, binds and compiles (202 behaviours, 149
conditions); an idle ped reaches `NoRoad` with the wander intent; a flee want takes it to `Fleeing` (intent 4,
fleeing from the target); once the target is more than 15 m away it releases the want and wanders again.
Muted DownTown run (90 s): `PED_GRAPH loaded: 202 behaviours, 149 conditions; 115 operation names not ported yet`; all 36
peds went `InitialState -> NoRoad` with the wander intent; no panic or error.

**Open.** The flee movement (`826A35F8` is a chain of navmesh calls with 1/6, `0x822F8604`; until it is decoded
the body does not run away), the mood field binding and the want producer, the rest of the operations (chase,
takedown, taze, observe, speech events, crosswalk and road ops), the ped motion graph's own operations.

## Ped mood system: wants from the stock mood tables (2026-10-09)

**Retail [code] [data] (`.local/research/peds/b4-ped-want-producer.md`, `b5-ped-mood-events.md`,
`b6-ped-mood-fields.md`; main checked the producer call sites, the roll, the presence constants, the magnitude
getter hash and its fallback).** Peds keep mood records (`brain+1040`, at most 10, keyed by instigator, second
entity and category): presence of a player within 35 m posts every 0.5 s (`sub_82E3CA20`, category field `B1DD`,
which is also the per-post magnitude), a collision posts `collision` to the hit ped and `nearbycollision` to the
others (`sub_82E41060` rewrites the category when the "ped itself" prerequisite fails). Records age and expire
after the category's lifetime (`A379`, 30 s); magnitude and count never decay. The producer (`sub_82E41B98`,
evaluator `sub_82E41EB0`) takes the ped type's reaction results by priority and checks, in order: target,
category, instigator type, magnitude (`C404`/`8239`), Nth bump (`B1A9`/`0982`), second-entity type, hand prop,
two target checks, outstanding reactions (`7FF7`), cooldown, prerequisites, no pending want; the winner is rolled
(`rand() % 100 + 1`), every roll starts its cooldown, and a pass raises its wants on the target and suppresses
the record (`4EC2`). Discrete categories have no `B1DD`: the attribute default at `0x830D0850` is 0.0 (the field
research named 0x820D0850 and 3.5e13; corrected).

**Change.** skate-core `living_world::peds::mood`: tables, store, presence scan, prerequisites and the producer,
all values from the tables. skate-game `living_world::ped_mood` reads them from setup data (`tables.json`, parents
resolved; each entity type's reaction set by field `712E`); `think_peds` ticks each ped's records, posts presence
and the skater collisions (`PedEvent::Hit`), runs the producer with a per-ped seeded RNG and raises the wants on
the ped's brain before its graph runs. Log `PED_MOOD` (result, category, roll, wants), `PedEvent::Mood`; mod switch
`ped_brain.mood` (setting, mod patch later).

**Engine choices / inferred.** Prerequisite kind 3 measures to the instigator, kind 6 asks whether the second
entity is the ped (consistent with the collision rewrite); the undecoded target checks, hand-prop bit and
ped-state prerequisites answer false; the outstanding count is the number of raised wants about the instigator;
the sight test is not applied; collisions name the first player as instigator; the evicted record when full is
the oldest.

**Verification.** skate-core `mood` tests (4). Data-gated `living_world_ped_mood_tables_warn_then_chase_on_collisions`
on the stock tables (27 categories, 88 results, 17 reaction sets): an adult male hit by the skater does nothing
on the first hit, raises `warn` on the second (`skatercollisionwarn`, Nth 2) and `angrychase` on the third
(`skatercollisionchase`, Nth 3); a bystander gets neither.
Muted DownTown run (120 s, the player standing at the spawn): `PED_MOOD tables: 27 categories, 88 results, 17
reaction sets, 110 entity types`, 45 peds wandering, no reaction (a default ped's presence results need about 10 s
within 12 m or a hand prop; the rest are ped-to-ped greets, whose presence posting between peds is not done yet).

**Warning stops the ped and faces the skater.** Ported from `.local/research/peds/b7-ped-reaction-movement.md`
(main checked the constants): `StopAndFaceWantTarget` (Begin saves the speed suggestion; Update faces the want
target flat, speed 0.0 `0x82165A10`; End restores), `WatchWantTargetWithoutInterruptingLocomotion` (keeps walking
while the target is within 55 deg of the facing, `0x821DBCF4`, else latches into stop and face until End),
`TurnToFaceSkater`, `StandAndWatchSkater` (watch point 4 m ahead, `0x82257308`; jumps to the skater's position +
velocity x `predictTime` 2.0 when the directions differ, dot < `maxAngle` 0.4), `IsFacingSkater` (dot > `FOVAngle`).
The body stands and turns toward the face point while the speed suggestion is 0 (turn rate: our nav's 45 deg/s
until the motion graph's turn branches run). Data-gated graph test: a warn want takes the ped to
`PedestrianDoWarning`, speed 0, facing the skater. So in game: a ped hit twice by the skater stops and faces them
for 3.5 s; hit a third time it raises a chase (chase movement not ported yet).

**Open.** Flee movement (the steering goal provider, research running), chase movement, speech events, the
undecoded checks above, presence posting between peds (greets, conversations), other posters (trick, slam, chase,
noise, greeted).

## Fleeing peds run away from the threat (2026-10-09)

**Retail [code] (`.local/research/peds/b8-ped-flee-steering.md`; main checked the 15 m leg `0x820BD16C`, the 1 m/s
speed gate `0x8231A844` and the 0.5 blend `0x8209975C` in `sub_82E318F8`).** The Flee intent installs a goal
provider (`sub_82E35E80`) with a 2.0 m arrival radius. Each leg goes 15 m from the ped: straight away from the
threat when the threat moves slower than 1 m/s or the ped is behind its motion, else `0.5 x (side + away)` (side =
the threat's motion x up, on the ped's side; not renormalised), then a navmesh cast. A new leg starts when the ped
is within 2 m of its goal (`sub_82E2BC70`). The speed is the ped type's chase record field `CD65` (11 for
pedestrians), sent to the motion graph as a run gait (`sub_82E2D260`).

**Change.** skate-core `living_world::peds::flee` (direction, goal, arrival; `FleeParams` with the retail values);
the ped animation gains the run gait (`Intent::Run`, `Locomotion::Run`, logical clip `FwdChaseRunCyc`, the stock
`*_CHASE_RUN_N_0_CYC`, hash `7746273E01422734`). `think_peds` turns a fleeing brain into nav legs (a one-point
route per leg, cleared when the flee ends; a blocked leg ends at the last clear point along it, our stand-in for
`sub_82C46208`), logged `PED_FLEE`; `advance_peds` runs where the nav walks while fleeing. Settings
`ped_brain.flee` (retail values).

**Engine choices.** The run's own start, stop and turn clips are not wired (it enters from standing or walking and
stops through the walk stop) until the motion graph runs; the speed is the run clip's root motion, not the chase
field; a mod route on a fleeing ped is replaced by the flee legs and cleared after.

**Verification.** skate-core `flee` tests (2: straight away 15 m with arrival at 2 m; blended to the side for a
threat coming at the ped, straight away from behind it). Full workspace run: only the 4 known pre-existing
failures. Not seen in a game run yet (a ped flees only on a raised flee want; for default peds that comes from the
female reaction set's `fleeperp` after repeated hits).

**Open.** Which stock reactions raise `flee` for which ped types in practice (play-test), the run start / stop
clips, retail's exact blocked-goal rewrite.

## Peds speak from their AI graph (2026-10-09)

**Retail [code] (`.local/research/peds/b9-ped-speech-events.md`; main checked every address below in the TU3
recomp).** A graph speech value is not a sound request. `SendSpeechEvent` Begin (`sub_826A3508`) calls ped vfunc
+204 with the op's `speechvalue` (float attribute truncated to an int; `speechevent` is only a label); that virtual
(`sub_82E22798`) stores it at `ped+2468`. The ped constructor (`sub_82E33198`) starts the field at 68, which no line
uses. `ChannelWarnWantTarget` Begin (`sub_8269F7B0`) sends 53 through the same virtual (54 when the ped's `+2000`
component answers non-zero; that query is open). The audio side's PedestrianSpeech asks for a line only when the
value changes, so a state re-sending the same value stays silent. `SendChaseStateMessage` and
`EmotionalResponseToGivingWarning` post game messages and do not speak.

**Change.** skate-core brain: `PedOp::SendSpeech { value }`, `PedBrain::speech` (the `ped+2468` value, `None` = the
constructor's 68), the warn op writes `BrainSettings::warn_speech` (53). `think_peds` sends a changed value as
`PedSpeechEvent` to the audio side (which already ports value -> line, cooldowns and the pick), logs `PED_SPEECH`
and posts `PedEvent::Speech`. Mods: `ped_brain.warn_speech` (0..=127) and the living-world event `ped_speech` (`id`,
`value`, `state`).

**Engine choices.** Only a change is sent, so the audio bridge's "repeat through 0" rule (kept for the Lua `speech`
call) never fires for graph speech. The listener-side gating (15 nearest peds within 50 m) stays in the audio port.

**Verification.** skate-core `speech_ops_store_the_value_on_the_ped` (truncation, default 0, Begin only, warn 53);
skate-mods patch validation for `warn_speech`; `living_world_messages_become_mod_events` covers `ped_speech`.
skate-game living-world tests: 67 passed. Not heard in a game run yet.

**Open.** The `ped+2000` query (53 vs 54); value 55 (most likely a speech value from StartChase Update `826A3780`, inferred) has no mapping in our
speech manager; value 15 from NewChasee (`826A37F0`) maps to 607_chase_join and arrives with the chase port.

### Graph timers: SetSimpleTimer and SimpleTimerExpired (2026-10-09)

**Retail [code] (main read these in the TU3 recomp).** `SetSimpleTimer` (factory `826C9E88`, Begin `826A2810`) and
`SimpleTimerExpired` (factory `826CFA50`, condition `826ACFF0`) read `timerName` (default "TimedRangeRandom") and
`length` (default 0.0). The name maps to one of 47 timer indices (`sub_82E42B08`, e.g. 21 StartChase, 42
TargetUnreachableTimer; unknown name = 47). The brain's timer map (`sub_82E40940`) removes a timer set to 0 or less
and drops a new timer when 14 are running; reading a timer that is not running gives 0 (`sub_82E40A80`), so it
counts as expired. These ops are used 13 and 16 times in the stock graph (chase hold, alert, sit, wander waits).

**Change.** skate-core brain: `timers::NAMES` / `timers::index`, `PedBrain::set_timer` / `timer`, ops
`PedOp::SetSimpleTimer` and `PedOp::SimpleTimerExpired`; the warn op uses the same timer rules.

**Verification.** skate-core `simple_timers_follow_the_retail_timer_map`.

## Ped chases: chase record, chase groups and the intercept (2026-10-09)

**Retail [code] (`.local/research/peds/b10-ped-chase-takedown.md`, `b11-ped-intercept-takedown.md`,
`b12-ped-chase-group.md`; main checked the addresses named here).**
- Chase record: each ped type points at a `livingworld_entities_chase` record (entity field `FBC4...`; escape
  distance 65 m for pedestrians, 500 m security, run speed `CD65` 11 m/s, max lead time `47C9` 6 s, give up after
  `78E2` takedowns). The chase manager (`*0x830854B8`) holds the `global` record (predict angle `5D48` 22.5 deg,
  max chasers `CEA5` 5) and the "chases allowed" switch (`+6332` bit 0x10, on at init `826B41B0`).
- Chase group: every chasee (the player's actor and every ped) has a group of up to 25 chasers in join order; entry
  0 is the primary chaser (`82D97078`). Joining (`82D96C38`) refuses duplicates and full groups and resets the end
  reason on the first join; leaving the last chaser resets it too (`82D96E88`). `GiveUpBeingPrimaryChaser` swaps
  entry 0 with entry 1 and holds timer 29 at max(itself, `timeout`) (`82D97108`).
- Ops: `IsChasing` (`826AC470`), `ChaseeEscaped` (`826AC668`, 3D distance past the escape distance),
  `CanNewChaseStart` (`826AAC08`), `CanChaseeAddNewChaser` (`826ACAC8`), `IsPrimaryChaser`, `IsChaserEndingChase`
  (group reason, else the chaser's own), `ChaserShouldGiveUpDueToTakedowns` (`826AD690`), `NewChasee` (`826A37F0`:
  speech 15, takedowns 0, join the angrychase target's group), `StartChase` Update (speech 55), `ChaserEndChase` /
  `ChaserGroupEndChase` (reason: default 0, aggressivecapture 1, returntopatrolzone 2, lostinterest 3 via
  `sub_82BFEF30`, inferred), `EndChase` (`826A53F0`: leave, forget the chasee, steering speed 0).
  `ChaseeHasProtector` and `ChasersAreScared` answer false (only the actor has a protector interface, contents open;
  scared is never true in retail).
- Intercept (`826A3BF0` / solver `sub_82E3D4D8`): each tick, if the chasee is reachable, the flat intercept time
  `t` with `|Q + V t - P| = run speed x t`; no solution or `t` not below the max lead time gives no new goal. When
  the angle between (chaser - chasee) and the chasee's velocity is strictly between the predict angle and 180 deg
  minus it, the goal leads the chasee (`Q + V t`), else it is the chasee. Goal height = the higher of the two; nav
  speed = run speed, arrival 0.5 m.
- `ActivateRelatedWant` (`826A6210`): copies the original want's slot (`originalWant`) to `relatedWant` and flags
  it (`82E42A58(brain, want, 1)` writes the 0x80 needs-addressing bit); with `deactivateOriginalWant` (default
  true) the original's bit is cleared (joinchase -> angrychase). `ChangeNavModifierSetting` (factory `826A74C8`,
  Begin `826A75D8` saves the ped's setting and sets `set_to`, End `826A7658` restores it; modifiers ChaseGlue 0,
  Pedestrian 1, SkaterAvoidance 2, VehicleAvoidance 3).

**Change.** skate-core `living_world::peds::chase` (`ChaseRecord`, `ChaseGroup`, `intercept`, `escaped`, end
reasons); brain ops for all of the above with `ChaseRequest`s the host applies in order and a `ChaseView` (own id,
record, groups); `BrainSettings` gains `chases_enabled`, `start_chase_speech` (55), `new_chasee_speech` (15).
skate-game: `PedData.chase` / `chase_global` from `tables.json`, the host-owned `PedChaseGroups` resource keyed by
chasee id (chasers that leave the world leave their groups), `think_peds` applies the requests (log `PED_CHASE`,
`PedEvent::Chase` -> mod event `ped_chase`) and steers intercepting peds to the solver's goal with the run gait (log
`PED_INTERCEPT`). Brain ops `ActivateRelatedWant` and `ChangeNavModifierSetting` (`PedBrain::nav_modifier`); the
nav skips ped avoidance while the Pedestrian modifier is off.

**Engine choices.** The player's group reads the `global` record for its max chasers (the actor's own record is
not decoded). The solver takes the smallest positive root (`sub_82E15CD0`'s choice between two roots is not
decoded). The intercept's reachability and projection use our navmesh line and locate. GiveUpPrimary's trailing
reorder after the swap is not decoded. Nav modifiers default to on (per-type defaults open); only Pedestrian is
applied (ChaseGlue, SkaterAvoidance and VehicleAvoidance are kept but not used by our nav yet).

**Verification.** skate-core: `crossing_chasee_is_led_and_one_running_away_is_chased_directly`,
`record_fallbacks_and_escape`, `groups_keep_join_order_and_hand_over_the_primary`,
`chase_conditions_read_the_chasee_and_the_record`, `chase_group_ops_ask_the_host_and_read_the_group`; skate-game
`chase_records_merge_parents_per_entity_type` and the mod event test. Not seen in a game run yet; the takedown
itself (contact, success, the skater's bail) is not ported.

**Open.** Takedown (`b13-takedown-skater-side.md`: success sets a pending flag and direction on the player's actor;
the bail it causes is being researched), the takedown entry choice `82E3C000`, LostChasee / perception
(CanSeeChasee), RestFromChase, BlockChasee / PursueChasee, the protector, value 55's line.

## Ped takedowns and chase exhaustion (2026-10-09)

**Retail [code] (`.local/research/peds/b13` to `b17`; main checked the addresses named here and the setup data).**
- Choice (`82E3C000`, each tick from TakeDownTargetablePredictions): the ped type's `livingworld_entity_takedown`
  list (`generic_male` 7 entries) in priority order; approach side from `normalize(ped - target) . target
  velocity`; per entry the target `lead + 0.06` s ahead, at most 1.25 m above or below, distance from the reach point
  `ped + 0.2 x forward` inside `(min + 0.2 + 0.06 s, reach + 0.2 + 0.06 s]`, signed bearing inside the entry's
  angles (or mirrored). The first fit wins.
- Attempt (`pedestrian_attempttakedowntargetable.xml`): sustained by the ActiveTakedown intent, timer 16 = 3.0 s,
  speech 17; AttemptTakeDownTargetable Update (`826A4BE8`) succeeds when the ped touched its target during the
  takedown (the contact callback `82E38FB8` records the touch and counts it at `brain+3248`); success
  (`826A4E50`) calls the target's `vfn20(chaser position)`: on the player's actor (`82592390`) that sets `1904` bit
  30 and the direction away from the chaser (main checked; the actor always accepts, `8281DD70` returns 1). Speech
  65 for the player, else 19. Failure: speech 18.
- On the skater: the flag reaches the animation packet (P+10496, +10480; `82593640`), Processed +2468 bits 2 and 18
  (`82DB5BE0`, `82BDA0D0`); bit 18 is phys-out byte 59, which the stock motion graph reads (`IsPhysicsWiping`,
  `onboard.xml`) to enter WipeOut; WipeoutGround Enter (`82D3B5E8`) pushes skeleton part 12 along the direction
  scaled by the vault value `LivingWorldPushForce`.
- Exhaustion (`b17`): a float at `brain+3220` grows by the tick in InterceptChasee / PursueChasee; NeedToRest is
  `accumulator > exhaustion_limit` (`82E3D7B0`: peds 30 s) or resting and not rested (timer 4 still running).
  RestFromChase: accumulator 0, resting, timer 4 = rest time (`5960`, 5 s); End: not resting. ClearExhaustionTimer:
  accumulator 0. Investigate: timer 1 = investigate time (`1AE2`, peds 10 s), End sets 0, exceeded at 0.

**Change.** skate-core `living_world::peds::takedown` (`TakedownTable`, `choose`); brain ops
ChaserEvaluateWhoToTakeDown, TakeDownTargetablePredictions, HasTakeDownTargetable, CanAttemptTakeDownTargetable,
HasMonitoredIntent (ActiveTakedown), AttemptTakeDownTargetable, TakeDownAttemptSuccessful / Failed,
TakeDownTargetableSuccess / Failure, TakedownTargetableClearOnEnd, NeedToRest, RestFromChase, ClearExhaustionTimer,
Start / EndInvestigateTimer, InvestigateTimeExceeded; `ChaseRecord` exhaustion / rest / investigate getters.
skate-game: takedown tables from `tables.json`; the contact (our skater cylinder against the ped while the takedown
plays) sets the brain's contact and counts the takedown; a success writes `SkaterTakedownRequest` and
`apply_skater_takedowns` sets the skater's takedown latch, which the animation packet publishes as the external
impulse (flags 10496, vector 10480) until WipeoutGround Enter uses it. Logs `PED_TAKEDOWN`, `SKATER_TAKEDOWN`; mod
event `ped_takedown` (`id`, `target`, `success`).

**Engine choices.** No takedown clips play yet: the ped stands (SuggestVelocity 0) and the takedown intent ends
with the decision; the contact is our skater cylinder, not the ped body's Havok hit (`*(ped+2032)`, its installer
not found); the target never vetoes (`T.vfn24` byte +59 open); the latch drops after 2 s if no wipeout used it
(retail's clear of bit 30 not found); the ped's speed for the window is its position change.

**Verification.** skate-core `first_fitting_takedown_wins`, `a_takedown_attempt_succeeds_on_contact_with_the_target`,
`chasing_exhausts_and_resting_recovers`; skate-game data-gated
`a_ped_takedown_publishes_the_external_impulse_and_wipes_the_skater_out` (DownTown: our skater is in WipeoutGround
from the first tick after the latch, and the latch is used). Not play-tested.

**Open.** The takedown clips and the ped's own stumble on failure; the perception (CanSeeChasee, LostChasee) and
SetAltTargetToChaseePosition / mood suppression ops (research b18 running); Pursue / Block for secondary chasers.

## Ped perception, secondary chasers and tazers (2026-10-09)

**Retail [code] (`.local/research/peds/b17` to `b20`; main checked the setup data and the key addresses).**
- Perception (`b17`, `b18`): at most 5 entries per ped (`brain+624`), a new one evicts the oldest and starts "just
  told"; each tick memory and suppression run down, age up, the vision test (`82E27240`) sets "visible" and refreshes
  the last position: seen within 1 m (2 m height), or within the near range (20 m x the type's `D036`), or within the
  far range (30 m x `D238`) inside the 120 deg cone (`7435`) with a clear line of sight. CanSeeChasee = visible or
  just told; KnowAboutChasee = 30 s memory; SuppressMoodAboutChasee / UnsetWantAndSuppressMoodForATime block mood
  events about the target; MoodResetAboutChasee forgets them; LostChasee walks to the last known position.
- Secondary chasers (`b19`): every non-primary chaser runs UpdateBlockPrediction each tick; it blocks when the
  chasee is within 20 m and touching (1 m) or running at it faster than 5 m/s inside 45 deg (record fields
  `8BAE` / `F3E7` / `670A` / `E48B`); BlockChasee walks to the block point at 2 m/s and stands there facing the
  chasee (recovering exhaustion); else PursueChasee runs to its formation point = the chasee plus the offset it
  joined at.
- Tazers (`b20`): DrawTazer (0.133 s draw), TazeWantTarget (speech 66, after 0.3 s one hit through the target's
  `vfn20`: the same knock-down as a takedown, `826A8420`), RegisterTazer (live tazer), EndTaze (speech 67); the
  stock graph tazes within 20 deg and 10 m (IsFacingWantTarget: `826AA1B8`, half the angle either side, flat
  distance) with a line of sight.

**Change.** skate-core `living_world::peds::perception` (`Perceptions`, `sees`, `Sight`); `ChaseGroup` formation
offsets and `chase::should_block`; brain ops for all of the above (`ChaseSteer` Intercept / Pursue / Block),
`MoodStore::forget`. skate-game: per-type sight from `tables.json`, the perception tick in `think_peds` (eye 1.6 m
above the feet, line of sight = our navmesh line), suppressed instigators post no mood, Pursue / Block / LostChasee
routes, the taze hit writes the takedown's `SkaterTakedownRequest`, RegisterTazer plays the tazer burst
(`PedTazerEvent`). Logs `PED_TAZE`; mod event `ped_taze`.

**Engine choices.** The eye point, the line-of-sight ray (retail: physics rays) and the chasee radius (`G+1648`, 0.0)
are ours; the tazer is drawn when timer 37 ends (retail: the draw clip) and the `tazr` hand prop is not attached;
CanTazeUnreachableTargets and CannotReachChaseTarget stay Pending (their per-type byte and nav-result writer are
open), so the unreachable-taze branch does not run.

**Verification.** skate-core `entries_follow_the_retail_list_rules`, `vision_has_touch_near_and_a_cone_with_line_of_sight`,
`secondary_chasers_block_a_chasee_running_at_them`, `a_tazer_is_drawn_then_hits_its_target_once` (+ the earlier chase
tests). Not play-tested.

**Open.** Greets and conversations between peds (presence about other peds is retail, `sub_82E3CA20`, 35 m; waits for
the greet ops, research b22), the taunt after a takedown, hand props.

## Peds greet each other (2026-10-09)

**Retail [code] (`.local/research/peds/b21-ped-greets-conversations.md`, `b22-ped-greet-ops.md`; main checked the
radius constants and the greet speech).** Every 0.5 s a ped posts presence about the other live peds within 35 m (at
most 30, `sub_82E3CA20`) and then about players within 35 m. The stock greet results fire on presence (gated on the
other ped's type, probability 0.5, cooldown 20 s) and raise `greet`. ChannelGreetWantTarget (`8269FAC0` /
`8269FC30`): greet timer 3.5 s, posts `greeted` into the greeted ped's mood store (which raises `returngreet` there),
speech 56 (63 for the return greet), unsets the want when the timer runs out (`deactivateWant`; the graph's
`unsetWant` attribute is never read). ApproachWantTarget walks to the target at `speed` (2.0); AlertToWantTarget
pushes the target on the look-at stack. Correction to our warn op: ChannelWarnWantTarget unsets its want only with
`deactivateWant` (the unreachable chase keeps it).

**Change.** `mood::presence` takes the other peds (35 m, 30); brain ops ChannelGreetWantTarget, ApproachWantTarget,
AlertToWantTarget; `ChaseRequest::Greeted` queued into the greeted ped's store on its next think (log `PED_GREET`);
ChannelWarnWantTarget gains `deactivate`.

**Engine choices.** (The temporary hold-back of the `startconversation` results that this section first had was removed the same day, once the retail mood gate and the conversation plugin were ported: see "Ped conversations".) The world query order for the 30-ped cap is ours (id order); the
look-at stack has no head-tracking consumer yet; `greeted` reaches the target one think later.

**Verification.** skate-core `presence_posts_other_peds_then_players`,
`a_greet_posts_greeted_once_and_speaks_until_the_timer_runs_out`. Muted DownTown live run (25 s): 13 greets, return
greets answered, no ped held, 68 mood lines.

**Open.** Conversations (SpawnConversationArea, the conversation plugin and tables), the presence filter's exact
meaning (`ped+2020`), the `ped+2480` speech tag.

## Ped conversations: the plugin runner and the conversation object (2026-10-09)

**Retail [code] (`.local/research/peds/b23-ped-plugins-conversations.md`, `b24-conversation-object.md`; main checked
the spawn constants, the setup data chain, the conversation vtable and its completion state).** A conversation is a
waypoint plugin: SpawnConversationArea (`826A6CD8`) spawns one 3 m ahead of the starter unless another is within
50 m, and adds the starter and the greeted ped. In the main graph, HasPlugin moves a member into the Plugin state,
whose op runs the plugin's own graph (`plugin/conversation.stategraph`) on the same brain. The conversation object
(vtable `0x8232B868`): 3 member slots, 3 waypoints on a 1.5 m circle 120 deg apart; a member locks the nearest free
waypoint, walks onto it (`template/moveontowaypoint.xml`: 2.0 m/s, then 1.0 m/s, 0.1 m), turns to it, signals in
position; when all are in position it starts (fewer than 2 members ends it), picks a candidate row
(`livingworld_conversations`, via the entity's conversation group and weighted categories) and one value from the
row's list; 5 turns of 3.0 s round-robin (state 2 to 7), then complete; members unlock and exit the plugin.

**Change.** skate-core `living_world::peds::conversation` (`Conversation`, `ConversationRow`,
`ConversationParams`); brain ops Plugin, ExitPlugin, SpawnConversationArea, PedestrianInConversation /
PedestrianIsInConversation, LockClosestWaypoint, HasWaypointLocked, DistanceFromWaypointXZ, TargetWaypoint,
LockToCurrentPosition, TurnToFaceWaypointOrientation, IsFacingWaypointOrientation, UnlockWaypoint,
CreateSimpleMonitoredIntent, IncrementMonitoredPacketStage, HasMonitoredIntent (any intent), ConversationSignalInPosition,
ConversationThisParticipantIsSpeaker, ConversationSpeak, ConversationListenToSpeaker, ConversationIsComplete.
skate-game: the conversation plugin graph and tables load with the ped data, `PedConversations` (host-owned, seeded
RNG), the plugin graph runs on the brain while the main graph is in Plugin, requests applied in `think_peds` (log
`PED_CONVERSATION`).

**Mood gate (`b28`, `b29`, main checked `sub_82E3BF70` and the producer's `ori 32`).** The mood producer
(`sub_82E41B98`) does nothing for a busy ped (`sub_82E3BF70`: WaitingToReact `brain+3278` 0x20, IsReactingToMoodEvent
0x10, `brain+3277` 0x04 (written by PedestrianColliding), `*(ped+96)`); a pass sets WaitingToReact itself; the want
groups in `pedestrian_addresswants.xml` clear it when they begin (and set / unset IsReactingToMoodEvent). Check 8 of
a result (`D44E`) compares the target's busy state. A post whose reaction-set prerequisites fail is dropped
(`sub_82E41060`; presence: within 12 m in the stock sets). Ported: the four flag ops, `PedBrain::busy` (ours: the two
open terms are a plugin membership), the gate in `think_peds`, check 8 (`MoodContext::busy`), prerequisites at post
time for presence (`MoodTables::accepts`). By the code (`b29`) a ped whose conversation spawn is refused (another
within 50 m) raises the want again on its next pass, as in retail; we keep that (no heuristic stop).

**Engine choices.** Waypoint orientation = towards the centre; TargetWaypoint's slide moves the body straight onto
the point (no slide clip); LockToCurrentPosition pins the body to where it stood (an origin-locked trajectory: root
motion does not move it) and drops the route; ConversationListenToSpeaker only records the speaker to look at (no
body turn: the body keeps the waypoint orientation; no head tracking consumer yet); no conversation speech lines yet
(the line ids' meaning is open); the waypoints have no navmesh probe; players are never busy for check 8
(unverified).

**Live run (muted DownTown, 45 s).** Three conversations spawned; one ran its 5 turns with alternating speakers and
completed (logs `PED_CONVERSATION ... turn state=3..7`, `PED_PLUGIN`). Open from that run: two conversations whose
members never all arrived, a facing jitter between TurnToFaceWaypointOrientation and the converse states (the 0.3
rad check at the threshold), and about 30 mood lines a second from peds whose spawn was refused (retail-identical
loop by the code).

**Verification.** skate-core `a_conversation_gathers_starts_and_runs_five_turns`,
`a_lone_member_ends_the_conversation`; skate-game data-gated `living_world_ped_plugin_graphs_load` (conversation, sit,
lookat, spectate graphs load on the runtime).

**Open.** The slot fill from the chosen row, vf32 / vf52, conversation speech, the facing jitter and the members
that never arrive (above), prerequisite kinds 2 and 8 (OwnPluginObject / DisableCollisionsWithBehaviourSource flag,
`brain+540`), the other plugins (sit, ATM, vending, ...).
