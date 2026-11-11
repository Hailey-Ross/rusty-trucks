<h1 align="center">Rusty Trucks</h1>

<p align="center">
  <img src="docs/images/rusty-trucks-board.svg" alt="The Rusty Trucks skateboard" width="560">
</p>

<p align="center">
  <a href="https://github.com/Hailey-Ross/rusty-trucks/actions/workflows/rusty-trucks-win.yml"><img src="https://github.com/Hailey-Ross/rusty-trucks/actions/workflows/rusty-trucks-win.yml/badge.svg" alt="Rusty Trucks Windows build"></a>
  <a href="https://github.com/Hailey-Ross/rusty-trucks/actions/workflows/rusty-trucks-linux.yml"><img src="https://github.com/Hailey-Ross/rusty-trucks/actions/workflows/rusty-trucks-linux.yml/badge.svg" alt="Rusty Trucks Linux build"></a>
  <a href="https://github.com/Hailey-Ross/rusty-trucks/actions/workflows/rusty-trucks-macos.yml"><img src="https://github.com/Hailey-Ross/rusty-trucks/actions/workflows/rusty-trucks-macos.yml/badge.svg" alt="Rusty Trucks macOS build"></a>
</p>

Rusty Trucks is a custom flavour of the [Skate 3 Rust Engine](https://github.com/SK8-ENGINE/skate-3-rust-engine), the Rust and Bevy rewrite of Skate 3. It's where I build and try out new things before they head upstream: a living city to skate through, Skate 3's own menus, the instant replay, your DLC, and now career mode and challenges. The `hails-additions` branch is the rolling build, so whatever I'm working on lands here first.

Why the name? It's written in Rust, and trucks are the bits that hold your wheels on. It felt right.

> **Heads up: this is an early alpha.** Things will break, and I'd love to hear about it when they do (see [What to test](#what-to-test)). You'll need your own Skate 3 Xbox 360 disc image; no game files are included here.

## What I'm aiming for

- **It should feel like Skate 3.** I port systems from Skate 3's own code (read through the [skate3recomp](https://github.com/mchughalex/skate3recomp) recompilation and Ghidra) instead of guessing how they work. Anything I haven't worked out yet is marked "NOT RETAIL YET" until I have.
- **Your disc, your game.** Every value, model, sound and menu movie comes from your own disc during setup. Nothing from the game lives in this repository.
- **Made to be modded.** Every system gets a Lua entry point (the mod SDK, API 2) from day one, and the disc's values are just defaults a mod can change. If you want to build something on top of it, I want that to be easy.
- **Ready for skating together.** New systems are written so they can be shared between players later (stable ids, events that can go over a network). There's no multiplayer yet, but I don't want to paint us into a corner.
- **Give it back.** Every change gets a write-up in [`docs/hails-additions/`](docs/hails-additions/README.md) with the evidence and the tests, and goes to the upstream project as a pull request.

## Getting started

The easiest way in is the Skate launcher:

<p align="center">
  <img src="https://raw.githubusercontent.com/Hailey-Ross/rusty-trucks/pr-assets/launcher/01-camcorder-lcd-panel.gif" alt="The Skate launcher: a camcorder's LCD panel swings open, shows the HOME menu and folds shut" width="640">
</p>

1. Grab `skate-launcher-windows.zip` from the **[hails-additions release](https://github.com/Hailey-Ross/rusty-trucks/releases/tag/hails-additions)** and extract it into a folder of its own.
2. Double-click `build.bat` once. It downloads `SkateLauncher.exe` and starts it.
3. Pick a game folder and your Skate 3 ISO (or an extracted folder with `default.xex`). Your first play downloads the newest build and converts your disc once, so give it a little while.
4. After that it keeps itself up to date and saves a session log for every game you play.

The launcher ([`tools/steam-launcher`](tools/steam-launcher/README.md)) is styled after the flip-out screen of a mid-2000s camcorder ([how it's made](docs/hails-additions/41-launcher-camcorder-viewfinder.md#change)). Add it to Steam as a non-Steam game and you can start everything from the couch with a controller. It can also run upstream `main`, open pull requests, the recomp and retail Skate 3 in RPCS3 side by side, which is handy when you want to compare.

Rather skip the launcher? Download `skate3rust-windows-x64.zip` from the same release, extract it and start `PLAY-WITH-LOG.bat`. The full setup guide is in the [release notes](https://github.com/Hailey-Ross/rusty-trucks/releases/tag/hails-additions).

You'll need Windows 10 or 11 (64-bit), a controller (Xbox, PlayStation, Switch or generic) and about 15 GB free.

## What's in so far

Everything here is in the `hails-additions` build, and the [release notes](https://github.com/Hailey-Ross/rusty-trucks/releases/tag/hails-additions) go into detail on each one.

- **A living world:** NPC skaters riding Skate 3's recorded lines, pedestrians going about their day (benches, phone calls, warnings, chases), traffic you can skitch, and props you can push, carry and grab.
- **Skate 3's own menus:** the pause menu, Game Settings, the Trick Guide with its 3D demo, Extras with cheat codes, Skate Profile and the Challenge Map.
- **Free Play:** Spawn Skater (play as someone else, or call up to three skaters to ride with you), traffic and pedestrian density, and the HUD minimap.
- **The loading screen:** Skate 3's tips screen, with Coach Frank and your career stats, while a map loads.
- **Instant replay:** the last 90 seconds of the whole world with sound, the replay editor, and Save to PC.
- **Your DLC:** the DLC maps, Challenge Map folders, skate.Park layouts and teleporters from the packs you own.
- **World shading:** Skate 3's lightmaps and shaders for fences, signs, billboards, traffic lights and trees.
- **Audio:** Skate 3's audio engine, ported natively, with audio modding.
- **Opt-ins:** anything that's off by default or not quite Skate 3 lives in one place, the Opt-Ins section of the Guide / F1 / Esc menu.
- **Already upstream:** the audio engine, water, retail map spawns, trigger volumes, camera angles, SDL3 gamepads, grind bails at rail bends, see-through fences and the retail menus foundation started here and are now in upstream `main`.

## What I'm working on

- **Career mode and challenges** (upstream draft [#68](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/68), behind the Career and Challenges opt-in): you can already play the own the spot challenges, and the career core and all 12 Skate 101 tutorials run from your disc. Next up are the reward screens and the end-of-challenge screens, then more challenge types (races, films, Hall of Meat, contests).
- **The retail startup:** the boot movies, Press Start and the new career welcome (difficulty, camera, team name, Coach Frank), plus a cross-platform player for Skate 3's movies so it works beyond Windows too.
- **NPC skaters:** finishing their landings, spins and grab releases.
- **Further out:** the pros at their challenge spots, the Object Dropper, pedestrians watching you skate, and multiplayer.

Curious where things stand upstream? The living world, menus, replay and DLC are in [#52](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/52); career mode and challenges are in [#68](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/68).

## What to test

This is where you can help the most. A few things I'd love a second pair of eyes on:

- Map switches and Challenge Map teleports: does the loading screen show up and hand you back cleanly?
- Free Play's Spawn Skater and the minimap.
- The instant replay and its editor (Back / F6, then A). Does your replay look like what you actually did?
- DLC maps and teleporters, if you own the packs.
- Pedestrians and NPC skaters: anyone floating, getting stuck or riding backwards?
- Carrying and pushing props (hold RB next to a bench or bin). Does it feel like Skate 3?
- Skate 3's menus (press Start).
- **New:** turn on **Career and Challenges** in the Opt-Ins, ride up to an own the spot and press D-pad Up, and give the Skate 101 tutorials a go. How close does it feel to the real thing?

**Found something?** Every game the launcher starts saves a session log (without the launcher, use `PLAY-WITH-LOG.bat`). Tell me which opt-ins were on and roughly when and where it happened, attach the log (and a short video if you can) to a [test report](https://github.com/Hailey-Ross/rusty-trucks/issues/new?title=Test%20report%3A%20), and I'll dig in. One problem per session makes it much easier to track down.

## History

Before this rewrite existed, **dumbad** spent more than two years reverse
engineering Skate 3 and building the tools needed to understand and work with
it. That meant countless hours digging through undocumented file formats,
animation data, and game interaction systems, then testing those discoveries
in the original game. Much of that work is collected in
[DumbadsSkate3ModdingTools](https://github.com/Ethanw05/DumbadsSkate3ModdingTools),
including tools for custom maps, meshes, collision, challenges, and DLC.

That research laid the groundwork for this project. Chasm later worked on a
recompilation and a custom renderer based on dumbad's earlier renderer work,
before moving into the Rust/Bevy rewrite. The rewrite's development time tells
only part of the story: the knowledge and tools it relies on took years of
work to establish.

## AI usage

AI coding tools were used to develop this rewrite, but none of it would have
been possible without dumbad's extraordinary effort to reverse engineer the
original game. The AI had years of hard-earned research and working tools to
build on. Describing the project as simply “AI rewriting Skate 3” leaves out
the work that made it possible in the first place.

AI helped turn that knowledge into a new implementation; it does not replace
credit for discovering how the game works. This is still a work in progress,
and using original assets or showing working tricks does not mean every
system behaves exactly like the original.

## Build

On Windows, install Rust with the MSVC toolchain and LLVM in its default
location. Run `BUILD.bat` to build, then `PLAY.bat` to launch the test world.
`PLAY.bat` opens your saved map (University by default); use the in-game menu to switch maps, or drag a `.skate` file onto `PLAY.bat`. An SDL3-compatible gamepad is required for gameplay;
Escape opens difficulty and graphics settings.

On Apple Silicon macOS and on x86_64 or aarch64 Linux, `nix develop` (Nix with
flakes enabled) provides Rust, the C/C++ compiler and linker, CMake for SDL3,
Python with the setup packages, extract-xiso, vgmstream, and the runtime libraries.
`flake.lock` pins these dependencies. On macOS it also supplies the Apple SDK:
engine builds use Nix's Clang and Mach-O linker, not the system Command Line
Tools or whichever SDK `xcrun` happens to select. Xcode-only profiling tools
remain optional system tools; run them outside this shell.

```sh
nix develop
skate-build
skate-setup path/to/Skate3.iso  # Once; also accepts an extracted default.xex
skate-run                     # Builds if needed, then launches installed assets
```

If assets are already installed, skip setup. `skate-setup` builds the engine
before conversion and refreshes an existing installation. `skate-assets` prints
the prepared asset directory: `SKATE3_ASSETS` takes precedence, then checkout
`assets/`, then the installation recorded in `data/installation.json`.
Relative overrides are resolved from the checkout root.
`skate-run` forwards game arguments, for example `skate-run --test-world`.
These commands also work from checkout subdirectories, or without entering an
interactive shell: `nix develop -c skate-run`.

Build and run inside the shell: the game needs its Vulkan libraries and dynamic
Rust/Bevy libraries. Linux still requires a working graphical session and GPU
driver on the host. On macOS the renderer runs Vulkan through MoltenVK with
non-bindless materials; the shell disables MoltenVK fast-math, which otherwise
corrupts customiser skaters. No devenv, direnv, or system SDK override is needed.

On Linux, `skate-install` (inside the shell) builds a release package and links
it as `skate-play` in the checkout. Run `./skate-play/bin/skate-play [args]` from
the checkout root without `nix develop`: it resolves assets like `skate-assets`
and keeps mods beside the installation's settings (`SKATE3_MODS` overrides).
The link is a Nix GC root, so garbage collection keeps the game and its runtime
libraries but not the build toolchain; the GPU driver comes from the host.
Rerun `skate-install` after pulling changes.

When switching from a different compiler, an existing SDL CMake cache may reject
the change. Run `cargo clean -p sdl3-sys` once inside the shell, then `skate-build`;
the rest of the Rust build cache can stay.

Development builds use a prepared asset set in `assets/private/` or the
installed asset directory. `scripts/Build-Release.ps1` builds the portable Windows
package and requires Python 3.13. GitHub Actions builds `main` automatically;
numbered releases are published separately.

### Linux

Requires Rust, clang, cmake, patchelf, Python 3 with numpy and Pillow, and a
Vulkan driver. Run `./BUILD.sh` (`--dev` for the debug profile), then
`./SETUP.sh /path/to/skate3.iso` once (or pass `default.xex` from an extracted
game folder) to prepare the assets in `data/`, then `./PLAY.sh`. Setup runs
headless and downloads the Linux builds of the same pinned extract-xiso and
vgmstream releases. The character importer, updater and Steam relay remain
Windows-only.

Custom animations and climbing support remain available, but no custom clips
are shipped. The included format-demo map is original procedural content.

Implementation notes are in [`docs/`](docs/). Patched Bevy dependencies and
their licenses are in [`vendor/`](vendor/). This is an unofficial project,
not affiliated with EA.

## Advanced diagnostics

Windows builds support opt-in [performance timeline capture](docs/performance-tracing.md)
through the `--trace` CLI option, including optional GPU pass diagnostics.

## License

Copyright (c) 2026 dumbad and the Skate 3 Rust Engine contributors.
Unless otherwise noted, this project's original code is licensed under the
[GNU General Public License version 3 only](LICENSE) (`GPL-3.0-only`).
You may use, modify, and distribute it, including commercially. If you distribute
a modified version or a binary of the covered software, you must also make its
corresponding source available under GPLv3 and preserve the required notices.

Third-party code retains its existing licenses and copyright notices, including
the vendored Bevy crates and tooling under `tools/vendor/`. This license does
not grant rights to Electronic Arts' game code, data, assets, or trademarks,
or to content supplied by other map and mod authors.
