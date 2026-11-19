#!/usr/bin/env python3
"""Pin the source/DT assumptions of the standalone PAS4 auth-return test.

This checks Linux caller inputs, not secure-world behavior or full power/DMA
readiness. The test has no SMP2P/GLINK writer and cannot report modem READY.
"""
import hashlib
import importlib.util
import json
from pathlib import Path

PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
spec=importlib.util.spec_from_file_location('audit',PORT/'audit-capdl-memory.py')
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)

def inspect(dtb):
    node='/soc/qcom,mss@4080000';smp='/soc/qcom,smp2p-modem'
    props=set(audit.fdtget(dtb,'-p',node).splitlines())
    if props & {'qcom,active-reg-names','qcom,active-clock-names'}:
        raise ValueError('active regulator/clock requirements changed')
    for key,expected in [('compatible','qcom,pil-tz-generic'),('qcom,firmware-name','modem'),
                         ('qcom,proxy-reg-names','vdd_cx vdd_mss'),
                         ('qcom,proxy-clock-names','xo')]:
        if audit.fdtget(dtb,'-t','s',node,key)!=expected:raise ValueError('changed '+key)
    for key,expected in [('qcom,pas-id',[4]),('qcom,smem-id',[421]),
                         ('qcom,vdd_cx-uV-uA',[0x181,100000]),
                         ('qcom,vdd_mss-uV-uA',[0x181,100000])]:
        if audit.read_cells(dtb,node,key)!=expected:raise ValueError('changed '+key)
    names=audit.fdtget(dtb,'-t','s',node,'interrupt-names').split()
    if names!=['qcom,wdog','qcom,err-fatal','qcom,err-ready','qcom,proxy-unvote','qcom,stop-ack','qcom,shutdown-ack']:
        raise ValueError('changed modem status events')
    child=audit.read_cells(dtb,smp+'/slave-kernel','phandle')[0]
    irq=audit.read_cells(dtb,node,'interrupts-extended')
    if len(irq)!=19 or irq[1:4]!=[0,0x10a,1] or irq[4:]!=[n for bit in [0,1,2,3,7] for n in [child,bit,0]]:
        raise ValueError('changed status interrupt routing')
    if audit.read_cells(dtb,smp,'qcom,smem')!=[435,428] or audit.read_cells(dtb,smp,'interrupts')!=[0,451,1]:
        raise ValueError('changed SMP2P transport')
    return {'dtb_sha256':hashlib.sha256(dtb.read_bytes()).hexdigest(),
            'active_regulator_count':0,'active_clock_count':0,'proxy_regulators':['vdd_cx','vdd_mss'],
            'proxy_clock':'xo','pas_id':4,'watchdog_intid':298,'smp2p_parent_intid':483,
            'smp2p_inbound_item':435,'smp2p_outbound_item':428,
            'slave_kernel_bits':dict(zip(names[1:],[0,1,2,3,7]))}

def main():
    records=[inspect(p) for p in [audit.DEFAULT_DTB,ROOT/'.cache/pixel-sunfish/live-runtime.dtb']]
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/soc/qcom/subsys-pil-tz.c'
    text=source.read_text()
    for token in ['PAS_AUTH_AND_RESET_CMD = 5','desc.arginfo = SCM_ARGS(1);',
                  'of_read_clocks(dev, &d->clks, "qcom,active-clock-names")',
                  'of_read_regs(dev, &d->regs, "qcom,active-reg-names")']:
        if token not in text:raise ValueError('changed matched caller')
    print(json.dumps({'device_trees':records,'caller_source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
          'auth_envelope':['0x42000205','0x1','0x4','0x0','0x0','0x0','0x0','0x0'],
          'scope':'Source/DT caller assumptions only. Existing proxy/SCM bus holds are empirical resource experiments. No full power, shutdown, DMA, secure-result or modem-ready proof. No ready claim or WLAN/DMA setup in the standalone auth-return test.'},indent=2))
if __name__=='__main__':main()
