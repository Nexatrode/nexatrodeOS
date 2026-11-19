#!/usr/bin/env python3
"""Pin native HELLO completion/request and one fresh fixed intent2 receive."""
import json,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-qrtr-hello-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-qrtr-hello-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='2fd686fa7b4d70113ec47f4841b2144bd98f2f63ad943e4eabd6fce2a4c87370' or prior['native_result']!='HELLO TRANSPORT PASS' or prior['error']!='0x0' or not all(prior[k] for k in ['tx_consumed','done_seen','queue_complete','buffer_guards']):raise ValueError('missing native HELLO transport result')
    if [prior[k] for k in ['peer_intent_id','peer_capacity','done_command','rx_available','rx_captured','control_records','intent_requests','request_size']]!=[0,128,14,16,16,2,1,52]:raise ValueError('changed native next receive request')
    if [prior[k] for k in ['tx_tail','tx_head','rx_tail','rx_head']]!=['0x90','0x90','0x290','0x2a0']:raise ValueError('changed native HELLO indices')
    proof.update(previous_native_hello_transport=prior,owned_actual_hello_checkpoint_required=True,
        owned_double_copied_control_bytes_required=True,copied_report_is_receive_authority=False,
        next_receive_packets=[5,0,1,0,1,0,0,0,64,0,0,0,2,0,0,0,8,0,1,0,1,0,0,0],
        next_local_intent_id=2,retired_local_intent1_reused=False,peer_intent0_reused=False,
        next_tx_offset=144,next_tx_head_after=168,next_rx_tail_unchanged=656,
        next_data_rx_offset=672,preserved_pending_control_bytes=16,
        expected_pending_records=[[14,2,0],[7,2,52]],pending_order_not_assumed=True,
        next_buffer_bytes=64,next_guard_bytes=32,private_buffer_count=2,
        buffers_disjoint_and_rw_nx_required=True,both_buffers_held_through_restart=True,
        next_buffer_address_transmitted=False,next_expected_header=[9,2,2],
        next_expected_chunk_bytes=52,next_expected_left_bytes=0,
        next_expected_wire_bytes=72,next_max_wire_read_bytes=80,
        next_notifications=1,total_glink_notifications=7,post_auth_smem_write_bytes=204,
        next_receive_tail_written=False,next_rx_done_sent=False,extra_outgoing_qrtr=False,
        next_packet_header_and_all_five_body_words_diagnostic_only=True,
        next_packet_identity_not_assumed=True,service_request=False,wlan=False,host_dma=False,
        scope='Consume the actual HELLO transport continuation and its stable complete16-byte pending control capture. Require exactly RX_DONE_W_REUSE14/remote2/peer-intent0 and request7/remote2/52, either order, current144/144/656/672 descriptor and full TX. Preserve private first52-byte packet against the retained HELLO-stage copy and both guards/zero remainder. Reserve a fresh disjoint private64-byte buffer with32 guards through restart, never reuse retired local1 or peer0. Append24 fixed scalar advertisement5/local1/count1/size64/localintent2 and positive ACK8/local1/1 at TX144, verify all TX twice, publish head168 once and issue one seventh bit12 notification outside lock. Preserve RXtail656 and the16-byte control prefix. Double-copy at most80 bytes at RX672; accept only first complete72-byte DATA9/remote2/localintent2 chunk52/left0 frame, copy52 to the new buffer, two volatile readbacks and both private buffers/zero remainders/guards plus final ready/state/TX/prefix checks. Later suffix stays unclassified/unconsumed. Decode QRTR header and all five control-body words as observations without assuming packet identity. No further RX_DONE, receive consumption, extra QRTR TX, intent reuse, service/WLAN QMI, host DMA, retry or ambiguous cleanup. Retain both buffer/peer-intent leases to restart.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
