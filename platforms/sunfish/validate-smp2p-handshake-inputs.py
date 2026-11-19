#!/usr/bin/env python3
"""Pin the sole AP status item/notification and optional-feature contract."""
import hashlib
import importlib.util
import json
import subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
spec=importlib.util.spec_from_file_location('ready',PORT/'validate-modem-ready-inputs.py')
ready=importlib.util.module_from_spec(spec);spec.loader.exec_module(ready)
audit=ready.audit

def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-modem-ready-inputs.py')],text=True))
    for dtb in [audit.DEFAULT_DTB,ROOT/'.cache/pixel-sunfish/live-runtime.dtb']:
        smp='/soc/qcom,smp2p-modem';ipc='/soc/syscon@17c0000c'
        if audit.read_cells(dtb,ipc,'reg')!=[0x17c0000c,4] or audit.read_cells(dtb,smp,'qcom,ipc')!=[audit.read_cells(dtb,ipc,'phandle')[0],0,14]:
            raise ValueError('changed status doorbell route')
        if audit.fdtget(dtb,'-t','s',smp+'/master-kernel','qcom,entry-name')!='master-kernel':
            raise ValueError('changed AP status name')
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/soc/qcom/smp2p.c'
    text=source.read_text()
    for token in ['features = in->features & out->features;',
                  'if (features & SMP2P_FEATURE_SSR_ACK)',
                  'out->version = SMP2P_VERSION;',
                  'out->valid_entries++;']:
        if token not in text:raise ValueError('changed matched status protocol')
    prior=ROOT/'.cache/pixel-sunfish/modem-ready-probe/candidate.json'
    observation=json.loads(prior.read_text())['native_observations'][-1]
    if observation['readout']!={'error':'00000010','item_offset':'0011D030','valid_entries':'00000000','slave_kernel':'FFFFFFFF'}:
        raise ValueError('missing previous native status-header evidence')
    previous=json.loads((ROOT/'.cache/pixel-sunfish/modem-smp2p-probe/candidate.json').read_text())['native_observations'][-1]
    if previous['readout']!={'error':'00000011','ap_published':'00000000','peer_negotiated':'00000000'}:
        raise ValueError('missing previous native allocation guard evidence')
    proof.update(previous_native_bounds_observation=previous,ap_item=428,ap_name='master-kernel',ap_entry_value=0,ap_features=0,
        ssr_ack_implemented=False,doorbell_physical='0x17c0000c',doorbell_mask='0x00004000',
        writable_smem_page='0x8611d000',writable_smem_bytes=4096,
        preauth_requirement='Exact host0:1 partition0x11d000 size0x7c000/cacheline32, freeU32/freeC0x7c000 and no inbound/outbound item',
        publication_requirement='Before AUTH: consume one opaque permit from complete image staging, two successful INFO replies and fresh metadata. Exact empty partition and missing435/428 under lock; allocation wholly inside first page with full readback and version-last publication. Reinspect metadata before AUTH. Notify only after successful permitted AUTH and a new valid remote435 header',
        previous_native_header_observation=observation,protocol_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        scope='One new AP-owned status item before AUTH and one fixed modem doorbell after AUTH and peer header publication. No SSR ACK support advertised, arbitrary AP state updates, retry, GLINK, WLAN QMI, IRQ setup or host DMA. Not a production lifecycle/transport service.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
