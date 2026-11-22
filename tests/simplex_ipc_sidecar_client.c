#include <microkit.h>
#include <stdint.h>

#include "simplex_ipc_protocol.h"

#define AGENT_ENDPOINT 0u
#define REPLY_OK 0x300u

uintptr_t simplex_ipc_page;
static volatile struct nxsm_page *shared_page;

void init(void)
{
    shared_page = (volatile struct nxsm_page *)simplex_ipc_page;
    if (shared_page == 0) {
        microkit_dbg_puts("SIMPLEXIPC|FAIL_NO_PAGE\n");
        return;
    }

    shared_page->magic[0] = 'N';
    shared_page->magic[1] = 'X';
    shared_page->magic[2] = 'S';
    shared_page->magic[3] = 'M';
    shared_page->version = NXSM_VERSION;
    shared_page->operation = NXSM_OP_LIST_CONNECTIONS;
    shared_page->request_id = 1;
    shared_page->status = NXSM_STATUS_REQUEST;
    shared_page->payload_len = 0;

    microkit_msginfo reply = microkit_ppcall(
        AGENT_ENDPOINT, microkit_msginfo_new(1, 0));
    if (microkit_msginfo_get_label(reply) != REPLY_OK ||
        shared_page->magic[0] != 'N' || shared_page->magic[1] != 'X' ||
        shared_page->magic[2] != 'S' || shared_page->magic[3] != 'M' ||
        shared_page->version != NXSM_VERSION || shared_page->request_id != 1 ||
        shared_page->status != NXSM_STATUS_OK || shared_page->payload_len != 2 ||
        shared_page->payload[0] != 0 || shared_page->payload[1] != 0) {
        microkit_dbg_puts("SIMPLEXIPC|FAIL_BAD_RESPONSE\n");
        return;
    }

    microkit_dbg_puts("SIMPLEXIPC|PASS_GUEST_PROTECTED_CALL\n");
}

void notified(microkit_channel channel)
{
    (void)channel;
    microkit_dbg_puts("SIMPLEXIPC|ERROR_UNEXPECTED_CLIENT_NOTIFICATION\n");
}
