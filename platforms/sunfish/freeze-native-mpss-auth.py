#!/usr/bin/env python3
"""Freeze a completed private MPSS auth/ready image and its source/proof identities.

No device access. A later runner must consume the attempt before fastboot boot;
this tool refuses to overwrite a frozen or consumed experiment directory.
"""
from pathlib import Path
import argparse,datetime,hashlib,json,os,shlex,shutil,subprocess,sys
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/'platforms/sunfish'))
from proof_source_compat import source_matches_compatible
def identity(p):return {'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
def read(p):return json.loads(p.read_text())
def write(p,r):p.write_text(json.dumps(r,indent=2)+'\n')
def maps(value):
    if isinstance(value,dict):
        for key,v in value.items():
            if key in ['source_sha256','native_sha256']and isinstance(v,dict):yield v
            else:yield from maps(v)
    elif isinstance(value,list):
        for v in value:yield from maps(v)
def main():
    a=argparse.ArgumentParser(description=__doc__);a.add_argument('--output',type=Path,required=True);a.add_argument('--source-diagnostic',action='store_true');args=a.parse_args();out=args.output.resolve();os.umask(0o077)
    if out.stat().st_mode&0o077:raise ValueError('private experiment directory required')
    if (out/'manifest.json').exists()or(out/'attempt.json').exists():raise ValueError('experiment already frozen or consumed')
    before=read(out/'precompile-inputs.json')
    if before['before_cargo_log_exists'] or (out/'cargo-build.log').stat().st_birthtime*1e9<before['captured_unix_ns']:raise ValueError('snapshot was not before Cargo')
    if 'Finished `release` profile'not in (out/'cargo-build.log').read_text():raise ValueError('completed Cargo build required')
    capability=read(out/'capability-audit.json');audit=read(out/'mpss-auth-source-audit.json')
    if not capability['mpss_start']or capability['glink_started']or not capability['smp2p_activated'] or capability['qmp_payload_bytes']!=64:raise ValueError('wrong capability profile')
    layout=read(out/'boot-layout-audit.json')
    if not layout['source_placement_checked_before_relocation'] or not layout['full_envelopes_and_copy_targets_avoid_three_dt_reservations']:raise ValueError('missing full boot-layout proof')
    profile=read(out/'loader-sdk-profile.json')
    if args.source_diagnostic:
        source_audit=layout.get('source_guard_diagnostic_audit')
        if not profile.get('native_source_diagnostic') or not source_audit or not source_audit.get('all_paths_stop_before_relocation'):
            raise ValueError('missing source-diagnostic loader proof')
    else:
        source_audit=layout.get('source_guard_diagnostic_audit')
        if (profile.get('native_source_diagnostic')
                or not profile.get('native_source_report_guard')
                or not source_audit
                or not source_audit.get('all_failure_paths_stop_before_relocation')
                or not source_audit.get('pass_path_reaches_relocation')):
            raise ValueError('auth image requires the report-only guard with a passing relocation path')
    for record in [capability,layout]:
        for name,expected in record['input_sha256'].items():
            if identity(Path(name))['sha256']!=expected:raise ValueError('stale image audit: '+name)
    if capability['mutations_rejected']<251 or audit['source_mutations_rejected']!=9:raise ValueError('missing mutations')
    proof_paths={
        'auth-root':'mpss-auth-root/result.json','auth-boot':'mpss-linux-boot/result.json','auth-loader':'mpss-linux-loader/result.json',
        'power':'mpss-linux-power/result.json','smp2p':'mpss-linux-smp2p/result.json','entry':'mpss-preauth-entry/result.json',
        'postinstall':'mpss-postinstall/result.json',
        'extension':'mpss-linux-smem/result.json','smem-entry':'mpss-smem-entry/result.json',
        'storage-server':'linux-rmtfs-server/manifest.json','storage-resources':'linux-rmtfs/manifest.json',
        'power-linux-c':'mpss-power-linux/result.json','smp2p-linux-c':'mpss-smp2p-linux/result.json','smem-linux-c':'mpss-smem-layout/result.json'}
    private=ROOT/'.cache/sunfish-native-wifi/private';proofs={};sources=set();bound_documents={}
    for name,rel in proof_paths.items():
        path=private/rel;r=read(path)
        if r.get('result','passed')!='passed' or r.get('device_actions',[]):raise ValueError('invalid offline proof '+name)
        proofs[name]=dict(path=str(path),**identity(path))
        dest=out/'proofs'/name;dest.mkdir(parents=True,exist_ok=True);shutil.copyfile(path,dest/path.name)
        for field in maps(r):
            for rel,h in field.items():
                p=ROOT/rel
                if p.is_file():
                    if not source_matches_compatible(rel,h):
                        if not rel.startswith('docs/'):raise ValueError('stale proof '+name+': '+rel)
                        saved=path.parent/'source'/rel
                        if not saved.is_file()or identity(saved)['sha256']!=h:raise ValueError('missing original proof-bound document '+name+': '+rel)
                        target_doc=out/'proof-bound-documents'/name/rel;target_doc.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(saved,target_doc)
                        bound_documents[name+'/'+rel]={'path':rel,'snapshot':str(target_doc.relative_to(out)),
                            'sha256':h,'current_sha256':identity(p)['sha256']}
                        continue
                    sources.add(p)
        # Preserve the original C fixtures/object/log evidence without copying
        # mutation executables or unrelated large private-memory test images.
        if name.endswith('linux-c'):
            for p in path.parent.iterdir():
                if p.is_file()and p.suffix in ['.c','.txt','.log','.bin','.json']:shutil.copyfile(p,dest/p.name)
    if len(read(private/proof_paths['entry'])['cases'])!=16:raise ValueError('missing entry cases')
    root_proof=read(private/proof_paths['auth-root'])
    if root_proof['root_groups']!=10 or len(root_proof['mutations_rejected'])!=8:raise ValueError('missing auth root behavioral groups')
    if read(private/proof_paths['auth-boot'])['handoff_groups']!=10:raise ValueError('missing retained auth/quarantine groups')
    for field in maps(audit):
        for rel,h in field.items():
            p=ROOT/rel
            if p.is_file():
                if not source_matches_compatible(rel,h):raise ValueError('stale source audit: '+rel)
                sources.add(p)
    proof_sources=set(sources)
    target=ROOT/'.cache/sunfish-native-wifi/private/preflight-target/aarch64-sel4-microkit-pixel4k/release'
    dependencies=set()
    for depfile in [target/'rust_sunfish_native_startup_preflight.d',target/'libpersonal_os_rust_server.d']:
        first=depfile.read_text().splitlines()[0].split(': ',1)[1]
        dependencies.update(Path(p).resolve()for p in shlex.split(first))
    snapshots={}
    for p in sorted(dependencies):
        ident=identity(p)
        if before['files'].get(str(p))!=ident:raise ValueError('compiler input changed/not captured: '+str(p))
        try:
            rel=p.relative_to(ROOT)
        except ValueError:
            # Cargo may canonicalize a dependency through one of the explicit
            # cache symlinks used by this worktree. Recover its stable lexical
            # path under .cache so the runner can re-check the same input.
            rel=None
            shared_cache=(ROOT.parent.parent/'.cache').resolve() if ROOT.parent.name=='.worktrees' else None
            if shared_cache is None or not p.is_relative_to(shared_cache):
                raise ValueError('compiler input is outside the reviewed shared cache: '+str(p))
            for candidate in (ROOT/'.cache').rglob('*'):
                if not candidate.is_symlink():
                    continue
                resolved=candidate.resolve()
                if resolved.is_relative_to(shared_cache) and (p == resolved or p.is_relative_to(resolved)):
                    mapped=Path('.cache')/candidate.relative_to(ROOT/'.cache')
                    suffix=p.relative_to(resolved) if p != resolved else Path()
                    rel=mapped/suffix
                    break
            if rel is None:
                raise ValueError('compiler input is outside the reviewed checkout/cache: '+str(p))
        snapshots[str(rel)]=ident
        dest=out/'compiler-inputs'/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest)
        if not str(rel).startswith('.cache/'):sources.add(p)
    for p in [ROOT/'rust/Cargo.toml',ROOT/'rust/Cargo.lock',ROOT/'rust/targets/aarch64-sel4-microkit-pixel4k.json',ROOT/'platforms/sunfish/native-startup-mpss-auth.system',ROOT/'platforms/sunfish/build-native-startup-preflight.sh',Path(__file__).resolve()]:sources.add(p)
    # The actual validator chain is part of the reviewed artifact, including
    # the strict inherited frame, alias, IRQ, SMC and reserved-RAM checks.
    sources.update((ROOT/'platforms/sunfish').glob('validate-native-*.py'))
    sources.update(ROOT/'platforms/sunfish'/p for p in [
        'wrap-microkit-image.py','repack-boot-v2.py','audit-capdl-memory.py',
        'prepare-image-loader-sdk.py','snapshot-native-mpss-auth-inputs.py',
        'build-native-mpss-auth-report-guard.py','proof_source_compat.py',
        'run-native-mpss-auth.py','test-native-mpss-auth-runner.py',
        'test-native-source-guard-diagnostic.py'])
    source_hashes={}
    for p in sorted(sources):
        rel=p.relative_to(ROOT);ident=identity(p)
        captured=before['files'].get(str(p))
        if p!=Path(__file__).resolve()and (captured!=ident if captured is not None else p not in proof_sources):raise ValueError('reviewed source changed/not proof-bound: '+str(rel))
        dest=out/'source'/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest);source_hashes[str(rel)]=ident['sha256']
    inputs=ROOT/'.cache/sunfish-native-wifi/private/prepared-boot-inputs-20261025';private_inputs={}
    for p in sorted(inputs.iterdir()):
        if not p.is_file():continue
        ident=identity(p)
        if before['files'].get(str(p.resolve()))!=ident:raise ValueError('private input changed: '+p.name)
        dest=out/'private-inputs'/p.name;dest.parent.mkdir(exist_ok=True);shutil.copyfile(p,dest);private_inputs[p.name]=ident
    previous=read(private/'native-startup-mpss-smem/manifest.json');environment={}
    for name,expected in previous['environment_identities'].items():
        p=Path(name);ident=identity(p)
        if ident!=expected or before['files'].get(name)!=ident:raise ValueError('runtime/DTB/boot environment changed: '+name)
        environment[name]=ident
    for p in [out/'sdk/board/sunfish/debug/elf'/name for name in ['loader.elf','sel4.elf','monitor.elf','initialiser.elf']]+[out/'sdk/bin/microkit',out/'loader-sdk-profile.json']+list((out/'loader-checkpoint-source').rglob('*')):
        if not p.is_file():continue
        ident=identity(p)
        if before['files'].get(str(p))!=ident:raise ValueError('private loader environment changed: '+str(p))
        environment[str(p)]=ident
    versions={}
    for name,cmd in [('image-rustc',['rustup','run','nightly-2026-09-25','rustc','--version','--verbose']),('host-rustc',['rustc','--version','--verbose'])]:
        versions[name]=subprocess.check_output(cmd,cwd=ROOT,text=True).strip()
    write(out/'toolchain-versions.json',versions)
    note=ROOT/'docs/sunfish-native-wifi-startup.md';dest=out/'notes'/note.name;dest.parent.mkdir(exist_ok=True);shutil.copyfile(note,dest)
    artifacts={p.name:identity(p)for p in out.iterdir()if p.is_file()and p.name not in ['manifest.json','attempt.json']}
    image=artifacts['fastboot.img'];elf=identity(target/'rust_sunfish_native_startup_preflight.elf')
    if artifacts['native_startup_preflight.elf']!=elf:raise ValueError('image ELF not the completed Cargo ELF')
    diagnostic_feature='sunfish-native-startup-mpss-auth-source-diagnostic'
    result={'created_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'feature':diagnostic_feature if args.source_diagnostic else 'sunfish-native-startup-mpss-auth',
            'serial':'02281FQC200058','product':'sunfish','requires_full_poweroff_seconds_at_least':10,
            'scope':('Source guard diagnostic only; displays the loader source check and stops before relocation. No runtime MPSS/IPA/WLAN startup occurs.' if args.source_diagnostic else capability['scope']),
            'diagnostic_only':args.source_diagnostic,'source_guard_stop_before_relocation':args.source_diagnostic,
            'compiler_dependencies':snapshots,'source_sha256':source_hashes,
            'private_input_identities':private_inputs,'environment_identities':environment,'proofs':proofs,'artifacts':artifacts,
            'toolchain_versions':versions,
            'proof_bound_documents':bound_documents,'current_notes':{'path':str(note),'snapshot':str(dest.relative_to(out)),**identity(note)},
            'boot_layout_audit_sha256':identity(out/'boot-layout-audit.json')['sha256'],'capability_mutations_rejected':capability['mutations_rejected'],'selected_frames':capability['selected_frames'],
            'source_mutations_rejected':9,'entry_host_cases':16,'auth_root_host_groups':7,'ready_for_cold_device_check':True,
            'attempt_consumed':False,'fastboot_attempted':False,'device_actions':[],
            'physical_rmtfs_assignment_proven':False,'physical_modem_power_proven':False,'physical_smp2p_publication_proven':False,
            'native_mpss_started':False,'native_wifi_proven':False,
            'next_required':('One cold temporary boot of this exact diagnostic image, capture the source-guard headline/value, then return to LineageOS. This image always stops before relocation; MPSS, GLINK, WLAN and network traffic remain unproved.' if args.source_diagnostic else 'One cold temporary boot for PAS4 and delivered Q6 ready, capture settled screen including historical receipts and current errors, then return to LineageOS. Runtime GLINK/WLFW and network traffic remain.')}
    write(out/'manifest.json',result)
    print('FROZEN:',image['bytes'],'bytes, SHA256',image['sha256'],';',len(dependencies),'compiler inputs,',len(sources),'source bindings. No device action.')
if __name__=='__main__':main()
