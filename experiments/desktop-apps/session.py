"""Attach a curated restart group to the existing three-app desktop build.

No regular VM/disk inputs. Read-only device/clock views are never erase ranges.
The generated layout matches the private composer's explicit service catalog.
"""
import json
from pathlib import Path
import struct
import xml.etree.ElementTree as ET
from runtime_manifest import APPS, PUBLIC_APP_REGIONS

NAMES=tuple(name for app in APPS.values() for name in (app['frontend'],*app['services']))
assert len(NAMES)==14
MAP_NAMES=('offline_map','map_store')
def names(maps=False):return NAMES+(MAP_NAMES if maps else ())
BASE=0x2000000000
STRIDE=0x20000000


def program(path):
    data=path.read_bytes();assert data[:6]==b'\x7fELF\x02\x01'
    off=struct.unpack_from('<Q',data,32)[0];size,count=struct.unpack_from('<HH',data,54)
    assert size==56 and count<32
    return struct.unpack_from('<Q',data,24)[0],[struct.unpack_from('<II6Q',data,off+size*i) for i in range(count)]


def attach(system,out,mapping,channel,mr,*,maps=False):
    members=names(maps)
    domains={p.get('name'):p for p in system.iter('protection_domain')}
    manager=domains['desktop_manager'];manager.set('stack_size','0x40000')
    # Microkit PPC targets must outrank their callers. Keep the input manager's
    # existing priority; its two lifecycle providers sit below the timer (130).
    domains['vault'].set('priority','129')
    domains['dhcp_client'].set('priority','128')
    # Cold AgentStore creation can consume the agent's entire initialization
    # budget. It uses notifications/shared IPC with the broker, so it can run
    # below the frontends while its vault/network/clock PPC targets stay higher.
    domains['simplex_agent'].set('priority','95')
    cspace=ET.SubElement(manager,'cspace')
    ET.SubElement(cspace,'cap_vspace',slot='4',pd='desktop_manager')
    excluded=PUBLIC_APP_REGIONS|({'map_disk_regs'} if maps else set())
    if maps:
        mr('map_session_key',4096)
        mapping(domains['offline_map'],'map_session_key',0x79000000,'rw','map_session_key')
        mapping(domains['vault'],'map_session_key',0x45002000,'rw','map_session_key')
        channel('offline_map',5,'vault',13,True,False)
        mapping(manager,'map_disk_regs',0x78003000,'rw',None)
        manager.findall('map')[-1].set('cached','false')
    sizes={r.get('name'):int(r.get('size').replace('_',''),0) for r in system.findall('memory_region')}
    owned={m.get('mr') for name in members for m in domains[name].findall('map')} - excluded
    boot=[];children=[]
    for i,name in enumerate(members):
        pd=domains[name]
        image=pd.find('program_image').get('path');entry,segments=program(out/image)
        base=BASE+i*STRIDE
        for kind,flags,_,va,_,_,mem,_ in segments:
            if kind==1 and flags&2:
                start=va&~4095;end=(va+mem+4095)&~4095
                assert end<STRIDE
                boot.append((base+start,end-start))
        stack=int(pd.get('stack_size','0x1000').replace('_',''),0)
        boot.extend(((base+0x10000000,4096),(base+0x11000000,stack)))
        children.append((base,0,entry))
        pd.set('id',str(32+i));pd.attrib.pop('passive',None)
        if 'budget' not in pd.attrib:pd.set('budget','2500');pd.set('period','10000')
        ET.SubElement(cspace,'cap_sc',slot=str(32+i),pd=name)
        system.remove(pd);manager.append(pd)
    ranges=list(boot);alias=0x3000000000;aliases={}
    for name in sorted(owned):
        size=sizes[name]
        # Hardware/read-only clocks are excluded by the catalog. Everything
        # else has a writable group owner and a bounded page-aligned region.
        assert size%4096==0
        existing=manager.find(f"map[@mr='{name}']")
        if existing is not None:
            existing.set('perms','rw')
            address=int(existing.get('vaddr'),0)
            ranges.append((address,size));aliases[name]=(address,size)
            continue
        alias=(alias+0x1fffff)&~0x1fffff
        mapping(manager,name,alias,'rw',None);ranges.append((alias,size));aliases[name]=(alias,size);alias+=size
        if name=='map_disk_dma':manager.findall('map')[-1].set('cached','false')
    for name,size,phys in [('desktop_lock_state',4096,0x5fb93000),('lock_display_ack',4096,0x5fb94000),
                           ('trio_session_state',4096,0x5fb95000),('trio_auth_frame',0x1000000,None),
                           ('trio_auth_heap',0x2000000,None),('trio_auth_input',4096,None),('device_lock_transfer',4096,None)]:
        mr(name,size,**({'phys_addr':hex(phys)} if phys else {}),**({'page_size':'0x200000'} if size>=0x200000 else {}))
    mapping(manager,'desktop_lock_state',0x78000000,'rw','desktop_lock_state')
    mapping(manager,'lock_display_ack',0x78001000,'r','lock_display_ack')
    mapping(manager,'trio_session_state',0x78002000,'rw',None)
    mapping(manager,'trio_auth_input',0x7a000000,'rw','auth_input')
    mapping(domains['desktop_gpu'],'desktop_lock_state',0x7b003000,'r','desktop_lock_state')
    mapping(domains['desktop_gpu'],'lock_display_ack',0x7b004000,'rw','lock_display_ack')
    mapping(domains['desktop_gpu'],'trio_auth_frame',0x7c000000,'r','auth_surface')
    auth=ET.SubElement(system,'protection_domain',name='lock_auth',priority='108',budget='2500',period='10000',stack_size='0x40000')
    ET.SubElement(auth,'program_image',path='auth.elf')
    for name,addr,perms in [('trio_auth_frame',0x60000000,'rw'),('trio_auth_heap',0x64000000,'rw'),
                           ('trio_auth_input',0x66000000,'rw'),('desktop_lock_state',0x67000000,'r'),('device_lock_transfer',0x68000000,'rw')]:
        mapping(auth,name,addr,perms,None)
    mapping(domains['vault'],'device_lock_transfer',0x45001000,'rw','device_lock_transfer')
    channel('lock_auth',0,'desktop_manager',60,True,False)
    channel('lock_auth',1,'desktop_manager',9)
    channel('desktop_manager',8,'lock_auth',2)
    channel('lock_auth',3,'entropy_rng',4,True,False)
    channel('lock_auth',4,'vault',10,True,False)
    channel('desktop_manager',13,'vault',12,True,False)
    channel('desktop_manager',17,'dhcp_client',7,True,False)
    channel('desktop_manager',12,'timer_driver',13,True)
    probe=ET.SubElement(system,'protection_domain',name='lock_probe',priority='105',budget='1000',period='10000')
    ET.SubElement(probe,'program_image',path='lock_probe.elf')
    mapping(probe,'desktop_lock_state',0x50000000,'r','desktop_lock_state')
    channel('lock_probe',0,'desktop_manager',59,True,False)
    channel('desktop_manager',10,'lock_probe',1)
    if maps:channel('lock_probe',2,'vault',14,True,False)
    # Extra shared pages are also erased but are never captured as boot data.
    header='''#pragma once
#define SESSION_CHILD_COUNT MEMBERSu
#define SESSION_FIRST_CHILD 32u
#define SESSION_SC_CAP_BASE 32u
#define SESSION_NOTIFICATION_CAP_BASE 8UL
#define SESSION_STACK_TOP ((seL4_UserVSpaceTop & ~4095UL)-4096)
#define SESSION_STATE_ADDRESS 0x78002000UL
struct session_child { uintptr_t address;size_t pages;uintptr_t entry; };
struct session_range { uintptr_t address;size_t bytes; };
static const struct session_child session_children[]={
'''.replace('MEMBERS',str(len(members)))+''.join(f'{{{base}UL,{pages},{entry}UL}},\n' for base,pages,entry in children)+'};\n'
    if maps:header+='#define SESSION_MAPS_REGS_ADDRESS 0x78003000UL\n'
    header+='static const struct session_range session_ranges[]={\n'+''.join(f'{{{address}UL,{size}}},\n' for address,size in ranges)+'};\n'
    header+=f'#define SESSION_RANGE_COUNT {len(ranges)}u\n#define SESSION_BOOT_RANGE_COUNT {len(boot)}u\n'
    (out/'session_layout.h').write_text(header)
    (out/'session-layout.json').write_text(json.dumps(dict(children=dict(zip(members,children)),private_shared=aliases,
        boot_ranges=boot,ranges=ranges,saved_data_used=False,readonly_excluded=sorted(PUBLIC_APP_REGIONS)),indent=2)+'\n')
    validate(system,aliases,ranges,boot,maps=maps)


def validate(system,aliases,ranges,boot,*,maps=False):
    """Check actual assembled authority/erasure coverage before composition."""
    domains={p.get('name'):p for p in system.iter('protection_domain')}
    manager=domains['desktop_manager']
    members=names(maps)
    assert tuple(p.get('name') for p in manager.findall('protection_domain'))==members
    sizes={r.get('name'):int(r.get('size'),0) for r in system.findall('memory_region')}
    owned={m.get('mr') for n in members for m in domains[n].findall('map')}-PUBLIC_APP_REGIONS-({'map_disk_regs'} if maps else set())
    assert owned==aliases.keys()
    assert not owned&{'device_lock_transfer','trio_auth_heap','trio_auth_frame','desktop_lock_state','lock_display_ack'}
    for name,(address,size) in aliases.items():
        maps=manager.findall(f"map[@mr='{name}']")
        assert len(maps)==1 and maps[0].get('perms')=='rw'
        assert int(maps[0].get('vaddr'),0)==address and size==sizes[name]
        assert (address,size) in ranges and (address,size) not in boot
    assert manager.find("map[@mr='device_lock_transfer']") is None
    for i,name in enumerate(members):
        child=domains[name]
        assert int(child.get('id'))==32+i and child.get('passive')!='true'
        assert manager.find(f"cspace/cap_sc[@pd='{name}'][@slot='{32+i}']") is not None
    for c in system.findall('channel'):
        ends=list(c)
        for i,e in enumerate(ends):
            if e.get('pp')=='true':
                assert int(domains[e.get('pd')].get('priority'))<int(domains[ends[1-i].get('pd')].get('priority'))
