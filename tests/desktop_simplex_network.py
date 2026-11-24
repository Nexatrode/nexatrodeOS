"""Fresh-fixture network inputs; never builds or imports the installation vault."""
import copy
import os
from pathlib import Path
import shutil
import xml.etree.ElementTree as ET

NAMES={'ethernet_driver','net_virt_tx','net_virt_rx','dhcp_client','net_copier'}
IMAGES=('eth_driver.elf','network_virt_tx.elf','network_virt_rx.elf','network_copy.elf')

def prepare(root, out, inputs, run):
    build=out/'network';build.mkdir()
    alias=build/'sdk';(alias/'board').mkdir(parents=True)
    (alias/'board/qemu_virt_aarch64').symlink_to(inputs/'sdk/board/qemu_virt_aarch64_el1')
    (alias/'bin').symlink_to(inputs/'sdk/bin')
    shutil.copyfile(root/'tests/net/net.mk',build/'Makefile')
    # All source and build output are local. SDK and prepared public driver
    # ELFs are read-only; no installation.json or vault ELF is consulted.
    run('make','-C',build,'-j4','dhcp_client.elf',
        f'NET_SRC={root}/tests/net',f'NET_INCLUDE={root}/tests/net/include',
        f'MICROKIT_SDK={alias}','MICROKIT_BOARD=qemu_virt_aarch64','MICROKIT_CONFIG=smp-debug',
        f'SDDF={inputs.parent}/sddf-personal-os',
        f'MBEDTLS_DIR={inputs.parent}/mbedtls-3.6.2',
        'EXTRA_CFLAGS=-DDNS_SERVER_PORT=53 -DSIMPLEX_LOCK_LIFECYCLE=1',
        env=dict(os.environ,PATH='/opt/homebrew/opt/llvm/bin:'+os.environ['PATH']))
    obj='/opt/homebrew/opt/llvm/bin/llvm-objcopy'
    for section in ('timer_client_config','net_client_config','lib_sddf_lwip_config'):
        target=build/(section+'.data')
        run(obj,'--dump-section',f'.{section}={target}',inputs/'pds/dhcp_client.elf')
        run(obj,'--update-section',f'.{section}={target}',build/'dhcp_client.elf')
    shutil.copyfile(build/'dhcp_client.elf',out/'dhcp_client.elf')
    for name in IMAGES: shutil.copyfile(inputs/'pds'/name,out/name)

def compose(root, system, manager, channel):
    source=ET.parse(root/'experiments/hvf-smp/accelerated.system').getroot()
    for pd in source.findall('protection_domain'):
        if pd.get('name') in NAMES: system.append(copy.deepcopy(pd))
    for link in source.findall('channel'):
        ends={e.get('pd') for e in link}
        if ends <= NAMES|{'timer_driver'} and ends & NAMES:
            system.append(copy.deepcopy(link))
    used={m.get('mr') for m in system.iter('map')}
    existing={m.get('name') for m in system.findall('memory_region')}
    for m in source.findall('memory_region'):
        if m.get('name') in used and m.get('name') not in existing:
            system.append(copy.deepcopy(m));existing.add(m.get('name'))
    channel('session_agent',1,'dhcp_client',3,True)
    channel('runtime_manager',17,'dhcp_client',7,True)
