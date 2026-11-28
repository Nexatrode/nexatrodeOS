/* Disposable phone Maps lifecycle guardian. Only the trusted UI controls it. */
#include <microkit.h>
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#define ROOT_VSPACE (4UL<<58)
#define SESSION_SNAPSHOT_BYTES (1024*1024)
#define SESSION_MAX_BOOT_PAGES 256
static uint64_t lock_epoch=1,activation_epoch;
static bool opened,closing;
static void print(const char *s){microkit_dbg_puts(s);}
static void line(const char *s,uint64_t n){char b[24];unsigned i=0;print(s);do{b[i++]='0'+n%10;n/=10;}while(n);while(i)microkit_dbg_putc(b[--i]);print("\n");}
static void fail(const char *s,uint64_t n){print("MAPS_LOCK|FAIL|");line(s,n);for(;;)seL4_Yield();}
static void check(seL4_Error e,const char *s){if(e)fail(s,e);}
static void copy(void *dst,const void *src,size_t n){uint8_t *d=dst;const uint8_t *s=src;while(n--)*d++=*s++;}
#include "../desktop-simplex/session.h"
struct status {uint32_t revision,active;uint64_t epoch,activation;uint32_t failed,pad;};
static volatile struct status *const status=(void*)0x78000000UL;
static void publish(void){++status->revision;__atomic_thread_fence(__ATOMIC_RELEASE);status->active=closing?2:opened;status->epoch=lock_epoch;status->activation=activation_epoch;status->failed=session_failed;__atomic_thread_fence(__ATOMIC_RELEASE);++status->revision;}
static void stop(void){
    simplex_session_suspend();
    volatile uint32_t *regs=(void*)0x78003000UL;regs[0x70/4]=0;__asm__ volatile("dsb sy":::"memory");
    if(regs[0x70/4])fail("disk reset",0);
    /* Do not detach a donated worker SC while the passive Vault may still
     * be serving its read. The UI first asks Vault to evict its root, then
     * returns with FINALIZE. Until then input is gated by active==2. */
    opened=false;closing=true;publish();
}
void init(void){simplex_session_boot();publish();print("MAPS_LOCK|BOOT_LOCKED\n");}
void notified(microkit_channel ch){(void)ch;}
microkit_msginfo protected(microkit_channel ch,microkit_msginfo msg){
    if(ch!=0||microkit_msginfo_get_count(msg))return microkit_msginfo_new(0x7de,0);
    seL4_Word label=microkit_msginfo_get_label(msg);
    if(label==0x7d0){if(opened){++lock_epoch;stop();}line("MAPS_LOCK|QUIESCED|epoch=",lock_epoch);}
    else if(label==0x7d2){
        if(closing){
            microkit_msginfo r=microkit_ppcall(1,microkit_msginfo_new(0x459,0));
            if(microkit_msginfo_get_label(r)!=0x300||microkit_msginfo_get_count(r)!=2||microkit_mr_get(1)!=0)return microkit_msginfo_new(0x7de,0);
            simplex_session_scrub();closing=false;publish();
        }
        line("MAPS_LOCK|CLOSED|epoch=",lock_epoch);
    }
    else if(label==0x7d1){
        microkit_msginfo r=microkit_ppcall(1,microkit_msginfo_new(0x459,0));
        if(microkit_msginfo_get_label(r)!=0x300||microkit_msginfo_get_count(r)!=2||microkit_mr_get(1)!=1||microkit_mr_get(0)<=activation_epoch||opened||closing||session_failed)return microkit_msginfo_new(0x7de,0);
        activation_epoch=microkit_mr_get(0);
        if(!simplex_session_start())return microkit_msginfo_new(0x7de,0);
        opened=true;publish();line("MAPS_LOCK|OPEN|activation=",activation_epoch);
    }else return microkit_msginfo_new(0x7de,0);
    if(session_failed)return microkit_msginfo_new(0x7de,0);
    microkit_mr_set(0,lock_epoch);microkit_mr_set(1,opened);microkit_mr_set(2,1);return microkit_msginfo_new(0x7df,3);
}
