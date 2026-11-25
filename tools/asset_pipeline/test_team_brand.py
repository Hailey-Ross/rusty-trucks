import struct
import unittest

from . import team_brand as tb


def arena(records):
    """A minimal RW4 arena: one binding section with a 16-byte header and the 32-byte records."""
    body = struct.pack('>II8x', len(records), 16) + b''.join(struct.pack('>4Q', *r) for r in records)
    table = 64
    base = table + 24
    head = bytearray(b'\x89RW4xb2' + bytes(25))
    head += struct.pack('>I', 1) + bytes(12) + struct.pack('>I', table) + bytes(12)
    head += struct.pack('>6I', base, 0, len(body), 0, 0, tb.BINDINGS)
    return bytes(head) + body


class TeamBrandTest(unittest.TestCase):
    def test_binding_records_read_texture_and_tier(self):
        raw = arena([(1, 2, 0x2C70170A000700BC, 0x12E7716775F3110E), (3, 4, 0x2C70170A00060009, 0x4C5ACD841A00BFFE)])
        rows = tb.arena_bindings(raw)
        self.assertEqual([(r['texture'], r['tier']) for r in rows],
                         [('0x2c70170a000700bc', '12E7716775F3110E'), ('0x2c70170a00060009', '4C5ACD841A00BFFE')])

    def test_a_header_past_its_section_is_refused(self):
        raw = bytearray(arena([(1, 2, 3, 4)]))
        struct.pack_into('>I', raw, 64 + 24, 9)
        with self.assertRaises(ValueError):
            tb.arena_bindings(bytes(raw))


if __name__ == '__main__':
    unittest.main()
