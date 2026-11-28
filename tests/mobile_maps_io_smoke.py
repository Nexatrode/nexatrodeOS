#!/usr/bin/env python3
"""Final shared Maps I/O checks in an existing disposable mobile fixture.

Uses only the published test credentials enrolled by comaps_private_utm.py.
Never asks for or reads a personal passphrase, wallet or chat disk.
"""
import argparse,hashlib,json,os,plistlib,re,statistics,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
ROOT=Path(__file__).resolve().parents[1];SCALE=2.6086957
DEVICE_PASSWORD='disposabledevice42';MAPS_PASSWORD='mapstestprivate42'

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    stage=a.guest.resolve();out=a.output.resolve()
    assert stage.is_relative_to(ROOT/'build/mobile-browser') and (stage/'.disposable-build').is_file() and out.is_relative_to(ROOT/'build')
    m=json.loads((stage/'build.json').read_text());session=json.loads((stage/'session.json').read_text());assert m['map_storage'] and m['map_private'] and m['observation'] and m['saved_data_used'] is session['saved_data_used'] is False
    data=Path(session['bundle'])/'Data';assert data.parent.parent==stage and data.resolve()==data
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid'] and [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
    assert not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    assert all(hashlib.sha256((ROOT/f).read_bytes()).hexdigest()==h for f,h in m['sources'].items())
    assert not host_locked(),'Mac must be unlocked'
    out.mkdir(parents=True,exist_ok=True);source=Path(__file__).read_bytes();(out/'harness-used.py').write_bytes(source)
    q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
    try:q.send('query-status')
    except RuntimeError as e:
        if 'Expecting capabilities negotiation' not in str(e):raise
        q.send('qmp_capabilities')
    checks=[];samples=[]
    def memory(address,n):
        f=data/'mobile-io-state.bin'
        try:q.send('pmemsave',dict(val=address,size=n,filename=str(f)));return f.read_bytes()
        finally:f.unlink(missing_ok=True)
    def ui():return struct.unpack('<13Q',memory(0x5fb92000,104))
    def state():
        for _ in range(100):
            v=struct.unpack('<1024Q',memory(0x5fb96000,8192))
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<Q',memory(0x5fb96000,8))[0] and v[848]==0x4e584d5052495631:return v
            time.sleep(.005)
        raise TimeoutError('Maps timing seqlock')
    def until(fn,seconds=90):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            trace=(data/'trace.log').read_text(errors='replace');assert not any(s in trace for s in ('MON|ERROR','VMFault','panicked at','COMAPS|FAIL|','Data Abort')),trace[-2000:]
            r=fn()
            if r:return r
            time.sleep(.05)
        raise TimeoutError('mobile page '+str(ui()[1])+' map '+str(state()[:16])+' private '+str(state()[849:857]))
    def pos(x,y,down=None):
        events=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in [('x',x,1080),('y',y,2340)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',dict(events=events))
    def tap_phys(x,y):pos(x,y,True);time.sleep(.09);pos(x,y,False)
    def tap_ui(x,y):tap_phys(x*SCALE,y*SCALE)
    def tap_map(x,y):tap_phys(x*3,80*SCALE+y*3)
    def key(name):q.send('input-send-event',dict(events=[{'type':'key','data':{'down':v,'key':{'type':'qcode','data':name}}} for v in (True,False)]))
    def type_word(text):
        for c in text:key(c);time.sleep(.15)
    def screenshot(name,words):
        for _ in range(15):
            f=data/'mobile-io-screen.ppm';q.send('screendump',dict(filename=str(f)));target=out/(name+'.png');subprocess.run(['magick',str(f),str(target)],check=True)
            text=subprocess.run(['tesseract',str(target),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=25).stdout
            norm=lambda s:re.sub('[^a-z0-9]','',s.lower())
            if all(norm(w) in norm(text) for w in words):(out/(name+'.txt')).write_text(text);return
            time.sleep(.1)
        raise AssertionError((name,words,text))
    def device_unlock():
        assert ui()[1]==14
        pos(540,1900,True);time.sleep(.1);pos(540,1500);time.sleep(.1);pos(540,1500,False);time.sleep(.15)
        type_word(DEVICE_PASSWORD);key('ret');until(lambda:ui()[1]==0)
    def home():
        page=ui()[1]
        if page==14:device_unlock();return
        if page!=0:tap_ui(40 if page==16 else 370,65 if page==16 else 60);until(lambda:ui()[1]==0)
    def open_maps():
        home();tap_ui(300,360);until(lambda:ui()[1]==19 and state()[4] and state()[13]==0)
    def choose(region):
        old=state()[15];tap_map(85,26);until(lambda:state()[880]);tap_map(85,92+region*48)
        until(lambda:state()[11]==region+1 and state()[13]==0 and state()[15]>old)
    def saved():
        tap_map(230,state()[9]-16);until(lambda:state()[849] in (1,4) and not state()[852])
    def unlock_maps():
        saved();assert state()[849]==4
        type_word(MAPS_PASSWORD);key('ret');until(lambda:state()[850] and state()[851]==2 and not state()[852]);until(lambda:state()[9]>600)
    def lock_maps():
        if state()[849]!=1:saved()
        tap_map(295,92);until(lambda:not state()[850] and not state()[851] and state()[854] and not state()[849])
    try:
        until(lambda:ui()[0]==0x4e584445534b3031 and ui()[1]==14)
        screenshot('device-locked-after-upgrade',['DEVICE LOCKED']);device_unlock()
        checks.append('existing disposable device passphrase still unlocks after source upgrade')
        open_maps();assert state()[900]==0x4e584d494f303031
        choose(1);assert state()[884:886]==(32619,383542);screenshot('san-marino',['San Marino','Offline'])
        checks.append('San Marino whole-file integrity and non-aligned final DMA bytes')
        choose(2);screenshot('liechtenstein',['Liechtenstein','Offline'])
        for _ in range(5):
            tap_map(325,35);until(lambda:state()[8]==0 and state()[13]==0 and not state()[891])
            for zoom in (1,2,3):tap_map(271,35);until(lambda:state()[8]==zoom)
            st=until(lambda:state() if state()[13]==0 and state()[891] else None)
            assert st[884]<=32768 and st[885]<=524288 and sum(st[911:919])==st[901]
            samples.append(dict(total_ms=st[889]/1000,read_ms=st[886]/1000,hash_ms=st[887]/1000,decode_ms=st[888]/1000,pages=st[883],requests=st[901],features=st[884],points=st[885]))
        screenshot('liechtenstein-detail',['Liechtenstein','Zoom 3'])
        checks.append('five bounded larger-region detail loads using final shared I/O policy')
        choose(0)
        from mobile_maps_input import exercise
        exercise(q,data,out/'map-input',browser_regression=False)
        checks.append('native tapping, drag pan, zoom, Fit, labels and app switching under four bounded host workers')
        saved();assert state()[849]==4;type_word('wrongpassphrase');key('ret');screenshot('wrong-maps-passphrase',['Unable to unlock']);assert not state()[850] and not state()[851]
        type_word(MAPS_PASSWORD);key('ret');until(lambda:state()[850] and state()[851]==2);until(lambda:state()[9]>600)
        screenshot('saved-places-recovered',['Saved places','secretcafe42']);lock_maps();assert state()[854]
        checks.append('wrong Maps passphrase denied; existing two encrypted test bookmarks recover; explicit lock clears key and structured plaintext')
        unlock_maps();until(lambda:ui()[1]==14 and not state()[850] and state()[854],85)
        screenshot('device-auto-lock',['DEVICE LOCKED']);old=state()[2];key('a');time.sleep(.2);assert state()[2]==old
        device_unlock();open_maps();assert not state()[850];unlock_maps();screenshot('unlock-recovery',['Saved places','secretcafe42']);lock_maps()
        checks.append('device auto-lock while Maps is open, blocked hidden keyboard input, device unlock and separate Maps reauthentication')
        assert not host_locked()
        result=dict(passed=True,source_commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),loader_sha256=m['loader_sha256'],harness_sha256=hashlib.sha256(source).hexdigest(),saved_data_used=False,pixel_wifi_touched=False,checks=checks,samples=samples,detail_median_ms=statistics.median(s['total_ms'] for s in samples),scope='disposable 4-vCPU mobile UTM, existing public test credentials, actual scanout; no physical Pixel or whole-memory erasure claim')
        (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        try:q.send('input-send-event',dict(events=[{'type':'btn','data':{'button':'left','down':False}}]))
        finally:q.close()
if __name__=='__main__':main()
