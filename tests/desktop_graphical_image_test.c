#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../experiments/desktop-graphical/image.h"

int main(int argc, char **argv)
{
    assert(argc == 2);
    FILE *file = fopen(argv[1], "rb"); assert(file);
    assert(!fseek(file, 0, SEEK_END)); long size = ftell(file); assert(size > 0);
    rewind(file);
    size_t length = (size_t)size;
    uint8_t *original = malloc(length), *mutated = malloc(length); assert(original && mutated);
    assert(fread(original, 1, length, file) == length); fclose(file);
    assert(graphical_image_valid(original, length));
    assert(!graphical_image_valid(original, 7));
    struct elf_header *h = (void *)mutated;
    struct program_header *p = (void *)(mutated + sizeof(*h));
    unsigned load = 0;
    const struct elf_header *source = (void *)original;
    const struct program_header *programs = (void *)(original + source->phoff);
    while (load < source->phnum && programs[load].type != 1) ++load;
    assert(load < source->phnum);
#define REJECT(change) do { memcpy(mutated, original, length); change; assert(!graphical_image_valid(mutated, length)); } while (0)
    REJECT(h->phoff = UINT64_MAX);
    REJECT(h->phentsize = 0);
    REJECT(h->phnum = UINT16_MAX);
    REJECT(h->entry = GUI_IMAGE_BASE);
    REJECT(p[load].flags = 7);
    REJECT(p[load].address = GUI_IMAGE_LIMIT);
    REJECT(p[load].memsz = UINT64_MAX);
    REJECT(p[load].filesz = UINT64_MAX);
    REJECT(p[load].offset = UINT64_MAX);
    REJECT(p[load].address++);
    REJECT(p[load].align = 0);
    REJECT(p[load].type = 3);
    REJECT(p[load].filesz = 0);
    uint32_t random = 0x12345678;
    for (unsigned trial = 0; trial < 10000; ++trial) {
        memcpy(mutated, original, 512);
        random = random * 1664525u + 1013904223u;
        mutated[random % 512] ^= (uint8_t)(1u << (random >> 29));
        (void)graphical_image_valid(mutated, length);
    }
    free(original); free(mutated);
    puts("PASS real graphical ELF, policy rejections and 10000 bounded parser mutations");
}
