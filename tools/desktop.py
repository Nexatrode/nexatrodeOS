#!/usr/bin/env python3
"""One entry point for the current disposable ARM64 three-app desktop.

Build, inspect, run, install in an existing disposable fixture, or check under
load. It never imports a regular VM disk or an installation vault key.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = ROOT / 'platforms/utm-desktop.json'
CTL = '/Applications/UTM.app/Contents/MacOS/utmctl'
sys.path.insert(0, str(ROOT / 'tests'))
sys.path.insert(0, str(ROOT / 'experiments/desktop-apps'))


def run(*args):
    subprocess.run([str(a) for a in args], cwd=ROOT, check=True)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def built(guest):
    guest = guest.resolve()
    if not guest.is_relative_to(ROOT / 'build/desktop-apps'):
        raise ValueError('use a worktree build/desktop-apps directory')
    if not (guest / '.disposable-build').is_file():
        raise ValueError('not a disposable desktop build')
    manifest = json.loads((guest / 'build.json').read_text())
    if (manifest['saved_data_used'] or manifest['development_storage_key']==bool(manifest.get('wrapped_session')) or
            not all(manifest.get(k) for k in ('browser', 'virgl', 'desktop_resize'))):
        raise ValueError('not the current disposable three-app GPU/resize profile')
    if digest(guest / 'loader.img') != manifest['loader_sha256']:
        raise ValueError('loader differs from build manifest')
    if (digest(guest / 'desktop-apps.system') != manifest['system_sha256'] or
            digest(guest / 'runtime.json') != manifest['runtime_sha256']):
        raise ValueError('system or runtime inventory differs from build manifest')
    for path, expected in manifest['sources'].items():
        if digest(ROOT / path) != expected:
            raise ValueError('source changed since build: '+path)
    from runtime_manifest import inventory
    actual = inventory(guest / 'desktop-apps.system', maps=manifest.get('maps',False))
    if actual != json.loads((guest / 'runtime.json').read_text()):
        raise ValueError('runtime inventory differs from the assembled system')
    return guest, manifest, actual


def session(guest, *, installed=True):
    guest, manifest, runtime = built(guest)
    state = json.loads((guest / 'session.json').read_text())
    bundle = Path(state['bundle']).resolve()
    if state['saved_data_used'] or bundle.parent != guest or not bundle.name.startswith('Desktop-Apps-'):
        raise ValueError('not this build directory\'s disposable VM')
    config = plistlib.loads((bundle / 'config.plist').read_bytes())
    if config['Information']['UUID'] != state['uuid'] or len(config['Drive']) != (2 if manifest.get('map_storage') else 1):
        raise ValueError('VM identity or drive count changed')
    if config['Drive'][0].get('ImageName') != 'blank.img':
        raise ValueError('only the fixture blank.img drive is allowed')
    if manifest.get('map_storage') and (not manifest.get('maps') or config['Drive'][1].get('ImageName')!='maps.img'):
        raise ValueError('the extra disposable drive must be the public maps.img')
    # Never follow a disk/image link out into an installation or another VM.
    data = bundle / 'Data'
    if (data/'build-manifest.json').is_file():
        deployed_format=json.loads((data/'build-manifest.json').read_text())
        if any(bool(deployed_format.get(k))!=bool(manifest.get(k)) for k in ('wrapped_session','map_import','map_files')):
            raise ValueError('use a fresh disposable VM for a different vault/key format or map format')
    if not data.resolve().is_relative_to(bundle):
        raise ValueError('disposable VM data directory must stay inside its bundle')
    if any((data / n).is_symlink() for n in ('blank.img', 'loader.img', *(['maps.img'] if manifest.get('map_storage') else []))):
        raise ValueError('disposable drive/loader must not be symbolic links')
    if config['System']['CPUCount'] != 4 or not config['QEMU']['Hypervisor']:
        raise ValueError('expected four-vCPU accelerated UTM')
    if installed:
        deployed = json.loads((data / 'build-manifest.json').read_text())
        if deployed != manifest or state['loader_sha256'] != manifest['loader_sha256'] or digest(data / 'loader.img') != manifest['loader_sha256']:
            raise ValueError('running fixture needs this build installed first')
    return guest, manifest, runtime, state, data


def install(guest,background=False):
    guest, manifest, runtime, state, data = session(guest, installed=False)
    from desktop_gpu_motion import MotionQmp, host_locked
    if host_locked() and not background:
        raise ValueError('unlock the Mac before installing in disposable UTM')
    # Stop only the UUID and bundle verified above; never rebuild or replace its
    # disk. Mark checks stale before changing the loader, including failed boots.
    run(CTL, 'stop', state['uuid'])
    stopped=subprocess.run([CTL,'status',state['uuid']],capture_output=True,text=True,check=True,timeout=30)
    if stopped.stdout.strip()!='stopped':
        raise RuntimeError('UTM did not confirm this disposable VM stopped; loader was not changed')
    state.update(test_passed=False, validation_scope='new build installed; checks pending')
    state.pop('boot_readiness',None)
    (guest / 'session.json').write_text(json.dumps(state, indent=2)+'\n')
    shutil.copyfile(guest / 'loader.img', data / 'loader.img')
    shutil.copyfile(guest / 'build.json', data / 'build-manifest.json')
    shutil.copyfile(guest / 'runtime.json', data / 'runtime.json')
    state.update(loader_sha256=manifest['loader_sha256'], gpu_verify=manifest['gpu_verify'])
    (guest / 'session.json').write_text(json.dumps(state, indent=2)+'\n')
    run(CTL, 'start', *(['--hide'] if background else []),state['uuid'])
    from desktop_boot_ready import gpu_boot_state
    q = MotionQmp(data / 'monitor')
    try:
        end = time.monotonic()+120
        # Defer QMP memory reads until CapDL initialization has yielded.
        while time.monotonic() < end:
            trace = (data / 'trace.log').read_text(errors='replace') if (data / 'trace.log').exists() else ''
            if any(x in trace for x in ('DESKTOP_GPU|FAIL|', 'DISPLAY_AGENT|FAIL|', 'VMFault', 'MON|ERROR', 'panicked at')):
                raise RuntimeError('guest fault during boot: '+trace[-2000:])
            if 'DESKTOP_GPU|READY|' in trace or 'DESKTOP_GPU|frame=' in trace:
                q.connect()
                break
            time.sleep(.2)
        else:
            raise RuntimeError('boot did not reach GPU initialization')
        while time.monotonic() < end:
            status = gpu_boot_state(q, data,manifest.get('wrapped_session',False))
            if status['ready']:
                print('PASS disposable desktop boot:', state['uuid'], flush=True)
                return
            time.sleep(.2)
        raise RuntimeError('desktop frontends or GPU did not become ready')
    finally:
        q.close()


def launch(guest, template, keep_running=False):
    guest, _, _ = built(guest)
    if (guest/'session.json').exists():
        raise ValueError('this build already has a VM session; use install instead of creating another VM')
    command = [sys.executable, ROOT/'tests/desktop_apps_run.py', '--guest', guest,
               '--template', template, '--boot-only']
    if keep_running:
        command.append('--leave-running')
    run(*command)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest='action', required=True)
    b = sub.add_parser('build', help='build the canonical 2x three-app GPU/resize profile')
    for name in ('inputs', 'storage', 'shell-base', 'composer', 'output'):
        b.add_argument('--'+name, type=Path, required=True)
    b.add_argument('--maps',action='store_true',help='opt-in fourth offline Maps window; separate unlocked disposable profile')
    b.add_argument('--map-import',action='store_true',help='fresh local region import/update format; requires --maps --map-storage --locked')
    b.add_argument('--map-storage',action='store_true',help='two public offline regions on a separate disposable Maps disk')
    b.add_argument('--resume', action='store_true', help='rebuild a marked disposable staging directory; no VM/disk changes')
    b.add_argument('--diagnostic', action='store_true', help='GPU readback/pixel checks; slower than the normal image')
    b.add_argument('--locked',action='store_true',help='fresh disposable passphrase-wrapped session; requires the managed desktop composer')
    b.add_argument('--idle-ms',type=int,default=60000)
    for action in ('inspect', 'install', 'check'):
        c = sub.add_parser(action)
        c.add_argument('--guest', type=Path, required=True)
        if action=='install':
            c.add_argument('--background',action='store_true',help='boot-only checks in a hidden disposable VM; no native host input/display check')
        if action == 'check':
            c.add_argument('--label', required=True)
            c.add_argument('--live', action='store_true', help='diagnostic build: fresh relay peer/photo, wallet scan and browser HTTPS')
            c.add_argument('--lock-cycle',action='store_true',help='fresh --locked diagnostic disk: enrollment, live scan/relay recovery, idle lock and reboot')
    r = sub.add_parser('run', help='create a fresh disposable VM/disk from a sanitized public template')
    r.add_argument('--guest', type=Path, required=True)
    r.add_argument('--template', type=Path, required=True)
    r.add_argument('--keep-running', action='store_true')
    a = p.parse_args()
    if a.action == 'build':
        profile = json.loads(PROFILE.read_text())
        command = [sys.executable, ROOT/'tests/desktop_apps_utm.py', *profile['build_flags']]
        for name in ('inputs', 'storage', 'shell_base', 'composer', 'output'):
            command += ['--'+name.replace('_', '-'), getattr(a, name)]
        if a.maps:
            command.append('--maps')
            if a.locked:
                if not a.map_storage:raise ValueError('locked Maps requires --map-storage')
                command.append('--map-private')
        if a.map_storage:
            if not a.maps:raise ValueError('--map-storage requires --maps')
            command.append('--map-storage')
        if a.map_import:
            if not (a.maps and a.map_storage and a.locked):raise ValueError('--map-import requires --maps --map-storage --locked')
            command.append('--map-import')
        if a.resume:
            command.append('--resume')
        if a.diagnostic:
            command.append('--gpu-verify')
        if a.locked:
            command+=['--wrapped-session','--lock-idle-ms',str(a.idle_ms)]
        run(*command)
        built(a.output)
    elif a.action == 'inspect':
        _, manifest, runtime = built(a.guest)
        profile=json.loads(PROFILE.read_text());profile['full_session_lock']=bool(manifest.get('wrapped_session'))
        if manifest.get('maps'):profile['apps']=[*profile['apps'],'maps']
        if profile['full_session_lock']:profile['storage']='disposable-passphrase-wrapped-key'
        print(json.dumps(dict(profile=profile,
            loader_sha256=manifest['loader_sha256'], runtime=runtime), indent=2))
    elif a.action == 'install':
        install(a.guest,a.background)
    elif a.action == 'run':
        launch(a.guest,a.template,a.keep_running)
    else:
        guest, manifest, runtime, state, data = session(a.guest)
        if not a.label.replace('-', '').isalnum():
            raise ValueError('use a short alphanumeric/hyphen test label')
        state.update(test_passed=False,validation_scope='desktop checks in progress')
        (guest / 'session.json').write_text(json.dumps(state, indent=2)+'\n')
        if manifest.get('maps'):
            if a.live or a.lock_cycle:raise ValueError('the Maps fixture uses its own four-app checks; live relay/lock-cycle checks remain three-app only')
            from desktop_gpu_motion import MotionQmp,host_locked
            if host_locked():raise ValueError('unlock the Mac for four-app input checks')
            from desktop_maps_input import exercise
            q=MotionQmp(data/'monitor')
            try:
                q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
                try:q.send('query-status')
                except RuntimeError as error:
                    if 'Expecting capabilities negotiation' not in str(error):raise
                    q.send('qmp_capabilities')
                exercise(q,data,guest/('maps-'+a.label),gpu=manifest['gpu_verify'],functional=manifest['gpu_verify'],load=True)
            finally:q.close()
            scope='disposable four-app Maps input/GPU-pixel checks and bounded map profiling' if manifest['gpu_verify'] else 'disposable four-app normal GPU map profiling; no readback or lock guarantee'
        elif a.live or a.lock_cycle:
            if not manifest['gpu_verify']:
                raise ValueError('live app pixel checks require a --diagnostic build')
            if a.lock_cycle and not manifest.get('wrapped_session'):
                raise ValueError('lock-cycle requires the fresh passphrase-wrapped profile')
            from desktop_gpu_motion import MotionQmp, host_locked
            if host_locked():
                raise ValueError('unlock the Mac for live input checks')
            evidence=guest/('live-'+a.label)
            evidence.mkdir()
            q=MotionQmp(data/'monitor')
            try:
                q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
                q.send('query-status')
                from desktop_boot_ready import gpu_boot_state
                lifecycle=None
                if a.lock_cycle:
                    from desktop_trio_lock_input import Lifecycle
                    lifecycle=Lifecycle(q,data,evidence)
                    lifecycle.enroll()
                boot=gpu_boot_state(q,data)
                if lifecycle:
                    lifecycle.until(lambda:gpu_boot_state(q,data)['ready'],awake=True)
                    boot=gpu_boot_state(q,data)
                if not boot['ready']:
                    raise ValueError('desktop frontends are not ready')
                viewport=tuple(boot['gpu'][9:11])
                from desktop_apps_input import exercise
                exercise(q,data/'trace.log',data,evidence,browser=True,gpu=True,viewport=viewport,lifecycle=lifecycle)
            finally:
                try:
                    if q.input is not None:
                        q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
                except (OSError,RuntimeError,TimeoutError):
                    pass
                q.close()
            scope=('disposable three-app wrapped-key enrollment, manual/idle lock, live scan/relay/photo recovery and locked reboot' if a.lock_cycle else
                   'disposable live relay/photo, wallet scan and browser HTTPS; no session-lock guarantee')
        else:
            run(sys.executable, ROOT/'tests/desktop_damage_load.py', '--guest', guest, '--label', a.label)
            scope='disposable damage/focus/held drag under six CPU workers; no session-lock guarantee'
        state.update(test_passed=True, validation_scope=scope)
        (guest / 'session.json').write_text(json.dumps(state, indent=2)+'\n')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as e:
        raise SystemExit(str(e))
