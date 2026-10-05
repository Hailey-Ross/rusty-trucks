# Audio Example

A small audio mod (off by default: enable it in the mod menu). Everything it plays is self-made
(`synthesize.py` regenerates the WAVs); no game audio is copied.

- `audio.json` (the content overlay, active only while the mod runs): adds a sample bank
  `EXAMPLE_chime` (three bell tones), a location set `example_chimes` that plays it every 4–8 s
  from a random direction, and a box region on the `format-demo` map (`maps/format-demo.skate`)
  that selects the set. Load the format-demo map to hear it.
- `main.lua`: subscribes to the game's audio events tagged `pop` and `land`, counts them on the
  HUD and layers a short click on each pop (setting "Click on pops"). The game's own pop sound
  is not changed.

Check it before shipping changes:

    cargo run -p skate-mods --example check_mod -- sdk/examples/audio-example
    cargo run -p skate-mods --example check_mod -- sdk/examples/audio-example --install <your assets folder>

The second form also lists entries the install does not have and conflicts with other audio mods
(`--with <package>`). See `docs/hails-additions/16-audio-modding.md` for every section of
`audio.json`, the runtime API and the limits.
