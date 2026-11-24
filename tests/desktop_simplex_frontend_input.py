"""Drive the real SimpleX UI with pointer/keyboard input on a fresh UTM disk."""
import base64
import json
import re
import struct
import subprocess
import time
from pathlib import Path
from desktop_simplex_input import PASSWORD
from simplex_live_peer import Peer

MAGIC=0x4e5846524f4e5431
SCALE=2.6086957

def exercise(monitor,trace,temporary,evidence):
    peer=None;contact=None;checks=[];last_activity=0
    def output():
        text=trace.read_text(errors='replace')
        for marker in ('LOCK|FAIL|','RUNTIME|FAIL|','MON|ERROR','panicked at','SIMPLEX_SESSION|FAULT|','ERASURE_FAILED','DYNAMIC|UNEXPECTED_FAULT|','GUEST_AGENT_RESTORE_FAIL','HASKELL_EXIT|','SIMPLEX_FRONTEND|INPUT_OVERFLOW'):
            if marker in text:raise RuntimeError('frontend fixture failure '+text[-4500:])
        return text
    def activity():
        nonlocal last_activity
        if time.monotonic()-last_activity>1:
            monitor.send('input-send-event',{'events':[{'type':'abs','data':{'axis':'x','value':30000 if int(time.monotonic())%2 else 29900}}]})
            last_activity=time.monotonic()
    def until(predicate,timeout=240,awake=True):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            output();v=predicate()
            if v:return v
            if awake:activity()
            time.sleep(.1)
        raise RuntimeError('frontend timeout '+str(state()[0:2])+': '+output()[-2500:])
    def wait(marker,count=1):return until(lambda:output().count(marker)>=count)
    def key(name):monitor.send('input-send-event',{'events':[{'type':'key','data':{'down':b,'key':{'type':'qcode','data':name}}} for b in (True,False)]})
    def word(text):
        for c in text:key({' ':'spc','-':'minus'}.get(c,c));time.sleep(.05)
    def tap(x,y):
        monitor.send('input-send-event',{'events':[{'type':'abs','data':{'axis':a,'value':int(v)}} for a,v in (('x',x*SCALE/1079*32767),('y',y*SCALE/2339*32767))]})
        time.sleep(.1)
        for down in (True,False):
            monitor.send('input-send-event',{'events':[{'type':'btn','data':{'down':down,'button':'left'}}]});time.sleep(.1)
    def state():
        path=temporary/'frontend-state.bin'
        monitor.send('pmemsave',{'val':0x5fbb0000,'size':8192,'filename':str(path)})
        raw=path.read_bytes();path.unlink();assert len(raw)==8192
        return struct.unpack_from('<QQ',raw),struct.unpack_from('<11Q',raw,4096),raw[4096:]
    def ui(epoch=1):
        st,values,raw=state()
        if st!=(epoch,1) or values[:2]!=(MAGIC,epoch):return None
        return values,raw[128:128+values[6]].decode(errors='replace'),raw[2176:2176+values[7]].decode(errors='replace')
    def locked(epoch):
        st,_,raw=state();return st==(epoch,0) and raw==bytes(4096)
    def capture(name):
        frame=temporary/(name+'.ppm');monitor.send('screendump',{'filename':str(frame)})
        (evidence/frame.name).write_bytes(frame.read_bytes())
        return subprocess.run(['tesseract',str(frame),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout
    def visible_photo(name):
        frame=temporary/(name+'.ppm');monitor.send('screendump',{'filename':str(frame)})
        data=frame.read_bytes();header=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',data)
        assert header and tuple(map(int,header.groups()))==(1080,2340)
        pixels=data[header.end():];assert len(pixels)==1080*2340*3
        # The generated JPEG is solid RGB approximately (18,91,134). Count
        # actual presented pixels, not only the UI's decoded/ready flag.
        count=sum(1 for i in range(0,len(pixels),3) if abs(pixels[i]-18)<=5 and abs(pixels[i+1]-91)<=5 and abs(pixels[i+2]-134)<=5)
        if count<10000:return False
        (evidence/frame.name).write_bytes(data)
        return count
    def received(text):
        def check():
            try:return peer.received(text,timeout=.15)
            except TimeoutError:return None
        return until(check)
    def open_chat(epoch):
        until(lambda:(v:=ui(epoch)) and v[0][8]>0)
        tap(150,292)
        until(lambda:(v:=ui(epoch)) and v[0][3]==1)
    def send(text,epoch):
        tap(180,828);until(lambda:(v:=ui(epoch)) and v[0][4]==6)
        word(text);until(lambda:(v:=ui(epoch)) and v[0][5]==len(text))
        tap(100,195);until(lambda:(v:=ui(epoch)) and v[0][4]==0)
        tap(370,828);received(text)
    try:
        wait('SIMPLEX_SESSION|BOOT_LOCKED')
        until(lambda:'set passphrase' in capture('setup').lower(),timeout=30)
        assert locked(1) and 'SIMPLEX_FRONTEND|STARTED' not in output()
        word(PASSWORD);key('ret');wait('LOCK_AUTH|INPUT_CLEARED');word(PASSWORD);key('ret');wait('LOCK_AUTH|ENROLLED')
        peer=Peer(temporary/'peer')
        key('ret');time.sleep(.25);word(PASSWORD);key('ret')
        until(lambda:(v:=ui()) and v[0][2]==6 and v[0][9]==1)
        capture('simplex-list')
        tap(247,135)
        def invitation():
            v=ui()
            if v and v[1]:return v[1]
            tap(247,135);return None
        uri=until(invitation)
        peer.command('/connect '+uri)
        def connected():
            contacts=peer.command('/contacts')['contacts'];return contacts[0] if contacts else None
        contact=until(connected);cid=contact['contactId']
        tap(350,135);open_chat(1)
        peer.command(f'/_send @{cid} text hostbefore')
        until(lambda:(v:=ui()) and 'hostbefore' in v[2])
        send('frontbefore',1)
        capture('before-lock')
        print('CHECK full UI sends and receives through the live relay',flush=True)
        checks.append('real full-resolution SimpleX UI, chat selection, physical typing, send button and live receive work before lock')
        # A tiny inline image uses the real chat protocol and decoder without
        # depending on an unrelated remote XFTP upload service.
        data=base64.b64encode((Path(__file__).resolve().parents[1]/'experiments/desktop-simplex/fixture.jpg').read_bytes()).decode()
        command=[{'msgContent':{'type':'image','text':'fixturephoto','image':'data:image/jpg;base64,'+data}}]
        peer.command(f'/_send @{cid} json '+json.dumps(command))
        until(lambda:(v:=ui()) and v[0][10]>0)
        until(lambda:visible_photo('photo-before-lock'))
        print('CHECK inline photo displayed',flush=True)
        checks.append('a live inline photo reaches the real receive/photo/transcript workers and appears in the frontend')
        tap(180,828);until(lambda:(v:=ui()) and v[0][4]==6);word('unsaveddraft')
        until(lambda:(v:=ui()) and v[0][5]==12)
        key('f9');until(lambda:locked(2));wait('LOCK|LOCKED|epoch=2')
        assert 'simplex' not in capture('locked').lower()
        key('ret');time.sleep(.2);word('wrongpassword');key('ret');wait('LOCK_AUTH|DENIED');assert locked(2)
        peer.command(f'/_send @{cid} text hostwhilelocked')
        time.sleep(3);assert locked(2)
        word(PASSWORD);key('ret')
        until(lambda:(v:=ui(2)) and v[0][2]==6 and v[0][5]==0 and v[0][4]==0)
        open_chat(2)
        until(lambda:(v:=ui(2)) and 'hostwhilelocked' in v[2] and 'hostbefore' in v[2])
        until(lambda:(v:=ui(2)) and v[0][10]>0)
        send('frontafter',2)
        until(lambda:visible_photo('restored-photo'))
        text=capture('after-unlock')
        assert 'hostwhilelocked' in text.lower().replace(' ','') or 'frontafter' in text.lower().replace(' ','')
        print('CHECK manual lock/unlock restored chat and photo, discarded draft, and sent/received live messages',flush=True)
        checks.append('same UTM unlock restarts the real frontend, discards unsaved draft/keyboard state, restores committed chat/photo, receives queued message and sends successfully')
        # Let the trusted auto-lock deadline expire without synthetic activity.
        until(lambda:locked(3),timeout=50,awake=False);wait('LOCK|LOCKED|epoch=3')
        key('ret');time.sleep(.2);word(PASSWORD);key('ret')
        until(lambda:(v:=ui(3)) and v[0][5]==0 and v[0][4]==0)
        open_chat(3);until(lambda:(v:=ui(3)) and 'hostwhilelocked' in v[2])
        send('frontthird',3)
        checks.append('auto-lock applies the same frontend/worker erasure and live recovery boundary')
        key('f9');until(lambda:locked(4));wait('LOCK|LOCKED|epoch=4');capture('before-reboot-locked')
        monitor.send('system_reset');until(lambda:locked(1));wait('LOCK_AUTH|RESTORED_LOCKED')
        key('ret');time.sleep(.25);word(PASSWORD);key('ret')
        until(lambda:(v:=ui()) and v[0][5]==0 and v[0][4]==0)
        open_chat(1);until(lambda:(v:=ui()) and 'hostwhilelocked' in v[2])
        until(lambda:(v:=ui()) and v[0][10]>0)
        until(lambda:visible_photo('reboot-photo'))
        send('frontreboot',1)
        checks.append('locked reboot stays gated; same encrypted chat/photo and relay connection recover only after the passphrase')
        print('CHECK locked reboot recovered full UI, photo and live sending',flush=True)
        finished=output().count('LOCK|LOCKED|epoch=2')
        key('f9');until(lambda:locked(2));wait('LOCK|LOCKED|epoch=2',finished+1);capture('final-locked')
        disk=(temporary/'blank.img').read_bytes()
        assert all(x not in disk for x in (b'hostbefore',b'frontbefore',b'unsaveddraft',b'hostwhilelocked'))
        (evidence/'frontend-result.json').write_text(json.dumps({'passed':True,'checks':checks,'saved_data_used':False,'real_full_frontend':True,'synthetic_receive_injection':False,'same_vm_across_unlock':True,'scrubbed_bytes':[int(n) for n in re.findall(r'SIMPLEX_SESSION\|SCRUBBED\|bytes=(\d+)',output())]},indent=2)+'\n')
        print('PASS full SimpleX frontend: live messages, photo, typing, manual/auto lock and recovery',flush=True)
    finally:
        if peer:
            try:
                peer.save_events(evidence/'frontend-peer-events.json')
                if contact:peer.command(f'/_delete @{contact["contactId"]} full notify=on',timeout=15)
            finally:peer.close()
