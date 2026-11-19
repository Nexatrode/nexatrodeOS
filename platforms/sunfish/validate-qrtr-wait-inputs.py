#!/usr/bin/env python3
"""Pin native consumed-credit idle result and unchanged-write measured wait."""
import json,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-qrtr-credit-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-qrtr-credit-probe/native-observation.json').read_text())
    assert prior['artifact_sha256']=='d19796b7785210117da8b7c6f1cb67baad74a7fc4d96fe6eee07cd9d25396942'
    assert prior['native_result']=='DISCOVERY STOPPED' and prior['error']=='0x10' and prior['guard_stage']==8
    assert [prior[k]for k in ['records','new_copies','done_consumed','fresh_intents','request_acks']]==[2,1,2,2,1]
    assert [prior[k]for k in ['tx_tail','tx_head','rx_tail','rx_head','rx_available']]==['0xe0','0xe0','0x338','0x338',0]
    assert prior['old_guards'] and prior['pool_guards'] and not prior['elapsed_time_shown']
    proof.update(previous_native_consumed_credit_timeout=prior,max_seconds=65,max_data_wait_seconds=60,max_data_wait_samples=8192,max_wait_samples=512,
        global_budget_discovery_phase_only=True,last_poll_elapsed_ms_diagnostic=True,clock_frequency_diagnostic=True,last_slave_kernel_diagnostic=True,
        exact_elapsed_of_prior_timeout_known=False,additional_packet_or_write_authority=False,reoffer_on_silence=False,timeout_receive_consumption=False,
        scope='Same owned optional-request credit continuation and all prior packet/write/buffer bounds. Extend only DATA observation to60seconds/8192 samples with independent65second global discovery-phase budget and stopped-clock work bounds. Display last-poll elapsed ms, clock frequency, DATA polls and last observed slave-kernel status. Validate retained buffers, full TX, ready/route identities and monotonic indices throughout. A pending credit is never reoffered on silence; timeout does not consume receive bytes, release a lease, retry or clean up. No new packet/write authority, extra QRTR beyond HELLO, service QMI, DMA, intent reuse or global absence claim.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
