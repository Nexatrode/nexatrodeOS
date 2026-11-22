ENTRY(_start)
SECTIONS
{
    . = 0;
    .text : ALIGN(16)
    {
        KEEP(*(.text.entry))
        *(.text*)
        *(.rodata*)
    }
    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
