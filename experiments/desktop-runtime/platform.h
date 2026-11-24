#pragma once
#ifdef RUNTIME_HOSTED
#include <microkit.h>
#define ROOT_NODE 458
#define ROOT_DEPTH 12
#define ROOT_VSPACE (4UL << 58)
#define ROOT_TCB (5UL << 58)
#define ROOT_ASID (3UL << 58)
#define HOSTED_POOL (2UL << 58)
#define HOSTED_SC (6UL << 58)
#define HOSTED_BUDGET_TCB (7UL << 58)
static inline seL4_CPtr slot_cap(seL4_CPtr slot) { return (1UL << 58) | slot; }
static inline seL4_CPtr cap_slot(seL4_CPtr cap) { return cap & 4095; }
#else
#define ROOT_NODE seL4_CapInitThreadCNode
#define ROOT_DEPTH seL4_WordBits
#define ROOT_VSPACE seL4_CapInitThreadVSpace
#define ROOT_TCB seL4_CapInitThreadTCB
#define ROOT_ASID seL4_CapInitThreadASIDPool
static inline seL4_CPtr slot_cap(seL4_CPtr slot) { return slot; }
static inline seL4_CPtr cap_slot(seL4_CPtr cap) { return cap; }
#endif
