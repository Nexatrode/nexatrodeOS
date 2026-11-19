#!/usr/bin/env python3
"""Pin PAS4 reservation; admit only the isolated whole-image staging pool and owned gap snapshot after MEM_SETUP."""
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


def validate(pages, kernel_regions, live_regions, pas_id, memory_reference, pool_handle, system, observe_ready=False, handshake=False, glink_prepare=False, fatal_trace=False, servreg_state_probe=False, servreg_restart_probe=False):
    allowed = audit.fixed_modem_image_pages(system,observe_ready,handshake,glink_prepare,fatal_trace,servreg_state_probe)
    chosen = audit.validate_modem_image_frames(pages, allowed)
    trace_allowed=audit.fixed_modem_fatal_trace_pages(system,fatal_trace)
    trace_pages=audit.validate_modem_fatal_trace_frames(pages,trace_allowed)
    remaining = [p for p in pages if p not in chosen and p not in trace_pages]
    r = old.validate(remaining, kernel_regions, live_regions, pas_id, memory_reference, pool_handle)
    snapshots = [p for p in pages if p[2].startswith('frame_mr_modem_gap_snapshot_')]
    if len(snapshots) != 1 or snapshots[0][1] != 0x200000 or snapshots[0][0] % 0x200000:
        raise ValueError('gap snapshot requires one dedicated owned aligned2MiB frame')
    snapshot = snapshots[0]
    for name, start, size, _ in kernel_regions + live_regions:
        if snapshot[0] < start + size and start < snapshot[0] + snapshot[1]:
            raise ValueError('gap snapshot overlaps fixed reservation ' + name)
    for start, size, name in pages:
        if (start,size,name) != snapshot and snapshot[0] < start+size and start < snapshot[0]+snapshot[1]:
            raise ValueError('gap snapshot aliases another allocated frame ' + name)
    r.update({'ap_frames_in_modem_reservation': 76, 'mapped_pool_bytes': SIZE,
              'modem_page_bytes': 0x200000, 'pool_permissions': 'RW/NX uncached',
              'file_bytes': 68393962, 'zero_fill_bytes': 89350155,
              'loaded_memory_bytes': 157744117, 'gap_bytes': 1639435,
              'gicd_watchdog_status_pages':len(trace_pages),
              'gicd_watchdog_status_access':'not mapped; distributor range is seL4-kernel-owned' if fatal_trace else None,
              'owned_gap_snapshot_physical': hex(snapshot[0]), 'owned_gap_snapshot_bytes': snapshot[1],
              'scope': ('SERVREG_NOTIF resolved msm/modem/wlan_pd state query only; no listener registration, no PDR restart, no WLFW client, no WLAN MSA mapping, no SCM ownership transfer, no WLAN power, DMA, or firmware start; SMC capability is limited to prerequisite modem PAS boot' if servreg_state_probe and not servreg_restart_probe else ('SERVREG_NOTIF listener profile sends at most one guarded PDR restart; no WLAN MSA mapping or ownership transfer, no WLAN power, DMA, or firmware start; SMC capability is limited to prerequisite modem PAS boot' if servreg_restart_probe else 'Entire fixed modem reservation mapped only to isolated staging domain, snapshot separately owned; not authentication, modem start or DMA proof'))})
    return r


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--system', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--kernel-dtb', type=Path, default=audit.DEFAULT_DTB)
    p.add_argument('--cached-live-dtb', type=Path, default=ROOT / '.cache/pixel-sunfish/live-runtime.dtb')
    p.add_argument('--observe-modem-ready',action='store_true')
    p.add_argument('--handshake-modem-status',action='store_true')
    p.add_argument('--prepare-glink-memory',action='store_true')
    p.add_argument('--fatal-trace',action='store_true')
    p.add_argument('--servreg-state-probe',action='store_true')
    p.add_argument('--servreg-restart-probe',action='store_true')
    args = p.parse_args()
    kernel, _ = audit.read_regions(args.kernel_dtb)
    live, _ = audit.read_regions(args.cached_live_dtb)
    node = '/soc/qcom,mss@4080000'
    if audit.fdtget(args.cached_live_dtb, '-t', 's', node, 'qcom,firmware-name') != 'modem':
        raise ValueError('changed cached modem firmware name')
    r = validate(audit.read_pages(args.report), kernel, live,
                 audit.read_cells(args.cached_live_dtb, node, 'qcom,pas-id'),
                 audit.read_cells(args.cached_live_dtb, node, 'memory-region'),
                 audit.read_cells(args.cached_live_dtb, '/reserved-memory/' + POOL, 'phandle'), args.system,args.observe_modem_ready,args.handshake_modem_status,args.prepare_glink_memory,args.fatal_trace,args.servreg_state_probe,args.servreg_restart_probe)
    r['cached_live_dtb_sha256'] = hashlib.sha256(args.cached_live_dtb.read_bytes()).hexdigest()
    r['kernel_dtb_sha256'] = hashlib.sha256(args.kernel_dtb.read_bytes()).hexdigest()
    print(json.dumps(r, indent=2))


if __name__ == '__main__':
    main()
