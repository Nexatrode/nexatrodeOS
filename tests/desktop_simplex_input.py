"""Passphrase and real SimpleX-worker lifecycle checks on a fresh disk only."""
import json
import re
import subprocess
import struct
import time

PASSWORD = 'disposabletest42'
BODY = b'disposable receive after unlock'
CANARY = b'disposable pending plaintext canary'


def exercise(monitor, trace, temporary, evidence):
    evidence=evidence.resolve()
    checks=[]
    def output():
        text=trace.read_text(errors='replace')
        text=re.sub(r'GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n','',text)
        text=re.sub(r'GPU_VIRTIO_DRIVER\|ERROR: Received interrupt but is neither a used buffer notification nor a gpu event, irq status: 0x0\r?\n','',text)
        failures=('LOCK|FAIL|','RUNTIME|FAIL|','MON|ERROR','panicked at','SIMPLEX_SESSION|FAULT|',
                  'ERASURE_FAILED','DYNAMIC|UNEXPECTED_FAULT|','GUEST_AGENT_RESTORE_FAIL','HASKELL_EXIT|')
        if any(word in text for word in failures): raise RuntimeError('SimpleX lock fixture failed: '+text[-5000:])
        return text
    def wait(marker,count=1,timeout=240,keep_awake=True):
        until=time.monotonic()+timeout
        activity=0
        while time.monotonic()<until:
            if output().count(marker)>=count: return
            if keep_awake and time.monotonic()-activity>=1:
                # First-run SQLite/SQLCipher initialization can outlast the
                # fixture's short idle timeout. Real input keeps this phase
                # active; the dedicated auto-lock check disables it below.
                monitor.send('input-send-event',{'events':[{'type':'abs','data':{'axis':'x','value':30000 if int(time.monotonic())%2 else 29900}}]})
                activity=time.monotonic()
            time.sleep(.05)
        raise RuntimeError('missing '+marker+': '+output()[-5000:])
    def keys(events):
        monitor.send('input-send-event',{'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':name}}} for name,down in events]})
    def key(name): keys([(name,True),(name,False)])
    def type_word(word):
        for n in range(0,len(word),8):
            keys([(c,down) for c in word[n:n+8] for down in (True,False)])
            time.sleep(.15)
    def state():
        path=temporary/'session-state.bin'
        monitor.send('pmemsave',{'val':0x5fd90000,'size':8192,'filename':str(path)})
        raw=path.read_bytes();path.unlink()
        assert len(raw)==8192
        epoch,active=struct.unpack_from('<QQ',raw)
        magic,result_epoch,phase,rows=struct.unpack_from('<QQQQ',raw,4096)
        return epoch,active,magic,result_epoch,phase,raw[4096:],rows
    def wait_state(epoch,active,ready=False,timeout=240,keep_awake=True):
        until=time.monotonic()+timeout;activity=0
        while time.monotonic()<until:
            output()
            current=state()
            if current[:2]==(epoch,int(active)):
                if ready and current[2:5]==(0x4e58534d54455354,epoch,5): return
                if not ready and (active or current[5]==bytes(4096)): return
            if keep_awake and time.monotonic()-activity>=1:
                monitor.send('input-send-event',{'events':[{'type':'abs','data':{'axis':'x','value':30000 if int(time.monotonic())%2 else 29900}}]})
                activity=time.monotonic()
            time.sleep(.1)
        raise RuntimeError('session state timeout '+str(current[:5])+': '+output()[-3000:])
    def unlock(epoch,expected_rows):
        key('ret');time.sleep(.2);type_word(PASSWORD);key('ret')
        wait_state(epoch,True,ready=True)
        assert state()[6]==expected_rows
    wait('SIMPLEX_SESSION|BOOT_LOCKED')
    # Startup prints from independent PDs can interleave character by
    # character. Confirm the actual setup screen rather than reconstructing
    # a potentially interleaved auth/probe log marker.
    until=time.monotonic()+25
    while time.monotonic()<until:
        frame=temporary/'setup.ppm'
        monitor.send('screendump',{'filename':str(frame)})
        text=subprocess.run(['tesseract',str(frame),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout.lower()
        if 'set passphrase' in text: break
        output();time.sleep(.3)
    else: raise RuntimeError('setup screen missing: '+text)
    wait_state(1,False)
    assert 'SIMPLEX_GATE|STARTING' not in output()
    assert 'SIMPLEX_TEST|FRESH_QUEUES' not in output()
    type_word(PASSWORD);key('ret');wait('LOCK_AUTH|INPUT_CLEARED')
    type_word(PASSWORD);key('ret');wait('LOCK_AUTH|ENROLLED')
    assert 'SIMPLEX_SESSION|STARTED' not in output()
    checks.append('boot/enrollment keep GHC agent, Messages and every worker stopped')
    unlock(1,1)
    print('CHECK first unlock: real agent, receive, transcript and photo passed',flush=True)
    disk=(temporary/'blank.img').read_bytes()
    assert BODY not in disk and CANARY not in disk
    checks.append('real GHC AgentStore opens; generated receive event commits through receive worker; transcript and encrypted JPEG decode correctly')
    key('f9');wait_state(2,False);wait('LOCK|LOCKED|epoch=2')
    time.sleep(1)
    assert state()[:2]==(2,0)
    key('ret');time.sleep(.2);type_word('wrongpassword');key('ret');wait('LOCK_AUTH|DENIED')
    assert state()[:2]==(2,0)
    checks.append('manual lock stops all producers, evicts key, verifies every writable/private/shared byte erased; wrong password starts nothing')
    time.sleep(2);type_word(PASSWORD);key('ret')
    wait_state(2,True,ready=True)
    assert state()[6]==2
    print('CHECK manual lock, wrong password and second unlock passed',flush=True)
    checks.append('unlock reconstructs GHC/SQLite, Messages, reader/decoder and receive/transcript workers; abandoned queue entries are absent')
    wait_state(3,False,timeout=45,keep_awake=False)
    # The service group is erased before the manager finishes the dynamic GUI
    # teardown and drops pre-lock input. Wait for that completion before typing.
    wait('LOCK|LOCKED|epoch=3',keep_awake=False)
    unlock(3,3)
    print('CHECK auto-lock and third unlock passed',flush=True)
    checks.append('auto-lock applies the same erasure boundary; workers recover again after passphrase verification')
    monitor.send('system_reset');wait_state(1,False);wait('LOCK_AUTH|RESTORED_LOCKED')
    unlock(1,3)
    print('CHECK locked reboot and committed transcript/photo recovery passed',flush=True)
    checks.append('reboot remains locked, reuses wrapped vault key only after unlock, and recovers persisted transcript/AgentStore')
    locked=output().count('LOCK|LOCKED|epoch=2')
    key('f9');wait_state(2,False);wait('LOCK|LOCKED|epoch=2',locked+1)
    frame=temporary/'simplex-locked.ppm'
    monitor.send('screendump',{'filename':str(frame)})
    (evidence/'simplex-locked.ppm').write_bytes(frame.read_bytes())
    assert BODY not in (temporary/'blank.img').read_bytes()
    (evidence/'simplex-result.json').write_text(json.dumps({'checks':checks,'saved_data_used':False,
        'external_relay_tested':False,'incoming_event':'generated decoded-event seam; actual receive/history/photo pipeline',
        'service_starts':4,'manual_lock':True,'auto_lock':True,'locked_reboot':True,
        'scrubbed_bytes':[int(n) for n in re.findall(r'SIMPLEX_SESSION\|SCRUBBED\|bytes=(\d+)',output())]},indent=2)+'\n')
    print('PASS disposable SimpleX lock/recovery: '+str(len(checks))+' lifecycle checks',flush=True)
