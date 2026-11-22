/*
 * Relocated Pongo -> EL1 MMU transition probe for iPhone9,2.
 * It copies itself from Pongo's receive buffer to Microkit's loader address,
 * then uses distinct L1 blocks for Normal code/RAM and the Device framebuffer.
 */
.section .text.entry, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    /* Pongo's receive buffer and display currently share one 1 GiB block.
     * Move to the Microkit loader address so code and display mappings are
     * separate, as they are in the actual loader. */
    ldr     x9, =0x4e5854524f444532
    cmp     x24, x9
    b.eq    .Lrelocated
    mov     x23, x0                 /* retain Pongo boot_args across the move */
    mov     x24, x9
    adr     x0, _start              /* source base: current Pongo buffer */
    adr     x1, .Limage_end
    sub     x1, x1, x0              /* exact image length, 8-byte aligned */
    ldr     x3, =0x810000000        /* same target as Microkit's loader */
    mov     x4, x3
.Lcopy:
    ldr     x5, [x0], #8
    str     x5, [x4], #8
    subs    x1, x1, #8
    b.ne    .Lcopy

    /* Publish copied instructions even if Pongo entered with caches on. */
    ldr     x6, =0x810000000
    ldr     x7, =.Limage_end - _start
    add     x7, x6, x7
    mrs     x10, ctr_el0
    ubfx    x10, x10, #16, #4       /* DminLine, in 4-byte words */
    mov     x11, #4
    lsl     x11, x11, x10          /* cache-line size in bytes */
    sub     x12, x11, #1
    bic     x6, x6, x12
.Lclean_copy:
    dc      cvac, x6
    add     x6, x6, x11
    cmp     x6, x7
    b.lo    .Lclean_copy
    dsb     sy
    ic      iallu
    dsb     sy
    isb
    ldr     x13, =0x810000000
    br      x13

.Lrelocated:
    mov     x0, x23
    cbz     x0, .Lhalt
    ldr     x19, [x0, #0x28]      /* boot_args framebuffer base */
    ldr     x20, [x0, #0x38]      /* framebuffer stride in bytes */
    ldr     x21, [x0, #0x40]      /* width */
    ldr     x22, [x0, #0x48]      /* height */
    cbz     x19, .Lhalt
    cmp     x21, #128
    b.lo    .Lhalt
    cmp     x22, #1024
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

    /* Green/yellow means relocated entry and boot_args are valid. */
    mov     x0, #0
    movz    w1, #0xff00
    movk    w1, #0xff00, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band

    /* Violet/white is a unique signature that the image was copied to the
     * Microkit loader block (L1 index 32), separate from the display block. */
    mov     x0, #56
    movz    w1, #0x00ff
    movk    w1, #0xffff, lsl #16
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
    ldr     x12, =0x711           /* Microkit Normal, AF, inner-shareable */
    orr     x10, x10, x12
    str     x10, [x8, x11, lsl #3]

    /* Device-nGnRnE outer-shareable identity block for the framebuffer. */
    and     x10, x19, #0xffffffffc0000000
    lsr     x11, x19, #30
    and     x11, x11, #0x1ff
    ldr     x12, =0x601           /* AF, outer-shareable, AttrIdx=0, block */
    orr     x10, x10, x12
    str     x10, [x8, x11, lsl #3]

    /* Match Microkit's MAIR layout, including Normal WB at AttrIdx4. */
    ldr     x8, =0x000000ff440c0400
    msr     MAIR_EL1, x8

    /* Match Microkit's TCR fields exactly: 39-bit TTBR0 range, EPD1, NC
     * walks, 4 KiB granules, 16-bit ASIDs, and inner-shareable walks. */
    ldr     x8, =0x00000010b0903019
    mrs     x9, ID_AA64MMFR0_EL1
    bfi     x8, x9, #32, #3
    msr     TCR_EL1, x8
    adr     x8, .Ltable
    add     x8, x8, #0xfff
    and     x8, x8, #0xfffffffffffff000
    msr     TTBR0_EL1, x8
    msr     TTBR1_EL1, x8          /* ignored because TCR_EL1.EPD1 is set */
    dsb     sy
    tlbi    vmalle1
    dsb     sy
    isb

    /* Yellow/white at y=112 means tables/registers are ready, still MMU-off. */
    mov     x0, #112
    movz    w1, #0xff00
    movk    w1, #0xffff, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band

    /* Control test: a handled SVC proves our vector and marker work with the
     * MMU off before using the same vector to diagnose the transition. */
    svc     #0

    /* Enable only translation. Cyan/white at y=168 is the success marker. */
    ldr     x8, =0x30d00801
    msr     SCTLR_EL1, x8
    isb
    mov     x0, #168
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
    /* Red/white at y=224 is the intentional pre-MMU SVC control marker. */
    mrs     x9, ESR_EL1
    ubfx    x10, x9, #26, #6
    cmp     x10, #0x15
    b.ne    .Lexception_halt
    mov     x0, #224
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
.ltorg
.balign 8
.Limage_end:
.size _start, .-_start
