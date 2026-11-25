"""Real guest DOM/layout, image, focus and repeated-update regression checks."""
import hashlib
import json
from pathlib import Path
import re
import statistics
import subprocess
import time
from netsurf_native_image_input import Probe,rgba,locate

TEST_SHA256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


def exercise(monitor,trace,temporary,evidence,scale):
    p=Probe(monitor,trace,temporary,evidence,scale);checks=[]
    compact=lambda s:''.join(s.lower().split())
    def screen(name,words):
        end=time.monotonic()+30
        while time.monotonic()<end:
            time.sleep(.2);p.output();raw,pixels=p.capture();path=evidence/(name+'.ppm');path.write_bytes(raw)
            text=subprocess.check_output(['tesseract',str(path),'stdout','--psm','11'],text=True,stderr=subprocess.DEVNULL)
            if all(compact(w) in compact(text) for w in words):
                p.save(raw,name);return text,pixels
        raise RuntimeError('DOM reflow screen mismatch '+name+': '+text)
    def word(name,token):
        tsv=subprocess.check_output(['tesseract',str(evidence/(name+'.ppm')),'stdout','--psm','11','tsv'],text=True,stderr=subprocess.DEVNULL)
        rows=[s.split('\t') for s in tsv.splitlines()[1:]]
        r=next(r for r in rows if len(r)>=12 and compact(r[11])==compact(token))
        return (int(r[6])+int(r[8])/2)/scale,(int(r[7])+int(r[9])/2)/scale
    at=p.navigate('resource:reflow.html')
    screen('reflow-initial',('dynamic layout lab','status zero','remove me','afterflow'))
    button=word('reflow-initial','Change');initial_y=word('reflow-initial','AFTERFLOW')[1]
    at=len(p.output());p.click(*button)
    p.wait(r'NETSURF\|REFLOW\|done\|',at)
    text,pixels=screen('reflow-mutated',('status 1','inserted 1','afterflow','character 1','fragment 1'))
    assert compact('remove me') not in compact(text),'removed DOM text still painted'
    moved_y=word('reflow-mutated','AFTERFLOW')[1]
    assert moved_y>initial_y+60,(initial_y,moved_y)
    assert compact('hide me') not in compact(text),'display:none did not remove painted text'
    style_y=moved_y-32
    assert list(pixels[round(style_y*scale),400*scale])==[42,112,213],'updated stylesheet colour missing'
    # New image source must be fetched/decoded, not left on the old render box.
    end=time.monotonic()+20;match=None
    while time.monotonic()<end:
        time.sleep(.2);p.output();raw,pixels=p.capture()
        match=locate(pixels,rgba(evidence/'alpha.png'),scale,background=[255,255,255],dimensions=(48,32),tolerance=2)
        if match:break
    assert match,'DOM image source update did not reach actual scanout'
    p.save(raw,'reflow-image-updated')
    checks.append({'text_character_data_innerHTML_structure_class_inline_and_stylesheet_changes':True,'marker_y_before':initial_y,'marker_y_after':moved_y,'updated_png_pixels':match})
    # Reattach a retained removed node and repeat alternating layouts. This
    # exercises stale node/box pointers, image handles, forms and style caches.
    start=len(p.output())
    for i in range(2,23):
        at=len(p.output());p.click(*button)
        p.wait(r'NETSURF\|REFLOW\|done\|',at);time.sleep(.05)
        if i==3:
            _,pixels=screen('reflow-sheet-removed',('status 3','afterflow'))
            y=word('reflow-sheet-removed','AFTERFLOW')[1]-32
            assert list(pixels[round(y*scale),400*scale])==[52,120,92],'removed stylesheet still applied'
    screen('reflow-repeated',('status 22','inserted 22','remove me','hide me','character 22','fragment 22'))
    events=[tuple(map(int,m)) for m in re.findall(r'REFLOW\|done\|revision=(\d+)\|nodes=(\d+)\|ms=(\d+)\|height=(\d+)\|layout_bytes=(\d+)\|heap_live=(\d+)',p.output()[start:])]
    assert len(events)>=21,'updates were dropped'
    assert max(e[4] for e in events)<200000,'layout tree accumulated old generations'
    assert events[-1][5]-events[0][5]<512*1024,'live heap grows excessively across repeated small updates'
    checks.append({'repeated_updates':21,'rebuilds':len(events),'guest_rebuild_median_ms':statistics.median(e[2] for e in events),'guest_rebuild_max_ms':max(e[2] for e in events),'live_heap_delta_bytes':events[-1][5]-events[0][5],'scope':'guest rebuild work; excludes host display/input latency'})
    timer=word('reflow-repeated','Start');entry=word('reflow-repeated','KEEP')
    at=len(p.output());p.click(*timer);p.click(*entry);p.key('end')
    for c in 'before':p.key(c)
    p.wait(r'JS_CONSOLE\|REFLOW TYPING TIMER',at)
    p.wait(r'REFLOW\|done\|',at);time.sleep(.25)
    for c in 'after':p.key(c)
    p.click(1100,100) # Remove the caret before OCR without editing the value.
    screen('reflow-typing',('timer reflowed','KEEPbeforeafter'))
    checks.append({'typed_value_and_keyboard_focus_survive_reflow':True})
    rapid=word('reflow-typing','Rapid');at=len(p.output());p.click(*rapid)
    p.wait(r'JS_CONSOLE\|REFLOW RAPID END',at)
    during=p.output()[at:].split('JS_CONSOLE|REFLOW RAPID END')[0]
    frames=during.count('NETSURF|REFLOW|done|')
    assert frames>=2,'continuous mutations postponed every layout until the stream stopped'
    screen('reflow-continuous',('rapid complete','KEEPbeforeafter'))
    checks.append({'continuous_mutation_updates':60,'rebuilds_before_stream_ended':frames})
    grow=word('reflow-typing','Grow');at=len(p.output());p.click(*grow)
    p.wait(r'REFLOW\|done\|',at);p.key('end')
    screen('reflow-grown-bottom',('bottom growth',))
    p.wait(r'JS_CONSOLE\|REFLOW SHRUNK',at)
    screen('reflow-shrunk-top',('dynamic layout lab','KEEPbeforeafter'))
    checks.append({'page_extent_growth_scrolling_and_shrink_clamping':True})
    assert p.output()[start:].count('JS_CONSOLE|REFLOW SCRIPT START')==0,'reflow reran document scripts'
    # Leave while a mutation timer/rebuild may still be scheduled, then revisit
    # and mutate again in this boot. No old callback may outlive its document.
    p.click(*timer);at=len(p.output())
    events=[]
    for x,y in (button,(45,30)):
        events.extend([{'type':'abs','data':{'axis':'x','value':round(x*32767/1279)}},
                       {'type':'abs','data':{'axis':'y','value':round(y*32767/799)}},
                       {'type':'btn','data':{'button':'left','down':True}},
                       {'type':'btn','data':{'button':'left','down':False}}])
    monitor.send('input-send-event',{'events':events})
    screen('reflow-close-pending',('explore the web',));time.sleep(2)
    assert 'JS_CONSOLE|REFLOW BATCH' in p.output()[at:],'queued mutation click was not delivered'
    assert 'NETSURF|REFLOW|done|' not in p.output()[at:],'closed document ran queued layout work'
    p.navigate('resource:reflow.html');screen('reflow-reopened',('dynamic layout lab','afterflow'))
    at=len(p.output());p.click(*button);p.wait(r'NETSURF\|REFLOW\|done\|',at)
    screen('reflow-after-reopen',('inserted',))
    p.click(45,30);screen('reflow-tested',('explore the web',))
    checks.append({'navigation_with_pending_work_and_reopen':True})
    assert not re.search(r'NETSURF\|REFLOW\|(rollback|unsupported)',p.output()[start:]),'unexpected rebuild fallback'
    result={'checks':checks,'scale':scale,'saved_data_used':False,'storage':False,'test_sha256':TEST_SHA256}
    (evidence/'reflow-result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS native DOM text/structure/CSS/image reflow, repeated updates, typing focus and navigation',flush=True)
