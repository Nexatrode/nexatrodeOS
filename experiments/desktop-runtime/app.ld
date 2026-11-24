ENTRY(app_entry)
PHDRS { text PT_LOAD FLAGS(5); data PT_LOAD FLAGS(6); }
SECTIONS {
    . = 0x10000000;
    .text : { KEEP(*(.entry)) *(.text*) *(.rodata*) } :text
    . = ALIGN(4096);
    .data : { *(.data*) } :data
    .bss : { *(.bss*) *(COMMON) } :data
    ASSERT(. <= 0x10010000, "app exceeds test image bound")
    /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) }
}
