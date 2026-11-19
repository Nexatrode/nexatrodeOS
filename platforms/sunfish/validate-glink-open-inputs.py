#!/usr/bin/env python3
"""Pin native VERSION_ACK/IPCRTR offer and one fixed local OPEN request."""
import hashlib,json,re,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-pending-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-pending-probe/native-observation.json').read_text())
    if prior['native_result']!='RX SNAPSHOT PASS' or not all(prior[k] for k in ['tx_consumed','ack_in_snapshot','queue_complete']) or prior['parsed_messages']!=4 or prior['rx_captured_bytes']!=80 or prior['snapshot_error']!='0x0' or prior['parse_error']!='0x0':raise ValueError('missing complete native version/OPEN evidence')
    if [(p['remote_channel_id'],p['name_bytes_including_nul'])for p in prior['opens']]!=[(1,10),(2,7),(3,19)]:raise ValueError('changed native offer shape')
    current_dir=ROOT/'.cache/pixel-sunfish/modem-qrtr-discovery-probe'
    current=json.loads((current_dir/'native-open-order-observation.json').read_text())
    for field,path in [('artifact_sha256',current_dir/current['artifact']),('photo_sha256',current_dir/current['photo'])]:
        if hashlib.sha256(path.read_bytes()).hexdigest()!=current[field]:raise ValueError('changed native OPEN-order evidence: '+field)
    expected_rows=[('0x00',2,1,7),('0x10',2,2,10),('0x28',2,3,19),('0x48',1,1,0)]
    actual_rows=[(p['offset'],p['command'],p['id'],p['name_bytes'])for p in current['observed_offer_rows']]
    if current['native_result']!='IPCRTR REQUEST STOPPED' or current['error']!='0x8' or current['plan_detail']!='0x4' or current['expected_name_bytes']!='0x0A' or current['seen_name_bytes']!='0x07' or actual_rows!=expected_rows or current['observed_offer_rows'][0].get('visible_name')!='IPCRTR' or any(current[k]!=0 for k in ['staged','head_store','notifications','tx_consumed','open_ack_seen','byte_writes']):raise ValueError('missing bounded native OPEN-order evidence')
    sources={}
    for rel in ['drivers/rpmsg/qcom_glink_native.c','net/qrtr/smd.c']:
        path=ROOT/'.cache/sunfish-lineage-kernel'/rel;sources[rel]=hashlib.sha256(path.read_bytes()).hexdigest()
    source=(ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c').read_text()
    for name,value in [('RPM_CMD_OPEN',2),('RPM_CMD_OPEN_ACK',4),('RPM_CMD_INTENT',5),('RPM_GLINK_CID_MIN',1),('GLINK_NAME_SIZE',32)]:
        if not re.search(r'#define\s+'+name+r'\s+'+str(value)+r'\b',source):raise ValueError('changed '+name)
    if 'req.msg.param1 = cpu_to_le16(channel->lcid);' not in source or 'int req_len = ALIGN(sizeof(req.msg) + name_len, 8);' not in source:raise ValueError('changed OPEN layout')
    if '{ "IPCRTR" }' not in (ROOT/'.cache/sunfish-lineage-kernel/net/qrtr/smd.c').read_text():raise ValueError('changed QRTR channel name')
    proof.update(previous_native_complete_rx_snapshot=prior,native_reordered_offer_observation=current,open_sources_sha256=sources,
        requires_owned_version_checkpoint=True,readout_report_cannot_mint_checkpoint=True,
        exact_wire_target_rechecked_on_new_boot='IPCRTR',accepted_offer_orders=[[(1,10),(2,7),(3,19)],[(1,7),(2,10),(3,19)]],target_remote_channel_id_may_be_1_or_2=True,unused_open_names_require_literal_match=False,
        unused_open_shape_and_live_byte_preservation_required=True,implementation_revision=3,
        guard_stage_codes={'plan':1,'initial_live':2,'before_byte_stores':3,'staged_live':4,'before_head':5,'published_live':6,'before_notify':7,'reply':8},
        plan_detail_codes={'queue_length':1,'indices':2,'parser':3,'open_shape':4,'target_name':5,'ack':6},
        plan_shape_diagnostic_fields=['packet','expected_offset','offset','expected_id','id','expected_name_bytes','name_bytes','command'],
        fixed_open_request=[2,0,1,0,7,0,0,0,73,80,67,82,84,82,0,0],open_request_tx_offset=16,open_tx_head_after=32,
        open_ap_rx_tail_unchanged=8,peer_open_ack_sent=False,open_max_notifications=1,total_glink_notifications=2,
        open_reply_max_new_bytes=128,open_reply_commands=[4,5,13,15],open_expected_ack=[4,1,0],
        receive_consumed_in_open_test=False,intent_advertisement=False,qrtr_payload=False,
        scope='Standalone fixed local IPCRTR OPEN request experiment revision3. Actual fresh boot/version exchange owns a non-copyable checkpoint; copied reports cannot upgrade to it. Require complete native-shaped80-byte queue with one exact raw IPCRTR offer (remote ID1 or ID2, including NUL length7) and the other bounded 10-byte offer, plus SSM RTR offer and ACK1/1/0. The first two offers may be in either observed order; do not match the unused offer name. Retain allocations; require zero unused TX and live ready before staging sixteen fixed OPEN bytes. Preserve every live queued byte. Verify all TX, publish head32 once and notify bit12 once outside lock. Keep RX tail8 and original80 bytes unchanged; read bounded new control replies for ACK4/local1/0 and full TX consumption. Report guard stage, plan detail and issued byte stores. No peer OPEN_ACK, receive-index write, intent advertisement, QRTR payload, WLAN/DMA, generic send, retry or cleanup. This is not a completed channel or production lifecycle/epoch authority.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
