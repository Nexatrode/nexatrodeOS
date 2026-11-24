/*
 * Copyright 2024, UNSW
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>
#include <stddef.h>
#include <microkit.h>
#include <sddf/util/util.h>
#include <sddf/util/printf.h>
#include <sddf/gpu/queue.h>
#include <sddf/gpu/events.h>
#include <sddf/timer/client.h>
#include <gpu_config.h>
#include "slint_frame.h"

/* Queue records live in device memory, which can fault on wide, unaligned
 * stores even when the compiler considers them valid for normal RAM. */
static inline int enqueue_gpu_request(gpu_queue_handle_t *handle, gpu_req_t request)
{
    if (gpu_queue_full_req(handle)) {
        return -1;
    }
    gpu_req_queue_t *queue = handle->req_queue;
    volatile uint8_t *dst = (volatile uint8_t *)&queue->buffers[queue->tail % handle->capacity];
    const uint8_t *src = (const uint8_t *)&request;
    for (size_t i = 0; i < sizeof(request); ++i) {
        dst[i] = src[i];
    }
    queue->tail++;
    return 0;
}
#define gpu_enqueue_req enqueue_gpu_request

/* Uncomment this to enable debug logging */
// #define DEBUG_GPU_CLIENT

#if defined(DEBUG_GPU_CLIENT)
#define LOG_GPU_CLIENT(...) do{ sddf_dprintf("GPU_CLIENT|INFO: "); sddf_dprintf(__VA_ARGS__); }while(0)
#else
#define LOG_GPU_CLIENT(...) do{}while(0)
#endif
#define LOG_GPU_CLIENT_ERR(...) do{ sddf_dprintf("GPU_CLIENT|ERROR: "); sddf_dprintf(__VA_ARGS__); }while(0)

#define VIRT_CH 0
#define TIMER_CH 1
#define UI_FOCUS_CH 2

#if defined(NEXATRODE_GPU_PERF_TRACE)
static uint64_t gpu_perf_time_us(void)
{
    uint64_t ticks, frequency;
    __asm__ volatile("isb; mrs %0, cntpct_el0; mrs %1, cntfrq_el0"
                     : "=r"(ticks), "=r"(frequency));
    return ticks / (frequency / 1000000u);
}
#endif

#define DISPLAY_INFO_DATA_OFFSET 0x1000
#define APP_VIEW_TEXT_CAPACITY 128
#define NOTES_VISIBLE_COLUMNS 21
#define NOTES_VISIBLE_ROWS 3

typedef struct {
    uint32_t revision;
    uint32_t length;
    uint8_t text[APP_VIEW_TEXT_CAPACITY];
    uint32_t scroll_start_line;
    uint32_t notes_mode;
    uint32_t selected_note;
    uint32_t note_count;
    uint32_t notes_delete_armed;
    uint32_t notes_field;
} app_view_t;

_Static_assert(sizeof(app_view_t) == 160, "app view wire layout changed");

#ifndef FB_IMG_WIDTH
#error "FB_IMG_WIDTH not defined"
#endif
#ifndef FB_IMG_HEIGHT
#error "FB_IMG_HEIGHT not defined"
#endif

_Static_assert(FB_IMG_WIDTH *FB_IMG_HEIGHT *GPU_BPP_2D <= GPU_DATA_REGION_SIZE_CLI0 - DISPLAY_INFO_DATA_OFFSET,
               "FB_IMG_WIDTH * FB_IMG_HEIGHT * GPU_BPP_2D must be less than or "
               "equal to GPU_VIRTIO_DATA_REGION_SIZE");

gpu_events_t *gpu_events;
gpu_req_queue_t *gpu_req_queue;
gpu_resp_queue_t *gpu_resp_queue;
uintptr_t gpu_data;
uintptr_t ui_focus;
uintptr_t ui_pointer;
uintptr_t notes_view;
uintptr_t contacts_view;
uintptr_t calculator_view;
uintptr_t clock_view;
uintptr_t calendar_view;
uintptr_t messages_view;
uintptr_t settings_view;
uintptr_t phone_view;
uintptr_t slint_frame;

typedef struct {
    uint32_t revision;
    uint32_t x;
    uint32_t y;
    uint32_t visible;
} ui_pointer_t;

_Static_assert(sizeof(ui_pointer_t) == 16, "pointer wire layout changed");

extern char _fb_img[];
extern char _fb_img_end[];

static gpu_queue_handle_t gpu_queue_handle;
static uint32_t req_id = 0;
static uint32_t display_info_req_id = -1;
static gpu_resp_get_display_info_t display_info = { 0 };
static bool display_info_init = false;
static bool pending_display_info_request = false;
static uint32_t rendered_focus = UINT32_MAX;
static uint32_t rendered_view_revision = UINT32_MAX;
static uint32_t rendered_pointer_revision = UINT32_MAX;
static uint32_t rendered_slint_frame_revision = UINT32_MAX;
static bool slint_full_retry;

static const uint8_t font_5x7[26][7] = {
    {0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11},
    {0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e},
    {0x0f, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0f},
    {0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e},
    {0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f},
    {0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10},
    {0x0f, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0f},
    {0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11},
    {0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1f},
    {0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0c},
    {0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11},
    {0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f},
    {0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11},
    {0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11},
    {0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e},
    {0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10},
    {0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d},
    {0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11},
    {0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e},
    {0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04},
    {0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e},
    {0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04},
    {0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a},
    {0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11},
    {0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04},
    {0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f},
};

static const uint8_t font_digits_5x7[10][7] = {
    {0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e},
    {0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e},
    {0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f},
    {0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e},
    {0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02},
    {0x1f, 0x10, 0x10, 0x1e, 0x01, 0x01, 0x1e},
    {0x0e, 0x10, 0x10, 0x1e, 0x11, 0x11, 0x0e},
    {0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08},
    {0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e},
    {0x0e, 0x11, 0x11, 0x0f, 0x01, 0x01, 0x0e},
};

static const uint8_t font_minus_5x7[7] = { 0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00 };

static int get_and_inc_req_id(void)
{
    int ret = req_id;
    req_id++;
    return ret;
}

#ifdef GPU_BLOB_SUPPORT
static void draw_image_blob(int resource_id, int res_width, int res_height, gpu_rect_t rect)
{
    int err = 0;
    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_CREATE_BLOB,
        .id = get_and_inc_req_id(),
        .resource_create_blob = {
            .resource_id = resource_id,
            .mem_offset = DISPLAY_INFO_DATA_OFFSET,
            .mem_size = res_width * res_height * GPU_BPP_2D,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_SET_SCANOUT_BLOB,
        .id = get_and_inc_req_id(),
        .set_scanout_blob = {
            .resource_id = resource_id,
            .scanout_id = 0,
            .rect.x = rect.x,
            .rect.y = rect.y,
            .rect.width = rect.width,
            .rect.height = rect.height,
            .format = GPU_FORMAT_B8G8R8A8_UNORM,
            .width = res_width,
            .height = res_height,
            .offset = 0,
            .stride = res_width * GPU_BPP_2D,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_FLUSH,
        .id = get_and_inc_req_id(),
        .resource_flush = {
            .resource_id = resource_id,
            .rect.x = rect.x,
            .rect.y = rect.y,
            .rect.width = rect.width,
            .rect.height = rect.height,
        },
    });
    assert(!err);

    LOG_GPU_CLIENT("Drawing blob image! Making the following requests:\n\
    resource_create_blob: mem_offset: %d mem_size: %d\n\
    set_scanout_blob: rect.x: %d rect.y: %d rect.width: %d rect.height: %d scanout_id: %d, format: %d\n\
                      width: %d height: %d offset: %d stride: %d\n\
    resource_flush: rect.x: %d rect.y: %d rect.width: %d rect.height: %d\n",
                   DISPLAY_INFO_DATA_OFFSET, res_width * res_height * GPU_BPP_2D, rect.x, rect.y, rect.width,
                   rect.height, 0, GPU_FORMAT_B8G8R8A8_UNORM, res_width, res_height, 0, res_width * GPU_BPP_2D, rect.x,
                   rect.y, rect.width, rect.height);
}
#endif

static void draw_image(int resource_id, int res_width, int res_height, int xfer_offset, gpu_rect_t rect)
{
    int err = 0;
    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_CREATE_2D,
        .id = get_and_inc_req_id(),
        .resource_create_2d = {
            .resource_id = resource_id,
            .width = res_width,
            .height = res_height,
            .format = GPU_FORMAT_B8G8R8A8_UNORM,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_ATTACH_BACKING,
        .id = get_and_inc_req_id(),
        .resource_attach_backing = {
            .resource_id = resource_id,
            .mem_offset = DISPLAY_INFO_DATA_OFFSET,
            .mem_size = res_width * res_height * GPU_BPP_2D,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_SET_SCANOUT,
        .id = get_and_inc_req_id(),
        .set_scanout = {
            .resource_id = resource_id,
            .scanout_id = 0,
            .rect.x = rect.x,
            .rect.y = rect.y,
            .rect.width = rect.width,
            .rect.height = rect.height,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_TRANSFER_TO_2D,
        .id = get_and_inc_req_id(),
        .transfer_to_2d = {
            .resource_id = resource_id,
            .rect.x = rect.x,
            .rect.y = rect.y,
            .rect.width = rect.width,
            .rect.height = rect.height,
            .mem_offset = xfer_offset,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_FLUSH,
        .id = get_and_inc_req_id(),
        .resource_flush = {
            .resource_id = resource_id,
            .rect.x = rect.x,
            .rect.y = rect.y,
            .rect.width = rect.width,
            .rect.height = rect.height,
        },
    });
    assert(!err);

    LOG_GPU_CLIENT("Drawing image! Making the following requests:\n\
    resource_create_2d: width: %d height: %d format: bgra\n\
    attach_backing: mem_offset: %d mem_size: %d\n\
    set_scanout: scanout_id: 0 rect.x: %d rect.y: %d rect.width: %d rect.height: %d\n\
    transfer_to_2d: rect.x: %d rect.y: %d rect.width: %d rect.height: %d mem_offset: %d\n\
    resource_flush: rect.x: %d rect.y: %d rect.width: %d rect.height: %d\n",
                   res_width, res_height, DISPLAY_INFO_DATA_OFFSET, res_width * res_height, rect.x, rect.y, rect.width,
                   rect.height, rect.x, rect.y, rect.width, rect.height, xfer_offset, rect.x, rect.y, rect.width,
                   rect.height);
}

static void cleanup_resources(int resource_id_1, int resource_id_2)
{
    int err = 0;
    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_SET_SCANOUT,
        .id = get_and_inc_req_id(),
        .set_scanout = {
            .resource_id = 0,
            .scanout_id = 0,
            .rect = {
                .x = 0,
                .y = 0,
                .width = 0,
                .height = 0,
            },
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_DETACH_BACKING,
        .id = get_and_inc_req_id(),
        .resource_detach_backing = {
            .resource_id = resource_id_1,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_DETACH_BACKING,
        .id = get_and_inc_req_id(),
        .resource_detach_backing = {
            .resource_id = resource_id_2,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_UNREF,
        .id = get_and_inc_req_id(),
        .resource_unref = {
            .resource_id = resource_id_1,
        },
    });
    assert(!err);

    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_UNREF,
        .id = get_and_inc_req_id(),
        .resource_unref = {
            .resource_id = resource_id_2,
        },
    });
    assert(!err);
}

static bool read_app_view(uintptr_t address, char *text, uint32_t *length,
                          uint32_t *revision, uint32_t *scroll_start_line,
                          uint32_t *notes_mode, uint32_t *selected_note,
                          uint32_t *note_count, uint32_t *notes_delete_armed,
                          uint32_t *notes_field)
{
    const app_view_t *view = (const app_view_t *)address;
    for (int attempt = 0; attempt < 8; attempt++) {
        uint32_t first = __atomic_load_n(&view->revision, __ATOMIC_ACQUIRE);
        if (first & 1) {
            continue;
        }
        uint32_t size = __atomic_load_n(&view->length, __ATOMIC_RELAXED);
        uint32_t scroll = __atomic_load_n(&view->scroll_start_line, __ATOMIC_RELAXED);
        uint32_t mode = __atomic_load_n(&view->notes_mode, __ATOMIC_RELAXED);
        uint32_t selected = __atomic_load_n(&view->selected_note, __ATOMIC_RELAXED);
        uint32_t count = __atomic_load_n(&view->note_count, __ATOMIC_RELAXED);
        uint32_t delete_armed = __atomic_load_n(&view->notes_delete_armed, __ATOMIC_RELAXED);
        uint32_t field = __atomic_load_n(&view->notes_field, __ATOMIC_RELAXED);
        if (size > APP_VIEW_TEXT_CAPACITY) {
            return false;
        }
        const volatile uint8_t *source = view->text;
        for (uint32_t i = 0; i < size; i++) {
            text[i] = (char)source[i];
        }
        text[size] = '\0';
        __atomic_thread_fence(__ATOMIC_ACQUIRE);
        uint32_t last = __atomic_load_n(&view->revision, __ATOMIC_ACQUIRE);
        if (first == last && !(last & 1)) {
            *length = size;
            *revision = last;
            *scroll_start_line = scroll;
            *notes_mode = mode;
            *selected_note = selected;
            *note_count = count;
            *notes_delete_armed = delete_armed;
            *notes_field = field;
            return true;
        }
    }
    return false;
}

static void draw_app_text(uint8_t *frame, int x, int y, const char *text,
                          uint32_t length, uint8_t blue, uint8_t green, uint8_t red,
                          int scale)
{
    for (uint32_t i = 0; i < length; i++) {
        unsigned char character = (unsigned char)text[i];
        if (character == ' ') {
            continue;
        }
        const uint8_t *glyph;
        if (character >= 'A' && character <= 'Z') {
            glyph = font_5x7[character - 'A'];
        } else if (character >= '0' && character <= '9') {
            glyph = font_digits_5x7[character - '0'];
        } else if (character == '-') {
            glyph = font_minus_5x7;
        } else {
            continue;
        }
        for (int row = 0; row < 7; row++) {
            for (int column = 0; column < 5; column++) {
                if (glyph[row] & (1u << (4 - column))) {
                    for (int dy = 0; dy < scale; dy++) {
                        for (int dx = 0; dx < scale; dx++) {
                            size_t offset = ((size_t)(y + row * scale + dy) * FB_IMG_WIDTH +
                                             (size_t)(x + (int)i * 6 * scale +
                                                      column * scale + dx)) * GPU_BPP_2D;
                            frame[offset + 0] = blue;
                            frame[offset + 1] = green;
                            frame[offset + 2] = red;
                            frame[offset + 3] = 255;
                        }
                    }
                }
            }
        }
    }
}

static void draw_notes_text(uint8_t *frame, const char *text, uint32_t length,
                            uint8_t blue, uint8_t green, uint8_t red,
                            int requested_first_line, int *cursor_x, int *cursor_y)
{
    int total_lines = 1;
    int column = 0;
    for (uint32_t i = 0; i < length; i++) {
        if (text[i] == '\n') {
            total_lines++;
            column = 0;
        } else {
            if (column == NOTES_VISIBLE_COLUMNS) {
                total_lines++;
                column = 0;
            }
            column++;
        }
    }

    int maximum_first_line = total_lines > NOTES_VISIBLE_ROWS
        ? total_lines - NOTES_VISIBLE_ROWS : 0;
    int first_line = requested_first_line < 0 ? maximum_first_line :
        (requested_first_line > maximum_first_line ? maximum_first_line : requested_first_line);
    int line = 0;
    column = 0;
    for (uint32_t i = 0; i < length; i++) {
        char character = text[i];
        if (character == '\n') {
            line++;
            column = 0;
            continue;
        }
        if (column == NOTES_VISIBLE_COLUMNS) {
            line++;
            column = 0;
        }
        if (line >= first_line && line < first_line + NOTES_VISIBLE_ROWS) {
            draw_app_text(frame, 24 + column * 12,
                          137 + (line - first_line) * 18,
                          &character, 1, blue, green, red, 2);
        }
        column++;
    }
    if (column == NOTES_VISIBLE_COLUMNS) {
        line++;
        column = 0;
    }
    if (line >= first_line && line < first_line + NOTES_VISIBLE_ROWS) {
        *cursor_x = 24 + column * 12;
        *cursor_y = 136 + (line - first_line) * 18;
    } else {
        *cursor_x = -1;
        *cursor_y = -1;
    }
}

static void draw_rect(uint8_t *frame, int x, int y, int width, int height,
                      uint8_t blue, uint8_t green, uint8_t red)
{
    for (int py = y; py < y + height; py++) {
        for (int px = x; px < x + width; px++) {
            size_t offset = ((size_t)py * FB_IMG_WIDTH + (size_t)px) * GPU_BPP_2D;
            frame[offset + 0] = blue;
            frame[offset + 1] = green;
            frame[offset + 2] = red;
            frame[offset + 3] = 255;
        }
    }
}

static void draw_border(uint8_t *frame, int x, int y, int width, int height,
                        int thickness, uint8_t blue, uint8_t green, uint8_t red)
{
    draw_rect(frame, x, y, width, thickness, blue, green, red);
    draw_rect(frame, x, y + height - thickness, width, thickness, blue, green, red);
    draw_rect(frame, x, y, thickness, height, blue, green, red);
    draw_rect(frame, x + width - thickness, y, thickness, height, blue, green, red);
}

static void draw_label(uint8_t *frame, int x, int y, const char *text, int length,
                       uint8_t blue, uint8_t green, uint8_t red, int scale)
{
    draw_app_text(frame, x, y, text, (uint32_t)length, blue, green, red, scale);
}

static void draw_nav_button(uint8_t *frame, int x, int width, const char *label,
                            int label_length, bool selected, bool home)
{
    bool highlighted = home || selected;
    uint8_t blue = highlighted ? 80 : 79;
    uint8_t green = highlighted ? 57 : 48;
    uint8_t red = highlighted ? 24 : 41;
    uint8_t border_blue = selected ? 84 : (home ? 193 : 255);
    uint8_t border_green = selected ? 206 : (home ? 224 : 155);
    uint8_t border_red = selected ? 255 : (home ? 105 : 169);
    draw_rect(frame, x, 12, width, 32, blue, green, red);
    draw_border(frame, x, 12, width, 32, 2, border_blue, border_green, border_red);
    draw_label(frame, x + 8, 23, label,
               label_length, 245, 242, 232, 1);
}

static void draw_contacts_control(uint8_t *frame, int x, int width,
                                  const char *label, int label_length)
{
    draw_rect(frame, x, 198, width, 20, 75, 58, 32);
    draw_border(frame, x, 198, width, 20, 1, 117, 101, 74);
    draw_label(frame, x + (width - label_length * 6) / 2, 204,
               label, label_length, 245, 242, 232, 1);
}

static void draw_notes_control(uint8_t *frame, int x, int width,
                               const char *label, int label_length, bool enabled)
{
    draw_rect(frame, x, 198, width, 20,
              enabled ? 75 : 52, enabled ? 58 : 49, enabled ? 32 : 43);
    draw_border(frame, x, 198, width, 20, 1,
                enabled ? 117 : 83, enabled ? 101 : 76, enabled ? 74 : 61);
    if (label_length > 0) {
        draw_label(frame, x + (width - label_length * 6) / 2, 204,
                   label, label_length, 245, 242, 232, 1);
    }
}

static void draw_notes_controls(uint8_t *frame, uint32_t notes_mode,
                                uint32_t notes_field)
{
    const int x[5] = { 20, 72, 124, 176, 228 };
    const int width[5] = { 48, 48, 48, 48, 55 };
    if (notes_mode == 1) {
        draw_notes_control(frame, x[0], width[0], "OPEN", 4, true);
        draw_notes_control(frame, x[1], width[1], "UP", 2, true);
        draw_notes_control(frame, x[2], width[2], "DOWN", 4, true);
        draw_notes_control(frame, x[3], width[3], "NEW", 3, true);
        draw_notes_control(frame, x[4], width[4], "DELETE", 6, true);
    } else {
        draw_notes_control(frame, x[0], width[0], "LIST", 4, true);
        draw_notes_control(frame, x[1], width[1], "TITLE", 5, true);
        draw_notes_control(frame, x[2], width[2], "BODY", 4, true);
        draw_notes_control(frame, x[3], width[3],
                           notes_field == 0 ? "OLDER" : "", notes_field == 0 ? 5 : 0,
                           notes_field == 0);
        draw_notes_control(frame, x[4], width[4],
                           notes_field == 0 ? "NEWER" : "", notes_field == 0 ? 5 : 0,
                           notes_field == 0);
    }
}

static void draw_calculator_key(uint8_t *frame, int x, int y, int width,
                                const char *label, int label_length)
{
    draw_rect(frame, x, y, width, 22, 75, 58, 32);
    draw_border(frame, x, y, width, 22, 1, 117, 101, 74);
    draw_label(frame, x + (width - label_length * 6) / 2, y + 7,
               label, label_length, 245, 242, 232, 1);
}

static void draw_calculator_keypad(uint8_t *frame)
{
    static const char *labels[4][4] = {
        { "7", "8", "9", "DIV" },
        { "4", "5", "6", "MUL" },
        { "1", "2", "3", "SUB" },
        { "0", "DEL", "CLR", "ADD" },
    };
    draw_rect(frame, 12, 222, 276, 140, 53, 38, 18);
    draw_border(frame, 12, 222, 276, 140, 2, 89, 73, 51);
    for (int row = 0; row < 4; row++) {
        for (int column = 0; column < 4; column++) {
            const char *label = labels[row][column];
            int length = 1;
            while (label[length] != '\0') {
                length++;
            }
            draw_calculator_key(frame, 16 + column * 67, 226 + row * 26,
                                60, label, length);
        }
    }
    draw_calculator_key(frame, 16, 330, 261, "ENTER", 5);
}

static void draw_phone_keypad(uint8_t *frame)
{
    static const char *labels[4][4] = {
        { "7", "8", "9", "" },
        { "4", "5", "6", "" },
        { "1", "2", "3", "" },
        { "0", "DEL", "CLR", "" },
    };
    draw_rect(frame, 12, 222, 276, 140, 53, 38, 18);
    draw_border(frame, 12, 222, 276, 140, 2, 89, 73, 51);
    for (int row = 0; row < 4; row++) {
        for (int column = 0; column < 4; column++) {
            const char *label = labels[row][column];
            if (label[0] == '\0') {
                continue;
            }
            int length = 1;
            while (label[length] != '\0') {
                length++;
            }
            draw_calculator_key(frame, 16 + column * 67, 226 + row * 26,
                                60, label, length);
        }
    }
    draw_calculator_key(frame, 16, 330, 261, "CALL", 4);
}

static void draw_notes_list(uint8_t *frame, const char *text, uint32_t length,
                            uint32_t selected_note, uint32_t note_count)
{
    uint32_t offset = 0;
    for (uint32_t index = 0; index < note_count && index < 4 && offset < length; index++) {
        char line[22] = { 0 };
        int line_length = 0;
        while (offset < length && text[offset] != '\n' && line_length < 21) {
            line[line_length++] = text[offset++];
        }
        if (offset < length && text[offset] == '\n') {
            offset++;
        }
        int y = 130 + (int)index * 15;
        if (index == selected_note) {
            draw_rect(frame, 20, y, 260, 14, 80, 57, 24);
            draw_border(frame, 20, y, 260, 14, 1, 84, 206, 255);
        }
        draw_app_text(frame, 24, y + 3, line, (uint32_t)line_length,
                      216, 212, 189, 1);
    }
}

#ifndef NEXATRODE_CURSOR_SCALE
#define NEXATRODE_CURSOR_SCALE 4
#endif

static void draw_pointer_cursor(uint8_t *frame, uint32_t x, uint32_t y)
{
    /* Neon light-blue arrow with a dark halo so UTM USB Tablet mode stays aimable.
     * The mark was sized for a 300px-wide frame. On 1080 it needs to be larger. */
    static const char *rows[] = {
        "X      ",
        "XX     ",
        "XXX    ",
        "XXXX   ",
        "XXXXX  ",
        "XXXXXX ",
        "XXXXXXX",
        "XXXX   ",
        "XXX    ",
        "XX     ",
        "X      ",
    };
    const int scale = NEXATRODE_CURSOR_SCALE;
    for (int row = 0; row < 11; row++) {
        for (int column = 0; column < 7; column++) {
            if (rows[row][column] != 'X') {
                continue;
            }
            int px = (int)x + column * scale;
            int py = (int)y + row * scale;
            for (int sy = 0; sy < scale; sy++) {
                for (int sx = 0; sx < scale; sx++) {
                    int qx = px + sx;
                    int qy = py + sy;
                    if (qx < 0 || qy < 0 || qx >= FB_IMG_WIDTH || qy >= FB_IMG_HEIGHT) {
                        continue;
                    }
                    size_t offset = ((size_t)qy * FB_IMG_WIDTH + (size_t)qx) * GPU_BPP_2D;
                    frame[offset] = 255;
                    frame[offset + 1] = 244;
                    frame[offset + 2] = 64;
                    frame[offset + 3] = 255;
                }
            }
        }
    }
    for (int row = 0; row < 11; row++) {
        for (int column = 0; column < 7; column++) {
            if (rows[row][column] != 'X') {
                continue;
            }
            int px = (int)x + column * scale;
            int py = (int)y + row * scale;
            for (int sy = 0; sy < scale; sy++) {
                for (int sx = 0; sx < scale; sx++) {
                    int qx = px + sx;
                    int qy = py + sy;
                    for (int oy = -1; oy <= 1; oy++) {
                        for (int ox = -1; ox <= 1; ox++) {
                            if (ox == 0 && oy == 0) {
                                continue;
                            }
                            int hx = qx + ox;
                            int hy = qy + oy;
                            if (hx < 0 || hy < 0 || hx >= FB_IMG_WIDTH || hy >= FB_IMG_HEIGHT) {
                                continue;
                            }
                            int lx = hx - (int)x;
                            int ly = hy - (int)y;
                            if (lx >= 0 && ly >= 0) {
                                int cover_column = lx / scale;
                                int cover_row = ly / scale;
                                if (cover_column < 7 && cover_row < 11 &&
                                    rows[cover_row][cover_column] == 'X') {
                                    continue;
                                }
                            }
                            size_t outline = ((size_t)hy * FB_IMG_WIDTH + (size_t)hx) * GPU_BPP_2D;
                            frame[outline] = 18;
                            frame[outline + 1] = 21;
                            frame[outline + 2] = 11;
                        }
                    }
                }
            }
        }
    }
}

static void draw_app_page(uint8_t *frame, uint32_t focus, const char *text,
                          uint32_t length, uint32_t scroll_start_line,
                          uint32_t notes_mode, uint32_t selected_note,
                          uint32_t note_count, uint32_t notes_delete_armed,
                          uint32_t notes_field)
{
    /* Keep the already-rendered keyboard below y=222 and replace the launcher. */
    draw_rect(frame, 0, 0, FB_IMG_WIDTH, 222, 34, 21, 11);
    draw_rect(frame, 0, 0, FB_IMG_WIDTH, 3, 193, 224, 105);
    draw_nav_button(frame, 12, 32, "HOME", 4, false, true);
    draw_nav_button(frame, 46, 24, "NT", 2, focus == 1, false);
    draw_nav_button(frame, 72, 24, "CT", 2, focus == 2, false);
    draw_nav_button(frame, 98, 24, "CA", 2, focus == 3, false);
    draw_nav_button(frame, 124, 24, "CK", 2, focus == 4, false);
    draw_nav_button(frame, 150, 24, "CL", 2, focus == 5, false);
    draw_nav_button(frame, 176, 24, "MS", 2, focus == 6, false);
    draw_nav_button(frame, 202, 24, "PH", 2, focus == 8, false);
    draw_nav_button(frame, 228, 24, "ST", 2, focus == 7, false);

    if (focus == 1) {
        char status_body[] = "BODY 1 OF 1";
        char status_title[] = "TITLE 1 OF 1";
        char *status = notes_field == 1 ? status_title : status_body;
        uint32_t status_length = notes_field == 1 ? 12 : 11;
        uint32_t selected_index = notes_field == 1 ? 6 : 5;
        uint32_t count_index = notes_field == 1 ? 11 : 10;
        status[selected_index] = '1' + (selected_note < 4 ? selected_note : 0);
        status[count_index] = '1' + (note_count > 0 && note_count <= 4 ? note_count - 1 : 0);
        if (notes_mode == 1) {
            draw_label(frame, 18, 58, "MY NOTES", 8, 245, 242, 232, 2);
            if (notes_delete_armed) {
                draw_label(frame, 20, 82, "TAP DELETE AGAIN", 16, 193, 241, 131, 1);
            } else {
                draw_label(frame, 20, 82, "TAP A NOTE TO OPEN", 18, 216, 212, 189, 1);
            }
        } else {
            draw_label(frame, 18, 58, "PRIVATE NOTES", 13, 245, 242, 232, 2);
            draw_label(frame, 20, 82, status, (int)status_length, 216, 212, 189, 1);
        }
    } else if (focus == 2) {
        draw_label(frame, 18, 58, "CONTACTS", 8, 255, 241, 244, 2);
        draw_label(frame, 20, 82, "PEOPLE AND DETAILS", 18, 228, 204, 208, 1);
    } else if (focus == 3) {
        draw_label(frame, 18, 58, "CALCULATOR", 10, 245, 242, 232, 2);
        draw_label(frame, 20, 82, "FOUR FUNCTION INTEGER", 21, 216, 212, 189, 1);
    } else if (focus == 4) {
        draw_label(frame, 18, 58, "CLOCK", 5, 245, 242, 232, 2);
        draw_label(frame, 20, 82, "UTC DATE AND TIME FROM RTC", 26, 216, 212, 189, 1);
    } else if (focus == 5) {
        draw_label(frame, 18, 58, "CALENDAR", 8, 224, 236, 255, 2);
        draw_label(frame, 20, 82, "LOCAL REMINDERS", 15, 189, 204, 228, 1);
    } else if (focus == 6) {
        draw_label(frame, 18, 58, "MESSAGES", 8, 240, 255, 224, 2);
        draw_label(frame, 20, 82, "EXPLICIT CONTACT ONLY", 21, 208, 228, 189, 1);
    } else if (focus == 8) {
        draw_label(frame, 18, 58, "PHONE", 5, 224, 240, 255, 2);
        draw_label(frame, 20, 82, "FAKE CARRIER", 12, 189, 208, 228, 1);
    } else {
        draw_label(frame, 18, 58, "SETTINGS", 8, 245, 242, 232, 2);
        draw_label(frame, 20, 82, "LOCAL DEVICE OPTIONS", 20, 216, 212, 189, 1);
    }

    draw_rect(frame, 12, 105, 276, 89, 53, 38, 18);
    {
        uint8_t border_r = 255;
        uint8_t border_g = 155;
        uint8_t border_b = 169;
        if (focus == 1) {
            border_r = 193;
            border_g = 224;
            border_b = 105;
        } else if (focus == 3) {
            border_r = 84;
            border_g = 206;
            border_b = 255;
        } else if (focus == 4) {
            border_r = 193;
            border_g = 241;
            border_b = 131;
        } else if (focus == 5) {
            border_r = 120;
            border_g = 168;
            border_b = 255;
        } else if (focus == 6) {
            border_r = 180;
            border_g = 220;
            border_b = 120;
        } else if (focus == 7) {
            border_r = 200;
            border_g = 180;
            border_b = 160;
        } else if (focus == 8) {
            border_r = 120;
            border_g = 168;
            border_b = 255;
        }
        draw_border(frame, 12, 105, 276, 89, 2, border_r, border_g, border_b);
    }
    if (focus == 1 && notes_mode == 1) {
        draw_label(frame, 24, 114, "SAVED NOTES", 11, 216, 212, 189, 1);
        draw_notes_list(frame, text, length, selected_note, note_count);
    } else if (focus == 1 && notes_field == 1) {
        draw_label(frame, 24, 114, "TITLE", 5, 216, 212, 189, 1);
        draw_app_text(frame, 24, 137, text, length, 216, 212, 189, 2);
        int cursor_x = 24 + (int)length * 12;
        if (length <= 21) {
            draw_rect(frame, cursor_x, 136, 2, 16, 193, 241, 131);
        }
    } else if (focus == 1) {
        draw_label(frame, 24, 114, "NOTE TEXT", 9, 216, 212, 189, 1);
        int cursor_x;
        int cursor_y;
        draw_notes_text(frame, text, length, 216, 212, 189,
                        (int)scroll_start_line,
                        &cursor_x, &cursor_y);
        if (cursor_x >= 0) {
            draw_rect(frame, cursor_x, cursor_y, 2, 16, 193, 241, 131);
        }
    } else if (focus == 2) {
        draw_label(frame, 24, 114, "CONTACT LIST", 12, 228, 204, 208, 1);
        int cursor_x;
        int cursor_y;
        draw_notes_text(frame, text, length, 228, 204, 208, -1,
                        &cursor_x, &cursor_y);
        if (cursor_x >= 0) {
            draw_rect(frame, cursor_x, cursor_y, 2, 16, 193, 241, 131);
        }
    } else if (focus == 3) {
        draw_label(frame, 24, 114, "RESULT", 6, 216, 212, 189, 1);
        draw_app_text(frame, 24, 143, text, length, 216, 212, 189, 2);
    } else if (focus == 4) {
        const char *date = text;
        uint32_t date_length = length;
        const char *time = "";
        uint32_t time_length = 0;
        for (uint32_t index = 0; index < length; index++) {
            if (text[index] == '\n') {
                date_length = index;
                if (index + 1 < length) {
                    time = text + index + 1;
                    time_length = length - index - 1;
                }
                break;
            }
        }
        draw_label(frame, 24, 114, "UTC DATE", 8, 193, 241, 131, 1);
        draw_app_text(frame, 24, 132, date, date_length, 193, 241, 131, 2);
        draw_label(frame, 24, 156, "UTC TIME", 8, 193, 241, 131, 1);
        draw_app_text(frame, 102, 174, time, time_length, 193, 241, 131, 2);
    } else if (focus == 5) {
        draw_label(frame, 24, 114, "REMINDERS", 9, 189, 204, 228, 1);
        int cursor_x;
        int cursor_y;
        draw_notes_text(frame, text, length, 189, 204, 228, -1,
                        &cursor_x, &cursor_y);
        if (cursor_x >= 0) {
            draw_rect(frame, cursor_x, cursor_y, 2, 16, 193, 241, 131);
        }
    } else if (focus == 6) {
        draw_label(frame, 24, 114, "PEER KEYS", 9, 208, 228, 189, 1);
        int cursor_x;
        int cursor_y;
        draw_notes_text(frame, text, length, 208, 228, 189, -1,
                        &cursor_x, &cursor_y);
        if (cursor_x >= 0) {
            draw_rect(frame, cursor_x, cursor_y, 2, 16, 193, 241, 131);
        }
    } else if (focus == 8) {
        draw_label(frame, 24, 114, "STATUS", 6, 189, 208, 228, 1);
        int cursor_x;
        int cursor_y;
        draw_notes_text(frame, text, length, 189, 208, 228, -1,
                        &cursor_x, &cursor_y);
        if (cursor_x >= 0) {
            draw_rect(frame, cursor_x, cursor_y, 2, 16, 193, 241, 131);
        }
    } else {
        draw_label(frame, 24, 114, "STATUS", 6, 216, 212, 189, 1);
        int cursor_x;
        int cursor_y;
        draw_notes_text(frame, text, length, 216, 212, 189, -1,
                        &cursor_x, &cursor_y);
        if (cursor_x >= 0) {
            draw_rect(frame, cursor_x, cursor_y, 2, 16, 193, 241, 131);
        }
    }
    if (focus == 1) {
        draw_notes_controls(frame, notes_mode, notes_field);
    } else if (focus == 3) {
        draw_label(frame, 23, 204, "F1 ADD  F2 SUB  ENTER EQUALS", 28,
                   190, 168, 139, 1);
        draw_calculator_keypad(frame);
    } else if (focus == 4) {
        draw_label(frame, 20, 204, "LIVE UTC DATE AND TIME", 22, 190, 184, 139, 1);
    } else if (focus == 6) {
        bool empty_peers = length >= 10 && text[0] == 'N' && text[1] == 'O' &&
                           text[2] == ' ' && text[3] == 'T';
        bool adding_key = length >= 7 && text[0] == 'A' && text[1] == 'D' &&
                          text[2] == 'D' && text[3] == ' ' && text[4] == 'K';
        bool composing = length >= 12 && text[0] == 'C' && text[1] == 'O' &&
                         text[2] == 'M' && text[3] == 'P' && text[4] == 'O';
        if (empty_peers) {
            draw_label(frame, 20, 204, "ENTER ADD A PEER KEY", 20,
                       190, 168, 139, 1);
        } else if (adding_key) {
            draw_label(frame, 20, 204, "HEX 0-9 A-F  ENTER AT 8", 23,
                       190, 168, 139, 1);
        } else if (composing) {
            draw_label(frame, 20, 204, "A-Z BODY  ENTER SEAL", 20,
                       190, 168, 139, 1);
        } else {
            draw_contacts_control(frame, 20, 84, "UP", 2);
            draw_contacts_control(frame, 108, 76, "DOWN", 4);
            draw_contacts_control(frame, 188, 95, "DELETE", 6);
        }
    } else if (focus == 7) {
        draw_label(frame, 20, 204, "LOCAL ONLY  NO NET YET", 22, 190, 184, 139, 1);
    } else if (focus == 8) {
        draw_label(frame, 20, 204, "DEL CLR  ENTER CALL", 19, 190, 168, 139, 1);
        draw_phone_keypad(frame);
    } else {
        bool editing = length >= 4 && text[0] == 'A' && text[1] == 'D' &&
                       text[2] == 'D' && text[3] == ' ';
        bool empty_contacts = length >= 11 && text[0] == 'N' && text[1] == 'O' &&
                              text[2] == ' ' && text[3] == 'C';
        bool empty_reminders = length >= 12 && text[0] == 'N' && text[1] == 'O' &&
                               text[2] == ' ' && text[3] == 'R';
        if (editing) {
            draw_label(frame, 20, 204, "ENTER NEXT  BACKSPACE EDIT", 26,
                       190, 168, 139, 1);
        } else if (empty_contacts) {
            draw_label(frame, 20, 204, "ENTER ADD A CONTACT", 19,
                       190, 168, 139, 1);
        } else if (empty_reminders) {
            draw_label(frame, 20, 204, "ENTER ADD A REMINDER", 20,
                       190, 168, 139, 1);
        } else {
            draw_contacts_control(frame, 20, 84, "UP", 2);
            draw_contacts_control(frame, 108, 76, "DOWN", 4);
            draw_contacts_control(frame, 188, 95, "DELETE", 6);
        }
    }
}

#define CURSOR_SCALE NEXATRODE_CURSOR_SCALE
#define CURSOR_COLS 7
#define CURSOR_ROWS 11
#define CURSOR_BACKUP_W (40 * CURSOR_SCALE / 4)
#define CURSOR_BACKUP_H (56 * CURSOR_SCALE / 4)

static int cursor_saved;
static int saved_x, saved_y, saved_w, saved_h;
static uint8_t cursor_backup[CURSOR_BACKUP_W * CURSOR_BACKUP_H * 4];

static void flush_band(int y, int height)
{
    if (height <= 0) {
        return;
    }
    if (y < 0) {
        height += y;
        y = 0;
    }
    if (y >= FB_IMG_HEIGHT || height <= 0) {
        return;
    }
    if (y + height > FB_IMG_HEIGHT) {
        height = FB_IMG_HEIGHT - y;
    }
    gpu_rect_t rect = {
        .x = 0,
        .y = y,
        .width = FB_IMG_WIDTH,
        .height = height,
    };
    int err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_TRANSFER_TO_2D,
        .id = get_and_inc_req_id(),
        .transfer_to_2d = {
            .resource_id = 1,
            .rect = rect,
            .mem_offset = (uint64_t)y * FB_IMG_WIDTH * GPU_BPP_2D,
        },
    });
    if (err) {
        return;
    }
    uint32_t flush_id = get_and_inc_req_id();
    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_RESOURCE_FLUSH,
        .id = flush_id,
        .resource_flush = {
            .resource_id = 1,
            .rect = rect,
        },
    });
    if (err) {
        return;
    }
#if defined(NEXATRODE_GPU_PERF_TRACE)
    sddf_printf("PERF|GPU_FLUSH|%u|%llu|%d|%d\n", flush_id,
                (unsigned long long)gpu_perf_time_us(), y, height);
#endif
}

static void restore_cursor_outside(uint8_t *frame, int band_y, int band_bottom)
{
    if (!cursor_saved) {
        return;
    }
    for (int row = 0; row < saved_h; row++) {
        int frame_y = saved_y + row;
        if (band_bottom > band_y && frame_y >= band_y && frame_y < band_bottom) {
            continue;
        }
        memcpy(frame + ((size_t)frame_y * FB_IMG_WIDTH + (size_t)saved_x) * GPU_BPP_2D,
               cursor_backup + (size_t)row * (size_t)saved_w * GPU_BPP_2D,
               (size_t)saved_w * GPU_BPP_2D);
    }
    cursor_saved = 0;
}

static void backup_cursor(uint8_t *frame, uint32_t x, uint32_t y)
{
    int left = (int)x - 1;
    int top = (int)y - 1;
    int right = (int)x + CURSOR_COLS * CURSOR_SCALE + 1;
    int bottom = (int)y + CURSOR_ROWS * CURSOR_SCALE + 1;
    if (left < 0) {
        left = 0;
    }
    if (top < 0) {
        top = 0;
    }
    if (right > FB_IMG_WIDTH) {
        right = FB_IMG_WIDTH;
    }
    if (bottom > FB_IMG_HEIGHT) {
        bottom = FB_IMG_HEIGHT;
    }
    saved_w = right - left;
    saved_h = bottom - top;
    if (saved_w <= 0 || saved_h <= 0 || saved_w > CURSOR_BACKUP_W || saved_h > CURSOR_BACKUP_H) {
        cursor_saved = 0;
        return;
    }
    saved_x = left;
    saved_y = top;
    for (int row = 0; row < saved_h; row++) {
        memcpy(cursor_backup + (size_t)row * (size_t)saved_w * GPU_BPP_2D,
               frame + ((size_t)(saved_y + row) * FB_IMG_WIDTH + (size_t)saved_x) * GPU_BPP_2D,
               (size_t)saved_w * GPU_BPP_2D);
    }
    cursor_saved = 1;
}

static int span_inside(int top, int bottom, uint32_t y, uint32_t height)
{
    return top >= 0 && bottom > top && top >= (int)y && bottom <= (int)(y + height);
}

static void do_draw_state(void)
{
    uint32_t focus = *(volatile uint32_t *)ui_focus;
    ui_pointer_t pointer = { 0 };
    if (ui_pointer != 0) {
        volatile ui_pointer_t *shared = (volatile ui_pointer_t *)ui_pointer;
        uint32_t first = shared->revision;
        if ((first & 1U) == 0) {
            pointer.x = shared->x;
            pointer.y = shared->y;
            pointer.visible = shared->visible;
            pointer.revision = shared->revision;
            if (pointer.revision != first) {
                pointer.visible = 0;
            }
        }
    }
    char app_text[APP_VIEW_TEXT_CAPACITY + 1] = { 0 };
    uint32_t app_text_length = 0;
    uint32_t view_revision = 0;
    uint32_t scroll_start_line = 0;
    uint32_t notes_mode = 0;
    uint32_t selected_note = 0;
    uint32_t note_count = 1;
    uint32_t notes_delete_armed = 0;
    uint32_t notes_field = 0;
    if (focus == 1 && !read_app_view(notes_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    if (focus == 2 && !read_app_view(contacts_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    if (focus == 3 && !read_app_view(calculator_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    if (focus == 4 && !read_app_view(clock_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    if (focus == 5 && !read_app_view(calendar_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    if (focus == 6 && !read_app_view(messages_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    if (focus == 7 && !read_app_view(settings_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    if (focus == 8 && !read_app_view(phone_view, app_text, &app_text_length,
                                     &view_revision, &scroll_start_line, &notes_mode,
                                     &selected_note, &note_count, &notes_delete_armed,
                                     &notes_field)) {
        return;
    }
    const slint_frame_header_t *slint_surface = (const slint_frame_header_t *)slint_frame;
    uint32_t slint_revision = 0;
    bool slint_frame_ready = focus == 1 && slint_frame != 0 &&
        FB_IMG_WIDTH == SLINT_FRAME_WIDTH && FB_IMG_HEIGHT == SLINT_FRAME_HEIGHT &&
        slint_frame_header_valid(slint_surface, SLINT_FRAME_REGION_BYTES, &slint_revision);
    if (focus != rendered_focus || view_revision != rendered_view_revision
        || pointer.revision != rendered_pointer_revision
        || (slint_frame_ready && slint_revision != rendered_slint_frame_revision)) {
        /* A full queue used to abort this program and leave the last frame up. */
        if (gpu_queue_length_req(&gpu_queue_handle) + 8 > gpu_queue_handle.capacity) {
            sddf_printf("GPU|QUEUE FULL\n");
            return;
        }
        uint8_t *frame = (uint8_t *)(gpu_data + DISPLAY_INFO_DATA_OFFSET);
        size_t frame_size = FB_IMG_WIDTH * FB_IMG_HEIGHT * GPU_BPP_2D;
        bool have_slint = rendered_slint_frame_revision != UINT32_MAX;
        /* A click publishes the Slint frame while this copy may already be
         * running. Keep the picture on screen instead of the old notes page. */
        if (!slint_frame_ready && have_slint) {
            return;
        }

        bool content = false;
        bool drew_slint_frame = false;
        uint32_t band_y = 0;
        uint32_t band_h = FB_IMG_HEIGHT;
        int old_top = cursor_saved ? saved_y : -1;
        int old_bottom = cursor_saved ? saved_y + saved_h : -1;
        bool pointer_only = slint_frame_ready && have_slint &&
            slint_revision == rendered_slint_frame_revision;

        if (!slint_frame_ready) {
            size_t image_bytes = (size_t)(_fb_img_end - _fb_img);
            if (image_bytes > frame_size) {
                image_bytes = frame_size;
            }
            memset(frame, 0, frame_size);
            memcpy(frame, _fb_img, image_bytes);
            cursor_saved = 0;
            old_top = -1;
            content = true;
        } else if (!pointer_only) {
            uint32_t copied_revision, y, height;
#if defined(NEXATRODE_GPU_PERF_TRACE)
            uint64_t copy_start = gpu_perf_time_us();
#endif
            drew_slint_frame = slint_frame_copy_update_to_bgra(
                slint_surface,
                (const uint8_t *)slint_surface + sizeof(*slint_surface),
                SLINT_FRAME_REGION_BYTES, frame, frame_size,
                slint_full_retry ? UINT32_MAX : rendered_slint_frame_revision,
                &copied_revision, &y, &height);
            if (!drew_slint_frame) {
                /* A racing writer may have changed pixels during our copy.
                 * The next attempt must reconstruct the entire GPU buffer. */
                slint_full_retry = true;
                sddf_printf("GPU|SLINT COPY FAIL\n");
                return;
            }
            if (drew_slint_frame) {
                slint_full_retry = false;
                slint_revision = copied_revision;
#if defined(NEXATRODE_GPU_PERF_TRACE)
                sddf_printf("PERF|GPU_COPY|%u|%llu|%llu|%u\n", copied_revision,
                            (unsigned long long)copy_start,
                            (unsigned long long)gpu_perf_time_us(), height);
#endif
                sddf_printf("GPU|SLINT COPY rev=%u y=%u h=%u\n", slint_revision, y, height);
                content = true;
                band_y = y;
                band_h = height;
                restore_cursor_outside(frame, (int)band_y, (int)(band_y + band_h));
            }
        } else {
            restore_cursor_outside(frame, 0, 0);
        }

        /* Legacy C notes/contacts UI was authored for a ~300px panel. On the
         * 1080 Slint image it flashes as a tiny PRIVATE NOTES strip whenever
         * ui_focus is set but the Slint surface is not copyable yet. Slint is
         * always mapped here — never fall back to that old page. */
        if (!drew_slint_frame && !have_slint && !pointer_only && slint_frame == 0 &&
            (focus == 1 || focus == 2 || focus == 3 || focus == 4 || focus == 5
            || focus == 6 || focus == 7 || focus == 8)) {
            draw_app_page(frame, focus, app_text, app_text_length, scroll_start_line,
                          notes_mode, selected_note, note_count, notes_delete_armed,
                          notes_field);
            content = true;
            band_y = 0;
            band_h = FB_IMG_HEIGHT;
        }
        if (pointer.visible) {
            backup_cursor(frame, pointer.x, pointer.y);
            draw_pointer_cursor(frame, pointer.x, pointer.y);
        }
        int new_top = cursor_saved ? saved_y : -1;
        int new_bottom = cursor_saved ? saved_y + saved_h : -1;

        if (rendered_focus == UINT32_MAX) {
            gpu_rect_t full = {
                .x = 0,
                .y = 0,
                .width = FB_IMG_WIDTH,
                .height = FB_IMG_HEIGHT,
            };
            draw_image(1, FB_IMG_WIDTH, FB_IMG_HEIGHT, 0, full);
        } else {
            if (content) {
                flush_band((int)band_y, (int)band_h);
            }
            bool pointer_moved = pointer.revision != rendered_pointer_revision;
            if (old_top >= 0 && (pointer_moved || !pointer.visible) &&
                !(content && span_inside(old_top, old_bottom, band_y, band_h))) {
                flush_band(old_top, old_bottom - old_top);
            }
            if (pointer.visible && new_top >= 0 && pointer_moved &&
                !(content && span_inside(new_top, new_bottom, band_y, band_h))) {
                flush_band(new_top, new_bottom - new_top);
            }
        }
        rendered_focus = focus;
        rendered_view_revision = view_revision;
        rendered_pointer_revision = pointer.revision;
        if (drew_slint_frame) {
            rendered_slint_frame_revision = slint_revision;
        } else if (!have_slint) {
            rendered_slint_frame_revision = UINT32_MAX;
        }
        microkit_notify(VIRT_CH);
    }
}

static void handle_display_info_response()
{
    memcpy(&display_info, (void *)(gpu_data), sizeof(gpu_resp_get_display_info_t));
    LOG_GPU_CLIENT("Display info received: num_scanouts=%d\n", display_info.num_scanouts);
    for (int i = 0; i < display_info.num_scanouts; i++) {
        LOG_GPU_CLIENT("Scanout %d: enabled=%d, rect=(%d, %d, %d, %d) {x, y, "
                       "width, height}\n",
                       i, display_info.scanouts[i].enabled, display_info.scanouts[i].rect.x,
                       display_info.scanouts[i].rect.y, display_info.scanouts[i].rect.width,
                       display_info.scanouts[i].rect.height);
    }
}

static void request_display_info()
{
    int err = 0;
    display_info_req_id = get_and_inc_req_id();
    err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
        .code = GPU_REQ_GET_DISPLAY_INFO,
        .id = display_info_req_id,
        .get_display_info = {
            .mem_offset = 0,
        },
    });
    assert(!err);
}

void init(void)
{
    LOG_GPU_CLIENT("Initialising GPU client!\n");
    /* Assert image size does not exceed data region size */
    assert(FB_IMG_WIDTH * FB_IMG_HEIGHT * GPU_BPP_2D + DISPLAY_INFO_DATA_OFFSET <= GPU_DATA_REGION_SIZE_CLI0);
    gpu_queue_init(&gpu_queue_handle, gpu_req_queue, gpu_resp_queue, GPU_QUEUE_CAPACITY_CLI0);
    memcpy((void *)(gpu_data + 0x1000), (void *)_fb_img, (size_t)(_fb_img_end - _fb_img));

    /* As part of initialisation, request for display info before sending anything else */
    LOG_GPU_CLIENT("Requesting initial display info\n");
    request_display_info();
    microkit_notify(VIRT_CH);
}

void notified(microkit_channel ch)
{
    int err = 0;
    if (!display_info_init) {
        if (ch == VIRT_CH) {
            gpu_resp_t resp = { 0 };
            err = gpu_dequeue_resp(&gpu_queue_handle, &resp);
            assert(!err);
            assert(resp.id == display_info_req_id);
            LOG_GPU_CLIENT("Initialised display info\n");
            handle_display_info_response();
            assert(display_info.num_scanouts > 0);
            assert(display_info.scanouts[0].enabled);
            display_info_init = true;
            LOG_GPU_CLIENT("Now starting the draw state machine\n");
            do_draw_state();
            /* Poll the Slint frame. A publish during sync can miss the
             * one-shot focus notification, and this client is above the
             * network so a copy finishes once it is queued. */
            sddf_timer_set_timeout(TIMER_CH, 200000000ull);
        }
        return;
    }

    if (ch == TIMER_CH) {
        do_draw_state();
        sddf_timer_set_timeout(TIMER_CH, 200000000ull);
        return;
    }

    if (ch == UI_FOCUS_CH) {
        do_draw_state();
        return;
    }

    LOG_GPU_CLIENT("Received notification from virtualiser\n");

    if (gpu_events_check_display_info(gpu_events) && !pending_display_info_request) {
        gpu_events_clear_display_info(gpu_events);
        LOG_GPU_CLIENT("Display info event received\n");
        LOG_GPU_CLIENT("Requesting display info\n");
        request_display_info();
        pending_display_info_request = true;
        microkit_notify(VIRT_CH);
        return;
    }

    gpu_resp_t resp = { 0 };
    while (!gpu_queue_empty_resp(&gpu_queue_handle)) {
        err = gpu_dequeue_resp(&gpu_queue_handle, &resp);
        assert(!err);
        if (resp.status != GPU_RESP_OK) {
            LOG_GPU_CLIENT_ERR("Failed request %d with status %d\n", resp.id, resp.status);
            continue;
        }
#if defined(NEXATRODE_GPU_PERF_TRACE)
        sddf_printf("PERF|GPU_RESPONSE|%u|%llu\n", resp.id,
                    (unsigned long long)gpu_perf_time_us());
#endif
        LOG_GPU_CLIENT("Response received for request %d\n", resp.id);
        if (resp.id == display_info_req_id) {
            handle_display_info_response();
            pending_display_info_request = false;
        }
    }
    /* A tap can publish the Slint frame while this scanout is in flight.
     * Present that frame when the virtio response lands. */
    do_draw_state();
}
