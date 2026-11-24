"""End-to-end disposable lock/lifecycle checks; no real user's credentials/data."""
import json
import re
import secrets
import subprocess
import time

PASSWORD = 'disposabletest42'


def exercise(monitor, trace, temporary, evidence, control=False, wrapped=False, eviction_control=False):
    checks = []
    def output():
        text = trace.read_text(errors='replace')
        text = re.sub(r'GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n', '', text)
        text = re.sub(r'GPU_VIRTIO_DRIVER\|ERROR: Received interrupt but is neither a used buffer notification nor a gpu event, irq status: 0x0\r?\n', '', text)
        if any(x in text for x in ('LOCK|FAIL|','RUNTIME|FAIL|','MON|ERROR','panicked at','DYNAMIC|UNEXPECTED_FAULT|')):
            raise RuntimeError('lock fixture failed: ' + text[-3000:])
        return text
    def wait(marker, count=1, timeout=90):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            if output().count(marker) >= count: return
            time.sleep(.05)
        raise RuntimeError('missing ' + marker + ': ' + output()[-2500:])
    def keys(events):
        monitor.send('input-send-event', {'events': [
            {'type':'key','data':{'down':down,'key':{'type':'qcode','data':key}}} for key,down in events]})
    def key(name): keys([(name,True),(name,False)])
    def type_word(word):
        for n in range(0,len(word),12):
            keys([(c,down) for c in word[n:n+12] for down in (True,False)])
            time.sleep(.15)
    def click(x,y):
        monitor.send('input-send-event', {'events':[
            {'type':'abs','data':{'axis':'x','value':x*32767//1079}},
            {'type':'abs','data':{'axis':'y','value':y*32767//2339}},
            {'type':'btn','data':{'button':'left','down':True}},
            {'type':'btn','data':{'button':'left','down':False}}]})
    def swipe_up():
        monitor.send('input-send-event', {'events':[
            {'type':'abs','data':{'axis':'x','value':540*32767//1079}},
            {'type':'abs','data':{'axis':'y','value':1900*32767//2339}},
            {'type':'btn','data':{'button':'left','down':True}},
            {'type':'abs','data':{'axis':'y','value':1500*32767//2339}},
            {'type':'btn','data':{'button':'left','down':False}}]})
    def capture(name, words=(), absent=()):
        until=time.monotonic()+25
        while time.monotonic()<until:
            time.sleep(.4)
            path=temporary/'lock-screen.ppm'
            monitor.send('screendump',{'filename':str(path)})
            contents=path.read_bytes()
            signature,dimensions,maximum,pixels=contents.split(b'\n',3)
            assert (signature,dimensions,maximum)==(b'P6',b'1080 2340',b'255') and len(pixels)==1080*2340*3
            (evidence/(name+'.ppm')).write_bytes(contents)
            if not words and not absent: return pixels
            text=subprocess.run(['tesseract',str(path),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout.lower()
            if all(w in text for w in words) and not any(w in text for w in absent): return pixels
            output()
        raise RuntimeError('screen mismatch '+name+': '+text)
    def submit(password, outcome, count=1):
        type_word(password); key('ret'); wait(outcome,count)
    def heap_dump():
        bases=re.findall(r'LOCK\|TEST_HEAP_PHYSICAL\|base=(\d+)',output())
        assert bases
        path=temporary/'private-heap.bin'
        monitor.send('pmemsave',{'val':int(bases[-1]),'size':0x800000,'filename':str(path)})
        contents=path.read_bytes(); path.unlink()
        assert len(contents)==0x800000
        return contents
    wait('LOCK_AUTH|SETUP'); wait('LOCK_PROBE|UNLOCK_DENIED')
    if wrapped:
        wait('LOCK|VAULT_EVICTED|epoch=1')
        wait('LOCK_VAULT_PROBE|DENIED_AND_CLEAR')
    assert 'DYNAMIC|LAUNCHED' not in output()
    key('f6'); time.sleep(.2)
    assert 'DYNAMIC|LAUNCHED' not in output()
    capture('enroll',('set passphrase',))
    submit(PASSWORD,'LOCK_AUTH|INPUT_CLEARED')
    capture('confirm',('confirm passphrase',))
    submit(PASSWORD,'LOCK_AUTH|ENROLLED')
    capture('enrolled-locked',('device locked',),absent=('clear',))
    assert 'DYNAMIC|LAUNCHED' not in output()
    checks.append('boot and enrollment keep dynamic app absent; launch hotkey cannot bypass lock')
    swipe_up() # Reveal only, no password entered.
    capture('swipe-revealed',('unlock','clear'))
    submit(PASSWORD,'DYNAMIC|READY|generation=1')
    if wrapped:
        wait('LOCK_VAULT_PROBE|RECOVERED')
        assert b'disposable wrapped lifecycle note' not in (temporary/'blank.img').read_bytes()
    wait('LOCK_APP|UNLOCK_DENIED')
    click(150,700)
    marker='lockcanary'+''.join(secrets.choice('abcdefghijklmnopqrstuvwxyz') for _ in range(10))
    type_word(marker); time.sleep(.6)
    assert marker.encode() in heap_dump(), 'memory detector did not find live canary'
    checks.append('disposable app marker present in physical heap before lock')
    keys([('z',True)]); key('f9')
    wait('DYNAMIC|CLOSED|generation=1')
    if eviction_control:
        wait('LOCK|VAULT_EVICTION_FAILED')
        assert 'DYNAMIC_VAULT|EVICT|epoch=2|cleared=false' in output()
        assert 'LOCK|LOCKED|epoch=2' not in output(), 'failed eviction was acknowledged as complete'
        key('f6'); key('ret'); type_word(PASSWORD); key('ret'); time.sleep(1)
        assert 'DYNAMIC|LAUNCHED|generation=2' not in output()
        capture('eviction-failed-closed',('restart required',))
        (evidence/'lock-result.json').write_text(json.dumps({'negative_control':'skip-key-eviction','provider_key_retention_detected':True,'app_destroyed':True,'further_launch_blocked':True,'saved_data_used':False},indent=2)+'\n')
        print('PASS negative control: retained vault key detected; lock remains fail closed')
        return
    if wrapped:
        wait('LOCK|VAULT_EVICTED|epoch=2')
        wait('LOCK_VAULT_PROBE|DENIED_AND_CLEAR',2)
    wait('LOCK_PROBE|UNLOCK_DENIED',2)
    retained=marker.encode() in heap_dump()
    if control:
        assert retained, 'skip-scrub control did not retain marker'
        (evidence/'lock-result.json').write_text(json.dumps({'negative_control':'skip-scrub','detected_old_heap_bytes':True,'saved_data_used':False},indent=2)+'\n')
        print('PASS negative control: host RAM detector caught omitted erasure')
        return
    assert not retained, 'app text survived lock in physical RAM'
    checks.append('manual lock suspends/destroys process, revokes capabilities and eagerly erases private user frames')
    capture('locked-after-app',('device locked',),absent=(marker,))
    key('f6'); time.sleep(.2)
    assert 'DYNAMIC|LAUNCHED|generation=2' not in output()
    key('ret'); time.sleep(.3)
    empty=capture('unlock-empty')
    type_word('wrong'); short=capture('unlock-five')
    assert empty==short, 'locked entry leaked typed length through pixels'
    type_word('x'*95); long=capture('unlock-hundred')
    assert empty==long, 'locked entry leaked long input through pixels'
    key('ret'); wait('LOCK_AUTH|DENIED')
    # A request during the software cooldown must be rejected without unlocking.
    key('ret'); wait('LOCK_AUTH|WAIT')
    assert 'DYNAMIC|LAUNCHED|generation=2' not in output()
    time.sleep(1.1)
    submit(PASSWORD,'DYNAMIC|READY|generation=2')
    if wrapped: wait('LOCK_VAULT_PROBE|RECOVERED',2)
    keys([('z',False)]) # Old owner's release must not reach a fresh instance.
    click(150,700); type_word('fresh')
    capture('fresh-after-unlock',('fresh',),absent=(marker,))
    assert marker.encode() not in heap_dump()
    checks.append('wrong passphrase and cooldown deny unlock; 0/5/100-character locked entry has identical pixels')
    checks.append('correct passphrase creates new generation, fresh text/input and zeroed reused memory')
    wait('DYNAMIC|CLOSED|generation=2',timeout=25)
    capture('auto-locked',('device locked',),absent=('fresh',))
    if wrapped: wait('LOCK|VAULT_EVICTED|epoch=3')
    checks.append('root-authoritative idle timeout locks and destroys the app')
    # Cancel a correct verifier while it runs. Its old epoch must not unlock.
    key('ret'); type_word(PASSWORD)
    verifying = output().count('LOCK_AUTH|VERIFY_BEGIN') + 1
    key('ret'); wait('LOCK_AUTH|VERIFY_BEGIN', verifying)
    key('f9'); wait('LOCK_AUTH|STALE_UNLOCK_DENIED')
    assert 'DYNAMIC|LAUNCHED|generation=3' not in output()
    capture('cancelled-unlock',('device locked',))
    checks.append('a re-lock during correct-password verification invalidates its pending unlock')
    if wrapped:
        wait('LOCK|VAULT_EVICTED|epoch=4')
        key('ret'); key('f8')
        denied = output().count('LOCK_VAULT_PROBE|DENIED_AND_CLEAR')
        type_word(PASSWORD); key('ret')
        wait('LOCK_AUTH|PREPARE_ABANDONED')
        wait('LOCK_VAULT_PROBE|DENIED_AND_CLEAR',denied+1)
        wait('LOCK|UNLOCK_LEASE_EXPIRED')
        wait('LOCK|VAULT_EVICTED|epoch=5')
        assert 'DYNAMIC|LAUNCHED|generation=3' not in output()
        capture('abandoned-key-evicted',('device locked',))
        checks.append('abandoned prepared key denies app access and is erased by the manager lease timeout')
    # Restart only the disposable guest; retain only this freshly enrolled test disk.
    monitor.send('system_reset')
    wait('LOCK_AUTH|RESTORED_LOCKED')
    if wrapped: wait('LOCK|VAULT_EVICTED|epoch=1',count=2)
    assert output().count('DYNAMIC|LAUNCHED|generation=1')==1
    capture('restored-locked',('device locked',))
    key('f6'); time.sleep(.2)
    assert output().count('DYNAMIC|LAUNCHED|generation=1')==1
    key('ret'); submit(PASSWORD,'DYNAMIC|READY|generation=1',count=2)
    if wrapped: wait('LOCK_VAULT_PROBE|RECOVERED',count=3)
    capture('restored-unlocked',('trusted desktop',),absent=(marker,'fresh'))
    checks.append('credential survives guest restart; boot stays locked until correct passphrase')
    click(800,1130)
    wait('DYNAMIC|CLOSED|generation=1',count=2)
    if wrapped:
        wait('LOCK|VAULT_EVICTED|epoch=2',count=2)
        checks += ['random vault key unwrapped and prepared only for an armed current epoch; manager commits before app launch',
                   'manual and idle locks synchronously erase provider key and transfer buffers before lock acknowledgement',
                   'vault app reads/writes are denied while locked; encrypted test note survives lock, unlock and reboot',
                   'test note absent as plaintext from disposable disk; no installation key or saved user disk used']
    capture('trusted-button-locked',('device locked',),absent=('clear',))
    checks.append('trusted on-screen lock button destroys app; keyboard appears only after deliberate reveal')
    text=output()
    assert PASSWORD not in text and marker not in text and 'INPUT|KEY|' not in text and 'INPUT|TEXT|' not in text
    checks.append('serial diagnostics contain no passphrase, text canary or key-coordinate trace')
    (evidence/'lock-result.json').write_text(json.dumps({'checks':checks,'wrapped_random_vault_key':wrapped,'saved_data_used':False,'pixel_wifi_touched':False,'test_idle_ms':10000,'security_scope':'debug-kernel lifecycle prototype; no hardware, rollback resistance or whole-RAM erasure claim'},indent=2)+'\n')
    print('PASS lock, wrong/correct unlock, physical heap erasure, idle lock and persistent restart')
