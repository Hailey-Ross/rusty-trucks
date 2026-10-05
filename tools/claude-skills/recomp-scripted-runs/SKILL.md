---
name: recomp-scripted-runs
description: Run repeatable, unattended gameplay sessions in a local skate3recomp research build to collect data (audio trace, screenshots) - scripted pad input, a recorded teleport route, background mode so the machine stays usable. Use whenever retail behaviour must be measured again (sounds per action, timings), instead of performing and announcing actions by hand.
---

# Scripted recomp runs (unattended data collection)

Why: hand-marked sessions ("ollie now", "grinding now") are slow, tiring and imprecise (marks lagged actions by
about 4 s). A research build of the recomp can play a pad script by itself, write exact `MARK` lines into the trace,
screenshot its own window, and run in the background without touching the real controller, focus or speakers.

Rules from skill `recomp-audio-trace` apply: your own legal copy of the game, your own recomp build, no licence on
skate3recomp (local only, never copy its code), credit skate3recomp, rexglue and Xenia. The script player, background
mode and trace hooks are research additions to the recomp (the `research-hooks` branch linked from upstream PR #37's
`tools/recomp-code-search/README.md`, or your own equivalent); a stock recomp build does not have them.

## Pieces
| what | where |
|---|---|
| runner (boot -> script -> trace -> stop) | `tools/recomp_script_run.sh` here (Git Bash; needs `RECOMP_EXE`, `RECOMP_GAME_DIR`) |
| window screenshots, dev-build stop | `tools/screen_loop.ps1`, `tools/stop_dev_recomp.ps1` |
| Challenge Map teleport script generator | `tools/make_location_script.py` |
| build helpers | `recomp_{env,configure,build}.bat` (skill `recomp-audio-trace`) |
| script player + route recorder (C++) | recomp SDK `rex/input/input_script.h`, called from the XAM input code |
| trace hooks | recomp `src/research/hooks_*.cpp` + `trace_common.h` (skill `recomp-research`), `rex/audio/audio_trace.h` |
| your scripts / routes / sessions | a gitignored folder, for example `.local/recomp/{scripts,routes,sessions}/` |

## Run
```bash
export RECOMP_EXE=<recomp checkout>/out/build/clang-relwithdebinfo/skate3.exe RECOMP_GAME_DIR=<your game folder>
bash <skill>/tools/recomp_script_run.sh <ABSOLUTE script.txt> <ABSOLUTE out/trace.tsv> [max_seconds=120] [autostart_ms=4000] [extra recomp flags]
# env: BACKGROUND=1 (default) | MUTE=true (default) | SHOTS=1 (default; 0 = no screenshots) | SKATE3_TRACE=<categories>
```
- Refuses to start if any `skate3.exe` runs. Stops only the dev build (by exe folder) when `script end` appears or
  time runs out.
- Boots with `--skate3_demo_path --skate3_demo_path_signed_in` (straight to gameplay with your save).
- Outputs next to the trace: `<name>.out/.err` (game log), `<name>_shots/shot_<unix ms>.jpg` (every 2 s), the trace,
  and `<name>.f32` (the audio capture). Look at shots to see what really happened.
- Trace `MARK` lines: `script loaded`, `gameplay active; script autostart...`, `script start`, one per labelled step,
  `script end`. Analyse windows between MARKs (exact, unlike hand marks).
- Put hard time limits on every run and wait loop, and check for leftover processes afterwards.

## Background mode (default)
`SKATE3_BACKGROUND=1`: window shown without activation at the bottom of the z-order, real pad zeroed before the
demo path's synthetic presses and keystrokes dropped, `--audio_mute` (mutes only the SDL output; EA's mixer and the
hooks still run), 960x540 window, below-normal priority. Screenshots use `PrintWindow(PW_RENDERFULLCONTENT)` on the
recomp window, so they work while it is behind other windows (not when minimised). GPU/CPU load remains: keep runs
short and run one game instance at a time.

## Script format (`SKATE3_INPUT_SCRIPT`)
One step per line: `<ms> [tokens...] [# label]`; `#` lines and blanks ignored; a step without tokens is a neutral
pad. Tokens: `a b x y lb rb start back up down left right l3 r3 lt rt`, sticks `lx= ly= rx= ry=` (-1..1, up/right
positive), and `gameplay` = neutral until gameplay has run `<ms>` without a pause/loading break (put it after a
teleport). Durations can be given in polls (`Np`, about 60 polls/s) so hitches don't shift inputs; start scripts with
`600p gameplay` + `120p` settle and use autostart 8000 on the first load-in. Labels become `MARK` lines. Starts on
L3+R3 on the real pad, or `SKATE3_INPUT_SCRIPT_AUTOSTART_MS` after gameplay has been steady that long.

## Bails on demand
From an on-board spawn near a drop edge (for example the Mega-Park): `y` off the board, turn and walk a little,
place a **session marker** (LB opens the marker menu: Go To Marker = up, Place Marker = down, Object Dropper = B;
`300 lb`, `300 lb down`). Per attempt: go to the marker (`300 lb`, `3000 lb up`; a tap does not respawn), settle, run
toward the edge tapping `a` with `ly=1`, then `lt rt l3 r3` (both sticks + both triggers) forces the bail at the edge
at once. Wait about 6 s, `300 a` to get up, settle. The game does not respawn while the skater is down or in the
arms-out landing stance, so get up first and check a shot shows them standing. Verify bails from the trace (bail
state flags / impact lines), not from the script.

## Getting somewhere: record a route once
The route recorder writes a script from the first moment of gameplay while someone navigates by menu to a spot; paste
it at the top of a session script, shorten the first idle, add `6000 gameplay # at <spot>`. Launch the recorder from
Explorer, not a terminal (a terminal crash once took the game down). Arrival can be on foot carrying the board: press
`y` to get on before board actions, and check a shot, because arrival state varies.

## Teleport anywhere: Challenge Map > Locations
- Menu path from gameplay: `start`, `a` (Challenge Map), `rt`, `rt` (Locations tab), `down` x district, `a`, `down`
  x location, `a`, `a`, `a`, then `4000 gameplay # at <Name>`. Districts: 0 Downtown, 1 Industrial, 2 University,
  then skate.School, skate.Park, Maloof Money Cup, Black Box Park, and DLC areas.
- Generator: `py -3.13 tools/make_location_script.py <out> <district> <listen s> <i>:<Name>...`.
- Batches of 4-5 locations per recomp boot; chain districts as separate background jobs with hard time limits, never
  two recomps at once. A batch can occasionally die during boot (trace holds only "script loaded"): check each
  batch's MARK lines and re-run batches without `at` marks.

## Verify actions from the trace, not from the script
A scripted input is not proof the action happened. Check POSTs (audio objects) between MARKs; the first caller-chain
address falls inside the object's constructor (from upstream PR #4's notes): `824AF498` foot drag (obj 0),
`824AF678` wheel skid (1), `824AF8C8` grind, `824AFAD8` Class_Flips (4), `824AFDD0` seams (6), `824AFF48`
squeaks/powerslide, `824B0080` treatments (9), `824B0248` rolling rattle (10), `824B0388` speed wind (12),
`824B0520` speed rattle (13), `824B0670` board slide, `824B7070` body slide, `824B71C0` cloth_trick (22), `824B72D8`
cloth falls, `824B73E0` footstep. SPLC (Skate_Collisions / sk8_foley) plays show as `SPLC` lines (bank index, id,
callers). Plus the screenshots.

## Limits of scripting
- **Replaying a free-form recording does NOT work:** a few minutes of recorded skating, replayed, went off course
  within seconds (the emulated game is not deterministic enough for long open-loop replays). Use recordings only for
  menu routes. For actions: short scripted segments from a fixed spawn, each verified by POST objects + screenshots.
- **Open-loop wandering doesn't work:** scripts can't steer to a target. Rolling, grinds, NPC encounters and traffic
  need **passive sessions**: a person plays the instrumented build normally (sound on, their pad, normal window and
  priority, demo-path boot) while it writes `trace.tsv`, `pad.txt` (recorder), screenshots every 2 s
  (`screen_loop.ps1 -KeepPriority`) and the audio capture. The trace's first line `CLOCK 0.0 <unix ms>` aligns trace
  time with screenshot names. Useful presets: audio (`audio,dsp`), everything for NPC/traffic work
  (`audio,dsp,npc,world,traffic,aiskater`), NPC/world only (`npc,world`), high-volume audio detail
  (`audio,audiox,dsp`).
- While someone is playing on the machine, launch nothing that opens a window or steals focus.

## Real game sound capture
`SKATE3_AUDIO_CAPTURE=<file>` appends the final mix as stereo float32 LE 48 kHz, taken before `--audio_mute`;
`CAPTURE <frames>` trace lines every ~0.5 s align it to trace time. Captures lag their trace by roughly 110 ms: search
per-hit windows. Use it to cut out retail's real pops/landings and compare with the engine's render (the trace alone
lacks pitch/filters/mix). Never build listening/A-B pages from it; measure internally.

## Recomp artefacts to discount
- After a teleport the recomp ends the loading screen before the streams finish: for a few seconds the previous
  area's state lingers (for example the audio location set).
- On long teleport runs the character's hair and clothing get left lying on the ground.
- Audio-thread stalls of up to about 1 s happen; voices queued during one start together.
- A stall at the end of a session may not be in the trace (the batched writer and the capture stop first); if it
  recurs, rerun without the heaviest category (`dsp`).

## Closing the recomp and menus from a script
- Escape or F1 opens the recomp's own settings overlay, which has "Quit to the desktop" and is controller-navigable.
  A research build can add a held pad chord for it (for example LB + RB + Back for 1 s; Start would open Skate 3's
  own menu). Back alone opens Skate 3's Instant Replay.
- **Start** in free roam opens the Career > Main menu (Challenge Map, Edit Skaters, ...); `b` closes it. Game logic
  (frame update `82859E70`) stops while it is open.
- **Back** opens Instant Replay / Replay Editor (a loading screen first; the first use shows a help page). Back inside
  the editor asks "exit the Replay Editor?" with No selected: `down` then `a` to leave. Game logic stops there too.

## Trace line catalogue (field lists: each hook file's header and `recomp-research/tools/trace.py` FIELD_COUNTS)
- Audio (`audio`): `POST` (audio object posts), `SPLC` (Splice plays), `PLAY` / `GAIN` / `SEND` (voices; `PLAY`
  "level" is a counter and `SEND` values aren't gains), `CONTACT` (`sub_824B86E0`: impact speed, wheel classes,
  surface), `CSET` (contact set pick), `GREC` (rolling grain bed per frame, local player, `sub_824C6BD8`), `TREAT` /
  `SEAMPAT` / `SEAMHIT` (Class_Treatment / Class_Seams), world one-shots `WPPOS` / `WPSET` / `WPINT` / `WPFIRE`, zone
  ambience `AMBST` / `AMBBED` / `AMBXF`.
- DSP (`dsp`): `MOD <kind> <owner> <module> v1 v2 v3` per-voice PITCH / LPF / HPF / SHELF / PEAK.
- High-volume audio (`audiox`): `GRECX`, `FIRSTHIT`, `SKID`, `EMITSLOT`, bail impacts `BAILLOCAL` / `BAILSTEP` /
  `BAILREG`, `COLLPOST` (every collision-manager message with the local72 test), `PLAYERPOST` (posts per Player
  sound instance: local rider and nearby NPC skater), marker/teleport flow `TPMARK` / `TPDEC` / `TPSTREAM` / `TPFX` /
  `FEREQ` / `GSTATE` / `GEVENT` / `HUBMSG`.
- World (`world`): trigger volumes `TRIGLOAD` / `TRIGVOL` / `TRIGUNLOAD` / `TRIGMGR` / `TRIGADD` / `TRIGREM` /
  `TRIGENTER` / `TRIGEXIT` / `TRIGQRY` / `TRIGENT`, teleports `TELE*`, `WPKEY` (region layer key changes).
- NPC (`npc`): spawn/cull `NPCSPAWN` / `NPCCULL`, moods `NPCMOOD` / `MOODOUT` (mood event id and want enum), ped
  timers `PEDTIMER(S)`, positions `PEDXYZ`, perception `PEDSEE`, chase / takedown `SECCHASE` / `SECTAKE` / `SECTAZE`,
  ped audio `PEDAUD`.
- Traffic (`traffic`, `skitch`): `VEHSTATE`, `VEHAUD`, horns/skids/engine/lights `TRAF*`, junctions `VEHJUNC` /
  `VEHCONN`, skitching `SKITCH*`, `VEHBAIL`.
- NPC skater boards (`aiskater`): `SKATEB` per board (contact pattern: grind / slide / manual / air, contact point,
  deck velocity), `SKATER` state flags.
- Physics (`physics`): solver iterations `ITERSET` / `ITERTICK`, board/body lines.
- Per-player hooks must gate on the local-rider byte (`local72`) or they log nearby NPC skaters too; filter older
  traces accordingly.
- Trace size: pass only the categories a run needs.

## Gotchas
- Edit C++ with an editor tool: Python/shell edits turned `\t`/`\n` escapes into literal characters several times.
- Bash double quotes expand PowerShell's `$_`; keep PowerShell in `.ps1` files.
- Pass absolute paths everywhere (the runner changes directory).
