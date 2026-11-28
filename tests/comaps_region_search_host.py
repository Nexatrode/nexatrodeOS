#!/usr/bin/env python3
"""Verify full-source index coverage, original anchors and bounded parsing."""
import hashlib,json,struct,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'experiments/comaps-native'
sys.path.insert(0,str(HERE))
from prepare import prepare,prepare_storage
from spatial import FEATURE
ENTRY=struct.Struct('<96sffI')
def main():
    base=ROOT/'build/comaps';prepare(base);out=base/'search-tests';out.mkdir(exist_ok=True);(out/'.disposable-build').touch()
    catalog=prepare_storage(base,out);profile=catalog['regions'][2]['spatial'];pin=profile['search'];path=out/'spatial/Liechtenstein.nxsearch';raw=path.read_bytes()
    assert hashlib.sha256(raw).hexdigest()==pin['sha256'] and len(raw)==pin['bytes']
    styles=[int(s) for s in (base/'styles.h').read_text().split('{')[1].split('}')[0].split(',')];expected=[]
    with (out/'spatial/Liechtenstein.features').open('rb') as stream:
        while head:=stream.read(140):
            f=FEATURE.unpack(head);points=stream.read(f[1]*8);style=0
            for t in f[7:15][:f[15]]:
                value=styles[t] if t<len(styles) else 0
                if not style or value in (1,3,5):style=value
            if f[17].split(b'\0')[0] and ((f[16]==0 and style==8) or (f[16]==1 and style in (4,5))):expected.append(ENTRY.pack(f[17],*struct.unpack_from('<ff',points,(f[1]//2)*8),f[16]))
    assert raw[16:]==b''.join(expected) and len(expected)==3275
    with (out/'maps.img').open('rb') as stream:stream.seek(pin['offset']);assert stream.read(len(raw))==raw
    with (out/'spatial/Liechtenstein.nxmap').open('rb') as stream:
        count=struct.unpack('<4I',stream.read(16))[1];overview=[]
        for _ in range(count):
            f=FEATURE.unpack(stream.read(140));overview.append(f[17].split(b'\0')[0]);stream.seek(f[1]*8,1)
    assert b'Altneugut' not in overview and b'Ruggell' in overview
    cc='/opt/homebrew/opt/llvm/bin/clang'
    for name in ('catalogue','search'):
        exe=out/(name+'-test');subprocess.run([cc,'-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all',str(ROOT/('tests/comaps_'+name+'_test.c')),'-o',str(exe)],check=True)
        subprocess.run([str(exe)]+([str(path)] if name=='catalogue' else []),check=True)
    result=dict(passed=True,source_sha256=catalog['regions'][2]['sha256'],index=pin,original_name_anchor_coverage=True,minor_road_absent_from_overview=True,parser_sanitizers=True)
    (out/'validation.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()
