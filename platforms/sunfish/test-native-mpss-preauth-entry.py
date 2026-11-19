#!/usr/bin/env python3
"""Run the exact combined pre-auth entry over native RAM views and RSC model."""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-preauth-entry'
CASES=['success','duplicate','late-fault','earlier-error','no-receipt','rx-not-ready','domain-not-quiet','cold-lost','wrong-physical','wrong-db','lost-irq-after-power','lost-irq-after-smp','lost-irq-final','aop-timeout','rail-timeout','guard-during-aop']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(args,label,env):
    r=subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True);(OUT/(label+'.log')).write_text(r.stdout+r.stderr)
    if r.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
def main():
    os.umask(0o077);OUT.mkdir(parents=True,exist_ok=True,mode=0o700)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    names=['qcom_bcm','qcom_bcm_vote','qcom_cmd_db','qcom_mdt','qcom_pas_init','qcom_pas_metadata','qcom_rpmh_bus','qcom_rpmh_handoff','qcom_rpmh_read','sunfish_ipa_fws','sunfish_ipa_linux_pas','sunfish_ipa_pas','sunfish_scm_linux_power','qcom_qmp','qcom_qmp_link','sunfish_mpss_linux_power','sunfish_mpss_linux_power_native',
           'qcom_modem_status','sunfish_ipa_gsi_profile','sunfish_ipa_shared_memory','sunfish_ipa_smem','sunfish_ipa_smem_native','sunfish_mpss_linux_smem','sunfish_mpss_linux_smem_native','sunfish_mpss_linux_smp2p','sunfish_mpss_linux_smp2p_native','sunfish_smmu_lpae']
    selected=['rust/src/'+n+'.rs'for n in names]+['rust/src/pixel_mpss_preauth.rs','rust/src/sunfish_native_startup_preflight.rs','rust/src/pixel_smmu_install.rs',
              'tests/sunfish_mpss_preauth_entry_host.rs','tests/sunfish_scm_linux_power_test.rs','tests/sunfish_ipa_shared_fixture.rs','platforms/sunfish/test-native-mpss-preauth-entry.py']
    hashes={p:sha(ROOT/p)for p in selected}
    modules=OUT/'modules.rs';modules.write_text('\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"]pub mod {n};'for n in names))
    base=(ROOT/'tests/sunfish_scm_linux_power_test.rs').read_text()
    fixture=OUT/'rsc-model.rs';fixture.write_text('use qcom_rpmh_handoff::{ReadRegisters,Register as GuardRegister};\nuse qcom_rpmh_read::{BcmResource,Register,RegisterIo};\nuse sunfish_scm_linux_power::NAMES;\n'+base[base.index('fn metadata()'):base.index('#[test]')])
    env=dict(os.environ,SUNFISH_MPSS_PREAUTH_MODULES=str(modules),SUNFISH_MPSS_PREAUTH_RSC_FIXTURE=str(fixture))
    flags=[shutil.which('rustc'),'--edition=2024','-O','-D','warnings','--cfg','feature="sunfish-native-startup-linux-rmtfs"','--cfg','feature="sunfish-native-startup-mpss-smp2p"','--cfg','feature="sunfish-native-startup-mpss-preauth"']
    run(flags+['tests/sunfish_mpss_preauth_entry_host.rs','-o',str(OUT/'entry')],'build',env)
    for case in CASES:run([str(OUT/'entry'),case],case,env)
    for p,h in hashes.items():
        if sha(ROOT/p)!=h:raise ValueError('source changed: '+p)
        dest=OUT/'source'/p;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes((ROOT/p).read_bytes())
    result={'result':'passed','source_sha256':hashes,'cases':CASES,'fresh_process_per_case':True,
            'scope':'Exact root pre-auth orchestration with real power, QMP and SMEM owners; simulated outer factory/health/IRQ/RSC and remote AOP. No physical mappings, IRQ/SMC or modem proof.',
            'relocated_host_virtual_views':True,'physical_identity_and_capability_proven':False,'device_actions':[],'native_mpss_started':False}
    (OUT/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(f'PASS: {len(CASES)} actual pre-auth entry scenarios, retained native QMP/SMEM and modeled outer providers. No device action.')
if __name__=='__main__':main()
