---
name: vendor-patch
description: Vendor and patch a crates.io dependency (Bevy, sdl3-sys, ...) the way this repo does, or add/upgrade a native C/C++ dependency safely. Use when a dependency needs a local fix, when a link error comes from a -sys crate, or when upgrading a vendored crate.
---

# Vendoring and patching a dependency

Existing vendored crates: `vendor/bevy_pbr`, `vendor/bevy_core_pipeline`, `vendor/sdl3-sys`. `vendor/README.md`
documents each change; read it before touching one. Don't bump Bevy or sdl3 casually.

## Convention
1. Copy the exact crates.io source (the version already in `Cargo.lock`) from
   `~/.cargo/registry/src/index.crates.io-*/<crate>-<version>` to `vendor/<crate>`. Keep its licenses and
   `Cargo.toml` untouched.
2. Make the smallest possible change; mark it in code with a `skate3rust patch:` comment saying why.
3. Add to the root `Cargo.toml` `[patch.crates-io]`: `<crate> = { path = "vendor/<crate>" }`.
4. Update the lockfile once: `cargo update -p <crate> --offline` (otherwise every `--locked` build fails with
   "cannot update the lock file").
5. Document it in `vendor/README.md`: upstream version, exactly what changed and why, how it was verified, and what
   to do on upgrade.
6. Verify both link configurations (below).

## Native (C/C++) dependencies: build both configurations
- Dev (dynamic CRT, dynamic Bevy):
  `cargo build -p skate-game --bin skate3rust --release`
- Release/CI (static CRT, static Bevy; slow, run it in the background):
  `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --locked
  --target x86_64-pc-windows-msvc -p skate-game --bin skate3rust --no-default-features`
- CMake-based `-sys` crates need `CMAKE` set (scripts use `scripts/Ensure-CMake.ps1`; from Bash export the path to
  Visual Studio's bundled cmake.exe).
- Check the result: `llvm-readobj --coff-imports <exe>`; the static build must not import `vcruntime140.dll` or
  `api-ms-win-crt-*`.

## Diagnosing link errors from a C library
- `unresolved external symbol __imp_<crt fn>` -> C runtime mismatch (object built for the DLL CRT, Rust linking the
  static one, or the reverse).
- Look at `target\<triple>\release\build\<crate>-*\out\build\CMakeCache.txt` for `CMAKE_C_FLAGS` /
  `CMAKE_MSVC_RUNTIME_LIBRARY`.
- CMake >= 3.15 projects (policy CMP0091) ignore `-MT`/`-MD` in flags; set `CMAKE_MSVC_RUNTIME_LIBRARY` from the
  crate's `build.rs` instead (see the `vendor/sdl3-sys/build.rs` patch). Don't try CMake env vars or a
  `CMAKE_TOOLCHAIN_FILE` env var (cmake-rs then switches to cross-compile mode).
