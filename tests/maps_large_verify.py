#!/usr/bin/env python3
"""Audit a completed disposable run against the final rebuilt runtime.

During development the UI harness can run before documentation/test fingerprints
are finalized. This audit accepts only those two noncompiled source differences;
it requires identical installed/final loaders and every ELF, plus current
compiled-source fingerprints. Documentation/harness changes are recorded separately. It never installs an image or opens a regular VM disk.
"""
import argparse,hashlib,json,plistlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def audit(stage):
 stage=stage.resolve();assert stage in (ROOT/'build/mobile-browser/maps-large-files',ROOT/'build/desktop-apps/maps-large-files') and (stage/'.disposable-build').is_file()
 m=json.loads((stage/'build.json').read_text());session=json.loads((stage/'session.json').read_text());data=Path(session['bundle']).resolve()/'Data'
 assert data.parent.parent==stage and data.parent.suffix=='.utm' and m['saved_data_used'] is session['saved_data_used'] is False
 assert m['map_large_import'] and not any((data/n).is_symlink() for n in ('blank.img','maps.img','loader.img'))
 cfg=plistlib.loads((data.parent/'config.plist').read_bytes());assert cfg['Information']['UUID']==session['uuid'] and cfg['System']['CPUCount']==4 and cfg['QEMU']['Hypervisor']
 old=json.loads((data/'build-manifest.json').read_text());assert old['loader_sha256']==m['loader_sha256']==session['loader_sha256']==sha(data/'loader.img')==sha(stage/'loader.img')
 assert old['artifacts']==m['artifacts'] and all(sha(stage/n)==h for n,h in m['artifacts'].items())
 assert old['sources'].keys()==m['sources'].keys()
 differences=[n for n,h in old['sources'].items() if m['sources'][n]!=h]
 assert set(differences)<= {'experiments/comaps-native/README.md','tests/maps_large_utm.py'},differences
 current_differences=[n for n,h in m['sources'].items() if sha(ROOT/n)!=h]
 assert set(current_differences)<= {'experiments/comaps-native/README.md','tests/maps_large_utm.py'},current_differences
 out=stage/'large-files-verification';run=json.loads((out/'validation.json').read_text());assert run['loader_sha256']==m['loader_sha256'] and run['saved_data_used'] is False
 assert run['source_bytes']==13816320 and run['region_versions'][3]==2 and len(run['checks'])>=9 and len(run['lock_observations'])==4 and all(x['picker_erased'] for x in run['lock_observations'])
 resumed=run.get('initial_version',0)==1
 assert [s['label'] for s in run['samples']]==(['v2','restarted'] if resumed else ['v1','v2','restarted']) and [t['version'] for t in run['install_timings']]==([2] if resumed else [1,2])
 for sample in run['samples']:
  assert 0<=sample['peak_workspace']<=sample['workspace_limit']==8388608 and sample['loaded_region']==4 and sample['names']==3275
 for sample in run['samples']:
  if sample['label']!='restarted':assert sample['peak_workspace']==3065244
 assert any(i['trigger_phase']==30 and i['trigger_state']==3 for i in run['lock_observations']) and any(i['initial_phase']==31 for i in run['lock_observations'])
 result=dict(run,passed=True,uuid=session['uuid'],runtime_rebuild_identical=True,all_compiled_source_fingerprints_verified=True,current_noncompiled_changes=current_differences,noncompiled_fingerprint_changes=differences,provisional_report_sha256=sha(out/'validation.json'),executed_harness_sha256=sha(out/'harness-used.py'),current_harness_sha256=sha(ROOT/'tests/maps_large_utm.py'),artifacts=m['artifacts'])
 (out/'audited-validation.json').write_text(json.dumps(result,indent=2)+'\n');session.update(maps_large_files_passed=True,large_files_validation=str(out/'audited-validation.json'));(stage/'session.json').write_text(json.dumps(session,indent=2)+'\n');return result
if __name__=='__main__':
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);a=p.parse_args();r=audit(a.guest);print('PASS final rebuilt runtime and completed larger-import matrix',r['uuid'])
