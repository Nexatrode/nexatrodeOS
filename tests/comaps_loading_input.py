#!/usr/bin/env python3
"""Observe Maps drawing while its dedicated disposable disk is throttled."""
import argparse,hashlib,json,statistics,struct,subprocess,time
from pathlib import Path
from comaps_io_utm import Probe,ROOT
from desktop_gpu_capture import capture

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--load',action='store_true');a=p.parse_args()
    out=a.output.resolve();assert out.is_relative_to(ROOT/'build');out.mkdir(parents=True,exist_ok=True);source=Path(__file__).read_bytes();(out/'harness-used.py').write_bytes(source)
    probe=Probe(a.guest);workers=[];held=False
    def throttle(bps):probe.q.send('block_set_io_throttle',dict(device=probe.drive,bps=bps,bps_rd=0,bps_wr=0,iops=0,iops_rd=0,iops_wr=0))
    def point(x,y,down):
        sc=struct.unpack('<20I',probe.read(0x5fbb4000,80));m=probe.mode()
        events=[{'type':'abs','data':{'axis':axis,'value':round(v*32767/(limit-1))}} for axis,v,limit in [('x',sc[16]+x,m[2]//2),('y',sc[17]+y,m[3]//2)]]
        if down is not None:events.append({'type':'btn','data':{'button':'left','down':down}})
        probe.q.send('input-send-event',dict(events=events))
    try:
        probe.open();probe.tap(525,35);probe.until(lambda:probe.state()[8]==0 and probe.state()[13]==0 and not probe.state()[891])
        for i in (1,2):probe.tap(471,35);probe.until(lambda:probe.state()[8]==i)
        if a.load:workers=[subprocess.Popen(['python3','-c','import time\ne=time.monotonic()+30\nwhile time.monotonic()<e: sum(i*i for i in range(30000))']) for _ in range(4)]
        throttle(2*1024*1024)
        probe.q.send('input-send-event',dict(events=[{'type':'key','data':{'down':v,'key':{'type':'qcode','data':'equal'}}} for v in (True,False)]))
        probe.until(lambda:probe.state()[8]==3 and probe.state()[13]==1)
        point(200,250,True);held=True;probe.until(lambda:probe.state()[515]);samples=[];start=probe.state();phases=[]
        for i in range(12):
            old=probe.state();began=time.monotonic();point(208+i*6,254+i*3,None)
            st=probe.until(lambda:probe.state() if probe.state()[3]>old[3] and probe.state()[5:7]!=old[5:7] else None)
            assert st[4] and st[515] and st[13]==1 and st[881],st[:16]
            r=st[16+(st[3]%32)*14:30+(st[3]%32)*14]
            samples.append((time.monotonic()-began)*1000);phases.append((r[8]-r[2])/1000);time.sleep(.02)
        capture(probe.q,probe.data/'maps-loading.ppm',True);subprocess.run(['magick',str(probe.data/'maps-loading.ppm'),str(out/'maps-loading.png')],check=True)
        point(274,287,False);held=False;throttle(0);probe.until(lambda:probe.state()[13]==0 and not probe.state()[881])
        assert statistics.median(samples)<250 and max(samples)<500,samples
        result=dict(passed=True,loader_sha256=probe.m['loader_sha256'],harness_sha256=hashlib.sha256(source).hexdigest(),saved_data_used=False,pixel_wifi_touched=False,host_cpu_workers=len(workers),samples=12,all_frames_while_loading=True,throttled_map_disk_bps=2*1024*1024,observed_ms=dict(median=statistics.median(samples),maximum=max(samples)),handler_to_publication_ms=dict(median=statistics.median(phases),maximum=max(phases)),raw_observed_ms=samples,scope='QMP drag and composed GPU scanout while map I/O is pending; not physical input-to-display latency')
        (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        try:
            throttle(0)
            if held:point(274,287,False)
        finally:
            for w in workers:
                if w.poll() is None:w.terminate()
                w.wait()
            probe.close()
if __name__=='__main__':main()
