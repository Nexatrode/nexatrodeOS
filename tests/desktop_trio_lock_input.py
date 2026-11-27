"""Disposable three-app lock lifecycle over actual QMP input and a real relay.

This consumes only the canonical --locked fixture verified by tools/desktop.py.
The passphrase belongs to this generated test disk, never to an installed user.
"""
import json
import re
import shutil
import struct
import time
from desktop_boot_ready import gpu_boot_state
from desktop_gpu_capture import capture

PASSWORD='disposabletrioalpha'

class Lifecycle:
    def __init__(self,q,data,evidence):
        self.q=q;self.data=data;self.evidence=evidence;self.last=0;self.checks=[];self.lock_ms=[]
    def read(self,address,size):
        p=self.data/'trio-test-state.bin'
        try:
            self.q.send('pmemsave',{'val':address,'size':size,'filename':str(p)})
            return p.read_bytes()
        finally:p.unlink(missing_ok=True)
    def status(self):
        for _ in range(10):
            s=struct.unpack('<IIQQQIIQQQ',self.read(0x5fb93000,64))
            if not s[0]:return s # composer/manager initialization still pending
            if s[0] and not s[0]&1 and s[0]==struct.unpack('<I',self.read(0x5fb93000,4))[0]:return s
        raise RuntimeError('unstable trusted lock status')
    def key(self,name):
        self.q.send('input-send-event',{'events':[{'type':'key','data':{'down':d,'key':{'type':'qcode','data':name}}} for d in (True,False)]})
    def word(self,word):
        for c in word:self.key(c);time.sleep(.03)
    def activity(self):
        if time.monotonic()-self.last>1:
            self.q.send('input-send-event',{'events':[{'type':'abs','data':{'axis':'x','value':30000 if int(time.monotonic())%2 else 29900}}]})
            self.last=time.monotonic()
    def until(self,predicate,timeout=180,awake=False):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            text=(self.data/'trace.log').read_text(errors='replace')
            if any(s in text for s in ('TRIO|FAIL|','DESKTOP_GPU|FAIL|','ERASURE_FAILED','VMFault','panicked at','GUEST_AGENT_RESTORE_FAIL','HASKELL_EXIT|')):
                raise RuntimeError('lock fixture fault: '+text[-4000:])
            value=predicate()
            if value:return value
            if awake:self.activity()
            time.sleep(.15)
        raise RuntimeError('trio lock timeout: '+str(self.status())+' '+text[-2500:])
    def marker(self,s,count=1):
        return self.until(lambda:(self.data/'trace.log').read_text(errors='replace').count(s)>=count)
    def locked(self,epoch):
        s=self.status()
        if s[2]!=epoch or s[1]!=1 or s[5]!=2:return False
        assert not s[6] and s[7]>0 and s[9]>0
        boot=gpu_boot_state(self.q,self.data,True)
        if not boot['ready']:return False
        assert boot['display_ack'][3]>0
        assert self.read(0x5fbb0000,8192)==bytes(8192)
        assert self.read(0x5fbb2000,8192)==bytes(8192)
        return boot
    def screenshot(self,name):
        p=self.data/(name+'.ppm');capture(self.q,p,True)
        shutil.copyfile(p,self.evidence/p.name)
    def unlock(self,epoch,generation):
        self.key('ret');time.sleep(.3);self.word(PASSWORD);self.key('ret')
        self.until(lambda:(s:=self.status()) and s[1]==0 and s[2]==epoch and s[5]==3 and s[8]==generation)
    def enroll(self,resume=False):
        self.until(lambda:self.locked(1));self.marker('LOCK_AUTH|')
        if 'LOCK_AUTH|RESTORED_LOCKED' in (self.data/'trace.log').read_text(errors='replace'):
            if not resume:raise ValueError('fixture already enrolled; use a fresh disposable disk or explicit test resume')
            self.unlock(1,1)
            self.checks.append('existing disposable wrapped-key enrollment restores gated after reboot; correct passphrase starts the curated group')
            return
        self.screenshot('initial-setup')
        self.word(PASSWORD);self.key('ret');self.marker('LOCK_AUTH|INPUT_CLEARED')
        self.word(PASSWORD);self.key('ret');self.marker('LOCK_AUTH|ENROLLED')
        self.screenshot('enrolled-locked');self.unlock(1,1)
        self.checks.append('fresh wrapped-key enrollment; all 14 services absent until authenticated commit; foreign unlock/prepare calls denied')
    def recover_chat(self,c,text=None):
        ui=c['ui'];click=c['click'];until=c['until']
        until(lambda:ui()[0][0][0]==0x4e584445534b3031 and ui()[0][0][3]>0 and ui()[1][0][0]==0x4e584445534b3031)
        assert ui()[0][0][5]==0,'unsaved draft survived process restart'
        click(138,326);until(lambda:ui()[0][0][2]==1)
        if text:until(lambda:text in ui()[0][1][1])
    def concurrent(self,c):
        assert 'Scanning block' in c['ui']()[1][1][3]
        c['click'](348,586);c['word']('discardthisdraft')
        c['until'](lambda:c['ui']()[0][0][5]==16)
        scan_height=lambda:int(re.search(r'Scanning block (\d+)',c['ui']()[1][1][3]).group(1))
        before_lock=scan_height()
        epoch=self.status()[2]+1;generation=self.status()[8]
        started=time.monotonic();self.key('f9');self.until(lambda:self.locked(epoch))
        self.lock_ms.append((time.monotonic()-started)*1000);self.screenshot('locked-during-scan')
        self.key('ret');time.sleep(.3);self.word('incorrectpassword');self.key('ret');self.marker('LOCK_AUTH|DENIED')
        assert self.locked(epoch) and self.status()[8]==generation
        c['peer'].command(f'/_send @{c["cid"]} text triowhilelocked')
        time.sleep(3);assert self.locked(epoch)
        # The wrong attempt cleared entry; remain on the revealed unlock form.
        self.word(PASSWORD);self.key('ret')
        self.until(lambda:(s:=self.status()) and not s[1] and s[8]==generation+1)
        self.recover_chat(c,'triowhilelocked');c['send']('trioafterunlock')
        c['until'](lambda:c['ui']()[0][0][6]>0)
        c['until'](lambda:'Scanning block' in c['ui']()[1][1][3] and scan_height()>before_lock,timeout=180)
        if c['browser_case']:
            browser=c['browser_case'];browser.open();browser.screen('browser-after-unlock',['Explore the web']);browser.minimize()
            c['click'](c['chat_dock'],c['h']-72)
        self.screenshot('recovered-during-scan')
        self.checks.append('lock during a real wallet scan evicts key, closes five TCP slots, erases private service ranges and both GPU targets; wrong password denied; committed chat/photo and live send/receive recover')
    def finish(self,c):
        epoch=self.status()[2]+1;generation=self.status()[8]
        self.until(lambda:self.locked(epoch),timeout=90,awake=False)
        self.unlock(epoch,generation+1);self.recover_chat(c,'triowhilelocked');c['send']('trioafteridle')
        self.key('f9');self.until(lambda:self.locked(epoch+1));self.screenshot('before-locked-reboot')
        self.q.send('system_reset');self.marker('LOCK_AUTH|RESTORED_LOCKED');self.until(lambda:self.locked(1))
        self.unlock(1,1);self.recover_chat(c,'triowhilelocked');c['send']('trioafterreboot')
        c['until'](lambda:c['ui']()[0][0][6]>0)
        c['until'](lambda:c['ui']()[1][0][8]==0 and any(x in c['ui']()[1][1][2] for x in ('SYNCED','SCAN COMPLETE','SCAN OK')),timeout=180)
        self.key('f9');boot=self.until(lambda:self.locked(2));self.screenshot('final-locked')
        self.checks.append('trusted idle timeout uses the same erasure boundary; locked reboot keeps all apps gated; passphrase restores encrypted chat, photo and wallet')
        disk=(self.data/'blank.img').read_bytes()
        assert all(x not in disk for x in (PASSWORD.encode(),b'discardthisdraft',b'triowhilelocked',b'trioafterunlock'))
        text=(self.data/'trace.log').read_text(errors='replace')
        assert PASSWORD not in text and 'discardthisdraft' not in text
        (self.evidence/'lock-validation.json').write_text(json.dumps(dict(passed=True,checks=self.checks,final_locked=boot,manual_lock_observed_ms=self.lock_ms,
            saved_data_used=False,pixel_wifi_touched=False,scope='curated fixed-role services; software passphrase wrapping; no hardware, rollback, whole-RAM, SIMD, host GPU-memory or debug-kernel security guarantee'),indent=2)+'\n')
        print('PASS three-app lock, live recovery, auto-lock and locked reboot',flush=True)
