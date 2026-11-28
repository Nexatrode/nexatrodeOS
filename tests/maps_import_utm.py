#!/usr/bin/env python3
"""Local imports, real interrupted writes and OS-encrypted saved-place recovery.

Only explicitly marked new disposable desktop/mobile fixtures are accepted.
Inbox injection uses the guarded local-pack tool while the VM is stopped.
"""
import argparse,hashlib,importlib.util,json,os,plistlib,re,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from desktop_trio_lock_input import Lifecycle,PASSWORD as DESKTOP_PASSWORD
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1];CTL='/Applications/UTM.app/Contents/MacOS/utmctl';SCALE=2.6086957
NAME='importkeep42';PASSWORD='disposableimportalpha'
spec=importlib.util.spec_from_file_location('local_pack',ROOT/'experiments/comaps-native/local_pack.py');packs=importlib.util.module_from_spec(spec);spec.loader.exec_module(packs)
def ctl(*a):return subprocess.run([CTL,*map(str,a)],capture_output=True,text=True,timeout=35)
def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--guest',type=Path,required=True);ap.add_argument('--mobile',action='store_true');ap.add_argument('--resume',action='store_true');a=ap.parse_args()
    stage=a.guest.resolve();assert stage.is_relative_to(ROOT/('build/mobile-browser' if a.mobile else 'build/desktop-apps')) and (stage/'.disposable-build').is_file()
    m=json.loads((stage/'build.json').read_text());session=json.loads((stage/'session.json').read_text());assert m['map_import'] and m['saved_data_used'] is session['saved_data_used'] is False
    data=Path(session['bundle']).resolve()/'Data';assert data.parent.parent==stage and data.parent.suffix=='.utm'
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid'] and [x['ImageName'] for x in cfg['Drive']]==['blank.img','maps.img']
    assert not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    assert all(hashlib.sha256((ROOT/n).read_bytes()).hexdigest()==h for n,h in m['sources'].items())
    assert not host_locked();out=stage/'import-verification';out.mkdir(exist_ok=True);(out/'harness-used.py').write_bytes(Path(__file__).read_bytes())
    q=None;lc=None;checks=[];interrupts=[];last_activity=0
    def connect(fresh=False):
        nonlocal q,lc
        q=MotionQmp(data/'monitor')
        if fresh:q.connect()
        else:
            q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
            try:q.send('query-status')
            except RuntimeError as e:
                if 'Expecting capabilities negotiation' not in str(e):raise
                q.send('qmp_capabilities')
        if not a.mobile:lc=Lifecycle(q,data,out)
    def read(address,n):
        p=data/'import-read.bin'
        try:q.send('pmemsave',{'val':address,'size':n,'filename':str(p)});return p.read_bytes()
        finally:p.unlink(missing_ok=True)
    def st():
        for _ in range(30):
            raw=read(0x5fb96000,8192);s=struct.unpack('<1024Q',raw)
            if s[0] and not s[0]&1 and s[0]==struct.unpack('<Q',read(0x5fb96000,8))[0]:return s
        return (0,)*1024
    def ui():return struct.unpack('<13Q',read(0x5fb92000,104))
    def trusted():return struct.unpack('<IIQQII',read(0x5fb98000,32)) if a.mobile else lc.status()
    def active():return trusted()[1]==(1 if a.mobile else 0)
    def mode():return struct.unpack('<6I',read(0x5fb92000,24))
    def scene():return struct.unpack('<20I',read(0x5fbb4000,80))
    def pos(x,y,down=None):
        w,h=(1080,2340) if a.mobile else (mode()[2]//2,mode()[3]//2)
        e=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in (('x',x,w),('y',y,h))]
        if down is not None:e.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',dict(events=e))
    def tap_p(x,y):pos(x,y,True);time.sleep(.08);pos(x,y,False);time.sleep(.1)
    def tap(x,y):
        if a.mobile:tap_p(x*3,80*SCALE+y*3)
        else:s=scene();tap_p(s[16]+x,s[17]+y)
    def key(k):q.send('input-send-event',dict(events=[{'type':'key','data':{'down':d,'key':{'type':'qcode','data':k}}} for d in (True,False)]))
    def word(s):
        for c in s:key(c);time.sleep(.05)
    def activity():
        nonlocal last_activity
        if time.monotonic()-last_activity>1:pos(1035 if a.mobile else 20,70);key('shift');last_activity=time.monotonic()
    def until(f,seconds=100,awake=True):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            text=(data/'trace.log').read_text(errors='replace');assert not any(k in text for k in ('MAPS_LOCK|FAIL|','TRIO|FAIL|','COMAPS|FAIL|','MON|ERROR','VMFault','panicked at','ERASURE_FAILED')),text[-4000:]
            v=f()
            if v:return v
            if awake:activity()
            time.sleep(.05)
        raise TimeoutError('local import '+str(st()[960:972])+' storage '+str(st()[11:16]))
    def screen(name,words=()):
        norm=lambda x:re.sub('[^a-z0-9]','',x.lower());end=time.monotonic()+40
        while time.monotonic()<end:
            p=data/'import-screen.ppm';capture(q,p,not a.mobile);target=out/(name+'.png');subprocess.run(['magick',str(p),str(target)],check=True)
            if not a.mobile:
                sc=scene();crop=data/'import-crop.png';subprocess.run(['magick',str(target),'-crop',f'1120x{st()[9]*2}+{sc[16]*2}+{sc[17]*2}','+repage','-colorspace','Gray','-negate',str(crop)],check=True);p=crop
            text=subprocess.run(['tesseract',str(p),'stdout','--psm','11'],text=True,capture_output=True,check=True,timeout=25).stdout
            (out/(name+'.txt')).write_text(text)
            if all(norm(w) in norm(text) for w in words):return
            activity();time.sleep(.2)
        raise AssertionError((name,words,text))
    def click_word(word):
        import csv
        end=time.monotonic()+45
        while time.monotonic()<end:
            path=data/'import-click.ppm';capture(q,path,False)
            raw=subprocess.check_output(['tesseract',str(path),'stdout','--psm','11','tsv'],text=True,stderr=subprocess.DEVNULL)
            rows=[r for r in csv.DictReader(raw.splitlines(),delimiter='\t') if r.get('text','').lower()==word.lower()]
            if rows:
                r=rows[0];tap_p(int(r['left'])+int(r['width'])/2,int(r['top'])+int(r['height'])/2);return
            time.sleep(.2)
        raise AssertionError('missing phone control '+word)
    def swipe():
        pos(540,2050,True)
        for y in (1950,1850,1750,1650,1550):pos(540,y);time.sleep(.08)
        pos(540,1550,False);time.sleep(.2)
    def unlock():
        if a.mobile:
            swipe();screen('unlock-form',['CONTINUE']);tap_p(280,2340-77*SCALE);word(PASSWORD);key('ret');until(active)
        else:lc.unlock(lc.status()[2],lc.status()[8]+1)
    def open_maps():
        until(lambda:active() and st()[1]==0x4e584d4150303031 and st()[13]==0)
        if a.mobile:
            if ui()[1]!=0:
                if ui()[1]==19:return
                tap_p(350*SCALE,136*SCALE);until(lambda:ui()[1]==0)
            tap_p(300*SCALE,360*SCALE)
        else:w,h=mode()[2]//2,mode()[3]//2;tap_p((w-360)//2+306,h-65)
        until(lambda:st()[4] and (a.mobile or scene()[2]==4))
    def saved():
        if st()[880]:
            if st()[961]:tap(80,264);until(lambda:st()[961]==0)
            tap(85,26);until(lambda:not st()[880])
        tap(230 if a.mobile else 430,st()[9]-16);until(lambda:st()[849]==1 and st()[850] and not st()[852])
    def close_saved():
        if st()[849]:tap(335 if a.mobile else 535,92);until(lambda:st()[849]==0)
    def banks():
        with (data/'maps.img').open('rb') as f:f.seek(1024*1024);return f.read(8192)
    def save_place():
        tap(169 if a.mobile else 369,35);until(lambda:st()[705]);word('gibraltar');until(lambda:st()[707]>0)
        if a.mobile:
            tap_p(80*SCALE,2340-248*SCALE);until(lambda:not st()[706] and st()[9]>600);tap(80,168)
        else:key('ret')
        until(lambda:not st()[705] and st()[513]);tap(305 if a.mobile else 505,st()[9]-(74 if a.mobile else 58))
        until(lambda:st()[849]==1 and st()[851]==1 and not st()[852]);tap(270 if a.mobile else 470,148);until(lambda:st()[849]==6);word(NAME);key('ret');until(lambda:st()[849]==1 and not st()[852]);screen('saved-before-import',[NAME]);close_saved()
    def manager():
        close_saved();tap(85,26);until(lambda:st()[880]);
        if st()[961] in (5,6):tap(80,264);until(lambda:st()[961]==0)
    def probe():
        manager();assert 276<=st()[9]-(52 if a.mobile else 36),'import controls covered by footer';screen('region-manager',['Import local pack','Close']);tap(260 if a.mobile else 460,258);until(lambda:st()[961] in (2,6));return st()[961]
    def install(expected,throttle=False):
        assert probe()==2;screen('pack-preview-'+str(expected),['Local offline region','Version'])
        if throttle:q.send('block_set_io_throttle',dict(device='drive'+cfg['Drive'][1]['Identifier'],bps=32768,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0))
        tap(100,220)
        if throttle:
            until(lambda:st()[961] in (3,4))
            # A genuine keyboard shortcut must not replace the in-flight job.
            for _ in range(3):
                q.send('input-send-event',dict(events=[{'type':'key','data':{'down':True,'key':{'type':'qcode','data':'ctrl'}}}]))
                key('f')
                q.send('input-send-event',dict(events=[{'type':'key','data':{'down':False,'key':{'type':'qcode','data':'ctrl'}}}]))
            time.sleep(1);assert st()[961] in (3,4) and not st()[705] and st()[880]
            until(lambda:st()[962]==11 and st()[971]>0,100);interrupts.append(dict(phase=int(st()[962]),progress=int(st()[971]),catalog_generation=int(st()[963])));stop();return
        until(lambda:st()[961]==5 and st()[13]==0,120);assert st()[964+(3 if expected==1 else 0)]==expected
    def stop():
        nonlocal q
        assert session['saved_data_used'] is False
        if q:q.send('quit');q.close();q=None
        end=time.monotonic()+45
        while time.monotonic()<end:
            if ctl('status',session['uuid']).stdout.strip()=='stopped':return
            time.sleep(.2)
        raise TimeoutError('disposable fixture did not stop')
    def start():
        assert ctl('status',session['uuid']).stdout.strip()=='stopped';(data/'trace.log').unlink(missing_ok=True)
        r=ctl('start',session['uuid']);assert r.returncode==0,r.stderr;connect(True)
        until(lambda:(trusted()[0]!=0 and ui()[0]==0x4e584445534b3031 and ui()[1]==14) if a.mobile else lc.locked(1),120,awake=False);assert not active();unlock();open_maps()
    def stage_pack(p):packs.stage_inbox(stage,p)
    def check_saved(label):saved();assert st()[851]==1;screen(label,[NAME,'Saved places']);assert banks()==private_banks;close_saved()
    try:
        connect()
        if a.resume:
            if not active():unlock()
            open_maps()
        elif a.mobile:
            until(lambda:ui()[0] and trusted()[0],awake=False);screen('enrollment',['SET PASSPHRASE']);word(PASSWORD);key('ret');screen('confirmation',['same passphrase']);word(PASSWORD);key('ret');screen('enrolled',['swipe']);unlock();open_maps()
        else:lc.enroll();open_maps()
        assert st()[960]==0x4e584d494d503031
        if not a.resume:
            assert st()[968]==7;save_place();private_banks=banks();assert b'NXMPS002' in private_banks
            # New local region starts absent and has a separate stable slot/identity.
            install(1);assert st()[968]==15 and st()[11]==4;screen('imported-local-region',['Local Gibraltar']);check_saved('saved-after-import')
        else:
            assert st()[968] in (7,15) and st()[964]==1
            private_banks=banks();check_saved('saved-after-resume')
            if st()[968]==7:
                install(1);assert st()[968]==15;screen('imported-local-region',['Local Gibraltar']);check_saved('saved-after-import')
            assert st()[967]==1
        checks.append('guest local pack preview, structural validation, import into absent slot and automatic encrypted saved-place recovery')
        stop();stage_pack(stage/'local-packs/gibraltar-v2.nxregion');start();assert st()[964]==1 and st()[967]==1;check_saved('saved-after-cold-import')
        install(2,True);start();assert st()[964]==1 and st()[967]==1;check_saved('saved-after-interrupted-data-write')
        checks.append('real interrupted inactive payload write leaves the old version and saved-place ciphertext intact after cold OS unlock')
        install(2);assert st()[964]==2;check_saved('saved-after-update')
        checks.append('updated payload readback and durable catalogue commit recover private saved places without rewriting their banks')
        stop()
        disk=data/'maps.img'
        with disk.open('r+b') as f:
            f.seek(packs.CATALOG);journals=f.read(8192);gens=[struct.unpack_from('<Q',journals,i*4096+8)[0] for i in range(2)];newest=max(range(2),key=lambda i:gens[i]);f.seek(packs.CATALOG+newest*4096+1024);f.write(bytes([journals[newest*4096+1024]^1]));f.flush();os.fsync(f.fileno())
        start();assert st()[964]==1 and st()[967]==1;check_saved('saved-after-torn-catalogue');checks.append('torn newest catalogue record falls back to previous complete generation and preserves the imported region and saved places')
        # Same version can be retried after rollback; then stale versions reject.
        install(2);stop();stage_pack(stage/'local-packs/gibraltar-v2.nxregion');start();assert probe()==6;screen('stale-pack-rejected',['Pack rejected']);assert st()[964]==2
        # A correct header/hash with invalid MWM must fail before any writes.
        stop();bad=stage/'local-packs/invalid.mwm';bad.write_bytes(bytes(512));package=stage/'local-packs/invalid-v3.nxregion';package.write_bytes(packs.pack(bad,'Gibraltar',0,3));stage_pack(package);start();assert probe()==2;tap(100,220);until(lambda:st()[961]==6);assert st()[964]==2;screen('invalid-geometry-rejected',['Pack rejected']);check_saved('saved-after-bad-pack')
        checks.append('stale versions and structurally invalid geometry reject while the last working map and encrypted saved places remain available')
        # Reconstructed services retain the installed metadata through full lock.
        if a.mobile:
            tap_p(370*SCALE,60*SCALE);until(lambda:ui()[1]==0);click_word('SETTINGS');click_word('Privacy');click_word('NOW');until(lambda:not active(),awake=False);unlock();open_maps()
        else:
            key('f9');until(lambda:not active(),awake=False);unlock();open_maps()
        assert st()[964]==2 and st()[967]==1;check_saved('saved-after-os-lock')
        stop();start();assert st()[964]==2 and st()[967]==1;check_saved('saved-after-final-restart')
        checks.append('OS lock/unlock and final cold restart recover both installed versions and the unchanged private saved-place banks')
        raw=(data/'maps.img').read_bytes();assert NAME.encode() not in raw and PASSWORD.encode() not in raw
        result=dict(passed=True,mobile=a.mobile,checks=checks,interruptions=interrupts,loader_sha256=m['loader_sha256'],saved_data_used=False,pixel_wifi_touched=False,region_versions=list(st()[964:968]),scope='bounded prepared RAW-MWM packages, one extra local slot, staged inbox; not arbitrary CoMaps imports or a file-picker/download service')
        (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');session.update(maps_import_passed=True,import_validation=str(out/'validation.json'));(stage/'session.json').write_text(json.dumps(session,indent=2)+'\n')
        print('PASS local Maps import, interrupted updates and saved-place recovery',session['uuid'],flush=True)
    finally:
        if q:
            try:q.send('block_set_io_throttle',dict(device='drive'+cfg['Drive'][1]['Identifier'],bps=0,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0))
            finally:q.close()
if __name__=='__main__':main()
