"""Deterministic, packaged BMP fixture; no host image files are read."""
import struct


def scene():
    width, height = 440, 220
    stride = (width * 3 + 3) & ~3
    data = bytearray(stride * height)
    for y in range(height):
        for x in range(width):
            colour = (13 + y // 12, 32 + y // 6, 63 + y // 5)
            if (x - 325) ** 2 + (y - 55) ** 2 < 25 ** 2:
                colour = (116, 226, 220)
            if y > 100 + abs(x - 130) // 2:
                colour = (25, 84, 111)
            if y > 130 + abs(x - 310) // 3:
                colour = (21, 125, 135)
            if y > 190:
                colour = (18, 70 + (y - 190), 93)
            offset = (height - 1 - y) * stride + x * 3
            data[offset:offset+3] = bytes(reversed(colour))
    header = struct.pack('<2sIHHI', b'BM', 54+len(data), 0, 0, 54)
    header += struct.pack('<IiiHHIIiiII', 40, width, height, 1, 24, 0, len(data), 3780, 3780, 0, 0)
    return header + data


def decoder_fixtures(output):
    """Synthetic decoder coverage; no personal/remote image is packaged."""
    import subprocess
    import zlib
    width, height = 96, 64
    rgba = bytearray()
    for y in range(height):
        for x in range(width):
            rgb = ((220, 30, 40), (20, 180, 60), (30, 60, 220), (240, 200, 20))[(y >= 32)*2+(x >= 48)]
            alpha = 0 if x < 16 and y < 16 else 128 if x >= 80 and y >= 48 else 255
            rgba.extend((*rgb, alpha))
    def chunk(kind, data):
        return struct.pack('>I', len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
    signature=b'\x89PNG\r\n\x1a\n'
    raw=b''.join(b'\0'+rgba[y*width*4:(y+1)*width*4] for y in range(height))
    header=struct.pack('>IIBBBBB',width,height,8,6,0,0,0)
    png=signature+chunk(b'IHDR',header)+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b'')
    result={'alpha.png':png,'corrupt.png':signature+b'\0'*25,'corrupt.jpg':b'\xff\xd8\xff\xe0'+b'not a JPEG'*4}
    alpha=output/'alpha.png';alpha.write_bytes(png)
    for name,args in [
        ('interlaced.png',['-alpha','off','-interlace','PNG']),
        ('baseline.jpg',['-background','white','-alpha','remove','-sampling-factor','1x1','-quality','92','-interlace','None']),
        ('progressive.jpg',['-background','white','-alpha','remove','-sampling-factor','1x1','-quality','92','-interlace','Plane']),
        ('cmyk.jpg',['-background','white','-alpha','remove','-colorspace','CMYK','-quality','92']),
    ]:
        target=output/name
        subprocess.run(['magick',str(alpha),*args,'-strip',str(target)],check=True,timeout=20)
        result[name]=target.read_bytes()
    # Valid, CRC-correct IHDR above the decoder's pixel budget; no allocation is needed.
    result['oversize.png']=signature+chunk(b'IHDR',struct.pack('>IIBBBBB',3000,3000,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b'\0'))+chunk(b'IEND',b'')
    # Keep a real JPEG header, but make its SOF dimensions exceed the same bound.
    jpeg=bytearray(result['baseline.jpg']);pos=2
    while pos<len(jpeg)-10:
        if jpeg[pos]!=255:raise ValueError('unexpected JPEG marker')
        marker=jpeg[pos+1];length=int.from_bytes(jpeg[pos+2:pos+4],'big')
        if marker in (0xc0,0xc1,0xc2):
            jpeg[pos+5:pos+9]=struct.pack('>HH',3000,3000);break
        pos+=2+length
    else:raise ValueError('JPEG SOF absent')
    result['oversize.jpg']=bytes(jpeg)
    return result
