#!/usr/bin/env python3
"""Two public regions, cold restart and corrupted disposable map disks.

QMP input and actual scanout. Never reads an installed app disk or credentials.
Only the recorded fixture UUID may be restarted or have its maps.img modified.
"""
import argparse,hashlib,json,os,plistlib,shutil,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1]
CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
def ctl(*args):return subprocess.run([CTL,*map(str,args)],capture_output=True,text=True,timeout=30)
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--mobile',action='store_true');p.add_argument('--regression',action='store_true',help='also run the existing map/app integration checks');a=p.parse_args()
    guest=a.guest.resolve();assert guest.is_relative_to(ROOT/('build/mobile-browser' if a.mobile else 'build/desktop-apps')) and (guest/'.disposable-build').is_file()
    manifest=json.loads((guest/'build.json').read_text());session=json.loads((guest/'session.json').read_text())
    assert manifest['saved_data_used'] is session['saved_data_used'] is False and manifest['map_storage'] is True
    data=Path(session['bundle'])/'Data';tag=session['uuid'];disk=data/'maps.img'
    def guard():
        assert data.parent.parent==guest and data.resolve()==data and data.parent.suffix=='.utm'
        cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==tag
        assert [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
        assert not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
        assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==session['loader_sha256']==manifest['loader_sha256']
    guard();assert not host_locked(),'Mac must be unlocked for visible UTM checks'
    evidence=guest/'storage-verification';evidence.mkdir(exist_ok=True);steps=[];q=None
    def connect():
        nonlocal q
        q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
        try:q.send('query-status')
        except RuntimeError as e:
            if 'Expecting capabilities negotiation' not in str(e):raise
            q.send('qmp_capabilities')
    def memory(address,size):
        path=data/'map-storage-check.bin'
        try:q.send('pmemsave',{'val':address,'size':size,'filename':str(path)});return path.read_bytes()
        finally:path.unlink(missing_ok=True)
    def state():
        for _ in range(50):
            raw=memory(0x5fb96000,128);v=struct.unpack('<16Q',raw)
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<Q',memory(0x5fb96000,8))[0] and v[1]==0x4e584d4150303031:return v
        return (0,)*16
    def check():
        text=(data/'trace.log').read_text(errors='replace')
        assert not any(x in text for x in ('COMAPS|FAIL|','VMFault','panicked at','MON|ERROR','Data Abort','DESKTOP_GPU|FAIL|')),text[-4000:]
    def until(fn,seconds=90):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            check();v=fn()
            if v:return v
            time.sleep(.04)
        raise TimeoutError('map storage state: '+str(state()))
    def mode():return struct.unpack('<6I',memory(0x5fb92000,24))
    def scene():return struct.unpack('<20I',memory(0x5fbb4000,80))
    def pointer(x,y,down=None):
        w,h=(1080,2340) if a.mobile else (mode()[2]//2,mode()[3]//2)
        events=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in [('x',x,w),('y',y,h)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':events})
    def tap(x,y):pointer(x,y,True);time.sleep(.06);pointer(x,y,False)
    def map_tap(x,y):
        if a.mobile:tap(x*3,80*2.6086957+y*3)
        else:
            s=scene();tap(s[16]+x,s[17]+y)
    def open_maps():
        until(lambda:state()[0])
        if a.mobile:
            until(lambda:struct.unpack('<Q',memory(0x5fb92000,8))[0]==0x4e584445534b3031)
            page=struct.unpack('<Q',memory(0x5fb92008,8))[0]
            if page!=19:
                if page!=0:
                    x,y=(40,65) if page==16 else (370,60) if page==18 else (350,136)
                    tap(x*2.6086957,y*2.6086957)
                    until(lambda:struct.unpack('<Q',memory(0x5fb92008,8))[0]==0)
                tap(300*2.6086957,360*2.6086957)
        else:
            until(lambda:mode()[1] and scene()[0]);w,h=mode()[2]//2,mode()[3]//2;tap((w-360)//2+306,h-65)
        until(lambda:state()[4]==1)
    def choose(region,*,error=False):
        old=state();t=time.monotonic();map_tap(85,26);time.sleep(.15);map_tap(85,64+6+48*region+22)
        if error:until(lambda:state()[13]==2)
        else:until(lambda:state()[11]==region+1 and state()[13]==0 and state()[15]>old[15])
        result=state();steps.append(dict(action='choose',region=region,error=error,state=list(result),host_observed_ms=round((time.monotonic()-t)*1000,3)))
        return result
    def screenshot(name,words):
        end=time.monotonic()+35
        while time.monotonic()<end:
            path=data/'map-storage-screen.ppm';capture(q,path,not a.mobile)
            target=evidence/(name+'.png');subprocess.run(['magick',str(path),str(target)],check=True)
            text=subprocess.run(['tesseract',str(target),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout
            normalized=lambda t:''.join(x for x in t.lower() if x.isalnum())
            if all(normalized(w) in normalized(text) for w in words):
                (evidence/(name+'.txt')).write_text(text);return
            time.sleep(.1)
        raise AssertionError((words,text))
    def stop():
        nonlocal q
        guard()
        if q:
            try:q.send('quit')
            finally:q.close();q=None
        end=time.monotonic()+45
        while time.monotonic()<end:
            if ctl('status',tag).stdout.strip()=='stopped':return
            time.sleep(.2)
        raise TimeoutError('fixture did not stop; disk unchanged')
    def start(expected,*,error=False):
        guard();assert ctl('status',tag).stdout.strip()=='stopped'
        result=ctl('start',tag);assert result.returncode==0,result.stderr
        nonlocal q
        q=MotionQmp(data/'monitor');q.connect();until(lambda:state()[0] and state()[13]==(2 if error else 0))
        assert state()[11]==expected+1 if expected is not None else state()[11]==0,state()
        open_maps();steps.append(dict(action='cold boot',state=list(state())))
    def banks():
        with disk.open('rb') as f:raw=f.read(8192)
        result=[]
        for i in range(2):
            p=raw[i*4096:(i+1)*4096];magic,g,r,inverse=struct.unpack_from('<4Q',p)
            if magic==0x4e584d53544f5231 and 0<g<2**64-1 and r<2 and inverse==r^((1<<64)-1) and hashlib.sha256(p[:32]).digest()==p[32:64]:result.append((g,r,i))
        return sorted(result)
    def restore_files():
        guard();assert ctl('status',tag).stdout.strip()=='stopped'
        # Restore only this fixture's public map payloads, retaining selections.
        with disk.open('r+b') as f, (guest/'maps.img').open('rb') as source:
            f.truncate(32*1024*1024)
            for i,r in enumerate(manifest['map_region']['storage_regions']['regions']):
                offset=(i+1)*8*1024*1024;source.seek(offset);f.seek(offset);f.write(source.read(r['bytes']))
            f.flush();os.fsync(f.fileno())
    try:
        connect();until(lambda:state()[13]==0 and state()[11] in (1,2));open_maps()
        if state()[11]!=1:choose(0)
        screenshot('gibraltar',['Gibraltar','CoMaps','Offline'])
        choose(1);screenshot('san-marino',['San Marino','CoMaps','Offline'])
        choose(0);choose(1);choose(0);choose(1)
        last=state();stop();journal=banks();assert len(journal)==2 and journal[-1][1]==1 and journal[-2][1]==0,journal
        start(1);assert state()[15]==last[15];screenshot('restart-san-marino',['San Marino'])
        stop()
        with disk.open('r+b') as f:f.seek(journal[-1][2]*4096);f.write(bytes(32));f.flush();os.fsync(f.fileno())
        start(0);assert state()[15]==journal[-2][0];steps.append(dict(action='torn newest selection recovered older bank',banks=journal))
        choose(1);stop()
        # A byte-corrupt region must not replace the currently valid region.
        with disk.open('r+b') as f:f.seek(16*1024*1024+4000);b=f.read(1);f.seek(-1,1);f.write(bytes([b[0]^128]));f.flush();os.fsync(f.fileno())
        start(None,error=True);assert state()[14]==2;screenshot('corrupt-selected-region',['San Marino','damaged'])
        choose(0);old=state();bad=choose(1,error=True);assert bad[11]==old[11] and bad[5:9]==old[5:9] and bad[14]==2
        screenshot('corrupt-file-retains-map',['Gibraltar','damaged']);choose(0);stop();restore_files()
        # An incomplete region payload is rejected by the pinned whole-file hash.
        length=manifest['map_region']['storage_regions']['regions'][1]['bytes']
        with disk.open('r+b') as f:f.seek(16*1024*1024+length-512);f.write(bytes(512));f.flush();os.fsync(f.fileno())
        start(0);choose(1,error=True);assert state()[11]==1 and state()[14]==2;stop();restore_files()
        # A physically truncated pack fails capacity validation without a fault.
        with disk.open('r+b') as f:f.truncate(16*1024*1024+4096);f.flush();os.fsync(f.fileno())
        start(None,error=True);assert state()[14]==1;screenshot('truncated-pack',['damaged']);stop();restore_files()
        start(0);choose(1);screenshot('recovered',['San Marino','CoMaps','Offline']);check()
        if a.regression:
            choose(0)
            if a.mobile:
                from mobile_maps_input import exercise
                exercise(q,data,evidence/'regression')
            else:
                from desktop_maps_input import exercise
                exercise(q,data,evidence/'regression',gpu=True,functional=True,load=True)
        assert not host_locked(),'host locked during visible checks'
        result=dict(passed=True,mobile=a.mobile,uuid=tag,loader_sha256=manifest['loader_sha256'],map_pack_sha256=manifest['map_pack_sha256'],harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),saved_data_used=False,pixel_wifi_touched=False,regression_passed=a.regression,steps=steps,checks=['two disk-backed regions and repeated switching','actual scanout of both regions','cold restart preserves last successful region without rewriting selection','torn newest journal recovers older selection','corrupt selected region fails closed and another region remains loadable','corrupt and incomplete region payload retain good map and remain retryable','truncated map disk fails gracefully','restoring public payloads recovers both regions'],scope='disposable public-map disk, QMP input and scanout; not personal data, Pixel touch, downloads, general filesystem, private bookmarks or a lock integration test')
        (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='steps'},indent=2),flush=True)
    finally:
        if q:
            try:q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
            finally:q.close()
if __name__=='__main__':main()
