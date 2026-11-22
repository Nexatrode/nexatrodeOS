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
#include "lwip/err.h"
#include "lwip/dns.h"
#include "lwip/ip_addr.h"
#include "lwip/pbuf.h"
#include "lwip/tcp.h"

#define RX_CAPACITY 32768u

enum session_state { IDLE, CONNECTING, CONNECTED, PEER_CLOSED, FAILED };
static enum session_state state;
static struct tcp_pcb *active_pcb;
static uint8_t rx_buffer[RX_CAPACITY];
static uint32_t rx_length;
static bool network_ready;
enum dns_state { DNS_IDLE, DNS_PENDING, DNS_DONE, DNS_FAILED };
static enum dns_state lookup_state;
static char lookup_name[HASKELL_TCP_MAX + 1];
static uint8_t lookup_address[4];

static volatile struct haskell_tcp_page *page(void)
{
    return (volatile struct haskell_tcp_page *)haskell_tcp_page;
}

void haskell_tcp_network_ready(void)
{
    network_ready = true;
}

void haskell_dns_configure_server(void)
{
    ip_addr_t server;
    IP_ADDR4(&server, 10, 0, 2, 2);
    dns_setserver(0, &server);
}

static void dns_found(const char *name, const ip_addr_t *addr, void *arg)
{
    (void)name;
    (void)arg;
    if (addr == NULL || !IP_IS_V4(addr)) {
        lookup_state = DNS_FAILED;
        return;
    }
    const ip4_addr_t *v4 = ip_2_ip4(addr);
    lookup_address[0] = ip4_addr1(v4);
    lookup_address[1] = ip4_addr2(v4);
    lookup_address[2] = ip4_addr3(v4);
    lookup_address[3] = ip4_addr4(v4);
    lookup_state = DNS_DONE;
}

unsigned haskell_dns_start(void)
{
    if (!network_ready) return HASKELL_TCP_NOT_READY;
    if (lookup_state == DNS_PENDING) return HASKELL_TCP_BUSY;
    volatile struct haskell_tcp_page *io = page();
    uint32_t length = io->request_len;
    if (length == 0 || length >= HASKELL_TCP_MAX) return HASKELL_TCP_ERROR;
    for (uint32_t i = 0; i < length; ++i) {
        uint8_t ch = io->request[i];
        if (!((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
              (ch >= '0' && ch <= '9') || ch == '-' || ch == '.'))
            return HASKELL_TCP_ERROR;
        lookup_name[i] = (char)ch;
    }
    lookup_name[length] = '\0';
    lookup_state = DNS_PENDING;
    ip_addr_t address;
    err_t result = dns_gethostbyname(lookup_name, &address, dns_found, NULL);
    if (result == ERR_OK) dns_found(lookup_name, &address, NULL);
    else if (result != ERR_INPROGRESS) lookup_state = DNS_FAILED;
    return lookup_state == DNS_FAILED ? HASKELL_TCP_ERROR : HASKELL_TCP_ACCEPTED;
}

unsigned haskell_dns_poll(void)
{
    if (lookup_state == DNS_PENDING) return HASKELL_TCP_PENDING;
    if (lookup_state != DNS_DONE) return HASKELL_TCP_ERROR;
    volatile struct haskell_tcp_page *io = page();
    io->response_len = 4;
    for (unsigned i = 0; i < 4; ++i) io->response[i] = lookup_address[i];
    return HASKELL_TCP_DATA;
}

static err_t release_pcb(struct tcp_pcb *pcb)
{
    active_pcb = NULL;
    tcp_arg(pcb, NULL);
    tcp_recv(pcb, NULL);
    tcp_err(pcb, NULL);
    if (tcp_close(pcb) == ERR_OK) return ERR_OK;
    tcp_abort(pcb);
    return ERR_ABRT;
}

static err_t fail_pcb(struct tcp_pcb *pcb)
{
    state = FAILED;
    rx_length = 0;
    return release_pcb(pcb);
}

static err_t receive(void *arg, struct tcp_pcb *pcb, struct pbuf *p, err_t err)
{
    (void)arg;
    if (p == NULL) {
        sddf_dprintf("HASKELL_TCP|peer closed connection\n");
        state = PEER_CLOSED;
        return release_pcb(pcb);
    }
    if (err != ERR_OK || state != CONNECTED) {
        sddf_dprintf("HASKELL_TCP|receive callback error: %s state=%u\n",
                     lwip_strerr(err), (unsigned)state);
        pbuf_free(p);
        return fail_pcb(pcb);
    }
    if (p->tot_len > RX_CAPACITY - rx_length) {
        /* lwIP retains a refused pbuf and retries after the client drains data. */
        return ERR_MEM;
    }
    uint16_t received = p->tot_len;
    uint16_t copied = pbuf_copy_partial(p, rx_buffer + rx_length, received, 0);
    pbuf_free(p);
    if (copied != received) return fail_pcb(pcb);
    rx_length += copied;
    sddf_dprintf("HASKELL_TCP|rx queued=%u added=%u\n",
                 (unsigned)rx_length, (unsigned)copied);
    return ERR_OK;
}

static void connection_error(void *arg, err_t err)
{
    (void)arg;
    sddf_dprintf("HASKELL_TCP|connection error: %s\n", lwip_strerr(err));
    active_pcb = NULL;
    rx_length = 0;
    state = FAILED;
}

static err_t connected(void *arg, struct tcp_pcb *pcb, err_t err)
{
    (void)arg;
    if (err != ERR_OK) return fail_pcb(pcb);
    state = CONNECTED;
    return ERR_OK;
}

unsigned haskell_tcp_open(void)
{
    volatile struct haskell_tcp_page *io = page();
    if (io->magic != HASKELL_TCP_MAGIC) return HASKELL_TCP_ERROR;
    if (!network_ready) return HASKELL_TCP_NOT_READY;
    if (state == CONNECTING || state == CONNECTED) return HASKELL_TCP_BUSY;
    if (state == PEER_CLOSED && rx_length != 0) return HASKELL_TCP_BUSY;
    uint16_t port = io->dest_port;
    uint8_t addr[4];
    for (unsigned i = 0; i < 4; ++i) addr[i] = io->dest_ip[i];
    if (port == 0 || (addr[0] | addr[1] | addr[2] | addr[3]) == 0)
        return HASKELL_TCP_ERROR;

    struct tcp_pcb *pcb = tcp_new_ip_type(IPADDR_TYPE_V4);
    if (pcb == NULL) return HASKELL_TCP_ERROR;
    rx_length = 0;
    active_pcb = pcb;
    state = CONNECTING;
    tcp_arg(pcb, NULL);
    tcp_recv(pcb, receive);
    tcp_err(pcb, connection_error);
    ip_addr_t dest;
    IP_ADDR4(&dest, addr[0], addr[1], addr[2], addr[3]);
    if (tcp_connect(pcb, &dest, port, connected) != ERR_OK) {
        (void)fail_pcb(pcb);
        return HASKELL_TCP_ERROR;
    }
    return HASKELL_TCP_ACCEPTED;
}

unsigned haskell_tcp_status(void)
{
    if (state == CONNECTING) return HASKELL_TCP_PENDING;
    if (state == CONNECTED) return HASKELL_TCP_CONNECTED;
    if (state == PEER_CLOSED || state == IDLE) return HASKELL_TCP_CLOSED;
    return HASKELL_TCP_ERROR;
}

unsigned haskell_tcp_send(void)
{
    if (state != CONNECTED || active_pcb == NULL) return HASKELL_TCP_ERROR;
    volatile struct haskell_tcp_page *io = page();
    uint32_t length = io->request_len;
    if (length == 0 || length > HASKELL_TCP_MAX) return HASKELL_TCP_ERROR;
    uint8_t snapshot[HASKELL_TCP_MAX];
    for (uint32_t i = 0; i < length; ++i) snapshot[i] = io->request[i];
    err_t err = tcp_write(active_pcb, snapshot, length, TCP_WRITE_FLAG_COPY);
    if (err == ERR_MEM) return HASKELL_TCP_BUSY;
    if (err != ERR_OK) return HASKELL_TCP_ERROR;
    /* Once tcp_write copies the bytes they are accepted, even if the immediate
     * output attempt cannot transmit them yet. lwIP will retry queued data. */
    (void)tcp_output(active_pcb);
    return HASKELL_TCP_ACCEPTED;
}

unsigned haskell_tcp_recv(void)
{
    volatile struct haskell_tcp_page *io = page();
    uint32_t capacity = io->request_len;
    io->response_len = 0;
    if (capacity == 0 || capacity > HASKELL_TCP_MAX) return HASKELL_TCP_ERROR;
    if (rx_length != 0) {
        uint32_t length = rx_length < capacity ? rx_length : capacity;
        for (uint32_t i = 0; i < length; ++i) io->response[i] = rx_buffer[i];
        memmove(rx_buffer, rx_buffer + length, rx_length - length);
        rx_length -= length;
        io->response_len = length;
        if (active_pcb != NULL) tcp_recved(active_pcb, length);
        return HASKELL_TCP_DATA;
    }
    if (state == CONNECTED || state == CONNECTING) return HASKELL_TCP_PENDING;
    if (state == PEER_CLOSED || state == IDLE) return HASKELL_TCP_CLOSED;
    return HASKELL_TCP_ERROR;
}

unsigned haskell_tcp_close(void)
{
    if (active_pcb != NULL) {
        struct tcp_pcb *pcb = active_pcb;
        tcp_recv(pcb, NULL);
        tcp_err(pcb, NULL);
        if (tcp_close(pcb) != ERR_OK) {
            tcp_recv(pcb, receive);
            tcp_err(pcb, connection_error);
            return HASKELL_TCP_BUSY;
        }
        active_pcb = NULL;
    }
    state = IDLE;
    rx_length = 0;
    return HASKELL_TCP_CLOSED;
}

unsigned haskell_tcp_cancel(void)
{
    if (active_pcb != NULL) {
        struct tcp_pcb *pcb = active_pcb;
        active_pcb = NULL;
        tcp_recv(pcb, NULL);
        tcp_err(pcb, NULL);
        tcp_abort(pcb);
    }
    state = IDLE;
    rx_length = 0;
    return HASKELL_TCP_CLOSED;
}
