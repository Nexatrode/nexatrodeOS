/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Guest TLS 1.2 ECDHE-ECDSA smoke with a pinned self-signed cert (TOFU demo):
 * connect to 10.0.2.2:1239, verify the leaf SHA-256 pin, send the fixed PSL1
 * sealed demo datagram, require echo, print TLS OK / TLS SEAL OK, then keep the
 * session open so Messages can queue UI sealed datagrams (MSG TLS TX).
 */

#include <stdbool.h>
#include <stdarg.h>
#include <stdint.h>
#include <string.h>
#include <os/sddf.h>

#include "lwip/err.h"
#include "lwip/ip_addr.h"
#include "lwip/pbuf.h"
#include "lwip/tcp.h"

#include "mbedtls/ctr_drbg.h"
#include "mbedtls/entropy.h"
#include "mbedtls/memory_buffer_alloc.h"
#include "mbedtls/sha256.h"
#include "mbedtls/ssl.h"
#include "mbedtls/x509_crt.h"

#include <sddf/util/printf.h>
#include "dhcp_client.h"
#include "psl1_demo_packet.h"
#include "tls_tofu_pin.h"

#define TLS_RX_CAP 4096
#define TLS_HEAP_BYTES (128 * 1024)
#define TLS_PENDING_MAX MESSAGES_NET_TX_MAX

static unsigned char tls_heap[TLS_HEAP_BYTES];
static unsigned char tls_rx[TLS_RX_CAP];
static size_t tls_rx_len;
static size_t tls_rx_off;

static mbedtls_entropy_context entropy;
static mbedtls_ctr_drbg_context ctr_drbg;
static mbedtls_ssl_config conf;
static mbedtls_ssl_context ssl;
static mbedtls_x509_crt ca;
static struct tcp_pcb *tls_pcb;
static bool tls_started;
static bool tls_done;
static bool tls_mbedtls_ready;
static bool tls_handshaken;
static bool tls_sent;
static bool tls_seal_ok;

static uint8_t tls_pending[TLS_PENDING_MAX];
static size_t tls_pending_len;
static bool tls_pending_active;
static bool tls_pending_written;

static uint8_t tls_deferred[TLS_PENDING_MAX];
static size_t tls_deferred_len;
static bool tls_deferred_active;

static void tls_activate_pending(const uint8_t *data, size_t len)
{
    memcpy(tls_pending, data, len);
    tls_pending_len = len;
    tls_pending_active = true;
    tls_pending_written = false;
}

static void tls_promote_deferred(void)
{
    if (!tls_deferred_active || tls_pending_active || !tls_seal_ok) {
        return;
    }
    tls_activate_pending(tls_deferred, tls_deferred_len);
    tls_deferred_active = false;
    tls_deferred_len = 0;
}

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

static int tls_tofu_verify(void *data, mbedtls_x509_crt *crt, int depth, uint32_t *flags)
{
    (void)data;
    /* No wall clock in the guest — ignore validity window for this smoke. */
    *flags &= ~(MBEDTLS_X509_BADCERT_EXPIRED | MBEDTLS_X509_BADCERT_FUTURE
                | MBEDTLS_X509_BADCERT_CN_MISMATCH);

    if (depth == 0) {
        unsigned char hash[32];
        if (mbedtls_sha256(crt->raw.p, crt->raw.len, hash, 0) != 0
            || memcmp(hash, TLS_TOFU_CERT_SHA256, sizeof(TLS_TOFU_CERT_SHA256)) != 0) {
            sddf_dprintf("TLS|LOG: TOFU pin mismatch\n");
            *flags |= MBEDTLS_X509_BADCERT_OTHER;
            return -1;
        }
    }
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

static void tls_drive_pending(void)
{
    int ret;

    if (!tls_pending_active) {
        return;
    }

    if (!tls_pending_written) {
        ret = mbedtls_ssl_write(&ssl, tls_pending, tls_pending_len);
        if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE) {
            return;
        }
        if (ret < 0) {
            sddf_dprintf("TLS|LOG: UI sealed write failed: -0x%04x\n", (unsigned)-ret);
            tls_pending_active = false;
            return;
        }
        tls_pending_written = true;
    }

    unsigned char got[TLS_PENDING_MAX];
    ret = mbedtls_ssl_read(&ssl, got, tls_pending_len);
    if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE) {
        return;
    }
    if (ret == (int)tls_pending_len
        && memcmp(got, tls_pending, tls_pending_len) == 0) {
        sddf_printf("MSG TLS TX\n");
    } else {
        sddf_dprintf("TLS|LOG: bad UI sealed echo (%d)\n", ret);
    }
    tls_pending_active = false;
    tls_pending_written = false;
    tls_pending_len = 0;
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
        sddf_printf("TLS|LOG: handshake failed: -0x%04x\n", (unsigned)-ret);
        tls_done = true;
        return;
    }

    if (!tls_handshaken) {
        tls_handshaken = true;
        sddf_printf("TLS OK\n");
    }

    if (!tls_seal_ok) {
        if (!tls_sent) {
            ret = mbedtls_ssl_write(&ssl, PSL1_DEMO_PACKET, PSL1_DEMO_PACKET_LEN);
            if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE) {
                return;
            }
            if (ret < 0) {
                sddf_dprintf("TLS|LOG: sealed write failed: -0x%04x\n", (unsigned)-ret);
                tls_done = true;
                return;
            }
            tls_sent = true;
        }

        unsigned char got[PSL1_DEMO_PACKET_LEN];
        ret = mbedtls_ssl_read(&ssl, got, PSL1_DEMO_PACKET_LEN);
        if (ret == MBEDTLS_ERR_SSL_WANT_READ || ret == MBEDTLS_ERR_SSL_WANT_WRITE) {
            return;
        }
        if (ret == (int)PSL1_DEMO_PACKET_LEN
            && memcmp(got, PSL1_DEMO_PACKET, PSL1_DEMO_PACKET_LEN) == 0) {
            sddf_printf("TLS SEAL OK\n");
            tls_seal_ok = true;
            tls_promote_deferred();
        } else {
            sddf_dprintf("TLS|LOG: bad sealed echo (%d)\n", ret);
            tls_done = true;
            return;
        }
    }

    tls_drive_pending();
}

bool tls_session_ready(void)
{
    return tls_seal_ok && tls_pcb != NULL && !tls_done;
}

int tls_queue_sealed(const uint8_t *data, size_t len)
{
    if (tls_done || data == NULL || len == 0 || len > TLS_PENDING_MAX) {
        return -1;
    }
    if (tls_session_ready()) {
        if (tls_pending_active) {
            return -1;
        }
        tls_activate_pending(data, len);
        tls_drive();
        return 0;
    }
    /* Isolation can fire UI TX before the TLS demo seal finishes — defer once. */
    if (tls_deferred_active || tls_pending_active) {
        return -1;
    }
    memcpy(tls_deferred, data, len);
    tls_deferred_len = len;
    tls_deferred_active = true;
    return 0;
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
    sddf_printf("TLS|LOG: tcp err %d\n", (int)err);
    tls_pcb = NULL;
    tls_done = true;
    tls_pending_active = false;
}

static err_t tls_on_sent(void *arg, struct tcp_pcb *pcb, u16_t len)
{
    (void)arg;
    (void)pcb;
    (void)len;
    tls_drive();
    return ERR_OK;
}

static err_t tls_connected(void *arg, struct tcp_pcb *pcb, err_t err)
{
    (void)arg;
    if (err != ERR_OK) {
        sddf_printf("TLS|LOG: connect failed %d\n", (int)err);
        tls_done = true;
        return err;
    }
    tcp_recv(pcb, tls_recv);
    tcp_sent(pcb, tls_on_sent);
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
    mbedtls_x509_crt_init(&ca);

    int ret = mbedtls_ctr_drbg_seed(&ctr_drbg, mbedtls_entropy_func, &entropy,
                                    (const unsigned char *)"personal-os-tls", 14);
    if (ret != 0) {
        return ret;
    }
    ret = mbedtls_x509_crt_parse_der(&ca, TLS_TOFU_CERT_DER, TLS_TOFU_CERT_DER_LEN);
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
    mbedtls_ssl_conf_authmode(&conf, MBEDTLS_SSL_VERIFY_REQUIRED);
    mbedtls_ssl_conf_ca_chain(&conf, &ca, NULL);
    mbedtls_ssl_conf_verify(&conf, tls_tofu_verify, NULL);
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
        sddf_printf("TLS|LOG: mbedtls setup failed\n");
        tls_done = true;
        return;
    }

    tls_pcb = tcp_new_ip_type(IPADDR_TYPE_V4);
    if (tls_pcb == NULL) {
        sddf_printf("TLS|LOG: tcp_new failed\n");
        tls_done = true;
        return;
    }
    tcp_err(tls_pcb, tls_err);
    ip_addr_t dest;
    IP_ADDR4(&dest, 10, 0, 2, 2);
    err_t err = tcp_connect(tls_pcb, &dest, TLS_SMOKE_PORT, tls_connected);
    if (err != ERR_OK) {
        sddf_printf("TLS|LOG: tcp_connect failed %d\n", (int)err);
        tls_done = true;
    }
}
