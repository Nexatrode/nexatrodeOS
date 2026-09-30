/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <os/sddf.h>
#include <string.h>
#include "lwip/ip.h"
#include "lwip/pbuf.h"
#include "lwip/udp.h"
#include "lwip/err.h"
#include <sddf/util/util.h>
#include <sddf/util/printf.h>
#include "dhcp_client.h"
#include "psl1_demo_packet.h"

static struct udp_pcb *udp_echo_socket;
static struct udp_pcb *udp_seal_socket;

static void lwip_udp_echo_callback(void *arg, struct udp_pcb *pcb, struct pbuf *p,
                                   const ip_addr_t *addr, u16_t port)
{
    err_t error = udp_sendto(pcb, p, addr, port);
    if (error) {
        sddf_dprintf("Failed to send UDP packet through socket: %s\n", lwip_strerr(error));
    }
    pbuf_free(p);
}

static void lwip_udp_seal_callback(void *arg, struct udp_pcb *pcb, struct pbuf *p,
                                   const ip_addr_t *addr, u16_t port)
{
    (void)arg;
    (void)pcb;
    (void)addr;
    (void)port;
    if (p != NULL && p->tot_len == PSL1_DEMO_PACKET_LEN) {
        uint8_t buffer[PSL1_DEMO_PACKET_LEN];
        if (pbuf_copy_partial(p, buffer, PSL1_DEMO_PACKET_LEN, 0) == PSL1_DEMO_PACKET_LEN
            && memcmp(buffer, PSL1_DEMO_PACKET, PSL1_DEMO_PACKET_LEN) == 0) {
            sddf_printf("SEAL OK\n");
        } else {
            sddf_dprintf("SEAL|LOG: unexpected PSL1 datagram\n");
        }
    } else {
        sddf_dprintf("SEAL|LOG: bad datagram length\n");
    }
    pbuf_free(p);
}

int setup_udp_socket(void)
{
    udp_echo_socket = udp_new_ip_type(IPADDR_TYPE_V4);
    if (udp_echo_socket == NULL) {
        sddf_dprintf("Failed to open a UDP socket\n");
        return -1;
    }

    int error = udp_bind(udp_echo_socket, IP_ANY_TYPE, UDP_ECHO_PORT);
    if (error == ERR_OK) {
        udp_recv(udp_echo_socket, lwip_udp_echo_callback, udp_echo_socket);
    } else {
        sddf_dprintf("Failed to bind the UDP socket\n");
        return -1;
    }

    udp_seal_socket = udp_new_ip_type(IPADDR_TYPE_V4);
    if (udp_seal_socket == NULL) {
        sddf_dprintf("Failed to open the sealed UDP socket\n");
        return -1;
    }
    error = udp_bind(udp_seal_socket, IP_ANY_TYPE, UDP_SEAL_PORT);
    if (error == ERR_OK) {
        udp_recv(udp_seal_socket, lwip_udp_seal_callback, udp_seal_socket);
    } else {
        sddf_dprintf("Failed to bind the sealed UDP socket\n");
        return -1;
    }
    return 0;
}
