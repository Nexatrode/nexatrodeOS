#!/usr/bin/env python3
"""Pin GLINK item identifiers for metadata-only inspection after observed ready."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
spec=importlib.util.spec_from_file_location('handshake',PORT/'validate-smp2p-handshake-inputs.py')
handshake=importlib.util.module_from_spec(spec);spec.loader.exec_module(handshake)
audit=handshake.audit
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-smp2p-handshake-inputs.py')],text=True))
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_smem.c'
    text=source.read_text()
    names=['SMEM_GLINK_NATIVE_XPRT_DESCRIPTOR','SMEM_GLINK_NATIVE_XPRT_FIFO_0','SMEM_GLINK_NATIVE_XPRT_FIFO_1']
    for name,value in zip(names,[478,479,480]):
        if not re.search(r'#define\s+'+name+r'\s+'+str(value)+r'\b',text):raise ValueError('changed '+name)
    for dtb in [audit.DEFAULT_DTB,ROOT/'.cache/pixel-sunfish/live-runtime.dtb']:
        node='/soc/qcom,glink/modem'
        if audit.read_cells(dtb,node,'qcom,remote-pid')!=[1] or audit.fdtget(dtb,'-t','s',node,'transport')!='smem':
            raise ValueError('changed modem GLINK edge')
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-smp2p-preauth-probe/candidate.json').read_text())
    if prior['native_result']!='READY BIT SEEN' or not prior['ready_bit_observed']:
        raise ValueError('missing native ready observation')
    proof.update(glink_item_ids=[478,479,480],matched_glink_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        previous_native_ready_observation=prior['native_observations'][-1],
        metadata_only=True,glink_payload_reads=False,glink_writes=False,
        scope='After a sealed experimental ready observation, read and compare only directory allocation metadata for478/479/480 under lock3. Recheck AP/peer identity and ready status before/after. No GLINK descriptor indices or FIFO payload are read or written; no GLINK allocation, reset, notification or IRQ setup occurs.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
