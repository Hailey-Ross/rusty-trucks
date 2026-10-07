# 29. RPCS3 as a timing reference

Branch `rpcs3-reference-support` (from `main` b3c9679). Two parts:

1. **Launcher:** `tools/steam-launcher` can start the PS3 version of Skate 3 in RPCS3 (entries `rpcs3-play`,
   fullscreen, and `rpcs3-windowed`, for recording), and only lists the recomp and RPCS3 modes that are set up.
2. **Measurement tools:** `tools/rpcs3-reference/` measures frame cadence and event timings from a recording, so
   retail timing can be compared with ours.

Goal: use RPCS3 for its closer emulation of the retail console game, and measure timing, cadence and other
factors the recomp might reproduce improperly or not at all.

---

## Problem

Our retail reference so far is the Xbox 360 recompilation. It is the right source for the game's **logic** (the
code is the retail code), but it is not the console:
- It runs natively on a PC: its frame rate, frame pacing, thread scheduling and timers are the PC's, not the
  360's. Per-frame retail processes are tuned for the console's ~30 fps; on the recomp they can run at a
  different rate, and anything timed in frames runs at a different speed.
- So durations measured in the recomp (how long an animation, a fade or a respawn takes; how often a per-frame
  process ticks) can differ from what a player saw on the console.

## Why RPCS3

- RPCS3 emulates the PS3 hardware, including its video timing (vblank), so the game runs at its console cadence
  (about 30 fps for Skate 3) and its frame-based logic runs at console speed.
- The PS3 build is a separate compile of the same game. Its **code addresses do not carry over** from the 360
  build, so research hooks and addresses from the recomp do not apply to it. The **logic stays from the 360
  code**; RPCS3 is used only to measure what the player sees and hears: timing, cadence, durations.

## Setup

You need RPCS3 (rpcs3.net), your own PS3 copy of Skate 3 (dumped from your own disc or console) and the PS3
firmware installed in RPCS3. Nothing from the game is in this repository.

RPCS3 settings that matter for measurement (per-game config, RPCS3 window: right-click the game, **Configure**):
- **Frame limit: Auto** (follows the game's own vblank pacing). Not Off (the game would run faster than the
  console) and not a fixed value that differs from the game's rate.
- **VSync:** on or off makes no difference to the game's own cadence with the frame limit on Auto; keep it the
  same across recordings you compare.
- **No speed hacks:** clocks scale 100 %, vblank frequency 60 Hz (the default), no "skip frames" or
  frame-skipping options, no patches that change frame rate (60 fps patches change exactly what is measured).
- Keep the host fast enough that the game never drops below its target: RPCS3's performance overlay (in the
  **Emulator** settings) shows the frame time graph; a recording where the emulator itself stutters measures the
  host, not the game.

Recording (OBS or any recorder):
- **60 fps, constant frame rate** (OBS: Settings, Video, Common FPS Values 60; the default encoder output is
  constant frame rate). At 60 fps every 30 fps game frame appears twice, so missing or tripled frames are visible.
- Record the RPCS3 game window (window or game capture), at its native size if possible.
- Use the launcher entry `rpcs3-windowed` when recording beside other windows, `rpcs3-play` for fullscreen.

## Method

1. Record the action in RPCS3 (a few seconds is enough; one action per clip is easiest to read).
2. Record the same action in our engine with the same recorder settings.
3. Run the tools on both recordings (Python 3, ffmpeg 5.1 or newer on `PATH`, or pass `--ffmpeg` / set `FFMPEG`):
   - `python tools/rpcs3-reference/frame_times.py CLIP.mp4 [--roi x,y,w,h] [--csv frames.csv]`
     effective game fps (unique frames per second), the spacing of unique frames in recorded frames (a steady
     30 fps game recorded at 60 fps shows spacing 2 only), the interval median / p5 / p95 / max and hitches.
     A still picture reads as duplicates, so measure while something moves, or restrict `--roi` to a moving area.
   - `python tools/rpcs3-reference/event_timer.py CLIP.mp4 --roi x,y,w,h [--csv events.csv]`
     onset and end frame, times and duration of each burst of change in a screen region: how long a fade, an
     animation, a prop movement or a menu transition takes.
   - `python tools/rpcs3-reference/selftest.py` checks both tools on synthetic ffmpeg clips (expects 30 game fps
     at spacing 2, and one event from 1.0 s lasting 1.0 s).
4. Compare the two sets of numbers, and where available the engine's own logs (game log, frame-time log) for the
   same action. A difference in a duration points at a per-frame process that is not normalised to the console's
   cadence; a difference in the logic itself is checked against the retail code, not tuned to the recording.

## Limits

- **Emulation accuracy:** RPCS3 is very close but not the console; a game-specific emulation bug could change
  timing. A finding that matters is checked against the retail code before it changes the engine.
- **PS3 versus 360:** the PS3 build can differ from the 360 build in rendering, loading and small tuning values.
  Timings that should be platform-independent (game logic at the game's frame rate) are the useful ones.
- **Recording jitter:** a recorder can drop or repeat a frame under load, which reads as a hitch. Repeat the
  clip; a real game hitch repeats, a recorder hitch does not. Resolution is one recorded frame (16.7 ms at 60 fps).
- **Change detection:** encoder noise, HUD counters or camera shake in the region can start or extend an event.
  Pick the region tightly and check the per-frame CSV when a number looks off.

## Files

- `tools/steam-launcher/`: `launcher.ps1`, `SkateLauncher.cs`, `config.example.json`, `README.md` (RPCS3 config
  block and entries, `rpcs3.exe` in the one-game-at-a-time check, modes hidden when their exe or game is missing,
  `hidden <id>` command for the reason).
- `tools/rpcs3-reference/`: `videoframes.py` (shared decoder), `frame_times.py`, `event_timer.py`, `selftest.py`.

## Verification

- `SkateLauncher.cs` compiles with the Windows C# compiler (as `build.bat` does); `launcher.ps1` parses with no
  errors. `launcher.ps1 entries` with the example config (paths that do not exist) and with no config lists no
  recomp or RPCS3 modes; with existing `rpcs3.exe` / `rpcs3.game` paths it lists `rpcs3-play` and
  `rpcs3-windowed`. A direct start of a hidden entry writes one "Not started: ... is not available" line.
- `selftest.py` passes: 60 fps clip of a 30 fps source reads 30.00 game fps, spacing 2 for all 88 intervals; the
  event clip reads onset 1.033 s (the first changed frame), duration 1.000 s.

## Credit

- [RPCS3](https://rpcs3.net), the PS3 emulator (GPLv2), by the RPCS3 team. Only used as an external program.
- [FFmpeg](https://ffmpeg.org) (LGPL / GPL), used as an external program for decoding.

## Open questions

- Which RPCS3 settings affect Skate 3's frame pacing in practice (for example the RSX and SPU accuracy
  options) is not measured yet; record the settings used with every clip.
- Whether PS3 and 360 retail timings match for the actions we compare is unknown until both are measured once.
