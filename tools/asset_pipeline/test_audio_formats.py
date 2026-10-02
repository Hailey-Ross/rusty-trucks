"""EA SNR container helpers, checked on synthetic streams (never game audio)."""
import struct
import unittest

import numpy

from tools.asset_pipeline import audio_formats as audio


def snr(samples=(300, 200), rate=48000, channels=1, codec=3, loop_start=None, last_flag=True):
    """A fake RAM SNR stream: header plus one block per entry in `samples`."""
    header = struct.pack('>I', (codec << 24) | ((channels - 1) << 18) | rate)
    flags = (1 << 29 if loop_start is not None else 0) | sum(samples)
    data = header + struct.pack('>I', flags)
    if loop_start is not None:
        data += struct.pack('>I', loop_start)
    for index, count in enumerate(samples):
        payload = bytes([0x5A]) * (16 + index)
        flag = 0x80 if last_flag and index == len(samples) - 1 else 0x00
        data += struct.pack('>I', (flag << 24) | (8 + len(payload))) + struct.pack('>I', count) + payload
    return data


class SnrStreams(unittest.TestCase):
    def test_reads_header_and_block_chain(self):
        data = b'junk' + snr(rate=44100, channels=2)
        stream = audio.snr_at(data, 4)
        self.assertEqual((stream.channels, stream.sample_rate, stream.samples), (2, 44100, 500))
        self.assertEqual(stream.end, len(data))
        self.assertIsNone(stream.loop_start)
        self.assertEqual(audio.standalone(data, stream), data[4:])

    def test_loop_start_and_unflagged_final_block(self):
        stream = audio.snr_at(snr(loop_start=7, last_flag=False), 0)
        self.assertEqual((stream.loop_start, stream.samples), (7, 500))

    def test_rejects_inconsistent_chains(self):
        good = snr()
        self.assertIsNone(audio.snr_at(good[:-1], 0))                     # truncated block
        self.assertIsNone(audio.snr_at(snr(codec=5), 0))                  # not EA-XMA
        bad_count = bytearray(good)
        struct.pack_into('>I', bad_count, 4, 499)                         # header disagrees with blocks
        self.assertIsNone(audio.snr_at(bytes(bad_count), 0))

    def test_scan_finds_unaligned_back_to_back_streams(self):
        data = b'\x01\x02\x03' + snr() + b'\xff' + snr(samples=(64,)) + b'tail'
        streams = audio.scan_snr(data)
        self.assertEqual([s.offset for s in streams], [3, 3 + len(snr()) + 1])
        self.assertEqual([s.samples for s in streams], [500, 64])

    def test_splc_count_must_match(self):
        header = bytearray(0x40)
        header[:4] = b'SPLC'
        struct.pack_into('>I', header, 0x18, 2)
        bank = bytes(header) + snr() + snr()
        self.assertEqual(len(audio.splc_streams(bank)), 2)
        struct.pack_into('>I', header, 0x18, 3)
        with self.assertRaises(ValueError):
            audio.splc_streams(bytes(header) + snr() + snr())

    def test_grain_offset_and_duration(self):
        stream = snr(samples=(48000,))
        data = struct.pack('>If', 0x20, 1.0) + bytes(0x18) + stream
        parsed = audio.grain(data)
        self.assertEqual((parsed.stream.offset, parsed.stream.samples), (0x20, 48000))
        wrong = struct.pack('>If', 0x20, 2.0) + bytes(0x18) + stream
        with self.assertRaises(ValueError):
            audio.grain(wrong)


class SplcPatches(unittest.TestCase):
    @staticmethod
    def bank():
        # 2 records, 1 container, 3 samples: record 0 = 1 group of 2 members,
        # record 1 = 2 groups of 1 member; container 0 picks record 0 or 1.
        records = []
        for rid, groups in ((0, 1), (1, 2)):
            r = bytearray(36)
            struct.pack_into('>H', r, 4, rid)
            r[7] = groups
            records.append(bytes(r))
        container = bytearray(72)
        struct.pack_into('>HH', container, 4, 0, 1)
        container[68] = 2
        def group(mode, members):
            g = bytearray(12)
            g[8], g[9] = len(members), mode
            out = bytes(g)
            for sample, gain, pitch, gain_range, probability in members:
                m = bytearray(72)
                struct.pack_into('>H', m, 0, sample)
                for offset, value in ((8, gain), (44, pitch), (48, gain_range), (64, probability)):
                    struct.pack_into('>f', m, offset, value)
                out += bytes(m)
            return out
        tree = b''.join(records) + bytes(container) + group(2, [(0, 1.0, 1.2, 0.125, 1.0), (1, 1.0, 1.0, 0.0, 1.0)])             + group(1, [(2, 0.5, 1.0, 0.0, 0.75)]) + group(0, [(1, 1.0, 1.0, 0.0, 1.0)])
        header = bytearray(60)
        header[:4] = b'SPLC'
        struct.pack_into('>IIIII', header, 8, len(tree), 2, 1, 0, 3)
        return bytes(header) + tree

    def test_reads_records_layers_and_containers(self):
        patches = audio.splc_patches(self.bank())
        self.assertEqual(patches['containers'], [[0, 1]])
        self.assertEqual(len(patches['records']), 2)
        first = patches['records'][0]
        self.assertEqual([g['mode'] for g in first], [2])
        self.assertEqual(first[0]['members'], [[0, 1.0, 0.125, 1.2, 1.0], [1, 1.0, 0.0, 1.0, 1.0]])
        self.assertEqual([g['members'][0][0] for g in patches['records'][1]], [2, 1])
        self.assertEqual(patches['records'][1][0]['members'][0][4], 0.75)

    def test_rejects_a_tree_that_does_not_end_at_the_sample_table(self):
        data = bytearray(self.bank())
        struct.pack_into('>I', data, 8, struct.unpack_from('>I', data, 8)[0] + 4)
        with self.assertRaises(ValueError):
            audio.splc_patches(bytes(data))


class Downmix(unittest.TestCase):
    def test_weights_sum_to_one(self):
        for channels in (3, 4, 5, 6):
            for row in audio.downmix_weights(channels):
                self.assertAlmostEqual(sum(row), 1.0, places=6)
        self.assertIsNone(audio.downmix_weights(2))

    def test_five_channel_downmix_never_exceeds_input_peak(self):
        frames = numpy.array([[32767, 32767, 32767, 32767, 32767],
                              [-32768, -32768, -32768, -32768, -32768],
                              [1000, 0, -1000, 500, -500]], dtype='<i2')
        mixed = numpy.frombuffer(audio.downmix_pcm16(frames.tobytes(), 5), dtype='<i2').reshape(-1, 2)
        self.assertEqual(mixed[0].tolist(), [32767, 32767])
        self.assertEqual(mixed[1].tolist(), [-32768, -32768])
        # Left takes L, C and Ls only; right takes C, R and Rs.
        self.assertEqual(mixed[2].tolist(), [round(1500 / 2.7071), round(-1500 / 2.7071)])

    def test_stereo_passes_through(self):
        frames = numpy.array([[1, -2], [3, -4]], dtype='<i2').tobytes()
        self.assertEqual(audio.downmix_pcm16(frames, 2), frames)



class LoopBands(unittest.TestCase):
    def test_bands_wrap_seamlessly_without_gain(self):
        ramp = numpy.arange(6000, dtype=numpy.float32)
        signal = (numpy.sin(ramp * 0.05) * (2000 + ramp * 3)).astype('<i2')
        bands = audio.loop_bands(signal.tobytes(), 3, 100)
        self.assertEqual(len(bands), 3)
        part = signal[:2000].astype(numpy.int32)
        loop = numpy.frombuffer(bands[0], dtype='<i2').astype(numpy.int32)
        self.assertEqual(len(loop), 1900)
        # Wrapping from the loop's end to its start continues the original recording.
        self.assertEqual(loop[-1], part[1899])
        self.assertEqual(loop[0], part[1900])
        self.assertLessEqual(numpy.abs(loop).max(), numpy.abs(part).max())
        # Later bands come from later (louder) parts of the sweep.
        loudness = [numpy.abs(numpy.frombuffer(b, dtype='<i2').astype(numpy.float32)).mean() for b in bands]
        self.assertEqual(loudness, sorted(loudness))

    def test_too_short_recordings_are_rejected(self):
        with self.assertRaises(ValueError):
            audio.loop_bands(bytes(400), 3, 100)


if __name__ == '__main__':
    unittest.main()


class Emitters(unittest.TestCase):
    def test_name_id_matches_the_ids_on_the_disc(self):
        # Sound ids read from the disc's sfx_university.ems next to the bank they name.
        self.assertEqual(audio.name_id('water_fountain'), 0xFAE3503B95E0A3C8)

    def test_name_id_handles_keys_longer_than_one_block(self):
        long = 'a' * 30
        self.assertNotEqual(audio.name_id(long), audio.name_id(long[:24]))
        self.assertEqual(audio.name_id(long), audio.name_id('a' * 30))

    def test_reads_records(self):
        record = audio.EMS_RECORD.pack(7, 0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.5, -0.866, 0.0, 0.5,
                                       audio.name_id('water_fountain'), 1.0, 0.0, 0.5, 1.0)
        emitters = audio.ems_emitters(struct.pack('>I', 2) + record + record)
        self.assertEqual(len(emitters), 2)
        first = emitters[0]
        self.assertEqual((first['index'], first['position'], first['extent']), (7, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]))
        self.assertEqual(first['sound_id'], 0xFAE3503B95E0A3C8)
        self.assertEqual(first['gains'], [1.0, 0.0, 0.5, 1.0])
        self.assertAlmostEqual(first['scalars'][1], -0.866, places=5)

    def test_rejects_a_truncated_file(self):
        with self.assertRaises(ValueError):
            audio.ems_emitters(struct.pack('>I', 2) + bytes(72))


class EmitterAttributes(unittest.TestCase):
    def test_inherited_fields_resolve_through_parents(self):
        from tools.asset_pipeline.audio_export import EMITTER_CLASS, emitter_attributes

        def record(key, parent, **fields):
            return {'class': EMITTER_CLASS, 'key': key, 'parent': parent, 'fields': fields}
        f32 = lambda v: {'type': 'EA::Reflection::Float', 'data': struct.pack('>f', v).hex().upper()}
        i32 = lambda v, t='EA::Reflection::Int32': {'type': t, 'data': struct.pack('>i', v).hex().upper()}
        collections = [
            record('default', '', volume=f32(1.0), Hash_9908F2D75D7381BD=f32(10.0), Hash_6D18B8674D7E5337=i32(0, 'Sk8::Audio::eVolumeType')),
            record('Hash_0000000000000AAA', 'default', Hash_6D18B8674D7E5337=i32(1, 'Sk8::Audio::eVolumeType'),
                   Hash_F209C093F40A4CCC=i32(1, 'Sk8::Audio::eVolumeFalloffType')),
            record('Hash_FAE3503B95E0A3C8', 'Hash_0000000000000AAA', volume=f32(0.5),
                   Hash_BE88128A30BE926E={'type': 'EA::Reflection::Text', 'data': 'water_fountain.abk'},
                   Hash_C493ED34D1D32521=i32(81)),
            {'class': 'Hash_OTHER', 'key': 'Hash_0000000000000BBB', 'parent': '', 'fields': {}},
        ]
        sound = emitter_attributes(collections)[0xFAE3503B95E0A3C8]
        self.assertEqual(sound, {'volume': 0.5, 'seconds': 10.0, 'kind': 1, 'falloff': 1,
                                 'bank_file': 'water_fountain.abk', 'patch': 81})
        self.assertNotIn(0xBBB, emitter_attributes(collections))
