/*
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * First bounded-memory adapter for GHC 9.6.6 on a Microkit protection domain.
 * The fixed arena is part of the PD image; no libc or VM syscalls are used.
 */

#include "Rts.h"

#include "RtsUtils.h"
#include "sm/OSMem.h"
#include "rts/storage/Block.h"

#include <stddef.h>
#include <stdint.h>

#ifndef SEL4_MBLOCK_COUNT
/* 8 MiB was enough for text/SMP bring-up; group joins + XFTP encrypt need more. */
#define SEL4_MBLOCK_COUNT 24
#endif

_Alignas(MBLOCK_SIZE)
static uint8_t mblock_arena[(size_t)SEL4_MBLOCK_COUNT * MBLOCK_SIZE];
static uint8_t mblock_used[SEL4_MBLOCK_COUNT];

void osMemInit(void)
{
    for (uint32_t i = 0; i < SEL4_MBLOCK_COUNT; i++) {
        mblock_used[i] = 0;
    }
}

void *osGetMBlocks(uint32_t count)
{
    if (count == 0 || count > SEL4_MBLOCK_COUNT) {
        barf("osGetMBlocks: request exceeds fixed Microkit arena");
    }

    uint32_t run = 0;
    for (uint32_t i = 0; i < SEL4_MBLOCK_COUNT; i++) {
        if (mblock_used[i]) {
            run = 0;
            continue;
        }

        run++;
        if (run == count) {
            uint32_t first = i + 1 - count;
            for (uint32_t j = first; j <= i; j++) {
                mblock_used[j] = 1;
            }
            return &mblock_arena[(size_t)first * MBLOCK_SIZE];
        }
    }

    barf("osGetMBlocks: fixed Microkit arena exhausted");
}

void osFreeMBlocks(void *addr, uint32_t count)
{
    uintptr_t base = (uintptr_t)mblock_arena;
    uintptr_t ptr = (uintptr_t)addr;
    uintptr_t arena_size = sizeof(mblock_arena);

    if (count == 0 || ptr < base || ptr >= base + arena_size) {
        barf("osFreeMBlocks: invalid Microkit arena range");
    }

    uintptr_t offset = ptr - base;
    uint32_t first = (uint32_t)(offset / MBLOCK_SIZE);
    if ((offset % MBLOCK_SIZE) != 0 ||
        count > SEL4_MBLOCK_COUNT - first) {
        barf("osFreeMBlocks: unaligned or out-of-bounds Microkit arena range");
    }

    for (uint32_t i = first; i < first + count; i++) {
        if (!mblock_used[i]) {
            barf("osFreeMBlocks: double free in Microkit arena");
        }
    }
    for (uint32_t i = first; i < first + count; i++) {
        mblock_used[i] = 0;
    }
}

void osReleaseFreeMemory(void)
{
    /* The PD's statically mapped memory cannot be returned to a host VM. */
}

void osFreeAllMBlocks(void)
{
    for (uint32_t i = 0; i < SEL4_MBLOCK_COUNT; i++) {
        mblock_used[i] = 0;
    }
}

size_t getPageSize(void)
{
    return 4096;
}

StgWord64 getPhysicalMemorySize(void)
{
    /* Unknown here: Microkit owns the static PD map, not a process VM. */
    return 0;
}

bool osBuiltWithNumaSupport(void)
{
    return false;
}

bool osNumaAvailable(void)
{
    return false;
}

uint32_t osNumaNodes(void)
{
    return 1;
}

uint64_t osNumaMask(void)
{
    return 1;
}

void osBindMBlocksToNode(void *addr STG_UNUSED, StgWord size STG_UNUSED,
                         uint32_t node STG_UNUSED)
{
}
