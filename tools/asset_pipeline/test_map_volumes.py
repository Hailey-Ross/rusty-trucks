import json
import math
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path

from . import map_volumes as mv

ROOT = Path(__file__).resolve().parents[2]


def arena(sections):
    """RW4 arena with the given (type, payload) sections, in order."""
    table = 64
    offset = table + 24 * len(sections)
    raw = bytearray(offset)
    raw[:7] = b'\x89RW4xb2'
    struct.pack_into('>I', raw, 32, len(sections))
    struct.pack_into('>I', raw, 48, table)
    for index, (kind, payload) in enumerate(sections):
        offset = len(raw)
        struct.pack_into('>6I', raw, table + 24 * index, offset, 0, len(payload), 16, index, kind)
        raw += payload
    return bytes(raw)


def box_volume(centre, half, yaw=0.0, kind=4):
    payload = bytearray(96)
    c, s = math.cos(yaw), math.sin(yaw)
    # Rows are the box's local axes in world space (row-vector convention).
    rows = [(c, 0.0, -s), (0.0, 1.0, 0.0), (s, 0.0, c)]
    for r, row in enumerate(rows):
        struct.pack_into('>4f', payload, 16 * r, *row, 0.0)
    struct.pack_into('>4f', payload, 48, *centre, 0.0)
    struct.pack_into('>I3f', payload, 64, kind, *half)
    struct.pack_into('>I', payload, 92, 1)
    return bytes(payload)


def volume_set(items):
    """items: (name, lo, hi, link GUID, instance id, link section index)."""
    names = b''
    offsets = []
    strings = 32 + mv.ITEM_SIZE * len(items)
    for item in items:
        offsets.append(strings + len(names))
        names += item[0].encode() + b'\0'
    raw = bytearray(strings) + names
    struct.pack_into('>5I', raw, 0, mv.SET_MAGIC, len(items), len(items), 32, strings)
    for i, (name, lo, hi, guid, iid, link) in enumerate(items):
        at = 32 + mv.ITEM_SIZE * i
        struct.pack_into('>16f', raw, at, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)
        struct.pack_into('>3f', raw, at + 64, *lo)
        struct.pack_into('>3f', raw, at + 80, *hi)
        struct.pack_into('>QQ', raw, at + 176, guid, iid)
        struct.pack_into('>II', raw, at + 220, link, offsets[i])
    return bytes(raw)


def link(box_index, mesh_index=0):
    return struct.pack('>5I', box_index, 1, 12, mesh_index, 0)


class TriggerVolumeExportTests(unittest.TestCase):
    def sample(self, yaw=0.0, half=(2.0, 1.0, 3.0), lo=None, hi=None, kind=4):
        centre = (10.0, 1.0, -5.0)
        if lo is None:
            ex = abs(math.cos(yaw)) * half[0] + abs(math.sin(yaw)) * half[2]
            ez = abs(math.sin(yaw)) * half[0] + abs(math.cos(yaw)) * half[2]
            lo = (centre[0] - ex, centre[1] - half[1], centre[2] - ez)
            hi = (centre[0] + ex, centre[1] + half[1], centre[2] + ez)
        name = 'tele_test_volume_01_0x0000041203e38705:0x2c7017060025128f::[0x2c701704002e0002]_HighLOD'
        return arena([
            (0x00EB0008, b'\0' * 16),
            (mv.VOLUME_SET, volume_set([(name, lo, hi, 0x971B7CF0E0BC510A, 0x2C7017060025128F, 2)])),
            (mv.LINK_RECORD, link(3)),
            (mv.RW_VOLUME, box_volume(centre, half, yaw, kind)),
        ])

    def test_item_exports_identity_name_group_and_oriented_box(self):
        [volume] = mv.arena_volumes(self.sample())
        self.assertEqual(volume['name'], 'tele_test_volume_01')
        self.assertTrue(volume['full_name'].endswith('_HighLOD'))
        self.assertEqual(volume['id'], '2c7017060025128f')
        self.assertEqual(volume['instance_id'], '2c7017060025128f')
        self.assertEqual(volume['link_guid'], '971b7cf0e0bc510a')
        self.assertEqual(volume['group'], 'challenge')
        shape = volume['shape']
        self.assertEqual(shape['kind'], 'box')
        self.assertEqual(shape['center'], [10.0, 1.0, -5.0])
        self.assertEqual(shape['half_extents'], [2.0, 1.0, 3.0])
        self.assertEqual(shape['fatness'], 0.0)
        self.assertEqual(volume['aabb'], {'min': [8.0, 0.0, -8.0], 'max': [12.0, 2.0, -2.0]})

    def test_rotated_box_keeps_its_axes_not_its_bounds(self):
        [volume] = mv.arena_volumes(self.sample(yaw=math.pi / 4, half=(3.0, 1.0, 3.0)))
        axes = volume['shape']['axes']
        self.assertAlmostEqual(axes[0][0], math.sqrt(0.5), places=5)
        self.assertAlmostEqual(axes[0][2], -math.sqrt(0.5), places=5)
        self.assertEqual(volume['shape']['half_extents'], [3.0, 1.0, 3.0])

    def test_link_must_name_a_box_matching_the_bounds(self):
        with self.assertRaisesRegex(ValueError, 'not a box'):
            mv.arena_volumes(self.sample(kind=6))
        with self.assertRaisesRegex(ValueError, 'does not match'):
            mv.arena_volumes(self.sample(lo=(0, 0, 0), hi=(1, 1, 1)))
        raw = bytearray(self.sample())
        # Point the item at the box volume instead of its link record.
        base = struct.unpack_from('>I', raw, 64 + 24)[0]
        struct.pack_into('>I', raw, base + 32 + 220, 3)
        with self.assertRaisesRegex(ValueError, 'link record'):
            mv.arena_volumes(bytes(raw))

    def test_bad_set_header_is_rejected(self):
        raw = bytearray(self.sample())
        base = struct.unpack_from('>I', raw, 64 + 24)[0]
        struct.pack_into('>I', raw, base, 0x12345678)
        with self.assertRaisesRegex(ValueError, 'header'):
            mv.arena_volumes(bytes(raw))

    def test_short_name_strips_the_editor_path(self):
        self.assertEqual(mv.short_name('tut_sksc_reset_vol01_0x0000:0x1::[0x2]_HighLOD'), 'tut_sksc_reset_vol01')
        self.assertEqual(mv.short_name('custom'), 'custom')

    def test_document_is_versioned_json(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = mv.write(Path(tmp) / 'Park.triggers', 'Park', mv.arena_volumes(self.sample()))
            value = json.loads(path.read_text(encoding='utf-8'))
        self.assertEqual((value['format'], value['version'], value['map']), (mv.FORMAT, 1, 'Park'))
        self.assertEqual(len(value['volumes']), 1)


# Research (notes triggers-volumes-re.md §1; PR #25's surfaceless boxes): 11 volumes in the world streams.
RETAIL = {
    'SkateSchool': ['coach_frank_sksc', 'ws_sksc_coachfrank_instance_01', 'tut_sksc_inthehub_vol_01',
                    'tut_sksc_inthehub_vol_02', 'tut_sksc_reset_vol01', 'tut_sksc_wrongway_vol_01'],
    'MegaPark': ['tele_stadium_to_world_volume_a'],
    'MaloofMoneyCup': ['tele_mega_ramp_up_volume_01', 'tele_mmcp_to_dwtn_volume_01'],
    'DownTown': ['dwtn_sessionspot_01_kubetower_volume_01', 'dwtn_sessionspot_02_spillway_volume_02'],
    'University': [], 'Industrial': [], 'StartPark': [], 'BlackBoxPark': [],
    'DownTownSkatePark': [], 'IndustrialSkatePark': [],
}


def disc_content():
    value = os.environ.get('SKATE3_DISC_CONTENT')
    path = Path(value) if value else ROOT / '.local/skate3-disc/data/content'
    return path if (path / 'worldDIST_SkateSchool.big').is_file() else None


@unittest.skipUnless(disc_content(), 'needs the extracted disc (SKATE3_DISC_CONTENT)')
class RetailTriggerVolumeTests(unittest.TestCase):
    """Data-gated: the owned disc's districts, read in place."""

    @classmethod
    def setUpClass(cls):
        sys.path.insert(0, str(ROOT / 'tools/vendor/university/tools/vanilla_map_extraction/tools'))
        cls.tmp = tempfile.TemporaryDirectory()
        cls.rows = {}
        for label in RETAIL:
            cls.rows[label] = mv.export_archive(disc_content() / f'worldDIST_{label}.big', cls.tmp.name)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_counts_and_names_match_the_research(self):
        self.assertEqual({k: [v['name'] for v in rows] for k, rows in self.rows.items()}, RETAIL)
        self.assertEqual(sum(len(r) for r in self.rows.values()), 11)

    def test_known_volumes(self):
        school = {v['name']: v for v in self.rows['SkateSchool']}
        reset = school['tut_sksc_reset_vol01']
        self.assertEqual(reset['id'], '2c7017060025128f')
        self.assertEqual(reset['link_guid'], 'a2456e5cb2b3f6ae')
        self.assertEqual([round(v, 3) for v in reset['aabb']['min']], [-211.973, -73.027, -482.759])
        self.assertEqual([round(v, 3) for v in reset['shape']['half_extents']], [487.281, 76.021, 394.996])
        # inthehub_vol_02 is a 45 degree square; its box is not its bounds.
        hub = school['tut_sksc_inthehub_vol_02']['shape']
        self.assertAlmostEqual(abs(hub['axes'][0][0]), math.sqrt(0.5), places=3)
        kube = {v['name']: v for v in self.rows['DownTown']}['dwtn_sessionspot_01_kubetower_volume_01']
        self.assertEqual(kube['stream'], 'cSim_250_-150_high.xsf')
        self.assertEqual([round(v, 2) for v in kube['aabb']['min']], [259.07, 48.88, -152.19])
        for rows in self.rows.values():
            for volume in rows:
                self.assertEqual(volume['group'], 'challenge')
                self.assertEqual(volume['shape']['fatness'], 0.0)


if __name__ == '__main__':
    unittest.main()
