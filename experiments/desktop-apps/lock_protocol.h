/* Trusted desktop lock state. Manager is sole state writer; GPU alone acks. */
#pragma once
#include <stdatomic.h>
#include <stdint.h>
#include "../desktop-lock/protocol.h"
#define TRIO_AUTH_CHANNEL 60u
#define TRIO_PROBE_CHANNEL 59u
#define TRIO_STATUS_PHYS 0x5fb93000u
#define TRIO_ACK_PHYS 0x5fb94000u
#define TRIO_AUTH_W 2560u
#define TRIO_AUTH_H 1600u
enum trio_lock_phase {TRIO_LOCKING=1,TRIO_LOCKED=2,TRIO_OPEN=3,TRIO_FAILED=4};
struct trio_status {
    _Atomic uint32_t revision;uint32_t locked;
    uint64_t epoch,activity_ms,idle_ms;
    uint32_t phase,failed;
    uint64_t scrubbed,generation,foreign_denials;
};
struct trio_display_ack {
    _Atomic uint32_t revision;uint32_t targets;
    uint64_t epoch,erased_bytes;
    uint32_t width,height;
};
_Static_assert(sizeof(struct trio_status)==64,"desktop lock status");
_Static_assert(sizeof(struct trio_display_ack)==32,"desktop display lock ack");
struct trio_auth_box {int left,top,width,height;};
static inline struct trio_auth_box trio_auth_box(unsigned w,unsigned h) {
    unsigned height=h,width=(uint64_t)h*TRIO_AUTH_W/TRIO_AUTH_H;
    if(width>w){width=w;height=(uint64_t)w*TRIO_AUTH_H/TRIO_AUTH_W;}
    return (struct trio_auth_box){(w-width)/2,(h-height)/2,width,height};
}
