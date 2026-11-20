/*
 * Freestanding C half of the Pongo raw-entry boot_args / stack probe.
 * All drawing is bounded to a small top-left patch and uses white-on-black
 * hexadecimal glyphs so the report does not depend on color-channel order.
 */
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
    const volatile u64 *p = (const volatile u64 *)(base + offset);
    return *p;
}

static u32 read32(const volatile u8 *base, u32 offset)
{
    const volatile u32 *p = (const volatile u32 *)(base + offset);
    return *p;
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

static void draw_nibble(struct screen *s, u64 x, u64 y, u32 value,
                        u32 scale)
{
    for (u32 row = 0; row < 5; ++row) {
        u8 bits = hex_font[value & 15][row];
        for (u32 col = 0; col < 3; ++col) {
            if (bits & (4u >> col))
                block(s, x + col * scale, y + row * scale, scale, 0xffffffffu);
        }
    }
}

static void draw_row(struct screen *s, u64 y, u32 id, u64 value)
{
    const u32 scale = 8;
    draw_nibble(s, 8, y, id, scale);
    for (u32 i = 0; i < 16; ++i) {
        u32 shift = (15u - i) * 4u;
        draw_nibble(s, 48 + (u64)i * 32, y, (u32)(value >> shift), scale);
    }
}

void report_main(const volatile void *arg_pointer, u64 payload_base,
                 u64 stack_top, u64 cntfrq, u64 mpidr)
{
    const volatile u8 *args = (const volatile u8 *)arg_pointer;
    u64 fb = read64(args, 0x28);
    u64 stride = read64(args, 0x38);
    u64 width = read64(args, 0x40);
    u64 height = read64(args, 0x48);

    if (!fb || (fb & 3) || stride < 512 || (stride & 3) ||
        stride > 32768 || width < 300 || height < 320 ||
        width > 4096 || height > 4096)
        for (;;) __asm__ volatile("wfe");

    /* Force real nested calls and volatile local stack traffic before output. */
    volatile u64 stack_check[64];
    for (u32 i = 0; i < 64; ++i)
        stack_check[i] = 0x535441434b4f4b00ULL ^ (u64)i;
    for (u32 i = 0; i < 64; ++i)
        if (stack_check[i] != (0x535441434b4f4b00ULL ^ (u64)i))
            for (;;) __asm__ volatile("wfe");

    struct screen s = {
        (volatile u32 *)fb,
        stride / 4,
        width,
        height
    };

    for (u64 y = 0; y < 720; ++y)
        for (u64 x = 0; x < 620; ++x)
            pixel(&s, x, y, 0);

    /* Row 0 is the ASCII marker "STACKOK"; remaining IDs map below. */
    draw_row(&s,   8,  0, 0x535441434b4f4bULL);
    draw_row(&s,  56,  1, read64(args, 0x08)); /* virtBase */
    draw_row(&s, 104,  2, read64(args, 0x10)); /* physBase */
    draw_row(&s, 152,  3, read64(args, 0x18)); /* memSize */
    draw_row(&s, 200,  4, read64(args, 0x20)); /* topOfKernelData */
    draw_row(&s, 248,  5, read64(args, 0x60)); /* deviceTreeP */
    draw_row(&s, 296,  6, read32(args, 0x68)); /* deviceTreeLength */
    draw_row(&s, 344,  7, fb);                 /* framebuffer base */
    draw_row(&s, 392,  8, stride);             /* row bytes */
    draw_row(&s, 440,  9, width);              /* pixel width */
    draw_row(&s, 488, 10, height);             /* pixel height */
    draw_row(&s, 536, 11, payload_base);       /* raw payload address */
    draw_row(&s, 584, 12, stack_top);          /* temporary stack top */
    draw_row(&s, 632, 13, cntfrq);             /* CNTFRQ_EL0 in Hz */
    draw_row(&s, 680, 14, mpidr);              /* MPIDR_EL1 */
    __asm__ volatile("dsb sy" ::: "memory");

    for (;;) __asm__ volatile("wfe");
}
