/* Raw Pongo entry for a bounded validator matching Pongo's dt_check(). */
.section .text.entry, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    adr     x1, _start
    mov     x9, #0xf0000
    add     x2, x1, x9
    and     x2, x2, #0xfffffffffffffff0
    mov     sp, x2
    bl      dtcheck_report_main
.Lhalt:
    wfe
    b       .Lhalt
.size _start, .-_start
