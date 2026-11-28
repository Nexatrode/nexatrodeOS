"""Phone-only Maps supervision; fresh wrapped-key fixtures, no personal disks."""
import json,struct,xml.etree.ElementTree as ET
from pathlib import Path
NAMES=('offline_map','map_store')
BASE=0x2000000000
STRIDE=0x20000000

def attach(root,system,out,mapping,region,channel,inputs,run):
    # Parse ELF directly; importing desktop catalog is unnecessary here.
    def program(path):
        data=path.read_bytes();off=struct.unpack_from('<Q',data,32)[0];size,count=struct.unpack_from('<HH',data,54)
        assert data[:6]==b'\x7fELF\x02\x01' and size==56
        return struct.unpack_from('<Q',data,24)[0],[struct.unpack_from('<II6Q',data,off+size*i) for i in range(count)]
    domains={p.get('name'):p for p in system.findall('protection_domain')}
    domains['vault'].set('priority','129')
    manager=ET.SubElement(system,'protection_domain',name='maps_lock_manager',priority='128',budget='8000',period='10000',stack_size='0x40000')
    ET.SubElement(manager,'program_image',path='maps-lock.elf')
    cs=ET.SubElement(manager,'cspace');ET.SubElement(cs,'cap_vspace',slot='4',pd='maps_lock_manager')
    region('map_session_key',4096);mapping(domains['offline_map'],'map_session_key',0x79000000,'rw','map_session_key');mapping(domains['vault'],'map_session_key',0x45002000,'rw','map_session_key')
    channel('offline_map',5,'vault',13,True)
    channel('maps_lock_manager',1,'vault',14,True);channel('slint_ui',21,'maps_lock_manager',0,True)
    # Non-dynamic SimpleX startup wakes only after the wrapped root is active.
    channel('vault',11,'simplex_agent',6)
    mapping(manager,'map_disk_regs',0x78003000,'rw',cached=False)
    region('maps_lock_state',4096,phys_addr='0x5fb98000');region('maps_session_state',4096)
    mapping(manager,'maps_lock_state',0x78000000,'rw');mapping(manager,'maps_session_state',0x78002000,'rw')
    mapping(domains['slint_ui'],'maps_lock_state',0x94000000,'r','maps_lock_state')
    boot=[];children=[]
    owned={m.get('mr') for name in NAMES for m in domains[name].findall('map')}-{'map_disk_regs'}
    sizes={m.get('name'):int(m.get('size'),0) for m in system.findall('memory_region')}
    for i,name in enumerate(NAMES):
        d=domains[name];entry,segments=program(out/d.find('program_image').get('path'));base=BASE+i*STRIDE
        for kind,flags,_,va,_,_,mem,_ in segments:
            if kind==1 and flags&2:boot.append((base+(va&~4095),((va+mem+4095)&~4095)-(va&~4095)))
        boot.extend(((base+0x10000000,4096),(base+0x11000000,int(d.get('stack_size','0x1000'),0))))
        children.append((base,0,entry));d.set('id',str(32+i));d.attrib.pop('passive',None)
        ET.SubElement(cs,'cap_sc',slot=str(32+i),pd=name);system.remove(d);manager.append(d)
    ranges=list(boot);aliases={};address=0x3000000000
    for name in sorted(owned):
        address=(address+0x1fffff)&~0x1fffff;size=sizes[name]
        mapping(manager,name,address,'rw',cached=name!='map_disk_dma');ranges.append((address,size));aliases[name]=(address,size);address+=size
    header='''#pragma once
#define SESSION_CHILD_COUNT 2u
#define SESSION_FIRST_CHILD 32u
#define SESSION_SC_CAP_BASE 32u
#define SESSION_STACK_TOP ((seL4_UserVSpaceTop & ~4095UL)-4096)
#define SESSION_STATE_ADDRESS 0x78002000UL
struct session_child {uintptr_t address;size_t pages;uintptr_t entry;};
struct session_range {uintptr_t address;size_t bytes;};
static const struct session_child session_children[]={
'''+''.join(f'{{{b}UL,{n},{e}UL}},\n' for b,n,e in children)+'};\nstatic const struct session_range session_ranges[]={\n'+''.join(f'{{{a}UL,{s}}},\n' for a,s in ranges)+'};\n'+f'#define SESSION_RANGE_COUNT {len(ranges)}u\n#define SESSION_BOOT_RANGE_COUNT {len(boot)}u\n'
    (out/'session_layout.h').write_text(header);(out/'session-layout.json').write_text(json.dumps(dict(children=dict(zip(NAMES,children)),boot_ranges=boot,ranges=ranges,private_shared=aliases,saved_data_used=False),indent=2)+'\n')
    assert not any(m.get('mr')=='map_session_key' for m in domains['slint_ui'].findall('map'))
    assert 'map_disk_regs' not in aliases and 'map_disk_dma' in aliases
    all_domains={p.get('name'):p for p in system.iter('protection_domain')}
    key_owners={name for name,d in all_domains.items() if d.find("map[@mr='map_session_key']") is not None}
    assert key_owners=={'maps_lock_manager','offline_map','vault'}
    assert domains['slint_ui'].find("map[@mr='maps_lock_state']").get('perms')=='r'
    assert tuple(p.get('name') for p in manager.findall('protection_domain'))==NAMES
    endpoints=set()
    for link in system.findall('channel'):
        ends=list(link)
        for i,e in enumerate(ends):
            key=(e.get('pd'),int(e.get('id')));assert key not in endpoints,key;endpoints.add(key)
            if e.get('pp')=='true':assert int(all_domains[e.get('pd')].get('priority'))<int(all_domains[ends[1-i].get('pd')].get('priority'))
    board=inputs/'sdk/board/qemu_virt_aarch64_el1/smp-debug';sysroot=root/'build/netsurf-native/sysroot'
    run('clang','--target=aarch64-none-elf','-mcpu=cortex-a53','-mstrict-align','-ffreestanding','-fno-stack-protector','-O2','-Wall','-Wextra','-I'+str(board/'include'),'-I'+str(sysroot/'include'),'-I'+str(out),'-c',root/'experiments/mobile-browser/maps_lock.c','-o',out/'maps-lock.o')
    run('ld.lld','-L',board/'lib','-L',sysroot/'lib','-T',board/'lib/microkit.ld',out/'maps-lock.o','--start-group','-lmicrokit','-lc','--end-group','-o',out/'maps-lock.elf')
