import io
import json
import os
import struct
import tempfile
import unittest
import zlib
from pathlib import Path
from types import SimpleNamespace

import numpy as np
from PIL import Image
from unittest import mock

from . import map_writer
from .map_writer import SpawnSelector, write_textures


class MapWriterTests(unittest.TestCase):
    def test_parallel_textures_match_serial_format_exactly(self):
        with tempfile.TemporaryDirectory() as work:
            root=Path(work);textures={};expected=io.BytesIO()
            rng=np.random.default_rng(23)
            for name, cube, png in [('z-cube', True, False), ('a-rgba', False, False), ('m-png', False, True), ('b-flat', False, False)]:
                pixels=rng.integers(0,256,(24 if cube else 4,4,4),dtype=np.uint8)
                if name=='b-flat':pixels[:]=0
                path=root/(name+('.png' if png else '.rgba'))
                if png:Image.fromarray(pixels).save(path)
                else:path.write_bytes(pixels.tobytes())
                textures[name]=dict(width=4,height=len(pixels),cube_faces=6 if cube else 1,
                                    **{'png' if png else 'rgba':path.name})
            for name,entry in sorted(textures.items()):
                if 'png' in entry:
                    with Image.open(root/entry['png']) as image:pixels=np.array(image.convert('RGBA'))
                else:pixels=np.frombuffer((root/entry['rgba']).read_bytes(),dtype=np.uint8).reshape(entry['height'],4,4)
                raw=(pixels if entry['cube_faces']==6 else pixels[::-1]).tobytes()
                packed=zlib.compress(raw,1);method=1
                if len(packed)>=len(raw):packed=raw;method=0
                encoded=name.encode()
                expected.write(struct.pack('<I',len(encoded))+encoded)
                expected.write(struct.pack('<5I',4,entry['height'],1,method,len(packed))+packed)
            actual=io.BytesIO();write_textures(actual,root,textures)
            self.assertEqual(actual.getvalue(),expected.getvalue())
            # Same bytes whatever the worker count (setup_budget.job_threads).
            for workers in (1,2,3,7):
                actual=io.BytesIO();write_textures(actual,root,textures,workers)
                self.assertEqual(actual.getvalue(),expected.getvalue(),workers)
            empty=io.BytesIO();write_textures(empty,root,{})
            self.assertEqual(empty.getvalue(),b'')

    def test_stored_blob_layout_is_unchanged(self):
        rng=np.random.default_rng(5)
        for data in (b'',b'x',bytes(1000),rng.integers(0,256,4096,dtype=np.uint8).tobytes()):
            packed=zlib.compress(data,1);method=1
            if len(packed)>=len(data):packed=data;method=0
            out=io.BytesIO();map_writer.stored(out,data)
            self.assertEqual(out.getvalue(),struct.pack('<II',method,len(packed))+packed)
            self.assertEqual(b''.join(map_writer.packed_blob(data)),out.getvalue())

    def test_whole_map_is_identical_for_any_thread_count(self):
        """write() compresses textures and the geometry/extension blobs on
        threads; the file must not depend on how many (old code = 2 texture
        workers, blobs inline)."""
        with tempfile.TemporaryDirectory() as work:
            root=Path(work);rng=np.random.default_rng(11);textures={}
            for n in range(9):
                name=f'0x{n:016x}';pixels=rng.integers(0,256,(8,8,4),dtype=np.uint8)
                (root/(name+'.rgba')).write_bytes(pixels.tobytes())
                textures[name]=dict(width=8,height=8,rgba=name+'.rgba')
            arrays={};meshes=[]
            for i in range(3):
                arrays[f'vertices_{i}']=rng.normal(size=(30,3)).astype('<f4')
                arrays[f'faces_{i}']=rng.integers(0,30,(20,3)).astype('<u4')
                arrays[f'uvs_{i}']=rng.random((30,2)).astype('<f4')
                meshes.append(dict(index=i,texture_id=f'0x{i:016x}',alpha_mode=i%3,source_offsets={}))
            np.savez(root/'model.npz',**arrays)
            manifest=dict(map_name='Synthetic',district_name='DIST_Synthetic',textures=textures,
                          models=[dict(asset_id='0xabc',npz='model.npz',meshes=meshes)],
                          normal_texture_policy=dict(excluded_texture_ids=[]),grind_splines=[],
                          other_presentation_assets=[])
            (root/'manifest.json').write_text(json.dumps(manifest))
            (root/'collision.rwcmset').write_bytes(rng.integers(0,256,5000,dtype=np.uint8).tobytes())
            outputs={}
            for threads in (1,2,6):
                for render_only in (False,True):
                    with mock.patch.dict(os.environ,{'SKATE_SETUP_THREADS':str(threads)}):
                        out=root/f'out-{threads}-{render_only}.skate'
                        map_writer.write(root/'manifest.json',out,root/'collision.rwcmset',
                                         render_only=render_only,prepared_spawn=(1.,2.,3.))
                        outputs.setdefault(render_only,set()).add(out.read_bytes())
            self.assertEqual(len(outputs[False]),1)
            self.assertEqual(len(outputs[True]),1)
            data=next(iter(outputs[False]))
            # The tail is the extension list: RWCM then WMET, each a stored blob.
            wmet=json.dumps(manifest,separators=(',',':')).encode()
            packed=b''.join(map_writer.packed_blob(wmet))
            self.assertTrue(data.endswith(b'WMET'+struct.pack('<II',1,len(wmet))+packed))
            rwcm=(root/'collision.rwcmset').read_bytes()
            self.assertIn(b'RWCM'+struct.pack('<II',1,len(rwcm))+b''.join(map_writer.packed_blob(rwcm)),data)

    def test_texture_failure_is_propagated(self):
        with tempfile.TemporaryDirectory() as work:
            with self.assertRaises(FileNotFoundError):
                write_textures(io.BytesIO(),Path(work),{'missing':dict(rgba='missing',width=4,height=4)})

    def test_spawn_streaming_preserves_ties_and_university_height(self):
        def mesh(x,y,z):
            return SimpleNamespace(bounds_min=(x-10,y,z-10),bounds_max=(x+10,y,z+10),triangles=[
                SimpleNamespace(a=(x-10,y,z-10),b=(x,y,z+10),c=(x+10,y,z-10))])
        selector=SpawnSelector('DIST_Test')
        selector.consider([mesh(0,5,0),mesh(0,20,0)])
        self.assertEqual(selector.result('test'),(0.,6.,-10/3))
        selector.consider([mesh(500,30,500)])
        self.assertEqual(selector.result('test'),(0.,6.,-10/3))
        university=SpawnSelector('DIST_University')
        university.consider([mesh(330,100,-710),mesh(330,132,-710),mesh(0,132,0)])
        self.assertEqual(university.result('University'),(330.,133.,-710.))
        with self.assertRaisesRegex(ValueError,'No supported spawn'):
            SpawnSelector('DIST_Test').result('test')


if __name__=='__main__':unittest.main()
