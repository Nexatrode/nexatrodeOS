# SPDX-License-Identifier: Apache-2.0
"""Build a pinned, spatially indexed rendering subset; no user disks or data.
The original MWM remains the authoritative source. Pages have per-page hashes.
"""
import hashlib,json,struct,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
FEATURE=struct.Struct('<IIffffH8HBB96s')
assert FEATURE.size==140

def prepare(base, source, work=None):
    # Each build stage owns its compiler/export/page outputs. Desktop and phone
    # packaging may run together, but never overwrite an executable or archive
    # another builder is reading.
    work=work or base;work.mkdir(parents=True,exist_ok=True)
    exe=work/'export';raw=work/(source.stem+'.features')
    subprocess.run(['clang','-O2','-I'+str(HERE),str(HERE/'mwm.c'),str(HERE/'export.c'),'-o',str(exe)],check=True)
    info=json.loads(subprocess.check_output([str(exe),str(source),str(raw)],text=True))
    styles=[int(s) for s in (base/'styles.h').read_text().split('{')[1].split('}')[0].split(',')]
    groups=[[] for _ in range(65)];bounds=info['bounds'];total_bytes=raw.stat().st_size
    with raw.open('rb') as stream:
        while head:=stream.read(140):
            assert len(head)==140
            f=FEATURE.unpack(head);count=f[1];points=stream.read(count*8);assert len(points)==count*8
            types=f[7:15][:f[15]];geometry=f[16];name=f[17].split(b'\0')[0]
            style=0
            for t in types:
                value=styles[t] if t<len(styles) else 0
                if not style or value in (1,3,5):style=value
            if not style:continue
            absolute=[f[2]+info['origin'][0],f[3]+info['origin'][1],f[4]+info['origin'][0],f[5]+info['origin'][1]]
            x=min(7,max(0,int(((absolute[0]+absolute[2])/2-bounds[0])/(bounds[2]-bounds[0])*8)))
            y=min(7,max(0,int(((absolute[1]+absolute[3])/2-bounds[1])/(bounds[3]-bounds[1])*8)))
            item=(head,points,absolute);groups[1+y*8+x].append(item)
            # An explicit overview LOD: retain land, main roads and place labels.
            # Detail pages preserve the supported geometry without simplification.
            if (geometry==0 and style==8) or (geometry==1 and style==5) or (geometry==2 and style in (1,2,7,9)):
                groups[0].append(item)
    pack=work/(source.stem+'.nxmap');pages=[]
    with pack.open('wb') as stream:
        for index,items in enumerate(groups):
            count=len(items);points=sum(len(p)//8 for _,p,_ in items)
            assert count<=32768 and points<=524288,(index,count,points)
            payload=struct.pack('<4I',0x3150584e,count,points,0)+b''.join(h+p for h,p,_ in items)
            assert len(payload)<=8*1024*1024
            box=[min((b[i] for _,_,b in items),default=bounds[i]) if i<2 else max((b[i] for _,_,b in items),default=bounds[i]) for i in range(4)]
            at=stream.tell();stream.write(payload);stream.write(bytes((-len(payload))%512))
            pages.append(dict(offset=at,bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest(),features=count,points=points,bounds=box))
    info.update(source_bytes=source.stat().st_size,export_bytes=total_bytes,bytes=pack.stat().st_size,sha256=hashlib.sha256(pack.read_bytes()).hexdigest(),pages=pages)
    (work/(source.stem+'-profile.json')).write_text(json.dumps(info,indent=2)+'\n')
    return pack,info
