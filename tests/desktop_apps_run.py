#!/usr/bin/env python3
"""Boot only a new desktop-apps build with a blank GPT disk and fresh relay peer."""
import argparse, hashlib, json, os
from pathlib import Path
import plistlib, shutil, subprocess, time, uuid
from utm_vault_key_storage import QmpPipe
from desktop_gpu_capture import capture
from desktop_display_config import configure_display
from desktop_boot_ready import gpu_boot_state
ROOT=Path(__file__).resolve().parents[1]
CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
def ctl(*args):return subprocess.run([CTL,*map(str,args)],capture_output=True,text=True,timeout=30)
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--template',type=Path,required=True);p.add_argument('--boot-only',action='store_true');p.add_argument('--leave-running',action='store_true');p.add_argument('--gpu-navigation',action='store_true');p.add_argument('--boot-hidden',action='store_true',help='prepare a boot-only disposable VM in the background; interactive checks still need an unlocked Mac');p.add_argument('--native-input-probe',action='store_true',help='display-only agent mouse bypass and host event trace in this disposable VM');p.add_argument('--gpu-command-trace',action='store_true',help='timestamped GPU command/fence diagnostics in this disposable VM; never input events');a=p.parse_args()
    guest=a.guest.resolve();assert guest.is_relative_to(ROOT/'build/desktop-apps')
    m=json.loads((guest/'build.json').read_text());assert m['saved_data_used'] is False and m['development_storage_key'] is True
    assert hashlib.sha256((guest/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']
    for path,h in m['sources'].items():assert hashlib.sha256((ROOT/path).read_bytes()).hexdigest()==h,path
    if a.boot_hidden:assert a.boot_only,'hidden boot does not run interactive checks'
    state=plistlib.loads(subprocess.check_output(['ioreg','-n','Root','-d','1','-a'],timeout=10));state=[state] if isinstance(state,dict) else state
    if not a.boot_hidden:
        assert not any(u.get('CGSSessionScreenIsLocked',False) for row in state for u in row.get('IOConsoleUsers',[])),'Mac must be unlocked'
    os.umask(0o077);tag=str(uuid.uuid4()).upper();bundle=guest/('Desktop-Apps-'+tag[:8]+'.utm');data=bundle/'Data';data.mkdir(parents=True)
    cfg=plistlib.loads(a.template.read_bytes());assert cfg['System']['CPUCount']==4 and cfg['QEMU']['Hypervisor'] is True
    assert cfg['Network']==[] and len(cfg['Drive'])==1
    cfg['Information'].update(UUID=tag,Name=('NexatrodeOS Desktop Trio — Disposable ' if m.get('browser') else 'NexatrodeOS Apps — Disposable ')+tag[:8]);cfg['Serial']=[];cfg['Sharing'].update(ClipboardSharing=False,DirectoryShareMode='None');cfg['QEMU']['DebugLog']=False
    cfg['Information']['Notes']='Disposable desktop fixture. Blank test disk; no saved wallet or chats imported.'
    if m.get('virgl'):
        cfg['Display'][0]['Hardware']='virtio-gpu-gl-device'
        cfg['Information']['Name']='NexatrodeOS GPU Desktop — Disposable '+tag[:8]
        assert a.boot_only or a.gpu_navigation or m.get('gpu_verify'),'pixel tests require a diagnostic readback build'
    if m.get('desktop_resize'):assert a.boot_only,'use boot-only plus desktop_resize_input.py; the older app test has fixed coordinates'
    if a.gpu_navigation:assert m.get('virgl'),'GPU navigation requires a GPU image'
    old=cfg['Drive'][0]['Identifier'];drive=str(uuid.uuid4()).upper();cfg['Drive'][0].update(Identifier=drive,ImageName='blank.img')
    opts=cfg['QEMU']['AdditionalArguments'];assert all(x.startswith(('-global virtio-','-device loader,','-device virtio-','-netdev user,id=net0')) for x in opts)
    assert sum(x.startswith('-device loader,') for x in opts)==1
    for i,v in enumerate(opts):
        if v.startswith('-device loader,'):opts[i]=f'-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0'
        elif v.startswith('-device virtio-blk-device,'):opts[i]=v.replace(old,drive)
        elif v.startswith('-netdev user,id=net0'):opts[i]='-netdev user,id=net0'
    configure_display(cfg,opts,m)
    shutil.copyfile(guest/'loader.img',data/'loader.img')
    with (data/'blank.img').open('wb') as f:f.truncate(64*1024*1024)
    subprocess.run(['sgdisk','--clear','--new=1:2048:0','--typecode=1:0700',str(data/'blank.img')],check=True,stdout=subprocess.DEVNULL)
    trace=data/'trace.log';mp=data/'monitor'
    for suffix in ('.in','.out'):os.mkfifo(str(mp)+suffix,0o600)
    opts.extend(['-serial file:'+str(trace),'-qmp pipe:'+str(mp)])
    assert not (a.native_input_probe and a.gpu_command_trace),'use one bounded trace at a time'
    if a.gpu_command_trace:
        assert m.get('virgl')
        opts.extend(['-msg timestamp=on','-D '+str(data/'gpu-command.trace'),'-d trace:virtio_gpu_fence_*,trace:virtio_gpu_cmd_suspended,trace:virtio_gpu_cmd_ctx_submit,trace:virtio_gpu_cmd_set_scanout,trace:virtio_gpu_cmd_res_flush,trace:qemu_spice_gl_*'])
    if a.native_input_probe:
        assert m.get('desktop_resize')
        opts.extend(['-D '+str(data/'native-input.trace'),'-d trace:input_event_*'])
    (bundle/'config.plist').write_bytes(plistlib.dumps(cfg));monitor=QmpPipe(mp)
    session={'uuid':tag,'bundle':str(bundle),'saved_data_used':False,'test_passed':False,
             'validation_scope':'boot only; native input not checked' if a.boot_only else 'automated QMP app input',
             'native_input_trace':a.native_input_probe,'gpu_command_trace':a.gpu_command_trace,'loader_sha256':m['loader_sha256'],'gpu_verify':m.get('gpu_verify',False)};(guest/'session.json').write_text(json.dumps(session,indent=2)+'\n')
    registered=started=success=False
    try:
        print('Disposable desktop apps VM '+tag,flush=True);subprocess.run(['open','-g','-a','UTM',str(bundle)],check=True)
        end=time.monotonic()+45
        while time.monotonic()<end:
            if any(tag in row and 'stopped'in row for row in ctl('list').stdout.splitlines()):registered=True;break
            time.sleep(.25)
        if not registered:raise RuntimeError('VM did not register')
        r=ctl('start',*(['--hide'] if a.boot_hidden else []),tag);assert r.returncode==0,r.stderr;started=True;monitor.connect()
        end=time.monotonic()+120
        while time.monotonic()<end:
            text=trace.read_text(errors='replace') if trace.exists() else ''
            if any(s in text for s in ('MON|ERROR','panicked at','VMFault','Data Abort','Prefetch Abort','DESKTOP_GPU|FAIL|','DISPLAY_AGENT|FAIL|')):raise RuntimeError(text[-4500:])
            if m.get('virgl'):
                # Debug serial writes from four cores may interleave. A good
                # rendered boot must not depend on intact text markers.
                state=gpu_boot_state(monitor,data)
                if state['ready']:
                    session['boot_readiness']=state
                    break
            elif 'DESKTOP|WORKSPACE_READY|'in text and text.count('SLINT|FRAME_COPY_RAM_V1')>=2:break
            time.sleep(.2)
        else:raise RuntimeError('boot timeout: '+text[-4500:])
        time.sleep(1)
        if not m.get('virgl') or m.get('gpu_verify'):
            capture(monitor,data/'initial.ppm',m.get('gpu_verify',False));shutil.copyfile(data/'initial.ppm',guest/'initial.ppm');subprocess.run(['magick',str(guest/'initial.ppm'),str(guest/'initial.png')],check=True)
        if a.gpu_navigation:
            from desktop_gpu_navigation import exercise
            exercise(monitor,trace,data,guest,m.get('gpu_verify',False))
        elif not a.boot_only:
            from desktop_apps_input import exercise
            exercise(monitor,trace,data,guest,browser=m.get('browser',False),gpu=m.get('gpu_verify',False))
        if (guest/'validation.json').is_file() and not a.boot_only and not a.gpu_navigation:
            validation=json.loads((guest/'validation.json').read_text())
            session['wallet_full_sync_completed']=validation.get('wallet_full_sync_completed',False)
        success=True;session['test_passed']=True;(guest/'session.json').write_text(json.dumps(session,indent=2)+'\n')
    except BaseException:
        if started:
            try:
                monitor.send('screendump',{'filename':str(data/'failed.ppm')});shutil.copyfile(data/'failed.ppm',guest/'failed.ppm')
                subprocess.run(['magick',str(guest/'failed.ppm'),str(guest/'failed.png')],check=True)
            except Exception:pass
        raise
    finally:
        monitor.close()
        if trace.exists():shutil.copyfile(trace,guest/'serial.log')
        if not(a.leave_running and success):
            if started:ctl('stop',tag)
            if registered:ctl('delete',tag)
        if success:print('Desktop app test passed'+('; VM left running' if a.leave_running else ''),flush=True)
if __name__=='__main__':main()
