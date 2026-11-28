/* SPDX-License-Identifier: Apache-2.0 */
#include <microkit.h>
#include <stdatomic.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include "protocol.h"
#include "../comaps-native/raster.h"
#include "../../tests/input/slint_frame.h"
#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"
uintptr_t files_frame,files_heap,files_input,files_state,files_request,files_vault_transfer,files_chunk;
#ifdef MAP_DESKTOP
uintptr_t desktop_display_mode;
#define W 560
#define SCALE 2
#else
#define W 360
#define SCALE 3
#endif
static unsigned height=520,tail,serial,pressed=~0u;
static size_t heap_used;
static struct files_grant grant;
static struct {uint64_t id,length;char name[121];} entries[12];
static unsigned count,first;
static bool opened,error,listing,more;
static uint64_t cursor;
static stbtt_fontinfo font;
extern const uint8_t map_font[];
#ifdef FILES_FIXTURE
extern const uint8_t files_sample[],files_sample_end[],files_sample2[],files_sample2_end[];
static bool seeded;
#endif
void *sbrk(ptrdiff_t n){if(n<0||(size_t)n>0x800000-heap_used){errno=ENOMEM;return (void*)-1;}void*p=(void*)(files_heap+heap_used);heap_used+=n;return p;}
static int put(char c,FILE*f){(void)f;microkit_dbg_putc(c);return 0;}
static int get(FILE*f){(void)f;return _FDEV_EOF;}
static FILE console=FDEV_SETUP_STREAM(put,get,NULL,_FDEV_SETUP_RW);
FILE *const stdin=&console,*const stdout=&console,*const stderr=&console;
void _exit(int code){(void)code;__builtin_trap();}
static struct files_state *state(void){return (void*)files_state;}
static uint8_t *pixels(void){return (uint8_t*)files_frame+sizeof(slint_frame_header_t);}
static void fill(int x,int y,int w,int h,uint32_t c){map_fill_rgb(pixels(),W*SCALE,height*SCALE,x*SCALE,y*SCALE,(x+w)*SCALE,(y+h)*SCALE,c);}
static void text(int x,int y,int size,uint32_t colour,const char*s){
 float scale=stbtt_ScaleForPixelHeight(&font,size*SCALE),pen=x*SCALE;
 for(;*s&&pen<(W-12)*SCALE;s++){int w,h,xx,yy;uint8_t *bits=stbtt_GetCodepointBitmap(&font,scale,scale,(uint8_t)*s,&w,&h,&xx,&yy);
  for(int j=0;j<h;j++)for(int i=0;i<w;i++){int px=(int)pen+xx+i,py=y*SCALE+yy+j;if(px<0||py<0||px>=W*SCALE||py>=(int)(height*SCALE))continue;uint8_t*p=pixels()+((size_t)py*W*SCALE+px)*3;unsigned a=bits[j*w+i];for(int k=0;k<3;k++)p[k]=(p[k]*(255-a)+((colour>>(16-8*k))&255)*a+127)/255;}
  stbtt_FreeBitmap(bits,NULL);int advance,bearing;stbtt_GetCodepointHMetrics(&font,(uint8_t)*s,&advance,&bearing);pen+=advance*scale;
 }
}
static void publish(unsigned status){struct files_state*s=state();unsigned r=__atomic_load_n(&s->revision,__ATOMIC_RELAXED);__atomic_store_n(&s->revision,r+1,__ATOMIC_RELEASE);atomic_thread_fence(memory_order_seq_cst);s->open=opened;s->status=status;s->serial=serial;s->token[0]=grant.token[0];s->token[1]=grant.token[1];s->length=grant.length;__atomic_store_n(&s->revision,r+2,__ATOMIC_RELEASE);microkit_notify(0);microkit_notify(1);}
static unsigned visible(void){unsigned n=(height-180)/62;return n>5?5:n;}
static void draw(void){
 slint_frame_header_t*h=(void*)files_frame;unsigned r=atomic_load_explicit(&h->revision,memory_order_relaxed);atomic_store_explicit(&h->revision,r+1,memory_order_release);atomic_thread_fence(memory_order_seq_cst);
 fill(0,0,W,height,0x081b28);text(20,38,24,0xe9f7fb,"Files");text(20,65,15,0x91b5c5,"Choose one offline region pack");text(20,91,13,0x91b5c5,"Maps can read only the file you select.");
 for(unsigned i=0;i<visible()&&first+i<count;i++){unsigned j=first+i;fill(14,112+i*62,W-28,56,pressed==j?0x245566:0x12303e);text(24,137+i*62,16,0xe9f7fb,entries[j].name);char info[64];snprintf(info,sizeof(info),"%.2f MB  -  Read access only",entries[j].length/1e6);text(24,157+i*62,13,0x91b5c5,info);}
 if(!count)text(20,143,16,0x91b5c5,error?"Files unavailable - unlock and try again":listing?"Loading files...":"No region packs in Files");
 fill(14,height-56,W-28,40,0x245566);text(25,height-30,17,0x92e3e0,"Cancel");
 if(count>visible()||more||(!count&&cursor)){fill(W-130,14,116,32,0x245566);text(W-120,36,14,0x92e3e0,"More files");}
 h->width=W*SCALE;h->height=height*SCALE;h->stride_bytes=W*SCALE*3;h->format=1;atomic_store_explicit(&h->revision,r+2,memory_order_release);microkit_notify(0);
}
static microkit_msginfo vault(unsigned label,unsigned n){return microkit_ppcall(3,microkit_msginfo_new(label,n));}
#ifdef FILES_FIXTURE
static bool seed(const uint8_t*data,size_t n,const char*name){
 uint32_t*p=(void*)files_vault_transfer;
 for(size_t off=0;off<n;){size_t amount=n-off;if(amount>FILES_CHUNK)amount=FILES_CHUNK;memset(p,0,4096);*p=amount;memcpy(p+1,data+off,amount);microkit_mr_set(0,off);if(microkit_msginfo_get_label(vault(0x436,1))!=0x300)return false;off+=amount;}
 memset(p,0,4096);*p=strlen(name);memcpy(p+1,name,*p);microkit_mr_set(0,n);return microkit_msginfo_get_label(vault(0x44c,1))==0x300;
}
#endif
static void next_file(void){
 if(!listing)return;microkit_mr_set(0,cursor);microkit_msginfo r=vault(0x44a,1);
 if(microkit_msginfo_get_label(r)!=0x300){error=microkit_msginfo_get_label(r)!=0x30d;listing=false;draw();return;}
 uint64_t next=microkit_mr_get(0),id=microkit_mr_get(1),length=microkit_mr_get(2);uint32_t*p=(void*)files_vault_transfer;
 if(next<=cursor||*p==0||*p>120||length>FILES_LIMIT){error=true;listing=false;draw();return;}cursor=next;
 const char*name=(void*)(p+1);unsigned n=*p;
 if(n>=9&&!memcmp(name+n-9,".nxregion",9)&&count<12){entries[count].id=id;entries[count].length=length;memcpy(entries[count].name,name,n);entries[count].name[n]=0;count++;draw();}
 if(count==12){listing=false;more=true;}else microkit_notify(1); // peer renderer yields and wakes us for another directory step
}
static void choose(unsigned index){
 microkit_msginfo r=microkit_ppcall(4,microkit_msginfo_new(0x401,0));if(microkit_msginfo_get_label(r)!=0x400||microkit_msginfo_get_count(r)!=2){error=true;draw();return;}
 grant=(struct files_grant){{microkit_mr_get(0),microkit_mr_get(1)},entries[index].id,entries[index].length};if(!(grant.token[0]||grant.token[1])){files_revoke(&grant);error=true;draw();return;}
 memset(state()->name,0,sizeof(state()->name));memcpy(state()->name,entries[index].name,strlen(entries[index].name));opened=false;listing=false;memset(entries,0,sizeof(entries));count=0;publish(1);
}
static void input(void){uint32_t*q=(void*)files_input,head=__atomic_load_n(q+2,__ATOMIC_ACQUIRE);if(head-tail>224){tail=head;pressed=~0u;}
 while(tail!=head){uint32_t e[4];memcpy(e,q+4+(tail++%224)*4,16);unsigned dev=e[0]&65535,kind=e[0]>>16;
#ifndef MAP_DESKTOP
 if(dev==3&&kind==1&&e[2]>=320&&e[2]<=720){height=e[2];if(opened)draw();}
#endif
 if(!opened)continue;
 if(dev==0&&kind==1&&e[2]&&(e[1]==103||e[1]==108||e[1]==28)){
  if(count){if(e[1]==28){if(pressed<count)choose(pressed);}
   else {pressed=pressed>=count?0:e[1]==108?(pressed+1)%count:(pressed+count-1)%count;first=pressed/visible()*visible();draw();}}
  continue;
 }
 if(dev==0&&kind==1&&e[1]==1&&e[2]){opened=false;listing=false;files_revoke(&grant);publish(2);continue;}
 if(dev!=1||kind!=1||e[1]!=272)continue;int x=(int16_t)e[3]/SCALE,y=(int16_t)(e[3]>>16)/SCALE;
 if(e[2]){pressed=~0u;if(x>=14&&x<W-14&&y>=112&&y<112+(int)visible()*62){unsigned i=first+(y-112)/62;if(i<count)pressed=i;}draw();}
 else {unsigned selected=pressed;pressed=~0u;if(y>=(int)height-56&&x>=14&&x<W-14){opened=false;listing=false;files_revoke(&grant);publish(2);}
 else if(y>=14&&y<46&&x>W-130){if(first+visible()<count)first+=visible();else if(more){first=count=0;memset(entries,0,sizeof(entries));more=false;listing=true;microkit_notify(1);}else{first=0;if(!count){cursor=0;listing=true;microkit_notify(1);}}draw();}
 else if(selected<count&&x>=14&&x<W-14&&y>=112&&y<112+(int)visible()*62&&first+(unsigned)(y-112)/62==selected)choose(selected);else draw();}
 }
 __atomic_store_n(q+3,tail,__ATOMIC_RELEASE);
}
void init(void){stbtt_InitFont(&font,map_font,stbtt_GetFontOffsetForIndex(map_font,0));publish(0);microkit_dbg_puts("FILES|READY|selected-file broker\n");}
void notified(microkit_channel ch){
#ifdef MAP_DESKTOP
 const uint32_t*m=(void*)desktop_display_mode;unsigned next=m[5];if(next>=360&&next<=520&&next!=height){height=next;if(opened)draw();}
#endif
 if(ch==0)input();
 if(ch==1){uint64_t request=__atomic_load_n(&((struct files_request*)files_request)->open,__ATOMIC_ACQUIRE);
  if(request&&request!=serial){serial=request;files_revoke(&grant);memset(state()->name,0,128);opened=true;listing=true;error=more=false;count=first=0;cursor=0;pressed=~0u;memset(entries,0,sizeof(entries));publish(0);draw();
#ifdef FILES_FIXTURE
   if(!seeded){microkit_mr_set(0,0);microkit_msginfo r=vault(0x44a,1);if(microkit_msginfo_get_label(r)!=0x300){seeded=seed(files_sample,files_sample_end-files_sample,"gibraltar-v1.nxregion")&&seed(files_sample2,files_sample2_end-files_sample2,"gibraltar-v2.nxregion");}else seeded=true;}
#endif
  }
  if(listing&&opened)next_file();
 }
}
microkit_msginfo protected(microkit_channel ch,microkit_msginfo m){
 struct files_chunk*out=(void*)files_chunk;memset(out,0,4096);unsigned label=microkit_msginfo_get_label(m),n=microkit_msginfo_get_count(m);uint64_t a=microkit_mr_get(0),b=microkit_mr_get(1),off=microkit_mr_get(2),amount=microkit_mr_get(3);
 if(ch!=2||n!=(label==FILES_RELEASE?2:4)||!files_permits(&grant,a,b,label==FILES_RELEASE?0:off,label==FILES_RELEASE?1:amount)){state()->denied++;return microkit_msginfo_new(1,0);}
 if(label==FILES_RELEASE){files_revoke(&grant);publish(3);return microkit_msginfo_new(0,0);}
 if(label!=FILES_READ){state()->denied++;return microkit_msginfo_new(1,0);}
 // A read can straddle two encrypted attachment chunks (e.g. 4096-byte header).
 unsigned copied=0;while(copied<amount){uint64_t at=off+copied;microkit_mr_set(0,grant.id);microkit_mr_set(1,at/FILES_CHUNK);microkit_mr_set(2,UINT64_MAX);microkit_msginfo r=vault(0x438,3);uint32_t*p=(void*)files_vault_transfer;unsigned within=at%FILES_CHUNK,take=amount-copied;
  if(microkit_msginfo_get_label(r)!=0x300||*p>FILES_CHUNK||within>=*p){files_revoke(&grant);memset(out,0,4096);publish(4);return microkit_msginfo_new(1,0);}if(take>*p-within)take=*p-within;memcpy(out->bytes+copied,(uint8_t*)(p+1)+within,take);copied+=take;memset(p,0,4096);
 }
 out->length=copied;state()->reads++;microkit_notify(2);return microkit_msginfo_new(0,0);
}
