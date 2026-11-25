"""Fault-injected reflow and real guest TLS loading/cancellation regressions.

Only the explicitly marked disposable test image trusts this ephemeral CA, and
only for the QEMU host-loopback IP. The guest still verifies chain, name and time.
No page data is injected into the browser or supplied through a fetch proxy.
"""
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import json
from pathlib import Path
import re
import ssl
import subprocess
import threading
import time
from netsurf_native_image_input import Probe

TEST_SHA256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


class Fixture:
    def __init__(self,evidence):
        self.counts=Counter();self.main=threading.Event();self.image=threading.Event()
        self.main_requested=threading.Event();self.image_requested=threading.Event()
        fixture=self
        class Handler(BaseHTTPRequestHandler):
            protocol_version='HTTP/1.1'
            def log_message(self,*args):pass
            def do_GET(self):
                path=self.path.split('?',1)[0];fixture.counts[path]+=1
                code=200;mime='text/html';extra={}
                body='<h1>PAGE OK</h1><p>Hit '+str(fixture.counts[path])+'</p><img src="/missing.png">'
                if path=='/missing':code=404;body='<h1>MAIN MISSING</h1>'
                elif path=='/missing.png':code=404;mime='image/png';body=b'not a png'
                elif path=='/redirect':code=302;extra['Location']='/page';body='Redirect'
                elif path=='/slow':
                    fixture.main_requested.set();fixture.main.wait(60);body='<h1>SLOW FINISHED</h1>'
                elif path=='/slow-image.png':
                    fixture.image_requested.set();body=(evidence/'alpha.png').read_bytes()
                    # Keep real TLS traffic advancing while withholding a
                    # complete image. Do not weaken the guest's idle timeout.
                    try:
                        self.send_response(200);self.send_header('Content-Type','image/png')
                        self.send_header('Content-Length',str(len(body)))
                        self.send_header('Cache-Control','no-store');self.send_header('Connection','close');self.end_headers()
                        sent=0;deadline=time.monotonic()+60
                        while not fixture.image.wait(1) and time.monotonic()<deadline and sent<len(body)-1:
                            self.wfile.write(body[sent:sent+1]);self.wfile.flush();sent+=1
                        self.wfile.write(body[sent:])
                    except (BrokenPipeError,ConnectionResetError,ssl.SSLError):pass
                    self.close_connection=True;return
                elif path=='/with-slow-image':
                    body='''<h1>Pending image lab</h1><button id="change">Change caption</button>
                    <p id="caption">BEFORE IMAGE</p><img src="/slow-image.png" width="96" height="64">
                    <script>setTimeout(function(){document.getElementById('caption').textContent='AFTER TIMER';},100);
                    var n=0;document.getElementById('change').addEventListener('click',function(){
                    n++;document.getElementById('caption').textContent='IMAGE UPDATED '+n;});</script>'''
                elif path=='/page':pass
                else:code=404;body='Not found'
                if isinstance(body,str):body=('<!doctype html><html><head><style>body{margin:24px;font:24px sans-serif}button{font:24px sans-serif}</style></head><body>'+body+'</body></html>').encode()
                try:
                    self.send_response(code)
                    self.send_header('Content-Type',mime);self.send_header('Content-Length',str(len(body)))
                    self.send_header('Cache-Control','no-store');self.send_header('Connection','close')
                    for key,value in extra.items():self.send_header(key,value)
                    self.end_headers();self.wfile.write(body)
                except (BrokenPipeError,ConnectionResetError,ssl.SSLError):pass
                self.close_connection=True
        self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls=evidence.parent/'test-fixture-tls'
        context.load_cert_chain(str(tls/'server.pem'),str(tls/'server.key'))
        self.server.socket=context.wrap_socket(self.server.socket,server_side=True)
        self.url='https://10.0.2.2:'+str(self.server.server_port)
        self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start()
    def close(self):
        self.main.set();self.image.set();self.server.shutdown();self.server.server_close();self.thread.join(5)


def exercise(monitor,trace,temporary,evidence,scale,group="all"):
    p=Probe(monitor,trace,temporary,evidence,scale);checks=[]
    compact=lambda s:''.join(s.lower().split())
    def screen(name,words,absent=()):
        end=time.monotonic()+25;text=''
        while time.monotonic()<end:
            time.sleep(.2);p.output();raw,_=p.capture();path=evidence/(name+'.ppm');path.write_bytes(raw)
            text=subprocess.check_output(['tesseract',str(path),'stdout','--psm','11'],text=True,stderr=subprocess.DEVNULL)
            bar=evidence/'recovery-status.png'
            subprocess.run(['magick',str(path),'-crop',f'{310*scale}x{60*scale}+{970*scale}+0',str(bar)],check=True)
            text+='\n'+subprocess.check_output(['tesseract',str(bar),'stdout','--psm','7'],text=True,stderr=subprocess.DEVNULL)
            if all(compact(w) in compact(text) for w in words) and all(compact(w) not in compact(text) for w in absent):
                p.save(raw,name);return text
        raise RuntimeError('recovery screen mismatch '+name+': '+text)
    def word(name,token):
        tsv=subprocess.check_output(['tesseract',str(evidence/(name+'.ppm')),'stdout','--psm','11','tsv'],text=True,stderr=subprocess.DEVNULL)
        rows=[s.split('\t') for s in tsv.splitlines()[1:]]
        row=next(r for r in rows if len(r)>=12 and compact(r[11])==compact(token))
        return (int(row[6])+int(row[8])/2)/scale,(int(row[7])+int(row[9])/2)/scale
    def event(pattern,at):return p.wait(pattern,at)

    if group!='network':
        # Fail different real allocator calls, not a branch that skips construction.
        for number in (1,2,8,24,48,96):
            p.navigate('resource:reflow.html')
            # Reload reconstructs a fresh document, including when it was cached.
            p.click(835,30);screen('fault-initial',('status zero','dynamic layout lab'))
            time.sleep(.5);at=len(p.output());p.key('f8')
            event(r'FAULT\|armed='+str(number)+r'\b',at)
            p.click(*word('fault-initial','Change'))
            event(r'FAULT\|allocation='+str(number)+r'\|calls=\d+\|failed=1',at)
            event(r'REFLOW\|rollback\|',at)
            screen('fault-'+str(number),('status zero','layout update failed'),('inserted 1',))
            at=len(p.output());p.click(*word('fault-initial','Change'))
            event(r'REFLOW\|done\|',at)
            screen('fault-recovered-'+str(number),('status 2','inserted 2'),('layout update failed',))
            checks.append({'failed_allocation':number,'old_frame_preserved':True,'later_mutation_recovers':True})

        p.click(835,30);screen('layout-fault-initial',('status zero',));time.sleep(.3)
        at=len(p.output());p.key('f9');p.click(*word('layout-fault-initial','Change'))
        event(r'FAULT\|allocation=4294967295\|calls=\d+\|failed=1',at)
        event(r'REFLOW\|rollback\|',at)
        screen('layout-allocation-failed',('status zero','layout update failed'))
        p.click(835,30);screen('layout-allocation-recovered',('status zero',),('layout update failed',))
        p.click(*word('layout-fault-initial','Change'))
        screen('layout-after-reload',('status 1','inserted 1'))
        checks.append({'allocation_failure_during_final_layout_rolls_back':True,'Reload_recovers_failed_layout':True})

        p.navigate('resource:recovery.html');screen('bounds-initial',('recovery bounds lab','safe frame'))
        for button in ('Deep','Wide'):
            at=len(p.output());p.click(*word('bounds-initial',button))
            event(r'DOM_WORK\|finished\|[^\n]*\|limit=(2|5)\|',at)
            screen('bounds-'+button.lower(),('layout update failed',))
            # The old retained render boxes remain usable even though the DOM has
            # become ineligible; restoring a small DOM permits the next transaction.
            p.click(*word('bounds-initial','Restore'))
            screen('bounds-recovered-'+button.lower(),('restored frame',),('layout update failed',))
        checks.append({'fragment_depth_and_size_bounds_preserve_old_DOM_and_layout_and_recover':True})

    if group!='reflow':
        fixture=Fixture(evidence)
        try:
            at=p.navigate(fixture.url+'/page');event(r'HTTPS\|id=\d+\|status=404\|',at)
            screen('status-subresource-404',('page ok','hit 1','HTTPS 200'),('HTTPS 404',))
            p.click(835,30);screen('reload-main',('page ok','hit 2','HTTPS 200'))
            at=p.navigate(fixture.url+'/missing');event(r'PAGE\|Ready\|http=404',at)
            screen('status-main-404',('main missing','HTTPS 404'))
            p.click(145,30);screen('status-back',('page ok','HTTPS 200'))
            p.navigate(fixture.url+'/redirect');screen('status-redirect',('page ok','HTTPS 200'))
            checks.append({'main_http_code_ignores_subresource_404':True,'reload_refetches':True,'main_404_and_back_and_redirect':True})

            at=p.navigate(fixture.url+'/slow')
            assert fixture.main_requested.wait(15),'delayed main request never reached TLS server'
            screen('loading-delayed-main',('loading page',))
            p.click(920,30);event(r'ACTION\|stop',at);screen('stopped-main',('Stopped',),('slow finished',))
            fixture.main.set();time.sleep(1)
            screen('stopped-late-main',('Stopped',),('slow finished',))
            p.key('f5');screen('reload-stopped-target',('slow finished','HTTPS 200'))
            assert fixture.counts['/slow']==2,dict(fixture.counts)
            checks.append({'loading_feedback':True,'stop_ignores_late_main_response':True,'F5_retries_stopped_target':True})

            at=p.navigate(fixture.url+'/with-slow-image')
            assert fixture.image_requested.wait(15),'delayed image request never reached TLS server'
            screen('reflow-pending-image',('after timer','HTTPS 200 loading'))
            assert not fixture.image.is_set()
            p.key('f8');event(r'FAULT\|armed=1\b',at)
            p.click(*word('reflow-pending-image','Change'))
            event(r'REFLOW\|rollback\|',at)
            screen('pending-image-rollback',('after timer','layout update failed'))
            at=len(p.output());p.click(*word('reflow-pending-image','Change'))
            event(r'REFLOW\|done\|',at)
            screen('pending-image-recovered',('image updated 2','HTTPS 200 loading'))
            assert fixture.counts['/slow-image.png']==1,'reflow restarted the pending image download'
            p.key('esc');screen('stopped-image',('Stopped','image updated 2'))
            fixture.image.set();time.sleep(1)
            screen('stopped-late-image',('Stopped','image updated 2'))
            p.click(835,30);screen('reload-stopped-image',('after timer','HTTPS 200'),('loading',))
            checks.append({'reflow_during_pending_image':True,'rollback_with_active_image':True,'Stop_cancels_subresource_and_reflow':True,'reload_after_Stop_recovers':True})

            fixture.image.clear();fixture.main.clear();fixture.image_requested.clear();fixture.main_requested.clear()
            p.click(835,30)
            assert fixture.image_requested.wait(15),'second delayed image never requested'
            p.navigate(fixture.url+'/slow');fixture.image.set()
            assert fixture.main_requested.wait(15),'new navigation never requested'
            screen('loading-after-old-resource-finishes',('loading page',),('slow finished',))
            p.click(920,30);fixture.main.set();time.sleep(.5)
            screen('old-resource-navigation-stopped',('Stopped',),('slow finished',))
            checks.append({'old_page_resource_completion_cannot_replace_new_loading_status':True})

            at=p.navigate('https://wrong.host.badssl.com/');event(r'PAGE\|Failed\|',at)
            screen('status-tls-failure',('Failed',))
            at=len(p.output());p.click(835,30);event(r'PAGE\|Failed\|',at)
            p.navigate('resource:reflow.html');screen('status-recovered-local',('Local page','dynamic layout lab'))
            checks.append({'TLS_error_is_visible':True,'local_navigation_clears_stale_error':True})
        finally:fixture.close()
    (evidence/'recovery-result.json').write_text(json.dumps({'group':group,'checks':checks,'saved_data_used':False,'storage':False,'scale':scale,'test_sha256':TEST_SHA256},indent=2)+'\n')
    print('PASS allocation rollback, pending-image reflow, main page status, Reload/Stop and late TLS response recovery',flush=True)
