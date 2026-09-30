/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Minimal Mbed TLS 3.6 config: TLS 1.2 ECDHE-ECDSA + AES-128-GCM client with
 * a pinned self-signed cert (TOFU demo) for ./dev test-net.
 * Non-production demo cert only.
 */

#pragma once

#include <stddef.h>
#include <stdarg.h>

int printf(const char *format, ...);
int snprintf(char *str, size_t size, const char *format, ...);
int vsnprintf(char *str, size_t size, const char *format, va_list ap);
int fprintf(void *stream, const char *format, ...);
void exit(int status);

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
#define MBEDTLS_GCM_C
#define MBEDTLS_CIPHER_C
#define MBEDTLS_MD_C
#define MBEDTLS_SHA256_C

#define MBEDTLS_BIGNUM_C
#define MBEDTLS_NO_UDBL_DIVISION
#define MBEDTLS_ASN1_PARSE_C
#define MBEDTLS_ASN1_WRITE_C
#define MBEDTLS_OID_C
#define MBEDTLS_PK_C
#define MBEDTLS_PK_PARSE_C
#define MBEDTLS_ECP_C
#define MBEDTLS_ECDH_C
#define MBEDTLS_ECDSA_C
#define MBEDTLS_ECP_DP_SECP256R1_ENABLED

#define MBEDTLS_X509_USE_C
#define MBEDTLS_X509_CRT_PARSE_C

#define MBEDTLS_SSL_TLS_C
#define MBEDTLS_SSL_CLI_C
#define MBEDTLS_SSL_PROTO_TLS1_2
#define MBEDTLS_KEY_EXCHANGE_ECDHE_ECDSA_ENABLED

#define MBEDTLS_SSL_CIPHERSUITES \
    MBEDTLS_TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256

/* Cert handshake messages need more headroom than the old PSK smoke. */
#define MBEDTLS_SSL_IN_CONTENT_LEN  2048
#define MBEDTLS_SSL_OUT_CONTENT_LEN 2048

/* No MBEDTLS_NET_C — lwIP TCP BIOs in tls_smoke.c. */
/* No MBEDTLS_FS_IO / PEM — DER pin only. */
/* No wall clock — verify callback clears expired/future flags. */
