/* One bounded TCP session behind the native Haskell network capability.
 * Haskell maps only a request/response page and holds a protected endpoint;
 * this PD alone owns lwIP and the VirtIO Ethernet device.
 */
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <os/sddf.h>
#include <sddf/util/printf.h>

#include "dhcp_client.h"
#include "dns_slot_state.h"
#include "lwip/err.h"
#include "lwip/dns.h"
#include "lwip/ip_addr.h"
#include "lwip/pbuf.h"
#include "lwip/tcp.h"

/* lwIP retries a refused chain whole. An empty session must fit any chain
 * the raw TCP callback can deliver, including coalesced out-of-order segments. */
#define RX_CAPACITY UINT16_MAX
#ifdef NEXAWAL_UTM_RX_AHEAD
#ifndef NEXAWAL_UTM_WIDE_RX
#error "NEXAWAL_UTM_RX_AHEAD requires NEXAWAL_UTM_WIDE_RX"
#endif
_Static_assert(TCP_WND <= UINT16_MAX, "receive-ahead is limited to one unscaled window");
#endif
_Static_assert(sizeof(((struct pbuf *)0)->tot_len) == sizeof(uint16_t),
               "receive storage must cover the lwIP pbuf length");

enum session_state { IDLE, CONNECTING, CONNECTED, PEER_CLOSED, FAILED };
struct tcp_session {
    enum session_state state;
    struct tcp_pcb *pcb;
#ifndef NEXAWAL_UTM_RX_AHEAD
    uint8_t rx[RX_CAPACITY];
#endif
    uint32_t rx_length;
#ifdef NEXAWAL_UTM_RX_NOTIFY
    bool rx_waiting;
#endif
#ifdef NEXAWAL_UTM_WIDE_RX
    uint32_t rx_head;
#endif
};
static struct tcp_session sessions[HASKELL_TCP_SESSIONS];
#ifdef NEXAWAL_UTM_RX_AHEAD
/* One additional TCP window belongs to the wallet. Each other slot keeps
 * its original ring and returns credit only when its client reads. */
static uint8_t rx_buffers[HASKELL_TCP_SESSIONS * RX_CAPACITY + TCP_WND];
#endif

static inline uint32_t ahead_limit(struct tcp_session *session)
{
#ifdef NEXAWAL_UTM_RX_AHEAD
    return session == &sessions[HASKELL_TCP_WALLET_SLOT] ? TCP_WND : 0;
#else
    (void)session;
    return 0;
#endif
}

static inline uint8_t *rx_buffer(struct tcp_session *session)
{
#ifdef NEXAWAL_UTM_RX_AHEAD
    size_t slot = session - sessions;
    return rx_buffers + slot * RX_CAPACITY + (slot > HASKELL_TCP_WALLET_SLOT ? TCP_WND : 0);
#else
    return session->rx;
#endif
}
static bool network_ready;
static struct dns_slot_table dns_slots;

/* Wallet/worker use per-slot wire pages; agent/shell share page 0 + reserved. */
static uint32_t active_slot;
static bool wire_is_slot_local;

static volatile struct haskell_tcp_page *pages_base(void)
{
    return (volatile struct haskell_tcp_page *)haskell_tcp_page;
}

static volatile struct haskell_tcp_page *page(void)
{
    if (wire_is_slot_local)
        return pages_base() + active_slot;
    return pages_base();
}

void haskell_tcp_select(uint32_t slot, bool slot_local_wire)
{
    active_slot = slot;
    wire_is_slot_local = slot_local_wire;
}

void haskell_tcp_network_ready(void)
{
    network_ready = true;
}

void haskell_dns_configure_server(void)
{
    ip_addr_t server;
#if DNS_SERVER_PORT == 53
    /* QEMU user networking answers DNS here, inside the emulator. */
    IP_ADDR4(&server, 10, 0, 2, 3);
#else
    IP_ADDR4(&server, 10, 0, 2, 2);
#endif
    dns_setserver(0, &server);
}

static void dns_found(const char *name, const ip_addr_t *addr, void *arg)
{
    (void)name;
    uintptr_t encoded = (uintptr_t)arg;
    if (encoded == 0 || encoded > DNS_SLOT_COUNT) return;
    uint32_t slot = (uint32_t)(encoded - 1);
    if (addr == NULL || !IP_IS_V4(addr)) {
        dns_slot_complete(&dns_slots, slot, NULL);
        return;
    }
    const ip4_addr_t *v4 = ip_2_ip4(addr);
    uint8_t address[4] = { ip4_addr1(v4), ip4_addr2(v4), ip4_addr3(v4), ip4_addr4(v4) };
    dns_slot_complete(&dns_slots, slot, address);
    sddf_printf("HASKELL_DNS|answer %u.%u.%u.%u\n",
                address[0], address[1], address[2], address[3]);
}

extern void sddf_lwip_process_timeout(void);
extern void sddf_lwip_process_rx(void);

unsigned haskell_dns_start(void)
{
    /* The agent polls this before DHCP has an address. Step lwIP here so
     * Discover/Offer still run; do not do that once a lookup is in flight. */
    if (!network_ready) {
        sddf_lwip_process_timeout();
        sddf_lwip_process_rx();
        return HASKELL_TCP_NOT_READY;
    }
    /* lwIP fails a query immediately when the resolver is still 0.0.0.0.
     * That must stay retryable until configure_server has run. */
    if (ip_addr_isany(dns_getserver(0))) {
        sddf_lwip_process_timeout();
        sddf_lwip_process_rx();
        return HASKELL_TCP_NOT_READY;
    }
    if (active_slot >= DNS_SLOT_COUNT) return HASKELL_TCP_ERROR;
    struct dns_slot *lookup = &dns_slots.slots[active_slot];
    if (lookup->state == DNS_SLOT_PENDING) return HASKELL_TCP_BUSY;
    volatile struct haskell_tcp_page *io = page();
    uint32_t length = io->request_len;
    if (length == 0 || length >= HASKELL_TCP_MAX) return HASKELL_TCP_ERROR;
    for (uint32_t i = 0; i < length; ++i) {
        uint8_t ch = io->request[i];
        if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
              (ch >= '0' && ch <= '9') || ch == '-' || ch == '.'))
            return HASKELL_TCP_ERROR;
        lookup->name[i] = (char)ch;
    }
    lookup->name[length] = '\0';
    if (!dns_slot_begin(&dns_slots, active_slot)) return HASKELL_TCP_BUSY;
    ip_addr_t address;
    void *callback_arg = (void *)(uintptr_t)(active_slot + 1);
    sddf_printf("HASKELL_DNS|query %s via %s\n", lookup->name, ipaddr_ntoa(dns_getserver(0)));
    err_t result = dns_gethostbyname(lookup->name, &address, dns_found, callback_arg);
    /* A missing resolver calls the callback with NULL and still returns ERR_OK. */
    if (lookup->state == DNS_SLOT_FAILED) return HASKELL_TCP_NOT_READY;
    if (result == ERR_OK) dns_found(lookup->name, &address, callback_arg);
    else if (result != ERR_INPROGRESS) {
        sddf_printf("HASKELL_DNS|lookup failed %s name=%s\n", lwip_strerr(result), lookup->name);
        dns_slot_complete(&dns_slots, active_slot, NULL);
    }
    return lookup->state == DNS_SLOT_FAILED ? HASKELL_TCP_ERROR : HASKELL_TCP_ACCEPTED;
}

unsigned haskell_dns_poll(void)
{
    if (active_slot >= DNS_SLOT_COUNT) return HASKELL_TCP_ERROR;
    const struct dns_slot *lookup = &dns_slots.slots[active_slot];
    if (lookup->state == DNS_SLOT_PENDING) return HASKELL_TCP_PENDING;
    if (lookup->state != DNS_SLOT_DONE) return HASKELL_TCP_ERROR;
    volatile struct haskell_tcp_page *io = page();
    io->response_len = 4;
    for (unsigned i = 0; i < 4; ++i) io->response[i] = lookup->address[i];
    return HASKELL_TCP_DATA;
}

static struct tcp_session *selected_session(void)
{
    if (active_slot >= HASKELL_TCP_SESSIONS) return NULL;
    return &sessions[active_slot];
}

static struct tcp_session *session_from_arg(void *arg)
{
    uintptr_t index = (uintptr_t)arg;
    if (index >= HASKELL_TCP_SESSIONS) return NULL;
    return &sessions[index];
}

static err_t release_pcb(struct tcp_session *session, struct tcp_pcb *pcb)
{
    session->pcb = NULL;
    tcp_arg(pcb, NULL);
    tcp_recv(pcb, NULL);
    tcp_err(pcb, NULL);
    if (tcp_close(pcb) == ERR_OK) return ERR_OK;
    tcp_abort(pcb);
    return ERR_ABRT;
}

static err_t fail_pcb(struct tcp_session *session, struct tcp_pcb *pcb)
{
    session->state = FAILED;
    session->rx_length = 0;
    return release_pcb(session, pcb);
}

static err_t receive(void *arg, struct tcp_pcb *pcb, struct pbuf *p, err_t err)
{
    struct tcp_session *session = session_from_arg(arg);
    if (session == NULL) {
        if (p != NULL) pbuf_free(p);
        return ERR_ARG;
    }
    if (p == NULL) {
        sddf_dprintf("HASKELL_TCP|peer closed connection\n");
        session->state = PEER_CLOSED;
        return release_pcb(session, pcb);
    }
    if (err != ERR_OK || session->state != CONNECTED) {
        sddf_dprintf("HASKELL_TCP|receive callback error: %s state=%u\n",
                     lwip_strerr(err), (unsigned)session->state);
        pbuf_free(p);
        return fail_pcb(session, pcb);
    }
    const uint32_t ahead = ahead_limit(session);
    const uint32_t capacity = RX_CAPACITY + ahead;
    if (p->tot_len > capacity - session->rx_length) {
        /* lwIP retains a refused pbuf and retries after the client drains data. */
        return ERR_MEM;
    }
    uint16_t received = p->tot_len;
#ifdef NEXAWAL_UTM_WIDE_RX
    /* An empty queue starts at zero, including after close, reset or reuse. */
    if (session->rx_length == 0) session->rx_head = 0;
    uint32_t tail = session->rx_head + session->rx_length;
    if (tail >= capacity) tail -= capacity;
    uint16_t first = received < capacity - tail ? received : capacity - tail;
    uint8_t *rx = rx_buffer(session);
    uint16_t copied = pbuf_copy_partial(p, rx + tail, first, 0);
    if (copied == first && first < received)
        copied += pbuf_copy_partial(p, rx, received - first, first);
#else
    uint16_t copied = pbuf_copy_partial(p, session->rx + session->rx_length, received, 0);
#endif
    pbuf_free(p);
    if (copied != received) return fail_pcb(session, pcb);
    session->rx_length += copied;
#ifdef NEXAWAL_UTM_RX_AHEAD
    /* Credit only bytes already copied into the window reserved for read-ahead.
     * Cumulative credit = client bytes read + min(queued bytes, ahead). */
    uint32_t before = session->rx_length - copied;
    if (before < ahead && copied != 0) {
        uint16_t credit = copied < ahead - before ? copied : ahead - before;
        tcp_recved(pcb, credit);
    }
#endif
    return ERR_OK;
}

static void connection_error(void *arg, err_t err)
{
    struct tcp_session *session = session_from_arg(arg);
    sddf_printf("HASKELL_TCP|connection error: %s slot=%u\n", lwip_strerr(err),
                session ? (unsigned)(session - sessions) : 99u);
    if (session == NULL) return;
    /* lwIP has already freed the pcb before this callback. */
    session->pcb = NULL;
    session->rx_length = 0;
    session->state = FAILED;
}

static err_t connected(void *arg, struct tcp_pcb *pcb, err_t err)
{
    struct tcp_session *session = session_from_arg(arg);
    if (session == NULL) return ERR_ARG;
    if (err != ERR_OK) {
        sddf_printf("HASKELL_TCP|connected err %s\n", lwip_strerr(err));
        return fail_pcb(session, pcb);
    }
    sddf_printf("HASKELL_TCP|connected slot=%u\n",
                (unsigned)(session - sessions));
    session->state = CONNECTED;
    return ERR_OK;
}

unsigned haskell_tcp_open(void)
{
    volatile struct haskell_tcp_page *io = page();
    struct tcp_session *session = selected_session();
    if (session == NULL) {
        sddf_printf("HASKELL_TCP|open bad session slot=%u\n", (unsigned)active_slot);
        return HASKELL_TCP_ERROR;
    }
    if (io->magic != HASKELL_TCP_MAGIC) {
        sddf_printf("HASKELL_TCP|open bad magic slot=%u mag=0x%x local=%u\n",
                    (unsigned)active_slot, (unsigned)io->magic,
                    (unsigned)wire_is_slot_local);
        return HASKELL_TCP_ERROR;
    }
    if (!network_ready) return HASKELL_TCP_NOT_READY;
    if (session->state == CONNECTING || session->state == CONNECTED) return HASKELL_TCP_BUSY;
    if (session->state == PEER_CLOSED && session->rx_length != 0) return HASKELL_TCP_BUSY;
    uint16_t port = io->dest_port;
    uint8_t addr[4];
    for (unsigned i = 0; i < 4; ++i) addr[i] = io->dest_ip[i];
    if (port == 0 || (addr[0] | addr[1] | addr[2] | addr[3]) == 0) {
        sddf_printf("HASKELL_TCP|open bad dest slot=%u port=%u ip=%u.%u.%u.%u\n",
                    (unsigned)active_slot, (unsigned)port, addr[0], addr[1],
                    addr[2], addr[3]);
        return HASKELL_TCP_ERROR;
    }

    struct tcp_pcb *pcb = tcp_new_ip_type(IPADDR_TYPE_V4);
    if (pcb == NULL) {
        sddf_printf("HASKELL_TCP|open tcp_new fail slot=%u\n",
                    (unsigned)active_slot);
        return HASKELL_TCP_ERROR;
    }
#ifdef NEXAWAL_UTM_RX_NOTIFY
    session->rx_waiting = false;
#endif
    session->rx_length = 0;
    session->pcb = pcb;
    session->state = CONNECTING;
    tcp_arg(pcb, (void *)(uintptr_t)(session - sessions));
    tcp_recv(pcb, receive);
    tcp_err(pcb, connection_error);
    /* r14/r15: qemu usernet showed DNS UDP and Cake TCP both on guest
     * ephemeral 49153 (TCP[SYN_RCVD] stuck, then Connection aborted). Slirp
     * NAT keys by guest port — bind wallet TCP away from DNS's range. */
    {
        static uint16_t ephemeral = 42000;
        uint16_t local = ephemeral;
        ephemeral = (uint16_t)(42000 + ((ephemeral - 42000 + 1) % 2000));
        err_t berr = tcp_bind(pcb, IP_ANY_TYPE, local);
        if (berr != ERR_OK) {
            sddf_printf("HASKELL_TCP|tcp_bind %u failed %s\n",
                        (unsigned)local, lwip_strerr(berr));
            (void)fail_pcb(session, pcb);
            return HASKELL_TCP_ERROR;
        }
        sddf_printf("HASKELL_TCP|bind local=%u slot=%u\n", (unsigned)local,
                    (unsigned)(session - sessions));
    }
    ip_addr_t dest;
    IP_ADDR4(&dest, addr[0], addr[1], addr[2], addr[3]);
    sddf_printf("HASKELL_TCP|open slot=%u %u.%u.%u.%u:%u\n",
                (unsigned)(session - sessions), addr[0], addr[1], addr[2],
                addr[3], (unsigned)port);
    err_t cerr = tcp_connect(pcb, &dest, port, connected);
    if (cerr != ERR_OK) {
        sddf_printf("HASKELL_TCP|tcp_connect failed %s\n", lwip_strerr(cerr));
        (void)fail_pcb(session, pcb);
        return HASKELL_TCP_ERROR;
    }
    return HASKELL_TCP_ACCEPTED;
}

unsigned haskell_tcp_status(void)
{
    struct tcp_session *session = selected_session();
    if (session == NULL) return HASKELL_TCP_ERROR;
    if (session->state == CONNECTING) return HASKELL_TCP_PENDING;
    if (session->state == CONNECTED) return HASKELL_TCP_CONNECTED;
    if (session->state == PEER_CLOSED || session->state == IDLE) return HASKELL_TCP_CLOSED;
    return HASKELL_TCP_ERROR;
}

unsigned haskell_tcp_send(void)
{
    struct tcp_session *session = selected_session();
    if (session == NULL || session->state != CONNECTED || session->pcb == NULL)
        return HASKELL_TCP_ERROR;
    volatile struct haskell_tcp_page *io = page();
    uint32_t length = io->request_len;
    if (length == 0 || length > HASKELL_TCP_MAX) return HASKELL_TCP_ERROR;
    uint8_t snapshot[HASKELL_TCP_MAX];
    for (uint32_t i = 0; i < length; ++i) snapshot[i] = io->request[i];
    err_t err = tcp_write(session->pcb, snapshot, length, TCP_WRITE_FLAG_COPY);
    if (err == ERR_MEM) return HASKELL_TCP_BUSY;
    if (err != ERR_OK) return HASKELL_TCP_ERROR;
    /* Once tcp_write copies the bytes they are accepted, even if the immediate
     * output attempt cannot transmit them yet. lwIP will retry queued data. */
    (void)tcp_output(session->pcb);
    return HASKELL_TCP_ACCEPTED;
}

#ifdef NEXAWAL_UTM_WIDE_RX
#define RX_LIMIT max
static unsigned recv_into(volatile uint8_t *dst, uint32_t max)
#else
#define RX_LIMIT HASKELL_TCP_MAX
unsigned haskell_tcp_recv(void)
#endif
{
    struct tcp_session *session = selected_session();
    volatile struct haskell_tcp_page *io = page();
    uint32_t capacity = io->request_len;
    io->response_len = 0;
    if (session == NULL || capacity == 0 || capacity > RX_LIMIT)
        return HASKELL_TCP_ERROR;
#ifdef NEXAWAL_UTM_RX_NOTIFY
    session->rx_waiting = false;
#endif
    if (session->rx_length != 0) {
        uint32_t length = session->rx_length < capacity ? session->rx_length : capacity;
#ifdef NEXAWAL_UTM_WIDE_RX
        uint32_t ring_size = RX_CAPACITY + ahead_limit(session);
        uint8_t *rx = rx_buffer(session);
        uint32_t head = session->rx_head;
        uint32_t first = length < ring_size - head ? length : ring_size - head;
        for (uint32_t i = 0; i < first; ++i) dst[i] = rx[head + i];
        for (uint32_t i = first; i < length; ++i) dst[i] = rx[i - first];
        head += length;
        session->rx_head = head >= ring_size ? head - ring_size : head;
#else
        for (uint32_t i = 0; i < length; ++i) io->response[i] = session->rx[i];
        memmove(session->rx, session->rx + length, session->rx_length - length);
#endif
#ifdef NEXAWAL_UTM_RX_AHEAD
        uint32_t ahead = ahead_limit(session);
        uint32_t withheld = session->rx_length > ahead ? session->rx_length - ahead : 0;
        uint16_t credit = length < withheld ? length : withheld;
#endif
        session->rx_length -= length;
        io->response_len = length;
#ifdef NEXAWAL_UTM_RX_AHEAD
        if (session->pcb != NULL && credit != 0) tcp_recved(session->pcb, credit);
#else
        if (session->pcb != NULL) tcp_recved(session->pcb, length);
#endif
        return HASKELL_TCP_DATA;
    }
    if (session->state == CONNECTED || session->state == CONNECTING) return HASKELL_TCP_PENDING;
    if (session->state == PEER_CLOSED || session->state == IDLE) return HASKELL_TCP_CLOSED;
    return HASKELL_TCP_ERROR;
}


#undef RX_LIMIT

#ifdef NEXAWAL_UTM_WIDE_RX
unsigned haskell_tcp_recv(void)
{
    return recv_into(page()->response, HASKELL_TCP_MAX);
}
unsigned haskell_tcp_recv_wide(void)
{
    if (!wire_is_slot_local || active_slot != HASKELL_TCP_WALLET_SLOT) return HASKELL_TCP_ERROR;
    _Static_assert(sizeof(struct haskell_tcp_page) == 280, "keep legacy stride");
    _Static_assert(sizeof(struct haskell_tcp_page)*HASKELL_TCP_SESSIONS <= WALLET_WIDE_RX_OFFSET, "legacy page range");
    _Static_assert(WALLET_WIDE_RX_OFFSET + WALLET_WIDE_RX_BYTES <= 4096, "wallet receive buffer must fit mapped page");
    return recv_into((volatile uint8_t *)(haskell_tcp_page + WALLET_WIDE_RX_OFFSET), WALLET_WIDE_RX_BYTES);
}

#endif

#ifdef NEXAWAL_UTM_RX_NOTIFY
/* Single-threaded lwIP PD. Register after the empty read and recheck after
 * draining RX, including the drain performed before returning from a PPC. */
unsigned haskell_tcp_recv_wide_wait(bool notify)
{
    unsigned result = haskell_tcp_recv_wide();
    if (result == HASKELL_TCP_PENDING)
        sessions[HASKELL_TCP_WALLET_SLOT].rx_waiting = notify;
    return result;
}

bool haskell_tcp_take_wallet_rx_wake(void)
{
    struct tcp_session *session = &sessions[HASKELL_TCP_WALLET_SLOT];
    if (!session->rx_waiting) return false;
    if (session->rx_length == 0 && (session->state == CONNECTED || session->state == CONNECTING))
        return false;
    session->rx_waiting = false;
    return true;
}
#endif

unsigned haskell_tcp_close(void)
{
    struct tcp_session *session = selected_session();
    if (session == NULL) return HASKELL_TCP_ERROR;
    if (session->pcb != NULL) {
        struct tcp_pcb *pcb = session->pcb;
        tcp_recv(pcb, NULL);
        tcp_err(pcb, NULL);
        if (tcp_close(pcb) != ERR_OK) {
            tcp_recv(pcb, receive);
            tcp_err(pcb, connection_error);
            return HASKELL_TCP_BUSY;
        }
        session->pcb = NULL;
    }
#ifdef NEXAWAL_UTM_RX_NOTIFY
    session->rx_waiting = false;
#endif
    session->state = IDLE;
    session->rx_length = 0;
    return HASKELL_TCP_CLOSED;
}

unsigned haskell_tcp_cancel(void)
{
    struct tcp_session *session = selected_session();
    if (session == NULL) return HASKELL_TCP_ERROR;
    if (session->pcb != NULL) {
        struct tcp_pcb *pcb = session->pcb;
        session->pcb = NULL;
        tcp_recv(pcb, NULL);
        tcp_err(pcb, NULL);
        tcp_abort(pcb);
    }
#ifdef NEXAWAL_UTM_RX_NOTIFY
    session->rx_waiting = false;
#endif
    session->state = IDLE;
    session->rx_length = 0;
    return HASKELL_TCP_CLOSED;
}
