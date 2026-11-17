import struct
import unittest

from tools.asset_pipeline import subtitles


def table(rows):
    """A labels file: 28-byte header (count +8, size 28 +12, pool +16), `{hash, offset}` records, 8-byte pool head."""
    pool = b''
    records = b''
    for key, label in rows:
        records += struct.pack('<II', key, len(pool))
        pool += label.encode('latin-1') + b'\0'
    header = struct.pack('<IIIII', 0, 0, len(rows), 28, 28 + len(records)) + b'\0' * 8
    return header + records + b'\0' * 8 + pool


class SubtitleTests(unittest.TestCase):
    def test_cues_are_little_endian_and_stop_at_the_end_record(self):
        data = struct.pack('<HHIHHIHHI', 43, 0xD7, 0x4CA6AF68, 0xFFFF, 0, 0x1A20BA, 7, 0, 1)
        self.assertEqual(subtitles.cues(data), [(43, 0x4CA6AF68), (0xFFFF, 0x1A20BA)])

    def test_hashes_resolve_to_labels_and_unknown_ones_clear(self):
        labels = subtitles.label_hashes(table([(0x4CA6AF68, 'SUBTITLE_A'), (0x4CA6AF89, 'SUBTITLE_B')]))
        self.assertEqual(labels, {0x4CA6AF68: 'SUBTITLE_A', 0x4CA6AF89: 'SUBTITLE_B'})
        data = struct.pack('<HHIHHIHHI', 43, 0xD7, 0x4CA6AF68, 62, 0, 0x1A20BA, 0xFFFF, 0, 0x1A20BA)
        out = subtitles.convert({'data/fe/subtitles/SubTitles_English/02_ProChallenge_Cardiel.sub': data}, labels)
        self.assertEqual(out, {'languages': {'English': {'02_ProChallenge_Cardiel': [[43, 'SUBTITLE_A'], [62, None]]}}})


if __name__ == '__main__':
    unittest.main()
