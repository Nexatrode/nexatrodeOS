/* SPDX-License-Identifier: GPL-2.0-only */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "utils/errors.h"
#include "netsurf/plotters.h"
#include "netsurf/layout.h"
#include "netsurf/bitmap.h"
#ifndef NATIVE_SCALE
#define NATIVE_SCALE 2
#endif
#ifdef NATIVE_DESKTOP
#define WIDTH 560
#define MAX_HEIGHT 520
#ifdef NATIVE_DISPLAY_RESIZE
extern int native_view_height;
#define HEIGHT native_view_height
#else
#define HEIGHT 520
#endif
#define BAR 80
#define ADDRESS_LEFT 128
#define ADDRESS_RIGHT (WIDTH-152)
#define RELOAD_LEFT (WIDTH-142)
#define RELOAD_RIGHT (WIDTH-82)
#define STOP_LEFT (WIDTH-76)
#define STOP_RIGHT (WIDTH-12)
#define HOME_RIGHT 60
#define BACK_RIGHT 120
#else
#define WIDTH 1280
#define HEIGHT 800
#define MAX_HEIGHT 800
#define BAR 60
#define ADDRESS_LEFT 193
#define ADDRESS_RIGHT (WIDTH-500)
#define RELOAD_LEFT (WIDTH-490)
#define RELOAD_RIGHT (WIDTH-400)
#define STOP_LEFT (WIDTH-395)
#define STOP_RIGHT (WIDTH-320)
#define HOME_RIGHT 95
#define BACK_RIGHT 185
#endif
#define PIXEL_WIDTH (WIDTH * NATIVE_SCALE)
#define PIXEL_HEIGHT (HEIGHT * NATIVE_SCALE)
#define MAX_PIXEL_HEIGHT (MAX_HEIGHT * NATIVE_SCALE)
struct asset { const char *name; const unsigned char *data; size_t size; };
extern const struct asset native_assets[];
extern const size_t native_asset_count;
extern const unsigned char native_font[];
extern const size_t native_font_size;
extern struct gui_layout_table native_layout;
extern struct gui_bitmap_table native_bitmaps;
extern const struct plotter_table native_plotters;
const struct asset *native_asset(const char *path);
void native_raster_init(unsigned char *pixels);
void native_fill(int x, int y, int w, int h, uint32_t colour);
void native_text(int x, int y, int size, uint32_t colour, const char *text);
uint64_t native_ms(void);
void native_fail(const char *where, int error);
