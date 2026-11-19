#include <microkit.h>
#include <stdint.h>

#include "simplex_ipc_protocol.h"

#define SIMPLEX_AGENT_CHANNEL 5u
#define SIMPLEX_IPC_REPLY_CHANNEL SIMPLEX_AGENT_CHANNEL
/* Distinct from NXSM_STATUS_NO_EVENT (2). A busy page must not look like an
 * empty receive, or a later inbox wake can discard the request in flight. */
#define NXSM_STATUS_BUSY 6u
#define NXSM_STATUS_ERROR 3u
#define NXSM_REPLY_OK 0x300u

uintptr_t simplex_ipc_page;
static volatile struct nxsm_page *page;
static volatile int pending_inbox_wake;

static void maybe_wake_pending_inbox(void)
{
    if (pending_inbox_wake == 0 || page == 0) {
        return;
    }
    if (page->status == NXSM_STATUS_BUSY ||
        page->status == NXSM_STATUS_REQUEST) {
        return;
    }
    pending_inbox_wake = 0;
    microkit_notify(SIMPLEX_IPC_REPLY_CHANNEL);
}

void sel4_simplex_ipc_init(void)
{
    page = (volatile struct nxsm_page *)simplex_ipc_page;
}

void sel4_simplex_ipc_notified(microkit_channel channel)
{
    if (channel == SIMPLEX_AGENT_CHANNEL && page != 0) {
        /* The shared request page is authoritative.  The Agent loop polls it
         * as well, so a request sent during lengthy service initialization is
         * not lost if its notification arrives before the loop is running. */
    }
}

int sel4_simplex_ipc_take(uint32_t *operation, uint32_t *request_id,
                          uint8_t *payload, uint32_t payload_capacity,
                          uint32_t *payload_length)
{
    if (page == 0 || operation == 0 || request_id == 0 ||
        payload_length == 0) {
        return 0;
    }

    if (page->magic[0] != 'N' || page->magic[1] != 'X' ||
        page->magic[2] != 'S' || page->magic[3] != 'M' ||
        page->version != NXSM_VERSION || page->request_id == 0 ||
        page->status != NXSM_STATUS_REQUEST ||
        page->payload_len > NXSM_PAYLOAD_CAPACITY ||
        page->payload_len > payload_capacity ||
        (page->payload_len != 0 && payload == 0)) {
        return -1;
    }

    *operation = page->operation;
    *request_id = page->request_id;
    *payload_length = page->payload_len;
    for (uint32_t i = 0; i < page->payload_len; ++i) {
        payload[i] = page->payload[i];
    }
    page->status = NXSM_STATUS_BUSY;
    return 1;
}

void sel4_simplex_ipc_complete(uint32_t request_id, uint32_t status,
                               uint32_t connection_count)
{
    if (page == 0 || page->request_id != request_id ||
        page->status != NXSM_STATUS_BUSY) {
        return;
    }
    if (status == NXSM_STATUS_OK) {
        uint16_t count = connection_count > UINT16_MAX
            ? UINT16_MAX : (uint16_t)connection_count;
        page->payload[0] = (uint8_t)(count & 0xffu);
        page->payload[1] = (uint8_t)(count >> 8);
        page->payload_len = 2;
        page->status = NXSM_STATUS_OK;
    } else if (status == NXSM_STATUS_NO_EVENT) {
        page->payload_len = 0;
        page->status = NXSM_STATUS_NO_EVENT;
    } else {
        page->payload_len = 0;
        page->status = NXSM_STATUS_ERROR;
    }
    microkit_notify(SIMPLEX_IPC_REPLY_CHANNEL);
    maybe_wake_pending_inbox();
}

void sel4_simplex_ipc_complete_bytes(uint32_t request_id, uint32_t status,
                                     const uint8_t *payload, uint32_t length)
{
    if (page == 0 || page->request_id != request_id ||
        page->status != NXSM_STATUS_BUSY || length > NXSM_PAYLOAD_CAPACITY ||
        (length != 0 && payload == 0)) {
        return;
    }
    if (status == NXSM_STATUS_OK) {
        for (uint32_t i = 0; i < length; ++i) page->payload[i] = payload[i];
        page->payload_len = length;
        page->status = NXSM_STATUS_OK;
    } else if (status == NXSM_STATUS_NO_EVENT) {
        page->payload_len = 0;
        page->status = NXSM_STATUS_NO_EVENT;
    } else {
        page->payload_len = 0;
        page->status = NXSM_STATUS_ERROR;
    }
    microkit_notify(SIMPLEX_IPC_REPLY_CHANNEL);
    maybe_wake_pending_inbox();
}

void sel4_simplex_ipc_notify(void)
{
    if (page == 0) {
        return;
    }
    /* A wake during an in-flight request must not be dropped: the reply
     * notify is consumed as "request done", so remember to wake again. */
    if (page->status == NXSM_STATUS_BUSY ||
        page->status == NXSM_STATUS_REQUEST) {
        pending_inbox_wake = 1;
        return;
    }
    pending_inbox_wake = 0;
    microkit_notify(SIMPLEX_IPC_REPLY_CHANNEL);
}
