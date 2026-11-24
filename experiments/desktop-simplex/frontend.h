/* Trusted full-resolution frontend routing. The managed child only writes its
 * private surface/ring. It cannot access the authenticator or GPU output. */
static uint32_t frontend_x, frontend_y;
static bool frontend_input(struct input_event_record event)
{
    if (device_locked || !session_state->active) return false;
    if (event.reserved || event.device > 1) return true;
    activity_ms = lock_now();
    if (event.device == INPUT_DEVICE_TABLET && event.type == 3 && event.code <= 1) {
        if (event.value > 32767) return true;
        if (!event.code) frontend_x = event.value * 1079 / 32767;
        else frontend_y = event.value * 2339 / 32767;
    }
    volatile uint32_t *ring = (void *)FRONTEND_FOCUS_ADDRESS;
    uint32_t head = __atomic_load_n(&ring[2], __ATOMIC_RELAXED);
    uint32_t tail = __atomic_load_n(&ring[3], __ATOMIC_ACQUIRE);
    if (head - tail >= 224) { print("SIMPLEX_FRONTEND|INPUT_OVERFLOW\n"); return true; }
    volatile uint32_t *record = &ring[4 + (head % 224) * 4];
    record[0] = event.device | ((uint32_t)event.type << 16);
    record[1] = event.code; record[2] = event.value;
    record[3] = frontend_x | (frontend_y << 16);
    ring[0] = 1;
    __atomic_store_n(&ring[2], head + 1, __ATOMIC_RELEASE);
    microkit_notify(18);
    return true;
}
static bool frontend_compose(void)
{
    const slint_frame_header_t *source = (void *)FRONTEND_FRAME_ADDRESS;
    uint32_t offered = atomic_load_explicit(&source->revision, memory_order_acquire);
    if (offered && offered == frontend_revision) return true;
    if (offered && !(offered & 1) && offered != frontend_revision &&
        source->width == SLINT_FRAME_WIDTH && source->height == SLINT_FRAME_HEIGHT &&
        source->stride_bytes == SLINT_FRAME_STRIDE_BYTES && source->format == SLINT_FRAME_FORMAT_RGB8) {
        const volatile uint8_t *pixels = (const void *)(source + 1);
        for (size_t i = 0; i < sizeof(composed_pixels); ++i) composed_pixels[i] = pixels[i];
        atomic_thread_fence(memory_order_acquire);
        if (atomic_load_explicit(&source->revision, memory_order_acquire) != offered) {
            for (size_t i = 0; i < sizeof(composed_pixels); ++i) composed_pixels[i] = 0;
            frontend_revision = 0;
        } else frontend_revision = offered;
    }
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
    microkit_notify(2);
    return true;
}
static void frontend_notify(void)
{
    if (device_locked || !session_state->active) return;
    volatile uint32_t *focus = (void *)FRONTEND_FOCUS_ADDRESS;
    if (__atomic_exchange_n(&focus[1], 0, __ATOMIC_ACQ_REL)) microkit_notify(18);
    compose();
}
