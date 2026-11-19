#!/usr/bin/env python3
"""Native peer ACK and partial intent prefix justify a wider read-only copy."""
import hashlib,json,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-channel-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-channel-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='582ce57d51d79ca6ee0b3da09cee4f58fd6a38b17cbb912641f7e32d91c9b215' or prior['native_result']!='IPCRTR ACK PASS' or prior['error']!='0x0':raise ValueError('missing native peer ACK')
    if not all(prior[k] for k in ['staged','head_store','tx_consumed','peer_open_ack_sent']) or prior['additional_notifications']!=1:raise ValueError('missing peer ACK publication/consumption')
    if [prior[k]for k in ['tx_tail','tx_head','rx_tail','rx_head','rx_available','new_rx_captured','command','param_one','param_two']]!=['0x28','0x28','0x8','0x248','0x240','0x80','0x5','0x2','0x1']:raise ValueError('changed native partial control snapshot')
    if [prior[k]for k in ['control_records','intent_records','intent_entries']]!=[8,8,8]:raise ValueError('changed native prefix counts')
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c';text=source.read_text();start=text.index('static void qcom_glink_handle_intent(');end=text.index('static int qcom_glink_rx_open_ack',start);handler=text[start:end]
    for token in ['__le32 size;','__le32 iid;','sizeof(*msg) + sizeof(struct intent_pair) * count','intent->id = le32_to_cpu(msg->intents[i].iid);','intent->size = le32_to_cpu(msg->intents[i].size);','idr_find(&glink->rcids, cid)']:
        if token not in handler:raise ValueError('changed intent pair/channel layout')
    for token in ['#define RPM_CMD_RX_INTENT_REQ\t\t7','case RPM_CMD_RX_INTENT_REQ:','ret = qcom_glink_rx_defer(glink, 0);','cmd.id = RPM_CMD_RX_INTENT_REQ;','cmd.cid = channel->lcid;','cmd.size = size;']:
        if token not in text:raise ValueError('changed passive receive-intent-request layout')
    proof.update(previous_native_peer_ack=prior,intent_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        new_control_snapshot_max_bytes=512,max_diagnostic_entries=64,max_diagnostic_size_groups=8,
        intent_inventory_revision=2,new_control_commands=[5,7,13,15],
        max_diagnostic_entries_per_record=63,record_count_bound_derived_from_512_byte_copy=True,
        receive_intent_request_is_passive_header_only_diagnostic=True,receive_intent_request_response=False,
        failing_record_offset_header_and_preview_reported=True,partial_inventory_commits_whole_records_only=True,
        intent_size_and_id_are_diagnostics_only=True,intent_selection=False,allocation_from_remote_size=False,
        additional_packet_index_or_notification_writes=False,full_queue_marker_requires_capture_of_all_new_bytes=True,
        scope='Read-only expanded intent inventory revision2 after repeating the same actual owned boot/version/OPEN/reciprocal-ACK path. Same512-byte copy bound,40 TX bytes, heads16/32/40, RX tail8 and3 total GLINK notifications. Double-copy and ready/directory/TX/preserved-prefix checks remain. One authoritative copied-byte scanner classifies bounded INTENT5/remote2 pairs, passive header-only receive-intent request7/remote2, READ_NOTIF13 and SIGNALS15/remote2; no request is answered. Retain only whole-record prefix stats on failure, exact stopping offset/header and up to16 preview bytes. Summarize at most64 entries/eight size groups; sizes/IDs are diagnostics only. No receive advance, remote-size allocation, buffer advertisement, generic send, extra packet/notification, QRTR payload, WLAN/DMA, retries or cleanup. Complete inventory requires all current new bytes to fit and parse. Original Payload8 record/cause remains unknown until new native evidence.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
