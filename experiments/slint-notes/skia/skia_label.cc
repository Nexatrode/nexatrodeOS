#include "include/core/SkCanvas.h"
#include "include/core/SkData.h"
#include "include/core/SkFont.h"
#include "include/core/SkFontMetrics.h"
#include "include/core/SkGraphics.h"
#include "include/core/SkFontMgr.h"
#include "include/core/SkImageInfo.h"
#include "include/core/SkPaint.h"
#include "include/core/SkSurface.h"
#include "include/ports/SkFontMgr_empty.h"

static sk_sp<SkTypeface> g_face;

extern "C" int skia_prepare_font(const unsigned char *font_bytes, int font_len) {
    if (g_face) {
        return 1;
    }
    if (!font_bytes || font_len <= 0) {
        return 0;
    }
    SkGraphics::Init();
    sk_sp<SkData> data = SkData::MakeWithoutCopy(font_bytes, static_cast<size_t>(font_len));
    sk_sp<SkFontMgr> manager = SkFontMgr_New_Custom_Empty();
    if (!data || !manager) {
        return 0;
    }
    g_face = manager->makeFromData(std::move(data));
    if (!g_face || g_face->countGlyphs() <= 0) {
        g_face.reset();
        return 0;
    }
    return 1;
}

extern "C" int skia_draw_text(
    unsigned char *rgba,
    int width,
    int height,
    int stride,
    const unsigned char *text,
    int text_len,
    float size,
    unsigned int color,
    float baseline,
    int embolden) {
    if (!g_face || !rgba || !text || text_len <= 0 || width <= 0 || height <= 0 || stride < width * 4 || size <= 0.0f) {
        return 0;
    }
    SkImageInfo info = SkImageInfo::Make(width, height, kRGBA_8888_SkColorType, kPremul_SkAlphaType);
    sk_sp<SkSurface> surface = SkSurfaces::WrapPixels(info, rgba, static_cast<size_t>(stride));
    if (!surface) {
        return 0;
    }
    SkFont font(g_face, size);
    font.setEdging(SkFont::Edging::kAntiAlias);
    font.setEmbolden(embolden != 0);
    if (baseline < 0.0f) {
        SkFontMetrics metrics;
        font.getMetrics(&metrics);
        baseline = -metrics.fAscent;
    }
    SkPaint paint;
    paint.setAntiAlias(true);
    paint.setColor(color);
    SkCanvas *canvas = surface->getCanvas();
    canvas->clear(SK_ColorTRANSPARENT);
    canvas->drawSimpleText(text, static_cast<size_t>(text_len), SkTextEncoding::kUTF8, 0.0f, baseline, font, paint);
    for (int i = 3; i < width * height * 4; i += 4) {
        if (rgba[i] != 0) {
            return 1;
        }
    }
    return 4;
}
