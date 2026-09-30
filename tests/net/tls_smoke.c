/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Guest TLS 1.2 PSK smoke: connect to 10.0.2.2:1239, echo PERSONAL-OS-TLS, print TLS OK.
 */

#include <stdbool.h>
#include <stdarg.h>
#include <string.h>
#include <os/sddf.h>

#include "lwip/err.h"
#include "lwip/ip_addr.h"
#include "lwip/pbuf.h"
#include "lwip/tcp.h"

#include "mbedtls/ctr_drbg.h"
#include "mbedtls/entropy.h"
#include "mbedtls/memory_buffer_alloc.h"
#include "mbedtls/ssl.h"

#include <sddf/util/printf.h>
#include "dhcp_client.h"

#define TLS_RX_CAP 2048
#define TLS_HEAP_BYTES (48 * 1024)

static const char TLS_PAYLOAD[] = "PERSONAL-OS-TLS";
static const char TLS_PSK_ID[] = "personal-os";
/* Demo only — not a production secret. */
static const unsigned char TLS_PSK[16] = {
    0x50, 0x65, 0x72, 0x73, 0x6f, 0x6e, 0x61, 0x6c,
    0x4f, 0x53, 0x2d, 0x54, 0x4c, 0x53, 0x21, 0x21
};

static unsigned char tls_heap[TLS_HEAP_BYTES];
static unsigned char tls_rx[TLS_RX_CAP];
static size_t tls_rx_len;
static size_t tls_rx_off;

static mbedtls_entropy_context entropy;
static mbedtls_ctr_drbg_context ctr_drbg;
static mbedtls_ssl_config conf;
static mbedtls_ssl_context ssl;
static struct tcp_pcb *tls_pcb;
static bool tls_started;
static bool tls_done;
static bool tls_mbedtls_ready;
static bool tls_sent;

int mbedtls_hardware_poll(void *data, unsigned char *output, size_t len, size_t *olen)
{
    (void)data;
    /* Deterministic smoke entropy — not for production keys. */
    static uint32_t counter = 0xA5A5C3C3u;
    for (size_t i = 0; i < len; i++) {
        counter = counter * 1664525u + 1013904223u;
        output[i] = (unsigned char)(counter >> 16);
    }
    *olen = len;
    return 0;
}

void exit(int status)
{
    (void)status;
    while (1) {
    }
}

void mbedtls_platform_zeroize(void *buf, size_t len)
{
    volatile unsigned char *p = buf;
    while (len--) {
        *p++ = 0;
    }
}

int printf(const char *format, ...)
{
    (void)format;
    return 0;
}

int snprintf(char *str, size_t size, const char *format, ...)
{
    (void)format;
    if (str != NULL && size > 0) {
        str[0] = '\0';
    }
    return 0;
}

int vsnprintf(char *str, size_t size, const char *format, va_list ap)
{
    (void)format;
    (void)ap;
    if (str != NULL && size > 0) {
        str[0] = '\0';
    }
    return 0;
}

int fprintf(void *stream, const char *format, ...)
{
    (void)stream;
    (void)format;
    return 0;
}

static void tls_rx_push(const uint8_t *data, u16_t len)
{
    if (tls_rx_len + len > TLS_RX_CAP) {
        sddf_dprintf("TLS|LOG: rx overflow\n");
        return;
    }
    memcpy(tls_rx + tls_rx_len, data, len);
    tls_rx_len += len;
}

static int tls_bio_send(void *ctx, const unsigned char *buf, size_t len)
{
    struct tcp_pcb *pcb = ctx;
    if (pcb == NULL) {
        return MBEDTLS_ERR_SSL_INTERNAL_ERROR;
    }
    size_t avail = tcp_sndbuf(pcb);
    if (avail == 0) {
        return MBEDTLS_ERR_SSL_WANT_WRITE;
    }
    if (len > avail) {
        len = avail;
    }
    err_t err = tcp_write(pcb, buf, (u16_t)len, TCP_WRITE_FLAG_COPY);
    if (err == ERR_MEM) {
        return MBEDTLS_ERR_SSL_WANT_WRITE;
    }
    if (err != ERR_OK) {
        return MBEDTLS_ERR_SSL_INTERNAL_ERROR;
    }
    tcp_output(pcb);
    return (int)len;
}

static int tls_bio_recv(void *ctx, unsigned char *buf, size_t len)
{
    (void)ctx;
    if (tls_rx_off >= tls_rx_len) {
        return MBEDTLS_ERR_SSL_WANT_READ;
    }
    size_t have = tls_rx_len - tls_rx_off;
    if (len > have) {
        len = have;
    }
    memcpy(buf, tls_rx + tls_rx_off, len);
    tls_rx_off += len;
    if (tls_rx_off == tls_rx_len) {
        tls_rx_off = 0;
        tls_rx_len = 0;
    }
    return (int)len;
}

static void tls_drive(void)
{
    int ret;

    if (tls_done || tls_pcb == NULL || !tls_mbedtls_ready) {
        return;
    }

    while ((ret = mbedtls_ssl_handshake(&ssl)) != 0) {
        if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE) {
            return;
        }
        sddf_dprintf("TLS|LOG: handshake failed: -0x%04x\n", (unsigned)-ret);
        tls_done = true;
        return;
    }

    if (!tls_sent) {
        ret = mbedtls_ssl_write(&ssl, (const unsigned char *)TLS_PAYLOAD,
                                sizeof(TLS_PAYLOAD) - 1);
        if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE) {
            return;
        }
        if (ret < 0) {
            sddf_dprintf("TLS|LOG: write failed: -0x%04x\n", (unsigned)-ret);
            tls_done = true;
            return;
        }
        tls_sent = true;
    }

    unsigned char got[sizeof(TLS_PAYLOAD)];
    ret = mbedtls_ssl_read(&ssl, got, sizeof(TLS_PAYLOAD) - 1);
    if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE) {
        return;
    }
    if (ret == sizeof(TLS_PAYLOAD) - 1
        && memcmp(got, TLS_PAYLOAD, sizeof(TLS_PAYLOAD) - 1) == 0) {
        sddf_printf("TLS OK\n");
    } else {
        sddf_dprintf("TLS|LOG: bad echo (%d)\n", ret);
    }
    tls_done = true;
}

static err_t tls_recv(void *arg, struct tcp_pcb *pcb, struct pbuf *p, err_t err)
{
    (void)arg;
    if (err != ERR_OK || p == NULL) {
        if (p != NULL) {
            pbuf_free(p);
        }
        return ERR_OK;
    }
    for (struct pbuf *q = p; q != NULL; q = q->next) {
        tls_rx_push(q->payload, q->len);
    }
    tcp_recved(pcb, p->tot_len);
    pbuf_free(p);
    tls_drive();
    return ERR_OK;
}

static void tls_err(void *arg, err_t err)
{
    (void)arg;
    sddf_dprintf("TLS|LOG: tcp err %d\n", (int)err);
    tls_pcb = NULL;
    tls_done = true;
}

static err_t tls_connected(void *arg, struct tcp_pcb *pcb, err_t err)
{
    (void)arg;
    if (err != ERR_OK) {
        sddf_dprintf("TLS|LOG: connect failed %d\n", (int)err);
        tls_done = true;
        return err;
    }
    tcp_recv(pcb, tls_recv);
    mbedtls_ssl_set_bio(&ssl, pcb, tls_bio_send, tls_bio_recv, NULL);
    tls_drive();
    return ERR_OK;
}

static int tls_setup_mbedtls(void)
{
    mbedtls_memory_buffer_alloc_init(tls_heap, sizeof(tls_heap));
    mbedtls_entropy_init(&entropy);
    mbedtls_ctr_drbg_init(&ctr_drbg);
    mbedtls_ssl_config_init(&conf);
    mbedtls_ssl_init(&ssl);

    int ret = mbedtls_ctr_drbg_seed(&ctr_drbg, mbedtls_entropy_func, &entropy,
                                    (const unsigned char *)"personal-os-tls", 14);
    if (ret != 0) {
        return ret;
    }
    ret = mbedtls_ssl_config_defaults(&conf, MBEDTLS_SSL_IS_CLIENT,
                                      MBEDTLS_SSL_TRANSPORT_STREAM,
                                      MBEDTLS_SSL_PRESET_DEFAULT);
    if (ret != 0) {
        return ret;
    }
    mbedtls_ssl_conf_rng(&conf, mbedtls_ctr_drbg_random, &ctr_drbg);
    mbedtls_ssl_conf_authmode(&conf, MBEDTLS_SSL_VERIFY_NONE);
    ret = mbedtls_ssl_conf_psk(&conf, TLS_PSK, sizeof(TLS_PSK),
                               (const unsigned char *)TLS_PSK_ID,
                               sizeof(TLS_PSK_ID) - 1);
    if (ret != 0) {
        return ret;
    }
    ret = mbedtls_ssl_setup(&ssl, &conf);
    if (ret != 0) {
        return ret;
    }
    tls_mbedtls_ready = true;
    return 0;
}

void start_tls_smoke(void)
{
    if (tls_started) {
        return;
    }
    tls_started = true;

    if (tls_setup_mbedtls() != 0) {
        sddf_dprintf("TLS|LOG: mbedtls setup failed\n");
        tls_done = true;
        return;
    }

    tls_pcb = tcp_new_ip_type(IPADDR_TYPE_V4);
    if (tls_pcb == NULL) {
        sddf_dprintf("TLS|LOG: tcp_new failed\n");
        tls_done = true;
        return;
    }
    tcp_err(tls_pcb, tls_err);
    ip_addr_t dest;
    IP_ADDR4(&dest, 10, 0, 2, 2);
    err_t err = tcp_connect(tls_pcb, &dest, TLS_SMOKE_PORT, tls_connected);
    if (err != ERR_OK) {
        sddf_dprintf("TLS|LOG: tcp_connect failed %d\n", (int)err);
        tls_done = true;
    }
}
