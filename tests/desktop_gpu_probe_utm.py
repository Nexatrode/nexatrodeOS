#!/usr/bin/env python3
"""Run and remove only diskless disposable GPU capability fixtures."""
import argparse, hashlib, json, os, plistlib, re, shutil, subprocess, time, uuid
from pathlib import Path
from utm_vault_key_storage import QmpPipe
ROOT=Path(__file__).resolve().parents[1]
CTL='/Applications/UTM.app/Contents/MacOS/utmctl'
def ctl(*v):return subprocess.run([CTL,*v],capture_output=True,text=True,timeout=30)
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--guest',type=Path,required=True);p.add_argument('--template',type=Path,required=True);p.add_argument('--device',choices=['virtio-gpu-device','virtio-gpu-gl-device'],required=True);a=p.parse_args()
 guest=a.guest.resolve();assert guest.is_relative_to(ROOT/'build/desktop-gpu')
 m=json.loads((guest/'build.json').read_text());assert not m['disk'] and not m['network']
 sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
 assert sha(guest/'loader.img')==m['loader_sha256']
 for p,h in m['sources'].items():assert sha(ROOT/p)==h,p
 tag=str(uuid.uuid4()).upper();out=guest/a.device;out.mkdir(exist_ok=False)
 bundle=out/('GPU-Probe-'+tag[:8]+'.utm');data=bundle/'Data';data.mkdir(parents=True)
 cfg=plistlib.loads(a.template.read_bytes());cfg['Information'].update(UUID=tag,Name='NexatrodeOS GPU Probe — Disposable '+tag[:8])
 cfg['Drive']=[];cfg['Network']=[];cfg['Serial']=[];cfg['Sharing'].update(ClipboardSharing=False,DirectoryShareMode='None')
 cfg['Display'][0]['Hardware']=a.device;cfg['Display'][0]['DynamicResolution']=False
 shutil.copyfile(guest/'loader.img',data/'loader.img')
 trace=data/'trace.log';cfg['QEMU']['AdditionalArguments']=['-global virtio-mmio.force-legacy=false',f'-device loader,file={data}/loader.img,addr=0x70000000,cpu-num=0','-serial file:'+str(trace)]
 mp=data/'monitor'
 for suffix in ('.in','.out'):os.mkfifo(str(mp)+suffix,0o600)
 cfg['QEMU']['AdditionalArguments'].append('-qmp pipe:'+str(mp))
 monitor=QmpPipe(mp)
 cfg['QEMU']['DebugLog']=False
 (bundle/'config.plist').write_bytes(plistlib.dumps(cfg));os.chmod(bundle/'config.plist',0o600)
 result={'uuid':tag,'device':a.device,'disk':False,'network':False,'rendering_tested':False,'passed':False}
 registered=False
 try:
  subprocess.run(['open','-g','-a','UTM',str(bundle)],check=True)
  end=time.monotonic()+45
  while time.monotonic()<end:
   if tag in ctl('list').stdout:registered=True;break
   time.sleep(.25)
  if not registered:raise RuntimeError('probe registration timeout')
  r=ctl('start',tag);result['start_returncode']=r.returncode;result['start_error']=r.stderr
  if r.returncode:raise RuntimeError(r.stderr)
  monitor.connect()
  end=time.monotonic()+45
  while time.monotonic()<end:
   text=trace.read_text(errors='replace') if trace.exists() else ''
   if any(s in text for s in ('GPU_PROBE|FAIL|','MON|ERROR','VMFault','Data Abort')):raise RuntimeError(text[-3000:])
   if 'GPU_PROBE|DONE|' in text:
    result['passed']=True;result['textures_verified']='GPU_PROBE|TEXTURES|PASS' in text;result['features_low']=int(re.search(r'GPU_PROBE\|features_low=(\d+)',text)[1]);result['capset_ids']=[int(n) for n in re.findall(r'GPU_PROBE\|capset_id=(\d+)',text)];result['rendering_tested']='GPU_PROBE|RENDER_READBACK|PASS' in text;result['virgl_context']='GPU_PROBE|VIRGL_CONTEXT|PASS' in text;break
   time.sleep(.1)
  if not result['passed']:raise RuntimeError('probe completion timeout')
  if result.get('rendering_tested'):
   tree=monitor.send('human-monitor-command',{'command-line':'info qtree'})
   (out/'qtree.json').write_text(json.dumps(tree,indent=2)+'\n')
   objects=monitor.send('qom-list',{'path':'/machine/peripheral'})
   (out/'qom.json').write_text(json.dumps(objects,indent=2)+'\n')
   devices=[o['name'] for o in objects['return'] if 'virtio-gpu' in o.get('type','') or 'virtio-ramfb' in o.get('type','')]
   screen={'device':devices[0]} if len(devices)==1 else {}
   result['capture_device']=screen.get('device','default')
   end=time.monotonic()+8
   while time.monotonic()<end:
    monitor.send('screendump',{'filename':str(data/'scanout.ppm'),**screen})
    raw=(data/'scanout.ppm').read_bytes();h=re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s',raw)
    if h:
     w,height=map(int,h.groups());rgb=raw[h.end():]
     if [w,height]==m['resolution'] and len(rgb)==w*height*3:
      def correct(flip):
       for y in range(64):
        for x in range(64):
         tx=ty=-1
         for left,top in ((30,32),(40,8),(24,24)):
          if left<=x<left+16 and top<=y<top+16:tx=x-left;ty=y-top
         blue=(((tx//2)^(ty//2))&1)*255 if tx>=0 else 0
         expected=bytes((0,255-blue,blue)) if tx>=0 else bytes((255,0,0))
         row=height-1-y if flip else y;at=(row*w+x)*3
         if rgb[at:at+3]!=expected:return False
       return True
      orientation=next((flip for flip in (False,True) if correct(flip)),None)
      if orientation is not None:
       result['scanout_origin']='bottom-left' if orientation else 'top-left';break
    time.sleep(.2)
   else:raise RuntimeError('GPU readback passed but scanout pixels did not match')
   shutil.copyfile(data/'scanout.ppm',out/'scanout.ppm')
   subprocess.run(['magick',str(out/'scanout.ppm'),str(out/'scanout.png')],check=True,timeout=20)
   result['scanout_captured']=True
 except Exception as e:result['error']=str(e);result['passed']=False
 finally:
  if (data/'scanout.ppm').exists():shutil.copyfile(data/'scanout.ppm',out/'scanout.ppm')
  monitor.close()
  if registered:
   try:
    ctl('stop',tag)
    if trace.exists():shutil.copyfile(trace,out/'serial.log')
    r=ctl('delete',tag);result['cleanup_returncode']=r.returncode
    if r.returncode:result['passed']=False
   except Exception as e:result['cleanup_error']=str(e);result['passed']=False
  (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps(result,indent=2))
 if not result['passed']:raise SystemExit(1)
if __name__=='__main__':main()
