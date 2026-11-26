#!/usr/bin/env python3
"""Damage and decoration checks in an explicitly disposable 2x UTM desktop.

Input-to-completion times include QMP polling; diagnostic readback adds overhead.
They are not physical input/display latency or host-display FPS measurements.
"""
import argparse,hashlib,json,os,struct,time,statistics
from pathlib import Path
from desktop_gpu_motion import MotionQmp
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1]

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--label',required=True);a=p.parse_args()
    assert a.label.replace('-','').isalnum();guest=a.guest.resolve();assert guest.is_relative_to(ROOT/'build/desktop-apps')
    session=json.loads((guest/'session.json').read_text());assert session['saved_data_used'] is False
    data=Path(session['bundle'])/'Data';assert data.parent.parent==guest
    m=json.loads((data/'build-manifest.json').read_text());assert m['development_storage_key'] and m['desktop_resize']
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK);q.send('query-status')
    def read(address,n,fmt='I'):
        f=data/'damage-state.bin';q.send('pmemsave',{'val':address,'size':n*struct.calcsize(fmt),'filename':str(f)})
        b=f.read_bytes();f.unlink();return struct.unpack('<'+fmt*n,b)
    def stable(address,n):
        r=read(address,n);return r if r[0] and not r[0]&1 and r[0]==read(address,1)[0] else None
    def guard():
        text=(data/'trace.log').read_text(errors='replace')
        assert not any(bad in text for bad in ('DESKTOP_GPU|FAIL|','DISPLAY_AGENT|FAIL|','VMFault','MON|ERROR','panicked at')),text[-2000:]
    def wait(fn):
        end=time.monotonic()+45
        while time.monotonic()<end:
            guard();r=fn()
            if r:return r
            time.sleep(.01)
        raise TimeoutError('damage/focus completion: '+str(scene())+' '+str(gpu()))
    mode=None
    def pointer(x,y,down=None):
        events=[{'type':'abs','data':{'axis':axis,'value':(round(v)*32767+limit-1)//limit}} for axis,v,limit in (('x',x,w-1),('y',y,h-1))]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':events})
    def click(x,y):pointer(x,y,True);pointer(x,y,False)
    def scene():return stable(0x5fbb4000,16)
    def gpu():return stable(0x5fba0000,32)
    def settled():
        s=scene();g=gpu();return (s,g) if s and g and g[1]>=s[0] else None
    def dock(owner):click((w-276)//2+54+84*(owner-1),h-65)
    switches=[];held=[];backdrops=0
    try:
        # Native UTM mode changes can arrive shortly after boot.
        end=time.monotonic()+45;candidate=None;since=0
        while time.monotonic()<end:
            r=stable(0x5fb92000,6);g=gpu()
            if r and r[1:]!=candidate:candidate=r[1:];since=time.monotonic()
            if r and g and tuple(g[9:11])==tuple(r[2:4]) and g[11]==r[1] and time.monotonic()-since>=2:mode=r;break
            time.sleep(.05)
        assert mode and mode[4]==2;w,h=mode[2]//2,mode[3]//2
        pointer(100,80,False)
        for owner in (3,2,1):dock(owner);wait(lambda:(s:=settled()) and s[0][2]==owner and s)
        time.sleep(.3);wait(settled)
        for owner in (2,3,1)*4:
            s,before=wait(settled);old=s[2];assert old!=owner
            start=time.monotonic();dock(owner)
            first=wait(lambda:(s:=settled()) and s[0][2]==owner and s)
            first_ms=(time.monotonic()-start)*1000
            mask={old-1,owner-1}
            after=wait(lambda:(g:=gpu()) and all(g[6+i]>before[6+i] for i in mask) and g)
            assert all(after[6+i]==before[6+i] for i in range(3) if i not in mask),'unchanged third decoration republished'
            backdrops+=after[5]!=before[5]
            switches.append(dict(owner=owner,first_completion_ms=first_ms,chrome_ready_ms=(time.monotonic()-start)*1000,
                                 decoration_before=list(before[6:9]),decoration_after=list(after[6:9])))
        # Dock hover/pressed feedback may change the backdrop source; its
        # bounded damage history keeps those updates local to the dock.
        s,g=wait(settled);x,y=s[4:6];start_frame=g[2]
        pointer(x+200,y-24,True)
        for dx in (32,-24,48,-16,24,-32):
            wanted=max(12,min(w-572,x+dx));pointer(wanted+200,y-24)
            moved=wait(lambda:(s:=settled()) and s[0][4]==wanted and s)
            held.append(moved[1][2])
        assert len(set(held))==len(held) and min(held)>start_frame
        if m['gpu_verify']:
            capture(q,data/'damage-held.ppm',True)
            # A portable PPM is diagnostic GPU readback, not a host screenshot.
            (guest/('damage-'+a.label+'.ppm')).write_bytes((data/'damage-held.ppm').read_bytes())
        pointer(wanted+200,y-24,False);wait(settled)
        # Minimize/restore must repair uncovered and subsequently covered pixels.
        s,g=wait(settled);click(s[4]+490,s[5]-22)
        wait(lambda:(s:=settled()) and s[0][7]==0 and s);dock(1)
        wait(lambda:(s:=settled()) and s[0][7]==1 and s[0][2]==1 and s)
        def cursor(x):
            old=wait(gpu)[2];pointer(x,80);return wait(lambda:(g:=gpu()) and g[2]>old and g)
        for x in (100,101,100,101):cursor(x)
        before=wait(gpu);cursor(102);after=cursor(103)
        u64=lambda g,i:g[i]|g[i+1]<<32
        damaged=u64(after,27)-u64(before,27);full=(after[2]-before[2])*mode[2]*mode[3]
        assert full and 0<damaged/full<.02,('cursor redrew too much',damaged,full)
        assert stable(0x5fb92000,6)[1:]==mode[1:],'mode changed during measurement'
        result=dict(saved_data_used=False,uuid=session['uuid'],loader_sha256=session['loader_sha256'],size=list(mode[2:4]),scale=2,
            gpu_verify=m['gpu_verify'],switches=switches,held_completed_frames=held,backdrop_revisions_during_switches=backdrops,
            switch_first_completion_median_ms=statistics.median(x['first_completion_ms'] for x in switches),
            switch_chrome_ready_median_ms=statistics.median(x['chrome_ready_ms'] for x in switches),
            cursor_damaged_pixels=damaged,cursor_full_frame_equivalent=full,cursor_damaged_fraction=damaged/full,
            scope=__doc__,harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
        (guest/('damage-'+a.label+'.json')).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        try:q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
        finally:q.close()

if __name__=='__main__':main()
