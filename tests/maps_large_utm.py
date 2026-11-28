#!/usr/bin/env python3
"""Larger paged Files imports, bounded memory and lock/restart recovery.

Only explicitly marked new disposable desktop/mobile fixtures are accepted.
Real encrypted Files library; disposable public source packs only.
"""
import argparse,hashlib,importlib.util,json,os,plistlib,re,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from desktop_trio_lock_input import Lifecycle,PASSWORD as DESKTOP_PASSWORD
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1];CTL='/Applications/UTM.app/Contents/MacOS/utmctl';SCALE=2.6086957
NAME='largekeep27';PASSWORD='disposableimportalpha'
spec=importlib.util.spec_from_file_location('local_pack',ROOT/'experiments/comaps-native/local_pack.py');packs=importlib.util.module_from_spec(spec);spec.loader.exec_module(packs)
def ctl(*a):return subprocess.run([CTL,*map(str,a)],capture_output=True,text=True,timeout=35)
def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--guest',type=Path,required=True);ap.add_argument('--mobile',action='store_true');ap.add_argument('--resume',action='store_true');ap.add_argument('--development-sources',action='store_true');a=ap.parse_args()
    stage=a.guest.resolve();assert stage.is_relative_to(ROOT/('build/mobile-browser' if a.mobile else 'build/desktop-apps')) and (stage/'.disposable-build').is_file()
    m=json.loads((stage/'build.json').read_text());session=json.loads((stage/'session.json').read_text());assert m['map_large_import'] and m['map_files'] and m['map_import'] and m['saved_data_used'] is session['saved_data_used'] is False
    data=Path(session['bundle']).resolve()/'Data';assert data.parent.parent==stage and data.parent.suffix=='.utm'
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid'] and [x['ImageName'] for x in cfg['Drive']]==['blank.img','maps.img']
    assert not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    assert a.development_sources or all(hashlib.sha256((ROOT/n).read_bytes()).hexdigest()==h for n,h in m['sources'].items())
    assert not host_locked();out=stage/'large-files-verification';out.mkdir(exist_ok=True);(out/'harness-used.py').write_bytes(Path(__file__).read_bytes())
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
    def until(f,seconds=300,awake=True):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            text=(data/'trace.log').read_text(errors='replace');assert not any(k in text for k in ('DESKTOP_GPU|FAIL|','MAPS_LOCK|FAIL|','TRIO|FAIL|','COMAPS|FAIL|','MON|ERROR','VMFault','panicked at','ERASURE_FAILED')),text[-4000:]
            v=f()
            if v:return v
            if awake:activity()
            time.sleep(.05)
        raise TimeoutError('local import '+str(st()[960:972])+' storage '+str(st()[11:16]))
    def screen(name,words=()):
        norm=lambda x:re.sub('[^a-z0-9]','',x.lower());end=time.monotonic()+(300 if name=='files-picker' else 40)
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
                page=ui()[1];x,y=(40,65) if page==16 else (370,60) if page==18 else (350,136)
                tap_p(x*SCALE,y*SCALE);until(lambda:ui()[1]==0)
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
        close_saved()
        if not st()[880]:tap(85,26);until(lambda:st()[880])
        if st()[961] in (5,6,7):tap(80,264);until(lambda:st()[961]==0)
    def file_state():return read(0x5fb9c000,4096)
    def picker():
        if not st()[976]:manager();tap(260 if a.mobile else 460,258);until(lambda:st()[976]==1,300)
        screen('files-picker',['Files','Choose one offline region','liechtenstein-v1','liechtenstein-v2'])
    def choose(row):
        tap(60,138+row*62);until(lambda:st()[961] in (2,6),120)
        return st()[961]
    def throttle(index,on):
        q.send('block_set_io_throttle',dict(device='drive'+cfg['Drive'][index]['Identifier'],bps=32768 if on else 0,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0))
    def os_lock():
        initial=st();offset=len((data/'trace.log').read_text(errors='replace'))
        reading=initial[962]==30 and initial[961]==3
        if reading:throttle(0,True)
        if a.mobile:
            tap_p(370*SCALE,60*SCALE);until(lambda:ui()[1]==0);click_word('SETTINGS');until(lambda:ui()[1]==8);screen('settings-for-lock',['Network','Display','Sound']);click_word('Privacy');until(lambda:ui()[1]==13)
            triggered=st();click_word('NOW');until(lambda:trusted()[1]==0,120,awake=False)
        else:
            triggered=st();key('f9');until(lambda:lc.locked(lc.status()[2]),120,awake=False)
        until(lambda:file_state()==bytes(4096),120,awake=False)
        if reading:throttle(0,False)
        if initial[962]==31:throttle(1,False)
        if reading:assert triggered[962]==30 and triggered[961]==3,'source read finished before the lock request'
        log=(data/'trace.log').read_text(errors='replace')[offset:]
        if a.mobile:
            markers=['MAPS_LOCK|QUIESCED','WRAPPED_VAULT|LOCKED','SIMPLEX_SESSION|SCRUBBED','MAPS_LOCK|CLOSED']
            positions=[log.index(x) for x in markers];assert positions==sorted(positions),log
        interrupts.append(dict(initial_phase=initial[962],trigger_phase=triggered[962],trigger_state=triggered[961],trigger_progress=triggered[971],source_disk_slowed=reading,picker_erased=True))
        unlock();open_maps()
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
        until(lambda:(trusted()[0]!=0 and ui()[0]==0x4e584445534b3031 and ui()[1]==14) if a.mobile else ('TRIO|LOCK_COMPLETE|epoch=1' in (data/'trace.log').read_text(errors='replace') and lc.locked(1)),180,awake=False);assert not active();unlock();open_maps()
    def stage_pack(p):packs.stage_inbox(stage,p)
    def check_saved(label):saved();assert st()[851]==saved_count;screen(label,[NAME,'Saved places']);assert banks()==private_banks;close_saved()
    samples=[];timings=[];saved_count=1
    def exercise_map(label):
        nonlocal private_banks,saved_count
        if st()[880]:
            if st()[961]:tap(80,264);until(lambda:st()[961]==0)
            tap(85,26);until(lambda:not st()[880])
        until(lambda:st()[11]==4 and st()[893] and st()[894]>0 and not st()[881])
        assert 0<=st()[984]<=st()[985]==8*1024*1024 and st()[984]<13816320
        if label!='restarted':assert st()[984]>0
        tap(169 if a.mobile else 369,35);until(lambda:st()[705]);word('vaduz');until(lambda:st()[707]>0)
        screen(label+'-place-search',['Vaduz'])
        if a.mobile:tap_p(80*SCALE,2340-248*SCALE);until(lambda:not st()[706] and st()[9]>600);tap(80,168)
        else:key('ret')
        until(lambda:not st()[705] and st()[513] and not st()[881])
        screen(label+'-place-details',['Vaduz'])
        if label=='v1':
            tap(305 if a.mobile else 505,st()[9]-(74 if a.mobile else 58));until(lambda:st()[849]==1 and st()[851]==2 and not st()[852]);saved_count=2;private_banks=banks();screen('imported-region-saved-place',['Vaduz',NAME]);close_saved()
        tap(335 if a.mobile else 535,st()[9]-(126 if a.mobile else 110));until(lambda:not st()[513]);before=st();tap(275 if a.mobile else 475,35);until(lambda:st()[8]!=before[8] and not st()[881])
        before=st();
        def pointer(x,y,down=None):
            if a.mobile:pos(x*3,80*SCALE+y*3,down)
            else:sc=scene();pos(sc[16]+x,sc[17]+y,down)
        pointer(150,350,True)
        for i in range(1,9):pointer(150+i*7,350-i*5);time.sleep(.04)
        pointer(206,310,False);until(lambda:st()[5:7]!=before[5:7] and not st()[881])
        screen(label+'-panned')
        tap(169 if a.mobile else 369,35);until(lambda:st()[705]);word('altneugut');until(lambda:st()[707]>0)
        screen(label+'-road-search',['Altneugut'])
        if a.mobile:tap_p(80*SCALE,2340-248*SCALE);until(lambda:not st()[706] and st()[9]>600);tap(80,168)
        else:key('ret')
        until(lambda:not st()[705] and st()[513] and not st()[881]);screen(label+'-road-details',['Road'])
        tap(335 if a.mobile else 535,st()[9]-(126 if a.mobile else 110));until(lambda:not st()[513]);tap(329 if a.mobile else 529,35);until(lambda:st()[8]==0 and not st()[881])
        samples.append(dict(label=label,peak_workspace=st()[984],workspace_limit=st()[985],decoded_candidates=st()[986],search_catalogue=st()[987],loaded_region=st()[11],names=st()[894]))
    try:
        connect()
        if not a.mobile:until(lambda:'TRIO|LOCK_COMPLETE|epoch=1' in (data/'trace.log').read_text(errors='replace'),180,awake=False)
        if a.resume:
            if not active():unlock()
            open_maps()
        elif a.mobile:
            until(lambda:ui()[0] and trusted()[0],awake=False);screen('enrollment',['SET PASSPHRASE']);word(PASSWORD);key('ret');screen('confirmation',['same passphrase']);word(PASSWORD);key('ret');screen('enrolled',['swipe']);unlock();open_maps()
        else:lc.enroll();open_maps()
        assert st()[960]==0x4e584d494d503031
        initial_version=st()[967];assert initial_version in (0,1);initial_mask=7 if not initial_version else 15;source_row=0 if not initial_version else 1
        assert st()[968]==initial_mask
        if b'NXMPS002' not in banks():save_place()
        saved();saved_count=st()[851];assert saved_count in (1,2);close_saved();private_banks=banks()
        picker();tap(80,st()[9]-35);until(lambda:st()[961]==0 and not st()[976]);assert st()[968]==initial_mask
        checks.append('trusted Files picker lists encrypted-library source packs; cancelling selection imports nothing')
        picker();assert choose(source_row)==2;screen('selected-file-preview',['Local Liechtenstein','Version'])
        assert st()[980]>=4
        tap(80,264);until(lambda:st()[961]==0 and st()[977]==3)
        checks.append('provider denies forged grants, directory enumeration, oversized and out-of-range reads; backing out revokes the selected grant')
        picker();assert choose(source_row)==2
        tap(100,220);until(lambda:st()[962]==30 and st()[971]>0,120)
        before=st();tap(80,264);until(lambda:st()[961]==7,120)
        assert st()[968]==initial_mask and st()[977]==3;screen('background-read-cancelled',['Import cancelled'])
        checks.append('bounded encrypted background reads publish progress and accept cancellation without installing a region')
        picker();assert choose(source_row)==2
        throttle(1,True);tap(100,220);until(lambda:st()[962]==31 and st()[971]>140000,120)
        screen('background-write-progress',['Installing','Cancel import'])
        tap(80,264);until(lambda:st()[961]==7,120);throttle(1,False)
        assert st()[968]==initial_mask;check_saved('saved-after-cancelled-write')
        checks.append('cancel waits for the current payload DMA, retains the previous catalogue and preserves encrypted saved places')
        picker();assert choose(source_row)==2;throttle(1,True);tap(100,220);until(lambda:st()[962]==31 and st()[971]>140000,300)
        stop();start();assert st()[968]==initial_mask and not st()[976];check_saved('saved-after-interrupted-install-restart')
        checks.append('cold restart during an inactive-bank page write retains the old catalogue and saved places')
        # Lock with an open chooser. Its directory, frame, input and grants are
        # in the supervised erase ranges, then reconstructed without access.
        picker();os_lock();assert st()[976]==0 and st()[977]==0 and st()[968]==initial_mask
        check_saved('saved-after-picker-lock')
        checks.append('OS lock scrubs picker state and selected authority; unlock reconstructs Files and restores private saved places')
        picker();assert choose(source_row)==2
        tap(100,220);until(lambda:st()[962]==30 and st()[971]>0,120)
        os_lock();assert st()[976]==0 and st()[968]==initial_mask
        check_saved('saved-after-read-lock')
        picker();assert choose(source_row)==2;throttle(1,True);tap(100,220);until(lambda:st()[962]==31 and st()[971]>140000,120)
        os_lock();throttle(1,False);assert st()[968]==initial_mask
        check_saved('saved-after-write-lock')
        checks.append('locking during encrypted reads and inactive payload writes revokes access, resets the worker and recovers the prior durable catalogue')
        first_version=initial_version+1
        picker();assert choose(source_row)==2;started=time.monotonic();tap(100,220);until(lambda:st()[961]==5 and st()[13]==0,600);timings.append(dict(version=first_version,install_seconds=time.monotonic()-started))
        assert st()[968]==15 and st()[967]==first_version;check_saved('saved-after-file-import');exercise_map('v'+str(first_version))
        if first_version==1:
            picker();assert choose(1)==2;started=time.monotonic();tap(100,220);until(lambda:st()[961]==5 and st()[13]==0,600);timings.append(dict(version=2,install_seconds=time.monotonic()-started))
            assert st()[967]==2;check_saved('saved-after-file-update');exercise_map('v2')
        checks.append('selected paged Files source installs/updates with unchanged encrypted saved places and no legacy inbox staging')
        stop();start();assert st()[967]==2 and not st()[976]
        check_saved('saved-after-files-cold-restart');exercise_map('restarted')
        picker();assert choose(1)==6;screen('stale-selected-pack',['Pack rejected']);tap(80,264)
        os_lock();assert st()[967]==2;check_saved('saved-after-final-lock')
        checks.append('cold restart, stale-pack rejection and another OS lock/unlock retain installed maps and unchanged saved-place ciphertext')
        if not a.mobile:
            for cycle in range(6):
                picker();tap(80,st()[9]-35);until(lambda:st()[961]==0 and not st()[976]);screen('picker-return-'+str(cycle),['Choose from Files'])
            checks.append('six repeated trusted-picker/source switches pass real GPU readback and retained-texture damage checks')
        assert 'DESKTOP_GPU|FAIL|' not in (data/'trace.log').read_text(errors='replace')
        assert NAME.encode() not in (data/'maps.img').read_bytes()
        result=dict(passed=not a.development_sources,mobile=a.mobile,checks=checks,lock_observations=interrupts,loader_sha256=m['loader_sha256'],saved_data_used=False,pixel_wifi_touched=False,region_versions=list(st()[964:968]),initial_version=initial_version,samples=samples,install_timings=timings,source_bytes=13816320,scope='13.8 MB prepared paged package through segmented encrypted Files; fixed mutable workspace; public fixture source blobs are read-only test-only assets and excluded from workspace figure')
        (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');session.update(maps_large_files_passed=not a.development_sources,large_files_validation=str(out/'validation.json'));(stage/'session.json').write_text(json.dumps(session,indent=2)+'\n')
        print('PASS larger Files imports, search, pan/zoom and lock/restart recovery',session['uuid'],flush=True)
    finally:
        if q:
            try:
                for drive in cfg['Drive']:q.send('block_set_io_throttle',dict(device='drive'+drive['Identifier'],bps=0,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0))
            finally:q.close()
if __name__=='__main__':main()
