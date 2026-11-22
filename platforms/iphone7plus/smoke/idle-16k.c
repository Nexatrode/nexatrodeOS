/* SPDX-License-Identifier: BSD-2-Clause */
#include <microkit.h>
#include <stdint.h>

uintptr_t smoke_framebuffer;
volatile unsigned int started;
#if defined(IPHONE_TIMER_SPINNER)
volatile uint64_t spin_count;
#endif

static const char *glyph(char c)
{
    switch (c) {
    case '1': return "00100011000010000100001000010001110";
    case '6': return "01110100001000011110100011000101110";
    case 'I': return "11111001000010000100001000010011111";
    case 'M': return "10001110111010110101100011000110001";
    case 'K': return "10001100101010011000101001001010001";
    case 'B': return "11110100011000111110100011000111110";
    case 'O': return "01110100011000110001100011000101110";
    case 'T': return "11111001000010000100001000010000100";
    case 'U': return "10001100011000110001100011000101110";
    case 'S': return "01111100001000001110000010000111110";
    case 'E': return "11111100001000011110100001000011111";
    case 'R': return "11110100011000111110101001001010001";
    default: return "00000000000000000000000000000000000";
    }
}

static void text(unsigned int y, const char *s)
{
    volatile uint32_t *fb = (volatile uint32_t *)smoke_framebuffer;
    for (unsigned int i = 0; s[i]; i++) {
        const char *g = glyph(s[i]);
        for (unsigned int r = 0; r < 7; r++) {
            for (unsigned int c = 0; c < 5; c++) {
                if (g[r * 5 + c] != '1') continue;
                for (unsigned int dy = 0; dy < 8; dy++) {
                    for (unsigned int dx = 0; dx < 8; dx++) {
                        fb[(y + r * 8 + dy) * (0x1100 / 4) + 24 + i * 48 + c * 8 + dx] = 0xffffffffu;
                    }
                }
            }
        }
    }
}

/* Distinct instructions let the emulator distinguish this address space
 * from the monitor, which uses overlapping virtual addresses. */
__attribute__((noinline)) void smoke_ready(void)
{
    asm volatile("nop\n nop\n nop\n nop" ::: "memory");
}

void init(void)
{
#if !defined(IPHONE_TIMER_OBSERVER) || !defined(IPHONE_OBSERVER_LEGACY)
    volatile uint32_t *fb = (volatile uint32_t *)smoke_framebuffer;
#endif
#if defined(IPHONE_TIMER_OBSERVER)
#if !defined(IPHONE_OBSERVER_LEGACY)
    /* The first task clears only rows 0..191. Give the entire third line a
     * dark background so white glyphs below row 191 remain visible. Row 239
     * is the last row that fits in the shared 1 MiB framebuffer mapping. */
    for (unsigned int y = 184; y < 240; y++) {
        for (unsigned int x = 0; x < 640; x++) fb[y * (0x1100 / 4) + x] = 0;
    }
#endif
#if defined(IPHONE_TIMER_CONTROL) || defined(IPHONE_TIMER_SYSCALL)
    text(184, "TEST OK");
#else
    text(184, "TIMER OK");
#endif
    asm volatile("dsb sy" ::: "memory");
    smoke_ready();
    started = 1;
#else
    for (unsigned int y = 0; y < 192; y++) {
        for (unsigned int x = 0; x < 640; x++) fb[y * (0x1100 / 4) + x] = 0;
    }
    text(24, "16K BOOT");
    text(104, "USER OK");
    asm volatile("dsb sy" ::: "memory");
    started = 1;
    smoke_ready();
#if defined(IPHONE_TIMER_SPINNER)
#if defined(IPHONE_TIMER_CONTROL)
    /* Control: block on this PD's input cap to prove the observer can run. */
    seL4_Word badge;
    seL4_Recv(1, &badge, 4);
#endif
#if defined(IPHONE_TIMER_SYSCALL)
    /* Enter the kernel periodically so budget accounting can run without a
     * timer FIQ. The lower-priority observer still needs budget exhaustion. */
    for (;;) {
        for (unsigned int i = 0; i < 65536; i++) spin_count++;
        seL4_Yield();
    }
#endif
#if !defined(IPHONE_TIMER_CONTROL) && !defined(IPHONE_TIMER_SYSCALL)
    /* This task never yields. Only a timer-driven budget expiry lets the
     * lower-priority observer write the third line. */
    for (;;) spin_count++;
#endif
#endif
#endif
}

void notified(microkit_channel channel) { (void)channel; }
