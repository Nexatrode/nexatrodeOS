#!/usr/bin/env python3
"""Create only a new, blank-data mobile browser UTM fixture and test it."""
import argparse,hashlib,json,os,plistlib,shutil,subprocess,time,uuid
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
ROOT=Path(__file__).resolve().parents[1]
CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
def ctl(*args):return subprocess.run([CTL,*map(str,args)],capture_output=True,text=True,timeout=30)
def stage(path):
    path=path.resolve();assert path.is_relative_to(ROOT/'build/mobile-browser') and (path/'.disposable-build').is_file()
    manifest=json.loads((path/'build.json').read_text())
    assert manifest['profile'] in ('mobile-netsurf','mobile-netsurf-maps','mobile-maps-os-unlock') and manifest['saved_data_used'] is False
    assert manifest['development_storage_key'] is (not manifest.get('maps_os_unlock',False))
    digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    assert not (path/'loader.img').is_symlink(),'linked loader forbidden'
    assert digest(path/'loader.img')==manifest['loader_sha256']
    for name,sha in manifest['sources'].items():assert digest(ROOT/name)==sha,'rebuild changed source: '+name
    return path,manifest
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--template',type=Path,required=True);p.add_argument('--leave-running',action='store_true');p.add_argument('--boot-only',action='store_true');p.add_argument('--reuse',action='store_true',help='reuse only this verified generated disposable VM');a=p.parse_args()
    guest,m=stage(a.guest);assert a.reuse or not (guest/'session.json').exists(),'use --reuse for this existing disposable fixture'
    assert not host_locked(),'unlock the Mac for the disposable UTM test'
    os.umask(0o077)
    if a.reuse:
        old_state=json.loads((guest/'session.json').read_text());tag=old_state['uuid'];bundle=Path(old_state['bundle']).resolve();data=bundle/'Data'
        assert not old_state['saved_data_used'] and bundle.parent==guest and bundle.name=='Mobile-Browser-'+tag[:8]+'.utm'
        installed=json.loads((data/'build-manifest.json').read_text())
        assert installed.get('maps_os_unlock',False)==m.get('maps_os_unlock',False) and installed.get('map_import',False)==m.get('map_import',False) and installed.get('map_files',False)==m.get('map_files',False),'refuse cross-format disk reuse'
        old_cfg=plistlib.loads((bundle/'config.plist').read_bytes())
        assert old_cfg['Information']['UUID']==tag and len(old_cfg['Drive'])==(2 if m.get('map_storage') else 1) and old_cfg['Drive'][0]['ImageName']=='blank.img'
        assert data.resolve()==data and not any((data/n).is_symlink() for n in ('blank.img','loader.img'))
        subprocess.run(['open','-g','-a','UTM',str(bundle)],check=True)
        end=time.monotonic()+45
        while time.monotonic()<end:
            stopped=ctl('status',tag)
            if stopped.stdout.strip()=='stopped':break
            if stopped.stdout.strip()=='started':ctl('stop',tag)
            time.sleep(.25)
        else:raise RuntimeError('cannot confirm the disposable VM stopped; loader unchanged')
    else:
        tag=str(uuid.uuid4()).upper();bundle=guest/('Mobile-Browser-'+tag[:8]+'.utm');data=bundle/'Data';data.mkdir(parents=True)
    cfg=old_cfg if a.reuse else plistlib.loads(a.template.read_bytes());assert cfg['System']['CPUCount']==4 and cfg['QEMU']['Hypervisor'] and cfg['Network']==[] and len(cfg['Drive'])==(2 if a.reuse and m.get('map_storage') else 1)
    cfg['Information'].update(UUID=tag,Name=('NexatrodeOS Mobile Maps' if m.get('maps') else 'NexatrodeOS Mobile Browser')+' — Disposable '+tag[:8]);cfg['Serial']=[];cfg['Sharing'].update(ClipboardSharing=False,DirectoryShareMode='None');cfg['QEMU']['DebugLog']=False
    old=cfg['Drive'][0]['Identifier'];drive=old if a.reuse else str(uuid.uuid4()).upper();cfg['Drive'][0].update(Identifier=drive,ImageName='blank.img')
    opts=cfg['QEMU']['AdditionalArguments']
    if a.reuse:opts[:]=[x for x in opts if not x.startswith(('-spice agent-mouse=off','-serial file:','-qmp pipe:'))]
    assert all(x.startswith(('-global virtio-','-device loader,','-device virtio-','-netdev user,id=net0')) for x in opts)
    assert sum(x.startswith('-device loader,') for x in opts)==1 and sum(x.startswith('-device virtio-blk-device,') for x in opts)==(2 if a.reuse and m.get('map_storage') else 1)
    for i,x in enumerate(opts):
        if x.startswith('-device loader,'):opts[i]=f'-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0'
        elif x.startswith('-device virtio-blk-device,'):opts[i]=x.replace(old,drive)
        elif x.startswith('-netdev user,id=net0'):opts[i]='-netdev user,id=net0'
    shutil.copyfile(guest/'loader.img',data/'loader.img')
    shutil.copyfile(guest/'build.json',data/'build-manifest.json')
    if not a.reuse:
        with (data/'blank.img').open('wb') as f:f.truncate(64*1024*1024)
        subprocess.run(['sgdisk','--clear','--new=1:2048:0','--typecode=1:0700',str(data/'blank.img')],stdout=subprocess.DEVNULL,check=True)
    if m.get('map_storage'):
        from map_storage_fixture import configure
        configure(guest,data,cfg,m,reuse=a.reuse)
    for suffix in ('.in','.out'):
        if not Path(str(data/'monitor')+suffix).exists():os.mkfifo(str(data/'monitor')+suffix,0o600)
    opts+=['-spice agent-mouse=off','-serial file:'+str(data/'trace.log'),'-qmp pipe:'+str(data/'monitor')]
    (bundle/'config.plist').write_bytes(plistlib.dumps(cfg));q=MotionQmp(data/'monitor');passed=False
    state=dict(uuid=tag,bundle=str(bundle),saved_data_used=False,test_passed=False,loader_sha256=m['loader_sha256'])
    (guest/'session.json').write_text(json.dumps(state,indent=2)+'\n')
    try:
        subprocess.run(['open','-g','-a','UTM',str(bundle)],check=True)
        end=time.monotonic()+45
        while time.monotonic()<end:
            if any(tag in row and 'stopped' in row for row in ctl('list').stdout.splitlines()):break
            time.sleep(.25)
        else:raise RuntimeError('disposable VM did not register')
        started=ctl('start',tag);assert started.returncode==0,started.stderr
        q.connect();end=time.monotonic()+180
        while time.monotonic()<end:
            text=(data/'trace.log').read_text(errors='replace') if (data/'trace.log').exists() else ''
            assert not any(x in text for x in ('VMFault','panicked at','MON|ERROR','Data Abort')),text[-3500:]
            if 'SLINT FIRST FRAME' in text and 'NETSURF|BOOT|' in text and (not m.get('maps') or ('MAPS_LOCK|BOOT_LOCKED' in text if m.get('maps_os_unlock') else 'COMAPS|READY|' in text)):break
            time.sleep(.2)
        else:raise RuntimeError('mobile boot timeout: '+text[-3500:])
        if not a.boot_only:
            assert m['observation'],'use the disposable --observe build for the input test'
            if m.get('maps'):
                from mobile_maps_input import exercise
            else:
                from mobile_browser_input import exercise
            evidence=guest/'interaction';evidence.mkdir(exist_ok=True);exercise(q,data,evidence)
        passed=True
        state.update(test_passed=True,scope='boot only' if a.boot_only else 'phone Maps, touch pan/zoom, labels, Home and four-app private routing' if m.get('maps') else 'phone launcher, forms, keyboard, touch scrolling, images, JavaScript, HTTPS and app routing')
        (guest/'session.json').write_text(json.dumps(state,indent=2)+'\n');print('PASS disposable mobile browser',tag,flush=True)
    finally:
        if not (a.leave_running and passed):
            try:q.send('quit')
            except Exception:ctl('stop',tag)
        q.close()
if __name__=='__main__':main()
