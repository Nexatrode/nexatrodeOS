#include <microkit.h>
#include <stdint.h>

#include "simplex_ipc_protocol.h"

#define IPC_ENDPOINT 0u
#define REPLY_BAD_REQUEST 0x301u
#define REPLY_OK 0x300u

uintptr_t simplex_ipc_page;
static volatile struct nxsm_page *shared_page;

void init(void)
{
    shared_page = (volatile struct nxsm_page *)simplex_ipc_page;
    microkit_dbg_puts("SIMPLEXIPC|SIDECAR_READY\n");
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo message)
{
    (void)message;
    if (channel != IPC_ENDPOINT || shared_page == 0 ||
        shared_page->magic[0] != 'N' || shared_page->magic[1] != 'X' ||
        shared_page->magic[2] != 'S' || shared_page->magic[3] != 'M' ||
        shared_page->version != NXSM_VERSION ||
        shared_page->request_id == 0 ||
        shared_page->status != NXSM_STATUS_REQUEST ||
        shared_page->payload_len > NXSM_PAYLOAD_CAPACITY ||
        shared_page->operation != NXSM_OP_LIST_CONNECTIONS ||
        shared_page->payload_len != 0) {
        return microkit_msginfo_new(REPLY_BAD_REQUEST, 0);
    }

    /* ListConnections succeeds with an empty result set: u16 count = 0. */
    shared_page->payload[0] = 0;
    shared_page->payload[1] = 0;
    shared_page->payload_len = 2;
    shared_page->status = NXSM_STATUS_OK;
    microkit_dbg_puts("SIMPLEXIPC|SERVICE_ROUNDTRIP\n");
    return microkit_msginfo_new(REPLY_OK, 0);
}

void notified(microkit_channel channel)
{
    (void)channel;
    microkit_dbg_puts("SIMPLEXIPC|ERROR_UNEXPECTED_NOTIFICATION\n");
}
