/*
 * Minimal Pongo -> EL1 MMU transition probe for iPhone9,2.
 * No Microkit/seL4 loader, C runtime, stack, interrupts, or persistent writes.
 * Uses a single level-1 1 GiB block table for the payload and framebuffer.
 */
.section .text.entry, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    cbz     x0, .Lhalt
    ldr     x19, [x0, #0x28]      /* boot_args framebuffer base */
    ldr     x20, [x0, #0x38]      /* framebuffer stride in bytes */
    ldr     x21, [x0, #0x40]      /* width */
    ldr     x22, [x0, #0x48]      /* height */
    cbz     x19, .Lhalt
    cmp     x21, #128
    b.lo    .Lhalt
    cmp     x22, #960
    b.lo    .Lhalt

    /* Remove old loader stripes in the area this probe will use. */
    mov     x9, #0
.Lclear_row:
    madd    x10, x9, x20, x19
    mov     x11, #128
.Lclear_pixel:
    str     wzr, [x10], #4
    subs    x11, x11, #1
    b.ne    .Lclear_pixel
    add     x9, x9, #1
    cmp     x9, #1024
    b.ne    .Lclear_row

    /* Green/yellow means the raw payload and boot_args are valid. */
    mov     x0, #0
    movz    w1, #0xff00
    movk    w1, #0xff00, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band

    /* Turn off translation and caches before replacing EL1's table state. */
    ldr     x8, =0x30d00800
    msr     SCTLR_EL1, x8
    isb
    adr     x8, .Lvectors
    msr     VBAR_EL1, x8
    isb

    /* Build one 4 KiB L1 table in the uploaded image's writable RAM. */
    adr     x8, .Ltable
    add     x8, x8, #0xfff
    and     x8, x8, #0xfffffffffffff000
    mov     x9, #512
.Lzero_table:
    str     xzr, [x8], #8
    subs    x9, x9, #1
    b.ne    .Lzero_table
    adr     x8, .Ltable
    add     x8, x8, #0xfff
    and     x8, x8, #0xfffffffffffff000

    /* Normal RW identity block for this payload's 1 GiB region. */
    adr     x9, _start
    and     x10, x9, #0xffffffffc0000000
    lsr     x11, x9, #30
    and     x11, x11, #0x1ff
    ldr     x12, =0x705           /* AF, inner-shareable, AttrIdx=1, block */
    orr     x10, x10, x12
    str     x10, [x8, x11, lsl #3]

    /* Device-nGnRnE outer-shareable identity block for the framebuffer. */
    and     x10, x19, #0xffffffffc0000000
    lsr     x11, x19, #30
    and     x11, x11, #0x1ff
    ldr     x12, =0x601           /* AF, outer-shareable, AttrIdx=0, block */
    orr     x10, x10, x12
    str     x10, [x8, x11, lsl #3]

    /* MAIR AttrIdx0=Device-nGnRnE, AttrIdx1=Normal WB. */
    ldr     x8, =0x000000000000ff00
    msr     MAIR_EL1, x8

    /* T0SZ=25 gives a 39-bit low VA range, rooted directly at L1.
     * EPD1 disables unused high-address walks; SH0 is inner-shareable.
     * IRGN0/ORGN0 stay non-cacheable for this minimal test. */
    ldr     x8, =0x0000000000903019
    mrs     x9, ID_AA64MMFR0_EL1
    bfi     x8, x9, #32, #3
    msr     TCR_EL1, x8
    adr     x8, .Ltable
    add     x8, x8, #0xfff
    and     x8, x8, #0xfffffffffffff000
    msr     TTBR0_EL1, x8
    dsb     sy
    tlbi    vmalle1
    dsb     sy
    isb

    /* Yellow/white at y=56 means tables/registers are ready, still MMU-off. */
    mov     x0, #56
    movz    w1, #0xff00
    movk    w1, #0xffff, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band

    /* Control test: a handled SVC proves our vector and marker work with the
     * MMU off before using the same vector to diagnose the transition. */
    svc     #0

    /* Enable only translation. Cyan/white at y=112 is the success marker. */
    ldr     x8, =0x30d00801
    msr     SCTLR_EL1, x8
    isb
    mov     x0, #112
    movz    w1, #0xffff
    movk    w1, #0xff00, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band
    dsb     sy
.Lhalt:
    wfe
    b       .Lhalt

/* x0=y offset, w1/w2=top/bottom colors. Stackless, keeps x19/x20 intact. */
.Ldraw_band:
    madd    x9, x0, x20, x19
    mov     x10, #48
.Lrow:
    cmp     x10, #24
    b.gt    .Lbottom
    mov     w11, w1
    b       .Lcolor_ready
.Lbottom:
    mov     w11, w2
.Lcolor_ready:
    mov     x12, #128
.Lpixel:
    str     w11, [x9], #4
    subs    x12, x12, #1
    b.ne    .Lpixel
    add     x9, x9, x20
    sub     x9, x9, #512
    subs    x10, x10, #1
    b.ne    .Lrow
    ret

.balign 2048
.Lvectors:
.rept 16
    b       .Lexception
    .space  124
.endr
.Lexception:
    /* Red/white at y=168 is the intentional pre-MMU SVC control marker. */
    mrs     x9, ESR_EL1
    ubfx    x10, x9, #26, #6
    cmp     x10, #0x15
    b.ne    .Lexception_halt
    mov     x0, #168
    mov     w1, #0xff
    movk    w1, #0xff00, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band
    mrs     x9, ELR_EL1
    /* SVC already sets ELR to the following instruction. */
    msr     ELR_EL1, x9
    eret
.Lexception_halt:
    wfe
    b       .Lexception_halt

.balign 4096
.Ltable:
    .space  8192, 0
.size _start, .-_start
