#!/usr/bin/env python3
"""Pin the complete native copy followed by an unidentified Payload8 stop."""
import json,subprocess
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-intents-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-intents-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='37c94ded69b54776837279fed63c724ef375d215da6984132b3786b4f64b79e6' or prior['native_result']!='INTENT SNAPSHOT STOPPED' or prior['error']!='0x8':raise ValueError('missing original native intent stop')
    if not prior['tx_consumed'] or not prior['captured_all_current_new_bytes'] or prior['new_rx_captured_bytes']!=488 or prior['queue_complete'] or prior['offending_record_identified']:raise ValueError('changed full-copy/unknown-stop evidence')
    proof.update(previous_native_intent_stop=prior,original_failure_record_and_cause_unknown=True,revision=2,write_bounds_and_read_bound_unchanged=True,
        old_15_entry_record_limit_replaced_by_512_byte_derived_63_entry_limit=True,
        scan_detail_codes={'input_bound':1,'unknown_command':2,'channel':3,'count':4,'truncated_record':5,'entry_storage':6,'size_group_storage':7},
        displayed_size_group_prefix=5)
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
