#!/usr/bin/env python3
"""Trace fixed read-only descriptor/header fields to the matched local source."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-directory-inputs.py')],text=True))
    directory=ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg'
    smem=(directory/'qcom_glink_smem.c').read_text();native=(directory/'qcom_glink_native.c').read_text()
    for assignment in ['tx_pipe->tail = &descs[0];','tx_pipe->head = &descs[1];','rx_pipe->tail = &descs[2];','rx_pipe->head = &descs[3];']:
        if assignment not in smem:raise ValueError('changed descriptor field '+assignment)
    for name,value in [('RPM_CMD_VERSION',0),('RPM_CMD_VERSION_ACK',1),('GLINK_VERSION_1',1)]:
        if not re.search(r'#define\s+'+name+r'\s+'+str(value)+r'\b',native):raise ValueError('changed '+name)
    if not re.search(r'struct glink_msg\s*\{\s*__le16 cmd;\s*__le16 param1;\s*__le32 param2;',native):raise ValueError('changed command header')
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-directory-probe/candidate.json').read_text())
    if prior['native_result']!='DIRECTORY SNAPSHOT PASS':raise ValueError('missing native inventory result')
    observation=prior['native_observation']
    if observation['present_mask']!='0x5' or observation['descriptor']['bytes']!='0x20' or observation['ap_rx_fifo']['bytes']!='0x4000':raise ValueError('changed observed transport shape')
    proof.update(metadata_only=False,glink_payload_reads=True,glink_writes=False,
        descriptor_bytes=32,rx_fifo_bytes=16384,max_rx_header_bytes=8,content_snapshot_passes=2,
        previous_native_directory_observation=observation,
        matched_glink_native_source_sha256=hashlib.sha256((directory/'qcom_glink_native.c').read_bytes()).hexdigest(),
        public_reference='https://android.googlesource.com/kernel/common/+/c2544fb30080aecc/drivers/rpmsg/qcom_glink_smem.c',
        scope='After the existing pre-AUTH AP status, authentication and sealed-ready gates, revalidate allocation/ready/AP identity around two exact32-byte descriptor and at-most8-byte pending RX-header reads. Never touch TX payload, advance/reset indices, allocate/publish GLINK, send GLINK notification/ACK or bind an IRQ. Point-in-time observation only, not negotiation, persistent ownership, WLAN or DMA containment.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
