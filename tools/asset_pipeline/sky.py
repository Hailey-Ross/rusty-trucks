"""Extract authored retail skies through world references and RX2 GUID bindings."""
import json
import struct
import sys
from pathlib import Path
from tools.owned_game.big import BigArchive
from .environment import Collections, NAMES, field, key_hash, sky_parameters, world_environment, fog_parameters


def _material(raw, table):
    channels = {}
    for section in table.entries:
        if section.type_id != 0x00eb0005:
            continue
        o = section.f0
        h = struct.unpack_from('>8I', raw, o)
        if not h[1] or (h[4]-h[3])//h[1] != 32:
            raise ValueError('Unexpected sky material layout')
        for i in range(h[1]):
            v = struct.unpack_from('>8I', raw, o+h[3]+i*32)
            at = o+v[0]
            kind = raw[at:raw.index(b'\0', at)].decode('ascii')
            if kind in ('diffuse', 'specular'):
                channels[kind] = (v[4]<<32)|v[5]
    return channels


def _texture(textures, guid):
    handle = None
    for section in textures.entries:
        if section.type_id != 0x00eb000b:
            continue
        o = section.f0
        n, start = struct.unpack_from('>2I', textures.data, o)
        for i in range(n):
            _, _, hi, lo, _, index = struct.unpack_from('>6I', textures.data, o+start+i*24)
            if (hi<<32)|lo == guid:
                handle = index
    texture = next((t for t in textures.textures if t.index == handle), None)
    if texture is None:
        raise ValueError(f'Sky texture GUID did not resolve: {guid:016X}')
    return texture


def _sky(collections, row, read, mdl_parser, rx2_parser):
    """A world's sky: its environment, dome texture, sun gradient and mesh (raises when it does not resolve)."""
    env = world_environment(collections, row['key'])
    raw = read(row, 'data/content/'+env['sky_model']+'.rx2')
    model = mdl_parser.parse_rx2(raw)
    table = rx2_parser.RX2File(raw)
    table.parse()
    channels = _material(raw, table)
    textures = rx2_parser.parse_rx2(read(row, 'data/content/'+env['sky_textures']+'.rx2'))
    diffuse = _texture(textures, channels['diffuse'])
    sun = _texture(textures, channels['specular'])
    shader = next(p.value for p in model.materials if p.kind == 'AttribulatorMaterialName')
    env.update(sky_parameters(collections, shader))
    env['fog_frame'] = fog_parameters(env['fog'])
    if len(model.meshes) != 1 or model.meshes[0].uvs is None:
        raise ValueError('Unexpected sky mesh layout')
    mesh = model.meshes[0]
    if len(sun.rgba) != sun.width*sun.height*4 or sun.height != 16:
        raise ValueError('Unexpected sky sun-gradient dimensions')
    return env, diffuse, sun, mesh


def convert(game_root, assets, converted, dlc_vaults=()):
    """Every disc world with a worldDIST_<D>.big, plus the worlds of the player's DLC map packs (`dlc_vaults`,
    asset_pipeline.dlc.load): a DLC sky is read from its own pack first, then from miscload.big (b417)."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/utt'))
    import mdl_parser
    import rx2_parser
    from .dlc import merged, stream_districts
    collections = Collections(merged(converted, list(dlc_vaults)))
    archive = BigArchive(game_root/'data/big/miscload.big')
    entries = {e.path: e for e in archive.entries}
    packs = {}
    for v in dlc_vaults:
        if v['vault'] not in packs:
            pack = BigArchive(v['big'])
            packs[v['vault']] = (pack, {e.path.replace('\\', '/'): e for e in pack.entries})
    def read(row, path):
        pack = packs.get(row.get('vault'))
        if pack and path in pack[1]:
            return pack[0].read(pack[1][path])
        return archive.read(entries[path])
    output = assets/'private/native-skies'
    output.mkdir(parents=True, exist_ok=True)
    available = {p.stem.removeprefix('world') for p in (game_root/'data/content').glob('worldDIST_*.big')}
    for pack, paths in packs.values():
        available |= set(stream_districts(paths))
    count = 0
    skipped = []
    for (cls, _), row in collections.rows.items():
        if cls != key_hash('world'):
            continue
        own_stream = next((v['data'] for k, v in row['fields'].items()
                           if key_hash(k) == key_hash('WorldStream')), None)
        if own_stream not in available:
            continue
        # The map's label (asset_pipeline.dlc.map_jobs: the stream folder without DIST_ / Dist_).
        name = own_stream[5:] if own_stream.lower().startswith('dist_') else own_stream
        try:
            sky = _sky(collections, row, read, mdl_parser, rx2_parser)
        except (KeyError, ValueError) as error:
            if not row.get('vault'):
                raise
            # NOT RETAIL YET: Hawaiian Dream's "sky" model is no sky material (retail swaps in a multi-mesh copy built
            # into the executable, b419); that map gets no authored sky until it is ported.
            skipped.append(f"{row['key']} ({error!r})")
            continue
        env, diffuse, sun, mesh = sky
        (output/(name+'.rgba')).write_bytes(diffuse.rgba)
        (output/(name+'.sun.rgba')).write_bytes(sun.rgba)
        (output/(name+'.json')).write_text(json.dumps({
            'width': diffuse.width, 'height': diffuse.height,
            'positions': mesh.vertices.tolist(), 'uvs': mesh.uvs.tolist(),
            'indices': mesh.faces.reshape(-1).tolist(),
            'environment': env, 'sun_width': sun.width, 'sun_height': sun.height,
            'source_sha256': row['sha256'],
        }, separators=(',', ':')), encoding='utf-8')
        count += 1
    if not count:
        raise ValueError('No authored world skies resolved')
    if skipped:
        print('DLC worlds without a resolvable sky: ' + ', '.join(skipped), flush=True)
    return count


if __name__ == '__main__':
    import argparse
    import tempfile
    from .vlt import convert as convert_vlt
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-root', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    args = parser.parse_args()
    # A targeted update reads only the small database pair and sky resources.
    with tempfile.TemporaryDirectory(prefix='skate-sky-') as work:
        database = BigArchive(args.game_root/'data/big/db.big')
        needed = {'skaterschema.bin', 'skaterschema.vlt', 'skatercollections.bin', 'skatercollections.vlt'}
        database.extract_entries([e for e in database.entries if Path(e.path).name.lower() in needed], Path(work))
        stem = Path(work)/'data/db'
        names = (Path(__file__).parent/'names.txt').read_text(encoding='utf-8').splitlines()
        converted = convert_vlt(stem/'skaterschema', stem/'skatercollections', [*names, *NAMES])
        print('Prepared', convert(args.game_root, args.assets, converted), 'authored skies')
