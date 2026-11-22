/*
 * Stack setup for the boot_args report payload.
 * PongoOS 2.6.3 allocates at least a 1 MiB contiguous upload buffer; this
 * small probe places its temporary stack near the top of that same buffer.
 */
.section .text.entry, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    /* x0 is Apple's boot_args pointer. Keep it intact for report_main. */
    adr     x1, _start
    mov     x9, #0xf0000
    add     x2, x1, x9
    and     x2, x2, #0xfffffffffffffff0
    mov     sp, x2
    mrs     x3, cntfrq_el0
    mrs     x4, mpidr_el1
    bl      report_main
.Lhalt:
    wfe
    b       .Lhalt
.size _start, .-_start
