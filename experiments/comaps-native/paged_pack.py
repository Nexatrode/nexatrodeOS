#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Bundle existing bounded spatial pages/search into a streaming local import."""
import hashlib,importlib.util,json,os,struct
from pathlib import Path
HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('local_map_pack',HERE/'local_pack.py');local=importlib.util.module_from_spec(spec);spec.loader.exec_module(local)
DIRECTORY=8192;LIMIT=32*1024*1024;EXTENT=32*1024*1024;DISK_BYTES=384*1024*1024
SEGMENT=struct.Struct('<QQ32s4d');assert SEGMENT.size==80

def package(spatial,search,profile,name,region,version,output):
    assert region in (0,1,2,3) and 0<version<2**64-1
    directory=bytearray(DIRECTORY);at=DIRECTORY;segments=[]
    for p in profile['pages']:
        assert p['bytes']<=local.LIMIT
        segments.append(dict(p,offset=at));at+=(p['bytes']+511)&~511
    assert len(segments)==65
    q=profile['search'];segments.append(dict(q,offset=at,bounds=[0.,0.,0.,0.]));at+=(q['bytes']+511)&~511
    assert DIRECTORY<at<=LIMIT
    struct.pack_into('<8sIIQ2d4d16sQ',directory,0,b'NXMPGD01',1,65,at,*profile['origin'],*profile['bounds'],local.identity(name),version)
    for i,s in enumerate(segments):SEGMENT.pack_into(directory,256+i*80,s['offset'],s['bytes'],bytes.fromhex(s['sha256']),*s['bounds'])
    header=bytearray(4096);struct.pack_into('<8sIIQQ16s64s32s',header,0,b'NXMRPK02',2,region,version,at,local.identity(name),local.name_bytes(name),hashlib.sha256(directory).digest());header[256:288]=hashlib.sha256(header[:256]).digest()
    # Build-time packaging streams public files as well; no whole-container read.
    with output.open('wb') as out,spatial.open('rb') as pages,search.open('rb') as names:
        out.write(header);out.write(directory)
        for i,s in enumerate(segments):
            src=pages if i<65 else names;src.seek(profile['pages'][i]['offset'] if i<65 else 0);left=s['bytes'];h=hashlib.sha256()
            while left:
                b=src.read(min(left,65536));assert b;h.update(b);out.write(b);left-=len(b)
            assert h.hexdigest()==s['sha256'];out.write(bytes((-s['bytes'])%512))
    assert output.stat().st_size==4096+at
    return dict(bytes=4096+at,body_bytes=at,largest_page=max(s['bytes'] for s in segments),directory_sha256=hashlib.sha256(directory).hexdigest(),segments=len(segments))

def initialize(stage,catalog):
    assert (stage/'.disposable-build').is_file() and not (stage/'maps.img').is_symlink()
    spatial=stage/'spatial';profile=json.loads((spatial/'Liechtenstein-profile.json').read_text());dest=stage/'local-packs'
    descriptions=[]
    for v in (1,2):descriptions.append(package(spatial/'Liechtenstein.nxmap',spatial/'Liechtenstein.nxsearch',profile,'Local Liechtenstein',3,v,dest/f'liechtenstein-paged-v{v}.nxregion'))
    with (stage/'maps.img').open('r+b') as f:f.truncate(DISK_BYTES);f.flush();os.fsync(f.fileno())
    catalog.update(pack_bytes=DISK_BYTES,large_import=True,large_sources=descriptions)
    h=hashlib.sha256()
    with (stage/'maps.img').open('rb') as f:
        while b:=f.read(1024*1024):h.update(b)
    catalog['pack_sha256']=h.hexdigest();(stage/'map-pack.json').write_text(json.dumps(catalog,indent=2)+'\n')
    return catalog
