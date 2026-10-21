"""The player's own Skate 3 DLC packages as an optional setup input (research b374 / b410).

Setup never needs DLC: with no folder or no packages everything runs as before. When a folder holds Skate 3
marketplace packages (STFS, title 0x454108E6), their BIG archives are copied out to the conversion work folder and
offered to the exporters next to the disc's archives. Nothing from a package is committed or written back.
"""
from __future__ import annotations

import hashlib
import os
import struct
from pathlib import Path

ENV = 'SKATE3_DLC_DIR'


def folder(source: str | Path | None, saved: str | None = None) -> Path | None:
    """The DLC folder: SKATE3_DLC_DIR, else the folder the last setup used, else a `dlc` folder next to the game
    folder (the recomp's layout: `<install>/game/default.xex` + `<install>/dlc/<pack>/454108E6/...`)."""
    for candidate in (os.environ.get(ENV), saved):
        if candidate and Path(candidate).is_dir():
            return Path(candidate).resolve()
    if source:
        start = Path(source)
        start = start.parent if start.suffix else start
        for root in (start.parent, start):
            if (root / 'dlc').is_dir():
                return (root / 'dlc').resolve()
    return None


def packages(dlc: Path | None):
    if dlc is None:
        return []
    from .stfs import find_packages
    return find_packages(dlc)


def signature(dlc: Path | None) -> str:
    """A short identity of the packages found (display name, file name, size); '' without any."""
    found = packages(dlc)
    if not found:
        return ''
    digest = hashlib.sha256()
    for package in sorted(found, key=lambda p: p.path.name):
        digest.update(f'{package.display_name}|{package.path.name}|{package.path.stat().st_size}\n'.encode())
    return digest.hexdigest()[:16]


def big_paths(package, entry) -> list[str]:
    """The entry paths of a BIG inside a package, from its index alone (the archive is not copied)."""
    from tools.owned_game.big import BigArchive
    head = package.read(entry, 0, 48)
    metadata = int.from_bytes(head[12:16], 'big') + int.from_bytes(head[16:20], 'big')
    # BigArchive's index parser needs only the index bytes and the archive's size (for its bounds checks).
    index = BigArchive.__new__(BigArchive)
    index.path = Path(entry.path)
    index.file_size = entry.size
    index.metadata = package.read(entry, 0, metadata)
    return [e.path.replace('\\', '/') for e in index._parse()]


def extract_bigs(dlc: Path | None, work: Path, wanted=None) -> list[Path]:
    """Copies each package's BIG archives to `work/<package file>/<big>` (kept while the size matches) and returns
    their paths. `wanted(entry_paths)` sees the BIG's entry list (read from its index) and limits which are copied."""
    result = []
    for package in packages(dlc):
        for entry in package.files():
            if not entry.path.lower().endswith('.big') or (wanted and not wanted(big_paths(package, entry))):
                continue
            target = work / package.path.name / entry.path
            if not (target.is_file() and target.stat().st_size == entry.size):
                package.extract(entry, target)
            result.append(target)
    return result


def _strings(data: bytes) -> list[str]:
    # Same layout as vendor/skate3_ui/language.py: count at 8, header 28, pool descriptor (8 bytes) at the offset at
    # 16, then NUL-separated strings. The DLC label tables end with one NUL where the disc's end with two, so the
    # last (empty) string is padded here instead of rejecting the table (and extra empty padding is dropped).
    count, header, pool = struct.unpack_from('<III', data, 8)
    if header != 28 or pool + 8 > len(data):
        raise ValueError('unsupported language table')
    strings = [s.decode('latin-1') for s in data[pool + 8:].split(b'\0')]
    while len(strings) < count + 2:
        strings.append('')
    # Some DLC tables carry one more trailing NUL instead: drop empty padding past the declared count.
    while len(strings) > count + 2 and strings[-1] == '':
        strings.pop()
    if len(strings) != count + 2:
        raise ValueError(f'expected {count + 2} language strings, found {len(strings)}')
    return strings


def language(archive, language_name: str = 'english') -> dict[str, str]:
    """Label -> text from every labels / <language> table pair in a DLC BIG (e.g. the creator pack's
    LANGUAGE_Labels_creator_* + LANGUAGE_English_creator_*); HISTOGRAM tables are skipped."""
    tables = {}
    for entry in archive.entries:
        path = entry.path.replace('\\', '/')
        lower = path.lower()
        if not lower.startswith('data/fe/languages/') or 'histogram' in lower or not lower.endswith('.bin'):
            continue
        folder_name, name = lower.split('/')[-2:]
        if folder_name in ('labels', language_name):
            suffix = name.split('_', 2)[-1]
            tables.setdefault(suffix, {})[folder_name] = entry
    result = {}
    for pair in tables.values():
        if 'labels' in pair and language_name in pair:
            labels = _strings(archive.read(pair['labels']))
            values = _strings(archive.read(pair[language_name]))
            if len(labels) == len(values):
                for label, value in zip(labels, values):
                    if label:
                        result.setdefault(label.strip(), value)
    return result
