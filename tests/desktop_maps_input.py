"""Four-app private input, actual GPU pixels and map-phase profiling in disposable UTM."""
import hashlib,json,re,shutil,struct,subprocess,time
from pathlib import Path
from desktop_gpu_capture import capture


def exercise(q, data, evidence, *, gpu=True, functional=True, load=False):
    evidence.mkdir(parents=True,exist_ok=True)
    def read(address,size):
        p=data/'maps-state.bin'
        try:q.send('pmemsave',{'val':address,'size':size,'filename':str(p)});return p.read_bytes()
        finally:p.unlink(missing_ok=True)
    def check():
        s=(data/'trace.log').read_text(errors='replace')
        assert not any(x in s for x in ('COMAPS|FAIL|','DESKTOP_GPU|FAIL|','MON|ERROR','VMFault','panicked at','Data Abort')),s[-4000:]
    def until(fn,seconds=45):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            check();value=fn()
            if value:return value
            time.sleep(.05)
        raise RuntimeError('four-app map timeout: '+str(scene())+' '+str(map_state()[:10]))
    def scene():
        for _ in range(200):
            a=struct.unpack('<20I',read(0x5fbb4000,80));b=struct.unpack('<I',read(0x5fbb4000,4))[0]
            if a[0] and not a[0]&1 and a[0]==b:return a
            time.sleep(.002)
        raise TimeoutError('no stable four-window state')
    def mode():return struct.unpack('<6I',read(0x5fb92000,24))
    def map_state():
        for _ in range(200):
            a=struct.unpack('<512Q',read(0x5fb96000,4096));b=struct.unpack('<Q',read(0x5fb96000,8))[0]
            if a[0] and not a[0]&1 and a[0]==b:return a
            time.sleep(.002)
        raise TimeoutError('no stable map profile state')
    def window(i):s=scene();return s[4+(i-1)*4:8+(i-1)*4]
    def pointer(x,y,down=None):
        m=mode();w,h=m[2]//2,m[3]//2
        events=[{'type':'abs','data':{'axis':a,'value':round(v*32767/maximum)}} for a,v,maximum in [('x',x,w-1),('y',y,h-1)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':events})
    def tap(x,y):pointer(x,y,True);time.sleep(.06);pointer(x,y,False)
    def key(code):q.send('input-send-event',{'events':[{'type':'key','data':{'down':v,'key':{'type':'qcode','data':code}}} for v in (True,False)]})
    def dock(i):
        m=mode();w,h=m[2]//2,m[3]//2
        tap((w-360)//2+16+(i-1)*84+38,h-65)
        until(lambda:scene()[2]==i and window(i)[3]==1)
        if i==4:until(lambda:map_state() and map_state()[4]==1 and map_state()[3]>0)
        else:until(lambda:map_state() and map_state()[4]==0)
    def stable():
        # The same scene generation must have completed GPU presentation.
        # No ELF/private map parsing: structured GPU observation reports scene.
        target=scene()[0]
        def done():
            a=struct.unpack('<32I',read(0x5fba0000,128))
            return a[0] and not a[0]&1 and a[1]==target and a[2]>0 and a[21]==0
        until(done)
    def settled():
        candidate=None;since=time.monotonic()
        def ready():
            nonlocal candidate,since
            m=mode();s=struct.unpack('<32I',read(0x5fba0000,128))
            if not m[1] or not s[0] or s[0]&1 or tuple(s[9:11])!=tuple(m[2:4]) or s[11]!=m[1]:return False
            if m!=candidate:candidate=m;since=time.monotonic();return False
            return time.monotonic()-since>=.8
        until(ready)
    def screen(name,words,held=False):
        if not held:pointer(mode()[2]//2-50,50)
        stable();time.sleep(.15)
        end=time.monotonic()+35
        while time.monotonic()<end:
            p=data/'maps-check.ppm';capture(q,p,gpu)
            raw=p.read_bytes();head=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',raw);assert head
            text=subprocess.run(['tesseract',str(p),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout
            compact=lambda s:re.sub('[^a-z0-9]','',s.lower())
            if all(compact(w) in compact(text) for w in words):
                shutil.copyfile(p,evidence/(name+'.ppm'));subprocess.run(['magick',str(p),str(evidence/(name+'.png'))],check=True)
                (evidence/(name+'.txt')).write_text(text);return raw,head.end()
            time.sleep(.2)
        raise RuntimeError('four-app pixels missing '+str(words)+': '+text)
    def map_tap(x,y):a=window(4);tap(a[0]+x,a[1]+y)
    until(lambda:map_state() and map_state()[1]==0x4e584d4150303031)
    settled();dock(4);stable();initial=map_state()
    if functional:
        raw,offset=screen('maps-open',['Gibraltar','CoMaps','Maps'])
        import numpy as np
        captured=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',raw)
        p=np.frombuffer(raw[offset:],dtype=np.uint8).reshape(int(captured[2]),int(captured[1]),3)
        x,y,_,_=window(4);region=p[(y+64)*2:(y+mode()[5]-36)*2,x*2:(x+560)*2]
        counts={n:int(np.sum(np.all(region==c,axis=2))) for n,c in {'water':(155,203,214),'green':(182,211,171),'building':(185,179,171),'street':(255,255,255)}.items()}
        assert min(counts.values())>200,counts
        initial=map_state()
        map_tap(470,35);until(lambda:map_state()[8]==1);screen('maps-zoomed',['Gibraltar','Zoom 1'])
        before=map_state();a=window(4);pointer(a[0]+200,a[1]+220,True)
        for i in range(1,13):pointer(a[0]+200+8*i,a[1]+220+2*i);time.sleep(.03)
        pointer(a[0]+296,a[1]+244,False)
        until(lambda:map_state()[5]!=before[5] and map_state()[6]!=before[6]);screen('maps-panned',['Gibraltar'])
        # Hidden Maps receives neither browser typing nor wallet shortcuts.
        dock(3);screen('browser-opens',['NetSurf','Explore'])
        old=map_state()[2:4];key('equal');key('right');time.sleep(.4);assert map_state()[2:4]==old
        dock(2);screen('wallet-opens',['Wallet'])
        dock(1);screen('simplex-opens',['SimpleX'])
        dock(4);map_tap(525,35);until(lambda:map_state()[8]==0 and map_state()[5:8]==initial[5:8]);screen('maps-restored',['Gibraltar'])
        # Minimize stops map work; restore retains this public map state.
        a=window(4);tap(a[0]+490,a[1]-22);until(lambda:window(4)[3]==0 and map_state()[4]==0)
        old=map_state()[3];time.sleep(.5);assert map_state()[3]==old
        dock(4);screen('four-app-desktop',['Gibraltar','SimpleX','NexaWal','NetSurf','Maps'])
        # Move the retained window while held; app content must not rerasterize.
        a=window(4);old=map_state()[3];dx=-24 if a[0]>48 else 24
        pointer(a[0]+180,a[1]-22,True);pointer(a[0]+180+dx,a[1]-22)
        until(lambda:window(4)[0]==a[0]+dx);stable()
        screen('maps-held-drag',['Gibraltar','CoMaps','Maps'],held=True)
        pointer(a[0]+180+dx,a[1]-22,False);assert map_state()[3]==old
    def presented_map(frame):
        a=struct.unpack('<1024I',read(0x5fba0000,4096));b=struct.unpack('<I',read(0x5fba0000,4))[0]
        return a[0]==b and not a[0]&1 and a[992]==0x54455831 and a[993]==9 and a[998]>=frame*2
    samples={};completion=[];switches=[]
    def history():
        s=map_state()
        for i in range(32):
            r=s[16+i*14:30+i*14]
            if r[0]:samples[r[0]]=r
        return s
    dock(4);test_mode=mode();start=history()[3];workers=[]
    try:
        if load:
            workers=[subprocess.Popen(['python3','-c','import time\ne=time.monotonic()+25\nx=1\nwhile time.monotonic()<e: x=(x*1664525+1013904223)&0xffffffff'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL) for _ in range(4)]
        # Each key is acknowledged in app state, making queue delay measurable.
        for i in range(16):
            old=history()[3];sent_at=time.monotonic();key('right' if i%2==0 else 'left');until(lambda:history()[3]>old);target=history()[3]
            until(lambda:presented_map(target));completion.append((time.monotonic()-sent_at)*1000)
        history();stable()
        for owner in (1,2,3,4)*3:
            sent_at=time.monotonic();dock(owner);stable()
            switches.append((time.monotonic()-sent_at)*1000)
        assert mode()==test_mode,'display changed during timed pan sequence'
    finally:
        for p in workers:
            if p.poll() is None:p.terminate()
        for p in workers:p.wait(timeout=10)
    held=[r for k,r in sorted(samples.items()) if k>start]
    assert len(held)>=12
    timings={name:[(r[b]-r[a])/1000 for r in held] for name,a,b in [('input_to_draw',2,3),('areas',3,5),('lines',5,6),('labels_chrome',6,7),('drawing',3,7),('notify',7,8),('input_to_publication',2,8)]}
    def stats(v):v=sorted(v);return dict(median=v[len(v)//2],p95=v[min(len(v)-1,int(len(v)*.95))],max=max(v))
    manifest=json.loads((data/'build-manifest.json').read_text())
    result=dict(passed=True,loader_sha256=manifest['loader_sha256'],mode=list(test_mode),apps=['SimpleX','NexaWal','NetSurf','Maps'],saved_data_used=False,pixel_wifi_touched=False,gpu_readback=gpu,functional=functional,host_cpu_workers=4 if load else 0,
        checks=['map pixels render through GPU composition','private input/focus and all four app switches','map pan/zoom, retained minimize/restore and held dragging','bounded map phase profiling and four-app switches under host load'] if functional else ['normal compositor map timing and four-app switches under host load'],
        profile_ms={n:stats(v) for n,v in timings.items()},host_injection_to_observed_gpu_completion_ms=stats(completion),host_injection_to_observed_switch_completion_ms=stats(switches),samples=[list(r) for r in held],scope='guest map handler/drawing/publication, not physical touch latency; QMP observations include polling/host scheduling; diagnostic readback timings are separate from normal composition')
    (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='samples'},indent=2),flush=True)
