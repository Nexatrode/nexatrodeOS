/* ELF envelope for QEMU/UTM's -kernel loader. The SDK image remains unchanged. */
.section .text.boot, "ax"
.global _start
.balign 16
_start:
.incbin "build/loader.img"
