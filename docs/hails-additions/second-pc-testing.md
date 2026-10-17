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

## Moving the ISO

A FAT32 flash drive cannot hold a file over 4 GB, whatever its free space. Format it as exFAT (Explorer: right-click
the drive > Format > File system exFAT > Quick Format; this erases the drive).

## Couch tips

- Pads work over USB or Bluetooth (SDL); the log names each pad and whether it is wired or wireless
  (`Controller <n>:` and `RUMBLE pad <n>:` lines).
- A crash shows a report window; the report is in the log too.
- Vibration is Off by default for now: Start > Options > Game Settings switches it on.
