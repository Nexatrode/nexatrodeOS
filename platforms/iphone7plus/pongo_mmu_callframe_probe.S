/*
 * Relocated Pongo -> EL1 MMU probe that reproduces Microkit's stack frames,
 * cache maintenance, register setup, MMU-on marker, and stack-based return.
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
    adr     x9, .Lstack_end
    mov     sp, x9
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

    /* The Microkit path installs its own vector before changing MMU state. */
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

    /* Yellow/white at y=112 means the L1 table is ready, still MMU-off. */
    mov     x0, #112
    movz    w1, #0xff00
    movk    w1, #0xffff, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band

    /* Control test: a handled SVC proves our vector and marker work with the
     * MMU off before using the same vector to diagnose the transition. */
    svc     #0
    bl      .Lel1_mmu_enable

    /* Green/white at y=280 means stack restore and function return succeeded. */
    mov     x0, #280
    movz    w1, #0xff00
    movk    w1, #0xff00, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band
.Lhalt:
    wfe
    b       .Lhalt

/* Same set/way D-cache clean as Microkit's util64.S. */
.macro dcache op
    dsb     sy
    mrs     x0, clidr_el1
    and     x3, x0, #0x7000000
    lsr     x3, x3, #23
    cbz     x3, .Lcache_done_\op
    mov     x10, #0
.Lcache_level_\op:
    add     x2, x10, x10, lsr #1
    lsr     x1, x0, x2
    and     x1, x1, #7
    cmp     x1, #2
    b.lt    .Lcache_skip_\op
    msr     csselr_el1, x10
    isb
    mrs     x1, ccsidr_el1
    and     x2, x1, #7
    add     x2, x2, #4
    mov     x4, #0x3ff
    and     x4, x4, x1, lsr #3
    clz     w5, w4
    mov     x7, #0x7fff
    and     x7, x7, x1, lsr #13
.Lcache_set_\op:
    mov     x9, x4
.Lcache_way_\op:
    lsl     x6, x9, x5
    orr     x11, x10, x6
    lsl     x6, x7, x2
    orr     x11, x11, x6
    dc      \op, x11
    subs    x9, x9, #1
    b.ge    .Lcache_way_\op
    subs    x7, x7, #1
    b.ge    .Lcache_set_\op
.Lcache_skip_\op:
    add     x10, x10, #2
    cmp     x3, x10
    b.gt    .Lcache_level_\op
.Lcache_done_\op:
    mov     x10, #0
    msr     csselr_el1, x10
    dsb     sy
    isb
.endm

.Lflush_dcache:
    dcache cisw
    ret

.Linvalidate_icache:
    ic      iallu
    dsb     nsh
    isb
    ret

#ifdef EXACT_NESTED_DISABLE
/* Exact nested helper call shape from Microkit util64.S::el1_mmu_disable. */
.Lel1_mmu_disable:
    stp     x29, x30, [sp, #-16]!
    mov     x29, sp
    bl      .Lflush_dcache
    mrs     x8, SCTLR_EL1
    bic     x8, x8, #(1 << 0)
    bic     x8, x8, #(1 << 2)
    bic     x8, x8, #(1 << 12)
    msr     SCTLR_EL1, x8
    isb
    bl      .Linvalidate_icache
    ldp     x29, x30, [sp], #16
    ret
#endif

/* Mirrors Microkit's EL1 prologue, cache-off sequence, register setup, and
 * return path. The stack lives in the relocated Normal-mapped code block. */
.Lel1_mmu_enable:
    stp     x29, x30, [sp, #-16]!
    stp     x27, x28, [sp, #-16]!
    mov     x29, sp
#ifdef EXACT_NESTED_DISABLE
    bl      .Lel1_mmu_disable
#else
    bl      .Lflush_dcache
    mrs     x8, SCTLR_EL1
    bic     x8, x8, #(1 << 0)
    bic     x8, x8, #(1 << 2)
    bic     x8, x8, #(1 << 12)
    msr     SCTLR_EL1, x8
    isb
    bl      .Linvalidate_icache
#endif

    ldr     x8, =0x000000ff440c0400
    msr     MAIR_EL1, x8
    ldr     x8, =0x00000010b0903019
    mrs     x9, ID_AA64MMFR0_EL1
    bfi     x8, x9, #32, #3
    msr     TCR_EL1, x8
    adr     x8, .Ltable
    add     x8, x8, #0xfff
    and     x8, x8, #0xfffffffffffff000
    msr     TTBR0_EL1, x8
#ifdef MATCH_LOADER_TABLE_LAYOUT
    ldr     x8, =0x810005000      /* Microkit boot_lvl0_upper address */
#endif
    msr     TTBR1_EL1, x8
    isb
    tlbi    vmalle1is
    dsb     ish
    isb

#if defined(MATCH_LOADER_POST_ENABLE_MARKER) || defined(FORCE_VISIBLE_POSTSTORE)
    /* Mirror util64.S: preserve framebuffer inputs in caller-saved registers
     * before SCTLR.M, exactly as the loader does with its BSS globals. */
    mov     x0, x19
    mov     x1, x20
#endif
    movz    x8, #0x0801
    movk    x8, #0x30d0, lsl #16
    msr     SCTLR_EL1, x8
    isb

#ifdef MATCH_LOADER_POST_ENABLE_MARKER
    bl      .Lpongo_mmu_post_enable_marker
#elif defined(FORCE_VISIBLE_POSTSTORE)
    bl      .Lvisible_poststore_marker
#else
    /* Cyan/white marks the first translated instruction and framebuffer write. */
    mov     x0, #168
    movz    w1, #0xffff
    movk    w1, #0xff00, lsl #16
    mov     w2, #-1
    bl      .Ldraw_band
#endif

    ldp     x27, x28, [sp], #16
    ldp     x29, x30, [sp], #16
    ret

#ifdef MATCH_LOADER_POST_ENABLE_MARKER
/* Byte-for-byte instruction equivalent of the patched loader's first
 * post-SCTLR.M framebuffer marker, using its x0=base / x1=stride contract. */
.Lpongo_mmu_post_enable_marker:
    mov     x9, x0
    cbz     x9, 9f
    mov     x10, x1
    mov     x11, #560
    madd    x9, x11, x10, x9
    mov     x12, #48
1:
    mov     w15, #0x2030
    movk    w15, #0xff10, lsl #16
    mov     x13, #128
4:
    str     w15, [x9], #4
    subs    x13, x13, #1
    b.ne    4b
    add     x9, x9, x10
    sub     x9, x9, #512
    subs    x12, x12, #1
    b.ne    1b

    mov     x9, x0
    ldr     x12, =560
    add     x12, x12, #8
    madd    x9, x12, x10, x9
    add     x9, x9, #64
    movz    w15, #0xff00
    movk    w15, #0xffff, lsl #16
    mov     x12, #32
5:
    mov     x13, #16
6:
    str     w15, [x9], #4
    subs    x13, x13, #1
    b.ne    6b
    add     x9, x9, x10
    sub     x9, x9, #64
    subs    x12, x12, #1
    b.ne    5b
9:
    ret
#endif

#ifdef FORCE_VISIBLE_POSTSTORE
/* Deliberately exaggerated control marker: same preloaded x0/x1 inputs,
 * same post-MMU address path, but a solid bright band is unambiguous. */
.Lvisible_poststore_marker:
    mov     x9, x0
    cbz     x9, 9f
    mov     x10, x1
    mov     x11, #560
    madd    x9, x11, x10, x9
    mov     w15, #0x0000
    movk    w15, #0xffff, lsl #16
    mov     x12, #48
1:
    mov     x13, #128
2:
    str     w15, [x9], #4
    subs    x13, x13, #1
    b.ne    2b
    add     x9, x9, x10
    sub     x9, x9, #512
    subs    x12, x12, #1
    b.ne    1b
9:
    ret
#endif

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

#ifdef MATCH_LOADER_TABLE_LAYOUT
/* Match the loader's TTBR1 root and TTBR0 L1 root addresses. EPD1 disables
 * TTBR1 walks, but retain the actual root address for this differential. */
.org    0x5000
.Lupper_table:
    .space  4096, 0
.org    0x9000
#else
.balign 4096
#endif
.Ltable:
#ifdef MATCH_LOADER_TABLE_LAYOUT
    .space  4096, 0
#else
    .space  8192, 0
#endif
.balign 16
.Lstack:
    .space  16384, 0
.Lstack_end:
.ltorg
.balign 8
.Limage_end:
.size _start, .-_start
