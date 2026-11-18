"""scene_clips.scene_actor_clips: the clip paths of the NIS scene table (doc 43 "NIS scenes")."""
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.asset_pipeline.scene_clips import SCENE_ACTOR, SCENE_RECORD, scene_actor_clips


def actor(character, kind, clip, scene):
    pad = lambda s, n: s.encode().ljust(n, b'\0')
    return pad(character, 32) + pad(kind, 16) + pad(clip, 64) + pad(scene, 64) + b'\0' * 4


class SceneActorClips(unittest.TestCase):
    def table(self, actors, records=1):
        return struct.pack('>4I', 0x7363656E, 1, records, len(actors)) + b'\0' * (records * SCENE_RECORD) + b''.join(actors)

    def test_clips_in_actor_order(self):
        raw = self.table([actor('benny_fairfax', 'marquee', 'scene/anim/tut_sksc_011.abin', 'tut_sksc_01'),
                          actor('male_adult_1', 'livingworld', 'scene/anim/x1.abin', 'x')])
        self.assertEqual(scene_actor_clips(raw), ['scene/anim/tut_sksc_011.abin', 'scene/anim/x1.abin'])
        self.assertEqual(SCENE_ACTOR, 180)

    def test_wrong_size_is_an_error(self):
        raw = self.table([actor('a', 'marquee', 'scene/anim/a1.abin', 'a')])
        with self.assertRaises(RuntimeError):
            scene_actor_clips(raw[:-1])


if __name__ == '__main__':
    unittest.main()
