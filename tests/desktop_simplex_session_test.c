/* Execute the real parent lifecycle code with observable kernel stubs. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef uintptr_t seL4_Word;
typedef struct { uintptr_t pc, sp, registers[32]; } seL4_UserContext;
#define BASE_TCB_CAP 202
#define ROOT_VSPACE 1
#ifndef TEST_CHILDREN
#define TEST_CHILDREN 6
#endif
#ifndef TEST_SC_BASE
#define TEST_SC_BASE 16
#endif
static uint8_t private_pages[TEST_CHILDREN][8192] __attribute__((aligned(4096)));
static uint8_t shared[8192] __attribute__((aligned(4096)));
static uint64_t state_words[2], lock_epoch=1;
static unsigned stopped, resumed, polls, cleared, tls_cleared, cache_ops, scrub_logs;
static seL4_UserContext contexts[TEST_CHILDREN];
static bool bound[TEST_CHILDREN];
static bool scheduled[TEST_CHILDREN];
static unsigned budget_detaches,budget_restores;
static unsigned index_of(uintptr_t cap) { assert(cap>=234 && cap<234+TEST_CHILDREN); return cap-234; }
static int seL4_TCB_Suspend(uintptr_t cap) { (void)index_of(cap); assert(!state_words[1]); stopped++; return 0; }
static int seL4_TCB_Resume(uintptr_t cap) { unsigned i=index_of(cap); assert(state_words[1] && bound[i] && scheduled[i]); assert(contexts[i].pc==(i+1)*0x10000); resumed++;return 0; }
static int seL4_TCB_UnbindNotification(uintptr_t cap) { unsigned i=index_of(cap);assert(bound[i]);bound[i]=false;return 0; }
static int seL4_TCB_BindNotification(uintptr_t cap,uintptr_t notification) { unsigned i=index_of(cap);assert(!bound[i] && notification==((8UL+i)<<58));bound[i]=true;return 0; }
static int seL4_SchedContext_Unbind(uintptr_t sc) { unsigned i=(sc>>58)-TEST_SC_BASE;assert(i<TEST_CHILDREN && !state_words[1]);scheduled[i]=false;budget_detaches++;return 0; }
static int seL4_SchedContext_Bind(uintptr_t sc,uintptr_t tcb) { unsigned i=(sc>>58)-TEST_SC_BASE;assert(i<TEST_CHILDREN && !scheduled[i] && index_of(tcb)==i);scheduled[i]=true;budget_restores++;return 0; }
static int seL4_TCB_WriteRegisters(uintptr_t cap,bool resume,unsigned flags,size_t count,seL4_UserContext *context) {
    assert(!resume && !flags && count==sizeof(*context)/sizeof(seL4_Word));
    contexts[index_of(cap)]=*context;if(!context->pc){assert(!state_words[1]);cleared++;}return 0;
}
static int seL4_TCB_SetTLSBase(uintptr_t cap,uintptr_t base) { (void)index_of(cap);assert(!base);tls_cleared++;return 0; }
static void seL4_Poll(uintptr_t cap,seL4_Word *badge) { assert(cap>=((uintptr_t)8<<58) && cap<=((uintptr_t)(7+TEST_CHILDREN)<<58));assert(!bound[(cap>>58)-8]);*badge=0;polls++; }
static int seL4_ARM_VSpace_CleanInvalidate_Data(uintptr_t space,uintptr_t start,uintptr_t end) { assert(space==1 && start<end && (start>>12)==((end-1)>>12));cache_ops++;return 0; }
static int seL4_ARM_VSpace_Clean_Data(uintptr_t space,uintptr_t start,uintptr_t end) { return seL4_ARM_VSpace_CleanInvalidate_Data(space,start,end); }
static void check(int error,const char *where) { (void)where;assert(!error); }
static void fail(const char *where,int error) { fprintf(stderr,"%s: %d\n",where,error);abort(); }
static void copy(void *out,const void *in,size_t n) {memcpy(out,in,n);}
static void print(const char *s) { assert(!strcmp(s,"SIMPLEX_SESSION|BOOT_LOCKED\n")||!strcmp(s,"SIMPLEX_SESSION|ERASURE_FAILED\n")); }
static void line(const char *s,uint64_t n) {
    if(!strcmp(s,"SIMPLEX_SESSION|SCRUBBED|bytes=")){assert(n==sizeof(private_pages)+sizeof(shared));scrub_logs++;}
    else {assert(!strcmp(s,"SIMPLEX_SESSION|STARTED|epoch="));assert(n==lock_epoch);}
}
static bool inject_fault;
#define SESSION_TEST_ERASURE_FAULT(p) do { if (inject_fault) (p)[0]=1; } while (0)
#include "../experiments/desktop-simplex/session.h"
static void all_zero(const uint8_t *p,size_t n) {for(size_t i=0;i<n;i++)assert(p[i]==0);}
int main(void) {
    for(unsigned i=0;i<TEST_CHILDREN;i++){
        memcpy(private_pages[i],"public boot metadata",20);bound[i]=scheduled[i]=true;
        session_children[i]=(struct session_child){(uintptr_t)private_pages[i],2,(i+1)*0x10000};
    }
    memset(shared,0x7b,sizeof(shared));
    simplex_session_boot();
    assert(session_boot_used==TEST_CHILDREN && !session_failed && scrub_logs==1);
    all_zero((void *)private_pages,sizeof(private_pages));all_zero(shared,sizeof(shared));
    for(unsigned round=0;round<8;round++) {
        assert(simplex_session_start());
        all_zero(shared,sizeof(shared));
        for(unsigned i=0;i<TEST_CHILDREN;i++) {
            assert(!memcmp(private_pages[i],"public boot metadata",20));
            all_zero(private_pages[i]+20,8192-20);
            memset(private_pages[i],0xa5+i,sizeof(private_pages[i]));
        }
        memset(shared,0x5a,sizeof(shared));
        ++lock_epoch;simplex_session_suspend();simplex_session_scrub();
        assert(!state_words[1] && state_words[0]==lock_epoch);
        all_zero((void *)private_pages,sizeof(private_pages));all_zero(shared,sizeof(shared));
    }
    assert(budget_detaches==TEST_CHILDREN*9 && budget_restores==TEST_CHILDREN*8);
    assert(stopped==TEST_CHILDREN*9 && resumed==TEST_CHILDREN*8 && cleared==TEST_CHILDREN*9 && tls_cleared==TEST_CHILDREN*9 && polls==TEST_CHILDREN*9);
    assert(simplex_session_start());++lock_epoch;simplex_session_suspend();inject_fault=true;
    simplex_session_scrub();assert(session_failed);
    assert(!simplex_session_start() && resumed==TEST_CHILDREN*9);
    puts("PASS parent session: restart/scrub cycles, shared-data snapshot exclusion, fresh contexts, stale wakes, retained-byte failure latch");
}
