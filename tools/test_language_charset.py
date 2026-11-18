import struct
import unittest

from vendor.skate3_ui.language import character_table, decode_text


def histogram(entries: dict[int, int], count: int = 271) -> bytes:
    codes = [entries.get(i, 0) for i in range(count)]
    return struct.pack('<III', 0x39001, 4 + count * 2, count) + struct.pack(f'<{count}H', *codes)


class LanguageCharsetTests(unittest.TestCase):
    # The disc's English table: 0x81 = U+2019, 0xF7 = U+0164, 0x100 = U+0165, 0x10E = U+20AC.
    table = character_table(histogram({0x80: 0x0002, 0x81: 0x2019, 0x83: 0x201D, 0x84: 0x201C, 0xE9: 0x00E9,
                                       0xF7: 0x0164, 0x100: 0x0165, 0x10E: 0x20AC}))

    def test_a_byte_maps_through_the_table(self):
        self.assertEqual(decode_text(b"don\x81t", self.table), "don’t")
        self.assertEqual(decode_text(b"\x84Hi\x83 caf\xe9", self.table), "“Hi” café")

    def test_0x80_escapes_into_the_upper_entries(self):
        self.assertEqual(decode_text(b"Tt\xf7\x80\x80", self.table), "TtŤť")
        self.assertEqual(decode_text(b"\x80\x8e", self.table), "€")

    def test_no_table_or_no_entry_keeps_latin1(self):
        self.assertEqual(decode_text(b"caf\xe9\x81", None), "café\u0081")
        self.assertEqual(decode_text(b"\xa1", self.table), "¡")
        self.assertEqual(decode_text(b"plain ASCII", self.table), "plain ASCII")

    def test_a_bad_header_is_refused(self):
        with self.assertRaises(Exception):
            character_table(b"\0" * 20)


if __name__ == '__main__':
    unittest.main()
