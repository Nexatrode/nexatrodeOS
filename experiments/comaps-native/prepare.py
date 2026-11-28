#!/usr/bin/env python3
"""Fetch a pinned public CoMaps source and authenticated-by-hash offline region."""
import csv, hashlib, json, subprocess, urllib.request, importlib.util
from urllib.parse import quote
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

def prepare_storage(base, out):
    """Produce a new public-map pack; never open an installed VM disk."""
    base,out=base.resolve(),out.resolve()
    assert base.is_relative_to(ROOT/'build/comaps') and out.is_relative_to(ROOT/'build')
    assert (out/'.disposable-build').is_file() and not any(p.suffix=='.utm' for p in (out,*out.parents))
    catalog=json.loads((HERE/'regions.json').read_text());regions=catalog['regions']
    assert len(regions)==3 and [r["id"] for r in regions]==list(range(3)) and [r["name"] for r in regions[:2]]==["Gibraltar","San Marino"] and all(16<r['bytes']<=8*1024*1024 for r in regions)
    paths=[]
    for r in regions:
        path=base/(r['name']+'.mwm')
        if not path.exists():
            with urllib.request.urlopen(catalog['base_url']+quote(r['name'])+'.mwm',timeout=60) as response:raw=response.read(r['bytes']+1)
            assert len(raw)==r['bytes'] and hashlib.sha256(raw).hexdigest()==r['sha256'];path.write_bytes(raw)
        assert path.stat().st_size==r['bytes'] and hashlib.sha256(path.read_bytes()).hexdigest()==r['sha256']
        paths.append(path)
    spec=importlib.util.spec_from_file_location('comaps_spatial',HERE/'spatial.py')
    spatial=importlib.util.module_from_spec(spec);spec.loader.exec_module(spatial)
    spatial_path,profile=spatial.prepare(base,paths[2],out/'spatial');paths[2]=spatial_path
    header='/* Generated from pinned public regions.json. */\nstruct map_region_pin { const char *name; unsigned length; unsigned char digest[32]; };\nstatic const struct map_region_pin map_regions[MAP_REGION_COUNT]={\n'
    for r in regions:header+='{'+json.dumps(r['name'])+','+str(r['bytes'])+',{'+','.join('0x'+r['sha256'][i:i+2] for i in range(0,64,2))+'}},\n'
    header+='};\nstruct map_page_pin { unsigned offset,length; unsigned char digest[32]; double bounds[4]; };\nstatic const struct map_page_pin map_pages[65]={\n'
    for page in profile['pages']:
        header+='{'+str(page['offset'])+','+str(page['bytes'])+',{'+','.join('0x'+page['sha256'][i:i+2] for i in range(0,64,2))+'},{'+','.join(map(repr,page['bounds']))+'}},\n'
    header+='};\nstatic const double map_spatial_bounds[4]={'+','.join(map(repr,profile['bounds']))+'};\nstatic const double map_spatial_origin[2]={'+','.join(map(repr,profile['origin']))+'};\n'
    index=profile['search']
    header+='static const struct map_page_pin map_search_pin={'+str(index['offset'])+','+str(index['bytes'])+',{'+','.join('0x'+index['sha256'][i:i+2] for i in range(0,64,2))+'},{0,0,0,0}};\n'
    (out/'regions_generated.h').write_text(header)
    catalog['pack_bytes']=64*1024*1024
    catalog['regions'][2]['spatial']=profile
    for i,r in enumerate(regions):r['offset']=(i+1)*8*1024*1024

    pack=out/'maps.img';assert not pack.is_symlink()
    with pack.open('wb') as disk:
        disk.truncate(catalog['pack_bytes'])
        for i,path in enumerate(paths):disk.seek((i+1)*8*1024*1024);disk.write(path.read_bytes())
        disk.seek(index['offset']);disk.write((out/'spatial/Liechtenstein.nxsearch').read_bytes())
    catalog['pack_sha256']=hashlib.sha256(pack.read_bytes()).hexdigest()
    (out/'map-pack.json').write_text(json.dumps(catalog,indent=2)+'\n')
    return catalog

if __name__ == '__main__':
    pin,src,region=prepare(ROOT/'build/comaps')
    print(pin['region'],pin['revision'],region)
