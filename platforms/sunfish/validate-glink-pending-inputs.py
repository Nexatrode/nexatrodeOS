#!/usr/bin/env python3
"""Pin the observed peer OPEN and the bounded read-only follow-up scope."""
import json
from pathlib import Path
import subprocess
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-version-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-version-probe/native-observation.json').read_text())
    header=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-version-probe/native-pending-header-observation.json').read_text())
    if prior['error']!='0x8' or not prior['tx_consumed'] or prior['ack_seen']:raise ValueError('missing native fixed-TX consumption')
    if [header[k] for k in ['command','channel_id','name_bytes','rx_available']]!=['0x2','0x1','0xa','0x50']:raise ValueError('changed pending OPEN evidence')
    if prior['artifact_sha256']!=header['artifact_sha256'] or prior['artifact_sha256']!='4cad56b81d2794b296ce70857dfae041adff249deba5d1078b991156104cfb26':raise ValueError('changed native evidence image')
    proof.update(previous_native_version_result=prior,previous_native_pending_open=header,
        pending_rx_max_bytes=128,pending_rx_byte_comparisons=2,pending_rx_readonly=True,
        pending_control_parser_commands=[0,1,2,13],pending_open_name_limit=32,
        pending_rx_tail_advance=False,extra_tx_packets=0,extra_notifications=0,
        scope=proof['scope']+' Then copy up to128 pending RX bytes twice with unchanged descriptor/identities and live ready checks. Parse bounded known control headers and printable OPEN names in the owned snapshot; stop at unknown/truncated input. Report an exact ACK anywhere in the validated prefix without accepting/consuming OPEN or ACK, opening a channel, sending another packet or notifying again. A prefix snapshot cannot establish absence beyond captured bytes.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
