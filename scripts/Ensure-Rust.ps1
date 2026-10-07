# Find the user's Rust installation even when this script is launched directly.
$ErrorActionPreference = 'Stop'
if ((Get-Command cargo -ErrorAction SilentlyContinue) -and (Get-Command rustc -ErrorAction SilentlyContinue)) { return }
$cargoRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$rustBin = Join-Path $cargoRoot 'bin'
if ((Test-Path -LiteralPath (Join-Path $rustBin 'cargo.exe')) -and
    (Test-Path -LiteralPath (Join-Path $rustBin 'rustc.exe'))) {
    $env:PATH = $rustBin + ';' + $env:PATH
    return
}
throw 'Cargo and rustc were not found. Install Rust with rustup or add your Rust toolchain to PATH.'
