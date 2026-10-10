> **EARLY ALPHA, rolling build.** This is the latest `hails-additions` branch of the Hailey-Ross fork: everything below is work in progress, built on top of upstream `main` (SK8-ENGINE/skate-3-rust-engine). Expect bugs, and please report them (see **Testing and reports**). You need your own Skate 3 Xbox 360 disc image; no game files are included.

## Setup

1. **What you need:** Windows 10 or 11 (64-bit), a Skate 3 Xbox 360 disc image (`.iso`) or an extracted game folder with `default.xex`, a controller (Xbox / XInput; other pads through SDL3), and an internet connection for the first setup. About 10 GB free for the converted game files.
2. **Download** `skate3rust-windows-x64.zip` below (or with the link above) and extract it to a folder of its own (not inside Program Files). Optional: check it against `skate3rust-windows-x64.zip.sha256`.
3. **Download `PLAY-WITH-LOG.bat`** below and put it in the same folder as `skate3rust.exe`.
4. **Start the game** by double-clicking `PLAY-WITH-LOG.bat` (or `skate3rust.exe` if you don't want a log). On the first start, choose your Skate 3 ISO, or `default.xex` in an extracted game folder. Setup converts the skater, animations, all maps, audio and the living world data into the `data` folder next to the exe, then starts the game. The first conversion takes a while; later starts are quick.
5. **Updates:** Esc > **EXTRAS** > **Updates**. This build looks for updates on this fork. Choose **Branch** and enter `hails-additions` to follow the rolling build of this branch; in-place updates only refresh what changed.

Each extracted copy runs its own setup. Keep the `data` folder next to the exe.

## Controls (the new parts)

- **Esc** (keyboard) opens the menu: **MAPS** (switch maps and drop-in spots without restarting), **SKATER** (Camera angle High / Low), **GRAPHICS** (display, sound, **Frame-time counter**, **NPC draw distance**), **EXTRAS** (mods, updates).
- **Y:** step off the board. **LT** (on foot): drop the board.
- **Hold RB** next to a bench, bin or other movable prop to grab it: the **left stick** pushes, pulls and side-steps it, the **right stick** turns you and the prop together. Your board drops when you grab. Let go of RB to drop the prop.

## Testing and reports

Every report with a log helps. Logs let us see exactly where and when something went wrong, so we don't have to guess.

**How to use logging**
- Start the game with `PLAY-WITH-LOG.bat`. It saves the whole session to `logs\game-<date>-<time>.stderr.log` next to the exe and shows the file name when you quit. One file per session.
- Peds are logged too: the log notes your position and the nearest ped every 2.5 s, and any ped drawn floating above the floor gets a `PED_FLOATING` line with its exact position, so a floating ped can be found from the log alone.
- If the game closes on its own, it also writes a crash report to `%LOCALAPPDATA%\Skate3RustEngine\CrashReports` (paste that path into Explorer's address bar). The crash report is short; please attach the session log as well.
- For frame rate problems, turn on **GRAPHICS > Frame-time counter** before you play. It shows the 1 % low, the worst frame and hitches (a plain FPS average hides stutter); include a screenshot or video of it with your report.
- Try to keep one problem per session: start a session, reproduce it, quit. Note roughly when it happened (minutes in) and where (map and spot). A short video makes it much easier.
- Logs contain no personal files, but read them before sharing if you want to be sure.

**How to report:** open a report here: [**New test report**](https://github.com/Hailey-Ross/rusty-trucks/issues/new?title=Test%20report%3A%20&body=%2A%2ABuild%3A%2A%2A%20hails-additions%20rolling%20build%20%28number%20at%20the%20top%20of%20the%20release%29%0A%2A%2AMap%20and%20spot%3A%2A%2A%20%28for%20example%20DownTown%2C%20by%20the%20default%20spawn%2C%20near%20the%20road%20curb%29%0A%2A%2AWhat%20I%20did%3A%2A%2A%0A%2A%2AWhat%20happened%3A%2A%2A%0A%2A%2AWhat%20retail%20Skate%203%20does%20instead%3A%2A%2A%0A%2A%2AAbout%20when%20in%20the%20session%3A%2A%2A%20%28minutes%20in%2C%20or%20the%20time%20on%20your%20clock%29%0A%0AAttach%3A%0A-%20the%20session%20log%20logs%5Cgame-....stderr.log%20%28from%20PLAY-WITH-LOG.bat%29%0A-%20a%20crash%20report%20from%20%25LOCALAPPDATA%25%5CSkate3RustEngine%5CCrashReports%20if%20the%20game%20closed%0A-%20a%20short%20video%20if%20you%20have%20one%0A) (a form with the questions below is filled in for you). Attach the `.stderr.log`, the crash report if there is one, and a video if you have it. Say what retail Skate 3 does differently if you know.

**Please test and send logs for**
1. **Carrying props:** grab benches, bins and vending machines (hold RB) and push, pull, side-step and turn them. Your board should now drop when you grab a prop. Do props stay on the ground and settle when you let go (they used to sink, rock forever or fall through the floor)? Does the grab ever let go on its own?
2. **NPC skaters:** some now ride fakie on purpose, like the recorded lines (with the fakie upper-body animation). Tell us if one looks like it rides backwards by mistake, or spins round when it switches to another line.
3. **Pedestrians:** anyone floating, stuck walking on the spot, walking through things, or looking wrong (body shape, clothing colours)? New: peds use vending machines, sit on benches, take newspapers and throw empty cans into bins; many carry something (bags, coffee, a skateboard, a map). Annoy one that holds a can: it may throw it at you (a can pushes you, a light one should not knock you down). Tell us if a held item floats away from the hand or a ped gets stuck at a bench or machine.
4. **Cars:** a car now pushes a pedestrian out of its way (that is what retail does, no knock-down). Cars driving through things, flying or vanishing.
5. **Grinding:** the stair handrail at the **PCU Library** spawn should no longer throw you off at its bottom bend. The two corners of the flat rail there still bail. Tell us any other spot where you bail for no reason.
6. **Frame rate:** big flips on the Spillway jumps used to hitch; the trick text no longer rebuilds itself every frame. Play with the Frame-time counter on and send a screenshot or video.
7. **Audio:** sounds that are missing, late, doubled, too loud or quiet compared with retail.
8. **Living world:** cars honking, braking, pulling over and parking; run into a parked car (its alarm should sound for about 8 s and the car should not drive off meanwhile); bump the same pedestrian two or three times (warning, then a chase). With the opt-in switches: skitching (`SKATE_SKITCH=1`) and grabbing props (`SKATE_PROP_GRAB=1`, watch the hands).
9. **Setup:** if the first conversion fails or stops, send the setup window's text and the folder the game is in (long paths and closing the console are handled now).

## Fixed in this build

Not play-tested yet; tell us if any of these still happen.
- **Props sinking into the ground and never settling:** props now use retail's contact solver (25 iterations) and its sleep rule, so dragged props stay on the street and placed props come to rest.
- **Board stays with you when grabbing a prop:** grabbing now drops a carried board where you stand, like retail.
- **Carry movement:** retail's hold rule (you keep hold while your hands stay at the prop's edge) and retail's way of moving you along with the prop replace our own.
- **NPC skaters riding backwards:** retail's facing rule, with the fakie upper-body animation for skaters that ride fakie on purpose.
- **Pedestrians and cars:** a car pushes a ped out of its way, as in retail; a `VEHICLE_CONTACT` line goes to the session log.
- **Grind bails at the PCU Library handrail:** the board no longer catches a post under the bottom bend (retail's tighter collision query box).
- **Car shadows under bridges:** traffic on a bridge no longer casts a shadow onto the street below (retail's world shadow floor).
- **Props look like retail:** benches, bins and other props use retail's prop shader (dents, grime, rust) after a setup refresh.
- **Rolling on grass and dirt:** uses retail's sound routing for those surfaces instead of a stand-in grain sound.
- **Frame hitches from the trick text:** the trick and score text no longer rebuilds its graphics every frame.
- **Better logs:** a body hit while skating is heard again (and logged), every landing logs its sound, frame hitches are logged with their cause, and a trace-all mode turns on every log at once for test sessions.

**Fixed in build 9**
- **Peds floating in the air all over the map:** peds were stood on the first solid surface within 1.6 m above them (awnings, ledges, signs, invisible collision). They are now drawn on the floor under them: the search only reaches the navmesh step height (0.2 m) upward. On DownTown's navmesh this lifted peds at 195 spots before and none after.
- **Peds spawning in the air or under the ground:** a ped could spawn at your height where the floor was far above or below you (on ledges, ramps and roofs) and stay there. Those spawns are now skipped and the ped spawns somewhere else; a session after the fix logged no floating peds.
- **Game closed when letting go of a carried prop:** while carrying, the walk cycle's timing was asked to reach a phase in zero seconds, which turned it into an invalid number; on letting go, the stand-up animation could not pick a clip and the game stopped. Carrying now holds the walk cycle still. Found from a tester log thanks to the new error line, which now names the broken animation input and the clips it was choosing between.
- **Landings going silent the longer you played:** NPC skaters that left kept their sounds held, which slowly filled the sound mixer until new sounds (your landings among them) were refused. NPC skaters now release their sounds when they go, and a full mixer clears finished sounds first.
- **Frame-time counter modes:** Esc > GRAPHICS > Frame-time counter now cycles Off / Simple / Verbose. Simple is a small readout (fps, frame time, 1% low, worst) with a tiny spike graph; Verbose is the full one.
- **No-NPC option:** NPC draw distance (Esc > GRAPHICS) has a **None** step that turns off all NPC skaters, peds and cars.
- **Manual landing log:** every landing writes a `MANUAL_LANDING` line to the session log (stick position, whether a manual was asked for and whether it was granted, or why not), to check landings that should have gone into a manual.
- **Better crash logs:** any animation choice that gets an invalid input now names that input and the candidate clips in the session log.

## Added since main

Upstream `main` (SK8-ENGINE/skate-3-rust-engine) has merged almost all of this fork's work (latest 2026-10-09, upstream `2e16697`). This build includes all of it. What this build adds on top of it:

**Living world (upstream draft #52, still open)**
- **Ambient NPC skaters** riding the retail recorded lines: they chain from line to line, blend between animations like the player, play their recorded tricks on body and board and leave beyond 120 m like retail; they push props too.
- **Pedestrians** with their retail models, tinted clothing, animation, fades and navigation on the retail navmesh (they walk round props); drawn on the floor under them and never spawned in the air.
- **Traffic:** road graph, traffic signals, the vehicle census and cars driving on screen. New: cars brake and honk for skaters, peds and props in their way (peds run off the road from a honk), knock you down when they hit you, change lanes, pull over, park and pull out again, and a parked car's alarm goes off when you run into it (it stays parked while the alarm sounds).
- **Pedestrians react to you:** hit the same ped twice and they stop and warn you, three times and they chase you (with friends joining in and security tazers); a caught skater gets taken down and taunted. Peds greet each other, gather and hold conversations.
- **NPC skaters** steer, slow down or stop for peds, cars, props and you.
- **Opt-in, still being tested** (off by default; set the variable before starting, for example `set SKATE_SKITCH=1` in a command window, then run `PLAY-WITH-LOG.bat` from it): **skitching** on traffic cars (`SKATE_SKITCH=1`: ride behind a car and hold grab), **carrying props by their real handles** with the hands placed on the edge (`SKATE_PROP_GRAB=1`), **NPC skaters with full physics** near you (`SKATE_NPC_SIM=1`).
- **Movable objects (#15 follow-up):** grab, carry, drag and drop props on foot (hold RB, left stick moves the object, right stick turns), placement mode with saved layouts, authored prop physics.
- All from the disc data, seeded and multiplayer-ready, with a Lua mod API (`sdk.world.set_tuning`), an NPC draw distance option, and always-on logging of ped positions and floating peds.
- **Frame drop with the board thrown away fixed:** a hidden board no longer scans every collision triangle each tick.
- **New in this build:** peds use vending machines, bench seats, bins and newspaper boxes placed from the disc's prop data, with the retail clips; they hold the can or newspaper in the hand, throw empty cans into bins or drop things, and many start out carrying something (by each ped type's retail odds). An angry ped holding a can throws it at you, aimed the way retail aims it, and a thrown can hits you as an ordinary prop (your body now feels thrown props). NPC skaters that get stuck step off, walk back to their line and ride on (retail mode 7). Mods can spawn and remove props while the game runs (`sdk.world.spawn_prop` / `remove_prop`). The benches, bins and hand item models need the setup's map and living world data from this build (a fresh setup or a setup refresh).

**This fork's releases**
- The release workflow builds, publishes and updates from this repository (the in-game updater follows `Hailey-Ross/rusty-trucks`), a rolling `hails-additions` build on every push, and `PLAY-WITH-LOG.bat` for session logs.

**Now in upstream main** (shipped there, listed so nothing is lost; each has a document in `docs/hails-additions/`)
- Retail audio engine port, world audio and audio modding (#32), water (#30), retail map spawns (#27), named trigger volumes (#50), Camera Angle Low / High (#47), retail's 50 solver iterations (#35), SDL3 gamepads (#24, #53).
- Setup: Windows long paths (#23), keeps converting when the console closes (#42), faster setup (#29, #51), one map validator pass (#28).
- Merged 2026-10-09: grind bails at rail bends (retail per-volume query box, #57), see-through fences and grates (#58), the retail menus foundation (menu data, hooks, mod API, settings values and menu movies, #59), the Frame-time counter's Simple / Verbose modes (#60), and upstream's APT example fixes.
- Fixes and tools: invisible walls (#25), offboard jump crash (#40, issue #10), crash reports (#41), Lua empty tables (#45), frame-time diagnostics (#51), research and regression tools and shareable skills (#37).

## In progress

Being worked on now, ported from the retail game code (the rolling `hails-additions` build gets them first):
- **NPC skaters:** each pro's own stance (goofy or regular) from the game data; turning speed at line switches needs the full skater physics.
- **Grinding:** the two flat-rail corners at the PCU Library spawn.
- **Object Dropper:** the LB phone menu's Object Dropper is a full editor in retail (catalogue, free camera, snapping); research has started. Resetting and uprighting a single object already works for mods.
- **Peds and moved props:** peds should walk round props after you move them.

## Known issues

From the latest play tests and the overnight work (not play-tested yet):
- **Carrying props:** the grab point is a straight edge on top of the prop, not retail's authored grab shapes, and stepping up curbs while dragging is not retail's yet. The prop fixes above come from headless tests only.
- **NPC skaters:** they can spin round when switching to another line (retail's turning comes from full skater physics, not ported yet). Every NPC uses the same stance until each pro's stance is read from the data. Some animations still look stiff, grabs show an ollie, and trick height, grind variety, landings and spins are not finished. They push props but do not steer round obstacles or bail on heavy props yet.
- **Pedestrians:** they walk through props you have moved; one model walks stiffly and one has a flat head; peds still need a lighting pass; ped clothing colours follow the retail shader but are not fully checked against retail. Held items: the carrying arm keeps the walk pose (retail's carry poses are not played yet). Placed props (benches, bins) still pass through your body; only thrown items hit you.
- **Cars:** paint colours and glass are estimates (the vehicle shader is not decoded yet).
- **NPC draw distance** (Esc > GRAPHICS) is a quality-of-life option, not retail; higher settings cost frame time. None turns all NPCs off.
- **Grinding:** the two corners of the flat rail at the PCU Library spawn still make you bail. Some drops off stair edges now land a little faster than before; tell us if landings feel wrong.
- Known upstream test failures (also on `main`): `pipelines_accept_valid_group_outputs_when_fingerprint_changes` (skate-game) and `a_moving_group_8_body_...` (skate-core).

## Known missing features

- **NPC skaters:** full skater physics and bails on by default (opt-in for now), grabs, grind variety, landings, spins.
- **Pedestrians:** spectating and looking at you, carry poses for held items, ATMs and water fountains (placement not found yet).
- **Traffic:** roof landings and following a lead car the retail way; skitching is opt-in while it is tested.
- **Movable objects:** retail object streaming by distance, per-type physics values from the game data, the Object Dropper editor, the phone rows for Reset and Upright, grindable props.
- **Modes:** Free Play options (Traffic, Pedestrians, A.I. Skaters), zombie mode, the standing pros.
- **Multiplayer for the living world:** built ready for it, no networking yet.
- **Teammate recruit menu:** teammate looks come from `settings/living_world_teammates.json` until the menu system exists.

## Credits

Built on SK8-ENGINE/skate-3-rust-engine and its contributors. Research used the skate3recomp static recompilation (rexglue, Xenia) as reference only; no game code or data is included.
