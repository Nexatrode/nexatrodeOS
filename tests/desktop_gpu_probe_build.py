#!/usr/bin/env python3
"""Build a diskless seL4 VirtIO-GPU capability probe from source."""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--inputs',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--desktop-size',action='store_true');p.add_argument('--skip-clear',action='store_true',help='negative control: omit rendering');a=p.parse_args()
 out=a.output.resolve();assert out.is_relative_to(ROOT/'build') and not out.exists();out.mkdir(parents=True)
 sdk=a.inputs.resolve()/'sdk';board=sdk/'board/qemu_virt_aarch64_el1/smp-debug'
 env=dict(os.environ,PATH='/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:'+os.environ['PATH'])
 def run(*v):subprocess.run(list(map(str,v)),env=env,check=True)
 source=ROOT/'experiments/desktop-gpu/probe.c'
 run('clang','--target=aarch64-none-elf','-mcpu=cortex-a53','-mgeneral-regs-only','-mstrict-align','-O2',*(['-DGPU_PROBE_SKIP_CLEAR'] if a.skip_clear else []),*(['-DGPU_PROBE_DESKTOP'] if a.desktop_size else []),'-ffreestanding','-fno-builtin','-nostdlib','-Wall','-Wextra','-Werror','-I',board/'include','-c',source,'-o',out/'probe.o')
 run('ld.lld','-L',board/'lib',out/'probe.o','-lmicrokit','-T','microkit.ld','-o',out/'probe.elf')
 (out/'probe.system').write_text('''<?xml version="1.0"?>
<system>
 <memory_region name="mmio" size="0x4000" phys_addr="0xa000000"/>
 <memory_region name="dma" size="0x4000000" page_size="0x200000"/>
 <protection_domain name="gpu_probe" priority="100">
  <program_image path="probe.elf"/>
  <map mr="mmio" vaddr="0x2000000" perms="rw" cached="false" setvar_vaddr="mmio"/>
  <map mr="dma" vaddr="0x60000000" perms="rw" cached="false" setvar_vaddr="dma"/>
  <setvar symbol="dma_paddr" region_paddr="dma"/>
 </protection_domain>
</system>
''')
 run(sdk/'bin/microkit',out/'probe.system','--board','qemu_virt_aarch64_el1','--config','smp-debug','--search-path',out,'-o',out/'loader.img','-r',out/'report.txt')
 sha=lambda x:hashlib.sha256(x.read_bytes()).hexdigest()
 (out/'build.json').write_text(json.dumps({'loader_sha256':sha(out/'loader.img'),'sources':{str(s.relative_to(ROOT)):sha(s) for s in (source,Path(__file__).resolve(),ROOT/'tests/desktop_gpu_probe_utm.py')},'disk':False,'network':False,'rendering_tested':False,'skip_clear':a.skip_clear,'resolution':[2560,1600] if a.desktop_size else [64,64]},indent=2)+'\n')
if __name__=='__main__':main()
