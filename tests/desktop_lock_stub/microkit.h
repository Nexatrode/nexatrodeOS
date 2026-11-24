#pragma once
#include <stdint.h>
typedef unsigned microkit_channel;
typedef struct { uint64_t label; unsigned count; } microkit_msginfo;
microkit_msginfo microkit_ppcall(unsigned, microkit_msginfo);
microkit_msginfo microkit_msginfo_new(uint64_t, unsigned);
uint64_t microkit_msginfo_get_label(microkit_msginfo);
unsigned microkit_msginfo_get_count(microkit_msginfo);
void microkit_mr_set(unsigned, uint64_t);
uint64_t microkit_mr_get(unsigned);
void microkit_dbg_puts(const char *);
