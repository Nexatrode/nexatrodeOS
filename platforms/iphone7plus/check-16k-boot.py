#!/usr/bin/env python3
"""Verify the 16 KiB loader/kernel handoff in generic AArch64 emulation.

QEMU is not an Apple A10 model. Hardware breakpoints observe the original
payload without modifying its instructions. No disk or networking is used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import tempfile
import time


def symbols(path):
    nm = os.environ.get('LLVM_NM', '/opt/homebrew/opt/llvm/bin/llvm-nm')
    out = {}
    for line in subprocess.check_output([nm, str(path)], text=True).splitlines():
        fields = line.split()
        if len(fields) == 3:
            out[fields[2]] = int(fields[0], 16)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('payload', type=Path)
    parser.add_argument('kernel', type=Path)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--user-elf', type=Path)
    parser.add_argument('--initialiser', type=Path)
    parser.add_argument('--observer-elf', type=Path)
    args = parser.parse_args()
    syms = symbols(args.kernel)
    stages = [(name, syms[name]) for name in ['_start', 'init_kernel', 'create_it_address_space']]
    if args.initialiser:
        initialiser_syms = symbols(args.initialiser)
        stages.append(('initialiser_entry', initialiser_syms['_start']))
        stages.append(('initialiser_start_threads', next(addr for name,addr in initialiser_syms.items() if name.endswith('13start_threads'))))
    if args.user_elf:
        stages.append(('protection_domain_screen_ready', symbols(args.user_elf)['smoke_ready']))
    if args.observer_elf:
        stages.append(('timer_observer_ready', symbols(args.observer_elf)['smoke_ready']))
    report = {'sha256': hashlib.sha256(args.payload.read_bytes()).hexdigest(),
              'model': 'QEMU max, 16 KiB capable; not Apple A10', 'stages': [], 'passed': False}
    with tempfile.TemporaryDirectory(prefix='iphone16k-') as directory:
        tmp = Path(directory)
        (tmp/'entry.bin').write_bytes(struct.pack('<IIIIQQ',
            0x58000080, 0x580000a1, 0xd61f0020, 0xd503201f, 0x41000000, 0x8badc0000))
        bootargs = bytearray(256)
        for off, value in {0x28:0x8bb80c000, 0x38:0x1100, 0x40:1080, 0x48:1920}.items():
            struct.pack_into('<Q', bootargs, off, value)
        (tmp/'args.bin').write_bytes(bootargs)
        (tmp/'framebuffer.bin').write_bytes(b'\xff'*(0x1100*1920))
        cmd = ['qemu-system-aarch64', '-machine', 'virt,virtualization=off', '-cpu','max,cntfrq=24000000',
               '-accel','tcg','-icount','shift=0,align=off,sleep=off','-m','36G','-S','-display','none','-serial','none','-nic','none',
               '-qmp',f'unix:{tmp}/qmp,server=on,wait=off',
               '-chardev',f'socket,path={tmp}/gdb,server=on,wait=off,id=gdb0','-gdb','chardev:gdb0']
        for path, addr, cpu in [(tmp/'entry.bin',0x40200000,True),
            (tmp/'args.bin',0x41000000,False), (tmp/'framebuffer.bin',0x8bb80c000,False),
            (args.payload.resolve(),0x8badc0000,False)]:
            cmd += ['-device',f'loader,file={path},addr={addr:#x},force-raw=on'+(',cpu-num=0' if cpu else '')]
        with (tmp/'stderr').open('w') as err:
            proc = subprocess.Popen(cmd,stdout=subprocess.DEVNULL,stderr=err)
            try:
                for _ in range(50):
                    if proc.poll() is not None: raise RuntimeError((tmp/'stderr').read_text())
                    if (tmp/'gdb').exists() and (tmp/'qmp').exists(): break
                    time.sleep(.1)
                with socket.socket(socket.AF_UNIX) as gdb, socket.socket(socket.AF_UNIX) as qmp:
                    gdb.connect(str(tmp/'gdb'));gdb.settimeout(8)
                    qmp.connect(str(tmp/'qmp'));qmp.settimeout(3)
                    stream=qmp.makefile('rwb');stream.readline()
                    def query(execute, arguments=None):
                        item={'execute':execute}
                        if arguments is not None:item['arguments']=arguments
                        stream.write(json.dumps(item).encode()+b'\n');stream.flush()
                        while True:
                            result=json.loads(stream.readline())
                            if 'error' in result:raise RuntimeError(result['error'])
                            if 'return' in result:return result['return']
                    query('qmp_capabilities')
                    def packet():
                        while gdb.recv(1)!=b'$':pass
                        data=b''
                        while True:
                            ch=gdb.recv(1)
                            if ch==b'#':break
                            data+=ch
                        gdb.recv(2);gdb.sendall(b'+');return data.decode()
                    def send(command):
                        raw=command.encode();gdb.sendall(b'$'+raw+b'#'+f'{sum(raw)%256:02x}'.encode())
                        return packet()
                    send('?')
                    # Budget expiry is an expected transition in the observer
                    # probes. Stopping there would conceal whether it scheduled
                    # the lower-priority PD.
                    failure_names = ['handleVMFault', 'handleUserLevelFault']
                    if not args.observer_elf:
                        failure_names.append('endTimeslice')
                    failure_points = {name:syms[name] for name in failure_names if name in syms}
                    if args.initialiser:
                        failure_points.update({name:addr for name,addr in initialiser_syms.items() if name.endswith('9panic_fmt')})
                    for addr in failure_points.values(): assert send(f'Z1,{addr:x},4')=='OK'
                    report['failure_points'] = {name:hex(addr) for name,addr in failure_points.items()}
                    previous_address = None
                    for name, address in stages:
                        if previous_address == address: send('s')
                        assert send(f'Z1,{address:x},4')=='OK'
                        for hit in range(8):
                            timed_out=False
                            try: stop=send('c')
                            except TimeoutError:
                                timed_out=True;gdb.sendall(b'\x03');stop=packet()
                            regs=query('human-monitor-command',{'command-line':'info registers'})
                            pc=int(re.search(r'PC=([0-9a-fA-F]+)',regs)[1],16)
                            if (name not in ('protection_domain_screen_ready', 'timer_observer_ready') or pc != address or
                                send(f'm{address:x},10') == '1f2003d5' * 4):
                                break
                            # Same virtual address in the monitor. Step past it,
                            # then re-arm for the smoke task's address space.
                            assert send(f'z1,{address:x},4')=='OK'
                            send('s')
                            assert send(f'Z1,{address:x},4')=='OK'
                        matched=pc==address and not timed_out
                        if name in ('protection_domain_screen_ready', 'timer_observer_ready') and matched:
                            # A monitor/PD can share this VA. Require the distinctive
                            # NOP sequence and the completed physical screen output.
                            code = send(f'm{address:x},10')
                            matched &= code == '1f2003d5' * 4
                            def physical_word(x, y):
                                v = query('human-monitor-command', {'command-line':f'xp /1wx {0x8bb80c000+y*0x1100+x*4:#x}'})
                                return int(v.split(':',1)[1].strip(),16)
                            if name == 'protection_domain_screen_ready':
                                report['screen_samples'] = {'background':physical_word(0,0),
                                    'one':physical_word(44,28), 'u':physical_word(28,108)}
                                matched &= report['screen_samples'] == {'background':0,'one':0xffffffff,'u':0xffffffff}
                            else:
                                report['timer_screen_sample'] = physical_word(28,188)
                                matched &= report['timer_screen_sample'] == 0xffffffff

                        report['stages'].append({'name':name,'expected_pc':hex(address),
                            'pc':hex(pc),'reached':matched,'registers':regs,'stop':stop})
                        print(name, 'PASS' if matched else 'FAIL',hex(pc),flush=True)
                        assert send(f'z1,{address:x},4')=='OK'
                        previous_address = address
                        if not matched:break
                    report['passed']=all(s['reached'] for s in report['stages']) and len(report['stages'])==len(stages)
                    report['translation_registers']=query('human-monitor-command',{'command-line':'info registers -a'})
            finally:
                proc.terminate()
                try:proc.wait(timeout=3)
                except subprocess.TimeoutExpired:proc.kill();proc.wait()
    args.report.parent.mkdir(parents=True,exist_ok=True)
    args.report.write_text(json.dumps(report,indent=2)+'\n')
    raise SystemExit(0 if report['passed'] else 1)

if __name__=='__main__':main()
