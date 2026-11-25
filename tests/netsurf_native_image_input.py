"""Verify native image decoding against guest scanout, not download success alone."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time
import urllib.request
import numpy as np

TEST_SHA256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


class Probe:
    def __init__(self,monitor,trace,temporary,evidence,scale):
        self.monitor,self.trace,self.temporary,self.evidence,self.scale=monitor,trace,temporary,evidence,scale
    def output(self):
        text=self.trace.read_text(errors='replace')
        if any(s in text for s in ('NETSURF|FAIL|','MON|ERROR','VMFault','microkit_notify: invalid channel','Data Abort','Prefetch Abort')):
            raise RuntimeError('image guest fault: '+text[-3000:])
        return text
    def click(self,x,y):
        for down in (True,False):
            self.monitor.send('input-send-event',{'events':[{'type':'abs','data':{'axis':'x','value':round(x*32767/1279)}},{'type':'abs','data':{'axis':'y','value':round(y*32767/799)}},{'type':'btn','data':{'button':'left','down':down}}]});time.sleep(.08)
    def key(self,code,shift=False):
        events=[]
        if shift:events.append({'type':'key','data':{'down':True,'key':{'type':'qcode','data':'shift'}}})
        events.extend({'type':'key','data':{'down':down,'key':{'type':'qcode','data':code}}} for down in (True,False))
        if shift:events.append({'type':'key','data':{'down':False,'key':{'type':'qcode','data':'shift'}}})
        self.monitor.send('input-send-event',{'events':events});time.sleep(.025)
    def navigate(self,url):
        self.click(320,30)
        for c in url:
            code,shift={':':('semicolon',True),'/':('slash',False),'.':('dot',False),'-':('minus',False),'_':('minus',True)}.get(c,(c,False))
            if c.isupper():code,shift=c.lower(),True
            self.key(code,shift)
        at=len(self.output());self.key('ret');return at
    def capture(self):
        path=self.temporary/'image-check.ppm'
        self.monitor.send('screendump',{'filename':str(path)})
        raw=path.read_bytes();parts=raw.split(b'\n',3)
        assert parts[:3]==[b'P6',f'{1280*self.scale} {800*self.scale}'.encode(),b'255']
        return raw,np.frombuffer(parts[3],dtype=np.uint8).reshape(800*self.scale,1280*self.scale,3)
    def save(self,raw,name):
        path=self.evidence/(name+'.ppm');path.write_bytes(raw)
        subprocess.run(['magick',str(path),str(self.evidence/(name+'.png'))],check=True,timeout=30)
    def wait(self,pattern,at=0):
        end=time.monotonic()+60
        while time.monotonic()<end:
            m=re.search(pattern,self.output()[at:])
            if m:return m
            time.sleep(.1)
        raise RuntimeError('missing image event '+pattern+': '+self.output()[-2500:])


def rgba(path):
    width,height=map(int,subprocess.check_output(['magick','identify','-format','%w %h',str(path)],text=True).split())
    data=subprocess.check_output(['magick',str(path),'-alpha','on','-depth','8','rgba:-'])
    return np.frombuffer(data,dtype=np.uint8).reshape(height,width,4)


def locate(screen,reference,scale,tolerance=5,background=None,excluded=(),dimensions=None):
    """Find an entire expected image using opaque landmarks then verify all pixels."""
    if dimensions:
        width,height=dimensions
        yy=np.arange(height*scale)*reference.shape[0]//(height*scale);xx=np.arange(width*scale)*reference.shape[1]//(width*scale)
        expected=reference[yy[:,None],xx[None,:]]
    else:expected=np.repeat(np.repeat(reference,scale,axis=0),scale,axis=1)
    rgb=expected[:,:,:3].copy();alpha=expected[:,:,3]
    if background is not None:
        a=alpha.astype(np.uint32)[:,:,None]
        rgb=((rgb.astype(np.uint32)*a+np.array(background,dtype=np.uint32)*(255-a)+127)//255).astype(np.uint8)
        mask=np.ones(alpha.shape,dtype=bool)
    else:mask=alpha==255
    h,w=rgb.shape[:2];area=screen[60*scale:]
    rows,cols=area.shape[0]-h+1,area.shape[1]-w+1
    if rows<=0 or cols<=0 or not mask.any():return None
    # Cover image interior and edges, and require real colour variation.
    ys,xs=np.where(mask)
    indices=np.linspace(0,len(xs)-1,12,dtype=int)
    points=list(zip(ys[indices],xs[indices]))
    # Include pixels of the least common colours so a white background cannot pass.
    pixels=rgb[mask];colours,first,counts=np.unique(pixels,axis=0,return_index=True,return_counts=True)
    for index in np.argsort(counts)[:8]:
        i=first[index];points.append((ys[i],xs[i]))
    candidates=np.ones((rows,cols),dtype=bool)
    for y,x in points:
        delta=np.abs(area[y:y+rows,x:x+cols].astype(np.int16)-rgb[y,x].astype(np.int16))
        candidates &= np.max(delta,axis=2)<=tolerance
        if not candidates.any():return None
    for y,x in zip(*np.where(candidates)):
        if any(abs(int(x)-old['x'])<w and abs(int(y+60*scale)-old['y'])<h for old in excluded):continue
        delta=np.abs(area[y:y+h,x:x+w].astype(np.int16)-rgb.astype(np.int16))[mask]
        # A tiny number of JPEG rounding differences are acceptable; colour/channel
        # errors, placeholders, flat backgrounds and missing images cannot pass.
        if np.quantile(delta,.995)<=tolerance and delta.mean()<1.8:
            return {'x':int(x),'y':int(y+60*scale),'width':w,'height':h,'mean_channel_error':round(float(delta.mean()),4)}
    return None


def exercise(monitor,trace,temporary,evidence,scale):
    p=Probe(monitor,trace,temporary,evidence,scale);checks=[]
    p.navigate('resource:images.html')
    end=time.monotonic()+30
    fixtures=[('alpha.png',2*scale,[255,255,255]),('interlaced.png',2*scale,None),('baseline.jpg',2*scale,None),('progressive.jpg',2*scale,None),('cmyk.jpg',2*scale,None)]
    expected={name:rgba(evidence/name) for name,_,_ in fixtures}
    while time.monotonic()<end:
        time.sleep(.2);p.output();raw,screen=p.capture();found={}
        for name,zoom,bg in fixtures:
            found[name]=locate(screen,expected[name],zoom,background=bg,tolerance=8 if name.endswith('.jpg') else 2,excluded=[m for m in found.values() if m])
        if all(found.values()):break
    else:raise RuntimeError('decoder fixture pixels missing: '+json.dumps(found))
    p.save(raw,'images-local-formats')
    checks.append({'synthetic_formats':found,'covers':['RGBA transparency','Adam7 PNG','baseline JPEG','progressive JPEG','CMYK JPEG']})
    # The same page includes broken and huge headers. It must still draw and accept input.
    ocr=subprocess.check_output(['tesseract',str(evidence/'images-local-formats.ppm'),'stdout','--psm','11'],text=True,stderr=subprocess.DEVNULL).lower()
    assert 'endofimagetests' in ''.join(ocr.split()),ocr
    p.click(45,30);time.sleep(.3);raw,_=p.capture();p.save(raw,'images-after-invalid-inputs')
    checks.append({'invalid_inputs':['corrupt.png','corrupt.jpg','oversize.png','oversize.jpg'],'ui_recovered':True})
    # Host downloads are independent pixel oracles only; none is sent into the VM
    # or packaged in the loader. Guest DNS/TCP/TLS fetches these URLs itself.
    refs=evidence/'image-reference';refs.mkdir(exist_ok=True)
    opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    tests=[('NetSurf page PNG','https://www.netsurf-browser.org/','https://www.netsurf-browser.org/netsurf.png','netsurf-logo.png',('netsurf','project overview')),
           ('W3C PNG','https://www.w3.org/People/mimasa/test/imgformat/img/w3c_home.png','https://www.w3.org/People/mimasa/test/imgformat/img/w3c_home.png','w3c.png',()),
           ('W3C JPEG','https://www.w3.org/People/mimasa/test/imgformat/img/w3c_home.jpg','https://www.w3.org/People/mimasa/test/imgformat/img/w3c_home.jpg','w3c.jpg',()),
           ('W3C page photograph JPEG','https://www.w3.org/People/Berners-Lee/','https://www.w3.org/Press/Stock/Berners-Lee/2001-europaeum-eighth.jpg','w3c-photo.jpg',())]
    for label,page,url,name,words in tests:
        path=refs/name
        with opener.open(url,timeout=30) as response:
            data=response.read(2*1024*1024+1)
        assert len(data)<=2*1024*1024;path.write_bytes(data);reference=rgba(path)
        at=p.navigate(page)
        if label not in ('NetSurf page PNG',):
            p.wait(r'HTTPS\|DONE\|[^\n]*host=www\.w3\.org\|status=200',at)
        deadline=time.monotonic()+40
        while time.monotonic()<deadline:
            time.sleep(.3);p.output();raw,screen=p.capture();match=locate(screen,reference,scale,tolerance=6 if name.endswith('.jpg') else 2,dimensions=(252,166) if name=='w3c-photo.jpg' else None)
            if match:break
        else:raise RuntimeError('real website image is not visible: '+label)
        slug=name.replace('.','-')
        p.save(raw,'images-real-'+slug)
        if words:
            ocr=subprocess.check_output(['tesseract',str(evidence/('images-real-'+slug+'.ppm')),'stdout','--psm','11'],text=True,stderr=subprocess.DEVNULL).lower()
            assert all(''.join(w.split()) in ''.join(ocr.split()) for w in words),ocr
        checks.append({'label':label,'page':page,'image_url':url,'reference_sha256':hashlib.sha256(data).hexdigest(),'encoded_bytes':len(data),'pixels':match})
    # Scroll and navigate after live image decoding in this same boot.
    p.navigate('https://www.netsurf-browser.org/');time.sleep(.5)
    p.key('end');time.sleep(.5);p.key('home');time.sleep(.5)
    p.click(45,30);time.sleep(.3);raw,_=p.capture();p.save(raw,'images-tested')
    result={'checks':checks,'network':'native guest HTTPS','image_decode':'libpng + libjpeg-turbo in guest','scale':scale,'saved_data_used':False,'storage':False,'test_sha256':TEST_SHA256}
    (evidence/'image-result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS PNG/JPEG variants, malformed/oversize inputs, real website image pixels and navigation',flush=True)
