# Second PC: play the traced Skate 3 recompilation (research hooks) for one trace preset, check the trace, then send
# it to the main PC like any other session. Sessions are folders logs\recomp-<preset>-<date>-<time>\ (the "recomp-"
# prefix tells them apart from engine sessions); play-and-send.ps1 sends them with the rest.
# The recomp build and its game folder come from the main PC's share (share-recomp.ps1 puts them there); they hold
# recompiled game code and your game files, so they only ever travel over your own network, never to GitHub.
# Guide: docs/hails-additions/second-pc-testing.md#recomp-trace-sessions
param(
    # The trace preset (see -List).
    [string]$Trace = '',
    # Print the presets and what to do in each.
    [switch]$List,
    # Copy the newest recomp build from the main PC's share (do this when the main PC says there is a new one).
    [switch]$Update,
    # First time only: copy the recomp game folder (about 7.5 GB) from the share, then remove the staged copy there.
    [switch]$GetGame,
    # Do not send after the session (send later with play-and-send.ps1 -SendOnly).
    [switch]$NoSend
)
$ErrorActionPreference = 'Stop'
# The game folder: next to the script (release ZIP), else the checkout this script is part of (tools\second-pc).
$Root = $PSScriptRoot
$checkout = Split-Path (Split-Path $PSScriptRoot)
if (-not (Test-Path (Join-Path $Root 'skate3rust.exe')) -and (Test-Path (Join-Path $checkout 'Cargo.toml'))) { $Root = $checkout }
Set-Location $Root
$recomp = Join-Path $Root '.local\recomp'
$binDir = Join-Path $recomp 'bin'
$gameDir = Join-Path $recomp 'game'
$configFile = Join-Path $Root 'send-sessions.txt'

# Presets: trace categories (SKATE3_TRACE), whether the mixed game sound is captured, extra variables, what to do.
$presets = [ordered]@{
    'props'   = @{ cats = 'props'; capture = $false; env = @{}; todo = 'About 2 minutes: ride slowly into an orange pylon (drum or pole), then fast into another pylon, roll onto a bench seat (no grind), grind a bench seat, then get off the board and walk into a bench. About 5 seconds between them, no other props touched in between, in this order.' }
    'npc'     = @{ cats = 'npc,world,traffic'; capture = $false; env = @{}; todo = 'Skate around peds and NPC skaters as normal.' }
    'pedbody' = @{ cats = 'npc,pedbody'; capture = $false; env = @{}; todo = 'Bump into peds, run into them fast, knock some down.' }
    'traffic' = @{ cats = 'traffic,skitch,npc'; capture = $false; env = @{}; todo = 'Skitch cars, let go at different speeds, also of a stopped car; ride near junctions.' }
    'grind'   = @{ cats = 'grind'; capture = $false; env = @{ SKATE3_GRIND_BOX = '250 300 -445 -410' }; todo = 'Grind the rails and stairs by the University library (full-rate lines inside that box).' }
    'audio'   = @{ cats = 'audio'; capture = $true; env = @{}; todo = 'Play as normal; the sound is recorded with the trace.' }
    'audiox'  = @{ cats = 'audio,audiox,dsp'; capture = $true; env = @{}; todo = 'Heavy: keep it to 1 or 2 minutes around the sound being studied.' }
    'all'     = @{ cats = 'audio,dsp,npc,world,traffic,aiskater'; capture = $true; env = @{}; todo = 'Everything at once (large): play as normal.' }
}

function Show-Presets {
    foreach ($k in $presets.Keys) { Write-Host ("{0,-8} {1}" -f $k, $presets[$k].todo) }
}

function Get-Share {
    if (-not (Test-Path $configFile)) { throw 'Not set up: run play-and-send.ps1 -Setup first (the same share is used).' }
    $c = @{}
    foreach ($line in Get-Content $configFile) { if ($line -match '^\s*(\w+)\s*=\s*(.*?)\s*$') { $c[$Matches[1]] = $Matches[2] } }
    foreach ($server in @($c.main_pc, $c.main_ip) | Where-Object { $_ }) {
        $unc = "\\$server\$($c.share)"
        # Windows PowerShell 5.1 turns a native program's stderr into errors, fatal under Stop: run net with Continue.
        $ErrorActionPreference = 'Continue'
        & net.exe use $unc /persistent:no 2>&1 | Out-Null
        $ErrorActionPreference = 'Stop'
        if (Test-Path "$unc\_recomp") { return "$unc\_recomp" }
    }
    throw 'The main PC did not answer, or it has not shared a recomp build yet (share-recomp.ps1 on the main PC).'
}

if ($List -or (-not $Trace -and -not $Update -and -not $GetGame)) {
    Write-Host 'Usage: PLAY-RECOMP.bat <preset>   (first time: PLAY-RECOMP.bat -GetGame, then -Update)'
    Show-Presets
    return
}

if ($GetGame) {
    $share = Get-Share
    if (-not (Test-Path "$share\game\game\default.xex")) { throw "No staged game folder in $share\game (the main PC stages it with share-recomp.ps1 -WithGame)." }
    Write-Host "Copying the recomp game folder (about 7.5 GB) to $gameDir ..."
    New-Item -ItemType Directory -Force $gameDir | Out-Null
    & robocopy.exe "$share\game" $gameDir /E /NFL /NDL /NJH /NP /R:2 /W:5 | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE); run -GetGame again." }
    # Verified by size and file count before the staged copy on the main PC goes.
    $a = Get-ChildItem "$share\game" -Recurse -File | Measure-Object Length -Sum
    $b = Get-ChildItem $gameDir -Recurse -File | Measure-Object Length -Sum
    if ($a.Count -ne $b.Count -or $a.Sum -ne $b.Sum) { throw "Copy incomplete ($($b.Count) of $($a.Count) files); run -GetGame again." }
    Remove-Item -Recurse -Force "$share\game"
    Write-Host "Game folder ready ($($b.Count) files); the staged copy on the main PC was removed."
}

if ($Update) {
    $share = Get-Share
    if (-not (Test-Path "$share\bin\skate3.exe")) { throw "No recomp build in $share\bin." }
    New-Item -ItemType Directory -Force $binDir | Out-Null
    & robocopy.exe "$share\bin" $binDir /MIR /NFL /NDL /NJH /NP /R:2 /W:5 | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed ($LASTEXITCODE)." }
    Write-Host "Recomp build updated: $(Get-Content (Join-Path $binDir 'build.txt') -ErrorAction SilentlyContinue | Select-Object -First 1)"
}

if (-not $Trace) { return }
if (-not $presets.Contains($Trace)) { Write-Host "Unknown preset '$Trace'."; Show-Presets; exit 2 }
$p = $presets[$Trace]
$exe = Join-Path $binDir 'skate3.exe'
if (-not (Test-Path $exe)) { throw 'No recomp build yet: run PLAY-RECOMP.bat -Update.' }
if (-not (Test-Path (Join-Path $gameDir 'game\default.xex'))) { throw 'No recomp game folder yet: run PLAY-RECOMP.bat -GetGame.' }
# One game at a time.
if (Get-Process skate3, skate3rust -ErrorAction SilentlyContinue) { throw 'A game is already running: close it first.' }

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$out = Join-Path $Root "logs\recomp-$Trace-$stamp"
New-Item -ItemType Directory -Force $out | Out-Null
$env:SKATE3_AUDIO_TRACE_FILE = Join-Path $out 'trace.tsv'
$env:SKATE3_TRACE = $p.cats
$env:SKATE3_INPUT_RECORD = Join-Path $out 'pad.txt'
if ($p.capture) { $env:SKATE3_AUDIO_CAPTURE = Join-Path $out 'audio.f32' } else { Remove-Item Env:SKATE3_AUDIO_CAPTURE -ErrorAction SilentlyContinue }
foreach ($k in $p.env.Keys) { Set-Item "Env:$k" $p.env[$k] }
Write-Host "Recomp trace '$Trace' ($($p.cats)). $($p.todo)"
Write-Host 'Quit: Escape or F1 (or hold LB + RB + Back), then "Quit to the desktop".'
$start = Get-Date
$proc = Start-Process -FilePath $exe -ArgumentList '--skate3_demo_path', '--skate3_demo_path_signed_in' -WorkingDirectory $gameDir `
    -RedirectStandardOutput (Join-Path $out 'game.out') -RedirectStandardError (Join-Path $out 'game.err') -PassThru -Wait
$end = Get-Date

# Every line must be KIND<tab>ms...; a trace with any other line is unusable and is marked so.
$lines = 0; $bad = 0
$tracePath = Join-Path $out 'trace.tsv'
if (Test-Path $tracePath) {
    foreach ($line in [IO.File]::ReadLines($tracePath)) {
        if ($line.Length -eq 0) { continue }
        $lines++
        if ($line -notmatch '^[A-Z][A-Z0-9_]*\t[0-9.]+(\t|$)') { $bad++ }
    }
}
$build = Get-Content (Join-Path $binDir 'build.txt') -ErrorAction SilentlyContinue | Select-Object -First 1
@(
    "preset=$Trace", "categories=$($p.cats)", "audio_capture=$($p.capture)", "pc=$env:COMPUTERNAME",
    "start=$($start.ToString('s'))", "end=$($end.ToString('s'))", "minutes=$([math]::Round(($end - $start).TotalMinutes, 1))",
    "exit_code=$($proc.ExitCode)", "recomp_build=$build", "trace_lines=$lines", "malformed_lines=$bad"
) | Set-Content -Encoding ASCII (Join-Path $out 'session.txt')
if ($lines -eq 0) { Write-Host 'Warning: the trace is empty (did the game start?). game.err may say why.' }
elseif ($bad -gt 0) { Write-Host "Warning: $bad malformed lines of $lines; this trace cannot be used." }
else { Write-Host "Trace OK: $lines lines, 0 malformed." }

if (-not $NoSend) {
    & (Join-Path $PSScriptRoot 'play-and-send.ps1') -SendOnly
}
