#!/usr/bin/env python3
"""Pin PAS4 reservation; admit only the isolated segment23 page after MEM_SETUP."""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

PORT = Path(__file__).resolve().parent
ROOT = PORT.parents[1]
spec = importlib.util.spec_from_file_location('sunfish_memory_audit', PORT / 'audit-capdl-memory.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
old_spec = importlib.util.spec_from_file_location('mem_setup_layout', PORT / 'validate-pas-mem-setup-layout.py')
old = importlib.util.module_from_spec(old_spec)
old_spec.loader.exec_module(old)
BASE, SIZE = 0x8b000000, 0x09800000
POOL = 'modem_region@8b000000'


def validate(pages, kernel_regions, live_regions, pas_id, memory_reference, pool_handle, system):
    allowed = audit.fixed_modem_data_pages(system)
    chosen = [p for p in pages if audit.is_modem_data_page(p, allowed)]
    if len(chosen) != 1:
        raise ValueError('expected exactly one fixed 4K modem data frame')
    remaining = [p for p in pages if p not in chosen]
    r = old.validate(remaining, kernel_regions, live_regions, pas_id, memory_reference, pool_handle)
    r.update({'ap_frames_in_modem_reservation': 1, 'mapped_page_base': '0x93200000',
              'mapped_page_bytes': 4096, 'data_bytes': 45, 'data_segment_index': 23,
              'other_modem_bytes_excluded': SIZE - 4096,
              'scope': 'One fixed RW/NX uncached page for a gated non-executable data transfer; not secure ownership, authentication or DMA proof'})
    return r


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--kernel-dtb', type=Path, default=audit.DEFAULT_DTB)
    p.add_argument('--cached-live-dtb', type=Path, default=ROOT / '.cache/pixel-sunfish/live-runtime.dtb')
    args = p.parse_args()
    kernel, _ = audit.read_regions(args.kernel_dtb)
    live, _ = audit.read_regions(args.cached_live_dtb)
    node = '/soc/qcom,mss@4080000'
    if audit.fdtget(args.cached_live_dtb, '-t', 's', node, 'qcom,firmware-name') != 'modem':
        raise ValueError('changed cached modem firmware name')
    r = validate(audit.read_pages(args.report), kernel, live,
                 audit.read_cells(args.cached_live_dtb, node, 'qcom,pas-id'),
                 audit.read_cells(args.cached_live_dtb, node, 'memory-region'),
                 audit.read_cells(args.cached_live_dtb, '/reserved-memory/' + POOL, 'phandle'), args.system)
    r['cached_live_dtb_sha256'] = hashlib.sha256(args.cached_live_dtb.read_bytes()).hexdigest()
    r['kernel_dtb_sha256'] = hashlib.sha256(args.kernel_dtb.read_bytes()).hexdigest()
    print(json.dumps(r, indent=2))


if __name__ == '__main__':
    main()
