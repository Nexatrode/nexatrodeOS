/* Display a bounded read of the first 32 bytes at the translated DT address. */
typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long u64;

struct screen { volatile u32 *p; u64 stride, width, height; };
static const u8 font[16][5] = {
    {7,5,5,5,7},{2,6,2,2,7},{7,1,7,4,7},{7,1,7,1,7},
    {5,5,7,1,1},{7,4,7,1,7},{7,4,7,5,7},{7,1,1,1,1},
    {7,5,7,5,7},{7,5,7,1,7},{7,5,7,5,5},{6,5,6,5,6},
    {7,4,4,4,7},{6,5,5,5,6},{7,4,6,4,7},{7,4,6,4,4}
};
static u32 rd32(const volatile u8 *p)
{ return (u32)p[0]|((u32)p[1]<<8)|((u32)p[2]<<16)|((u32)p[3]<<24); }
static u64 rd64(const volatile u8 *p)
{ return (u64)rd32(p)|((u64)rd32(p+4)<<32); }
static void px(struct screen *s,u64 x,u64 y,u32 c)
{ if(x<s->width&&y<s->height)s->p[y*s->stride+x]=c; }
static void nibble(struct screen*s,u64 x,u64 y,u32 v)
{
    for(u32 r=0;r<5;r++)for(u32 c=0;c<3;c++)if(font[v&15][r]&(4u>>c))
        for(u32 dy=0;dy<8;dy++)for(u32 dx=0;dx<8;dx++)
            px(s,x+c*8+dx,y+r*8+dy,0xffffffffu);
}
static void row(struct screen*s,u32 id,u64 value)
{
    u64 y=8+(u64)id*48;
    nibble(s,8,y,(id>>4)&15);nibble(s,40,y,id&15);
    for(u32 i=0;i<16;i++)nibble(s,88+(u64)i*32,y,(u32)(value>>((15-i)*4)));
}
void dtread_report_main(const volatile void *arg,u64 payload,u64 stack)
{
    (void)payload;(void)stack;
    const volatile u8 *a=(const volatile u8*)arg;
    u64 fb=rd64(a+0x28),stride=rd64(a+0x38),width=rd64(a+0x40),height=rd64(a+0x48);
    u64 vb=rd64(a+0x08),pb=rd64(a+0x10),va=rd64(a+0x60);
    u32 len=rd32(a+0x68);
    if(!fb||(fb&3)||stride<512||(stride&3)||width<300||height<320||
       width>4096||height>4096||len<32||len>0x100000||va<vb)
        for(;;)__asm__ volatile("wfe");
    struct screen s={(volatile u32*)fb,stride/4,width,height};
    for(u64 y=0;y<640;y++)for(u64 x=0;x<620;x++)px(&s,x,y,0);
    row(&s,0,0x4454484541444552ULL); /* "DTHEADER" */
    row(&s,1,len);
    u64 pa=va-vb+pb;
    row(&s,2,pa);
    __asm__ volatile("dsb sy":::"memory");
    const volatile u8 *dt=(const volatile u8*)pa;
    u64 first=rd64(dt),second=rd64(dt+8),third=rd64(dt+16),fourth=rd64(dt+24);
    row(&s,3,first);row(&s,4,second);row(&s,5,third);row(&s,6,fourth);
    __asm__ volatile("dsb sy":::"memory");
    for(;;)__asm__ volatile("wfe");
}
