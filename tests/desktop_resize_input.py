#!/usr/bin/env python3
"""Inspect a disposable resize VM after a real UTM window/full-screen change.

This does not synthesize a monitor request: UTM/SPICE must produce it. QMP is
used only for bounded readback and ordinary dock/titlebar input verification.
"""
import argparse,json,os,re,shutil,struct,subprocess,time
from pathlib import Path
from utm_vault_key_storage import QmpPipe
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1]


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--label',required=True);p.add_argument('--changed',action='store_true');p.add_argument('--exercise',action='store_true');p.add_argument('--type-search',action='store_true');a=p.parse_args()
    guest=a.guest.resolve();assert guest.is_relative_to(ROOT/'build/desktop-apps')
    assert a.label.replace('-','').isalnum()
    session=json.loads((guest/'session.json').read_text());data=Path(session['bundle'])/'Data'
    deployed=data/'build-manifest.json'
    manifest=json.loads((deployed if deployed.is_file() else guest/'build.json').read_text())
    if session.get('loader_sha256'):assert manifest['loader_sha256']==session['loader_sha256'],'manifest does not describe the running image'
    assert manifest['desktop_resize'] and manifest['gpu_verify'] and not manifest['saved_data_used'] and not session['saved_data_used']
    app_count=4 if manifest.get('maps') else 3
    dock_width=24+84*app_count
    monitor=QmpPipe(data/'monitor')
    monitor.input=os.open(str(data/'monitor.in'),os.O_WRONLY|os.O_NONBLOCK)
    try:monitor.send('query-status') # Usually negotiated by the runner.
    except RuntimeError as error:
        if 'Expecting capabilities negotiation' not in str(error):raise
        monitor.send('qmp_capabilities');monitor.send('query-status')
    path=guest/'resize-observations.json';results=json.loads(path.read_text()) if path.exists() else []
    def memory(address,size):
        f=data/'resize-check.bin';monitor.send('pmemsave',{'val':address,'size':size,'filename':str(f)});b=f.read_bytes();f.unlink();return b
    def stable(address,count):
        b=struct.unpack('<'+'I'*count,memory(address,count*4));after=struct.unpack('<I',memory(address,4))[0]
        return b if b[0] and not b[0]&1 and b[0]==after else None
    def guard():
        text=(data/'trace.log').read_text(errors='replace')
        assert not any(s in text for s in ('DISPLAY_AGENT|FAIL|','DESKTOP_GPU|FAIL|','VMFault','MON|ERROR','panicked at','Data Abort')),text[-4000:]
    def until(fn):
        deadline=time.monotonic()+45
        while time.monotonic()<deadline:
            guard();value=fn()
            if value:return value
            time.sleep(.05)
        raise TimeoutError('resize readiness: '+str(stable(0x5fb92000,6)))
    candidate=None;candidate_since=0.0
    def ready():
        nonlocal candidate,candidate_since
        mode=stable(0x5fb92000,6);request=stable(0x5fb90000,4);ack=stable(0x5fb91000,4);stats=stable(0x5fba0000,13)
        if not all((mode,request,ack,stats)) or not mode[1]:return None
        if a.changed and results and mode[1]==results[-1]['mode'][1]:return None
        if mode[1]!=request[1] or ack[1]!=mode[1] or stats[11]!=mode[1] or tuple(mode[2:4])!=tuple(ack[2:4]) or tuple(mode[2:4])!=tuple(stats[9:11]):return None
        w=min(3840,max(1600,request[2]))&~1;h=min(2160,max(1200,request[3]))&~1
        assert mode[2:]==(w,h,2,min(520,max(360,h//2-220)))
        signature=(mode[1:],request[1:])
        if signature!=candidate:candidate=signature;candidate_since=time.monotonic();return None
        if time.monotonic()-candidate_since<.8:return None
        return mode,request,ack,stats
    try:
        mode,request,ack,stats=until(ready);lw,lh=mode[2]//2,mode[3]//2
        if a.exercise or a.type_search:
            monitor.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
        def state():return stable(0x5fbb4000,4+4*app_count)
        def completed():
            s=stable(0x5fba0000,13);return s[1] if s else 0
        def pointer(x,y,down=None):
            events=[{'type':'abs','data':{'axis':axis,'value':(value*32767+limit-1)//limit}} for axis,value,limit in [('x',x,lw-1),('y',y,lh-1)]]
            if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
            monitor.send('input-send-event',{'events':events})
        def click(x,y):pointer(x,y,True);pointer(x,y,False)
        if a.exercise:
            for owner in (*range(1,app_count+1),1):
                click((lw-dock_width)//2+54+84*(owner-1),lh-65)
                s=until(lambda:(s:=state()) and s[2]==owner and s)
                until(lambda:completed()>=s[0])
            s=until(state);x,y=s[4:6];dx=-16 if x>40 else 16
            pointer(x+180,y-24,True);pointer(x+180+dx,y-24)
            moved=until(lambda:(s:=state()) and s[4]==x+dx and s)
            until(lambda:completed()>=moved[0]);pointer(x+180+dx,y-24,False)
        if a.type_search:
            s=until(state);assert s[2]==1,'focus SimpleX via --exercise first'
            x,y=s[4:6];click(x+70,y+95)
            def key(name,down):return {'type':'key','data':{'down':down,'key':{'type':'qcode','data':name}}}
            monitor.send('input-send-event',{'events':[key('ctrl',True),key('a',True),key('a',False),key('ctrl',False),key('backspace',True),key('backspace',False)]})
            for letter in 'resizetest':
                monitor.send('input-send-event',{'events':[key(letter,True),key(letter,False)]});time.sleep(.04)
            time.sleep(.5)
        s=until(state)
        for i in range(app_count):
            x,y,opened,visible=s[4+4*i:8+4*i]
            if visible:assert 12<=x<=lw-572 and 92<=y<=lh-mode[5]-128,(i,s,mode)
        frame=data/('resize-'+a.label+'.ppm');capture(monitor,frame,True)
        raw=frame.read_bytes();header=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',raw)
        assert header and tuple(map(int,header.groups()))==tuple(mode[2:4])
        rgb=raw[header.end():];w,h=mode[2:4]
        def pixel(x,y):return tuple(rgb[(y*w+x)*3:(y*w+x)*3+3])
        # Independent layout checks: comparing the GPU with a corrupt shell
        # surface alone cannot catch stale stride/layout after a resize.
        for x in range(2,w,31):
            r,g,b=pixel(x,10);assert r<45 and g<70 and b<90,('top bar stale',x,pixel(x,10))
        for y in (h-215,h-40):
            r,g,b=pixel(w//2,y);assert 5<=r<40 and 20<=g<75 and 30<=b<100,('dock must follow desktop height',pixel(w//2,y),mode)
        assert sum(pixel(x,h-2)==(0,0,0) for x in range(0,w,13))<w//130, 'new framebuffer rows were not painted'
        shutil.copyfile(frame,guest/frame.name)
        subprocess.run(['magick',str(frame),str((guest/frame.name).with_suffix('.png'))],check=True)
        result={'label':a.label,'mode':mode,'request':request,'ack':ack,'stats':stats,'scene':s,'search_typing_exercised':a.type_search,'independent_layout_pixels_checked':True,'dock_and_held_drag_checked':a.exercise,'saved_data_used':False,
                'qmp_input_injected':a.exercise or a.type_search,
                'scope':'Real UTM SPICE monitor request -> manager bounded layout -> successful GPU scanout and flush; diagnostic CPU-reference pixels. Host display checked separately.'}
        results.append(result);path.write_text(json.dumps(results,indent=2)+'\n')
        print(json.dumps(result),flush=True)
    finally:
        try:
            if a.exercise or a.type_search:
                monitor.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
        finally:monitor.close()


if __name__=='__main__':main()
