#!/usr/bin/env python3
"""Boot only a new desktop-apps build with a blank GPT disk and fresh relay peer."""
import argparse, hashlib, json, os
from pathlib import Path
import plistlib, shutil, subprocess, time, uuid
from utm_vault_key_storage import QmpPipe
ROOT=Path(__file__).resolve().parents[1]
CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
def ctl(*args):return subprocess.run([CTL,*map(str,args)],capture_output=True,text=True,timeout=30)
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--template',type=Path,required=True);p.add_argument('--boot-only',action='store_true');p.add_argument('--leave-running',action='store_true');a=p.parse_args()
    guest=a.guest.resolve();assert guest.is_relative_to(ROOT/'build/desktop-apps')
    m=json.loads((guest/'build.json').read_text());assert m['saved_data_used'] is False and m['development_storage_key'] is True
    assert hashlib.sha256((guest/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']
    for path,h in m['sources'].items():assert hashlib.sha256((ROOT/path).read_bytes()).hexdigest()==h,path
    state=plistlib.loads(subprocess.check_output(['ioreg','-n','Root','-d','1','-a'],timeout=10));state=[state] if isinstance(state,dict) else state
    assert not any(u.get('CGSSessionScreenIsLocked',False) for row in state for u in row.get('IOConsoleUsers',[])),'Mac must be unlocked'
    os.umask(0o077);tag=str(uuid.uuid4()).upper();bundle=guest/('Desktop-Apps-'+tag[:8]+'.utm');data=bundle/'Data';data.mkdir(parents=True)
    cfg=plistlib.loads(a.template.read_bytes());assert cfg['System']['CPUCount']==4 and cfg['QEMU']['Hypervisor'] is True
    assert cfg['Network']==[] and len(cfg['Drive'])==1
    cfg['Information'].update(UUID=tag,Name='NexatrodeOS Apps — Disposable '+tag[:8]);cfg['Serial']=[];cfg['Sharing'].update(ClipboardSharing=False,DirectoryShareMode='None');cfg['QEMU']['DebugLog']=False
    old=cfg['Drive'][0]['Identifier'];drive=str(uuid.uuid4()).upper();cfg['Drive'][0].update(Identifier=drive,ImageName='blank.img')
    opts=cfg['QEMU']['AdditionalArguments'];assert all(x.startswith(('-global virtio-','-device loader,','-device virtio-','-netdev user,id=net0')) for x in opts)
    assert sum(x.startswith('-device loader,') for x in opts)==1
    for i,v in enumerate(opts):
        if v.startswith('-device loader,'):opts[i]=f'-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0'
        elif v.startswith('-device virtio-blk-device,'):opts[i]=v.replace(old,drive)
        elif v.startswith('-netdev user,id=net0'):opts[i]='-netdev user,id=net0'
    shutil.copyfile(guest/'loader.img',data/'loader.img')
    with (data/'blank.img').open('wb') as f:f.truncate(64*1024*1024)
    subprocess.run(['sgdisk','--clear','--new=1:2048:0','--typecode=1:0700',str(data/'blank.img')],check=True,stdout=subprocess.DEVNULL)
    trace=data/'trace.log';mp=data/'monitor'
    for suffix in ('.in','.out'):os.mkfifo(str(mp)+suffix,0o600)
    opts.extend(['-serial file:'+str(trace),'-qmp pipe:'+str(mp)])
    (bundle/'config.plist').write_bytes(plistlib.dumps(cfg));monitor=QmpPipe(mp)
    session={'uuid':tag,'bundle':str(bundle),'saved_data_used':False,'test_passed':False};(guest/'session.json').write_text(json.dumps(session,indent=2)+'\n')
    registered=started=success=False
    try:
        print('Disposable desktop apps VM '+tag,flush=True);subprocess.run(['open','-g','-a','UTM',str(bundle)],check=True)
        end=time.monotonic()+45
        while time.monotonic()<end:
            if any(tag in row and 'stopped'in row for row in ctl('list').stdout.splitlines()):registered=True;break
            time.sleep(.25)
        if not registered:raise RuntimeError('VM did not register')
        r=ctl('start',tag);assert r.returncode==0,r.stderr;started=True;monitor.connect()
        end=time.monotonic()+120
        while time.monotonic()<end:
            text=trace.read_text(errors='replace') if trace.exists() else ''
            if any(s in text for s in ('MON|ERROR','panicked at','VMFault','Data Abort','Prefetch Abort')):raise RuntimeError(text[-4500:])
            if 'DESKTOP|WORKSPACE_READY|'in text and text.count('SLINT|FRAME_COPY_RAM_V1')>=2:break
            time.sleep(.2)
        else:raise RuntimeError('boot timeout: '+text[-4500:])
        time.sleep(1);monitor.send('screendump',{'filename':str(data/'initial.ppm')});shutil.copyfile(data/'initial.ppm',guest/'initial.ppm');subprocess.run(['magick',str(guest/'initial.ppm'),str(guest/'initial.png')],check=True)
        if not a.boot_only:
            from desktop_apps_input import exercise
            exercise(monitor,trace,data,guest)
        if (guest/'validation.json').is_file() and not a.boot_only:
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
