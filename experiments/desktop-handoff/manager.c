#include "app.h"
#include <microkit.h>

extern const uint8_t chess_start[], chess_end[], next_start[], next_end[];
enum phase { WAIT_WALLET, CHESS, NEXT, FINAL_READ, FINISHED };
static enum phase phase;
static volatile uint8_t *const code = (volatile uint8_t *)IMAGE_ADDR;
static volatile uint8_t *const scratch = (volatile uint8_t *)RAM_ADDR;
uintptr_t chess_launch, next_launch;

static int start(unsigned child, const uint8_t *bytes, size_t size, uintptr_t launch)
{
    if (size == 0 || size > IMAGE_BYTES)
        return 0;
#ifdef CONTROL_KEEP_CODE_TAIL
    for (size_t i = 0; i < size; ++i)
        code[i] = bytes[i];
#else
    for (size_t i = 0; i < IMAGE_BYTES; ++i)
        code[i] = i < size ? bytes[i] : 0;
#endif
    *(volatile uint32_t *)launch = (uint32_t)size;
    if (seL4_ARM_VSpace_Unify_Instruction(microkit_cspace_root_slot_to_cptr(1), IMAGE_ADDR,
                                          IMAGE_ADDR + IMAGE_BYTES) != seL4_NoError)
        return 0;
    microkit_notify(child);
    return 1;
}

void init(void) {}
static void replace_chess(void)
{
    microkit_pd_stop(1);
    for (size_t i = 0; i < RAM_BYTES; ++i) {
        if (scratch[i] != 0xa5) {
            microkit_dbg_puts("HANDOFF|FAIL|chess did not dirty entire shared arena\n");
            return;
        }
    }
    if (code[IMAGE_BYTES - 1] != 0xcb) {
        microkit_dbg_puts("HANDOFF|FAIL|missing old code tail\n");
        return;
    }
#ifndef CONTROL_KEEP_SCRATCH
    for (size_t i = 0; i < RAM_BYTES; ++i)
        scratch[i] = 0;
#endif
    phase = NEXT;
#ifdef CONTROL_KEEP_IDENTITY
    if (!start(1, next_start, next_end - next_start, chess_launch)) {
        microkit_dbg_puts("HANDOFF|FAIL|publishing replacement\n");
        return;
    }
    microkit_pd_resume(1);
#else
    if (!start(2, next_start, next_end - next_start, next_launch))
        microkit_dbg_puts("HANDOFF|FAIL|publishing replacement\n");
#endif
}

void notified(microkit_channel ch)
{
    if (ch == 0 && phase == WAIT_WALLET) {
        phase = CHESS;
        if (!start(1, chess_start, chess_end - chess_start, chess_launch))
            microkit_dbg_puts("HANDOFF|FAIL|publishing chess\n");
#ifdef CONTROL_KEEP_IDENTITY
    } else if (ch == 1 && phase == CHESS) {
        replace_chess();
#endif
    } else if (ch == 0 && phase == FINAL_READ) {
        phase = FINISHED;
        microkit_dbg_puts("HANDOFF|PASS|complete\n");
    } else {
        microkit_dbg_puts("HANDOFF|FAIL|unexpected notification\n");
    }
}

seL4_Bool fault(microkit_child child, microkit_msginfo info, microkit_msginfo *reply)
{
    (void)reply;
    uintptr_t expected = child == 1 ? WALLET_PAGE : CHESS_PAGE;
    if (!((child == 1 && phase == CHESS) || (child == 2 && phase == NEXT)) ||
        microkit_msginfo_get_label(info) != seL4_Fault_VMFault ||
        microkit_msginfo_get_count(info) != seL4_VMFault_Length ||
        microkit_mr_get(seL4_VMFault_Addr) != expected ||
        microkit_mr_get(seL4_VMFault_PrefetchFault)) {
        microkit_dbg_puts("HANDOFF|FAIL|unexpected guest fault\n");
        return seL4_False;
    }
    if (child == 1) {
        microkit_dbg_puts("HANDOFF|PASS|chess cannot read wallet page\n");
        replace_chess();
    } else {
        microkit_pd_stop(child);
        microkit_dbg_puts("HANDOFF|PASS|successor cannot read chess transfer page\n");
        phase = FINAL_READ;
        microkit_notify(0);
    }
    return seL4_False;
}
