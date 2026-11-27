#!/usr/bin/env python3
"""Fetch a pinned public CoMaps source and authenticated-by-hash offline region."""
import csv, hashlib, json, subprocess, urllib.request
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent

def prepare(base):
    base = base.resolve()
    assert base.is_relative_to(ROOT / 'build/comaps')
    base.mkdir(parents=True, exist_ok=True)
    pin = json.loads((HERE / 'sources.json').read_text())
    src = base / 'upstream'
    if not (src / '.git').exists():
        subprocess.run(['git', 'init', str(src)], check=True)
        subprocess.run(['git', '-C', str(src), 'fetch', '--depth=1', pin['repository'], pin['revision']], check=True)
        subprocess.run(['git', '-C', str(src), 'checkout', '--detach', 'FETCH_HEAD'], check=True)
    assert subprocess.check_output(['git','-C',str(src),'rev-parse','HEAD'],text=True).strip() == pin['revision']
    subprocess.run(['git','-C',str(src),'diff','--exit-code','HEAD'],check=True,stdout=subprocess.DEVNULL)
    region = base / 'Gibraltar.mwm'
    if not region.exists():
        with urllib.request.urlopen(pin['map_url'], timeout=60) as response:
            raw = response.read(pin['map_bytes'] + 1)
        assert len(raw) == pin['map_bytes'] and hashlib.sha256(raw).hexdigest() == pin['map_sha256']
        region.write_bytes(raw)
    assert region.stat().st_size == pin['map_bytes']
    assert hashlib.sha256(region.read_bytes()).hexdigest() == pin['map_sha256']
    # The generator assigns stable, one-based IDs in this CSV; MWM uses ID - 1.
    kinds = {}
    with (src/'data/mapcss-mapping.csv').open() as stream:
        for row in csv.reader(stream, delimiter=';'):
            if not row or row[0].startswith('#'): continue
            number = int(row[1] if len(row)<=4 else row[5]) - 1
            name = row[0]
            style = 0
            if name.startswith('natural|water') or name.startswith('waterway|riverbank'): style = 1
            elif name.startswith(('landuse|forest','natural|wood','leisure|park','natural|scrub','landuse|grass','natural|grassland','natural|heath')): style = 2
            elif name.startswith('building'): style = 3
            elif name.startswith('highway|') and not name.startswith(('highway|bus','highway|traffic','highway|speed','highway|cross','highway|stop','highway|give','highway|turn','highway|mini','highway|elevator','highway|street')):
                style = 5 if any(x in name for x in ('motorway','trunk','primary','secondary','tertiary')) else 4
            elif name.startswith('railway|rail'): style = 6
            elif name.startswith('natural|coastline'): style = 7
            elif name.startswith('place|'): style = 8
            elif name.startswith('landuse|') or name.startswith('natural|bare_rock'): style = 9
            kinds[number] = style
    count=max(kinds)+1
    values=[kinds.get(i,0) for i in range(count)]
    (base/'styles.h').write_text('/* Generated from pinned CoMaps mapcss-mapping.csv. */\n'
        +f'static const unsigned char map_styles[{count}]={{'+','.join(map(str,values))+'};\n')
    return pin,src,region

if __name__ == '__main__':
    pin,src,region=prepare(ROOT/'build/comaps')
    print(pin['region'],pin['revision'],region)
