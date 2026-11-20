/* Save/test/restore two words in every Pongo-reported free 16 KiB page. */
typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long u64;

#define FREE_START 0x807154000ULL
#define FREE_END   0x8BAD80000ULL
#define PAGE_SIZE  0x4000ULL

struct screen {
    volatile u32 *pixels;
    u64 stride_pixels;
    u64 width;
    u64 height;
};

static const u8 hex_font[16][5] = {
    {7,5,5,5,7}, {2,6,2,2,7}, {7,1,7,4,7}, {7,1,7,1,7},
    {5,5,7,1,1}, {7,4,7,1,7}, {7,4,7,5,7}, {7,1,1,1,1},
    {7,5,7,5,7}, {7,5,7,1,7}, {7,5,7,5,5}, {6,5,6,5,6},
    {7,4,4,4,7}, {6,5,5,5,6}, {7,4,6,4,7}, {7,4,6,4,4}
};

static u64 read64(const volatile u8 *base, u32 offset)
{
    return *(const volatile u64 *)(base + offset);
}

static void pixel(struct screen *s, u64 x, u64 y, u32 color)
{
    if (x < s->width && y < s->height)
        s->pixels[y * s->stride_pixels + x] = color;
}

static void block(struct screen *s, u64 x, u64 y, u32 scale, u32 color)
{
    for (u32 dy = 0; dy < scale; ++dy)
        for (u32 dx = 0; dx < scale; ++dx)
            pixel(s, x + dx, y + dy, color);
}

static void draw_nibble(struct screen *s, u64 x, u64 y, u32 value, u32 scale)
{
    for (u32 row = 0; row < 5; ++row) {
        u8 bits = hex_font[value & 15][row];
        for (u32 col = 0; col < 3; ++col)
            if (bits & (4u >> col))
                block(s, x + col * scale, y + row * scale, scale, 0xffffffffu);
    }
}

static void draw_row(struct screen *s, u64 y, u32 id, u64 value)
{
    const u32 scale = 8;
    draw_nibble(s, 8, y, id, scale);
    for (u32 i = 0; i < 16; ++i)
        draw_nibble(s, 48 + (u64)i * 32, y,
                    (u32)(value >> ((15u - i) * 4u)), scale);
}

static void render(const volatile u8 *args, u64 payload, u64 stack_top,
                   u64 pages, u64 fail_address, u32 ok, u32 stack_ok)
{
    u64 fb = read64(args, 0x28);
    u64 stride = read64(args, 0x38);
    u64 width = read64(args, 0x40);
    u64 height = read64(args, 0x48);
    if (!fb || (fb & 3) || stride < 512 || (stride & 3) ||
        stride > 32768 || width < 300 || height < 320 ||
        width > 4096 || height > 4096)
        for (;;) __asm__ volatile("wfe");

    struct screen s = {(volatile u32 *)fb, stride / 4, width, height};
    for (u64 y = 0; y < 400; ++y)
        for (u64 x = 0; x < 620; ++x)
            pixel(&s, x, y, 0);

    draw_row(&s,   8, 0, ok ? 0x53574545504f4bULL : 0x5357454550455252ULL);
    draw_row(&s,  56, 1, pages);
    draw_row(&s, 104, 2, FREE_START);
    draw_row(&s, 152, 3, FREE_END);
    draw_row(&s, 200, 4, fail_address);
    draw_row(&s, 248, 5, stack_ok ? 1 : 0);
    draw_row(&s, 296, 6, payload);
    draw_row(&s, 344, 7, stack_top);
    __asm__ volatile("dsb sy" ::: "memory");
    for (;;) __asm__ volatile("wfe");
}

void ramsweep_report(const volatile void *arg_pointer, u64 payload,
                     u64 stack_top)
{
    const volatile u8 *args = (const volatile u8 *)arg_pointer;
    volatile u64 stack_check[64];
    u32 stack_ok = 1;
    u64 pages = 0;
    u64 fail_address = 0;
    u32 ok = 1;

    for (u32 i = 0; i < 64; ++i)
        stack_check[i] = 0x535441434b4f4b00ULL ^ (u64)i;
    for (u32 i = 0; i < 64; ++i)
        if (stack_check[i] != (0x535441434b4f4b00ULL ^ (u64)i))
            stack_ok = 0;
    if (!stack_ok)
        ok = 0;

    for (u64 pa = FREE_START; pa < FREE_END; pa += PAGE_SIZE) {
        volatile u64 *first = (volatile u64 *)pa;
        volatile u64 *last = (volatile u64 *)(pa + PAGE_SIZE - sizeof(u64));
        u64 old_first = *first;
        u64 old_last = *last;
        u64 pattern = 0xa55aa55a00000000ULL ^ pa;
        u64 inverse = ~pattern;

        *first = pattern;
        *last = inverse;
        __asm__ volatile("dsb sy" ::: "memory");
        if (*first != pattern || *last != inverse) {
            ok = 0;
            fail_address = pa;
        }
        *first = old_first;
        *last = old_last;
        __asm__ volatile("dsb sy" ::: "memory");
        if (*first != old_first || *last != old_last) {
            ok = 0;
            fail_address = pa;
        }
        ++pages;
        if (!ok)
            break;
    }

    for (u32 i = 0; i < 64; ++i)
        if (stack_check[i] != (0x535441434b4f4b00ULL ^ (u64)i))
            stack_ok = 0;
    if (!stack_ok)
        ok = 0;

    render(args, payload, stack_top, pages, fail_address, ok, stack_ok);
}
