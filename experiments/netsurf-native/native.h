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
#define WIDTH 1280
#define HEIGHT 800
#define BAR 60
#define PIXEL_WIDTH (WIDTH * NATIVE_SCALE)
#define PIXEL_HEIGHT (HEIGHT * NATIVE_SCALE)
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
