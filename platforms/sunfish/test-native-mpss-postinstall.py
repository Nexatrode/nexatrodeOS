#!/usr/bin/env python3
"""Reproduce the real post-install handoff with RAM-backed native adapters.

This uses Native.install_experiment, including its permanently consumed write
path, followed by the real SMEM/SMP2P owners. No device, SMC or physical DMA.
"""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'.cache/sunfish-native-wifi/private/mpss-postinstall'
NATIVE=ROOT/'rust/src/sunfish_smmu_native.rs'
HARNESS=ROOT/'tests/sunfish_smmu_native_test.rs'
FLAGS=['--edition=2024','-D','warnings','--test',
       '--cfg','feature="sunfish-smmu-install-probe"',
       '--cfg','feature="sunfish-native-startup-linux-rmtfs"',
       '--cfg','feature="sunfish-native-startup-mpss-smp2p"']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(cmd,label,success=True):
    r=subprocess.run(cmd,cwd=ROOT,capture_output=True,text=True)
    (OUT/(label+'.log')).write_text(r.stdout+r.stderr)
    if success and r.returncode:raise SystemExit(f'{label} failed: {OUT/(label+".log")}')
    return r
def links(p):
    for name in re.findall(r'#\[path\s*=\s*"([^"]+)"\]|include!\("([^"]+)"\)',p.read_text()):
        yield (p.parent/(name[0]or name[1])).resolve()
def absolute_paths(text,parent):
    for p in links(parent):
        for spelling in re.findall(r'"([^"]+\.rs)"',text):
            if (parent.parent/spelling).resolve()==p:text=text.replace('"'+spelling+'"',json.dumps(str(p)))
    return text
def main():
    os.umask(0o077);OUT.mkdir(mode=0o700,parents=True,exist_ok=True)
    if OUT.stat().st_mode&0o077:raise ValueError('private output required')
    inputs={HARNESS,Path(__file__).resolve()};pending=[HARNESS]
    while pending:
        for p in links(pending.pop()):
            if p not in inputs:inputs.add(p);pending.append(p)
    hashes={str(p.relative_to(ROOT)):sha(p)for p in sorted(inputs)}
    rustc=shutil.which('rustc');binary=OUT/'test'
    run([rustc,*FLAGS,str(HARNESS),'-o',str(binary)],'build')
    result=run([str(binary)],'tests')
    total=int(re.search(r'test result: ok\. (\d+) passed',result.stdout)[1])
    count=len(re.findall(r'test .*mpss_postinstall::.* \.\.\. ok',result.stdout))
    if count!=4:raise ValueError('all four post-install groups required')
    source=NATIVE.read_text();start=source.index('    fn with_retained_modem<T>(')
    end=source.index('    pub fn attach_modem_smem(',start)
    body=source[start:end]
    mutations={
        'old-consumed-install-check':body.replace('self.retained_receive_domain()?;','self.check()?;'),
        'omit-before-domain-check':body.replace('self.retained_receive_domain()?;','',1),
        'ignore-terminal-quarantine':body.replace('if self.modem_quarantined {','if false {'),
        'reopen-install-writes':body.replace('self.modem_quarantined = false;','self.modem_quarantined = false; self.failed = false;'),
    }
    # Remove only the last of the two independent retained-domain checks.
    at=body.rindex('self.retained_receive_domain()?;')
    mutations['omit-after-domain-check']=body[:at]+body[at:].replace('self.retained_receive_domain()?;','',1)
    rejected=[]
    for label,changed in mutations.items():
        if changed==body:raise ValueError('mutation missing '+label)
        folder=OUT/label;folder.mkdir(exist_ok=True)
        mutant=absolute_paths(source[:start]+changed+source[end:],NATIVE)
        (folder/'native.rs').write_text(mutant)
        harness=absolute_paths(HARNESS.read_text(),HARNESS).replace(json.dumps(str(NATIVE)),json.dumps(str(folder/'native.rs')))
        (folder/'test.rs').write_text(harness)
        run([rustc,*FLAGS,str(folder/'test.rs'),'-o',str(folder/'test')],label+'-build')
        if run([str(folder/'test'),'mpss_postinstall::'],label,False).returncode==0:raise ValueError('accepted '+label)
        rejected.append(label)
    for rel,h in hashes.items():
        p=ROOT/rel
        if sha(p)!=h:raise ValueError('input changed '+rel)
        dest=OUT/'source'/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(p.read_bytes())
    report={'result':'passed','source_sha256':hashes,'postinstall_groups':count,'total_native_groups':total,
        'mutations_rejected':rejected,'scope':'RAM-backed actual Native install closure, receive handoff, SMEM extension and SMP2P publication; failure, fault, changed-table and unwind quarantine.',
        'installation_write_path_remains_consumed':True,'device_actions':[],
        'native_mpss_started':False,'physical_handoff_proven':False,'native_wifi_proven':False}
    (OUT/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    print(f'PASS: {count} post-install groups, {total} total native groups, {len(rejected)} rejected handoff mutations. No device action.')
if __name__=='__main__':main()
