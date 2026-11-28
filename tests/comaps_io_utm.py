#!/usr/bin/env python3
"""Focused, disposable-only Maps I/O profile; no password, disk edits or reboot."""
import argparse,hashlib,json,os,plistlib,statistics,struct,subprocess,time
from pathlib import Path
from desktop_gpu_motion import MotionQmp,host_locked
ROOT=Path(__file__).resolve().parents[1]
FIELDS=['requests','wait_us','max_wait_us','copy_us','max_copy_us','issue_us','max_issue_us','dispatch_us','viewer_us','empty_irqs']
class Probe:
    def __init__(self,stage):
        self.stage=stage.resolve();assert self.stage.is_relative_to(ROOT/'build/desktop-apps') and (self.stage/'.disposable-build').is_file()
        self.m=json.loads((stage/'build.json').read_text());s=json.loads((stage/'session.json').read_text());self.data=Path(s['bundle'])/'Data'
        assert self.data.parent.parent==self.stage and self.m['map_storage'] and self.m['saved_data_used'] is s['saved_data_used'] is False
        cfg=plistlib.loads((self.data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==s['uuid'] and [d['ImageName'] for d in cfg['Drive']]==['blank.img','maps.img']
        assert not any((self.data/n).is_symlink() for n in ('maps.img','blank.img','loader.img'))
        assert hashlib.sha256((self.data/'loader.img').read_bytes()).hexdigest()==self.m['loader_sha256']==s['loader_sha256']
        assert all(hashlib.sha256((ROOT/f).read_bytes()).hexdigest()==h for f,h in self.m['sources'].items())
        assert not host_locked();self.drive='drive'+cfg['Drive'][1]['Identifier']
        self.q=MotionQmp(self.data/'monitor');self.q.input=os.open(str(self.q.path)+'.in',os.O_WRONLY|os.O_NONBLOCK)
        try:self.q.send('query-status')
        except RuntimeError as e:
            if 'Expecting capabilities negotiation' not in str(e):raise
            self.q.send('qmp_capabilities')
    def read(self,address,n):
        p=self.data/'map-io-profile.bin'
        try:self.q.send('pmemsave',dict(val=address,size=n,filename=str(p)));return p.read_bytes()
        finally:p.unlink(missing_ok=True)
    def state(self):
        for _ in range(60):
            v=struct.unpack('<1024Q',self.read(0x5fb96000,8192))
            if v[0] and not v[0]&1 and v[0]==struct.unpack('<Q',self.read(0x5fb96000,8))[0]:return v
            time.sleep(.005)
        raise TimeoutError('Maps timing seqlock')
    def until(self,fn):
        end=time.monotonic()+90
        while time.monotonic()<end:
            trace=(self.data/'trace.log').read_text(errors='replace');assert not any(x in trace for x in ('MON|ERROR','VMFault','panicked at','COMAPS|FAIL|','DESKTOP_GPU|FAIL|')),trace[-1500:]
            v=fn()
            if v:return v
            time.sleep(.06)
        raise TimeoutError(str(self.state()[:16]))
    def mode(self):return struct.unpack('<6I',self.read(0x5fb92000,24))
    def pointer(self,x,y,down):
        m=self.mode();events=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in [('x',x,m[2]//2),('y',y,m[3]//2)]]
        events.append({'type':'btn','data':{'button':'left','down':down}});self.q.send('input-send-event',dict(events=events))
    def tap(self,x,y):
        sc=struct.unpack('<20I',self.read(0x5fbb4000,80));x+=sc[16];y+=sc[17];self.pointer(x,y,True);time.sleep(.09);self.pointer(x,y,False)
    def open(self):
        last=None;since=time.monotonic()
        def ready():
            nonlocal last,since
            m=self.mode();g=struct.unpack('<32I',self.read(0x5fba0000,128))
            if not g[0] or g[0]&1 or tuple(g[9:11])!=tuple(m[2:4]) or g[11]!=m[1]:return False
            if m!=last:last=m;since=time.monotonic();return False
            return time.monotonic()-since>=.8
        self.until(ready)
        m=self.mode();self.pointer((m[2]//2-360)//2+306,m[3]//2-65,True);time.sleep(.09);self.pointer((m[2]//2-360)//2+306,m[3]//2-65,False)
        self.until(lambda:self.state()[4] and self.state()[13]==0)
        if self.state()[11]!=3:
            self.tap(85,26);time.sleep(.1);self.tap(85,188);self.until(lambda:self.state()[11]==3 and self.state()[13]==0)
    def blocks(self):return next(x['stats'] for x in self.q.send('query-blockstats')['return'] if x['device']==self.drive)
    def detail(self):
        self.tap(525,35);self.until(lambda:self.state()[8]==0 and self.state()[13]==0 and not self.state()[891])
        baseline=self.blocks();frame=self.state()[3];start=time.monotonic()
        for level in (1,2,3):
            self.tap(471,35);self.until(lambda:self.state()[8]==level)
        st=self.until(lambda:self.state() if self.state()[13]==0 and self.state()[891] else None);end=self.blocks()
        io={name:st[901+i] for i,name in enumerate(FIELDS)} if st[900]==0x4e584d494f303031 else {}
        if io:io.update(ack_us=st[923],max_ack_us=st[924])
        return dict(total_ms=st[889]/1000,read_ms=st[886]/1000,hash_ms=st[887]/1000,decode_ms=st[888]/1000,bytes=st[890],pages=st[883],frames=st[3]-frame,features=st[884],points=st[885],mode=list(self.mode()),io=io,wait_histogram=list(st[911:919]),worst_request=list(st[919:923]),backend_read_ms=(end['rd_total_time_ns']-baseline['rd_total_time_ns'])/1e6,backend_reads=end['rd_operations']-baseline['rd_operations'],host_observed_ms=(time.monotonic()-start)*1000)
    def close(self):self.q.close()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--samples',type=int,default=5);p.add_argument('--load',action='store_true');a=p.parse_args()
    assert 1<=a.samples<=12;out=a.output.resolve();assert out.is_relative_to(ROOT/'build');out.mkdir(parents=True,exist_ok=True)
    source=Path(__file__).read_bytes();(out/'harness-used.py').write_bytes(source);probe=Probe(a.guest);workers=[]
    try:
        probe.open()
        if a.load:workers=[subprocess.Popen(['python3','-c','import time\ne=time.monotonic()+120\nwhile time.monotonic()<e: sum(i*i for i in range(30000))']) for _ in range(4)]
        samples=[probe.detail() for _ in range(a.samples)]
        assert all(s['points']<=524288 and s['features']<=32768 for s in samples)
        summary={key:statistics.median(s[key] for s in samples) for key in ('total_ms','read_ms','hash_ms','decode_ms','backend_read_ms','frames')}
        result=dict(passed=True,loader_sha256=probe.m['loader_sha256'],harness_sha256=hashlib.sha256(source).hexdigest(),saved_data_used=False,pixel_wifi_touched=False,host_cpu_workers=len(workers),samples=samples,medians=summary,scope='Guest issue_us overlaps wait_us when the doorbell MMIO write stalls; do not add those fields. Same Liechtenstein viewport, three zoom steps. Guest elapsed times include scheduling; backend times are QEMU block completion times, not guest IRQ delivery.')
        (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(summary,indent=2))
    finally:
        for w in workers:
            if w.poll() is None:w.terminate()
            w.wait()
        probe.close()
if __name__=='__main__':main()
