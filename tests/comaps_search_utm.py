#!/usr/bin/env python3
"""Offline name search, real mobile keyboard and result navigation in UTM."""
import argparse,hashlib,json,os,plistlib,re,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
from desktop_gpu_capture import capture
ROOT=Path(__file__).resolve().parents[1];SCALE=2.6086957
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--mobile',action='store_true');a=p.parse_args()
    stage=a.guest.resolve();assert stage.is_relative_to(ROOT/('build/mobile-browser' if a.mobile else 'build/desktop-apps')) and (stage/'.disposable-build').is_file()
    m=json.loads((stage/'build.json').read_text());session=json.loads((stage/'session.json').read_text());assert m['map_storage'] and m['saved_data_used'] is session['saved_data_used'] is False
    data=Path(session['bundle'])/'Data';assert data.parent.parent==stage and data.resolve()==data
    cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid'] and [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
    assert not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==m['loader_sha256']==session['loader_sha256']
    assert all(hashlib.sha256((ROOT/s).read_bytes()).hexdigest()==h for s,h in m['sources'].items())
    assert not host_locked(),'Mac must be unlocked'
    evidence=stage/'search-verification';evidence.mkdir(exist_ok=True);checks=[];samples=[]
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
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<Q',read(0x5fb96000,8))[0] and v[704]==0x4e584d5345413031:return v,raw
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
        for c in text:key({' ':'spc','-':'minus','.':'dot'}.get(c,c.lower()),c.isupper())
        until(lambda:query()==text)
    def soft(char):
        bottom=2340/SCALE;w=1080/SCALE
        if char in 'qwertyuiop':i='qwertyuiop'.index(char);x=4+i*(w-8)/10+((w-8)/10-2)/2;y=bottom-228+59
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
            if page==14:
                # Only the public disposable credential from private Maps tests.
                assert m.get('map_private')
                position(540,1900,True);time.sleep(.1);position(540,1500);time.sleep(.1);position(540,1500,False);time.sleep(.2)
                for c in 'disposabledevice42':key(c);time.sleep(.12)
                key('ret');until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==0);page=0
            if page!=19:
                if page:
                    x,y=(40,65) if page==16 else (370,60) if page==18 else (350,136)
                    tap_physical(x*SCALE,y*SCALE);until(lambda:struct.unpack('<Q',read(0x5fb92008,8))[0]==0)
                tap_physical(300*SCALE,360*SCALE)
        else:until(lambda:mode()[1] and scene()[0]);w,h=mode()[2]//2,mode()[3]//2;tap_physical((w-360)//2+306,h-65)
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
            text=subprocess.run(['tesseract',str(target),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=25).stdout
            norm=lambda t:re.sub('[^a-z0-9]','',t.lower())
            if all(norm(w) in norm(text) for w in words):(evidence/(name+'.txt')).write_text(text);return
            time.sleep(.12)
        raise AssertionError((name,words,text))
    try:
        q.send('set_link',{'name':'net0','up':False})
        until(state);open_maps()
        if s()[11]!=1:choose(0)
        begin();assert not query() and s()[710]>0
        if a.mobile:
            for c in 'gibraltar':soft(c)
            until(lambda:query()=='gibraltar');soft('DELETE');until(lambda:query()=='gibralta');soft('r');until(lambda:query()=='gibraltar')
        else:word('gIbRaLtAr')
        first=results()[0];assert first['name']=='Gibraltar' and first['kind']==0 and first['rank']==0
        screenshot('place-results',['Gibraltar','Place','Choose a result']);samples.append(dict(query=query(),results=results()))
        clear();word('kInGsWaY');until(lambda:s()[707]>0)
        roads=results();assert len([r for r in roads if r['name'].lower()=='kingsway' and r['kind']==1])==1
        screenshot('road-results',['Kingsway','Road']);samples.append(dict(query=query(),results=roads))
        if a.mobile:
            tap_physical(80*SCALE,2340-248*SCALE);until(lambda:not s()[706] and s()[9]>600)
        chosen=results()[0];tap(80,64+82+22);until(lambda:not s()[705] and s()[513]==chosen['id'] and s()[8]>=3)
        if a.mobile:until(lambda:s()[9]>600)
        screenshot('road-opened',['Kingsway','Road','Map point']);checks.append('real mobile keyboard or desktop typing, backspace/clear, case-insensitive place/road matches and deduplicated road result opens details')
        begin();word('road');until(lambda:s()[707]==8 and s()[711]);camera=s()[5:9]
        pointer(100,210,True)
        for i in range(1,7):pointer(100,210-12*i);time.sleep(.025)
        pointer(100,138,False);until(lambda:s()[709]>0);assert s()[705] and not s()[513] and s()[5:9]==camera and s()[708]>=s()[709]
        if not a.mobile:
            for _ in range(8):key('down')
            until(lambda:s()[708]==7 and s()[709]>0)
            chosen=results()[7];key('ret');until(lambda:not s()[705] and s()[513]==chosen['id'])
            begin()
        clear();word('zzzzqwertynotaplace');assert not s()[707];screenshot('no-matches',['No matches in this region'])
        clear()
        for _ in range(60):key('x')
        until(lambda:len(query())==48);key('backspace');until(lambda:len(query())==47)
        checks.append('bounded first-eight results, result-list drag without map pan, keyboard navigation, no-match state and query capacity')
        clear();word('kingsway');saved=query()
        if a.mobile:tap_physical(370*SCALE,60*SCALE)
        else:w,h=mode()[2]//2,mode()[3]//2;tap_physical((w-360)//2+54,h-65)
        until(lambda:not s()[4] and not s()[706]);time.sleep(.2)
        # Private builds deliberately wipe queries and the retained frame on
        # focus loss. Non-private builds retain the search for navigation.
        expected='' if m.get('map_private') else saved
        old=s()[2];key('a');time.sleep(.2);assert s()[2]==old and query()==expected and not s()[706]
        open_maps();assert query()==expected and bool(s()[705])==(not m.get('map_private')) and not s()[706]
        if not s()[705]:begin()
        else:tap(80,64+28);until(lambda:s()[706])
        clear();close()
        if not a.mobile:key('f',ctrl=True);until(lambda:s()[705]);key('esc');until(lambda:not s()[705])
        begin();word('gibraltar');choose(1);begin();word('gibraltar');assert not s()[707]
        clear();word('san marino');first=results()[0];assert first['name']=='San Marino' and first['kind']==0
        screenshot('san-marino-results',['San Marino','Place']);samples.append(dict(query=query(),results=results()))
        key('ret');until(lambda:not s()[705] and s()[513]==first['id']);screenshot('san-marino-opened',['San Marino','Place','Map point'])
        choose(0);assert not query() and not s()[707];checks.append('hidden-app input isolation, keyboard hide/restore, search cancellation, region-scoped index rebuild and San Marino result selection')
        choose(2);assert s()[8]==0 and not s()[891] and s()[893] and s()[894]==3275
        # Altneugut is a minor road absent from the overview LOD. Ruggell is
        # outside the subsequently loaded southern viewport: both must be
        # found from the same immutable whole-region index.
        for name,kind in (('Altneugut',1),('Ruggell',0)):
            if name=='Ruggell':
                st=s();cy=struct.unpack('<q',struct.pack('<Q',st[6]))[0]/1e9;units=st[7]/1e12
                catalogue=(stage/'spatial/Liechtenstein.nxsearch').read_bytes()
                target=next(struct.unpack_from('<96sffI',catalogue,at) for at in range(16,len(catalogue),108) if catalogue[at:at+96].split(b'\0',1)[0]==b'Ruggell')
                assert abs(target[2]-cy)>(st[9]-64-(52 if a.mobile else 36))*units*.75,'destination must be outside loaded viewport margin'
            begin();assert s()[710]==3275;word(name);first=results()[0]
            assert first['name']==name and first['kind']==kind and first['rank']==0
            screenshot('whole-region-'+name.lower(),[name,'Choose a result']);samples.append(dict(query=query(),results=results()))
            key('ret');until(lambda:not s()[705] and s()[13]==0 and s()[891] and s()[513])
            if a.mobile:until(lambda:s()[9]>600)
            until(lambda:state()[1][520*8:520*8+96].split(b'\0',1)[0].decode()==name)
            assert s()[893] and s()[894]==3275 and s()[884]<=32768 and s()[885]<=524288
            screenshot('whole-region-opened-'+name.lower(),[name,'Road' if kind else 'Place','Map point'])
        checks.append('whole-region index finds minor road absent from overview and place outside loaded viewport; asynchronous detail loads resolve original feature and details')
        begin();word('Altneugut');assert results()[0]['name']=='Altneugut';close();choose(0);begin();word('Altneugut');assert not s()[707];close()
        assert not host_locked()
        result=dict(passed=True,mobile=a.mobile,uuid=session['uuid'],loader_sha256=m['loader_sha256'],network_disconnected_during_search=True,saved_data_used=False,pixel_wifi_touched=False,checks=checks,samples=samples,scope='disposable UTM; QMP hardware/touch events, actual native map results and composed pixels; whole-region place/road names, not addresses/global search or physical Pixel touch')
        (evidence/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
    finally:
        try:
            q.send('set_link',{'name':'net0','up':True})
            q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
        finally:q.close()
if __name__=='__main__':main()
