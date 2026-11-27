#!/usr/bin/env python3
"""Opt-in phone NetSurf + existing phone apps, fresh development-key disks only.

Public SDK, driver code and block configuration are read-only inputs. Never read
the installation manifest, installed vault ELF, any VM disk or personal app data.
"""
import argparse,hashlib,json,os,shutil,struct,subprocess,sys
from pathlib import Path
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT/'tests'),str(ROOT/'experiments/netsurf-native')]
from build import frontend,export_licenses,run as browser_run
from network import build_worker
import desktop_simplex_network

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('inputs','storage','composer','output'):p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--resume',action='store_true');p.add_argument('--observe',action='store_true',help='disposable-only UI observation; never personal data')
    p.add_argument('--maps',action='store_true',help='add the shared offline map renderer and private phone adapter')
    a=p.parse_args();inputs=a.inputs.resolve();out=a.output.resolve()
    if not out.is_relative_to(ROOT/'build/mobile-browser') or '.utm' in str(out):p.error('use a worktree build/mobile-browser staging directory')
    if out.exists() and any(out.iterdir()) and not (a.resume and (out/'.disposable-build').is_file()):p.error('use a fresh stage or --resume on a marked stage')
    out.mkdir(parents=True,exist_ok=True);(out/'.disposable-build').touch()
    env=dict(os.environ,PATH='/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:'+os.environ['PATH'],
        SEL4_PREFIX=str(inputs/'sel4-install'),SEL4_INCLUDE_DIRS=str(inputs/'sdk/board/qemu_virt_aarch64_el1/smp-debug/include'),
        CARGO_TARGET_DIR=str(ROOT/'build/mobile-browser-cargo'),CARGO_BUILD_JOBS='2')
    def run(*args,**kw):subprocess.run(list(map(str,args)),cwd=ROOT,env=kw.pop('env',env),check=True,**kw)
    def cargo(manifest,features,bins,target):
        e=dict(env,CARGO_TARGET_DIR=str(ROOT/'build'/target),CC_aarch64_sel4_microkit='clang',AR_aarch64_sel4_microkit='llvm-ar',ARFLAGS_aarch64_sel4_microkit='--format=gnu',CFLAGS_aarch64_sel4_microkit=f'--target=aarch64-none-elf -ffreestanding -I{ROOT}/experiments/ghc-microkit/freestanding-headers')
        cmd=['cargo','+nightly-2026-09-25','build','--locked','--offline','--release','-Zjson-target-spec','--config',ROOT/'rust/.cargo/config.toml','--manifest-path',ROOT/manifest,'--target',ROOT/'rust/targets/aarch64-sel4-microkit.json','--features',features]
        for name in bins:cmd+=['--bin',name]
        run(*cmd,env=e);return ROOT/'build'/target/'aarch64-sel4-microkit/release'
    system=ET.parse(ROOT/'experiments/hvf-smp/accelerated.system').getroot()
    demo={'client','crasher','server','c_reference'}
    for domain in list(system.findall('protection_domain')):
        if domain.get('name') in demo:system.remove(domain)
    for link in list(system.findall('channel')):
        if any(e.get('pd') in demo for e in link):system.remove(link)
    # Copy public code only; the vault is always newly compiled with a disposable
    # development key. No installed vault ELF or installation.json is inspected.
    for image in {p.get('path') for p in system.iter('program_image')} - {'rust_vault_disk.elf','slint_notes_utm.elf','slint_photo_worker.elf','slint_photo_reader.elf','slint_transcript_worker.elf','rust_messages_agent_app.elf','rust_entropy_rng.elf','rust_clock.elf','input_service.elf'}:
        shutil.copyfile(inputs/'pds'/image,out/image)
    ui=cargo('experiments/slint-notes/Cargo.toml','mobile-netsurf,simplex-agent-ui,utm-photo-worker,utm-transcript-worker,utm-device-lock'+(',mobile-maps' if a.maps else '')+(',mobile-browser-test' if a.observe else ''),['slint_notes_utm','slint_photo_worker','slint_photo_reader','slint_transcript_worker'],'mobile-browser-cargo')
    for name in ('slint_notes_utm','slint_photo_worker','slint_photo_reader','slint_transcript_worker'):shutil.copyfile(ui/(name+'.elf'),out/(name+'.elf'))
    backend=cargo('rust/Cargo.toml','storage-test,simplex-agent-vault,utm-slint-input,utm-photo-reader,utm-transcript-worker,utm-device-lock',['rust_vault_disk','rust_messages_agent_app','rust_entropy_rng','rust_input_service','rust_clock'],'mobile-browser-backends')
    for name in ('rust_vault_disk','rust_messages_agent_app','rust_entropy_rng','rust_clock'):shutil.copyfile(backend/(name+'.elf'),out/(name+'.elf'))
    shutil.copyfile(backend/'rust_input_service.elf',out/'input_service.elf')
    sections=subprocess.check_output(['llvm-readelf','-S',out/'rust_vault_disk.elf'],text=True,env=env)
    assert '.utm_vault_key' not in sections,'installed vault keys are forbidden'
    storage=a.storage.resolve()
    for name in ('blk_driver.elf','blk_virt.elf'):shutil.copyfile(storage/name,out/name)
    config=bytearray((storage/'blk_client_vault.data').read_bytes());assert len(config)==80 and config[:5]==b'sDDF\x02'
    for offset,address in ((8,0x48001000),(24,0x48002000),(40,0x48003000),(64,0x48004000)):struct.pack_into('<Q',config,offset,address)
    cfg=out/'public-block-config.data';cfg.write_bytes(config)
    run('llvm-objcopy','--update-section',f'.blk_client_config={cfg}',out/'rust_vault_disk.elf')
    run(ROOT/'experiments/ghc-microkit/build_wallet_scan_workers.sh',inputs/'sel4-install',out,'--handoff-prime-m1-100',env=dict(env,CARGO_TARGET_DIR=str(ROOT/'build/desktop-apps-wallet')))
    if not (out/'network').exists():desktop_simplex_network.prepare(ROOT,out,inputs,run)
    for name in ('dhcp_client.o','haskell_tcp_service.o','dhcp_client.elf'):(out/'network'/name).unlink(missing_ok=True)
    run('make','-C',out/'network','-j4','dhcp_client.elf',f'NET_SRC={ROOT}/tests/net',f'NET_INCLUDE={ROOT}/tests/net/include',f'MICROKIT_SDK={out}/network/sdk','MICROKIT_BOARD=qemu_virt_aarch64','MICROKIT_CONFIG=smp-debug',f'SDDF={inputs.parent}/sddf-personal-os',f'MBEDTLS_DIR={inputs.parent}/mbedtls-3.6.2','EXTRA_CFLAGS=-DDNS_SERVER_PORT=53 -DDESKTOP_BROWSER_NETWORK=1 -DDESKTOP_PRIVATE_TCP=1','NEXAWAL_UTM_WIDE_RX=1','NEXAWAL_UTM_RX_NOTIFY=1','NEXAWAL_UTM_RX_AHEAD=1')
    for section in ('timer_client_config','net_client_config','lib_sddf_lwip_config'):run('llvm-objcopy','--update-section',f'.{section}={out}/network/{section}.data',out/'network/dhcp_client.elf')
    shutil.copyfile(out/'network/dhcp_client.elf',out/'dhcp_client.elf')
    # Match the phone framebuffer contract by rebuilding the display client.
    import importlib.util
    spec=importlib.util.spec_from_file_location('accelerated_public_gpu',ROOT/'experiments/hvf-smp/accelerated.py');gpu=importlib.util.module_from_spec(spec);spec.loader.exec_module(gpu)
    os.environ['NEXATRODE_SDDF']=str(inputs.parent/'sddf-personal-os')
    gpu_stage=out/'display';gpu_stage.mkdir(exist_ok=True)
    if not (gpu_stage/'gpu/gpu_client.elf').is_file():gpu.build_gpu_from_source(inputs,gpu_stage)
    shutil.copyfile(gpu_stage/'gpu/gpu_client.elf',out/'gpu_client.elf')
    base=ROOT/'build/netsurf-native'
    frontend(base,out,inputs,3,https=True,javascript=True,mobile=True)
    build_worker(base,out,inputs,browser_run,desktop=True);export_licenses(base,out,True)
    def pd(name):return system.find(f"protection_domain[@name='{name}']")
    def region(name,size,**kw):ET.SubElement(system,'memory_region',name=name,size=hex(size),**kw)
    def mapping(domain,name,va,perms,symbol=None,cached=True):ET.SubElement(domain,'map',mr=name,vaddr=hex(va),perms=perms,cached=str(cached).lower(),**({'setvar_vaddr':symbol} if symbol else {}))
    def channel(left,lid,right,rid,pp=False):
        c=ET.SubElement(system,'channel');ET.SubElement(c,'end',pd=left,id=str(lid),**({'pp':'true','notify':'false'} if pp else {}));ET.SubElement(c,'end',pd=right,id=str(rid),**({'notify':'false'} if pp and right!='timer_driver' else {}))
    for name,size in [('mobile_browser_frame',0x800000),('browser_heap',0x4000000),('browser_https_heap',0x1000000),('mobile_browser_input',4096),('browser_request',4096),('browser_response',0x205000),('browser_tcp_io',4096),('wallet_tcp_io',4096)]:region(name,size,**({'page_size':'0x200000'} if size%0x200000==0 else {}))
    region('mobile_browser_state',4096,phys_addr='0x5fb90000')
    browser=ET.SubElement(system,'protection_domain',name='native_browser',priority='96',budget='2000',period='10000',stack_size='0x100000');ET.SubElement(browser,'program_image',path='browser.elf')
    for name,va,perms,symbol in [('mobile_browser_frame',0x60000000,'rw','browser_frame'),('browser_heap',0x64000000,'rw','browser_heap'),('mobile_browser_input',0x70000000,'rw','input_handoff'),('browser_request',0x71000000,'rw','https_request'),('browser_response',0x72000000,'r','https_response'),('mobile_browser_state',0x73000000,'rw','mobile_state')]:mapping(browser,name,va,perms,symbol)
    worker=ET.SubElement(system,'protection_domain',name='browser_https',priority='101',budget='2000',period='10000',stack_size='0x40000');ET.SubElement(worker,'program_image',path='https-worker.elf')
    for name,va,perms,symbol in [('browser_https_heap',0x64000000,'rw','https_heap'),('browser_request',0x68000000,'r','https_request'),('browser_response',0x69000000,'rw','https_response'),('browser_tcp_io',0x70000000,'rw','haskell_tcp_page'),('rtc',0x71000000,'r','https_rtc')]:mapping(worker,name,va,perms,symbol,cached=name!='rtc')
    for name,va,perms in [('mobile_browser_frame',0x90000000,'r'),('mobile_browser_input',0x91000000,'rw'),('mobile_browser_state',0x91001000,'r')]:mapping(pd('slint_ui'),name,va,perms,name)
    system.find("memory_region[@name='slint_heap']").set('size','0x4000000')
    pd('slint_ui').find("map[@mr='slint_heap']").set('vaddr','0x80000000')
    mapping(pd('dhcp_client'),'browser_tcp_io',0x49001000,'rw','browser_tcp_page');mapping(pd('dhcp_client'),'wallet_tcp_io',0x49002000,'rw','wallet_tcp_page')
    for name in ('wallet_app','scan_worker'):pd(name).find("map[@mr='haskell_tcp_io']").set('mr','wallet_tcp_io')
    if a.observe:
        region('mobile_ui_observation',8192,phys_addr='0x5fb92000');mapping(pd('slint_ui'),'mobile_ui_observation',0x91002000,'rw','desktop_test_observation')
    for left,lid,right,rid,pp in [('native_browser',0,'slint_ui',17,False),('native_browser',2,'timer_driver',9,True),('native_browser',3,'browser_https',0,False),('native_browser',4,'browser_https',4,True),('browser_https',1,'dhcp_client',8,True),('browser_https',2,'entropy_rng',0,True),('browser_https',3,'timer_driver',10,True)]:channel(left,lid,right,rid,pp)
    channel('slint_ui',18,'native_browser',1)
    if a.maps:
        spec=importlib.util.spec_from_file_location('shared_maps_adapter',ROOT/'experiments/desktop-apps/maps.py')
        maps=importlib.util.module_from_spec(spec);spec.loader.exec_module(maps)
        map_pin=maps.build(ROOT,out,inputs,run,mobile=True)
        maps.attach_mobile(system,pd('slint_ui'),mapping,region,channel)
        maps.validate_mobile(system)
    validate(system,a.observe)
    ET.indent(system);ET.ElementTree(system).write(out/'mobile.system',encoding='utf-8',xml_declaration=True)
    run(a.composer.resolve(),out/'mobile.system','--board','qemu_virt_aarch64_el1','--config','smp-debug','--search-path',out,'-o',out/'loader.img','-r',out/'report.txt',env=dict(env,MICROKIT_SDK=str(inputs/'sdk')))
    sources=[ROOT/'experiments/hvf-smp/accelerated.system',ROOT/'rust/Cargo.toml',ROOT/'experiments/slint-notes/Cargo.toml',ROOT/'experiments/slint-notes/ui/full_suite.slint',ROOT/'experiments/slint-notes/src/utm.rs',ROOT/'experiments/slint-notes/src/mobile_browser_utm.rs',Path(__file__)]
    sources += [x for x in (ROOT/'experiments/netsurf-native').rglob('*') if x.is_file() and '__pycache__' not in str(x)]
    sources += [ROOT/'experiments/mobile-browser/lab.html',ROOT/'tests/mobile_browser_utm.py',ROOT/'tests/mobile_browser_input.py']
    digest=lambda x:hashlib.sha256(x.read_bytes()).hexdigest()
    if a.maps:
        sources += [x for x in (ROOT/'experiments/comaps-native').rglob('*') if x.is_file() and '__pycache__' not in str(x)]
        sources += [ROOT/'experiments/desktop-apps/maps.py',ROOT/'experiments/slint-notes/src/mobile_maps_utm.rs',ROOT/'experiments/slint-notes/assets/icons/maps.svg',ROOT/'tests/mobile_maps_input.py',ROOT/'tests/mobile_maps_contract_test.py']
    manifest=dict(profile='mobile-netsurf-maps' if a.maps else 'mobile-netsurf',maps=a.maps,saved_data_used=False,development_storage_key=True,observation=a.observe,resolution=[1080,2340],browser_css_width=360,browser_scale=3,javascript=True,https=True,certificate_verification=True,private_tcp_slot=4,full_session_lock=False,source_commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),sources={str(x.relative_to(ROOT)):digest(x) for x in sources},artifacts={x.name:digest(x) for x in out.glob('*.elf')},composer_sha256=digest(a.composer.resolve()),loader_sha256=digest(out/'loader.img'))
    if a.maps:manifest['map_region']=map_pin
    (out/'build.json').write_text(json.dumps(manifest,indent=2)+'\n');print('PASS mobile browser with existing phone apps; disposable development key only',flush=True)

def validate(system,observe=False):
    domains={p.get('name'):p for p in system.findall('protection_domain')}
    sizes={m.get('name'):int(m.get('size').replace('_',''),0) for m in system.findall('memory_region')}
    for name,domain in domains.items():
        ranges=sorted((int(m.get('vaddr').replace('_',''),0),sizes[m.get('mr')],m.get('mr')) for m in domain.findall('map'))
        for (address,size,mr),(next_address,_,next_mr) in zip(ranges,ranges[1:]):
            assert address+size<=next_address,(name,mr,next_mr,'overlapping mappings')
    maps=lambda n:{m.get('mr'):m for m in domains[n].findall('map')}
    assert set(maps('native_browser'))=={'mobile_browser_frame','browser_heap','mobile_browser_input','browser_request','browser_response','mobile_browser_state'}
    assert set(maps('browser_https'))=={'browser_https_heap','browser_request','browser_response','browser_tcp_io','rtc'}
    assert {n:m.get('perms') for n,m in maps('native_browser').items()}==dict(mobile_browser_frame='rw',browser_heap='rw',mobile_browser_input='rw',browser_request='rw',browser_response='r',mobile_browser_state='rw')
    assert {n:m.get('perms') for n,m in maps('browser_https').items()}==dict(browser_https_heap='rw',browser_request='r',browser_response='rw',browser_tcp_io='rw',rtc='r')
    for name in ('vault','wallet_app','simplex_agent','messages_app'):
        assert not (set(maps('native_browser'))|set(maps('browser_https')))&set(maps(name)),name
    assert maps('slint_ui')['mobile_browser_frame'].get('perms')=='r'
    assert maps('slint_ui')['mobile_browser_state'].get('perms')=='r'
    assert maps('browser_https')['rtc'].get('perms')=='r'
    assert maps('wallet_app')['wallet_tcp_io'].get('perms')=='rw'
    assert ('mobile_ui_observation' in maps('slint_ui'))==observe

if __name__=='__main__':main()
