#
# Copyright 2026, Personal OS contributors
# SPDX-License-Identifier: BSD-2-Clause
#
# Build a trimmed Mbed TLS static lib for the Personal OS net fragment.
# Requires MBEDTLS_DIR (extracted mbedtls-3.6.x tree).
#

ifeq ($(strip $(MBEDTLS_DIR)),)
$(error MBEDTLS_DIR must point at an extracted Mbed TLS 3.6 tree)
endif

MBEDTLS_LIB_DIR := $(MBEDTLS_DIR)/library
MBEDTLS_INC := $(MBEDTLS_DIR)/include

MBEDTLS_SRCS := \
	aes.c \
	asn1parse.c \
	asn1write.c \
	bignum.c \
	bignum_core.c \
	block_cipher.c \
	cipher.c \
	cipher_wrap.c \
	constant_time.c \
	ctr_drbg.c \
	ecdh.c \
	ecdsa.c \
	ecp.c \
	ecp_curves.c \
	entropy.c \
	gcm.c \
	md.c \
	memory_buffer_alloc.c \
	oid.c \
	pk.c \
	pk_ecc.c \
	pk_wrap.c \
	pkparse.c \
	platform.c \
	platform_util.c \
	sha256.c \
	ssl_ciphersuites.c \
	ssl_client.c \
	ssl_msg.c \
	ssl_tls.c \
	ssl_tls12_client.c \
	x509.c \
	x509_crt.c

MBEDTLS_OBJS := $(addprefix mbedtls_out/,$(MBEDTLS_SRCS:.c=.o))
MBEDTLS_LOCAL_OBJS := mbedtls_out/mbedtls_libc.o
MBEDTLS_ALL_OBJS := $(MBEDTLS_OBJS) $(MBEDTLS_LOCAL_OBJS)

CFLAGS += -DMBEDTLS_CONFIG_FILE='<mbedtls_config.h>'
CFLAGS += -I$(MBEDTLS_INC)

libmbedtls_tls.a: $(MBEDTLS_ALL_OBJS)
	$(AR) crv $@ $^
	$(RANLIB) $@

# Keep the old archive name as an alias so incremental builds still link.
libmbedtls_psk.a: libmbedtls_tls.a
	cp -f libmbedtls_tls.a $@

mbedtls_out/%.o: $(MBEDTLS_LIB_DIR)/%.c | mbedtls_out
	$(CC) $(CFLAGS) -c -o $@ $<

mbedtls_out/mbedtls_libc.o: $(NET_SRC)/mbedtls_libc.c | mbedtls_out
	$(CC) $(CFLAGS) -c -o $@ $<

mbedtls_out:
	mkdir -p $@

clean::
	$(RM) -rf mbedtls_out libmbedtls_tls.a libmbedtls_psk.a
