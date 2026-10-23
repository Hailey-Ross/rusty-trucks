"""Challenge visual indicators (the floating ribbon icon over a teleporter; research b532 / b533, USER-ASKS 264).

Two sources:
- `miscload.big data/content/world/models/visual_indicators_sk82.rx2`: an RW4 arena whose object of type 0x00EB0021
  (VISUALINDICATORDATA) holds the ribbon splines. Layout [data]: u32 count, u32 total segments, u32 table offset,
  u32 segment offset; `count` entries of 64 bytes (u64 name hash, u64 spline id, f32x4 bounds min, f32x4 bounds max,
  u32 kind, u32 segments, u32 segment offset from the object, pad); segments of 64 bytes, a 4x4 power-basis cubic
  (rows c3, c2, c1, c0: p(t) = c3 t^3 + c2 t^2 + c1 t + c0, t in [0, 1]).
- the `ribbon_indicator` rows (Offset, the `ribbon_spline` row naming the spline id, the `visualindicators` style row).
Output: assets/private/visual-indicators.json. Details: docs/hails-additions/38-dlc-support.md#teleporter-markers
"""
import json
import struct
from pathlib import Path

from .environment import Collections, key_hash

FORMAT = 'skate3rust.visual-indicators'
VERSION = 1
ARENA = 'data/content/world/models/visual_indicators_sk82.rx2'
SPLINES = 0x00EB0021
OFFSET = 'Hash_4374C0E6F4CB45ED'
SPLINE_REF = 'Hash_8D090C8B5931547A'
STYLE_REF = 'Hash_66FC5F54E2D8B317'
ENABLED = 'Hash_6C09761AE6E69545'
SPLINE_ID = 'Hash_1AFE1B981FB92727'


def arena_objects(data):
    """[(type, offset, size)] of an RW4 arena's object table."""
    if data[:7] != b'\x89RW4xb2':
        raise ValueError('not an RW4 arena')
    count, table = struct.unpack_from('>I', data, 0x20)[0], struct.unpack_from('>I', data, 0x30)[0]
    out = []
    for i in range(count):
        offset, _, size, _, _, kind = struct.unpack_from('>6I', data, table + i * 24)
        out.append((kind, offset, size))
    return out


def parse_splines(data):
    """{spline id '%016X': {kind, name, min, max, segments}} from the arena's VISUALINDICATORDATA object."""
    found = [(o, s) for k, o, s in arena_objects(data) if k == SPLINES]
    if len(found) != 1:
        raise ValueError(f'{len(found)} visual indicator data objects')
    base, size = found[0]
    count, total, table, first = struct.unpack_from('>4I', data, base)
    if table + count * 64 > first or first + total * 64 > size:
        raise ValueError('visual indicator data out of bounds')
    out = {}
    for i in range(count):
        at = base + table + i * 64
        name, spline = struct.unpack_from('>QQ', data, at)
        low, high = struct.unpack_from('>3f', data, at + 16), struct.unpack_from('>3f', data, at + 32)
        kind, segments, offset = struct.unpack_from('>3I', data, at + 48)
        if offset + segments * 64 > size:
            raise ValueError(f'spline {spline:016X} out of bounds')
        rows = [list(struct.unpack_from('>16f', data, base + offset + j * 64)) for j in range(segments)]
        out['%016X' % spline] = dict(name='%016X' % name, kind=kind, min=list(low), max=list(high),
                                     segments=[[r[0:3], r[4:7], r[8:11], r[12:15]] for r in rows])
    return out


def _ref_key(raw):
    """The key hash of a RefSpec value (class u64, key u64, ...)."""
    return struct.unpack_from('>Q', bytes.fromhex(raw), 8)[0]


def indicators(collections):
    """{ribbon_indicator key: {offset, spline, enabled, style}} with inherited fields resolved."""
    cls = key_hash('ribbon_indicator')
    out = {}
    for (row_cls, key), row in collections.rows.items():
        if row_cls != cls:
            continue
        fields, _ = collections.resolve(cls, key)
        spline_row = collections.rows.get((key_hash('ribbon_spline'), _ref_key(fields[key_hash(SPLINE_REF)]['data'])))
        spline = collections.resolve('ribbon_spline', key_hash(spline_row['key']))[0] if spline_row else {}
        style = collections.rows.get((key_hash('visualindicators'), _ref_key(fields[key_hash(STYLE_REF)]['data'])))
        offset = struct.unpack('>3f', bytes.fromhex(fields[key_hash(OFFSET)]['data'])[:12])
        out[row['key']] = dict(offset=list(offset),
                               spline=spline[key_hash(SPLINE_ID)]['data'] if key_hash(SPLINE_ID) in spline else None,
                               enabled=fields[key_hash(ENABLED)]['data'] not in ('', '00'),
                               style=style['key'] if style else None)
    return out


def export(game_root, collections, output, report=print):
    """Writes `output` and returns the indicator keys it holds."""
    from tools.owned_game.big import BigArchive
    archive = BigArchive(Path(game_root) / 'data/big/miscload.big')
    entry = next((e for e in archive.entries if e.path.replace('\\', '/').lower() == ARENA), None)
    if entry is None:
        raise ValueError(ARENA + ' missing')
    splines = parse_splines(archive.read(entry))
    rows = indicators(collections if isinstance(collections, Collections) else Collections(collections))
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'format': FORMAT, 'version': VERSION, 'splines': splines, 'indicators': rows}),
                      encoding='utf-8')
    missing = sorted(k for k, r in rows.items() if r['spline'] and r['spline'] not in splines)
    report(f'Visual indicators: {len(rows)} rows, {len(splines)} splines'
           + (f'; splines missing for {", ".join(missing)}' if missing else ''))
    return sorted(rows)
