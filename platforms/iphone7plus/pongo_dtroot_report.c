/* Validate only root properties and report the last bounded cursor position. */
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
static u64 key8(const volatile u8*p)
{u64 v=0;for(u32 i=0;i<8;i++)v=(v<<8)|p[i];return v;}
void dtroot_report_main(const volatile void*arg,u64 payload,u64 stack)
{
 (void)payload;(void)stack;const volatile u8*a=(const volatile u8*)arg;
 u64 fb=r64(a+0x28),stride=r64(a+0x38),w=r64(a+0x40),h=r64(a+0x48);
 u64 vb=r64(a+0x08),pb=r64(a+0x10),va=r64(a+0x60);u32 len=r32(a+0x68);
 if(!fb||(fb&3)||stride<512||(stride&3)||w<300||h<320||w>4096||h>4096||len<8||len>0x100000||va<vb)
   for(;;)__asm__ volatile("wfe");
 struct screen s={(volatile u32*)fb,stride/4,w,h};
 for(u64 y=0;y<640;y++)for(u64 x=0;x<620;x++)px(&s,x,y,0);
 u64 pa=va-vb+pb;row(&s,0,0x4454524f4f540000ULL); /* "DTROOT" */
 row(&s,1,len);row(&s,2,pa);__asm__ volatile("dsb sy":::"memory");
 const volatile u8*dt=(const volatile u8*)pa;
 u32 np=r32(dt),nc=r32(dt+4);row(&s,3,((u64)nc<<32)|np);
 if(np>64||nc>128){row(&s,4,0xBAD00001);for(;;)__asm__ volatile("wfe");}
 u64 off=8;row(&s,4,0x5354415254000000ULL); /* START */
 for(u32 i=0;i<np;i++){
   if(off>len||len-off<36){row(&s,4,0xBAD00002);row(&s,5,i);row(&s,6,off);break;}
   const volatile u8*prop=dt+off;u32 l=r32(prop+32)&0x00ffffffu;
   u64 padded=((u64)l+3)&~3ULL;
   if(padded>len-off-36){row(&s,4,0xBAD00003);row(&s,5,i);row(&s,6,off);row(&s,7,l);row(&s,8,key8(prop));break;}
   off+=36+padded;
   row(&s,4,0x50524f5000000000ULL|i);row(&s,5,off);row(&s,6,l);row(&s,7,key8(prop));
   __asm__ volatile("dsb sy":::"memory");
 }
 row(&s,9,off);row(&s,10,((u64)nc<<32)|np);row(&s,11,0x444f4e4500000000ULL);
 __asm__ volatile("dsb sy":::"memory");for(;;)__asm__ volatile("wfe");
}
