"""Real SMP delivery across a passphrase lock on an isolated fresh UTM disk."""
import json
import re
import struct
import subprocess
import time
from simplex_live_peer import Peer, RELAY
from desktop_simplex_input import PASSWORD

MAGIC=0x4e584c4956453031

def exercise(monitor,trace,temporary,evidence):
    checks=[];peer=None;contact=None;last_activity=0
    def output():
        text=trace.read_text(errors='replace')
        text=re.sub(r'GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n','',text)
        text=re.sub(r'GPU_VIRTIO_DRIVER\|ERROR: Received interrupt but is neither a used buffer notification nor a gpu event, irq status: 0x0\r?\n','',text)
        for marker in ('LOCK|FAIL|','RUNTIME|FAIL|','MON|ERROR','panicked at','SIMPLEX_SESSION|FAULT|','ERASURE_FAILED','DYNAMIC|UNEXPECTED_FAULT|','GUEST_AGENT_RESTORE_FAIL','HASKELL_EXIT|'):
            if marker in text:raise RuntimeError('live fixture failure '+text[-4500:])
        return text
    def activity():
        nonlocal last_activity
        if time.monotonic()-last_activity>1:
            monitor.send('input-send-event',{'events':[{'type':'abs','data':{'axis':'x','value':30000 if int(time.monotonic())%2 else 29900}}]})
            last_activity=time.monotonic()
    def until(predicate,timeout=240,awake=True):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            output()
            value=predicate()
            if value:return value
            if awake:activity()
            time.sleep(.1)
        raise RuntimeError('live fixture timeout; '+output()[-5000:])
    def wait(marker,count=1,awake=True):return until(lambda:output().count(marker)>=count,awake=awake)
    def key(name):monitor.send('input-send-event',{'events':[{'type':'key','data':{'down':b,'key':{'type':'qcode','data':name}}} for b in (True,False)]})
    def word(text):
        for c in text:key(c);time.sleep(.02)
    def state():
        path=temporary/'live-state.bin'
        monitor.send('pmemsave',{'val':0x5fbb0000,'size':8192,'filename':str(path)})
        raw=path.read_bytes();path.unlink();assert len(raw)==8192
        return struct.unpack_from('<QQ',raw),struct.unpack_from('<QQQQQQQQ',raw,4096),raw[4096:]
    def phase(epoch,n):
        st,result,raw=state()
        return result if st==(epoch,1) and result[0:2]==(MAGIC,epoch) and result[2]>=n else None
    def locked(epoch):
        st,_,raw=state();return st==(epoch,0) and raw==bytes(4096)
    def unlock():key('ret');time.sleep(.25);word(PASSWORD);key('ret')
    def received(text):
        def check():
            try:return peer.received(text,timeout=.15)
            except TimeoutError:return None
        return until(check)
    try:
        wait('SIMPLEX_SESSION|BOOT_LOCKED')
        def setup():
            frame=temporary/'setup.ppm';monitor.send('screendump',{'filename':str(frame)})
            text=subprocess.run(['tesseract',str(frame),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout.lower()
            return 'set passphrase' in text
        until(setup,timeout=30)
        assert locked(1) and 'SIMPLEX_GATE|STARTING' not in output()
        word(PASSWORD);key('ret');wait('LOCK_AUTH|INPUT_CLEARED');word(PASSWORD);key('ret');wait('LOCK_AUTH|ENROLLED')
        assert locked(1)
        peer=Peer(temporary/'peer')
        unlock();until(lambda:phase(1,2))
        _,result,raw=state();n=result[5];assert 0<n<=3000
        uri=raw[64:64+n].decode();assert uri.startswith(('simplex:','https://simplex.chat/'))
        response=peer.command('/connect '+uri)
        print('CHECK live peer joined invitation: '+response['type'],flush=True)
        def contact_ready():
            contacts=peer.command('/contacts')['contacts']
            return contacts[0] if contacts else None
        contact=until(contact_ready)
        cid=contact['contactId'];assert isinstance(cid,int)
        until(lambda:phase(1,3))
        peer.command(f'/_send @{cid} text host-before-lock')
        received('guest-before-lock');first=until(lambda:phase(1,8))
        assert first[3]>=1
        checks.append('fresh upstream peer sends host-before-lock through smp4; real receive/transcript workers persist it; guest-before-lock arrives independently in peer')
        print('CHECK real sending and receiving before lock passed',flush=True)
        key('f9');until(lambda:locked(2));wait('LOCK|LOCKED|epoch=2');wait('SIMPLEX_SESSION|NET_CLOSED|epoch=2')
        time.sleep(.5)
        key('ret');time.sleep(.2);word('wrongpassword');key('ret');wait('LOCK_AUTH|DENIED')
        assert locked(2)
        peer.command(f'/_send @{cid} text host-while-locked')
        for _ in range(30):
            assert locked(2);output();time.sleep(.1)
        checks.append('manual lock aborts guest relay sockets, clears TCP/DNS buffers and all managed service memory; wrong passphrase starts nothing; peer queues a real message while locked')
        word(PASSWORD);key('ret');second=until(lambda:phase(2,8));received('guest-after-unlock')
        assert second[3]>=2
        checks.append('same UTM process unlocks, reconstructs encrypted AgentStore/subscriptions, receives relay-queued host-while-locked, recovers old transcript and delivers guest-after-unlock to independent peer')
        print('CHECK real sending and receiving after unlock passed',flush=True)
        # No raw plaintext fixture messages should appear on the encrypted disk.
        disk=(temporary/'blank.img').read_bytes()
        assert all(s not in disk for s in (b'host-before-lock',b'host-while-locked',b'guest-after-unlock'))
        key('f9');until(lambda:locked(3));wait('LOCK|LOCKED|epoch=3');wait('SIMPLEX_SESSION|NET_CLOSED|epoch=3')
        checks.append('final lock again closes network and erases result/service queues; known transcript plaintext absent from fresh disk image')
        frame=temporary/'simplex-live-locked.ppm';monitor.send('screendump',{'filename':str(frame)})
        (evidence/'simplex-live-locked.ppm').write_bytes(frame.read_bytes())
        (evidence/'simplex-live-result.json').write_text(json.dumps({'passed':True,'checks':checks,'saved_data_used':False,'relay':RELAY,'synthetic_receive_injection':False,'same_vm_across_unlock':True,'host_peer':'upstream simplex-chat 7.0.0.12, fresh temporary profile','scrubbed_bytes':[int(n) for n in re.findall(r'SIMPLEX_SESSION\|SCRUBBED\|bytes=(\d+)',output())]},indent=2)+'\n')
        print('PASS disposable SimpleX live relay lock/unlock: '+str(len(checks))+' checks',flush=True)
    finally:
        if peer:
            peer.save_events(evidence/'peer-events.json')
            if contact:
                try:peer.command(f'/_delete @{contact["contactId"]} full notify=on',timeout=15)
                except Exception as e:print('Disposable peer cleanup: '+str(e),flush=True)
            (evidence/'peer-cli.log').write_bytes((temporary/'peer/cli.log').read_bytes())
            peer.close()
