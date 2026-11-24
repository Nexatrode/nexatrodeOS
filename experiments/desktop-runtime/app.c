#include "runtime.h"

seL4_IPCBuffer *__sel4_ipc_buffer = (seL4_IPCBuffer *)APP_IPC;
char __sel4_print_error;
static volatile uint64_t private_bss;

static void fail(const char *why)
{
    print("RUNTIME|FAIL|app ");
    print(why);
    print("\n");
    for (;;)
        seL4_Yield();
}

static seL4_MessageInfo_t call(unsigned request, uint64_t first, uint64_t second)
{
    seL4_SetMR(0, first);
    seL4_SetMR(1, second);
    return seL4_Call(APP_SERVER, seL4_MessageInfo_new(request, 0, 0, 2));
}

__attribute__((section(".entry"))) void app_entry(uint64_t kind, uint64_t generation,
                                                  uint64_t should_fault)
{
    const volatile uint8_t *memory = (const volatile uint8_t *)APP_SCRATCH;
    if (private_bss)
        fail("old private bss");
    for (size_t i = 0; i < PAGE_BYTES; ++i)
        if (memory[i])
            fail("old scratch memory");
    /* Function calls use the top of this stack. Check its unused lower half. */
    for (size_t i = 0; i < PAGE_BYTES / 2; ++i)
        if (((const volatile uint8_t *)APP_STACK)[i])
            fail("old stack memory");
    seL4_MessageInfo_t rejected = seL4_Call(APP_SERVER, seL4_MessageInfo_new(SAVE, 0, 0, 0));
    if (seL4_MessageInfo_get_label(rejected) != 1)
        fail("malformed save accepted");
    rejected = call(0x7ff, 0, 0);
    if (seL4_MessageInfo_get_label(rejected) != 1)
        fail("unknown operation accepted");
    seL4_MessageInfo_t reply = call(LOAD, 0, 0);
    if (seL4_MessageInfo_get_label(reply) != 0 || seL4_GetMR(0) != kind)
        fail("wrong private namespace");
    uint64_t previous = seL4_GetMR(1);
    uint64_t value = 0xc0ffee0000000000ULL | generation;
    reply = call(SAVE, value, 3); /* A forged wallet ID cannot change caller identity. */
    if (seL4_MessageInfo_get_label(reply) != 0)
        fail("save");
    reply = call(LOAD, 3, 0);
    if (seL4_MessageInfo_get_label(reply) != 0 || seL4_GetMR(0) != kind || seL4_GetMR(1) != value ||
        value == previous)
        fail("reopen");
    reply = call(WALLET_READ, 3, 0);
    if (seL4_MessageInfo_get_label(reply) != 1)
        fail("wallet permission");
    private_bss = value;
    for (size_t i = 0; i < PAGE_BYTES; ++i)
        ((volatile uint8_t *)APP_SCRATCH)[i] = 0xa5;
    if (should_fault) {
        reply = call(PROBE_FAULT, 0, 0);
        if (seL4_MessageInfo_get_label(reply))
            fail("fault preparation");
        (void)*(volatile uint8_t *)PRIVATE_WALLET;
        fail("private memory readable");
    }
    (void)call(FINISHED, 0, 0);
    fail("terminated app resumed");
}
