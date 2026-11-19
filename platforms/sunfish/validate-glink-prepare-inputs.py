#!/usr/bin/env python3
"""Pin the standalone fresh478/479 allocation plan; never start a transport."""
import hashlib
import json
from pathlib import Path
import subprocess
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-receive-inputs.py')],text=True))
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/soc/qcom/smem.c';text=source.read_text()
    for fragment in ['alloc_size = sizeof(*hdr) + ALIGN(size, 8);','hdr->canary = SMEM_PRIVATE_CANARY;',
                     'hdr->padding_hdr = 0;','le32_add_cpu(&phdr->offset_free_uncached, alloc_size);']:
        if fragment not in text:raise ValueError('changed matched allocator '+fragment)
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-receive-probe/native-observation.json').read_text())
    if prior['native_result']!='RX EMPTY SNAPSHOT PASS' or prior['error']!='0x0' or prior['rx_available']!='0x0':raise ValueError('missing native empty-ring readout')
    proof.update(metadata_only=False,glink_payload_reads=True,glink_writes=True,
        fresh_allocation_ids=[478,479],descriptor_bytes=32,tx_fifo_bytes=16384,
        fresh_descriptor_offset='0x11d198',fresh_tx_offset='0x11d1c8',free_before='0x188',free_after='0x41c8',
        writable_smem_pages=5,writable_smem_base='0x8611d000',writable_smem_end='0x86122000',
        authenticated_modem_admitted=False,auth_session_consumed_without_auth_call=True,
        previous_native_receive_observation=prior,matched_smem_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        scope='Standalone pre-AUTH memory test, not permission to initialize a running transport. Consume the real completed-image/INFO/fresh-metadata session; require exact existing AP-only partition under lock3. Initialize only two fresh allocations478/479, read back three times, commit free cursor once and check4028 untouched bytes. Never reset an existing descriptor, authenticate/start firmware, publish a packet, notify GLINK, bind an IRQ, start WLAN or set up host DMA. No rollback/retry; retain memory/votes until full restart.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
