#!/usr/bin/env python3
"""Measure completed GPU frames during held dragging in a disposable desktop.

Guest timestamps exclude QMP polling from frame work, but include guest
scheduling and host GPU delays. They do not measure physical display latency.
"""
import argparse,hashlib,json,math,os,plistlib,statistics,struct,subprocess,time
from pathlib import Path
from utm_vault_key_storage import QmpPipe

ROOT=Path(__file__).resolve().parents[1]

class MotionQmp(QmpPipe):
    # The generic fixture client polls every 20 ms. Motion needs finer polling
    # to avoid imposing that interval on the input stream being measured.
    def send(self,command,arguments=None):
        self.next_id+=1;ident=self.next_id
        payload={'execute':command,'id':ident}
        if arguments is not None:payload['arguments']=arguments
        os.write(self.input,(json.dumps(payload)+'\n').encode());end=time.monotonic()+30
        while time.monotonic()<end:
            try:self.buffer+=os.read(self.output,65536)
            except BlockingIOError:pass
            while b'\n' in self.buffer:
                line,self.buffer=self.buffer.split(b'\n',1)
                try:reply=json.loads(line)
                except (ValueError,UnicodeDecodeError):continue
                if reply.get('id')==ident:
                    if 'error' in reply:raise RuntimeError(reply['error'])
                    return reply
            time.sleep(.001)
        raise TimeoutError(command)

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--guest',type=Path,required=True);p.add_argument('--label',required=True)
    p.add_argument('--seconds',type=float,default=4);p.add_argument('--hz',type=int,default=60)
    p.add_argument('--require-pacing',action='store_true')
    p.add_argument('--idle-seconds',type=float,default=0)
    p.add_argument('--allow-locked-host',action='store_true',help='diagnostic only; a locked Mac is not a visible dragging test')
    a=p.parse_args();assert a.label.replace('-','').isalnum() and 1<=a.seconds<=15 and 10<=a.hz<=120
    assert 0<=a.idle_seconds<=10
    def host_locked():
        rows=plistlib.loads(subprocess.check_output(['ioreg','-n','Root','-d','1','-a'],timeout=10))
        rows=rows if isinstance(rows,list) else [rows]
        users=[user for row in rows for user in row.get('IOConsoleUsers',[])]
        assert users,'host screen-lock state unavailable'
        return any(user.get('CGSSessionScreenIsLocked',False) for user in users)
    locked_before=host_locked()
    assert a.allow_locked_host or not locked_before,'unlock the Mac for a visible dragging measurement'
    guest=a.guest.resolve();assert guest.is_relative_to(ROOT/'build/desktop-apps')
    session=json.loads((guest/'session.json').read_text());assert session['saved_data_used'] is False
    data=Path(session['bundle'])/'Data';assert data.parent.parent==guest
    assert session.get('gpu_verify') is False,'performance runs must omit diagnostic GPU readback'
    assert hashlib.sha256((data/'loader.img').read_bytes()).hexdigest()==session['loader_sha256']
    q=MotionQmp(data/'monitor');q.input=os.open(str(q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
    try:q.send('query-status')
    except RuntimeError as e:
        if 'Expecting capabilities negotiation' not in str(e):raise
        q.send('qmp_capabilities')
    def memory(addr,size):
        path=data/'motion-state.bin';q.send('pmemsave',{'val':addr,'size':size,'filename':str(path)})
        raw=path.read_bytes();path.unlink();return raw
    def stable(addr,words):
        for _ in range(30):
            v=struct.unpack('<'+'I'*words,memory(addr,words*4))
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<I',memory(addr,4))[0]:return v
        raise RuntimeError('unstable observation')
    mode=None;w=h=0
    def pointer(x,y,down=None):
        events=[{'type':'abs','data':{'axis':axis,'value':(round(value)*32767+limit-1)//limit}}
                for axis,value,limit in (('x',x,w-1),('y',y,h-1))]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        q.send('input-send-event',{'events':events})
    def click(x,y):pointer(x,y,True);pointer(x,y,False)
    def guard():
        text=(data/'trace.log').read_text(errors='replace')
        assert not any(x in text for x in ('DESKTOP_GPU|FAIL|','DISPLAY_AGENT|FAIL|','VMFault','MON|ERROR','panicked at')),text[-2500:]
    def wait_scene():
        end=time.monotonic()+15
        while time.monotonic()<end:
            guard();s=stable(0x5fbb4000,16);v=stable(0x5fba0000,16)
            if v[1]>=s[0]:return s
            time.sleep(.02)
        raise TimeoutError('scene completion')
    samples={};last_stats=None
    def history():
        nonlocal last_stats
        v=stable(0x5fba0000,1024);assert v[13]==0x4d4f5431 and v[14]==48,'build lacks motion timing history'
        last_stats=v
        stride=v[26] or 16;assert stride in (16,20)
        for i in range(48):
            r=v[32+stride*i:32+stride*(i+1)]
            if r[0]:samples[r[0]]=r
        return v[2]
    try:
        # UTM may apply a remembered native window size shortly after boot.
        # Do not start a motion trial in that transient default display mode.
        deadline=time.monotonic()+45;candidate=None;since=0
        while time.monotonic()<deadline:
            guard();current=stable(0x5fb92000,6);gpu=stable(0x5fba0000,32)
            if current[1:]!=candidate:
                candidate=current[1:];since=time.monotonic()
            if tuple(gpu[9:11])==tuple(current[2:4]) and gpu[11]==current[1] and time.monotonic()-since>=2:
                mode=current;w,h=mode[2]//2,mode[3]//2;break
            time.sleep(.05)
        assert mode is not None,'native display did not settle'
        pointer(100,100,False)
        # Same overlapping three-app workload for each run, with SimpleX on top.
        for owner in (3,2,1):click((w-276)//2+54+84*(owner-1),h-65);wait_scene()
        time.sleep(.4);s=wait_scene();x,y=s[4:6]
        anchor=min(w-572-65,max(12+65,x));pointer(x+200,y-24,True)
        pointer(anchor+200,y-24);wait_scene();history();first=max(samples);initial_stats=last_stats
        started=time.monotonic();next_history=started+.2;sent=0
        while time.monotonic()-started<a.seconds:
            now=time.monotonic();elapsed=now-started
            pointer(anchor+200+64*math.sin(elapsed*math.tau/.9),y-24);sent+=1
            if now>=next_history:history();guard();next_history=now+.2
            delay=started+sent/a.hz-time.monotonic()
            if delay>0:time.sleep(delay)
        input_elapsed=time.monotonic()-started
        assert stable(0x5fb92000,6)[1:4]==mode[1:4],'display resized during motion measurement'
        # Keep the button held until every submitted position is accounted for.
        wait_scene();last=history();held=sorted((v for k,v in samples.items() if first<k<=last),key=lambda r:r[0])
        assert len(held)>=3,'no continuous held-button frames'
        pointer(anchor+200,y-24);pointer(anchor+200,y-24,False);wait_scene()
        guard()
        def summary(values):
            values=sorted(values);return {'median':statistics.median(values),'p95':values[math.ceil(len(values)*.95)-1],'max':max(values)}
        intervals=[((b[2]+b[3]-a[2]-a[3])&0xffffffff)/1000 for a,b in zip(held,held[1:]) if b[0]==a[0]+1]
        duration=((held[-1][2]+held[-1][3]-held[0][2]-held[0][3])&0xffffffff)/1000000
        fields={'frame_ms':3,'snapshot_ms':4,'compose_ms':5,'present_ms':6,'commands':7,'command_wait_ms':8,'uploads':9}
        paced=last_stats[15]==0x50414331
        pacing=None
        if paced:
            fields.update(handler_elapsed_ms=12,start_interval_ms=13,pending_to_start_ms=14)
            names={'gpu_irqs':16,'timer_wakes':17,'coalesced_requests':18,'paced_deferrals':19,'watchdog_recoveries':20,'irq_batches':24,'timer_batches':25}
            pacing={name:(last_stats[index]-initial_stats[index])&0xffffffff for name,index in names.items()}
            pacing.update(maximum_batch_commands=last_stats[22],completion_sources={str(i):sum(v[15]==i for v in held) for i in (0,1,2)})
            assert min(v[13] for v in held)>=16_665,'catch-up burst exceeded the 60 Hz cadence'
            assert last_stats[22]<=4,'GPU command leases exceeded the bounded queue'
            assert max(v[8] for v in held)==0,'normal motion entered a blocking GPU command wait'
            if len(held[0])==20:fields.update(max_poll_gap_ms=18,poll_calls=19)
        if a.require_pacing:
            assert paced,'build lacks completion scheduling'
            assert pacing['irq_batches']>0,'no GPU completion interrupt resumed the renderer'
        idle=None
        if a.idle_seconds:
            time.sleep(.2);before=stable(0x5fba0000,32);time.sleep(a.idle_seconds);after=stable(0x5fba0000,32)
            idle={'seconds':a.idle_seconds,'new_frames':after[2]-before[2],
                  'gpu_irqs':after[16]-before[16],'timer_wakes':after[17]-before[17],
                  'final_phase':after[21]}
            guard()
        locked_after=host_locked()
        assert a.allow_locked_host or not locked_after,'Mac locked during measurement; reject visible-drag result'
        result={'uuid':session['uuid'],'loader_sha256':session['loader_sha256'],'gpu_verify':False,
                'host_screen_locked_before':locked_before,'host_screen_locked_after':locked_after,
                'locked_host_diagnostic_allowed':a.allow_locked_host,
                'harness_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'mode_settled_seconds':2,
                'saved_data_used':False,'size':list(mode[2:4]),'input_events':sent,'input_target_hz':a.hz,
                'input_elapsed_seconds':input_elapsed,'input_achieved_hz':sent/input_elapsed,
                'held_frames':len(held),'guest_completed_fps':(len(held)-1)/duration,'completed_frame_interval_ms':summary(intervals),
                'pacing':pacing,'idle':idle,
                'metrics':{name:summary([v[index]/(1000 if name.endswith('_ms') else 1) for v in held]) for name,index in fields.items()},
                'samples':[list(v) for v in held],'scope':__doc__}
        (guest/('motion-'+a.label+'.json')).write_text(json.dumps(result,indent=2)+'\n')
        print(json.dumps({k:v for k,v in result.items() if k not in ('samples','scope')},indent=2))
    finally:
        try:q.send('input-send-event',{'events':[{'type':'btn','data':{'button':'left','down':False}}]})
        finally:q.close()

if __name__=='__main__':main()
