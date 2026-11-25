"""Check actual guest pixels and real browser form/navigation/scroll behaviour."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def exercise(monitor, trace, temporary, evidence, scale):
    checks=[]
    def output():
        text=trace.read_text(errors='replace')
        text=re.sub(r'GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n','',text)
        if any(s in text for s in ('NETSURF|FAIL|','MON|ERROR','Data Abort','Prefetch Abort')):
            raise RuntimeError('browser guest fault: '+text[-3000:])
        return text
    def wait(marker):
        deadline=time.monotonic()+30
        while time.monotonic()<deadline:
            if marker in output():return
            time.sleep(.05)
        raise RuntimeError('missing '+marker+': '+output()[-1500:])
    def pointer(x,y,button=None):
        events=[{'type':'abs','data':{'axis':'x','value':round(x*32767/1279)}},
                {'type':'abs','data':{'axis':'y','value':round(y*32767/799)}}]
        if button is not None:events.append({'type':'btn','data':{'button':'left','down':button}})
        monitor.send('input-send-event',{'events':events})
    def click(x,y):
        pointer(x,y,True);time.sleep(.08);pointer(x,y,False);time.sleep(.15)
    def key(code):
        monitor.send('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':code}}} for down in (True,False)]})
        time.sleep(.06)
    def screen(name,required=(),absent=()):
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            time.sleep(.2)
            capture=temporary/'browser-check.ppm'
            monitor.send('screendump',{'filename':str(capture)})
            data=capture.read_bytes()
            fields=data.split(b'\n',3)
            if fields[:3]!=[b'P6',f'{1280*scale} {800*scale}'.encode(),b'255']:continue
            target=evidence/(name+'.ppm');target.write_bytes(data)
            ocr=subprocess.run(['tesseract',str(target),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout.lower()
            output()
            compact=lambda s:''.join(s.lower().split())
            if all(compact(s) in compact(ocr) for s in required) and all(compact(s) not in compact(ocr) for s in absent):
                subprocess.run(['magick',str(target),str(evidence/(name+'.png'))],check=True,timeout=30)
                (evidence/(name+'.ocr.txt')).write_text(ocr)
                return fields[3]
        raise RuntimeError('browser screen mismatch '+name+': '+ocr)
    click(48,31)
    initial=screen('browser-home',('a real web page','try the form','follow a link'))
    # Match the packaged decoded BMP at an interior landmark, away from cursor/chrome.
    x,y=1040*scale,330*scale
    pixel=initial[(y*1280*scale+x)*3:(y*1280*scale+x)*3+3]
    if pixel!=bytes((116,226,220)):raise RuntimeError('packaged BMP landmark is absent: '+repr(pixel))
    checks.append('HTML and external CSS laid out; packaged BMP decoded and visible at native display scale')
    click(225,445)
    for c in 'nettesx':key(c)
    key('backspace');key('t')
    screen('browser-typed',('nettest','try the form'))
    click(137,492)
    click(201,545)
    wait('NETSURF|FORM|submitted|name=1|choice=1')
    screen('browser-form-result',('form submitted locally','browser handled the form'))
    checks.append('real HTML text field edits and backspace work; checked input and typed value included in GET form navigation')
    click(48,31)
    screen('browser-returned-home',('a real web page','follow a link'),('nettest',))
    key('end')
    screen('browser-scrolled-bottom',('end of local document','a small beginning'),('try the form',))
    key('home')
    screen('browser-scrolled-home',('a real web page','try the form'))
    before=len(output())
    monitor.send('input-send-event',{'events':[{'type':'btn','data':{'button':'wheel-down','down':down}} for down in (True,False)]})
    deadline=time.monotonic()+5
    while time.monotonic()<deadline and not re.search(r'NETSURF\|SCROLL\|[1-9]',output()[before:]):time.sleep(.05)
    if not re.search(r'NETSURF\|SCROLL\|[1-9]',output()[before:]):raise RuntimeError('wheel did not advance scrolling')
    key('home')
    screen('browser-wheel-return',('a real web page','follow a link'))
    checks.append('document scrolling reaches the footer and returns to the top without page corruption')
    click(230,704)
    screen('browser-second-page',('the link worked','second html document'))
    click(130,31)
    screen('browser-history-back',('a real web page','try the form'))
    checks.append('HTML link loads another packaged document; native Back restores the previous page')
    # A fresh navigation leaves the disposable trial at an empty form.
    click(48,31)
    screen('browser-tested',('a real web page','try the form'),('nettest',))
    result={'checks':checks,'network':False,'storage':False,'javascript':False,'saved_data_used':False,
            'scale':scale,'resolution':[1280*scale,800*scale],
            'observed':'Actual guest input, engine URL callback assertions, scanout OCR and image pixels',
            'test_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    (evidence/'interaction-result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS NetSurf local HTML/CSS/BMP, form typing/submission, links, Back and scrolling',flush=True)
