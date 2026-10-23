import struct
import unittest

from .teleporters import CHALLENGE, indicator
from .visual_indicators import SPLINES, parse_splines


def arena(segments):
    """A minimal RW4 arena with one VISUALINDICATORDATA object holding one spline of `segments` 4x4 rows."""
    body = struct.pack('>4I', 1, len(segments), 16, 80)
    body += struct.pack('>QQ', 0x1234, 0x0001DA7E03E38707) + struct.pack('>4f', -1, 0, 0, 0) + struct.pack('>4f', 1, 2, 0, 0)
    body += struct.pack('>4I', 3, len(segments), 80, 0)
    for rows in segments:
        body += b''.join(struct.pack('>4f', *r, 1.0 if i == 3 else 0.0) for i, r in enumerate(rows))
    base = 0x40
    table = base + len(body)
    head = bytearray(b'\x89RW4xb2'.ljust(0x40, b'\0'))
    struct.pack_into('>I', head, 0x20, 1)
    struct.pack_into('>I', head, 0x30, table)
    return bytes(head) + body + struct.pack('>6I', base, 0, len(body), 16, 0x24, SPLINES)


class VisualIndicatorTests(unittest.TestCase):
    def test_splines_read_power_basis_segments(self):
        rows = [(0, 0.671, 0), (0, -1.007, 0), (0, 0, 0), (-0.487, 1.489, 0)]
        splines = parse_splines(arena([rows]))
        spline = splines['0001DA7E03E38707']
        self.assertEqual((spline['kind'], spline['name'], len(spline['segments'])), (3, '0000000000001234', 1))
        self.assertAlmostEqual(spline['segments'][0][3][1], 1.489, places=5)
        self.assertEqual(spline['max'], [1.0, 2.0, 0.0])

    def test_challenge_indicator_inherits_the_group_key_and_uses_its_own_locator(self):
        rows = {(CHALLENGE, 'tele_world_to_stadium'): ('teleports', {'locator': 'tele_world_to_stadium_origin_locator_01'}),
                (CHALLENGE, 'teleports'): ('main', {'indicator': 0x3A6EF9FB9B0381CF}),
                (CHALLENGE, 'main'): ('', {})}
        matrix = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [373.88, 64.18, -61.72, 1]]
        found = indicator(rows, 'tele_world_to_stadium', {0x3A6EF9FB9B0381CF: 'teleport'}, lambda name: [matrix])
        self.assertEqual(found, dict(key='teleport', locator='tele_world_to_stadium_origin_locator_01', matrix=matrix))
        self.assertIsNone(indicator(rows, 'teleports', {}, lambda name: [matrix]))  # the group has no locator
        self.assertIsInstance(indicator(rows, 'tele_world_to_stadium', {}, lambda name: []), str)


if __name__ == '__main__':
    unittest.main()
