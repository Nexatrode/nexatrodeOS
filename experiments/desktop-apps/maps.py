"""Offline CoMaps-data window: private frame/input, no network or app data."""
import importlib.util
from pathlib import Path
import shutil
import subprocess
import xml.etree.ElementTree as ET


def build(root, out, inputs, run):
    here=root/'experiments/comaps-native';base=root/'build/comaps'
    spec=importlib.util.spec_from_file_location('comaps_prepare',here/'prepare.py')
    preparation=importlib.util.module_from_spec(spec);spec.loader.exec_module(preparation)
    pin,src,region=preparation.prepare(base)
    board=inputs/'sdk/board/qemu_virt_aarch64_el1/smp-debug'
    sysroot=root/'build/netsurf-native/sysroot'
    stb=root/'build/netsurf-native/deps/stb'
    flags=['--target=aarch64-none-elf','-mcpu=cortex-a53','-mstrict-align','-ffreestanding','-fno-stack-protector','-ffunction-sections','-fdata-sections','-O2','-Wall','-Wextra',
        '-I'+str(board/'include'),'-I'+str(sysroot/'include'),'-I'+str(here),'-I'+str(base),'-I'+str(stb),
        '-DMAP_DESKTOP=1','-DMAP_PROFILE=1','-DSLINT_FRAME_WIDTH=1120u','-DSLINT_FRAME_HEIGHT=1040u','-DSLINT_FRAME_REGION_BYTES=0x400000u']
    for unit in ('viewer','mwm'):run('clang',*flags,'-c',here/(unit+'.c'),'-o',out/('maps-'+unit+'.o'))
    font=root/'experiments/slint-notes/assets/fonts/Roboto-Regular.ttf'
    (out/'maps-assets.S').write_text('.section .rodata\n.balign 8\n.global map_blob,map_blob_end,map_font\nmap_blob:\n.incbin "'+str(region)+'"\nmap_blob_end:\n.balign 8\nmap_font:\n.incbin "'+str(font)+'"\n')
    run('clang','--target=aarch64-none-elf','-c',out/'maps-assets.S','-o',out/'maps-assets.o')
    run('ld.lld','--gc-sections','-L',board/'lib','-L',sysroot/'lib','-T',board/'lib/microkit.ld',out/'maps-viewer.o',out/'maps-mwm.o',out/'maps-assets.o','--start-group','-lmicrokit','-lc','-lm','--end-group','-o',out/'offline-map.elf')
    notices=out/'licenses/maps';notices.mkdir(parents=True,exist_ok=True)
    for name in ('LICENSE','NOTICE'):shutil.copyfile(src/name,notices/('CoMaps-'+name))
    shutil.copyfile(here/'NOTICE',notices/'NexatrodeOS-NOTICE')
    shutil.copyfile(root/'experiments/slint-notes/assets/fonts/Roboto-OFL.txt',notices/'Roboto-OFL.txt')
    text=(stb/'stb_truetype.h').read_text();(notices/'stb-truetype-license.txt').write_text(text[text.rfind('ALTERNATIVE A'):])
    return pin


def attach(system, manager, mapping, mr, channel):
    mr('map_frame',0x400000,page_size='0x200000')
    mr('map_heap',0x1000000,page_size='0x200000')
    mr('map_input',4096)
    mr('map_observation',4096,phys_addr='0x5fb96000')
    # A full-region draw slightly exceeds 4 ms on this guest. A 4/10 ms
    # reservation therefore inserts a 6 ms replenishment wait mid-frame.
    # Keep the reservation bounded and below every interactive frontend's
    # priority, but let a typical frame finish in one activation.
    app=ET.SubElement(system,'protection_domain',name='offline_map',priority='95',budget='8000',period='10000',stack_size='0x100000')
    ET.SubElement(app,'program_image',path='offline-map.elf')
    for name,va,perms,symbol in [('map_frame',0x60000000,'rw','map_frame'),('map_heap',0x64000000,'rw','map_heap'),('map_input',0x70000000,'rw','input_handoff'),('map_observation',0x71000000,'rw','map_observation')]:
        mapping(app,name,va,perms,symbol)
    mapping(manager,'map_frame',0x65000000,'r','surface_d')
    mapping(manager,'map_input',0x66006000,'rw','input_d')
    channel('offline_map',0,'desktop_manager',29)
    channel('offline_map',1,'desktop_manager',28)


def validate(system):
    domains={p.get('name'):p for p in system.findall('protection_domain')}
    physical=sorted((int(m.get('phys_addr').replace('_',''),0),int(m.get('size').replace('_',''),0),m.get('name')) for m in system.findall('memory_region') if m.get('phys_addr'))
    for (start,size,name),(next_start,_,next_name) in zip(physical,physical[1:]):
        assert start+size<=next_start,('overlapping physical regions',name,next_name)
    app=domains['offline_map']
    assert {m.get('mr'):m.get('perms') for m in app.findall('map')}=={
        'map_frame':'rw','map_heap':'rw','map_input':'rw','map_observation':'rw','desktop_display_mode':'r'}
    links={}
    for c in system.findall('channel'):
        a,b=list(c)
        for x,y in ((a,b),(b,a)):links[x.get('pd'),int(x.get('id'))]=(y.get('pd'),int(y.get('id')))
    assert {i:peer for (pd,i),peer in links.items() if pd=='offline_map'}=={0:('desktop_manager',29),1:('desktop_manager',28)}
    for name in ('chat_ui','wallet_ui','native_browser','wallet_app','simplex_agent','vault'):
        assert not {'map_frame','map_heap','map_input','map_observation'} & {m.get('mr') for m in domains[name].findall('map')}
    for name in ('desktop_manager','desktop_gpu'):
        assert domains[name].find("map[@mr='map_frame']").get('perms')=='r'
    assert domains['desktop_gpu'].find("map[@mr='map_input']") is None
    assert 'lock_auth' not in domains,'Maps is not part of the curated three-app wrapped lock group'
