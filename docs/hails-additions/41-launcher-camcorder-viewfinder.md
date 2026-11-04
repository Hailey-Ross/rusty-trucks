# 41. Launcher: the camcorder

Status: on `hails-additions` (launcher only, `tools/steam-launcher/SkateLauncher.cs`). Not for upstream.

## Problem

The couch launcher looked like a generic dark console menu. The goal (USER-ASKS 484, 486, 487) is a launcher that is
a late-90s / 2000s MiniDV skate-video camcorder (VX1000 era) the way the nativeMons launcher is a Pokedex phone: the
window is the device, its keys are the buttons, every screen is the camera's own menu, and skate culture around it.
The first step (an overlay over the old screens) was replaced by this.

## Change

- **The window is the camera:** a borderless per-pixel-alpha (layered) window, 72 % of the work area's height at
  16:10, drawn in a 1600 x 1000 unit space: the "SK8-1000 Digital Video Camera" seen from its LCD side (lens with
  focus ring and hood, carry handle with microphone and accessory shoe, body with model print, tally lamp), standing
  on a concrete ledge with angle-iron coping, grind scuffs, wax and a tag. The board rolls along the coping in front
  of the camera and pops an ollie every few seconds. Drag the camera by its body. Every frame is drawn into one bitmap
  and handed over with `UpdateLayeredWindow` (`Native.Push`).
- **The keys are the controls** (clickable; the pad and keyboard work as before):

  | Key | Does |
  | --- | --- |
  | MENU, bezel STOP | Back (B) |
  | SEL / PUSH EXEC dial: up, down, EXEC; bezel PLAY | Move, select (A) |
  | Zoom rocker W / T, bezel REW / FF | Move (left / right step the list like the dial) |
  | START/STOP | Play the selected version on the main menu, select elsewhere |
  | POWER | Close (with the power-off) |

- **The flip-out LCD with its full bezel:** moulded frame with a raised lip, the hinge knuckle on the left, the ridged
  OPEN thumb tab, the backlight sensor, a dark gasket round the glass with one reflection, and on the bezel's bottom a
  speaker grille, "16:9 WIDE LCD" print, DISPLAY and the tape keys. Behind it (seen while it swings) the body's recess
  with the DV IN/OUT, A/V, S VIDEO, LANC and DC IN jacks.
- **Every screen is the camera's menu**, drawn low-res (one LCD pixel = 2 to 3 screen pixels) with 1-bit text and
  scaled up with hard pixels: the status row (STBY / REC with its blinking dot while a job runs, SP, the tape counter
  that only runs while recording, the battery), a title tab, the list with an orange selection bar and the selected
  row's info box (READY / NOT AVAILABLE, what it does, what it records), the job screen (segmented tape bar for cargo's
  step count, counts, the first error or the newest log lines), text pages, the power-on logo with the start check's
  step, the key hints and the date and time stamp. Behind the menus the camera points at a dimmed skate spot at dusk
  (stairs with a handrail, a ledge). All lists are one column now (the old tile grids are gone).
- **Power on and off:** the whole camera unfolds on one clock (`Assembly`, 1.75 s): the ledge slides up, the body
  drops onto it with a bounce, the lens runs out, the handle rises, the keys pop in one by one, the LCD cover swings
  out and the screen side turns to face you, the screen comes on, the board rolls in. Every visible close (POWER,
  Exit, the auto-close after a session, a close from Steam or the taskbar, which arrive as `None` /
  `TaskManagerClosing`) plays it backwards, 1.3 times faster, then the window closes. A hidden close (self-update
  restart) skips it.
- **Variance:** every power-on and power-off rolls its own timing jitter, drop height, bounce strength, tilt while
  falling, lens overshoot, key pop beats and overshoot, LCD swing speed, and the board's speed, position and ollie
  rhythm (`Vary`), so it never plays the same way twice.
- **The sticker:** one sticker slapped on the camera years ago, picked once per user at the first start (design, spot
  on the bezel / lens / body print, angle, wear) and kept in `camcorder-sticker.txt` in the state folder. Worn: faded,
  scratched, grimy, chipped at the edge and one edge peeled up and folded back with old glue under it. The set: an
  original "filmer" character on clear vinyl in the style of hand-drawn riso character stickers (wobbly purple line art,
  flat fills printed off the lines), and parodies of skate brand stickers: Blickey (workwear tag), Spitball Wheels
  (flame-head wheel badge), Supremely Mid (box logo), Thrashed (flame lettering, "skate and rest").
- **Game stickers replace the parody set:** PNGs in the state folder's `stickers\` folder are used instead when there
  are any (same saved spot and angle). Finding the retail stickers and exporting them there is the living-world TODO
  `.claude/todo/living-world-stickers.md`; nothing from the game is committed.

## Cost

Measured with `SkateLauncher.exe --bench <out.txt> [WxH]` at 1243 x 777: about 7 to 8 ms per frame settled and through
the whole unfold, plus about 1.7 ms to hand the frame to the window. That came from:
- The LCD's two faces are drawn once per size on the first (invisible) frame and cut into 48 strips; the swing only
  draws the strips (each with a pixel of overlap so they meet without seams), nearest-neighbour while moving. Drawing
  one big image per strip, a texture brush, source rectangles, image attributes or bilinear sampling each cost 20 to
  110 ms a frame in GDI+.
- The flat faces are copied 1:1 without resampling.
- The low-res LCD canvas and the dusk scene are cached per size.

The window redraws every timer tick while it animates and every second tick (about 30 fps) otherwise; nothing while a
game runs (the launcher hides).

## Verification

- `SkateLauncher.exe --snapshot <out.png> <screen>[@<0..1>][#<seed>] [WxH]`: any screen, `@` a point in the power-on,
  `#` the sticker from that seed (snapshots never write the sticker file).
- Live window runs (screen captures of the window only): power-on stages, the settled main menu, the power-off reverse
  (gone after about 1.4 s), the sticker kept between runs.
- Built with the Windows csc (C# 5) like `build.bat`, no warnings.

## Open / next

- Mouse clicks on the LCD rows and the shell keys are mapped (`LoHit`, `Key`) but not yet clicked through by hand.
- More sticker designs; the game's own stickers once found (living-world TODO).
- The menus could use the camera's own icon column (Handycam style tabs) on the main menu.
