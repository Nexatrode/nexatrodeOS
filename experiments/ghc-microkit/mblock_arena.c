/*
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Small deterministic allocator for GHC-style 1 MiB aligned megablocks.
 * Microkit protection domains have statically mapped memory, so this reserves
 * all backing storage in the PD image and never calls an OS allocator.
 */
#include "mblock_arena.h"

#include <stddef.h>
#include <stdint.h>

_Alignas(GHC_MBLOCK_SIZE)
static uint8_t arena[GHC_MBLOCK_COUNT * GHC_MBLOCK_SIZE];
static bool occupied[GHC_MBLOCK_COUNT];
static uint32_t used;

void ghc_mblock_init(void)
{
    for (uint32_t i = 0; i < GHC_MBLOCK_COUNT; i++) {
        occupied[i] = false;
    }
    used = 0;
}

void *ghc_mblock_alloc(uint32_t count)
{
    if (count == 0 || count > GHC_MBLOCK_COUNT || count > GHC_MBLOCK_COUNT - used) {
        return NULL;
    }

    uint32_t run = 0;
    for (uint32_t i = 0; i < GHC_MBLOCK_COUNT; i++) {
        if (occupied[i]) {
            run = 0;
            continue;
        }

        run++;
        if (run == count) {
            uint32_t first = i + 1 - count;
            for (uint32_t j = first; j <= i; j++) {
                occupied[j] = true;
            }
            used += count;
            return &arena[(size_t)first * GHC_MBLOCK_SIZE];
        }
    }

    return NULL;
}

bool ghc_mblock_free(void *addr, uint32_t count)
{
    uintptr_t base = (uintptr_t)arena;
    uintptr_t ptr = (uintptr_t)addr;
    uintptr_t size = sizeof(arena);

    if (count == 0 || ptr < base || ptr >= base + size) {
        return false;
    }

    uintptr_t offset = ptr - base;
    if ((offset % GHC_MBLOCK_SIZE) != 0 ||
        count > GHC_MBLOCK_COUNT - (uint32_t)(offset / GHC_MBLOCK_SIZE)) {
        return false;
    }

    uint32_t first = (uint32_t)(offset / GHC_MBLOCK_SIZE);
    for (uint32_t i = first; i < first + count; i++) {
        if (!occupied[i]) {
            return false;
        }
    }

    for (uint32_t i = first; i < first + count; i++) {
        occupied[i] = false;
    }
    used -= count;
    return true;
}

uint32_t ghc_mblock_used(void)
{
    return used;
}

