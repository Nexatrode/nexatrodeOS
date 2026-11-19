#!/usr/bin/env python3
"""Record the un-published native stop and the limited revision2 guard fix."""
import json
from pathlib import Path
import subprocess
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-glink-open-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-glink-open-probe/native-observation.json').read_text())
    if prior['artifact_sha256']!='372e67f0bf0b69152c71b68101a2f36dc3cdf0d58070c1d50c2b40eb443237c2' or prior['error']!='0x8' or prior['head_store'] or prior['additional_notifications']!=0 or prior['publication_stage_pending']:raise ValueError('missing original stopped/native stage evidence')
    proof.update(previous_native_open_stop=prior,revision=2,
        first_stop_cause_not_independently_identified=True,
        exact_target_name_required=True,unused_literal_name_checks_removed=True,
        byte_writer_indices_and_packet_bounds_unchanged=True,
        issued_byte_store_count_reported=True,guard_stage_and_plan_detail_reported=True)
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
