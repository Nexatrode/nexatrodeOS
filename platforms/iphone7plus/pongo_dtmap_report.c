/*
 * Read Pongo's Apple-format device tree and report chosen/memory-map
 * MemoryMapReserved-* entries. This is not an FDT parser: pongoOS converts
 * Apple's compact recursive node format into dt_node_t/dt_prop_t records.
 */
typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long u64;

#define MAX_DEPTH 24
#define MAX_NODES 4096
#define MAX_REPORTED_RANGES 8
#define APPLE_NAME_PREFIX "MemoryMapReserved-"

struct screen {
    volatile u32 *pixels;
    u64 stride_pixels;
    u64 width;
    u64 height;
};

struct range {
    u64 name_code;
    u64 address;
    u64 size;
};

struct parser {
    const volatile u8 *base;
    const volatile u8 *cursor;
    const volatile u8 *end;
    u32 nodes;
    u32 errors;
    u32 chosen_seen;
    u32 memory_map_seen;
    u32 range_count;
    struct range ranges[MAX_REPORTED_RANGES];
};

static const u8 hex_font[16][5] = {
    {7,5,5,5,7}, {2,6,2,2,7}, {7,1,7,4,7}, {7,1,7,1,7},
    {5,5,7,1,1}, {7,4,7,1,7}, {7,4,7,5,7}, {7,1,1,1,1},
    {7,5,7,5,7}, {7,5,7,1,7}, {7,5,7,5,5}, {6,5,6,5,6},
    {7,4,4,4,7}, {6,5,5,5,6}, {7,4,6,4,7}, {7,4,6,4,4}
};

static u32 load_le32(const volatile u8 *p)
{
    return (u32)p[0] | ((u32)p[1] << 8) | ((u32)p[2] << 16) |
           ((u32)p[3] << 24);
}

static u64 load_le64(const volatile u8 *p)
{
    return (u64)load_le32(p) | ((u64)load_le32(p + 4) << 32);
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
        for (u32 col = 0; col < 3; ++col)
            if (bits & (4u >> col))
                block(s, x + col * scale, y + row * scale, scale,
                      0xffffffffu);
    }
}

static void draw_row(struct screen *s, u64 row, u32 id, u64 value)
{
    const u32 scale = 8;
    const u64 y = 8 + row * 48;
    draw_nibble(s, 8, y, id >> 4, scale);
    draw_nibble(s, 40, y, id, scale);
    for (u32 i = 0; i < 16; ++i) {
        u32 shift = (15u - i) * 4u;
        draw_nibble(s, 88 + (u64)i * 32, y, (u32)(value >> shift), scale);
    }
}

static int key_is(const volatile u8 *key, const char *name)
{
    u32 i = 0;
    while (name[i]) {
        if (key[i] != (u8)name[i]) return 0;
        ++i;
    }
    return key[i] == 0;
}

static u64 pack_name_code(const volatile u8 *name, u32 length)
{
    u64 value = 0;
    for (u32 i = 0; i < 8; ++i) {
        u8 c = i < length ? name[i] : 0;
        value = (value << 8) | c;
    }
    return value;
}

static int parse_node(struct parser *p, u32 depth, int parent_is_chosen)
{
    if (depth >= MAX_DEPTH || p->nodes >= MAX_NODES ||
        p->end - p->cursor < 8) {
        ++p->errors;
        return 0;
    }

    const volatile u8 *node = p->cursor;
    u32 nprop = load_le32(node);
    u32 nchld = load_le32(node + 4);
    const volatile u8 *prop_start = node + 8;
    const volatile u8 *scan = prop_start;
    const volatile u8 *name = (const volatile u8 *)"";
    u32 name_len = 0;

    if (nprop > 1024 || nchld > MAX_NODES) {
        ++p->errors;
        return 0;
    }

    for (u32 i = 0; i < nprop; ++i) {
        if (p->end - scan < 36) {
            ++p->errors;
            return 0;
        }
        const volatile u8 *key = scan;
        u32 len = load_le32(scan + 32) & 0x00ffffffu;
        const volatile u8 *value = scan + 36;
        u64 padded = ((u64)len + 3) & ~3ULL;
        if ((u64)(p->end - value) < padded) {
            ++p->errors;
            return 0;
        }
        if (key_is(key, "name")) {
            name = value;
            name_len = len;
        }
        scan = value + padded;
    }
    const volatile u8 *prop_end = scan;
    int is_chosen = parent_is_chosen ||
                    (depth == 1 && name_len == 7 &&
                     name[0] == 'c' && name[1] == 'h' && name[2] == 'o' &&
                     name[3] == 's' && name[4] == 'e' && name[5] == 'n' &&
                     name[6] == 0);
    int is_memory_map = parent_is_chosen && name_len == 11 &&
                        name[0] == 'm' && name[1] == 'e' && name[2] == 'm' &&
                        name[3] == 'o' && name[4] == 'r' && name[5] == 'y' &&
                        name[6] == '-' && name[7] == 'm' && name[8] == 'a' &&
                        name[9] == 'p' && name[10] == 0;
    if (is_chosen) p->chosen_seen = 1;
    if (is_memory_map) p->memory_map_seen = 1;

    if (is_memory_map) {
        scan = prop_start;
        for (u32 i = 0; i < nprop; ++i) {
            const volatile u8 *key = scan;
            u32 len = load_le32(scan + 32) & 0x00ffffffu;
            const volatile u8 *value = scan + 36;
            u64 padded = ((u64)len + 3) & ~3ULL;
            if (key_is(key, "name")) {
                scan = value + padded;
                continue;
            }
            static const char prefix[] = APPLE_NAME_PREFIX;
            u32 prefix_len = sizeof(prefix) - 1;
            int match = len == 16 && key[0] == 'M';
            for (u32 k = 0; match && k < prefix_len; ++k)
                if (key[k] != (u8)prefix[k]) match = 0;
            if (match) {
                ++p->range_count;
                if (p->range_count <= MAX_REPORTED_RANGES) {
                    struct range *r = &p->ranges[p->range_count - 1];
                    r->name_code = pack_name_code(key + prefix_len,
                                                  32 - prefix_len);
                    r->address = load_le64(value);
                    r->size = load_le64(value + 8);
                }
            }
            scan = value + padded;
        }
    }

    p->cursor = prop_end;
    ++p->nodes;
    for (u32 i = 0; i < nchld; ++i)
        if (!parse_node(p, depth + 1, is_chosen))
            return 0;
    return 1;
}

void dtmap_report_main(const volatile void *arg_pointer, u64 payload_base,
                       u64 stack_top)
{
    (void)payload_base;
    (void)stack_top;
    const volatile u8 *args = (const volatile u8 *)arg_pointer;
    u64 fb = load_le64(args + 0x28);
    u64 stride = load_le64(args + 0x38);
    u64 width = load_le64(args + 0x40);
    u64 height = load_le64(args + 0x48);
    u64 virt_base = load_le64(args + 0x08);
    u64 phys_base = load_le64(args + 0x10);
    u64 tree_ptr = load_le64(args + 0x60);
    u32 tree_len = load_le32(args + 0x68);

    if (!fb || (fb & 3) || stride < 512 || (stride & 3) ||
        stride > 32768 || width < 300 || height < 320 ||
        width > 4096 || height > 4096 || tree_len < 8 || tree_len > 0x100000 ||
        tree_ptr < virt_base)
        for (;;) __asm__ volatile("wfe");

    struct screen s = {
        (volatile u32 *)fb,
        stride / 4,
        width,
        height
    };
    for (u64 y = 0; y < 640; ++y)
        for (u64 x = 0; x < 620; ++x)
            pixel(&s, x, y, 0);

    /* Leave a visible checkpoint before the first device-tree memory read. */
    draw_row(&s, 0, 0, 0x4454505245424500ULL); /* "DTPREBE" */
    draw_row(&s, 1, 1, tree_len);
    draw_row(&s, 2, 2, tree_ptr);
    __asm__ volatile("dsb sy" ::: "memory");

    /* deviceTreeP is an XNU VA; this maps it to the physical boot-time address. */
    u64 tree_phys = tree_ptr - virt_base + phys_base;
    draw_row(&s, 3, 3, tree_phys);
    __asm__ volatile("dsb sy" ::: "memory");
    struct parser p = {
        (const volatile u8 *)tree_phys,
        (const volatile u8 *)tree_phys,
        (const volatile u8 *)tree_phys + tree_len,
        0, 0, 0, 0, 0, {{0, 0, 0}}
    };
    int parsed = parse_node(&p, 0, 0);

    for (u64 y = 0; y < 1600; ++y)
        for (u64 x = 0; x < 640; ++x)
            pixel(&s, x, y, 0);

    /* The fixed header rows are followed by triples: name bytes, PA, length. */
    draw_row(&s, 0, 0, 0x4150504c45445400ULL); /* ASCII "APPLEDT" */
    draw_row(&s, 1, 1, tree_len);
    draw_row(&s, 2, 2, ((u64)p.errors << 32) | p.nodes);
    draw_row(&s, 3, 3, p.chosen_seen);
    draw_row(&s, 4, 4, p.memory_map_seen);
    draw_row(&s, 5, 5, p.range_count);
    u32 shown = p.range_count < MAX_REPORTED_RANGES ?
                p.range_count : MAX_REPORTED_RANGES;
    for (u32 i = 0; i < shown; ++i) {
        u32 first = 6 + i * 3;
        draw_row(&s, first,     first & 0xff, p.ranges[i].name_code);
        draw_row(&s, first + 1, (first + 1) & 0xff, p.ranges[i].address);
        draw_row(&s, first + 2, (first + 2) & 0xff, p.ranges[i].size);
    }
    __asm__ volatile("dsb sy" ::: "memory");
    for (;;) __asm__ volatile("wfe");
}
