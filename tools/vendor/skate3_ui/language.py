from __future__ import annotations

import struct
from dataclasses import dataclass
from pathlib import Path

from .binary import FormatError


@dataclass(frozen=True)
class LanguageTable:
    source: str
    declared_count: int
    pool_offset: int
    strings: tuple[str, ...]


def character_table(histogram: bytes) -> tuple[int, ...]:
    """The FE language character table (LANGUAGE_<lang>_Histogram.bin): a 12-byte header (magic 0x39001, payload
    length, entry count), then one little-endian UTF-16 code per entry. Entry 0 = no mapping."""
    if len(histogram) < 12:
        raise FormatError("language histogram is shorter than its header")
    magic, _, count = struct.unpack_from("<III", histogram, 0)
    if magic != 0x39001 or 12 + count * 2 > len(histogram):
        raise FormatError("unsupported language histogram")
    return struct.unpack_from(f"<{count}H", histogram, 12)


def decode_text(raw: bytes, table: tuple[int, ...] | None) -> str:
    """Language bytes to text. A byte maps to table[byte]; 0x80 then n maps to table[0x100 + n - 0x80] (the keyboard
    rows: "Tt" F7 80 80 = T t T-caron t-caron). No table, or no entry: the byte as Latin-1."""
    if not table:
        return raw.decode("latin-1")
    out = []
    i = 0
    while i < len(raw):
        b = raw[i]
        code = 0
        if b == 0x80 and i + 1 < len(raw) and raw[i + 1] >= 0x80:
            index = 0x100 + raw[i + 1] - 0x80
            code = table[index] if index < len(table) else 0
            if code:
                i += 1
        elif b < len(table):
            code = table[b]
        out.append(chr(code) if code else chr(b))
        i += 1
    return "".join(out)


def parse_language_table(path: Path, table: tuple[int, ...] | None = None) -> LanguageTable:
    path = Path(path)
    data = path.read_bytes()
    if len(data) < 28:
        raise FormatError(f"{path}: language table is shorter than its header")
    declared_count = struct.unpack_from("<I", data, 8)[0]
    header_size = struct.unpack_from("<I", data, 12)[0]
    pool_offset = struct.unpack_from("<I", data, 16)[0]
    if header_size != 28:
        raise FormatError(f"{path}: unsupported language header size {header_size}")
    # The pool descriptor occupies eight bytes at pool_offset. The actual
    # null-separated byte strings follow it.
    strings_offset = pool_offset + 8
    if strings_offset > len(data):
        raise FormatError(f"{path}: string pool lies outside the file")
    raw_strings = data[strings_offset:].split(b"\0")
    # Bytes 0x80+ go through the language's character table (the fonts carry
    # the Unicode codes: U+2019 for byte 0x81); without one, Latin-1.
    strings = tuple(decode_text(value, table) for value in raw_strings)
    expected = declared_count + 2
    if len(strings) != expected:
        raise FormatError(
            f"{path}: expected {expected} pool strings, found {len(strings)}"
        )
    return LanguageTable(str(path), declared_count, pool_offset, strings)


def pair_language_tables(labels_path: Path, values_path: Path, histogram_path: Path | None = None) -> dict:
    table = character_table(Path(histogram_path).read_bytes()) if histogram_path else None
    labels = parse_language_table(labels_path)
    values = parse_language_table(values_path, table)
    if len(labels.strings) != len(values.strings):
        raise FormatError(
            "language label/value pools have different numbers of strings"
        )

    entries = []
    seen: dict[str, int] = {}
    duplicate_labels = []
    for index, (label, value) in enumerate(zip(labels.strings, values.strings)):
        if label:
            if label in seen:
                duplicate_labels.append(
                    {"label": label, "first_index": seen[label], "index": index}
                )
            else:
                seen[label] = index
        entries.append({"index": index, "label": label, "value": value})

    return {
        "format": "skate3-language-pair",
        "encoding": "histogram-table" if table else "iso-8859-1-byte-preserving",
        "character_table": list(table) if table else None,
        "labels_source": labels.source,
        "values_source": values.source,
        "declared_count": labels.declared_count,
        "pool_entries": len(entries),
        "entries": entries,
        "duplicate_labels": duplicate_labels,
    }
