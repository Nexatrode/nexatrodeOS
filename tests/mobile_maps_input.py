"""Real phone scanout, touch gestures, measured labels and private app routing."""
import json,re,shutil,struct,subprocess,time
SCALE=2.6086957
def exercise(q,data,evidence):
    evidence.mkdir(parents=True,exist_ok=True);checks=[];label_frames=[]
    def guard():
        s=(data/'trace.log').read_text(errors='replace')
        assert not any(x in s for x in ('COMAPS|FAIL|','VMFault','panicked at','MON|ERROR','Data Abort')),s[-3500:]
    def read(address,size):
        p=data/'mobile-maps-state.bin'
        try:q.send('pmemsave',{'val':address,'size':size,'filename':str(p)});return p.read_bytes()
        finally:p.unlink(missing_ok=True)
    def ui():return struct.unpack('<13Q',read(0x5fb92000,104))
    def state():
        for _ in range(200):
            raw=read(0x5fb96000,4096);s=struct.unpack('<512Q',raw)
            if s[0] and not s[0]&1 and s[0]==struct.unpack('<Q',read(0x5fb96000,8))[0]:return s,raw
            time.sleep(.002)
        raise TimeoutError('map observation remained unstable')
    def until(fn,seconds=50):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            guard();v=fn()
            if v:return v
            time.sleep(.04)
        raise RuntimeError('phone Maps timeout: '+str(ui()[:3])+' '+str(state()[0][:11]))
    def position(x,y,down=None):
        e=[{'type':'abs','data':{'axis':a,'value':round(v*32767/m)}} for a,v,m in [('x',x,1079),('y',y,2339)]]
        if down is not None:e.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':e})
    def tap_p(x,y):position(x,y,True);time.sleep(.08);position(x,y,False);time.sleep(.1)
    def tap(x,y):tap_p(x*SCALE,y*SCALE)
    def key(name):q.send('input-send-event',{'events':[{'type':'key','data':{'down':d,'key':{'type':'qcode','data':name}}} for d in (True,False)]})
    def screen(name,words):
        position(1045,65);end=time.monotonic()+40
        while time.monotonic()<end:
            p=data/'maps-phone.ppm';q.send('screendump',{'filename':str(p)})
            raw=p.read_bytes();header=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',raw)
            assert header and tuple(map(int,header.groups()))==(1080,2340)
            text=subprocess.run(['tesseract',str(p),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=25).stdout
            if 'HOME' in words:
                # Sparse full-page OCR misses the small cyan HOME beside a
                # large map. Check that trusted header independently too.
                header_image=data/'maps-phone-header.png'
                subprocess.run(['magick',str(p),'-crop','1080x110+0+100','-colorspace','Gray','-negate','-threshold','50%',str(header_image)],check=True)
                text+='\n'+subprocess.run(['tesseract',str(header_image),'stdout','--psm','6'],capture_output=True,text=True,check=True,timeout=25).stdout
            compact=lambda s:re.sub('[^a-z0-9]','',s.lower())
            if all(compact(w) in compact(text) for w in words):
                shutil.copyfile(p,evidence/(name+'.ppm'));subprocess.run(['magick',str(p),str(evidence/(name+'.png'))],check=True)
                (evidence/(name+'.txt')).write_text(text);return raw[header.end():]
            time.sleep(.15)
        raise AssertionError('phone screen missing '+str(words)+': '+text)
    def labels(name):
        s,raw=state();n=s[10];assert 0<n<=24
        rect=[struct.unpack_from('<4I',raw,464*8+i*16) for i in range(n)]
        for i,(l,t,r,b) in enumerate(rect):
            assert 0<=l<r<=1080 and 64*3<=t<b<=(s[9]-52)*3,(name,i,rect[i],s[9])
            for ll,tt,rr,bb in rect[:i]:assert not (l<rr and ll<r and t<bb and tt<b),(name,rect[i],(ll,tt,rr,bb))
        label_frames.append(dict(name=name,frame=s[3],zoom=s[8],rectangles=rect));return s
    def home():
        page=ui()[1]
        if page in (18,19):tap(370,60)
        elif page==16:tap(40,65)
        elif page!=0:tap(350,136)
        until(lambda:ui()[1]==0);until(lambda:state()[0][4]==0)
    def open_maps():
        tap(300,360);until(lambda:ui()[1]==19 and state()[0][4]==1 and state()[0][3]>0)
    until(lambda:ui()[0]==0x4e584445534b3031 and state()[0][1]==0x4e584d4150303031)
    home();screen('maps-home',['SIMPLEX','NETSURF','NEXAWAL','MAPS','TOOLS','SETTINGS'])
    open_maps();raw=screen('maps-phone-open',['MAPS','HOME','Gibraltar','CoMaps','Zoom 0'])
    import numpy as np
    pixels=np.frombuffer(raw,dtype=np.uint8).reshape(2340,1080,3)[450:2150]
    counts={n:int(np.sum(np.all(pixels==c,axis=2))) for n,c in {'water':(155,203,214),'green':(182,211,171),'building':(185,179,171),'street':(255,255,255)}.items()}
    assert min(counts.values())>1000,counts
    initial=labels('overview');body_y=80*SCALE
    def map_tap(x,y):tap_p(x*3,body_y+y*3)
    map_tap(270,35);until(lambda:state()[0][8]==1)
    screen('maps-phone-zoom',['Gibraltar','Zoom 1']);labels('zoom')
    old=state()[0];position(160*3,body_y+240*3,True)
    for i in range(1,13):position((160+6*i)*3,body_y+(240+3*i)*3);time.sleep(.04)
    position(232*3,body_y+276*3,False)
    until(lambda:state()[0][5]!=old[5] and state()[0][6]!=old[6])
    screen('maps-phone-pan',['Gibraltar']);labels('pan')
    map_tap(325,35);until(lambda:state()[0][8]==0 and state()[0][5:8]==initial[5:8]);labels('fit')
    checks.append('phone Home tile, real map colours, touch zoom/drag/Fit and measured nonoverlapping labels')
    home();old=state()[0][2:4];key('right');key('equal');time.sleep(.4);assert state()[0][2:4]==old
    for page,x,y,words in [(6,80,190,['SIMPLEX']),(16,80,360,['Wallet']),(18,300,190,['Explore the web']),(10,80,550,['TOOLS']),(8,300,550,['SETTINGS'])]:
        tap(x,y);until(lambda:ui()[1]==page);screen('maps-navigation-'+str(page),words);home()
    open_maps();until(lambda:state()[0][5:9]==initial[5:9]);screen('maps-phone-restored',['Gibraltar','Zoom 0'])
    checks.append('hidden Maps receives no keyboard events or redraws; Home/SimpleX/NexaWal/NetSurf/Tools/Settings navigation and map state recovery')
    start=state()[0][3];samples={};workers=[]
    try:
        workers=[subprocess.Popen(['python3','-c','import time\ne=time.monotonic()+30\nx=1\nwhile time.monotonic()<e: x=(x*1664525+1013904223)&0xffffffff'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL) for _ in range(4)]
        for i in range(12):
            old=state()[0][3];key('right' if i%2==0 else 'left');until(lambda:state()[0][3]>old)
            s=state()[0]
            for slot in range(32):
                r=s[16+slot*14:30+slot*14]
                if r[0]>start:samples[r[0]]=list(r)
    finally:
        for p in workers:
            if p.poll() is None:p.terminate()
        for p in workers:p.wait(timeout=10)
    assert len(samples)>=10
    timings=sorted((r[8]-r[2])/1000 for r in samples.values())
    map_tap(325,35);until(lambda:state()[0][5:9]==initial[5:9]);labels('final')
    screen('maps-phone-final',['Gibraltar','CoMaps','Zoom 0'])
    # The new launcher must also preserve the existing phone browser behavior.
    home();browser_evidence=evidence/'browser-regression';browser_evidence.mkdir(exist_ok=True)
    from mobile_browser_input import exercise as browser_exercise
    browser_exercise(q,data,browser_evidence)
    open_maps();screen('maps-phone-final',['MAPS','HOME','Gibraltar','CoMaps','Zoom 0'])
    checks.append('existing nine phone browser scenario groups pass in the same four-app assembly')
    manifest=json.loads((data/'build-manifest.json').read_text())
    result=dict(passed=True,loader_sha256=manifest['loader_sha256'],checks=checks,browser_regression_passed=True,saved_data_used=False,pixel_wifi_touched=False,viewport=[1080,2340],map_scale=3,map_native_height=state()[0][9],colour_counts=counts,label_frames=label_frames,host_cpu_workers=4,
        input_handler_to_publication_ms=dict(median=timings[len(timings)//2],p95=timings[min(len(timings)-1,int(len(timings)*.95))],maximum=max(timings)),samples=list(samples.values()),scope='disposable phone map integration and guest timing; QMP input and real scanout, no physical touch latency, GPS, search, routing or shared wrapped-session lock guarantee')
    (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ('samples','label_frames')},indent=2),flush=True)
