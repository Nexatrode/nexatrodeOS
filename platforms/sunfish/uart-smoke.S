/*
 * Pixel 4a pre-seL4 handoff probe. This is an AArch64 Linux Image-shaped
 * payload, intended only for a temporary `fastboot boot` experiment.
 * It writes a short marker to the already-enabled GENI debug UART, waits,
 * then asks PSCI to reset back into the untouched LineageOS slot.
 *
 * Physical MMIO assumes the bootloader enters with the MMU off, as required
 * by the arm64 boot protocol. The real seL4 port will need its own mapping.
 */

    .section .text.boot, "ax"
    .balign 4096
    .global _start
_start:
    b       start
    .long   0
    .quad   0x80000                 /* text_offset, matching current Image */
    .quad   image_end - _start
    .quad   0x0a                    /* little endian, 4 KiB pages */
    .quad   0
    .quad   0
    .quad   0
    .long   0x644d5241             /* "ARM\x64" */
    .long   0

start:
    msr     daifset, #0xf
    mov     x24, x0                  /* DTB pointer from bootloader */
    adr     x25, _start              /* physical PC while MMU is off */
    movz    x20, #0x8000
    movk    x20, #0x00a8, lsl #16  /* uart8 physical base 0x00a88000 */
    adr     x21, message
    bl      send_string
    mrs     x22, CurrentEL
    cmp     x22, #4
    b.eq    entered_el1
    cmp     x22, #8
    b.eq    entered_el2
    adr     x21, unknown_el
    bl      send_string
    b       print_addresses

entered_el1:
    mrs     x22, SCTLR_EL1
    adr     x21, el1_off
    tbz     x22, #0, 1f
    adr     x21, el1_on
1:
    bl      send_string
    b       print_addresses

entered_el2:
    mrs     x22, SCTLR_EL2
    adr     x21, el2_off
    tbz     x22, #0, 1f
    adr     x21, el2_on
1:
    bl      send_string
    b       print_addresses

print_addresses:
    adr     x21, pc_label
    bl      send_string
    mov     x0, x25
    bl      print_hex64
    adr     x21, dtb_label
    bl      send_string
    mov     x0, x24
    bl      print_hex64
    b       wait_then_reset

send_string:
    mov     x23, x30
1:
    ldrb    w1, [x21], #1
    cbz     w1, 2f
    bl      putchar
    b       1b
2:
    mov     x30, x23
    ret

print_hex64:
    mov     x29, x30
    mov     x27, x0
    mov     x26, #16
1:
    lsr     x4, x27, #60
    cmp     x4, #10
    b.lo    2f
    add     w1, w4, #('A' - 10)
    b       3f
2:
    add     w1, w4, #'0'
3:
    bl      putchar
    lsl     x27, x27, #4
    subs    x26, x26, #1
    b.ne    1b
    mov     w1, #13
    bl      putchar
    mov     w1, #10
    bl      putchar
    mov     x30, x29
    ret

/* One GENI UART_START_TX command per byte, following the Linux poll path. */
putchar:
    mov     w2, #1
    str     w2, [x20, #0x618]      /* clear M_CMD_DONE */
    str     w2, [x20, #0x270]      /* SE_UART_TX_TRANS_LEN */
    movz    w2, #0x800, lsl #16    /* UART_START_TX << M_OPCODE_SHFT */
    str     w2, [x20, #0x600]      /* SE_GENI_M_CMD0 */
    str     w1, [x20, #0x700]      /* SE_GENI_TX_FIFOn */
    movz    x3, #0xffff
    movk    x3, #0x000f, lsl #16
2:
    ldr     w2, [x20, #0x610]      /* SE_GENI_M_IRQ_STATUS */
    tbnz    w2, #0, 3f             /* M_CMD_DONE */
    subs    x3, x3, #1
    b.ne    2b
3:
    ret

wait_then_reset:
    mrs     x1, cntfrq_el0
    mov     x2, #5
    mul     x1, x1, x2
    mrs     x2, cntpct_el0
    add     x1, x1, x2
4:
    mrs     x2, cntpct_el0
    cmp     x2, x1
    b.lo    4b
    movz    w0, #0x0009
    movk    w0, #0x8400, lsl #16   /* PSCI SYSTEM_RESET */
    smc     #0
5:
    wfe
    b       5b

message:
    .asciz  "\r\nSUNFISH UART SMOKE OK\r\n"
el1_off:
    .asciz  "EL1 MMU OFF\r\n"
el1_on:
    .asciz  "EL1 MMU ON\r\n"
el2_off:
    .asciz  "EL2 MMU OFF\r\n"
el2_on:
    .asciz  "EL2 MMU ON\r\n"
unknown_el:
    .asciz  "UNKNOWN EL\r\n"
pc_label:
    .asciz  "PC  0x"
dtb_label:
    .asciz  "DTB 0x"
    .balign 4
image_end:
