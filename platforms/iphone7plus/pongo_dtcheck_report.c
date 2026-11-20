/* Walk the live tree with Pongo dt_check()'s node/property layout and bounds. */
typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long u64;
struct screen { volatile u32 *p; u64 stride,width,height; };
static const u8 font[16][5]={
 {7,5,5,5,7},{2,6,2,2,7},{7,1,7,4,7},{7,1,7,1,7},
 {5,5,7,1,1},{7,4,7,1,7},{7,4,7,5,7},{7,1,1,1,1},
 {7,5,7,5,7},{7,5,7,1,7},{7,5,7,5,5},{6,5,6,5,6},
 {7,4,4,4,7},{6,5,5,5,6},{7,4,6,4,7},{7,4,6,4,4}};
static u32 r32(const volatile u8*p)
{return (u32)p[0]|((u32)p[1]<<8)|((u32)p[2]<<16)|((u32)p[3]<<24);}
static u64 r64(const volatile u8*p)
{return (u64)r32(p)|((u64)r32(p+4)<<32);}
static void px(struct screen*s,u64 x,u64 y,u32 c)
{if(x<s->width&&y<s->height)s->p[y*s->stride+x]=c;}
static void nib(struct screen*s,u64 x,u64 y,u32 v)
{for(u32 r=0;r<5;r++)for(u32 c=0;c<3;c++)if(font[v&15][r]&(4u>>c))
 for(u32 dy=0;dy<8;dy++)for(u32 dx=0;dx<8;dx++)px(s,x+c*8+dx,y+r*8+dy,0xffffffffu);}
static void row(struct screen*s,u32 id,u64 v)
{u64 y=8+(u64)id*48;nib(s,8,y,id>>4);nib(s,40,y,id);for(u32 i=0;i<16;i++)nib(s,88+(u64)i*32,y,(u32)(v>>((15-i)*4)));}
struct walk { const volatile u8 *base; u32 length,nodes,maxdepth,error; u64 bad_offset; struct screen *screen; };
static int check_node(struct walk*w,u64 off,u32 depth,u64*used)
{
    if(depth>64||w->nodes>=4096||off>w->length||w->length-off<8){
        w->error=1;w->bad_offset=off;return 0;
    }
    ++w->nodes;if(depth>w->maxdepth)w->maxdepth=depth;
    if((w->nodes&15)==0){row(w->screen,4,0x50524f4752455353ULL);row(w->screen,5,w->nodes);row(w->screen,6,depth);row(w->screen,7,off);__asm__ volatile("dsb sy":::"memory");}
    const volatile u8*node=w->base+off;u32 np=r32(node),nc=r32(node+4);u64 pos=off+8;
    for(u32 i=0;i<np;i++){
        if(pos>w->length||w->length-pos<36){w->error=2;w->bad_offset=pos;return 0;}
        u32 len=r32(w->base+pos+32)&0x00ffffffu;u64 padded=((u64)len+3)&~3ULL;
        if(padded>w->length-pos-36){w->error=3;w->bad_offset=pos;return 0;}
        pos+=36+padded;
    }
    for(u32 i=0;i<nc;i++){
        u64 child_size=0;
        if(!check_node(w,pos,depth+1,&child_size))return 0;
        pos+=child_size;
    }
    *used=pos-off;return 1;
}
void dtcheck_report_main(const volatile void*arg,u64 payload,u64 stack)
{
    (void)payload;(void)stack;const volatile u8*a=(const volatile u8*)arg;
    u64 fb=r64(a+0x28),stride=r64(a+0x38),width=r64(a+0x40),height=r64(a+0x48);
    u64 vb=r64(a+0x08),pb=r64(a+0x10),va=r64(a+0x60);u32 len=r32(a+0x68);
    if(!fb||(fb&3)||stride<512||(stride&3)||width<300||height<320||width>4096||height>4096||len<8||len>0x100000||va<vb)
        for(;;)__asm__ volatile("wfe");
    struct screen s={(volatile u32*)fb,stride/4,width,height};
    for(u64 y=0;y<640;y++)for(u64 x=0;x<620;x++)px(&s,x,y,0);
    u64 pa=va-vb+pb;row(&s,0,0x4454434845434b00ULL); /* "DTCHECK" */
    row(&s,1,len);row(&s,2,pa);__asm__ volatile("dsb sy":::"memory");
    struct walk w={(const volatile u8*)pa,len,0,0,0,0,&s};u64 total=0;
    int ok=check_node(&w,0,0,&total);
    row(&s,4,ok?0x5355434345535300ULL:0x4641494c00000000ULL);
    row(&s,5,w.nodes);row(&s,6,w.maxdepth);row(&s,7,w.error);
    row(&s,8,w.bad_offset);row(&s,9,total);row(&s,10,((u64)r32((const volatile u8*)pa+4)<<32)|r32((const volatile u8*)pa));
    row(&s,11,ok?0x444f4e4500000000ULL:0x4552524f52000000ULL);
    __asm__ volatile("dsb sy":::"memory");for(;;)__asm__ volatile("wfe");
}
