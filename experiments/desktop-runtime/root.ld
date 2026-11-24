ENTRY(_start)
PHDRS { text PT_LOAD FLAGS(5); data PT_LOAD FLAGS(6); }
SECTIONS {
    . = 0x200000;
    .text : { *(.text.start) *(.text*) *(.rodata*) } :text
    . = ALIGN(4096);
    .data : { *(.data*) } :data
    .bss : {
        *(.bss*) *(COMMON)
        . = ALIGN(16);
        . += 65536;
        root_stack_end = .;
    } :data
    /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) }
}
