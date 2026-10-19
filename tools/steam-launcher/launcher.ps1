# Couch / Steam launcher: start any engine version (hails-additions, upstream main, your branches, open PRs), the
# recomp with a tracing option or retail Skate 3 in RPCS3, from the couch with a controller. SkateLauncher.exe (the
# window Steam starts) draws the menu and runs this script for the work; it reads the output below.
# Windows PowerShell 5.1 compatible. See README.md.
#   launcher.ps1 <entry> [label] [-Version <id>] [-NoPrompt]   run an entry (rust-*, recomp-*, rpcs3-*, tool-*)
#   launcher.ps1 entries                                      the entries: id, name, hint, ask, group, on (1/0), why off
#   launcher.ps1 versions                                     the versions: id, name, status, selected
#   launcher.ps1 version-next | version-prev | version-set <id>   select a version (prints its notes)
#   launcher.ps1 notes [id]                                   what to test in a version
#   launcher.ps1 build <id>                                   set up / update / build a version without playing
#   launcher.ps1 pending                                      check a session that closed before its check
#   launcher.ps1 status                                       one line per fact for the window's header (key, value)
#   launcher.ps1 sizes                                        per version: id, name, build cache bytes, bin bytes
#   launcher.ps1 clear-cache <id|all> | clear-build <id|all>  delete build caches / builds (cache + bin\)
#   launcher.ps1 room <id>                                    apply the build size cap as before a build of <id>
#   launcher.ps1 setup-check                                  what the first-time setup finds (see setup.ps1)
# While a game runs, PLAYING.txt exists in the state folder: tools and agents should launch nothing while it does.
param([string]$Entry, [string]$Label, [switch]$NoPrompt, [string]$Version)

$ErrorActionPreference = 'Stop'
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = (Resolve-Path (Join-Path $Here '..\..')).Path
# Standalone (the launcher-only download, STANDALONE.txt next to this script): no checkout around it. Config and state
# live here, and each version is its own single-branch clone in versions\<id>, made the first time it is played or
# built, so a version you never use is never downloaded or built (user 2026-10-19).
$Standalone = Test-Path (Join-Path $Here 'STANDALONE.txt')
if ($Standalone) { $Repo = $Here }

# Config and state: next to the script when it lives outside tools\ (a private copy), otherwise in
# .local\steam-launcher\ (gitignored), so the repository stays clean.
$StateDir = $Here
if ($Here -like (Join-Path $Repo 'tools') + '\*') { $StateDir = Join-Path $Repo '.local\steam-launcher' }
New-Item -ItemType Directory -Path $StateDir -Force | Out-Null
$Playing = Join-Path $StateDir 'PLAYING.txt'
$LastSession = Join-Path $StateDir 'last_session.txt'
$SelectedFile = Join-Path $StateDir 'version.txt'
$script:BuildLog = ''
$StateLogs = Join-Path $Repo '.local\audio-state-logs'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

function Read-Json([string]$Path) {
    # PowerShell 5.1 returns a JSON array as one object: callers unroll arrays with ForEach-Object.
    return (Get-Content $Path -Raw | ConvertFrom-Json)
}
$Config = $null
$ConfigFile = Join-Path $StateDir 'config.json'
if (Test-Path $ConfigFile) { $Config = Read-Json $ConfigFile }
function Setting([string]$Name, $Default) { if ($Config -and $null -ne $Config.$Name -and "$($Config.$Name)" -ne '') { return $Config.$Name }; return $Default }
$Remote = Setting 'remote' 'origin'

# Always there: the fork's hails-additions and upstream main, each a launcher-only detached worktree that follows its
# pushed branch and rebuilds when it moved. The URLs can be changed in config.json (fork_url, upstream_url); a
# versions.json entry with the same id replaces the built-in one.
$Builtin = @(
    [pscustomobject]@{ id = 'hails-additions'; name = 'hails-additions (newest on the fork)'; branch = 'hails-additions'
                       url = (Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git'); dir = $(if ($Standalone) { 'versions/hails-additions' } else { '.local/versions/hails-additions' }); detached = $true
                       notes = 'Always the newest pushed hails-additions: every launch fetches it and rebuilds when it moved. What changed: docs\hails-additions\RELEASE-NOTES.md.' },
    [pscustomobject]@{ id = 'upstream-main'; name = 'Upstream main (for comparing)'; branch = 'main'
                       url = (Setting 'upstream_url' 'https://github.com/SK8-ENGINE/skate-3-rust-engine.git'); dir = $(if ($Standalone) { 'versions/upstream-main' } else { '.local/versions/upstream-main' }); detached = $true
                       notes = 'Upstream main as it is (SK8-ENGINE), rebuilt when it moved. Use it to compare against hails-additions.' }
)
# config.json "builtin_versions": ["hails-additions"] keeps only the built-ins you use (default: both).
$keepBuiltin = @(Setting 'builtin_versions' @('hails-additions', 'upstream-main') | ForEach-Object { $_ })
$Builtin = @($Builtin | Where-Object { $keepBuiltin -contains $_.id })
$Versions = @()
$VersionsFile = Join-Path $StateDir 'versions.json'
if (Test-Path $VersionsFile) { $Versions = @((Read-Json $VersionsFile) | ForEach-Object { $_ }) }
elseif (-not $Standalone) {
    $Versions = @([pscustomobject]@{ id = 'this'; name = 'This checkout'; branch = ''; dir = '.'; dev = $true
                                     notes = 'This checkout as it is. Add a versions.json to compare more branches (see README.md).' })
}
$own = @($Versions | ForEach-Object { $_.id })
$Versions = @($Builtin | Where-Object { $own -notcontains $_.id }) + $Versions

function Get-Version([string]$Id) {
    if (-not $Id) {
        $Id = $Versions[0].id
        if (Test-Path $SelectedFile) { $s = (Get-Content $SelectedFile -Raw).Trim(); if ($s) { $Id = $s } }
    }
    $v = $Versions | Where-Object { $_.id -eq $Id } | Select-Object -First 1
    # A selected version that left versions.json falls back to the first one.
    if (-not $v -and -not $PSBoundParameters.ContainsKey('Id')) { $v = $Versions[0] }
    if (-not $v) { $v = $Versions[0] }
    return $v
}
function Version-Dir($v) { if ($v.dir -eq '.') { return $Repo }; return (Join-Path $Repo $v.dir) }
function Invoke-Git([string]$Dir) { $ErrorActionPreference = 'Continue'; & git.exe -C $Dir @args 2>$null }
# Where a version's branch is fetched from and the ref it follows: its own URL (the built-ins) or the remote.
function Version-Source($v) {
    if ($v.url) { return @{ Fetch = $v.url; Ref = "refs/remotes/launcher-$($v.id)/$($v.branch)" } }
    return @{ Fetch = $Remote; Ref = "refs/remotes/$Remote/$($v.branch)" }
}
function Fetch-Version($v, [string]$Dir) {
    $s = Version-Source $v
    # Full refspec: a release tag with the branch's name (the fork's rolling build) would win otherwise.
    & git -C $Dir fetch -q $s.Fetch "+refs/heads/$($v.branch):$($s.Ref)" 2>$null
    return $s.Ref
}

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

# The published installation's assets (setup's marker), else the checkout's own assets link.
function Current-Assets {
    # Standalone: the first version that has converted game files shares them with the others (one conversion).
    $bases = @(Join-Path $Repo 'data')
    if ($Standalone) { $bases += @($Versions | ForEach-Object { Join-Path (Version-Dir $_) 'data' }) }
    foreach ($b in $bases) {
        try {
            $marker = Get-Content -Raw (Join-Path $b 'installation.json') | ConvertFrom-Json
            $a = Join-Path $b (($marker.directory -replace '/', '\') + '\assets')
            if (Test-Path $a) { return $a }
        } catch {}
    }
    try {
        $marker = Get-Content -Raw (Join-Path $Repo 'data\installation.json') | ConvertFrom-Json
        $a = Join-Path (Join-Path $Repo 'data') (($marker.directory -replace '/', '\') + '\assets')
        if (Test-Path $a) { return $a }
    } catch {}
    $a = (Get-Item (Join-Path $Repo 'assets') -Force -ErrorAction SilentlyContinue).Target | Select-Object -First 1
    if ($a) { return $a }
    return (Join-Path $Repo 'assets')
}

# Each version builds into its own target folder (cargo would mix the branches' crates in a shared one): the version's
# "target", else <target_root>\<id> from config.json, else the worktree's own target\.
function Target-Dir($v) {
    if ($v.target) { return (Resolve-Local $v.target) }
    $root = Setting 'target_root' ''
    if ($root) { return (Join-Path $root $v.id) }
    return (Join-Path (Version-Dir $v) 'target')
}
function Folder-Bytes([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return [long]0 }
    $sum = [long]0
    try { foreach ($f in [System.IO.Directory]::EnumerateFiles($Path, '*', 'AllDirectories')) { try { $sum += ([System.IO.FileInfo]$f).Length } catch {} } } catch {}
    return $sum
}
# Size cap (config.json "build_cap_gb", e.g. 200): the build folders together, or everything under "build_cap_dir" when
# set (a drive folder other builds share too), stay under the cap. Before a build, while the total plus the room a
# build needs ("build_reserve_gb", default 30) is over it, the launcher deletes the build cache of the version that
# was built longest ago, never the one it is about to build. Only the launcher's own version target folders are
# deleted; a version's bin\ keeps its exe, so it still plays and only its next build starts from scratch.
function Make-Room($v) {
    $cap = [double](Setting 'build_cap_gb' 0)
    if ($cap -le 0) { return }
    $reserve = [double](Setting 'build_reserve_gb' 30)
    $capDir = Setting 'build_cap_dir' ''
    $others = @($Versions | Where-Object { $_.id -ne $v.id } | ForEach-Object { [pscustomobject]@{ V = $_; Dir = (Target-Dir $_) } } |
        Where-Object { (Test-Path -LiteralPath $_.Dir) -and $_.Dir -ne (Target-Dir $v) })
    $total = { if ($capDir) { Folder-Bytes $capDir } else { (@($Versions | ForEach-Object { Target-Dir $_ } | Sort-Object -Unique | ForEach-Object { Folder-Bytes $_ }) | Measure-Object -Sum).Sum } }
    $used = [double](& $total) / 1GB
    Write-Host ("  Build folders: {0:N1} GB of {1:N0} GB (a build needs about {2:N0} GB)." -f $used, $cap, $reserve)
    # Oldest build first: the time of its exe in bin\ (never built = oldest).
    $queue = @($others | Sort-Object { $e = Join-Path (Version-Dir $_.V) 'bin\skate3rust.exe'; if (Test-Path $e) { (Get-Item $e).LastWriteTime } else { [datetime]::MinValue } })
    foreach ($o in $queue) {
        if ($used + $reserve -le $cap) { break }
        $gb = (Folder-Bytes $o.Dir) / 1GB
        Write-Host ("  Over the cap: deleting the build cache of {0} ({1:N1} GB, {2})..." -f $o.V.name, $gb, $o.Dir)
        Remove-Item -LiteralPath $o.Dir -Recurse -Force -ErrorAction SilentlyContinue
        $used = [double](& $total) / 1GB
    }
    if ($used + $reserve -gt $cap) { Write-Host ("  Still {0:N1} GB of {1:N0} GB: nothing more of the launcher's to delete; building anyway." -f $used, $cap) }
}

# Points a checkout's assets junction at the current installation when it no longer resolves: Refresh Game Files
# publishes a new installation and deletes the old one, re-pointing only the link of the copy that ran it, so the
# others failed with "Asset root assets: The system cannot find the path specified" (laptop 2026-10-18). A real
# folder is never touched. Returns a line when it repaired one.
function Repair-AssetLink([string]$Dir, [string]$Name) {
    $link = Join-Path $Dir 'assets'
    $item = Get-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue
    if ($item -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return '' }
    if ($item -and (Test-Path (Join-Path $link 'private\game.json'))) { return '' }
    $target = Current-Assets
    if (-not (Test-Path (Join-Path $target 'private\game.json'))) { return '' }
    if ($item) { cmd /c rmdir "$link" | Out-Null }
    elseif ($Dir -eq $Repo) { return '' }
    cmd /c mklink /J "$link" "$target" | Out-Null
    return "Assets link of $Name pointed at the current installation."
}

# Every checkout the launcher knows: this one and each version's worktree that exists.
function Repair-AllAssetLinks {
    $lines = @(Repair-AssetLink $Repo 'this checkout')
    foreach ($v in $Versions) {
        $d = Version-Dir $v
        if ($d -ne $Repo -and (Test-Path (Join-Path $d '.git'))) { $lines += Repair-AssetLink $d $v.name }
    }
    return @($lines | Where-Object { $_ })
}

# Makes sure the version's worktree exists, follows its pushed branch and is built. True = ready.
function Ensure-Version($v) {
    $ErrorActionPreference = 'Continue'
    $d = Version-Dir $v
    if (-not (Test-Path (Join-Path $d '.git'))) {
        New-Item -ItemType Directory -Path (Split-Path $d) -Force | Out-Null
        if ($Standalone) {
            # Only this branch, from its own URL (or the fork's); updates fetch it again below.
            $url = if ($v.url) { $v.url } else { Setting 'fork_url' 'https://github.com/Hailey-Ross/rusty-trucks.git' }
            Write-Host "  Setting up $($v.name): downloading $($v.branch) from $url..."
            & git clone -q --single-branch --branch $v.branch $url $d
            if ($LASTEXITCODE -ne 0) { Write-Host '  Could not download it (git clone failed).'; return $false }
        }
    }
    if (-not (Test-Path (Join-Path $d '.git'))) {
        Write-Host "  Setting up $($v.name): new worktree for $($v.branch)..."
        $ref = Fetch-Version $v $Repo
        # A launcher-only copy ("detached": true) follows the pushed branch without checking it out, so it never
        # conflicts with a worktree where that branch is being worked on.
        if ($v.detached -or $v.url) { & git -C $Repo worktree add --detach $d $ref }
        else { & git -C $Repo worktree add $d $v.branch }
        if ($LASTEXITCODE -ne 0) { Write-Host '  Could not create the worktree.'; return $false }
    }
    # Every version shares this checkout's prepared assets (a junction, like the dev setup's own). A setup refresh
    # replaces the installation: a link to the old one no longer resolves, so it is pointed at the current one.
    $fixed = Repair-AssetLink $d $v.name
    if ($fixed) { Write-Host "  $fixed" }
    $dirty = [bool](Invoke-Git $d status --porcelain --untracked-files=no)
    if (-not $v.dev -and $dirty -and $v.branch) {
        # Local edits block the fast-forward: say so instead of silently starting an old build.
        Write-Host "  NOT UPDATED: $($v.name) has local changes, so it stays at its current commit."
    }
    if (-not $v.dev -and -not $dirty -and $v.branch) {
        # Follow the pushed branch (fast-forward only; local work is never thrown away).
        $ref = Fetch-Version $v $d
        $behind = (Invoke-Git $d rev-list --count "HEAD..$ref")
        if ($behind -and [int]$behind -gt 0) { Write-Host "  Updating $($v.branch) ($behind new commits)..."; & git -C $d merge -q --ff-only $ref 2>$null }
    }
    $exe = Join-Path $d 'bin\skate3rust.exe'
    $marker = Join-Path $d 'bin\built_commit.txt'
    $head = (Invoke-Git $d rev-parse --short HEAD)
    if ($v.dev) {
        # A working-tree build: the commit plus a hash of the uncommitted changes, so editing the tree rebuilds it.
        $diff = (Invoke-Git $d diff HEAD) -join "`n"
        if ($diff) {
            $sha = [System.Security.Cryptography.SHA1]::Create()
            $hash = -join ($sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($diff)) | Select-Object -First 6 | ForEach-Object { $_.ToString('x2') })
            $head = "$head+$hash"
        }
    }
    $built = ''; if (Test-Path $marker) { $built = (Get-Content $marker -Raw).Trim() }
    if ((Test-Path $exe) -and $built -eq $head) { return $true }
    Write-Host "  Building $($v.name) at $head. The first build of a version takes several minutes..."
    $target = Target-Dir $v
    Make-Room $v
    # The full build output goes to <version>\logs\launcher-build-<id>-<time>.log (written by SkateLauncher.exe, which
    # draws the progress from it); the newest 10 per version are kept.
    $logDir = Join-Path $d 'logs'
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    $script:BuildLog = Join-Path $logDir ('launcher-build-' + $v.id + '-' + (Get-Date -Format 'yyyyMMdd_HHmmss') + '.log')
    Get-ChildItem $logDir -Filter ('launcher-build-' + $v.id + '-*.log') -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -Skip 9 | Remove-Item -ErrorAction SilentlyContinue
    # The window (SKATE_LAUNCHER_UI=2) gets plain text plus cargo's own "Building [..] n/total" bar; a terminal run keeps
    # cargo's normal output.
    $ui = $env:SKATE_LAUNCHER_UI -eq '2'
    $keep = @{}
    if ($ui) {
        $cargoEnv = @{ CARGO_TERM_COLOR = 'never'; CARGO_TERM_PROGRESS_WHEN = 'always'; CARGO_TERM_PROGRESS_WIDTH = '120'; CARGO_TERM_PROGRESS_TERM_INTEGRATION = 'false' }
        foreach ($k in $cargoEnv.Keys) { $keep[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $cargoEnv[$k]) }
        Write-Host ("LAUNCHER_BUILD`tbegin`t{0}`t{1}`t{2}" -f $v.name, $head, $script:BuildLog)
    }
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $d 'scripts\Build.ps1') -TargetDirectory $target
    $code = $LASTEXITCODE
    if ($ui) {
        foreach ($k in $keep.Keys) { [Environment]::SetEnvironmentVariable($k, $keep[$k]) }
        Write-Host ("LAUNCHER_BUILD`tend`t{0}" -f $code)
    }
    if ($code -ne 0 -or -not (Test-Path $exe)) { Write-Host "  Build failed. Build log: $script:BuildLog"; return $false }
    Set-Content -Path $marker -Value $head -Encoding ASCII
    return $true
}

$GameNames = @('skate3', 'skate3rust', 'rpcs3')
function Running-Games { @(Get-Process $GameNames -ErrorAction SilentlyContinue | ForEach-Object { $_.ProcessName + '.exe' } | Sort-Object -Unique) }

# A game can outlive the script that started it: Refresh Game Files restarts the game into setup and back (the
# laptop's session check and send ran while setup was still converting, 2026-10-18). Wait until none runs.
function Wait-GameTree {
    $said = $false
    while ((Running-Games).Count -gt 0) {
        if (-not $said) { Write-Host '  The game is still running (it restarted itself, for example to refresh its files)...'; $said = $true }
        Start-Sleep -Seconds 2
    }
}

function Run-Bat([string]$Bat, [string]$Arg) {
    $cmd = '/c ""' + $Bat + '"'
    if ($Arg) { $cmd += ' ' + $Arg }
    $cmd += '"'
    $run = Start-Process -FilePath cmd.exe -ArgumentList $cmd -NoNewWindow -PassThru
    $run.WaitForExit()
    Wait-GameTree
}

# Where engine sessions write logs\: this checkout, or in standalone the selected version's clone.
function Logs-Root { if ($Standalone) { return (Join-Path (Version-Dir (Get-Version $Version)) 'logs') }; return (Join-Path $Repo 'logs') }
function Resolve-Local([string]$Path) { if ([System.IO.Path]::IsPathRooted($Path)) { return $Path }; return (Join-Path $Repo $Path) }
function Recomp-Sessions { $p = '.local\recomp\sessions'; if ($Config.recomp.sessions) { $p = $Config.recomp.sessions }; return (Resolve-Local $p) }

# Starts the recomp with research-hooks tracing (environment variables of the research-hooks branch) and waits.
function Run-Recomp($RE, [string]$Lbl) {
    $r = $Config.recomp
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

# Starts RPCS3 straight into the game (config.json "rpcs3": exe, game = disc folder or EBOOT.BIN, args) and waits.
function Run-Rpcs3($PE) {
    $r = $Config.rpcs3
    $a = '--no-gui --fullscreen'
    if ($r.args) { $a = $r.args }
    if ($PE.args) { $a = $PE.args }
    Start-Process -FilePath $r.exe -ArgumentList ($a + ' "' + $r.game + '"') -WorkingDirectory (Split-Path $r.exe) -Wait
}

# Checks the session written since $Since; returns a one-line summary.
function Check-Session([string]$Kind, [datetime]$Since, $Ver) {
    if ($Kind -eq 'rust') {
        $root = if ($Ver) { Version-Dir $Ver } else { $Repo }
        $f = Get-ChildItem (Join-Path $root 'logs') -Filter 'game-*.log' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $Since -and $_.Name -notlike '*.stderr.log' } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
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
    if ($Kind -eq 'traceall') {
        $root = if ($Ver) { Version-Dir $Ver } else { $Repo }
        $d = Get-ChildItem (Join-Path $root 'logs') -Directory -Filter 'trace-all-*' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $Since } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $d) { return 'No new trace-all folder was written.' }
        Write-Host '  Checking the trace-all tables...'
        $parts = @()
        foreach ($name in 'audio_state.tsv', 'frames.tsv') {
            $t = Join-Path $d.FullName $name
            if (-not (Test-Path $t)) { continue }
            $cols = 0; $rows = 0; $bad = 0
            foreach ($l in [IO.File]::ReadLines($t)) {
                # frames.tsv starts with a '# skate3rust frame log v1' comment line before its header.
                if ($l.StartsWith('#')) { continue }
                $n = ($l -split "`t").Count
                if ($cols -eq 0) { $cols = $n; continue }
                $rows++; if ($n -ne $cols) { $bad++ }
            }
            $parts += "$name $rows rows, $bad malformed" + $(if ($bad -ne 0) { ' - MALFORMED - unusable' } else { '' })
        }
        $mb = [math]::Round((Get-ChildItem $d.FullName -Recurse -File | Measure-Object Length -Sum).Sum / 1MB)
        if ($parts.Count -eq 0) { return "$($d.Name): $mb MB, no tables" }
        return "$($d.Name) ($mb MB): " + ($parts -join '; ')
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

# Optional: config.json "after_session" names a send script (path relative to the repository, e.g. the second PC's
# play-and-send.ps1), run with -SendOnly after each session.
function Send-Script { if (-not $Config -or -not $Config.after_session) { return $null }; return (Resolve-Local $Config.after_session) }
# Why the send can't run ('' = it can).
function Send-Blocked {
    $script = Send-Script
    if (-not $script) { return 'Not set up: name a send script as "after_session" in config.json (see README.md).' }
    if (-not (Test-Path $script)) { return "Send script not found: $script" }
    # Without its settings the script would ask its setup questions, which nobody answers from Steam.
    # Its settings sit next to it (release ZIP copy) or at the checkout root (tools\second-pc\play-and-send.ps1).
    if (-not ((Test-Path (Join-Path (Split-Path $script) 'send-sessions.txt')) -or (Test-Path (Join-Path $Repo 'send-sessions.txt')))) { return "Not set up: run $(Split-Path $script -Leaf) -Setup once." }
    return ''
}
# Returns one line for the result screen ('' = no send configured).
function Send-Sessions {
    if (-not (Send-Script)) { return '' }
    $blocked = Send-Blocked
    if ($blocked) { return "Send: $blocked" }
    Write-Host '  Sending the logs...'
    try { $out = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Send-Script) -SendOnly | ForEach-Object { "$_" } | Where-Object { $_ }) }
    catch { return "Send: failed ($($_.Exception.Message))." }
    if ($out.Count -eq 0) { return 'Send: no output.' }
    $sent = @($out | Where-Object { $_ -like 'Sent *' -and $_ -notlike 'Sent * file(s) to GitHub*' }).Count
    if ($sent -gt 0) { return "Send: $sent file(s). $($out[-1])" }
    return "Send: $($out[-1])"
}

# The second PC's recomp tools (tools\second-pc\play-recomp.ps1 -Update / -GetSave): config.json "recomp_script", else
# that script once the second PC is set up (send-sessions.txt at the checkout root).
function Recomp-Script {
    $s = Setting 'recomp_script' ''
    if ($s) { return (Resolve-Local $s) }
    $d = Join-Path $Repo 'tools\second-pc\play-recomp.ps1'
    if ((Test-Path $d) -and (Test-Path (Join-Path $Repo 'send-sessions.txt'))) { return $d }
    return $null
}

# Entries. Group: rust (the selected version), recomp, retail (RPCS3), tools. Every entry is always listed; one that
# isn't set up is greyed out in the window with the reason (Why), so the menu shows what could be there.
$Entries = [ordered]@{
    'rust-play'     = @{ Group = 'rust'; Name = 'Play';                       Kind = 'rust';     Hint = "The version's PLAY.bat." }
    'rust-traceall' = @{ Group = 'rust'; Name = 'Play + trace all';           Kind = 'traceall'; Hint = "Every session trace the version has (SKATE_TRACE_ALL: frame log, audio state log, performance report, GPU / render timing, audio trace, living-world readout), in the version's logs\trace-all-<time> folder." }
    'rust-statelog' = @{ Group = 'rust'; Name = 'Play + audio state log';     Kind = 'statelog'; Hint = 'Records the per-frame audio state (.local\audio-state-logs).' }
    'rust-trace'    = @{ Group = 'rust'; Name = 'Play + audio trace';         Kind = 'rust';     Hint = 'Logs every native sound start in logs\game-*.log (SKATE_AUDIO_TRACE=1).' }
    'rust-devmods'  = @{ Group = 'rust'; Name = 'Play with the dev test mods'; Kind = 'rust';    Hint = "Turns on the version's dev test mod (mods_enable in versions.json)." }
    'rust-perf'     = @{ Group = 'rust'; Name = 'Play + performance trace';   Kind = 'rust';     Hint = 'Bevy performance trace (PLAY.bat -trace).' }
    'tool-refresh'  = @{ Group = 'tools'; Name = 'Refresh game files';        Kind = 'tool';     Hint = 'Runs the check the game runs at start, without starting the game. Out of date: converts again only what changed (setup shows its progress).' }
    'tool-build'    = @{ Group = 'tools'; Name = 'Update and build the selected version'; Kind = 'tool'; Hint = 'Fetches its branch and builds it now, so the next launch starts at once.' }
    'tool-send'     = @{ Group = 'tools'; Name = 'Send the logs now';         Kind = 'tool';     Hint = 'Runs the after-session send (config.json after_session).' }
    'tool-recomp-update'  = @{ Group = 'tools'; Name = 'Recomp: get the newest build'; Kind = 'tool'; Hint = "Copies the main PC's newest recomp build (play-recomp.ps1 -Update)." }
    'tool-recomp-getsave' = @{ Group = 'tools'; Name = 'Recomp: get the main PC save'; Kind = 'tool'; Hint = "Copies the main PC's recomp save after backing up this PC's (play-recomp.ps1 -GetSave)." }
    'tool-setup'    = @{ Group = 'tools'; Name = 'First-time setup';          Kind = 'tool';     Hint = 'Checks git, Rust, the C++ build tools and Python, and installs what is missing.' }
}
# The session traces a version's source contains; rust-traceall turns on all of them. SKATE_TRACE_ALL (newer
# versions) switches on every diagnostic itself; older versions get the single switches they have.
$SessionTraces = [ordered]@{
    traceall   = 'SKATE_TRACE_ALL'
    statelog   = 'SKATE_AUDIO_STATE_LOG'
    audiotrace = 'SKATE_AUDIO_TRACE'
    frametimes = 'SKATE_FRAME_LOG'
    livingdbg  = 'SKATE_LIVING_WORLD_DEBUG'
}
# The switches a version's source contains (null = not set up yet, so unknown).
function Version-Has($v, [string[]]$Needles) {
    $src = Join-Path (Version-Dir $v) 'crates\skate-game\src'
    if (-not (Test-Path $src)) { return $null }
    $files = Get-ChildItem -Path $src -Filter '*.rs' -Recurse -File | Where-Object { $_.Name -notlike '*tests*' }
    $found = @()
    foreach ($n in $Needles) { if ($files | Select-String -Pattern ('"' + $n + '"') -SimpleMatch -List -Quiet) { $found += $n } }
    return ,$found
}
function Version-Traces($v) {
    $has = Version-Has $v @($SessionTraces.Values)
    if ($null -eq $has) { return ,@() }
    return ,@($SessionTraces.Keys | Where-Object { $has -contains $SessionTraces[$_] })
}
if ($Config -and $Config.recomp_entries) {
    foreach ($re in @($Config.recomp_entries | ForEach-Object { $_ })) {
        $Entries[$re.id] = @{ Group = 'recomp'; Name = $re.name; Kind = $(if ($re.trace -or $re.bat) { 'recomp' } else { 'none' }); Hint = "$($re.hint)"; Recomp = $re }
    }
}
# RPCS3 (PS3 retail) entries from config.json: "rpcs3" { exe, game, args } and "rpcs3_entries" [{ id, name, hint, args }].
if ($Config -and $Config.rpcs3_entries) {
    foreach ($pe in @($Config.rpcs3_entries | ForEach-Object { $_ })) {
        $Entries[$pe.id] = @{ Group = 'retail'; Name = $pe.name; Kind = 'none'; Hint = "$($pe.hint)"; Rpcs3 = $pe }
    }
}
$Labels = @('session', 'ride', 'grind', 'bail', 'push', 'emit', 'carve', 'water', 'marker')

# What the first-time setup still needs ('' = nothing): setup.ps1's checks (git, Rust, C++ build tools, LLVM, Python,
# Vulkan). The window opens on the setup page while this names something.
. (Join-Path $Here 'setup.ps1')
$script:MissingCache = $null
function Setup-Missing { if ($null -eq $script:MissingCache) { $script:MissingCache = Missing-Tools }; return $script:MissingCache }

# Why an entry can't run now ('' = it can), for the selected version $V.
function Entry-Blocked([string]$Key, $V) {
    $e = $Entries[$Key]
    if ($Key -eq 'tool-setup') { return '' }
    if ($e.Group -eq 'rust' -or $Key -in @('tool-refresh', 'tool-build')) {
        $m = Setup-Missing
        if ($m) { return "Needs $m first: run the first-time setup (Tools)." }
    }
    switch ($Key) {
        'rust-traceall' { if ((Test-Path (Version-Dir $V)) -and (Version-Traces $V).Count -eq 0) { return "$($V.name) has no session traces: use Play." } }
        'rust-statelog' { $h = Version-Has $V @('SKATE_AUDIO_STATE_LOG'); if ($null -ne $h -and $h.Count -eq 0) { return "$($V.name) has no audio state log." } }
        'rust-trace'    { $h = Version-Has $V @('SKATE_AUDIO_TRACE'); if ($null -ne $h -and $h.Count -eq 0) { return "$($V.name) has no audio trace." } }
        'rust-devmods'  { if (-not $V.mods_enable) { return "$($V.name) has no dev test mod (mods_enable in versions.json)." } }
        'tool-refresh'  {
            $h = Version-Has $V @('--update-game-files')
            if ($null -ne $h -and $h.Count -eq 0) { return "$($V.name) can't refresh from here (built before the launcher refresh): use the game's Extras > Refresh Game Files." }
        }
        'tool-send'     { return (Send-Blocked) }
        { $_ -like 'tool-recomp-*' } { if (-not (Recomp-Script)) { return 'Not set up: this PC has no recomp share (tools\second-pc, see docs\hails-additions\second-pc-testing.md), or set recomp_script in config.json.' } }
    }
    if ($e.Recomp) {
        if ($e.Recomp.bat) { if (-not (Test-Path (Resolve-Local $e.Recomp.bat))) { return "Not set up: $($e.Recomp.bat) not found." } }
        elseif (-not $Config.recomp -or -not $Config.recomp.exe -or -not (Test-Path $Config.recomp.exe)) { return 'Not set up: set recomp.exe in config.json (see README.md).' }
    }
    if ($e.Rpcs3) {
        $r = $Config.rpcs3
        if (-not $r -or -not $r.exe -or -not (Test-Path -LiteralPath $r.exe)) { return 'Not set up: set rpcs3.exe in config.json (see README.md).' }
        if (-not $r.game -or -not (Test-Path -LiteralPath $r.game)) { return 'Not set up: set rpcs3.game in config.json (see README.md).' }
    }
    return ''
}

function Save-Result([string]$Text) { Set-Content -Path $LastSession -Value $Text -Encoding UTF8 }

# The tools: no game session, the result goes to last_session.txt like a session's.
function Run-Tool([string]$Key, $Ver) {
    $ErrorActionPreference = 'Continue'
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    switch ($Key) {
        'tool-setup' {
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here 'setup.ps1') -Install
            $r = if ($LASTEXITCODE -eq 0) { 'First-time setup: everything is installed.' } else { 'First-time setup: not finished (see the steps above).' }
        }
        'tool-build' {
            $r = if (Ensure-Version $Ver) { "Ready: $($Ver.name) ($(Version-Status $Ver))" } else { "Build failed: $($Ver.name). Build log: $script:BuildLog" }
        }
        'tool-send' { $r = Send-Sessions }
        'tool-recomp-update'  { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Recomp-Script) -Update; $r = "Recomp update: exit code $LASTEXITCODE" + $(if ($LASTEXITCODE -eq 0) { ' (done)' } else { ' - see above' }) }
        'tool-recomp-getsave' { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Recomp-Script) -GetSave; $r = "Recomp save: exit code $LASTEXITCODE" + $(if ($LASTEXITCODE -eq 0) { ' (done)' } else { ' - see above' }) }
        'tool-refresh' {
            if (-not (Ensure-Version $Ver)) { $r = "Refresh game files: $($Ver.name) could not be built. Build log: $script:BuildLog"; break }
            $d = Version-Dir $Ver
            . (Join-Path $d 'scripts\Resolve-DevAssets.ps1')
            $assets = Resolve-DevAssets -ProjectRoot $d
            Write-Host "  Checking the game files ($assets)..."
            $said = ''
            & (Join-Path $d 'bin\skate3rust.exe') --update-game-files --assets $assets 2>&1 | ForEach-Object { $l = "$_"; if ($l -like 'GAME_FILES *') { $said = $l.Substring(11) }; Write-Host "  $l" }
            $code = $LASTEXITCODE
            # Setup published a new installation: the other versions' links follow at their next launch.
            $r = switch ($code) { 0 { "Refresh game files: $said." } 2 { 'Refresh game files: no game files yet. Convert them first (tools\setup-dev-assets.ps1).' } default { "Refresh game files: $(if ($said) { $said } else { 'failed' }) (exit code $code)." } }
        }
    }
    Save-Result "$stamp  $Key [$($Ver.id)]`r`n$r"
    Write-Host ''
    Write-Host "  $r"
}

function Launch([string]$Key, [string]$Lbl) {
    $e = $Entries[$Key]
    if (-not $e) { Save-Result "Unknown entry: $Key"; return }
    if ((Running-Games).Count -gt 0) { Save-Result 'Not started: another game is running.'; return }
    $ver = Get-Version $Version
    $blocked = Entry-Blocked $Key $ver
    if ($blocked) { Save-Result "Not started: $blocked"; Write-Host "  $blocked"; return }
    $verText = ''
    if ($e.Group -eq 'tools') { Run-Tool $Key $ver; return }
    $play = $null
    if ($e.Group -eq 'rust') {
        $verText = " [$($ver.id)]"
        Write-Host ("  Version: " + $ver.name)
        if (-not (Ensure-Version $ver)) {
            Save-Result "Not started: $($ver.name) could not be set up or built.$(if ($script:BuildLog) { " Build log: $script:BuildLog" })"
            return
        }
        $play = Join-Path (Version-Dir $ver) 'PLAY.bat'
        # Without converted game files the game only shows an error; say what to do instead (couch users).
        if (-not (Test-Path (Join-Path (Version-Dir $ver) 'assets\private\stock'))) {
            $msg = "Not started: $($ver.name) has no game files (assets\private). Convert them from the disc once: powershell -ExecutionPolicy Bypass -File `"$(Join-Path (Version-Dir $ver) 'tools\setup-dev-assets.ps1')`" -Disc <your Skate 3 ISO or default.xex>."
            Save-Result $msg; Write-Host "  $msg"
            return
        }
        # Per-version environment (versions.json "env": { NAME: value }).
        if ($ver.env) { foreach ($p in $ver.env.PSObject.Properties) { Set-Item -Path ("env:" + $p.Name) -Value ([string]$p.Value) } }
    }
    $since = Get-Date
    Set-Content -Path $Playing -Value ("entry=$Key label=$Lbl version=$($ver.id) started=" + $since.ToString('yyyy-MM-dd HH:mm:ss')) -Encoding ASCII
    Write-Host ("  Starting: " + $e.Name + $verText)
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
            'rust-traceall' {
                $traces = Version-Traces $ver
                # Everything in one folder next to the version's game logs (the send script picks it up from there).
                $dir = Join-Path (Version-Dir $ver) ('logs\trace-all-' + (Get-Date -Format 'yyyyMMdd_HHmmss'))
                New-Item -ItemType Directory -Path $dir -Force | Out-Null
                if ($traces -contains 'statelog') {
                    $env:SKATE_AUDIO_STATE_LOG = Join-Path $dir 'audio_state.tsv'
                    $env:SKATE_AEMS = '1'; $env:SKATE_AUDIO_TIMING = '1'
                }
                if ($traces -contains 'audiotrace') { $env:SKATE_AUDIO_TRACE = '1' }
                if ($traces -contains 'frametimes') { $env:SKATE_FRAME_LOG = Join-Path $dir 'frames.tsv' }
                if ($traces -contains 'livingdbg') { $env:SKATE_LIVING_WORLD_DEBUG = '1' }
                if ($traces -contains 'traceall') {
                    # The engine turns on the rest and writes it next to the paths named here.
                    $env:SKATE_TRACE_ALL = '1'
                    $env:SKATE_PERF_REPORT = Join-Path $dir 'perf.json'
                }
                Write-Host ("  Traces: " + ($traces -join ', ') + " -> $dir")
                Run-Bat $play ''
            }
            default {
                if ($e.Rpcs3) { Run-Rpcs3 $e.Rpcs3; Wait-GameTree; break }
                $re = $e.Recomp
                if ($re.label) { $Lbl = $re.label }
                if ($re.bat) { Run-Bat (Resolve-Local $re.bat) $(if ($Lbl -and $Lbl -ne 'session') { $Lbl } else { "$($re.bat_args)" }) }
                else { Run-Recomp $re $(if ($Lbl -eq 'session') { '' } else { $Lbl }); Wait-GameTree }
            }
        }
    } finally {
        Remove-Item $Playing -ErrorAction SilentlyContinue
    }
    Write-Host '  Session ended.'
    $summary = Check-Session $e.Kind $since $ver
    $repaired = @(Repair-AllAssetLinks)
    if ($repaired.Count -gt 0) { $summary = (@($summary) + $repaired) -join "`r`n" }
    # The send script picks up the logs of this version's checkout as well as the main one's
    # (play-and-send.ps1 reads SKATE_SEND_LOGS_DIR; a version's sessions write to its own logs\, laptop 2026-10-18).
    if ($ver) { $env:SKATE_SEND_LOGS_DIR = Version-Dir $ver }
    $sendResult = Send-Sessions
    if ($sendResult) { $summary = (@($summary, $sendResult) | Where-Object { $_ }) -join "`r`n" }
    $stamp = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    Save-Result ("$stamp  $Key $Lbl$verText`r`n$summary")
}

# Steam's "Exit game" closes the launcher together with the game, so the after-session check can be skipped.
# 'pending' checks the newest session (state log or recomp) if it's newer than last_session.txt and writes the result;
# SkateLauncher.exe runs it at every start. Exit code 10 = a result was written, 0 = nothing pending.
if ($Entry -eq 'pending') {
    # A pulled launcher updates its own exe: SkateLauncher.cs newer than SkateLauncher.exe = build it again. The running
    # exe can be renamed (not overwritten), so the new one takes its name and the next start uses it.
    $cs = Join-Path $Here 'SkateLauncher.cs'; $exe = Join-Path $Here 'SkateLauncher.exe'; $old = Join-Path $Here 'SkateLauncher.old.exe'
    if (Test-Path $old) { Remove-Item $old -ErrorAction SilentlyContinue }
    if ((Test-Path $cs) -and (Test-Path $exe) -and (Get-Item $cs).LastWriteTime -gt (Get-Item $exe).LastWriteTime) {
        $new = Join-Path $Here 'SkateLauncher.new.exe'
        & (Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe') /nologo /target:winexe "/out:$new" $cs | Out-Null
        if ($LASTEXITCODE -eq 0 -and (Test-Path $new)) {
            try { Move-Item $exe $old -Force; Move-Item $new $exe -Force } catch { Remove-Item $new -ErrorAction SilentlyContinue }
        } else { Remove-Item $new -ErrorAction SilentlyContinue }
    }
    # A refresh in any copy can leave the others' assets links dangling: repair them at every start.
    if ((Running-Games).Count -eq 0) { Repair-AllAssetLinks | ForEach-Object { Write-Host "  $_" } }
    $since = [datetime]::MinValue
    if (Test-Path $LastSession) { $since = (Get-Item $LastSession).LastWriteTime }
    $log = Get-ChildItem $StateLogs -Filter 'state_*.tsv' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $rec = $null; if ($Config) { $rec = Get-ChildItem (Recomp-Sessions) -Directory -ErrorAction SilentlyContinue | Sort-Object CreationTime -Descending | Select-Object -First 1 }
    $kind = ''; $t0 = $since
    if ($log -and $log.LastWriteTime -gt $since) { $kind = 'statelog'; $t0 = $log.LastWriteTime.AddSeconds(-1) }
    if ($rec -and $rec.CreationTime -gt $since -and (-not $log -or $rec.CreationTime -gt $log.LastWriteTime)) { $kind = 'recomp'; $t0 = $rec.CreationTime.AddSeconds(-1) }
    # A trace-all session (rust-traceall, version '.') writes logs\trace-all-<time>\ in the repository.
    $ta = Get-ChildItem (Logs-Root) -Directory -Filter 'trace-all-*' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($ta -and $ta.LastWriteTime -gt $since -and $ta.LastWriteTime -gt $t0) { $kind = 'traceall'; $t0 = $ta.CreationTime.AddSeconds(-1) }
    # A PLAYING.txt left by a launcher that was closed with its game is stale once no game runs.
    if ((Test-Path $Playing) -and (Running-Games).Count -eq 0) { Remove-Item $Playing -ErrorAction SilentlyContinue }
    if (Test-Path $Playing) { exit 0 }
    # An engine session closed the same way also missed its send (after_session); send it now.
    $game = Get-ChildItem (Logs-Root) -Filter 'game-*.log' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $missedSend = $Config -and $Config.after_session -and (($game -and $game.LastWriteTime -gt $since) -or ($ta -and $ta.LastWriteTime -gt $since))
    # A plain engine session (version '.') closed with Exit game: its panic check too.
    if ($kind -eq '' -and $game -and $game.LastWriteTime -gt $since) { $kind = 'rust'; $t0 = $game.CreationTime.AddSeconds(-1) }
    if ($kind -eq '' -and -not $missedSend) { exit 0 }
    $summary = ''
    if ($kind) {
        Write-Host '  Checking the last session (it closed before its check)...'
        $summary = Check-Session $kind $t0 $null
    }
    if ($missedSend) { $summary = (@($summary, (Send-Sessions)) | Where-Object { $_ }) -join "`r`n" }
    Save-Result ((Get-Date).ToString('yyyy-MM-dd HH:mm:ss') + "  (checked at next start)`r`n$summary")
    exit 10
}

# Commands SkateLauncher.exe reads (tab-separated).
if ($Entry -eq 'entries') {
    # id, name, hint, ask (1 = ask for a session-folder label), group, on (1/0), why it is off
    $selVer = Get-Version $Version
    foreach ($k in $Entries.Keys) {
        $e = $Entries[$k]
        $ask = 0; if ($e.Recomp -and $e.Recomp.ask_label) { $ask = 1 }
        $why = Entry-Blocked $k $selVer
        "{0}`t{1}`t{2}`t{3}`t{4}`t{5}`t{6}" -f $k, $e.Name, ($e.Hint -replace "`t", ' '), $ask, $e.Group, $(if ($why) { 0 } else { 1 }), $why
    }
    exit 0
}
if ($Entry -eq 'versions') {
    $sel = (Get-Version '').id
    foreach ($v in $Versions) { "{0}`t{1}`t{2}`t{3}" -f $v.id, $v.name, (Version-Status $v), $(if ($v.id -eq $sel) { '*' } else { '' }) }
    exit 0
}
if ($Entry -eq 'status') {
    $v = Get-Version ''
    "version`t$($v.name)"
    "version_status`t$(Version-Status $v)"
    $last = if (Test-Path $LastSession) { ((Get-Content $LastSession) -join ' / ') } else { 'No session yet.' }
    "last`t$last"
    $sb = Send-Blocked; "send`t$(if (-not (Send-Script)) { 'off' } elseif ($sb) { $sb } else { 'on' })"
    "setup_missing`t$(Setup-Missing)"
    exit 0
}
if ($Entry -eq 'setup-check') { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Here 'setup.ps1'); exit $LASTEXITCODE }
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
    if (Ensure-Version $v) { "Ready: $($v.name) ($(Version-Status $v))"; exit 0 } else { "Build failed: $($v.name). Build log: $script:BuildLog"; exit 1 }
}
# Clearing (Tools > Clear builds): a version's build cache is its cargo target folder; its build is that plus bin\
# (the exe it plays), so it builds again at its next launch. Only these launcher-known folders are ever deleted.
if ($Entry -eq 'sizes') {
    # id, name, build cache bytes, bin bytes
    foreach ($v in $Versions) { "{0}`t{1}`t{2}`t{3}" -f $v.id, $v.name, (Folder-Bytes (Target-Dir $v)), (Folder-Bytes (Join-Path (Version-Dir $v) 'bin')) }
    exit 0
}
if ($Entry -in @('clear-cache', 'clear-build')) {
    if ((Running-Games).Count -gt 0) { Save-Result 'Not cleared: a game is running.'; 'Not cleared: a game is running.'; exit 1 }
    $which = @($Versions | Where-Object { $Label -eq 'all' -or $_.id -eq $Label })
    if ($which.Count -eq 0) { "Unknown version: $Label"; exit 1 }
    $freed = [long]0; $done = @()
    foreach ($v in $which) {
        $dirs = @(Target-Dir $v)
        if ($Entry -eq 'clear-build') { $dirs += (Join-Path (Version-Dir $v) 'bin') }
        foreach ($d in $dirs) {
            if (-not (Test-Path -LiteralPath $d)) { continue }
            $freed += Folder-Bytes $d
            Write-Host "  Deleting $d..."
            Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction SilentlyContinue
        }
        $done += $v.name
    }
    $what = if ($Entry -eq 'clear-build') { 'builds' } else { 'build caches' }
    $r = "Cleared the $what of " + ($done -join ', ') + (" ({0:N1} GB freed)." -f ($freed / 1GB))
    Save-Result ((Get-Date).ToString('yyyy-MM-dd HH:mm:ss') + "  $Entry $Label`r`n$r")
    "  $r"
    exit 0
}
if ($Entry -eq 'room') { Make-Room (Get-Version $(if ($Label) { $Label } else { $Version })); exit 0 }
if ($Entry -eq 'send') { $r = Send-Sessions; if (-not $r) { $r = 'Send: no after_session in config.json.' }; $r; exit 0 }
if ($Entry -eq 'list') { $Entries.Keys | ForEach-Object { '{0,-20} {1}' -f $_, $Entries[$_].Name }; exit 0 }
if ($Entry) { Launch $Entry $Label; exit 0 }

# No entry: open the window (the menu lives in SkateLauncher.exe now).
$exe = Join-Path $Here 'SkateLauncher.exe'
if (-not (Test-Path $exe)) { Write-Host "SkateLauncher.exe is not built yet: run build.bat in $Here first."; exit 1 }
Start-Process -FilePath $exe
