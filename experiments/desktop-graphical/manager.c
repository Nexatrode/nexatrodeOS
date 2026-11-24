/* Disposable one-dynamic-window lifecycle experiment. The other window is static. */
#ifdef DYNAMIC_LOCK
#include <sel4/sel4.h>
#include <stddef.h>
static void track_private_frame(seL4_CPtr cap, size_t bytes);
#define RUNTIME_TRACK_FRAME track_private_frame
#define SURFACE_LOCK
#endif
#define RUNTIME_HOSTED
#define RUNTIME_NO_ENTRY
#ifdef DESKTOP_SHELL
#define MAX_OBJECTS 2048
#else
#define MAX_OBJECTS 1024
#endif
#include "../desktop-runtime/root.c"
#define SURFACE_INTERACTIVE
#define SURFACE_DYNAMIC
#define init compositor_init
#define notified compositor_notified
#define fault compositor_fault
#ifdef DESKTOP_SHELL
#include "../desktop-shell/compositor.c"
#else
#include "../desktop-surfaces/compositor.c"
#endif
#undef init
#undef notified
#undef fault
#include "image.h"
#ifdef DYNAMIC_SIMPLEX_SESSION
#include "session_layout.h"
static void simplex_session_boot(void);
static void simplex_session_fault(void);
#endif
#ifdef DYNAMIC_LOCK
#include "../desktop-lock/protocol.h"
static bool device_locked = true;
static void begin_device_lock(bool force);
static void lock_timer(void);
static void publish_lock_status(void);
static void scrub_private_frames(void);
#ifdef DYNAMIC_SIMPLEX_FRONTEND
static uint32_t frontend_revision;
static bool frontend_input(struct input_event_record);
static bool frontend_compose(void);
static void frontend_notify(void);
#endif
static unsigned private_frame_count;
#ifdef DYNAMIC_WRAPPED_LOCK
static bool evict_vault(void);
#endif
#endif
#ifdef DYNAMIC_PRIVATE_NOTES
#include "../desktop-notes/protocol.h"
static uintptr_t private_notes_page;
static void private_notes_clear(void);
static bool private_notes_control(struct input_event_record event);
static microkit_msginfo private_notes_request(microkit_channel, microkit_msginfo);
#endif

extern const uint8_t gui_elf_start[], gui_elf_end[];
#define GUI_HEAP 0x64000000UL
#define GUI_INPUT 0x66000000UL
#define GUI_BOOT 0x68000000UL
#define GUI_IPC (seL4_UserVSpaceTop & ~4095UL)
#define GUI_STACK_TOP (GUI_IPC - 4096)
#define LARGE_TEMP 0x40400000UL
#define LARGE_BYTES 0x200000UL
#define GUI_CALL_BADGE (1UL << 63)

static struct process gui;
static seL4_CPtr input_signal, attack_signal, fault_endpoint;
static uint64_t generation, original_surface;
static unsigned pending_action;
static bool first_frame, fault_requested;

static void line(const char *prefix, uint64_t value)
{
    char digits[21]; unsigned count = 0;
    do { digits[count++] = '0' + value % 10; value /= 10; } while (value);
    print(prefix);
    while (count) seL4_DebugPutChar(digits[--count]);
    print("\n");
}

static seL4_CPtr alias(struct process *p, seL4_CPtr source)
{
    if (p->next_slot - p->start_slot >= MAX_OBJECTS) fail("alias quota", 0);
    seL4_CPtr slot = p->next_slot++;
    check(seL4_CNode_Copy(ROOT_NODE, slot, ROOT_DEPTH, ROOT_NODE, cap_slot(source),
                          ROOT_DEPTH, seL4_AllRights), "mapping alias");
    return slot_cap(slot);
}

static void mint_local(struct process *p, seL4_CPtr source, seL4_CPtr *result, uint64_t badge)
{
    if (p->next_slot - p->start_slot >= MAX_OBJECTS) fail("notification quota", 0);
    seL4_CPtr slot = p->next_slot++;
    check(seL4_CNode_Mint(ROOT_NODE, slot, ROOT_DEPTH, ROOT_NODE, cap_slot(source), ROOT_DEPTH,
                          seL4_CapRights_new(0, 0, 0, 1), badge), "notification alias");
    *result = slot_cap(slot);
}

static seL4_CPtr large_page(struct process *p, uintptr_t address)
{
    seL4_CPtr cap = app_object(p, seL4_ARM_LargePageObject, 0);
#ifdef DYNAMIC_LOCK
    track_private_frame(cap, LARGE_BYTES);
    if (address == GUI_HEAP) {
        seL4_ARM_Page_GetAddress_t physical = seL4_ARM_Page_GetAddress(cap);
        check(physical.error, "private heap address");
        line("LOCK|TEST_HEAP_PHYSICAL|base=", physical.paddr);
    }
#endif
    check(seL4_CNode_Copy(ROOT_NODE, temporary_cap, ROOT_DEPTH, ROOT_NODE, cap_slot(cap),
                          ROOT_DEPTH, seL4_AllRights), "large temporary copy");
    map_with_tables(slot_cap(temporary_cap), ROOT_VSPACE, LARGE_TEMP, seL4_AllRights,
                    seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
    for (size_t i = 0; i < LARGE_BYTES; ++i)
        if (((const volatile uint8_t *)LARGE_TEMP)[i]) fail("old graphical RAM", 0);
    unmap_temporary();
    map_with_tables(cap, p->vspace, address, seL4_AllRights,
                    seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, p);
    return cap;
}

static void image_page(struct process *p, const struct program_header *ph, uintptr_t address)
{
    seL4_CPtr cap = app_object(p, seL4_ARM_SmallPageObject, 0);
#ifdef DYNAMIC_LOCK
    track_private_frame(cap, PAGE_BYTES);
#endif
    uint8_t *buffer = temporary_mapping(cap);
    for (size_t i = 0; i < PAGE_BYTES; ++i)
        if (buffer[i]) fail("old graphical image RAM", 0);
    uint64_t start = address > ph->address ? address : ph->address;
    uint64_t end = ph->address + ph->filesz;
    if (end > address + PAGE_BYTES) end = address + PAGE_BYTES;
    if (start < end) copy(buffer + start - address, gui_elf_start + ph->offset + start - ph->address, end - start);
    if (ph->flags & 1)
        check(seL4_ARM_Page_Unify_Instruction(slot_cap(temporary_cap), 0, PAGE_BYTES), "GUI instruction flush");
    unmap_temporary();
    map_with_tables(cap, p->vspace, address, seL4_CapRights_new(0, 0, 1, (ph->flags & 2) != 0),
                    seL4_ARM_Default_VMAttributes | ((ph->flags & 1) ? 0 : seL4_ARM_ExecuteNever), p);
}

static void launch(void)
{
#ifdef DYNAMIC_SIMPLEX_FRONTEND
    /* The real UI is already a managed session child; no demo Notes process. */
    return;
#endif
#ifdef DYNAMIC_LOCK
    if (device_locked) return;
    if (generation >= 59) { print("DYNAMIC|LIMIT|instance generation quota\n"); return; }
#endif
    if (dynamic_active) return;
#ifdef DYNAMIC_LOCK
    private_frame_count = 0;
#endif
    if (generation >=
#ifdef DYNAMIC_SIMPLEX_SESSION
        31
#else
        61
#endif
       ) { print("DYNAMIC|LIMIT|instance generation quota\n"); return; }
    size_t length = gui_elf_end - gui_elf_start;
    if (!graphical_image_valid(gui_elf_start, length)) fail("graphical executable policy", 0);
    gui = (struct process){.start_slot = app_start, .next_slot = app_start,
                           .generation = ++generation, .sc = HOSTED_SC};
    gui.cnode = app_object(&gui, seL4_CapTableObject, APP_CNODE_BITS);
    gui.vspace = app_object(&gui, seL4_ARM_VSpaceObject, 0);
    check(seL4_ARM_ASIDPool_Assign(ROOT_ASID, gui.vspace), "GUI ASID");
    /* An instance lease derives from the permanent manager endpoint. */
    gui.endpoint = slot_cap(gui.next_slot++);
    check(seL4_CNode_Copy(ROOT_NODE, cap_slot(gui.endpoint), ROOT_DEPTH, ROOT_NODE, 0, ROOT_DEPTH,
                          seL4_AllRights), "instance endpoint lease");
    check(seL4_CNode_Copy(ROOT_NODE, escaped_cap, ROOT_DEPTH, ROOT_NODE, cap_slot(gui.endpoint),
                          ROOT_DEPTH, seL4_AllRights), "escaped GUI endpoint");
    check(seL4_CNode_Mint(gui.cnode, 74, APP_CNODE_BITS, ROOT_NODE, cap_slot(gui.endpoint), ROOT_DEPTH,
                          seL4_CapRights_new(1, 0, 0, 1), GUI_CALL_BADGE | generation), "GUI service endpoint");
    fault_endpoint = slot_cap(gui.next_slot++);
    check(seL4_CNode_Mint(ROOT_NODE, cap_slot(fault_endpoint), ROOT_DEPTH, ROOT_NODE,
                          cap_slot(gui.endpoint), ROOT_DEPTH, seL4_CapRights_new(1, 0, 0, 1),
                          (1UL << 62) | generation), "GUI fault endpoint");
    seL4_CPtr notification = app_object(&gui, seL4_NotificationObject, 0);
    check(seL4_CNode_Copy(gui.cnode, 1, APP_CNODE_BITS, ROOT_NODE, cap_slot(notification), ROOT_DEPTH,
                          seL4_CapRights_new(0, 0, 1, 0)), "GUI event receiver");
    seL4_CPtr child_reply = app_object(&gui, seL4_ReplyObject, 0);
    check(seL4_CNode_Copy(gui.cnode, 4, APP_CNODE_BITS, ROOT_NODE, cap_slot(child_reply), ROOT_DEPTH,
                          seL4_AllRights), "GUI reply object");
    mint_local(&gui, notification, &input_signal, 1UL << 2);
    mint_local(&gui, notification, &attack_signal, 1UL << 1);

    for (uintptr_t offset = 0; offset < 0x800000; offset += LARGE_BYTES)
        large_page(&gui, GUI_HEAP + offset);
    seL4_CPtr surface = large_page(&gui, 0x60000000);
    seL4_ARM_Page_GetAddress_t physical = seL4_ARM_Page_GetAddress(surface);
    check(physical.error, "GUI physical address");
    if (original_surface && original_surface != physical.paddr) fail("GUI physical reuse", 0);
    original_surface = physical.paddr;
    map_with_tables(alias(&gui, surface), ROOT_VSPACE, surface_a, seL4_CapRights_new(0, 0, 1, 0),
                    seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
#ifdef DESKTOP_SHELL
    for (uintptr_t offset = LARGE_BYTES; offset < sizeof(struct surface_header) + APP_BYTES; offset += LARGE_BYTES) {
        seL4_CPtr extra = large_page(&gui, 0x60000000 + offset);
        map_with_tables(alias(&gui, extra), ROOT_VSPACE, surface_a + offset, seL4_CapRights_new(0, 0, 1, 0),
                        seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
    }
#endif
    seL4_CPtr queue = frame(&gui, GUI_INPUT, NULL, 0, 6);
    map_with_tables(alias(&gui, queue), ROOT_VSPACE, input_a, seL4_AllRights,
                    seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
    frame(&gui, GUI_BOOT, &generation, sizeof(generation), 4);
#ifdef DYNAMIC_PRIVATE_NOTES
    seL4_CPtr notes = frame(&gui, PRIVATE_NOTE_PAGE, NULL, 0, 6);
    map_with_tables(alias(&gui, notes), ROOT_VSPACE, PRIVATE_NOTE_PARENT_PAGE, seL4_AllRights,
                    seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
    private_notes_page = PRIVATE_NOTE_PARENT_PAGE;
    seL4_ARM_Page_GetAddress_t note_physical = seL4_ARM_Page_GetAddress(notes);
    check(note_physical.error, "private Notes exchange address");
    line("PRIVATE_NOTES|TEST_PAGE_PHYSICAL|base=", note_physical.paddr);
#endif
    const struct elf_header *h = (const void *)gui_elf_start;
    const struct program_header *programs = (const void *)(gui_elf_start + h->phoff);
    for (unsigned i = 0; i < h->phnum; ++i) {
        const struct program_header *ph = &programs[i];
        if (ph->type != 1 || !ph->memsz) continue;
        for (uintptr_t address = ph->address & ~4095UL; address < ph->address + ph->memsz; address += PAGE_BYTES)
            image_page(&gui, ph, address);
    }
    for (uintptr_t address = GUI_STACK_TOP - 0x20000; address < GUI_STACK_TOP; address += PAGE_BYTES)
        frame(&gui, address, NULL, 0, 6);
    gui.ipc = frame(&gui, GUI_IPC, NULL, 0, 6);
    gui.tcb = app_object(&gui, seL4_TCBObject, 0);
    check(seL4_TCB_Configure(gui.tcb, gui.cnode,
            seL4_CNode_CapData_new(0, seL4_WordBits - APP_CNODE_BITS).words[0],
            gui.vspace, 0, GUI_IPC, gui.ipc), "GUI thread");
    check(seL4_TCB_BindNotification(gui.tcb, notification), "GUI bound notification");
    check(seL4_TCB_SetSchedParams(gui.tcb, ROOT_TCB, 98, 98, gui.sc, fault_endpoint), "GUI CPU budget");
    dynamic_active = true;
    first_frame = false;
    fault_requested = false;
    focus_dirty = true;
    seL4_UserContext regs = {.pc = h->entry, .sp = GUI_STACK_TOP};
    check(seL4_TCB_WriteRegisters(gui.tcb, true, 0, sizeof(regs) / sizeof(seL4_Word), &regs), "GUI start");
    line("DYNAMIC|LAUNCHED|generation=", generation);
}

static void close_window(void)
{
    if (!dynamic_active) return;
    check(seL4_TCB_Suspend(gui.tcb), "GUI quiesce");
#ifdef DYNAMIC_LOCK
    scrub_private_frames();
#endif
#ifdef DYNAMIC_PRIVATE_NOTES
    private_notes_clear();
#endif
    discard_dynamic_surface();
    /* The service endpoint is permanent. Delete its tracked instance aliases;
     * the client has no CNode or Grant authority to create external copies. */
#ifndef CONTROL_KEEP_ENDPOINT
    check(seL4_CNode_Delete(ROOT_NODE, escaped_cap, ROOT_DEPTH), "delete escaped endpoint lease");
#endif
    check(seL4_CNode_Delete(ROOT_NODE, cap_slot(fault_endpoint), ROOT_DEPTH), "delete fault endpoint lease");
    check(seL4_CNode_Delete(ROOT_NODE, cap_slot(gui.endpoint), ROOT_DEPTH), "delete GUI endpoint lease");
    destroy(&gui); /* Revokes aliases mapped into this trusted compositor too. */
    input_signal = attack_signal = 0;
    line("DYNAMIC|CLOSED|generation=", generation);
}

static void dynamic_input_notify(void) { seL4_Signal(input_signal); }
static bool dynamic_control(struct input_event_record event)
{
#ifdef DESKTOP_SHELL
    (void)event; return false; /* Window management belongs only to the shell endpoint. */
#else
#ifdef DYNAMIC_PRIVATE_NOTES
    if (private_notes_control(event)) return true;
#endif
#ifdef DYNAMIC_LOCK
    if (event.device == 0 && event.type == 1 && event.code == 67 && event.value <= 2) {
        if (event.value == 1) begin_device_lock(true);
        return true;
    }
    if (device_locked) return false;
#endif
#ifdef DYNAMIC_SIMPLEX_FRONTEND
    if (frontend_input(event)) return true;
#endif
    if (event.device || event.type != 1 || event.code < 63 || event.code > 65 || event.reserved || event.value > 2)
        return false;
    if (event.value == 1) pending_action = event.code - 62;
    return true;
#endif
}

static void notifications(seL4_Word badge)
{
#ifdef DESKTOP_SHELL
    if (badge & (1UL << 20)) compositor_notified(20);
#endif
#ifdef DYNAMIC_SIMPLEX_FRONTEND
    if (badge & (1UL << 18)) frontend_notify();
#endif
#ifdef DYNAMIC_LOCK
    if (badge & (1UL << LOCK_TIMER_CHANNEL)) lock_timer();
    if (badge & (1UL << LOCK_AUTH_SURFACE_CHANNEL)) { flush_reset(2); compose(); }
#endif
    for (unsigned ch = 0; ch < 7; ++ch)
        if (badge & (1UL << ch)) compositor_notified(ch);
    unsigned action = pending_action;
    pending_action = 0;
    if (action == 1) close_window();
    else if (action == 2) launch();
    else if (action == 3 && dynamic_active) { fault_requested = true; seL4_Signal(attack_signal); }
}

void init(void)
{
    check(seL4_TCB_Suspend(HOSTED_BUDGET_TCB), "stop GUI budget placeholder");
    check(seL4_SchedContext_UnbindObject(HOSTED_SC, HOSTED_BUDGET_TCB), "take GUI CPU budget");
    manager_slot = 1;
    manager_pool = allocate(HOSTED_POOL, &manager_slot, seL4_UntypedObject, 20);
    app_pool = allocate(HOSTED_POOL, &manager_slot, seL4_UntypedObject,
#ifdef DESKTOP_SHELL
                        25
#else
                        24
#endif
                       );
    temporary_cap = manager_slot++;
    escaped_cap = manager_slot++;
    reply_cap = allocate(manager_pool, &manager_slot, seL4_ReplyObject, 0);
    app_start = manager_slot + 16;
    surface_a = 0x60000000;
    input_a = 0x66001000;
    compositor_init();
#ifdef DYNAMIC_LOCK
#ifdef DYNAMIC_SIMPLEX_SESSION
    simplex_session_boot();
#endif
#ifdef DYNAMIC_WRAPPED_LOCK
    evict_vault();
#endif
    publish_lock_status();
    lock_timer();
    compose();
    print("LOCK|BOOT|locked\n");
#elif defined(DESKTOP_SHELL)
    print("DESKTOP|BOOT|workspace\n");
#else
    launch();
#endif
}
void notified(microkit_channel channel) { notifications(1UL << channel); }
#ifdef DESKTOP_SHELL
static microkit_msginfo shell_request(microkit_msginfo message)
{
    if (microkit_msginfo_get_label(message) != SHELL_REQUEST || microkit_msginfo_get_count(message) != 4 || seL4_MessageInfo_get_extraCaps(message)) return microkit_msginfo_new(1, 0);
    uint64_t command = microkit_mr_get(0), id = microkit_mr_get(1);
    if (id < 1 || id > 2 || command < 1 || command > 5) return microkit_msginfo_new(1, 0);
    struct shell_window *w = &desktop.windows[id - 1];
    if (command == SHELL_APP_LAUNCH) {
        if (id == 1 && !dynamic_active && generation >= 60) return microkit_msginfo_new(1, 0);
        w->open = w->visible = 1;
        if (id == 1) launch();
        focus(id);
    } else if (command == SHELL_APP_MINIMIZE) {
        if (!w->open) return microkit_msginfo_new(1, 0);
        w->visible = 0;
        if (desktop.focused == id) focus(0);
        release_owner(id);
    } else if (command == SHELL_APP_CLOSE) {
        if (id == 1) close_window();
        else { w->visible = w->open = 0; release_owner(2); microkit_notify(23); if (desktop.focused == 2) focus(0); }
    } else if (command == SHELL_APP_MOVE) {
        if (!w->visible) return microkit_msginfo_new(1, 0);
        /* Reject values outside signed 32-bit ABI before clamping coordinates. */
        int64_t x = (int64_t)microkit_mr_get(2), y = (int64_t)microkit_mr_get(3);
        if (x < INT32_MIN || x > INT32_MAX || y < INT32_MIN || y > INT32_MAX) return microkit_msginfo_new(1, 0);
        shell_position(w, x, y);
    } else {
        if (!w->visible) return microkit_msginfo_new(1, 0);
        focus(id);
    }
    publish_state(); compose();
    line("DESKTOP|COMMAND|", command * 10 + id);
    return microkit_msginfo_new(0, 0);
}
#endif
microkit_msginfo protected(microkit_channel channel, microkit_msginfo message)
{
#ifdef DESKTOP_SHELL
    if (channel == SHELL_CHANNEL) return shell_request(message);
#endif
#ifdef DYNAMIC_LOCK
    extern microkit_msginfo lock_request(microkit_channel, microkit_msginfo);
    if (channel == LOCK_AUTH_CHANNEL || channel == LOCK_PROBE_CHANNEL) return lock_request(channel, message);
    if (device_locked) return microkit_msginfo_new(1, 0);
#endif
#ifdef DYNAMIC_PRIVATE_NOTES
    if (microkit_msginfo_get_label(message) == PRIVATE_NOTE_LOAD || microkit_msginfo_get_label(message) == PRIVATE_NOTE_SAVE)
        return private_notes_request(channel, message);
#endif
    if (!dynamic_active || channel != generation || microkit_msginfo_get_label(message) != 0x780 ||
        microkit_msginfo_get_count(message) != 2 || seL4_MessageInfo_get_extraCaps(message) ||
        microkit_mr_get(0) != generation) return microkit_msginfo_new(1, 0);
    const struct surface_header *header = (const void *)surface_a;
    uint32_t revision = atomic_load_explicit(&header->revision, memory_order_acquire);
    if (!revision || (revision & 1) || header->width !=
#ifdef DESKTOP_SHELL
        APP_PIXEL_WIDTH || header->height != APP_PIXEL_HEIGHT || header->stride != APP_PIXEL_WIDTH * 3
#else
        APP_WIDTH || header->height != APP_HEIGHT || header->stride != APP_WIDTH * 3
#endif
       ) return microkit_msginfo_new(1, 0);
    compositor_notified(0);
    if (!first_frame) { first_frame = true; line("DYNAMIC|READY|generation=", generation); }
    return microkit_msginfo_new(0, 0);
}
seL4_Bool fault(microkit_child child, microkit_msginfo info, microkit_msginfo *reply)
{
    (void)reply;
#ifdef DYNAMIC_SIMPLEX_SESSION
    if (child >= SESSION_FIRST_CHILD && child < SESSION_FIRST_CHILD + SESSION_CHILD_COUNT) {
        line("SIMPLEX_SESSION|FAULT|child=", child);
        simplex_session_fault();
        return seL4_False;
    }
#endif
    if (!dynamic_active || child != generation) fail("graphical fault identity", 0);
    bool expected = fault_requested && microkit_msginfo_get_label(info) == seL4_Fault_VMFault &&
                    microkit_mr_get(seL4_VMFault_Addr) == 0x61000000 &&
                    !microkit_mr_get(seL4_VMFault_PrefetchFault);
    if (!expected) line("DYNAMIC|UNEXPECTED_FAULT|generation=", generation);
    close_window();
    line("DYNAMIC|FAULT_CONTAINED|generation=", generation);
    return seL4_False;
}
#ifdef DYNAMIC_LOCK
#include "../desktop-lock/manager.h"
#endif
#ifdef DYNAMIC_PRIVATE_NOTES
#include "../desktop-notes/broker.h"
#endif

#ifdef DYNAMIC_SIMPLEX_FRONTEND
#include "../desktop-simplex/frontend.h"
#endif
