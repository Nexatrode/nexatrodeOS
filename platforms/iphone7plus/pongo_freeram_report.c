/* Read/write/restore test inside Pongo's PAGE_FREE run, plus stack canary. */
typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long u64;

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

void freeram_report(const volatile void *arg_pointer)
{
    const volatile u8 *args = (const volatile u8 *)arg_pointer;
    volatile u64 stack_check[64];
    volatile u64 *test = (volatile u64 *)0x8074E0000ULL;
    u64 saved0 = test[0];
    u64 saved1 = test[1];
    const u64 pattern0 = 0x6E65786174726F64ULL;
    const u64 pattern1 = 0x6552414D4F4B21A5ULL;
    u32 stack_ok = 1;
    u32 ram_ok;

    for (u32 i = 0; i < 64; ++i)
        stack_check[i] = 0x535441434b4f4b00ULL ^ (u64)i;
    for (u32 i = 0; i < 64; ++i)
        if (stack_check[i] != (0x535441434b4f4b00ULL ^ (u64)i))
            stack_ok = 0;

    test[0] = pattern0;
    test[1] = pattern1;
    __asm__ volatile("dsb sy" ::: "memory");
    ram_ok = test[0] == pattern0 && test[1] == pattern1;
    test[0] = saved0;
    test[1] = saved1;
    __asm__ volatile("dsb sy" ::: "memory");
    if (test[0] != saved0 || test[1] != saved1)
        ram_ok = 0;

    u64 fb = read64(args, 0x28);
    u64 stride = read64(args, 0x38);
    u64 width = read64(args, 0x40);
    u64 height = read64(args, 0x48);
    if (!fb || (fb & 3) || stride < 512 || (stride & 3) ||
        stride > 32768 || width < 300 || height < 320 ||
        width > 4096 || height > 4096)
        for (;;) __asm__ volatile("wfe");

    struct screen s = {(volatile u32 *)fb, stride / 4, width, height};
    for (u64 y = 0; y < 240; ++y)
        for (u64 x = 0; x < 620; ++x)
            pixel(&s, x, y, 0);

    /* Row 0: STACKOK ASCII-as-hex; row 1: RAM pass bit; rows 2-4: addresses/status. */
    draw_row(&s,   8, 0, 0x535441434b4f4bULL);
    draw_row(&s,  56, 1, ram_ok ? 1 : 0);
    draw_row(&s, 104, 2, (u64)test);
    draw_row(&s, 152, 3, stack_ok ? 1 : 0);
    draw_row(&s, 200, 4, 0x8BAD80000ULL);
    __asm__ volatile("dsb sy" ::: "memory");

    for (;;) __asm__ volatile("wfe");
}
