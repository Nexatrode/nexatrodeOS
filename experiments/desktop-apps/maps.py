"""Offline CoMaps-data window: private frame/input, no network or app data."""
import importlib.util
from pathlib import Path
import shutil
import subprocess
import xml.etree.ElementTree as ET


def build(root, out, inputs, run, *, mobile=False, storage=False, private=False, session=False, importing=False, files=False, files_fixture=False, large=False):
    here=root/'experiments/comaps-native';base=root/'build/comaps'
    spec=importlib.util.spec_from_file_location('comaps_prepare',here/'prepare.py')
    preparation=importlib.util.module_from_spec(spec);spec.loader.exec_module(preparation)
    pin,src,region=preparation.prepare(base)
    if storage:pin=dict(pin,storage_regions=preparation.prepare_storage(base,out))
    if importing:
        assert storage and private
        spec=importlib.util.spec_from_file_location('maps_local_pack',here/'local_pack.py');local=importlib.util.module_from_spec(spec);spec.loader.exec_module(local)
        pin['storage_regions']=local.initialize(out,pin['storage_regions'])
    if large:
        assert files and importing
        spec=importlib.util.spec_from_file_location('maps_paged_pack',here/'paged_pack.py');paged=importlib.util.module_from_spec(spec);spec.loader.exec_module(paged)
        pin['storage_regions']=paged.initialize(out,pin['storage_regions'])
    board=inputs/'sdk/board/qemu_virt_aarch64_el1/smp-debug'
    sysroot=root/'build/netsurf-native/sysroot'
    stb=root/'build/netsurf-native/deps/stb'
    flags=['--target=aarch64-none-elf','-mcpu=cortex-a53','-mstrict-align','-ffreestanding','-fno-stack-protector','-ffunction-sections','-fdata-sections','-O2','-Wall','-Wextra',
        '-I'+str(board/'include'),'-I'+str(sysroot/'include'),'-I'+str(here),'-I'+str(base),'-I'+str(stb),'-I'+str(out),
        '-DMAP_MOBILE=1' if mobile else '-DMAP_DESKTOP=1','-DMAP_PROFILE=1',
        '-DSLINT_FRAME_WIDTH=1080u' if mobile else '-DSLINT_FRAME_WIDTH=1120u',
        '-DSLINT_FRAME_HEIGHT=2160u' if mobile else '-DSLINT_FRAME_HEIGHT=1040u',
        '-DSLINT_FRAME_REGION_BYTES=0x800000u' if mobile else '-DSLINT_FRAME_REGION_BYTES=0x400000u']
    if storage:flags+=['-DMAP_STORAGE=1']
    if importing:flags+=['-DMAP_IMPORT=1']
    if files:flags+=['-DMAP_FILES=1']
    if large:flags+=['-DMAP_LARGE_IMPORT=1']
    if files_fixture:flags+=['-DMAP_FILES_VERIFY=1']
    crypto=[]
    if private:
        assert storage
        flags+=['-DMAP_PRIVATE=1']
        if session:flags+=['-DMAP_SESSION_PRIVATE=1']
        run('cargo','+nightly-2026-09-25','build','--locked','--offline','--release','-Zjson-target-spec','--config',root/'rust/.cargo/config.toml','--manifest-path',here/'private-crypto/Cargo.toml','--target',root/'rust/targets/aarch64-sel4-microkit.json','--target-dir',root/'build/maps-private-crypto')
        crypto=[root/'build/maps-private-crypto/aarch64-sel4-microkit/release/libmaps_private_crypto.a']
    for unit in ('viewer','mwm'):run('clang',*flags,'-c',here/(unit+'.c'),'-o',out/('maps-'+unit+'.o'))
    font=root/'experiments/slint-notes/assets/fonts/Roboto-Regular.ttf'
    blob='' if storage else 'map_blob:\n.incbin "'+str(region)+'"\nmap_blob_end:\n'
    (out/'maps-assets.S').write_text('.section .rodata\n.balign 8\n.global map_blob,map_blob_end,map_font\n'+blob+'.balign 8\nmap_font:\n.incbin "'+str(font)+'"\n')
    run('clang','--target=aarch64-none-elf','-c',out/'maps-assets.S','-o',out/'maps-assets.o')
    run('ld.lld','--gc-sections','-L',board/'lib','-L',sysroot/'lib','-T',board/'lib/microkit.ld',out/'maps-viewer.o',out/'maps-mwm.o',out/'maps-assets.o',*crypto,'--start-group','-lmicrokit','-lc','-lm','--end-group','-o',out/'offline-map.elf')
    if storage:
        for unit in ('store','hash'):run('clang',*flags,'-c',here/(unit+'.c'),'-o',out/('maps-'+unit+'.o'))
        run('ld.lld','--gc-sections','-L',board/'lib','-L',sysroot/'lib','-T',board/'lib/microkit.ld',out/'maps-store.o',out/'maps-hash.o','--start-group','-lmicrokit','-lc','--end-group','-o',out/'map-store.elf')
    if files:
        assert importing
        spec=importlib.util.spec_from_file_location('files_picker_build',root/'experiments/files-picker/build.py');picker=importlib.util.module_from_spec(spec);spec.loader.exec_module(picker)
        picker.build(root,out,flags,board,sysroot,run,fixture=files_fixture)
    notices=out/'licenses/maps';notices.mkdir(parents=True,exist_ok=True)
    for name in ('LICENSE','NOTICE'):shutil.copyfile(src/name,notices/('CoMaps-'+name))
    shutil.copyfile(here/'NOTICE',notices/'NexatrodeOS-NOTICE')
    shutil.copyfile(root/'experiments/slint-notes/assets/fonts/Roboto-OFL.txt',notices/'Roboto-OFL.txt')
    text=(stb/'stb_truetype.h').read_text();(notices/'stb-truetype-license.txt').write_text(text[text.rfind('ALTERNATIVE A'):])
    return pin


def attach(system, manager, mapping, mr, channel, *, storage=False, private=False):
    mr('map_frame',0x400000,page_size='0x200000')
    mr('map_heap',0x1000000,page_size='0x200000')
    mr('map_input',4096)
    mr('map_observation',8192,phys_addr='0x5fb96000')
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
    if storage:attach_storage(system,app,mapping,mr,channel)
    if private:attach_private(system,app,mapping,mr,channel)


def validate(system):
    domains={p.get('name'):p for p in system.iter('protection_domain')}
    physical=sorted((int(m.get('phys_addr').replace('_',''),0),int(m.get('size').replace('_',''),0),m.get('name')) for m in system.findall('memory_region') if m.get('phys_addr'))
    for (start,size,name),(next_start,_,next_name) in zip(physical,physical[1:]):
        assert start+size<=next_start,('overlapping physical regions',name,next_name)
    app=domains['offline_map']
    stored='map_store' in domains
    files='files_picker' in domains
    private=system.find("memory_region[@name='map_private_cipher']") is not None
    locked='lock_auth' in domains
    assert {m.get('mr'):m.get('perms') for m in app.findall('map')}==dict({
        'map_frame':'rw','map_heap':'rw','map_input':'rw','map_observation':'rw','desktop_display_mode':'r'},**({'map_store_control':'rw','map_store_bytes':'r'} if stored else {}),**({'map_private_cipher':'rw'} if private else {}),**({'map_session_key':'rw'} if locked else {}),**({'files_state':'r','files_request':'rw'} if files else {}))
    links={}
    for c in system.findall('channel'):
        a,b=list(c)
        for x,y in ((a,b),(b,a)):links[x.get('pd'),int(x.get('id'))]=(y.get('pd'),int(y.get('id')))
    assert {i:peer for (pd,i),peer in links.items() if pd=='offline_map'}==({0:('desktop_manager',29),1:('desktop_manager',28)}|({2:('map_store',1)} if stored else {})|({3:('entropy_rng',5),4:('timer_driver',14)} if private else {})|({5:('vault',13)} if locked else {})|({6:('files_picker',1)} if files else {}))
    if stored:validate_storage(system)
    if private:validate_private(system)
    for name in ('chat_ui','wallet_ui','native_browser','wallet_app','simplex_agent','vault'):
        assert not {'map_frame','map_heap','map_input','map_observation'} & {m.get('mr') for m in domains[name].findall('map')}
    for name in ('desktop_manager','desktop_gpu'):
        assert domains[name].find("map[@mr='map_frame']").get('perms')==('rw' if locked and name=='desktop_manager' else 'r')
    assert domains['desktop_gpu'].find("map[@mr='map_input']") is None
    if locked:
        assert private and stored and app.get('parent') is None
        assert app in domains['desktop_manager'].findall('protection_domain')
        assert {n for n,p in domains.items() if p.find("map[@mr='map_session_key']") is not None}=={'offline_map','vault','desktop_manager'}
        assert links['vault',13]==('offline_map',5)


def attach_mobile(system, ui, mapping, mr, channel, *, storage=False, private=False):
    mr('mobile_maps_frame',0x800000,page_size='0x200000')
    mr('mobile_maps_heap',0x1000000,page_size='0x200000')
    mr('mobile_maps_input',4096)
    mr('map_observation',8192,phys_addr='0x5fb96000')
    mr('mobile_maps_state',4096)
    app=ET.SubElement(system,'protection_domain',name='offline_map',priority='95',budget='8000',period='10000',stack_size='0x100000')
    ET.SubElement(app,'program_image',path='offline-map.elf')
    for name,va,symbol in [('mobile_maps_frame',0x60000000,'map_frame'),('mobile_maps_heap',0x64000000,'map_heap'),('mobile_maps_input',0x70000000,'input_handoff'),('map_observation',0x71000000,'map_observation')]:mapping(app,name,va,'rw',symbol)
    mapping(app,'mobile_maps_state',0x77000000,'rw','maps_ui_state')
    mapping(ui,'mobile_maps_frame',0x92000000,'r','mobile_maps_frame')
    mapping(ui,'mobile_maps_input',0x93000000,'rw','mobile_maps_input')
    mapping(ui,'mobile_maps_state',0x93001000,'r','mobile_maps_state')
    channel('offline_map',0,'slint_ui',19)
    channel('slint_ui',20,'offline_map',1)
    if storage:attach_storage(system,app,mapping,mr,channel)
    if private:attach_private(system,app,mapping,mr,channel)


def validate_mobile(system):
    domains={p.get('name'):p for p in system.iter('protection_domain')}
    uses=lambda name:{m.get('mr'):m.get('perms') for m in domains[name].findall('map')}
    stored='map_store' in domains
    files='files_picker' in domains
    private=system.find("memory_region[@name='map_private_cipher']") is not None
    assert uses('offline_map')==dict(mobile_maps_frame='rw',mobile_maps_heap='rw',mobile_maps_input='rw',map_observation='rw',mobile_maps_state='rw',**({'map_store_control':'rw','map_store_bytes':'r'} if stored else {}),**({'map_private_cipher':'rw'} if private else {}),**({'files_state':'r','files_request':'rw'} if files else {}))
    assert uses('slint_ui')['mobile_maps_frame']=='r' and uses('slint_ui')['mobile_maps_input']=='rw'
    assert uses('slint_ui')['mobile_maps_state']=='r'
    private_regions={'mobile_maps_frame','mobile_maps_heap','mobile_maps_input','map_observation','mobile_maps_state'}
    for name in domains:
        if name not in ('offline_map','slint_ui'):assert not private_regions&uses(name).keys(),name
    assert not {'mobile_maps_heap','map_observation'}&uses('slint_ui').keys()
    links={}
    for c in system.findall('channel'):
        a,b=list(c)
        for x,y in ((a,b),(b,a)):links[x.get('pd'),int(x.get('id'))]=(y.get('pd'),int(y.get('id')))
    assert {i:peer for (pd,i),peer in links.items() if pd=='offline_map'}==({0:('slint_ui',19),1:('slint_ui',20)}|({2:('map_store',1)} if stored else {})|({3:('entropy_rng',5),4:('timer_driver',14)} if private else {})|({6:('files_picker',1)} if files else {}))
    if stored:validate_storage(system)
    if private:validate_private(system)
    assert 'lock_auth' not in domains

def attach_storage(system, app, mapping, mr, channel):
    mr('map_store_control',4096)
    mr('map_store_bytes',0x800000,page_size='0x200000')
    mr('map_disk_regs',4096,phys_addr='0x0a002000')
    mr('map_disk_dma',0x20000,phys_addr='0x5f880000')
    mapping(app,'map_store_control',0x72000000,'rw','map_store_control')
    mapping(app,'map_store_bytes',0x76000000,'r','map_store_bytes')
    store=ET.SubElement(system,'protection_domain',name='map_store',priority='94',budget='8000',period='10000',stack_size='0x4000')
    ET.SubElement(store,'program_image',path='map-store.elf')
    for name,va,perms in [('map_store_control',0x72000000,'rw'),('map_store_bytes',0x73000000,'rw'),('map_disk_regs',0x74000000,'rw'),('map_disk_dma',0x75000000,'rw')]:
        mapping(store,name,va,perms,name)
        if name in ('map_disk_regs','map_disk_dma'):store.findall('map')[-1].set('cached','false')
    ET.SubElement(store,'irq',irq='64',id='0',trigger='edge')
    channel('offline_map',2,'map_store',1)

def validate_storage(system):
    domains={p.get('name'):p for p in system.iter('protection_domain')}
    # Public hashing may use idle CPU, but it must remain preemptible by Maps
    # drawing and every interactive frontend. Keep a bounded reservation.
    service=domains['map_store'];app=domains['offline_map'];files='files_picker' in domains
    assert int(service.get('priority'))<int(app.get('priority'))
    assert 0<int(service.get('budget'))<int(service.get('period'))<=10000
    expected={'map_store_control':'rw','map_store_bytes':'rw','map_disk_regs':'rw','map_disk_dma':'rw'}
    if files:expected.update(files_state='r',files_chunk='r')
    if system.find("memory_region[@name='map_private_cipher']") is not None:expected['map_private_cipher']='rw'
    assert {m.get('mr'):m.get('perms') for m in domains['map_store'].findall('map')}==expected
    private=set(expected)-{'files_state','files_chunk'}
    for name,pd in domains.items():
        if name not in ('offline_map','map_store') and not (name=='desktop_manager' and 'lock_auth' in domains):assert not private&{m.get('mr') for m in pd.findall('map')},name
    assert [(i.get('irq'),i.get('id')) for i in domains['map_store'].findall('irq')]==[('64','0')]
    links=[(e.get('pd'),e.get('id')) for c in system.findall('channel') for e in c if any(x.get('pd')=='map_store' for x in c)]
    assert sorted(links)==sorted([('map_store','1'),('offline_map','2')]+([('map_store','3'),('files_picker','2')] if files else []))


def attach_private(system,app,mapping,mr,channel):
    mr('map_private_cipher',8192)
    mapping(app,'map_private_cipher',0x78000000,'rw','map_private_cipher')
    store=system.find("protection_domain[@name='map_store']")
    mapping(store,'map_private_cipher',0x76000000,'rw','map_private_cipher')
    # Dedicated entropy endpoint; no access to another application's data/key.
    channel('offline_map',3,'entropy_rng',5,True)
    channel('offline_map',4,'timer_driver',14,True)


def validate_private(system):
    domains={p.get('name'):p for p in system.iter('protection_domain')}
    owners={n for n,p in domains.items() if p.find("map[@mr='map_private_cipher']") is not None}
    assert owners==({'offline_map','map_store','desktop_manager'} if 'lock_auth' in domains else {'offline_map','map_store'})
    assert all(domains[n].find("map[@mr='map_private_cipher']").get('perms')=='rw' for n in owners)
    for c in system.findall('channel'):
        ends=list(c)
        for i,e in enumerate(ends):
            if e.get('pp')=='true':assert int(domains[e.get('pd')].get('priority'))<int(domains[ends[1-i].get('pd')].get('priority'))
