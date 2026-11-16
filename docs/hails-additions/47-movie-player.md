# 47. Movie player

**Problem.** Skate 3 plays its boot and career movies (EA intro, the new career intro, Coach Frank, the park flyby,
the camera previews) through its FE 24 movie player. The engine had no movie support: the retail startup and the
career welcome flow (PR 68 "Planned") need it.

**Retail (research b898, b921, b924, b926, b931, b955).**
- The disc's movies are EA MV containers (`data/movies/<name>_english_ntsc.vp6`): `MVhd` (`VP60`, width, height, frame
  count, largest frame, rate / scale, e.g. 299699 / 10000 = 29.97 fps), per language `SH` / `SC`, then per frame one
  `MV0K` / `MV0F` (VP6) and one `SD<lang>` audio block per language (EA-XA v2, 6 channels, 48 kHz) [data b924].
- Frame pick: target frame = (offset + clock ms) x fps x 0.001; late frames are dropped, the newest due frame shows;
  no new frame for over 0.25 s counts as stalled [code 82A92DC8, b955 §3].
- Skip: A or Start shows the prompt, A again skips, B hides it; skippable rows after 5000 ms; ea_intro and a few rows
  never [code 825E0680 / 8260B240, b955 §1].

**Change (step 1, 2026-11-15).**
- `vendor/nihav-vp6`: the VP6 decoder (ruffle-rs/nihav-vp6 12d0bdf1, MIT, pure Rust, every OS) with the two fixes that
  make all 11 disc movies byte-identical to FFmpeg (`vendor/README.md` "nihav-vp6").
- `crates/skate-movie`: the decode side, no rendering or audio. `container::Reader` streams the EA chunks (one chunk in
  memory), `FrameDecoder` is the replaceable decoder trait (a mod or platform can bring its own), `Vp6Decoder` returns
  top-down YUV 4:2:0 planes.

**Planned.** Setup export of the movies and their audio per language (vgmstream decodes EA-XA, b924); a worker thread
that decodes ahead; frames on screen as three R8 planes with a YUV shader (the `video_screens.rs` render-world path,
b926 §2); a streaming 6-channel audio source folded to stereo with the retail output table, its sample count as the
movie clock (b926 §3); the FE 24 screen with the retail skip rules, subtitles (`.sub`, b955 §5) and the fe_movie rows
as data; Lua `sdk.movies.play(key, on_finished)` / `skip()`; the boot (ea_intro) and career hooks (b926 §1).

**Verification.** `skate-movie` tests: the container reads its header and chunks in order, rewinds, and refuses a file
without `VP60` or with a cut chunk; a non-VP6 packet is a header error; data-gated `a_disc_movie_decodes_like_ffmpeg`
(`SKATE_MOVIE_CHECK=<file.vp6>;<sha256>`) compares a disc movie's frames with the FFmpeg hash.

**Open questions.** The 4th audio subsong (b898); the two decoder fixes are offered upstream as ruffle-rs/nihav-vp6#4 (2026-11-15); drop the vendored copy once they land.
