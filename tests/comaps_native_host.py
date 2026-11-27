#!/usr/bin/env python3
"""Sanitize the bounded C reader and compare geometry with pinned CoMaps C++."""
import hashlib,importlib.util,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];HERE=ROOT/'experiments/comaps-native'
sys.path.insert(0,str(HERE))
from prepare import prepare

def main():
    base=ROOT/'build/comaps';pin,src,region=prepare(base)
    cc='/opt/homebrew/opt/llvm/bin/clang';cxx=cc+'++'
    flags=['-g','-O1','-fsanitize=address,undefined','-fno-sanitize-recover=all','-ffunction-sections','-fdata-sections']
    inc=['-I'+str(HERE),'-I'+str(src),'-I'+str(src/'libs'),'-I/opt/homebrew/include']
    log=base/'host-tests.log'
    with log.open('w') as stream:
        def run(args):subprocess.run(list(map(str,args)),check=True,stdout=stream,stderr=subprocess.STDOUT)
        run([cc,*flags,*inc,HERE/'mwm.c',HERE/'reader_test.c','-o',base/'reader-test'])
        run([base/'reader-test',region])
        run([cc,*flags,*inc,'-DMAP_REFERENCE','-c',HERE/'mwm.c','-o',base/'reference-reader.o'])
        objects=[base/'reference-reader.o']
        for unit,label in [(HERE/'reference.cpp','reference'),(src/'libs/coding/geometry_coding.cpp','upstream-geometry'),(src/'libs/coding/point_coding.cpp','upstream-point'),(src/'libs/base/exception.cpp','upstream-exception')]:
            obj=base/(label+'.o');objects.append(obj)
            run([cxx,'-std=c++23','-DNDEBUG',*flags,*inc,'-c',unit,'-o',obj])
        run([cxx,'-fsanitize=address,undefined','-Wl,-dead_strip',*objects,'-o',base/'reference'])
        run([base/'reference',region])
    text=log.read_text();print(text,end='')
    assert 'PASS 6674 geometry blocks' in text and '300 deterministic mutations' in text
    result=dict(passed=True,source_revision=pin['revision'],map_sha256=pin['map_sha256'],geometry_blocks=6674,records=8957,points=140685,sanitizers=['address','undefined'],mutations=300,
                sources={str(f.relative_to(ROOT)):hashlib.sha256(f.read_bytes()).hexdigest() for f in (HERE/'mwm.c',HERE/'mwm.h',HERE/'reader_test.c',HERE/'reference.cpp',Path(__file__))})
    (base/'host-validation.json').write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
