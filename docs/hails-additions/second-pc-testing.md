# Testing hails-additions on a second PC (couch testing without Steam Link)

Two ways to run `hails-additions` on a second PC (a laptop on the TV):

| | A. Release ZIP | B. Source checkout + Steam launcher |
|---|---|---|
| Needs | nothing but the download | Git, Rust, Visual Studio Build Tools, LLVM, Python 3.13 |
| New builds | in game: Extras > Updates | `git pull`; the launcher rebuilds on the next start |
| Trace-all sessions (`rust-traceall`) | no | yes |
| Ready after a push | about 12 minutes (GitHub builds the release) | at once |

Both send their session logs to the main PC the same way ([Sending sessions back](#sending-sessions-back)).

Commands below are complete: set the variables in the first block of a section once per PowerShell window, then
paste the rest. Steps marked **(you)** ask for a password or click through UAC: run them yourself in a terminal; a
Claude Code session cannot type into those prompts.

## A. Release ZIP

```powershell
$game = "$HOME\Games\RustyTrucks"       # where the game goes (not Program Files, not inside OneDrive)
$disc = "$HOME\Downloads\Skate 3.iso"   # your Skate 3 ISO, or an extracted disc's default.xex
```

1. Download and extract:
   ```powershell
   $rel = 'https://github.com/Hailey-Ross/rusty-trucks/releases/download/hails-additions'
   New-Item -ItemType Directory -Force $game | Out-Null
   Invoke-WebRequest -UseBasicParsing "$rel/skate3rust-windows-x64.zip" -OutFile "$game\skate3rust.zip"
   Expand-Archive "$game\skate3rust.zip" $game -Force; Remove-Item "$game\skate3rust.zip"
   Invoke-WebRequest -UseBasicParsing "$rel/PLAY-WITH-LOG.bat" -OutFile "$game\PLAY-WITH-LOG.bat"
   ```
2. Game files: start `"$game\PLAY-WITH-LOG.bat"` and pick `$disc`. Setup converts the game files into `$game\data`
   (4.7 GB and about 6 minutes measured on a fresh laptop setup; opting in to Ped Speech later adds about 3.4 GB).
   Keep the disc copy: a later **Refresh Game Files** reuses it.
   Alternative (untested): with the game closed on both PCs, copy the main PC's `data\installation.json` and
   `data\installations\<the id named in installation.json>` into `$game\data\`, same layout (leave out `setup.lock`).
   This only works while both run the same build.
3. Optional, same settings as the main PC: copy `%LOCALAPPDATA%\Skate3RustEngine` across (game closed).

New builds: in game, Guide / F1 / Esc > **EXTRAS** > **Updates**, choose **Branch**, enter `hails-additions`. If the
menu then says **GAME FILES OUT OF DATE**: Extras > **Refresh Game Files**.

## B. Source checkout + Steam launcher

```powershell
$repo = "$HOME\Documents\VS-Code\rusty-trucks"   # where the checkout goes
$disc = "$HOME\Downloads\Skate 3.iso"            # your Skate 3 ISO, or an extracted disc's default.xex
```

### Tools (once)

Install one at a time: two installers at once broke the Rust install on the laptop (rustup-init exited with
0xC000013A; afterwards `rustup show` said "no active toolchain"). Each of these shows a UAC prompt **(you)**, and a
prompt hidden behind other windows looks like a hung install, so check the taskbar.

```powershell
winget install --id Git.Git -e
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id LLVM.LLVM -e
winget install --id Python.Python.3.13 -e
winget install --id GitHub.cli -e
```

A window opened before an install does not see the new tools ("not recognized"). Open a new terminal, or reload:

```powershell
$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')
rustc --version; git --version; py -3.13 --version; gh --version
```

If `rustc` says "no active toolchain" or "missing manifest": `rustup toolchain uninstall stable; rustup toolchain
install stable; rustup default stable`.

Long paths must be on (setup's work files pass 260 characters). Check, and switch on if it prints 0 **(you, admin)**:

```powershell
(Get-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem).LongPathsEnabled
# if 0, in an administrator PowerShell:
Set-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem LongPathsEnabled 1
```

### Checkout (once)

The fork has a release TAG named `hails-additions` (the rolling release) next to the BRANCH of the same name, so a
plain `git checkout hails-additions` lands on the tag (detached) and a plain `git push origin hails-additions` fails
with "matches more than one". Use these instead:

```powershell
gh auth login            # (you) GitHub.com, HTTPS, browser
gh auth setup-git        # lets git push with that login
git clone https://github.com/Hailey-Ross/rusty-trucks.git $repo
cd $repo
git switch -c hails-additions --track origin/hails-additions
git config user.name "Hails"
git config user.email "26784904+Hailey-Ross@users.noreply.github.com"
# Later: git pull, and push with
# git push origin refs/heads/hails-additions:refs/heads/hails-additions
```

### Build (and after every pull)

```powershell
cd $repo
powershell -ExecutionPolicy Bypass -File scripts\Build.ps1
```

This builds `bin\skate3rust.exe` (the first build took 9 minutes on the laptop, a rebuild about 3). The Steam launcher
rebuilds by itself when the branch moved.

### Game files on a source checkout

```powershell
cd $repo
powershell -ExecutionPolicy Bypass -File tools\setup-dev-assets.ps1 -Disc $disc
```

It installs the converter's Python packages into `.local\venv-setup`, converts the disc into `data\` (4.7 GB, about
6 minutes on the laptop), points `assets\` at the result and ends with `SKATE_ASSETS_READY`. Run it again after a pull
that changes the converters (the game says **GAME FILES OUT OF DATE**); it refreshes only what changed.
(First run on the laptop 2026-10-17: the conversion and the link worked; its final check stopped on Windows
PowerShell 5.1 and is fixed since.)

The game's own **Extras > Refresh Game Files** works on a checkout too: setup installs its Python packages into
`.local\venv-setup` itself on the first run (about a minute) and restarts in it. Before 2026-10-17 it stopped with
"No module named 'numpy'" there.

### Steam launcher

```powershell
cd $repo
tools\steam-launcher\build.bat
```

Then add `SkateLauncher.exe` to Steam (details in [the launcher README](../../tools/steam-launcher/README.md)). Without
`versions.json` the launcher has one version, this checkout. **Rust engine: play + trace all** (`rust-traceall`) records
every session trace into `logs\trace-all-<time>\`; the after-session send below carries those folders too.

To send after every session, create `.local\steam-launcher\config.json`:

```powershell
New-Item -ItemType Directory -Force "$repo\.local\steam-launcher" | Out-Null
'{ "after_session": "tools\\second-pc\\play-and-send.ps1" }' | Set-Content -Encoding ASCII "$repo\.local\steam-launcher\config.json"
```

A session closed with Steam's **Exit game** is checked and sent on the launcher's next start.

## Sending sessions back

When a session ends, every log, trace-all folder and crash report not sent yet goes to a shared folder on the main PC
over the home network (Windows file sharing). If the main PC cannot be reached they can go as a pull request on GitHub
instead (optional). If both fail they stay in `logs\` and go with the next send, so nothing is lost.

| Script (`tools/second-pc/`) | Runs on | Does |
|---|---|---|
| `setup-main-pc.ps1` | main PC, once, as administrator | creates the account `skatelogs` (hidden from the sign-in screen, no groups, password chosen at the prompt), gives it write access to one folder only, shares that folder as `SkateSessions` (only that account), sets the network to Private and allows File and Printer Sharing (SMB-In) on Private networks only |
| `test-share.ps1` | either PC | logs in as `skatelogs`, writes and deletes a test file, prints `SHARE_TEST ok` |
| `play-and-send.ps1` (+ `PLAY-AND-SEND.bat`) | second PC | play with logging, then send. `-SendOnly` sends without playing, `-Setup` asks the questions again (and does not start the game), `-GitHubTest` checks the GitHub fallback |
| `fetch-github-sessions.ps1` | main PC | unpacks every open `session-logs/*` pull request into the shared folder, then closes it and deletes its branch |
| `remove-main-pc.ps1` | main PC, as administrator | undo: removes the share and the account, turns SMB-In off again (keeps the folder and the logs) |

### Main PC (once)

In an administrator PowerShell, from a checkout of this branch **(you: UAC and the password prompt)**:

```powershell
powershell -ExecutionPolicy Bypass -File tools\second-pc\setup-main-pc.ps1 -Folder "$PWD\.local\session-logs\second-pc"
```

`-InterfaceAlias` picks the network adapter if it is not called `Ethernet` (`Get-NetConnectionProfile` lists them).
The last lines print the share's addresses and the test command. On the main PC itself test with its IP (Windows
allows one login per server name, and your own login already uses the PC name).

Only the main PC's network has to be Private. The second PC's network can stay Public (sending out works from it).

### Second PC (once)

```powershell
$main = 'HAILEY-DESKTOP'   # the main PC's name (Settings > System > About on it) or its IP address
```

Where the scripts are: on a source checkout (B) they are already in `$repo\tools\second-pc` and run from there; they
work on the checkout root (`logs\`, `send-sessions.txt`). For the release ZIP (A) download them next to the game:

```powershell
$base = 'https://raw.githubusercontent.com/Hailey-Ross/rusty-trucks/hails-additions/tools/second-pc'
foreach ($f in 'PLAY-AND-SEND.bat', 'play-and-send.ps1', 'test-share.ps1') { Invoke-WebRequest -UseBasicParsing "$base/$f" -OutFile "$game\$f" }
Unblock-File "$game\PLAY-AND-SEND.bat", "$game\play-and-send.ps1", "$game\test-share.ps1"
$tools = $game              # for A
# $tools = "$repo\tools\second-pc"   # for B
```

1. Check the share **(you: password prompt)**. Expect `SHARE_TEST ok`:
   ```powershell
   powershell -ExecutionPolicy Bypass -File "$tools\test-share.ps1" -Server $main -Computer HAILEY-DESKTOP
   ```
   Use `test-share.ps1` or a real send to check: a plain `Test-Path \\<main PC>\SkateSessions` or `net view` can say
   "does not exist" on a working share until a login has opened it. If it fails: both PCs on the same home network?
   Main PC on and its network Private? Try the IP instead of the name.
2. For the GitHub fallback (optional): `gh auth login` **(you)** first, and the account needs push access to the
   repository. Answer `n` at the GitHub question below if `gh` is not set up yet. The repository is public: the logs
   (PC name, Windows user paths, hardware) become visible there.
3. Answer the setup questions **(you: password prompt)**. This does not start the game:
   ```powershell
   powershell -ExecutionPolicy Bypass -File "$tools\play-and-send.ps1" -Setup
   ```
   It asks for the main PC's name, its IP address (Enter for the one it found; sends fall back to it when the name
   does not answer, as happened on Wi-Fi with "System error 67"), the share (Enter for `SkateSessions`), the account
   (Enter for `skatelogs`) and its password, saves the login for both name and IP in Windows Credential Manager
   (`cmdkey`; never in a file), then asks about the
   GitHub fallback (Enter for `Hailey-Ross/rusty-trucks`). Answers go to `send-sessions.txt` in the game folder (A)
   or checkout root (B).
4. Optional, check the fallback without playing:
   `powershell -ExecutionPolicy Bypass -File "$tools\play-and-send.ps1" -GitHubTest`

Playing: A, double-click `PLAY-AND-SEND.bat` in the game folder. B, use the Steam launcher (sends after each session
with the config above), or `tools\second-pc\PLAY-AND-SEND.bat` (runs `PLAY.bat`, then sends). Send by hand:
`play-and-send.ps1 -SendOnly`.

### Where things land

- Shared folder: `<main PC folder>\<second PC name>\` gets `game-<date>-<time>.log` / `.stderr.log`, the
  `trace-all-<time>\` folders, and crash reports in `CrashReports\`. The main PC watches this folder.
- GitHub fallback: a branch `session-logs/<second PC name>-<date>-<time>` holding one zip (in 40 MB parts
  `.zip.001`, `.002`, ... when it is bigger), and a pull request into `hails-additions` titled "Session logs from
  <PC> (<time>)". Never merge it: `fetch-github-sessions.ps1 -Folder <main PC folder>` on the main PC unpacks it into
  the same place the share would and closes it.
- What was sent is listed in the second PC's `logs\sent.txt` (full paths: a moved folder sends everything again);
  delete a line to send that item again.

## Recomp trace sessions

The second PC can also play the traced Skate 3 recompilation (the research hooks of
[13](13-recomp-research-hooks.md#using-it)) and send its traces back like any other session. A recomp session is a
folder `logs\recomp-<preset>-<date>-<time>\`: the `recomp-` prefix is how the main PC tells it apart from an engine
session. Nothing has to be built on the second PC: the main PC shares its recomp build.

What a session holds: `trace.tsv` (the trace), `pad.txt` (your pad input), `game.out` / `game.err` (the recomp's own
output), `session.txt` (preset, trace categories, PC, start and end, exit code, recomp build, line count, malformed
lines) and, for the audio presets, `audio.f32` (the mixed game sound, float stereo 48 kHz).

**Keep these on your own network.** The recomp build is recompiled game code and the game folder is your copy of the
game: they only travel from the main PC's share to the second PC. A recomp session with `audio.f32` (game sound) never
goes through the GitHub fallback; it waits for the share. Traces without sound may use the fallback (they hold
measurements, not game code).

| Script (`tools/second-pc/`) | Runs on | Does |
|---|---|---|
| `share-recomp.ps1` | main PC | puts the recomp build (`skate3.exe`, its `rexruntime` DLL, `build.txt` with the build's commit) in `<shared folder>\_recomp\bin\`; with `-WithGame` also stages the recomp game folder in `_recomp\game\` (once, about 7.5 GB); with `-WithSave` your recomp save in `_recomp\save\` (about 1 GB) |
| `play-recomp.ps1` (+ `PLAY-RECOMP.bat`) | second PC | `-GetGame` copies the staged game folder, checks it by size and file count and then removes the staged copy; `-GetSave` backs up the second PC's save and copies the main PC's in (verified by hash); `-Update` copies the newest build; `-Trace <preset>` plays, checks the trace and sends; `-List` lists the presets; `-NoSend` keeps the session for a later send |

### Main PC (each new recomp build)

From a checkout of this branch, with the recomp built from the `research-hooks` branch (see 13):

```powershell
powershell -ExecutionPolicy Bypass -File tools\second-pc\share-recomp.ps1 -Build <recomp checkout>\out\build\<preset> -Folder <the shared folder>
# first time, also the game folder (the folder you play the recomp from, with game\ and dlc\):
#   ... -Game <recomp game folder> -WithGame
# your save (career, settings, downloaded content from %APPDATA%\skate3; recomp closed):
#   ... -WithSave
```

### Second PC (once)

The share must work already ([Sending sessions back](#sending-sessions-back), `play-and-send.ps1 -Setup`): the recomp
scripts use the same login. Then, from the game folder (A) or `$repo\tools\second-pc` (B):

```powershell
.\PLAY-RECOMP.bat -GetGame   # about 7.5 GB over the network, once
.\PLAY-RECOMP.bat -GetSave   # the main PC's save (about 1 GB); backs up this PC's own save first
.\PLAY-RECOMP.bat -Update    # the recomp build; again whenever the main PC shares a new one
```

Both land in `.local\recomp\` (`bin\`, `game\`) under the game folder or checkout. For the release ZIP (A), download
the scripts next to the game first:

```powershell
$base = 'https://raw.githubusercontent.com/Hailey-Ross/rusty-trucks/hails-additions/tools/second-pc'
foreach ($f in 'PLAY-RECOMP.bat', 'play-recomp.ps1', 'play-and-send.ps1') { Invoke-WebRequest -UseBasicParsing "$base/$f" -OutFile "$game\$f" }
Unblock-File "$game\PLAY-RECOMP.bat", "$game\play-recomp.ps1", "$game\play-and-send.ps1"
```

### Playing a recomp session

`PLAY-RECOMP.bat <preset>` (for example `PLAY-RECOMP.bat props`). It refuses to start while another game runs (one
game at a time), prints what to do, plays, writes `session.txt`, checks every trace line (`KIND<tab>ms...`; any other
line makes the trace unusable and it says so) and sends with `play-and-send.ps1 -SendOnly`.

| Preset | Trace categories | What to do |
|---|---|---|
| `props` | props | About 2 minutes at DownTown, Rosalita Skate Park (map teleport): the garbage bin about 14 m straight ahead and the flat bench about 13 m away. Roll slowly into the bin, then hit it fast (same bin is fine once it settled), roll onto the bench seat (no grind), grind the bench seat, then get off the board and walk into the bench. About 5 seconds between them, in this order. |
| `npc` | npc, world, traffic | Skate around peds and NPC skaters as normal. |
| `pedbody` | npc, pedbody | Bump into peds, run into them fast, knock some down. |
| `traffic` | traffic, skitch, npc | Skitch cars, let go at different speeds, also of a stopped car; ride near junctions. |
| `grind` | grind | Grind the rails and stairs by the University library (full-rate lines inside that box). |
| `audio` | audio, plus the sound | Play as normal. |
| `audiox` | audio, audiox, dsp, plus the sound | Heavy: 1 or 2 minutes around the sound being studied. |
| `all` | every hook category: audio, dsp, npc, world, traffic, aiskater, props, pedbody, skitch, grind (with the library box), physics, menus, render, cas, feinput, plus the sound; not audiox / dspmod (per-frame, own presets) or watch (needs an address list) | Everything at once (large). A new hook category is added here too. |

Quit with Escape or F1 (or hold LB + RB + Back for about a second), then **Quit to the desktop**. Closing with Steam's
**Exit game** is safe too: the trace is written as it goes.

The recomp's save is in `%APPDATA%\skate3` (profile, settings and downloaded content), not in the game folder. Without
`-GetSave` the second PC starts a new career at character creation. After `-GetSave` it plays a copy of the main PC's
career; changes there do not go back to the main PC.

Remember what the recomp is ([13](13-recomp-research-hooks.md#warning-reference-and-information-gathering-only)): its
frame rate and timing differ from the console, so its traces validate retail logic; they are not ground truth.
Credits: [skate3recomp](https://github.com/mchughalex/skate3recomp) by @mchughalex, the
[rexglue SDK](https://github.com/rexglue/rexglue-sdk) and [Xenia](https://github.com/xenia-project/xenia).

## Local hails-additions patches

Between pushes (for example during the hours when nothing is committed), the main PC can share its work in progress
so the second PC tests it before it is pushed: the pushed `hails-additions` plus the main PC's unpushed commits,
uncommitted changes and new files, as one patch in the same share (`<share>\_patches\`). The second PC applies it to
a launcher-only copy and plays it as the launcher version **hails-local**, next to its normal `hails-additions`
(which keeps following the fork). Only what git tracks travels: no build output, game files or logs.

### Main PC (each new patch)

```powershell
powershell -ExecutionPolicy Bypass -File tools\second-pc\share-patches.ps1 -Folder <the shared SkateSessions folder> `
  -Source .local\wt-living, .local\wt-hails -Notes "What to test\nSecond line"
```

Name every worktree with new work (user 2026-10-19: "everything should be updated that has new changes"): the
living-world work and `wt-hails` (the launcher, these scripts, release notes).

`-Source` takes one or more worktrees (default `.local\wt-living`). Each one's local work is taken against its pushed
branch through a temporary index (its own files and index are not touched) and applied to `origin/hails-additions` in
a throwaway worktree first, so a conflict stops here (merge `hails-additions` into the branch, then run it again). The
share then holds `hails-local.patch`, `files.txt`, `patch.txt` (base commit, time, file count, SHA-256, notes, sources)
and `tools\` (a copy of the second PC's script until it is on `hails-additions`).

### Second PC

1. Once, until the scripts are on `hails-additions`: copy `<share>\_patches\tools\get-patches.ps1` and
   `GET-PATCHES.bat` into the checkout's `tools\second-pc\`. The share account from `play-and-send.ps1 -Setup` is used.
2. Each new patch: `tools\second-pc\GET-PATCHES.bat` (`-Status` shows what the share has and what is applied,
   `-Remove` removes the copy and its launcher version). It checks the hash, resets `.local\versions\hails-local` to
   the base commit (the previous patch's changes and new files go), applies the patch and adds or updates `hails-local`
   in `.local\steam-launcher\versions.json` (a working-tree version: rebuilt whenever a new patch changes it).
   When the patch changes the launcher (`tools\steam-launcher`) or these scripts (`tools\second-pc`), they are updated
   in the checkout too and `SkateLauncher.exe` is rebuilt (a running launcher keeps going; restart it to get the new
   one). Each run first puts those two folders back to the checkout's own commit, so a later pull stays clean;
   `-Remove` does the same.
3. In the launcher pick **hails-local**; its first launch builds it (several minutes). Sessions are sent as usual.

Once the work is pushed, `hails-additions` itself has it; `GET-PATCHES.bat -Remove` then tidies up.

## Moving the ISO

A FAT32 flash drive cannot hold a file over 4 GB, whatever its free space. Format it as exFAT (Explorer: right-click
the drive > Format > File system exFAT > Quick Format; this erases the drive).

## Couch tips

- Pads work over USB or Bluetooth (SDL); the log names each pad and whether it is wired or wireless
  (`Controller <n>:` and `RUMBLE pad <n>:` lines).
- A crash shows a report window; the report is in the log too.
- Vibration is Off by default for now: Start > Options > Game Settings switches it on.
