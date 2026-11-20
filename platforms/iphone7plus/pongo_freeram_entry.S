/*
 * Temporary Pongo raw-entry smoke test for the allocator-reported RAM run.
 * Pongo bootr passes Apple's boot_args in x0; this shim moves the stack into
 * the reported free interval before entering C.
 */
.section .text.entry, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    mov     x19, x0
    adr     x1, _start
    movz    x2, #0x8, lsl #32
    movk    x2, #0xBAD8, lsl #16  /* exclusive top: 0x8BAD80000 */
    mov     sp, x2
    mov     x0, x19
    bl      freeram_report
.Lhalt:
    wfe
    b       .Lhalt
.size _start, .-_start
