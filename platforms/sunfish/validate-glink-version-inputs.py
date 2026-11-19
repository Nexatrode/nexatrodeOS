#!/usr/bin/env python3
"""Pin fixed VERSION messages, native offer and the exact bit12 modem mailbox."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
spec=importlib.util.spec_from_file_location('directory',PORT/'validate-glink-directory-inputs.py')
directory=importlib.util.module_from_spec(spec);spec.loader.exec_module(directory)
audit=directory.audit
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-boot-inputs.py')],text=True))
    for dtb in [audit.DEFAULT_DTB,ROOT/'.cache/pixel-sunfish/live-runtime.dtb']:
        mailbox='/soc/mailbox@17c00000';edge='/soc/qcom,glink/modem'
        if audit.read_cells(dtb,mailbox,'reg')!=[0x17c00000,4096] or audit.read_cells(dtb,mailbox,'#mbox-cells')!=[1]:raise ValueError('changed APCS provider')
        if audit.fdtget(dtb,'-t','s',mailbox,'compatible')!='qcom,sm8150-apcs-hmss-global':raise ValueError('changed APCS offset contract')
        if audit.read_cells(dtb,edge,'mboxes')!=[audit.read_cells(dtb,mailbox,'phandle')[0],12]:raise ValueError('changed GLINK mailbox channel')
    sources={}
    for relative in ['drivers/rpmsg/qcom_glink_native.c','drivers/rpmsg/qcom_glink_smem.c','drivers/mailbox/qcom-apcs-ipc-mailbox.c']:
        path=ROOT/'.cache/sunfish-lineage-kernel'/relative;sources[relative]=hashlib.sha256(path.read_bytes()).hexdigest()
    native=(ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c').read_text()
    for name,value in [('RPM_CMD_VERSION',0),('RPM_CMD_VERSION_ACK',1),('GLINK_VERSION_1',1)]:
        if not re.search(r'#define\s+'+name+r'\s+'+str(value)+r'\b',native):raise ValueError('changed '+name)
    if 'glink->features &= features;' not in native:raise ValueError('changed version intersection')
    mailbox=(ROOT/'.cache/sunfish-lineage-kernel/drivers/mailbox/qcom-apcs-ipc-mailbox.c').read_text()
    if 'writel(BIT(idx), apcs->reg);' not in mailbox or '.compatible = "qcom,sm8150-apcs-hmss-global", .data = (void *)12' not in mailbox:raise ValueError('changed mailbox write')
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-boot-probe/native-header-observation.json').read_text())
    if [prior[k] for k in ['command','version','features']]!=['0x0','0x1','0x7'] or not prior['header_fields_confirmed']:raise ValueError('missing native version offer')
    proof['pre_submission_zero_tx_passes']=proof.pop('post_auth_tx_zero_passes')
    proof.update(previous_native_version_offer=prior,version_sources_sha256=sources,local_features=0,
        fixed_tx_bytes=[1,0,1,0,0,0,0,0,0,0,1,0,0,0,0,0],tx_head_after=16,rx_tail_after=8,
        glink_doorbell_physical='0x17c0000c',glink_doorbell_mask='0x1000',max_glink_notifications=1,
        post_auth_glink_writes=True,glink_packets_sent=True,glink_notifications=True,max_wait_seconds=5,max_samples=512,
        ap_tx_head_and_rx_tail_unchanged=False,ap_indices_unchanged_until_submission=True,
        staged_fixed_tx_passes=2,runtime_scope_not_native_observation=True,
        channel_open=False,production_lifecycle_token=False,
        scope='Separately built one-shot version experiment after actual fresh preparation/AUTH/ready, using distinct FreshBoot evidence from Started rather than upgrading a readout identity. Require exact initial VERSION1/features7 with RX head8/tail0, zero TX and intact identities. Write only ACK1/features0 plus VERSION1/features0, verify before one AP TX head16 and RX tail8 store, then notify bit12 once outside lock. Poll read-only for TX tail16 and peer ACK1/features0, leaving that ACK unconsumed. No index reset, other packet/channel, IRQ, WLAN, SMMU write, host DMA, retry or cleanup. This does not implement production ownership/epoch/recovery.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
