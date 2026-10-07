# Steam launcher (couch testing)

Start any version of the engine (`main`, your branches, open PRs, work in progress), the Skate 3 recomp with a
tracing option or the PS3 version of Skate 3 in RPCS3, from Steam or Steam Link with a controller, and see after each session whether it went fine.
Built for testing from the couch: compare two versions back to back, try a PR exactly as it was pushed, record a
recomp trace session, without touching a keyboard.

What it does:
- **Versions:** each version is its own git worktree with its own `bin\`, sharing this checkout's prepared assets.
  Launching a version that isn't built yet, or whose branch has new pushed commits, sets it up and builds it first
  (fast-forward only; local work is never touched).
- **What to test:** each version carries notes, shown for a few seconds before the game starts.
- **Modes** for the engine: play, play with the per-frame audio state log, play with the audio trace, play with a
  dev test mod switched on, play with a performance trace.
- **Recomp modes** (optional, for use with the Skate 3 recomp's research hooks:
  [Hailey-Ross/skate3recomp, branch `research-hooks`](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks)):
  start the recomp with trace categories, a sound recording and the pad recording, one session folder per run.
- **RPCS3 modes** (optional, for the [RPCS3](https://rpcs3.net) PS3 emulator and your own PS3 copy of Skate 3):
  start the game straight from the menu, fullscreen or in a window, as a retail reference for timing and cadence
  (see `docs/hails-additions/29-rpcs3-reference.md`).
- **Only what is set up shows:** the recomp modes only appear when `recomp.exe` exists, the RPCS3 modes only when
  `rpcs3.exe` and `rpcs3.game` exist; with neither, the retail item is not in the menu at all.
- **After each session** a short check: the game log's panics, the state log's or trace's malformed lines.
- **One game at a time:** it starts nothing while `skate3rust.exe`, `skate3.exe` or `rpcs3.exe` runs. While a game runs,
  `PLAYING.txt` exists in the state folder, so other tools and scripts can wait for it.

Windows only. Needs Windows PowerShell 5.1 (built in), git, the Rust toolchain and this repository's dev setup
(`BUILD.bat` works and `assets\` points at your prepared assets).

## Files

| File | What it is |
|---|---|
| `SkateLauncher.cs` | The small exe Steam starts: menu, controller input, result screens. Build it with `build.bat`. |
| `launcher.ps1` | Does the work: versions, builds, launching, session checks. Run it directly from a keyboard too. |
| `launcher.bat` | Keyboard menu (`launcher.bat`), or `launcher.bat <entry> [label] [-Version <id>]`. |
| `build.bat` | Builds `SkateLauncher.exe` with the C# compiler that ships with Windows. |
| `versions.example.json` | Example version list. |
| `config.example.json` | Example config for the recomp and RPCS3 modes. |

State and your own settings live in `.local\steam-launcher\` (gitignored): `versions.json`, `config.json`,
`version.txt` (the selected version), `last_session.txt` (the last result, also readable remotely) and `PLAYING.txt`.
If you copy the launcher folder somewhere outside `tools\` (two levels below the repository, e.g.
`.local\steam\`), it keeps those files next to itself instead.

## Setup

1. **Build the exe:** double-click `tools\steam-launcher\build.bat`. It writes `SkateLauncher.exe` next to it.
2. **List your versions:** copy `versions.example.json` to `.local\steam-launcher\versions.json` and edit it.
   Without it, the launcher has one version: this checkout as it is.

   | Field | Meaning |
   |---|---|
   | `id` | Short name, used in launch options (`@id`). |
   | `name` | Shown in the menu. |
   | `branch` | The branch to check out and follow (fetched from `remote`, default `origin`). |
   | `dir` | Where its worktree lives, relative to the repository (e.g. `.local/versions/main`); `.` = this checkout. |
   | `notes` | What to test, shown before the game starts (`\n` for new lines). |
   | `mods_enable` | Optional: the mod id(s) the "dev test mods" mode turns on (`SKATE3_MODS_ENABLE`). |
   | `dev` | Optional: `true` = build from the working tree as it is; rebuilt only when `bin\` is missing. |
   | `target` | Optional: cargo target folder, relative to the repository (default: the worktree's own `target\`). |

   Each version builds into its own target folder (they can't share one: cargo would mix the branches' crates). The
   first build of a version takes several minutes; `launcher.ps1 build <id>` builds one ahead of time.
3. **Optional, the recomp modes:** copy `config.example.json` to `.local\steam-launcher\config.json` and set
   `recomp.exe` (your build of the recomp) and `recomp.working_dir` (the folder it runs from). Each entry in
   `recomp_entries` becomes a mode:

   | Field | Meaning |
   |---|---|
   | `id`, `name`, `hint` | Entry id (starts with `recomp-`), menu text, help line. |
   | `trace` | Trace categories (`SKATE3_TRACE`); empty = plain play. |
   | `capture` | `true` = also record the mixed game sound (`audio.f32`). |
   | `ask_label` / `label` | Ask for a label for the session folder, or always use this one. |
   | `folder` | Folder name prefix (default: the id without `recomp-`). |
   | `bat`, `bat_args` | Run your own batch file instead (path relative to the repository). |

   Sessions go to `recomp.sessions` (default `.local\recomp\sessions\<prefix>_[label_]<time>\`: `trace.tsv`,
   `pad.txt`, `audio.f32`, `game.out`, `game.err`). `recomp.trace_check` can name a Python script that prints
   malformed-line counts for a trace; the result screen shows them. You build the recomp yourself from your own copy
   of the game; nothing from the game is included here.
4. **Optional, the RPCS3 modes:** in the same `config.json`, set `rpcs3.exe` (your RPCS3 install) and `rpcs3.game`
   (the folder of your own Skate 3 PS3 copy that contains `PS3_GAME`, or its `PS3_GAME\USRDIR\EBOOT.BIN`), and
   optionally `rpcs3.args` (default `--no-gui --fullscreen`). Each entry in `rpcs3_entries` becomes a mode: `id`
   (starts with `rpcs3-`), `name`, `hint`, optional `args` overriding `rpcs3.args`. The example has `rpcs3-play`
   (fullscreen) and `rpcs3-windowed` (`--no-gui`, for recording). RPCS3 uses its own per-game config and pads: set
   them up once in the RPCS3 window. Close the game with Alt+F4 or Steam's **Exit game**; nothing is checked after
   an RPCS3 session.
5. **Add it to Steam:** Steam → **Games → Add a Non-Steam Game to My Library → Browse** → pick
   `tools\steam-launcher\SkateLauncher.exe` → **Add Selected Programs**. In the new entry's **Properties**, rename it
   and put an entry in **Launch options** (table below). Add the exe once per shortcut you want.
6. **Controller:** leave Steam Input on its normal **Gamepad** layout; the games get the controller as usual.

### Why one shortcut per mode

Steam's virtual controller (Steam Link, or Steam Input on) is not visible to a console program, so the menu inside
the exe only works with a keyboard, or with Steam's controller set to mouse / keyboard mode. Shortcuts with launch
options need no menu: each starts straight away, shows the result for 10 s at the end and closes by itself.

| Shortcut (suggested name) | Launch options |
|---|---|
| Skate: next version | `version-next` (selects the next version and shows its notes) |
| Skate: play | `rust-play` (the selected version) |
| Skate: play + audio state log | `rust-statelog` |
| Skate: play + dev test mods | `rust-devmods` |
| Skate: play + audio trace | `rust-trace` |
| Skate: main | `rust-play @main` (a fixed version, for comparing) |
| Skate: my branch | `rust-play @my-branch` |
| Skate: recomp | `recomp-play` |
| Skate: recomp audio trace | `recomp-audio` |
| Skate: RPCS3 | `rpcs3-play` |

`version-next` plus the "selected version" shortcuts cover every version with a handful of shortcuts; the `@id`
shortcuts are fixed, for back-to-back comparisons. A shortcut without launch options opens the full menu.

## Entries and commands

| Entry | What it runs |
|---|---|
| `rust-play` | the version's `PLAY.bat` |
| `rust-statelog` | play + `SKATE_AUDIO_STATE_LOG` (`.local\audio-state-logs\state_*.tsv`; builds with the native audio port write it) |
| `rust-trace` | play + `SKATE_AUDIO_TRACE=1` (native sound starts in `logs\game-*.log`) |
| `rust-devmods` | play with the version's `mods_enable` mod(s) on |
| `rust-perf` | play + Bevy performance trace (`PLAY.bat -trace`) |
| `recomp-*` | the recomp modes from `config.json` (only when `recomp.exe` exists) |
| `rpcs3-*` | the RPCS3 modes from `config.json` (only when `rpcs3.exe` and `rpcs3.game` exist) |

A shortcut for a mode that is not set up shows one message saying which config value is missing, and starts nothing.

Commands (`launcher.ps1 <command>`, or as launch options): `versions` (list with status), `version-next`,
`version-prev`, `version-set <id>`, `notes [id]`, `build <id>`, `list` / `entries`, `hidden <id>` (why an entry is not available), `pending` (checks a session that
was closed with Steam's **Exit game** before its check; the exe runs it at every start).

## Example

```
> launcher.ps1 versions
main        main             built at 4488651   *
my-branch   My branch        out of date (built 1a2b3c4, now 5d6e7f8): next launch rebuilds

> launcher.ps1 rust-play -Version my-branch
  Version: My branch
  Updating my-branch (2 new commits)...
  Building My branch at 5d6e7f8. The first build of a version takes several minutes...
  Starting: Rust engine: play [my-branch]
  ...
> type .local\steam-launcher\last_session.txt
2026-10-04 13:03:44  rust-play  [my-branch]
game-20261004-130134.log: no panics
```

## Notes

- Closing a game with Steam's **Exit game** also closes the launcher, so its after-session check is skipped; the
  next start of any shortcut catches it up.
- A version's worktree is a normal git worktree: remove it with `git worktree remove <dir>` (its `assets` folder is
  a junction to your prepared assets; remove the junction first, never delete through it).
- `rust-devmods` sets `SKATE3_MODS_ENABLE` (where the version reads it), which enables those mods unless you
  switched them off in the mod menu before; mods are read from the version's own `mods\` folder.
