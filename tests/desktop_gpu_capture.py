"""Capture diagnostic GPU readback; never label it a host-display screenshot."""
import struct
import time


def capture(monitor, path, gpu=False):
    if not gpu:
        monitor.send('screendump', {'filename':str(path)})
        return
    raw_path=path.with_suffix('.readback')
    header_path=path.with_suffix('.header')
    end=time.monotonic()+15
    try:
        while time.monotonic()<end:
            monitor.send('pmemsave', {'val':0x5d000000,'size':28,'filename':str(header_path)})
            revision,w,h,stride,fmt,_,_=struct.unpack('<7I',header_path.read_bytes())
            if not revision or revision&1 or not (1600<=w<=3840 and 1200<=h<=2160 and stride==w*3 and fmt==1):
                time.sleep(.05);continue
            monitor.send('pmemsave', {'val':0x5d000000,'size':28+w*h*3,'filename':str(raw_path)})
            raw=raw_path.read_bytes()
            monitor.send('pmemsave', {'val':0x5d000000,'size':4,'filename':str(header_path)})
            after=struct.unpack('<I',header_path.read_bytes())[0]
            if revision==after and struct.unpack_from('<5I',raw)==(revision,w,h,stride,fmt):
                path.write_bytes(f'P6\n{w} {h}\n255\n'.encode()+raw[28:]);return
            time.sleep(.05)
        raise TimeoutError('no stable diagnostic GPU readback')
    finally:
        raw_path.unlink(missing_ok=True);header_path.unlink(missing_ok=True)
