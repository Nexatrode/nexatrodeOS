#!/usr/bin/env python3
"""Check the whole relocated loader payload, source Image and actual copy targets."""
from __future__ import annotations
import argparse
import importlib.util
import json
import re
import struct
from pathlib import Path
PORT=Path(__file__).resolve().parent

def load(name):
    spec=importlib.util.spec_from_file_location(name,PORT/(name+'.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m

def validate_range(label,start,size,ram,regions):
    if not size or not any(start>=base and start+size<=base+length for base,length in ram):
        raise ValueError(label+': outside a fixed DT RAM bank')
    for name,base,length,_ in regions:
        if start<base+length and base<start+size:raise ValueError(label+': overlaps fixed reservation '+name)

def validate_disjoint(source,source_bytes,destination,destination_bytes):
    if source<destination+destination_bytes and destination<source+source_bytes:
        raise ValueError('entire source Image overlaps the relocated loader payload')

def validate_generated_task_manifest(out,manifest):
    sources=sorted(out.glob('loader-*-source/src/loader-task-manifest.h'))
    if len(sources)!=1:
        raise ValueError('expected one generated loader task-manifest header')
    text=sources[0].read_text()
    header=re.search(r'loader_task_expected_header\[8\]\s*=\s*\{([^}]*)\}',text)
    regions=re.search(r'loader_task_expected_regions\[11\]\[4\]\s*=\s*\{(.*?)\};',text,re.S)
    if not header or not regions:
        raise ValueError('generated loader task-manifest header is incomplete')
    parse=lambda body:[int(token.removesuffix('ull'),0) for token in re.findall(r'0x[0-9a-fA-F]+ull',body)]
    actual_header=parse(header.group(1))
    actual_regions=parse(regions.group(1))
    expected_header=[int(value,0) for value in manifest['header']]
    expected_regions=[int(value,0) for row in manifest['regions'] for value in row]
    if actual_header!=expected_header or actual_regions!=expected_regions:
        raise ValueError('generated loader source manifest is stale; rebuild the complete image after changing the task manifest')

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out-dir',type=Path,required=True);args=p.parse_args();out=args.out_dir
    audit=load('audit-capdl-memory');sdk=load('prepare-image-loader-sdk');pack=load('repack-boot-v2')
    kernel_regions,_=audit.read_regions(audit.DEFAULT_DTB)
    live=audit.ROOT/'.cache/pixel-sunfish/live-runtime.dtb';live_regions,_=audit.read_regions(live)
    ram=[]
    cells=audit.read_cells(live,'/memory','reg')
    assert cells and len(cells)%4==0
    ram.extend(((cells[i]<<32)|cells[i+1],(cells[i+2]<<32)|cells[i+3]) for i in range(0,len(cells),4))
    profile=json.loads((out/'loader-sdk-profile.json').read_text())
    source=int(profile.get('source_image_base','0x80080000'),0)
    b=(out/'loader.img').read_bytes();wrapped=(out/'modem-image-probe.Image').read_bytes();assert wrapped[4096:]==b
    relocated=sdk.segments(out/'loader-relocated-build/loader.elf');base=relocated[0][0]
    assert base==int(profile['loader_link_address'],0)
    validate_disjoint(source,len(wrapped),base,len(b))
    assert b[:64]==relocated[0][3][:64], 'loader entry differs from the private ELF'
    header_at=max(start+length for start,_,length,_ in relocated)-base
    magic,size,entry,initial_start,initial_end,pv,ve,count=struct.unpack_from('<8Q',b,header_at)
    assert magic==0x5e14dead14de5ead and size+header_at==len(b) and 0<count<=64
    if profile.get('task_images',False):
        manifest_path=sdk.task_manifest_path(profile.get('stage_modem_image',False),profile.get('authenticate_modem',False),profile.get('observe_modem_ready',False),profile.get('handshake_modem_status',False) or profile.get('modem_servreg_state_probe',False) or profile.get('modem_servreg_restart_probe',False) or profile.get('modem_tftp_observer',False),profile.get('fatal_trace',False),profile.get('qrtr_servreg_probe',False),profile.get('qrtr_servreg_pdr_followup',False),profile.get('glink_open_plan_snapshot',False),profile.get('glink_open_probe',False),profile.get('glink_channel_probe',False),profile.get('glink_intents_probe',False),profile.get('glink_rx_packet_probe',False),profile.get('glink_done_probe',False),profile.get('qrtr_hello_probe',False),profile.get('qrtr_next_probe',False),profile.get('qrtr_discovery_probe',False),profile.get('qrtr_credit_probe',False),profile.get('qrtr_wait_probe',False),profile.get('qrtr_servreg_pdr_reply',False),profile.get('modem_servreg_state_probe',False) or profile.get('modem_servreg_restart_probe',False),profile.get('modem_servreg_restart_probe',False),profile.get('modem_tftp_observer',False))
        manifest=json.loads(manifest_path.read_text())
        validate_generated_task_manifest(out,manifest)
        if profile.get('task_manifest_sha256'):
            import hashlib
            assert hashlib.sha256(manifest_path.read_bytes()).hexdigest()==profile['task_manifest_sha256']
        assert header_at%8==0 and header_at<=0x20000
        assert list(struct.unpack_from('<8Q',b,header_at))==[int(v,0) for v in manifest['header']]
        assert [[hex(v) for v in struct.unpack_from('<4Q',b,header_at+64+i*32)] for i in range(count)]==manifest['regions']
        import hashlib
        assert hashlib.sha256((out/'modem_image_probe.elf').read_bytes()).hexdigest()==manifest['native_pd_elf_sha256']
    targets=[];data_start=header_at+64+count*32
    for i in range(count):
        start,length,offset,kind=struct.unpack_from('<4Q',b,header_at+64+i*32)
        assert kind==1 and data_start+offset+length<=len(b)
        targets.append((start,length))
    for regions in [kernel_regions,live_regions]:
        validate_range('source Image',source,len(wrapped),ram,regions)
        validate_range('entire relocated loader',base,len(b),ram,regions)
        validate_range('initial task memory',initial_start,initial_end-initial_start,ram,regions)
        for i,(start,length) in enumerate(targets):
            validate_range('copy target'+str(i),start,length,ram,regions)
            if start<base+len(b) and base<start+length:raise ValueError('loader relocation overlaps a copy target')
    for i,(start,length) in enumerate(targets):
        if any(start<other+size and other<start+length for other,size in targets[i+1:]):raise ValueError('copy targets overlap')
    for start,length,name in audit.read_pages(out/'report.txt'):
        if start<base+len(b) and base<start+length:raise ValueError('relocated payload aliases AP frame '+name)
    _,parts,_=pack.components((out/'fastboot.img').read_bytes())
    reference=(Path.home()/'Backups/pixel-sunfish-20261006/live-slots/boot_a.img').read_bytes();_,original,_=pack.components(reference)
    assert parts['dtb']==original['dtb'] and parts['ramdisk']==(out/'minimal-ramdisk.cpio.gz').read_bytes()
    assert len(parts['ramdisk'])<4096
    result={'source_image_base':hex(source),'source_image_end':hex(source+len(wrapped)),
            'relocated_loader_base':hex(base),'relocated_loader_end':hex(base+len(b)),
            'entire_relocated_payload_bytes':len(b),'initial_task_start':hex(initial_start),'initial_task_end':hex(initial_end),
            'actual_copy_targets':[[hex(start),hex(length)] for start,length in targets],
            'kernel_and_cached_live_fixed_reservations_avoided':True,'cached_live_ram_banks_contain_ranges':True,
            'relocated_payload_disjoint_from_copy_targets':True,'copy_targets_disjoint':True,
            'entire_source_and_relocated_payload_disjoint':True,
            'loader_task_manifest_source_matches':True,
            'original_boot_dtb_preserved':True,'minimal_ramdisk_bytes':len(parts['ramdisk']),
            'scope':'Build/packing range proof using the source placement observed for the earlier large native image; runtime guards must still confirm the placement before copying. Not a successful native boot or matched ABL source contract.'}
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()
