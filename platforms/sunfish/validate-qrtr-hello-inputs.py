#!/usr/bin/env python3
"""Pin native completion and one constant first QRTR HELLO transmission."""
import hashlib,json,struct,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-done-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-done-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='544ba0e1c440d0226d022749f8c8bfb6d7ed15c9e5ec6da4d92c5816da170bff' or prior['native_result']!='RXDONE CONSUMED PASS' or prior['error']!='0x0' or not all(prior[k] for k in ['tx_consumed','rx_tail_store','tx_head_store','buffer_guards','intent_retired']):raise ValueError('missing native completion')
    if [prior[k] for k in ['tx_tail','tx_head','rx_tail','rx_head','rx_available']]!=['0x48','0x48','0x290','0x290',0]:raise ValueError('changed native completion state')
    tree=ROOT/'.cache/sunfish-lineage-kernel'
    traces={}
    for rel,tokens in {
        'drivers/rpmsg/qcom_glink_native.c':['req.msg.cmd = cpu_to_le16(RPM_CMD_TX_DATA);','req.msg.param1 = cpu_to_le16(channel->lcid);','req.msg.param2 = cpu_to_le32(iid);','req.chunk_size = cpu_to_le32(chunk_size);','req.left_size = cpu_to_le32(left_size);','intent->in_use = true;','#define RPM_CMD_RX_DONE_W_REUSE\t\t14'],
        'net/qrtr/qrtr.c':['struct qrtr_hdr_v1 {','hdr->version = cpu_to_le32(QRTR_PROTO_VER_1);','pkt->cmd = cpu_to_le32(QRTR_TYPE_HELLO);','from.sq_node = qrtr_local_nid;','to.sq_node = node->nid;'],
        'net/qrtr/Kconfig':['config QRTR_NODE_ID','default 1'],
        'include/uapi/linux/qrtr.h':['QRTR_TYPE_HELLO\t\t= 2,'],
    }.items():
        path=tree/rel;text=path.read_text()
        for token in tokens:
            if token not in text:raise ValueError('changed source trace: '+token)
        traces[rel]=hashlib.sha256(path.read_bytes()).hexdigest()
    packet=list(struct.pack('<HHIII13I',9,1,0,52,0,1,2,1,0xfffffffe,0,20,0,0xfffffffe,2,0,0,0,0)+bytes(4))
    if len(packet)!=72:raise ValueError('changed fixed packet length')
    proof.update(previous_native_completion=prior,qrtr_hello_source_sha256=traces,
        owned_actual_completed_receive_checkpoint_required=True,copied_report_is_send_authority=False,
        fixed_qrtr_hello_wire=packet,peer_intent_id=0,minimum_peer_capacity=52,
        peer_capacity_from_owned_current_inventory=True,peer_size_controls_allocation=False,
        qrtr_local_node_id=1,qrtr_remote_node_id=0,qrtr_node_id_is_explicit_probe_choice=True,
        hello_tx_offset=72,hello_tx_head_after=144,hello_rx_tail_unchanged=656,
        hello_notifications=1,total_glink_notifications=6,post_auth_smem_write_bytes=176,
        expected_completion_commands=[6,14],expected_completion_remote_cid=2,expected_completion_intent_id=0,
        max_control_rx_bytes=512,control_rx_copies=2,completion_is_consumed=False,
        observed_reuse_grants_reuse=False,peer_intent_reused=False,new_receive_intent=False,
        hello_request_answered=False,hello_receive_tail_written=False,
        rx_done_sent=True,receive_tail_written=True,peer_intent_selected=True,
        outgoing_qrtr_payload=True,intent_words_used_for_transport=True,
        service_query=False,wlan=False,host_dma=False,
        scope='Owned actual completed first receive and retained private buffer/exchange gate one fixed outgoing QRTR HELLO. Recheck actual empty descriptor72/72/656/656, full TX, private buffer/guards, ready and identities. Parse sealed current488-byte inventory, require30 unique IDs0..29 and exactly one intent0 capacity>=52; the native photo did not provide its individual size, so no cached mapping is assumed. Reserve intent0 once before any write; no fallback/reuse/rollback. Stage72 fixed wire bytes at TX72: DATA9/localCID1/peer-intent0, chunk52/left0, QRTR v1 HELLO node1 to0 and control ports,20-byte body command2 with remaining words/padding zero. Verify full TX twice, publish head144 once and notify bit12 once outside lock. Double-copy at most512 new control bytes at RX656, preserve RXtail656 and all released bytes, require exact completion6 or14/remote2/intent0 and TXtail144, full parsed snapshot and final live checks. Observe but do not answer requests or use new intent metadata. No extra RX tail store, receive advertisement, peer-intent reuse, service/WLAN QMI, host DMA, retries or cleanup; retain leases through restart. Peer GLINK completion does not alone establish QRTR application acceptance or service discovery.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
