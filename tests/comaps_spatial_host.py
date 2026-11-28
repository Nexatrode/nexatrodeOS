#!/usr/bin/env python3
"""Profile larger source and validate fixed-memory pages with ASan/UBSan."""
import hashlib,importlib.util,json,subprocess,sys,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'experiments/comaps-native'
sys.path.insert(0,str(HERE));from prepare import prepare,prepare_storage

def main():
    base=ROOT/'build/comaps';prepare(base);out=base/'spatial-tests';out.mkdir(exist_ok=True);(out/'.disposable-build').touch()
    catalog=prepare_storage(base,out);r=catalog['regions'][2];profile=r['spatial']
    cc='/opt/homebrew/opt/llvm/bin/clang';exe=out/'page-test'
    subprocess.run([cc,'-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all','-I'+str(HERE),str(ROOT/'tests/comaps_spatial_test.c'),'-o',str(exe)],check=True)
    records=[]
    with (out/'spatial/Liechtenstein.nxmap').open('rb') as stream:
        for i,p in enumerate(profile['pages']):
            stream.seek(p['offset']);raw=stream.read(p['bytes']);assert hashlib.sha256(raw).hexdigest()==p['sha256']
            path=out/'page.bin';path.write_bytes(raw)
            # Sanitized parser on every page, deeper deterministic mutations on
            # representative overview, large and small pages (not empty cells).
            log=subprocess.check_output([str(exe),str(path)]+([] if i in (0,16,32,48,64) else ['--quick']),text=True);records.append(dict(page=i,log=log.strip()))
    assert profile['features']==58130 and profile['points']==906581
    result=dict(passed=True,source_sha256=r['sha256'],profile=profile,checks=records,loader_file_buffer_bytes=8*1024*1024,decoded_features_cap=32768,decoded_points_cap=524288)
    (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='profile'},indent=2))
if __name__=='__main__':main()
