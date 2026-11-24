#pragma once
#include <stdatomic.h>
#include <stdint.h>
#define LOCK_AUTH_CHANNEL 61u
#define LOCK_PROBE_CHANNEL 60u
#define LOCK_REQUEST 0x790u
#define LOCK_UNLOCK 0x791u
#define LOCK_STATUS 0x792u
#define LOCK_PREPARE 0x793u
#define LOCK_ABORT 0x794u
#define LOCK_VAULT_CHANNEL 13u
#define LOCK_VAULT_LEASE_MS 5000u
#define LOCK_AUTH_SURFACE_CHANNEL 9u
#define LOCK_TIMER_CHANNEL 12u
#ifndef LOCK_IDLE_MS
#define LOCK_IDLE_MS 60000u
#endif
struct lock_status_page {
    _Atomic uint32_t revision;
    uint32_t locked;
    uint64_t epoch, activity_ms, idle_ms;
};
struct lock_surface_header {
    _Atomic uint32_t revision;
    uint32_t width, height, stride;
    uint64_t epoch;
    uint32_t locked, reserved;
};
_Static_assert(sizeof(struct lock_status_page) == 32, "lock status ABI");
_Static_assert(sizeof(struct lock_surface_header) == 32, "lock surface ABI");
