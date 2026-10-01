# Hails' additions

Changes made in the [Hailey-Ross fork](https://github.com/Hailey-Ross/skate-3-rust-engine)
of SK8-ENGINE/skate-3-rust-engine, documented so upstream maintainers have the
full context when reviewing a pull request: what broke, why, what changed, how
it was verified, and what is still open.

Each document stands alone. File paths are relative to the repository root.

| # | Change | Area | Status |
|---|---|---|---|
| 1 | [ISO extraction argument order](01-iso-extraction.md) | Setup (Python) | Done, verified with a real ISO |
| 2 | [Windows long paths during setup](02-long-paths.md) | Setup (Python) | Done; UAC path not exercised end to end |
| 3 | [SDL3 gamepad input](03-sdl3-gamepad-input.md) | Game (Rust), build scripts, vendored `sdl3-sys` | Done, verified in play with an Xbox Elite Series 2 |
| 4 | [Authored map spawns](04-map-spawns.md) | Asset pipeline (Python) | Done, all 10 maps confirmed in play |
| 5 | [Invisible collision volumes](05-collision-volumes.md) | Game data loader (Rust) | Done, all four affected maps confirmed in play |

## Environment used for verification

- Windows 11, Intel i7-14700KF, NVIDIA RTX 4080 SUPER (Vulkan).
- Rust 1.98.1 (MSVC), Visual Studio 2026 C++ tools, Windows SDK 10.0.26100, LLVM 23.1.2, Python 3.13.15.
- Skate 3 Xbox 360 disc image (103 files, 6,404,940,920 bytes).
- Xbox Elite Series 2 controller over Bluetooth LE.

## Conventions

- Upstream behaviour is preserved unless a document says otherwise; where a
  behaviour changes on purpose (for example, a map's default spawn), the
  document lists the old and new values.
- Project choices that are not dictated by retail data are called out as
  such, so maintainers can overrule them.
