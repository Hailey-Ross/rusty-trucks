# Stop only the dev recomp build (never an installed copy of the game): processes named skate3 whose exe
# lives in $env:RECOMP_EXE_DIR (set by recomp_script_run.sh) or in -ExeDir.
param([string]$ExeDir = $env:RECOMP_EXE_DIR)
if (-not $ExeDir) { Write-Host 'set RECOMP_EXE_DIR or pass -ExeDir'; exit 2 }
$prefix = $ExeDir.TrimEnd('\') + '\*'
Get-Process skate3 -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -like $prefix } |
  Stop-Process -Force
