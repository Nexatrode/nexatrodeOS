/* Terminal 16 KiB translation test. Reuse 0x810004000..0x810008000
 * (old globals/upper tables) as one aligned L2 root. The existing stack ends
 * below it; code/vectors are below 0x810003000. Never return to C or kernel.
 */
    adr     x21, report_values
    mrs     x0, ID_AA64MMFR0_EL1
    str     x0, [x21, #64]
    ubfx    x1, x0, #20, #4
    cbz     x1, .Lunsupported
    cmp     x1, #2
    b.hi    .Lunsupported
    ldr     x8, =0x810004000
    mov     x9, #2048
1:
    str     xzr, [x8], #8
    subs    x9, x9, #1
    b.ne    1b
    ldr     x8, =0x810004000
    /* 16 KiB L2 entries cover 32 MiB. Map the loader block and BOTH blocks
     * touched by framebuffer 0x8BB80C000..0x8BC004000. */
    ldr     x9, =0x810000711
    str     x9, [x8, #(0x408*8)]
    ldr     x9, =0x8BA000601
    str     x9, [x8, #(0x45d*8)]
    ldr     x9, =0x8BC000601
    str     x9, [x8, #(0x45e*8)]
    dsb     sy
    msr     TTBR0_EL1, x8
    msr     TTBR1_EL1, x8
    /* 36-bit VA, L2 root; TG0=10 and TG1=01 select 16 KiB. EPD1=1,
     * non-cacheable, inner-shareable walks, 8-bit ASIDs. IPS from hardware. */
    ldr     x10, =0x709cb01c
    mrs     x9, ID_AA64MMFR0_EL1
    bfi     x10, x9, #32, #3
    msr     TCR_EL1, x10
    isb
    tlbi    vmalle1is
    dsb     sy
    isb
    movz    x8, #0x0801
    movk    x8, #0x30d0, lsl #16
    str     x8, [x21, #80]
    msr     SCTLR_EL1, x8
    isb
    mrs     x0, SCTLR_EL1
    str     x0, [x21, #16]
    tbz     x0, #0, .Lunsupported
    /* Ask the now-enabled translation regime to resolve both addresses. */
    ldr     x0, =0x810001400
    at      s1e1r, x0
    isb
    mrs     x0, PAR_EL1
    str     x0, [x21, #112]
    tbnz    x0, #0, .Lunsupported
    ldr     x0, =0x8BB80C000
    at      s1e1r, x0
    isb
    mrs     x0, PAR_EL1
    str     x0, [x21, #120]
    tbnz    x0, #0, .Lunsupported
    ldr     x0, =0x0031364b50415353 /* 16KPASS: set only after M=1 and ATs */
    str     x0, [x21]
    b       .Lcapture16
.Lunsupported:
    ldr     x0, =0x0031364b53544f50 /* 16KSTOP: unsupported or bad readback */
    str     x0, [x21]
    mrs     x0, SCTLR_EL1
    str     x0, [x21, #16]
    bic     x0, x0, #1
    msr     SCTLR_EL1, x0
    isb
    b       .Lcapture16
.global report_fault
report_fault:
    adr     x21, report_values
    /* Capture registers before switching back to physical accesses. */
    mrs     x0, SCTLR_EL1
    mrs     x1, ESR_EL1
    mrs     x2, FAR_EL1
    mrs     x3, ELR_EL1
    bic     x4, x0, #1
    bic     x4, x4, #4
    bic     x4, x4, #0x1000
    msr     SCTLR_EL1, x4
    isb
    str     x0, [x21, #16]
    str     x1, [x21, #104]         /* D/E/F become ESR/FAR/ELR on fault */
    str     x2, [x21, #112]
    str     x3, [x21, #120]
    ldr     x0, =0x0031364b4641494c /* 16KFAIL */
    str     x0, [x21]
.Lcapture16:
    mrs     x0, CurrentEL
    str     x0, [x21, #8]
    mrs     x0, TCR_EL1
    str     x0, [x21, #24]
    mrs     x0, TTBR0_EL1
    str     x0, [x21, #32]
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
    ldr     x1, =0x810004000
    ldr     x0, [x1, #(0x408*8)]
    str     x0, [x21, #88]
    ldr     x0, [x1, #(0x45d*8)]
    str     x0, [x21, #96]
    /* Preserve D=ESR on failure; success records the second display block. */
    ldr     x0, [x21]
    ldr     x2, =0x0031364b50415353
    cmp     x0, x2
    b.ne    .Lrender
    ldr     x0, [x1, #(0x45e*8)]
    str     x0, [x21, #104]
    b       .Lrender
