"""Real input and presented-pixel checks for the third desktop app."""
import json
import re
import subprocess
import struct
import time
from desktop_gpu_capture import capture


class BrowserCase:
    x,y=340,152
    def __init__(self, monitor, trace, temporary, evidence, click, until, ui,gpu=False):
        self.monitor=monitor;self.trace=trace;self.temporary=temporary;self.evidence=evidence
        self.click=click;self.until=until;self.ui=ui;self.checks=[]
        self.gpu=gpu
    def state(self):
        path=self.temporary/'shell-state.bin'
        self.monitor.send('pmemsave',{'val':0x5fbb4000,'size':64,'filename':str(path)})
        state=struct.unpack('<16I',path.read_bytes());path.unlink()
        return state if state[0] and not state[0]&1 else None
    def open(self):
        self.click(724,728)
        self.until(lambda:(s:=self.state()) and s[2]==3 and s[15]==1,timeout=15)
    def minimize(self):
        self.click(self.x+490,self.y-22)
        self.until(lambda:(s:=self.state()) and s[15]==0,timeout=15)
    def key(self,code,shift=False):
        events=[]
        if shift:events.append({'type':'key','data':{'down':True,'key':{'type':'qcode','data':'shift'}}})
        events.extend({'type':'key','data':{'down':d,'key':{'type':'qcode','data':code}}} for d in (True,False))
        if shift:events.append({'type':'key','data':{'down':False,'key':{'type':'qcode','data':'shift'}}})
        self.monitor.send('input-send-event',{'events':events});time.sleep(.015)
    def url(self,url,already_editing=False):
        if not already_editing:self.click(self.x+210,self.y+30)
        for c in url:
            self.key({'/':'slash','.':'dot','-':'minus',':':'semicolon'}.get(c,c),c==':')
        before=len(self.trace.read_text(errors='replace'));self.key('ret');return before
    def screen(self,name,required):
        end=time.monotonic()+35
        while time.monotonic()<end:
            ppm=self.temporary/'browser-check.ppm';crop=self.temporary/'browser-crop.png'
            capture(self.monitor,ppm,self.gpu)
            subprocess.run(['magick',str(ppm),'-crop',f'1120x1040+{self.x*2}+{self.y*2}','+repage',str(crop)],check=True,timeout=20)
            text=subprocess.run(['tesseract',str(crop),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout
            compact=lambda s:re.sub('[^a-z0-9]','',s.lower())
            if all(compact(s) in compact(text) for s in required):
                subprocess.run(['magick',str(ppm),str(self.evidence/(name+'.png'))],check=True,timeout=20)
                (self.evidence/(name+'.ocr.txt')).write_text(text)
                return
            time.sleep(.2)
        raise RuntimeError('windowed browser scanout did not contain '+str(required)+': '+text)
    def initial(self):
        self.open();self.screen('three-apps-browser-home',['Explore the web'])
        self.url('resource:script.html');self.screen('three-apps-browser-javascript',['JavaScript'])
        self.click(self.x+30,self.y+30);self.screen('three-apps-browser-home-again',['Explore the web'])
        self.minimize()
        self.checks.append('third dock item opens a native 2x browser window; packaged HTML/JavaScript renders and Home works')
    def concurrent(self):
        before=self.ui()[1][1][3]
        assert 'Scanning block' in before,'browser network test must overlap active wallet scanning'
        self.open();self.click(self.x+210,self.y+30);self.minimize()
        self.click(1050,90)
        for c in 'notbrowser':self.key(c)
        self.open()
        start=self.url('https://www.netsurf-browser.org/about/',already_editing=True)
        self.until(lambda:re.search(r'HTTPS\|DONE\|[^\n]*host=www\.netsurf-browser\.org\|status=200',self.trace.read_text(errors='replace')[start:]),timeout=120)
        assert re.search(r'slot=4\b',self.trace.read_text(errors='replace')[start:]), 'browser did not use its dedicated TCP session'
        self.screen('three-apps-https-during-sync',['About NetSurf'])
        self.minimize()
        self.until(lambda:self.ui()[1][1][3]!=before,timeout=60)
        self.checks.append('browser HTTPS renders while the wallet scans; minimized browser receives no wallet-owned typing')
    def finish(self):
        self.open();self.screen('three-apps-final',['About NetSurf'])
        self.click(self.x+530,self.y-22) # Close retains service; this is not a lock.
        self.open();self.screen('three-apps-restored',['About NetSurf'])
        self.checks.append('browser closes and restores from its dock with the loaded page retained')
        report={'checks':self.checks,'saved_data_used':False,'browser_storage':False,
                'private_tcp_slot':4,'logical_window':[560,520],'scale':2,
                'lock_integrated':False,'static_services':True}
        (self.evidence/'browser-validation.json').write_text(json.dumps(report,indent=2)+'\n')
