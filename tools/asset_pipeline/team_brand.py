"""TeamBrand texture bindings of a district's presentation streams (research b1068, b1072).

Each presentation arena (`cPres_*.xsf`) can hold a binding section (0x00EB0028): a 16-byte header {count, offset}
and `count` 32-byte records {placement a, placement b, texture asset id, brand tier row key}. Retail hands them to
TeamBrandPresentation_BindRecords (827C9A30), which swaps the texture's image for the tier's (a career photo, the
team photo, a logo) while the tier is unlocked. The sidecar `<Map>.teambrand` keeps them per map; the texture id is
written the way the map writer names textures (`0x<16 hex digits>`).
"""
import json
import struct
from pathlib import Path

from .map_volumes import SIM_ARENA_PROCESSOR, _sections, stream_arenas

FORMAT = 'skate3rust-team-brand-bindings'
VERSION = 1
BINDINGS = 0x00EB0028
RECORD = 32
# Presentation arenas use the same arena processor as the simulation ones.
PRES_ARENA_PROCESSOR = SIM_ARENA_PROCESSOR


def arena_bindings(raw):
    """The binding records of one decoded presentation arena."""
    result = []
    for base, _, size, _, _, kind in _sections(raw):
        if kind != BINDINGS:
            continue
        if size < 16 or base + size > len(raw):
            raise ValueError('Truncated team brand binding section')
        count, offset = struct.unpack_from('>II', raw, base)
        if offset < 16 or offset + count * RECORD > size:
            raise ValueError('Invalid team brand binding header')
        for index in range(count):
            a, b, texture, tier = struct.unpack_from('>4Q', raw, base + offset + index * RECORD)
            result.append(dict(texture='0x%016x' % texture, tier='%016X' % tier, a='%016x' % a, b='%016x' % b))
    return result


def district_bindings(stream_directory):
    result = []
    for stream, asset_id, arena in stream_arenas(stream_directory, table='Pres', prefix='cPres_'):
        if arena[:7] != b'\x89RW4xb2':
            continue
        for binding in arena_bindings(arena):
            binding.update(stream=stream, arena='%016x' % asset_id)
            result.append(binding)
    return result


def export(stream_directory, output, map_name):
    bindings = district_bindings(stream_directory)
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + '.tmp')
    temporary.write_text(json.dumps({'format': FORMAT, 'version': VERSION, 'map': map_name,
                                     'source': 'retail presentation streams (0x00EB0028 bindings)',
                                     'bindings': bindings}, indent=1), encoding='utf-8')
    temporary.replace(output)
    return bindings


def export_archive(archive, output_directory):
    """Stage `<Map>.teambrand` from a worldDIST_*.big without a full map conversion."""
    import tempfile
    from tools.owned_game.big import BigArchive
    archive = Path(archive)
    district = archive.stem.removeprefix('world')
    big = BigArchive(archive)
    wanted = [e for e in big.entries if Path(e.path).name.startswith('cPres_') or e.path.endswith('_Pres.xst')]
    with tempfile.TemporaryDirectory(prefix='skate-teambrand-') as tmp:
        big.extract_entries(wanted, Path(tmp))
        stream = Path(tmp) / 'data/content/world/stream' / district
        label = district.removeprefix('DIST_')
        return export(stream, Path(output_directory) / (label + '.teambrand'), label)


if __name__ == '__main__':
    import argparse
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'vendor/university/tools/vanilla_map_extraction/tools'))
    parser = argparse.ArgumentParser(description='Stage team brand binding sidecars from district archives.')
    parser.add_argument('archives', type=Path, nargs='+', help='worldDIST_*.big')
    parser.add_argument('--output', type=Path, required=True, help='directory for <Map>.teambrand')
    arguments = parser.parse_args()
    for archive in arguments.archives:
        rows = export_archive(archive, arguments.output)
        print(archive.name, len(rows), 'bindings')
