"""Movie / scene subtitle cues (docs/hails-additions/47-movie-player.md "Subtitles").

The disc's `data/fe/subtitles/subtitles.big` holds `SubTitles_<language>/<name>.sub` cue files: little-endian 8-byte
records `{u16 time (tenths of a second), u16 pad, u32 string hash}` up to a record with time 0xFFFF (retail loader
8263E368). The hash is the language table's own key: its labels file lists `{u32 hash, u32 pool offset}` per string,
so each cue resolves to the string's label (`SUBTITLE_NIS_...`), which the engine localizes like any FE text. A hash
the table does not have (the blank 0x001A20BA every file ends with) clears the line, as retail's lookup failure does.
"""
import json
import struct
from pathlib import Path

LABELS = 'data/fe/languages/labels/language_labels_global_skate3ng.bin'
SUBTITLES = 'data/fe/subtitles/subtitles.big'
END = 0xFFFF


def cues(data: bytes) -> list[tuple[int, int]]:
    """(time in tenths, string hash) per record, the terminator included."""
    out = []
    for offset in range(0, len(data) - 7, 8):
        time, _pad, key = struct.unpack_from('<HHI', data, offset)
        out.append((time, key))
        if time == END:
            break
    return out


def label_hashes(table: bytes) -> dict[int, str]:
    """String hash -> label from the labels file: header (count at +8, pool at +16), then `{hash, offset}` records;
    offsets count from the pool's strings (8 bytes past the pool start)."""
    count = struct.unpack_from('<I', table, 8)[0]
    pool = struct.unpack_from('<I', table, 16)[0] + 8
    out = {}
    for i in range(count):
        key, offset = struct.unpack_from('<II', table, 28 + i * 8)
        end = table.index(b'\0', pool + offset)
        label = table[pool + offset:end].decode('latin-1')
        if label:
            out[key] = label
    return out


def convert(files: dict[str, bytes], labels: dict[int, str]) -> dict:
    """`{"languages": {"English": {"<name>": [[tenths, label or null], ...]}}}` (names as on the disc)."""
    languages: dict[str, dict] = {}
    for path, data in sorted(files.items()):
        parts = path.replace('\\', '/').split('/')
        if len(parts) < 2 or not parts[-1].lower().endswith('.sub'):
            continue
        language = parts[-2].split('_', 1)[-1]
        rows = [[time, labels.get(key)] for time, key in cues(data) if time != END]
        languages.setdefault(language, {})[parts[-1][:-4]] = rows
    return {'languages': languages}


def export(game_root: Path, stock: Path) -> bool:
    """Writes `stock/subtitles.json`; False when the disc has no subtitle archive."""
    import sys
    from tools.owned_game.big import BigArchive
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from vendor.skate3_ui.project import CORE_ARCHIVES
    archive_path = Path(game_root) / SUBTITLES
    if not archive_path.is_file():
        return False
    archive = BigArchive(archive_path)
    files = {e.path: archive.read(e) for e in archive.entries}
    table = None
    for name in CORE_ARCHIVES:
        core = BigArchive(Path(game_root) / 'data/big' / name)
        for entry in core.entries:
            if entry.path.replace('\\', '/').lower() == LABELS:
                table = core.read(entry)
    if table is None:
        raise RuntimeError(f'{LABELS} not found in the core archives')
    (stock / 'subtitles.json').write_text(json.dumps(convert(files, label_hashes(table))), encoding='utf-8')
    return True
