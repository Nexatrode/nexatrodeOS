#pragma once
#include <stdbool.h>
#include <stdint.h>
/* Disposable UTM network lifecycle endpoint, granted only to the lock manager. */
#define SIMPLEX_NET_MANAGER_CHANNEL 7u
#define SIMPLEX_NET_LOCK 0x46au
#define SIMPLEX_NET_UNLOCK 0x46bu
#define SIMPLEX_NET_OK 0x46cu
#define SIMPLEX_NET_DENIED 0x46du
#ifdef SIMPLEX_LOCK_LIFECYCLE
bool haskell_simplex_network_transition(bool unlock, uint64_t epoch);
bool haskell_simplex_network_active(void);
bool haskell_simplex_network_cleared(void);
uint64_t haskell_simplex_network_epoch(void);
#endif
