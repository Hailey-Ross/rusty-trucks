> **EARLY ALPHA, rolling build.** The latest `hails-additions` branch of the Hailey-Ross fork, built on top of upstream `main` (SK8-ENGINE/skate-3-rust-engine). Expect bugs and please report them (see **Testing and reports**). You need your own Skate 3 Xbox 360 disc image; no game files are included.

## Setup

1. **You need** Windows 10 or 11 (64-bit), a Skate 3 Xbox 360 disc image (`.iso`) or an extracted game folder with `default.xex`, a controller, and about 10 GB free.
2. **Download** `skate3rust-windows-x64.zip` and `PLAY-WITH-LOG.bat` below. Extract the zip to a folder of its own (not inside Program Files) and put the .bat next to `skate3rust.exe`.
3. **Start** with `PLAY-WITH-LOG.bat`. On the first start, pick your ISO or `default.xex`; setup converts the game files into the `data` folder, then starts the game.
4. **Updates:** Esc > **EXTRAS** > **Updates**, Branch `hails-additions`.

**Other downloads:** `skate-launcher-windows.zip` is the Steam / couch launcher on its own, for testers who switch between versions (it builds each version on your PC; see "Launcher-only download" in `tools/steam-launcher/README.md`). Developers clone the repository.

**Refreshing game files.** When a build needs newer converted files, the Guide / F1 / Esc menu says **GAME FILES OUT OF DATE**: open **Extras > Refresh Game Files** and the game restarts into setup, converts only what changed and starts again.

**Early features** are off by default; switch them on in the Guide / F1 / Esc menu under **OPT-INS**: Zombie Mode, NPC Skater Physics, More Audible World, Visualize Grabs, Ped Speech (needs a refresh to decode the speech). Please say which were on when you report a problem.

## Controls (the new parts)

- **Start:** Skate 3's own pause menu (Game Settings, Trick Guide, Challenge Map, Extras).
- **Guide / F1 / Esc:** the PC menu (maps, graphics, mods, updates, opt-ins).
- **Back (View / Select, F6):** the instant replay. **A** opens the editor, **B** the Finished menu with Save to PC.
- **Y:** step off the board. **Hold RB** next to a bench or bin to grab and move it.
- **D-pad Up** at a teleporter: Skate 3's teleport question.

## Testing and reports

- Start with `PLAY-WITH-LOG.bat`: it saves the session to `logs\game-<date>-<time>.stderr.log` and names the file when you quit. A crash also writes a report to `%LOCALAPPDATA%\Skate3RustEngine\CrashReports`; please send both.
- For frame rate problems, turn on **GRAPHICS > Frame-time counter** and include a screenshot or video.
- One problem per session if you can, with roughly when (minutes in) and where (map and spot). A short video helps a lot.
- Report here: [**New test report**](https://github.com/Hailey-Ross/rusty-trucks/issues/new?title=Test%20report%3A%20&body=%2A%2ABuild%3A%2A%2A%20hails-additions%20rolling%20build%20%28number%20at%20the%20top%20of%20the%20release%29%0A%2A%2AMap%20and%20spot%3A%2A%2A%20%28for%20example%20DownTown%2C%20by%20the%20default%20spawn%2C%20near%20the%20road%20curb%29%0A%2A%2AWhat%20I%20did%3A%2A%2A%0A%2A%2AWhat%20happened%3A%2A%2A%0A%2A%2AWhat%20retail%20Skate%203%20does%20instead%3A%2A%2A%0A%2A%2AAbout%20when%20in%20the%20session%3A%2A%2A%20%28minutes%20in%2C%20or%20the%20time%20on%20your%20clock%29%0A%0AAttach%3A%0A-%20the%20session%20log%20logs%5Cgame-....stderr.log%20%28from%20PLAY-WITH-LOG.bat%29%0A-%20a%20crash%20report%20from%20%25LOCALAPPDATA%25%5CSkate3RustEngine%5CCrashReports%20if%20the%20game%20closed%0A-%20a%20short%20video%20if%20you%20have%20one%0A)

**Please test:** the instant replay and its editor, DLC maps and teleporters (if you own the packs), peds and NPC skaters (anything floating, stuck or riding backwards), carrying props, and the retail menus.

## What's new

Everything below is ported from Skate 3's own code. Items marked "not play-tested yet" in the linked docs may still have problems.

- **Living world** (upstream #52): NPC skaters on Skate 3's recorded lines, pedestrians with their behaviours (benches, vending machines, phone calls, warnings, chases, tazers), traffic you can skitch and grind, and props you can push, carry and grab. Zombie Mode and NPC Skater Physics are opt-ins.
- **Retail menus:** Skate 3's own pause menu, Game Settings (saved), Video Calibration, Trick Guide with its 3D demo, Extras with cheat codes, Skate Profile and the Challenge Map. `SKATE3_RETAIL_MENUS=0` turns them off.
- **Instant replay:** records the whole world for 90 seconds with sound, on Skate 3's own replay screen and controls; help, hide and photo; the editor with trim, markers, speed, shake and cameras; Save to PC renders an .mp4. Extended Replays (2 to 4 minutes, not retail) is in Esc > EXTRAS > Replay.
- **Your DLC:** put your packages in a `dlc` folder next to `skate3rust.exe` (the game then offers Refresh Game Files), or use the launcher's Tools > Import DLC, which converts them at once: the DLC maps with their skies and ambience, the Challenge Map's DLC folders, skate.Park layouts, teleporters, and the Create pack's replay editor.
- **World shading:** Skate 3's lightmaps and shader families for fences, signs, billboards, traffic lights and trees. Better Water (not retail) now covers fountains and the open sea.
- **Performance:** loading happens behind the loading screen (the first frame went from 1.7 s to 0.6 s), steady play is cheaper, and maps no longer double their memory while loading (upstream #66 by JohnUzoka).
- **Launcher and setup:** a proper launcher window, builds with progress, "play + trace all", a launcher-only download; setup installs its own Python packages.
- **Mods:** every new system has a Lua entry point (`sdk.world`, `sdk.menus`, `sdk.replay`, `sdk.triggers.teleporter` and more).
- **Changes for everyone:** the Skyline example mod starts switched off (turn it on in the mod menu).

Already in upstream `main` (from this fork): the retail audio engine and audio modding, water, retail map spawns, trigger volumes, camera angles, SDL3 gamepads, grind bails at rail bends, see-through fences, the retail menus foundation, setup and crash fixes. Each change has a document in `docs/hails-additions/`.

## Known issues

- **NPC skaters:** some spin round at a few line switches; grind variety, landings, spins and grabs are not finished.
- **Pedestrians:** they walk through a prop you are holding; one model walks stiffly and one has a flat head.
- **Cars** pop in and out instead of fading with distance.
- **Carrying props:** stepping up curbs while dragging is not Skate 3's yet.
- **Not ported yet:** the Challenge Map's challenge rows, the Free Play menu, the zombie cheat screen, the pros at their spots, the Object Dropper editor, peds watching you, multiplayer.

## Credits

Built on SK8-ENGINE/skate-3-rust-engine and its contributors. Research used the skate3recomp static recompilation (rexglue, Xenia) as reference only; no game code or data is included.
