#!/usr/bin/env python3
"""Add the display-agent transport probe to a verified disposable desktop image."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[1]


def attach(system):
    def region(name,size,physical=None,page=4096):
        a={'name':name,'size':hex(size),'page_size':hex(page)}
        if physical is not None:a['phys_addr']=hex(physical)
        ET.SubElement(system,'memory_region',**a)
    region('desktop_pci_ecam',0x100000,0x3f000000)
    region('desktop_serial_bar',0x10000,0x10000000)
    region('desktop_serial_dma',0x200000,page=0x200000)
    region('display_request',4096,0x5fb90000)
    region('display_ack',4096,0x5fb91000)
    d=ET.SubElement(system,'protection_domain',name='display_agent',priority='127',budget='500',period='10000',stack_size='0x10000')
    ET.SubElement(d,'program_image',path='display_agent.elf')
    for name,va,symbol,cached in [('desktop_pci_ecam',0x40000000,'pci_ecam',False),('desktop_serial_bar',0x41000000,'pci_bar',False),('desktop_serial_dma',0x42000000,'serial_dma',False),('display_request',0x43000000,'display_request',True),('display_ack',0x43001000,'display_ack',True)]:
        ET.SubElement(d,'map',mr=name,vaddr=hex(va),perms='rw' if name!='display_ack' else 'r',cached=str(cached).lower(),setvar_vaddr=symbol)
    ET.SubElement(d,'setvar',symbol='serial_dma_paddr',region_paddr='desktop_serial_dma')
    for a,x,b,y,pp in [('display_agent',0,'desktop_manager',31,False),('display_agent',1,'timer_driver',11,True)]:
        c=ET.SubElement(system,'channel');ea=ET.SubElement(c,'end',pd=a,id=str(x));ET.SubElement(c,'end',pd=b,id=str(y))
        if pp:ea.set('pp','true')


def compile_agent(inputs,out):
    board=inputs/'sdk/board/qemu_virt_aarch64_el1/smp-debug'
    env=dict(os.environ,PATH='/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:'+os.environ['PATH'])
    subprocess.run(['clang','--target=aarch64-none-elf','-mcpu=cortex-a53','-mgeneral-regs-only','-mstrict-align','-O2','-ffreestanding','-fno-builtin','-nostdlib','-Wall','-Wextra','-Werror','-I',str(board/'include'),'-c',str(ROOT/'experiments/desktop-gpu/display_agent.c'),'-o',str(out/'display_agent.o')],env=env,check=True)
    subprocess.run(['ld.lld','-L',str(board/'lib'),str(out/'display_agent.o'),'-lmicrokit','-T','microkit.ld','-o',str(out/'display_agent.elf')],env=env,check=True)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--base',type=Path,required=True);p.add_argument('--inputs',type=Path,required=True);p.add_argument('--composer',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    base=a.base.resolve();out=a.output.resolve();inputs=a.inputs.resolve()
    assert base.is_relative_to(ROOT/'build/desktop-apps') and out.is_relative_to(ROOT/'build/desktop-apps') and not out.exists()
    m=json.loads((base/'build.json').read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
    assert m['saved_data_used'] is False and m['virgl'] and sha(base/'loader.img')==m['loader_sha256']
    # The runner is test-only and now enables agent negotiation for this new
    # fixture. All compiled guest sources must still match the verified base.
    for name,h in m['sources'].items():
        if name!='tests/desktop_apps_run.py':assert sha(ROOT/name)==h,name
    out.mkdir()
    for name,h in m['artifacts'].items():assert sha(base/name)==h;shutil.copyfile(base/name,out/name)
    shutil.copyfile(base/'blk-client-public-config.data',out/'blk-client-public-config.data')
    system=ET.parse(base/'desktop-apps.system').getroot();attach(system);compile_agent(inputs,out)
    ET.indent(system);ET.ElementTree(system).write(out/'desktop-apps.system',encoding='utf-8',xml_declaration=True)
    subprocess.run([str(a.composer.resolve()),str(out/'desktop-apps.system'),'--board','qemu_virt_aarch64_el1','--config','smp-debug','--search-path',str(out),'-o',str(out/'loader.img'),'-r',str(out/'report.txt')],env=dict(os.environ,MICROKIT_SDK=str(inputs/'sdk')),check=True)
    m.update(display_agent_probe=True,loader_sha256=sha(out/'loader.img'))
    m['artifacts'].update({'display_agent.elf':sha(out/'display_agent.elf')})
    for name in ('tests/desktop_apps_run.py','tests/desktop_resize_agent_build.py','experiments/desktop-gpu/display_agent.c','experiments/desktop-gpu/display_protocol.h'):m['sources'][name]=sha(ROOT/name)
    (out/'build.json').write_text(json.dumps(m,indent=2)+'\n')


if __name__=='__main__':main()
