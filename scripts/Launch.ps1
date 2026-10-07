param([string]$Map, [switch]$Trace, [string]$TraceFile, [string]$Assets)
$ProjectRoot = Split-Path $PSScriptRoot -Parent
$ErrorActionPreference = 'Stop'
Push-Location $ProjectRoot
try {
    $executable = Join-Path $ProjectRoot 'bin/skate3rust.exe'
    if (-not (Test-Path -LiteralPath $executable)) { throw 'Game is not built. Run BUILD.bat first.' }
    New-Item -ItemType Directory -Path (Join-Path $ProjectRoot 'logs') -Force | Out-Null
    $log = Join-Path $ProjectRoot ('logs/game-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
    Write-Host 'Starting Skate 3 Rust Engine. Use your XInput controller; Esc opens difficulty/graphics/pause settings.'
    Write-Host "Log: $log"
    $errorLog = [System.IO.Path]::ChangeExtension($log, 'stderr.log')
    # Start-Process ArgumentList arrays split on spaces on Windows PowerShell 5.1.
    # Pass one quoted command-line string so paths under "Ethans Desktop 2.0" stay intact.
    . (Join-Path $PSScriptRoot 'Resolve-DevAssets.ps1')
    $assetsPath = Resolve-DevAssets -ProjectRoot $ProjectRoot -Assets $Assets
    Write-Host "Assets: $assetsPath"
    $argumentList = '--assets "' + $assetsPath + '"'
    if (-not $Map -and $env:SKATE_TRACE_MAP) { $Map = $env:SKATE_TRACE_MAP }
    if (-not $TraceFile -and $env:SKATE_TRACE_FILE) { $TraceFile = $env:SKATE_TRACE_FILE }
    if ($Map) {
        $mapPath = (Resolve-Path -LiteralPath $Map).Path
        if ([System.IO.Path]::GetExtension($mapPath) -ine '.skate') { throw "Not a .skate map file: $mapPath" }
        $argumentList += ' --map "' + $mapPath + '"'
        Write-Host "Map: $mapPath"
    }
    if ($Trace) {
        $tracePath = if ($TraceFile) {
            if ([System.IO.Path]::IsPathRooted($TraceFile)) { $TraceFile } else { Join-Path $ProjectRoot $TraceFile }
        } else {
            Join-Path $ProjectRoot 'trace-downtown-lag.json'
        }
        if (Test-Path -LiteralPath $tracePath) { Remove-Item -LiteralPath $tracePath -Force }
        $argumentList += ' --trace "' + $tracePath + '" --trace-wait --trace-seconds 10'
        Write-Host "Trace: $tracePath"
        Write-Host 'F9 starts a 10-second CPU capture; export stops automatically. Open the JSON in Perfetto.'
    }
    # Dev packages live in repo mods/; the exe otherwise only looks beside bin/.
    $env:SKATE3_MODS = Join-Path $ProjectRoot 'mods'
    New-Item -ItemType Directory -Path $env:SKATE3_MODS -Force | Out-Null
    Write-Host "Mods: $env:SKATE3_MODS"
    $game = Start-Process -FilePath $executable -WorkingDirectory $ProjectRoot `
        -ArgumentList $argumentList -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $log -RedirectStandardError $errorLog
    if ($game.ExitCode -ne 0) {
        Get-Content -LiteralPath $errorLog -Tail 30
        throw "Game exited with code $($game.ExitCode). Log: $errorLog"
    }
} finally { Pop-Location }
