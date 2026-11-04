# 41. Launcher: camcorder viewfinder look

Status: first step on `hails-additions` (launcher only, `tools/steam-launcher/SkateLauncher.cs`). Not for upstream.

## Problem

The couch launcher looked like a generic dark console menu. The goal (USER-ASKS 484, 485) is a launcher that feels like
skate culture, starting with the view through a late-90s / 2000s MiniDV skate-video camcorder (VX1000 era): a
viewfinder overlay on top of the existing screens, not a camera metaphor in the navigation.

## Change

- **Viewfinder overlay** (`Viewfinder`), drawn after every screen (window and `--snapshot`):
  - corner framing marks and a small centre cross;
  - top right, left of the version chip: `STBY` in menus, `REC` with a blinking red dot while a job (fetch, build)
    runs, `SP`, the tape counter `h:mm:ss:ff`, a four-segment battery with `60min`;
  - footer, left of the button prompts: the date and time stamp, stacked (`NOV. 4 2026` / `1:41:00 AM`);
  - a radial vignette and faint scanlines for the lens and screen feel.
- **2000s OSD look:** the OSD is drawn at a third of 1080p (one OSD pixel = 3 screen pixels at 1080p, scaled with the
  window height) with 1-bit text (Tahoma Bold, black outline) and scaled up with nearest-neighbour, so it reads as a
  low-res viewfinder LCD next to the smooth UI.
- **Counter like a real camcorder:** it only runs while in REC (a job) and keeps the total for the launcher's run;
  in STBY it stands still. Static menus redraw once a second for the clock.
- **Flip-out screen:** at start the window swings open on a hinge at its left edge (0.6 s, eased, perspective from 96
  vertical strips, darker while it faces away); every visible close (`Exit`, the auto-close after a session) swings
  it shut the same way first (`OnFormClosing` cancels, `Poll` closes when the fold ends). Hidden closes (the
  self-update restart) skip it.
- **Off switch:** `viewfinder-off.txt` in the state folder (`.local\steam-launcher\`) turns off the overlay and the fold.

## Cost

Measured: the lens bitmap plus the OSD scale-up cost about 6.6 ms per redraw at 1920x1080 (60-draw average). The menu
redraws at about 30 fps only on the main screen and during jobs, once a second elsewhere, and not at all while a game
runs (the launcher hides). The lens bitmap and the OSD bitmap are cached per window size.

## Verification

- `SkateLauncher.exe --snapshot <out.png> main|job|play` with the overlay; `main@0.45` draws the main screen 45 % through
  the fold-out.
- Built with the Windows csc (C# 5) like `build.bat`.

## Open / next

- USER-ASKS 485: every part of the UI should feel like skate culture (cards, fonts, tiles, colours, prompts), the
  next step after this overlay.
- A long "newer on the fork" launcher line in the top bar can run under the OSD status row.
- Optional: a Tools entry for the off switch instead of the flag file.
