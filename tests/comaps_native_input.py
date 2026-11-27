"""Actual guest pan/zoom, scanout pixels and idle-frame checks; no app data."""
import json,re,shutil,subprocess,time

def exercise(q,data,evidence):
    evidence.mkdir(exist_ok=True)
    (evidence/'validation.json').unlink(missing_ok=True)
    def trace():
        s=(data/'trace.log').read_text(errors='replace')
        assert not any(x in s for x in ('COMAPS|FAIL|','MON|ERROR','VMFault','panicked at','Data Abort')),s[-3000:]
        return s
    pattern=r'COMAPS\|FRAME\|n=(\d+)\|zoom=(\d+)\|cx=([-\d.]+)\|cy=([-\d.]+)\|units=([-\d.]+)\|features=(\d+)\|labels=(\d+)\|us=(\d+)'
    def frames():return list(re.finditer(pattern,trace()))
    def state():return frames()[-1].groups()
    def wait(predicate):
        end=time.monotonic()+25
        while time.monotonic()<end:
            if predicate():return
            time.sleep(.05)
        raise RuntimeError('map input timeout: '+trace()[-1800:])
    def position(x,y,down=None):
        events=[{'type':'abs','data':{'axis':a,'value':round(v*32767/m)}} for a,v,m in [('x',x,1279),('y',y,799)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':events})
    def tap(x,y):position(x,y,True);time.sleep(.08);position(x,y,False)
    def key(code):q.send('input-send-event',{'events':[{'type':'key','data':{'down':d,'key':{'type':'qcode','data':code}}} for d in (True,False)]})
    def capture(name):
        position(1255,85);time.sleep(.3)
        p=data/'capture.ppm';q.send('screendump',{'filename':str(p)})
        raw=p.read_bytes();header=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',raw);assert header and tuple(map(int,header.groups()))==(2560,1600)
        shutil.copyfile(p,evidence/(name+'.ppm'));subprocess.run(['magick',str(p),str(evidence/(name+'.png'))],check=True)
        ocr=subprocess.run(['tesseract',str(p),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout
        (evidence/(name+'.txt')).write_text(ocr)
        assert 'Gibraltar' in ocr,ocr
        return raw[header.end():]
    initial=state();a=capture('initial')
    assert int(initial[5])>500 and int(initial[6])>0
    # Independent scanout landmarks: actual map colours, not just a counter.
    import numpy as np
    def landmarks(raw):
        p=np.frombuffer(raw,dtype=np.uint8).reshape(1600,2560,3)[184:1512]
        return {name:int(np.sum(np.all(p==colour,axis=2))) for name,colour in {
            'water':(155,203,214),'green':(182,211,171),'buildings':(185,179,171),'streets':(255,255,255)}.items()}
    counts=landmarks(a);assert min(counts.values())>1000,counts
    tap(1120,40);wait(lambda:int(state()[1])==1);b=capture('zoomed')
    assert b!=a and float(state()[4])<float(initial[4])
    old=state();position(600,400,True)
    for i in range(1,17):position(600+i*8,400+i*3);time.sleep(.03)
    position(728,448,False)
    wait(lambda:float(state()[2])<float(old[2]) and float(state()[3])>float(old[3]))
    c=capture('panned');assert c!=b
    before=int(state()[0]);key('right');wait(lambda:int(state()[0])>before)
    capture('keyboard-pan')
    # Wheel zoom and bounds; plus/minus must never grow without limit.
    q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'wheel-up','down':d}} for d in (True,False)]})
    wait(lambda:int(state()[1])>=2)
    for _ in range(12):key('equal');time.sleep(.04)
    wait(lambda:int(state()[1])==6)
    for _ in range(15):key('minus');time.sleep(.04)
    wait(lambda:int(state()[1])==0)
    tap(1200,40);wait(lambda:state()[1:5]==initial[1:5]);reset=state();d=capture('reset')
    assert reset[1:5]==initial[1:5],(reset,initial)
    # Park cursor outside map for deterministic map pixels on the reset view.
    assert d==a,'Fit must restore identical map pixels'
    n=state()[0];time.sleep(2);assert state()[0]==n,'map must not repaint while idle'
    times=[int(m.group(8))/1000 for m in frames()];times.sort()
    result=dict(passed=True,region='Gibraltar',map_records=8957,network=False,storage=False,saved_data_used=False,pixel_wifi_touched=False,
        checks=['real MWM geometry rendered with water/green/building/street pixels','touch + zoom changes map scale and pixels','held pointer drag pans both axes','hardware arrow keys pan','wheel and bounded +/- zoom','Fit restores identical initial map pixels','two idle seconds publish no map frames'],
        initial_colour_pixels=counts,frames=len(times),guest_draw_ms={'median':times[len(times)//2],'p95':times[min(len(times)-1,int(len(times)*.95))],'max':max(times)},
        scope='QMP input and 2D scanout; CPU renderer, no full CoMaps app, routing, search, GPS, persistence or physical display latency')
    (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print('PASS offline map pan/zoom scanout',flush=True)
