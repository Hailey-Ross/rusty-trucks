# Couch / Steam launcher: start any engine version (main, your branches, open PRs) or the recomp with a tracing
# option, from the couch with a controller. Started by SkateLauncher.exe (or launcher.bat from a keyboard).
# Windows PowerShell 5.1 compatible. See README.md.
#   launcher.ps1 <entry> [label] [-Version <id>] [-NoPrompt]   run an entry
#   launcher.ps1 entries                                      list the entries (id, name, hint; tab-separated)
#   launcher.ps1 versions                                     list the versions (id, name, status, selected)
#   launcher.ps1 version-next | version-prev | version-set <id>   select a version (prints its notes)
#   launcher.ps1 notes [id]                                   what to test in a version
#   launcher.ps1 build <id>                                   set up / update / build a version without playing
#   launcher.ps1 pending                                      check a session that closed before its check
#   launcher.ps1                                              keyboard menu
# While a game runs, PLAYING.txt exists in the state folder: tools and agents should launch nothing while it does.
param([string]$Entry, [string]$Label, [switch]$NoPrompt, [string]$Version)

$ErrorActionPreference = 'Stop'
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = (Resolve-Path (Join-Path $Here '..\..')).Path

# Config and state: next to the script when it lives outside tools\ (a private copy), otherwise in
# .local\steam-launcher\ (gitignored), so the repository stays clean.
$StateDir = $Here
if ($Here -like (Join-Path $Repo 'tools') + '\*') { $StateDir = Join-Path $Repo '.local\steam-launcher' }
New-Item -ItemType Directory -Path $StateDir -Force | Out-Null
$Playing = Join-Path $StateDir 'PLAYING.txt'
$LastSession = Join-Path $StateDir 'last_session.txt'
$SelectedFile = Join-Path $StateDir 'version.txt'
$StateLogs = Join-Path $Repo '.local\audio-state-logs'

function Read-Json([string]$Path) {
    # PowerShell 5.1 returns a JSON array as one object: callers unroll arrays with ForEach-Object.
    return (Get-Content $Path -Raw | ConvertFrom-Json)
}
$Config = $null
$ConfigFile = Join-Path $StateDir 'config.json'
if (Test-Path $ConfigFile) { $Config = Read-Json $ConfigFile }
$VersionsFile = Join-Path $StateDir 'versions.json'
if (Test-Path $VersionsFile) { $Versions = @((Read-Json $VersionsFile) | ForEach-Object { $_ }) }
else {
    # No versions.json: one version, this checkout as it is.
    $Versions = @([pscustomobject]@{ id = 'this'; name = 'This checkout'; branch = ''; dir = '.'; dev = $true;
                                     notes = 'This checkout as it is. Add a versions.json to compare branches (see README.md).' })
}
$Remote = 'origin'
if ($Config -and $Config.remote) { $Remote = $Config.remote }

function Get-Version([string]$Id) {
    if (-not $Id) {
        $Id = $Versions[0].id
        if (Test-Path $SelectedFile) { $s = (Get-Content $SelectedFile -Raw).Trim(); if ($s) { $Id = $s } }
    }
    $v = $Versions | Where-Object { $_.id -eq $Id } | Select-Object -First 1
    if (-not $v) { throw "Unknown version '$Id' (see versions.json)" }
    return $v
}
function Version-Dir($v) { if ($v.dir -eq '.') { return $Repo }; return (Join-Path $Repo $v.dir) }
function Invoke-Git([string]$Dir) { $ErrorActionPreference = 'Continue'; & git.exe -C $Dir @args 2>$null }

# One line of status for a version: built (commit) / needs a build / not set up.
function Version-Status($v) {
    $d = Version-Dir $v
    if (-not (Test-Path (Join-Path $d '.git'))) { return 'not set up yet (first launch builds it)' }
    $exe = Join-Path $d 'bin\skate3rust.exe'
    if (-not (Test-Path $exe)) { return 'not built yet (first launch builds it)' }
    if ($v.dev) { return "built from the working tree ($((Get-Item $exe).LastWriteTime.ToString('MM-dd HH:mm')))" }
    $marker = Join-Path $d 'bin\built_commit.txt'
    $head = (Invoke-Git $d rev-parse --short HEAD)
    $built = ''; if (Test-Path $marker) { $built = (Get-Content $marker -Raw).Trim() }
    if ($built -eq $head) { return "built at $head" }
    return "out of date (built $built, now $head): next launch rebuilds"
}

# Makes sure the version's worktree exists, follows its pushed branch and is built. True = ready.
function Ensure-Version($v) {
    $ErrorActionPreference = 'Continue'
    $d = Version-Dir $v
    if (-not (Test-Path (Join-Path $d '.git'))) {
        Write-Host "  Setting up $($v.name): new worktree for $($v.branch)..."
        New-Item -ItemType Directory -Path (Split-Path $d) -Force | Out-Null
        & git -C $Repo fetch -q $Remote $v.branch
        & git -C $Repo worktree add $d $v.branch
        if ($LASTEXITCODE -ne 0) { Write-Host '  Could not create the worktree.' -ForegroundColor Red; return $false }
    }
    # Every version shares this checkout's prepared assets (a junction, like the dev setup's own).
    $link = Join-Path $d 'assets'
    if (-not (Test-Path $link)) {
        $assets = (Get-Item (Join-Path $Repo 'assets') -Force -ErrorAction SilentlyContinue).Target | Select-Object -First 1
        if (-not $assets) { $assets = Join-Path $Repo 'assets' }
        cmd /c mklink /J "$link" "$assets" | Out-Null
    }
    $dirty = [bool](Invoke-Git $d status --porcelain --untracked-files=no)
    if (-not $v.dev -and -not $dirty -and $v.branch) {
        # Follow the pushed branch (fast-forward only; local work is never thrown away).
        & git -C $d fetch -q $Remote $v.branch 2>$null
        $behind = (Invoke-Git $d rev-list --count "HEAD..$Remote/$($v.branch)")
        if ($behind -and [int]$behind -gt 0) { Write-Host "  Updating $($v.branch) ($behind new commits)..."; & git -C $d merge -q --ff-only "$Remote/$($v.branch)" 2>$null }
    }
    $exe = Join-Path $d 'bin\skate3rust.exe'
    $marker = Join-Path $d 'bin\built_commit.txt'
    $head = (Invoke-Git $d rev-parse --short HEAD)
    $built = ''; if (Test-Path $marker) { $built = (Get-Content $marker -Raw).Trim() }
    $need = -not (Test-Path $exe)
    if (-not $need -and -not $v.dev -and $built -ne $head) { $need = $true }
    if (-not $need) { return $true }
    Write-Host "  Building $($v.name) at $head. The first build of a version takes several minutes..." -ForegroundColor Cyan
    $target = Join-Path $d 'target'
    if ($v.target) { $target = Join-Path $Repo $v.target }
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $d 'scripts\Build.ps1') -TargetDirectory $target
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $exe)) { Write-Host '  Build failed.' -ForegroundColor Red; return $false }
    Set-Content -Path $marker -Value $head -Encoding ASCII
    return $true
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Pad {
    [StructLayout(LayoutKind.Sequential)]
    public struct State { public uint Packet; public ushort Buttons; public byte LT; public byte RT;
                          public short LX; public short LY; public short RX; public short RY; }
    [DllImport("xinput1_4.dll")] static extern int XInputGetState(int index, out State state);
    // Buttons of the first connected controller (0 = none / nothing pressed).
    public static ushort Buttons() {
        for (int i = 0; i < 4; i++) {
            State s;
            try { if (XInputGetState(i, out s) == 0) { ushort b = s.Buttons;
                    if (s.LY > 16000) b |= 0x0001; if (s.LY < -16000) b |= 0x0002; return b; } }
            catch { return 0; }
        }
        return 0;
    }
    [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
}
'@

function Focus-Console { $h = [Pad]::GetConsoleWindow(); [void][Pad]::ShowWindow($h, 3); [void][Pad]::SetForegroundWindow($h) }

# Waits for one input: 'up', 'down', 'ok', 'back'. Controller edges with key repeat on the D-pad.
function Read-Input {
    $prev = [Pad]::Buttons(); $held = 0
    while ($true) {
        if ([Console]::KeyAvailable) {
            $k = [Console]::ReadKey($true).Key
            switch ($k) {
                'UpArrow' { return 'up' } 'W' { return 'up' }
                'DownArrow' { return 'down' } 'S' { return 'down' }
                'Enter' { return 'ok' } 'Spacebar' { return 'ok' }
                'Escape' { return 'back' } 'Backspace' { return 'back' }
            }
        }
        $b = [Pad]::Buttons()
        $new = $b -band (-bnot $prev)
        if ($new -band 0x1000) { return 'ok' }      # A
        if ($new -band 0x2000) { return 'back' }    # B
        if ($new -band 0x0001) { return 'up' }
        if ($new -band 0x0002) { return 'down' }
        if (($b -band 0x0003) -and ($b -eq $prev)) { $held++; if ($held -gt 8) { $held = 6; if ($b -band 1) { return 'up' } else { return 'down' } } } else { $held = 0 }
        $prev = $b
        Start-Sleep -Milliseconds 50
    }
}

# A menu: returns the chosen item's index, or -1 for back.
function Show-Menu([string]$Title, [string[]]$Items, [string[]]$Hints) {
    $i = 0
    while ($true) {
        Clear-Host
        Write-Host ''
        Write-Host "  $Title" -ForegroundColor Cyan
        Write-Host ''
        for ($n = 0; $n -lt $Items.Count; $n++) {
            if ($n -eq $i) { Write-Host ("  > " + $Items[$n]) -ForegroundColor Black -BackgroundColor Yellow }
            else { Write-Host ("    " + $Items[$n]) }
        }
        Write-Host ''
        if ($Hints -and $Hints[$i]) { Write-Host ("  " + $Hints[$i]) -ForegroundColor DarkGray }
        Write-Host ''
        Write-Host '  D-pad: move    A: select    B: back' -ForegroundColor DarkGray
        switch (Read-Input) {
            'up' { $i = ($i - 1 + $Items.Count) % $Items.Count }
            'down' { $i = ($i + 1) % $Items.Count }
            'ok' { return $i }
            'back' { return -1 }
        }
    }
}

function Show-Message([string]$Text, [string]$Color = 'White') {
    Clear-Host; Write-Host ''; foreach ($l in ($Text -split "`n")) { Write-Host "  $l" -ForegroundColor $Color }
    Write-Host ''; Write-Host '  A / B: back to the menu' -ForegroundColor DarkGray
    Focus-Console; [void](Read-Input)
}

function Running-Games { @(Get-Process skate3, skate3rust -ErrorAction SilentlyContinue | ForEach-Object { $_.ProcessName + '.exe' } | Sort-Object -Unique) }

# Waits until no game runs (B cancels). True = clear to launch.
function Wait-NoGame {
    while ($true) {
        $g = Running-Games
        if ($g.Count -eq 0) { return $true }
        Clear-Host; Write-Host ''
        Write-Host ("  Already running: " + ($g -join ', ')) -ForegroundColor Yellow
        Write-Host '  Only one game at a time. Close it, or wait here: this continues by itself.'
        Write-Host ''; Write-Host '  B: back to the menu' -ForegroundColor DarkGray
        for ($t = 0; $t -lt 20; $t++) {
            if ([Console]::KeyAvailable) { $k = [Console]::ReadKey($true).Key; if ($k -eq 'Escape' -or $k -eq 'Backspace') { return $false } }
            if ([Pad]::Buttons() -band 0x2000) { return $false }
            Start-Sleep -Milliseconds 50
        }
    }
}

function Run-Bat([string]$Bat, [string]$Arg) {
    $cmd = '/c ""' + $Bat + '"'
    if ($Arg) { $cmd += ' ' + $Arg }
    $cmd += '"'
    Start-Process -FilePath cmd.exe -ArgumentList $cmd -Wait -NoNewWindow
}

function Resolve-Local([string]$Path) { if ([System.IO.Path]::IsPathRooted($Path)) { return $Path }; return (Join-Path $Repo $Path) }
function Recomp-Sessions { $p = '.local\recomp\sessions'; if ($Config.recomp.sessions) { $p = $Config.recomp.sessions }; return (Resolve-Local $p) }

# Starts the recomp with research-hooks tracing (environment variables of the research-hooks branch) and waits.
function Run-Recomp($RE, [string]$Lbl) {
    $r = $Config.recomp
    if (-not $r -or -not $r.exe -or -not (Test-Path $r.exe)) { throw 'Set recomp.exe in config.json (see README.md).' }
    $rargs = '--skate3_demo_path --skate3_demo_path_signed_in'
    if ($r.args) { $rargs = $r.args }
    $cwd = Split-Path $r.exe
    if ($r.working_dir) { $cwd = $r.working_dir }
    if (-not $RE.trace) { Start-Process -FilePath $r.exe -ArgumentList $rargs -WorkingDirectory $cwd -Wait; return }
    $prefix = $RE.id -replace '^recomp-', ''
    if ($RE.folder) { $prefix = $RE.folder }
    $name = $prefix + '_'; if ($Lbl) { $name += $Lbl + '_' }; $name += (Get-Date -Format 'yyyyMMdd_HHmmss')
    $out = Join-Path (Recomp-Sessions) $name
    New-Item -ItemType Directory -Path $out -Force | Out-Null
    $env:SKATE3_AUDIO_TRACE_FILE = Join-Path $out 'trace.tsv'
    $env:SKATE3_TRACE = $RE.trace
    $env:SKATE3_INPUT_RECORD = Join-Path $out 'pad.txt'
    if ($RE.capture) { $env:SKATE3_AUDIO_CAPTURE = Join-Path $out 'audio.f32' }
    Write-Host "  Session folder: $out"
    Start-Process -FilePath $r.exe -ArgumentList $rargs -WorkingDirectory $cwd -Wait `
        -RedirectStandardOutput (Join-Path $out 'game.out') -RedirectStandardError (Join-Path $out 'game.err')
}

# Checks the session written since $Since; returns a one-line summary.
function Check-Session([string]$Kind, [datetime]$Since, $Ver) {
    if ($Kind -eq 'rust' -and $Ver) {
        $f = Get-ChildItem (Join-Path (Version-Dir $Ver) 'logs') -Filter 'game-*.log' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $Since -and $_.Name -notlike '*.stderr.log' } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $f) { return 'No new game log was written.' }
        $err = Join-Path $f.DirectoryName ($f.BaseName + '.stderr.log')
        $panics = 0; if (Test-Path $err) { $panics = @(Select-String -Path $err -Pattern 'panicked' -SimpleMatch).Count }
        $frontend = @(Select-String -Path $f.FullName -Pattern 'AUDIO_NATIVE frontend' -SimpleMatch).Count
        $line = "$($f.Name): " + $(if ($panics -gt 0) { "$panics panic(s) - see $($f.BaseName).stderr.log" } else { 'no panics' })
        if ($frontend -gt 0) { $line += ", $frontend menu / marker sounds logged" }
        return $line
    }
    if ($Kind -eq 'statelog') {
        $f = Get-ChildItem $StateLogs -Filter 'state_*.tsv' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $Since } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $f) { return 'No new state log was written.' }
        Write-Host '  Checking the state log...'
        $lines = Get-Content $f.FullName
        $cols = ($lines[0] -split "`t").Count
        $bad = @($lines | Select-Object -Skip 1 | Where-Object { ($_ -split "`t").Count -ne $cols }).Count
        $mins = [math]::Round(($lines.Count - 1) / 3600.0, 1)
        $ok = 'OK'; if ($bad -ne 0) { $ok = 'MALFORMED - unusable' }
        return "$($f.Name): $($lines.Count - 1) rows (~$mins min), $bad malformed - $ok"
    }
    if ($Kind -eq 'recomp') {
        $d = Get-ChildItem (Recomp-Sessions) -Directory -ErrorAction SilentlyContinue | Where-Object { $_.CreationTime -ge $Since } | Sort-Object CreationTime -Descending | Select-Object -First 1
        if (-not $d) { return 'No new recomp session folder.' }
        $t = Join-Path $d.FullName 'trace.tsv'
        if (-not (Test-Path $t)) { return "$($d.Name): no trace.tsv" }
        $mb = [math]::Round((Get-Item $t).Length / 1MB)
        $check = $null; if ($Config.recomp.trace_check) { $check = Resolve-Local $Config.recomp.trace_check }
        if (-not $check -or -not (Test-Path $check)) { return "$($d.Name): $mb MB trace (no trace_check configured)" }
        Write-Host '  Checking the trace (can take a minute for long sessions)...'
        $out = & py -3.13 $check $t 2>&1 | Select-String 'malformed' | Select-Object -Last 1
        $counts = @([regex]::Matches("$out", ":\s*(\d+)") | ForEach-Object { [int]$_.Groups[1].Value })
        if ($counts.Count -eq 0) { return "$($d.Name): could not read the check result" }
        $bad = ($counts | Measure-Object -Sum).Sum
        $ok = 'OK'; if ($bad -ne 0) { $ok = 'MALFORMED - unusable' }
        return "$($d.Name): $mb MB trace, $bad malformed lines - $ok"
    }
    return ''
}

# Entries: the engine modes, then the recomp modes from config.json (none without a config).
$Entries = [ordered]@{
    'rust-play'     = @{ Name = 'Rust engine: play';                        Kind = 'rust';     Hint = "The version's PLAY.bat." }
    'rust-statelog' = @{ Name = 'Rust engine: play + audio state log';      Kind = 'statelog'; Hint = 'Records the per-frame audio state (.local\audio-state-logs; builds with the native audio port).' }
    'rust-trace'    = @{ Name = 'Rust engine: play + audio trace';          Kind = 'rust';     Hint = 'Logs every native sound start in logs\game-*.log (SKATE_AUDIO_TRACE=1).' }
    'rust-devmods'  = @{ Name = 'Rust engine: play with the dev test mods'; Kind = 'rust';     Hint = "Turns on the version's dev test mod (mods_enable in versions.json)." }
    'rust-perf'     = @{ Name = 'Rust engine: play + performance trace';    Kind = 'rust';     Hint = 'Bevy performance trace (PLAY.bat -trace).' }
}
if ($Config -and $Config.recomp_entries) {
    foreach ($re in @($Config.recomp_entries | ForEach-Object { $_ })) {
        $Entries[$re.id] = @{ Name = $re.name; Kind = $(if ($re.trace -or $re.bat) { 'recomp' } else { 'none' }); Hint = "$($re.hint)"; Recomp = $re }
    }
}
$Labels = @('session', 'ride', 'grind', 'bail', 'push', 'emit', 'carve', 'water', 'marker')

function Launch([string]$Key, [string]$Lbl) {
    $e = $Entries[$Key]
    if (-not $e) { if ($NoPrompt) { Set-Content -Path $LastSession -Value "Unknown entry: $Key" -Encoding UTF8 } else { Show-Message "Unknown entry: $Key" 'Red' }; return }
    if ($NoPrompt) { if ((Running-Games).Count -gt 0) { Set-Content -Path $LastSession -Value 'Not started: another game is running.' -Encoding UTF8; return } }
    elseif (-not (Wait-NoGame)) { return }
    $ver = $null; $play = $null; $verText = ''
    if ($Key.StartsWith('rust-')) {
        $ver = Get-Version $Version
        $verText = " [$($ver.id)]"
        if ($Key -eq 'rust-devmods' -and -not $ver.mods_enable) {
            Set-Content -Path $LastSession -Value "Not started: $($ver.name) has no dev test mod. Use 'play' instead." -Encoding UTF8
            if (-not $NoPrompt) { Show-Message "$($ver.name) has no dev test mod." 'Yellow' }
            return
        }
        Clear-Host; Write-Host ''; Write-Host ("  Version: " + $ver.name) -ForegroundColor Cyan
        if (-not (Ensure-Version $ver)) {
            Set-Content -Path $LastSession -Value "Not started: $($ver.name) could not be set up or built (see the window above)." -Encoding UTF8
            if (-not $NoPrompt) { Show-Message "Could not build $($ver.name)." 'Red' } else { Start-Sleep -Seconds 15 }
            return
        }
        $play = Join-Path (Version-Dir $ver) 'PLAY.bat'
    }
    $since = Get-Date
    Set-Content -Path $Playing -Value ("entry=$Key label=$Lbl version=$($ver.id) started=" + $since.ToString('yyyy-MM-dd HH:mm:ss')) -Encoding ASCII
    Clear-Host; Write-Host ''; Write-Host ("  Starting: " + $e.Name + $verText) -ForegroundColor Cyan
    try {
        switch ($Key) {
            'rust-play'     { Run-Bat $play '' }
            'rust-statelog' {
                New-Item -ItemType Directory -Path $StateLogs -Force | Out-Null
                $env:SKATE_AUDIO_STATE_LOG = Join-Path $StateLogs ('state_' + (Get-Date -Format 'yyyyMMdd_HHmmss') + '.tsv')
                $env:SKATE_AEMS = '1'; $env:SKATE_AUDIO_TIMING = '1'
                Run-Bat $play ''
            }
            'rust-trace'    { $env:SKATE_AUDIO_TRACE = '1'; Run-Bat $play '' }
            'rust-devmods'  { $env:SKATE3_MODS_ENABLE = $ver.mods_enable; Run-Bat $play '' }
            'rust-perf'     { Run-Bat $play '-trace' }
            default {
                $re = $e.Recomp
                if ($re.label) { $Lbl = $re.label }
                if ($re.bat) { Run-Bat (Resolve-Local $re.bat) $(if ($Lbl -and $Lbl -ne 'session') { $Lbl } else { "$($re.bat_args)" }) }
                else { Run-Recomp $re $(if ($Lbl -eq 'session') { '' } else { $Lbl }) }
            }
        }
    } finally {
        Remove-Item $Playing -ErrorAction SilentlyContinue
    }
    if (-not $NoPrompt) { Focus-Console }
    Clear-Host; Write-Host ''; Write-Host '  Session ended.' -ForegroundColor Cyan
    $summary = Check-Session $e.Kind $since $ver
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    Set-Content -Path $LastSession -Value ("$stamp  $Key $Lbl$verText`r`n$summary") -Encoding UTF8
    if ($summary -and -not $NoPrompt) { $col = 'Green'; if ($summary -match 'MALFORMED|No new|panic\(s\)') { $col = 'Yellow' }; Show-Message ("Session ended.`n`n" + $summary) $col }
}

# Steam's "Exit game" closes the launcher together with the game, so the after-session check can be skipped.
# 'pending' checks the newest session (state log or recomp) if it's newer than last_session.txt and writes the result;
# SkateLauncher.exe runs it at every start. Exit code 10 = a result was written, 0 = nothing pending.
if ($Entry -eq 'pending') {
    $since = [datetime]::MinValue
    if (Test-Path $LastSession) { $since = (Get-Item $LastSession).LastWriteTime }
    $log = Get-ChildItem $StateLogs -Filter 'state_*.tsv' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $rec = $null; if ($Config) { $rec = Get-ChildItem (Recomp-Sessions) -Directory -ErrorAction SilentlyContinue | Sort-Object CreationTime -Descending | Select-Object -First 1 }
    $kind = ''; $t0 = $since
    if ($log -and $log.LastWriteTime -gt $since) { $kind = 'statelog'; $t0 = $log.LastWriteTime.AddSeconds(-1) }
    if ($rec -and $rec.CreationTime -gt $since -and (-not $log -or $rec.CreationTime -gt $log.LastWriteTime)) { $kind = 'recomp'; $t0 = $rec.CreationTime.AddSeconds(-1) }
    # A PLAYING.txt left by a launcher that was closed with its game is stale once no game runs.
    if ((Test-Path $Playing) -and (Running-Games).Count -eq 0) { Remove-Item $Playing -ErrorAction SilentlyContinue }
    if ($kind -eq '' -or (Test-Path $Playing)) { exit 0 }
    Write-Host '  Checking the last session (it closed before its check)...'
    $summary = Check-Session $kind $t0 $null
    Set-Content -Path $LastSession -Value ((Get-Date).ToString('yyyy-MM-dd HH:mm:ss') + "  (checked at next start)`r`n$summary") -Encoding UTF8
    exit 10
}

# Version commands (SkateLauncher.exe reads their output; tab-separated where it parses it).
if ($Entry -eq 'entries') {
    # id, name, hint, ask (1 = ask for a session-folder label)
    foreach ($k in $Entries.Keys) { $ask = 0; if ($Entries[$k].Recomp -and $Entries[$k].Recomp.ask_label) { $ask = 1 }; "{0}`t{1}`t{2}`t{3}" -f $k, $Entries[$k].Name, $Entries[$k].Hint, $ask }
    exit 0
}
if ($Entry -eq 'versions') {
    $sel = (Get-Version '').id
    foreach ($v in $Versions) { "{0}`t{1}`t{2}`t{3}" -f $v.id, $v.name, (Version-Status $v), $(if ($v.id -eq $sel) { '*' } else { '' }) }
    exit 0
}
if ($Entry -in @('version-next', 'version-prev', 'version-set')) {
    $cur = [array]::IndexOf(@($Versions | ForEach-Object { $_.id }), (Get-Version '').id)
    if ($Entry -eq 'version-set') { $new = Get-Version $Label }
    else { $step = 1; if ($Entry -eq 'version-prev') { $step = -1 }; $new = $Versions[($cur + $step + $Versions.Count) % $Versions.Count] }
    Set-Content -Path $SelectedFile -Value $new.id -Encoding ASCII
    $Entry = 'notes'; $Label = $new.id
}
if ($Entry -eq 'notes') {
    $v = Get-Version $(if ($Label) { $Label } else { $Version })
    "Version: $($v.name)"
    "Branch: $($v.branch)   Status: $(Version-Status $v)"
    ''
    'What to test:'
    $v.notes
    exit 0
}
if ($Entry -eq 'build') {
    $v = Get-Version $(if ($Label) { $Label } else { $Version })
    if (Ensure-Version $v) { "Ready: $($v.name) ($(Version-Status $v))"; exit 0 } else { "Build failed: $($v.name)"; exit 1 }
}
if ($Entry -eq 'list') { $Entries.Keys | ForEach-Object { '{0,-15} {1}' -f $_, $Entries[$_].Name }; exit 0 }
if ($Entry) { Launch $Entry $Label; exit 0 }

# Keyboard menu (launcher.bat). SkateLauncher.exe draws its own menu with the same entries.
try { $Host.UI.RawUI.WindowTitle = 'Skate launcher' } catch {}
Focus-Console
while ($true) {
    $cv = Get-Version ''
    $top = Show-Menu 'Skate launcher' @("Rust engine: $($cv.name)", 'Choose version', 'What to test (this version)', 'Recomp (Skate 3 retail)', 'Last session result', 'Exit') @('Our engine, the selected version.', '', '', 'The recompiled retail game, with or without tracing (config.json).', '', '')
    if ($top -eq 5 -or $top -eq -1) { break }
    if ($top -eq 4) { $t = 'No session yet.'; if (Test-Path $LastSession) { $t = (Get-Content $LastSession) -join "`n" }; Show-Message $t; continue }
    if ($top -eq 1) {
        $pick = Show-Menu 'Choose version' @($Versions | ForEach-Object { $_.name }) @($Versions | ForEach-Object { Version-Status $_ })
        if ($pick -ge 0) { Set-Content -Path $SelectedFile -Value $Versions[$pick].id -Encoding ASCII }
        continue
    }
    if ($top -eq 2) { Show-Message ("$($cv.name)`n`n" + $cv.notes); continue }
    $prefix = 'rust-'; if ($top -eq 3) { $prefix = 'recomp-' }
    $keys = @($Entries.Keys | Where-Object { $_.StartsWith($prefix) })
    if ($keys.Count -eq 0) { Show-Message 'No recomp entries: add them to config.json (see README.md).' 'Yellow'; continue }
    while ($true) {
        $pick = Show-Menu (@('Rust engine', 'Recomp')[[int]($top -eq 3)]) ($keys | ForEach-Object { $Entries[$_].Name }) ($keys | ForEach-Object { $Entries[$_].Hint })
        if ($pick -eq -1) { break }
        $key = $keys[$pick]; $lbl = ''
        if ($Entries[$key].Recomp -and $Entries[$key].Recomp.ask_label) {
            $l = Show-Menu 'Label for the session folder' $Labels @('No label.')
            if ($l -eq -1) { continue }
            $lbl = $Labels[$l]
        }
        Launch $key $lbl
    }
}
