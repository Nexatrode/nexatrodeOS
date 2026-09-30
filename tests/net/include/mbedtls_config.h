/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Minimal Mbed TLS 3.6 config: TLS 1.2 PSK + AES-CCM-8 client for ./dev test-net.
 * Adapted from Mbed TLS configs/config-ccm-psk-tls1_2.h (Apache-2.0 OR GPL-2.0-or-later).
 * Non-production demo PSK only.
 */

#pragma once

#define MBEDTLS_PLATFORM_C
#define MBEDTLS_PLATFORM_MEMORY
#define MBEDTLS_PLATFORM_NO_STD_FUNCTIONS
#define MBEDTLS_PLATFORM_ZEROIZE_ALT
#define MBEDTLS_MEMORY_BUFFER_ALLOC_C
#define MBEDTLS_NO_PLATFORM_ENTROPY
#define MBEDTLS_ENTROPY_HARDWARE_ALT
#define MBEDTLS_ENTROPY_C
#define MBEDTLS_ENTROPY_MAX_SOURCES 2
#define MBEDTLS_CTR_DRBG_C

#define MBEDTLS_PLATFORM_EXIT_MACRO(x) exit(x)
#define MBEDTLS_PLATFORM_SNPRINTF_MACRO snprintf
#define MBEDTLS_PLATFORM_VSNPRINTF_MACRO vsnprintf
#define MBEDTLS_PLATFORM_PRINTF_MACRO printf
#define MBEDTLS_PLATFORM_FPRINTF_MACRO fprintf

#define MBEDTLS_AES_C
#define MBEDTLS_AES_ROM_TABLES
#define MBEDTLS_CCM_C
#define MBEDTLS_CIPHER_C
#define MBEDTLS_MD_C
#define MBEDTLS_SHA256_C

#define MBEDTLS_SSL_TLS_C
#define MBEDTLS_SSL_CLI_C
#define MBEDTLS_SSL_PROTO_TLS1_2
#define MBEDTLS_KEY_EXCHANGE_PSK_ENABLED

#define MBEDTLS_SSL_CIPHERSUITES \
    MBEDTLS_TLS_PSK_WITH_AES_128_CCM_8

#define MBEDTLS_SSL_IN_CONTENT_LEN  1024
#define MBEDTLS_SSL_OUT_CONTENT_LEN 1024
#define MBEDTLS_PSK_MAX_LEN 16

/* No MBEDTLS_NET_C — lwIP TCP BIOs in tls_smoke.c. */
/* No MBEDTLS_FS_IO / time — PSK needs neither. */
