/* No auth, vault-key, app-memory, GPU or network authority. */
#include <microkit.h>
#include "lock_protocol.h"
uintptr_t desktop_lock_state;
static uint64_t epoch;
static unsigned phase;
void init(void) {microkit_dbg_puts("TRIO_PROBE|READY\n");}
void notified(microkit_channel channel) {
    (void)channel;const struct trio_status *s=(void*)desktop_lock_state;
    uint32_t r=atomic_load(&s->revision);uint64_t e=s->epoch;unsigned p=s->phase;
    atomic_thread_fence(memory_order_acquire);
    if(!r||(r&1)||r!=atomic_load(&s->revision)||(epoch==e&&phase==p))return;
    epoch=e;phase=p;
    for(unsigned label=LOCK_UNLOCK;label<=LOCK_ABORT;++label){
        microkit_mr_set(0,epoch);
        microkit_msginfo reply=microkit_ppcall(0,microkit_msginfo_new(label,1));
        if(microkit_msginfo_get_label(reply)!=1||microkit_msginfo_get_count(reply)){
            microkit_dbg_puts("TRIO_PROBE|FAIL|foreign auth accepted\n");return;
        }
    }
    microkit_dbg_puts("TRIO_PROBE|FOREIGN_AUTH_DENIED\n");
}
