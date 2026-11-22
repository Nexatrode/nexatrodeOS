#include <microkit.h>
#include <stdint.h>

#include "simplex_ipc_protocol.h"

#define CLIENT_CHANNEL 0u
#define REPLY_OK 0x300u
#define NXSM_STATUS_BUSY 2u

uintptr_t simplex_ipc_page;
static volatile struct nxsm_page *page;
static uint32_t expected_request_id = 1;

void init(void)
{
    page = (volatile struct nxsm_page *)simplex_ipc_page;
    if (page == 0) {
        microkit_dbg_puts("SIMPLEXIPC|FAIL_NO_PAGE\n");
        return;
    }

    page->magic[0] = 'N';
    page->magic[1] = 'X';
    page->magic[2] = 'S';
    page->magic[3] = 'M';
    page->version = NXSM_VERSION;
    page->operation = NXSM_OP_LIST_CONNECTIONS;
    page->request_id = expected_request_id;
    page->status = NXSM_STATUS_REQUEST;
    page->payload_len = 0;
    microkit_notify(CLIENT_CHANNEL);
}

void notified(microkit_channel channel)
{
    if (channel != CLIENT_CHANNEL || page == 0) return;
    if (page->magic[0] != 'N' || page->magic[1] != 'X' ||
        page->magic[2] != 'S' || page->magic[3] != 'M' ||
        page->version != NXSM_VERSION || page->request_id != expected_request_id ||
        page->status != NXSM_STATUS_OK || page->payload_len != 2 ||
        page->payload[0] != 0 || page->payload[1] != 0) {
        microkit_dbg_puts("SIMPLEXIPC|FAIL_GUEST_AGENT_LIST_CONNECTIONS\n");
        return;
    }
    microkit_dbg_puts("SIMPLEXIPC|PASS_GUEST_AGENT_LIST_CONNECTIONS|0\n");
}
