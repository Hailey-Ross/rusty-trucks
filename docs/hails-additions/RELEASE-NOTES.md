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
- **Hold RB** next to a bench, bin or other movable prop to grab it: the **left stick** pushes, pulls and side-steps it, the **right stick** turns you and the prop together. Let go of RB to drop it.

## Testing and reports

Every report with a log helps. Logs let us see exactly where and when something went wrong, so we don't have to guess.

**How to use logging**
- Start the game with `PLAY-WITH-LOG.bat`. It saves the whole session to `logs\game-<date>-<time>.stderr.log` next to the exe and shows the file name when you quit. One file per session.
- If the game closes on its own, it also writes a crash report to `%LOCALAPPDATA%\Skate3RustEngine\CrashReports` (paste that path into Explorer's address bar). The crash report is short; please attach the session log as well.
- For frame rate problems, turn on **GRAPHICS > Frame-time counter** before you play. It shows the 1 % low, the worst frame and hitches (a plain FPS average hides stutter); include a screenshot or video of it with your report.
- Try to keep one problem per session: start a session, reproduce it, quit. Note roughly when it happened (minutes in) and where (map and spot). A short video makes it much easier.
- Logs contain no personal files, but read them before sharing if you want to be sure.

**How to report:** open a report here: [**New test report**](https://github.com/Hailey-Ross/rusty-trucks/issues/new?title=Test%20report%3A%20&body=%2A%2ABuild%3A%2A%2A%20hails-additions%20rolling%20build%20%28number%20at%20the%20top%20of%20the%20release%29%0A%2A%2AMap%20and%20spot%3A%2A%2A%20%28for%20example%20DownTown%2C%20by%20the%20default%20spawn%2C%20near%20the%20road%20curb%29%0A%2A%2AWhat%20I%20did%3A%2A%2A%0A%2A%2AWhat%20happened%3A%2A%2A%0A%2A%2AWhat%20retail%20Skate%203%20does%20instead%3A%2A%2A%0A%2A%2AAbout%20when%20in%20the%20session%3A%2A%2A%20%28minutes%20in%2C%20or%20the%20time%20on%20your%20clock%29%0A%0AAttach%3A%0A-%20the%20session%20log%20logs%5Cgame-....stderr.log%20%28from%20PLAY-WITH-LOG.bat%29%0A-%20a%20crash%20report%20from%20%25LOCALAPPDATA%25%5CSkate3RustEngine%5CCrashReports%20if%20the%20game%20closed%0A-%20a%20short%20video%20if%20you%20have%20one%0A) (a form with the questions below is filled in for you). Attach the `.stderr.log`, the crash report if there is one, and a video if you have it. Say what retail Skate 3 does differently if you know.

**Please test and send logs for**
1. **Carrying props:** grab benches and bins (hold RB) and push, pull and side-step them in every direction. Does it go the way the stick points? Does the prop stay on the ground, especially near curbs and roads (DownTown default spawn)? Does letting go ever close the game? (One crash is known there; the new log line names the cause.)
2. **NPC skaters:** follow one for a while. Do they ever ride backwards (facing one way, moving the other)? Do limbs pop between animations? Do their tricks look right?
3. **Pedestrians:** anyone floating, stuck walking on the spot, walking through things, or looking wrong (body shape, clothing colours)?
4. **Cars:** cars driving through things, flying, or vanishing; how being hit by a car feels compared with retail.
5. **Grinding:** spots where you bail for no reason. The rails at the **PCU Library** spawn are known; tell us others.
6. **Frame rate:** any drops or stutter, with the Frame-time counter on (send a screenshot or video of it). Throwing the board far away used to drop the frame rate; does it still?
7. **Audio:** sounds that are missing, late, doubled, too loud or quiet compared with retail.
8. **Setup:** if the first conversion fails or stops, send the setup window's text and the folder the game is in (long paths and closing the console are handled now).

## Added since main

Each item has a document in `docs/hails-additions/` and, where open, an upstream PR.

**Gameplay and world**
- **Living world (early milestones, upstream draft #52):** ambient NPC skaters riding the retail recorded lines (they chain from line to line, blend between animations like the player, play their recorded tricks and leave beyond 120 m like retail), pedestrians with their retail models, tinted clothing, animation, fades and navigation on the retail navmesh (they walk round props), and traffic: road graph, traffic signals, the vehicle census and cars driving on screen. All from the disc data, seeded and multiplayer-ready, with a Lua mod API (`sdk.world.set_tuning`) and an NPC draw distance option.
- **Movable objects (#15 follow-up):** grab, carry, drag and drop props on foot (hold RB, left stick moves the object, right stick turns), placement mode with saved layouts, authored prop physics; NPC skaters push props too.
- **Water (#30):** retail water behaviour: shallow water is solid, deep water floats you and the board, bail in water, water camera and vignette, entry splash, animated water from the game.
- **Retail map spawns (#27):** every map starts at its retail-authored start point and heading.
- **Named trigger volumes (#50):** trigger volumes exported from the maps and tracked like retail, moddable.
- **Camera Angle setting (#47):** the retail Low / High camera option, moddable.
- **Board solver with retail's 50 constraint iterations (#35).**

**Audio (upstream #32, one PR)**
- A native port of Skate 3's retail audio engine: the AEMS patch programs, voice graph, MixMap mixer, buses, the granular rolling sound, board contacts, tricks, slides and every player sound component.
- Retail world audio: map ambience zones, emitters, location one-shots, traffic, ped and NPC skater sounds, the car alarm, session marker sounds and the teleport crackle, the announcer channel.
- Audio modding: an `audio.json` content overlay, custom map audio, mod voices, live tuning, mod emitters and sound rules, audio events for mods.

**Input**
- **SDL3 gamepad backend (#24):** wider controller support with exact XInput-shaped input and an XInput fallback (tested with an Xbox Elite Series 2).

**Setup and tools**
- **Windows long paths (#23):** setup enables long paths for the pro roster and relaunches when needed.
- **Setup keeps converting when the console window is closed (#42).**
- **Faster setup (#29, #51):** duplicate stream copies skipped, the character customiser runs in parallel, threaded map writing, below-normal priority and budget overrides.
- **One map validator pass (#28):** all map checks stream through one `--validate-maps` process with spawn and start-up warnings.
- **Published research and regression tools, and shareable development skills for working on the engine (#37).**
- **Fork releases:** the release workflow builds, publishes and updates from the repository it runs in.

**Fixes**
- **Frame drop with the board thrown away:** a hidden board no longer scans every collision triangle each tick.
- **Invisible walls (#25):** surfaceless zone and trigger boxes no longer act as collision.
- **Offboard jump runaway (#40):** fixes the "Nonfinite BipedAir launch packet" crash (#10).
- **Crash reports (#41):** keep the first panic, drop `<unknown>` frames, truncate long lines.
- **Lua empty tables (#45):** empty tables read as empty lists in mod command list fields.
- **Diagnostics (#51):** a frame-time counter and log, `sdk.snapshot.frame` for mods.

## In progress

Being worked on now, ported from the retail game code (the rolling `hails-additions` build gets them first):
- **NPC skaters riding backwards:** replacing our own "keep facing" rule with retail's line-chaining logic.
- **Carried props:** retail's own carry movement (free movement in every direction, stepping up curbs) and grab placement, props falling through the ground near curbs, and dropping the board when you grab a prop.
- **Crash when letting go of a carried prop:** the game now logs exactly which animation input broke; the fix follows from the next log.
- **Pedestrians hit by cars:** retail's reaction (knock-down and ragdoll, then fading out or getting up and fleeing), confirmed from the game code first.
- **Random grind bails** (also on upstream `main`), starting with the PCU Library rails.
- **Better session logs:** carried prop position and ground height, NPC skater facing versus travel direction, warnings when a prop ends up below the ground or a skater moves backwards, and car contacts.

## Known issues

From the latest play tests:
- **Game closes when letting go of a carried prop (seen once):** happened with the board thrown far away and the stick held back. Please send the log if it happens to you.
- **Carrying props:** while holding a prop you get pushed forwards or slightly to the side and cannot move fully freely (carry movement is not retail's yet, and stepping up curbs while dragging is lost). The grab lands close to the prop's edge but not exactly on it. A dragged bench can fall through the ground, especially near the curb by the DownTown default spawn. Your skateboard stays with you when you grab a prop (retail drops it on the floor).
- **NPC skaters:** after switching to another recorded line one can ride backwards (facing one way, moving the other), including through grinds. Some animations still look stiff, grabs show an ollie, and trick height, grind variety, landings, spins and fakie / goofy are not finished. They push props but do not steer round obstacles or bail on heavy props yet.
- **Pedestrians:** one can stand idle in the air above the wide DownTown stairs; one model walks stiffly and one has a flat head; peds still need a lighting pass; ped clothing colours follow the retail shader but are not fully checked against retail.
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

## Credits

Built on SK8-ENGINE/skate-3-rust-engine and its contributors. Research used the skate3recomp static recompilation (rexglue, Xenia) as reference only; no game code or data is included.
