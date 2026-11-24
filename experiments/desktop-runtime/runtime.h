#pragma once
#include <sel4/sel4.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define APP_BASE 0x10000000UL
#define APP_LIMIT 0x10010000UL
#define APP_STACK 0x10010000UL
#define APP_IPC 0x10011000UL
#define APP_SCRATCH 0x10012000UL
#define PRIVATE_WALLET 0x50000000UL
#define PAGE_BYTES 4096
#define APP_CNODE_BITS 8
#define APP_SERVER 1
#ifndef CYCLES
#define CYCLES 100
#endif

enum request { SAVE = 0x700, LOAD, WALLET_READ, FINISHED, PROBE_FAULT };
enum app_kind { CHESS = 1, SUCCESSOR = 2 };

static inline void print(const char *value)
{
    while (*value)
        seL4_DebugPutChar(*value++);
}
