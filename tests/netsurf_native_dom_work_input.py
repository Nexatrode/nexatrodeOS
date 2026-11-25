"""Native innerHTML phase timings and recovery through actual UTM input."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time
from netsurf_native_image_input import Probe

TEST_SHA256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


def exercise(monitor,trace,temporary,evidence,scale,faults=False):
    p=Probe(monitor,trace,temporary,evidence,scale);checks=[]
    p.navigate('resource:dom-work.html');time.sleep(.5)
    raw,_=p.capture();p.save(raw,'dom-work-initial')
    tsv=subprocess.check_output(['tesseract',str(evidence/'dom-work-initial.ppm'),'stdout','--psm','11','tsv'],text=True,stderr=subprocess.DEVNULL)
    def word(token):
        row=next(r for line in tsv.splitlines()[1:] if len(r:=line.split('\t'))>=12 and r[11].lower()==token.lower())
        return (int(row[6])+int(row[8])/2)/scale,(int(row[7])+int(row[9])/2)/scale
    def profile(at):
        events=re.findall(r'DOM_WORK\|finished\|bytes=(\d+)\|parse_ms=(\d+)\|commit_ms=(\d+)\|cleanup_ms=(\d+)\|total_ms=(\d+)\|limit=(\d+)\|nodes=(\d+)',p.output()[at:])
        assert events,'missing native operation profile'
        return dict(zip(('bytes','parse_ms','commit_ms','cleanup_ms','total_ms','limit','nodes'),map(int,events[-1])))
    def screen(name,words):
        end=time.monotonic()+20
        while time.monotonic()<end:
            raw,_=p.capture();p.save(raw,name)
            text=subprocess.check_output(['tesseract',str(evidence/(name+'.ppm')),'stdout','--psm','11'],text=True,stderr=subprocess.DEVNULL)
            if all(w.lower() in text.lower() for w in words):return
            time.sleep(.2)
        raise RuntimeError('DOM scanout mismatch: '+text)
    cases=[('Small',500),('Medium',2000),('Large',11000),('Nodes',3000),('Deep',540)]
    if faults:cases.append(('Deadline',500))
    for button,n in cases:
        p.click(*word('Restore'));screen('dom-restored',('RESTORED FRAME',))
        at=len(p.output())
        if button=='Deadline':p.key('f10');p.click(*word('Small'))
        else:p.click(*word(button))
        if button in ('Small','Medium'):
            p.wait(r'JS_CONSOLE\|DOM WORK END '+str(n)+r'\b',at)
            p.wait(r'JS_CONSOLE\|DOM CHILDREN '+str(n)+r'\b',at)
            stats=profile(at);assert stats['limit']==0
            p.wait(r'REFLOW\|done\|',at)
        else:
            p.wait(r'JS_CONSOLE\|DOM OLD INTACT true',at)
            stats=profile(at);assert stats['limit']=={'Large':5,'Nodes':1,'Deep':2,'Deadline':3}[button],stats
            assert stats['commit_ms']==0,'rejected fragment touched the live DOM'
            screen('dom-rejected-'+button.lower(),('RESTORED FRAME',))
        assert stats['total_ms']<1000,'native fragment work monopolised the UI for over one second'
        checks.append({'case':button,'requested_siblings_or_depth':n,**stats})
    # This creates well over the fixed 64 MiB heap's capacity in cumulative
    # temporary nodes. The rejected fragments must actually be reclaimed.
    repeated=[]
    for _ in range(96):
        at=len(p.output());p.click(*word('Nodes'))
        p.wait(r'JS_CONSOLE\|DOM OLD INTACT true',at)
        stats=profile(at);assert stats['limit'] in (1,3) and stats['commit_ms']==0
        assert stats['total_ms']<1000
        repeated.append(stats['total_ms'])
    checks.append({'repeated_rejections':len(repeated),'guest_native_max_ms':max(repeated),'old_DOM_retained':True})
    # The same instance remains interactive after all failures.
    p.click(*word('Restore'));screen('dom-after-rejection',('RESTORED FRAME',))
    p.click(*word('KEEP'));p.key('end')
    for c in 'typed':p.key(c)
    screen('dom-typing-after-rejection',('KEEPtyped','RESTORED FRAME'))
    p.click(45,30);screen('dom-home-after-rejection',('Explore the web',))
    (evidence/'dom-work-result.json').write_text(json.dumps({'checks':checks,'typing_and_navigation_after_rejection':True,'fault_controls_used':faults,'scale':scale,'test_sha256':TEST_SHA256,'saved_data_used':False,'storage':False},indent=2)+'\n')
    print('PASS native fragment work timings and subsequent interaction',flush=True)
