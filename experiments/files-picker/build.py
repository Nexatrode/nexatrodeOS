"""Trusted selected-file broker. Never mapped into an app's directory authority."""
import xml.etree.ElementTree as ET

def build(root,out,flags,board,sysroot,run,*,fixture=False):
    here=root/'experiments/files-picker'
    # Explicit disposable fixture seed, compiled out of non-fixture brokers.
    paths=[out/'local-packs/local-gibraltar-v1.nxregion',out/'local-packs/gibraltar-v2.nxregion']
    assets='.section .rodata\n.balign 8\n.global map_font,files_sample,files_sample_end,files_sample2,files_sample2_end\nmap_font:\n.incbin "'+str(root/'experiments/slint-notes/assets/fonts/Roboto-Regular.ttf')+'"\n'
    for label,path in (zip(('files_sample','files_sample2'),paths) if fixture else []):assets+='\n'+label+':\n.incbin "'+str(path)+'"\n'+label+'_end:\n'
    (out/'files-assets.S').write_text(assets)
    run('clang','--target=aarch64-none-elf','-c',out/'files-assets.S','-o',out/'files-assets.o')
    run('clang',*flags,*(['-DFILES_FIXTURE=1'] if fixture else []),'-c',here/'picker.c','-o',out/'files-picker.o')
    run('ld.lld','--gc-sections','-L',board/'lib','-L',sysroot/'lib','-T',board/'lib/microkit.ld',out/'files-picker.o',out/'files-assets.o','--start-group','-lmicrokit','-lc','-lm','--end-group','-o',out/'files-picker.elf')

def attach(system,mapping,mr,channel,*,mobile=False):
    domains={p.get('name'):p for p in system.iter('protection_domain')}
    root=domains['slint_ui' if mobile else 'desktop_manager']
    pd=ET.SubElement(system,'protection_domain',name='files_picker',priority='96',budget='4000',period='10000',stack_size='0x10000')
    ET.SubElement(pd,'program_image',path='files-picker.elf')
    for name,size in [('files_frame',0x800000 if mobile else 0x400000),('files_heap',0x800000),('files_input',4096),('files_state',4096),('files_request',4096),('files_chunk',4096),('files_vault_transfer',4096)]:
        mr(name,size,**({'phys_addr':'0x5fb9c000'} if name=='files_state' else {}),**({'page_size':'0x200000'} if size>=0x200000 else {}))
    for i,name in enumerate(('files_frame','files_heap','files_input','files_state','files_request','files_chunk','files_vault_transfer')):mapping(pd,name,0x60000000+i*0x1000000,'r' if name=='files_request' else 'rw',name)
    mapping(root,'files_input',0x95000000 if mobile else 0x68000000,'rw','files_input')
    mapping(root,'files_state',0x95001000 if mobile else 0x68001000,'r','files_state')
    if mobile:mapping(root,'files_frame',0x96000000,'r','files_frame')
    mapping(domains['offline_map'],'files_request',0x7a000000,'rw','files_request')
    mapping(domains['offline_map'],'files_state',0x7a001000,'r','files_state')
    mapping(domains['map_store'],'files_state',0x77000000,'r','files_state')
    mapping(domains['map_store'],'files_chunk',0x77001000,'r','files_chunk')
    mapping(domains['vault'],'files_vault_transfer',0x45003000,'rw','files_vault_transfer')
    channel('files_picker',0,root.get('name'),22 if mobile else 32)
    channel('files_picker',1,'offline_map',6)
    channel('map_store',3,'files_picker',2,True)
    # The broker's completion notification advances the background read loop.
    for c in system.findall('channel'):
        if any(e.get('pd')=='files_picker' and e.get('id')=='2' for e in c):
            for e in c:e.set('notify','true')
    channel('files_picker',3,'vault',15,True)
    channel('files_picker',4,'entropy_rng',6,True)
    if not mobile:
        # GPU is attached later by the desktop builder.
        mapping(pd,'desktop_display_mode',0x68000000,'r','desktop_display_mode')

def validate(system):
    d={p.get('name'):p for p in system.iter('protection_domain')}
    assert 'files_picker' in d
    uses=lambda n:{m.get('mr'):m.get('perms') for m in d[n].findall('map')}
    assert uses('offline_map')['files_state']=='r' and uses('map_store')['files_chunk']=='r'
    assert not {'files_input','files_frame','files_vault_transfer','files_heap','files_chunk'}&uses('offline_map').keys()
    assert {n for n in d if 'files_input' in uses(n)}<= {'files_picker','slint_ui','desktop_manager','maps_lock_manager'}
    assert {n for n in d if 'files_vault_transfer' in uses(n)}<= {'files_picker','vault','desktop_manager','maps_lock_manager'}
    for n in ('slint_ui','desktop_gpu'):
        if n in d:assert uses(n).get('files_frame')=='r'
    for n in ('wallet_app','simplex_agent','native_browser','chat_ui','wallet_ui'):
        if n in d:assert not any(x.startswith('files_') for x in uses(n))

    links={}
    for c in system.findall('channel'):
        a,b=list(c)
        for x,y in ((a,b),(b,a)):links[x.get('pd'),int(x.get('id'))]=(y.get('pd'),int(y.get('id')))
    mobile='slint_ui' in d
    assert {i:peer for (pd,i),peer in links.items() if pd=='files_picker'}=={
        0:('slint_ui',22) if mobile else ('desktop_manager',32),1:('offline_map',6),
        2:('map_store',3),3:('vault',15),4:('entropy_rng',6)}
    assert not any(peer[0]=='vault' for (pd,_),peer in links.items() if pd=='map_store')
    parent=d['maps_lock_manager' if mobile else 'desktop_manager']
    assert parent.find("protection_domain[@name='files_picker']") is not None,'picker must be covered by full-lock erasure'
