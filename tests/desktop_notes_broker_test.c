#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "desktop_lock_stub/microkit.h"
#include "input/input_handoff.h"
#include "../experiments/desktop-notes/protocol.h"
static bool dynamic_active = true, device_locked;
static uintptr_t private_notes_page;
static uint64_t generation = 7, mr[4], revision;
static unsigned extra_caps, calls;
static _Alignas(8) uint8_t app_page[4096], vault_page[4096], stored[PRIVATE_NOTE_RECORD_MAX];
static size_t stored_size;
#define LOCK_VAULT_CHANNEL 13u
static void line(const char *prefix, uint64_t value) { (void)prefix; (void)value; }
static void fail(const char *why, unsigned error) { (void)why; (void)error; assert(0); }
static unsigned seL4_MessageInfo_get_extraCaps(microkit_msginfo info) { (void)info; return extra_caps; }
#include "../experiments/desktop-notes/broker.h"
microkit_msginfo microkit_msginfo_new(uint64_t label, unsigned count) { return (microkit_msginfo){label,count}; }
uint64_t microkit_msginfo_get_label(microkit_msginfo info) { return info.label; }
unsigned microkit_msginfo_get_count(microkit_msginfo info) { return info.count; }
void microkit_mr_set(unsigned index, uint64_t value) { assert(index < 4); mr[index] = value; }
uint64_t microkit_mr_get(unsigned index) { assert(index < 4); return mr[index]; }
void microkit_dbg_puts(const char *text) { (void)text; }
microkit_msginfo microkit_ppcall(unsigned channel, microkit_msginfo request) {
    assert(channel == PRIVATE_NOTE_VAULT_CHANNEL); ++calls;
    if (request.label == 0x301) { mr[0]=revision; return microkit_msginfo_new(0x300,2); }
    if (request.label == 0x30b) {
        memcpy(vault_page+4,stored,stored_size); *(uint32_t *)vault_page=stored_size;
        mr[0]=revision; mr[1]=stored_size; return microkit_msginfo_new(0x300,2);
    }
    assert(request.label == 0x30c && request.count == 2);
    if (mr[0] != revision) return microkit_msginfo_new(0x305,0);
    assert(mr[1] <= sizeof(stored) && mr[1] == *(uint32_t *)vault_page);
    stored_size=mr[1]; memcpy(stored,vault_page+4,stored_size); mr[0]=++revision;
    return microkit_msginfo_new(0x300,1);
}
static void erased(const void *p, size_t n) { const uint8_t *b=p; for(size_t i=0;i<n;++i) assert(!b[i]); }
static void staged(const char *text) {
    memset(app_page,0,sizeof(app_page)); struct private_note_page *p=(void *)app_page;
    p->length=strlen(text); memcpy(p->text,text,p->length);
}
static microkit_msginfo request(unsigned caller, uint64_t supplied, unsigned label, unsigned count, uint64_t expected) {
    mr[0]=supplied;mr[1]=expected;
    microkit_msginfo result=private_notes_request(caller,microkit_msginfo_new(label,count));
    erased(private_notes_scratch,sizeof(private_notes_scratch));erased(vault_page,sizeof(vault_page));return result;
}
int main(void) {
    private_notes_page=(uintptr_t)app_page;private_notes_vault=vault_page;
    assert(!private_notes_control((struct input_event_record){0}));
    assert(request(7,7,PRIVATE_NOTE_LOAD,1,0).label==0 && mr[0]==0);
    staged("first note");assert(request(7,7,PRIVATE_NOTE_SAVE,2,0).label==0 && revision==1);erased(app_page,4096);
    assert(request(7,7,PRIVATE_NOTE_LOAD,1,0).label==0 && mr[0]==1);
    struct private_note_page *p=(void *)app_page;assert(p->length==10 && !memcmp(p->text,"first note",10));
    unsigned before=calls;
    assert(request(60,7,PRIVATE_NOTE_LOAD,1,0).label==1);
    assert(request(7,6,PRIVATE_NOTE_LOAD,1,0).label==1);
    assert(request(7,7,PRIVATE_NOTE_LOAD,2,8).label==1);
    extra_caps=1;assert(request(7,7,PRIVATE_NOTE_LOAD,1,0).label==1);extra_caps=0;
    device_locked=true;assert(request(7,7,PRIVATE_NOTE_LOAD,1,0).label==1);device_locked=false;
    dynamic_active=false;assert(request(7,7,PRIVATE_NOTE_LOAD,1,0).label==1);dynamic_active=true;
    assert(calls==before && p->length==10);
    generation=8;assert(request(8,7,PRIVATE_NOTE_LOAD,1,0).label==1);
    assert(request(8,8,PRIVATE_NOTE_LOAD,1,0).label==0);
    staged("stale writer");assert(request(8,8,PRIVATE_NOTE_SAVE,2,0).label==4 && revision==1);erased(app_page,4096);
    before=calls;p->length=PRIVATE_NOTE_MAX+1;assert(request(8,8,PRIVATE_NOTE_SAVE,2,1).label==2);
    p->reserved=1;assert(request(8,8,PRIVATE_NOTE_SAVE,2,1).label==2);assert(calls==before);erased(app_page,4096);
    p->length=PRIVATE_NOTE_MAX;memset(p->text,'x',PRIVATE_NOTE_MAX);
    assert(request(8,8,PRIVATE_NOTE_SAVE,2,1).label==0 && revision==2);
    assert(request(8,8,PRIVATE_NOTE_LOAD,1,0).label==0 && p->length==PRIVATE_NOTE_MAX);
    for(unsigned i=0;i<PRIVATE_NOTE_MAX;++i)assert(p->text[i]=='x');
    stored[0]^=1;assert(request(8,8,PRIVATE_NOTE_LOAD,1,0).label==2);erased(app_page,4096);
    uint8_t record[PRIVATE_NOTE_RECORD_MAX];memset(record,0,sizeof(record));private_note_header(record,0);uint32_t length;
    assert(private_note_decode(record,16,&length) && !length);
    private_note_header(record,PRIVATE_NOTE_MAX);assert(private_note_decode(record,sizeof(record),&length));
    assert(!private_note_decode(record,15,&length));assert(!private_note_decode(record,sizeof(record)-1,&length));
    uint32_t random=1;
    for(unsigned i=0;i<10000;++i) {
        random=random*1664525u+1013904223u;unsigned at=random%16;uint8_t old=record[at];record[at]^=(random>>24)|1;
        bool valid=private_note_decode(record,random%(sizeof(record)+100),&length);
        if(valid)assert(length<=PRIVATE_NOTE_MAX);record[at]=old;
    }
    private_notes_clear();assert(!private_notes_page);erased(vault_page,4096);erased(private_notes_scratch,sizeof(private_notes_scratch));
    puts("PASS private Notes scope, stale handles, revisions, bounds, format fuzzing and scratch erasure");
}
