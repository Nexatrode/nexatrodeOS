#!/usr/bin/env python3
"""Reject an HVF system whose network ELFs disagree with its DMA layout or MAC."""
import argparse
from pathlib import Path
import struct
import subprocess
import tempfile
import xml.etree.ElementTree as ET

REGIONS = (
    ('network ring', 'virtio_mmio@a000000/drivers/network/virtio/mmio/hw_ring_buffer', 'device', 48),
    ('network RX buffer', 'virtio_mmio@a000000/net/rx/data/device', 'rx', 64),
    ('network TX buffer', 'virtio_mmio@a000000/net/tx/data/client/dhcp_client', 'tx', 104),
)
SECTIONS = {
    'device': ('eth_driver.elf', '.device_resources'),
    'rx': ('network_virt_rx.elf', '.net_virt_rx_config'),
    'tx': ('network_virt_tx.elf', '.net_virt_tx_config'),
    'client': ('dhcp_client.elf', '.net_client_config'),
}

def validate(root, sections):
    errors = []
    for label, name, key, offset in REGIONS:
        node = root.find(f"memory_region[@name='{name}']")
        if node is None or node.get('phys_addr') is None:
            errors.append(f'{label}: missing physical region')
            continue
        data = sections[key]
        if len(data) < offset + 8 or struct.unpack_from('<Q', data, offset)[0] != int(node.get('phys_addr'), 0):
            errors.append(f'{label}: ELF address does not match system')
    if len(sections['rx']) < 134 or len(sections['client']) < 126 or sections['rx'][128:134] != sections['client'][120:126]:
        errors.append('network client MAC does not match RX filter')
    return errors

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('system', type=Path)
    parser.add_argument('elf_dir', type=Path, help='directory containing the final network ELFs')
    parser.add_argument('--objcopy', default='/opt/homebrew/opt/llvm/bin/llvm-objcopy')
    args = parser.parse_args()
    sections = {}
    with tempfile.TemporaryDirectory() as tmp:
        for key, (filename, section) in SECTIONS.items():
            target = Path(tmp) / key
            subprocess.run([args.objcopy, '--dump-section', f'{section}={target}', str(args.elf_dir / filename)], check=True)
            sections[key] = target.read_bytes()
    errors = validate(ET.parse(args.system).getroot(), sections)
    if errors:
        for error in errors: print(error)
        raise SystemExit(1)
    print('Network DMA addresses and client MAC match the system.')

if __name__ == '__main__': main()
