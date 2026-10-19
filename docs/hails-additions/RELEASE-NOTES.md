> **EARLY ALPHA, rolling build.** This is the latest `hails-additions` branch of the Hailey-Ross fork: everything below is work in progress, built on top of upstream `main` (SK8-ENGINE/skate-3-rust-engine). Expect bugs, and please report them (see **Testing and reports**). You need your own Skate 3 Xbox 360 disc image; no game files are included.

## Setup

1. **What you need:** Windows 10 or 11 (64-bit), a Skate 3 Xbox 360 disc image (`.iso`) or an extracted game folder with `default.xex`, a controller (Xbox / XInput; other pads through SDL3), and an internet connection for the first setup. About 10 GB free for the converted game files.
2. **Download** `skate3rust-windows-x64.zip` below (or with the link above) and extract it to a folder of its own (not inside Program Files). Optional: check it against `skate3rust-windows-x64.zip.sha256`.
3. **Download `PLAY-WITH-LOG.bat`** below and put it in the same folder as `skate3rust.exe`.
4. **Start the game** by double-clicking `PLAY-WITH-LOG.bat` (or `skate3rust.exe` if you don't want a log). On the first start, choose your Skate 3 ISO, or `default.xex` in an extracted game folder. Setup converts the skater, animations, all maps, audio and the living world data into the `data` folder next to the exe, then starts the game. The first conversion takes a while; later starts are quick.
5. **Updates:** Esc > **EXTRAS** > **Updates**. This build looks for updates on this fork. Choose **Branch** and enter `hails-additions` to follow the rolling build of this branch; in-place updates only refresh what changed.

Each extracted copy runs its own setup. Keep the `data` folder next to the exe.

Every start checks your converted game files (a fraction of a second, in the background; the log says `ASSET_CHECK`). If an update needs newer ones, the Guide / F1 / Esc menu says **GAME FILES OUT OF DATE**: open **Extras > Refresh Game Files** and the game restarts into setup, converts only what changed and starts again. This also catches copies an earlier build updated without converting the new menu files (there, Start opened the old menu instead of Skate 3's pause menu).

## Refreshing game files and trying early features

**Refreshing your game files.** New builds sometimes need game files converted from your disc that an older setup did not make (new menus, props, living world data). You don't need to set up again from scratch:
1. Update the game (Esc > **EXTRAS** > **Updates**, Branch `hails-additions`), or extract the new zip over your old folder, keeping its `data` folder.
2. Start the game. If your converted files are older than the build needs, the Guide / F1 / Esc menu shows **GAME FILES OUT OF DATE**.
3. Open **Extras > Refresh Game Files** (the row only appears while files are out of date). The game restarts into setup, converts only what changed and starts again. It reuses your disc image if it is still where setup found it, otherwise it asks for it again.
4. Still seeing old menus or missing props? Send the session log from `PLAY-WITH-LOG.bat`: the `ASSET_CHECK` lines name the files that differ.

**Opting in to early features.** Features that are still being tested are off by default. Switch them on in the Guide / F1 / Esc menu in the **OPT-INS** section. Changes are saved, and most take effect at once:
- **Zombie Mode:** the retail zombie cheat until the cheat screen exists: yellow screen, no traffic, every ped comes after you.
- **NPC Skater Physics:** NPC skaters near you run full skater physics and can bail.
- **More Audible World** (not retail, from the next start): more cars, peds and NPC skaters heard at once.
- **Visualize Grabs (new):** shows the props and car bumpers you can grab from where you stand (amber = in range, green with a ring and an arrow = what a grab takes now) and logs `GRAB_VIEW` lines, so a grab that does not start can be checked from the log.
- **Ped Speech:** peds, pros and the announcer talk. Their speech has to be decoded from your disc first (about 3.4 GB and a few minutes), so switching it on offers **Extras > Refresh Game Files**.

For a single run you can still use the old variables instead (`set SKATE_ZOMBIE=1`, `SKATE_NPC_SIM=1` before starting). Please mention which opt-ins were on when you report a problem.

## Controls (the new parts)

- **Start** opens Skate 3's own pause menu (its real menu files from your disc): **A** selects, **B** goes back, the d-pad / arrows move. Options > **Game Settings** is the retail settings screen; Learn > **Trick Guide** is Skate 3's own trick guide.
- **Guide** (the Xbox / PS / Home button), **F1** or **Esc** (keyboard) opens the menu: **MAPS** (switch maps and drop-in spots without restarting), **SKATER** (Camera angle High / Low), **GRAPHICS** (display, sound, **Frame-time counter**, **NPC draw distance**), **EXTRAS** (mods, updates), **OPT-INS** (features still being tested). The menu now looks and works like the recomp's settings screen: **B** / **Esc** goes back one level, **LB** / **RB** (or **Q** / **E**) switch category, **Y** (or **R**) resets a row, the mouse works too.
- **Y:** step off the board. **LT** (on foot): drop the board.
- **Hold RB** next to a bench, bin or other movable prop to grab it: the **left stick** pushes, pulls and side-steps it, the **right stick** turns you and the prop together. Your board drops when you grab. Let go of RB to drop the prop.

## Testing and reports

Every report with a log helps. Logs let us see exactly where and when something went wrong, so we don't have to guess.

**How to use logging**
- Start the game with `PLAY-WITH-LOG.bat`. It saves the whole session to `logs\game-<date>-<time>.stderr.log` next to the exe and shows the file name when you quit. One file per session.
- Peds are logged too: the log notes your position and the nearest ped every 2.5 s, and any ped drawn floating above the floor gets a `PED_FLOATING` line with its exact position, so a floating ped can be found from the log alone.
- The menus and rumble are logged as well: every menu key you press and which screen took it, what opened and closed, every setting you changed (old and new value) and every rumble with its pad. Zombie mode logs when it is on and how many peds are following you.
- If the game closes on its own, it also writes a crash report to `%LOCALAPPDATA%\Skate3RustEngine\CrashReports` (paste that path into Explorer's address bar). The crash report is short; please attach the session log as well.
- For frame rate problems, turn on **GRAPHICS > Frame-time counter** before you play. It shows the 1 % low, the worst frame and hitches (a plain FPS average hides stutter); include a screenshot or video of it with your report.
- Try to keep one problem per session: start a session, reproduce it, quit. Note roughly when it happened (minutes in) and where (map and spot). A short video makes it much easier.
- Logs contain no personal files, but read them before sharing if you want to be sure.

**How to report:** open a report here: [**New test report**](https://github.com/Hailey-Ross/rusty-trucks/issues/new?title=Test%20report%3A%20&body=%2A%2ABuild%3A%2A%2A%20hails-additions%20rolling%20build%20%28number%20at%20the%20top%20of%20the%20release%29%0A%2A%2AMap%20and%20spot%3A%2A%2A%20%28for%20example%20DownTown%2C%20by%20the%20default%20spawn%2C%20near%20the%20road%20curb%29%0A%2A%2AWhat%20I%20did%3A%2A%2A%0A%2A%2AWhat%20happened%3A%2A%2A%0A%2A%2AWhat%20retail%20Skate%203%20does%20instead%3A%2A%2A%0A%2A%2AAbout%20when%20in%20the%20session%3A%2A%2A%20%28minutes%20in%2C%20or%20the%20time%20on%20your%20clock%29%0A%0AAttach%3A%0A-%20the%20session%20log%20logs%5Cgame-....stderr.log%20%28from%20PLAY-WITH-LOG.bat%29%0A-%20a%20crash%20report%20from%20%25LOCALAPPDATA%25%5CSkate3RustEngine%5CCrashReports%20if%20the%20game%20closed%0A-%20a%20short%20video%20if%20you%20have%20one%0A) (a form with the questions below is filled in for you). Attach the `.stderr.log`, the crash report if there is one, and a video if you have it. Say what retail Skate 3 does differently if you know.

**Please test and send logs for**
1. **Carrying props:** grab benches, bins and vending machines (hold RB) and push, pull, side-step and turn them. Your board should now drop when you grab a prop. Do props stay on the ground and settle when you let go (they used to sink, rock forever or fall through the floor)? Does the grab ever let go on its own?
2. **NPC skaters:** some now ride fakie on purpose, like the recorded lines (with the fakie upper-body animation). Tell us if one looks like it rides backwards by mistake, or spins round when it switches to another line.
3. **Pedestrians:** anyone floating, stuck walking on the spot, walking through things, or looking wrong (body shape, clothing colours)? New: peds use vending machines, sit on benches, take newspapers and throw empty cans into bins; many carry something (bags, coffee, a skateboard, a map). Annoy one that holds a can: it may throw it at you (a can pushes you, a light one should not knock you down). Tell us if a held item floats away from the hand or a ped gets stuck at a bench or machine. Peds now hold their item with the carrying arm while they walk, and a throw moves only the upper body.
10. **Props appearing and disappearing:** like retail, placed props (benches, bins, cones and so on) now exist only near you: they appear within about 90 m and go beyond 100 m, and a knocked-over prop is back in its place when you return. Tell us if a prop pops in right in front of you, vanishes while you look at it, or a ped sits on an invisible bench. To compare, start with `set SKATE_DMO_STREAM=0` (props always there, as before).
11. **Zombie mode (opt-in until the cheat screen exists):** Guide / F1 / Esc menu > Opt-Ins > **Zombie Mode**: the screen turns yellow, the streets empty of traffic and every ped comes after you. Tell us how it plays.
4. **Cars:** a car now pushes a pedestrian out of its way (that is what retail does, no knock-down). Cars driving through things, flying or vanishing.
5. **Grinding:** the stair handrail at the **PCU Library** spawn should no longer throw you off at its bottom bend. Tell us any other spot where you bail for no reason.
6. **Frame rate:** big flips on the Spillway jumps used to hitch; the trick text no longer rebuilds itself every frame. Play with the Frame-time counter on and send a screenshot or video.
7. **Audio:** sounds that are missing, late, doubled, too loud or quiet compared with retail.
8. **Living world:** cars honking, braking, pulling over and parking; run into a parked car (its alarm should sound for about 8 s and the car should not drive off meanwhile); bump the same pedestrian two or three times (warning, then a chase). **Skitching** and **Prop Grab** are always on (watch the hands).
9. **Setup:** if the first conversion fails or stops, send the setup window's text and the folder the game is in (long paths and closing the console are handled now).
12. **Retail menus (now the default):** **Start** opens Skate 3's own pause menu: the world blurs, gameplay pauses, the arrows / d-pad move, **A** selects, **B** goes back. Options > **Game Settings**: every setting now steps with Left / Right and is saved (`settings\game-settings.json`). Working: Play Mode, the three volumes, Camera Angle, **HUD** (Off hides the trick HUD), **Control > Offboard Y axis** (inverts the right stick when you are off the board) and **Vibration** (switching it On gives a short test rumble; rumble during gameplay is not in yet). Subtitles, Minimap, Cameraman Mode, HOM mode and the skate.Feed toggles are greyed out until their features exist. Video > **Video Calibration** opens Skate 3's TV setup guide (**A** next page, **B** back). Tell us anything that looks or behaves differently from retail, and whether the test rumble works on your pad. If Start still opens the old menu, setup has not converted the new menu tables yet: the log says `run setup`. `set SKATE3_RETAIL_MENUS=0` turns the retail menus off.
13. **Guide button:** press your pad's Guide / PS / Home button in game; the menu should open and close. The log gets a `PC overlay toggled by the Guide button` line each time; if nothing happens, send the log (Windows Game Bar may take the Xbox button first: turn off "Open Game Bar using this button on a controller" in Windows settings to test).
14. **HUD colours:** coloured and tinted parts of the trick HUD now show the colours the console shows (darker than before where they were washed out). Tell us if any HUD part now looks wrong.
15. **Challenge Map (new):** pause menu > **Challenge Map** now opens Skate 3's own map screen. The list shows the districts; **A** opens one and **A** on a location teleports you there (on foot), **B** goes back to the row you came from, the triggers / d-pad left and right switch tabs (each tab keeps its row). Rows that do nothing yet (challenges, skate parks, the other tabs) are greyed out. The map picture changes with the highlighted location where Skate 3 has its own picture (for example Skate School); the city districts use the overview map. Each location shows its marker on the map and the cursor sits on the highlighted one. This build converts the map pictures from your disc on its first start (**Refresh Game Files** runs once by itself).
16. **Better Water (new, for your eyes):** Guide / F1 / Esc menu > **EXTRAS** > **Better Water** (not retail): a cleaner, more realistic water look that costs almost nothing. Tell us how it looks next to Skate 3's own water and where it looks out of place; we will tune it from your notes.
17. **Button prompts (new):** the Guide / F1 / Esc menu footer, the replay panel and the mod menu now show your pad's own buttons (Cross / Circle on PlayStation, B / A on Nintendo, LB / L1 / L and so on), the same glyphs as Skate 3's pause menu, instead of Xbox button names in text. Tell us if a glyph shows the wrong button for your pad.

## Fixed in this build

Not play-tested yet; tell us if any of these still happen.
- **Switching maps (new):** NPC skaters with physics now ride the new map's rails and settings after a map change (they kept the old map's), and peds no longer carry conversations or chases over from the old map.
- **Bumped NPC skaters (new):** an NPC skater bumped by another skater no longer also says its crash line for the next 3 s, and one you bump says its "team" line, both as in Skate 3 (with **Ped Speech** on).
- **Game files refresh on their own when the menus need it (new):** the first start of this build restarts into **Refresh Game Files** once if your menu files are older than the build reads (it adds the Challenge Map's menu files). Let it finish; the game starts again by itself.
- **Grabbing props works again (new):** in the previous build no prop could be grabbed (only 2 of a map's props were known to the grab check). Benches, bins, newspaper boxes and dumpsters grab again, and a grab takes the handle Skate 3 picks instead of a bottom edge or corner post. A grab press that starts nothing logs `PROP_GRAB_MISS` with the reason, and **Visualize Grabs** shows what is in reach.
- **Turning while carrying (new):** when you grab a prop you turn to face it over about half a second, like Skate 3, instead of snapping round at once.
- **Launcher (new):** A / Enter and the mouse work on the main page, and the rolling board no longer goes behind the Play card.
- **Refresh Game Files from the game (new):** it no longer ends with "exit code 1" after setup finished.
- **Peds you ride into (new):** a ped you hit on the board now gets the contact (before, no ped ever did, on foot or on board); the log says when a contact got no reaction and why.
- **No freeze when play starts (new):** the first moment of play used to freeze for about 1 to 2 seconds while the world's collision and the living world loaded. That now happens while the loading screen is still up (it stays up a little longer), and after a map switch play starts without the hitch.
- **People and cars ready at the start (new):** the people, cars, NPC skaters and the things peds carry now load while the loading screen is up, so they no longer pop in a few frames late or stutter the first seconds of play. The loading screen stays up a moment longer.
- **NPC Skater Physics opt-in (new):** switching it on no longer freezes the first moment of play for half a second while its skaters load; they load in the background.
- **Smoother frames (new):** cars no longer re-send their materials to the graphics card every frame, which lowers the average frame time (on the main test PC from 3.5 to 2.7 ms in DownTown).
- **No freeze a few seconds into the first start after a reboot (new):** the world's traffic and people sounds now load while the loading screen is up; reading them from a cold disk froze the game for about 4 seconds once play had started.
- **Shorter hitch when play starts, steadier frames without mods (new):** the first moment of play is about 60 ms shorter, and without mods the game no longer prepares mod data every frame that nothing reads (worst frames down about 15 % in DownTown on the main test PC).
- **Walking into peds (new):** on foot you no longer stop dead at a ped as if it were a wall. Like Skate 3, you carry on through the contact, and the ped stumbles or is knocked down depending on how hard it was hit.
- **Walking over benches (new):** on foot you step onto and over heavier props like benches, as in Skate 3, instead of shoving them along. Light things (cones, boxes, bin lids, rubbish bags) and the prop you are holding are not something you step onto, also like Skate 3.
- **Pushing an occupied bench (new):** a ped sitting on a bench you push now jumps off it, like Skate 3, instead of riding along or standing up normally.
- **Conversations (new):** a passing ped can now join two who are talking, as in Skate 3.
- **Refresh offered for the hand prop grips (new):** copies set up before the grip points now show **GAME FILES OUT OF DATE**, so **Refresh Game Files** appears.
- **Solid peds (new):** you can no longer roll through a ped that is already stumbling or lying on the ground; its body stays solid the whole time, like Skate 3.
- **Ped hand props (new):** peds hold bags, phones, coffee and the rest by the grip (a bag by its handle) at Skate 3's angle, not by the model's centre. Run **Refresh Game Files** once to get the grip points.
- **Bailing into props (new):** bailing into a bin or bench no longer throws it; like Skate 3, light props only get nudged and heavy ones (200 kg and up) stay put.
- **NPC skaters unsticking (new):** a stuck NPC skater that jumps back onto its line starts it fresh instead of carrying over air or trick state.
- **Opt-Ins (new):** the early features moved out of Graphics into their own **Opt-Ins** section of the Guide / F1 / Esc menu.
- **Rumble (new):** the controller no longer buzzes from the moment the game starts, and it stops when the game closes, crashes or is killed. Wired and wireless pads now get Skate 3's own motor strengths for each. **Vibration** is Off by default for now: switch it on in Start > Options > Game Settings to try it, and tell us how it feels (USB or wireless).
- **World shading (new):** fences, billboards, lit signs, traffic lights (Skate 3's 17 second cycle), trees and the memorial flag use Skate 3's own shading. Run **Refresh Game Files** once to get the new map data.
- **Rusty Trucks (new):** this fork's build now calls itself Rusty Trucks (window title, splash, setup and crash windows). Your files, settings and updates stay as they were.
- **Ped lighting:** peds no longer have rough, blocky dark patches on their shadow side. Their normal maps were converted in the wrong layout. Run **Refresh Game Files** once to get the fixed ped models.
- **Ped turns:** peds turn with Skate 3's turning animations (45, 90, 135 and 180 degrees) while standing and while walking, instead of stopping to pivot, and they follow their path at Skate 3's steering speed (measured in the recompiled game).
- **Peds no longer get stuck:** a ped pinned against a corner it cannot walk round moves on, or turns away and picks a new path.
- **NPC skater reactions:** NPC skaters near a wipeout (yours or another skater's) say Skate 3's slam lines, and their own wipeouts their crash lines.
- **Cars stopping:** cars stop at lights and behind other cars at Skate 3's distances, braking later and harder, and no longer run amber lights.
- **Zombie mode:** zombies pick their next spot round you a little earlier, like Skate 3.
- **Peds hold a grudge:** a ped who warned you now chases you if you bump them again, like Skate 3. Before, a ped whose warning had ended could never get angry enough to chase. Security guards also react when they see someone else get hit.
- **Who knocks peds over:** NPC skaters now knock peds over like you do, and a ped who walks into another ped makes both stumble.
- **Chases:** a full chase group stops taking new chasers, the lead chaser changes round the group, a taze ends the chase, and a chase ends cleanly.
- **Peds use their props:** peds now eat, drink, read, drop litter and take phone calls (you hear the ring, the call and the hang-up), with Skate 3's own timings for each ped type.
- **NPC skater sounds:** NPC skaters running full physics now make their bail, push, pop and impact sounds.
- **NPC skater tricks:** NPC skaters whose profile has no grabs no longer do grabs, fingerflips or bonelesses.
- **Cars:** each car pulls away with its own acceleration and some drive a bit slower or faster than the limit, like Skate 3; a car you run into from the front brakes to a stop properly instead of easing off.
- **City sounds:** distant one-shots (sirens, dogs, bangs) keep playing when you move into another area instead of being cut off, and their volumes follow Skate 3's steps. How often they play was checked against Skate 3 and already matched.
- **Loading NPC skaters:** every NPC skater with full physics after the first loads in well under half the time, so the pause when they first appear is shorter.
- **Stutters while skitching (play-tested):** being towed fast through the city no longer freezes the game for up to a second when a ped speaks or a distant city sound (siren, truck, horn) plays for the first time; those sounds now load in the background.
- **Skitching holds:** holding on, your hands, letting go at the edge of the bumper and the grab sound work like Skate 3; cars collide with their real body shape, so your arms no longer bump into an invisible box; with both bumpers held, the one you pressed first wins; your grip follows the bumper's curve.
- **Peds are solid:** you no longer pass through peds. They block you like Skate 3's peds do, and running into one fast or mid-trick makes you bail.
- **Ped speech timing:** peds, pros and the announcer start their lines with Skate 3's own short delays and leave Skate 3's gap between lines.
- **Startup screen:** the skateboard rolling on the loading screen is drawn smoother.
- **Angry peds attacking over and over:** after a ped knocks you down it now ends the chase like Skate 3 does: it forgets what made it angry and ignores your bumps for that moment, instead of chasing again the second its taunt ends.
- **Peds ignoring you while walking to a bench:** a ped you knock over now reacts and can get angry, even on its way to a bench, a vending machine or a bin.
- **Seated peds:** you can knock a seated ped off a bench with a proper hit (gentle bumps are ignored, like Skate 3).
- **Moved benches:** a bench you move takes its seats along, so peds sit on it, not in the air where it was. Moving a bench under a seated ped makes them jump off and sit back down at the new spot. A ped still in the middle of sitting down when you take the bench now finishes sitting, stands up and follows it, instead of staying seated in mid-air.
- **Camera in water:** falling into deep water no longer leaves the camera under the water, stuck close to you: the water shot (camera high above the water, dark edges) is back.
- **Zombie takedowns:** zombies no longer knock you down again while you are down or right after you respawn; once you are down, the chase ends.
- **Lag spikes with NPC Skater Physics:** fixed the 1-second hitches when an NPC skater was blocked by a prop far away.
- **Floating peds:** peds on some DownTown pavements were drawn up to 2 m above the ground; they now stand on it.
- **Zombie mode:** zombies now come after you like Skate 3: they follow you, close in, and attack once they are within a few metres; get away and they fall back to following. Up to 25 can chase you at once (it was 5, and most gave up before they started).
- **Peds react only to what they can see:** greetings, chats and reactions to collisions now need the ped to actually see you or the event, as in Skate 3, instead of any distance.
- **Peds and seats:** a ped takes a bench, vending machine or ATM only when it is in front of it (a 30 degree cone, like Skate 3), and does not sit right back down on the seat it just left.
- **Crash near benches:** fixed a crash when a ped using a bench or other prop lost it as props streamed out around you.
- **Settings screens:** the selected value ("< Normal >", "100%") is readable again and only the selected row is bright; the selection glow is softer (the menu and HUD layer now blends like the console).
- **Game files check:** when your converted game files do not match what setup recorded, the log now names the files (`ASSET_CHECK <group>: ... (count: first files)`), so a report shows what changed. Refresh Game Files also works in copies started from a source checkout (`--assets`), not only the release package.
- **Pause menu froze the game:** opening Skate 3's pause menu a few times (for example Challenge Map, back, and Start again) ran the menu system out of room; it stopped with the game still paused, Start no longer worked and the game had to be closed. Closed menus are now freed, and if the menu system ever stops, the game unpauses and Start opens the Guide / F1 / Esc menu instead.
- **Props sinking into the ground and never settling:** props now use retail's contact solver (25 iterations) and its sleep rule, so dragged props stay on the street and placed props come to rest.
- **Board stays with you when grabbing a prop:** grabbing now drops a carried board where you stand, like retail.
- **Carry movement:** retail's hold rule (you keep hold while your hands stay at the prop's edge) and retail's way of moving you along with the prop replace our own.
- **NPC skaters riding backwards:** retail's facing rule, with the fakie upper-body animation for skaters that ride fakie on purpose.
- **Pedestrians and cars:** a car pushes a ped out of its way, as in retail; a `VEHICLE_CONTACT` line goes to the session log.
- **Grind bails at the PCU Library handrail:** the board no longer catches a post under the bottom bend (retail's tighter collision query box).
- **Car paint and glass:** cars use Skate 3's own car shaders now: paint colours, lighting, reflections and the tinted glass follow retail instead of estimates (re-run setup so the car data is rebuilt).
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

**Living world (upstream draft #52: feature complete, being play-tested before review)**
- **Ambient NPC skaters** riding the retail recorded lines: they chain from line to line, blend between animations like the player, play their recorded tricks on body and board and leave beyond 120 m like retail; they push props too.
- **Pedestrians** with their retail models, tinted clothing, animation, fades and navigation on the retail navmesh (they walk round props); drawn on the floor under them and never spawned in the air.
- **Traffic:** road graph, traffic signals, the vehicle census and cars driving on screen. New: cars brake and honk for skaters, peds and props in their way (peds run off the road from a honk), knock you down when they hit you, change lanes, pull over, park and pull out again, and a parked car's alarm goes off when you run into it (it stays parked while the alarm sounds).
- **Pedestrians react to you:** hit the same ped twice and they stop and warn you, three times and they chase you (with friends joining in and security tazers); a caught skater gets taken down and taunted. Peds greet each other, gather and hold conversations.
- **NPC skaters** steer, slow down or stop for peds, cars, props and you.
- **Opt-in, still being tested** (off by default; switch them in the Guide / F1 / Esc menu under **Opt-Ins**, in effect at once and saved; the old `set SKATE_NPC_SIM=1` style variables still turn one on for a run): **NPC skaters with full physics** near you (**NPC Skater Physics**), **More Audible World** (not retail: more cars, peds and NPC skaters heard at once, from the next start) and **Ped Speech** (peds, pros and the announcer talk; switching it on offers Extras > **Refresh Game Files**, which decodes about 3.4 GB of speech from your disc).
- **Movable objects (#15 follow-up):** grab, carry, drag and drop props on foot (hold RB, left stick moves the object, right stick turns), placement mode with saved layouts, authored prop physics.
- All from the disc data, seeded and multiplayer-ready, with a Lua mod API (`sdk.world.set_tuning`), an NPC draw distance option, and always-on logging of ped positions and floating peds.
- **Frame drop with the board thrown away fixed:** a hidden board no longer scans every collision triangle each tick.
- **Newest:** props stream like retail (they appear near you and come back in place), peds hold items with a carrying arm and throw with the upper body, and zombie mode works (Opt-Ins > Zombie Mode: yellow screen, no traffic, every ped follows you; the cheat screen comes with the menu work).
- **New in this build:** peds use vending machines, bench seats, bins and newspaper boxes placed from the disc's prop data, with the retail clips; they hold the can or newspaper in the hand, throw empty cans into bins or drop things, and many start out carrying something (by each ped type's retail odds). An angry ped holding a can throws it at you, aimed the way retail aims it, and a thrown can hits you as an ordinary prop (your body now feels thrown props). NPC skaters that get stuck step off, walk back to their line and ride on (retail mode 7). Mods can spawn and remove props while the game runs (`sdk.world.spawn_prop` / `remove_prop`). The benches, bins and hand item models need the setup's map and living world data from this build (a fresh setup or a setup refresh).

**Retail menus (upstream #62, open)**
- Skate 3's own pause menu and Game Settings screen run on the engine from your disc's menu files, with retail's screen stack, blur, pad input and button prompts for your pad. Now the default (Start); `SKATE3_RETAIL_MENUS=0` turns them off.
- Game Settings rows that work are real settings (saved, mod-overridable): Play Mode, the volumes, Camera Angle, HUD, offboard axis invert and Vibration. Rows whose feature is not in yet (Subtitles, Minimap, Cameraman Mode, HOM mode, the skate.Feed toggles) are greyed out.
- **Video Calibration** (Game Settings > Video): Skate 3's TV setup guide (settings, contrast and brightness pages); **A** next page, **B** back. Like Skate 3 it changes nothing in the game. At the end Skate 3's own popup asks whether you are happy with it (Yes closes, No starts over). Mods can raise these popups too (`sdk.menus.popup`). Needs Refresh Game Files on an existing install (new menu files).
- **Trick Guide** (Learn > Trick Guide): Skate 3's own trick guide with every category, subcategory and trick, the stick gestures and buttons for each, and the description; Manuals opens its tricks straight away, like Skate 3. Mods can add or change entries (`sdk.menus.trick_guide`). Needs Refresh Game Files on an existing install (new menu files). It fades to black and back when it opens and closes, like Skate 3. Behind it, Skate 3's 3D demo: your own skater (custom models too) performs the highlighted trick in the Trick Guide set, under its own sky. Every listed trick has its demo clip. The camera plays each trick's own Skate 3 camera path: it moves with the skater, or stays still and turns to watch them where Skate 3 does. The set has its own fog and sun; its shadow settings are not applied yet. Needs Refresh Game Files on an existing install (the demo clips and the set). Not play-tested yet.
- Rumble: a port of retail's rumble controller for SDL and XInput pads, with a mod API (`sdk.input.rumble`). Gameplay rumble events are still to come.
- The Guide / F1 / Esc menu now looks and moves like the recomp's settings screen (blurred world, sliding highlight, category rail, button legend).

**This fork's releases**
- The release workflow builds, publishes and updates from this repository (the in-game updater follows `Hailey-Ross/rusty-trucks`), a rolling `hails-additions` build on every push, and `PLAY-WITH-LOG.bat` for session logs.

**Now in upstream main** (shipped there, listed so nothing is lost; each has a document in `docs/hails-additions/`)
- Retail audio engine port, world audio and audio modding (#32), water (#30), retail map spawns (#27), named trigger volumes (#50), Camera Angle Low / High (#47), retail's 50 solver iterations (#35), SDL3 gamepads (#24, #53).
- Setup: Windows long paths (#23), keeps converting when the console closes (#42), faster setup (#29, #51), one map validator pass (#28).
- Merged 2026-10-09: grind bails at rail bends (retail per-volume query box, #57), see-through fences and grates (#58), the retail menus foundation (menu data, hooks, mod API, settings values and menu movies, #59), the Frame-time counter's Simple / Verbose modes (#60), and upstream's APT example fixes.
- Fixes and tools: invisible walls (#25), offboard jump crash (#40, issue #10), crash reports (#41), Lua empty tables (#45), frame-time diagnostics (#51), research and regression tools and shareable skills (#37).

## In progress

Being worked on now, ported from the retail game code (the rolling `hails-additions` build gets them first):
- **Living world:** feature complete for its pull request (#52) and now being play-tested. Anything that looks or plays wrong with NPC skaters, peds, traffic, props or zombie mode: please send a log (see **Testing and reports**). Later additions (pros at their challenge spots, the Free Play menu and the zombie cheat screen with the menu work, more mod hooks) come in separate updates.
- **Object Dropper:** the LB phone menu's Object Dropper is a full editor in retail (catalogue, free camera, snapping); research has started. Resetting and uprighting a single object already works for mods.
- **Peds and moved props:** peds should walk round props after you move them.

## Known issues

Still open after the play tests up to 2026-10-12 (the fixes from the 2026-10-12 round are not play-tested yet):
- **Carrying props:** stepping up curbs while dragging is not retail's yet, and the stick controls are still being checked against Skate 3.
- **NPC skaters:** some still spin round at a few line switches. Some animations still look stiff, grind and slide spans all show one 50-50 clip, and trick height, grind variety, landings and spins are not finished.
- **Pedestrians:** they walk through a prop while you hold it; one model walks stiffly with its arms out and one has a flat head; peds still need a lighting pass; ped clothing colours follow the retail shader but are not fully checked against retail. Placed props (benches, bins) still pass through your body; only thrown items hit you.
- **Cars:** they appear and vanish at full opacity; retail fades them in and out with distance.
- **NPC draw distance** (Esc > GRAPHICS) is a quality-of-life option, not retail; higher settings cost frame time. None turns all NPCs off.
- Known upstream test failures (also on `main`): `pipelines_accept_valid_group_outputs_when_fingerprint_changes` (skate-game) and `a_moving_group_8_body_...` (skate-core).

## Known missing features

- **NPC skaters:** full skater physics and bails on by default (opt-in for now), grabs, grind variety, landings, spins.
- **Pedestrians:** spectating and looking at you, ATMs and water fountains (placement not found yet).
- **Traffic:** roof landings and following a lead car the retail way; skitching is opt-in while it is tested.
- **Movable objects:** the Object Dropper editor, the phone rows for Reset and Upright, grindable props.
- **Challenge Map:** the challenges and skate park rows (greyed) and the park screenshot are not ported yet. Older converted game files without the Challenge Map keep the **MAPS** section of the Guide / F1 / Esc menu in its place until **Refresh Game Files** runs.
- **Modes:** the Free Play menu (Traffic, Pedestrians, A.I. Skaters), the zombie cheat entry (zombie mode itself works from Opt-Ins), the pros at their challenge spots.
- **Multiplayer for the living world:** built ready for it, no networking yet.
- **Teammate recruit menu:** teammate looks come from `settings/living_world_teammates.json` until the menu system exists.

## Credits

Built on SK8-ENGINE/skate-3-rust-engine and its contributors. Research used the skate3recomp static recompilation (rexglue, Xenia) as reference only; no game code or data is included.
