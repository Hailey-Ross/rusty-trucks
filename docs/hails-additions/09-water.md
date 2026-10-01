# Water

Branch: `gameplay/water`. Status: **water bail and animated water confirmed in play. Splash/ripple effects not implemented.**

## Problem

Water does nothing in play: the skater rides over it or falls through it as if it
were ordinary ground or empty space. The retail game has water-specific behaviour
(the `IsInWater` motion-graph condition, the water ragdoll profile, and the
"special surface" path in the wipeout state).

## Evidence so far

### 1. Water is a retail collision surface type, and it is in the converted maps

Retail collision units carry a 16-bit surface ID. The surface **type** is
`(surface >> 7) & 31`; the engine already treats type 12 as water:

- `crates/skate-core/src/physics/board_ground.rs` — the board sets collision flag
  bit 25 (`Body872`) and records the contact height (`Body864`,
  `surface_twelve_height`) when any part touches type 12.
- `crates/skate-game/src/physics/wipeout_states/prediction.rs` — the wipeout
  trajectory query tests `surface & 0xF80 == 0x600` (type 12).

New tool: `crates/skate-data/examples/water_surfaces.rs` lists the surface types
in each map's embedded RWCM collision and details the type-12 triangles.

```
cargo run --locked --release -p skate-data --example water_surfaces -- data/installations/<id>/maps/*.skate
```

Results (installation `c82bd63f…`):

| Map | Water triangles | Surface ID | Heights | Notes |
|---|---|---|---|---|
| DownTown | 355 (330 flat, all one-sided) | 1591 only | 0.4 m … 48.3 m, many levels | fountains/pools spread over 26 tiles |
| University | 322 (all flat, all one-sided) | 1591 only | 296 at 217.9 m, rest 67.9–71.0 m | reservoir plus smaller pools |
| Industrial, all parks | 0 | — | — | — |

So water collision exists only in DownTown and University. Every collision
stream is already converted (cities use their `cSim_*_high` tiles; parks use
`cSim_Global`), and the volume filter from change 5 cannot drop water, because
water triangles carry a surface ID.

Render materials agree: `water.alpha` / `water.flowingalpha` appear only in
DownTown and `water.default` / `water.flowing` only in University. Industrial's
harbour/sea is only the backdrop `ocean.reflection` mesh
(`assets/private/native-backdrops/Industrial.skate`), with no water collision
under it; the harbour bed is ordinary collision at about y = -3 (see change 4).

### 2. The skater never learns it is in water (the missing link)

The skater-side water signal comes from the collision output:

- `CollisionOutputFields.flag_3481` → `Processed.flags_2488` bit 30
  (`0x4000_0000`), and
- `CollisionOutputFields.scalar_28` → `Processed.collision_scalar_2924`
  (water height)

(`crates/skate-core/src/player/input_phase/publication.rs:253-254`). The wipeout
state reads both (`wipeout_states/lifecycle.rs:81-82`, `update.rs:24-26`) to
enter the "special surface" path (ragdoll profile 10, `below_surface`, the
`IsInWater` motion-graph condition).

**Nothing in the codebase ever writes `flag_3481` or `scalar_28`.** They stay
at their defaults (0), so the water path can never start, even on DownTown and
University where the data exists.

The board already computes the matching values (`collision_flags` bit 25 and
`surface_twelve_height`), and the board manager (`offboard/board_manager/runtime.rs`
`surface()`) and the player state (`player_state/publication.rs`) already read bit 25.
The likely retail behaviour is that the physics output fill copies the board's
`Body872` bit 25 → `Collision+3481` and `Body864` → `Collision+28`. **This
is a hypothesis; the retail write site has not been found yet.**

### 3. The skater's own contacts also classify water

`SkeletonCollision` (82BD4A30, `crates/skate-core/src/physics/skeleton_body/collision_update.rs`)
sets `flags.material_12` and `material_12_height` when any body part touches
type 12. Its flags sit at native 4079/4080/4081 (materials 10/11/12); the
collision output's 3479/3480/3481 use the same layout 600 bytes lower, and
3479/3480 are already the material-10/11 results. This makes the skater
body the most likely retail source of Collision+3481/+28.

### 4. Industrial's sea

The user remembers that falling into Industrial's sea respawned the player in
retail, and suggests the sea may simply be out of reach in retail. Industrial's
harbour floor uses ordinary surfaces (concrete and so on) and its sea has no
collision of any kind on the disc (the proxy archives are visual only), so no
data-driven water detection is possible there. Not handled by this change.

## Change

`PlayerInputRuntime::publish_water` (`crates/skate-game/src/physics/player_input/mod.rs`),
called from `frame.rs` right after `publish_board` (after the packet reset and
the skeleton contact feedback for the frame):

- skater body touching type 12 → `flag_3481 = 1`, `scalar_28 = material_12_height`;
- otherwise board touching type 12 (`collision_flags` bit 25) → `flag_3481 = 1`,
  `scalar_28 = surface_twelve_height`;
- otherwise both stay at their reset values (0).

Project choice: retail's writer is unconfirmed; the skater body wins over the
board because the wipeout water path simulates the body. Water triangles stay
solid one-sided collision, as before.

### Water bail (added after the first in-play test)

First in-play test (University fountain basin): the skater landed on the water
and rode on it; no bail. Expected: the published flag only feeds the wipeout
state, and nothing starts a bail on water. I searched every recovered bail check
(`skate-core/src/player/wipeout/`, the state selector, all motion-graph conditions
in the stock state XML) and found no water/type-12 trigger. The retail
trigger is in code that hasn't been recovered, and no decrypted executable is
available.

`physics/wipeout.rs` `check_after_physics` now requests a bail (request slot 33,
`WATER_BAIL_REASON`, otherwise unused and not a runout reason) when the board
(`collision_flags` bit 25) or the skater body (`flags.material_12`) touches
water, unless the skater is already in WipeoutGround, Teleporting or Sleeping.
It then takes the ordinary path: state flag 65 → `PhysicsWantsWipeOut` →
WipeoutGround, where `Collision+3481` starts the water ragdoll and the
respawn timer. Logged as `WATER_BAIL tick=… state=…`. **Project choice**, not
recovered retail behaviour.

Not changed: water stays solid. The water buoyancy code
(`wipeout_state::body::special_surface`, which pushes parts up when below
height + 0.1) suggests that in retail the body sinks slightly into water, so
water may not be solid for the body in retail. Left alone until play shows
whether that matters.

### Water rendering: extract the ocean animation table from default.xex

Water (family 33) and ocean (family 31) shaders need `assets/private/ocean-pca.json`,
which nothing in setup created (see open question 5), so 52 water/ocean materials
fell back to static shading (black for `ocean.default`, which has no diffuse).

- `crates/skate-data/src/xex/`: XEX2 unpacker (retail AES-128 file-key and CBC
  payload decryption, "normal" LZX or "basic" decompression) producing the
  mapped base image. No new dependencies; AES is checked against FIPS-197 and
  NIST SP 800-38A vectors. The owned disc `default.xex` (normal encryption,
  LZX) unpacks to an 18,022,400-byte image at 0x82000000 in about 0.2 s
  (sha256 `ce1e3ae5…`, not the TU3 image `ocean_pca.py` expects).
- `crates/skate-data/src/ocean_pca.rs`: locates cPCAWaterAnimationData's table
  the way the code addresses it (`lis` + D-form pairs forming two addresses
  0x168 apart within 32 instructions, data plausibility check), so it works for
  any build. The disc build has exactly one match: means at 0x82FC3978, weights
  at 0x82FC3AE0 (TU3: 0x830118D8 / 0x83011A40); frame 0 mean is
  (127.55, 251.83, 127.54), an "up" normal. JSON layout matches `ocean_pca.py`.
- `skate3rust --extract-ocean-pca <default.xex> <out.json>` (`main.rs`, runs
  before any game initialization; prints `OCEAN_PCA_READY`).
- Setup: `ocean_pca.convert` runs it in the `environment` group
  (`asset_exports.environment`, new `game_exe` argument; `versions.py` adds
  `ocean_pca.py` to the group). Failure is optional content, like other
  environment parts. Changing the environment recipe refreshes that group once.

### Water animation was frozen: per-frame shader state never reached the GPU

With the table in place the water still did not move (user). The shared
`FrameStateData` buffer (clock, PCA frame, shadow floor) was updated by
replacing the `ShaderStorageBuffer` asset's data every frame. Bevy 0.18's
`GpuShaderStorageBuffer::prepare_asset` then creates a new GPU buffer
(`create_buffer_with_data`), but each world material's bind group cloned the
first buffer at preparation and never sees the new one, so every world shader
read the initial state: clock 0, no PCA frame. Pre-existing upstream bug; it also
froze family 14 UV scrolling and the shadow floor colour.

`retail_render.rs`: the buffer is created once with `COPY_DST`; `FrameStateData`
is extracted to the render world (`ExtractResourcePlugin`) and
`write_frame_state` writes it in place with `RenderQueue::write_buffer` in
`RenderSystems::PrepareResources`. Confirmed in play: the water animates.

### Water time

The user found the water over-animated. The animation itself is time-based,
not frame-rate based, and the PCA frame rate matches retail (the disc build's
update routine advances one frame per 1/30 s, nearest frame, rows /255, which is
what we do). The same routine also keeps the water shader's time: +1/60 per call
(once per 30 Hz frame), restarting after 5. So retail water time runs at half
real-time speed and loops every 10 s. `retail_render::water_time` reproduces
that in `clock.z`, used only by the water path (families 30/33); family 14 keeps
real time (its retail time source is unconfirmed). Behaviour re-implemented,
not copied; the disassembly was reference only.

### University water looks darker than DownTown's (data, not a bug)

`RENDER_AT` shows the materials at each test spot: University fountain
`water.flowing` (family 30), University reservoir `ocean.default` (31),
DownTown fountain `water.alpha` (33, transparent). University's flowing water
uses a pure black base texture and a reflection cube authored nearly black
(both the DXT1 256x1536 and the B5G6R5 32x192 copies decode to about 9,12,15),
so it shows only sun highlights; DownTown's has a dark-blue base and a bright
sky cube. Texture decoding was checked and is correct. The reservoir's
reflection is scaled by olm² × fresnel × 0.2 (retail tuning) and reads very dark.
No retail reference was found to compare; the user accepted the look for now.

## Verification

- Unit test `physics::player_input::tests::water_contact_prefers_the_skater_body_height`.
- `cargo test --locked -p skate-game --release --bin skate3rust -- water_contact`: passes.
- Release build staged into `bin\`; `--test-world --check-assets` → `SKATE_ASSETS_READY`.
- In play, first build: rode on top of the water, no bail (led to the water bail above).
- In play, second build: bails on University fountain basin (F2), University reservoir (F3) and DownTown fountain (F4). Log shows `WATER_BAIL ... state=KnownAir` on each drop.
- After the frame-state fix: University no longer falls back for any water or
  ocean material (52 -> 26; the rest are adverts, transparent environment
  pieces etc., logged by shader since this change); `RETAIL_OCEAN: loaded 30
  authored PCA frames`; water animates in play on all three spots (user).
- `tools.asset_pipeline.test_ocean_pca`: runs `convert` through the real
  `spawn()` (text-mode pipes) for success and failure.
- `retail_render` tests pass (water time unit test included) except
  `sky_shader_validates`, a known upstream failure.
- Asset-backed wipeout tests (`--ignored wipeout`): 4 pass; `marker_reply_restores_on_foot`
  fails identically with these changes stashed (pre-existing).

## Open questions

1. Where does retail write `Collision+3481` / `+28`? The wiring above is the
   best match to the evidence; it is not confirmed against retail.
2. Water triangles are solid one-sided surfaces in our collision world. Does the
   retail board/body pass through type 12, or rest on it? (In play: does the
   skater stand on the University reservoir?)
3. Industrial's sea: retail respawned the player (user's memory), possibly
   out of reach in retail. No collision data exists for it; see section 4.
5. Water rendering (resolved, see above; kept for the record): static texture on the fountains, black on the
   University reservoir). Cause found: the log says "52 of 8546 world
   materials use an unsupported shader family and render as family 1". Water
   (family 33) and ocean (family 31) shaders require `assets/private/ocean-pca.json`
   (`MaterialTuning::supported`, `read_pca`). Nothing in setup (here or on
   upstream `60efdef`) creates that file; `tools/asset_pipeline/ocean_pca.py`
   needs a decrypted, decompressed TU3 executable image (hard-coded sha256 and
   table addresses 0x830118D8 / 0x83011A40). The disc `default.xex` is XEX2 with
   normal encryption and LZX compression, and setup never unpacks it. Fallback
   family 1 shows the static diffuse; `ocean.default` has no diffuse, hence black.
   This affects upstream users equally.
6. Splashes and impact ripples when the player hits water: not implemented.
   The executable has a `cWaterEffect` class; the engine has no particle/effect
   system for it yet.
4. What is surface type 13 (DownTown 88, University 17,790 triangles)? It may be
   related (shallow water or a splash surface) or something unrelated, such as grass.

## Files

- `crates/skate-data/examples/water_surfaces.rs` (new, diagnostic only: surface types, water heights, `WATER_POINTS`, `WATER_VIEW`, `RENDER_AT`, `MODEL_MATERIALS`, `TEXTURES`, `MATERIAL`; SKATE material ids are 1-based).
- `crates/skate-game/src/physics/player_input/mod.rs` (`publish_water`, unit test).
- `crates/skate-game/src/physics/frame.rs` (call site).
- `crates/skate-game/src/physics/wipeout.rs` (water bail request).
- `mods/water-test-teleport/` (dev-only test mod: F2/F3/F4 into the water, Shift+F2/F3/F4 view spots; not committed, not for upstream).
- `crates/skate-game/src/retail_render.rs` (frame state written in place, `water_time`, per-shader fallback log), `retail_world.wgsl` (water uses `clock.z`).
- `tools/asset_pipeline/test_ocean_pca.py`.
- `crates/skate-data/src/xex/{mod,aes,lzx}.rs`, `crates/skate-data/src/ocean_pca.rs`, `crates/skate-data/src/lib.rs`.
- `crates/skate-data/examples/xex_unpack.rs` (diagnostic: unpack + locate table).
- `crates/skate-game/src/main.rs` (`--extract-ocean-pca`).
- `tools/asset_pipeline/ocean_pca.py` (`convert`), `asset_exports.py`, `install.py`, `versions.py`.
