/* Pongo raw-entry RAM page sweep; the stack stays inside the upload buffer. */
.section .text.entry, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    mov     x19, x0
    adr     x1, _start
    mov     x2, #0xf0000
    add     x2, x1, x2
    and     x2, x2, #0xfffffffffffffff0
    mov     sp, x2
    mov     x0, x19
    bl      ramsweep_report
.Lhalt:
    wfe
    b       .Lhalt
.size _start, .-_start
