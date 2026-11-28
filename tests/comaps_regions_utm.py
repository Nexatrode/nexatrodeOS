#!/usr/bin/env python3
"""Bounded larger-region loading, manager, timings and corruption recovery.

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
    assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest()==h for p,h in manifest['sources'].items())
    evidence=guest/'region-verification';evidence.mkdir(exist_ok=True);steps=[];q=None
    harness_source=Path(__file__).read_bytes();(evidence/'harness-used.py').write_bytes(harness_source)
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
            raw=memory(0x5fb96000,8192);v=struct.unpack('<1024Q',raw)
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<Q',memory(0x5fb96000,8))[0] and v[1]==0x4e584d4150303031:return v
        return (0,)*1024
    def check():
        text=(data/'trace.log').read_text(errors='replace')
        assert not any(x in text for x in ('COMAPS|FAIL|','VMFault','panicked at','MON|ERROR','Data Abort','DESKTOP_GPU|FAIL|')),text[-4000:]
    def until(fn,seconds=90):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            check();v=fn()
            if v:return v
            time.sleep(.04)
        raise TimeoutError('map storage state: '+str(state()[:16])+str(state()[880:893]))
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
        until(lambda:state()[0] and state()[13] in (0,2))
        if a.mobile:
            until(lambda:struct.unpack('<Q',memory(0x5fb92008,8))[0] in (0,16,18,19))
            page=struct.unpack('<Q',memory(0x5fb92008,8))[0]
            if page!=19:
                if page!=0:
                    x,y=(40,65) if page==16 else (370,60) if page==18 else (350,136)
                    tap(x*2.6086957,y*2.6086957)
                    until(lambda:struct.unpack('<Q',memory(0x5fb92008,8))[0]==0)
                tap(300*2.6086957,360*2.6086957)
        else:
            until(lambda:mode()[1] and scene()[0]);last=None;since=time.monotonic()
            def settled():
                nonlocal last,since
                m=mode();g=struct.unpack('<32I',memory(0x5fba0000,128))
                if not g[0] or g[0]&1 or tuple(g[9:11])!=tuple(m[2:4]) or g[11]!=m[1]:return False
                if m!=last:last=m;since=time.monotonic();return False
                return time.monotonic()-since>=.8
            until(settled);w,h=mode()[2]//2,mode()[3]//2;tap((w-360)//2+306,h-65)
        until(lambda:state()[4]==1)
    def choose(region,*,error=False):
        old=state();t=time.monotonic();map_tap(85,26);time.sleep(.15);map_tap(85,64+6+48*region+22)
        if error:until(lambda:state()[13]==2)
        else:until(lambda:state()[11]==region+1 and state()[13]==0 and state()[15]>old[15])
        result=state();steps.append(dict(action='choose',region=region,error=error,state=list(result[:16]),profile=list(result[880:893]),host_observed_ms=round((time.monotonic()-t)*1000,3)))
        return result
    def screenshot(name,words):
        end=time.monotonic()+35
        while time.monotonic()<end:
            path=data/'map-storage-screen.ppm';capture(q,path,not a.mobile)
            target=evidence/(name+'.png');subprocess.run(['magick',str(path),str(target)],check=True)
            ocr=target
            if not a.mobile:
                sc=scene();ocr=data/'region-map-crop.png';subprocess.run(['magick',str(target),'-crop',f'1120x{state()[9]*2}+{sc[16]*2}+{sc[17]*2}','+repage','-colorspace','Gray','-negate',str(ocr)],check=True)
            text=subprocess.run(['tesseract',str(ocr),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout
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
        if a.mobile:until(lambda:struct.unpack('<Q',memory(0x5fb92008,8))[0]==16)
        open_maps();steps.append(dict(action='cold boot',state=list(state())))
    width=360 if a.mobile else 560
    def key(code):q.send('input-send-event',{'events':[{'type':'key','data':{'down':v,'key':{'type':'qcode','data':code}}} for v in (True,False)]})
    def settled():
        st=until(lambda:state() if state()[13] in (0,2) and not state()[881] else None);assert st[884]<=32768 and st[885]<=524288;return st
    def sample(action):
        st=settled();record=dict(action=action,features=st[884],points=st[885],pages=st[883],read_ms=st[886]/1000,hash_ms=st[887]/1000,decode_ms=st[888]/1000,total_ms=st[889]/1000,bytes_read=st[890],detail=st[891]);steps.append(record);return record
    def zoom_detail(error=False):
        for _ in range(3):map_tap(width-89,35);time.sleep(.12)
        until(lambda:state()[13]==(2 if error else 0) and (error or state()[891]==1));return sample('detail failed' if error else 'detail load')
    def flip(byte):
        guard();assert ctl('status',tag).stdout.strip()=='stopped'
        with disk.open('r+b') as f:f.seek(byte);b=f.read(1);f.seek(byte);f.write(bytes([b[0]^128]));f.flush();os.fsync(f.fileno())
    pages=manifest['map_region']['storage_regions']['regions'][2]['spatial']['pages'];profile=manifest['map_region']['storage_regions']['regions'][2]['spatial']
    try:
        connect();open_maps()
        if state()[11]!=1:choose(0)
        map_tap(85,26);until(lambda:state()[880]);screenshot('region-manager',['Gibraltar','San Marino','Liechtenstein','Installed','chunked']);map_tap(85,26)
        for _ in range(3):choose(2);sample('larger overview');choose(0)
        # Slow only this disposable disk to observe a cancellation deterministically.
        cfg=plistlib.loads((data.parent/'config.plist').read_bytes());drive='drive'+cfg['Drive'][1]['Identifier']
        def throttle(bps):q.send('block_set_io_throttle',dict(device=drive,bps=bps,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0))
        throttle(131072)
        try:
            old=state()[15];map_tap(85,26);time.sleep(.08);map_tap(85,188);until(lambda:state()[13]==1)
            map_tap(85,26);until(lambda:state()[880]);map_tap(80,64+222)
        finally:throttle(0)
        until(lambda:state()[13]==0);assert state()[11]==1 and state()[15]==old
        steps.append(dict(action='cancel discards candidate and does not commit selection'))
        choose(2);screenshot('larger-overview',['Liechtenstein','Offline']);overview=sample('larger overview for pan')
        detail=zoom_detail();assert detail['points']<524288 and detail['bytes_read']<profile['bytes']
        screenshot('larger-detail',['Liechtenstein','Offline'])
        workers=[subprocess.Popen(['python3','-c','import time;end=time.monotonic()+25\nwhile time.monotonic()<end: sum(i*i for i in range(30000))'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL) for _ in range(4)]
        try:
            for code in ['right']*4+['down']*4+['left']*4+['up']*4:
                old=state();started=time.monotonic();key(code);until(lambda:state()[2]>old[2]);settled();steps.append(dict(action='pan under host load',clamped=state()[5:7]==old[5:7],observed_ms=(time.monotonic()-started)*1000,points=sample('pan area')['points']))
        finally:
            for worker in workers:worker.terminate();worker.wait()
        stop();start(2);screenshot('restart-larger',['Liechtenstein','Offline']);steps.append(dict(action='restart retains larger region'))
        # Damage the selected overview; startup must fail closed. A good region
        # stays usable and an invalid candidate never replaces it.
        stop();flip(24*1024*1024+4000);start(None,error=True);assert state()[14]==2
        choose(0);before=state();choose(2,error=True);assert state()[11]==1 and state()[5:9]==before[5:9]
        screenshot('corrupt-retains-map',['Gibraltar','damaged']);stop();flip(24*1024*1024+4000);start(0)
        choose(2);zoom_detail();st=state();ox,oy=profile['origin'];cx=struct.unpack('<q',struct.pack('<Q',st[5]))[0]/1e9;cy=struct.unpack('<q',struct.pack('<Q',st[6]))[0]/1e9;u=st[7]/1e12;dx=width*u*.75;dy=(st[9]-(52 if a.mobile else 36)-64)*u*.75
        selected=[i for i,p in enumerate(pages) if i and p['features'] and not (p['bounds'][2]<cx-dx+ox or p['bounds'][3]<cy-dy+oy or p['bounds'][0]>cx+dx+ox or p['bounds'][1]>cy+dy+oy)]
        assert selected;byte=24*1024*1024+pages[selected[0]]['offset']+32
        stop();flip(byte);start(2);zoom_detail(error=True);assert state()[11]==3 and state()[891]==0
        steps.append(dict(action='corrupt detail page retains authenticated overview',page=selected[0]));stop();flip(byte);start(2);zoom_detail()
        choose(0)
        if a.regression:
            if a.mobile:
                from mobile_maps_input import exercise
                exercise(q,data,evidence/'regression',browser_regression=False)
            else:
                from desktop_maps_input import exercise
                exercise(q,data,evidence/'regression',gpu=True,functional=True,load=True)
        assert not host_locked();check()
        result=dict(passed=True,mobile=a.mobile,uuid=tag,loader_sha256=manifest['loader_sha256'],harness_sha256=hashlib.sha256(harness_source).hexdigest(),saved_data_used=False,pixel_wifi_touched=False,regression_passed=a.regression,steps=steps,source_profile=profile,limits=dict(file_buffer_bytes=8388608,features_per_candidate=32768,points_per_candidate=524288),scope='Three pinned installed regions; public spatial rendering subset, no runtime downloads or arbitrary MWM support')
        (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k not in ('steps','source_profile')},indent=2))
    finally:
        if q and q.input is not None:
            try:q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
            finally:q.close()
if __name__=='__main__':main()
