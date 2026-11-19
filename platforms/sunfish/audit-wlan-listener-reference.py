#!/usr/bin/env python3
"""Check pinned Linux listener/ACK setup separately from the recovery RPC."""
from pathlib import Path
import argparse,hashlib,json,subprocess
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
refs=[('.cache/sunfish-linux-mainline-ref','fd78d17954a86ea6ceb0c535e618ea129e09f8dd','drivers/soc/qcom/pdr_interface.c'),('.cache/sunfish-lineage-kernel','28f9290ae067d3e7857126eb6312720dbe9c7c80','drivers/soc/qcom/icnss.c'),('.cache/sunfish-lineage-kernel','28f9290ae067d3e7857126eb6312720dbe9c7c80','drivers/soc/qcom/service-notifier.c')]
sources=[]
for repo,commit,path in refs:
 data=subprocess.check_output(['git','-C',str(ROOT/repo),'show',commit+':'+path])
 sources.append(dict(repo=repo,commit=commit,path=path,sha256=hashlib.sha256(data).hexdigest(),text=data.decode()))
def section(s,start,end):
 a=s.index(start);return s[a:s.index(end,a)]
linux,android,notifier=[s['text'] for s in sources]
register=section(linux,'static int pdr_register_listener(','static void pdr_notifier_work(')
assert 'req.enable = enable;' in register and 'SERVREG_REGISTER_LISTENER_REQ' in register and 'pds->state = resp.curr_state;' in register
work=section(linux,'static void pdr_notifier_work(','static int pdr_notifier_new_server(')
assert 'pdr_register_listener(pdr, pds, true)' in work and 'pdr->status(pds->state' in work and 'pdr_restart_pd(' not in work
ack=section(linux,'static int pdr_send_indack_msg(','static void pdr_indack_work(')
assert 'req.transaction_id = tid;' in ack and 'SERVREG_SET_ACK_REQ' in ack and 'qmi_txn_cancel(&txn);' in ack and 'Skip waiting for response' in ack
setup=section(android,'static int icnss_get_service_location_notify(','int __icnss_register_driver(')
assert 'service_notif_register_notifier(' in setup and 'get_service_location(' in setup and 'service_notif_pd_restart(' not in setup
recovery=section(android,'int icnss_trigger_recovery(','EXPORT_SYMBOL(icnss_trigger_recovery);')
assert 'ICNSS_PDR_REGISTERED' in recovery and 'service_notif_pd_restart(' in recovery
downstream_ack=section(notifier,'static void send_ind_ack(', 'static void root_service_service_ind_cb(')
assert 'qmi_txn_wait(' in downstream_ack and 'req.transaction_id = ind_info->transaction_id;' in downstream_ack and 'resp.resp.result' in downstream_ack
for source in sources:source.pop('text')
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(json.dumps(dict(result='passed',sources=sources,findings=['Listener registration reports current state during setup','State indications use matching transaction acknowledgements; mainline skips response wait, Lineage waits and checks the response','ICNSS restart call is in recovery; this does not establish a cold-start trigger'],device_actions=[]),indent=2)+'\n')
print('PASS: pinned listener/ACK sequence; recovery restart is not a proven cold-start trigger.')
