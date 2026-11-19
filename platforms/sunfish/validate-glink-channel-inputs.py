#!/usr/bin/env python3
"""Pin the native successful OPEN leg and the fixed reciprocal peer ACK."""
import hashlib,json,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-open-v2-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-open-v2-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='d4765751956223ef64896ecf4913a83c50b550de8cb425558565c501c8e8ccfd' or prior['native_result']!='IPCRTR REQUEST PASS' or prior['error']!='0x0':raise ValueError('missing native successful request')
    if not all(prior[k] for k in ['staged','head_store','tx_consumed','open_ack_observed']) or prior['additional_notifications']!=1:raise ValueError('missing request publication/consumption/ACK')
    if [prior[k] for k in ['tx_tail','tx_head','rx_tail','rx_head','new_rx_bytes','command','param_one','param_two']]!=['0x20','0x20','0x8','0x60','0x8','0x4','0x1','0x0']:raise ValueError('changed native request/reply shape')
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c';text=source.read_text()
    start=text.index('static void qcom_glink_send_open_ack(');end=text.index('static void qcom_glink_handle_intent_req_ack',start);ack=text[start:end]
    for token in ['msg.cmd = cpu_to_le16(RPM_CMD_OPEN_ACK);','msg.param1 = cpu_to_le16(channel->rcid);','msg.param2 = cpu_to_le32(0);','qcom_glink_tx(glink, &msg, sizeof(msg), NULL, 0, true);']:
        if token not in ack:raise ValueError('changed reciprocal OPEN_ACK layout')
    start=text.index('static struct glink_channel *qcom_glink_create_local(');end=text.index('/* Remote initiated rpmsg_create_ept */',start);local=text[start:end]
    if not local.index('wait_for_completion_timeout(&channel->open_ack')<local.index('wait_for_completion_timeout(&channel->open_req')<local.index('qcom_glink_send_open_ack(glink, channel);'):raise ValueError('changed local handshake sequence')
    proof.update(previous_native_ipcrtr_request=prior,channel_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        owned_actual_open_checkpoint_required=True,copied_report_cannot_mint_checkpoint=True,
        fixed_peer_open_ack=[4,0,2,0,0,0,0,0],ack_tx_offset=32,ack_tx_head_after=40,ack_ap_rx_tail_unchanged=8,
        ack_max_notifications=1,total_glink_notifications=3,retained_unread_prefix_bytes=88,
        new_control_snapshot_max_bytes=128,new_control_commands=[5,13,15],intent_words_used=False,
        receive_consumed=False,intent_advertisement=False,qrtr_payload=False,
        scope='Fixed reciprocal IPCRTR OPEN_ACK after the actual local-request continuation, not a production channel capability. Recheck exact native32/32/8/96 descriptor state, initial queue, exact target, local ACK and all TX before eight stores. Verify staged bytes, ready/identities and indices before one head40 store and one notification. Read bounded control prefix twice, never use intent words or consume RX. Stop on ambiguity; no retry/cleanup, QRTR payload, WLAN or host DMA. ACK consumption alone does not prove receipt of any intent.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
