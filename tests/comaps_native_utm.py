#!/usr/bin/env python3
"""Run only the diskless CoMaps-data image in its own disposable UTM VM."""
import argparse,hashlib,json,os,plistlib,shutil,subprocess,sys,time,uuid
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
ROOT=Path(__file__).resolve().parents[1]
CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
def ctl(*args):return subprocess.run([CTL,*args],capture_output=True,text=True,timeout=30)
def stage(path):
    path=path.resolve();assert path.is_relative_to(ROOT/'build/comaps') and (path/'.disposable-build').is_file()
    m=json.loads((path/'build.json').read_text());assert m['profile']=='comaps-offline-data'
    assert all(m[k] is False for k in ('saved_data_used','storage','network','gps','full_comaps_app'))
    digest=lambda f:hashlib.sha256(f.read_bytes()).hexdigest()
    assert not (path/'loader.img').is_symlink() and digest(path/'loader.img')==m['loader_sha256']
    assert digest(path/'map.system')==m['system_sha256']
    for f,h in m['sources'].items():assert digest(ROOT/f)==h,'rebuild changed source '+f
    sys.path.insert(0,str(ROOT/'experiments/comaps-native'))
    from build import validate
    import xml.etree.ElementTree as ET
    validate(ET.parse(path/'map.system').getroot());return path,m

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--template',type=Path,required=True);p.add_argument('--reuse',action='store_true');p.add_argument('--leave-running',action='store_true');a=p.parse_args()
    guest,m=stage(a.guest);assert not host_locked(),'unlock the Mac to run the disposable UTM fixture'
    os.umask(0o077)
    if a.reuse:
        s=json.loads((guest/'session.json').read_text());tag=s['uuid'];bundle=Path(s['bundle']).resolve()
        assert not s['saved_data_used'] and bundle.parent==guest and bundle.name=='CoMaps-'+tag[:8]+'.utm'
        config=plistlib.loads((bundle/'config.plist').read_bytes());assert config['Information']['UUID']==tag and config['Drive']==[] and config['Network']==[]
        assert not (bundle/'Data/loader.img').is_symlink()
        assert ctl('status',tag).stdout.strip()=='stopped','stop only this disposable VM before reusing it'
    else:
        assert not (guest/'session.json').exists(),'reuse this recorded disposable fixture'
        tag=str(uuid.uuid4()).upper();bundle=guest/('CoMaps-'+tag[:8]+'.utm');(bundle/'Data').mkdir(parents=True)
        config=plistlib.loads(a.template.read_bytes());assert config['System']['CPUCount']==4 and config['QEMU']['Hypervisor'] and config['Network']==[] and len(config['Drive'])==1
        options=config['QEMU']['AdditionalArguments'];assert all(x.startswith(('-global virtio-','-device loader,','-device virtio-','-netdev user,id=net0')) for x in options)
        config['QEMU']['AdditionalArguments']=[x for x in options if not x.startswith(('-device virtio-blk-device','-device virtio-net-device','-netdev '))]
    data=bundle/'Data';assert data.resolve()==data
    config['Information'].update(UUID=tag,Name='NexatrodeOS Offline Maps — Disposable '+tag[:8]);config['Drive']=[];config['Network']=[];config['Serial']=[]
    config['Sharing'].update(ClipboardSharing=False,DirectoryShareMode='None');config['QEMU']['DebugLog']=False
    opts=config['QEMU']['AdditionalArguments'];opts[:]=[x for x in opts if not x.startswith(('-serial file:','-qmp pipe:','-spice agent-mouse=off'))]
    for i,x in enumerate(opts):
        if x.startswith('-device loader,'):opts[i]=f'-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0'
    assert len([x for x in opts if x.startswith('-device loader,')])==1
    assert not any(x.startswith(('-netdev','-device virtio-net','-device virtio-blk')) for x in opts)
    shutil.copyfile(guest/'loader.img',data/'loader.img')
    for suffix in ('.in','.out'):
        if not Path(str(data/'monitor')+suffix).exists():os.mkfifo(str(data/'monitor')+suffix,0o600)
    opts+=['-spice agent-mouse=off','-serial file:'+str(data/'trace.log'),'-qmp pipe:'+str(data/'monitor')]
    (bundle/'config.plist').write_bytes(plistlib.dumps(config))
    state=dict(uuid=tag,bundle=str(bundle),loader_sha256=m['loader_sha256'],saved_data_used=False,network=False,storage=False,test_passed=False)
    (guest/'session.json').write_text(json.dumps(state,indent=2)+'\n');q=MotionQmp(data/'monitor');passed=False
    try:
        subprocess.run(['open','-g','-a','UTM',str(bundle)],check=True)
        end=time.monotonic()+45
        while time.monotonic()<end:
            if any(tag in row and 'stopped' in row for row in ctl('list').stdout.splitlines()):break
            time.sleep(.2)
        else:raise RuntimeError('disposable VM registration failed')
        result=ctl('start',tag);assert result.returncode==0,result.stderr
        q.connect();end=time.monotonic()+90
        while time.monotonic()<end:
            text=(data/'trace.log').read_text(errors='replace') if (data/'trace.log').exists() else ''
            assert not any(x in text for x in ('COMAPS|FAIL|','MON|ERROR','VMFault','panicked at','Data Abort')),text[-3500:]
            if 'COMAPS|READY|' in text:break
            time.sleep(.2)
        else:raise RuntimeError('map boot timeout: '+text[-3500:])
        from comaps_native_input import exercise
        exercise(q,data,guest/'interaction')
        result_path=guest/'interaction/validation.json'
        result=json.loads(result_path.read_text());result['loader_sha256']=m['loader_sha256']
        result_path.write_text(json.dumps(result,indent=2)+'\n')
        passed=True;state['test_passed']=True
        (guest/'session.json').write_text(json.dumps(state,indent=2)+'\n');print('PASS disposable offline maps',tag,flush=True)
    finally:
        if not (passed and a.leave_running):
            try:q.send('quit')
            except Exception:ctl('stop',tag)
        q.close()
if __name__=='__main__':main()
