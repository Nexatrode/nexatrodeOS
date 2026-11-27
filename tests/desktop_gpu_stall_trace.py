#!/usr/bin/env python3
"""Correlate timestamped QEMU GPU events with disposable guest timing samples.

Host spans start at command dispatch, not the guest queue kick. The residual
includes queue dispatch, guest wakeup and observation timing; it is not a direct
measurement of any one of those. Trace overhead and host load remain in results.
"""
import argparse,json,re,math,statistics
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

def summary(values):
    values=sorted(values)
    return dict(count=len(values),median=statistics.median(values),p95=values[math.ceil(.95*len(values))-1],max=max(values)) if values else None

def parse_commands(lines):
    commands={};dispatch={};events={}
    rx=re.compile(r'^\d+@(\d+\.\d+):(\w+)\s*(.*)$')
    types={'virtio_gpu_cmd_ctx_submit':0x207,'virtio_gpu_cmd_set_scanout':0x103,'virtio_gpu_cmd_res_flush':0x104}
    for line in lines:
        match=rx.match(line)
        if not match:continue
        now=float(match[1]);name=match[2];body=match[3]
        events[name]=events.get(name,0)+1
        if name in types:
            dispatch[types[name]]=now
        elif name=='virtio_gpu_fence_ctrl':
            fence=re.search(r'fence 0x([0-9a-f]+), type 0x([0-9a-f]+)',body)
            if fence:
                ident,kind=(int(value,16) for value in fence.groups())
                commands[ident]=dict(id=ident,type=kind,ctrl=now,start=dispatch.pop(kind,now),
                    draw_start=dispatch.get(0x207),scanout_start=dispatch.get(0x103))
        elif name=='virtio_gpu_fence_resp':
            fence=re.search(r'fence 0x([0-9a-f]+)',body)
            if fence and int(fence[1],16) in commands:
                cmd=commands[int(fence[1],16)]
                cmd.update(done=now,execution_ms=(cmd['ctrl']-cmd['start'])*1000,
                    fence_ms=(now-cmd['ctrl'])*1000,total_ms=(now-cmd['start'])*1000)
    return commands,events

def correlate(commands,runs):
    frames=[]
    for label,run in runs.items():
        for row in run['samples']:
            if len(row)<20 or not row[16]:continue
            draw=commands.get(row[16]);flush=commands.get(row[17])
            if not flush or 'done' not in flush:continue
            if draw is None and flush.get('draw_start') is not None:
                draw={'id':row[16],'type':0x207,'start':flush['draw_start'],'fenced':False}
            if not draw:continue
            scanout=commands.get(row[17]-1)
            scanout_start=scanout['start'] if scanout else flush.get('scanout_start')
            host_ms=(flush['done']-draw['start'])*1000
            if host_ms<0:raise ValueError('host clock moved backwards')
            frames.append(dict(run=label,frame=row[0],guest_frame_ms=row[3]/1000,
                guest_present_ms=row[6]/1000,max_guest_poll_gap_ms=row[18]/1000,guest_polls=row[19],
                host_frame_ms=host_ms,queue_or_wakeup_residual_ms=row[6]/1000-host_ms,
                host_draw_to_scanout_ms=(scanout_start-draw['start'])*1000 if scanout_start else None,
                draw=draw,scanout=scanout,flush=flush))
    return frames

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True)
    p.add_argument('--trace',type=Path,required=True);p.add_argument('--runs',nargs='+',required=True);p.add_argument('--label',required=True);a=p.parse_args()
    guest=a.guest.resolve();trace=a.trace.resolve()
    assert guest.is_relative_to(ROOT/'build/desktop-apps') and trace.is_relative_to(guest)
    assert trace.stat().st_size<=128*1024*1024
    assert all(label.replace('-','').isalnum() for label in [a.label,*a.runs])
    assert json.loads((guest/'session.json').read_text())['saved_data_used'] is False
    runs={label:json.loads((guest/('motion-'+label+'.json')).read_text()) for label in a.runs}
    assert all(run['saved_data_used'] is False for run in runs.values())
    commands,events=parse_commands(trace.read_text(errors='replace').splitlines());frames=correlate(commands,runs)
    assert frames,'no matching GPU frame IDs; use --gpu-command-trace and current motion instrumentation'
    result=dict(scope=__doc__,trace=str(trace),events=events,
        types={hex(typ):{key:summary([c[key] for c in commands.values() if c['type']==typ and key in c]) for key in ('execution_ms','fence_ms','total_ms')} for typ in (0x207,0x103,0x104)},
        host_frame_ms=summary([f['host_frame_ms'] for f in frames]),
        host_draw_to_scanout_ms=summary([f['host_draw_to_scanout_ms'] for f in frames if f['host_draw_to_scanout_ms'] is not None]),
        frames=frames,slowest_frames=sorted(frames,key=lambda f:f['guest_frame_ms'],reverse=True)[:10])
    (guest/('gpu-stall-analysis-'+a.label+'.json')).write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ('frames','slowest_frames')},indent=2))

if __name__=='__main__':main()
