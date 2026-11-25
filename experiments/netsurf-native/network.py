# SPDX-License-Identifier: BSD-2-Clause
"""Add only network + entropy hardware to the disposable NetSurf image."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import xml.etree.ElementTree as ET


def build_worker(base, out, inputs, run, test_hooks=False, desktop=False):
    here = Path(__file__).resolve().parent
    repo = here.parents[1]
    board = inputs / 'sdk/board/qemu_virt_aarch64_el1/smp-debug'
    target = base / 'https-cargo'
    env = dict(os.environ, SEL4_PREFIX=str(inputs / 'sel4-install'),
               SEL4_INCLUDE_DIRS=str(board / 'include'), CARGO_TARGET_DIR=str(target),
               PATH='/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:' + os.environ['PATH'],
               CC_aarch64_sel4_microkit='/opt/homebrew/opt/llvm/bin/clang',
               AR_aarch64_sel4_microkit='/opt/homebrew/opt/llvm/bin/llvm-ar',
               CFLAGS_aarch64_sel4_microkit='--target=aarch64-none-elf -ffreestanding -I' + str(repo/'experiments/ghc-microkit/freestanding-headers'))
    names=[]
    if desktop: names.append('desktop-tcp')
    features=[]
    if test_hooks:
        from fixtures import prepare_tls
        ca=prepare_tls(base,run);env['NETSURF_TEST_CA_DER']=str(ca)
        names.append('test-fixtures')
        shutil.copyfile(ca,out/'test-ca.der')
    if names:features=['--features',','.join(names)]
    run(['cargo', '+nightly-2026-09-25', 'build', *features, '--locked', '--offline', '--release', '-Zjson-target-spec',
         '--config', repo / 'rust/.cargo/config.toml', '--manifest-path', here / 'https-worker/Cargo.toml',
         '--target', repo / 'rust/targets/aarch64-sel4-microkit.json'], out / 'https-worker.log', env=env, cwd=repo)
    # Preserve exact dependency licenses alongside this optional worker binary.
    metadata = subprocess.check_output(['cargo','+nightly-2026-09-25','metadata','--offline','--locked','--format-version','1','--manifest-path',str(here/'https-worker/Cargo.toml')],env=env,cwd=repo)
    notices=out/'licenses';notices.mkdir(exist_ok=True)
    inventory=[]
    for package in json.loads(metadata)['packages']:
        directory=Path(package['manifest_path']).parent
        item={key:package.get(key) for key in ('name','version','license','source','repository')}
        item['notice_files']=[]
        for path in directory.iterdir():
            if path.is_file() and path.name.lower().startswith(('license','copying','notice','copyright')):
                name='https-'+package['name']+'-'+package['version']+'-'+path.name
                shutil.copyfile(path,notices/name);item['notice_files'].append(name)
        inventory.append(item)
    (notices/'https-crates.json').write_text(json.dumps(inventory,indent=2)+'\n')
    shutil.copyfile(target/'aarch64-sel4-microkit/release/netsurf-https-worker.elf',out/'https-worker.elf')


def attach(root, base, out, inputs, run, test_hooks=False):
    repo=Path(__file__).resolve().parents[2]
    build_worker(base,out,inputs,run,test_hooks)
    donor = ET.parse(repo/'experiments/hvf-smp/accelerated.system').getroot()
    names = {'ethernet_driver','net_virt_tx','net_virt_rx','net_copier','dhcp_client','entropy_rng'}
    for pd in donor.findall('protection_domain'):
        if pd.get('name') in names:
            if pd.get('name')=='entropy_rng':pd.set('priority','124')
            root.append(pd)
    # Same virtual addresses and configured driver sections as the public donor.
    for ch in donor.findall('channel'):
        ends=ch.findall('end')
        if all(e.get('pd') in names for e in ends) or {e.get('pd') for e in ends}=={'timer_driver','dhcp_client'}:
            root.append(ch)
    used={m.get('mr') for pd in root.findall('protection_domain') for m in pd.findall('map')}
    existing={m.get('name') for m in root.findall('memory_region')}
    for mr in donor.findall('memory_region'):
        if mr.get('name') in used|{'rtc'} and mr.get('name') not in existing:
            root.insert(0,mr);existing.add(mr.get('name'))
    for name,size,page in [('https_heap',0x1000000,'0x200000'),('https_request',0x1000,'0x1000'),('https_response',0x205000,'0x1000')]:
        ET.SubElement(root,'memory_region',name=name,size=hex(size),page_size=page)
    worker=ET.SubElement(root,'protection_domain',name='browser_https',priority='101',budget='4000',period='10000',stack_size='0x40000')
    ET.SubElement(worker,'program_image',path='https-worker.elf')
    def mapping(pd,mr,va,symbol,perms):ET.SubElement(pd,'map',mr=mr,vaddr=hex(va),perms=perms,cached='false' if mr=='rtc' else 'true',setvar_vaddr=symbol)
    for mr,va,symbol,perms in [('https_heap',0x64000000,'https_heap','rw'),('https_request',0x68000000,'https_request','r'),('https_response',0x69000000,'https_response','rw'),('haskell_tcp_io',0x70000000,'haskell_tcp_page','rw'),('rtc',0x71000000,'https_rtc','r')]:mapping(worker,mr,va,symbol,perms)
    ui=root.find("protection_domain[@name='native_browser']")
    mapping(ui,'https_request',0x71000000,'https_request','rw');mapping(ui,'https_response',0x72000000,'https_response','r')
    # Channel 1 in timer_driver belongs to DHCP; 2 remains the display timer.
    for ch in root.findall('channel'):
        ends=ch.findall('end')
        if {e.get('pd') for e in ends}=={'native_browser','timer_driver'}:
            next(e for e in ends if e.get('pd')=='timer_driver').set('id','3')
    for left,lid,right,rid,pp in [('browser_https',0,'native_browser',3,False),('browser_https',1,'dhcp_client',4,True),('browser_https',2,'entropy_rng',0,True),('browser_https',3,'timer_driver',4,True),('native_browser',4,'browser_https',4,True)]:
        ch=ET.SubElement(root,'channel');ET.SubElement(ch,'end',pd=left,id=str(lid),**({'pp':'true','notify':'false'} if pp else {}));ET.SubElement(ch,'end',pd=right,id=str(rid),**({'notify':'false'} if pp and right!='timer_driver' else {}))
    for image in ('eth_driver.elf','network_virt_tx.elf','network_virt_rx.elf','network_copy.elf','dhcp_client.elf','rust_entropy_rng.elf'):
        shutil.copyfile(inputs/'pds'/image,out/image)
    assert not any(any(s in p.get('name','') for s in ('vault','wallet','simplex','disk')) for p in root.findall('protection_domain'))
    assert {m.get('mr') for m in ui.findall('map')}=={'slint_frame','browser_heap','input_handoff','ui_focus','ui_pointer','https_request','https_response'}
    return names|{'browser_https'}
