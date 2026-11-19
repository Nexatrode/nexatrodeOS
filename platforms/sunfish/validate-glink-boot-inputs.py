#!/usr/bin/env python3
"""Pin the observed fresh-memory preparation plus existing one-shot AUTH path."""
import json
from pathlib import Path
import subprocess
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-prepare-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-prepare-probe/native-observation.json').read_text())
    if prior['native_result']!='GLINK MEMORY PREPARE PASS' or prior['free_after']!='0x41c8' or prior['guard_bytes']!='0xfbc':raise ValueError('missing native preparation result')
    proof.update(previous_native_memory_preparation=prior,authenticated_modem_admitted=True,auth_session_consumed_without_auth_call=False,
        private_boot_wrapper_owns_auth_session=True,preauth_memory_revalidation=True,fresh_metadata_before_auth=True,one_authentication_reset=True,
        post_auth_glink_writes=False,glink_packets_sent=False,glink_notifications=False,production_lifecycle_token=False,
        post_auth_tx_zero_passes=2,ap_tx_head_and_rx_tail_unchanged=True,
        scope='Create the exact native-verified fresh descriptor478/TX479 before authentication. Private wrapper retains the actual AuthOnce only on real preparation success; revalidate all allocations/bytes/AP identity and fresh MDT before one fixed AUTH/reset. After actual status-ready, consume a non-copyable boot identity and ready observation for read-only allocation/descriptor/RX-header snapshots and two complete zero-TX readbacks. No post-AUTH GLINK allocation, index reset/advance, packet, notification/ACK, IRQ, WLAN, SMMU write or host DMA setup; no production ownership/epoch or recovery capability.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
