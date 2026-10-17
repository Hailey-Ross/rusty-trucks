# World shading parity: the retail 4-tap lightmap fetch and the fence render state

Branch `fix/fence-alpha` (off `main`). Follow-up to [32](32-transparent-fences.md) (upstream #58, merged
2026-10-09).

**Scope grew during the work.** This started as the remaining retail-parity items for the see-through fences
(doc 32 open questions: texture fetch swizzles, specular mask, render state, which is ported here too). Reading the fence program showed a
lightmap fetch that every lightmapped world program uses, not only the fence one, so this is now a world shading
parity fix.

## Problem

Baked lighting on world surfaces is sampled once at the lightmap UV. Retail takes four samples and averages them,
which gives slightly softer baked light and shadow edges. Our engine did this only for water (family 33).

## Root cause

`retail_world.wgsl` read the lightmap with one `sample_lightmap` call shared by every family; the 4-tap average
existed only inside the family 33 water branch.

## Evidence (retail programs)

From `data/big/shaders_final.big`, read with the Xenos shader microcode disassembler (instruction slots; fetch
offsets from the texture fetch instruction's offset fields, in texels):

- [code] `transparentenvironment_defaultPS` 23..26: four `tf3` (lightmap) fetches at offsets (+0.5, +0.5),
  (-0.5, +0.5), (-0.5, -0.5), (+0.5, -0.5); 28, 31, 33 sum them, 34 scales by 0.25 (literal), 37 squares.
- [code] The same four corner fetches of `tf3` are in every lightmapped environment program:
  `baseenvironment`, `defaultenvironment`, `transparentdefaultenvironment`, `alphatestdefaultenvironment`,
  `environmentdiffuse`, `decalenvironment`, `decal2environment`, `decalenvironment_tileable`,
  `decalenvironment_simple`, `decalenvironment_simple_tileable`, `baseenvironmentreflective`,
  `baseenvironmentreflective_simple`, `transparentbaseenvironmentreflective`, `transparentenvironment`,
  `transparentenvironmentreflective`, `advertisement`, `water`, `wateralpha`, `flowingwater`,
  `flowingwateralpha` (all `_defaultPS`).
- [code] `flowingwater_defaultPS` 60..63 fetch the four corners at one coordinate, 67..69 add them, 70 scales by
  0.25: the same average as `water_defaultPS` 96..99.
- [code] No corner fetches in `tree`, `treeanimate`, `ocean`, `oceanreflection`, `baseincandescent`,
  `scrollincandescent`, `transparentincandescent`, `trafficlight_one`, `trafficlight_two` or the
  `environmentpark*` programs: these keep one tap.

Fence program items that needed no change (checked, our port already matches):

- [code] `tf4` (diffuse) destination swizzle `0x688` = rgba as fetched; 9, 10, 14 square rgb; 53 writes
  `alpha * alpha`.
- [code] `tf5` (specular map) destination swizzle `0xfc1` = (y, x, masked, masked): the map's red channel is the
  specular mask (42 multiplies the highlight by it), green sets the power, `10 + 290 * green` (30: `290 * g`,
  36: `+ 10`). Our `masks = sample_specular_map(...).rgb` uses `masks.x` as the mask and `masks.y` for the
  power (`retail_world.wgsl`, the `flags & 16u` specular term), so the channels agree.
- [code] The highlight uses the shadowed lightmap's green (41) and is added after the alpha-scaled diffuse term
  (43), so it is not multiplied by alpha; our port does the same.

## Evidence (retail render state of the fence draws)

- [code] The `transparentenvironment` instance's technique setter `sub_82CEC170` matches the technique name hash
  (0x2EA8FB98 default, 0x935F85F9 skatepark) and picks entry 2 of the depth state table 0x8307DDA0, entry 27 of
  the blend state table 0x8307DE80 and entry 1 of the rasterizer table 0x830782DC (checked in the recomp code).
- [code] Those objects are built once in `sub_82CF5A10` (descriptor contents recovered by constant propagation of
  its stores; not re-derived by hand): depth state 2 = test on, LESSEQUAL, write on, stencil off; blend state 27 =
  SRC_ALPHA / INV_SRC_ALPHA / ADD for colour and alpha, colour write RGB only, alpha test on, GREATEREQUAL,
  reference 16 (read as 16/255, the convention under which the world alpha-test state's reference 30 matches our
  `ALPHA_REF = 30/255`), alpha to coverage off; rasterizer state 1 = the one-sided cull the opaque world
  techniques use.
- [code] The alpha test runs on the shader's output alpha, which for this program is diffuse alpha squared (slot
  53), so texels with diffuse alpha below about 0.25 are discarded.

## Evidence (retail `advertisement.default`)

Billboards (37 materials: DownTown 16, MegaPark 9, University 10, Industrial 2; diffuse and lightmap only, flags
0). They had no family: the converter stored 0, and the renderer drew them as family 1 (environment.default),
logging them as unsupported.

- [code] `advertisement_defaultPS`: diffuse squared; lightmap boxed over four corners and squared (19..30); `min`
  with the shadow plus (0.05, 0.09, 0.13) (31); `max` with the global constant `g_ViewDotLight.z` (c6.z) (32);
  times diffuse (33) and `m_params.y` (c10, slot 34); fog (35); then the reduced output curve (36..41), the same
  instruction sequence as `environmentdiffuse_defaultPS` (our family 8). No `kd`, no normal, specular, detail,
  macro or cube map; output alpha 0. Verified by hand in the disassembly.
- [code] Render state (technique setter `sub_82CAA0F8`, decoded from the recomp code): depth state 0, blend state 0,
  rasterizer state 1, the plain opaque world state.
- [code] `g_ViewDotLight` is one global float4 shared by every draw, bound by name hash 0xE552F1C3 in
  `sub_826DD6B8` to `*(0x83083C60)+0x44370` (verified by hand in the recomp code).
- [trace] Read in the recomp on Industrial (watch list, 2026-10-09): .y 0.02 and .z 0.4 constant, .w 0, .x
  changes with the view (0.32..0.59). The recomp is not retail, but these are the game's own constants.
- [trace + code] Writer: recomp before/after probes on the renderer's methods (`src/research/hooks_render.cpp`,
  four rounds) narrowed it to `sub_828012D0` (skate3_recomp.38.cpp:57749), reached from the per-frame update
  `sub_827FF7D0` through `sub_82800FF8`. It stores (`stvx` at view + 0x44370)
  `(bias + scale * dot(L, light), tree floor, light floor, 0)`; L is row 2 of the camera matrix the caller copies
  (`sub_827A0C70`), and the four inputs are looked up by hash from the VLT `rendering` row `default` (the same in
  every district): light (0.5, 0, -0.879), (bias, scale) (0.5, 0.2), tree floor 0.02, light floor 0.4.
- [trace] Checked: a recomp run logging the camera's view direction (listener record `*(0x830CFDD4)+32`) next to
  .x matches `0.5 + 0.2 * dot(view direction, light)` to four decimals; the opposite sign does not fit.
- [code] `tree_defaultPS` / `treeanimate_defaultPS` slots 6..7 / 9..10: `max(lightmap, g_ViewDotLight.y) *
  g_ViewDotLight.x`. Our trees used a fixed 0.3435 / 0.02 from an earlier reference capture.

## Evidence (retail `incandescent.transparent`)

Lit signs (9 materials on Industrial, `obj_SignsAds_*`: a `diffuse` binding and an `exposure` scalar of 0). They
had no family either and drew as family 1, which wrongly added the lightmap, shadow and `kd`, and wrote alpha 1.

- [code] `transparentincandescent_defaultPS` (verified by hand): one diffuse fetch (tf3, rgba); diffuse squared
  (3) times `m_params.y` (c3, slot 4); fog (5); the full output curve (6..13); output alpha = diffuse alpha (14).
  No lightmap, shadow or `kd`.
- [code] Technique setter `sub_82CED6D8` (verified by hand: depth entry 2, blend entry 24, rasterizer entry 1).
  Blend state 24 (descriptor decoded from the recomp code, not re-derived by hand): SRC_ALPHA / INV_SRC_ALPHA / ADD for colour and alpha,
  RGBA writes, no alpha test; depth state 2 writes depth; rasterizer 1 is one-sided.

## Evidence (retail traffic lights)

DownTown only: `trafficlight.one` (15 materials) and `trafficlight.two` (17), `diffuse` binding only. They had no
family and drew as family 1, with every lamp showing its "off" texels.

- [code] `trafficlight_one_defaultPS` and `trafficlight_two_defaultPS` (verified by hand): one diffuse fetch, NOT
  squared, times `m_params.y` (slot 3), fog (4), the full output curve (5..12), alpha out (13).
- [code] Vertex programs (decoded with a hand-set literal base, since our disassembler cannot read .vpo headers; not re-derived by hand): the
  only difference is the constant `g_TrafficLightsStatus_1` or `_2`. Each vertex reads lamp slot
  `floor(4 * uvA.z)` and uses its second UV set when that component is above 1.0.
- [data] Our converter already carries both: uvA.zw arrive as the decal UVs (`decal.x` = lamp slot), the second
  UV set as the lightmap UVs (`tools/asset_pipeline/map_writer.py:216-219`), so no converter or map change.
- [code] Technique setters `sub_82CE9BE8` / `sub_82CEA368` (verified by hand): depth entry 2, blend entry 26
  (opaque, RGB writes), rasterizer entry 0. Entry 0 is built in `sub_82CF5A10` (store into the table checked by
  main at `skate3_recomp.83.cpp:70585`); its cull word is 4 against the world's 5, so it does not cull (decoded by
  the recomp code).
- [code] The two status constants are bound by name hash (0xC8E04C37, 0xC8E04C34) in `sub_826DD6B8` to the
  renderer fields `*(0x83083C60)+0x443E0` / `+0x443F0` (from the recomp code).
- [trace] Read in the recomp (watch list, DownTown Aletown, 2026-10-09, `.local/research/traffic-light-status/`):
  each component is 0 or 2.0; slots 0 red, 1 amber, 2 and 3 green; a 17 s cycle: direction 1 green 7 s, amber
  1 s, all red 0.5 s, then direction 2 the same. This matches the `trafficlights` controller timing already in our
  living-world notes (0.5 / 8 / 0.5 / 7 / 1 s).

## Evidence (retail `animated.flag`)

The DownTown memorial flag (6 materials). It had no family and drew as family 1, static.

- [code] `vertexanimate_defaultPS` (verified by hand): lightmap (tf3, one tap) squared, times `g_ViewDotLight.x`
  (c0.x), times diffuse squared (5..8); fog; the full output curve; alpha = diffuse alpha (18). No shadow, no
  `m_params`.
- [code] `vertexanimate_defaultVS` (decoded with a hand-set literal base, not re-derived by hand; our disassembler
  cannot read .vpo headers): per axis `w * amplitude * sin((t - w) * frequency + phase)` with t =
  `g_fAnimationTime` (c7), weights w = (E2.z - 0.5, E2.w - 0.5, 1 - E2.z - E2.w) from the second TEXCOORD's zw,
  amplitudes c8.w / c9.w / c10.w, frequencies c9.xyz, phases c10.xyz.
- [data] `m_params` of the VLT row `material_animated` / `flag`: c8 (0, 1, 0, 0.1), c9 (10, 9, 5, 0.1), c10 (1, 6, 5,
  0.2).
- [code] Technique setter `sub_82CF1528` (decoded from the recomp code): depth 2, blend 1 (alpha test, no blending), rasterizer 0
  (no culling).
- [data] Its second TEXCOORD is four normalised shorts; our decoder kept only xy (the lightmap UVs).

Our engine before this change: `AlphaMode::Blend` made Bevy turn depth write off
(`vendor/bevy_pbr/src/render/mesh.rs`, the `BLEND_ALPHA` arm) and write alpha with Bevy's blend, and blended
materials had cutoff -1, so nothing was alpha-tested; the shader's test compared the raw diffuse alpha.

## Change

- `retail_material_bindings.wgsl`: new `sample_lightmap_box(slot, uv)`: four taps at the half-texel corners of
  the lightmap, averaged.
- `retail_world.wgsl`: families 1 to 8, 13 and 16 read the lightmap through it; the water branch (families 30
  and 33) uses it at its refracted lightmap UV. Trees (9, 10), proxy (11), incandescent (12, 14), ocean (31,
  32) and the unknown-shader fallback (0) keep the single tap.
- `retail_render.rs`: new render class `BlendedDepthWrite` for family 16 (whatever the material's stored alpha
  mode): blended, depth write on, colour writes RGB only, one-sided culling; cutoff `TRANSPARENT_ALPHA_REF` =
  16/255 (`class_and_cutoff`).
- `retail_world.wgsl`: for family 16 the alpha test compares the output alpha (diffuse alpha squared).
- Advertisements: new family 17 (`ADVERTISEMENT_FAMILY`). The converter maps `advertisement.default` to it
  (`retail_material.py`), packages that stored 0 are upgraded on load, and the shader gives it the box lightmap,
  shadow, `m_params`, `kd` = 1 and the reduced output curve. The light floor (slot 32) is `max(L, view_dot_light.z)` with the
  retail 0.4 as the default of a new `FrameStateData::view_dot_light` row (shared by every draw, like retail).
- `g_ViewDotLight`: `advance_view_dot_light` computes it every frame from the retail camera's forward axis and
  `ViewDotLightParams` (exported by `render_parameters.py` as `rendering.default` into `render-parameters.json`;
  the retail values are the fallback). Trees (families 9 and 10) now take their lightmap scale and floor from it,
  so their brightness follows the view direction as in retail.
- Billboard multiplier: `m_params.y` of the VLT `material_advertisement` row is 0.35 (0x3EB33333; the other world
  classes carry 1.0, `.local/research/world-m-params.md`, row verified by hand). `render_parameters.py` now
  exports the advertisement rows into `render-parameters.json`, and family 17's `surface.w` takes `m_params.y`
  from there (`material_multiplier`; retail 0.35 when an install predates the export). Before this our billboards
  were about 2.9 times too bright.
- Memorial flag: family 21 (`ANIMATED_FLAG_FAMILY`). The vendored extractor decodes the second TEXCOORD's zw
  (`decode_secondary_texcoord_zw`) into the flag's otherwise empty decal UV set; `render_parameters.py` exports the
  `animated.flag` rows; the world and depth vertex stages add the sway (`flag_sway`), the pixel stage the
  lightmap / `g_ViewDotLight.x` terms; class `CutoutTwoSided` at the world alpha reference. Installs that predate
  the export keep the flag on the fallback (the family needs its three rows).
- Videoscreens (`incandescent.videoscreen`, MaloofMoneyCup 7 and MegaPark 1 materials): `videoscreen_defaultPS` is
  byte-identical to `baseincandescent_defaultPS` (verified by hand), so they join family 12; the converter maps them,
  stored 0 is upgraded on load. Family 12 now applies `m_params.y` (`surface.w`) like retail (slot 4); its row
  comes from `render-parameters.json` (videoscreen 0.25, already exported; `incandescent.default` 1.0, unchanged).
  Render state (decoded from the recomp code): technique setter `sub_82CF1BE0`, depth 0 / blend 0 / raster 1, the same as
  baseincandescent (`sub_82CAED90`), so family 12's class fits. In retail the big park screens show a live view:
  checked on the PS3 version in RPCS3 (user, 2026-10-09): "Confirmed its the skaters view with no HUD". The recomp
  draws the static placeholder texture ("JUMBO TRON TEMP") instead, and so does this change; the live feed is not
  ported yet.
- Lit signs: new family 18 (`INCANDESCENT_TRANSPARENT_FAMILY`), converter mapping and on-load upgrade like 17;
  the shader takes the family 12 path (`lin = d`) plus `m_params` and alpha = diffuse alpha. New render class
  `BlendedDepthWriteRgba` (blend state 24: depth write, all channels written, no alpha test).
- Traffic lights: families 19 / 20 (`TRAFFIC_LIGHT_ONE_FAMILY` / `_TWO_`), converter mapping and on-load upgrade.
  The shader picks each vertex's UV set from `FrameStateData::traffic_lights[family - 19][floor(4 * color.x)]`
  and draws the unsquared diffuse. `TrafficLightCycle` (a resource holding the retail phase table, looped on
  game time) writes those rows every frame; a mod or the traffic controllers can replace the table or the clock.
- Both depth-writing classes blend alpha with SRC_ALPHA / INV_SRC_ALPHA like retail (Bevy's default blend uses
  ONE for the source alpha).

## Files

- `crates/skate-game/src/retail_material_bindings.wgsl` (`sample_lightmap_box`, `flag_sway`, frame-state rows)
- `crates/skate-game/src/retail_world.wgsl` (4-tap lightmap, families 12 and 16 to 21, tree terms, flag sway)
- `crates/skate-game/src/retail_depth.wgsl` (flag sway in the depth prepass)
- `crates/skate-game/src/retail_render.rs` (families, render classes and cutoffs, `material_multiplier`,
  `ViewDotLightParams`, `TrafficLightCycle`, frame-state rows, tests)
- `crates/skate-game/src/retail_character.rs` (test fixture: the new frame-state rows)
- `crates/skate-game/src/skate_world.rs` (`SKATE_RENDER_READY` counts the new classes)
- `tools/asset_pipeline/retail_material.py`, `tools/asset_pipeline/test_environment.py` (families)
- `tools/asset_pipeline/render_parameters.py` (advertisement, animated.flag and `rendering.default` rows)
- `tools/vendor/university/tools/vanilla_map_extraction/tools/retail_lightmap_uv.py`,
  `.../prepare_hawaiian_dream.py` (the flag's sway weights)

## Setup impact

`retail_material.py` (families 12 and 16 to 21; also in the `maps` group) and `render_parameters.py` (billboard,
flag and `rendering.default` rows) are in the `environment` setup group, and the extractor change for the flag's
sway weights is in the vendored parsers shared by the `character`, `environment` and `maps` groups
(`tools/asset_pipeline/versions.py`). So the next setup refresh rebuilds those groups, characters included.
Existing installs look the same before the refresh: stored family 0 is upgraded on load, the billboard
multiplier and `g_ViewDotLight` fall back to the retail values, and the flag stays on the fallback family until
its rows and weights are in the data.

## Moddability

- Families follow the material's shader name (map data, mapped in `tools/asset_pipeline/retail_material.py`), so
  a mod or custom map material that names one of these shaders gets the retail shading, render state and lightmap
  filtering with its own textures. The fixed numbers in the shaders (tap offsets, weights, the 16/255 fence
  reference, the output curves) are the retail programs' and states' values.
- Material multipliers (`m_params`), the flag's sway rows and the `g_ViewDotLight` inputs come from the setup data
  (`render-parameters.json`); a mod can override them there, and the retail values are the fallback.
- `g_ViewDotLight` and the traffic light status live in the shared frame state; `ViewDotLightParams` and
  `TrafficLightCycle` are resources a mod or another system (the traffic controllers later) can replace.

## Retail parity and tests

1:1 with the retail programs and render states listed above; values the game sets at run time
(`g_ViewDotLight`, the traffic light status rows) were read from the running game in the recomp and checked
against the code.

Tests (`retail_render::tests`): `lightmapped_world_families_average_four_lightmap_taps`,
`classes_split_on_culling_blending_and_alpha_testing`, `transparent_environment_shader_matches_retail`,
`advertisement_takes_its_own_family`, `incandescent_transparent_takes_its_own_family`,
`traffic_lights_take_their_own_families`, `traffic_light_cycle_matches_the_recomp_reading`,
`view_dot_light_follows_the_camera_like_retail`, `animated_flag_takes_its_own_family`. The naga validation test
`world_shader_validates_under_non_uniform_material_slots` (`retail_shader_tests.rs`) covers the edited shaders.
Python: `tools/asset_pipeline/test_environment.py` checks each shader's family.

## Verification

- `cargo test -p skate-game --locked` (2026-10-09, on `main` 2e166977 plus this change, families 17 and 18 and
  the frame-state rows, trees and the memorial flag included): 492 passed, 1 failed, 179 ignored. The failure is the known pre-existing upstream one,
  `setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`
  ([PULL-REQUESTS](PULL-REQUESTS.md)). The new and changed tests pass, and
  `retail_render::shader_tests::world_shader_validates_under_non_uniform_material_slots` validates the edited
  shader.
- In game (user, 2026-10-09, Industrial Ghetto Spot, build with the 4-tap lightmap, fence render state and
  family 17): "after running around for a moment it looks much better on the fence. i can't really tell on the
  baked lighting but im probably just missing it." (The 4-tap average is a sub-texel softening, so a small
  difference is expected.)
- In game (user, 2026-10-09, DownTown Aletown, build with traffic lights 19 / 20 and the retail light cycle):
  "Yep the lights render now yay."
- Not done: the cross-map regression check of the converter side (spawns, collision, map validation) needs a
  setup refresh, which was skipped (a refresh from this branch also rebuilds other setup groups of the shared
  install). Collision and spawns are not touched by this change; the converter only assigns families and exports
  setup rows. No side-by-side recomp shot was taken.

## Open questions

- Pass and sort order of the fence draws in retail is not traced yet (two recomp trace runs did not capture a
  fence apply; the bind slot is `sub_82CA5C60`, the per-frame renderer update `sub_827FF7D0`);
  Bevy sorts them back to front in its transparent phase. With depth write and alpha test on, order matters
  less.
- Memorial flag: not checked in game yet (needs a setup refresh for the weights and rows). `g_fAnimationTime`
  counts game seconds like our `clock.x` (read in the recomp, 2026-10-09: about 0.82 per wall
  second while the recomp ran below full speed, not frames); the vertex program decode was not re-derived by
  hand. The vendored parser change moves the fingerprint of every setup group that includes the parsers
  (`versions.py` PARSERS: character, environment, maps), so a refresh re-extracts characters too.
- `model_default` (University, 1 material) is EA's placeholder for a missing material ("Pipeline Missing
  Material", no textures), not a retail shader; nothing to port. No other world shader is left on the fallback.
- Traffic lights (drawn two-sided, class `OpaqueTwoSided`): the retail cycle's start phase and whether
  each crossing has its own offset are not known (the recomp shows one shared pair of rows for the whole map).
  Once the living-world traffic controllers land, they should drive `TrafficLightCycle` so cars and lamps agree.
