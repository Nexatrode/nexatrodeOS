#!/usr/bin/env python3
"""Fresh phone OS-wrapped Maps lifecycle: actual input, scanout and physical RAM."""
import argparse,hashlib,json,os,plistlib,re,shutil,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from mobile_browser_utm import stage as verify_stage,ctl
ROOT=Path(__file__).resolve().parents[1]
PASSWORD='disposablemobilealpha'
NAME='mobileprivate42'
SCALE=2.6086957

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--guest',type=Path,required=True);a=ap.parse_args()
    stage,m=verify_stage(a.guest);session=json.loads((stage/'session.json').read_text())
    assert m['maps_os_unlock'] and session['saved_data_used'] is False
    data=Path(session['bundle'])/'Data';assert data.parent.parent==stage and data.resolve()==data
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid']
    assert [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
    assert all(not (data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    assert not host_locked();out=stage/'session-verification';out.mkdir(exist_ok=True)
    (out/'harness-used.py').write_bytes(Path(__file__).read_bytes());q=None;checks=[];erasures=[]
    def connect():
        nonlocal q
        q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
        try:q.send('query-status')
        except RuntimeError as e:
            if 'Expecting capabilities negotiation' not in str(e):raise
            q.send('qmp_capabilities')
    def read(address,n):
        p=data/'mobile-session-read.bin'
        try:q.send('pmemsave',{'val':address,'size':n,'filename':str(p)});return p.read_bytes()
        finally:p.unlink(missing_ok=True)
    def status():
        for _ in range(30):
            b=read(0x5fb98000,32);s=struct.unpack('<IIQQII',b)
            if s[0] and not s[0]&1 and s[0]==struct.unpack('<I',read(0x5fb98000,4))[0]:return s
        return (0,)*6
    def ui():return struct.unpack('<13Q',read(0x5fb92000,104))
    def st():
        for _ in range(40):
            raw=read(0x5fb96000,8192);s=struct.unpack('<1024Q',raw)
            if s[0] and not s[0]&1 and s[0]==struct.unpack('<Q',read(0x5fb96000,8))[0]:return s
        return (0,)*1024
    def guard():
        s=(data/'trace.log').read_text(errors='replace')
        assert not any(x in s for x in ('MAPS_LOCK|FAIL|','ERASURE_FAILED','COMAPS|FAIL|','VMFault','panicked at','MON|ERROR','Data Abort')),s[-5000:]
    def until(fn,seconds=90):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            guard();v=fn()
            if v:return v
            time.sleep(.08)
        raise TimeoutError('phone lifecycle status '+str(status())+' UI '+str(ui()[:4])+' private '+str(st()[848:858]))
    def pos(x,y,down=None):
        events=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in (('x',x,1080),('y',y,2340))]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':events})
    def tap_p(x,y):pos(x,y,True);time.sleep(.08);pos(x,y,False);time.sleep(.15)
    def tap(x,y):tap_p(x*3,80*SCALE+y*3)
    def key(k):q.send('input-send-event',{'events':[{'type':'key','data':{'down':d,'key':{'type':'qcode','data':k}}} for d in (True,False)]})
    def word(text):
        for c in text:key(c);time.sleep(.06)
    def screen(name=None):
        p=data/'mobile-session.ppm';q.send('screendump',{'filename':str(p)})
        rows=subprocess.run(['tesseract',str(p),'stdout','--psm','11','tsv'],capture_output=True,text=True,check=True,timeout=25).stdout
        import csv
        result=[r for r in csv.DictReader(rows.splitlines(),delimiter='\t') if r.get('text','').strip()]
        if name:
            subprocess.run(['magick',str(p),str(out/(name+'.png'))],check=True)
            (out/(name+'.txt')).write_text(' '.join(r['text'] for r in result))
        return result
    def click_word(text):
        end=time.monotonic()+45
        while time.monotonic()<end:
            rows=screen();found=[r for r in rows if r['text'].lower()==text.lower()]
            if found:
                r=found[0];tap_p(int(r['left'])+int(r['width'])/2,int(r['top'])+int(r['height'])/2);return
            guard();time.sleep(.2)
        raise AssertionError((text,' '.join(r['text'] for r in rows)))
    def screenshot(name,words):
        end=time.monotonic()+35
        norm=lambda s:re.sub('[^a-z0-9]','',s.lower())
        while time.monotonic()<end:
            rows=screen(name);text=' '.join(r['text'] for r in rows)
            if all(norm(w) in norm(text) for w in words):return
            time.sleep(.2)
        raise AssertionError((name,words,text))
    def swipe():
        pos(540,2050,True)
        for y in (1950,1850,1750,1650,1550):pos(540,y);time.sleep(.1)
        pos(540,1550,False);time.sleep(.3)
    def unlock(wrong=False):
        swipe();word('incorrectpassword' if wrong else PASSWORD);key('ret')
        if wrong:
            time.sleep(3);assert not status()[1];screenshot('wrong-password',['Unable to unlock']);return
        until(lambda:status()[1]==1 and ui()[1]!=14)
    def home():
        if ui()[1]==19:tap_p(370*SCALE,60*SCALE)
        elif ui()[1]!=0:click_word('HOME')
        until(lambda:ui()[1]==0)
    def open_maps():
        home();tap_p(300*SCALE,360*SCALE);until(lambda:ui()[1]==19 and st()[4] and st()[13]==0)
    def saved():
        tap(230,st()[9]-16);until(lambda:st()[849]==1 and st()[850]==1 and not st()[852]);assert st()[857]==status()[3]
    # Independently enumerate actual physical pages, rather than trusting erase logs.
    frames={}
    for size,n,address in re.findall(r"Page\((4 KiB|2 MiB)\): '([^']+)' @ (0x[0-9a-f]+)",(stage/'report.txt').read_text()):frames[n]=(int(address,16),4096 if size=='4 KiB' else 2097152)
    shared=json.loads((stage/'session-layout.json').read_text())['private_shared'];private=[]
    for n,v in frames.items():
        if any(n.startswith('frame_mr_'+r+'_') for r in shared):private.append(v)
    import sys
    sys.path.insert(0,str(ROOT/'experiments/desktop-apps'));from session import program
    for pd,image in (('offline_map','offline-map.elf'),('map_store','map-store.elf')):
        _,segments=program(stage/image);seq=0
        for kind,flags,_,va,_,_,length,_ in segments:
            if kind!=1:continue
            at=va&~4095;end=(va+length+4095)&~4095
            while at<end:
                size=2097152 if at%2097152==0 and end-at>=2097152 else 4096
                n=f'frame_elf_{pd}_{seq:09d}';assert n in frames
                if flags&2:assert frames[n][1]==size;private.append(frames[n])
                at+=size;seq+=1
        for n,v in frames.items():
            if n.startswith('frame_'+pd+'_stack_') or n=='frame_ipcbuf_'+pd:private.append(v)
    assert len(private)>20
    def erased():
        assert not status()[1] and not status()[4]
        for address,size in private:assert read(address,size)==bytes(size),('retained private page',hex(address))
        erasures.append(dict(epoch=status()[2],activation=status()[3],frames=len(private),bytes=sum(n for _,n in private)))
    def manual_lock():
        old=status()[2];home();click_word('SETTINGS');click_word('Privacy');click_word('NOW')
        until(lambda:not status()[1] and status()[2]>old);erased();screenshot('manual-lock',['swipe'])
    def reopen():
        unlock();open_maps();saved();assert st()[851]==1;screenshot('recovered-'+str(status()[3]),[NAME,'Saved places'])
    def banks():
        with (data/'maps.img').open('rb') as f:f.seek(1024*1024);return f.read(8192)
    def restart():
        nonlocal q
        q.send('quit');q.close();q=None
        end=time.monotonic()+40
        while time.monotonic()<end:
            if ctl('status',session['uuid']).stdout.strip()=='stopped':break
            time.sleep(.2)
        else:raise TimeoutError('fixture failed to stop')
        (data/'trace.log').unlink(missing_ok=True)
        r=ctl('start',session['uuid']);assert r.returncode==0,r.stderr
        q=MotionQmp(data/'monitor');q.connect();until(lambda:status()[0] and ui()[0])
    try:
        connect();until(lambda:status()[0] and ui()[0]);assert not status()[1];erased();screenshot('initial-setup',['SET PASSPHRASE'])
        word(PASSWORD);key('ret');time.sleep(2);word(PASSWORD);key('ret');until(lambda:ui()[1]==14)
        screenshot('enrolled-locked',['swipe']);assert not status()[1]
        unlock(True);word(PASSWORD);key('ret');until(lambda:status()[1]==1);open_maps()
        tap(169,35);until(lambda:st()[705] and st()[706]);word('gibraltar')
        until(lambda:st()[707]>0);tap_p(80*SCALE,2340-248*SCALE);until(lambda:not st()[706] and st()[9]>600)
        tap(80,168);until(lambda:not st()[705] and st()[513]);tap(305,st()[9]-74)
        until(lambda:st()[849]==1 and st()[850] and st()[851]==1 and not st()[852]);assert st()[857]==1
        tap(270,148);until(lambda:st()[849]==6);word(NAME);key('ret');until(lambda:st()[849]==1 and not st()[852])
        screenshot('saved-private-place',[NAME,'Saved places']);b=banks();assert b'NXMPS002' in b and NAME.encode() not in b
        checks.append('OS enrollment and wrong/correct unlock; saved places opens without a second passphrase; save/rename produces v2 ciphertext')
        manual_lock();reopen();checks.append('manual OS lock wipes Maps/store pages and derived key; one OS unlock recovers private name')
        # Stay on the actual saved-place screen with no synthetic activity.
        old=status()[2];until(lambda:not status()[1] and status()[2]>old,100);erased();screenshot('idle-lock',['swipe']);reopen()
        checks.append('real idle timeout uses the same process erasure and automatic recovery')
        # Exercise public region I/O and pointer state before locking again.
        tap(335,92);until(lambda:st()[849]==0);tap(271,35);before=st()[8];until(lambda:st()[8]>0)
        pointer_before=st()[5:8];pos(180,900,True)
        for x,y in ((240,960),(330,1040),(420,1120)):pos(x,y);time.sleep(.08)
        pos(420,1120,False);until(lambda:st()[5:8]!=pointer_before);tap(325,35);until(lambda:st()[8]==0)
        tap(85,26);tap(85,140);manual_lock();reopen();checks.append('native pan/zoom/Fit and region-load restart recover after repeated OS locks')
        manual_lock();restart();assert not status()[1] and status()[3]==0;erased();screenshot('cold-boot-locked',['swipe']);reopen()
        checks.append('cold restart starts Maps stopped, then the OS passphrase restores authenticated saved places')
        manual_lock();disk=(data/'maps.img').read_bytes();root=(data/'blank.img').read_bytes();trace=(data/'trace.log').read_text(errors='replace')
        assert all(v.encode() not in disk+root for v in (NAME,PASSWORD));assert all(v not in trace for v in (NAME,PASSWORD))
        result=dict(passed=True,checks=checks,erasures=erasures,saved_data_used=False,pixel_wifi_touched=False,loader_sha256=m['loader_sha256'],scope='Maps/store private pages and vault root eviction; phone UI/renderer and other apps are outside RAM erasure scope')
        (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');session.update(maps_os_unlock_passed=True,validation=str(out/'validation.json'));(stage/'session.json').write_text(json.dumps(session,indent=2)+'\n')
        print('PASS mobile Maps OS lock and recovery',session['uuid'],flush=True)
    finally:
        if q:q.close()
if __name__=='__main__':main()
