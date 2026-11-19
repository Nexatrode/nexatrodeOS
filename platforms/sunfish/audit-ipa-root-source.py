#!/usr/bin/env python3
"""Bind the private-RAM root experiment to the actual IPA memory geometry."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import re

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('linux_ipa_source',HERE/'audit-native-ipa-linux-source.py')
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
RUN=base.ROOT/'.cache/sunfish-native-wifi/private/device-runs/ipa-smem-allocate-v2-20261026T052736Z'

def check(native,adapter,values,requirements):
    imem=int(requirements['imem']['physical'],0)
    if (imem,requirements['imem']['bytes'],values['ITEM_PA'],values['ITEM_BYTES'],values['MAP_CANDIDATE'],values['MAP_PAGES']) != (0x146a8000,8192,0x8611d030,8192,0x8611d000,3):
        raise ValueError('re-audit live/shared-memory geometry')
    for token in ['const ROOT_PROFILE: bool = cfg!(feature = "sunfish-smmu-ipa-root-install-probe");',
                  'if ROOT_PROFILE {\n    37', '0x146a8000 + i as u64 * 4096',
                  '0x8611d000 + (i - 34) as u64 * 4096',
                  '0x8000_0000 + (i - if ROOT_PROFILE { 2 } else { 0 }) as u64 * 4096',
                  'physical_page: data[i]', 'adapter.stage_root_tables(&tables, extra_tables, extra_physical)',
                  '"ALL MAPS PRIVATE RAM"','"IMEM SMEM NOT MAPPED"']:
        if token not in native:raise ValueError('private root profile changed: '+token)
    inner=adapter.split('unsafe fn stage_root_tables_inner',1)[1].split('pub fn select',1)[0]
    if inner.index('self.root_extra = Some((')>inner.index('p.add(i).write_volatile(value)'):
        raise ValueError('table publication before extra frame retention')
    for token in ['(3, 3, 7) | (4, 4, 8)',
                  'matches!(backing, RootBacking::Receive) != (LEAVES == 4)',
                  '!self.data_pages.contains(&mapping.physical_page)',
                  'plan.table_pages() != expected_pages','all_physical[..i].contains(&page)',
                  'self.data_pages.contains(&page)','self.root_extra = Some((',
                  'InstallStep::PublishRootPageTables','self.failed = true;']:
        if token not in adapter:raise ValueError('root adapter ownership/containment changed: '+token)

def audit():
    result=base.audit();path=RUN/'screen/transcription.json';e=json.loads(path.read_text())
    if e['headline']!='SMEM ALLOC GET PASS':raise ValueError('allocation reference did not pass')
    v={k:int(value,16)for k,value in e['values'].items()}
    if any(v[k]!=x for k,x in {'WRITES_SENT':15,'WRITES_DONE':15,'POST_SCANS':2,'RELEASE_SENT':1,'LOCAL_ERROR':0}.items()):
        raise ValueError('allocation evidence differs')
    for name,identity in e['files'].items():
        photo=path.parent/name
        if photo.stat().st_size!=identity['bytes'] or hashlib.sha256(photo.read_bytes()).hexdigest()!=identity['sha256']:
            raise ValueError('allocation photo archive mismatch')
    native=(base.ROOT/'rust/src/pixel_smmu_install.rs').read_text()
    adapter=(base.ROOT/'rust/src/sunfish_smmu_native.rs').read_text()
    check(native,adapter,v,result['shared_memory_prerequisites'])
    cases=[('physical_page: data[i]','physical_page: 0x8611d000'),
           ('"IMEM SMEM NOT MAPPED"','"SHARED MEMORY READY"'),
           ('0x146a8000 + i as u64 * 4096','0x146a9000 + i as u64 * 4096')]
    for old,new in cases:
        if old not in native:raise ValueError('root profile mutation did not apply')
        try:check(native.replace(old,new),adapter,v,result['shared_memory_prerequisites'])
        except ValueError:pass
        else:raise ValueError('root source mutation accepted')
    changed=adapter.replace('!self.data_pages.contains(&mapping.physical_page)','false')
    try:check(native,changed,v,result['shared_memory_prerequisites'])
    except ValueError:pass
    else:raise ValueError('unowned physical mapping source mutation accepted')
    result.update({'private_root_experiment':{'root_indices':[0,2],'root_tables':1,'level2_tables':2,
        'level3_tables':3,'disabled_ttbr1_tables':1,'total_table_frames':7,'private_data_pages':37,
        'iova_spans':[['0x146a8000',8192],['0x80000000',131072],['0x8611d000',12288]],
        'descriptor_count':42,'actual_imem_mapped':False,'actual_smem_mapped':False,
        'scope':'IOVA geometry from the reference; all output PAs are distinct allocator-owned private RAM; no DMA/ATS/PAS or firmware readiness'},
        'root_source_mutations_rejected':4,'allocation_evidence':{'path':str(path),
        'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'files':e['files']}})
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();a.output.write_text(json.dumps(audit(),indent=2)+'\n')
    print('Pinned IPA memory IOVA geometry and private-only root backing verified; 4 mutations rejected.')
