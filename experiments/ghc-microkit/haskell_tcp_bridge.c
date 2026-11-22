/* Native Haskell's bounded TCP session adapter. The network PD owns lwIP. */
#include <stdint.h>
#include <microkit.h>
#include "haskell_tcp_protocol.h"

uintptr_t haskell_tcp_page;

#ifndef HASKELL_TCP_CLIENT_CHANNEL
#define HASKELL_TCP_CLIENT_CHANNEL 2
#endif

static unsigned call(unsigned label)
{
    microkit_msginfo reply = microkit_ppcall(HASKELL_TCP_CLIENT_CHANNEL,
                                             microkit_msginfo_new(label, 0));
    return microkit_msginfo_get_label(reply);
}

int sel4_haskell_dns_start(const uint8_t *name, int length)
{
    if (haskell_tcp_page == 0) return -2;
    if (name == 0 || length <= 0 || length >= HASKELL_TCP_MAX) return -1;
    volatile struct haskell_tcp_page *io =
        (volatile struct haskell_tcp_page *)haskell_tcp_page;
    if (io->magic != HASKELL_TCP_MAGIC) return 0;
    io->request_len = (uint32_t)length;
    for (int i = 0; i < length; ++i) io->request[i] = name[i];
    unsigned result = call(HASKELL_DNS_START);
    if (result == HASKELL_TCP_ACCEPTED) return 1;
    /* A lookup already in flight should be waited out, not failed. */
    if (result == HASKELL_TCP_NOT_READY || result == HASKELL_TCP_BUSY) return 0;
    return -1;
}

int sel4_haskell_dns_poll(uint8_t *address)
{
    if (haskell_tcp_page == 0 || address == 0) return -1;
    unsigned result = call(HASKELL_DNS_POLL);
    if (result == HASKELL_TCP_PENDING) return 0;
    if (result != HASKELL_TCP_DATA) return -1;
    volatile struct haskell_tcp_page *io =
        (volatile struct haskell_tcp_page *)haskell_tcp_page;
    if (io->response_len != 4) return -1;
    for (unsigned i = 0; i < 4; ++i) address[i] = io->response[i];
    return 4;
}

static volatile struct haskell_tcp_page *select_slot(uint8_t slot)
{
    volatile struct haskell_tcp_page *io =
        (volatile struct haskell_tcp_page *)haskell_tcp_page;
    io->reserved = slot;
    return io;
}

int sel4_haskell_tcp_open(uint8_t a, uint8_t b, uint8_t c, uint8_t d,
                          uint16_t port, uint8_t slot)
{
    if (haskell_tcp_page == 0) return -2;
    volatile struct haskell_tcp_page *io = select_slot(slot);
    if (io->magic != HASKELL_TCP_MAGIC) return 0;
    io->dest_ip[0] = a;
    io->dest_ip[1] = b;
    io->dest_ip[2] = c;
    io->dest_ip[3] = d;
    io->dest_port = port;
    unsigned result = call(HASKELL_TCP_OPEN);
    if (result == HASKELL_TCP_ACCEPTED) return 1;
    if (result == HASKELL_TCP_NOT_READY) return 0;
    if (result == HASKELL_TCP_BUSY) return 2;
    return -1;
}

int sel4_haskell_tcp_status(uint8_t slot)
{
    if (haskell_tcp_page == 0) return -1;
    (void)select_slot(slot);
    unsigned result = call(HASKELL_TCP_STATUS);
    if (result == HASKELL_TCP_CONNECTED) return 1;
    if (result == HASKELL_TCP_PENDING) return 0;
    if (result == HASKELL_TCP_CLOSED) return -2;
    return -1;
}

int sel4_haskell_tcp_send(const uint8_t *data, int length, uint8_t slot)
{
    if (haskell_tcp_page == 0 || data == 0 || length <= 0 ||
        length > HASKELL_TCP_MAX) return -1;
    volatile struct haskell_tcp_page *io = select_slot(slot);
    io->request_len = (uint32_t)length;
    for (int i = 0; i < length; ++i) io->request[i] = data[i];
    unsigned result = call(HASKELL_TCP_SEND);
    if (result == HASKELL_TCP_ACCEPTED) return length;
    if (result == HASKELL_TCP_BUSY) return 0;
    return -1;
}

int sel4_haskell_tcp_recv(uint8_t *out, int capacity, uint8_t slot)
{
    if (haskell_tcp_page == 0 || out == 0 || capacity <= 0 ||
        capacity > HASKELL_TCP_MAX) return -1;
    volatile struct haskell_tcp_page *io = select_slot(slot);
    io->request_len = (uint32_t)capacity;
    unsigned result = call(HASKELL_TCP_RECV);
    if (result == HASKELL_TCP_PENDING) return 0;
    if (result == HASKELL_TCP_CLOSED) return -2;
    if (result != HASKELL_TCP_DATA) return -1;
    uint32_t length = io->response_len;
    if (length == 0 || length > (uint32_t)capacity) return -1;
    for (uint32_t i = 0; i < length; ++i) out[i] = io->response[i];
    return (int)length;
}

int sel4_haskell_tcp_close(uint8_t slot)
{
    if (haskell_tcp_page == 0) return -1;
    (void)select_slot(slot);
    unsigned result = call(HASKELL_TCP_CLOSE);
    if (result == HASKELL_TCP_CLOSED) return 1;
    if (result == HASKELL_TCP_BUSY) return 0;
    return -1;
}

void sel4_haskell_tcp_cancel(uint8_t slot)
{
    if (haskell_tcp_page != 0) {
        (void)select_slot(slot);
        (void)call(HASKELL_TCP_CANCEL);
    }
}
