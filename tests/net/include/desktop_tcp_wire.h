/* Endpoint-selected wire ownership for the opt-in three-app desktop only. */
#pragma once
#include <stdbool.h>
#include "haskell_tcp_protocol.h"
static inline bool desktop_tcp_permitted(unsigned channel, bool active)
{
    return active || !(channel==HASKELL_TCP_CHANNEL || channel==WALLET_TCP_CHANNEL ||
        channel==WORKER_TCP_CHANNEL || channel==SHELL_TCP_CHANNEL || channel==BROWSER_TCP_CHANNEL);
}
static inline int desktop_tcp_slot(unsigned channel, unsigned requested)
{
    if (channel == WALLET_TCP_CHANNEL) return HASKELL_TCP_WALLET_SLOT;
    if (channel == WORKER_TCP_CHANNEL) return HASKELL_TCP_WORKER_SLOT;
    if (channel == BROWSER_TCP_CHANNEL) return HASKELL_TCP_BROWSER_SLOT;
    if ((channel == HASKELL_TCP_CHANNEL || channel == SHELL_TCP_CHANNEL) && requested < 2) return requested;
    return -1;
}
static inline volatile struct haskell_tcp_page *desktop_tcp_wire(
    uintptr_t agent, uintptr_t wallet, uintptr_t browser, unsigned slot, bool local)
{
    if (local && slot == HASKELL_TCP_BROWSER_SLOT) return (void *)browser;
    if (local && (slot == HASKELL_TCP_WALLET_SLOT || slot == HASKELL_TCP_WORKER_SLOT))
        return (volatile struct haskell_tcp_page *)wallet + slot;
    return (void *)agent;
}
