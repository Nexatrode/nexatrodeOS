#pragma once

#include <sddf/util/ialloc.h>

static inline void ialloc_init_with_offset(ialloc_t *allocator, uint32_t *idxlist,
                                            uint32_t capacity, uint32_t offset)
{
    ialloc_init(allocator, idxlist, capacity);
    while (offset--) {
        uint32_t reserved_id;
        assert(ialloc_alloc(allocator, &reserved_id) == 0);
    }
}
