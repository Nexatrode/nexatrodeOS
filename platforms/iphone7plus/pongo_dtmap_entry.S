/* Raw Pongo entry for the Apple device-tree reservation probe. */
.section .text.entry, "ax"
.balign 4
.global _start
.type _start, %function
_start:
    /* x0 is boot_args. Use the same temporary upload-buffer stack as probe 3. */
    adr     x1, _start
    mov     x9, #0xf0000
    add     x2, x1, x9
    and     x2, x2, #0xfffffffffffffff0
    mov     sp, x2
    bl      dtmap_report_main
.Lhalt:
    wfe
    b       .Lhalt
.size _start, .-_start
