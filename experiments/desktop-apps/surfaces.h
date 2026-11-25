/* Desktop adapter for the existing phone frame and queued-input contracts.
 * The manager owns focus, coordinates, titlebars and publication to the GPU. */
static uint32_t app_sent[DESKTOP_APP_COUNT];
static bool app_reset[DESKTOP_APP_COUNT];
static void app_record(unsigned owner, uint32_t device, uint32_t kind,
                       uint32_t code, uint32_t value, int32_t x, int32_t y)
{
    if (owner < 1 || owner > DESKTOP_APP_COUNT) return;
    uint32_t *q = (void *)app_input(owner);
    unsigned i = owner - 1;
    uint32_t tail = __atomic_load_n(&q[3], __ATOMIC_ACQUIRE);
    if (app_sent[i] - tail >= 224) { app_reset[i] = true; input_notify(owner); return; }
    if (app_reset[i]) {
        uint32_t *r = q + 4 + (app_sent[i]++ % 224) * 4;
        r[0] = 2; r[1] = 1; r[2] = desktop.focused == owner; r[3] = 0;
        app_reset[i] = false;
        __atomic_store_n(&q[2], app_sent[i], __ATOMIC_RELEASE);
        if (app_sent[i] - tail >= 224) { app_reset[i] = true; input_notify(owner); return; }
    }
    uint32_t *r = q + 4 + (app_sent[i]++ % 224) * 4;
    r[0] = device | (kind << 16); r[1] = code; r[2] = value;
    r[3] = (uint16_t)(int16_t)(x * (int)DESKTOP_SCALE) | ((uint32_t)(uint16_t)(int16_t)(y * (int)DESKTOP_SCALE) << 16);
    __atomic_store_n(&q[2], app_sent[i], __ATOMIC_RELEASE);
    input_notify(owner);
}
static void app_emit(unsigned owner, struct desktop_event e)
{
    if (e.kind == DESKTOP_KEY) app_record(owner, INPUT_DEVICE_KEYBOARD, 1, e.code, e.value, 0, 0);
    else if (e.kind == DESKTOP_POINTER) app_record(owner, INPUT_DEVICE_TABLET,
        e.value == 2 ? 3 : 1, e.value == 2 ? 0 : 272, e.value == 2 ? 0 : e.value, e.x, e.y);
    else app_record(owner, 2, 0, e.kind == DESKTOP_RESET,
        e.kind == DESKTOP_FOCUS ? e.value : desktop.focused == owner, 0, 0);
}
static void app_scroll(unsigned owner, int32_t amount)
{
    app_record(owner, INPUT_DEVICE_TABLET, 2, 8, amount,
        pointer_x - desktop.windows[owner - 1].x, pointer_y - desktop.windows[owner - 1].y);
}
static bool snapshot_app(uintptr_t source, uint8_t *cache, uint32_t *previous)
{
    const slint_frame_header_t *h = (const void *)source;
    uint32_t first = atomic_load_explicit(&h->revision, memory_order_acquire);
    if (!first || first & 1 || first == *previous || h->width != APP_PIXEL_WIDTH ||
        h->height != APP_PIXEL_HEIGHT || h->stride_bytes != APP_PIXEL_WIDTH * 3 || h->format != SLINT_FRAME_FORMAT_RGB8) return false;
    const volatile uint8_t *p = (const void *)(h + 1);
    for (size_t i = 0; i < APP_BYTES; ++i) candidate[i] = p[i];
    atomic_thread_fence(memory_order_acquire);
    if (atomic_load_explicit(&h->revision, memory_order_acquire) != first) return false;
    memcpy(cache, candidate, APP_BYTES); *previous = first; return true;
}
