# Testing hails-additions on a second PC (couch testing without Steam Link)

A second PC (a laptop on the TV) needs no Rust, Git or build tools: it runs the fork's rolling release of
`hails-additions`, which GitHub rebuilds on every push to the branch (workflow "Fork release (hails-additions)",
about 12 minutes after the push).

## First time

1. Download from https://github.com/Hailey-Ross/rusty-trucks/releases/tag/hails-additions :
   `skate3rust-windows-x64.zip` and `PLAY-WITH-LOG.bat`.
2. Extract the zip into a folder of its own (not Program Files, not inside OneDrive) and put `PLAY-WITH-LOG.bat`
   next to `skate3rust.exe`.
3. Game files, pick one:
   - **Setup on the second PC (simplest):** copy your Skate 3 ISO (about 6 GB) or an extracted disc folder (with
     `default.xex`) to it. Start `PLAY-WITH-LOG.bat` and pick it; setup converts everything (about 13.5 GB, takes a
     while). Keep the disc copy there: a later **Refresh Game Files** reuses it.
   - **Copy converted files from the main PC (untested):** with the game closed on both, copy the main PC's
     `data\installation.json` and `data\installations\<the id named in installation.json>` into
     `<second PC folder>\data\`, same layout (leave out `setup.lock`). Works only while both run the same build; if
     the menu says **GAME FILES OUT OF DATE**, run Extras > Refresh Game Files, which asks for the disc.
4. Optional, same settings as the main PC: copy `%LOCALAPPDATA%\Skate3RustEngine` across (game closed).
   Vibration is Off by default for now: Start > Options > Game Settings switches it on.

**Flash drive refuses the ISO?** A FAT32 drive cannot hold a file over 4 GB, whatever its free space. Format it as
exFAT (Explorer: right-click the drive > Format > File system exFAT > Quick Format; this erases the drive).

## Getting a newer build

In game: Guide / F1 / Esc > **EXTRAS** > **Updates**, choose **Branch**, enter `hails-additions`. It refreshes only
what changed. After a push it takes about 12 minutes before the update is there. If the menu then says
**GAME FILES OUT OF DATE**: Extras > **Refresh Game Files**.

## Sending sessions back

Always start with `PLAY-WITH-LOG.bat`. Each session writes `logs\game-<date>-<time>.stderr.log` next to the exe.
Copy the logs to look at back to the main PC and say what you did and roughly when in the session.

## Couch tips

- Pads work over USB or Bluetooth (SDL); the log names each pad and whether it is wired or wireless
  (`Controller <n>:` and `RUMBLE pad <n>:` lines).
- A crash shows a report window; the report is in the log too.
