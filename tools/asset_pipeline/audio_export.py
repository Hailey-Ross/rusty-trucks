"""Decode the owned disc's game audio into PCM16 WAV for the engine (assets/private/audio).

Only what the engine plays is exported (crates/skate-game/src/game_audio/): the
ambience beds, rolling grains, wheel spins, the sample banks below, and every
bank the per-map sound emitters (`sfx_*.ems`, `skateschool.ems`) place in the
world, with those emitters listed in the manifest. Audio is
written at its original level: nothing is normalised or boosted, and surround
ambience is downmixed to stereo by a weighted average (audio_formats.py).
"""
from __future__ import annotations

import json
import shutil
import struct
import subprocess
import wave
from pathlib import Path

from tools.owned_game.big import BigArchive
from .audio_formats import (downmix_pcm16, ems_emitters, grain, loop_bands, name_id, scan_snr, splc_patches,
                            splc_streams, standalone)

VERSION = 4

# Each rolling grain is a recording that sweeps from slow to fast rolling (about
# 10 dB louder and brighter by the end). It is cut into this many speed bands,
# each made loopable, so the engine can play the band matching board speed.
GRAIN_BANDS = 6
LOOP_CROSSFADE = 0.08  # seconds

# Pinned decoder (EA-XMA needs FFmpeg's XMA2 decoder, which this build bundles).
# The SHA-256 matches the digest GitHub publishes for the release asset.
VGMSTREAM_URL = 'https://github.com/vgmstream/vgmstream/releases/download/r2117/vgmstream-win64.zip'
VGMSTREAM_SHA = '6c4a8a3813864fefed081bbd337dbc0ad93bf88e0b92f5db98d7ab258b22dc6c'

# Per-map ambience beds (ambience.big + headers in ambienceresident.big).
AMBIENCE = (
    '01_dt_apt', '02_dt_less_busy', '03_dt_rez', '04_dt_main', '05_dt_parks', '06_dt_open',
    '07_univ_mt_high', '08_univ_mt_low', '09_univ_campus', '10_univ_housing',
    '11_indu_shipyard', '12_indu_drydock', '13_indu_quarry', '14_reclaimed_a', '15_indu_new_factory',
    '16_spillway', '17_space_park', '18_skate_school', '19_reclaimed_b', '20_indu_old_factory',
    '21_interior_arena_amb', '22_interior_tunnel_amb',
)

# Sample banks from audiofiles.big (data/audio/<name>).
BANKS = (
    'Skate_Collisions.bnk', 'Skate_Metal.bnk', 'sk8_foley.bnk', 'Sk82_Whsh_Bys.bnk',
    'Sk8_Air_Flip_Tricks.abk', 'GRINDS.abk', 'board_scrapes.abk', 'Brd_Squeaks.abk',
    'WHEEL_SKID_BANK.abk', 'FOOT_DRAG.abk', 'fstep_skateshoe1_sm.abk', 'Bodyslide.abk',
    'Rolling_Rattles.abk', 'Seams_Bank.abk', 'PatchBank_Rolling_Surfaces.abk', 'sense_of_speed.abk',
    'water_misc.abk', 'Foley_Cloth.abk',
    # Water emitters on the map's water (game_audio/water.rs) and splash tails.
    'water_lapping.abk', 'water_lapping_pond.abk', 'fountains_waterlaps_left.abk',
    'water_fountain.abk', 'ocean_wave_small.abk',
)

# vgmstream stops accepting input files somewhere between 600 and 1100 arguments.
_BATCH = 256


def _decode(vgmstream: Path, directory: Path, names: list[str], log) -> None:
    """Decode standalone `<name>` files in `directory` to `<name>.wav` beside them."""
    for start in range(0, len(names), _BATCH):
        batch = names[start:start + _BATCH]
        # Imported lazily: install imports this module's group function.
        from . import install as engine
        child = engine.spawn([vgmstream, '-i', '-o', '?f.wav', *batch], cwd=directory,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        with child as process:
            output = process.stdout.read()
            if process.wait():
                log.write(output)
                raise RuntimeError(f'vgmstream failed on {directory.name}')
        for name in batch:
            if not (directory/(name + '.wav')).is_file():
                raise RuntimeError(f'vgmstream did not decode {directory.name}/{name}')


def _wav_info(path: Path) -> dict:
    with wave.open(str(path)) as source:
        return {'channels': source.getnchannels(), 'sample_rate': source.getframerate(),
                'seconds': round(source.getnframes() / source.getframerate(), 4)}


def _stereo(source: Path, target: Path) -> dict:
    with wave.open(str(source)) as reader:
        channels, rate, width = reader.getnchannels(), reader.getframerate(), reader.getsampwidth()
        frames = reader.readframes(reader.getnframes())
    if width != 2:
        raise RuntimeError(f'{source.name}: expected PCM16 from vgmstream')
    mixed = downmix_pcm16(frames, channels)
    with wave.open(str(target), 'wb') as writer:
        writer.setnchannels(min(channels, 2))
        writer.setsampwidth(2)
        writer.setframerate(rate)
        writer.writeframes(mixed)
    return {**_wav_info(target), 'source_channels': channels}


def _grain_bands(source: Path, folder: Path, prefix: str) -> list[dict]:
    with wave.open(str(source)) as reader:
        channels, rate, width = reader.getnchannels(), reader.getframerate(), reader.getsampwidth()
        frames = reader.readframes(reader.getnframes())
    if width != 2 or channels != 1:
        raise RuntimeError(f'{source.name}: expected mono PCM16 from vgmstream')
    folder.mkdir(parents=True)
    bands = []
    for index, pcm in enumerate(loop_bands(frames, GRAIN_BANDS, round(LOOP_CROSSFADE * rate))):
        target = folder/f'{index}.wav'
        with wave.open(str(target), 'wb') as writer:
            writer.setnchannels(1)
            writer.setsampwidth(2)
            writer.setframerate(rate)
            writer.writeframes(pcm)
        bands.append({'file': f'{prefix}/{index}.wav', **_wav_info(target)})
    return bands


def _streams(name: str, data: bytes):
    streams = splc_streams(data) if data[:4] == b'SPLC' else scan_snr(data)
    if not streams:
        raise ValueError(f'{name} contains no decodable streams')
    return streams


# Emitter files whose sounds play in the world (the others place reverb, music,
# crowds and speakers).
SOUND_EMITTERS = ('sfx_', 'skateschool')


# The emitter attribute class in the disc's skatercollections database: one
# record per emitter sound, keyed by the same id as the `.ems` records.
EMITTER_CLASS = 'Hash_F0CEF367088EFFF8'
EMITTER_FIELDS = {
    'volume': 'volume',
    'Hash_BE88128A30BE926E': 'bank_file',
    'Hash_C493ED34D1D32521': 'patch',
    'Hash_6D18B8674D7E5337': 'kind',  # Sk8::Audio::eVolumeType: 1 = looping emitter, 5 = reverb zone
    'Hash_F209C093F40A4CCC': 'falloff',  # eVolumeFalloffType: 0 = (1-d)^2, 1 = 1-d, else flat
    'Hash_9908F2D75D7381BD': 'seconds',  # float, 10 by default (meaning not verified)
}


def emitter_attributes(collections: list[dict]) -> dict[int, dict]:
    """Each emitter sound's attributes by sound id, with inherited fields resolved."""
    records = {c['key']: c for c in collections if c['class'] == EMITTER_CLASS}

    def value(field):
        if field['type'].endswith('Text'):
            return field['data']
        raw = bytes.fromhex(field['data'])
        if field['type'].endswith('Float'):
            return round(struct.unpack('>f', raw)[0], 6)
        return struct.unpack('>i', raw)[0]

    resolved = {}
    for key in records:
        chain, cursor = [], key
        while cursor in records and len(chain) < 16:
            chain.append(records[cursor])
            cursor = records[cursor]['parent']
        fields = {}
        for record in reversed(chain):
            for name, field in record['fields'].items():
                if name in EMITTER_FIELDS:
                    fields[EMITTER_FIELDS[name]] = value(field)
        if key.startswith('Hash_'):
            resolved[int(key[5:], 16)] = fields
    return resolved


def emitters(files: BigArchive, attributes: dict[int, dict] | None = None) -> tuple[dict, set[str]]:
    """Every `.ems` file's records with their sound's attributes and bank (None
    when the disc has no such bank), and the banks placed by sound emitters."""
    attributes = attributes or {}
    banks = {}
    by_file = {Path(e.path).name.lower(): Path(e.path).name for e in files.entries}
    for entry in files.entries:
        name = Path(entry.path).name
        if name.endswith(('.abk', '.bnk')):
            # Ids hash the name either as the file is named or lower-cased (both occur).
            for key in {Path(name).stem, Path(name).stem.lower()}:
                banks[name_id(key)] = name
    listed, placed = {}, set()
    for entry in sorted(files.entries, key=lambda e: e.path):
        if not entry.path.endswith('.ems'):
            continue
        stem = Path(entry.path).stem
        records = ems_emitters(files.read(entry))
        for record in records:
            sound = dict(attributes.get(record['sound_id'], {}))
            named = sound.pop('bank_file', '')
            on_disc = by_file.get(named.lower()) if named else None
            bank = on_disc or banks.get(record['sound_id'])
            record.update(sound)
            record['bank'] = Path(bank).stem if bank else None
            record['sound_id'] = f"{record['sound_id']:016X}"
            if bank and stem.startswith(SOUND_EMITTERS):
                placed.add(bank)
        listed[stem] = records
    return listed, placed


def _collections(game_root: Path, work: Path) -> list[dict]:
    """The disc's skatercollections records (the same conversion the core group runs)."""
    from . import install as engine
    from .vlt import convert as convert_vlt
    database = work/'database'
    engine.extract(game_root/'data/big/db.big', database, lambda e: Path(e.path).name.lower() in {
        'skaterschema.bin', 'skaterschema.vlt', 'skatercollections.bin', 'skatercollections.vlt'})
    names = (engine.TOOLS/'asset_pipeline/names.txt').read_text(encoding='utf-8').splitlines()
    return convert_vlt(database/'data/db/skaterschema', database/'data/db/skatercollections', names)['collections']


def convert(game_root: Path, private: Path, work: Path, vgmstream: Path, report, log) -> dict:
    """Write private/audio/** and private/audio/audio_manifest.json; return the manifest."""
    audio_root = game_root/'data/audio'
    output = private/'audio'
    if output.exists():
        shutil.rmtree(output)
    work.mkdir(parents=True, exist_ok=True)
    manifest = {'version': VERSION, 'ambience': {}, 'grains': {}, 'wheels': {}, 'banks': {}, 'patches': {}}

    report('Decoding ambience')
    beds, headers = BigArchive(audio_root/'ambience.big'), BigArchive(audio_root/'ambienceresident.big')
    bodies = {Path(e.path).stem: e for e in beds.entries if e.path.endswith('.sns')}
    heads = {Path(e.path).stem: e for e in headers.entries if e.path.endswith('.snr')}
    folder = work/'ambience'
    folder.mkdir()
    for name in AMBIENCE:
        if name not in bodies or name not in heads:
            raise KeyError(f'ambience bed {name} is missing from the disc')
    (output/'ambience').mkdir(parents=True)
    for name in AMBIENCE:
        (folder/(name + '.snr')).write_bytes(headers.read(heads[name]))
        (folder/(name + '.sns')).write_bytes(beds.read(bodies[name]))
        _decode(vgmstream, folder, [name + '.snr'], log)
        decoded = folder/(name + '.snr.wav')
        info = _stereo(decoded, output/'ambience'/(name + '.wav'))
        decoded.unlink()  # ~65 MB of five-channel PCM each
        manifest['ambience'][name] = {'file': f'ambience/{name}.wav', **info}

    for kind, archive, suffix in (('grains', 'grains.big', '.grain'), ('wheels', 'wheels.big', '.snr')):
        report(f'Decoding {kind}')
        source = BigArchive(audio_root/archive)
        folder = work/kind
        folder.mkdir()
        names = []
        for entry in source.entries:
            if not entry.path.endswith(suffix):
                continue
            stem, data = Path(entry.path).stem, source.read(entry)
            stream = grain(data).stream if suffix == '.grain' else scan_snr(data)[0]
            (folder/(stem + '.snr')).write_bytes(standalone(data, stream))
            names.append(stem)
        _decode(vgmstream, folder, [stem + '.snr' for stem in names], log)
        (output/kind).mkdir(parents=True)
        for stem in names:
            decoded = folder/(stem + '.snr.wav')
            if kind == 'grains':
                manifest[kind][stem] = {'bands': _grain_bands(decoded, output/kind/stem, f'{kind}/{stem}')}
                continue
            target = output/kind/(stem + '.wav')
            shutil.move(decoded, target)
            manifest[kind][stem] = {'file': f'{kind}/{stem}.wav', **_wav_info(target)}

    report('Decoding sound effect banks')
    files = BigArchive(audio_root/'audiofiles.big')
    by_name = {Path(e.path).name: e for e in files.entries}
    manifest['emitters'], placed = emitters(files, emitter_attributes(_collections(game_root, work)))
    for bank in BANKS + tuple(sorted(placed - set(BANKS))):
        if bank not in by_name:
            raise KeyError(f'sound bank {bank} is missing from the disc')
        data = files.read(by_name[bank])
        stem = Path(bank).stem
        folder = work/'banks'/stem
        folder.mkdir(parents=True)
        streams = _streams(bank, data)
        for index, stream in enumerate(streams):
            (folder/f'{index:04d}.snr').write_bytes(standalone(data, stream))
        _decode(vgmstream, folder, [f'{index:04d}.snr' for index in range(len(streams))], log)
        target_folder = output/'banks'/stem
        target_folder.mkdir(parents=True)
        samples = []
        for index in range(len(streams)):
            target = target_folder/f'{index:04d}.wav'
            shutil.move(folder/f'{index:04d}.snr.wav', target)
            samples.append({'file': f'banks/{stem}/{index:04d}.wav', **_wav_info(target)})
        manifest['banks'][stem] = samples
        if data[:4] == b'SPLC':
            # Retail's own layering/randomisation per sound (audio_formats.splc_patches).
            manifest['patches'][stem] = splc_patches(data)

    (output/'audio_manifest.json').write_text(json.dumps(manifest, indent=1), encoding='utf-8')
    return manifest
