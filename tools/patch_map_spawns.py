"""Rewrite the baked spawn/heading in installed .skate headers.

Maps converted from a checkout before b495ed6 ("Asset pipeline: bake authored
map spawns and headings") carry the old geometric spawn and heading 0. The
authored values are the ones documented in docs/hails-additions/04-map-spawns.md
(§Resulting defaults), verified in play. A full re-conversion needs the disc;
this patches the installed headers in place so an existing installation gets
the same starts without one.

The header layout is fixed by skate-data's reader (skate_map.rs): magic,
version word, name, then three f32 spawn and one f32 heading. Nothing after
the heading is touched, so page-cache keys (which hash texture and geometry
bytes, not the spawn) stay valid. maps.json sha256 entries are recomputed
because switch_export --check and verify_install compare them.

Usage: python3 tools/patch_map_spawns.py <installation-dir> [--dry-run]
"""
import argparse
import hashlib
import json
import math
import struct
import sys
from pathlib import Path

# docs/hails-additions/04-map-spawns.md §Resulting defaults. Heading in
# degrees, game convention forward = (sin h, 0, cos h).
AUTHORED = {
    'BlackBoxPark': ((10.55, 0.08, 6.05), 90.0),
    'DownTown': ((-254.05, 40.54, 32.39), -85.5),
    'DownTownSkatePark': ((66.05, 0.0, -64.02), 0.0),
    'Industrial': ((-89.16, 1.01, -138.35), -45.4),
    'IndustrialSkatePark': ((-64.01, 0.75, 0.0), 0.0),
    'MaloofMoneyCup': ((0.56, -0.06, -43.42), 180.0),
    'MegaPark': ((230.63, 0.48, -73.08), -90.0),
    'SkateSchool': ((-31.28, 0.11, -31.5), 0.0),
    'StartPark': ((-64.01, 0.0, 0.0), 0.0),
    'University': ((294.64, 74.12, -432.6), 114.5),
}


def spawn_offsets(data: bytes) -> tuple[int, int, str]:
    """(spawn, heading) byte offsets for a v14 header, per skate_map.rs read()."""
    if data[:8] != b'SKATE14\0':
        raise ValueError(f'not a SKATE14 header (magic {data[:8]!r})')
    if struct.unpack_from('<I', data, 8)[0] != 0x12345678:
        raise ValueError('bad version word')
    name_len = struct.unpack_from('<I', data, 12)[0]
    name = data[16:16 + name_len].decode('utf-8')
    spawn = 16 + name_len
    if struct.unpack_from('<3f', data, spawn) == (0.0, 0.0, 0.0):
        raise ValueError(f'{name}: spawn is zero, header layout assumption broken')
    return spawn, spawn + 12, name


def patch(path: Path, position, heading_deg: float, dry: bool) -> str:
    data = bytearray(path.read_bytes())
    spawn, heading, name = spawn_offsets(bytes(data))
    before = struct.unpack_from('<3f', data, spawn)
    before_h = struct.unpack_from('<f', data, heading)[0]
    rad = math.radians(heading_deg)
    if dry:
        return f'{name}: ({before[0]:.2f},{before[1]:.2f},{before[2]:.2f}) h={before_h:.4f} -> {position} h={rad:.4f}'
    struct.pack_into('<3f', data, spawn, *position)
    struct.pack_into('<f', data, heading, rad)
    path.write_bytes(bytes(data))
    return f'{name}: spawn patched at {spawn}, heading at {heading} ({before} h={before_h:.4f} -> {position} h={rad:.4f})'


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('installation', type=Path)
    ap.add_argument('--dry-run', action='store_true')
    args = ap.parse_args()
    maps = args.installation / 'maps'
    catalog_path = args.installation / 'maps.json'
    catalog = json.loads(catalog_path.read_text(encoding='utf-8'))
    by_name = {entry['name']: entry for entry in catalog}
    missing = sorted(set(by_name) - set(AUTHORED))
    if missing:
        print(f'no authored values for {missing}; refusing to guess', file=sys.stderr)
        return 1
    for entry in catalog:
        path = args.installation / entry['path']
        print(patch(path, *AUTHORED[entry['name']], dry=args.dry_run))
        if not args.dry_run:
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            if digest != entry['sha256']:
                print(f'  sha256 {entry["sha256"]} -> {digest}')
                entry['sha256'] = digest
    if not args.dry_run:
        catalog_path.write_text(json.dumps(catalog, indent=2), encoding='utf-8')
        print(f'updated {catalog_path}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
