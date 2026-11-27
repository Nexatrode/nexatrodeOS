#!/usr/bin/env python3
"""Build a diskless offline CoMaps-data viewer; never read personal app inputs."""
import argparse, hashlib, json, os, shutil, subprocess, sys
from pathlib import Path
import xml.etree.ElementTree as ET
from prepare import prepare, ROOT, HERE
LLVM = Path('/opt/homebrew/opt/llvm/bin')

def validate(system):
    domains={p.get('name'):p for p in system.findall('protection_domain')}
    assert set(domains)=={'offline_map','gpu_client','gpu_driver','gpu_virt','input_driver','timer_driver','timer_poller'}
    assert {m.get('mr'):m.get('perms') for m in domains['offline_map'].findall('map')}=={
        'slint_frame':'rw','map_heap':'rw','input_handoff':'rw','ui_focus':'rw','ui_pointer':'rw'}
    assert not any(word in m.get('name','') for m in system.findall('memory_region') for word in ('vault','wallet','disk','net','chat'))
    sizes={m.get('name'):int(m.get('size').replace('_',''),0) for m in system.findall('memory_region')}
    for d in domains.values():
        spans=sorted((int(m.get('vaddr').replace('_',''),0),sizes[m.get('mr')]) for m in d.findall('map'))
        assert all(a+n<=b for (a,n),(b,_) in zip(spans,spans[1:]))

def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('inputs','display','composer','output'):p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--resume',action='store_true');a=p.parse_args()
    inputs,display,out=(v.resolve() for v in (a.inputs,a.display,a.output))
    assert out.is_relative_to(ROOT/'build/comaps') and not any(v.suffix=='.utm' for v in (out,*out.parents))
    if out.exists() and any(out.iterdir()):assert a.resume and (out/'.disposable-build').is_file()
    out.mkdir(parents=True,exist_ok=True);(out/'.disposable-build').touch()
    base=ROOT/'build/comaps';pin,src,region=prepare(base)
    public=json.loads((display/'build.json').read_text());assert public['resolution']==[2560,1600] and public['scale']==2 and public['vault'] is False
    board=inputs/'sdk/board/qemu_virt_aarch64_el1/smp-debug'
    sysroot=ROOT/'build/netsurf-native/sysroot'
    stb=ROOT/'build/netsurf-native/deps/stb'
    assert (stb/'stb_truetype.h').is_file() and (sysroot/'lib/libc.a').is_file()
    env=dict(os.environ,PATH=str(LLVM)+':/opt/homebrew/bin:'+os.environ['PATH'])
    def run(cmd,label):
        with (out/(label+'.log')).open('w') as stream:subprocess.run(list(map(str,cmd)),check=True,stdout=stream,stderr=subprocess.STDOUT,env=env)
    flags=['--target=aarch64-none-elf','-mcpu=cortex-a53','-mstrict-align','-ffreestanding','-fno-stack-protector','-ffunction-sections','-fdata-sections','-O2','-Wall','-Wextra','-I'+str(board/'include'),'-I'+str(sysroot/'include'),'-I'+str(HERE),'-I'+str(base),'-I'+str(stb),'-DSLINT_FRAME_WIDTH=2560u','-DSLINT_FRAME_HEIGHT=1600u','-DSLINT_FRAME_REGION_BYTES=0x1000000u']
    for unit in ('viewer','mwm'):run([LLVM/'clang',*flags,'-c',HERE/(unit+'.c'),'-o',out/(unit+'.o')],unit)
    font=ROOT/'experiments/slint-notes/assets/fonts/Roboto-Regular.ttf'
    (out/'assets.S').write_text('.section .rodata\n.balign 8\n.global map_blob,map_blob_end,map_font\nmap_blob:\n.incbin "'+str(region)+'"\nmap_blob_end:\n.balign 8\nmap_font:\n.incbin "'+str(font)+'"\n')
    run([LLVM/'clang','--target=aarch64-none-elf','-c',out/'assets.S','-o',out/'assets.o'],'assets')
    run(['ld.lld','--gc-sections','-L',board/'lib','-L',sysroot/'lib','-T',board/'lib/microkit.ld',out/'viewer.o',out/'mwm.o',out/'assets.o','--start-group','-lmicrokit','-lc','-lm','--end-group','-o',out/'offline-map.elf'],'link')
    run([LLVM/'clang',*flags,'-c',ROOT/'experiments/netsurf-native/poller.c','-o',out/'poller.o'],'poller')
    run(['ld.lld','-L',board/'lib','-T',board/'lib/microkit.ld',out/'poller.o','-lmicrokit','-o',out/'poller.elf'],'poller-link')
    original=ET.parse(display/'desktop.system').getroot();system=ET.Element('system')
    names={'gpu_client','gpu_driver','gpu_virt','input_driver','timer_driver'}
    for d in original.findall('protection_domain'):
        if d.get('name') in names:system.append(d)
    for c in original.findall('channel'):
        if all(e.get('pd') in names for e in c):system.append(c)
    used={m.get('mr') for d in system.findall('protection_domain') for m in d.findall('map')}
    for r in original.findall('memory_region'):
        if r.get('name') in used:system.insert(0,r)
    for name in ('gpu_driver.elf','gpu_virt.elf','gpu_client.elf','input_driver.elf','timer_driver.elf'):
        donor=next(d/name for d in (display,ROOT/'build/desktop-surfaces',inputs/'pds') if (d/name).is_file())
        shutil.copyfile(donor,out/name)
    ET.SubElement(system,'memory_region',name='map_heap',size='0x1000000',page_size='0x200000')
    app=ET.SubElement(system,'protection_domain',name='offline_map',priority='100',budget='8000',period='10000',stack_size='0x100000')
    ET.SubElement(app,'program_image',path='offline-map.elf')
    for name,address,perms,symbol in [('slint_frame',0x60000000,'rw','map_frame'),('map_heap',0x64000000,'rw','map_heap'),('input_handoff',0x70000000,'rw','input_handoff'),('ui_focus',0x70001000,'rw','ui_focus'),('ui_pointer',0x70002000,'rw','ui_pointer')]:
        ET.SubElement(app,'map',mr=name,vaddr=hex(address),perms=perms,cached='true',setvar_vaddr=symbol)
    poller=ET.SubElement(system,'protection_domain',name='timer_poller',priority='1',budget='1000',period='10000');ET.SubElement(poller,'program_image',path='poller.elf')
    ET.SubElement(poller,'map',mr='hvf_timer_shared',vaddr='0x50000000',perms='rw',cached='true',setvar_vaddr='hvf_timer_state')
    for left,lid,right,rid in [('offline_map',0,'gpu_client',2),('offline_map',1,'input_driver',2),('timer_poller',0,'timer_driver',0)]:
        c=ET.SubElement(system,'channel');ET.SubElement(c,'end',pd=left,id=str(lid));ET.SubElement(c,'end',pd=right,id=str(rid))
    validate(system);ET.indent(system);ET.ElementTree(system).write(out/'map.system',encoding='utf-8',xml_declaration=True)
    with (out/'compose.log').open('w') as stream:
        subprocess.run([str(a.composer.resolve()),str(out/'map.system'),'--board','qemu_virt_aarch64_el1','--config','smp-debug','--search-path',str(out),'-o',str(out/'loader.img'),'-r',str(out/'report.txt')],check=True,stdout=stream,stderr=subprocess.STDOUT,env=dict(env,MICROKIT_SDK=str(inputs/'sdk')))
    licenses=out/'licenses';licenses.mkdir(exist_ok=True)
    for name in ('LICENSE','NOTICE'):shutil.copyfile(src/name,licenses/('CoMaps-'+name))
    shutil.copyfile(HERE/'NOTICE',licenses/'NexatrodeOS-NOTICE')
    shutil.copyfile(ROOT/'experiments/slint-notes/assets/fonts/Roboto-OFL.txt',licenses/'Roboto-OFL.txt')
    stb_text=(stb/'stb_truetype.h').read_text();(licenses/'stb-truetype-license.txt').write_text(stb_text[stb_text.rfind('ALTERNATIVE A'):])
    digest=lambda f:hashlib.sha256(f.read_bytes()).hexdigest()
    sources=[f for f in HERE.rglob('*') if f.is_file() and '__pycache__' not in str(f)]
    sources += [ROOT/'tests/comaps_native_utm.py',ROOT/'tests/comaps_native_input.py']
    manifest=dict(profile='comaps-offline-data',saved_data_used=False,storage=False,network=False,gps=False,resolution=[2560,1600],scale=2,full_comaps_app=False,upstream=pin,
        sources={str(f.relative_to(ROOT)):digest(f) for f in sources},third_party={'stb_truetype.h':digest(stb/'stb_truetype.h'),'Roboto-Regular.ttf':digest(font),'mapcss-mapping.csv':digest(src/'data/mapcss-mapping.csv')},
        artifacts={f.name:digest(f) for f in out.glob('*.elf')},system_sha256=digest(out/'map.system'),loader_sha256=digest(out/'loader.img'))
    (out/'build.json').write_text(json.dumps(manifest,indent=2)+'\n');print('PASS built diskless CoMaps data experiment',out,flush=True)
if __name__=='__main__':main()
