/* Exercise the callback and the 128-byte wire protocol. lwIP supplies
 * pbuf_copy_partial. The PCB and synthetic pbufs stay under test control. */
#include <assert.h>
#define pbuf_free record_pbuf_free
#include "../haskell_tcp_service.c"

uintptr_t haskell_tcp_page;
static uint32_t frees, credited;
static struct tcp_pcb pcb;
static struct haskell_tcp_page wire[HASKELL_TCP_SESSIONS];
static uint8_t bytes[UINT16_MAX];
static uint8_t prefix[4096];

u8_t record_pbuf_free(struct pbuf *p) { assert(p != NULL); ++frees; return 1; }
void tcp_arg(struct tcp_pcb *p, void *arg) { (void)p; (void)arg; }
void tcp_recv(struct tcp_pcb *p, tcp_recv_fn cb) { (void)p; (void)cb; }
void tcp_err(struct tcp_pcb *p, tcp_err_fn cb) { (void)p; (void)cb; }
void tcp_recved(struct tcp_pcb *p, u16_t length) { assert(p == &pcb); credited += length; }
err_t tcp_close(struct tcp_pcb *p) { assert(p == &pcb); return ERR_OK; }
void tcp_abort(struct tcp_pcb *p) { assert(p == &pcb); }

static void drain(struct haskell_tcp_page *io, const uint8_t *expected, size_t size)
{
    while (size > 0) {
        size_t n = size < HASKELL_TCP_MAX ? size : HASKELL_TCP_MAX;
        io->request_len = n;
        assert(haskell_tcp_recv() == HASKELL_TCP_DATA);
        assert(io->response_len == n && memcmp(io->response, expected, n) == 0);
        expected += n;
        size -= n;
    }
}

static void check_recv(uint32_t slot)
{
    const bool local = slot == HASKELL_TCP_WALLET_SLOT || slot == HASKELL_TCP_WORKER_SLOT;
    void *arg = (void *)(uintptr_t)slot;
    haskell_tcp_select(slot, local);
    struct haskell_tcp_page *io = local ? &wire[slot] : &wire[0];
    struct tcp_session *s = &sessions[slot];
    s->state = CONNECTED;
    s->pcb = &pcb;
    struct pbuf tail = { .payload = bytes + 16384, .len = UINT16_MAX - 16384, .tot_len = UINT16_MAX - 16384 };
    struct pbuf chain = { .payload = bytes, .len = 16384, .tot_len = UINT16_MAX, .next = &tail };
    struct pbuf first = { .payload = prefix, .len = sizeof(prefix), .tot_len = sizeof(prefix) };

    frees = credited = 0;
    assert(receive(arg, &pcb, &first, ERR_OK) == ERR_OK);
    assert(receive(arg, &pcb, &chain, ERR_OK) == ERR_MEM);
    assert(frees == 1 && credited == 0 && s->rx_length == sizeof(prefix));
    drain(io, prefix, sizeof(prefix));
    assert(credited == sizeof(prefix));
    assert(receive(arg, &pcb, &chain, ERR_OK) == ERR_OK);
    assert(receive(arg, &pcb, &first, ERR_OK) == ERR_MEM);
    assert(frees == 2 && credited == sizeof(prefix));

    /* Retry the refused chain as soon as enough room becomes available. */
    drain(io, bytes, sizeof(prefix));
    assert(receive(arg, &pcb, &first, ERR_OK) == ERR_OK);
    assert(frees == 3 && credited == 2 * sizeof(prefix));
    /* FIN keeps the queued bytes readable in arrival order. */
    assert(receive(arg, &pcb, NULL, ERR_OK) == ERR_OK);
    drain(io, bytes + sizeof(prefix), sizeof(bytes) - sizeof(prefix));
    drain(io, prefix, sizeof(prefix));
    assert(haskell_tcp_recv() == HASKELL_TCP_CLOSED);
    assert(credited == 2 * sizeof(prefix) && s->pcb == NULL && s->rx_length == 0);

    s->state = CONNECTED;
    s->pcb = &pcb;
    assert(receive(arg, &pcb, &first, ERR_OK) == ERR_OK);
    assert(haskell_tcp_cancel() == HASKELL_TCP_CLOSED && s->rx_length == 0);
    assert(haskell_tcp_recv() == HASKELL_TCP_CLOSED);
    s->state = CONNECTED;
    s->pcb = &pcb;
    assert(receive(arg, &pcb, &first, ERR_OK) == ERR_OK);
    connection_error(arg, ERR_RST);
    assert(haskell_tcp_recv() == HASKELL_TCP_ERROR && s->pcb == NULL && s->rx_length == 0);

    s->state = CONNECTED;
    s->pcb = &pcb;
    assert(receive(arg, &pcb, &first, ERR_OK) == ERR_OK);
    drain(io, prefix, 7);
    assert(haskell_tcp_close() == HASKELL_TCP_CLOSED);
    assert(haskell_tcp_recv() == HASKELL_TCP_CLOSED && s->rx_length == 0);
}

#ifdef NEXAWAL_UTM_WIDE_RX
static void check_wallet_wide_recv(void)
{
    static union {
        struct haskell_tcp_page slots[HASKELL_TCP_SESSIONS];
        uint8_t bytes[4096];
    } mapping;
    memset(&mapping, 0xbd, sizeof(mapping));
    haskell_tcp_page = (uintptr_t)&mapping;
    haskell_tcp_select(HASKELL_TCP_WALLET_SLOT, true);
    struct tcp_session *session = &sessions[HASKELL_TCP_WALLET_SLOT];
    session->state = CONNECTED;
    session->pcb = &pcb;
    struct pbuf packet = { .payload = bytes, .len = 2300, .tot_len = 2300 };
    void *arg = (void *)(uintptr_t)HASKELL_TCP_WALLET_SLOT;
    struct haskell_tcp_page *io = &mapping.slots[HASKELL_TCP_WALLET_SLOT];
#ifdef NEXAWAL_UTM_RX_NOTIFY
    io->request_len = 1024;
    assert(haskell_tcp_recv_wide_wait(true) == HASKELL_TCP_PENDING);
    assert(!haskell_tcp_take_wallet_rx_wake());
    /* Arrival between the empty read and the next wait must wake exactly once. */
    assert(receive(arg, &pcb, &packet, ERR_OK) == ERR_OK);
    assert(haskell_tcp_take_wallet_rx_wake());
    assert(!haskell_tcp_take_wallet_rx_wake());
    assert(haskell_tcp_cancel() == HASKELL_TCP_CLOSED);
    session->state = CONNECTED;
    session->pcb = &pcb;
    assert(haskell_tcp_recv_wide_wait(true) == HASKELL_TCP_PENDING);
    assert(haskell_tcp_cancel() == HASKELL_TCP_CLOSED);
    assert(!haskell_tcp_take_wallet_rx_wake());
    session->state = CONNECTED;
    session->pcb = &pcb;
    assert(haskell_tcp_recv_wide_wait(true) == HASKELL_TCP_PENDING);
    assert(receive(arg, &pcb, NULL, ERR_OK) == ERR_OK);
    assert(haskell_tcp_take_wallet_rx_wake());
    assert(!haskell_tcp_take_wallet_rx_wake());
    session->state = CONNECTED;
    session->pcb = &pcb;
    assert(haskell_tcp_recv_wide_wait(true) == HASKELL_TCP_PENDING);
    assert(haskell_tcp_recv_wide_wait(false) == HASKELL_TCP_PENDING);
    assert(!haskell_tcp_take_wallet_rx_wake());
#endif
#ifdef NEXAWAL_UTM_RX_AHEAD
    const uint32_t accept_credit = packet.tot_len;
    const uint32_t read_credit = packet.tot_len;
#else
    const uint32_t accept_credit = 0;
    const uint32_t read_credit = 1152;
#endif
    credited = 0;
    assert(receive(arg, &pcb, &packet, ERR_OK) == ERR_OK);
#ifdef NEXAWAL_UTM_RX_NOTIFY
    assert(!haskell_tcp_take_wallet_rx_wake());
#endif
    io->request_len = 1024;
    assert(haskell_tcp_recv() == HASKELL_TCP_ERROR);
    haskell_tcp_select(HASKELL_TCP_WALLET_SLOT, false);
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_ERROR);
    haskell_tcp_select(HASKELL_TCP_WORKER_SLOT, true);
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_ERROR);
    haskell_tcp_select(HASKELL_TCP_WALLET_SLOT, true);
    io->request_len = 2049;
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_ERROR && credited == accept_credit);
    io->request_len = 1024;
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_DATA && io->response_len == 1024);
    assert(memcmp(mapping.bytes + 2048, bytes, 1024) == 0);
    assert(mapping.bytes[2047] == 0xbd && mapping.bytes[3072] == 0xbd);
    assert(io->response[0] == 0xbd);
    io->request_len = 128;
    assert(haskell_tcp_recv() == HASKELL_TCP_DATA && io->response_len == 128);
    assert(memcmp(io->response, bytes + 1024, 128) == 0 && credited == read_credit);
    assert(receive(arg, &pcb, NULL, ERR_OK) == ERR_OK);
    io->request_len = 1024;
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_DATA && io->response_len == 1024);
    assert(memcmp(mapping.bytes + 2048, bytes + 1152, 1024) == 0);
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_DATA && io->response_len == 124);
    assert(memcmp(mapping.bytes + 2048, bytes + 2176, 124) == 0);
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_CLOSED && credited == read_credit);
}

#ifndef NEXAWAL_UTM_RX_AHEAD
static void check_wallet_wide_refill(void)
{
    static struct {
        union {
            struct haskell_tcp_page slots[HASKELL_TCP_SESSIONS];
            uint8_t bytes[4096];
        } page;
        uint8_t guard;
    } mapping;
    const uint32_t slot = HASKELL_TCP_WALLET_SLOT;
    void *arg = (void *)(uintptr_t)slot;
    struct tcp_session *s = &sessions[slot];
    struct haskell_tcp_page *io = &mapping.page.slots[slot];
    uint8_t *response = mapping.page.bytes + WALLET_WIDE_RX_OFFSET;
    haskell_tcp_page = (uintptr_t)&mapping.page;
    haskell_tcp_select(slot, true);
    assert(haskell_tcp_cancel() == HASKELL_TCP_CLOSED);
    s->state = CONNECTED;
    s->pcb = &pcb;
    memset(&mapping, 0xbd, sizeof(mapping));

    /* The refill starts 513 bytes before the ring end, inside the first
     * pbuf. Both copying the chain in and reading it out must cross the end. */
    const uint16_t initial_len = UINT16_MAX - 513;
    struct pbuf initial = { .payload = bytes, .len = initial_len, .tot_len = initial_len };
    struct pbuf tail = { .payload = prefix + 700, .len = 1348, .tot_len = 1348 };
    struct pbuf refill = { .payload = prefix, .len = 700, .tot_len = 2048, .next = &tail };
    frees = credited = 0;
    assert(receive(arg, &pcb, &initial, ERR_OK) == ERR_OK);
    assert(receive(arg, &pcb, &refill, ERR_OK) == ERR_MEM);
    io->request_len = 1024;
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_DATA && io->response_len == 1024);
    assert(memcmp(response, bytes, 1024) == 0 && response[1024] == 0xbd);
    /* Delivering a prefix must leave the whole unread suffix in place. */
    assert(memcmp(s->rx + 1024, bytes + 1024, initial_len - 1024) == 0);
    assert(receive(arg, &pcb, &refill, ERR_OK) == ERR_MEM);
    assert(frees == 1 && credited == 1024);
    drain(io, bytes + 1024, 1024);
    assert(receive(arg, &pcb, &refill, ERR_OK) == ERR_OK);
    assert(frees == 2 && credited == 2048);
    drain(io, bytes + 2048, initial_len - 2048);
    assert(credited == initial_len);

    assert(receive(arg, &pcb, NULL, ERR_OK) == ERR_OK);
    io->request_len = 2049;
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_ERROR && io->response_len == 0);
    io->request_len = 2048;
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_DATA && io->response_len == 2048);
    assert(memcmp(response, prefix, 2048) == 0 && mapping.guard == 0xbd);
    assert(mapping.page.bytes[WALLET_WIDE_RX_OFFSET - 1] == 0xbd);
    assert(haskell_tcp_recv_wide() == HASKELL_TCP_CLOSED && credited == initial_len);
}
#else
static void check_wallet_receive_ahead(void)
{
    const uint32_t slot = HASKELL_TCP_WALLET_SLOT;
    void *arg = (void *)(uintptr_t)slot;
    struct tcp_session *s = &sessions[slot];
    struct haskell_tcp_page *io = &wire[slot];
    haskell_tcp_page = (uintptr_t)wire;
    haskell_tcp_select(slot, true);
    assert(haskell_tcp_cancel() == HASKELL_TCP_CLOSED);
    s->state = CONNECTED;
    s->pcb = &pcb;
    frees = credited = 0;

    /* A full maximum-size chain follows the initial transfer. Only the
     * first 513 bytes of that chain may receive further early credit. */
    const uint16_t first_len = TCP_WND - 513;
    struct pbuf initial = { .payload = bytes, .len = first_len, .tot_len = first_len };
    struct pbuf tail = { .payload = bytes + 4096, .len = UINT16_MAX - 4096, .tot_len = UINT16_MAX - 4096 };
    struct pbuf maximum = { .payload = bytes, .len = 4096, .tot_len = UINT16_MAX, .next = &tail };
    struct pbuf last = { .payload = prefix, .len = 2048, .tot_len = 2048 };
    assert(receive(arg, &pcb, &initial, ERR_OK) == ERR_OK);
    assert(credited == first_len);
    assert(receive(arg, &pcb, &maximum, ERR_OK) == ERR_OK);
    assert(credited == TCP_WND && s->rx_length == first_len + UINT16_MAX);
    assert(receive(arg, &pcb, &last, ERR_OK) == ERR_MEM);
    assert(frees == 2 && credited == TCP_WND);
    drain(io, bytes, 1024);
    assert(receive(arg, &pcb, &last, ERR_OK) == ERR_MEM);
    assert(frees == 2 && credited == TCP_WND + 1024);
    drain(io, bytes + 1024, 1024);
    assert(receive(arg, &pcb, &last, ERR_OK) == ERR_OK);
    assert(frees == 3 && credited == TCP_WND + 2048);
    drain(io, bytes + 2048, first_len - 2048);
    assert(credited == TCP_WND + first_len);
    drain(io, bytes, UINT16_MAX);
    const uint32_t total = first_len + UINT16_MAX + 2048;
    assert(credited == total && s->rx_length == 2048);
    drain(io, prefix, 2048);
    assert(credited == total && haskell_tcp_recv() == HASKELL_TCP_PENDING);

    /* Finishing with withheld data releases the PCB. The reader must still
     * drain bytes after FIN, without crediting a freed or replacement PCB. */
    for (unsigned end = 0; end < 4; ++end) {
        s->state = CONNECTED;
        s->pcb = &pcb;
        credited = 0;
        assert(receive(arg, &pcb, &maximum, ERR_OK) == ERR_OK);
        assert(credited == TCP_WND);
        drain(io, bytes, 17);
        assert(credited == TCP_WND + 17);
        if (end == 0) {
            assert(receive(arg, &pcb, NULL, ERR_OK) == ERR_OK);
            drain(io, bytes + 17, UINT16_MAX - 17);
        } else if (end == 1) {
            assert(haskell_tcp_close() == HASKELL_TCP_CLOSED);
        } else if (end == 2) {
            assert(haskell_tcp_cancel() == HASKELL_TCP_CLOSED);
        } else {
            connection_error(arg, ERR_RST);
        }
        assert(s->pcb == NULL && s->rx_length == 0 && credited == TCP_WND + 17);
    }
}
#endif

#endif

int main(void)
{
    haskell_tcp_page = (uintptr_t)wire;
    for (uint32_t i = 0; i < sizeof(bytes); ++i) bytes[i] = (uint8_t)(i * 31 + (i >> 8));
    for (uint32_t i = 0; i < sizeof(prefix); ++i) prefix[i] = (uint8_t)(i ^ 0xab);
    for (uint32_t slot = 0; slot < HASKELL_TCP_SESSIONS; ++slot) {
#ifdef NEXAWAL_UTM_RX_AHEAD
        if (slot == HASKELL_TCP_WALLET_SLOT) continue;
#endif
        check_recv(slot);
    }
#ifdef NEXAWAL_UTM_WIDE_RX
    check_wallet_wide_recv();
#ifdef NEXAWAL_UTM_RX_AHEAD
    check_wallet_receive_ahead();
#else
    check_wallet_wide_refill();
#endif
#endif
    return 0;
}
