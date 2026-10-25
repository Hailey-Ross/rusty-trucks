---
name: optimisation-pass
description: Run an optimisation pass over a new feature or change before it lands, so it follows the patterns the engine and setup already use (work off the game thread, loads behind the loading screen, sliced uploads, no per-frame allocations or re-parsing, setup work in parallel processes sized from the user's machine) and proves the same output. Use when implementing or finishing any feature that loads, converts, renders or runs every frame, when a feature adds setup / export work, or when the user asks to bring new code in line with the optimisations.
---

# Optimisation pass for new features

The checklist from the 2026-10-17 to 2026-10-24 passes (doc 25 map switch and loading sections, doc 08 "Export speed
pass"). Skill `optimisation` is the full method for a dedicated speed pass (proof tools, review checklist, audio);
this one is the shorter pass every new feature gets before it lands. Same bar: **same outcome, proven** (skill
`regression-check` §3b, memory optimise-without-regressions). A pass that cannot prove identity changes nothing.

## When
- After the feature works and has its tests, before the commit that lands it (or in the same batch).
- Any feature that: loads data, adds work to a map switch or start-up, runs per frame, adds a setup / export stage or
  converter, spawns threads or processes, or reads files the game already reads elsewhere.
- Skip for pure data or text changes. Say so in the doc instead of measuring nothing.

## Steps
1. **Find the cost before changing anything.** Engine: `SKATE_FRAME_LOG` + `tools/frame_log_summary.py` (skill
   `optimisation`, "Frame time"), the hitch line's render split, `system_timing`. Setup: skill `setup-bench` timing log;
   per-process CPU with temporary `report('PROBE ...')` lines in a snapshot copy only. Write the baseline numbers down
   with at least two runs (noise).
2. **Walk the checklist below** for the feature's code. Write down, per item, "applies / does not apply / done".
3. **Change one thing at a time**, prove identity (engine: tests + byte compares where outputs exist; setup:
   `setup-bench` hash manifest against a second run of the unchanged pipeline), re-measure with repeats.
4. **Check a smaller machine** when the change adds parallel work (below, "Machine shape").
5. **Document** in the feature's `docs/hails-additions/` entry: before / after numbers, identity proof, what was left
   and why. Update the PR description (memory pr-descriptions-current).

## Checklist: engine (Rust / Bevy)
- **Game thread does no blocking file I/O or decoding** after start-up. Read + parse + decode on a worker or the map
  loader thread; keep the state change at its old call site (doc 25 "Map switch: game data read on the loader thread",
  the emitter-bank prefetch in skill `optimisation`). Never delay the consumer: done (take), running (wait), else do it
  where it was.
- **Map switches and loads happen behind the loading screen**, not in the first frame of the new map (doc 25 "The
  living world loads behind the loading screen", "Map switch: the next map's ambience beds are read while it loads",
  "Map switch: sound banks read during the load").
- **Big GPU uploads are sliced** (64 MB per frame behind the paused world, doc 25 "Map switch: the render upload"),
  never one huge upload in one frame.
- **Nothing is re-uploaded or rebuilt every frame** when it has not changed (doc 25 "Car materials are no longer
  uploaded again every frame"): mark changed, rebuild once.
- **Parse once.** A table or file read by several systems is parsed once and shared (doc 25 "the ped data parses
  `tables.json` once"); no per-frame `serde` / string formatting / allocation in hot systems.
- **Real-time paths stay lock-light, allocation-free and log-free** (audio render: skill `optimisation`). Logging in
  hot paths is gated.
- **Worst case, not average:** check p99 / longest hitch and the map-switch frame, not just mean frame time.
- **Moddability and multiplayer are not traded away** for speed: data-driven values and stable ids stay (CLAUDE.md).

## Checklist: setup / export (Python)
- **Processes, not threads, for Python CPU work** (GIL; two threads in NumPy crashed BLAS). Threads only where the work
  releases the GIL (zlib, file I/O, hashing). New CPU-heavy stages run as their own process beside the others
  (`stage_job.py`), started as early as their inputs allow.
- **Every count comes from the machine** (memory scale-with-machine): workers and threads are a share of
  `setup_budget.cores()` (75% of the CPUs the process may use, less the load measured at start) and of free memory,
  never a number tuned on the desktop. New knobs get a `SKATE_SETUP_*` override and a line in the module docstring.
- **Shared work queues, not fixed splits:** batches claimed by exclusive file create (`customisation_workers.run_shared`)
  so late workers can join when cores free up (`setup_budget.customiser_limit`).
- **Cache per process what every batch needs** (archive index, parsed catalog, parser objects). The first shared-pool
  version re-parsed the clothing catalog per batch: 25.8 s of worker CPU instead of 12.3 s. Time the worker CPU, not
  only the wall clock.
- **Atomic outputs** (write `*.part` / temp suffix, rename; tolerate a racing writer), and clean leftovers of a killed
  run at the start. Outputs a worker made but the merge did not use are deleted.
- **Same output, merged in the original order**, so the result matches a serial run; keep a serial fallback when a
  worker fails.
- **Fingerprints:** a change inside a fingerprinted file makes every user's refresh redo that group. Prefer budget /
  scheduling code in unfingerprinted modules (`setup_budget.py`); say in the doc which groups a change re-runs.
- **Cross-stage reads:** before overlapping a stage, grep the other stages for its output folders.

## Machine shape (any change that adds parallel work)
- The desktop has 28 logical CPUs; the user's laptop 16 (8945HS). A change that wins on one can lose on the other, so
  measure both shapes before claiming a gain.
- Desktop as a laptop: `py -3.13 tools/with_affinity.py 0xFFFF <command>` (8 P-cores with hyperthreading; children
  inherit) plus the laptop's budget (`SKATE_SETUP_CORES=12` for snapshots older than f84b0e57, whose budget ignored the
  affinity). Python's `os.process_cpu_count()` ignores the affinity on Windows; count `GetProcessAffinityMask` bits.
- Laptop runs (skill `recomp-laptop-share` style handoff on the share, steps in `docs/hails-additions/second-pc-testing.md`):
  compare versions **in one sitting, alternating** (v1a v2a v3a v1b ...), plugged in, one power mode, screensaver off.
  Check the pre-work step time (setup: start to "Converting 10 maps") is the same in every run before comparing; one
  afternoon the laptop ran 35% slower as a whole and made a real gain look like a regression.

## Engine runs (before / after builds, both machines)
- Build the version before the feature and the one after it (one build at a time, `scripts/Build.ps1`, detach the
  checkout to the older commit and back) and copy each `bin` (exe, dlls, `manifest.json`, `built_commit.txt`; no pdbs)
  into a bench folder on K:.
- `tools/frame-time-run.ps1 -Bin <bench>\<label> -Install <install> [-SwitchTo DownTown]`: muted, minimised, trace-all
  frame log + `system_timing.log`; median / p90 / p99 / max of frames 100+; with `-SwitchTo` it covers a map switch
  behind the loading screen too. Alternate before / after, three runs each; average `system_timing.log` per system
  with `.local/tools/system-timing-sum.py` to see the feature's own systems.
- Laptop: `docs/hails-additions/second-pc-testing.md` "Engine frame-time test". The laptop runs `tools/keep-awake.ps1`
  first: its ASUS screensaver renders on the second screen when idle and spoiled a whole sitting once (2026-10-24).

## Rules
- No change without a before and after number with repeats; report the spread and drop disturbed runs openly.
- One test run at a time (memory one-test-at-a-time); no game windows without asking; build output on K:; delete bench
  folders when done and report the space.
- Product trade-offs (more memory for speed, a higher CPU baseline, more threads on the user's PC) are measured and
  then reported to the user, not decided silently.
