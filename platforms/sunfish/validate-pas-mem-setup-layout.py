#!/usr/bin/env python3
"""Pin PAS4's cached DT reservation and exclude its full range from all AP frames."""
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
BASE, SIZE = 0x8b000000, 0x09800000
POOL = 'modem_region@8b000000'


def validate(pages, kernel_regions, live_regions, pas_id, memory_reference, pool_handle):
    if pas_id != [4] or len(pool_handle) != 1 or memory_reference != pool_handle:
        raise ValueError('PAS4 does not reference the pinned modem pool')
    expected = (POOL, BASE, SIZE, True)
    for label, regions in [('kernel', kernel_regions), ('cached live', live_regions)]:
        pools = [r for r in regions if r[0] == POOL]
        if pools != [expected]:
            raise ValueError(label + ': changed/missing/ambiguous modem no-map reservation')
        for name, start, size, _ in regions:
            if name != POOL and start < BASE + SIZE and BASE < start + size:
                raise ValueError(label + ': modem pool overlaps another fixed reservation ' + name)
    if not pages:
        raise ValueError('missing generated AP frames')
    for start, size, name in pages:
        if start < BASE + SIZE and BASE < start + size:
            raise ValueError('modem reservation is AP allocated/mapped by ' + name)
    return {'pas_id': 4, 'modem_base': hex(BASE), 'modem_bytes': hex(SIZE),
            'modem_end': hex(BASE + SIZE), 'no_map': True,
            'kernel_and_cached_live_reservation_match': True,
            'ap_frames_in_modem_reservation': 0,
            'scope': 'Build-time fixed range exclusion only, not secure-world ownership, authentication or DMA proof'}


def main():
    p = argparse.ArgumentParser(description=__doc__)
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
                 audit.read_cells(args.cached_live_dtb, '/reserved-memory/' + POOL, 'phandle'))
    r['cached_live_dtb_sha256'] = hashlib.sha256(args.cached_live_dtb.read_bytes()).hexdigest()
    r['kernel_dtb_sha256'] = hashlib.sha256(args.kernel_dtb.read_bytes()).hexdigest()
    print(json.dumps(r, indent=2))


if __name__ == '__main__':
    main()
