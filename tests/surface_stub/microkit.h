#pragma once
#include <stdint.h>
typedef unsigned microkit_channel;
typedef unsigned microkit_child;
typedef unsigned microkit_msginfo;
typedef unsigned seL4_Bool;
enum { seL4_False, seL4_Fault_VMFault = 5, seL4_VMFault_Addr = 1, seL4_VMFault_PrefetchFault = 2 };
static inline void microkit_dbg_puts(const char *text) { (void)text; }
static inline void microkit_notify(unsigned channel) { (void)channel; }
static inline void microkit_pd_stop(unsigned child) { (void)child; }
static inline unsigned microkit_msginfo_get_label(unsigned info) { return info; }
static inline uintptr_t microkit_mr_get(unsigned index) { (void)index; return 0; }
