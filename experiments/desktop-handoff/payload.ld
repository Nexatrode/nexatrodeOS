ENTRY(run)
SECTIONS {
    . = 0x30000000;
    .payload : { KEEP(*(.entry)) *(.text*) *(.rodata*) }
    .tail 0x30000fff : { KEEP(*(.tail)) }
    /DISCARD/ : { *(.comment) *(.eh_frame*) *(.note*) }
    /* Writable state belongs in the explicitly reusable arena. */
    .unexpected : { *(.data*) *(.bss*) *(COMMON) *(.got*) }
    ASSERT(SIZEOF(.unexpected) == 0, "unexpected writable app data")
}
