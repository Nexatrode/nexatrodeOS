#!/usr/bin/env python3
"""Pin the native first receive and one non-reusable RX_DONE continuation."""
import hashlib,json,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-rx-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-rx-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='8c96b254ff6451d3b6d69b1b8c159ba3faec77031e8dcd976eefdd0624998702' or prior['native_result']!='RECEIVE COPY PASS' or prior['error']!='0x0' or not prior['tx_consumed'] or not prior['buffer_guards']:raise ValueError('missing native guarded receive')
    if [prior[k] for k in ['tx_head','rx_head','rx_available']]!=['0x40','0x290','0x288']:raise ValueError('changed receive indices')
    if [prior[k] for k in ['data_command','remote_cid','local_intent','chunk_bytes','left_bytes','copied_bytes','rx_tail_unchanged']]!=[9,2,1,52,0,52,8]:raise ValueError('changed receive shape')
    if prior['qrtr_words']!=[1,2,0,0xfffffffe,0,20,0xffffffff,0xfffffffe] or prior['body_word0']!=2:raise ValueError('changed displayed QRTR words')
    source=ROOT/'.cache/sunfish-lineage-kernel/drivers/rpmsg/qcom_glink_native.c';text=source.read_text()
    for token in ['#define RPM_CMD_RX_DONE\t\t\t6','unsigned int cid = channel->lcid;','unsigned int iid = intent->id;','cmd.id = reuse ? RPM_CMD_RX_DONE_W_REUSE : RPM_CMD_RX_DONE;','cmd.lcid = cid;','cmd.liid = iid;','idr_remove(&channel->liids, intent->id);','qcom_glink_rx_advance(glink, ALIGN(sizeof(hdr) + chunk_size, 8));']:
        if token not in text:raise ValueError('changed local source completion trace: '+token)
    proof.update(previous_native_guarded_receive=prior,completion_source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
        owned_actual_receive_checkpoint_required=True,copied_receive_report_cannot_mint_completion=True,
        fixed_done_bytes=[6,0,1,0,1,0,0,0],done_tx_offset=64,done_tx_head_after=72,
        done_rx_tail_before=8,done_rx_tail_after=656,consumed_verified_bytes=648,
        done_notifications=1,total_glink_notifications=5,post_auth_smem_write_bytes=100,
        reuse=False,new_intent=False,private_buffer_retained=True,consumed_peer_bytes_reread=False,
        later_rx_bytes_consumed=False,outgoing_qrtr_payload=False,wlan=False,host_dma=False,
        scope='Consume the unique successful actual first-receive continuation, never a copied report. Pin current64/64/8/656 descriptor and648 complete validated RX bytes. Double-copy/recheck DATA9/remote2/intent1,52 bytes/left0, prior576 bytes, all TX, guards/zero remainder/private payload and ready/identities before any cursor write. Stage8 fixed non-reusable RX_DONE6/localCID1/intent1 bytes at TX64, verify full TX twice, then one tail656 and head72 store with intermediate readiness/state checks, one fifth bit12 notification outside lock. After tail publication never reread the consumed bytes; use retained private payload for checks. Wait read-only for TXtail72, preserve later RX suffix, retain static private buffer through restart. No new/reused intent, outgoing QRTR, peer-intent selection, WLAN/DMA, retry or cleanup. TX consumption alone does not prove peer internal retirement semantics.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
