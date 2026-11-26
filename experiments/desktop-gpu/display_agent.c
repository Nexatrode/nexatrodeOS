/* SPDX-License-Identifier: MIT
 * Display-only SPICE agent over modern VirtIO PCI serial. No clipboard,
 * file transfer, command execution, filesystem, networking or app mappings.
 * Protocol: spice.pages.freedesktop.org/spice-space/agent-protocol.html
 */
#include <microkit.h>
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include "display_protocol.h"
void *memcpy(void *d,const void *s,size_t n) {uint8_t *out=d;const uint8_t *in=s;while(n--)*out++=*in++;return d;}
void *memset(void *d,int c,size_t n) {uint8_t *out=d;while(n--)*out++=c;return d;}
uintptr_t pci_ecam,pci_bar,serial_dma,serial_dma_paddr,display_request,display_ack;
#define N 8u
#define QUEUES 18u
#define BUFFER 2048u
#define BAR_PHYS 0x10000000u
struct desc {uint64_t address;uint32_t length;uint16_t flags,next;};
struct queue {uintptr_t base;uint16_t head,tail,notify;bool busy[N],enabled;};
static struct queue queues[QUEUES];
static uintptr_t common,notify_cfg,device_cfg;
static uint32_t notify_mult,selected_port=~0u,serial,awaiting,acked;
static bool online,announced,dead;
static uint8_t chunk_header[8],message[276];
static unsigned chunk_have,chunk_remaining,chunk_port,msg_have,msg_need;
static uint64_t pending_opaque;
static void zero(void *p,size_t n) {volatile uint8_t *b=p;while(n--)*b++=0;}
static uint32_t get32(const uint8_t *p) {return p[0]|((uint32_t)p[1]<<8)|((uint32_t)p[2]<<16)|((uint32_t)p[3]<<24);}
static void put32(uint8_t *p,uint32_t n) {for(unsigned i=0;i<4;++i)p[i]=n>>(i*8);}
static void number(uint64_t n) {char b[24];unsigned i=0;do{b[i++]=(char)('0'+n%10);n/=10;}while(n);while(i)microkit_dbg_putc(b[--i]);}
static void field(const char *name,uint64_t n) {microkit_dbg_puts(name);number(n);microkit_dbg_puts("\n");}
static void barrier(void) {asm volatile("dmb sy":::"memory");}
static uint8_t r8(uintptr_t p) {return *(volatile uint8_t*)p;}
static uint16_t r16(uintptr_t p) {return *(volatile uint16_t*)p;}
static uint32_t r32(uintptr_t p) {return *(volatile uint32_t*)p;}
static void w8(uintptr_t p,uint8_t n) {*(volatile uint8_t*)p=n;barrier();}
static void w16(uintptr_t p,uint16_t n) {*(volatile uint16_t*)p=n;barrier();}
static void w32(uintptr_t p,uint32_t n) {*(volatile uint32_t*)p=n;barrier();}
static void w64(uintptr_t p,uint64_t n) {w32(p,n);w32(p+4,n>>32);}
static void fail(const char *s) {dead=true;microkit_dbg_puts("DISPLAY_AGENT|FAIL|");microkit_dbg_puts(s);microkit_dbg_puts("\n");}
static void kick(unsigned id) {w16(notify_cfg+queues[id].notify*notify_mult,id);}
static void offer(unsigned q,unsigned d) {
    struct queue *v=&queues[q];volatile uint16_t *a=(void*)(v->base+0x100);
    a[2+v->head%N]=d;barrier();a[1]=++v->head;barrier();kick(q);
}
static bool setup_queue(unsigned id) {
    w16(common+22,id);if(r16(common+24)<N)return false;
    struct queue *q=&queues[id];q->base=serial_dma+id*0x10000;
    uint64_t pa=serial_dma_paddr+id*0x10000;
    zero((void*)q->base,0x6000);w16(common+24,N);w16(common+26,0xffff);
    w64(common+32,pa);w64(common+40,pa+0x100);w64(common+48,pa+0x200);
    q->notify=r16(common+30);
    if((uint64_t)q->notify*notify_mult+2>0x1000)return false;
    *(volatile uint16_t*)(q->base+0x100)=1;w16(common+28,1);q->enabled=true;
    return true;
}
static void receive_buffers(unsigned id) {
    struct queue *q=&queues[id];volatile struct desc *d=(void*)q->base;
    for(unsigned i=0;i<N;++i){d[i]=(struct desc){serial_dma_paddr+id*0x10000+0x1000+i*BUFFER,BUFFER,2,0};offer(id,i);}
}
static void reap_tx(unsigned id) {
    struct queue *q=&queues[id];volatile uint16_t *u=(void*)(q->base+0x200);
    if((uint16_t)(u[1]-q->tail)>N){fail("TX completion count");return;}
    while(q->tail!=u[1]){barrier();volatile uint32_t *e=(void*)(q->base+0x204+8*(q->tail%N));
        if(e[0]>=N||!q->busy[e[0]]){fail("TX descriptor");return;}q->busy[e[0]]=false;++q->tail;}
}
static bool send_bytes(unsigned id,const uint8_t *bytes,unsigned length) {
    if(id>=QUEUES||!queues[id].enabled||length>BUFFER||dead)return false;
    reap_tx(id);struct queue *q=&queues[id];unsigned i=0;while(i<N&&q->busy[i])++i;if(i==N)return false;
    volatile uint8_t *out=(void*)(q->base+0x1000+i*BUFFER);for(unsigned n=0;n<length;++n)out[n]=bytes[n];
    volatile struct desc *d=(void*)q->base;d[i]=(struct desc){serial_dma_paddr+id*0x10000+0x1000+i*BUFFER,length,0,0};q->busy[i]=true;offer(id,i);return true;
}
static bool control(uint32_t id,unsigned event,unsigned value) {
    uint8_t b[8];put32(b,id);b[4]=event;b[5]=0;b[6]=value;b[7]=0;return send_bytes(3,b,8);
}
static bool agent_message(unsigned type,const uint8_t *body,unsigned length,uint64_t opaque) {
    if(selected_port==~0u||length>64)return false;
    uint8_t b[92]={0};put32(b,1);put32(b+4,20+length);put32(b+8,1);put32(b+12,type);
    put32(b+16,opaque);put32(b+20,opaque>>32);put32(b+24,length);
    for(unsigned i=0;i<length;++i)b[28+i]=body[i];
    return send_bytes(selected_port?selected_port*2+3:1,b,28+length);
}
static bool capabilities(unsigned request) {uint8_t b[8];put32(b,request);put32(b+4,6);return agent_message(6,b,8,0);}
static void process_message(void) {
    unsigned type=get32(message+4),size=get32(message+16);const uint8_t *b=message+20;
    if(type==6&&size>=8){if(get32(b))capabilities(0);return;}
    if(type!=2)return;
    uint32_t width,height;
    uint64_t opaque=(uint64_t)get32(message+8)|((uint64_t)get32(message+12)<<32);
    if(!display_monitor_request(b,size,&width,&height)){
        uint8_t error[8];put32(error,2);put32(error+4,2);agent_message(3,error,8,opaque);return;
    }
    struct display_request *out=(void*)display_request;uint32_t revision=out->revision;
    __atomic_store_n(&out->revision,revision+1,__ATOMIC_RELEASE);__atomic_thread_fence(__ATOMIC_SEQ_CST);
    awaiting=++serial;out->serial=serial;out->width=width;out->height=height;
    pending_opaque=opaque;
    __atomic_store_n(&out->revision,revision+2,__ATOMIC_RELEASE);
    field("DISPLAY_AGENT|requested_width=",out->width);field("DISPLAY_AGENT|requested_height=",out->height);
    microkit_notify(0);
}
static void stream(const uint8_t *data,unsigned length) {
    for(unsigned i=0;i<length&&!dead;++i){
        uint8_t b=data[i];
        if(!chunk_remaining){chunk_header[chunk_have++]=b;if(chunk_have<8)continue;
            chunk_have=0;chunk_port=get32(chunk_header);chunk_remaining=get32(chunk_header+4);
            if((chunk_port!=1&&chunk_port!=2)||!chunk_remaining||chunk_remaining>4096){fail("chunk bounds");return;}continue;}
        --chunk_remaining;
        if(msg_have<sizeof(message))message[msg_have]=b;++msg_have;
        if(msg_have==20){
            if(get32(message)!=1||get32(message+16)>4096){fail("message bounds");return;}
            msg_need=20+get32(message+16);
        }
        if(msg_need&&msg_have==msg_need){if(msg_need<=sizeof(message))process_message();msg_have=msg_need=0;}
    }
}
static void received(unsigned id,const uint8_t *b,unsigned length) {
    if(id!=2){stream(b,length);return;}
    if(length<8)return;unsigned port=get32(b),event=b[4]|(b[5]<<8),value=b[6]|(b[7]<<8);
    if(event==1){control(port,3,port<8?1:0);return;}
    if(port>=8)return;
    if(event==7){
        const char name[]="com.redhat.spice.0";
        if(length==8+sizeof(name)||(length==8+sizeof(name)-1)){bool match=true;for(unsigned i=0;i<sizeof(name)-1;++i)if(b[8+i]!=(uint8_t)name[i])match=false;
            if(length==8+sizeof(name)&&b[length-1]!=0)match=false;
            if(match){selected_port=port;control(port,6,1);field("DISPLAY_AGENT|port=",port);}}
    } else if(event==6&&port==selected_port){online=value!=0;announced=false;if(!online){chunk_have=chunk_remaining=msg_have=msg_need=0;}}
}
static void poll_rx(unsigned id) {
    struct queue *q=&queues[id];volatile uint16_t *u=(void*)(q->base+0x200);
    if((uint16_t)(u[1]-q->tail)>N){fail("RX completion count");return;}
    unsigned limit=N;
    while(q->tail!=u[1]&&limit--&&!dead){barrier();volatile uint32_t *e=(void*)(q->base+0x204+8*(q->tail%N));unsigned d=e[0],length=e[1];
        if(d>=N||length>BUFFER){fail("RX descriptor");return;}
        uint8_t bytes[BUFFER];const volatile uint8_t *p=(void*)(q->base+0x1000+d*BUFFER);for(unsigned i=0;i<length;++i)bytes[i]=p[i];
        ++q->tail;received(id,bytes,length);offer(id,d);
    }
}
static void poll(void) {
    if(dead)return;poll_rx(2);reap_tx(3);
    if(selected_port!=~0u){unsigned rx=selected_port?selected_port*2+2:0;poll_rx(rx);reap_tx(rx+1);
        if(online&&!announced&&capabilities(1)){announced=true;microkit_dbg_puts("DISPLAY_AGENT|READY|monitor-config-only\n");}}
    const struct display_ack *a=(void*)display_ack;uint32_t rev=__atomic_load_n(&a->revision,__ATOMIC_ACQUIRE),n=a->serial;
    __atomic_thread_fence(__ATOMIC_ACQUIRE);
    if(awaiting&&acked!=awaiting&&rev&&!(rev&1)&&rev==__atomic_load_n(&a->revision,__ATOMIC_ACQUIRE)&&n==awaiting){
        uint8_t b[8];put32(b,2);put32(b+4,1);if(agent_message(3,b,8,pending_opaque))acked=awaiting;
    }
    microkit_mr_set(0,10000000);microkit_ppcall(1,microkit_msginfo_new(1,1));
}
void init(void) {
    uintptr_t config=0;
    for(unsigned slot=0;slot<32;++slot){uintptr_t p=pci_ecam+(slot<<15);uint32_t id=r32(p);
        if((id&0xffff)==0x1af4&&((id>>16)==0x1003||(id>>16)==0x1043)){if(config){fail("multiple serial devices");return;}config=p;}}
    if(!config){fail("PCI serial absent");return;}
    unsigned cap=r8(config+0x34),visits=0,bar=~0u;uint32_t common_off=~0u,notify_off=~0u,device_off=~0u;
    while(cap&&visits++<48){if(cap<0x40||cap>0xe8||(cap&3)){fail("PCI capability bounds");return;}
        if(r8(config+cap)==9){unsigned type=r8(config+cap+3),b=r8(config+cap+4);uint32_t off=r32(config+cap+8),size=r32(config+cap+12);
            if(type>=1&&type<=4){if(b>5||off>=0x10000||size>0x10000-off||r8(config+cap+2)<16){fail("PCI BAR capability");return;}
                if(type==1||type==2||type==4){if(bar!=~0u&&bar!=b){fail("split serial BARs");return;}bar=b;}
                if(type==1)common_off=off;if(type==2){if(r8(config+cap+2)<20){fail("notify capability");return;}notify_off=off;notify_mult=r32(config+cap+16);}if(type==4)device_off=off;
            }}cap=r8(config+cap+1);
    }
    if(cap||common_off==~0u||notify_off==~0u||device_off==~0u||!notify_mult){fail("modern capabilities absent");return;}
    w16(config+4,0);uint32_t flags=r32(config+0x10+bar*4)&15;
    if(flags&1){fail("serial BAR must be memory");return;}
    w32(config+0x10+bar*4,BAR_PHYS|flags);if(flags&4){if(bar==5){fail("64-bit BAR");return;}w32(config+0x14+bar*4,0);}w16(config+4,0x406);
    common=pci_bar+common_off;notify_cfg=pci_bar+notify_off;device_cfg=pci_bar+device_off;
    w8(common+20,0);w8(common+20,1);w8(common+20,3);
    w32(common,0);uint32_t low=r32(common+4);w32(common,1);uint32_t high=r32(common+4);
    if(!(low&2)||!(high&1)){fail("serial features");return;}
    w32(common+8,0);w32(common+12,2);w32(common+8,1);w32(common+12,1);w8(common+20,11);
    if(!(r8(common+20)&8)){fail("feature negotiation");return;}
    unsigned ports=r32(device_cfg+4);if(ports<3){fail("serial port count");return;}
    unsigned count=ports<8?ports:8;
    for(unsigned i=0;i<count*2+2;++i)if(!setup_queue(i)){fail("queue setup");return;}
    w8(common+20,15);receive_buffers(2);
    for(unsigned p=0;p<count;++p)receive_buffers(p?p*2+2:0);
    control(0,0,1);microkit_dbg_puts("DISPLAY_AGENT|PCI_SERIAL_READY\n");poll();
}
void notified(microkit_channel channel) {(void)channel;poll();}
