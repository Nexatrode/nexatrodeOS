"""Actual mouse/keyboard interactions with the offline disposable landscape VM."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time

def exercise(monitor, trace, temporary, evidence):
    checks = []
    build = json.loads((evidence / 'build.json').read_text())
    scale = build.get('scale', 1)
    width, height = 1280 * scale, 800 * scale
    def output():
        text = trace.read_text(errors='replace')
        # Per-vCPU debug output can interrupt an app marker mid-line. These
        # known noisy diagnostics carry no test assertions; remove them first.
        text = re.sub(r'GPU\|SLINT COPY rev=\d+ y=\d+ h=\d+\r?\n', '', text)
        text = re.sub(r'GPU_VIRTIO_DRIVER\|ERROR: Received interrupt but is neither a used buffer notification nor a gpu event, irq status: 0x0\r?\n', '', text)
        if any(s in text for s in ('RUNTIME|FAIL|', 'SURFACE|FAIL|', 'panicked at', 'DYNAMIC|UNEXPECTED_FAULT|', 'MON|ERROR')):
            raise RuntimeError('desktop guest failed: ' + text[-3000:])
        return text
    def wait(marker, count=1):
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline:
            if output().count(marker) >= count: return
            time.sleep(.05)
        raise RuntimeError('missing ' + marker + ': ' + output()[-3000:])
    def pointer(x, y, button=None):
        events = [{'type':'abs', 'data':{'axis':'x', 'value':round(x * 32767 / 1279)}}, {'type':'abs', 'data':{'axis':'y', 'value':round(y * 32767 / 799)}}]
        if button is not None: events.append({'type':'btn', 'data':{'button':'left', 'down':button}})
        monitor.send('input-send-event', {'events':events})
    def click(x,y): pointer(x,y,True); time.sleep(.08); pointer(x,y,False); time.sleep(.2)
    def keys(word):
        for c in word:
            monitor.send('input-send-event', {'events':[{'type':'key','data':{'down':down,'key':{'type':'qcode','data':c}}} for down in (True,False)]})
            time.sleep(.02)
    def screen(name, required=(), absent=(), number=None):
        deadline = time.monotonic() + 25
        while time.monotonic() < deadline:
            time.sleep(.3)
            monitor.send('screendump', {'filename':str(temporary / 'desktop.ppm')})
            data = (temporary / 'desktop.ppm').read_bytes()
            magic, dimensions, maximum, pixels = data.split(b'\n',3)
            if (magic,dimensions,maximum) != (b'P6',f'{width} {height}'.encode(),b'255') or len(pixels) != width*height*3:
                output()
                continue  # GPU mode setting may still be in progress.
            path = evidence / (name + '.ppm'); path.write_bytes(data)
            text = subprocess.run(['tesseract',str(path),'stdout','--psm','11'],capture_output=True,text=True,check=True,timeout=30).stdout.lower()
            output()
            number_ok = True
            if number is not None:
                crop = evidence / (name + '-result.png')
                subprocess.run(['magick',str(path),'-crop',f'{500*scale}x{70*scale}+{680*scale}+{157*scale}','+repage','-resize','200%','-trim','+repage','-bordercolor','#0c1e2c','-border','20',str(crop)],check=True,timeout=30)
                digits = subprocess.run(['tesseract',str(crop),'stdout','--psm','8','-c','tessedit_char_whitelist=0123456789'],capture_output=True,text=True,check=True,timeout=30).stdout.strip()
                number_ok = digits == number
            if number_ok and all(w.lower() in text for w in required) and all(w.lower() not in text for w in absent):
                subprocess.run(['magick',str(path),str(evidence / (name + '.png'))],check=True,timeout=30)
                return pixels
        raise RuntimeError(f'frame {name} mismatch: {text}')
    pixels = screen('desktop-empty', ('nexatrodeos','desktop','your space','notes','calculator'))
    if len(set(pixels[i:i+3] for i in range(0,len(pixels),3))) < 10000: raise RuntimeError('wallpaper was not presented')
    checks.append(f'{width}x{height} physical / 1280x800 logical, wallpaper, clock and dock presented')
    click(596,728); wait('DYNAMIC|READY|generation=1')
    click(140,265); keys('desktopnote'); wait('INPUT|TEXT|app=1|desktopnote')
    click(682,728)
    for x,y in [(995,373),(1125,373),(735,287),(995,545)]: click(x,y)
    wait('DESKTOP_CALC|DISPLAY|42')
    screen('desktop-apps', ('desktopnote','42','calculator','notes'))
    checks.append('dock launched dynamic Notes and isolated working Calculator; typed note and 6*7=42 visible')
    click(558,104); wait('DESKTOP|COMMAND|21')
    screen('desktop-minimized', ('42',), ('desktopnote',))
    before = output().count('INPUT|KEY|app=1|'); keys('ghost'); time.sleep(.2)
    if output().count('INPUT|KEY|app=1|') != before: raise RuntimeError('minimized app received unowned keyboard input')
    click(596,728)
    screen('desktop-restored', ('desktopnote','42'))
    checks.append('minimize hides Notes and releases focus; dock restores its retained note')
    pointer(250,103,True); time.sleep(.2); pointer(300,123); time.sleep(.3); pointer(300,123,False)
    wait('DESKTOP|COMMAND|41')
    screen('desktop-moved', ('desktopnote','42'))
    checks.append('title-bar drag moves a window using clamped manager geometry')
    # Notes moved +50,+20. Close the actual title-bar control and reopen via dock.
    click(648,124); wait('DYNAMIC|CLOSED|generation=1')
    screen('desktop-closed', ('42',), ('desktopnote',))
    click(596,728); wait('DYNAMIC|READY|generation=2')
    click(190,285); keys('freshnote'); wait('INPUT|TEXT|app=1|freshnote')
    screen('desktop-final', ('freshnote','42'), ('desktopnote',))
    wait('DYNAMIC_APP|RPC_DENIALS|generation=2')
    # Calculator close is a reset of its predeclared PD, not dynamic process destruction.
    click(1181,88)
    closed = screen('desktop-calculator-closed', ('freshnote',))
    for x,y in ((900,250),(1000,350),(1100,600)):
        offset = (y * scale * width + x * scale) * 3
        if closed[offset:offset+3] != pixels[offset:offset+3]: raise RuntimeError('closed Calculator left pixels behind')
    click(682,728)
    screen('desktop-calculator-reopened', ('freshnote','calculator'), number='0')
    click(735,545); click(735,459); click(1125,545); click(865,459); click(995,545)
    wait('DESKTOP_CALC|DISPLAY|3')
    screen('desktop-calculator-reset', ('freshnote','calculator'), number='3')
    # Produce a clean final workspace with the retained note and a useful result.
    click(735,545)
    for x,y in [(995,373),(1125,373),(735,287),(995,545)]: click(x,y)
    wait('DESKTOP_CALC|DISPLAY|42',2)
    screen('desktop-final', ('freshnote','42'), ('desktopnote',))
    checks.append('Calculator close resets arithmetic state; reopening and 1+2 work independently of Notes')
    checks.append('close destroys Notes; dock recreates generation 2 with fresh text and denied malformed/forged publications')
    from desktop_shell_perf import measure
    measure(monitor,temporary,evidence,scale)
    screen('desktop-tested', ('freshnote','42'), ('desktopnote',))
    (evidence/'shell-result.json').write_text(json.dumps({'checks':checks,'test_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'latency_measurement':'QMP scanout bounds; see responsiveness.json','saved_data_used':False,'network':'restricted','resolution':[width,height],'logical_resolution':[1280,800],'scale':scale,'observed':'actual guest scanout, OCR and serial diagnostics'},indent=2)+'\n')
    print('PASS landscape shell, dock, app input, arithmetic, focus, minimize, restore, drag and clean process recreation')
