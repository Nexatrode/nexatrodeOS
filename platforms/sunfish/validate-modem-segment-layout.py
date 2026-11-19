#!/usr/bin/env python3
"""Pin PAS4 reservation; admit only the isolated segment14 pages and read-only guards after MEM_SETUP."""
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
    allowed = audit.fixed_modem_segment_pages(system)
    chosen = audit.validate_modem_segment_frames(pages, allowed)
    remaining = [p for p in pages if p not in chosen]
    r = old.validate(remaining, kernel_regions, live_regions, pas_id, memory_reference, pool_handle)
    r.update({'ap_frames_in_modem_reservation': 4, 'mapped_segment_base': '0x8b780000',
              'writable_segment_pages': 2, 'read_only_guard_pages': 2,
              'file_bytes': 4160, 'zero_fill_bytes': 4032, 'memory_bytes': 8192,
              'segment_index': 14, 'other_modem_bytes_excluded': SIZE - 16384,
              'scope': 'One fixed non-executable segment with exact file/BSS lengths and read-only guards; not secure ownership, authentication or DMA proof'})
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
