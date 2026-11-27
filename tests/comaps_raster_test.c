/* Opaque span optimization must preserve the previous per-pixel result. */
#include <assert.h>
#include <stdio.h>
#include "../experiments/comaps-native/raster.h"
#define W 97
#define H 73
static uint8_t expected[32+W*H*3+32],actual[sizeof(expected)];
static uint32_t state=17;
static unsigned random32(void){state=state*1664525u+1013904223u;return state;}
int main(void) {
    for(unsigned n=0;n<10000;++n){
        memset(expected,0x69,sizeof(expected));memcpy(actual,expected,sizeof(expected));
        int x=(int)(random32()%250)-100,y=(int)(random32()%200)-80;
        int right=x+(int)(random32()%180),bottom=y+(int)(random32()%150);uint32_t c=random32();
        /* Independent old pixel-at-a-time bounds/opaque blend oracle. */
        for(int yy=y;yy<bottom;++yy)for(int xx=x;xx<right;++xx){
            if(xx<0||yy<0||xx>=W||yy>=H)continue;
            uint8_t *p=expected+32+((size_t)yy*W+xx)*3;
            for(unsigned i=0;i<3;++i)p[i]=(p[i]*(255-255)+((c>>(16-8*i))&255)*255+127)/255;
        }
        map_fill_rgb(actual+32,W,H,x,y,right,bottom,c);
        assert(!memcmp(expected,actual,sizeof(expected)));
    }
    puts("PASS 10000 clipped RGB rectangles match pixel oracle and guards");
}
