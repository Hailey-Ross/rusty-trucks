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

Play with `PLAY-AND-SEND.bat` instead of `PLAY-WITH-LOG.bat`: it logs the session the same way, and when you quit it
sends every log and crash report not sent yet to a shared folder on the main PC over the home network (Windows file
sharing, SMB). If the main PC cannot be reached it can send them as a pull request on GitHub instead (optional). If
both fail, the logs stay in `logs\` and go with the next send, so nothing is lost. Then say what you did and roughly
when in the session.

The scripts are in [`tools/second-pc/`](../../tools/second-pc/):

| Script | Runs on | Does |
|---|---|---|
| `setup-main-pc.ps1` | main PC, once, as administrator | creates the account `skatelogs` (hidden from the sign-in screen, no groups, password chosen at the prompt), gives it write access to one folder only, shares that folder as `SkateSessions` (only that account), sets the network to Private and allows File and Printer Sharing (SMB-In) on Private networks only |
| `test-share.ps1` | either PC | logs in as `skatelogs` with `net use`, writes and deletes a test file, prints `SHARE_TEST ok` |
| `PLAY-AND-SEND.bat` + `play-and-send.ps1` | second PC | play with logging, then send; `PLAY-AND-SEND.bat send` sends without playing, `PLAY-AND-SEND.bat setup` asks the questions again |
| `fetch-github-sessions.ps1` | main PC | unpacks every open `session-logs/*` pull request into the shared folder, then closes it and deletes its branch |
| `remove-main-pc.ps1` | main PC, as administrator | undo: removes the share and the account, turns SMB-In off again (keeps the folder and the logs) |

### Main PC (once)

From a checkout of this branch, in an administrator PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File tools\second-pc\setup-main-pc.ps1 -Folder <folder for the logs>
```

`-InterfaceAlias` picks the network adapter if it is not called `Ethernet` (`Get-NetConnectionProfile` lists them).
The last line prints the share's two addresses, `\\<PC name>\SkateSessions` and `\\<IP>\SkateSessions`. Check the
login with `tools\second-pc\test-share.ps1 -Server <its IP> -Computer <its PC name>` (on the main PC itself use the
IP: Windows allows one login per server name and yours already uses the PC name).

### Second PC (once)

Written so it can be followed by hand or by a Claude Code session on the second PC.

1. Download the three files next to `skate3rust.exe` (raw links, hails-additions branch):
   ```powershell
   $base = 'https://raw.githubusercontent.com/Hailey-Ross/rusty-trucks/hails-additions/tools/second-pc'
   foreach ($f in 'PLAY-AND-SEND.bat', 'play-and-send.ps1', 'test-share.ps1') {
       Invoke-WebRequest -UseBasicParsing "$base/$f" -OutFile "<game folder>\$f"
   }
   Unblock-File "<game folder>\*.ps1", "<game folder>\*.bat"
   ```
2. Check the share first: `powershell -ExecutionPolicy Bypass -File "<game folder>\test-share.ps1" -Server <main PC
   name or IP> -Computer <main PC name>`; type the `skatelogs` password. Expect `SHARE_TEST ok`. If it fails: both PCs
   on the same home network? The main PC's network Private? Try the IP instead of the name.
3. Optional, for the GitHub fallback: `winget install GitHub.cli`, then `gh auth login` (GitHub.com, HTTPS, browser).
   The account needs push access to the repository. The repository is public: the logs (PC name, Windows user paths,
   hardware) become visible there.
4. Double-click `PLAY-AND-SEND.bat`. The first run asks: the main PC's name (or IP), the share name (Enter for
   `SkateSessions`), the account (Enter for `skatelogs`), then saves the password in Windows Credential Manager
   (`cmdkey`; it is never written to a file), and asks whether to use the GitHub fallback and which repository (Enter
   for `Hailey-Ross/rusty-trucks`). The answers go to `send-sessions.txt` next to the exe; `PLAY-AND-SEND.bat setup`
   asks again.
5. Optional: `powershell -ExecutionPolicy Bypass -File "<game folder>\play-and-send.ps1" -GitHubTest` sends one small
   test file as a pull request (checks the fallback without playing).

### Where things land

- Shared folder: `<main PC folder>\<second PC name>\game-<date>-<time>.stderr.log` (and `.log`), crash reports in
  `<second PC name>\CrashReports\`.
- GitHub fallback: a branch `session-logs/<second PC name>-<date>-<time>` holding one zip, and a pull request into
  `hails-additions` titled "Session logs from <PC> (<time>)". Never merge it: run `fetch-github-sessions.ps1 -Folder
  <main PC folder>` on the main PC, which unpacks it into the same folder as the share would and closes it.
- What was sent is listed in the second PC's `logs\sent.txt`; delete a line to send that file again.

## Couch tips

- Pads work over USB or Bluetooth (SDL); the log names each pad and whether it is wired or wireless
  (`Controller <n>:` and `RUMBLE pad <n>:` lines).
- A crash shows a report window; the report is in the log too.
