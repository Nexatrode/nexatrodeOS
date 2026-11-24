#pragma once
#include <stdint.h>
typedef uint64_t seL4_Word;
typedef struct { seL4_Word label; unsigned count; } microkit_msginfo;
microkit_msginfo microkit_ppcall(unsigned, microkit_msginfo);
microkit_msginfo microkit_msginfo_new(seL4_Word, unsigned);
seL4_Word microkit_msginfo_get_label(microkit_msginfo);
unsigned microkit_msginfo_get_count(microkit_msginfo);
int seL4_Wait(unsigned, seL4_Word *);
void microkit_dbg_puts(const char *);
