---
name: aems-port
description: Implement or extend the native Rust port of Skate 3's retail audio runtime - AEMS patch-program evaluator, voice graph (resample, filters, gain, pan, sends), MixMap mixer, granular rolling bed, Splice, player and world components - from specs, validated against oracles and the recomp. Use when starting or continuing that port, or replacing a measured audio table with the real mechanism.
---

# Native AEMS / RenderWare Audio port

The port lives on the audio branch (upstream PR #32 and follow-ups #36, #43, #44, #49): `crates/skate-audio`
(formats, evaluator, voice graph, mixer, runtime, `mixmap/`, `grain/`, `splice`, `bus/`, `player/`, `world/`) and
the game host `crates/skate-game/src/game_audio/` (`native.rs`, `player_audio.rs`, `grain_bed.rs`,
`world_sources.rs`, `npc_skaters.rs`). The published specs are in the branch's docs (`audio-specs/`): evaluator,
voice graph, MixMap, grain player, buses, world audio. Helper scripts: `tools/` next to this file.

## Rules
- **Your own code.** Upstream PR #4 / #1 and skate3recomp have no licence: read them to understand behaviour, then
  write the code from specs. Constants, offsets and opcode numbers are facts and may be used. CC0 sources (for
  example dbalatoni13/nfsmw, mitsevox/tw2004) may be used. Repos without a licence (for example the Burnout decomp)
  are reference only.
- **Credit** every project whose findings you use (name, author, link, licence, what it gave) in the docs and PR.
- **Retail parity, proven:** measure, don't guess. Every module is checked against golden data before it replaces a
  measured table. Never tune by ear; every change needs a measured difference and a mechanism. No A/B env switches
  for behaviour changes: prove the change with the e2e bench, then people listen in game.
- **Optimise only with proof** of identical output (skill `optimisation`).
- **Modular and moddable from the first line:** each subsystem behind a clear boundary; every sound resolved through
  a content layer (retail identity -> mod override -> retail); retail values come from setup data as defaults, not
  hard-coded constants; a Lua SDK surface (posting, globals, MixMap, event hooks, mod emitters; skill `make-mod`);
  cleanup on mod disable. Ask "can a mod reach this?" in every design and review.
- **Multiplayer-ready:** stable identities, serialisable events, deterministic ticks with seeded RNGs.
- **Headless only for agents:** cargo build/test, Python, file reads. In-game listening is done by people.
- No game code or data in the repo: banks, WAVs and image dumps come from each user's own install at setup time.

## Workflow
1. Extract the disc's banks once (a local script that pulls the `.abk` / `.csi` files from your install into a
   gitignored folder such as `.local/audio-re/aems-banks`; check S10A slot order = WAV order).
2. Change the crate; `cargo test -p skate-audio --release` (unit tests + data-gated disc tests such as the bank census
   and DSP oracle, which skip without local data).
3. **Oracles:** a local build of upstream PR #4 (its own licence terms: reference only, never committed) provides an
   evaluator golden (per-op trace + RNG state log), DSP vectors and an end-to-end player render. Known PR #4
   evaluator gaps to fix locally in the oracle first: op 5 (CallFunction) sample counters never advance, ops 19/20
   (n-ary min/max) and 38 (ControlClass) missing. Compare our `aems_replay` / `mixmap_golden` example outputs with
   the oracle's per script: identical / diff.
4. Whole-graph check: `examples/aems_render.rs` (RMS/peak per second, optional WAV) on real banks + WAVs.
5. MixMap: an independent reference evaluator written from the spec matched PR #4's port on 73,800 golden cells;
   port its semantics exactly.
6. Grain: unit tests hold the oracle's PICK/CAND/POS vectors; `examples/board_mix_probe.rs`,
   `examples/grain_bed_render.rs` (whole bed headless: starts/s, RMS, `--wav`).
7. Game: `cargo test -p skate-game --release --bin skate3rust -- game_audio` and `--no-run`.
8. Player components (`skate_audio::player`): pure `process` / `update` state machines returning
   Post/Redeliver/Release commands; read the retail poster/constructor/updater with `fn.sh <addr>` (skill
   `recomp-research`) and port the words exactly. Headless checks in `player_audio.rs` tests (`--nocapture`): voice
   gains through the real banks, wind level vs retail medians.
9. **End-to-end** (`tools/e2e/`): `scenarios.py` writes synthetic scenarios (or `--from-log LOG --cut A-B NAME` on a
   `SKATE_AUDIO_STATE_LOG` capture of real play); ours: `E2E_DIR=<absolute dir> cargo test -p skate-game --release
   --bin skate3rust -- --ignored e2e_render` (`E2E_FPS`, `E2E_TIMING=1`, `E2E_<PART>=0` to drop a part); then
   `compare.py`, `frame_levels.py` (folded dBFS + each voice group per frame), `render_diff.py A B`,
   `voices_summary.py`, `bank_rate.py`, `wet_level.py`; retail windows with `retail_windows.py SESSION`. The oracle is
   a reference, not ground truth: where it disagrees with retail, retail wins.
10. Retail bed data: `tools/grec_level.py <trace>` on GREC lines (skill `recomp-scripted-runs`): straight-roll level
    = A / (1 - max(|I|, Bk)) per km/h band, carve rows. Pooled medians include paused / braking / manual / grind
    frames; clean straight-roll frames first. About a third of GREC rows have empty players (off board/menus).
11. Vault classes and fields: `tools/vault-inspect/` of upstream PR #37 (`vault_layout.py CLASS` prints a class's
    field offsets from the disc schema, `vault_fields.py`, `find_field.py HASH...`).

## Specs: key facts
| Area | Key facts |
|---|---|
| Evaluator | ABKC/MOIR layouts; walk every 6 blocks (31.999998 ms), newest instance first; 40 opcodes (17/18 Mux/Demux, 19/20 n-ary min/max, 37 Function, 38 ControlClass, 39 SetGlobalVariable); one shared 6-word add-with-carry RNG (zero at boot, ops 7-9); sine table generated by formula; op-27 player block |
| Voice graph | 256-frame blocks at 48 kHz; linear 16.16 resampler, <= 4x; RBJ biquads with Q = 1, bypass outside 24 Hz ... 0.999 Nyquist; 64-sample gain de-click; Send ramp to 64/65; internal channel order L C R Ls Rs LFE; output clamps to +-1, no master gain |
| MixMap | 14 slot sections of A (input x curve -> mB) / B (distance x azimuth roll-off + Doppler) / F (AHR/ADSR ducks) / C (clamped sums) / E (output sums) / G (conversions); per-instance copies; volume = mB -> Q15 (0 mB -> 32730), pitch cents, filter 25000 * 2^(c/1200) Hz, raw azimuth = pan; Master.in1-4 must be 32767 |
| Grain player | seek table maps sample -> byte offset only; per truck two players A/B (B 0.1 behind, turning/downhill layer); read position = Bezier of speed/max speed (retail's factored form); 2 voices per player, non-repeating picks of 3-4 windows in a 1.6 s / 1.5 s search window; every 256-frame block; equal-power sqrt fades; grain every 0.3 s (0.5 s above 35 km/h); a drift > 0.05 cuts the grain |
| Prior art | nfsmw `saems.c` / `aemsdef.h` official names; Burnout decomp plug-in IDs match Skate's |

## Learned while porting
- Sample-group entry bytes 3..7 are per-channel azimuths (stereo 224/32 = -45/+45 deg, quad 224/32/160/96); byte 8
  is the first byte of the stream-offset word.
- The rebase and interface lists sit after the sample data on all banks: the runtime needs whole files.
- Retail's biquad associates its feed-forward sum by position in each group of 8; found by black-box fitting against
  the oracle (`fit_*` ignored tests). Stop fitting after two failed rounds.
- The game inputs matter as much as the evaluator: for the local player PlayerPhysics.in9 and in13 are 0;
  Master.in1-4 = 32767.
- Grain player recent list: a sentinel (-2, -1) plus up to 15 inserted windows; "full" = the 15th insert; a collapse
  keeps the last inserted window. Pick index = trunc(((r as f32 * 2^-31) * 0.5) * count), all f32. Its generator is
  the same add-with-carry as AEMS's, own instance, seeded with image constants.
- Class_Seams needs the boot utility `Start_up_Play_ctl` (`Common.abk`); without it every hit plays sample 0 of its
  block. Packet sweeps: post utilities first, trigger period >= 2.
- When a component plays what retail never does, sweep its packet words one at a time on the real bank before
  suspecting the evaluator.
- A graph module's class defaults (Sen0 gain, Pn21 constructor args) are in the image's plug-in descriptors.
- **First-trigger rule:** every component must sound identical on its first trigger (decode/preload before use);
  test first vs second trigger renders.
- `post` log lines are not audible sound (the MixMap can hold a posted voice at about 0): judge by rendered level.
- Missing setup data logs an `error!` and those sounds stay silent: re-run setup's `audio` group rather than adding
  fallbacks.
- Parallel agents on one crate: standalone copies of the crate with their own `[workspace]`, merged with
  `git merge-file MAIN BASE AGENT` against a pristine base copy.

## Frame-rate rule (console cadence)
Retail processes that run once per rendered frame target the shipped 360's ~30 fps cadence, not the recomp's
uncapped frame rate. Run them on a 30 Hz virtual cadence with interpolated inputs and prove the same output at
30/60/144/365 fps (`E2E_FPS`). On the console cadence so far: Class_Seams, the MixMap evaluation (dt 1/30), the
Jitter walk and the eEQChain clear, the body and deck posters (cooldowns in console evaluations), the owner's turn
and brake slews. Physics-side conditioners (landed latch, 4-frame holds) stay per physics step (60 Hz). Note the
recomp's frame rate whenever a recomp trace is used as a per-frame reference.

## Validation against retail
- Recomp traces (skills `recomp-scripted-runs`, `recomp-audio-trace`); filter per-player lines to the local rider
  (`local72`).
- Capture levels are in the recomp host's downmix space: compare relative levels.
- Tolerance: bit-exact where the spec says the behaviour is replay-verified; otherwise within 0.1 dB per block for
  whole-voice renders.
- Before trusting a "late" voice in a recomp session, list its audio-thread stalls.

## Diagnosing a listening report
- Find the moment in the play session's state log: state ids 100 ground, 103 takeoff/pop, 200/201 air, 300 wipeout,
  400-403 grind, 500-502 off board / carrying the board, 701/702 reset; cut windows with `scenarios.py --from-log
  --cut`.
- Render them headless and read `frame_levels.py`; render the "good" moments too as controls.
- Compare with retail frames (clean GREC/GAIN/SEND/PLAY sessions); fix only what the retail evidence supports.
- Example: riderless-board contacts while off board were audible as rolling + skid; retail reports no wheel contacts
  there, so the audio record now does the same (audio-only; physics untouched).

## Order of work (how the port was staged)
1. An engine-independent crate (`#![forbid(unsafe_code)]`, no Bevy, pinned deps): formats (ABKC, MOIR, SPLC, MXB,
   grains), evaluator, voice graph, mixer, MixMap, grain player.
2. Per-module unit tests plus golden vectors.
3. A Bevy host in `skate-game`: one device-paced stream.
4. Retire measured tables one at a time, each with regression-check section 2 plus a listening pass.
