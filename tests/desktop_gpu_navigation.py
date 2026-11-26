"""Bounded input-to-GPU-completion checks in a disposable three-app VM."""
import json
import statistics
import struct
import subprocess
import time
from desktop_gpu_capture import capture


def exercise(monitor, trace, temporary, evidence, verify):
    def memory(address,size):
        path=temporary/'navigation-state.bin'
        monitor.send('pmemsave',{'val':address,'size':size,'filename':str(path)})
        raw=path.read_bytes();path.unlink();return raw
    def state():return struct.unpack('<16I',memory(0x5fbb4000,64))
    def stats():
        raw=struct.unpack('<9I',memory(0x5fba0000,36))
        after=struct.unpack('<I',memory(0x5fba0000,4))[0]
        return raw if raw[0] and not raw[0]&1 and raw[0]==after else None
    def completed():
        s=stats();return s[1] if s else 0
    def output():
        text=trace.read_text(errors='replace')
        assert not any(s in text for s in ('DESKTOP_GPU|FAIL|','VMFault','MON|ERROR','panicked at')),text[-3000:]
    def wait(predicate):
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            output();v=predicate()
            if v:return v
            time.sleep(.01)
        raise TimeoutError('desktop GPU navigation completion')
    def pointer(x,y,down=None):
        events=[{'type':'abs','data':{'axis':a,'value':round(v*32767/m)}} for a,v,m in [('x',x,1279),('y',y,799)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        monitor.send('input-send-event',{'events':events})
    def click(x,y):pointer(x,y,True);pointer(x,y,False)
    def settle():
        s=wait(lambda:(s:=state()) and s[0] and not s[0]&1 and s)
        wait(lambda:completed()>=s[0]);return s
    settle()
    move_samples=[];switch_samples=[];held_frames=[];backdrop_revisions=[]
    # The SimpleX titlebar is unobscured in the initial layout.
    for i in range(12):
        s=settle();x,y=s[4],s[5];dx=100 if i%2==0 else -100
        pointer(x+200,y-24,True)
        start=time.monotonic();pointer(x+200+dx,y-24)
        moved=wait(lambda:(n:=state()) and not n[0]&1 and n[4]==x+dx and n)
        wait(lambda:completed()>=moved[0]);move_samples.append((time.monotonic()-start)*1000)
        current=wait(stats);held_frames.append(current[2]);backdrop_revisions.append(current[5])
        if verify and i==0:capture(monitor,temporary/'gpu-held.ppm',True);subprocess.run(['magick',str(temporary/'gpu-held.ppm'),str(evidence/'gpu-held.png')],check=True)
        pointer(x+200+dx,y-24,False)
    if verify:capture(monitor,temporary/'gpu-drag.ppm',True);subprocess.run(['magick',str(temporary/'gpu-drag.ppm'),str(evidence/'gpu-drag.png')],check=True)
    for _ in range(4):
        for owner,x in ((1,556),(2,640),(3,724)):
            start=time.monotonic();click(x,728)
            switched=wait(lambda:(n:=state()) and not n[0]&1 and n[2]==owner and n[3+owner*4] and n)
            wait(lambda:completed()>=switched[0]);switch_samples.append((time.monotonic()-start)*1000)
    if verify:capture(monitor,temporary/'gpu-three-windows.ppm',True);subprocess.run(['magick',str(temporary/'gpu-three-windows.ppm'),str(evidence/'gpu-three-windows.png')],check=True)
    # Leave SimpleX and NexaWal unobscured, browser available from its dock.
    click(340+490,152-22);settle()
    def summary(values):return {'median_ms':statistics.median(values),'p95_ms':sorted(values)[-1],'samples_ms':values}
    assert len(set(held_frames))==12,'each movement must present while the mouse is held'
    assert len(set(backdrop_revisions))<=3,'geometry changes repeatedly repainted the backdrop'
    result={'backdrop_revisions_during_drag':backdrop_revisions,'held_completed_frames':held_frames,'drag':summary(move_samples),'app_switch':summary(switch_samples),'gpu_verify':verify,'saved_data_used':False,
            'scope':'QMP input through manager geometry/focus change to successful GPU SET_SCANOUT and RESOURCE_FLUSH completion; includes polling/file overhead; excludes host display latency. Diagnostic builds also include readback and CPU-reference verification.'}
    (evidence/'gpu-navigation.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS GPU drag and three-app dock switching:',json.dumps({k:result[k]['median_ms'] for k in ('drag','app_switch')}),flush=True)
