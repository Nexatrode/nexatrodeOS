#!/usr/bin/env python3
"""Pin native announcement evidence and a bounded fresh-intent service inventory."""
import json,subprocess,re,hashlib,os
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-qrtr-next-inputs.py')],text=True))
    next_out=Path(os.environ.get('SUNFISH_MODEM_QRTR_NEXT_INPUT_DIR',ROOT/'.cache/pixel-sunfish/modem-qrtr-next-probe'))
    prior=json.loads((next_out/'native-observation.json').read_text())
    artifact=next_out/'fastboot.img'
    assert prior['artifact_sha256']==hashlib.sha256(artifact.read_bytes()).hexdigest()
    assert prior['photo_sha256']==hashlib.sha256(Path(prior['photo_path']).read_bytes()).hexdigest()
    assert prior['native_result']=='NEXT PACKET COPY PASS' and prior['error']=='0x0'
    assert all(prior[k]for k in ['tx_consumed','buffer_guards','first_guards','qrtr_v1_shape'])
    assert len(prior['body_words'])==5 and prior['body_words'][0]==4
    assert all(isinstance(word,int) and 0<=word<=0xffffffff for word in prior['body_words'])
    assert (prior['rx_head'],prior['rx_available'],prior['rx_tail_unchanged'])==('0x2f0',96,656)
    kernel=ROOT/'.cache/sunfish-lineage-kernel'
    qrtr=(kernel/'include/uapi/linux/qrtr.h').read_text();wlfw=(kernel/'drivers/soc/qcom/wlan_firmware_service_v01.h').read_text()
    assert re.search(r'QRTR_TYPE_NEW_SERVER\s*= 4,',qrtr)
    assert '#define WLFW_SERVICE_ID_V01 0x45' in wlfw and '#define WLFW_SERVICE_VERS_V01 0x01' in wlfw
    proof.update(previous_native_next_receive=prior,owned_actual_next_checkpoint_required=True,owned_double_copied72_byte_frame_required=True,copied_report_is_discovery_authority=False,
        current_suffix_bytes=8,current_suffix_classified=False,request_validated_at_runtime=[7,2,52],request_must_be_only8_pending_bytes=True,
        max_additional_packets=32,max_inventory_records=33,fresh_local_intent_ids=list(range(3,35)),private_pool_buffers=32,private_pool_bytes=3072,all34_buffers_disjoint_rw_nx=True,
        max_tx_head=1192,max_rx_head=3312,max_additional_notifications=64,max_total_glink_notifications=71,max_additional_smem_write_bytes=1536,max_post_auth_smem_write_bytes=1740,
        fixed_done_header=[6,1],fixed_advertisement_header=[5,1,1],fixed_buffer_capacity=64,fixed_positive_ack=[8,1,1],fixed_data_shape=[9,2,52,0],
        target_service_id=0x45,target_match_service_id_only=True,compatible_version_established=False,service_endpoint_authority=False,
        source_trace_commit='28f9290ae067d3e7857126eb6312720dbe9c7c80',source_trace_running_binary_reproducibility_proven=False,qrtr_new_server_command=4,wlfw_source_version=1,
        max_seconds=20,max_wait_samples=512,stopped_clock_work_bounded=True,retained_payloads_and_guards_revalidated=True,final_data_uncompleted=True,released_rx_reread=False,peer_or_local_intent_reuse=False,
        extra_outgoing_qrtr=False,service_qmi=False,wlan_qmi=False,host_dma=False,automatic_retry=False,ambiguous_cleanup=False,
        scope='Consume owned successful next-packet checkpoint and stable wire frame. Parse strict QRTR v1 NEW_SERVER control messages only; preserve source instance values as opaque diagnostics. Complete only validated frames, consume only validated spans, validate each pending8-byte request7/remote2/52 before fresh INTENT+ACK. Reserve distinct private64-byte buffers with32 guards and IDs3..34 through restart. Double copy each DATA72 and verify copied52 bytes twice plus retained buffers and full TX/live readiness/identities/cursors around every publication. Stop at service ID0x45 or32 new packets; bound total time20seconds and no wrap past TX1192/RX3312. Final data remains unconsumed, no reuse, extra QRTR TX, service/WLAN QMI, DMA, retry or cleanup. A service-ID announcement and a bounded partial inventory do not confer endpoint authority or establish version compatibility.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
