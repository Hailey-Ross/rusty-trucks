# 8. Setup performance: duplicate stream copies and customiser overlap (PR G)

Measured on a full setup (Windows 11, i7-14700KF, 28 threads, 3 map workers):
466 s total before this change: stock data 20 s, maps 183 s (DownTown alone
182 s), character customiser 226 s (run only after all maps), publish 24 s;
with total CPU use around 10%.

## 1. Skip decoding identical copies of streamed assets

### Problem

City districts are stored as a grid of streaming cells (`cPres_*`, `cTex_*`,
`cSim_*` `.xsf` files). The same asset (texture, model) is copied into every
cell that uses it. `skate3_streams.load_district_stream` called `read_sfil` on
every cell file, which **fully decoded every copy** (RefPack), and only then
compared it with the first copy and discarded it.

| District | Copies / unique (Pres) | Copies / unique (Tex) | RefPack data decoded / unique |
|---|---|---|---|
| DownTown | 13,991 / 3,627 | 10,510 / 985 | 1,548 MB / 347 MB |
| University | 7,736 / 2,665 | 6,427 / 689 | 1,332 MB / 285 MB |
| Industrial | 6,915 / 2,163 | 5,088 / 578 | 806 MB / 188 MB |

In source checkouts the native RefPack DLL is not built (only the release
script builds `target/native/refpack.dll`), so decoding runs in pure Python.

### Change

`tools/vendor/university/tools/vanilla_map_extraction/tools/skate3_streams.py`:
`read_sfil` takes an optional `known_copies` (asset id → BLAKE2b digest of the
first stored copy: asset header + stored payload). In a district load, a later
copy whose stored bytes have the same digest is not decoded and not returned.
A copy whose stored bytes differ is decoded and returned as before, so the
existing conflict check (`conflicting SFIL copies`) still sees it. Record
lookup, in-file duplicate detection and stride validation still run for every
copy. Without `known_copies` (all other callers) behaviour is unchanged.

Why identical: identical stored bytes (same section table, sizes and
compressed payload) decode to identical data, and the loader already kept the
first copy; the returned list is still built in ATOC order from the first
copies.

### Verification

- All 10 districts × Pres/Sim/Tex loaded with the original and the new module:
  every asset's record fields, source path, source offset, stored size and
  decoded bytes are identical.

  | District | Stream load before | After |
  |---|---|---|
  | DownTown | 107.8 s | 24.8 s |
  | University | 83.4 s | 19.1 s |
  | Industrial | 49.3 s | 12.9 s |
  | all 10 | 247.9 s | 64.1 s |

- Unit tests (`test_streams.py`): identical copies from another cell are
  skipped, differing copies are still decoded and returned, in-file duplicates
  are still rejected. (The fixture uses the uncompressed storage path; the
  compressed path is covered by the full-district comparison above.)

## 2. Run the character customiser beside the map stage

### Problem

`customiser_setup.install` passes the customiser to `install._install` as
`finalize`, which ran after the whole map stage. It reads only stock data
(core group), default-skater textures (character group) and the disc, all
prepared before maps, and writes only `assets/private/customisation`. It
launched no processes then (it has since gained worker processes and, from the third pass of
"Export speed pass", runs as its own process; `spawn()` now holds a lock around the DLL setting).

### Change

`tools/asset_pipeline/install.py`: when the map stage runs and memory allows
(`overlap_customiser`: available memory ≥ 2 GiB desktop reserve + 3 GiB per
map worker + one more 3 GiB slot), the customiser starts on a background thread
(`Background`) just before the map jobs and is joined where it used to run;
its exception is re-raised there, so failures behave as before. Otherwise it
runs at the end exactly as before. `map_workers` and the new gate share
`available_memory()`.

### Verification

- Unit tests: `Background.join` waits and re-raises; the memory gate needs one
  slot more than the map workers.
- Full setup: see below.

## 3. Faster clothing library and pro roster

### Problem

After section 2 the customiser was the critical path. Its two big stages were
single-threaded loops: the clothing library (126.5 s alone) decodes ~2,900
textures one at a time, and the pro roster (89.6 s) prepares 41 characters
one at a time. In a dev checkout both also used the pure-Python RefPack
decoder, because only `Build-Release.ps1` built `target/native/refpack.dll`.

### Change

- `scripts/Build.ps1`: after the cargo build, compiles
  `tools/asset_pipeline/refpack_native.rs` into `target/native/refpack.dll`
  with the same rustc flags as the release script, when the DLL is missing or
  older than its source. `fast_refpack.py` already prefers it.
- New `tools/asset_pipeline/customisation_workers.py`: worker entry
  (`--warm-textures`, `--roster`) plus `run_parallel`, which runs workers through
  `install.run(task(...))` so frozen builds re-enter correctly. The worker count
  is `customiser_workers()`: a quarter of the logical CPUs, from 1 to 8. If a
  worker fails, `run_parallel` prints the tail of that worker's log.
- Library (`customisation_library.py`): `requested_textures(catalog)` lists
  exactly the textures `prepare()` will decode, using the same skip rules. Before
  the serial loop, `warm_up()` decodes them into the `decoded/` cache, spread
  across worker processes in disjoint chunks. The loop itself and its
  first-writer-wins ordering are unchanged; it now finds the decoded files
  already present. Decoded files that the loop doesn't use, and source files
  the warm-up created for them, are deleted before `prune_unavailable`. That
  leaves the cache exactly as a serial run would.
- Roster (`native_roster.py`): `prepare_parallel()` splits the characters
  round-robin over worker processes (`prepare(only=..., report_path=...)`).
  Each character writes to its own outputs (`library/entries/<id>`,
  `work/<key>`). The reports are merged back into roster order and
  `work/report.json` is written as before. It falls back to a serial
  `prepare()` with one worker, or if any worker fails or a report is missing.
- Shared roster caches (`work/source`, `work/decoded`) are published by
  `_publish()`: write a pid-unique temporary file, then rename it into place
  without overwriting. On Windows, renaming over a file that another process
  is opening fails on one side: WinError 5 for the renamer, or Errno 13 for the
  reader's `open()`. The first version used `os.replace`, which overwrites. In
  the first real run, 4 of 7 workers died this way and the stage quietly fell
  back to serial. Now `os.rename` is used (it doesn't overwrite on Windows). An
  existing destination, or `FileExistsError`, means another worker already
  published identical bytes, so that file is kept. A `PermissionError` is
  retried briefly.

### Verification

- RefPack: the native and Python decoders give identical bytes for every
  compressed entry in `createacharacter.big` (4,229 entries; 34.0 s → 1.4 s)
  and `marquee.big` (11.2 s → 0.5 s), and for every district stream
  (DownTown 23.9 s → 1.8 s).
- Library warm-up, 7 workers: all 2,921 requested textures decoded in 25.6 s,
  byte-identical to the installed `decoded/` cache.
- Roster, 7 workers, no fallback: 18.5 s. All 41 characters are ready and each
  entry's `character.glb`, `recipe.json`, `manifest.json` and `preview.png` is
  byte-identical to the installed set. The report equals the installed one and
  no temporary files are left behind.
- Unit tests (`test_customisation_workers.py`):
  - merge order;
  - fallback to serial on a failed worker;
  - workers never exceed the number of characters;
  - `_publish` is atomic and keeps an existing file;
  - rename contention is tolerated and retried;
  - `requested_textures` follows the skip rules.
- Refresh setup (customiser only, rebuilt once):
  - library 126.5 → **51.2 s**, roster 89.6 → **15.9 s**;
  - customiser stage total 73 s, with no worker failures;
  - `check_customiser.py`: IDENTICAL, all 8,288 receipted files;
  - `check_maps.py` and `check_spawns.py`: all OK, unchanged.

### Cost

The customiser fingerprint covers `customis*.py` and `native_roster.py`, so an
existing install rebuilds its customiser once after this change (about 75 s on
this machine). The outputs are identical, so nothing the user can see changes.
There is no equivalence mechanism for the customiser fingerprint, and the
rebuild is cheap, so none was added.

## Export speed pass (2026-10-24, USER-ASKS 305)

User: "run an optimization pass over our scripts that extract assets with the goal of making it faster while not
introducing any new issues into the export process."

**Measured first.** A full fresh setup of the disc (no DLC) into a scratch base on K:, with the native RefPack DLL and
the dev game exe the launcher's refresh uses, every progress line timestamped (`.local/perf-bench/setup_bench.py`: also
a sha256 manifest of every installed file). Baseline 228.3 s. The stages before the maps ran one after another: the
HUD and menu movies 31 s, audio 59 s (of it the bank decode loop 36 s, the ambience beds 10.5 s, the region read
11 s), the living world 28 s; then maps beside the customiser 61 s, park layouts 17 s, the final validation 18 s.
Inside the menu movie export (cProfile): DXT decoding in pure Python 13 s, a pure-Python RefPack 9 s (the vendored UI
tools' copy, not the setup's native one), PNG zlib 5 s.

**Changes:**
- Audio: the sound banks and the ambience beds decode on `setup_budget.decode_workers()` threads (vgmstream processes;
  `SKATE_SETUP_DECODE_WORKERS`, default cpu/2, 2..6); each writes only its own files and the manifest is filled in the
  original order (`audio_export._in_order`). The beds' NumPy downmix stays one at a time (`_NUMPY` lock): with several
  threads in NumPy's BLAS at once Python crashed (access violation) in a first attempt.
- Menu movies: the vendored RefPack uses the setup's native decoder when it is built (`vendor/skate3_ui/refpack.py`,
  as `owned_game/refpack.py` already did); the DXT decoder works on all blocks at once in NumPy with the same integer
  arithmetic (`xenos._decode_dxt`; the old one is `_decode_dxt_reference`, test `test_menu_texture_decode`: 480 random
  cases over the three formats, odd sizes, three-colour and six-step-alpha blocks, byte-identical).
- The HUD / menu movie stage (its own process; reads only the disc and the stock collections, writes only its folders)
  runs beside character, environment, audio and the living world, joined before validation (`install.py` `hud_job`).
  In-process stages stay one at a time (the audio overlapping the living world is what crashed in NumPy).
- `install.spawn` holds a lock: the frozen build's `SetDllDirectoryW` switch around a child start is process-wide.

**Result:** 228.3 s to 189.5 s (-17%); everything before the maps 131 s to 75 s. The maps and the customiser
(beside them) are now the critical path.

**Identity:** the new export writes the same 18,868 files. Every file that differs from the baseline also differs
between two runs of the unchanged pipeline (a second baseline): 40 files that are not reproducible run to run
(map `.skate` files, park layouts and props, the customiser set id in paths, a few HUD / menu manifests that hash
their temporary sources). The other 18,828 are byte-identical, including all audio and every menu movie texture.

**Second pass (2026-10-24, USER-ASKS 308 / 309):**
- **One core budget** (user: "uses 75% of the cores leaving headroom for the user"): `setup_budget.cores()` = 75% of
  the CPUs the process may run on (`os.process_cpu_count`), `SKATE_SETUP_CORES` overrides. Every pool derives from it:
  map jobs up to 3 (half the budget, memory permitting), the customiser's worker processes a quarter (1..8), the
  threads inside a map job what is left over 3 jobs, audio decodes a quarter (they run beside the maps), archive
  extraction and receipt hashing up to 8 / 16. On the 28-CPU desktop: budget 21, customiser 5, 5 threads per map job.
- **The map jobs start right after the core stage** (with the movable-object catalog and the map starts): they read
  only the disc, the starts and the catalog and write only their own files, so they run beside the HUD, environment,
  audio and living world; each map is validated once the validator runs, as before. A failure in a later stage cancels
  the queued jobs and waits for the running ones. The hand props (they need the living world's tables) follow the
  living world as before; a hand prop failure no longer deletes the catalog under the running map jobs.
- **The living world runs in its own process** (`stage_job.py`, the same stage function) beside the audio; its
  progress lines reach the setup window, its log lines the setup log.
- **Archive extraction** (`BigArchive.extract_entries`) checks every entry first, in order, then reads, decompresses
  (native RefPack) and writes on threads; **receipt hashing** (`group_receipts.record`, 19 GB) runs on threads
  (20.7 s of the old run's end).
- **Result:** 189.5 s to 135.3 s (from the first baseline: 228.3 s, -41%). The maps now finish at 72 s; the customiser
  (library 57 s, roster 20 s; it starts once the validator runs, at 50 s) is the critical path, ending at 131 s.
- **Identity:** the same 18,868 files; the differing ones are the run-to-run set (maps, park layouts, props, the
  customiser set id, HUD / menu manifests) plus the customiser's `*-complete.json` records, whose `version` is the
  customiser's pipeline fingerprint (it covers `customisation_workers.py`, changed here) and `timings.json`.

**Third pass (2026-10-24, USER-ASKS 312):**
- **The customiser starts right after the character stage**, as its own process (`stage_job.py customiser`, the same
  `customiser_setup.prepare`), so its NumPy work never shares a process with the in-process environment and audio
  stages. It needs one more memory slot than beside the maps alone (`overlap_customiser(workers+1)`); without it the
  customiser starts beside the maps or after them, as before. A failure in a later stage waits for the customiser
  process before the stage is cleaned up. `customiser_setup.py` itself is unchanged (every `customis*.py` is in the
  customiser's fingerprint, so editing it would make every install redo the customiser): `install.py` recognises its
  finalize by module.
- **Result:** 125.2 s / 125.4 s (two runs of the unchanged pipeline, today's machine state) to 94.3 s (-25%). The
  customiser now runs 6.5 to about 88 s (library 51.4 s, roster 19.6 s, as before) and ends before the final validation;
  the stages before the maps take 4 s longer beside it (46.3 to 50.7 s).
- **Identity:** the same 18,868 files; the 41 files that differ from the first baseline are exactly the 41 that differ
  between the two baselines.

**Fourth pass (2026-10-24, USER-ASKS 314):**
- **The post-map exports run beside the map wait** (`install.py` `post_maps`): map names, teleporters, visual
  indicators and park layouts read only the disc, the collections and the packs (whose map BIGs `dlc.load` copied
  whole before the map jobs started; `extract_bigs` keeps a file whose size matches) and write only their own files.
  They run on a thread while the main thread waits on the map jobs and the validator (no NumPy there), joined before
  the map catalog takes their names and before the kept DLC maps are carried over. Only when the customiser runs as
  its own process: park layouts use NumPy, so beside the in-process customiser fallback they stay after the maps.
- **Result:** two runs of the unchanged pipeline 102.6 s / 107.4 s, the change 97.9 s / 96.1 s (about -8 s, -7.6%).
  The post-map exports now run 64 to 80 s, inside the map phase; the maps end later beside them (77 to 84 s) and the
  customiser's roster (it ends at 92 to 93 s) is the critical path again.
- **Identity:** the same 18,868 files; the 41 files that differ from the first baseline are exactly the 41 that differ
  between the two baselines. No DLC on the bench machine, so the pack paths were checked by reading only.

**Open:** the run-to-run differences are pre-existing (not caused here); making them reproducible would let the
identity check cover them too. The customiser is the critical path (library 53 to 58 s, roster 20 to 24 s with 5
worker processes beside the maps); when the in-process customiser fallback runs (low memory) it shares the process with
the post-map exports' NumPy as before this pass. `customisation_workers.customiser_workers`'s docstring still says
"a quarter" (the budget is in `setup_budget.py`); it is fixed with the next change that touches the customiser, since
editing any `customis*.py` changes the customiser's fingerprint.

**Customiser worker count (2026-10-24, USER-ASKS 315, not landed):** the library stage is a texture warm-up on worker
processes, then one serial loop converting the 480 models; the roster splits over the workers. With
`SKATE_SETUP_CUSTOMISER_WORKERS` on the unchanged pipeline (28-CPU desktop, default 5): 5 workers 84.7 / 86.3 s, 8
workers 77.6 s, 10 workers 79.4 s. A default of 40% of the budget (8 here) then measured 86.9 / 81.1 s against 83.7 /
89.3 s for the unchanged pipeline: within the noise, so it was not landed (output identical in both runs: the 41
differing files are the run-to-run set). The laptop measures the knob on slower hardware (second-pc-testing.md
"Setup speed test"). Next candidate: the library's serial model loop (parallel over the workers; it changes the
customiser fingerprint, so every install redoes the customiser once).

**Customiser library over the workers (2026-10-24, USER-ASKS 322):**
- **Where the time went** (timing probes in a bench copy only): the GLB conversion of the 480 models was about 12 s
  serial, but the bigger serial part after the texture warm-up was the material loop's derived textures (normal maps,
  roughness maps, diffuses with the alpha mask multiplied in): 10.8 s. The texture decode on the workers (2,921
  textures) is about 20 s, and about 10 s comes before the library loop.
- **Change** (`customisation_library.py`, `customisation_workers.py`): the model conversion is one `ModelConverter`
  used by `prepare()` and by a new `--warm-models` worker job, which shares the warm-up pool with `--warm-textures`
  (models first: 5 workers convert all 480 in 2.5 s). Then `--derive-textures` builds the derived textures the material
  loop will ask for (`derived_jobs` follows that loop's rules: first use of a material wins, Hair keeps a separate
  alpha, models without a GLB are skipped). `prepare()` still converts or derives whatever a worker left or failed
  (and records the error as before), and deletes any derived file its loop did not ask for, so a failure in a worker
  never changes the output. Workers publish through a temporary file and a rename. With one worker (low memory) nothing
  changes.
- **Result** (28-CPU desktop, 5 customiser workers): library 50.8 / 49.4 / 53.1 s -> 36.5 / 37.4 s (-27%); the roster
  now ends at 66 to 68 s instead of 77 to 82 s; whole setup 80.9 / 86.1 s (111.3 s cold first run) -> 73.4 / 76.8 s.
- **Identity:** the same 18,868 files in all five runs; against each baseline the only differing files are the 41 of
  the run-to-run set. Tests: `test_customisation_workers.py` (the warm-up split, the derived-texture plan).
- **Cost:** the customiser fingerprint changes, so every existing install redoes the customiser once on its next
  refresh.
- **Open:** the worker count stays a share of the machine (`setup_budget.customiser_workers`, a quarter of the core
  budget, 1..8; memory scale-with-machine); the laptop's worker-count runs decide whether the share changes. The
  docstring "a quarter" in `customisation_workers.py` matches `setup_budget.py` (the note above about it was stale).

## Fingerprints

The stream change touches a file in the shared parser set, changing the
`character`, `environment` and `maps` fingerprints without changing outputs
(proven above); old→new pairs are in `pipeline-equivalence.json`. The
customiser fingerprint and stage versions are unchanged. An installation built
from the committed code rebuilds nothing.

## Full setup result

Full fresh setup, same machine and source, before → after:

| Stage | Before (466 s total) | After (331 s total) |
|---|---|---|
| Stock data, HUD, skater, environment | 0–20 s | 0–19 s |
| Map stage (10 maps, 3 workers, validated) | 21–204 s (183 s) | 19–138 s (**119 s**) |
| Character customiser | 216–442 s (226 s, after maps) | 19–311 s (292 s, beside maps) |
| Publish | 442–466 s | 311–331 s |

**Total: 466 s → 331 s (−29%), 0 warnings.**

- Identical outputs: all 8,288 receipted customiser files match a snapshot of
  the previous (sequential) installation, with only the random set id
  differing in 3 JSON paths; all maps pass validation with unchanged
  collision counts and no warnings; all baked spawns unchanged.
- Contention: overlapped, the customiser's library stage took 193.9 s instead
  of 126.5 s alone (three map workers compete for disk and memory bandwidth,
  boost clocks drop, and the customiser thread shares the GIL with the setup
  main thread). It is now the critical path; the roster (89.6 s) and library
  are single-threaded loops and the next candidates.

## Files

- `tools/vendor/university/tools/vanilla_map_extraction/tools/skate3_streams.py`
- `tools/asset_pipeline/install.py` (`available_memory`, `overlap_customiser`,
  `Background`, map stage), `pipeline-equivalence.json`
- `tools/asset_pipeline/test_streams.py`, `test_map_validator.py`
- Section 3: `scripts/Build.ps1`, `tools/asset_pipeline/customisation_workers.py` (new),
  `customisation_library.py`, `native_roster.py`, `customiser_setup.py`,
  `test_customisation_workers.py` (new), `test_customiser_setup.py`

## Refresh Game Files: setup window opened only after a full file check (2026-10-15)

**Problem.** After Extras > Refresh Game Files the setup window took a while to open (user, 2026-10-15: "I clicked
it, it took a while and then it eventually opened").

**Root cause.** `tools/setup.py` ran `damaged()` (`asset_pipeline/group_receipts.py`) before creating its window, only
to list the groups in its label. `damaged()` re-reads and SHA-256 hashes every installed output file of every group
(audio, maps, props, stock data), minutes on a hard drive. `install()` then ran the same check again on the worker
thread, so the files were hashed twice.

**Change.** setup.py keeps only the version check before the window (0.75 s measured on the user's installation);
the file check runs once, inside `install()` on the worker thread, and `damaged(report=...)` shows
"Checking your installed game files: <group> (n of N)..." in the window while the progress bar runs (user, 2026-10-15:
"make it show a progress screen while it rechecks every game file it had installed"). The groups that get prepared
are unchanged; the label now says setup checks the installed files first.

**Files.** `tools/setup.py`, `tools/asset_pipeline/install.py`, `tools/asset_pipeline/group_receipts.py`,
`tools/test_setup_assets.py` (the progress message per group).

**Verification.** Setup unit tests (37) pass. The window itself has not been opened yet (the user was playing).

## Code reference: asset_check.rs

Detail moved out of the code comments of `crates/skate-game/src/asset_check.rs`; each comment there links its section here.

### Asset staleness check

Launch-time asset staleness check. It reads the converted files themselves on every start, not a stamp: every output setup recorded must still be there at its recorded size, and each group's files must hold what this build reads (format versions and content). A failing group is logged (`ASSET_CHECK <group>: <reason>`) and marked stale (`setup::mark_stale`), so the PC overlay offers Extras > Refresh Game Files, which makes setup rebuild exactly those groups. Runs on a background thread once the startup loading screen is gone, so its thousands of file reads do not compete with the start's own loading on an HDD.

## Code reference: splash.rs

Detail moved out of the code comments of `crates/skate-game/src/splash.rs`; each comment there links its section here.

### Startup splash

Startup splash (Windows): a small window of its own, up from the first moment of the process, with a skateboard (no skater) rolling across it and exactly what is loading written under it, the last finished steps dimmed below (like RPCS3's progress lines). It runs on its own thread with its own message loop, so it keeps animating while the game thread is busy (config, skater, physics, world, menus, the audio runtime). The game window is created hidden and shown when the start is done (`startup_loading`), and then the splash closes.

Steps are reported from any thread with [`stage`] (a step: "Loading physics") and [`detail`] (the item being read: "sound bank grind_concrete"). Not retail: retail boots into its own movies; this only covers our engine's start. `SKATE_NO_SPLASH=1` turns it off (scripted runs that want no extra window).

## Code reference: startup_loading.rs

Detail moved out of the code comments of `crates/skate-game/src/startup_loading.rs`; each comment there links its section here.

### Startup loading phase

The start's loading phase behind the splash (`splash.rs`): the game window is created hidden and stays hidden until every hold is released, gameplay waits meanwhile (virtual time paused, as the menu pauses it, so the game starts in the same order as before), then the window is shown and brought forward and the splash closes.

The slowest hold is the native audio runtime's first start (~40 s on an HDD with a cold file cache; `game_audio::native::begin_launch` builds it on its own thread). Engine systems and mods hold the phase with [`StartupLoading::hold`] / [`StartupLoading::release`] (a label each, shown as the splash's step) and report the item being read with [`detail`].
