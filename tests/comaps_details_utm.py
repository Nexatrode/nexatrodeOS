#!/usr/bin/env python3
"""Tap-to-inspect and drag separation in generated disposable Maps VMs."""
import argparse,hashlib,json,os,plistlib,re,shutil,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1]
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--mobile',action='store_true');a=p.parse_args()
    stage=a.guest.resolve();assert stage.is_relative_to(ROOT/('build/mobile-browser' if a.mobile else 'build/desktop-apps')) and (stage/'.disposable-build').is_file()
    m=json.loads((stage/'build.json').read_text());session=json.loads((stage/'session.json').read_text())
    assert m['map_storage'] and m['saved_data_used'] is session['saved_data_used'] is False
    data=Path(session['bundle'])/'Data';assert data.parent.parent==stage and data.resolve()==data
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid'] and [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
    assert not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    assert all(hashlib.sha256((ROOT/s).read_bytes()).hexdigest()==h for s,h in m['sources'].items())
    assert not host_locked(),'Mac must be unlocked for scanout checks'
    evidence=stage/'details-verification';evidence.mkdir(exist_ok=True);checks=[];selections=[]
    (evidence/'harness-used.py').write_bytes(Path(__file__).read_bytes())
    q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
    try:q.send('query-status')
    except RuntimeError as e:
        if 'Expecting capabilities negotiation' not in str(e):raise
        q.send('qmp_capabilities')
    def read(address,n):
        path=data/'map-details-state.bin'
        try:q.send('pmemsave',{'val':address,'size':n,'filename':str(path)});return path.read_bytes()
        finally:path.unlink(missing_ok=True)
    def state():
        for _ in range(100):
            raw=read(0x5fb96000,8192);s=struct.unpack('<1024Q',raw)
            if s[0] and not s[0]&1 and s[0]==struct.unpack('<Q',read(0x5fb96000,8))[0] and s[512]==0x4e584d4445543031:return s,raw
        return None
    def until(fn,seconds=40):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            text=(data/'trace.log').read_text(errors='replace');assert not any(x in text for x in ('COMAPS|FAIL|','VMFault','panicked at','MON|ERROR','DESKTOP_GPU|FAIL|')),text[-2500:]
            value=fn()
            if value:return value
            time.sleep(.03)
        raise TimeoutError('details state '+str(state()[0][:16] if state() else None))
    def s():return until(state)[0]
    def mode():return struct.unpack('<6I',read(0x5fb92000,24))
    def scene():return struct.unpack('<20I',read(0x5fbb4000,80))
    def position(x,y,down=None):
        w,h=(1080,2340) if a.mobile else (mode()[2]//2,mode()[3]//2)
        events=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in [('x',x,w),('y',y,h)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':events})
    def pointer(x,y,down=None):
        if a.mobile:position(x*3,80*2.6086957+y*3,down)
        else:
            sc=scene();position(sc[16]+x,sc[17]+y,down)
    def tap(x,y):pointer(x,y,True);time.sleep(.06);pointer(x,y,False)
    def trusted_tap(x,y):position(x,y,True);time.sleep(.06);position(x,y,False)
    def open_maps():
        if a.mobile:
            page=struct.unpack('<Q',read(0x5fb92008,8))[0]
            if page!=19:
                if page:
                    x,y=(40,65) if page==16 else (370,60) if page==18 else (350,136)
                    trusted_tap(x*2.6086957,y*2.6086957);until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==0)
                trusted_tap(300*2.6086957,360*2.6086957)
        else:
            until(lambda:mode()[1] and scene()[0]);w,h=mode()[2]//2,mode()[3]//2;trusted_tap((w-360)//2+306,h-65)
        until(lambda:s()[4]==1 and s()[3]>0 and s()[13]==0)
    def choose(region):
        old=s()[15];tap(85,26);time.sleep(.12);tap(85,92+region*48)
        until(lambda:s()[11]==region+1 and s()[13]==0 and s()[15]>old);assert not s()[513]
    def fit():
        old=s()[3];tap(325 if a.mobile else 525,35);until(lambda:s()[3]>old and not s()[513] and s()[8]==0)
    def entries(kind=None):
        st=s();items=[st[536+i*6:542+i*6] for i in range(st[517])]
        return [v for v in items if (kind is None or v[5]==kind) and v[1]+12<v[3] and v[2]+6<v[4]]
    def center(entry):return ((entry[1]+entry[3])/2/(3 if a.mobile else 2),(entry[2]+entry[4])/2/(3 if a.mobile else 2))
    def close():
        if s()[513]:tap((360 if a.mobile else 560)-30,s()[518]+23);until(lambda:not s()[513])
    def name():return state()[1][520*8:532*8].split(b'\0',1)[0].decode('utf8',errors='replace')
    def screenshot(label,kind):
        end=time.monotonic()+30;scale=3 if a.mobile else 2;w=360 if a.mobile else 560
        while time.monotonic()<end:
            path=data/'map-details.ppm';capture(q,path,not a.mobile)
            if a.mobile:x=0;y=round(80*2.6086957+s()[518]*3)
            else:sc=scene();x=sc[16]*2;y=(sc[17]+s()[518])*2
            full=evidence/(label+'.png');card=evidence/(label+'-card.png')
            subprocess.run(['magick',str(path),str(full)],check=True)
            subprocess.run(['magick',str(path),'-crop',f'{w*scale}x{88*scale}+{x}+{y}','+repage',str(card)],check=True)
            text=subprocess.run(['tesseract',str(card),'stdout','--psm','6'],capture_output=True,text=True,check=True,timeout=20).stdout
            normalized=lambda t:re.sub('[^a-z0-9]','',t.lower())
            if all(normalized(word) in normalized(text) for word in [kind,'offline','Map point']):
                (evidence/(label+'.txt')).write_text(text);return
            time.sleep(.12)
        raise AssertionError('details card missing '+kind+': '+text)
    try:
        until(state);open_maps()
        if s()[11]!=1:choose(0)
        fit();entry=until(lambda:entries(0))[0];x,y=center(entry);camera=s()[5:9]
        tap(x,y);until(lambda:s()[513]==entry[0]);assert s()[5:9]==camera
        selections.append(dict(region='Gibraltar',feature=s()[513],name=name(),kind='Place'));screenshot('gibraltar-place','Place')
        # Clicking/dragging the card is consumed; it cannot pan the map below.
        pointer(30,s()[518]+60,True);pointer(70,s()[518]+64);pointer(70,s()[518]+64,False)
        assert s()[513]==entry[0] and s()[5:9]==camera;close()
        entry=until(lambda:entries(0))[0];x,y=center(entry);camera=s()[5:9]
        pointer(x,y,True);pointer(x+3,y+2);pointer(x+3,y+2,False)
        until(lambda:s()[513]==entry[0]);assert s()[5:9]==camera;close()
        # A drag from a selectable label must never open a place on release.
        entry=until(lambda:entries(0))[0];x,y=center(entry);camera=s()[5:9]
        pointer(x,y,True)
        for i in range(1,9):pointer(x+8*i,y+3*i);time.sleep(.02)
        until(lambda:s()[516]==1);pointer(x+64,y+24,False)
        until(lambda:not s()[515] and not s()[516]);assert not s()[513] and s()[5:7]!=camera[:2]
        fit();checks.append('named place tap, small jitter, complete drag displacement and noninteractive card area')
        # Named roads become selectable as more street labels enter the view.
        road=None
        for level in range(1,7):
            tap(270 if a.mobile else 470,35);until(lambda:s()[8]==level)
            roads=entries(1)
            if roads:road=roads[0];break
        assert road,'no selectable named road at any supported zoom'
        tap(*center(road));until(lambda:s()[513]==road[0]);selections.append(dict(region='Gibraltar',feature=s()[513],name=name(),kind='Road'));screenshot('gibraltar-road','Road')
        # Hide and restore retain details without accepting another app's input.
        selected=s()[513];old=s()[2:4]
        if a.mobile:trusted_tap(370*2.6086957,60*2.6086957)
        else:w,h=mode()[2]//2,mode()[3]//2;trusted_tap((w-360)//2+54,h-65)
        until(lambda:s()[4]==0);hidden=s()[2:4]
        q.send('input-send-event',{'events':[{'type':'key','data':{'down':v,'key':{'type':'qcode','data':'right'}}} for v in (True,False)]});time.sleep(.2)
        assert s()[2:4]==hidden and s()[513]==selected
        open_maps();assert s()[513]==selected;close();fit();checks.append('road label selection, close, retained details and private focus routing')
        choose(1);assert not s()[513];entry=until(lambda:entries(0))[0]
        tap(*center(entry));until(lambda:s()[513]==entry[0]);selections.append(dict(region='San Marino',feature=s()[513],name=name(),kind='Place'));screenshot('san-marino-place','Place')
        choose(0);assert not s()[513];fit();checks.append('both disk-backed regions select real places; changing region clears stale feature references')
        assert not host_locked()
        result=dict(passed=True,mobile=a.mobile,uuid=session['uuid'],loader_sha256=m['loader_sha256'],saved_data_used=False,pixel_wifi_touched=False,selections=selections,checks=checks,scope='disposable UTM, QMP input, structured public map state and actual composed pixels; no native physical touch claim or saved bookmarks')
        (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
    finally:
        try:q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
        finally:q.close()
if __name__=='__main__':main()
