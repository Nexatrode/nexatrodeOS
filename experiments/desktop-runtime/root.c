#include "runtime.h"
#include "platform.h"
#include "elf.h"

#ifndef RUNTIME_HOSTED
seL4_IPCBuffer *__sel4_ipc_buffer;
char __sel4_print_error;
#endif
/* Required by the image composer. The root task uses BootInfo directly. */
uint64_t sel4_phdrs_patched__vaddr;
uint16_t sel4_phdrs_patched__phnum;
extern const uint8_t app_elf_start[], app_elf_end[];

#define TEMP_ADDRESS 0x40000000UL
#define POOL_BITS 20
#ifndef MAX_OBJECTS
#define MAX_OBJECTS 128
#endif
#define MAX_SEGMENTS 8
#define MAX_PACKAGE_BYTES (128 * 1024)

struct process {
    seL4_CPtr cnode, vspace, tcb, sc, endpoint, ipc;
    seL4_CPtr start_slot, next_slot;
    uint64_t generation;
    unsigned kind;
    uint64_t scratch_physical;
};
#ifndef RUNTIME_HOSTED
static seL4_BootInfo *boot;
#endif
static seL4_CPtr manager_pool, app_pool, manager_slot, app_start, temporary_cap, escaped_cap,
    reply_cap;
static uint64_t saved[2];
static uint64_t first_scratch;
static uint8_t invalid_package[MAX_PACKAGE_BYTES] __attribute__((aligned(8)));

static void fail(const char *why, seL4_Error error)
{
    print("RUNTIME|FAIL|");
    print(why);
    print(" error=");
    seL4_DebugPutChar('0' + error);
    print("\n");
    for (;;)
        seL4_Yield();
}
static void check(seL4_Error error, const char *why)
{
    if (error != seL4_NoError)
        fail(why, error);
}
static void *copy(void *destination, const void *source, size_t size)
{
    for (size_t i = 0; i < size; ++i)
        ((uint8_t *)destination)[i] = ((const uint8_t *)source)[i];
    return destination;
}
void *memcpy(void *to, const void *from, size_t n) { return copy(to, from, n); }
void *memset(void *to, int value, size_t size)
{
    for (size_t i = 0; i < size; ++i)
        ((uint8_t *)to)[i] = value;
    return to;
}

static seL4_CPtr allocate(seL4_CPtr pool, seL4_CPtr *next, unsigned type, unsigned bits)
{
    seL4_CPtr slot = (*next)++;
    check(seL4_Untyped_Retype(pool, type, bits, ROOT_NODE, 0, 0, slot, 1), "retype");
    return slot_cap(slot);
}
static seL4_CPtr app_object(struct process *p, unsigned type, unsigned bits)
{
    if (p->next_slot - p->start_slot >= MAX_OBJECTS)
        fail("object quota", 0);
    return allocate(app_pool, &p->next_slot, type, bits);
}
static void map_with_tables(seL4_CPtr page, seL4_CPtr space, uintptr_t address,
                            seL4_CapRights_t rights, seL4_ARM_VMAttributes attributes,
                            struct process *p)
{
    for (unsigned attempt = 0; attempt < 4; ++attempt) {
        seL4_Error error = seL4_ARM_Page_Map(page, space, address, rights, attributes);
        if (error == seL4_NoError)
            return;
        if (error != seL4_FailedLookup)
            fail("map page", error);
        seL4_CPtr table = p ? app_object(p, seL4_ARM_PageTableObject, 0)
                            : allocate(manager_pool, &manager_slot, seL4_ARM_PageTableObject, 0);
        check(seL4_ARM_PageTable_Map(table, space, address, seL4_ARM_Default_VMAttributes),
              "map table");
    }
    fail("page table depth", 0);
}
static void *temporary_mapping(seL4_CPtr frame)
{
    check(seL4_CNode_Copy(ROOT_NODE, temporary_cap, ROOT_DEPTH,
                          ROOT_NODE, cap_slot(frame), ROOT_DEPTH, seL4_AllRights),
          "temporary copy");
    map_with_tables(slot_cap(temporary_cap), ROOT_VSPACE, TEMP_ADDRESS, seL4_AllRights,
                    seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
    return (void *)TEMP_ADDRESS;
}
static void unmap_temporary(void)
{
    check(seL4_ARM_Page_Unmap(slot_cap(temporary_cap)), "temporary unmap");
    check(seL4_CNode_Delete(ROOT_NODE, temporary_cap, ROOT_DEPTH),
          "temporary delete");
}
static seL4_CPtr frame(struct process *p, uintptr_t address, const void *bytes, size_t length,
                       unsigned flags)
{
    seL4_CPtr cap = app_object(p, seL4_ARM_SmallPageObject, 0);
#ifdef RUNTIME_TRACK_FRAME
    RUNTIME_TRACK_FRAME(cap, PAGE_BYTES);
#endif
    uint8_t *buffer = temporary_mapping(cap);
    for (size_t i = 0; i < PAGE_BYTES; ++i)
        if (buffer[i])
            fail("retyped memory not zero", 0);
    if (length)
        copy(buffer, bytes, length);
    if (flags & 1)
        check(seL4_ARM_Page_Unify_Instruction(slot_cap(temporary_cap), 0, PAGE_BYTES), "instruction flush");
    unmap_temporary();
    seL4_CapRights_t rights = seL4_CapRights_new(0, 0, 1, (flags & 2) != 0);
    map_with_tables(cap, p->vspace, address, rights,
                    seL4_ARM_Default_VMAttributes | ((flags & 1) ? 0 : seL4_ARM_ExecuteNever), p);
    return cap;
}

static bool valid_image(const uint8_t *bytes, size_t length)
{
    if (length < sizeof(struct elf_header))
        return false;
    const struct elf_header *h = (const void *)bytes;
    if (h->ident[0] != 0x7f || h->ident[1] != 'E' || h->ident[2] != 'L' || h->ident[3] != 'F' ||
        h->ident[4] != 2 || h->ident[5] != 1 || h->ident[6] != 1 || h->version != 1 ||
        h->ehsize != sizeof(*h) || h->type != 2 || h->machine != 183 || h->entry % 4 ||
        h->phentsize != sizeof(struct program_header) || h->phoff < sizeof(*h) || h->phnum == 0 ||
        h->phnum > MAX_SEGMENTS || h->phoff > length ||
        h->phnum > (length - h->phoff) / sizeof(struct program_header))
        return false;
    bool entry_found = false;
    uintptr_t previous_end = APP_BASE;
    for (unsigned i = 0; i < h->phnum; ++i) {
        struct program_header ph;
        copy(&ph, bytes + h->phoff + i * sizeof(ph), sizeof(ph));
        if (ph.type == 2 || ph.type == 3)
            return false;
        if (ph.type != 1)
            continue;
        if (ph.memsz == 0 && ph.filesz != 0)
            return false;
        if (ph.memsz == 0)
            continue;
        if (ph.filesz > ph.memsz || ph.offset > length || ph.filesz > length - ph.offset ||
            ph.address < previous_end || ph.address >= APP_LIMIT ||
            ph.memsz > APP_LIMIT - ph.address || ph.address % PAGE_BYTES || (ph.flags & 3) == 3 ||
            !(ph.flags & 4) || (ph.flags & ~7U))
            return false;
        previous_end = (ph.address + ph.memsz + PAGE_BYTES - 1) & ~(PAGE_BYTES - 1UL);
        if ((ph.flags & 1) && h->entry >= ph.address && h->entry - ph.address < ph.filesz)
            entry_found = true;
    }
    return entry_found;
}

static void check_image_validation(size_t size)
{
    if (size > sizeof(invalid_package) || !valid_image(app_elf_start, size) ||
        valid_image(app_elf_start, 7))
        fail("ELF validation", 0);
    copy(invalid_package, app_elf_start, size);
    struct elf_header *header = (void *)invalid_package;
    uint64_t offset = header->phoff;
    header->phoff = UINT64_MAX;
    if (valid_image(invalid_package, size))
        fail("overflowing ELF accepted", 0);
    header->phoff = offset;
    struct program_header original;
    copy(&original, invalid_package + offset, sizeof(original));
    struct program_header *segment = (void *)(invalid_package + offset);
    segment->flags = 7;
    if (valid_image(invalid_package, size))
        fail("writable executable segment accepted", 0);
    *segment = original;
    segment->offset = UINT64_MAX;
    if (valid_image(invalid_package, size))
        fail("outside payload accepted", 0);
    *segment = original;
    segment->filesz = segment->memsz + 1;
    if (valid_image(invalid_package, size))
        fail("invalid segment lengths accepted", 0);
    *segment = original;
    segment->memsz = UINT64_MAX;
    if (valid_image(invalid_package, size))
        fail("overflowing segment accepted", 0);
    print("RUNTIME|PASS|bounded ELF validation\n");
}

static void destroy(struct process *p)
{
    if (p->tcb)
        check(seL4_TCB_Suspend(p->tcb), "stop");
#ifdef RUNTIME_HOSTED
    if (p->tcb)
        check(seL4_SchedContext_UnbindObject(p->sc, p->tcb), "release CPU budget");
#endif
#ifndef CONTROL_SKIP_REVOKE
    check(seL4_CNode_Revoke(ROOT_NODE, cap_slot(app_pool), ROOT_DEPTH), "revoke app pool");
#endif
    for (seL4_CPtr slot = p->start_slot; slot < p->next_slot; ++slot)
        if (seL4_DebugCapIdentify(slot_cap(slot)))
            fail("object capability survived", 0);
    if (seL4_DebugCapIdentify(slot_cap(escaped_cap)))
        fail("escaped capability survived", 0);
    p->tcb = 0;
}
static void create(struct process *p, unsigned kind, unsigned generation, unsigned interruption)
{
    *p = (struct process){
        .start_slot = app_start, .next_slot = app_start, .kind = kind, .generation = generation};
    p->cnode = app_object(p, seL4_CapTableObject, APP_CNODE_BITS);
    p->vspace = app_object(p, seL4_ARM_VSpaceObject, 0);
    check(seL4_ARM_ASIDPool_Assign(ROOT_ASID, p->vspace), "asid");
#ifdef CONTROL_KEEP_ESCAPED
    p->endpoint = allocate(manager_pool, &manager_slot, seL4_EndpointObject, 0);
#else
    p->endpoint = app_object(p, seL4_EndpointObject, 0);
#endif
    /* This alias lives outside the process's own capability space. */
    check(seL4_CNode_Copy(ROOT_NODE, escaped_cap, ROOT_DEPTH,
                          ROOT_NODE, cap_slot(p->endpoint), ROOT_DEPTH, seL4_AllRights),
          "escape copy");
    check(seL4_CNode_Mint(p->cnode, APP_SERVER, APP_CNODE_BITS, ROOT_NODE,
                          cap_slot(p->endpoint), ROOT_DEPTH, seL4_AllRights, generation),
          "client endpoint");
    if (interruption == 1)
        return;
    const struct elf_header *h = (const void *)app_elf_start;
    for (unsigned i = 0; i < h->phnum; ++i) {
        struct program_header ph;
        copy(&ph, app_elf_start + h->phoff + i * sizeof(ph), sizeof(ph));
        if (ph.type != 1)
            continue;
        for (size_t offset = 0; offset < ph.memsz; offset += PAGE_BYTES) {
            size_t bytes = offset < ph.filesz ? ph.filesz - offset : 0;
            if (bytes > PAGE_BYTES)
                bytes = PAGE_BYTES;
            frame(p, ph.address + offset, bytes ? app_elf_start + ph.offset + offset : NULL, bytes,
                  ph.flags);
        }
    }
    frame(p, APP_STACK, NULL, 0, 6);
    p->ipc = frame(p, APP_IPC, NULL, 0, 6);
    seL4_CPtr scratch = frame(p, APP_SCRATCH, NULL, 0, 6);
    seL4_ARM_Page_GetAddress_t physical = seL4_ARM_Page_GetAddress(scratch);
    check(physical.error, "scratch physical");
    p->scratch_physical = physical.paddr;
    if (interruption == 2)
        return;
    p->tcb = app_object(p, seL4_TCBObject, 0);
#ifdef RUNTIME_HOSTED
    p->sc = HOSTED_SC;
#else
    p->sc = app_object(p, seL4_SchedContextObject, seL4_MinSchedContextBits);
    check(seL4_SchedControl_ConfigureFlags(boot->schedcontrol.start, p->sc, 1000, 10000, 0, 0, 0),
          "schedule");
#endif
    check(seL4_TCB_Configure(p->tcb, p->cnode,
                             seL4_CNode_CapData_new(0, seL4_WordBits - APP_CNODE_BITS).words[0],
                             p->vspace, 0, APP_IPC, p->ipc),
          "configure thread");
    check(seL4_TCB_SetSchedParams(p->tcb, ROOT_TCB, 100, 100, p->sc, p->endpoint),
          "priority");
}

static void serve(struct process *p, bool fault_expected)
{
    const struct elf_header *h = (const void *)app_elf_start;
    seL4_UserContext regs = {.pc = h->entry,
                             .sp = APP_STACK + PAGE_BYTES,
                             .x0 = p->kind,
                             .x1 = p->generation,
                             .x2 = fault_expected};
    check(seL4_TCB_WriteRegisters(p->tcb, true, 0, sizeof(regs) / sizeof(seL4_Word), &regs),
          "start");
    unsigned progress = 0;
    for (;;) {
        seL4_Word badge;
        seL4_MessageInfo_t request = seL4_Recv(p->endpoint, &badge, reply_cap);
        unsigned label = seL4_MessageInfo_get_label(request);
        if (label == seL4_Fault_VMFault) {
            if (!fault_expected || progress != 5 ||
                seL4_GetMR(seL4_VMFault_Addr) != PRIVATE_WALLET ||
                seL4_GetMR(seL4_VMFault_PrefetchFault))
                fail("unexpected app fault", 0);
            return;
        }
        if (badge != p->generation)
            fail("bad caller", 0);
        if (seL4_MessageInfo_get_length(request) != 2 || label < SAVE || label > PROBE_FAULT) {
            seL4_Send(reply_cap, seL4_MessageInfo_new(1, 0, 0, 0));
            continue;
        }
        if (label == FINISHED) {
            if (fault_expected || progress != 4)
                fail("early finish", 0);
            return;
        }
        unsigned result = 0;
        switch (label) {
        case LOAD:
            if (progress != 0 && progress != 2)
                fail("load order", 0);
            seL4_SetMR(0, p->kind);
            seL4_SetMR(1, saved[p->kind - 1]);
            break;
        case SAVE:
            if (progress != 1)
                fail("save order", 0);
            saved[p->kind - 1] = seL4_GetMR(0);
            break;
        case WALLET_READ:
            if (progress != 3)
                fail("wallet request order", 0);
            result = 1;
            break;
        case PROBE_FAULT:
            if (!fault_expected || progress != 4)
                fail("fault order", 0);
            break;
        default:
            fail("unknown request", 0);
        }
        progress++;
        seL4_Send(reply_cap, seL4_MessageInfo_new(result, 0, 0, 2));
    }
}

void root_main(seL4_BootInfo *info)
{
#ifdef RUNTIME_HOSTED
    (void)info;
    print("RUNTIME|BOOT|Microkit-hosted manager\n");
    manager_slot = 1;
    seL4_CPtr source = HOSTED_POOL;
    check(seL4_TCB_Suspend(HOSTED_BUDGET_TCB), "stop budget placeholder");
    check(seL4_SchedContext_UnbindObject(HOSTED_SC, HOSTED_BUDGET_TCB), "take reserved CPU budget");
#else
    boot = info;
    __sel4_ipc_buffer = info->ipcBuffer;
    print("RUNTIME|BOOT|dedicated root task\n");
    manager_slot = info->empty.start;
    seL4_CPtr source = 0;
    for (unsigned i = 0; i < info->untyped.end - info->untyped.start; ++i)
        if (!info->untypedList[i].isDevice && info->untypedList[i].sizeBits >= POOL_BITS + 1) {
            source = info->untyped.start + i;
            break;
        }
    if (!source)
        fail("no resource pool", 0);
#endif
    manager_pool = slot_cap(manager_slot++);
    app_pool = slot_cap(manager_slot++);
    check(seL4_Untyped_Retype(source, seL4_UntypedObject, POOL_BITS, ROOT_NODE, 0, 0,
                              cap_slot(manager_pool), 2),
          "reserve pools");
    temporary_cap = manager_slot++;
    escaped_cap = manager_slot++;
    reply_cap = allocate(manager_pool, &manager_slot, seL4_ReplyObject, 0);
    seL4_CPtr wallet = allocate(manager_pool, &manager_slot, seL4_ARM_SmallPageObject, 0);
    map_with_tables(wallet, ROOT_VSPACE, PRIVATE_WALLET, seL4_AllRights,
                    seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
    *(volatile uint64_t *)PRIVATE_WALLET = 0x123456789abcdef0ULL;
    /* Allocate the manager's temporary page tables before fixing the app slot range. */
    temporary_mapping(wallet);
    unmap_temporary();
    app_start = manager_slot + 8;
    if (app_start + MAX_OBJECTS >=
#ifdef RUNTIME_HOSTED
        4096
#else
        info->empty.end
#endif
        )
        fail("root slot capacity", 0);
    size_t app_size = app_elf_end - app_elf_start;
    check_image_validation(app_size);
    struct process process;
    for (unsigned stage = 1; stage <= 3; ++stage) {
        create(&process, CHESS, 1, stage);
        destroy(&process);
    }
    print("RUNTIME|PASS|interrupted creation reclaimed\n");
    create(&process, CHESS, 1, 2);
    seL4_CPtr quota_slot = process.next_slot++;
    seL4_Error quota = seL4_Untyped_Retype(app_pool, seL4_ARM_LargePageObject, 0,
                                           ROOT_NODE, 0, 0, quota_slot, 1);
    if (quota != seL4_NotEnoughMemory)
        fail("resource quota did not reject", quota);
    destroy(&process);
    print("RUNTIME|PASS|allocation failure reclaimed\n");
    for (unsigned iteration = 1; iteration <= CYCLES; ++iteration) {
        unsigned kind = iteration % 2 ? CHESS : SUCCESSOR;
        create(&process, kind, iteration, 0);
        if (!first_scratch)
            first_scratch = process.scratch_physical;
        else if (first_scratch != process.scratch_physical)
            fail("physical resource reuse", 0);
        serve(&process, iteration % 3 == 0);
        destroy(&process);
        if (*(volatile uint64_t *)PRIVATE_WALLET != 0x123456789abcdef0ULL)
            fail("wallet modified", 0);
    }
    print("RUNTIME|PASS|repeated launches with private saves and wallet denial\n");
    print("RUNTIME|PASS|fault exit and normal exit reclaimed\n");
    print("RUNTIME|PASS|physical memory reused and zeroed\n");
    print("RUNTIME|PASS|escaped endpoint revoked\n");
    print("RUNTIME|PASS|complete\n");
#ifndef RUNTIME_HOSTED
    for (;;)
        seL4_Yield();
#endif
}

#if defined(RUNTIME_HOSTED) && !defined(RUNTIME_NO_ENTRY)
void init(void)
{
    root_main(NULL);
    microkit_notify(0);
}
void notified(microkit_channel channel) { (void)channel; }
#endif
