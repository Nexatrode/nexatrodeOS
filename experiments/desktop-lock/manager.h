/* Trusted lifecycle/lock boundary, confined to the disposable experiment. */
static struct { seL4_CPtr cap; size_t bytes; } private_frames[MAX_OBJECTS];
static uint64_t lock_epoch = 1, activity_ms;
#ifdef DYNAMIC_WRAPPED_LOCK
static uint64_t vault_lease_until;
static bool vault_failed;
#endif
static uint32_t auth_revision;
static uint8_t auth_pixels[SLINT_FRAME_PIXEL_BYTES];
static const struct lock_surface_header *auth_surface = (const void *)0x70000000;
static struct lock_status_page *lock_state = (void *)0x69000000;

static uint64_t lock_now(void)
{
    uint64_t ticks, frequency;
#if CONFIG_EXPORT_VCNT_USER
    __asm__ volatile("isb; mrs %0, cntvct_el0; mrs %1, cntfrq_el0" : "=r"(ticks), "=r"(frequency));
#elif CONFIG_EXPORT_PCNT_USER
    __asm__ volatile("isb; mrs %0, cntpct_el0; mrs %1, cntfrq_el0" : "=r"(ticks), "=r"(frequency));
#else
#error "The disposable lock manager requires an exported monotonic counter"
#endif
    return ticks / frequency * 1000 + ticks % frequency * 1000 / frequency;
}
#ifdef DYNAMIC_WRAPPED_LOCK
/* The manager alone can arm, commit or revoke a key. Authentication only
 * prepares it; app services remain denied until this commit succeeds. */
static bool vault_transition(unsigned label, unsigned phase, bool erased)
{
    microkit_mr_set(0, lock_epoch);
    microkit_msginfo reply = microkit_ppcall(LOCK_VAULT_CHANNEL, microkit_msginfo_new(label, 1));
    return microkit_msginfo_get_label(reply) == 0x300 && microkit_msginfo_get_count(reply) == 3 &&
           microkit_mr_get(0) == lock_epoch && microkit_mr_get(1) == phase &&
           microkit_mr_get(2) == erased;
}
static bool evict_vault(void)
{
    vault_lease_until = 0;
    if (!vault_transition(0x453, 0, true)) {
        vault_failed = true;
        print("LOCK|VAULT_EVICTION_FAILED\n");
        return false;
    }
    line("LOCK|VAULT_EVICTED|epoch=", lock_epoch);
    return true;
}
#endif
static void track_private_frame(seL4_CPtr cap, size_t bytes)
{
    if (private_frame_count >= MAX_OBJECTS) fail("private frame ledger full", 0);
    private_frames[private_frame_count++] = (typeof(private_frames[0])){cap, bytes};
}
static void scrub_private_frames(void)
{
    size_t total = 0;
    for (unsigned i = 0; i < private_frame_count; ++i) {
        seL4_CPtr cap = private_frames[i].cap;
        size_t bytes = private_frames[i].bytes;
        uintptr_t address = bytes == LARGE_BYTES ? LARGE_TEMP : TEMP_ADDRESS;
        check(seL4_CNode_Copy(ROOT_NODE, temporary_cap, ROOT_DEPTH, ROOT_NODE, cap_slot(cap),
                              ROOT_DEPTH, seL4_AllRights), "scrub alias");
        map_with_tables(slot_cap(temporary_cap), ROOT_VSPACE, address, seL4_AllRights,
                        seL4_ARM_Default_VMAttributes | seL4_ARM_ExecuteNever, NULL);
#ifndef CONTROL_SKIP_SCRUB
        volatile uint8_t *memory = (void *)address;
        for (size_t j = 0; j < bytes; ++j) memory[j] = 0;
        check(seL4_ARM_Page_Clean_Data(slot_cap(temporary_cap), 0, bytes), "scrub clean data");
        for (size_t j = 0; j < bytes; ++j) if (memory[j]) fail("private frame not cleared", 0);
#endif
        unmap_temporary();
        total += bytes;
    }
#ifdef CONTROL_SKIP_SCRUB
    line("LOCK|CONTROL_SCRUB_SKIPPED|bytes=", total);
#else
    line("LOCK|SCRUBBED|bytes=", total);
#endif
    for (unsigned i = 0; i < private_frame_count; ++i) private_frames[i] = (typeof(private_frames[0])){0};
    private_frame_count = 0;
}
static void publish_lock_status(void)
{
    uint32_t revision = atomic_load_explicit(&lock_state->revision, memory_order_relaxed);
    atomic_store_explicit(&lock_state->revision, revision + 1, memory_order_release);
    atomic_thread_fence(memory_order_seq_cst);
    lock_state->locked = device_locked;
    lock_state->epoch = lock_epoch;
    lock_state->activity_ms = activity_ms;
    lock_state->idle_ms = LOCK_IDLE_MS;
    atomic_store_explicit(&lock_state->revision, revision + 2, memory_order_release);
    microkit_notify(LOCK_AUTH_SURFACE_CHANNEL);
}
static void reset_lock_input(void)
{
    desktop_reset(&router, emit);
    router.focus = 0;
    router.pointer_owner = 0;
    /* Drop events already received across the transition, including held keys. */
    struct input_event_ring *ring = (void *)input_handoff;
    driver_tail = __atomic_load_n(&ring->head, __ATOMIC_ACQUIRE);
    for (unsigned i = 0; i < INPUT_EVENT_CAPACITY; ++i) ring->events[i] = (struct input_event_record){0};
    __atomic_store_n(&ring->tail, driver_tail, __ATOMIC_RELEASE);
}
static void begin_device_lock(bool force)
{
    if (device_locked && !force) return;
    device_locked = true;
    ++lock_epoch;
    pending_action = 0;
#ifdef DYNAMIC_WRAPPED_LOCK
    // Stop the producer and close vault access before the longer pixel/frame
    // erasure work. No app service remains open behind the cover frame.
    if (dynamic_active) check(seL4_TCB_Suspend(gui.tcb), "lock producer quiesce");
    bool vault_evicted = evict_vault();
#endif
    auth_revision = 0;
    for (size_t i = 0; i < sizeof(auth_pixels); ++i) auth_pixels[i] = 0;
    publish_lock_status();
    compose(); /* Replace displayed app before potentially expensive cleanup. */
    close_window();
    reset_lock_input();
#ifdef DYNAMIC_WRAPPED_LOCK
    if (!vault_evicted) { publish_lock_status(); return; }
#endif
    line("LOCK|LOCKED|epoch=", lock_epoch);
    microkit_notify(10); /* Unprivileged adversarial probe. */
}
static void lock_timer(void)
{
#ifdef DYNAMIC_WRAPPED_LOCK
    if (device_locked && vault_lease_until && lock_now() >= vault_lease_until) {
        print("LOCK|UNLOCK_LEASE_EXPIRED\n");
        begin_device_lock(true);
    }
    if (device_locked && vault_lease_until) microkit_notify(10); /* Probe prepared-key access denial. */
#endif
    if (!device_locked && lock_now() - activity_ms >= LOCK_IDLE_MS) begin_device_lock(false);
    microkit_notify(LOCK_AUTH_SURFACE_CHANNEL);
    microkit_mr_set(0, 250000000);
    microkit_ppcall(LOCK_TIMER_CHANNEL, microkit_msginfo_new(1, 1));
}
static bool lock_input(struct input_event_record event)
{
    if (event.reserved) return true;
    if (!device_locked) {
        activity_ms = lock_now();
        return false;
    }
    /* While locked, route exclusively to the trusted full-screen authenticator. */
    if (event.device == INPUT_DEVICE_TABLET) {
        if (event.type == 3 && event.code <= 1 && event.value <= 32767) {
            if (!event.code) router.x = event.value * 1079 / 32767;
            else router.y = event.value * 2339 / 32767;
            emit(2, (struct desktop_event){.kind = DESKTOP_POINTER, .value = 2,
                 .x = (int32_t)router.x - 560, .y = (int32_t)router.y - 500});
        } else if (event.type == 1 && event.code == 272 && event.value <= 1) {
            emit(2, (struct desktop_event){.kind = DESKTOP_POINTER, .value = event.value,
                 .x = (int32_t)router.x - 560, .y = (int32_t)router.y - 500});
        }
    } else if (event.device == INPUT_DEVICE_KEYBOARD && event.type == 1 &&
               event.code <= DESKTOP_KEY_MAX && event.value <= 2) {
        if (event.value == 1) router.keys[event.code] = 2;
        if (router.keys[event.code] == 2)
            emit(2, (struct desktop_event){.kind = DESKTOP_KEY, .code = event.code, .value = event.value});
        if (!event.value) router.keys[event.code] = 0;
    }
    return true;
}
static bool lock_compose(void)
{
    uint32_t offered = atomic_load_explicit(&auth_surface->revision, memory_order_acquire);
    bool valid = offered && !(offered & 1) && auth_surface->width == SLINT_FRAME_WIDTH &&
                 auth_surface->height == SLINT_FRAME_HEIGHT && auth_surface->stride == SLINT_FRAME_STRIDE_BYTES &&
                 auth_surface->epoch == lock_epoch && auth_surface->locked == device_locked && !auth_surface->reserved;
    if (valid && offered != auth_revision) {
        const volatile uint8_t *source = (const void *)(auth_surface + 1);
        for (size_t i = 0; i < sizeof(auth_pixels); ++i) auth_pixels[i] = source[i];
        atomic_thread_fence(memory_order_acquire);
        if (atomic_load_explicit(&auth_surface->revision, memory_order_acquire) == offered)
            auth_revision = offered;
        else {
            auth_revision = 0;
            for (size_t i = 0; i < sizeof(auth_pixels); ++i) auth_pixels[i] = 0;
        }
    }
    uint32_t next_a = 0;
    for (size_t i = 0; i < sizeof(composed_pixels); ++i) composed_pixels[i] = auth_pixels[i];
    if (!device_locked && dynamic_active) blit(surface_a, 0, 40, 500, &next_a);
    slint_frame_header_t *output = (void *)output_frame;
    uint32_t revision = atomic_load_explicit(&output->revision, memory_order_relaxed);
    atomic_store_explicit(&output->revision, revision + 1, memory_order_release);
    atomic_thread_fence(memory_order_seq_cst);
    uint8_t *pixels = (void *)(output + 1);
    for (size_t i = 0; i < sizeof(composed_pixels); ++i) pixels[i] = composed_pixels[i];
    output->width = SLINT_FRAME_WIDTH; output->height = SLINT_FRAME_HEIGHT;
    output->stride_bytes = SLINT_FRAME_STRIDE_BYTES; output->format = SLINT_FRAME_FORMAT_RGB8;
    atomic_store_explicit(&output->damage_y, 0, memory_order_relaxed);
    atomic_store_explicit(&output->damage_height, SLINT_FRAME_HEIGHT, memory_order_relaxed);
    atomic_store_explicit(&output->revision, revision + 2, memory_order_release);
    revisions[0] = next_a;
    focus_dirty = false;
    microkit_notify(2);
    return true;
}
microkit_msginfo lock_request(microkit_channel channel, microkit_msginfo message)
{
    if (channel != LOCK_AUTH_CHANNEL || seL4_MessageInfo_get_extraCaps(message)) {
        print("LOCK|FOREIGN_UNLOCK_DENIED\n");
        return microkit_msginfo_new(1, 0);
    }
    unsigned label = microkit_msginfo_get_label(message), count = microkit_msginfo_get_count(message);
    if (label == LOCK_STATUS && count == 0) {
        microkit_mr_set(0, device_locked); microkit_mr_set(1, lock_epoch); microkit_mr_set(2, LOCK_IDLE_MS);
#ifdef DYNAMIC_WRAPPED_LOCK
        microkit_mr_set(3, vault_failed);
        return microkit_msginfo_new(0, 4);
#else
        return microkit_msginfo_new(0, 3);
#endif
    }
    if (label == LOCK_REQUEST && count == 0) {
        begin_device_lock(false);
        return microkit_msginfo_new(0, 0);
    }
#ifdef DYNAMIC_WRAPPED_LOCK
    if (label == LOCK_ABORT && count == 1 && device_locked && microkit_mr_get(0) == lock_epoch) {
        begin_device_lock(true);
        return microkit_msginfo_new(vault_failed ? 2 : 0, 0);
    }
    if (label == LOCK_PREPARE && count == 1 && device_locked && !vault_failed && !vault_lease_until && microkit_mr_get(0) == lock_epoch) {
        if (!vault_transition(0x456, 1, true)) { begin_device_lock(true); return microkit_msginfo_new(2, 0); }
        vault_lease_until = lock_now() + LOCK_VAULT_LEASE_MS;
        return microkit_msginfo_new(0, 0);
    }
#endif
    if (label == LOCK_UNLOCK && count == 1 && device_locked && microkit_mr_get(0) == lock_epoch) {
#ifdef DYNAMIC_WRAPPED_LOCK
        if (vault_failed || !vault_lease_until || lock_now() >= vault_lease_until) return microkit_msginfo_new(1, 0);
        if (!vault_transition(0x455, 3, false)) { begin_device_lock(true); return microkit_msginfo_new(2, 0); }
        vault_lease_until = 0;
#endif
        device_locked = false;
        activity_ms = lock_now();
        reset_lock_input();
        publish_lock_status();
        launch();
        line("LOCK|UNLOCKED|generation=", generation);
#ifdef DYNAMIC_WRAPPED_LOCK
        microkit_notify(10);
#endif
        return microkit_msginfo_new(0, 0);
    }
    return microkit_msginfo_new(1, 0);
}
