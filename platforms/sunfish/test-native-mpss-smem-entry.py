#!/usr/bin/env python3
"""Execute the actual SMEM boot entry with retained native CPU-memory owners."""
from pathlib import Path
import hashlib,json,os,shutil,subprocess
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-smem-entry'
CASES=['success','rx-not-ready','domain-not-quiet','storage-not-ready','cold-contract-lost','no-writable-pages',
       'pending-irq-before-step','pending-irq-after-final-step','fault-during-word-stores','duplicate','late-fault']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label):
    r=subprocess.run(args,cwd=ROOT,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
    if r.returncode:raise SystemExit(f'{label} failed; see {OUT/(label+".log")}')
def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    paths=['rust/src/pixel_mpss_smem.rs','tests/sunfish_mpss_smem_entry_host.rs','tests/sunfish_ipa_shared_fixture.rs',
           'rust/src/sunfish_ipa_shared_memory.rs','rust/src/sunfish_ipa_smem.rs','rust/src/sunfish_ipa_smem_native.rs',
           'rust/src/sunfish_mpss_linux_smem.rs','rust/src/sunfish_mpss_linux_smem_native.rs',
           'rust/src/qcom_modem_status.rs','rust/src/sunfish_smmu_lpae.rs','rust/src/sunfish_ipa_gsi_profile.rs',
           'platforms/sunfish/test-native-mpss-smem-entry.py']
    paths+=['rust/src/sunfish_mpss_linux_smp2p.rs']
    hashes={p:sha(ROOT/p)for p in paths}
    run([shutil.which('rustc'),'--edition=2024','-D','warnings','--cfg','feature="sunfish-native-startup-linux-rmtfs"',
         'tests/sunfish_mpss_smem_entry_host.rs','-o',str(OUT/'entry')],'build')
    for case in CASES:run([str(OUT/'entry'),case],case)
    for name,digest in hashes.items():
        if sha(ROOT/name)!=digest:raise ValueError('Source changed: '+name)
        p=OUT/'source'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes((ROOT/name).read_bytes())
    result={'result':'passed','scope':'Actual boot entry and real retained native memory composition under host-only display, outer lifecycle and pending-IRQ providers.',
            'cases':CASES,'source_sha256':hashes,'fresh_process_per_case':True,'device_actions':[],
            'native_irq_smc_or_hardware_proven':False}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f'PASS: {len(CASES)} actual boot-entry scenarios with retained native RAM views. No device action.')
if __name__=='__main__':main()
