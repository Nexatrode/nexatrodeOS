"""Actual guest JavaScript/DOM/timer input, timeout recovery and live-site tests."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time
from netsurf_native_image_input import Probe
TEST_SHA256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


def exercise(monitor,trace,temporary,evidence,scale):
    p=Probe(monitor,trace,temporary,evidence,scale);checks=[]
    def screen(name,words,timeout=30):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            time.sleep(.25);p.output();raw,pixels=p.capture();path=evidence/(name+'.ppm');path.write_bytes(raw)
            ocr=subprocess.check_output(['tesseract',str(path),'stdout','--psm','11'],text=True,stderr=subprocess.DEVNULL)
            compact=lambda x:''.join(x.lower().split())
            if all(compact(w) in compact(ocr) for w in words):
                p.save(raw,name);(evidence/(name+'.ocr.txt')).write_text(ocr);return ocr
        raise RuntimeError('JavaScript screen mismatch '+name+': '+ocr)
    p.navigate('resource:script.html')
    screen('js-local',('inline script worked','external script worked','timer callback worked','real UTC clock worked','count zero','paragraph DOM updated'))
    p.wait('NETSURF\\|JS_CONSOLE\\|NATIVE TIMER WORKED')
    p.wait('NETSURF\\|JS_CONSOLE\\|NATIVE PARAGRAPH PARAGRAPH DOM UPDATED')
    date=re.search(r'NETSURF\|JS_CONSOLE\|NATIVE DATE (\d+)',p.output())
    assert date and abs(time.time()*1000-int(date[1])) < 15000, 'guest JS clock must match current UTC'
    checks.append({'local':['inline JS','external JS','DOM text mutation and repaint','textarea repaint','setTimeout','current Date.now']})
    # Find the real button through OCR coordinates, then send actual pointer input.
    data=subprocess.check_output(['tesseract',str(evidence/'js-local.ppm'),'stdout','--psm','11','tsv'],text=True,stderr=subprocess.DEVNULL)
    rows=[line.split('\t') for line in data.splitlines()[1:]]
    click=next(row for row in rows if len(row)>=12 and row[11].strip().lower()=='click')
    x=(int(click[6])+int(click[8])/2)/scale;y=(int(click[7])+int(click[9])/2)/scale
    p.click(x,y);screen('js-click-one',('count 1',))
    p.click(x,y);screen('js-click-two',('count 2',))
    checks.append({'pointer_events':'two real clicks dispatch listeners and update visible DOM'})
    at=p.navigate('resource:loop.html');start=time.monotonic()
    p.wait(r'NETSURF\|JS_ERROR\|[^\n]*(execution timeout|RangeError)',at)
    screen('js-loop-interrupted',('execution timeout test','end of loop document'))
    p.click(45,30);screen('js-loop-home',('explore the web',))
    checks.append({'loop_interrupted':True,'host_verification_ms':round((time.monotonic()-start)*1000),'scope':'includes QMP and OCR; not an instruction-budget benchmark'})
    for label,url,words in [
        ('netsurf','https://www.netsurf-browser.org/',('netsurf','project overview')),
        ('kernel','https://www.kernel.org/',('linux kernel archives','mainline')),
        ('quirksmode','https://www.quirksmode.org/js/detect.html',('browser detect',)),
    ]:
        at=p.navigate(url)
        host=url.split('/')[2]
        ocr=screen('js-site-'+label,words,45)
        # A previous regression suite may have populated NetSurf's in-memory
        # cache. Require real scanout plus a native HTTPS success from this
        # boot, rather than requiring an unnecessary second network download.
        fetched=r'HTTPS\|DONE\|[^\n]*host='+re.escape(host)+r'\|status=200'
        assert re.search(fetched,p.output()), 'no native HTTPS evidence for '+host
        errors=re.findall(r'NETSURF\|JS_ERROR\|([^\n]*)',p.output()[at:])
        checks.append({'site':url,'page_visible':True,'native_https_fetch_seen':True,
                       'reused_cached_content':not bool(re.search(fetched,p.output()[at:])),
                       'script_errors':errors})
        if label=='quirksmode':
            p.key('end');time.sleep(.5)
            tail=screen('js-site-quirksmode-script',('navigator.',),30)
            checks[-1]['script_generated_navigator_output_visible']=True
    p.click(45,30);screen('js-tested',('explore the web',))
    report={'checks':checks,'javascript':True,'engine':'Duktape 2.7.0 + upstream NetSurf DOM bindings','script_budget_ms':250,'scale':scale,'saved_data_used':False,'storage':False,'test_sha256':TEST_SHA256}
    (evidence/'javascript-result.json').write_text(json.dumps(report,indent=2)+'\n')
    print('PASS guest JavaScript, DOM events, timers, RTC Date, loop recovery and real sites',flush=True)
