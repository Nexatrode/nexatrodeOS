/*
 * Temporary PongoOS raw-entry probe for iPhone 7 Plus bring-up.
 *
 * bootr enters the uploaded raw image with Apple boot_args in x0. Keep this
 * code position-independent, stackless, and self-contained: Pongo's raw handoff
 * does not provide the QEMU/Microkit load address or a C runtime.
 */
.section .text, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    /* Apple boot_args Video fields (PongoOS boot_args, packed):
     *   Video.v_baseAddr = 0x28
     *   Video.v_rowBytes = 0x38
     *   Video.v_width    = 0x40
     *   Video.v_height   = 0x48
     */
    cbz     x0, .Lhalt
    ldr     x1, [x0, #0x28]       /* framebuffer base */
    ldr     x2, [x0, #0x38]       /* bytes per row */
    ldr     x3, [x0, #0x40]       /* width */
    ldr     x4, [x0, #0x48]       /* height */

    /* Refuse to touch memory unless the framebuffer description is sane. */
    cbz     x1, .Lhalt
    tst     x1, #3
    b.ne    .Lhalt
    cmp     x3, #128
    b.lo    .Lhalt
    cmp     x3, #4096
    b.hi    .Lhalt
    cmp     x4, #48
    b.lo    .Lhalt
    cmp     x4, #4096
    b.hi    .Lhalt
    cmp     x2, #512             /* at least 128 32-bit pixels */
    b.lo    .Lhalt
    cmp     x2, #32768           /* bounded row stride */
    b.hi    .Lhalt

    /* Paint two 128x24 rows at the top-left: green, then amber. */
    mov     x5, #0
.Lrow:
    madd    x6, x5, x2, x1
    mov     x7, #0
    cmp     x5, #24
    b.lo    .Lgreen
    movz    w8, #0xff00
    movk    w8, #0xffff, lsl #16
    b       .Lpixel
.Lgreen:
    movz    w8, #0xff00
    movk    w8, #0xff00, lsl #16
.Lpixel:
    str     w8, [x6, x7, lsl #2]
    add     x7, x7, #1
    cmp     x7, #128
    b.lo    .Lpixel
    add     x5, x5, #1
    cmp     x5, #48
    b.lo    .Lrow

.Lhalt:
    wfe
    b       .Lhalt
.size _start, .-_start
