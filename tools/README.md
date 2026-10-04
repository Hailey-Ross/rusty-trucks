# Tools

The top-level scripts here are the repository's own tooling: setup and asset preparation (`setup.py`,
`prepare_assets.py`, `asset_pipeline/`, `owned_game/`), the updater, release and build helpers, and their
tests. They are documented where they are used (the main README and the scripts' own help).

The folders below are standalone helper tools for development and research. Each has a `README.md`
with what it does, its inputs, usage, example output and requirements. They read your own copy of the
game; none of them contain game code or data. Default work folders are under `.local/` (gitignored).

| Folder | What it is for |
|---|---|
| [`audio-file-inspect/`](audio-file-inspect/README.md) | Readers for the audio formats: ABKC banks and MOIR projects, SPLC banks, `.ems` emitter files, `.grain` data; decode bank samples. |
| [`audio-e2e/`](audio-e2e/README.md) | Scripted scenarios for the headless audio render and analysis of the renders (diffs, levels, voices, bus share). |
| [`audio-bench/`](audio-bench/README.md) | Audio performance: hashed e2e bench runs, timing summaries, emitter bank memory. |
| [`recomp-trace/`](recomp-trace/README.md) | **For use with the Skate 3 recomp's research hooks** ([`research-hooks` branch](https://github.com/Hailey-Ross/skate3recomp/tree/research-hooks)): read and analyse trace sessions. Reference only; you build the recomp and set up the paths yourself. |

The general research and regression helpers (regression checks, setup equivalence, collision / world-stream / vault inspection, recomp code search) are in their own PR, #37.

See `docs/hails-additions/14-published-tools.md` for the background.
