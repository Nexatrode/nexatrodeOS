"""Live native DNS/TCP/TLS, real scanout and certificate-negative checks."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def exercise(monitor, trace, temporary, evidence, scale):
    results=[]
    def output():
        text=trace.read_text(errors='replace')
        if any(s in text for s in ('NETSURF|FAIL|','MON|ERROR','VMFault','microkit_notify: invalid channel','Data Abort','Prefetch Abort')):raise RuntimeError(text[-4000:])
        return text
    def click(x,y):
        for down in (True,False):
            monitor.send('input-send-event',{'events':[{'type':'abs','data':{'axis':'x','value':round(x*32767/1279)}},{'type':'abs','data':{'axis':'y','value':round(y*32767/799)}},{'type':'btn','data':{'button':'left','down':down}}]})
            time.sleep(.08)
    def key(code,shift=False):
        events=[]
        if shift:events.append({'type':'key','data':{'down':True,'key':{'type':'qcode','data':'shift'}}})
        events.extend({'type':'key','data':{'down':down,'key':{'type':'qcode','data':code}}} for down in (True,False))
        if shift:events.append({'type':'key','data':{'down':False,'key':{'type':'qcode','data':'shift'}}})
        monitor.send('input-send-event',{'events':events});time.sleep(.025)
    def navigate(url):
        click(320,30)
        for c in url:
            if c==':':key('semicolon',True)
            elif c=='/':key('slash')
            elif c=='.':key('dot')
            elif c=='-':key('minus')
            else:key(c)
        before=len(output());key('ret');return before
    def screen(name,required):
        deadline=time.monotonic()+25
        while time.monotonic()<deadline:
            time.sleep(.3);tmp=temporary/'https-check.ppm';monitor.send('screendump',{'filename':str(tmp)})
            ppm=evidence/(name+'.ppm');ppm.write_bytes(tmp.read_bytes())
            ocr=subprocess.run(['tesseract',str(ppm),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout
            compact=lambda s:''.join(s.lower().split())
            output()
            if all(compact(s) in compact(ocr) for s in required):
                subprocess.run(['magick',str(ppm),str(evidence/(name+'.png'))],check=True,timeout=30)
                (evidence/(name+'.ocr.txt')).write_text(ocr);return
        raise RuntimeError('screen mismatch '+name+': '+ocr)
    def wait(regex,after):
        deadline=time.monotonic()+60
        while time.monotonic()<deadline:
            text=output()[after:];m=re.search(regex,text)
            if m:return m
            time.sleep(.1)
        raise RuntimeError('missing '+regex+': '+output()[-4000:])
    screen('https-start',('explore the web','guest DNS'))
    for host,words,slug in [('www.netsurf-browser.org',('netsurf','project overview'),'netsurf'),('www.iana.org/domains/reserved',('iana','reserved domains'),'iana'),('www.kernel.org',('linux kernel archives','mainline','stable'),'kernel')]:
        before=navigate('https://'+host+'/')
        m=wait(r'HTTPS\|DONE\|id=(\d+)\|host='+re.escape(host.split('/')[0])+r'\|status=200\|bytes=(\d+)\|ms=(\d+)',before)
        screen('https-'+slug,words)
        results.append({'host':host,'status':200,'body_bytes':int(m[2]),'fetch_ms':int(m[3]),'pixels_verified':True})
    # Return home while a failed lookup is pending: the UI must remain usable.
    before=navigate('https://does-not-exist.nexatrode.invalid/')
    start=time.monotonic();click(45,30);screen('https-cancel-home',('explore the web',))
    results.append({'cancellation':'Home usable during unresolved request','host_input_to_verified_home_ms':round((time.monotonic()-start)*1000)})
    for host in ('expired.badssl.com','wrong.host.badssl.com'):
        all_attempts=len(output());transport_retries=0
        for attempt in range(3):
            before=navigate('https://'+host+'/')
            outcome=wait(r'HTTPS\|(ERROR|DONE)\|id=\d+\|host='+re.escape(host)+r'\|([^\n]*)',before)
            if outcome[1]=='ERROR' and outcome[2].startswith('code=9|TLS certificate rejected'):
                break
            # A reset before the certificate arrives is inconclusive. Retry it
            # a bounded number of times; only a real verifier rejection passes.
            if outcome[1]=='ERROR' and outcome[2].startswith('code=2|') and attempt<2:
                transport_retries+=1;time.sleep(.5);continue
            raise RuntimeError('expected certificate rejection: '+outcome[0])
        assert not re.search(r'HTTPS\|DONE\|[^\n]*host='+re.escape(host),output()[all_attempts:])
        screen('https-rejected-'+host.split('.')[0],('certificate rejected',))
        results.append({'host':host,'certificate_rejected':True,'body_delivered':False,'transport_retries':transport_retries})
    # A rejected server must not poison subsequent browsing in this same boot.
    before=navigate('https://www.netsurf-browser.org/about/')
    wait(r'HTTPS\|DONE\|id=\d+\|host=www\.netsurf-browser\.org\|status=200',before)
    screen('https-recovery',('about netsurf',))
    click(45,30);screen('https-tested',('explore the web',))
    result={'checks':results,'network':'guest native DNS + TCP + rustls; no host fetch relay','storage':False,'saved_data_used':False,'javascript':json.loads((evidence/'build.json').read_text())['javascript'],'scale':scale,'test_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    (evidence/'https-result.json').write_text(json.dumps(result,indent=2)+'\n')
    print('PASS native HTTPS websites, responsive cancellation, certificate rejection and recovery',flush=True)
