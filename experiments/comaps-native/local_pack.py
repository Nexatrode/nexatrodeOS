#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Create bounded local region packs; stage only marked disposable fixture disks.

V1 accepts raw MWM understood by the bounded native decoder (8 MiB maximum).
IDs 0/1 update the builtin identities; ID 3 is one additional local region.
Checksums provide integrity, not a publisher signature or general MWM support.
"""
import argparse,hashlib,json,os,struct
from pathlib import Path
LIMIT=8*1024*1024;INBOX=160*1024*1024;CATALOG=2*1024*1024
ENTRY=struct.Struct('<16s64s32sQQII');assert ENTRY.size==136

def identity(name):return hashlib.sha256(('NexatrodeOS/local-map/'+name).encode()).digest()[:16]
def name_bytes(name):
    b=name.encode('ascii');assert b and len(b)<64 and all(32<=c<=126 for c in b);return b.ljust(64,b'\0')
def pack(source,name,region,version):
    body=Path(source).read_bytes();assert 16<=len(body)<=LIMIT and region in (0,1,3) and 0<version<2**64-1
    h=bytearray(4096);struct.pack_into('<8sIIQQ16s64s32s',h,0,b'NXMRPK01',1,region,version,len(body),identity(name),name_bytes(name),hashlib.sha256(body).digest());h[256:288]=hashlib.sha256(h[:256]).digest();return bytes(h)+body

def initialize(stage,catalog):
    disk=stage/'maps.img';assert (stage/'.disposable-build').is_file() and not disk.is_symlink()
    regions=catalog['regions'];entries=[]
    for r in regions:
        entries.append(ENTRY.pack(identity(r['name']),name_bytes(r['name']),bytes.fromhex(r['sha256']),r['bytes'],1,2,1))
    entries.append(bytes(136));record=bytearray(4096);struct.pack_into('<8sQ',record,0,b'NXMRCT01',1);record[16:560]=b''.join(entries);record[1024:1056]=hashlib.sha256(record[:1024]).digest()
    with disk.open('r+b') as f:f.truncate(192*1024*1024);f.seek(CATALOG);f.write(record);f.write(record);f.flush();os.fsync(f.fileno())
    dest=stage/'local-packs';dest.mkdir(exist_ok=True)
    base=Path(__file__).resolve().parents[2]/'build/comaps'
    # An actual additional slot, using public Gibraltar geometry and a distinct
    # stable local identity. Future packs can contain another supported region.
    (dest/'local-gibraltar-v1.nxregion').write_bytes(pack(base/'Gibraltar.mwm','Local Gibraltar',3,1))
    # A valid content change: harmless trailing container padding, included in
    # the new checksum/length and validated again by the guest decoder.
    updated=dest/'gibraltar-v2.mwm';updated.write_bytes((base/'Gibraltar.mwm').read_bytes()+bytes(512))
    (dest/'gibraltar-v2.nxregion').write_bytes(pack(updated,'Gibraltar',0,2))
    with disk.open('r+b') as f:f.seek(INBOX);f.write((dest/'local-gibraltar-v1.nxregion').read_bytes());f.flush();os.fsync(f.fileno())
    catalog['pack_bytes']=192*1024*1024;catalog['local_import']=True;catalog['pack_sha256']=hashlib.sha256(disk.read_bytes()).hexdigest()
    (stage/'map-pack.json').write_text(json.dumps(catalog,indent=2)+'\n')
    return catalog

def stage_inbox(guest,package):
    root=Path(__file__).resolve().parents[2];guest=guest.resolve();assert guest.is_relative_to(root/'build') and (guest/'.disposable-build').is_file()
    m=json.loads((guest/'build.json').read_text());s=json.loads((guest/'session.json').read_text());assert m['map_import'] and m['saved_data_used'] is s['saved_data_used'] is False
    data=Path(s['bundle']).resolve()/'Data';assert data.parent.parent==guest and data.parent.suffix=='.utm'
    import plistlib,subprocess
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==s['uuid'] and [x['ImageName'] for x in cfg['Drive']]==['blank.img','maps.img']
    status=subprocess.check_output(['/Applications/UTM.app/Contents/MacOS/utmctl','status',s['uuid']],text=True,timeout=30).strip();assert status=='stopped','stop this disposable VM before staging the inbox'
    disk=data/'maps.img';assert not disk.is_symlink() and disk.stat().st_size==192*1024*1024
    raw=package.read_bytes();assert 4096<=len(raw)<=4096+LIMIT
    with disk.open('r+b') as f:f.seek(INBOX);f.write(raw);f.flush();os.fsync(f.fileno())

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);sub=p.add_subparsers(dest='command',required=True)
    c=sub.add_parser('create');c.add_argument('--source',type=Path,required=True);c.add_argument('--name',required=True);c.add_argument('--region',type=int,required=True);c.add_argument('--version',type=int,required=True);c.add_argument('--output',type=Path,required=True)
    c=sub.add_parser('stage');c.add_argument('--guest',type=Path,required=True);c.add_argument('--package',type=Path,required=True)
    a=p.parse_args()
    if a.command=='create':a.output.write_bytes(pack(a.source,a.name,a.region,a.version))
    else:stage_inbox(a.guest,a.package)
