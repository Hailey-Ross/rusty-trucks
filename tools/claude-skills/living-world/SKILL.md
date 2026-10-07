---
name: living-world
description: Build the living world (ambient NPC skaters, pedestrians, traffic, movable objects, zombie mode, standing pros/marquee actors) in the Rust engine, ported from the retail code with scripted tests and a mod API. Use when continuing NPC skater, ped or traffic work, answering a question about how retail populates the world, or planning its milestones.
---

# Living world: NPC skaters, pedestrians and traffic

Upstream PR #52 ("Living world: NPC skaters, pedestrians and traffic") is the home of this work; read its
description and docs first. Movable objects (DMOs) build on upstream PR #15 (dynamic props; credit its author).

## Source of truth
Retail code (recomp disassembly as reference, skill `recomp-research`) and stock data. Recomp traces (PEDCOLL, NPC*,
SKATEB, VEH*) only validate; the recomp does not reproduce peds and NPC skaters perfectly. Label every value
[code]/[data]/[trace] with its address. No game code or data in the repo; values come from each user's own assets at
setup time (data-driven defaults a mod can override).

Format references worth checking (credit them if used): community Skate 3 modding tool documentation for AIPATH
structs, NavPower navmesh constants and trigger types (including Crowd).

## Scope (as decided for the PR)
- **Free Play mode:** the offline pause-menu mode with Traffic, Pedestrians and "A.I. Skaters" On/Off
  (`freeskate_options`, mode 3, `0x830B7AE8`+332/+336/+340). Career free roam has no switch. No NPC skaters in parks.
- **Online games:** no ambient peds, traffic or NPC skaters spawn (census spawn pass and AI desired count gated by
  the online bytes `0x830B7C2B` / `0x83082929`, `sub_826B71F0`, `sub_8245BA28`); culling still runs.
- Time of day is fixed. Zombie mode (cheat "zombie", free skate only) is in scope. Pros/marquee actors standing around
  in free skate are in scope. Angry peds throw held items; peds also randomly throw held items into trash cans.
- **Vehicles (traffic)** and **movable objects** are part of the same work.
- **Multiplayer-ready, no networking yet:** decisions run in one place (a future host) and downstream systems
  consume serialisable spawn / despawn / event records; observers are a list (local player now, remote players
  later); stable `LivingWorldId`s; deterministic ticks and seeded per-entity RNGs; a `NetRole` (Standalone default,
  Host, Client stub). No transport or replication code until it is asked for.
- Confirm a mechanic exists in Skate 3 before researching it (security guards, for example, are Skate 2 leftovers
  and not part of the Skate 3 port).

## Shared design (do once)
One `livingworld` setup export group, one population core (budget, spawn ring, cull, slots shared with online
players), one CrowdCharacter renderer (reuses multiplayer skinned looks), kinematic collision proxies (like
`physics::network::Proxies`), one `sdk.living_world` mod API (spawn tables, models, behaviour overrides, density,
cleanup on disable), host-authoritative multiplayer.

## Tools
- `tools/render_glb.py` (next to this file): renders a converted `.glb` model to a PNG offscreen (numpy + Pillow),
  for checking ped / NPC models without launching the game.
- Recomp analysis: `recomp-research/tools/first_pass.py`, `trace.py`; ped chase timelines and vehicle summaries are
  written per topic from the NPC / traffic trace lines (skill `recomp-scripted-runs`, trace line catalogue).

## Testing
Scripted and headless wherever possible: data-gated `skate-data` tests on your own assets, deterministic population
tests with a seeded RNG, behaviour-graph unit tests, collision thresholds (knockdown > 3.0, 6.0 flagged), per-frame
cadence normalised to the 360's ~30 fps. Gameplay feel and NPC encounters come from passive sessions where a person
plays (scripts can't steer to NPCs). Run skill `regression-check` before calling anything done, and document each
change with its retail evidence and tests.
