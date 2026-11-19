#!/usr/bin/env python3
"""Pin read-only status geometry, synchronization and EL0 counter access."""
import hashlib
import importlib.util
import json
from pathlib import Path

PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
spec=importlib.util.spec_from_file_location('auth_inputs',PORT/'validate-modem-auth-inputs.py')
auth=importlib.util.module_from_spec(spec);spec.loader.exec_module(auth)
audit=auth.audit

def inspect(dtb):
    result=auth.inspect(dtb)
    def cells(node,key,expected):
        if audit.read_cells(dtb,node,key)!=expected:raise ValueError('changed '+node+'/'+key)
    def string(node,key,expected):
        if audit.fdtget(dtb,'-t','s',node,key)!=expected:raise ValueError('changed '+node+'/'+key)
    smem='/soc/qcom,smem';lock='/soc/hwlock';block='/soc/syscon@1f40000'
    string(smem,'compatible','qcom,smem')
    string(lock,'compatible','qcom,tcsr-mutex')
    cells(smem,'hwlocks',[audit.read_cells(dtb,lock,'phandle')[0],3])
    cells(lock,'syscon',[audit.read_cells(dtb,block,'phandle')[0],0,0x1000])
    cells(block,'reg',[0x1f40000,0x20000])
    pool='/reserved-memory/smem@86000000'
    cells(smem,'memory-region',audit.read_cells(dtb,pool,'phandle'))
    cells(pool,'reg',[0,0x86000000,0,0x200000])
    cells('/soc/qcom,smp2p-modem','qcom,local-pid',[0])
    cells('/soc/qcom,smp2p-modem','qcom,remote-pid',[1])
    # An alternate AP owner ID would invalidate the sole permitted lock writes.
    if 'mutex-id' in audit.fdtget(dtb,'-p',lock).splitlines():
        raise ValueError('alternate hardware-lock owner ID')
    result.update(smem_physical='0x86000000',smem_bytes=0x200000,
                  lock_physical='0x01f43000',lock_owner=1)
    return result

def main():
    records=[inspect(p) for p in [audit.DEFAULT_DTB,ROOT/'.cache/pixel-sunfish/live-runtime.dtb']]
    base=ROOT/'.cache/microkit-src-2.3.1-sunfish'
    config=base/'release/microkit-sdk-2.3.1-sunfish/board/sunfish/debug/include/kernel/gen_config.h'
    kernel=base/'build/sunfish/debug/sel4/build/kernel_all.c'
    if '#define CONFIG_EXPORT_PCNT_USER  1' not in config.read_text():
        raise ValueError('EL0 physical counter is not exported')
    if '#ifdef CONFIG_EXPORT_PCNT_USER\n    val |= EL0PCTEN;' not in kernel.read_text():
        raise ValueError('changed kernel physical-counter access setup')
    sources=[ROOT/'.cache/sunfish-lineage-kernel'/p for p in [
        'drivers/soc/qcom/smem.c','drivers/soc/qcom/smp2p.c','drivers/hwspinlock/qcom_hwspinlock.c']]
    if '#define QCOM_MUTEX_APPS_PROC_ID\t1' not in sources[2].read_text():
        raise ValueError('changed AP lock owner')
    print(json.dumps(dict(device_trees=records,physical_counter='CNTPCT_EL0',
        input_hashes={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest()
                      for p in [config,kernel]+sources},
        scope='Read-only status input assumptions. Only lock3 owner acquire/release writes; no SMEM writes, IRQ setup, transport negotiation, WLAN or host DMA. A ready bit alone is not lifecycle readiness.'),indent=2))
if __name__=='__main__':main()
