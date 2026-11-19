/* Temporary entry diagnostic: draw once, then stop before any loader work. */
.section .text.start, "ax"
.global _start
.type _start, %function
_start:
    adrp x1, entry_stack_top
    add x1, x1, :lo12:entry_stack_top
    mov sp, x1
    msr daifset, #0xf
    adr x0, .Lentry_text
    bl loader_checkpoint
.global entry_hold
entry_hold:
    b entry_hold
.Lentry_text:
    .asciz "IMAGE ENTRY HOLD"
.size _start, . - _start

/* Include actual zero bytes so the boot image provides the entire stack. */
.section .entry_stack, "aw", %progbits
.balign 4096
    .space 4096
entry_stack_top:
