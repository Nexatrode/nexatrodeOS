.section .text.start, "ax"
.global _start
_start:
    ldr x1, =root_stack_end
    mov sp, x1
    bl root_main
1:  b 1b
