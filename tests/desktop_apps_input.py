"""Actual input, independent fresh relay peer and presented-pixel desktop checks."""
import base64, hashlib, json, math, re, shutil, statistics, struct, subprocess, time, urllib.request
from pathlib import Path
from simplex_live_peer import Peer
from desktop_gpu_capture import capture

def prepare_windows(monitor,temporary,viewport):
    """Place test windows through real titlebar input after native resizing."""
    width,height=viewport;w,h=width//2,height//2
    def state():
        p=temporary/'prepare-scene.bin'
        try:
            monitor.send('pmemsave',{'val':0x5fbb4000,'size':64,'filename':str(p)})
            s=struct.unpack('<16I',p.read_bytes())
            return s if s[0] and not s[0]&1 else None
        finally:p.unlink(missing_ok=True)
    def until(fn):
        end=time.monotonic()+15
        while time.monotonic()<end:
            value=fn()
            if value:return value
            time.sleep(.03)
        raise RuntimeError('window placement failed: '+str(state()))
    def pointer(x,y,down=None):
        events=[{'type':'abs','data':{'axis':a,'value':(v*32767+m-1)//m}} for a,v,m in [('x',x,w-1),('y',y,h-1)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        monitor.send('input-send-event',{'events':events})
    def click(x,y):pointer(x,y,True);time.sleep(.05);pointer(x,y,False);time.sleep(.1)
    try:
        pointer(0,0,False)
        for owner,(tx,ty) in enumerate(((68,126),(650,110),(340,min(152,h-520-128))),1):
            click((w-276)//2+54+84*(owner-1),h-72)
            s=until(lambda:(s:=state()) and s[2]==owner and s)
            x,y=s[4+4*(owner-1):6+4*(owner-1)]
            pointer(x+100,y-22,True);time.sleep(.03);pointer(tx+100,ty-22)
            # No synthetic keep-awake motion while the drag owns the pointer.
            until(lambda:(s:=state()) and tuple(s[4+4*(owner-1):6+4*(owner-1)])==(tx,ty))
            pointer(tx+100,ty-22,False)
            if owner==3:click(tx+490,ty-22)
        click((w-276)//2+54,h-72)
        return until(state)
    finally:
        monitor.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})

def exercise(monitor,trace,temporary,evidence,browser=False,gpu=False,viewport=(2560,1600),lifecycle=None):
    checks=[];peer=None;samples=[];browser_case=None
    width,height=viewport;w,h=width//2,height//2
    assert width>=2444 and height>=1560,'live app check needs the full 520px app viewport'
    if browser:prepare_windows(monitor,temporary,viewport)
    chat_dock=(w-276)//2+54 if browser else (w-192)//2+52
    def output():
        text=trace.read_text(errors='replace')
        for bad in ('MON|ERROR','panicked at','VMFault','Data Abort','Prefetch Abort','GUEST_AGENT_RESTORE_FAIL','HASKELL_EXIT|','DESKTOP_GPU|FAIL|'):
            if bad in text:raise RuntimeError('desktop app guest failed: '+text[-4000:])
        return text
    def until(predicate,timeout=120):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            if lifecycle:lifecycle.activity()
            output();v=predicate()
            if v:return v
            time.sleep(.05)
        raise RuntimeError('desktop app timeout: '+str(ui())+' '+output()[-2500:])
    def ui():
        path=temporary/'app-state.bin';monitor.send('pmemsave',{'val':0x5fbb0000,'size':16384,'filename':str(path)})
        raw=path.read_bytes();path.unlink();result=[]
        for off in (0,8192):
            nums=struct.unpack_from('<13Q',raw,off)
            texts=[raw[off+offset:off+offset+min(nums[9+i],limit)].decode(errors='replace') for i,(offset,limit) in enumerate(zip((128,3200,6400,6680),(3000,3000,256,256)))]
            result.append((nums,texts))
        return result
    def pointer(x,y,down=None):
        events=[{'type':'abs','data':{'axis':a,'value':round(v*32767/maximum)}} for a,v,maximum in [('x',x,w-1),('y',y,h-1)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        monitor.send('input-send-event',{'events':events})
    def click(x,y):pointer(x,y,True);time.sleep(.05);pointer(x,y,False);time.sleep(.1)
    def key(name):monitor.send('input-send-event',{'events':[{'type':'key','data':{'down':d,'key':{'type':'qcode','data':name}}} for d in (True,False)]})
    def word(text):
        for c in text:key({' ':'spc','-':'minus',':':'semicolon','.':'dot'}.get(c,c));time.sleep(.012)
    def frame(name):
        path=temporary/(name+'.ppm');capture(monitor,path,gpu);data=path.read_bytes();shutil.copyfile(path,evidence/path.name);h=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',data)
        assert h and tuple(map(int,h.groups()))==viewport,'desktop viewport changed during the live check'
        subprocess.run(['magick',str(path),str(evidence/(name+'.png'))],check=True)
        return data[h.end():]
    def received(text):
        def check():
            try:return peer.received(text,timeout=.1)
            except TimeoutError:return None
        return until(check)
    def send(text):
        click(68+280,126+460);word(text)
        until(lambda:ui()[0][0][5]==len(text))
        click(68+518,126+460);received(text)
    try:
        until(lambda:(v:=ui()) and v[0][0][0]==0x4e584445534b3031 and v[1][0][0]==0x4e584445534b3031)
        until(lambda:ui()[0][0][4]==1)
        if browser:
            from desktop_browser_input import BrowserCase
            browser_case=BrowserCase(monitor,trace,temporary,evidence,click,until,ui,gpu=gpu,viewport=viewport)
            browser_case.initial()
        frame('desktop-ready');checks.append('both real 2x frontends publish independent surfaces in the landscape shell')
        peer=Peer(temporary/'peer')
        click(68+50,126+142);uri=until(lambda:ui()[0][1][0]);assert uri.startswith(('simplex:','https://simplex.chat/'))
        peer.command('/connect '+uri)
        def connected():
            c=peer.command('/contacts')['contacts'];return c[0] if c else None
        contact=until(connected);cid=contact['contactId']
        until(lambda:ui()[0][0][3]>0)
        click(68+280,126+438);click(68+70,126+200)
        until(lambda:ui()[0][0][2]==1)
        peer.command(f'/_send @{cid} text desktopincoming');until(lambda:'desktopincoming' in ui()[0][1][1])
        send('desktopoutgoing');frame('desktop-chat')
        checks.append('actual desktop chat selection, native keyboard typing and bidirectional real SMP relay messages pass with a fresh upstream peer')
        data=base64.b64encode((Path(__file__).resolve().parents[1]/'experiments/desktop-simplex/fixture.jpg').read_bytes()).decode()
        command=[{'msgContent':{'type':'image','text':'desktopphoto','image':'data:image/jpg;base64,'+data}}]
        peer.command(f'/_send @{cid} json '+json.dumps(command));until(lambda:ui()[0][0][6]>0)
        blue=0
        def photo_visible():
            nonlocal blue
            pixels=frame('desktop-photo')
            blue=sum(1 for i in range(0,len(pixels),3) if abs(pixels[i]-18)<=5 and abs(pixels[i+1]-91)<=5 and abs(pixels[i+2]-134)<=5)
            return blue>10000
        # Decode-ready can precede frontend publication and GPU upload. Require
        # the actual image pixels, with a bounded wait for that asynchronous path.
        until(photo_visible,timeout=60)
        checks.append('real incoming photo runs through receive, transcript, thumbnail reader/decoder workers and appears in presented pixels')
        # Enroll only a fresh disposable wallet; no saved seed is imported.
        tip=json.load(urllib.request.urlopen('https://rpc.nexawal.com/get_height',timeout=20))['height']
        restore_height=min(3_809_410, max(1,tip-4440))
        assert restore_height<3_812_148<tip, 'test must cross the previously failing legacy-output range'
        until(lambda:ui()[1][0][8]==1)
        click(650+150,110+105)
        for _ in range(12):key('backspace')
        word(str(restore_height));click(650+200,110+166)
        until(lambda:ui()[1][0][8]==2,timeout=180)
        # New wallets deliberately begin at today's tip. Exercise creation,
        # then cancel before persistence and restore a fresh host-generated
        # unfunded test identity at an earlier birthday for the scan workload.
        click(650+200,110+337);until(lambda:ui()[1][0][8]==1)
        fixture=temporary/'disposable-wallet.words'
        generator=Path(__file__).resolve().parents[1]/'build/desktop-apps-host/debug/examples/disposable_wallet'
        subprocess.run([str(generator),str(fixture)],check=True)
        words=fixture.read_text();assert len(words.split())==25
        click(650+200,110+215);until(lambda:ui()[1][0][8]==4)
        click(650+150,110+105)
        for _ in range(12):key('backspace')
        word(str(restore_height))
        click(650+150,110+170);word(words)
        fixture.unlink();del words
        click(650+200,110+323)
        until(lambda:ui()[1][0][8]==0,timeout=180)
        until(lambda:'Scanning block' in ui()[1][1][3] or 'SCAN' in ui()[1][1][2],timeout=180)
        before=ui()[1][1][3]
        if lifecycle:
            lifecycle.concurrent(locals())
            before=ui()[1][1][3]
        peer.command(f'/_send @{cid} text walletbackground')
        concurrent=[{'msgContent':{'type':'image','text':'concurrentphoto','image':'data:image/jpg;base64,'+data}}]
        peer.command(f'/_send @{cid} json '+json.dumps(concurrent))
        if browser_case:browser_case.concurrent()
        # Minimize and restore SimpleX through trusted chrome while the wallet advances.
        click(558,104);click(850,90);word('unowned')
        assert ui()[0][0][5]==0,'hidden chat received wallet-owned keyboard input'
        time.sleep(.2);click(chat_dock,height//2-72)
        until(lambda:'walletbackground' in ui()[0][1][1]);send('backgroundreply')
        until(lambda:ui()[1][1][3]!=before,timeout=60)
        after=ui()[1][1][3]
        frame('desktop-concurrent')
        checks.append('wallet scans against rpc.nexawal.com while minimized SimpleX receives and restores; sending still works without restarting either service')
        def wallet_picture():
            path=temporary/'timed.ppm';capture(monitor,path,gpu)
            data=path.read_bytes();h=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',data);assert h
            rgb=data[h.end():]
            # The pointer and dock are outside this wallet-content region.
            return hashlib.sha256(b''.join(rgb[(y*width+1340)*3:(y*width+2360)*3] for y in range(250,650))).digest()
        for _ in range(6):
            for tab,x in ((3,650+490),(0,650+70)):
                pointer(x,110+486);before_pixels=wallet_picture()
                begin=time.monotonic();pointer(x,110+486,True);time.sleep(.008);pointer(x,110+486,False)
                until(lambda t=tab:ui()[1][0][7]==t and wallet_picture()!=before_pixels,timeout=10)
                samples.append((time.monotonic()-begin)*1000)
        frame('desktop-final')
        # Exit and restore retain the live service; close is not a lock/scrub claim.
        click(600,104);click(chat_dock,height//2-72);send('restoredsend')
        checks.append('repeated wallet tabs, window focus/minimize/restore and retained SimpleX connection work together')
        until(lambda:'concurrentphoto' in ui()[0][1][1] and ui()[0][0][6]>=2,timeout=120)
        frame('desktop-photos-complete')
        def terminal_wallet():
            status=ui()[1][1][2]
            return status if any(marker in status for marker in ('SYNCED','SCAN COMPLETE','SCAN OK','SCAN FAIL','SYNC FAIL')) else None
        terminal=until(terminal_wallet,timeout=600)
        fully_synced=any(marker in terminal for marker in ('SYNCED','SCAN COMPLETE','SCAN OK')) and 'FAIL' not in terminal
        checks.append('both distinct incoming photos finish decoding and displaying, including the photo received during scanning')
        frame('desktop-terminal')
        if browser_case:
            browser_case.finish()
            checks.extend(browser_case.checks)
        report={'checks':checks,'saved_data_used':False,'fresh_peer':True,'photo_presented_pixels':blue,'wallet_node':'rpc.nexawal.com:443','wallet_tip_at_start':tip,'wallet_restore_height':restore_height,'legacy_regression_range':3812148,'wallet_detail_before':before,'wallet_detail_after':after,'tab_observed_ms':samples,'tab_median_ms':statistics.median(samples),'tab_p95_ms':sorted(samples)[math.ceil(len(samples)*.95)-1],'timing_scope':'host QMP mouse press to changed presented wallet-content pixels plus matching tab state; sampled polling, excludes physical host display latency','static_service_lifecycle':True,'lock_integrated':lifecycle is not None,'wallet_full_sync_completed':fully_synced,'wallet_terminal_status':terminal,'photo_ready_after_run':ui()[0][0][6]}
        report['gpu_readback_verification']=gpu
        if gpu:
            report['timing_scope']='host QMP mouse press to changed diagnostic GPU-readback pixels plus matching tab state; includes readback/reference verification/polling and excludes physical display latency'
            report['checks']=[c.replace('presented pixels','GPU-readback pixels') for c in checks]
        (evidence/'validation.json').write_text(json.dumps(report,indent=2)+'\n')
        for c in checks:print('CHECK '+c,flush=True)
        assert fully_synced, 'full wallet scan failed: '+terminal
        if lifecycle:lifecycle.finish(locals())
    finally:
        if peer:
            if 'contact' in locals():
                try:peer.command(f'/_delete @{contact["contactId"]}')
                except Exception:pass
            peer.close()
