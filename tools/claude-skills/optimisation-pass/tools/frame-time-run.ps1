# Muted, minimised frame-time run of one game build (skill optimisation-pass, "Engine runs").
# Prints frames, median / p90 / p99 / max frame_ms of frames 100+, the FRAME_HITCH lines and error lines; keeps the
# run's frames.tsv and system_timing.log (SYSTEM_TIMING lines, on under trace-all) next to each other.
# Usage: frame-time-run.ps1 -Bin <folder with skate3rust.exe> -Install <install folder with assets\ and maps\>
#   [-Map University] [-SwitchTo DownTown] [-SwitchAfter 25] [-Seconds 70] [-Mods <mods folder>] [-Lock <file>]
# Do not pipe its output into Select-Object -First: that stops the script before it saves the logs.
# -SwitchTo requests one map switch -SwitchAfter seconds after start (SKATE_MAP_SWITCH). Without -Mods no mods load.
param(
    [Parameter(Mandatory)][string]$Bin,
    [Parameter(Mandatory)][string]$Install,
    [string]$Map = 'University',
    [string]$SwitchTo = '',
    [int]$SwitchAfter = 25,
    [int]$Seconds = 70,
    [string]$Mods = '',
    # A file held while the game runs (one game at a time); skipped when empty.
    [string]$Lock = '',
    # Only the frame log (SKATE_FRAME_LOG), no trace-all: no span bookkeeping, no system timing.
    [switch]$Plain
)
$ErrorActionPreference = 'Stop'
if (Get-Process skate3rust -ErrorAction SilentlyContinue) { throw 'A game is already running (one game at a time).' }
if ($Lock) {
    if (Test-Path $Lock) { throw "Game lock held: $(Get-Content $Lock -Raw)" }
    Set-Content $Lock "frame-time-run $Bin $(Get-Date -Format s)"
}
$err = Join-Path $env:TEMP "frame-time-run-$PID-$(Get-Date -Format HHmmss).stderr.log"
$empty = Join-Path $env:TEMP 'frame-time-run-nomods'
try {
    if ($Plain) {
        Remove-Item Env:SKATE_TRACE_ALL -ErrorAction SilentlyContinue
        $plainDir = Join-Path $Bin "logs\plain-$(Get-Date -Format yyyyMMdd-HHmmss)"
        New-Item -ItemType Directory -Force $plainDir | Out-Null
        $env:SKATE_FRAME_LOG = Join-Path $plainDir 'frames.tsv'
    } else { $env:SKATE_TRACE_ALL = '1' }
    $env:SKATE_REPORT_CHILD = '1'
    if (-not $Mods) { New-Item -ItemType Directory -Force $empty | Out-Null; $Mods = $empty }
    $env:SKATE3_MODS = $Mods
    $env:SKATE_MAP_SWITCH = if ($SwitchTo) { "${SwitchAfter}:$(Join-Path $Install "maps\$SwitchTo.skate")" } else { '' }
    $exe = Join-Path $Bin 'skate3rust.exe'
    $assets = (Get-Item (Join-Path $Install 'assets')).FullName
    $mapPath = Join-Path $Install "maps\$Map.skate"
    $p = Start-Process $exe -ArgumentList @('--assets', "`"$assets`"", '--map', "`"$mapPath`"", '--mute') `
        -WorkingDirectory $Bin -WindowStyle Minimized -RedirectStandardError $err -PassThru
    $deadline = (Get-Date).AddSeconds($Seconds + 90)
    $started = $null
    while (-not $p.HasExited -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        if (-not $started -and ($Plain -and (Test-Path $env:SKATE_FRAME_LOG) -or (Select-String -Path $err -Pattern 'TRACE_ACTIVE' -Quiet))) { $started = Get-Date }
        if ($started -and ((Get-Date) - $started).TotalSeconds -ge $Seconds) { break }
    }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(10000) | Out-Null }
    if (-not $Plain -and -not (Select-String -Path $err -Pattern 'TRACE_ACTIVE' -Quiet)) { Get-Content $err -Tail 20; throw 'No TRACE_ACTIVE line: the run did not start tracing.' }
    # The TRACE_ACTIVE line's paths hold spaces; this run's frames.tsv is the newest one.
    $tsv = if ($Plain) { $env:SKATE_FRAME_LOG } else { Get-ChildItem (Join-Path $Bin 'logs') -Recurse -Filter frames.tsv | Sort-Object LastWriteTime |
        Select-Object -Last 1 -ExpandProperty FullName }
    $ms = @(Import-Csv $tsv -Delimiter "`t" | Select-Object -Skip 100 | ForEach-Object { [double]$_.frame_ms } | Sort-Object)
    if ($ms.Count -eq 0) { throw "No frames after 100 in $tsv" }
    $q = { param($f) $ms[[Math]::Min($ms.Count - 1, [int][Math]::Floor($f * $ms.Count))] }
    '{0}{1}: {2} frames, median {3:N2} ms, p90 {4:N2} ms, p99 {5:N2} ms, max {6:N1} ms ({7})' -f $Map,
        $(if ($SwitchTo) { " -> $SwitchTo at $SwitchAfter s" } else { '' }), $ms.Count, (& $q 0.5), (& $q 0.9),
        (& $q 0.99), $ms[-1], $tsv
    Select-String -CaseSensitive -Path $err -Pattern 'SKATE_MAP_SWITCH requesting|MAP_TRANSITION_COMMITTED' | ForEach-Object {
        $l = $_.Line; $l.Substring([Math]::Max($l.IndexOf('SKATE_MAP_SWITCH'), $l.IndexOf('MAP_TRANSITION'))) }
} finally {
    if ($Lock) { Remove-Item $Lock -ErrorAction SilentlyContinue }
    if (Test-Path $err) {
        $bad = @(Select-String -CaseSensitive -Path $err -Pattern 'panicked|ERROR|OFFBOARD_INVALID|NONFINITE|physics_failed:true')
        "log check: $($bad.Count) panic/error line(s)"
        $bad | Select-Object -First 5 | ForEach-Object { $_.Line.Substring(0, [Math]::Min(200, $_.Line.Length)) }
        $hitches = @(Select-String -CaseSensitive -Path $err -Pattern 'FRAME_HITCH')
        "hitch lines: $($hitches.Count)"
        $hitches | Select-Object -First 12 | ForEach-Object { $l = $_.Line; $i = $l.IndexOf('FRAME_HITCH'); $l.Substring($i, [Math]::Min(400, $l.Length - $i)) }
        $timing = @(Select-String -CaseSensitive -Path $err -Pattern 'SYSTEM_TIMING frames=' | ForEach-Object { $_.Line.Substring($_.Line.IndexOf('SYSTEM_TIMING')) })
        if ($timing.Count -and $tsv) { $out = Join-Path (Split-Path $tsv) 'system_timing.log'; $timing | Set-Content -Encoding utf8 $out; "system timing: $($timing.Count) line(s) in $out" }
        if ($tsv) { Copy-Item $err (Join-Path (Split-Path $tsv) 'stderr.log') }
    }
    Remove-Item $err -ErrorAction SilentlyContinue
    Remove-Item Env:SKATE_MAP_SWITCH -ErrorAction SilentlyContinue
    if ($Plain) { Remove-Item Env:SKATE_FRAME_LOG -ErrorAction SilentlyContinue }
}
