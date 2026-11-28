#!/usr/bin/env python3
"""Fresh disposable four-app OS unlock, Maps persistence and erase recovery."""
import argparse,hashlib,json,os,plistlib,re,struct,subprocess,sys,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from desktop_trio_lock_input import Lifecycle,PASSWORD
ROOT=Path(__file__).resolve().parents[1];CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
sys.path.insert(0,str(ROOT/'experiments/desktop-apps'))
from session import program
NAME='osmapsprivate42'
def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--guest',type=Path,required=True);ap.add_argument('--resume',action='store_true');a=ap.parse_args()
    stage=a.guest.resolve();assert stage.is_relative_to(ROOT/'build/desktop-apps') and (stage/'.disposable-build').is_file()
    m=json.loads((stage/'build.json').read_text());s=json.loads((stage/'session.json').read_text())
    assert m['wrapped_session'] and m['map_private'] and m['maps'] and m['saved_data_used'] is s['saved_data_used'] is False
    data=Path(s['bundle'])/'Data';assert data.parent.parent==stage and data.resolve()==data
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==s['uuid'] and [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
    assert not any((data/f).is_symlink() for f in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==s['loader_sha256']
    assert all(hashlib.sha256((ROOT/f).read_bytes()).hexdigest()==h for f,h in m['sources'].items())
    assert not host_locked();out=stage/'session-verification';out.mkdir(exist_ok=True);source=Path(__file__).read_bytes();(out/'harness-used.py').write_bytes(source)
    q=None;lc=None;checks=[];erasures=[]
    def connect():
        nonlocal q,lc
        q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
        try:q.send('query-status')
        except RuntimeError as e:
            if 'Expecting capabilities negotiation' not in str(e):raise
            q.send('qmp_capabilities')
        lc=Lifecycle(q,data,out)
    def st():
        for _ in range(100):
            b=lc.read(0x5fb96000,8192);v=struct.unpack('<1024Q',b)
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<Q',lc.read(0x5fb96000,8))[0]:return v
        return (0,)*1024
    def until(f,timeout=120):return lc.until(f,timeout,awake=True)
    def scene():return struct.unpack('<20I',lc.read(0x5fbb4000,80))
    def mode():return struct.unpack('<6I',lc.read(0x5fb92000,24))
    def pos(x,y,down):
        w,h=mode()[2]//2,mode()[3]//2
        q.send('input-send-event',dict(events=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in (('x',x,w),('y',y,h))]+[{'type':'btn','data':{'button':'left','down':down}}]))
    def tap_screen(x,y):pos(x,y,True);time.sleep(.08);pos(x,y,False)
    def tap(x,y):sc=scene();tap_screen(sc[16]+x,sc[17]+y)
    def open_maps():
        until(lambda:st()[1]==0x4e584d4150303031 and st()[13]==0)
        w,h=mode()[2]//2,mode()[3]//2;tap_screen((w-360)//2+306,h-65);until(lambda:st()[4] and scene()[2]==4)
    def saved():
        tap(430,st()[9]-16);until(lambda:st()[849]==1 and st()[850] and not st()[852]);assert st()[857]==lc.status()[2]
    def name_on_screen():return NAME.encode() in lc.read(0x5fb96000,8192) # private names are deliberately not in observations
    def screen(name,words):
        from desktop_gpu_capture import capture
        end=time.monotonic()+30
        while time.monotonic()<end:
            f=data/'maps-session-screen.ppm';capture(q,f,True);p=out/(name+'.png');subprocess.run(['magick',str(f),str(p)],check=True)
            sc=scene();crop=data/'maps-session-crop.png';subprocess.run(['magick',str(p),'-crop',f'1120x{st()[9]*2}+{sc[16]*2}+{sc[17]*2}','+repage','-colorspace','Gray','-negate',str(crop)],check=True)
            text=subprocess.run(['tesseract',str(crop),'stdout','--psm','11'],text=True,capture_output=True,check=True).stdout
            norm=lambda x:''.join(c for c in x.lower() if c.isalnum())
            if all(norm(w) in norm(text) for w in words):(out/(name+'.txt')).write_text(text);return
            lc.activity();time.sleep(.15)
        raise AssertionError((name,words,text))
    # Physical frame inventory is independent of the manager's erase counter.
    report=(stage/'report.txt').read_text();frames={}
    for size,name,address in re.findall(r"Page\((4 KiB|2 MiB)\): '([^']+)' @ (0x[0-9a-f]+)",report):frames[name]=(int(address,16),4096 if size=='4 KiB' else 2097152)
    private=[]
    shared=('map_frame','map_heap','map_input','map_observation','map_store_control','map_store_bytes','map_disk_dma','map_private_cipher','map_session_key')
    for n,v in frames.items():
        if any(n.startswith('frame_mr_'+r+'_') for r in shared):private.append(v)
    for pd,image in (('offline_map','offline-map.elf'),('map_store','map-store.elf')):
        _,segments=program(stage/image);sequence=0
        for kind,flags,_,va,_,_,length,_ in segments:
            if kind!=1:continue
            at=va&~4095;end=(va+length+4095)&~4095
            while at<end:
                size=2097152 if at%2097152==0 and end-at>=2097152 else 4096
                key=f'frame_elf_{pd}_{sequence:09d}';assert key in frames,(key,at)
                if flags&2:assert frames[key][1]==size;private.append(frames[key])
                at+=size;sequence+=1
        for n,v in frames.items():
            if n.startswith('frame_'+pd+'_stack_') or n=='frame_ipcbuf_'+pd:private.append(v)
    assert len(private)>20
    def erased():
        assert lc.locked(lc.status()[2]);total=0
        for address,n in private:
            b=lc.read(address,n);assert b==bytes(n),('private Maps frame retained',hex(address));total+=n
        erasures.append(dict(epoch=lc.status()[2],bytes=total,frames=len(private)))
    def lock():
        epoch=lc.status()[2]+1;lc.key('f9');lc.until(lambda:lc.locked(epoch));erased();return epoch
    def unlock(epoch):
        generation=lc.status()[8]+1;lc.unlock(epoch,generation);open_maps();saved();assert st()[851]==1;screen('recovered-'+str(epoch),['Saved places',NAME]);assert not name_on_screen()
    def restart():
        nonlocal q
        q.send('quit');q.close();q=None
        end=time.monotonic()+45
        while subprocess.check_output([CTL,'status',s['uuid']],text=True).strip()!='stopped':assert time.monotonic()<end;time.sleep(.2)
        subprocess.run([CTL,'start',s['uuid']],check=True);time.sleep(2);connect();lc.until(lambda:lc.locked(1))
    try:
        connect();lc.enroll(resume=a.resume);open_maps();saved()
        assert st()[851] in (0,1)
        if not st()[851]:
            tap(535,92);until(lambda:not st()[849]);tap(369,35);until(lambda:st()[705]);lc.word('gibraltar');lc.key('ret');until(lambda:st()[513] and not st()[705])
            tap(505,st()[9]-36-22);until(lambda:st()[849]==1 and st()[850] and st()[851]==1 and not st()[852])
            tap(470,148);until(lambda:st()[849]==6);lc.word(NAME);lc.key('ret');until(lambda:st()[849]==1 and not st()[852] and st()[853]>=2)
        screen('saved-with-os-unlock',['Saved places',NAME]);assert not name_on_screen()
        raw=(data/'maps.img').read_bytes();assert NAME.encode() not in raw and PASSWORD.encode() not in raw
        assert raw[1048576:1048584]==b'NXMPS002' and raw[1052672:1052680]==b'NXMPS002'
        checks.append('OS enrollment gates 16 services; saved place and rename use derived Maps key with no second passphrase; only ciphertext persists')
        epoch=lock();generation=lc.status()[8];lc.key('ret');time.sleep(.2);lc.word('incorrectpassword');lc.key('ret');lc.marker('LOCK_AUTH|DENIED');assert lc.locked(epoch) and lc.status()[8]==generation;erased()
        time.sleep(3);lc.word(PASSWORD);lc.key('ret');until(lambda:not lc.status()[1] and lc.status()[8]==generation+1);open_maps();saved();assert st()[851]==1;screen('correct-os-unlock',['Saved places',NAME])
        checks.append('manual lock clears actual Maps writable ELF, stacks, IPC and shared frames; wrong OS password starts no service; correct unlock recovers saved place')
        # Lock with a real public-map request in flight. The disk is reset
        # before scrub, then initialized from scratch after authentication.
        tap(535,92);until(lambda:not st()[849]);drive='drive'+cfg['Drive'][1]['Identifier']
        q.send('block_set_io_throttle',dict(device=drive,bps=131072,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0))
        tap(85,26);until(lambda:st()[880]);tap(85,188);until(lambda:st()[13]==1);epoch=lock()
        q.send('block_set_io_throttle',dict(device=drive,bps=0,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0));unlock(epoch)
        checks.append('lock during throttled map I/O resets VirtIO queue; disk, catalogue and private saved places recover without VM restart')
        epoch=lc.status()[2]+1;lc.until(lambda:lc.locked(epoch),timeout=100);erased();unlock(epoch)
        checks.append('OS idle auto-lock clears Maps; same-VM unlock restores bookmarks automatically')
        lock();restart();erased();unlock(1)
        checks.append('cold boot starts locked with Maps memory clear; OS unlock derives the same app key and recovers persisted saved places')
        text=(data/'trace.log').read_text(errors='replace');assert 'MAPS_SESSION|FOREIGN_KEY_DENIED' in text and 'TRIO_PROBE|FAIL|' not in text
        checks.append('unrelated capability holder is denied Maps key access while locked and unlocked')
        result=dict(passed=True,loader_sha256=m['loader_sha256'],harness_sha256=hashlib.sha256(source).hexdigest(),checks=checks,erasures=erasures,saved_data_used=False,pixel_wifi_touched=False,scope='Disposable desktop curated four-app lock; bounded physical frame erasure checks, not whole-RAM/SIMD/host-GPU/hardware retry or rollback assurance')
        (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        if q:
            try:q.send('input-send-event',dict(events=[{'type':'btn','data':{'button':'left','down':False}}]))
            finally:q.close()
if __name__=='__main__':main()
