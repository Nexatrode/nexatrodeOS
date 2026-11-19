#pragma once
#include <stdint.h>
#include <stdatomic.h>
typedef struct { _Atomic uint64_t deadline_ns; } hvf_timer_shared;
