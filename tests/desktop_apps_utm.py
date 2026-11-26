#!/usr/bin/env python3
"""Build existing mobile backends in two isolated, 2x desktop windows.

Public prepared drivers are read-only inputs. Only a new disposable disk may be
used: this fixture deliberately uses the development vault key, not enrollment
or the user's installation key. Window close hides; it does not evict services.
"""
import argparse, copy, hashlib, json, os
from pathlib import Path
import shutil, struct, subprocess, sys
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[1]
BOARD='qemu_virt_aarch64_el1'; CONFIG='smp-debug'

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--inputs',type=Path,required=True)
    p.add_argument('--storage',type=Path,required=True)
    p.add_argument('--shell-base',type=Path,required=True)
    p.add_argument('--composer',type=Path,required=True,help='large-page composer for the combined app ELF footprint')
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--resume',action='store_true')
    p.add_argument('--browser',action='store_true',help='add windowed NetSurf with its own HTTPS session')
    p.add_argument('--virgl',action='store_true',help='experimental isolated retained-texture GPU compositor')
    p.add_argument('--gpu-verify',action='store_true',help='export GPU readback for pixel tests (adds frame overhead)')
    p.add_argument('--desktop-resize',action='store_true',help='native display-only SPICE agent and bounded 2x dynamic viewport')
    a=p.parse_args()
    if a.desktop_resize and not (a.browser and a.virgl):p.error('--desktop-resize requires --browser --virgl')
    inputs=a.inputs.resolve(); out=a.output.resolve(); storage=a.storage.resolve(); shellbase=a.shell_base.resolve()
    if (out.exists() and not (a.resume and (out/'.disposable-build').is_file())) or not out.is_relative_to(ROOT/'build') or out.is_relative_to(inputs): p.error('use a fresh worktree build directory')
    out.mkdir(parents=True,exist_ok=a.resume)
    (out/'.disposable-build').touch()
    sdk=inputs/'sdk';board=sdk/'board'/BOARD/CONFIG
    env=dict(os.environ,PATH='/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:'+os.environ['PATH'],
        SEL4_PREFIX=str(inputs/'sel4-install'),SEL4_INCLUDE_DIRS=str(board/'include'),
        CARGO_TARGET_DIR=str(ROOT/'build/desktop-apps-cargo'),
        SLINT_FONT_SIZES='12,14,15,16,18,20,22,24,25,28,30,32,38,42,44,48,52')
    def run(*args,**kw):subprocess.run(list(map(str,args)),cwd=ROOT,env=kw.pop("env",env),check=True,**kw)
    def cargo(manifest,features,bins,target='build/desktop-apps-cargo'):
        e=dict(env,CARGO_TARGET_DIR=str(ROOT/target))
        if manifest=='rust/Cargo.toml':e.update(CC_aarch64_sel4_microkit='clang',AR_aarch64_sel4_microkit='llvm-ar',ARFLAGS_aarch64_sel4_microkit='--format=gnu',CFLAGS_aarch64_sel4_microkit=f'--target=aarch64-none-elf -ffreestanding -I{ROOT}/experiments/ghc-microkit/freestanding-headers')
        command=['cargo','+nightly-2026-09-25','build','--locked','--offline','--release','-Zjson-target-spec','--config',ROOT/'rust/.cargo/config.toml','--manifest-path',ROOT/manifest,'--target',ROOT/'rust/targets/aarch64-sel4-microkit.json','--features',features]
        for b in bins:command+=['--bin',b]
        subprocess.run(list(map(str,command)),cwd=ROOT,env=e,check=True)
        return ROOT/target/'aarch64-sel4-microkit/release'
    target=cargo('experiments/slint-notes/Cargo.toml','desktop-chat-window,desktop-apps-observe'+(',desktop-resize-app' if a.desktop_resize else ''),['slint_notes_utm','slint_photo_reader','slint_photo_worker','slint_transcript_worker'])
    shutil.copyfile(target/'slint_notes_utm.elf',out/'chat_ui.elf')
    for b in ('slint_photo_reader','slint_photo_worker','slint_transcript_worker'):shutil.copyfile(target/(b+'.elf'),out/(b+'.elf'))
    target=cargo('experiments/slint-notes/Cargo.toml','desktop-wallet-window,desktop-apps-observe'+(',desktop-resize-app' if a.desktop_resize else ''),['slint_notes_utm'])
    shutil.copyfile(target/'slint_notes_utm.elf',out/'wallet_ui.elf')
    target=cargo('experiments/slint-notes/Cargo.toml','desktop-resize-shell' if a.desktop_resize else 'desktop-virgl-shell' if a.virgl else 'desktop-browser-shell' if a.browser else 'desktop-apps-shell',['desktop_shell'])
    shutil.copyfile(target/'desktop_shell.elf',out/'desktop_shell.elf')
    bins=['rust_vault_disk','rust_messages_agent_app','rust_entropy_rng','rust_clock']
    target=cargo('rust/Cargo.toml','storage-test,simplex-agent-vault,utm-slint-input,utm-photo-reader,utm-transcript-worker',bins,'build/desktop-apps-backends')
    for b in bins:shutil.copyfile(target/(b+'.elf'),out/(b+'.elf'))
    target=cargo('rust/Cargo.toml','wallet-https',['rust_scan_worker'],'build/desktop-apps-backends')
    shutil.copyfile(target/'rust_scan_worker.elf',out/'rust_scan_worker.elf')
    subprocess.run([str(ROOT/'experiments/ghc-microkit/build_wallet_scan_workers.sh'),str(inputs/'sel4-install'),str(out),'--handoff-prime-m1-100'],cwd=ROOT,env=dict(env,CARGO_TARGET_DIR=str(ROOT/'build/desktop-apps-wallet')),check=True)
    subprocess.run(['cargo','+nightly-2026-09-25','build','--offline','--locked','--manifest-path',str(ROOT/'rust/walletcore-core/Cargo.toml'),'--example','disposable_wallet'],cwd=ROOT,env=dict(env,CARGO_TARGET_DIR=str(ROOT/'build/desktop-apps-host')),check=True)
    if a.browser:
        sys.path.insert(0,str(ROOT/'experiments/desktop-apps'))
        import browser as desktop_browser
        desktop_browser.build(ROOT,out,inputs,ROOT/'build/netsurf-native',resize=a.desktop_resize)
    # Only the public agent code/config is imported. Never the vault ELF.
    shutil.copyfile(inputs/'pds/agentstore_probe.elf',out/'agentstore_probe.elf')
    for b in ('blk_driver.elf','blk_virt.elf'):shutil.copyfile(storage/b,out/b)
    for section in ('blk_client_config',):
        data=out/'blk-client-public-config.data'
        config=bytearray((storage/'blk_client_vault.data').read_bytes())
        assert len(config)==80 and config[:5]==b'sDDF\x02'
        for offset,address in [(8,0x48001000),(24,0x48002000),(40,0x48003000),(64,0x48004000)]:struct.pack_into('<Q',config,offset,address)
        data.write_bytes(config)
        run('llvm-objcopy','--update-section',f'.{section}={data}',out/'rust_vault_disk.elf')
    import desktop_simplex_network
    if not (a.resume and (out/'network/sdk/board/qemu_virt_aarch64').exists()):
        desktop_simplex_network.prepare(ROOT,out,inputs,run)
    else:
        assert (out/'network/sdk/board/qemu_virt_aarch64').resolve()==(inputs/'sdk/board/qemu_virt_aarch64_el1').resolve()
        for section in ('timer_client_config','net_client_config','lib_sddf_lwip_config'):
            run('llvm-objcopy','--dump-section',f'.{section}={out}/network/{section}.data',inputs/'pds/dhcp_client.elf')
    # This static fixture has no session lock manager. Do not import the
    # lock fixture's initially-closed network gate. Transport recovery remains.
    for name in ('dhcp_client.o','haskell_tcp_service.o','dhcp_client.elf'):(out/'network'/name).unlink(missing_ok=True)
    # Upgrade the network's wallet receive path.
    run('make','-C',out/'network','-j4','dhcp_client.elf',f'NET_SRC={ROOT}/tests/net',f'NET_INCLUDE={ROOT}/tests/net/include',f'MICROKIT_SDK={out}/network/sdk','MICROKIT_BOARD=qemu_virt_aarch64',f'MICROKIT_CONFIG={CONFIG}',f'SDDF={inputs.parent}/sddf-personal-os',f'MBEDTLS_DIR={inputs.parent}/mbedtls-3.6.2','EXTRA_CFLAGS=-DDNS_SERVER_PORT=53'+(' -DDESKTOP_BROWSER_NETWORK=1' if a.browser else ''),'NEXAWAL_UTM_WIDE_RX=1','NEXAWAL_UTM_RX_NOTIFY=1','NEXAWAL_UTM_RX_AHEAD=1')
    for section in ('timer_client_config','net_client_config','lib_sddf_lwip_config'):
        run('llvm-objcopy','--update-section',f'.{section}={out}/network/{section}.data',out/'network/dhcp_client.elf')
    shutil.copyfile(out/'network/dhcp_client.elf',out/'dhcp_client.elf')
    if a.virgl and not a.browser:p.error('VirGL currently requires the three-app fixture')
    if a.gpu_verify and not a.virgl:p.error('--gpu-verify requires --virgl')
    flags=['clang','--target=aarch64-none-elf','-mcpu=cortex-a53','-mgeneral-regs-only','-std=gnu11','-O2','-ffreestanding','-fno-stack-protector','-nostdlib','-Wall','-Wextra','-Werror','-I',board/'include']
    run(*flags,*(['-DDESKTOP_DYNAMIC'] if a.desktop_resize else []),*(['-DDESKTOP_VIRGL'] if a.virgl else []),*(['-DDESKTOP_APP_COUNT=3'] if a.browser else []),'-DDESKTOP_SCALE=2u','-DSLINT_FRAME_WIDTH=2560u','-DSLINT_FRAME_HEIGHT=1600u','-DSLINT_FRAME_REGION_BYTES=0x1000000u','-c',ROOT/'experiments/desktop-apps/manager.c','-o',out/'manager.o')
    run('ld.lld','-L',board/'lib',out/'manager.o','-lmicrokit','-T','microkit.ld','-o',out/'manager.elf')
    run(*flags,'-c',ROOT/'experiments/desktop-simplex/environment.c','-o',out/'environment.o')
    run('ld.lld','-L',board/'lib',out/'environment.o','-lmicrokit','-T','microkit.ld','-o',out/'environment.elf')
    # The shell base's public graphics inputs match its 2560x1600 frame contract.
    for b in ('gpu_client.elf',):shutil.copyfile(shellbase/b,out/b)
    for b in ('gpu_driver.elf','gpu_virt.elf','input_driver.elf','timer_driver.elf','timer_poller.elf'):
        shutil.copyfile(inputs/'pds'/b,out/b)
    source=ET.parse(ROOT/'experiments/hvf-smp/accelerated.system').getroot()
    shell=ET.parse(shellbase/'desktop.system').getroot()
    system=ET.Element('system')
    names={'vault','messages_app','wallet_app','scan_worker','clock','entropy_rng','blk_driver','blk_virt','simplex_agent','timer_driver','ethernet_driver','net_virt_tx','net_virt_rx','dhcp_client','net_copier','gpu_driver','gpu_virt','gpu_client','input_driver','photo_reader','transcript_worker','photo_worker','timer_poller','wallet_scan_1','wallet_scan_2','wallet_scan_3'}
    for pd in source.findall('protection_domain'):
        if pd.get('name') in names:
            q=copy.deepcopy(pd);name=q.get('name')
            priorities={'simplex_agent':107,'messages_app':106,'transcript_worker':104,'photo_reader':104,'photo_worker':104,'vault':110,'clock':120,'entropy_rng':121}
            if name in priorities:q.set('priority',str(priorities[name]))
            if name in {'simplex_agent','messages_app'}:
                q.attrib.pop('passive',None);q.set('budget','10000' if name=='simplex_agent' else '2500');q.set('period','10000')
            system.append(q)
    vault=system.find("protection_domain[@name='vault']")
    for m in vault.findall('map'):
        if m.get('setvar_vaddr')=='device_lock_transfer':vault.remove(m)
    # Use the desktop GPU mappings (phone-only readouts are not desktop authority).
    gpu=system.find("protection_domain[@name='gpu_client']");system.remove(gpu)
    system.append(copy.deepcopy(shell.find("protection_domain[@name='gpu_client']")))
    def pd(name):return system.find(f"protection_domain[@name='{name}']")
    def mr(name,size,**kw):ET.SubElement(system,'memory_region',name=name,size=hex(size),**kw)
    def mapping(p,name,addr,perms,symbol):ET.SubElement(p,'map',mr=name,vaddr=hex(addr),perms=perms,cached='true',setvar_vaddr=symbol)
    def channel(a,x,b,y,pp=False,notify=True):
        c=ET.SubElement(system,'channel');ea=ET.SubElement(c,'end',pd=a,id=str(x));eb=ET.SubElement(c,'end',pd=b,id=str(y))
        if pp:ea.set('pp','true')
        if not notify:ea.set('notify','false');eb.set('notify','false')
        return ea,eb
    for c in source.findall('channel'):
        ends={e.get('pd') for e in c}
        if ends<=names and not ('gpu_client'in ends and 'input_service'in ends):system.append(copy.deepcopy(c))
    manager=ET.SubElement(system,'protection_domain',name='desktop_manager',priority='125',budget='8000',period='10000',stack_size='0x20000')
    ET.SubElement(manager,'program_image',path='manager.elf')
    for name,addr,perms,symbol in [('slint_frame',0x62000000,'rw','output_frame'),('ui_focus',0x63000000,'rw','focus_page'),('chat_frame',0x60000000,'r','surface_a'),('wallet_frame',0x61000000,'r','surface_b'),('chat_focus',0x66000000,'rw','input_a'),('wallet_focus',0x66002000,'rw','input_b'),('input_handoff',0x67000000,'rw','input_handoff'),('shell_surface',0x69000000,'r','shell_surface'),('shell_state',0x6a000000,'rw','shell_state'),('shell_input',0x6a001000,'rw','shell_input'),('ui_pointer',0x6a002000,'rw','ui_pointer')]:mapping(manager,name,addr,perms,symbol)
    ds=copy.deepcopy(shell.find("protection_domain[@name='desktop_shell']"));system.append(ds)
    if a.browser:ds.set('priority','124')
    for a1,x,b1,y,pp in [('desktop_shell',0,'desktop_manager',61,True),('desktop_shell',1,'desktop_manager',20,False),('desktop_manager',21,'desktop_shell',3,False),('desktop_manager',22,'desktop_shell',2,False),('desktop_manager',2,'gpu_client',2,False),('input_driver',2,'desktop_manager',6,False),('clock',1,'desktop_shell',4,False)]:channel(a1,x,b1,y,pp)
    ui_source=source.find("protection_domain[@name='slint_ui']")
    for prefix,binary in [('chat','chat_ui.elf'),('wallet','wallet_ui.elf')]:
        ui=copy.deepcopy(ui_source);ui.set('name',prefix+'_ui');ui.find('program_image').set('path',binary)
        if prefix=='wallet':ui.set('priority','97')
        for m in ui.findall('map'):
            name=m.get('mr')
            if name=='device_lock_transfer' or (prefix=='wallet' and name in {'photo_mailbox','photo_read_mailbox','transcript_mailbox','live_receive_mailbox','navigation_mailbox','snapshot_mailbox'}):ui.remove(m);continue
            if name=='clock_view':continue
            if prefix=='chat' and name in {'messages_transfer','photo_mailbox','photo_read_mailbox','transcript_mailbox','live_receive_mailbox','navigation_mailbox','snapshot_mailbox'}:continue
            if prefix=='wallet' and name=='wallet_transfer':continue
            m.set('mr',prefix+'_'+name)
            if name=='slint_frame':m.set('mr',prefix+'_frame')
            if name=='ui_focus':m.set('mr',prefix+'_focus')
        mapping(ui,prefix+'_observation',0x72000000,'rw','desktop_test_observation')
        system.append(ui)
        channel(prefix+'_ui',1,'desktop_manager',24 if prefix=='chat' else 25)
        ea,_=channel(prefix+'_ui',8,'timer_driver',7 if prefix=='chat' else 8,True);ea.set('setvar_id','slint_timer_channel')
    # Chat-only workers and wallet-only backend authority are separate.
    for c in source.findall('channel'):
        ends=list(c);uiend=next((e for e in ends if e.get('pd')=='slint_ui'),None)
        if uiend is None:continue
        local=int(uiend.get('id'))
        if local in {5,9,10,11,12,13,14}:
            d=copy.deepcopy(c);next(e for e in d if e.get('pd')=='slint_ui').set('pd','chat_ui');system.append(d)
        if local==6:
            d=copy.deepcopy(c);next(e for e in d if e.get('pd')=='slint_ui').set('pd','wallet_ui');system.append(d)
    deny=ET.SubElement(system,'protection_domain',name='denied_services',priority='126',passive='true');ET.SubElement(deny,'program_image',path='environment.elf')
    remote=0
    for u,locals in [('chat_ui',(2,3,4,6,7)),('wallet_ui',(2,3,4,5,7))]:
        for local in locals:channel(u,local,'denied_services',remote,True,False);remote+=1
    # Source contains one phone network page split into bounded protocol slots.
    # Each frontend's unused TCP placeholder is private and has no network cap.
    used={m.get('mr') for m in system.iter('map')}
    originals={m.get('name'):m for m in source.findall('memory_region')}
    originals.update({m.get('name'):m for m in shell.findall('memory_region') if m.get('name') not in originals})
    for name in sorted(used):
        if name in {'chat_observation','wallet_observation'}:mr(name,8192,phys_addr='0x5fbb0000' if name.startswith('chat') else '0x5fbb2000')
        elif name in {'chat_frame','wallet_frame'}:mr(name,0x400000,page_size='0x200000')
        elif name in {'chat_focus','wallet_focus'}:mr(name,4096)
        elif name in originals:
            m=copy.deepcopy(originals[name]);
            if a.browser and name=='shell_state':m.set('phys_addr','0x5fbb4000')
            if name=='slint_frame':m.set('size','0x2000000' if a.desktop_resize else '0x1000000')
            if name=='shell_surface' and a.desktop_resize:m.set('size','0x2000000')
            system.append(m)
        else:
            base=name.split('_',1)[1]
            old=originals.get(base)
            if old is None:raise ValueError('unknown region '+name)
            m=copy.deepcopy(old);m.set('name',name);m.attrib.pop('phys_addr',None);system.append(m)
    if a.browser:desktop_browser.attach(system,manager,inputs,channel,mapping,mr)
    if a.virgl:
        gpu_irq=copy.deepcopy(pd('gpu_driver').find('irq'));gpu_irq.set('id','1')
        for name in ('gpu_client','gpu_driver','gpu_virt'):system.remove(pd(name))
        for c in list(system.findall('channel')):
            if any(e.get('pd') in {'gpu_client','gpu_driver','gpu_virt'} for e in c):system.remove(c)
        render=ET.SubElement(system,'protection_domain',name='desktop_gpu',priority='126',budget='2000',period='10000',stack_size='0x20000')
        ET.SubElement(render,'program_image',path='desktop_gpu.elf')
        render.append(gpu_irq)
        for name,addr,symbol in [('shell_surface',0x70000000,'shell_surface'),('shell_state',0x72000000,'shell_state'),('ui_pointer',0x72001000,'ui_pointer'),('chat_frame',0x74000000,'surface_a'),('wallet_frame',0x75000000,'surface_b'),('browser_frame',0x76000000,'surface_c')]:mapping(render,name,addr,'r',symbol)
        ET.SubElement(render,'map',mr='virtio_regs',vaddr='0x40000000',perms='rw',cached='false',setvar_vaddr='mmio')
        mr('desktop_gpu_dma',0x9000000 if a.desktop_resize else 0x6000000,page_size='0x200000')
        ET.SubElement(render,'map',mr='desktop_gpu_dma',vaddr='0x60000000',perms='rw',cached='false',setvar_vaddr='dma')
        ET.SubElement(render,'setvar',symbol='dma_paddr',region_paddr='desktop_gpu_dma')
        mr('shell_decorations',0xc00000,page_size='0x200000')
        mapping(pd('desktop_shell'),'shell_decorations',0x76000000,'rw','shell_decorations')
        mapping(render,'shell_decorations',0x7a000000,'r','shell_decorations')
        system.find("memory_region[@name='shell_heap']").set('size','0x8000000' if a.desktop_resize else '0x4000000')
        if a.desktop_resize:
            pd('desktop_shell').find("map[@mr='shell_heap']").set('vaddr','0x80000000')
        mr('gpu_observation',0x1000,phys_addr='0x5fba0000')
        mapping(render,'gpu_observation',0x7b000000,'rw','gpu_observation')
        if a.gpu_verify:
            frame=system.find("memory_region[@name='slint_frame']");frame.set('phys_addr','0x5d000000')
            mapping(render,'slint_frame',0x78000000,'rw','output_frame')
        channel('desktop_manager',2,'desktop_gpu',0)
        channel('desktop_gpu',2,'timer_driver',12,True)
        used={m.get('mr') for m in system.iter('map')}
        for r in list(system.findall('memory_region')):
            if r.get('name') not in used:system.remove(r)
        run(*flags,'-mstrict-align',*(['-DDESKTOP_DYNAMIC'] if a.desktop_resize else []),'-DDESKTOP_APP_COUNT=3','-DDESKTOP_SCALE=2u','-DSLINT_FRAME_WIDTH=2560u','-DSLINT_FRAME_HEIGHT=1600u','-DSLINT_FRAME_REGION_BYTES=0x1000000u',*(['-DDESKTOP_GPU_VERIFY'] if a.gpu_verify else []),'-c',ROOT/'experiments/desktop-gpu/renderer.c','-o',out/'desktop_gpu.o')
        run('ld.lld','-L',board/'lib',out/'desktop_gpu.o','-lmicrokit','-T','microkit.ld','-o',out/'desktop_gpu.elf')
    if a.desktop_resize:
        from desktop_resize_agent_build import attach,compile_agent
        attach(system);compile_agent(inputs,out)
        mr('desktop_display_mode',4096,phys_addr='0x5fb92000')
        manager.find("map[@mr='slint_frame']").set('vaddr','0x7c000000')
        manager.find("map[@mr='shell_surface']").set('vaddr','0x70000000')
        mapping(manager,'display_request',0x6b001000,'r','desktop_display_request')
        for name,addr,perms in [('desktop_manager',0x6b000000,'rw'),('desktop_shell',0x77000000,'r'),('desktop_gpu',0x7b001000,'r'),('chat_ui',0x73000000,'r'),('wallet_ui',0x73000000,'r'),('native_browser',0x73000000,'r')]:
            mapping(pd(name),'desktop_display_mode',addr,perms,'desktop_display_mode')
        mapping(pd('desktop_gpu'),'display_ack',0x7b002000,'rw','display_ack')
    # Retain the original 4-core wallet worker affinities; normal service PDs on CPU0.
    ET.indent(system);ET.ElementTree(system).write(out/'desktop-apps.system',encoding='utf-8',xml_declaration=True)
    from desktop_apps_manifest import validate
    validate(out/'desktop-apps.system',out,browser=a.browser)
    subprocess.run([str(a.composer.resolve()),str(out/'desktop-apps.system'),'--board',BOARD,'--config',CONFIG,'--search-path',str(out),'-o',str(out/'loader.img'),'-r',str(out/'report.txt')],cwd=ROOT,env=dict(env,MICROKIT_SDK=str(sdk)),check=True)
    sha=lambda x:hashlib.sha256(x.read_bytes()).hexdigest()
    sources=[Path(__file__),*[x for x in (ROOT/'experiments/desktop-apps').glob('*') if x.suffix in {'.c','.h','.rs','.py'}],ROOT/'experiments/slint-notes/ui/desktop_chat.slint',ROOT/'experiments/slint-notes/ui/full_suite.slint',ROOT/'experiments/desktop-shell/compositor.c',ROOT/'experiments/desktop-shell/shell.rs']
    sources += [ROOT/'experiments/slint-notes/Cargo.toml',ROOT/'experiments/slint-notes/build.rs',ROOT/'experiments/slint-notes/ui/desktop_shell.slint',ROOT/'experiments/hvf-smp/accelerated.system',*list((ROOT/'experiments/slint-notes/src').glob('*.rs')),ROOT/'tests/desktop_apps_manifest.py',ROOT/'rust/walletcore-core/examples/disposable_wallet.rs']
    sources += [*list((ROOT/'rust/walletcore-core/src').glob('*.rs')),ROOT/'rust/src/wallet_scan_pool.rs',ROOT/'rust/src/wallet_app.rs',ROOT/'rust/vendor/monero-oxide/wallet/src/scan.rs',ROOT/'rust/vendor/monero-oxide/wallet/src/output.rs',ROOT/'rust/vendor/monero-oxide/wallet/src/decoys.rs']
    sources += [ROOT/'experiments/desktop-shell/protocol.h',ROOT/'experiments/slint-notes/assets/icons/browser.svg',ROOT/'tests/desktop_browser_input.py',ROOT/'tests/desktop_apps_input.py',ROOT/'tests/desktop_apps_run.py',ROOT/'tests/desktop_display_config.py',ROOT/'tests/desktop_display_config_test.py']
    sources += [ROOT/'experiments/desktop-gpu/renderer.c',ROOT/'experiments/desktop-gpu/transport.h',ROOT/'experiments/desktop-gpu/pacing.h',ROOT/'tests/desktop_gpu_pacing_test.c',ROOT/'tests/desktop_gpu_capture.py',ROOT/'tests/desktop_gpu_navigation.py',ROOT/'tests/desktop_gpu_motion.py',ROOT/'tests/desktop_gpu_stall_trace.py',ROOT/'tests/desktop_gpu_stall_trace_test.py',ROOT/'tests/desktop_gpu_transport_test.c',ROOT/'tests/desktop_resize_agent_build.py',ROOT/'tests/desktop_resize_input.py',ROOT/'tests/desktop_resize_test.c',ROOT/'tests/desktop_resize_frame_test.rs',ROOT/'experiments/desktop-gpu/display_protocol.h',ROOT/'experiments/desktop-gpu/display_agent.c']
    sources += [ROOT/'tests'/name for name in ('desktop_boot_ready.py','desktop_boot_ready_test.py','desktop_boot_stress.py','desktop_boot_stress_test.py','utm_vault_key_storage.py')]
    if a.browser:
        sources += [p for p in (ROOT/'experiments/netsurf-native').rglob('*') if p.is_file() and '__pycache__' not in str(p)]
        sources += [ROOT/'tests/net/dhcp_client.c',ROOT/'tests/net/haskell_tcp_service.c',ROOT/'tests/net/include/haskell_tcp_protocol.h',ROOT/'experiments/guest-https/src/lib.rs',ROOT/'experiments/guest-https/Cargo.toml']
    (out/'build.json').write_text(json.dumps({'desktop_resize':a.desktop_resize,'resize_bounds':[[1600,1200],[3840,2160]] if a.desktop_resize else None,'virgl':a.virgl,'gpu_verify':a.gpu_verify,'browser':a.browser,'apps':['SimpleX','NexaWal']+(['NetSurf'] if a.browser else []),'scale':2,'resolution':[2560,1600],'vcpus':4,'saved_data_used':False,'development_storage_key':True,'static_services':True,'sources':{str(x.relative_to(ROOT)):sha(x) for x in sources if x.is_file()},'artifacts':{x.name:sha(x) for x in out.glob('*.elf')},'loader_sha256':sha(out/'loader.img')},indent=2)+'\n')

if __name__=='__main__':main()
