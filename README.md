<p align="center">
  <img src="docs/images/skating-crab.png" alt="Rust crab riding a skateboard" width="480">
</p>

# Skate 3 Rust Engine

A Rust and Bevy skating project built from Skate 3 reverse-engineering research.
Includes skating, tricks, grinds, offboard movement, difficulty settings and
`.skate` map support. Gameplay parity is still a work in progress.

## History

This rewrite builds on more than two years of Skate 3 reverse engineering and
modding work that began before the Rust project. Its development timeline
should not be mistaken for the time it took to understand the original game.

Ethan Wingfield, known as **dumbad**
([Ethanw05](https://github.com/Ethanw05)), developed the research and tooling
collected in
[DumbadsSkate3ModdingTools](https://github.com/Ethanw05/DumbadsSkate3ModdingTools).
That work established the foundation for understanding the game's file formats,
animation data, and game interaction systems used by the rewrite. It involved
examining the original game, decoding binary structures, building parsers and
exporters, and testing those discoveries against the game.

The tooling includes ArenaBuilder, collision PSG generation, clustered-mesh
serialization and compression, KD-tree construction, mesh and material data
builders, AI-path data, ChallengeEditor, and DLC-building tools. These are
concrete outputs of the earlier research, rather than discoveries that began
with the Rust rewrite.

Chasm later worked on a Skate 3 recompilation and a custom renderer based on
Ethan's earlier renderer work. The Rust/Bevy project followed that work,
bringing the accumulated research into a new implementation. Credit for
developing the rewrite and credit for the research that made it possible are
both part of this project's history.

This project is a reconstruction of Skate 3 systems in Rust and Bevy.
Recompilation of the original executable is a different approach. Loading
original assets and reproducing demonstrated behavior does not establish
complete equivalence with the original game; gameplay parity remains a work
in progress.

## AI usage

AI coding tools have been used in the broader research and development effort,
including work on tooling and the Rust implementation. AI assistance is part
of the development history and should be acknowledged alongside the people
who directed the work and supplied the underlying research.

The rewrite was not produced by giving an AI an unexplored game and having it
independently recover everything. It builds on the earlier reverse engineering,
format knowledge, tools, and experiments described above. The time spent
generating or adapting implementation code does not include the two-plus years
of work that established that foundation.

AI-generated code is not, by itself, evidence that a recovered format or
gameplay system is correct. Claims about accuracy need support from inspection
of the original game, reproducible tests, and behavioral comparisons.
Likewise, a successful demonstration should not be described as proof of
complete 1:1 parity.

When describing or reporting on this project, distinguish the original
reverse-engineering work, AI-assisted implementation, reuse of retail assets,
and the behavior actually verified in the rewrite. AI assistance does not
replace attribution to the people and projects whose work supplied the
necessary knowledge.

## Play

[Download Experimental](https://github.com/SK8-ENGINE/skate-3-rust-engine/releases/tag/experimental).
Successful `main` builds replace this prerelease. Choose **Latest** in Updates
for experimental updates; **Stable** is the default.

Extract the Windows release ZIP and run `skate3rust.exe`. Select your Skate 3
Xbox 360 ISO, or select `default.xex` in an extracted game folder. Keep its
`data` folder alongside it. Setup prepares the skater, animations and all disc maps, then
starts University. The original scoring and session-marker HUD assets are also
exported automatically during setup. No Blender, Python or Rust installation is needed.
ISO extraction needs internet access. The first conversion can take a while.

Use an XInput controller to play. Escape opens graphics, difficulty and map
settings. Maps can be switched without restarting the game.

**Skate 3 assets are not included.** Your converted files stay in
the `data` folder beside your executable. Each freshly unpacked copy runs its
own setup; it does not adopt another installation. In-place updates refresh
only changed asset groups.

## Build

Requires Windows, Rust with the MSVC toolchain, and LLVM installed in its default
location. Run `BUILD.bat` to build, then `PLAY.bat` to launch the test world.
`PLAY.bat` opens your saved map (University by default); use the in-game menu to switch maps, or drag a `.skate` file onto `PLAY.bat`. An XInput controller is required for gameplay;
Escape opens difficulty and graphics settings.

Development builds use a prepared asset set in `assets/private/` or the
installed asset directory. `scripts/Build-Release.ps1` builds the portable Windows
package and requires Python 3.13. GitHub Actions builds `main` automatically;
numbered releases are published separately.

Custom animations and climbing support remain available, but no custom clips
are shipped. The included format-demo map is original procedural content.

Implementation notes are in [`docs/`](docs/). Patched Bevy dependencies and
their licenses are in [`vendor/`](vendor/). This is an unofficial project,
not affiliated with EA.

## Advanced diagnostics

Windows builds support opt-in [performance timeline capture](docs/performance-tracing.md)
through the `--trace` CLI option, including optional GPU pass diagnostics.
