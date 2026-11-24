/* Bounded executable policy for this embedded graphical fixture. No relocations. */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include "../desktop-runtime/elf.h"
#define GUI_IMAGE_BASE 0x200000UL
#define GUI_IMAGE_LIMIT 0x1000000UL
#define GUI_PACKAGE_LIMIT (4 * 1024 * 1024UL)

static inline bool graphical_image_valid(const uint8_t *bytes, size_t length)
{
    if (length < sizeof(struct elf_header) || length > GUI_PACKAGE_LIMIT || (uintptr_t)bytes % 8)
        return false;
    const struct elf_header *h = (const void *)bytes;
    if (h->ident[0] != 0x7f || h->ident[1] != 'E' || h->ident[2] != 'L' || h->ident[3] != 'F' ||
        h->ident[4] != 2 || h->ident[5] != 1 || h->ident[6] != 1 || h->version != 1 ||
        h->type != 2 || h->machine != 183 || h->entry % 4 || h->ehsize != sizeof(*h) ||
        h->phoff != sizeof(*h) || h->phentsize != sizeof(struct program_header) ||
        !h->phnum || h->phnum > 8 || h->phnum > (length - h->phoff) / h->phentsize)
        return false;
    bool executable_entry = false, header_mapped = false;
    uint64_t previous_end = GUI_IMAGE_BASE;
    const struct program_header *programs = (const void *)(bytes + h->phoff);
    for (unsigned i = 0; i < h->phnum; ++i) {
        const struct program_header *p = &programs[i];
        if (p->type == 2 || p->type == 3) return false;
        if (p->type == 7 && (p->filesz || p->memsz > 4096 || !p->align ||
                            p->align > 64 || (p->align & (p->align - 1)))) return false;
        if (p->type != 1) continue;
        if (!p->memsz) { if (p->filesz) return false; else continue; }
        uint64_t first = p->address & ~4095UL;
        if (first < previous_end || p->address >= GUI_IMAGE_LIMIT || p->align != 4096 ||
            p->memsz > GUI_IMAGE_LIMIT - p->address || p->filesz > p->memsz ||
            p->offset > length || p->filesz > length - p->offset ||
            p->offset % 4096 != p->address % 4096 || !(p->flags & 4) ||
            (p->flags & ~7U) || (p->flags & 3) == 3)
            return false;
        previous_end = (p->address + p->memsz + 4095) & ~4095UL;
        if (p->address == GUI_IMAGE_BASE && !p->offset &&
            p->filesz >= h->phoff + h->phnum * h->phentsize) header_mapped = true;
        if ((p->flags & 1) && h->entry >= p->address && h->entry - p->address < p->filesz)
            executable_entry = true;
    }
    return header_mapped && executable_entry;
}
