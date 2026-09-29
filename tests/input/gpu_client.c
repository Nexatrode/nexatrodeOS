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
uintptr_t notes_view;
uintptr_t contacts_view;
uintptr_t calculator_view;
uintptr_t clock_view;

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

static void draw_app_page(uint8_t *frame, uint32_t focus, const char *text,
                          uint32_t length, uint32_t scroll_start_line,
                          uint32_t notes_mode, uint32_t selected_note,
                          uint32_t note_count, uint32_t notes_delete_armed,
                          uint32_t notes_field)
{
    /* Keep the already-rendered keyboard below y=222 and replace the launcher. */
    draw_rect(frame, 0, 0, FB_IMG_WIDTH, 222, 34, 21, 11);
    draw_rect(frame, 0, 0, FB_IMG_WIDTH, 3, 193, 224, 105);
    draw_nav_button(frame, 12, 48, "HOME", 4, false, true);
    draw_nav_button(frame, 64, 54, "NOTES", 5, focus == 1, false);
    draw_nav_button(frame, 122, 73, "CONTACTS", 8, focus == 2, false);
    draw_nav_button(frame, 199, 40, "CALC", 4, focus == 3, false);
    draw_nav_button(frame, 243, 45, "CLOCK", 5, focus == 4, false);

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
    } else {
        draw_label(frame, 18, 58, "CLOCK", 5, 245, 242, 232, 2);
        draw_label(frame, 20, 82, "UTC TIME FROM RTC", 17, 216, 212, 189, 1);
    }

    draw_rect(frame, 12, 105, 276, 89, 53, 38, 18);
    draw_border(frame, 12, 105, 276, 89, 2,
                focus == 1 ? 193 : (focus == 3 ? 84 : (focus == 4 ? 193 : 255)),
                focus == 1 ? 224 : (focus == 3 ? 206 : (focus == 4 ? 241 : 155)),
                focus == 1 ? 105 : (focus == 3 ? 255 : (focus == 4 ? 131 : 169)));
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
    } else {
        draw_label(frame, 24, 114, "UTC TIME", 8, 193, 241, 131, 1);
        draw_app_text(frame, 102, 144, text, length, 193, 241, 131, 2);
    }
    if (focus == 1) {
        draw_notes_controls(frame, notes_mode, notes_field);
    } else if (focus == 3) {
        draw_label(frame, 23, 204, "F1 ADD  F2 SUB  ENTER EQUALS", 28,
                   190, 168, 139, 1);
        draw_calculator_keypad(frame);
    } else if (focus == 4) {
        draw_label(frame, 20, 204, "LIVE UTC TIME", 13, 190, 184, 139, 1);
    } else {
        bool editing = length >= 4 && text[0] == 'A' && text[1] == 'D' &&
                       text[2] == 'D' && text[3] == ' ';
        bool empty = length >= 11 && text[0] == 'N' && text[1] == 'O' &&
                     text[2] == ' ' && text[3] == 'C';
        if (editing) {
            draw_label(frame, 20, 204, "ENTER NEXT  BACKSPACE EDIT", 26,
                       190, 168, 139, 1);
        } else if (empty) {
            draw_label(frame, 20, 204, "ENTER ADD A CONTACT", 19,
                       190, 168, 139, 1);
        } else {
            draw_contacts_control(frame, 20, 84, "UP", 2);
            draw_contacts_control(frame, 108, 76, "DOWN", 4);
            draw_contacts_control(frame, 188, 95, "DELETE", 6);
        }
    }
}

static void do_draw_state(void)
{
    uint32_t focus = *(volatile uint32_t *)ui_focus;
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
    if (focus != rendered_focus || view_revision != rendered_view_revision) {
        uint8_t *frame = (uint8_t *)(gpu_data + DISPLAY_INFO_DATA_OFFSET);
        size_t frame_size = FB_IMG_WIDTH * FB_IMG_HEIGHT * GPU_BPP_2D;
        memcpy(frame, _fb_img, frame_size);

        if (focus == 1 || focus == 2 || focus == 3 || focus == 4) {
            draw_app_page(frame, focus, app_text, app_text_length, scroll_start_line,
                          notes_mode, selected_note, note_count, notes_delete_armed,
                          notes_field);
        }

        gpu_rect_t full = {
            .x = 0,
            .y = 0,
            .width = FB_IMG_WIDTH,
            .height = FB_IMG_HEIGHT,
        };
        if (rendered_focus == UINT32_MAX) {
            draw_image(1, FB_IMG_WIDTH, FB_IMG_HEIGHT, 0, full);
        } else {
            int err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
                .code = GPU_REQ_TRANSFER_TO_2D,
                .id = get_and_inc_req_id(),
                .transfer_to_2d = {
                    .resource_id = 1,
                    .rect = full,
                    .mem_offset = 0,
                },
            });
            assert(!err);
            err = gpu_enqueue_req(&gpu_queue_handle, (gpu_req_t) {
                .code = GPU_REQ_RESOURCE_FLUSH,
                .id = get_and_inc_req_id(),
                .resource_flush = {
                    .resource_id = 1,
                    .rect = full,
                },
            });
            assert(!err);
        }
        rendered_focus = focus;
        rendered_view_revision = view_revision;
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
        }
        return;
    }

    if (ch == TIMER_CH) {
        LOG_GPU_CLIENT("Received notification from timer\n");
        do_draw_state();
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
        }
        assert(resp.status == GPU_RESP_OK);
        LOG_GPU_CLIENT("Response received for request %d\n", resp.id);
        if (resp.id == display_info_req_id) {
            handle_display_info_response();
            pending_display_info_request = false;
        }
    }
}
