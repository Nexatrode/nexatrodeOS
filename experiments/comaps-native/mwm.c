/* SPDX-License-Identifier: Apache-2.0
 * Adapted from CoMaps feature.cpp, geometry_coding.{cpp,hpp}, point_coding.cpp,
 * and files_container.cpp. Rewritten with explicit bounds and fixed capacities.
 * This is not the complete CoMaps indexer or renderer. See NOTICE.
 */
#include "mwm.h"
#include <math.h>
#include <string.h>

#ifdef MAP_REFERENCE
extern bool map_reference(const unsigned char *,size_t,unsigned,unsigned,unsigned,uint32_t,uint32_t,const struct map_point *,unsigned,double,double);
#endif

struct cursor { const unsigned char *p, *end; bool bad; };
struct section { const unsigned char *p; size_t size; };
struct upoint { uint32_t x, y; };
struct decoder {
    struct offline_map *map;
    struct section dat, geometry[4], triangles[4];
    struct upoint base;
    uint8_t scales[4];
    struct upoint scratch[MAP_MAX_GEOMETRY];
    uint32_t stack[MAP_MAX_GEOMETRY];
};
/* One reader per single-threaded app; no shared global app data. */
static struct decoder decoder;
static unsigned byte(struct cursor *c) {
    if(c->p == c->end) { c->bad=true; return 0; }
    return *c->p++;
}
static uint64_t var(struct cursor *c) {
    uint64_t n=0;
    for(unsigned shift=0; shift<64; shift+=7) {
        unsigned b=byte(c);
        if(c->bad || (shift==63 && b>1)) { c->bad=true; return 0; }
        n|=(uint64_t)(b&127)<<shift;
        if(!(b&128)) return n;
    }
    c->bad=true; return 0;
}
static void skip(struct cursor *c,uint64_t n) {
    if(n>(uint64_t)(c->end-c->p)) c->bad=true;
    else c->p+=(size_t)n;
}
static uint64_t little(struct cursor *c,unsigned n) {
    uint64_t v=0; for(unsigned i=0;i<n;i++) v|=(uint64_t)byte(c)<<(8*i);
    return v;
}
static int64_t unzig(uint64_t v) { return (int64_t)(v>>1)^-(int64_t)(v&1); }
static struct upoint split(uint64_t n) {
    struct upoint p={0};
    for(unsigned i=0;i<32;i++) {p.x|=((n>>(2*i))&1)<<i;p.y|=((n>>(2*i+1))&1)<<i;}
    return p;
}
static bool delta(uint64_t v,struct upoint base,uint32_t max,struct upoint *out) {
    struct upoint d=split(v);
    int64_t x=(int64_t)base.x+unzig(d.x), y=(int64_t)base.y+unzig(d.y);
    if(x<0||y<0||x>max||y>max) return false;
    *out=(struct upoint){(uint32_t)x,(uint32_t)y};return true;
}
static uint32_t clamp(int64_t n,uint32_t max) { return n<0?0:n>max?max:(uint32_t)n; }
static struct upoint predict(struct upoint a,struct upoint b,struct upoint c,uint32_t max,bool triangle) {
    if(triangle) return (struct upoint){clamp((int64_t)a.x+b.x-c.x,max),clamp((int64_t)a.y+b.y-c.y,max)};
    /* CoMaps uses double then truncates, including half integer predictions. */
    double x=a.x+((double)a.x-b.x)/2., y=a.y+((double)a.y-b.y)/2.;
    return (struct upoint){x<0?0:x>max?max:(uint32_t)x,y<0?0:y>max?max:(uint32_t)y};
}
static double coord(uint32_t v,unsigned bits) { return (double)v*360./((1ULL<<bits)-1)-180.; }
static struct upoint recode(struct upoint p,unsigned from,unsigned to) {
    double ratio=(double)((1ULL<<to)-1)/((1ULL<<from)-1);
    return (struct upoint){(uint32_t)(p.x*ratio+.5),(uint32_t)(p.y*ratio+.5)};
}
static bool append(struct decoder *d,struct map_feature *f,struct upoint p,unsigned bits) {
    struct offline_map *m=d->map;
    if(m->point_count==MAP_MAX_POINTS) {m->error="point capacity";return false;}
    struct map_point q={(float)(coord(p.x,bits)-m->origin_x),(float)(coord(p.y,bits)-m->origin_y)};
    if(!f->count) f->minx=f->maxx=q.x,f->miny=f->maxy=q.y;
    else {f->minx=fminf(f->minx,q.x);f->maxx=fmaxf(f->maxx,q.x);f->miny=fminf(f->miny,q.y);f->maxy=fmaxf(f->maxy,q.y);}
    m->points[m->point_count++]=q;f->count++;return true;
}
static bool path(struct decoder *d,struct cursor *c,unsigned count,bool to_end,bool triangle,
                 struct upoint base,unsigned bits,struct map_feature *f) {
    uint32_t max=(1ULL<<bits)-1;
#ifdef MAP_REFERENCE
    const unsigned char *raw=c->p; unsigned first=d->map->point_count;
#endif
    unsigned n=0;
    while(!c->bad && (to_end?c->p<c->end:n<count)) {
        if(n==MAP_MAX_GEOMETRY) return false;
        struct upoint pred=base;
        if(n) pred=d->scratch[n-1];
        if(n>=2 && (!triangle||n>=3)) pred=predict(d->scratch[n-1],d->scratch[n-2],n>=3?d->scratch[n-3]:base,max,triangle);
        if(!delta(var(c),pred,max,&d->scratch[n])) return false;
        n++;
    }
    if(c->bad||n<(triangle?3u:2u)) return false;
    if(triangle) {
        for(unsigned i=2;i<n;i++) for(unsigned j=i-2;j<=i;j++) if(!append(d,f,d->scratch[j],bits)) return false;
    } else for(unsigned i=0;i<n;i++) if(!append(d,f,d->scratch[i],bits)) return false;
#ifdef MAP_REFERENCE
    if(!map_reference(raw,c->p-raw,to_end?0:count,triangle?1:0,bits,base.x,base.y,d->map->points+first,d->map->point_count-first,d->map->origin_x,d->map->origin_y))return false;
#endif
    return true;
}
static bool chain(struct decoder *d,struct cursor *c,struct upoint base,unsigned bits,struct map_feature *f) {
    uint32_t max=(1ULL<<bits)-1;
#ifdef MAP_REFERENCE
    const unsigned char *raw=c->p; unsigned first=d->map->point_count;
#endif
    if(!delta(var(c),base,max,&d->scratch[0]) || !delta(var(c),d->scratch[0],max,&d->scratch[1])) return false;
    uint64_t v=var(c);
    if(!delta(v>>2,d->scratch[1],max,&d->scratch[2])) return false;
    unsigned n=3,ind=2,tree=v&3,sp=0;
    while(c->p<c->end && !c->bad) {
        unsigned a,b,opposite;
        if(tree&1) {a=ind;b=ind-1;opposite=ind-2;if(tree&2) d->stack[sp++]=ind;}
        else if(tree&2) {a=ind-2;b=ind;opposite=ind-1;}
        else {if(!sp) return false;ind=d->stack[--sp];tree=2;continue;}
        if(n+3>MAP_MAX_GEOMETRY||sp>=MAP_MAX_GEOMETRY) return false;
        v=var(c);
        struct upoint p;
        if(!delta(v>>2,predict(d->scratch[a],d->scratch[b],d->scratch[opposite],max,true),max,&p)) return false;
        d->scratch[n++]=d->scratch[a];d->scratch[n++]=d->scratch[b];d->scratch[n++]=p;
        tree=v&3;ind=n-1;
    }
    if(c->bad||tree||sp) return false;
    for(unsigned i=0;i<n;i++) if(!append(d,f,d->scratch[i],bits)) return false;
#ifdef MAP_REFERENCE
    if(!map_reference(raw,c->p-raw,0,2,bits,base.x,base.y,d->map->points+first,d->map->point_count-first,d->map->origin_x,d->map->origin_y))return false;
#endif
    return true;
}
static void name(struct cursor *c,char out[96]) {
    uint64_t n=var(c)+1;
    if(c->bad||n>(uint64_t)(c->end-c->p)) {c->bad=true;return;}
    const unsigned char *s=c->p;skip(c,n);
    int best=-1;
    for(size_t i=0;i<n;) {
        unsigned lang=s[i++]&63;size_t start=i;
        while(i<n) {
            unsigned b=s[i];if((b&0xc0)==0x80) break;
            unsigned len=b<128?1:b<224?2:b<240?3:4;
            if(len>n-i) {c->bad=true;return;}i+=len;
        }
        int score=lang==1?2:lang==0?1:0;
        if(score>best) {size_t count=i-start;if(count>95) count=95;memcpy(out,s+start,count);out[count]=0;best=score;}
    }
}
static bool feature(struct decoder *d,struct cursor c) {
    struct offline_map *m=d->map;
    if(m->feature_count==MAP_MAX_FEATURES) {m->error="feature capacity";return false;}
    struct map_feature *f=&m->features[m->feature_count];memset(f,0,sizeof(*f));f->first=m->point_count;
    unsigned h=byte(&c),geom=h&96;
    f->type_count=(h&7)+1;
    for(unsigned i=0;i<f->type_count;i++) {uint64_t t=var(&c);if(t>65535)return false;f->types[i]=t;if(!i)f->type=t;}
    if(h&8) name(&c,f->name);
    if(h&16) byte(&c);
    if(h&128) {
        if(geom==0)byte(&c);
        else {uint64_t n=var(&c);if(geom==32)skip(&c,n+1);else if(!(n&1))skip(&c,(n>>1)+1);}
    }
    unsigned bits=m->coord_bits;uint32_t max=(1ULL<<bits)-1;
    if(geom==0||geom==96) {
        uint64_t v=var(&c);if(m->dat_version) v>>=1;
        struct upoint p;if(!delta(v,d->base,max,&p)||!append(d,f,p,bits)) return false;
    } else {
        f->geometry=geom==32?1:2;
        unsigned h2=byte(&c),count=h2&15,mask=0;
        bool outer=m->dat_version?(h2&16)!=0:count==0;
        if(outer) {mask=m->dat_version?count:h2>>4;count=0;}
        if(count) {
            m->inner++;
            if(geom==32)skip(&c,(count+1)/4);
            else count+=2;
            if(!path(d,&c,count,false,geom==64,d->base,bits,f)) return false;
        } else {
            m->outer++;
            struct upoint base=d->base;
            if(geom==32&&!delta(var(&c),base,max,&base)) return false;
            unsigned selected=0;uint64_t offset=0;bool found=false;
            for(unsigned i=0;i<4;i++)if(mask&(1<<i)) {uint64_t o=var(&c);if(o!=1 && o!=UINT32_MAX){selected=i;offset=o;found=true;}}
            if(!found) return false;
            bits-= (d->scales[3]-d->scales[selected])/2;
            struct section s=geom==32?d->geometry[selected]:d->triangles[selected];
            if(!s.p||offset>=s.size) return false;
            struct cursor outerc={s.p+offset,s.p+s.size,false};
            unsigned chains=geom==64?var(&outerc):1;if(chains>MAP_MAX_GEOMETRY)return false;
            if(geom==32)base=recode(base,m->coord_bits,bits);
            for(unsigned i=0;i<chains;i++) {
                uint64_t n=var(&outerc);
                if(outerc.bad||n>(uint64_t)(outerc.end-outerc.p))return false;
                struct cursor part={outerc.p,outerc.p+n,false};skip(&outerc,n);
                if(geom==32) {if(!path(d,&part,0,true,false,base,bits,f))return false;}
                else if(!chain(d,&part,base,bits,f))return false;
            }
        }
    }
    if(c.bad) return false;
    m->feature_count++;return true;
}
bool map_load(struct offline_map *m,const unsigned char *data,size_t size) {
    memset(m,0,sizeof(*m));memset(&decoder,0,sizeof(decoder));decoder.map=m;
    m->error="invalid MWM";
    if(size<16||size>32*1024*1024) return false;
    struct cursor c={data,data+size,false};uint64_t table=little(&c,8);
    if(table>=size) return false;
    c.p=data+table;uint64_t n=var(&c);if(n>128)return false;
    struct section header={0};
    for(unsigned i=0;i<n;i++) {
        uint64_t len=var(&c);char tag[32]={0};
        if(c.bad||len>=sizeof(tag)||len>(uint64_t)(c.end-c.p))return false;
        memcpy(tag,c.p,len);skip(&c,len);uint64_t offset=var(&c),bytes=var(&c);
        if(c.bad||offset>size||bytes>size-offset)return false;
        struct section s={data+offset,bytes};
        if(!strcmp(tag,"header"))header=s;
        else if(!strcmp(tag,"features"))decoder.dat=s;
        else if(strlen(tag)==5&&!memcmp(tag,"geom",4)&&tag[4]>='0'&&tag[4]<='3')decoder.geometry[tag[4]-'0']=s;
        else if(strlen(tag)==4&&!memcmp(tag,"trg",3)&&tag[3]>='0'&&tag[3]<='3')decoder.triangles[tag[3]-'0']=s;
    }
    if(!header.p||!decoder.dat.p||header.size<4||decoder.dat.size<9)return false;
    c=(struct cursor){header.p,header.p+header.size,false};
    uint64_t bits=var(&c);if(bits<8||bits>31)return false;m->coord_bits=bits;
    decoder.base=split(var(&c));
    struct upoint lo=split((uint64_t)unzig(var(&c))),hi=split((uint64_t)unzig(var(&c)));
    m->minx=coord(lo.x,bits);m->maxx=coord(hi.x,bits);m->miny=coord(lo.y,bits);m->maxy=coord(hi.y,bits);
    if(!(m->minx<m->maxx&&m->miny<m->maxy))return false;
    m->origin_x=(m->minx+m->maxx)/2;m->origin_y=(m->miny+m->maxy)/2;
    if(var(&c)!=4)return false;
    for(unsigned i=0;i<4;i++) {decoder.scales[i]=byte(&c);if(i&&decoder.scales[i]<decoder.scales[i-1])return false;}
    if(c.bad||decoder.scales[3]-decoder.scales[0]>=2*bits)return false;
    c=(struct cursor){decoder.dat.p,decoder.dat.p+decoder.dat.size,false};
    m->dat_version=byte(&c);if(m->dat_version>1)return false;
    uint64_t start=little(&c,4),length=little(&c,4);
    if(start<9||start>decoder.dat.size||length>decoder.dat.size-start)return false;
    c.p=decoder.dat.p+start;c.end=c.p+length;
    while(c.p<c.end&&!c.bad) {
        uint64_t bytes=var(&c);
        if(c.bad||!bytes||bytes>(uint64_t)(c.end-c.p))return false;
        struct cursor record={c.p,c.p+bytes,false};skip(&c,bytes);m->records++;
        if(!feature(&decoder,record))return false;
    }
    if(c.bad||!m->feature_count||!m->point_count)return false;
    m->error=NULL;return true;
}
