"""Measure coherent guest UI frame publication without full scanout readback."""
import json
import statistics
import re
import struct
import time

def measure(monitor, temporary, evidence, scale):
    width, height = 1280*scale, 800*scale
    captures=[]
    pages = sorted((int(i),int(addr,16)) for i,addr in re.findall(r"Page\(2 MiB\): 'frame_mr_slint_frame_(\d+)' @ (0x[0-9a-fA-F]+)",(evidence/'report.txt').read_text()))
    assert len(pages) == (8 if scale == 2 else 4)
    base = pages[0][1]
    assert all(address == base + index * 0x200000 for index,address in pages)
    def memory(address,size,name):
        assert base <= address and address + size <= base + len(pages)*0x200000
        path=temporary/name
        monitor.send('pmemsave',{'val':address,'size':size,'filename':str(path)})
        data=path.read_bytes(); assert len(data)==size
        return data
    def capture():
        start=time.monotonic_ns()
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            header=struct.unpack('<7I',memory(base,28,'frame-header.bin'))
            revision,w,h,stride,fmt,_,_=header
            if not revision or revision&1: continue
            assert (w,h,stride,fmt)==(width,height,width*3,1)
            left,top,rw,rh=158*scale,260*scale,470*scale,40*scale
            address=base+28+top*stride+left*3
            size=(rh-1)*stride+rw*3
            data=memory(address,size,'frame-region.bin')
            after=struct.unpack('<7I',memory(base,28,'frame-header-after.bin'))
            if header[0]!=after[0] or after[0]&1:continue
            roi=b''.join(data[y*stride:y*stride+rw*3] for y in range(rh))
            captures.append((time.monotonic_ns()-start)/1e6)
            return roi
        raise RuntimeError('guest frame did not produce a coherent readback')
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
    result={'scale':scale,'physical_resolution':[width,height],'typing':summary(typing),'minimize':summary(minimized),'restore':summary(restored),'capture_overhead':summary(captures),'scope':'QMP input submission to coherent completed guest UI frame region; includes three bounded memory readbacks and file overhead. GPU transfer and host display are excluded; full scanout checked separately.','host_load':'uncontrolled current Mac workload','warmup_characters':2}
    (evidence/'frame-publication.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS coherent UI publication samples:', json.dumps({k:result[k]['median_ms'] for k in ('typing','minimize','restore')}))
