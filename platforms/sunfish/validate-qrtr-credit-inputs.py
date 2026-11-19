#!/usr/bin/env python3
"""Pin native empty-ring timeout and bounded fresh-credit continuation."""
import json,subprocess,hashlib
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-qrtr-discovery-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-qrtr-discovery-probe/native-observation.json').read_text())
    assert prior['artifact_sha256']=='98807d5ecefc8266e427a4cc7d4fd31c346f6a724102d0defa4cdbf1a0d1b37a'
    assert prior['native_result']=='DISCOVERY STOPPED' and prior['error']=='0x10' and prior['guard_stage']==7
    assert (prior['records'],prior['new_copies'],prior['done_consumed'],prior['fresh_intents'],prior['notifications'])==(2,1,2,1,3)
    assert all(prior[k]for k in ['old_guards','pool_guards']) and not prior['wlfw_seen']
    assert [prior[k]for k in ['tx_tail','tx_head','rx_tail','rx_head','rx_available']]==['0xd0','0xd0','0x338','0x338',0]
    kernel=ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c';text=kernel.read_text()
    assert 'static int qcom_glink_announce_create' in text and 'qcom_glink_advertise_intent(glink, channel, intent)' in text
    proof.update(previous_native_discovery_timeout=prior,source_trace_glink_native_sha256=hashlib.sha256(kernel.read_bytes()).hexdigest(),
        mandatory_per_packet_request=False,stable_empty_rx_allows_fresh_intent=True,empty_rx_credit_packet_bytes=16,empty_rx_positive_ack=False,empty_rx_tail_store=False,
        present_request_requires_exact_shape=[7,2,52],present_request_credit_and_ack_bytes=24,present_request_consumed_bytes=8,
        unknown_or_late_request_fails_closed=True,internal_closed_write_plans=True,write_addresses_from_wire=False,final_packet_uncompleted=True,
        request_must_be_only8_pending_bytes=True,request_or_empty_required=True,
        scope='Own actual successful next-receive checkpoint and32 disjoint private guarded buffers through restart. Preserve all legacy bounds and validation; replace mandatory per-packet request wait with stable RX-empty or exact sole request7/remote2/52 gate. An empty ring permits fresh INTENT16 only and no RX tail store or ACK. Present validated request permits INTENT+ACK24 and advances RX exactly8. Closed internal plans use verified AP cursors and fixed unique IDs3..34, never packet-supplied addresses. Stop before credit publication on changed, late or malformed control. Complete only verified DATA frames, no released-byte reread or cleanup. Up to32 new packets/33 records,20seconds, TX1192/RX3312,64 additional notifications and1536 additional SMEM write bytes maxima. Target service0x45 is announcement evidence only. No version/endpoint authority, intent reuse, extra QRTR beyond prerequisite HELLO, service/WLAN QMI, DMA or retry.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
