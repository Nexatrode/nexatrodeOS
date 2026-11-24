"""Compose the real SimpleX agent/receive/photo/transcript lock fixture.

All artifacts are opt-in and use a fresh wrapped-key vault. Live mode adds a
network gate; no installation key, regular VM bundle or existing disk is read.
"""
import copy
import hashlib
import importlib.util
from pathlib import Path
import shutil
import struct
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('session_layout', ROOT / 'experiments/desktop-simplex/composer_patch.py')
layout = importlib.util.module_from_spec(spec); spec.loader.exec_module(layout)
STACKS = (0x100000, 0x40000, 0x20000, 0x20000, 0x20000, 0x20000)
IMAGES = ('agentstore_probe.elf', 'rust_messages_agent_app.elf', 'slint_transcript_worker.elf',
          'slint_photo_worker.elf', 'slint_photo_reader.elf', 'simplex_session_client.elf')


def programs(path):
    data = path.read_bytes()
    assert data[:6] == b'\x7fELF\x02\x01'
    offset = struct.unpack_from('<Q', data, 32)[0]
    size, count = struct.unpack_from('<HH', data, 54)
    assert size == 56 and count < 32
    return struct.unpack_from('<Q', data, 24)[0], [struct.unpack_from('<IIQQQQQQ', data, offset + size*i) for i in range(count)]


def prepare(out, agent, cargo, flags, run, live=False):
    target = cargo('experiments/slint-notes/Cargo.toml', 'desktop-simplex-live-test' if live else 'utm-pd,utm-photo-worker,utm-transcript-worker',
        [name.removesuffix('.elf') for name in IMAGES[2:-1]] + ['simplex_live_client' if live else 'simplex_session_client'], 'build/desktop-simplex-ui-cargo')
    for name in IMAGES[2:]: shutil.copyfile(target/('simplex_live_client.elf' if live and name==IMAGES[-1] else name), out/name)
    shutil.copyfile(agent, out/IMAGES[0])
    # The old startup gate remains defense in depth. It rechecks vault status
    # after the parent has committed the new key before entering GHC.
    children=[]
    for i, name in enumerate(IMAGES):
        entry, ph = programs(out/name)
        pages=sum(((vaddr+mem+4095)//4096-vaddr//4096) for kind,flg,_,vaddr,_,_,mem,_ in ph if kind==1 and flg&2)
        pages += 1 + STACKS[i]//4096  # IPC buffer then stack, as in pinned composer.
        assert pages*4096 < layout.STRIDE
        children.append((layout.BASE+i*layout.STRIDE,pages,entry))
    run(*flags, *([f'-DSIMPLEX_LIVE_UNIX={__import__("time").time_ns()//1000000000}ULL'] if live else []), '-c', ROOT/'experiments/desktop-simplex/environment.c', '-o',out/'environment.o')
    board_include = Path(flags[flags.index('-I')+1]); board=board_include.parent
    run('ld.lld','-L',board/'lib',out/'environment.o','-lmicrokit','-T','microkit.ld','-o',out/'environment.elf')
    return children


def compose(system, manager, out, children, mapping, channel, mr, live=False):
    source=ET.parse(ROOT/'experiments/hvf-smp/accelerated.system').getroot()
    oldnames=('simplex_agent','messages_app','transcript_worker','photo_worker','photo_reader')
    remap=dict(zip(oldnames,layout.NAMES))
    manager.set('priority','109')
    manager.set('budget','8000')
    group=[]
    for i,(old,new) in enumerate(remap.items()):
        pd=copy.deepcopy(source.find(f"protection_domain[@name='{old}']"))
        pd.set('name',new); pd.set('id',str(layout.FIRST_CHILD+i))
        pd.set('cpu','0'); pd.set('priority',str((107,106,104,104,104)[i]))
        # Preserve the real agent's original initialization budget. The lock
        # manager still has higher priority and can always stop this group.
        pd.set('budget','10000' if i==0 else '2500');pd.set('period','10000');pd.attrib.pop('passive',None)
        manager.append(pd);group.append(pd)
    client=ET.SubElement(manager,'protection_domain',name='session_client',id='37',priority='98',budget='2500',period='10000',stack_size='0x20000')
    ET.SubElement(client,'program_image',path='simplex_session_client.elf');group.append(client)
    client_maps=[('photo_mailbox',0x50000000,'photo_mailbox_region'),('photo_read_mailbox',0x50200000,'photo_read_mailbox'),
        ('transcript_mailbox',0x50400000,'transcript_mailbox'),('live_receive_mailbox',0x50600000,'live_receive_mailbox'),
        ('messages_ui_transfer',0x50800000,'messages_ui_transfer'),('session_client_heap',0x50a00000,'session_client_heap')]
    for name,addr,symbol in client_maps:
        if not live or name not in ('photo_mailbox','photo_read_mailbox'): mapping(client,name,addr,'rw',symbol)
    mr('simplex_test_result',4096)
    system.find("memory_region[@name='simplex_test_result']").set('phys_addr','0x5fbb1000' if live else '0x5fd91000')
    mapping(client,'simplex_test_result',0x51200000,'rw','session_result')
    # Rename the old frontend-only page, keeping Messages' vault transfer separate.
    messages=group[1]
    for m in messages.findall('map'):
        if m.get('mr')=='messages_transfer': m.set('mr','messages_ui_transfer')
    vault=system.find("protection_domain[@name='vault']"); vault.set('priority','110')
    system.find("protection_domain[@name='entropy_rng']").set('priority','120')
    vault_messages=vault.find("map[@mr='messages_transfer']")
    vault_messages.set('mr','messages_vault_transfer');vault_messages.set('cached','true')
    mapping(vault,'simplex_agent_transfer',0x50000000,'rw','simplex_agent_transfer')
    mapping(vault,'photo_read_transfer',0x50001000,'rw','photo_read_transfer')
    channel('session_messages',0,'vault',6,True)
    channel('session_agent',0,'vault',7,True)
    channel('session_reader',1,'vault',9,True)
    channel('session_transcript',1,'session_messages',7,True)
    channel('session_messages',4,'session_agent',5)
    channel('session_agent',3,'entropy_rng',2,True)
    channel('session_messages',3,'entropy_rng',1,True)
    channel('session_agent',4,'timer_driver',3,True)
    channel('session_client',0,'timer_driver',8,True)
    channel('session_client',1,'session_messages',5,True)
    channel('session_client',2,'session_photo',0)
    channel('session_client',3,'session_reader',0)
    channel('session_client',4,'session_transcript',0)
    channel('session_client',5,'session_transcript',2)
    environment=ET.SubElement(system,'protection_domain',name='session_environment',priority='120',passive='true')
    ET.SubElement(environment,'program_image',path='environment.elf')
    channel('session_agent',2,'session_environment',0,True)
    channel('session_messages',2,'session_environment',2,True)
    if live:
        import desktop_simplex_network
        desktop_simplex_network.compose(ROOT, system, manager, channel)
    else: channel('session_agent',1,'session_environment',1,True)
    # Offline mode has no Ethernet. Neither mode includes a phone or wallet PD.
    names={m.get('mr') for pd in group for m in pd.findall('map')}
    sizes={m.get('name'):int(m.get('size').replace('_',''),0) for m in source.findall('memory_region')}
    sizes.update(messages_ui_transfer=4096,session_client_heap=0x200000,simplex_test_result=4096)
    existing={m.get('name') for m in system.findall('memory_region')}
    ranges=[]
    for i,name in enumerate(IMAGES):
        base=layout.BASE+i*layout.STRIDE
        _,ph=programs(out/name)
        for kind,flg,_,vaddr,_,_,mem,_ in ph:
            if kind==1 and flg&2:
                ranges.append((base+(vaddr&~4095), ((vaddr+mem+4095)&~4095)-(vaddr&~4095)))
        ranges += [(base+0x10000000,4096),(base+0x11000000,STACKS[i])]
    alias=0x3000000000
    aliases={}
    for name in sorted(names):
        if name not in existing:
            original=source.find(f"memory_region[@name='{name}']")
            if original is not None: system.append(copy.deepcopy(original))
            else: mr(name,sizes[name],sizes[name]>=0x200000)
        size=sizes[name]
        alias=(alias+0x1fffff)&~0x1fffff
        mapping(manager,name,alias,'rw')
        ranges.append((alias,size));aliases[name]=(alias,size)
        alias+=size
    mr('simplex_session_state',4096)
    system.find("memory_region[@name='simplex_session_state']").set('phys_addr','0x5fbb0000' if live else '0x5fd90000')
    mapping(manager,'simplex_session_state',0x71000000,'rw')
    mapping(client,'simplex_session_state',0x51000000,'r','session_state')
    header='''#pragma once
#define SESSION_CHILD_COUNT 6u
#define SESSION_FIRST_CHILD 32u
#define SESSION_STACK_TOP ((seL4_UserVSpaceTop & ~4095UL) - 4096)
#define SESSION_STATE_ADDRESS 0x71000000UL
struct session_child { uintptr_t address; size_t pages; uintptr_t entry; };
struct session_range { uintptr_t address; size_t bytes; };
static const struct session_child session_children[] = {
'''+''.join(f'{{{addr}UL,{pages},{entry}UL}},\n' for addr,pages,entry in children)+'};\n'
    header+='static const struct session_range session_ranges[] = {\n'+''.join(f'{{{addr}UL,{size}}},\n' for addr,size in ranges)+'};\n'
    header+=f'#define SESSION_RANGE_COUNT {len(ranges)}u\n'
    (out/'session_layout.h').write_text(header)
    # Test evidence can map each scrubbed range to its component/queue.
    import json
    (out/'session-layout.json').write_text(json.dumps({'children':dict(zip(layout.NAMES,children)),
        'shared':aliases,'network':'live public SMP relay' if live else 'offline stub','saved_data_used':False},indent=2)+'\n')
