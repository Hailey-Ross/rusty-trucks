"""Create-a-Skater part grids (FE 15 menu 10; research b1034 / b1035): every catalog (model, material instance) pair
with the fields the retail list builder, the thumbnails and the info panel read, as `cas-parts.json` next to
catalog.json. The game lists a category's parts from it by slot and flags (data a mod can extend)."""
import json
from pathlib import Path

VERSION = 2
# cas.* fields the part filters, the list sort and the info panel use (b1035: Gender, TopType, SockStyle,
# Location; b1034: Unlock, SponsorUnlock, ManufacturerID (brand), Cost, DlcID; the hidden flag IsUsedInGame and the
# wear style groups StyleSubcategory / IsDefaultWearStyle, docs/hails-additions/43-career.md#create-a-skater).
FIELDS = ('Gender', 'TopType', 'SockStyle', 'Location', 'Unlock', 'SponsorUnlock', 'ManufacturerID', 'Cost', 'DlcID',
          'IsUsedInGame', 'IsDefaultWearStyle', 'StyleSubcategory')


def parts(catalog: dict) -> list:
    """One entry per (model, material instance of its first LOD), in catalog order."""
    materials = catalog.get('materials', {})
    out = []
    for component in catalog.get('components', []):
        for model in component.get('models', []):
            lods = model.get('lods') or []
            for group in (lods[0].get('material_instances') or []) if lods else []:
                for instance in group:
                    flags = {**(materials.get(instance['id'], {}).get('flags') or {}), **(model.get('flags') or {})}
                    entry = {'slot': component['slot'], 'model_id': model['id'], 'model': model['name'],
                             'material_id': instance['id'], 'material': instance['name']}
                    entry.update({f.lower(): flags.get('cas.' + f, '') for f in FIELDS})
                    out.append(entry)
    return out


def write(directory: Path) -> int:
    catalog = json.loads((Path(directory) / 'catalog.json').read_text(encoding='utf-8'))
    listed = parts(catalog)
    (Path(directory) / 'cas-parts.json').write_text(json.dumps({'version': VERSION, 'parts': listed}, separators=(',', ':')),
                                                   encoding='utf-8')
    return len(listed)
