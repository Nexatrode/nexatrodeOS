"""Actual dynamic Notes storage/lifecycle checks on a freshly enrolled UTM disk."""
import json
import re
import secrets
import struct
import subprocess
import time

PASSWORD = 'disposabletest42'


def exercise(monitor, trace, temporary, evidence):
    checks, interruptions = [], []
    def output():
        text = trace.read_text(errors='replace')
        text = re.sub(r'GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n', '', text)
        text = re.sub(r'GPU_VIRTIO_DRIVER\|ERROR: Received interrupt but is neither a used buffer notification nor a gpu event, irq status: 0x0\r?\n', '', text)
        if any(s in text for s in ('LOCK|FAIL|','RUNTIME|FAIL|','MON|ERROR','panicked at','PRIVATE_NOTES_APP|LOAD_FAILED','PRIVATE_NOTES_APP|SAVE_FAILED','DYNAMIC|UNEXPECTED_FAULT|')):
            raise RuntimeError('private Notes fixture failed: '+text[-3500:])
        return text
    def count(marker): return output().count(marker)
    def wait(marker, n=1, timeout=90):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            if count(marker)>=n: return
            time.sleep(.05)
        raise RuntimeError('missing '+marker+': '+output()[-2200:])
    def keys(events):
        monitor.send('input-send-event', {'events':[
            {'type':'key','data':{'down':down,'key':{'type':'qcode','data':key}}} for key,down in events]})
    def key(k): keys([(k,True),(k,False)])
    def type_text(text):
        for start in range(0,len(text),12):
            keys([('ret' if c=='\n' else c, down) for c in text[start:start+12] for down in (True,False)])
            time.sleep(.18)
    def click(x,y):
        monitor.send('input-send-event', {'events':[
            {'type':'abs','data':{'axis':'x','value':x*32767//1079}},
            {'type':'abs','data':{'axis':'y','value':y*32767//2339}},
            {'type':'btn','data':{'button':'left','down':True}},
            {'type':'btn','data':{'button':'left','down':False}}]})
    def unique(prefix): return prefix+''.join(secrets.choice('abcdefghijklmnopqrstuvwxyz') for _ in range(14))
    def page_address():
        found=re.findall(r'PRIVATE_NOTES\|TEST_PAGE_PHYSICAL\|base=(\d+)',output())
        assert found, 'missing private exchange frame diagnostic'
        return int(found[-1])
    def page_bytes(address):
        path=temporary/'note-page.bin'
        monitor.send('pmemsave', {'val':address,'size':4096,'filename':str(path)})
        data=path.read_bytes();path.unlink();assert len(data)==4096
        return data
    def snapshot():
        n=count('PRIVATE_NOTES_APP|SNAPSHOT|')+1
        click(150,700) # Unlock deliberately leaves keyboard focus outside the app.
        key('f3');wait('PRIVATE_NOTES_APP|SNAPSHOT|',n)
        raw=page_bytes(page_address());length,reserved=struct.unpack_from('<II',raw)
        assert length<=2048 and reserved==0
        return raw[8:8+length].decode('utf-8')
    def replace(text):
        click(150,700)
        keys([('ctrl',True),('a',True),('a',False),('ctrl',False)])
        type_text(text)
        assert snapshot()==text, 'typed note differs from actual app contents'
    def save(text, button=False):
        replace(text)
        n=count('PRIVATE_NOTES_APP|SAVED|')+1
        if button: click(435,1212)
        else: key('f2')
        wait('PRIVATE_NOTES_APP|SAVED|',n)
        assert snapshot()==text
        assert text.encode() not in (temporary/'blank.img').read_bytes(), 'note appeared as plaintext on test disk'
    def unlock():
        loaded=count('PRIVATE_NOTES_APP|LOADED|')+1
        ready=count('DYNAMIC|READY|')+1
        key('ret');type_text(PASSWORD);key('ret')
        wait('PRIVATE_NOTES_APP|LOADED|',loaded)
        wait('DYNAMIC|READY|',ready)
        wait('PRIVATE_NOTES_APP|ISOLATION_CHECKED|',ready)
    def reset():
        restored=count('LOCK_AUTH|RESTORED_LOCKED')+1
        before=count('DYNAMIC|LAUNCHED|')
        monitor.send('system_reset');wait('LOCK_AUTH|RESTORED_LOCKED',restored)
        assert count('DYNAMIC|LAUNCHED|')==before, 'restart launched Notes before unlock'
        unlock()
    def screen(name):
        time.sleep(.5)
        path=temporary/'notes-screen.ppm'
        monitor.send('screendump',{'filename':str(path)})
        (evidence/(name+'.ppm')).write_bytes(path.read_bytes())

    wait('LOCK_AUTH|SETUP');wait('PRIVATE_NOTES_PROBE|DENIED')
    type_text(PASSWORD);key('ret');wait('LOCK_AUTH|INPUT_CLEARED')
    type_text(PASSWORD);key('ret');wait('LOCK_AUTH|ENROLLED')
    assert not count('DYNAMIC|LAUNCHED|')
    unlock();assert snapshot()==''
    wait('LOCK_VAULT_PROBE|RECOVERED') # Other app saves/reads only its Contacts namespace.
    checks.append('fresh enrolled disk stays locked; dynamic Notes starts with an empty private document')

    original=unique('privatenote')+'\n'+unique('secondline')
    save(original,button=True);screen('saved-private-note')
    checks.append('actual Notes Save button durably stores multiline UTF-8 text; plaintext absent from disk')
    address=page_address();closed=count('DYNAMIC|CLOSED|')+1
    evicted=count('LOCK|VAULT_EVICTED|')+1
    denied=count('PRIVATE_NOTES_PROBE|DENIED')+1
    key('f9');wait('DYNAMIC|CLOSED|',closed);wait('LOCK|VAULT_EVICTED|',evicted)
    wait('PRIVATE_NOTES_PROBE|DENIED',denied)
    assert page_bytes(address)==bytes(4096), 'old Notes exchange page retained plaintext after lock'
    assert not count('PRIVATE_NOTES_APP|LOADED|')>1
    unlock();assert snapshot()==original;screen('recovered-after-lock')
    checks.append('manual lock destroys app, erases exchange frame and revokes handles; fresh instance recovers its own note')
    checks.append('other-app namespace endpoint cannot read/write Notes; stale instance tags, namespace arguments, malformed pages and stale revisions rejected')

    # Unsaved state must not silently become durable when the instance is killed.
    unsaved=unique('unsaved')
    replace(unsaved);address=page_address();closed=count('DYNAMIC|CLOSED|')+1
    evicted=count('LOCK|VAULT_EVICTED|')+1
    wait('DYNAMIC|CLOSED|',closed,timeout=50);wait('LOCK|VAULT_EVICTED|',evicted)
    assert page_bytes(address)==bytes(4096)
    unlock();assert snapshot()==original
    checks.append('idle lock erases unsaved instance data; unlock restores the last acknowledged save')
    reset();assert snapshot()==original
    checks.append('ordinary guest restart starts locked and recovers the same encrypted document after passphrase unlock')

    for point, hotkey in [(1,'f10'),(2,'f11'),(3,'f12'),(4,'f4')]:
        baseline=unique('baseline');candidate=unique('candidate')
        save(baseline)
        replace(candidate)
        key(hotkey);wait('PRIVATE_NOTES|INTERRUPTION_ARMED|point='+str(point))
        saved_before=count('PRIVATE_NOTES_APP|SAVED|')
        key('f2');wait('PRIVATE_NOTES_VAULT|PAUSED|point='+str(point))
        assert count('PRIVATE_NOTES_APP|SAVED|')==saved_before, 'paused save falsely reported durable success'
        reset()
        recovered=snapshot()
        allowed=[baseline] if point in (1,2) else [candidate] if point==4 else [baseline,candidate]
        assert recovered in allowed, f'invalid recovery at flush point {point}'
        assert baseline.encode() not in (temporary/'blank.img').read_bytes()
        assert candidate.encode() not in (temporary/'blank.img').read_bytes()
        interruptions.append({'point':point,'recovered':'old' if recovered==baseline else 'new','complete_record':True,'false_success':False})
        # Verify recovery did not leave the new instance stuck or with stale authority.
        continued=unique('continued');save(continued)
        reset();assert snapshot()==continued
    checks.append('all four actual save flush boundaries recover a complete committed old/new record after interruption')
    checks.append('each recovered instance can save again and reopen that new save after another locked restart')
    screen('final-recovered-note')
    assert PASSWORD not in output() and original not in output() and unsaved not in output()
    checks.append('no passphrase or note text logged; only disposable disk and public test credentials used')
    (evidence/'notes-result.json').write_text(json.dumps({'checks':checks,'interrupted_saves':interruptions,'saved_data_used':False,'pixel_wifi_touched':False,'test_idle_ms':30000,'observation':'actual dynamic-app private frame snapshots, Save acknowledgements, vault flush barriers and guest framebuffer','scope':'debug-kernel disposable prototype; guest resets are not hardware power-loss or whole-RAM erasure proof'},indent=2)+'\n')
    print('PASS dynamic private encrypted Notes, lock/unlock, app isolation and four interrupted-save recovery points')
