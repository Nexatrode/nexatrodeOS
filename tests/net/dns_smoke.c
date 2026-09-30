/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Guest DNS smoke: resolve personal-os.test via host stub at 10.0.2.2:5353.
 */

#include <os/sddf.h>
#include "lwip/dns.h"
#include "lwip/err.h"
#include "lwip/ip_addr.h"
#include <sddf/util/printf.h>
#include "dhcp_client.h"

#define DNS_SMOKE_HOST "personal-os.test"

static void dns_smoke_found(const char *name, const ip_addr_t *ipaddr, void *arg)
{
    (void)arg;
    if (name == NULL || ipaddr == NULL || !IP_IS_V4(ipaddr)) {
        sddf_dprintf("DNS|LOG: lookup failed for %s\n", name ? name : "?");
        return;
    }
    const ip4_addr_t *v4 = ip_2_ip4(ipaddr);
    if (ip4_addr_get_u32(v4) == PP_HTONL(LWIP_MAKEU32(10, 0, 2, 2))) {
        sddf_printf("DNS OK\n");
    } else {
        sddf_dprintf("DNS|LOG: unexpected address %s for %s\n",
                     ipaddr_ntoa(ipaddr), name);
    }
}

void start_dns_smoke(void)
{
    ip_addr_t server;
    IP_ADDR4(&server, 10, 0, 2, 2);
    dns_setserver(0, &server);

    ip_addr_t addr;
    err_t err = dns_gethostbyname(DNS_SMOKE_HOST, &addr, dns_smoke_found, NULL);
    if (err == ERR_OK) {
        dns_smoke_found(DNS_SMOKE_HOST, &addr, NULL);
    } else if (err != ERR_INPROGRESS) {
        sddf_dprintf("DNS|LOG: dns_gethostbyname failed: %s\n", lwip_strerr(err));
    }
}
