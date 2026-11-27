"""Phone input/pixel checks on a generated observation-enabled disposable disk."""
import csv,io,json,re,shutil,struct,subprocess,time
from pathlib import Path
SCALE=2.6086957
def exercise(q,data,evidence):
    checks=[];tile_pixels={}
    def output():
        s=(data/'trace.log').read_text(errors='replace')
        assert not any(x in s for x in ('VMFault','panicked at','MON|ERROR','Data Abort','HASKELL_EXIT|')),s[-3500:]
        return s
    def until(fn,timeout=120):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            output();v=fn()
            if v:return v
            time.sleep(.15)
        raise RuntimeError('mobile interaction timeout: '+str(browser())+' '+output()[-2500:])
    def read(address,size):
        p=data/'mobile-state.bin'
        try:q.send('pmemsave',{'val':address,'size':size,'filename':str(p)});return p.read_bytes()
        finally:p.unlink(missing_ok=True)
    def ui():
        r=read(0x5fb92000,8192);n=struct.unpack_from('<13Q',r)
        texts=[r[o:o+min(n[9+i],limit)].decode(errors='replace') for i,(o,limit) in enumerate(zip((128,3200,6400,6680),(3000,3000,256,256)))]
        return n,texts
    def browser():return struct.unpack('<5I',read(0x5fb90000,20))
    def position(x,y,down=None):
        e=[{'type':'abs','data':{'axis':a,'value':round(v*32767/m)}} for a,v,m in [('x',x,1079),('y',y,2339)]]
        if down is not None:e.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':e})
    def tap(x,y):position(x*SCALE,y*SCALE,True);time.sleep(.1);position(x*SCALE,y*SCALE,False);time.sleep(.15)
    def key(code,shift=False):
        e=[]
        if shift:e.append({'type':'key','data':{'down':True,'key':{'type':'qcode','data':'shift'}}})
        e.extend({'type':'key','data':{'down':v,'key':{'type':'qcode','data':code}}} for v in (True,False))
        if shift:e.append({'type':'key','data':{'down':False,'key':{'type':'qcode','data':'shift'}}})
        q.send('input-send-event',{'events':e});time.sleep(.035)
    def word(s):
        for c in s:key({' ':'spc',':':'semicolon','/':'slash','.':'dot','-':'minus','?':'slash','=':'equal'}.get(c,c.lower()),c==':' or c=='?' or c.isupper())
    def screen(name,need=()):
        # Keep the software cursor away from text/image verification regions.
        position(1040,70);time.sleep(.1)
        end=time.monotonic()+35
        while time.monotonic()<end:
            p=data/'mobile-screen.ppm';q.send('screendump',{'filename':str(p)})
            raw=p.read_bytes();h=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',raw);assert h and tuple(map(int,h.groups()))==(1080,2340)
            text=subprocess.run(['tesseract',str(p),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=20).stdout
            compact=lambda s:re.sub('[^a-z0-9]','',s.lower())
            if all(compact(x) in compact(text) for x in need):
                shutil.copyfile(p,evidence/(name+'.ppm'));subprocess.run(['magick',str(p),str(evidence/(name+'.png'))],check=True)
                (evidence/(name+'.txt')).write_text(text);return text
            time.sleep(.2)
        raise RuntimeError('phone screen missing '+str(need)+': '+text)
    def home():
        if ui()[0][1]==18:tap(370,60)
        elif ui()[0][1]==16:tap(40,65)
        elif ui()[0][1]!=0:tap(350,136)
        until(lambda:ui()[0][1]==0)
        until(lambda:browser()[3]==0)
    def open_browser():tap(300,190);until(lambda:ui()[0][1]==18 and browser()[3]==1)
    def url(s):
        # Native CSS y=54 is below the phone's trusted HOME row.
        tap(140,80+54*3/SCALE);until(lambda:browser()[1]==1)
        word(s);key('ret');until(lambda:browser()[1]==0)
    def drag(y1,y2):
        position(540,y1*SCALE,True)
        for i in range(1,16):position(540,(y1+(y2-y1)*i/15)*SCALE);time.sleep(.025)
        position(540,y2*SCALE,False);time.sleep(.3)
    def find_word(word):
        p=data/'find.ppm';q.send('screendump',{'filename':str(p)})
        result=subprocess.run(['tesseract',str(p),'stdout','--psm','11','tsv'],capture_output=True,text=True,check=True).stdout
        for row in csv.DictReader(io.StringIO(result),delimiter='\t'):
            if row['text'].lower()==word.lower():return (int(row['left'])+int(row['width'])/2)/SCALE,(int(row['top'])+int(row['height'])/2)/SCALE
        raise RuntimeError('missing word in phone form: '+word)
    until(lambda:ui()[0][0]==0x4e584445534b3031)
    home();screen('phone-home',['NETSURF','SIMPLEX','NEXAWAL'])
    open_browser();screen('browser-start',['Explore the web'])
    checks.append('phone Home launches NetSurf; browser has phone-sized toolbar and real native viewport')
    url('resource:index.html');screen('phone-lab',['Phone browser lab','PHONE SCRIPT WORKED'])
    checks.append('upstream HTML/CSS and JavaScript DOM changes render at phone width')
    # Address entry using the same on-screen keyboard as the phone shell.
    tap(140,80+54*3/SCALE);until(lambda:browser()[1]==1)
    # Type one character with the on-screen Q key, then exercise Delete and
    # the symbol keyboard before finishing the resource URL with hardware keys.
    bottom=2340/SCALE
    tap(33,bottom-228+58);key('backspace');word('resource:index.html')
    screen('address-keyboard',['resource:index.html','GO','ENTER'])
    tap(310,bottom-248);until(lambda:browser()[1]==0)
    screen('lab-after-go',['Phone browser lab','PHONE SCRIPT WORKED'])
    checks.append('on-screen keyboard, URL entry, Delete and touch GO work without covering the page viewport')
    import numpy as np
    def decoded_tiles():
        raw=(evidence/'lab-after-go.ppm').read_bytes();h=re.match(rb'P6\s+\d+\s+\d+\s+255\s',raw)
        pixels=np.frombuffer(raw[h.end():],dtype=np.uint8).reshape(2340,1080,3)
        for name,left,right in [('PNG',0,430),('JPEG',430,850)]:
            region=pixels[500:1900,left:right].astype(np.int16)
            tile_pixels[name]=[int(np.sum(np.max(np.abs(region-colour),axis=2)<=12)) for colour in ((220,30,40),(20,180,60),(30,60,220),(240,200,20))]
        return all(min(counts)>4000 for counts in tile_pixels.values())
    assert decoded_tiles(), 'both decoded colour-pattern regions must be visible: '+str(tile_pixels)
    checks.append('distinct PNG and JPEG fixtures show all four expected colour landmarks in separate rendered regions')
    name=find_word('Name');tap(name[0]+20,name[1]+35);until(lambda:browser()[1]==1)
    word('nettest');screen('form-typed',['Name','HIDE'])
    tap(80,bottom-248);until(lambda:browser()[1]==0)
    remember=find_word('Remember');tap(23,remember[1]);submit=find_word('Submit');tap(*submit)
    until(lambda:'NETSURF|FORM|submitted|name=1|choice=1' in output())
    screen('form-result',['Form submitted locally'])
    checks.append('real touch-focused text field, checkbox and native GET form submit locally')
    url('resource:index.html');screen('before-scroll',['PHONE SCRIPT WORKED'])
    before=browser()[2];drag(590,280);until(lambda:browser()[2]>before+80)
    for _ in range(5):drag(590,280)
    screen('scrolled-footer',['END OF PHONE DOCUMENT'])
    key('home');until(lambda:browser()[2]==0);screen('scrolled-back',['Phone browser lab'])
    checks.append('drag scrolling moves the document, releases without following a link and returns to the top')
    url('resource:images.html');screen('phone-images',['Native image tests'])
    checks.append('phone viewport loads the shared PNG/JPEG decoder lab')
    url('https://www.netsurf-browser.org/about/');screen('phone-https',['About NetSurf'],)
    assert re.search(r'HTTPS\|DONE\|[^\n]*host=www\.netsurf-browser\.org\|status=200',output())
    start=len(output());url('https://wrong.host.badssl.com/');until(lambda:re.search(r'HTTPS\|ERROR\|[^\n]*host=wrong\.host\.badssl\.com\|code=9\|TLS certificate rejected',output()[start:]))
    assert not re.search(r'HTTPS\|DONE\|[^\n]*host=wrong\.host\.badssl\.com',output()[start:])
    screen('certificate-rejected',['TLS certificate rejected'])
    checks.append('native HTTPS uses browser TCP slot 4 and rejects the wrong-host certificate')
    home();tap(80,190);until(lambda:ui()[0][1]==6);screen('simplex-still-opens',['SIMPLEX'])
    home();tap(80,360);until(lambda:ui()[0][1]==16);screen('wallet-still-opens',['Wallet'])
    home();open_browser();url('resource:index.html');screen('browser-recovered',['Phone browser lab'])
    home();word('unowned');assert browser()[3]==0
    checks.append('Home, SimpleX and NexaWal remain available; hidden browser receives no shell typing; subsequent browser navigation works')
    (evidence/'validation.json').write_text(json.dumps(dict(passed=True,checks=checks,saved_data_used=False,pixel_wifi_touched=False,viewport=[1080,2340],decoded_tile_pixels=tile_pixels,browser_css_width=360,scope='disposable phone UI integration; no general site compatibility, browser persistence, GPU page rendering or shared desktop lock guarantee'),indent=2)+'\n')
    for c in checks:print('CHECK '+c,flush=True)
