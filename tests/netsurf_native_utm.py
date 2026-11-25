#!/usr/bin/env python3
"""Run only the offline NetSurf image in a new, blank accelerated UTM VM."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import time
import uuid
from utm_vault_key_storage import QmpPipe

ROOT = Path(__file__).resolve().parents[1]
CTL = '/Applications/UTM.app/Contents/MacOS/utmctl'


def ctl(*args):
    return subprocess.run([CTL, *map(str, args)], capture_output=True, text=True, timeout=30)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--guest', type=Path, required=True)
    p.add_argument('--template', type=Path, required=True)
    p.add_argument('--leave-running', action='store_true')
    p.add_argument('--boot-only', action='store_true')
    p.add_argument('--javascript', action='store_true', help='expect a JavaScript-enabled browser image')
    p.add_argument('--javascript-only', action='store_true', help='isolate JavaScript tests while developing the port')
    p.add_argument('--reflow-only', action='store_true', help='isolate dynamic DOM layout tests')
    p.add_argument('--dom-work-only', action='store_true', help='isolate bounded native DOM mutation checks')
    p.add_argument('--test-hooks', action='store_true', help='explicitly allow the fault-injection and loopback TLS fixture image')
    p.add_argument('--recovery-only', action='store_true', help='run fault-injection and delayed TLS response regressions')
    p.add_argument('--recovery-group',choices=('all','reflow','network'),default='all',help='isolate a recovery subgroup during development')
    p.add_argument('--images-only', action='store_true', help='run image validation without repeating the separate HTTPS regression suite')
    p.add_argument('--https', action='store_true', help='allow Internet egress only for the isolated HTTPS browser image')
    a = p.parse_args()
    if a.dom_work_only and (not a.javascript or a.recovery_only or a.reflow_only or a.javascript_only or a.images_only or a.boot_only):p.error('--dom-work-only requires --javascript and excludes other selectors')
    if a.test_hooks and (not a.javascript or not a.https):p.error('--test-hooks requires --javascript --https')
    if a.recovery_only and (not a.test_hooks or a.reflow_only or a.javascript_only or a.images_only or a.boot_only):p.error('--recovery-only requires --test-hooks and excludes other selectors')
    if a.recovery_group!='all' and not a.recovery_only:p.error('--recovery-group requires --recovery-only')
    if a.reflow_only and (not a.javascript or a.javascript_only or a.images_only or a.boot_only):p.error('--reflow-only requires --javascript and excludes other selectors')
    if a.javascript_only and (not a.javascript or a.images_only or a.boot_only):p.error('--javascript-only requires --javascript and excludes other test selectors')
    if a.images_only and (not a.https or a.boot_only):p.error('--images-only requires --https and excludes --boot-only')
    guest = a.guest.resolve()
    if not guest.is_relative_to(ROOT / 'build/netsurf-native'):
        p.error('use only this experiment build directory')
    manifest = json.loads((guest / 'build.json').read_text())
    assert all(manifest[x] is False for x in ('storage','saved_data_used'))
    assert manifest['network'] is a.https
    assert manifest['javascript'] is a.javascript
    assert manifest.get('test_hooks',False) is a.test_hooks
    if a.javascript:assert manifest['dom_reflow'] is True
    if a.images_only:assert {'png','jpeg'}.issubset(manifest.get('image_formats',[]))
    if a.https: assert manifest['https'] is True and manifest['certificate_verification'] is True
    assert hashlib.sha256((guest / 'loader.img').read_bytes()).hexdigest() == manifest['loader_sha256']
    for name,digest in manifest['sources'].items():
        assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest()==digest, 'rebuild changed source: '+name
    state = plistlib.loads(subprocess.check_output(['ioreg','-n','Root','-d','1','-a'],timeout=10))
    if isinstance(state,dict):state=[state]
    assert not any(user.get('CGSSessionScreenIsLocked',False) for row in state for user in row.get('IOConsoleUsers',[])), 'unlock the Mac to run UTM'
    tag = str(uuid.uuid4()).upper()
    os.umask(0o077)
    bundle = guest / ('NetSurf-Disposable-' + tag[:8] + '.utm')
    data = bundle / 'Data'
    data.mkdir(parents=True)
    config = plistlib.loads(a.template.read_bytes())
    assert config['System']['CPUCount'] == 4 and config['QEMU']['Hypervisor'] is True
    assert config['Network'] == [] and len(config['Drive']) == 1
    config['Information'].update(UUID=tag,Name='NexatrodeOS NetSurf — Disposable ' + tag[:8])
    config['Serial'] = []
    config['Sharing'].update(ClipboardSharing=False,DirectoryShareMode='None')
    config['QEMU']['DebugLog'] = False
    old = config['Drive'][0]['Identifier']
    drive = str(uuid.uuid4()).upper()
    config['Drive'][0].update(Identifier=drive,ImageName='blank.img')
    options = config['QEMU']['AdditionalArguments']
    allowed = ('-global virtio-', '-device loader,', '-device virtio-', '-netdev user,id=net0')
    assert all(value.startswith(allowed) for value in options)
    assert sum(v.startswith('-device loader,') for v in options) == 1
    assert sum(v.startswith('-device virtio-blk-device,') for v in options) == 1
    assert sum(v.startswith('-netdev user,id=net0') for v in options) == 1
    for i,value in enumerate(options):
        if value.startswith('-device loader,'):
            options[i]=f'-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0'
        elif value.startswith('-device virtio-blk-device,'):
            options[i]=value.replace(old,drive)
        elif value.startswith('-netdev user,id=net0'):
            options[i]='-netdev user,id=net0' + ('' if a.https else ',restrict=on')
    shutil.copyfile(guest / 'loader.img',data / 'loader.img')
    with (data / 'blank.img').open('wb') as disk:
        disk.truncate(16*1024*1024)
    trace=data / 'trace.log'
    monitor_path=data / 'monitor'
    for suffix in ('.in','.out'):
        os.mkfifo(str(monitor_path)+suffix,0o600)
    options.extend(['-serial file:'+str(trace),'-qmp pipe:'+str(monitor_path)])
    (bundle / 'config.plist').write_bytes(plistlib.dumps(config))
    monitor=QmpPipe(monitor_path)
    session={'uuid':tag,'bundle':str(bundle),'trace':str(trace),'monitor':str(monitor_path),
             'loader_sha256':manifest['loader_sha256'],'test_hooks':a.test_hooks,
             'saved_data_used':False,'network':('native guest DNS/TCP/TLS; QEMU user NAT' if a.https else 'restricted; no guest network service'),'blank_disk_bytes':16*1024*1024}
    (guest / 'session.json').write_text(json.dumps(session,indent=2)+'\n')
    registered=started=success=False
    try:
        subprocess.run(['open','-g','-a','UTM',str(bundle)],check=True)
        deadline=time.monotonic()+45
        while time.monotonic()<deadline:
            if any(tag in row and 'stopped' in row for row in ctl('list').stdout.splitlines()):
                registered=True;break
            time.sleep(.25)
        if not registered:raise RuntimeError('new disposable VM failed to register')
        result=ctl('start',tag,'--disposable')
        if result.returncode:raise RuntimeError(result.stderr)
        started=True
        monitor.connect()
        deadline=time.monotonic()+90
        while time.monotonic()<deadline:
            text=trace.read_text(errors='replace') if trace.exists() else ''
            if any(marker in text for marker in ('NETSURF|FAIL|','MON|ERROR','VMFault','Prefetch Abort','Data Abort')):
                raise RuntimeError(text[-5000:])
            if 'NETSURF|READY|' in text:break
            time.sleep(.2)
        else:raise RuntimeError('NetSurf did not finish loading: '+text[-5000:])
        time.sleep(1)
        capture=data / 'initial.ppm'
        monitor.send('screendump',{'filename':str(capture)})
        shutil.copyfile(capture,guest/'initial.ppm')
        subprocess.run(['magick',str(capture),str(guest/'initial.png')],check=True)
        if not a.boot_only:
            if a.dom_work_only:
                from netsurf_native_dom_work_input import exercise as dom_exercise
                dom_exercise(monitor,trace,data,guest,manifest['scale'],faults=a.test_hooks)
            else:
                run_browser_suites(a,monitor,trace,data,guest,manifest)
        (guest/'serial.log').write_bytes(trace.read_bytes())
        success=True
        session['test_passed']=True
        (guest/'session.json').write_text(json.dumps(session,indent=2)+'\n')
        print('PASS native NetSurf in disposable UTM: '+tag,flush=True)
    finally:
        monitor.close()
        if trace.exists():shutil.copyfile(trace,guest/'serial.log')
        if started and not (success and a.leave_running):ctl('stop',tag,'--force')
        if registered and not (success and a.leave_running):
            for _ in range(60):
                if ctl('status',tag).stdout.strip()=='stopped':break
                time.sleep(.2)
            ctl('delete',tag)


def run_browser_suites(a,monitor,trace,data,guest,manifest):
    if a.test_hooks:
        from netsurf_native_recovery_input import exercise as recovery_exercise
        recovery_exercise(monitor,trace,data,guest,manifest['scale'],group=a.recovery_group)
    if a.https:
        from netsurf_native_https_input import exercise
    else:
        from netsurf_native_input import exercise
    if not a.recovery_only and not a.images_only and not a.javascript_only and not a.reflow_only:exercise(monitor,trace,data,guest,manifest['scale'])
    if not a.recovery_only and not a.javascript_only and not a.reflow_only and a.https and {'png','jpeg'}.issubset(manifest.get('image_formats',[])):
        from netsurf_native_image_input import exercise as image_exercise
        image_exercise(monitor,trace,data,guest,manifest['scale'])
    if not a.recovery_only and a.javascript and not a.reflow_only and not a.images_only:
        from netsurf_native_javascript_input import exercise as javascript_exercise
        javascript_exercise(monitor,trace,data,guest,manifest['scale'])
    if not a.recovery_only and a.javascript and not a.javascript_only and not a.images_only:
        from netsurf_native_reflow_input import exercise as reflow_exercise
        reflow_exercise(monitor,trace,data,guest,manifest['scale'])
    if a.javascript and not (a.recovery_only or a.javascript_only or a.images_only or a.reflow_only):
        from netsurf_native_dom_work_input import exercise as dom_exercise
        dom_exercise(monitor,trace,data,guest,manifest['scale'],faults=a.test_hooks)


if __name__=='__main__':main()
