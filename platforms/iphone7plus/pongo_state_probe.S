/*
 * Stackless raw-entry state probe for iPhone 7 Plus / PongoOS.
 *
 * Draws a white geometry-coded report in the boot_args framebuffer:
 *   - one of four tracks identifies CurrentEL (EL0..EL3, top to bottom)
 *   - left/right marker identifies SCTLR_EL1.M (on/off)
 *   - four squares report DAIF.D/A/I/F mask bits (left to right)
 *
 * White/black geometry avoids relying on the uncalibrated color channel order.
 */
.section .text, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    cbz     x0, .Lhalt
    ldr     x1, [x0, #0x28]       /* framebuffer base */
    ldr     x2, [x0, #0x38]       /* row stride */
    ldr     x3, [x0, #0x40]       /* width */
    ldr     x4, [x0, #0x48]       /* height */
    cbz     x1, .Lhalt
    tst     x1, #3
    b.ne    .Lhalt
    cmp     x3, #128
    b.lo    .Lhalt
    cmp     x3, #4096
    b.hi    .Lhalt
    cmp     x4, #96
    b.lo    .Lhalt
    cmp     x4, #4096
    b.hi    .Lhalt
    cmp     x2, #512
    b.lo    .Lhalt
    cmp     x2, #32768
    b.hi    .Lhalt

    /* Clear a bounded 128 x 96 patch to black. */
    mov     x5, #0
.Lclear_row:
    madd    x6, x5, x2, x1
    mov     x7, #0
.Lclear_pixel:
    str     wzr, [x6, x7, lsl #2]
    add     x7, x7, #1
    cmp     x7, #128
    b.lo    .Lclear_pixel
    add     x5, x5, #1
    cmp     x5, #96
    b.lo    .Lclear_row

    /* CurrentEL: highlight one 96 x 4 track, EL0 at the top. */
    mrs     x8, CurrentEL
    lsr     x8, x8, #2
    and     x8, x8, #3
    mov     x5, #0
.Lel_track:
    cmp     x5, x8
    b.ne    .Lel_next
    mov     x9, #12
    madd    x10, x5, x9, xzr
    add     x10, x10, #4
    mov     x11, #0
.Lel_rows:
    add     x12, x10, x11
    madd    x6, x12, x2, x1
    mov     x7, #0
.Lel_pixels:
    mov     w13, #0xffff
    movk    w13, #0xffff, lsl #16
    str     w13, [x6, x7, lsl #2]
    add     x7, x7, #1
    cmp     x7, #96
    b.lo    .Lel_pixels
    add     x11, x11, #1
    cmp     x11, #4
    b.lo    .Lel_rows
.Lel_next:
    add     x5, x5, #1
    cmp     x5, #4
    b.lo    .Lel_track

    /* MMU: a 32 x 6 marker, left means SCTLR_EL1.M=1, right means 0. */
    mrs     x8, SCTLR_EL1
    and     x8, x8, #1
    mov     x9, #0
    cbnz    x8, .Lmmu_x_ready
    mov     x9, #64
.Lmmu_x_ready:
    mov     x10, #44
    mov     x11, #0
.Lmmu_rows:
    add     x12, x10, x11
    madd    x6, x12, x2, x1
    mov     x7, #0
.Lmmu_pixels:
    mov     w13, #0xffff
    movk    w13, #0xffff, lsl #16
    add     x14, x9, x7
    str     w13, [x6, x14, lsl #2]
    add     x7, x7, #1
    cmp     x7, #32
    b.lo    .Lmmu_pixels
    add     x11, x11, #1
    cmp     x11, #6
    b.lo    .Lmmu_rows

    /* DAIF.D/A/I/F: draw a 12 x 12 square when the corresponding bit is set. */
    mrs     x8, DAIF
    mov     x5, #0
.Ldaif_square:
    mov     x9, #9
    sub     x10, x9, x5
    lsr     x10, x8, x10
    and     x10, x10, #1
    cbz     x10, .Ldaif_next
    mov     x11, #72
    mov     x12, #20
    madd    x12, x5, x12, x12
    mov     x14, #0
.Ldaif_rows:
    add     x15, x11, x14
    madd    x6, x15, x2, x1
    mov     x7, #0
.Ldaif_pixels:
    mov     w13, #0xffff
    movk    w13, #0xffff, lsl #16
    add     x16, x12, x7
    str     w13, [x6, x16, lsl #2]
    add     x7, x7, #1
    cmp     x7, #12
    b.lo    .Ldaif_pixels
    add     x14, x14, #1
    cmp     x14, #12
    b.lo    .Ldaif_rows
.Ldaif_next:
    add     x5, x5, #1
    cmp     x5, #4
    b.lo    .Ldaif_square

.Lhalt:
    wfe
    b       .Lhalt
.size _start, .-_start
