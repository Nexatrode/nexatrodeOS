#!/usr/bin/env python3
"""Require five dedicated contiguous allocated pages outside all fixed reserves."""
from __future__ import annotations
import argparse
import importlib.util
import json
from pathlib import Path

PORT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('sunfish_memory_audit', PORT / 'audit-capdl-memory.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)

def validate(pages, reserved):
    chosen = sorted((base, size, name) for base, size, name in pages
                    if name.startswith('frame_mr_pas_metadata_'))
    if len(chosen) != 5 or any(size != 0x1000 for _, size, _ in chosen):
        raise ValueError('metadata requires exactly five allocated 4K pages')
    base = chosen[0][0]
    if not base or base % 0x1000 or [p[0] for p in chosen] != [base+i*0x1000 for i in range(5)]:
        raise ValueError('metadata physical pages are not contiguous and aligned')
    end = base + 0x5000
    for name, start, length, _ in reserved:
        if base < start + length and start < end:
            raise ValueError('metadata overlaps fixed reservation ' + name)
    for start, length, name in pages:
        if not name.startswith('frame_mr_pas_metadata_') and base < start+length and start < end:
            raise ValueError('metadata overlaps another allocated frame ' + name)
    return {'region_physical':hex(base), 'region_bytes':0x5000,
            'metadata_physical':hex(base+0x1000), 'metadata_capacity':0x3000,
            'guard_bytes_each':0x1000, 'physically_contiguous_pages':5,
            'fixed_reserved_overlap':False}

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--dtb', type=Path, default=audit.DEFAULT_DTB)
    args = p.parse_args()
    reserved, _ = audit.read_regions(args.dtb)
    print(json.dumps(validate(audit.read_pages(args.report), reserved), indent=2))

if __name__ == '__main__':
    main()
