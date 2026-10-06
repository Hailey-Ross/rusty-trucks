# Known issues (hails-additions)

The current known issues of the `hails-additions` branch, kept in step with the latest fork release
([early-alpha-1](https://github.com/Hailey-Ross/rusty-trucks/releases/tag/early-alpha-1)). Fixed items move
out of this list into the change's own document.

## In progress

Being worked on now, ported from the retail game code (the rolling `hails-additions` build gets them first):
- **NPC skaters riding backwards:** replacing our own "keep facing" rule with retail's line-chaining logic.
- **Carried props:** retail's own carry movement (free movement in every direction, stepping up curbs) and grab placement, dragged props getting pushed through the floor, and dropping the board when you grab a prop.
- **Pedestrians hit by cars:** retail's reaction (knock-down and ragdoll, then fading out or getting up and fleeing), confirmed from the game code first.
- **Random grind bails** (also on upstream `main`), starting with the PCU Library rails.
- **Peds and moved props:** peds should walk round props after you move them.
- **Better session logs:** carried prop position and ground height, NPC skater facing versus travel direction, warnings when a prop ends up below the ground or a skater moves backwards, and car contacts.

## Known issues

From the latest play tests:
- **Carrying props:** while holding a prop you get pushed forwards or slightly to the side and cannot move fully freely (carry movement is not retail's yet, and stepping up curbs while dragging is lost). The grab lands close to the prop's edge but not exactly on it. Dragged props (benches, trash cans, vending machines) can get pushed through the floor and drop into the void, and a dragged prop can pull back toward the spot it started at, dragging you with it. Props still fall through the map very easily. Your skateboard stays with you when you grab a prop (retail drops it on the floor).
- **NPC skaters:** after switching to another recorded line one can ride backwards (facing one way, moving the other), including through grinds. Some animations still look stiff, grabs show an ollie, and trick height, grind variety, landings, spins and fakie / goofy are not finished. They push props but do not steer round obstacles or bail on heavy props yet.
- **Pedestrians:** they walk through props you have moved; one model walks stiffly and one has a flat head; peds still need a lighting pass; ped clothing colours follow the retail shader but are not fully checked against retail.
- **Cars:** pedestrians pass straight through cars (retail cars knock them down). A car can knock you over, but through general physics, not retail's own car-hit rules. Paint colours and glass are estimates (the vehicle shader is not decoded yet).
- **NPC draw distance** (Esc > GRAPHICS) is a quality-of-life option, not retail; higher settings cost frame time.
- **Grinding:** some grind locations make you bail at random, for example the rails at the PCU Library spawn. This also happens on upstream `main`, so it is not caused by this fork's changes.
- Known upstream test failures (also on `main`): `pipelines_accept_valid_group_outputs_when_fingerprint_changes` (skate-game), `a_moving_group_8_body_...` and `predictive_contacts_and_retention_...` (skate-core).

## Known missing features

- **NPC skaters:** full skater physics and bails, trick choice from the retail profiles, grabs, grind variety, landings, spins, fakie and goofy, obstacle avoidance.
- **Pedestrians:** behaviour (perception, moods, reactions), stumbling and knock-downs, warnings, chases and takedowns, speech and conversations, hand props, benches, phones and vending machines.
- **Traffic:** cars do not stop for you, honk, change lanes or park yet; retail's car-hit bails and roof landings are missing; no skitching.
- **Movable objects:** retail object streaming by distance, per-type physics values from the game data, the safety layer and reset rule, grindable props.
- **Modes:** Free Play options (Traffic, Pedestrians, A.I. Skaters), zombie mode, the standing pros.
- **Multiplayer for the living world:** built ready for it, no networking yet.
- **Teammate recruit menu:** teammate looks come from `settings/living_world_teammates.json` until the menu system exists.
