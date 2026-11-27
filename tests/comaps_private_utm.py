#!/usr/bin/env python3
"""Disposable-only saved-places encryption, lock and restart recovery."""
import argparse,hashlib,json,os,plistlib,re,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1];SCALE=2.6086957
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--mobile',action='store_true');a=p.parse_args()
    stage=a.guest.resolve();assert stage.is_relative_to(ROOT/('build/mobile-browser' if a.mobile else 'build/desktop-apps')) and (stage/'.disposable-build').is_file()
    m=json.loads((stage/'build.json').read_text());session=json.loads((stage/'session.json').read_text());assert m['map_storage'] and m['map_private'] and m['saved_data_used'] is session['saved_data_used'] is False
    data=Path(session['bundle'])/'Data';assert data.parent.parent==stage and data.resolve()==data
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid'] and [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
    assert not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    assert all(hashlib.sha256((ROOT/s).read_bytes()).hexdigest()==h for s,h in m['sources'].items())
    assert not host_locked(),'Mac must be unlocked'
    evidence=stage/'private-verification';evidence.mkdir(exist_ok=True);checks=[];samples=[]
    (evidence/'harness-used.py').write_bytes(Path(__file__).read_bytes())
    q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
    try:q.send('query-status')
    except RuntimeError as e:
        if 'Expecting capabilities negotiation' not in str(e):raise
        q.send('qmp_capabilities')
    def read(address,n):
        path=data/'map-search-state.bin'
        try:q.send('pmemsave',{'val':address,'size':n,'filename':str(path)});return path.read_bytes()
        finally:path.unlink(missing_ok=True)
    def state():
        for _ in range(100):
            raw=read(0x5fb96000,8192);v=struct.unpack('<1024Q',raw)
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<Q',read(0x5fb96000,8))[0] and v[848]==0x4e584d5052495631:return v,raw
        return None
    def until(fn,seconds=45):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            text=(data/'trace.log').read_text(errors='replace');assert not any(x in text for x in ('COMAPS|FAIL|','VMFault','panicked at','MON|ERROR','DESKTOP_GPU|FAIL|')),text[-3000:]
            value=fn()
            if value:return value
            time.sleep(.03)
        raise TimeoutError('search state '+str(state()[0][704:712] if state() else None)+' query '+query())
    def s():return until(state)[0]
    def query():
        v=state();return v[1][712*8:712*8+49].split(b'\0',1)[0].decode('utf8',errors='replace') if v else ''
    def results():
        st,raw=until(state);assert st[707]<=8
        out=[]
        for i in range(st[707]):
            n=720+i*16;out.append(dict(id=st[n],kind=st[n+1],rank=st[n+2],name=raw[(n+3)*8:(n+15)*8].split(b'\0',1)[0].decode('utf8',errors='replace')))
        assert len({(r['kind'],r['name'].lower()) for r in out})==len(out)
        return out
    def mode():return struct.unpack('<6I',read(0x5fb92000,24))
    def scene():return struct.unpack('<20I',read(0x5fbb4000,80))
    def position(x,y,down=None):
        w,h=(1080,2340) if a.mobile else (mode()[2]//2,mode()[3]//2)
        ev=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in [('x',x,w),('y',y,h)]]
        if down is not None:ev.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':ev})
    def tap_physical(x,y):position(x,y,True);time.sleep(.07);position(x,y,False)
    def pointer(x,y,down=None):
        if a.mobile:position(x*3,80*SCALE+y*3,down)
        else:sc=scene();position(sc[16]+x,sc[17]+y,down)
    def tap(x,y):pointer(x,y,True);time.sleep(.07);pointer(x,y,False)
    def key(code,shift=False,ctrl=False):
        ev=[]
        for name,on in [('shift',shift),('ctrl',ctrl)]:
            if on:ev.append({'type':'key','data':{'down':True,'key':{'type':'qcode','data':name}}})
        ev.extend({'type':'key','data':{'down':v,'key':{'type':'qcode','data':code}}} for v in (True,False))
        for name,on in [('ctrl',ctrl),('shift',shift)]:
            if on:ev.append({'type':'key','data':{'down':False,'key':{'type':'qcode','data':name}}})
        q.send('input-send-event',{'events':ev})
    def word(text):
        for c in text:
            key({' ':'spc','-':'minus','.':'dot'}.get(c,c.lower()),c.isupper());time.sleep(.06)
        time.sleep(.1)
        until(lambda:query()==text)
    def soft(char):
        bottom=2340/SCALE;w=1080/SCALE
        if char in '1234567890':i='1234567890'.index(char);x=4+i*(w-8)/10+((w-8)/10-2)/2;y=bottom-228+21
        elif char in 'qwertyuiop':i='qwertyuiop'.index(char);x=4+i*(w-8)/10+((w-8)/10-2)/2;y=bottom-228+59
        elif char in 'asdfghjkl':i='asdfghjkl'.index(char);x=17+i*(w-34)/9+((w-34)/9-2)/2;y=bottom-228+97
        elif char in 'zxcvbnm':i='zxcvbnm'.index(char);x=37+i*(w-74)/7+((w-74)/7-2)/2;y=bottom-228+135
        elif char=='DELETE':x=10+3*(w-8)/4+((w-8)/4-2)/2;y=bottom-55
        elif char=='CLEAR':x=4+(w-12)/4;y=bottom-20
        else:raise ValueError(char)
        tap_physical(x*SCALE,y*SCALE)
    def clear():
        if a.mobile and s()[706]:soft('CLEAR')
        else:key('u',ctrl=True)
        until(lambda:query()=='' and s()[707]==0)
    def open_maps():
        if a.mobile:
            page=struct.unpack('<Q',read(0x5fb92008,8))[0]
            if page!=19:
                if page:
                    x,y=(40,65) if page==16 else (370,60) if page==18 else (350,136)
                    tap_physical(x*SCALE,y*SCALE);until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==0)
                tap_physical(300*SCALE,360*SCALE)
        else:
            until(lambda:mode()[1] and scene()[0]);last=None;since=time.monotonic()
            def settled():
                nonlocal last,since
                m=mode();g=struct.unpack('<32I',read(0x5fba0000,128))
                if not g[0] or g[0]&1 or tuple(g[9:11])!=tuple(m[2:4]) or g[11]!=m[1]:return False
                if m!=last:last=m;since=time.monotonic();return False
                return time.monotonic()-since>=.8
            until(settled)
            w,h=mode()[2]//2,mode()[3]//2;tap_physical((w-360)//2+306,h-65)
        until(lambda:s()[4] and s()[13]==0)
    def begin():
        tap(169 if a.mobile else 369,35);until(lambda:s()[705] and s()[706])
        if a.mobile:until(lambda:s()[9]<600)
    def close():
        if s()[705]:tap((360 if a.mobile else 560)-28,64+29);until(lambda:not s()[705] and not s()[706])
        if a.mobile:until(lambda:s()[9]>600)
    def choose(region):
        old=s()[15];tap(85,26);time.sleep(.12);tap(85,92+region*48)
        until(lambda:s()[11]==region+1 and s()[15]>old and s()[13]==0);assert not s()[705] and not s()[707] and not query()
    def screenshot(name,words):
        end=time.monotonic()+30
        while time.monotonic()<end:
            path=data/'map-search-screen.ppm';capture(q,path,not a.mobile);target=evidence/(name+'.png')
            subprocess.run(['magick',str(path),str(target)],check=True)
            ocr=target
            if not a.mobile:
                sc=scene();ocr=data/'private-map-crop.png'
                subprocess.run(['magick',str(target),'-crop',f'{560*2}x{s()[9]*2}+{sc[16]*2}+{sc[17]*2}','+repage','-colorspace','Gray','-negate',str(ocr)],check=True)
            text=subprocess.run(['tesseract',str(ocr),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=25).stdout
            norm=lambda t:re.sub('[^a-z0-9]','',t.lower())
            if all(norm(w) in norm(text) for w in words):(evidence/(name+'.txt')).write_text(text);return
            time.sleep(.12)
        raise AssertionError((name,words,text))
    PASSWORD='mapstestprivate42';PRIVATE_NAME='secretcafe42'
    CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
    def ctl(*args):return subprocess.run([CTL,*args],capture_output=True,text=True,timeout=30)
    def write_word(text):
        for c in text:
            key({' ':'spc','-':'minus','.':'dot'}.get(c,c.lower()),c.isupper());time.sleep(.06)
        time.sleep(.1)
    def private_state():return s()[849:857]
    def opened(count):return s()[850]==1 and s()[851]==count and not s()[852]
    def saved():
        tap((360 if a.mobile else 560)-130,s()[9]-16)
        until(lambda:s()[849] in (1,2,3,4,5) and not s()[852])
        if a.mobile and s()[849] in (2,3,4):until(lambda:s()[9]<600)
    def unlock(count):
        saved();assert s()[849]==4,private_state();write_word(PASSWORD);key('ret');until(lambda:opened(count));
        if a.mobile:until(lambda:s()[9]>600)
    def lock():
        if s()[849]!=1:saved()
        tap((360 if a.mobile else 560)-65,64+28)
        until(lambda:s()[850]==0 and s()[851]==0 and s()[854]==1 and s()[849]==0)
    def public_result(name):
        assert not s()[849];begin();word(name)
        if a.mobile:tap_physical(80*SCALE,2340-248*SCALE);until(lambda:not s()[706] and s()[9]>600)
        selected=results()[0];tap(80,168);until(lambda:not s()[705] and s()[513]==selected['id'])
        if a.mobile:until(lambda:s()[9]>600)
    def save_selected():tap((360 if a.mobile else 560)-55,s()[9]-(52 if a.mobile else 36)-22)
    def close_list():tap((360 if a.mobile else 560)-25,64+28);until(lambda:s()[849]==0)
    def stop():
        nonlocal q
        assert session['saved_data_used'] is False and data.parent.parent==stage
        if q:
            try:q.send('quit')
            finally:q.close();q=None
        until_at=time.monotonic()+45
        while time.monotonic()<until_at:
            if ctl('status',session['uuid']).stdout.strip()=='stopped':return
            time.sleep(.2)
        raise TimeoutError('disposable VM did not stop; disk unchanged')
    def start():
        nonlocal q
        assert ctl('status',session['uuid']).stdout.strip()=='stopped'
        r=ctl('start',session['uuid']);assert r.returncode==0,r.stderr
        q=MotionQmp(data/'monitor');q.connect();until(state,90)
        if a.mobile:until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==16,90)
        open_maps()
        assert s()[850]==0 and s()[851]==0 and s()[854]==1
    def disk_banks():
        with (data/'maps.img').open('rb') as f:f.seek(1024*1024);return f.read(8192)
    def replace_banks(b):
        assert len(b)==8192 and ctl('status',session['uuid']).stdout.strip()=='stopped'
        with (data/'maps.img').open('r+b') as f:f.seek(1024*1024);f.write(b);f.flush();os.fsync(f.fileno())
    try:
        open_maps()
        if s()[11]!=1:choose(0)
        public_result('gibraltar');save_selected();until(lambda:s()[849]==2 and not s()[852]);
        if a.mobile:
            until(lambda:s()[9]<600)
            for c in PASSWORD:soft(c)
            tap_physical(300*SCALE,2340-248*SCALE)
        else:write_word(PASSWORD);key('ret')
        until(lambda:s()[849]==3)
        write_word(PASSWORD);key('ret');until(lambda:opened(1),90)
        if a.mobile:until(lambda:s()[9]>600)
        screenshot('saved-place',['Saved places','Gibraltar','Rename','encrypted'])
        tap((360 if a.mobile else 560)-90,64+62+22);until(lambda:s()[849]==6)
        write_word(PRIVATE_NAME);key('ret');until(lambda:opened(1) and s()[849]==1)
        if a.mobile:until(lambda:s()[9]>600)
        screenshot('renamed-place',['Saved places',PRIVATE_NAME])
        close_list();public_result('gibraltar');save_selected();until(lambda:opened(1));screenshot('duplicate-keeps-name',[PRIVATE_NAME,'Already saved'])
        close_list();public_result('kingsway');save_selected();until(lambda:opened(2));screenshot('saved-place-and-road',[PRIVATE_NAME,'Kingsway'])
        close_list();choose(1);public_result('san marino');save_selected();until(lambda:opened(3))
        before=s()[5:9];pointer(80,64+62+22,True)
        for i in range(1,7):pointer(80,64+62+22+i*12);time.sleep(.035)
        pointer(80,64+62+22+72,False);time.sleep(.1);assert s()[849]==1 and s()[851]==3 and s()[5:9]==before and not s()[855]
        tap(80,64+62+22);until(lambda:s()[11]==1 and s()[855]==1 and s()[849]==0 and s()[13]==0);screenshot('cross-region-saved-place',[PRIVATE_NAME,'private'])
        saved();checks.append('enrollment and confirmation, place/road saves in both regions, private rename, cross-region opening and durable flush acknowledgement')
        lock();screenshot('locked-map',['Gibraltar','CoMaps'])
        saved();assert s()[849]==4;write_word('wrongpassphrase');key('ret');screenshot('wrong-passphrase',['Unable to unlock','Passphrase hidden']);assert s()[850]==0 and s()[851]==0
        time.sleep(2.1);write_word(PASSWORD);key('ret');until(lambda:opened(3),90)
        if a.mobile:until(lambda:s()[9]>600)
        tap(80,64+62+22);until(lambda:s()[855]==1 and s()[849]==0);screenshot('opened-saved-place',[PRIVATE_NAME,'private'])
        if a.mobile:tap_physical(370*SCALE,60*SCALE)
        else:w,h=mode()[2]//2,mode()[3]//2;tap_physical((w-360)//2+54,h-65)
        until(lambda:not s()[4] and not s()[850] and s()[854]==1)
        key('a');assert s()[851]==0
        open_maps();unlock(3);screenshot('focus-recovery',[PRIVATE_NAME,'Kingsway'])
        checks.append('wrong password denied; explicit lock and focus loss evict key and clear structured bookmarks/passphrase buffers; authenticated reopen restores records')
        generation=s()[853];stop();raw=disk_banks()
        disk=(data/'maps.img').read_bytes();assert PRIVATE_NAME.encode() not in disk and PASSWORD.encode() not in disk
        assert b'NXMPS001' in raw and b'NXVKP001' in raw
        start();unlock(3);assert s()[853]==generation;screenshot('cold-restart-recovered',[PRIVATE_NAME,'Kingsway'])
        checks.append('cold restart starts locked and restores all three records only after authentication; no plaintext name/passphrase in raw map disk')
        stop();original=disk_banks();gens=[struct.unpack_from('<Q',original,i*4096+8)[0] for i in range(2)];latest=max(range(2),key=lambda i:gens[i]);other=1-latest
        torn=bytearray(original);candidate=bytearray(original[latest*4096:(latest+1)*4096]);struct.pack_into('<Q',candidate,8,gens[latest]+1);candidate[1024:4096]=bytes(3072);torn[other*4096:(other+1)*4096]=candidate
        replace_banks(torn);start();unlock(3);assert s()[853]==generation;screenshot('interrupted-save-recovered',[PRIVATE_NAME,'Kingsway'])
        checks.append('interrupted newer bank fails authentication and falls back to intact last acknowledged save')
        stop();bad=bytearray(original);bad[:8]=b'BROKEN!!';bad[4096:4104]=b'BROKEN!!';replace_banks(bad);start();saved();assert s()[849]==5 and not s()[850];screenshot('both-banks-damaged',['storage is damaged']);key('ret');assert not s()[850] and s()[849]==5
        stop();replace_banks(original)
        # A failed public region load must not apply its private coordinate to
        # another region when the user subsequently returns to that map.
        with (data/'maps.img').open('r+b') as f:
            f.seek(16*1024*1024+4000);public_byte=f.read(1);f.seek(-1,1);f.write(bytes([public_byte[0]^128]));f.flush();os.fsync(f.fileno())
        start();unlock(3);tap(80,64+62+2*48+22);until(lambda:s()[13]==2)
        assert s()[11]==1 and not s()[855];choose(0);assert not s()[855] and s()[851]==3
        screenshot('unavailable-region-retains-public-map',['Gibraltar','CoMaps'])
        stop()
        with (data/'maps.img').open('r+b') as f:
            f.seek(16*1024*1024+4000);f.write(public_byte);f.flush();os.fsync(f.fileno())
        start();unlock(3);tap(80,64+62+2*48+22);until(lambda:s()[11]==2 and s()[855]==3 and s()[13]==0)
        screenshot('saved-region-restored',['San Marino','private','Map point']);saved();tap(80,64+62+22);until(lambda:s()[11]==1 and s()[855]==1 and s()[13]==0);saved()
        checks.append('unavailable public region does not apply a stale saved location to the wrong map; restoring the region reopens the saved place')
        tap((360 if a.mobile else 560)-25,64+62+48+22);until(lambda:opened(2));screenshot('deleted-road',[PRIVATE_NAME]);assert s()[851]==2
        stop();start();unlock(2);screenshot('delete-survived-restart',[PRIVATE_NAME]);lock()
        checks.append('damaged banks fail closed without reenrollment; restored ciphertext recovers; deletion survives restart')
        if a.mobile:
            device_password='disposabledevice42'
            tap_physical(370*SCALE,60*SCALE);until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==0)
            tap_physical(300*SCALE,540*SCALE);until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==8)
            tap_physical(200*SCALE,257*SCALE);until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==13)
            tap_physical(200*SCALE,422*SCALE);until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==14)
            screenshot('device-enrollment',['SET PASSPHRASE'])
            write_word(device_password);key('ret');screenshot('device-confirm',['CONFIRM PASSPHRASE'])
            write_word(device_password);key('ret');screenshot('device-enrolled-locked',['DEVICE LOCKED'])
            def device_unlock():
                position(540,1900,True);time.sleep(.08);position(540,1500);time.sleep(.08);position(540,1500,False)
                write_word(device_password);key('ret');until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==0,90)
            device_unlock();open_maps();unlock(2)
            until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==14 and not s()[850] and s()[854],85)
            screenshot('device-idle-lock',['DEVICE LOCKED']);old=s()[2];key('a');time.sleep(.2);assert s()[2]==old and not s()[850]
            device_unlock();open_maps();assert not s()[850];unlock(2);screenshot('device-unlock-maps-recovery',[PRIVATE_NAME]);lock()
            checks.append('mobile device enrollment, device auto-lock while Maps is open, blocked Maps input and separate Maps authentication after device unlock')
        assert not host_locked()
        result=dict(passed=True,mobile=a.mobile,uuid=session['uuid'],loader_sha256=m['loader_sha256'],harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),saved_data_used=False,pixel_wifi_touched=False,checks=checks,scope='disposable data, app-scoped separate Maps passphrase vault; QMP hardware/touch and composed pixels. Not hardware-backed retries, full-session lock, physical Pixel or whole-memory erasure')
        (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
    finally:
        if q:
            try:q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
            finally:q.close()
if __name__=='__main__':main()
