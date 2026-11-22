/* Halt before the MMU enable write and show the actual hardware state.
 * Linked into verified, unreachable alignment padding in a copy of the loader.
 * No stack, cache operations, MMU changes, or subroutine calls are used.
 * Geometry/addresses match this phone's previously verified boot_args report.
 */
.section .text, "ax"
.global register_report, report_values, report_wait
register_report:
#ifdef MMU_16K_TEST
#include "loader_mmu_16k_transition.inc"
#else
    adr     x21, report_values
    str     x8, [x21, #80]          /* A: value about to be written to SCTLR */
    ldr     x0, =0x004d4d5550524531 /* 0: MMUPRE1 */
    str     x0, [x21]
    mrs     x0, CurrentEL
    str     x0, [x21, #8]
    mrs     x0, SCTLR_EL1
    str     x0, [x21, #16]
    mrs     x0, TCR_EL1
    str     x0, [x21, #24]
    mrs     x1, TTBR0_EL1
    str     x1, [x21, #32]
    mrs     x0, TTBR1_EL1
    str     x0, [x21, #40]
    mrs     x0, MAIR_EL1
    str     x0, [x21, #48]
    mrs     x0, VBAR_EL1
    str     x0, [x21, #56]
    mrs     x0, ID_AA64MMFR0_EL1
    str     x0, [x21, #64]
    mov     x0, sp
    str     x0, [x21, #72]
    /* Do not follow an unexpected TTBR into arbitrary memory. B/C use the
     * actual table only when its address matches the verified loader table.
     * D/E always read the expected table directly with SCTLR.C already off. */
    ldr     x2, =EXPECTED_L1
    and     x1, x1, #0x0000fffffffff000
    mov     x0, #-1
    str     x0, [x21, #88]
    str     x0, [x21, #96]
    cmp     x1, x2
    b.ne    1f
    ldr     x0, [x1, #256]
    str     x0, [x21, #88]
    ldr     x0, [x1, #272]
    str     x0, [x21, #96]
1:
    ldr     x0, [x2, #256]
    str     x0, [x21, #104]
    ldr     x0, [x2, #272]
    str     x0, [x21, #112]
    mrs     x0, ID_AA64MMFR1_EL1
    str     x0, [x21, #120]
    dsb     sy
#endif

.Lrender:
    ldr     x19, =0x8BB80C000
    mov     x20, #0x1100
    /* Clear 560 x 784 pixels; all 16 rows then fit with an 8-pixel margin. */
    mov     x0, #0
2:
    madd    x1, x0, x20, x19
    mov     x2, #560
3:
    str     wzr, [x1], #4
    subs    x2, x2, #1
    b.ne    3b
    add     x0, x0, #1
    cmp     x0, #784
    b.lo    2b

    adr     x22, hex_font
    mov     x0, #0                 /* report row 0..15 */
.Lrow:
    ldr     x1, [x21, x0, lsl #3]   /* row value, shifted one nibble at a time */
    mov     x2, #0                 /* glyph 0=row ID; glyphs 1..16=value */
.Lglyph:
    cbnz    x2, .Lvalue
    mov     x3, x0
    mov     x4, #8                 /* row ID x coordinate */
    b       .Ldraw
.Lvalue:
    lsr     x3, x1, #60
    lsl     x1, x1, #4
    lsl     x4, x2, #5
    add     x4, x4, #16            /* first value glyph starts x=48 */
.Ldraw:
    mov     x5, #5
    madd    x5, x3, x5, x22        /* five font bytes per glyph */
    mov     x6, #0                 /* glyph row */
.Lfontrow:
    ldrb    w7, [x5, x6]
    mov     x8, #48
    mul     x8, x0, x8
    add     x8, x8, #8
    add     x8, x8, x6, lsl #3     /* screen y */
    madd    x9, x8, x20, x19
    add     x9, x9, x4, lsl #2
    mov     x10, #8                /* vertical scale */
.Lscale_y:
    mov     x11, #0                /* glyph column */
    mov     x12, x9
.Lcolumn:
    mov     x13, #2
    sub     x13, x13, x11
    lsr     x14, x7, x13
    and     w14, w14, #1
    neg     w14, w14               /* zero black or all-ones white */
    mov     x15, #8
.Lscale_x:
    str     w14, [x12], #4
    subs    x15, x15, #1
    b.ne    .Lscale_x
    add     x11, x11, #1
    cmp     x11, #3
    b.lo    .Lcolumn
    add     x9, x9, x20
    subs    x10, x10, #1
    b.ne    .Lscale_y
    add     x6, x6, #1
    cmp     x6, #5
    b.lo    .Lfontrow
    add     x2, x2, #1
    cmp     x2, #17
    b.lo    .Lglyph
    add     x0, x0, #1
    cmp     x0, #16
    b.lo    .Lrow
    dsb     sy
report_wait:
    wfe
    b       report_wait
.balign 8
report_values:
    .space 128
hex_font:
    .byte 7,5,5,5,7, 2,6,2,2,7, 7,1,7,4,7, 7,1,7,1,7
    .byte 5,5,7,1,1, 7,4,7,1,7, 7,4,7,5,7, 7,1,1,1,1
    .byte 7,5,7,5,7, 7,5,7,1,7, 7,5,7,5,5, 6,5,6,5,6
    .byte 7,4,4,4,7, 6,5,5,5,6, 7,4,6,4,7, 7,4,6,4,4
    .ltorg
