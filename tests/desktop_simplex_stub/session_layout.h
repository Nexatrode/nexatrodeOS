#pragma once
#define SESSION_CHILD_COUNT 6u
#define SESSION_FIRST_CHILD 32u
#define SESSION_STATE_ADDRESS ((uintptr_t)state_words)
#define SESSION_STACK_TOP 0x7ffffff000UL
struct session_child { uintptr_t address; size_t pages; uintptr_t entry; };
struct session_range { uintptr_t address; size_t bytes; };
static const struct session_child session_children[] = {
    {(uintptr_t)private_pages[0],2,0x10000}, {(uintptr_t)private_pages[1],2,0x20000},
    {(uintptr_t)private_pages[2],2,0x30000}, {(uintptr_t)private_pages[3],2,0x40000},
    {(uintptr_t)private_pages[4],2,0x50000}, {(uintptr_t)private_pages[5],2,0x60000},
};
static const struct session_range session_ranges[] = {
    {(uintptr_t)private_pages,sizeof(private_pages)}, {(uintptr_t)shared,sizeof(shared)},
};
#define SESSION_RANGE_COUNT 2u
