"""Trick Guide demo scene (the retail menus' FE 14): the clips scene/trickguide/<name>1.abin from scene.big and the
set DIST_TrickGuide (+ _Textures) from miscload.big, written as a render-only TrickGuide.skate plus its sky dome
(the set's sky.default mesh) in the native sky format as TrickGuide.sky.json / .rgba / .sun.rgba."""
import json
import shutil
import sys
from pathlib import Path
from tools.owned_game.big import BigArchive

PREFIX = 'data/scene/trickguide/'
SET_MODEL = 'world/models/DIST_TrickGuide'
SET_TEXTURES = 'world/models/DIST_TrickGuide_Textures'
# The set's ground, ledges and boxes; its dome (sky.default) goes to the sky package.
SET_SHADERS = ('environment.default', 'environmentsimple.default')


def extract(game_root, assets, work):
    archive = BigArchive(game_root/'data/big/scene.big')
    entries = [e for e in archive.entries if e.path.lower().startswith(PREFIX)]
    archive.extract_entries(entries, work)
    output = assets/'private/hud/scene/trickguide'
    if output.exists():
        shutil.rmtree(output)
    shutil.copytree(work/PREFIX, output)
    misc = BigArchive(game_root/'data/big/miscload.big')
    files = {e.path: e for e in misc.entries}
    read = lambda name: misc.read(files['data/content/' + name + '.rx2'])
    if not export_set(read(SET_MODEL), read(SET_TEXTURES), assets/'private/hud/scene/TrickGuide.skate'):
        raise RuntimeError('The Trick Guide set has no exportable meshes')
    export_sky(read(SET_MODEL), read(SET_TEXTURES), assets)
    return len(entries)


def export_sky(raw, texture_raw, assets):
    """The set's dome in the native sky format (retail_sky.rs SkyPackage). Its material_sky parameters come from the
    collections; the sun direction and anchor height have no Trick Guide record, so the engine takes the current
    map's (NOT RETAIL YET)."""
    vendor = Path(__file__).resolve().parents[1]/'vendor'
    sys.path.insert(0, str(vendor/'utt'))
    sys.path.insert(0, str(vendor/'university/tools/vanilla_map_extraction/tools'))
    import mdl_parser
    import rx2_parser
    import prepare_hawaiian_dream as prep
    from .backdrop import texture_groups
    from .environment import Collections, sky_parameters
    from .sky import _texture
    model = mdl_parser.parse_rx2(raw)
    groups, bindings = prep._bind_material_groups_by_guid(
        raw, prep._group_material_parameters(model.materials), len(model.meshes), allow_import_order_fallback=False)
    domes = [i for i in range(len(model.meshes)) if prep._material_metadata(groups, i)['shader_name'] == 'sky.default']
    if len(domes) != 1 or model.meshes[domes[0]].uvs is None:
        raise ValueError('Unexpected Trick Guide sky layout')
    mesh = model.meshes[domes[0]]
    table = rx2_parser.RX2File(raw)
    table.parse()
    channels = texture_groups(raw, table, prep.RETAIL_TEXTURE_CHANNELS)[bindings[domes[0]]['group_index']]
    textures = rx2_parser.parse_rx2(texture_raw)
    diffuse, sun = _texture(textures, channels['diffuse']), _texture(textures, channels['specular'])
    if sun.height != 16 or len(sun.rgba) != sun.width*sun.height*4:
        raise ValueError('Unexpected Trick Guide sun-gradient dimensions')
    collections = Collections(json.loads((assets/'private/stock/skater-collections.json').read_text(encoding='utf-8')))
    output = assets/'private/hud/scene'
    (output/'TrickGuide.sky.rgba').write_bytes(diffuse.rgba)
    (output/'TrickGuide.sky.sun.rgba').write_bytes(sun.rgba)
    (output/'TrickGuide.sky.json').write_text(json.dumps({
        'width': diffuse.width, 'height': diffuse.height,
        'positions': mesh.vertices.tolist(), 'uvs': mesh.uvs.tolist(), 'indices': mesh.faces.reshape(-1).tolist(),
        'sun_width': sun.width, 'sun_height': sun.height,
        'material': sky_parameters(collections, 'sky.default'),
    }, separators=(',', ':')), encoding='utf-8')


def export_set(raw, texture_raw, output):
    """The set's ground, ledges and boxes as a render-only .skate, in the format of backdrop.py's global-model export
    (kept separate from backdrop.py so this branch and the living-world branch merge cleanly; fold the two together
    once both are upstream)."""
    import hashlib
    import tempfile
    vendor = Path(__file__).resolve().parents[1]/'vendor'
    sys.path.insert(0, str(vendor/'utt'))
    sys.path.insert(0, str(vendor/'university/tools/vanilla_map_extraction/tools'))
    import numpy as np
    import mdl_parser
    import rx2_parser
    import prepare_hawaiian_dream as prep
    from retail_lightmap_uv import decode_lightmap_uvs
    from retail_texture_decode import B5G6R5_FORMAT_ID, decode_b5g6r5
    from .backdrop import texture_groups
    from .map_writer import write
    from .sky import _texture
    model = mdl_parser.parse_rx2(raw)
    groups, bindings = prep._bind_material_groups_by_guid(
        raw, prep._group_material_parameters(model.materials), len(model.meshes), allow_import_order_fallback=False)
    selected = [(i, prep._material_metadata(groups, i)) for i in range(len(model.meshes))]
    selected = [(i, m) for i, m in selected if m['shader_name'] in SET_SHADERS]
    if not selected:
        return False
    textures = rx2_parser.parse_rx2(texture_raw)
    table = rx2_parser.RX2File(raw)
    table.parse()
    channel_groups = texture_groups(raw, table, prep.RETAIL_TEXTURE_CHANNELS)
    with tempfile.TemporaryDirectory(prefix='skate-trickguide-') as temporary:
        root = Path(temporary)
        arrays, meshes, exported = {}, [], {}
        for i, material in selected:
            mesh, binding = model.meshes[i], bindings[i]
            meshes.append(dict(material, index=i, name=groups[i]['Name'][0],
                retail_material_guid=f"0x{binding['material_guid']:016X}",
                retail_material_handle=f"0x{binding['material_handle']:08X}",
                retail_material_group_index=binding['group_index'], source_offsets=mesh.source_offsets))
            arrays[f'vertices_{i}'] = mesh.vertices
            arrays[f'faces_{i}'] = mesh.faces
            arrays[f'uvs_{i}'] = mesh.uvs
            arrays[f'normals_{i}'] = mesh.normals
            lm = decode_lightmap_uvs(raw, vertex_buffer_offset=mesh.source_offsets['vertex_buffer'],
                vertex_count=mesh.vertex_count, vertex_stride=mesh.vertex_stride, attributes=mesh.attributes)
            if lm is not None:
                arrays[f'lightmap_uvs_{i}'] = lm.values
            for role, texture_id in material['retail_texture_ids'].items():
                if texture_id in exported:
                    continue
                texture = _texture(textures, channel_groups[binding['group_index']][role])
                rgba = texture.rgba
                if texture.fmt_id == B5G6R5_FORMAT_ID:
                    rgba = decode_b5g6r5(textures.data[texture.data_offset:texture.data_offset+texture.buffer_size],
                                        texture.width, texture.height)
                (root/(texture_id+'.rgba')).write_bytes(rgba)
                exported[texture_id] = dict(width=texture.width, height=texture.height, rgba=texture_id+'.rgba')
        np.savez(root/'model.npz', **arrays)
        digest = hashlib.sha256(raw).hexdigest()
        manifest = dict(map_name='TrickGuide', district_name='DIST_TrickGuide', models=[dict(
            asset_id='0x'+digest[:16], npz='model.npz', meshes=meshes)],
            textures=exported, normal_texture_policy=dict(excluded_texture_ids=[]), grind_splines=[],
            source_model=SET_MODEL, source_textures=SET_TEXTURES, source_sha256=digest)
        (root/'manifest.json').write_text(json.dumps(manifest))
        write(root/'manifest.json', output, None, render_only=True)
    return True
