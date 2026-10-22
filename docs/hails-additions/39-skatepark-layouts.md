# 39. skate.Park layouts: the Full / Basic worlds place their pieces

Part of PR #52 (living world, item 9 of its list; asked by the user 2026-10-20: "i don't believe our skate.park entries
load the maps properly. for instance the Stamium - Full has a bunch of placed objects to build it").

## Problem

Skate 3's skate.Park worlds share one district stream per park (MegaPark, StartPark, IndustrialSkatePark,
DownTownSkatePark and the DLC lots) and differ by a layout of placed pieces: "Stadium - Full" builds the stadium course
from 450 pieces, "PCU Training - Basic" from 251. Our maps were only the streams' shells: no world loaded its pieces,
and the Challenge Map could not reach the Full / Empty rows at all (setup kept only locations with their own artwork).

## What retail does

Research b424, b425, b429, b430, b431, b434 (`.local/research/`; addresses TU3):

- A world row names its layout in field `Hash_79B248E8469DC5DA` (`skatepark\MegaPark_Fullpark.park`, disc
  `miscload.big`; the DLC packs carry their own) [data]. The stream's own row names the Empty layout (0 pieces).
- The `.park` file: version, four block lengths, origin world hash, then static pieces (36-byte records in the
  tutorials, 48-byte with an RGB tint elsewhere; blocks are allocated larger, so a reader skips by length), movable
  pieces (40-byte records), and two id lists [data, b425]. Piece ids are `<mod>` rows of the static recipe
  (`recipe/static/static_db_*.xml`) whose first `<lod arenaid>` is the piece's RX2 [data].
- The static reader (82868318) skips id 0 and builds the matrix from the quaternion x y z w as R transposed for the
  row-vector engine, position as row 3; the piece ctor (82865108) copies it unchanged, no template matrix on top
  [code, b434; all 533 disc templates have identity matrices anyway, data].
- Every piece is its own object: collision is a per-piece registration in the physics world's static set, rebuilt on
  each layout apply (b431 section 2, [code] for the add / clear / build calls); grinds go through the static piece
  manager, a pluggable provider of the shared rail query, posing the template's splines by the piece matrix
  (82868860 -> 82C1F098, b431 section 1, [code]). The tint is a per-draw constant that multiplies the linear albedo
  (b433, [code] in the shaders).

## Change

Setup (`tools/asset_pipeline/park_layout.py`, wired into the maps group in `install.py`):
- `parse` (the container, both record formats, the movable block), `recipe` (piece id -> RX2, `cap.*`), `Sources`
  (disc `miscload.big` + `parkassets.big`, then the DLC packs' BIGs; a later archive's row, model or texture replaces
  an earlier one), `world_layouts` (every world row's layout through its parents).
- Each piece template is decoded with the DMO readers (`dynamic_props.template_meshes`: mesh, collision, grind
  splines) and its textures from `static/texture/<GUID>.rx2`; the placements are written by
  `dynamic_props.write_placements` (split out of the district prop export, unchanged output for props) as
  `private/park-layouts/<layout>.skate` + `.json` (instances with stable ids `<layout>#s<index>`, template, matrix,
  tint, caps; the movable pieces listed, not yet spawned), plus `worlds.json` {world row: {layout, stream}}.
- `teleports.py`: an fe_locations row whose world names a layout is a destination even without artwork, named by its
  world row's text ("Stadium - Full", "Team HQ - Empty", "PCU Training - Basic"); every destination records its
  world row. A location's locator resolves as retail does (research b436, locator system 82910DC0 [code]): by name
  in one global table, whatever the district (same district first, which keeps every other destination unchanged),
  so the Stadium's layout rows use DownTown park's `Z_SP_DT_Start` (66.1, 0, -64.0) as retail does; a name found
  nowhere (`vert_skp5_01_challengelocator_01`, Monster Park) gets retail's identity at (0, 0.5, -5) (player 1).

Engine (`crates/skate-game/src/park_layout.rs`):
- The Challenge Map plan (`challenge_map.rs`) resolves the destination's world to a layout; another layout on the
  current map is a map load with `map_library::Entry::world` set (the request stays serialisable).
- The map loader (`map_transition.rs`) marks the loaded map with the layout (an in-memory `PARK` extension, never in a
  file); `CurrentMap::layout` remembers it.
- Physics (`physics.rs`): `skate_world::collision_world_with` appends the posed piece triangles (retail shapes with
  their native edge codes, else the render mesh, as for props: `skate_world::posed_collision`) after the map's, one
  query range per piece; `StaticProvider::with_pieces` adds one rail asset per piece (`park_layout::pose_rails`,
  near-vertical bit as 82C1E568, owner tag `PIECE_OWNER_TAG` below the moving-rail tags).
- Render (`map_render.rs`): the pieces spawn as `ParkPiece` entities (never `PropInstance`).

Park start (the user's retail video, 2026-10-21: "in retail you spawn at the top of the big ramp"):
- The `.park` editor tail's first matrix is the park's player start (b425 section 4 had it as "likely the saved editor
  camera or player start"): Stadium - Full (241.2, 37.1, -91.0) on top of the big ramp, the video's first frame and
  ours match. `park_layout.parse` reads it (`start`); `teleports.py` gives a park row its layout's start instead of
  its location's locator, except a location with its own artwork and locator (`startpark_progression`, ours: not
  traced for those). Test `test_the_editor_tail_holds_the_park_start`.
- Tint: the recomp draws the pieces untinted (the deck beige), our port tinted as Skate 3's code and data give it
  (b445; the deck's record (126, 100, 47)). The user (2026-10-21): "the shaders in ours are already better than the
  recomp, the recomp is definitely not quite right"; decision "Keep the tints".

Park teleporters (research b438; the user, 2026-10-20: "you set them to the same 'channel' and then when a player
gets close a little dialog appears on screen asking if they want to tp"):
- The teleporter pieces have no model. The disc vault class `Hash_D01B99210BA7323C` gives each one a channel 0..3
  and a role, 0 start / 1 end (field `Hash_77B8CA7AC65C8504`) [data]. Per channel, retail takes the first placed
  start and end piece (8287B918 / 828687C0) and builds an ordinary teleport challenge (8286D6E8: title
  `ID_COMMON_TELEPORT`) with a box entry volume of half extents 2 (8286CFF8) around the start piece's world centre,
  arriving at the end piece's world centre with its axes [code].
- `park_layout.layout_teleporters` writes those as `private/park-layouts/teleporters.json` (the `teleporters.json`
  record shape, plus `layout`); `teleporters.rs` loads them beside the disc's and arms one only while its layout is
  loaded. They use the ported prompt and confirm (doc 38 "Teleporters"). Zen Plaza - Full: one, channel 0, entry at
  (37.6, 2.0, -21.8), arrival at (15.4, 16.8, -13.0). Tests `test_teleporters_pair_the_first_start_and_end_of_a_channel`,
  `a_park_teleporter_is_armed_only_with_its_layout`. Open: "first placed" follows our record order (retail's is a
  hash map order); whether the arrival snaps to the ground.

Movable pieces (block 2; research b435):
- They are ordinary DMOs [code: 82883828 spawns them with the district DMO descriptor; template = the dynamic recipe
  mod's arena id, 119 / 119 disc mods resolve to a parkassets.big `dynamic/model/dynamic/dmo_*` template; the u32
  flag is never read on load]. Setup writes them per layout as `<layout>.props.skate` / `.props.json` (the district
  prop shapes: types, collision, grab splines, rails; stable ids `<layout>#d<index>`). The engine's prop layer takes a
  loaded map's layout package in place of `native-props/<map>` (`skate_world::prop_files`; no park stream has
  district props today, so nothing is dropped; a stream with both would need them merged), and saved prop poses are
  kept per layout (ours). All 71 disc + DLC movable pieces resolve (Stadium 1, Starter 3, Back Lot 47, Zen Plaza 17);
  the Stadium start loads the barrier with its type data, collision, grab spline and rail.
- Open: their initial sleep state (descriptor bytes +0x50..+0x54), census culling of layout pieces.

Park shading (research b433, b439 to b443):
- The pieces' materials use Skate 3's own programs, families 50 / 51 / 52 for `environmentpark.diffuse` / `.decal` /
  `.alphatest` (`retail_material.py`; `retail_world.wgsl` branch `fam >= 50u && fam <= 52u`, ALU slots cited). Diffuse:
  albedo = diffuse squared times the piece's tint; the tangent normal is not normalised before the frame; the light is
  the tangent-frame pseudo light (kd, as dynamicobject's) times min(lerp(dynamic shadow colour, 1, visibility),
  lerp(exponential shadow colour, 1, saturate(10 N.L) x visibility), projected lightmap), with no direct N.L,
  counter light or ambient; the specular reflects the world's literal light, unshadowed; fog scale m_params.y. Decal:
  the same, with the decal mixed in by its alpha before the tint and the detail normal. Alphatest: dynamicobject's
  shape with the occlusion in place of its shadow; output alpha = the raw diffuse alpha, tested at 30 / 255
  (`GREATEREQUAL`, blend state 1, b441), two-sided.
- Data: the tint per piece (layout record, white default; b439: the renderer's slots start white and take the
  record's), the shadow colours per park world (the world row's two Vector4 fields when its park bool is set; other
  worlds (0.2, 0.2, 0.2), the retail code constant; written into each layout's sidecar as `shadow_colours`), and the
  park materials' `m_params` rows (vault class `material_environmentpark`, now in `render-parameters.json`; an older
  install falls back to the retail fog scale 1). The engine gives each piece a material with its `ParkDraw` (tint +
  colours), one per distinct value (at most 97 per layout).
- Projected lightmap (b439, b440, b444): each park world row names a lightmap (`Hash_AA9212BDE5A603B4`) and its
  x0, x1, z0, z1 bounds; the textures are 512 x 512 DXT1 RX2s (disc `miscboot.big` `data/content/textures/`: megapark,
  startpark, downtown, industrial; the Create pack: backlot, downtownlightmapnight; the San Van pack: zenplaza).
  Setup binds the layout's lightmap as every piece material's lightmap channel and writes `projected_lightmap`
  (texture, bounds) into the sidecar; the shader samples it once at retail's projection, u = (z - z0) / dz +
  y lz / (ly |dz|), v = (x - x0) / dx - y lx / (ly |dx|) (the `|d|` form kept as retail has it, also for Back Lot's
  reversed bounds), L = the sun direction toward the sun [code for the convention; that L equals our render
  location's sun direction is not traced, b444], used linearly inside the occlusion min. All 11 park layouts get
  theirs.
- Decal UVs: the piece export first wrote only the diffuse UV set, so the decal programs fell back to it. The user
  (2026-10-21, Stadium - Full): "It looked like they are not mapped to it properly, it seemed like the right texture,
  but like it was offset weirdly on the object". Each mesh now carries its decal UV set (`decode_decal_uvs`, as the
  map exporter writes it); after the fix: "that time it looked much better".
- Ours: our one shadow map feeds both of retail's shadow terms; the lightmap sampler addressing (retail's sampler
  state was not found; ours repeats); the alphatest specular's occlusion channel (green taken).

Not retail (ours, the same outcome): a layout change reloads the map instead of re-applying in place; piece rails
join the static pass, so under the 40-record cap they come before the props (retail queries them after the DMO
provider; parks have few props).

NOT RETAIL YET: the parks' projected lightmaps (see "Park shading"); the DLC packs' recipe precedence.

## Moddability and multiplayer

- A layout is data: a mod ships `<layout>.skate` / `.json` and a `worlds.json` row (same shapes as setup's) or
  replaces one; pieces keep stable ids; the world row choice is part of the map request a host would send.
- The pieces are fixed, so every peer builds the same static world from the same files.

## Verification

- Setup tests `test_park_layout` (container, filler, tutorial records, rejects truncation, rotation convention,
  recipe rows; data-gated: the disc layouts resolve every piece with b424 / b425's counts 450 / 328 / 256 / 251 /
  238). A disc export: 12 layouts, 0 missing pieces; MegaPark_Fullpark 56 templates, 45 with collision, 48 with rails.
- Engine tests `park_layout::tests` (a world places its layout only on its own stream, rails posed by the piece
  matrix, the layout mark replaced on re-attach) and the Challenge Map plan (Empty to Full reloads with the world, the
  same layout does not).
- Dev install (2026-10-21, maps refresh plus the park re-export with the DLC packs): 18 layouts, every disc and DLC
  piece resolved except Zen Plaza's two Create-a-Park teleporter pieces (`qa_SkateParkTeleportStart1`,
  `qb_SkateParkTeleportEnd1`: their RX2 has no DMO model, left out and reported). A muted start as "Stadium - Full"
  (`--map MegaPark.skate --teleport megapark_full`): `SKATE_PARK_LAYOUT ... pieces=450 shapes=45 collision_pieces=450
  triangles=197832 railed_pieces=398`, 7029 rail primitives, 450 piece draws; the frame shows the pieces the Empty
  start lacks. The whole skate-game suite passes (867).
- Not play-tested.
- Collision (research b437, data): every piece template carries one ClusteredMesh and no primitive volumes; the
  Stadium's 197832 piece triangles are retail's own (its meshes are subdivided, e.g. a street pad 384 triangles for a
  60-triangle render mesh). The rails and fences (11 of the Stadium's 56 templates) link their volume through the
  other EB000A form, (0, count, 12, volume index..., 0) with no assembly, which `dynamic_props.template_collision`
  now follows: all 661 parkassets.big templates get their retail shape (edge codes, surfaces), none falls back to the
  render mesh. The same reader serves the district DMO props: a DMO template with that link form gains its retail
  shape at the next maps refresh (check the props after it).

## Open questions

- The tint path on the CPU and the `environmentpark` families (b433 change list).
- Movable pieces (b435), the Monster Park challenge locator, the DLC recipe precedence, `cap.AllowCollision`.

## The pieces fuse into one collision set: no catching on seams (2026-10-21, research b477 / b478)

- **Problem (laptop session 2026-10-21, the user):** "going to stadium full, I tried skating down the big ramp and
  bailed almost immediately from the cracks between the objects not connecting properly causing you to get stuck,
  stop, bail, bump over the crack" and "backlot dlc map: ... tje cracks between surfaces often catch you or stop you
  while skating". The log: 4 bails in 25 s down the Stadium ramp, 4 on BackLot's flat ground, all on layout pieces.
- **Cause [data / code, b477]:** the seams are flush (0.1 to 1.5 mm), but each piece kept its own retail edge codes:
  its open borders are coded convex (0x60 = cosine -0.234), and nothing paired the edges two pieces share
  (`portable_parts` decoded the native codes over the welded pass). The trace shows the wheel contact mask dropping at
  the seam crossings.
- **Retail [code, b478, medium-high]:** the SkateParkView (`world+0xBC` slot +0x20, vtable `82310560`) fuses all
  static pieces in its "fuserJob" (`827762D0`, build `827765C0` -> `828728B8`): per piece `82872D70` copies only the
  posed corners, piece index and surface of each triangle (the edge-code bytes are skipped), and `82876188` merges
  vertices, removes zero-area triangles (`82AC2010`) and pairs neighbours over the whole set. Open: the exact
  edge-writing function and which builder default (0.01 or 0.1) is the vertex merge.
- **Change:** `skate_world.rs` `portable_parts_fused`: the posed pieces' vertices merge across the whole set within
  `Pieces::weld` (data: the layout sidecar's `fuse_weld`, default 0.01 m), every edge code is rebuilt over the fused
  set (the pieces' own codes are ignored, their surfaces kept), triangles the merge collapses are dropped and the
  piece ranges follow. Disc maps and props keep the exact 1 mm path. Log: `SKATE_PARK_FUSE`.
- **Verification:** `fused_park_pieces_pair_their_flush_seams` (1.5 mm seam paired and flat; the 1 mm path leaves it
  open; a 5 cm gap stays open; deterministic), `fused_pieces_drop_collapsed_triangles_and_keep_their_ranges`; verify
  runs load "Stadium - Full" (450 pieces, 197832 triangles) and "Back Lot Park - Full" (453, 188784). skate-game 882.
  Not play-tested: whether the ramp and BackLot still catch.
