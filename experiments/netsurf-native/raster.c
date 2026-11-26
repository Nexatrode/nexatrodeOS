/* SPDX-License-Identifier: GPL-2.0-only
 * Small CPU plotter for the offline experiment. Only packaged fonts are loaded.
 */
#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"
#include "native.h"
#include "native_image_limits.h"
#include "netsurf/types.h"

static unsigned char *pixels;
static struct rect clipping = {0, 0, PIXEL_WIDTH, MAX_PIXEL_HEIGHT};
static stbtt_fontinfo font;
struct bitmap { int width, height; bool opaque; unsigned char *data; };
struct glyph { int cp, size, width, height, xoff, yoff; unsigned char *alpha; };
static struct glyph glyphs[512];
static unsigned next_glyph;

static int utf8(const char *s, size_t n, size_t *used) {
    unsigned char c = (unsigned char)s[0]; *used = 1;
    if (c < 128) return c;
    unsigned more = c >= 0xf0 ? 3 : c >= 0xe0 ? 2 : c >= 0xc0 ? 1 : 0;
    if (!more || n <= more) return 0xfffd;
    unsigned cp = c & ((1u << (6-more)) - 1);
    for (unsigned i = 1; i <= more; ++i) {
        unsigned char v = (unsigned char)s[i];
        if ((v & 0xc0) != 0x80) return 0xfffd;
        cp = cp * 64 + (v & 63);
    }
    if (cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) return 0xfffd;
    *used = more + 1; return cp;
}
static int font_pixels(const plot_font_style_t *style) {
    int size = (int)((int64_t)style->size * 96 / (72 * PLOT_STYLE_SCALE));
    return size < 6 ? 6 : size > 128 ? 128 : size;
}
static float advance(int cp, int size) {
    int width, bearing; stbtt_GetCodepointHMetrics(&font, cp, &width, &bearing);
    return width * stbtt_ScaleForMappingEmToPixels(&font, size);
}
static void blend(int x, int y, colour c, unsigned a) {
    if (!a || x < clipping.x0 || y < clipping.y0 || x >= clipping.x1 || y >= clipping.y1) return;
    unsigned char *p = pixels + ((size_t)y * PIXEL_WIDTH + x) * 3;
    for (unsigned i = 0; i < 3; ++i) p[i] = (p[i] * (255-a) + ((c >> (i*8)) & 255) * a + 127) / 255;
}
void native_fill(int x, int y, int w, int h, uint32_t c) {
    if (c == NS_TRANSPARENT || w <= 0 || h <= 0) return;
    int x0 = x*NATIVE_SCALE, y0 = y*NATIVE_SCALE;
    int x1 = (x+w)*NATIVE_SCALE, y1 = (y+h)*NATIVE_SCALE;
    if (x0 < clipping.x0) x0 = clipping.x0; if (y0 < clipping.y0) y0 = clipping.y0;
    if (x1 > clipping.x1) x1 = clipping.x1; if (y1 > clipping.y1) y1 = clipping.y1;
    for (int yy = y0; yy < y1; ++yy) for (int xx = x0; xx < x1; ++xx) {
        unsigned char *p = pixels + ((size_t)yy * PIXEL_WIDTH + xx)*3;
        p[0] = c; p[1] = c >> 8; p[2] = c >> 16;
    }
}
static void draw_text(int x, int y, int size, colour c, const char *s, size_t length, bool bold) {
    float pen = x * NATIVE_SCALE; size *= NATIVE_SCALE;
    for (size_t offset = 0; offset < length;) {
        size_t used; int cp = utf8(s+offset, length-offset, &used); offset += used;
        struct glyph *g = NULL;
        for (unsigned i = 0; i < 512; ++i) if (glyphs[i].cp == cp && glyphs[i].size == size) { g = &glyphs[i]; break; }
        if (!g) {
            g = &glyphs[next_glyph++ % 512]; free(g->alpha); memset(g, 0, sizeof(*g));
            g->cp = cp; g->size = size;
            float scale = stbtt_ScaleForMappingEmToPixels(&font, size);
            g->alpha = stbtt_GetCodepointBitmap(&font, scale, scale, cp, &g->width, &g->height, &g->xoff, &g->yoff);
        }
        int gx = (int)pen + g->xoff, gy = y*NATIVE_SCALE + g->yoff;
        if (g->alpha) for (int yy = 0; yy < g->height; ++yy) for (int xx = 0; xx < g->width; ++xx) {
            unsigned a = g->alpha[yy*g->width+xx]; blend(gx+xx, gy+yy, c, a);
            if (bold) blend(gx+xx+1, gy+yy, c, a/2);
        }
        pen += advance(cp, size);
    }
}
void native_text(int x, int y, int size, uint32_t colour, const char *text) { draw_text(x,y,size,colour,text,strlen(text),false); }
void native_raster_init(unsigned char *p) {
    pixels = p;
    if (!stbtt_InitFont(&font, native_font, stbtt_GetFontOffsetForIndex(native_font, 0))) native_fail("font", 1);
}
static nserror width(const plot_font_style_t *f, const char *s, size_t n, int *out) {
    float sum = 0; int size = font_pixels(f);
    for (size_t i = 0; i < n;) { size_t used; int cp = utf8(s+i,n-i,&used); i += used; sum += advance(cp,size); }
    *out = (int)(sum+0.5f); return NSERROR_OK;
}
static nserror position(const plot_font_style_t *f,const char *s,size_t n,int x,size_t *off,int *actual) {
    float sum = 0; *off = 0; *actual = 0;
    for (size_t i = 0; i < n;) {
        size_t used; int cp = utf8(s+i,n-i,&used); float next = sum + advance(cp,font_pixels(f));
        if (x < (sum+next)/2) break;
        i += used; sum = next; *off = i; *actual = (int)(sum+0.5f);
    }
    return NSERROR_OK;
}
static nserror split(const plot_font_style_t *f,const char *s,size_t n,int x,size_t *off,int *actual) {
    float sum = 0; size_t last = 0; int at_last = 0;
    for (size_t i = 0; i < n;) {
        size_t used; int cp = utf8(s+i,n-i,&used);
        if (cp == ' ') { last = i; at_last = (int)(sum+0.5f); if (sum > x) break; }
        if (sum > x && last) break;
        sum += advance(cp,font_pixels(f)); i += used;
        if (i == n) { last = n; at_last = (int)(sum+0.5f); }
    }
    *off = last ? last : n; *actual = last ? at_last : (int)(sum+0.5f); return NSERROR_OK;
}
struct gui_layout_table native_layout = {.width=width,.position=position,.split=split};
static void *bitmap_create(int w,int h,enum gui_bitmap_flags flags) {
    if (w <= 0 || h <= 0 || !native_image_size_ok(w,h)) return NULL;
    struct bitmap *b = calloc(1,sizeof(*b)); if (!b) return NULL;
    b->width=w; b->height=h; b->opaque=(flags & BITMAP_OPAQUE)!=0; b->data=calloc((size_t)w*h,4);
    if (!b->data) { free(b);return NULL; } return b;
}
static void bitmap_destroy(void *p) { struct bitmap *b=p; if (b) { free(b->data);free(b); } }
static void set_opaque(void *p,bool o) { ((struct bitmap*)p)->opaque=o; }
static bool get_opaque(void *p) { return ((struct bitmap*)p)->opaque; }
static unsigned char *get_buffer(void *p) { return ((struct bitmap*)p)->data; }
static size_t stride(void *p) { return ((struct bitmap*)p)->width*4; }
static int bw(void *p) { return ((struct bitmap*)p)->width; }
static int bh(void *p) { return ((struct bitmap*)p)->height; }
static void modified(void *p) { (void)p; }
static nserror render_bitmap(struct bitmap *b,struct hlcache_handle *c) { (void)b;(void)c;return NSERROR_NOT_IMPLEMENTED; }
struct gui_bitmap_table native_bitmaps = {.create=bitmap_create,.destroy=bitmap_destroy,.set_opaque=set_opaque,
    .get_opaque=get_opaque,.get_buffer=get_buffer,.get_rowstride=stride,.get_width=bw,.get_height=bh,.modified=modified,.render=render_bitmap};
static nserror clip(const struct redraw_context *ctx,const struct rect *r) {
    (void)ctx; clipping=(struct rect){r->x0*NATIVE_SCALE,r->y0*NATIVE_SCALE,r->x1*NATIVE_SCALE,r->y1*NATIVE_SCALE};
    if (clipping.x0<0) clipping.x0=0; if (clipping.y0<0) clipping.y0=0;
    if (clipping.x1>PIXEL_WIDTH) clipping.x1=PIXEL_WIDTH; if (clipping.y1>PIXEL_HEIGHT) clipping.y1=PIXEL_HEIGHT;
    return NSERROR_OK;
}
static nserror rectangle(const struct redraw_context *ctx,const plot_style_t *s,const struct rect *r) {
    (void)ctx;
    if (s->fill_type) native_fill(r->x0,r->y0,r->x1-r->x0,r->y1-r->y0,s->fill_colour);
    if (s->stroke_type) {
        int w=s->stroke_width/PLOT_STYLE_SCALE; if (w<1) w=1;
        native_fill(r->x0,r->y0,r->x1-r->x0,w,s->stroke_colour); native_fill(r->x0,r->y1-w,r->x1-r->x0,w,s->stroke_colour);
        native_fill(r->x0,r->y0,w,r->y1-r->y0,s->stroke_colour); native_fill(r->x1-w,r->y0,w,r->y1-r->y0,s->stroke_colour);
    }
    return NSERROR_OK;
}
static nserror line(const struct redraw_context *ctx,const plot_style_t *s,const struct rect *r) {
    (void)ctx; int x=r->x0,y=r->y0,dx=abs(r->x1-x),dy=-abs(r->y1-y);
    int sx=x<r->x1?1:-1,sy=y<r->y1?1:-1,error=dx+dy;
    if (dx>20000 || -dy>20000) return NSERROR_BAD_PARAMETER;
    int w=s->stroke_width/PLOT_STYLE_SCALE; if(w<1)w=1;
    for (;;) { native_fill(x,y,w,w,s->stroke_colour); if (x==r->x1 && y==r->y1)break;
        int twice=2*error;if(twice>=dy){error+=dy;x+=sx;}if(twice<=dx){error+=dx;y+=sy;} }
    return NSERROR_OK;
}
static nserror polygon(const struct redraw_context *ctx,const plot_style_t *s,const int *p,unsigned n) {
    (void)ctx; if(n<3 || n>128)return NSERROR_BAD_PARAMETER;
    for(int y=clipping.y0/NATIVE_SCALE;y<(clipping.y1+NATIVE_SCALE-1)/NATIVE_SCALE;++y){
        int crossings[128];unsigned count=0;
        for(unsigned i=0,j=n-1;i<n;j=i++)if((p[i*2+1]>y)!=(p[j*2+1]>y))
            crossings[count++]=p[i*2]+(int)((int64_t)(y-p[i*2+1])*(p[j*2]-p[i*2])/(p[j*2+1]-p[i*2+1]));
        for(unsigned i=0;i<count;++i)for(unsigned j=i+1;j<count;++j)if(crossings[j]<crossings[i]){int t=crossings[i];crossings[i]=crossings[j];crossings[j]=t;}
        for(unsigned i=0;i+1<count;i+=2)native_fill(crossings[i],y,crossings[i+1]-crossings[i],1,s->fill_colour);
    }return NSERROR_OK;
}
static nserror disc(const struct redraw_context *ctx,const plot_style_t *s,int x,int y,int radius) {
    (void)ctx;if(radius<0||radius>10000)return NSERROR_BAD_PARAMETER;
    for(int yy=-radius;yy<=radius;++yy){int half=(int)sqrt((double)radius*radius-(double)yy*yy);
        native_fill(x-half,y+yy,half*2+1,1,s->fill_type?s->fill_colour:s->stroke_colour); }return NSERROR_OK;
}
static nserror arc(const struct redraw_context *ctx,const plot_style_t *s,int x,int y,int radius,int first,int last) {
    for(int a=first;a<last;++a){double r=a*3.141592653589793/180;struct rect p={x+(int)(cos(r)*radius),y+(int)(sin(r)*radius),x+(int)(cos(r+.018)*radius),y+(int)(sin(r+.018)*radius)};line(ctx,s,&p);}return NSERROR_OK;
}
static nserror path(const struct redraw_context *ctx,const plot_style_t *s,const float *p,unsigned n,const float t[6]) {
    (void)ctx;(void)s;(void)p;(void)n;(void)t;return NSERROR_NOT_IMPLEMENTED;
}
static nserror bitmap(const struct redraw_context *ctx,struct bitmap *b,int x,int y,int w,int h,colour bg,bitmap_flags_t flags) {
    (void)ctx;(void)bg;if(!b||w<=0||h<=0)return NSERROR_BAD_PARAMETER;
    int x0=x*NATIVE_SCALE,y0=y*NATIVE_SCALE,x1=(x+w)*NATIVE_SCALE,y1=(y+h)*NATIVE_SCALE;
    if(flags&BITMAPF_REPEAT_X){x0=clipping.x0;x1=clipping.x1;}if(flags&BITMAPF_REPEAT_Y){y0=clipping.y0;y1=clipping.y1;}
    if(x0<clipping.x0)x0=clipping.x0;if(y0<clipping.y0)y0=clipping.y0;if(x1>clipping.x1)x1=clipping.x1;if(y1>clipping.y1)y1=clipping.y1;
    for(int yy=y0;yy<y1;++yy)for(int xx=x0;xx<x1;++xx){
        int u=((xx-x*NATIVE_SCALE)%(w*NATIVE_SCALE)+w*NATIVE_SCALE)%(w*NATIVE_SCALE);
        int v=((yy-y*NATIVE_SCALE)%(h*NATIVE_SCALE)+h*NATIVE_SCALE)%(h*NATIVE_SCALE);
        unsigned char *p=b->data+((size_t)(v*b->height/(h*NATIVE_SCALE))*b->width+u*b->width/(w*NATIVE_SCALE))*4;
        blend(xx,yy,p[0]|(p[1]<<8)|(p[2]<<16),p[3]);
    }return NSERROR_OK;
}
static nserror text(const struct redraw_context *ctx,const plot_font_style_t *f,int x,int y,const char *s,size_t n) {
    (void)ctx;draw_text(x,y,font_pixels(f),f->foreground,s,n,f->weight>=600);return NSERROR_OK;
}
const struct plotter_table native_plotters={.clip=clip,.arc=arc,.disc=disc,.line=line,.rectangle=rectangle,
    .polygon=polygon,.path=path,.bitmap=bitmap,.text=text,.option_knockout=false};
