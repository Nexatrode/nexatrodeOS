#!/usr/bin/env python3
"""Repeat cold starts of an explicitly disposable desktop; capture failed SMP boots.

Never resets a disk or edits a VM configuration. Captures only disposable kernel
state and timing counters, and explicitly selects each vCPU in the QMP request.
"""
import argparse, hashlib, json, os, re, struct, subprocess, time
from pathlib import Path
from desktop_gpu_motion import MotionQmp
from desktop_boot_ready import gpu_boot_state

ROOT = Path(__file__).resolve().parents[1]
CTL = '/Applications/UTM.app/Contents/MacOS/utmctl'
LLVM = Path('/opt/homebrew/opt/llvm/bin')

def registers(q, core):
    raw = q.send('human-monitor-command', {'command-line': 'info registers', 'cpu-index': core})['return']
    assert re.search(rf'\bCPU#{core}\b', raw), 'monitor did not select the requested vCPU'
    return dict(raw=raw, values={k:int(v,16) for k,v in re.findall(r'\b(PC|X\d+|SP)=\s*([a-fA-F0-9]+)', raw)})

def kernel_symbol(kernel, name):
    output = subprocess.check_output([LLVM/'llvm-nm', '-S', kernel], text=True)
    row = next(line.split() for line in output.splitlines() if line.split()[-1:] == [name])
    return int(row[0],16), int(row[1],16)

def physical(kernel, address):
    elf = kernel.read_bytes()
    assert elf[:6] == b'\x7fELF\x02\x01'
    offset = struct.unpack_from('<Q', elf, 32)[0]
    size, count = struct.unpack_from('<HH', elf, 54)
    for i in range(count):
        typ,flags,off,va,pa,filesz,memsz,align = struct.unpack_from('<II6Q', elf, offset+i*size)
        if typ == 1 and va <= address < va+memsz:
            return pa+address-va
    raise ValueError('kernel symbol outside load segments')

def capture(q, kernel, data, out):
    # Pause only this disposable machine to make the lock graph coherent.
    q.send('stop')
    cpus = q.send('query-cpus-fast')['return']
    indices = sorted(row['cpu-index'] for row in cpus)
    assert indices == [0,1,2,3]
    samples = {str(i):registers(q,i) for i in indices}
    for sample in samples.values():
        sample['symbol'] = subprocess.check_output([LLVM/'llvm-addr2line','-f','-e',kernel,hex(sample['values']['PC'])],text=True).strip()
    va, size = kernel_symbol(kernel,'big_kernel_lock')
    tmp = data/'boot-kernel-lock.bin'
    q.send('pmemsave',{'val':physical(kernel,va),'size':size,'filename':str(tmp)})
    lock = tmp.read_bytes();tmp.unlink()
    (out/'kernel-lock.bin').write_bytes(lock)
    # These offsets describe the pinned four-core SDK's CLH ABI (64-byte lines).
    nodes = [dict(watch=hex(a), myreq=hex(b), ipi=c) for a,b,c in
             (struct.unpack_from('<3Q',lock,5*64+i*64) for i in indices)]
    result = dict(cpus=cpus, samples=samples, kernel_sha256=hashlib.sha256(kernel.read_bytes()).hexdigest(),
                  lock_va=hex(va), lock_size=size, request_states=[struct.unpack_from('<I',lock,i*64)[0] for i in range(5)],
                  nodes=nodes, tail=hex(struct.unpack_from('<Q',lock,9*64)[0]))
    (out/'kernel-state.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--guest',type=Path,required=True)
    p.add_argument('--kernel',type=Path,required=True)
    p.add_argument('--label',required=True)
    p.add_argument('--boots',type=int,default=3)
    p.add_argument('--timeout',type=int,default=45)
    p.add_argument('--stop-on-failure',action='store_true')
    p.add_argument('--early-qmp',action='store_true',help='control: reproduce the old installer polling during early boot')
    a=p.parse_args();assert a.label.replace('-','').isalnum() and 1<=a.boots<=20 and 15<=a.timeout<=180
    guest=a.guest.resolve();assert guest.is_relative_to(ROOT/'build/desktop-apps')
    s=json.loads((guest/'session.json').read_text());b=Path(s['bundle']).resolve();d=b/'Data'
    assert b.parent==guest and b.name.startswith('Desktop-Apps-') and s['saved_data_used'] is False
    m=json.loads((d/'build-manifest.json' if (d/'build-manifest.json').exists() else guest/'build.json').read_text())
    assert m['development_storage_key'] is True and m['saved_data_used'] is False and not m['gpu_verify']
    digest=hashlib.sha256((d/'loader.img').read_bytes()).hexdigest();assert digest==s['loader_sha256']==m['loader_sha256']
    out=guest/('boots-'+a.label);out.mkdir()
    result=dict(uuid=s['uuid'],saved_data_used=False,loader_sha256=digest,
                harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),timeout_seconds=a.timeout,early_qmp=a.early_qmp,runs=[])
    def ctl(action):
        r=subprocess.run([CTL,action,s['uuid']],capture_output=True,text=True,timeout=60)
        assert r.returncode==0,(r.stdout,r.stderr)
    for i in range(a.boots):
        trial=out/str(i+1);trial.mkdir();ctl('stop');started=time.monotonic();ctl('start')
        q=None;gpu=None;error=None;ready=False
        try:
            end=started+a.timeout
            while time.monotonic()<end:
                text=(d/'trace.log').read_text(errors='replace') if (d/'trace.log').exists() else ''
                if any(x in text for x in ('DESKTOP_GPU|FAIL|','DISPLAY_AGENT|FAIL|','VMFault','MON|ERROR','panicked at')):
                    error='guest failure marker';break
                # Avoid polling guest memory during early CapDL initialization.
                if a.early_qmp or 'DESKTOP_GPU|READY|' in text or 'DESKTOP_GPU|frame=' in text:
                    if q is None:q=MotionQmp(d/'monitor');q.connect()
                    state=gpu_boot_state(q,d);gpu=state['gpu']
                    if state['ready']:
                        ready=True;break
                time.sleep(.2)
            elapsed=time.monotonic()-started
            row=dict(boot=i+1,ready=ready,elapsed_seconds=elapsed,gpu=gpu,error=error)
            if ready:row['cpus']=q.send('query-cpus-fast')['return']
            if not ready:
                if q is None:q=MotionQmp(d/'monitor');q.connect()
                row['captured_kernel_state']=capture(q,a.kernel.resolve(),d,trial)
            result['runs'].append(row)
            (trial/'serial.log').write_bytes((d/'trace.log').read_bytes())
            print(json.dumps({k:v for k,v in row.items() if k not in ('captured_kernel_state','gpu')}),flush=True)
        finally:
            if q:q.close()
            (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        if not ready and a.stop_on_failure:break
    result['all_ready']=len(result['runs'])==a.boots and all(r['ready'] for r in result['runs'])
    (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    if not result['all_ready']:raise SystemExit(1)

if __name__=='__main__':main()
