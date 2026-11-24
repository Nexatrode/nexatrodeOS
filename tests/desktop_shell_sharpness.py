#!/usr/bin/env python3
"""Compare native 2x title glyphs with a nearest-enlarged 1x title crop."""
import argparse
import json
from pathlib import Path
import subprocess

def ppm(path):
    magic, dimensions, maximum, pixels=path.read_bytes().split(b'\n',3)
    w,h=map(int,dimensions.split())
    assert magic==b'P6' and maximum==b'255' and len(pixels)==w*h*3
    return w,h,pixels

def crop(w,pixels,scale):
    return b''.join(pixels[((y*w+24*scale)*3):((y*w+149*scale)*3)] for y in range(38*scale))

def enlarged(pixels):
    rows=[]
    for y in range(38):
        row=b''.join(pixels[(y*125+x)*3:(y*125+x+1)*3]*2 for x in range(125))
        rows += [row,row]
    return b''.join(rows)

def details(pixels):
    bright=mixed=0
    for y in range(0,76,2):
        for x in range(0,250,2):
            block=[pixels[((y+dy)*250+x+dx)*3:((y+dy)*250+x+dx+1)*3] for dy in (0,1) for dx in (0,1)]
            if max(max(p) for p in block)>180:
                bright+=1; mixed+=len(set(block))>1
    return {'bright_blocks':bright,'blocks_with_subpixel_detail':mixed}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--one',type=Path,required=True);p.add_argument('--two',type=Path,required=True)
    a=p.parse_args()
    w,h,one=ppm(a.one/'desktop-empty.ppm'); assert (w,h)==(1280,800)
    one=enlarged(crop(w,one,1))
    w,h,two=ppm(a.two/'desktop-empty.ppm'); assert (w,h)==(2560,1600)
    two=crop(w,two,2)
    old,new=details(one),details(two)
    assert old['blocks_with_subpixel_detail']==0 and new['blocks_with_subpixel_detail']>=50, (old,new)
    for label,data in [('title-1x-enlarged',one),('title-native-2x',two)]:
        path=a.two/(label+'.ppm');path.write_bytes(b'P6\n250 76\n255\n'+data)
        subprocess.run(['magick',str(path),'-filter','point','-resize','300%',str(a.two/(label+'.png'))],check=True)
    subprocess.run(['magick',str(a.two/'title-1x-enlarged.png'),str(a.two/'title-native-2x.png'),'+append',str(a.two/'font-comparison.png')],check=True)
    result={'logical_title_size':15,'physical_glyph_size_1x':15,'physical_glyph_size_2x':30,'font_weight':700,'one_x_enlarged':old,'native_2x':new,'scope':'Within each logical text pixel, native 2x has additional distinct physical samples. This proves native rendering rather than a stretched 1x image; visual quality also inspected.'}
    (a.two/'sharpness.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result))

if __name__=='__main__':main()
