/*
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Fixed, page-free megablock arena for the first non-threaded GHC RTS probe.
 * The eventual RTS adapter can wrap these operations as osGetMBlocks and
 * osFreeMBlocks after checking the selected RTS configuration.
 */
#pragma once

#include <stdbool.h>
#include <stdint.h>

#define GHC_MBLOCK_SHIFT 20u
#define GHC_MBLOCK_SIZE (1u << GHC_MBLOCK_SHIFT)
#define GHC_MBLOCK_COUNT 8u

void ghc_mblock_init(void);
void *ghc_mblock_alloc(uint32_t count);
bool ghc_mblock_free(void *addr, uint32_t count);
uint32_t ghc_mblock_used(void);

