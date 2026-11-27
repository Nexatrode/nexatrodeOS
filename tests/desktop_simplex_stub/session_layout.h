#pragma once
#define SESSION_CHILD_COUNT TEST_CHILDREN
#define SESSION_FIRST_CHILD 32u
#define SESSION_SC_CAP_BASE TEST_SC_BASE
#define SESSION_STATE_ADDRESS ((uintptr_t)state_words)
#define SESSION_STACK_TOP 0x7ffffff000UL
struct session_child { uintptr_t address; size_t pages; uintptr_t entry; };
struct session_range { uintptr_t address; size_t bytes; };
static struct session_child session_children[TEST_CHILDREN];
static const struct session_range session_ranges[] = {
    {(uintptr_t)private_pages,sizeof(private_pages)}, {(uintptr_t)shared,sizeof(shared)},
};
#define SESSION_RANGE_COUNT 2u

#define SESSION_BOOT_RANGE_COUNT 1u
