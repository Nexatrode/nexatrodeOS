"""Observe input-to-changed-scanout bounds through QMP, including capture overhead."""
import json
import statistics
import time

def measure(monitor, temporary, evidence, scale):
    width, height = 1280*scale, 800*scale
    captures=[]
    def capture():
        start=time.monotonic_ns()
        path=temporary/'latency.ppm'
        monitor.send('screendump',{'filename':str(path)})
        data=path.read_bytes(); header=data.split(b'\n',3)
        assert header[:3]==[b'P6',f'{width} {height}'.encode(),b'255']
        pixels=header[3]; assert len(pixels)==width*height*3
        # First transcript line; the pointer is kept outside this region.
        roi=b''.join(pixels[((y*width+158*scale)*3):((y*width+458*scale)*3)] for y in range(260*scale,300*scale))
        captures.append((time.monotonic_ns()-start)/1e6)
        return roi
    def pointer(x,y,down=None):
        events=[{'type':'abs','data':{'axis':'x','value':round(x*32767/1279)}},{'type':'abs','data':{'axis':'y','value':round(y*32767/799)}}]
        if down is not None: events.append({'type':'btn','data':{'button':'left','down':down}})
        monitor.send('input-send-event',{'events':events})
    def click(x,y): pointer(x,y,True); pointer(x,y,False)
    def character():
        monitor.send('input-send-event',{'events':[{'type':'key','data':{'down':v,'key':{'type':'qcode','data':'x'}}} for v in (True,False)]})
    def sample(action):
        before=capture(); start=time.monotonic_ns(); action()
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            after=capture()
            # A blinking insertion cursor alone cannot satisfy this threshold.
            if sum(a!=b for a,b in zip(before,after)) >= 120*scale*scale:
                return (time.monotonic_ns()-start)/1e6
        raise RuntimeError('no visible desktop change during latency sample')
    click(580,275); pointer(10,50); time.sleep(.5)
    for _ in range(2): sample(character)
    typing=[sample(character) for _ in range(12)]
    minimized=[]; restored=[]
    for _ in range(12):
        minimized.append(sample(lambda:click(608,124)))
        restored.append(sample(lambda:click(596,728)))
    def summary(values):
        values=sorted(values)
        return {'samples':len(values),'median_ms':statistics.median(values),'p95_ms':values[min(len(values)-1, int(.95*len(values)))],'max_ms':max(values),'raw_ms':values}
    result={'scale':scale,'physical_resolution':[width,height],'typing':summary(typing),'minimize':summary(minimized),'restore':summary(restored),'capture_overhead':summary(captures),'scope':'QMP input submission to first changed guest scanout capture; includes QMP and capture/file overhead; not host monitor/photon timing','host_load':'uncontrolled current Mac workload','warmup_characters':2}
    (evidence/'responsiveness.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS scanout responsiveness samples:', json.dumps({k:result[k]['median_ms'] for k in ('typing','minimize','restore')}))
