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
	block_cipher.c \
	ccm.c \
	cipher.c \
	cipher_wrap.c \
	constant_time.c \
	ctr_drbg.c \
	entropy.c \
	md.c \
	memory_buffer_alloc.c \
	platform.c \
	platform_util.c \
	sha256.c \
	ssl_ciphersuites.c \
	ssl_client.c \
	ssl_msg.c \
	ssl_tls.c \
	ssl_tls12_client.c

MBEDTLS_OBJS := $(addprefix mbedtls_out/,$(MBEDTLS_SRCS:.c=.o))

CFLAGS += -DMBEDTLS_CONFIG_FILE='<mbedtls_config.h>'
CFLAGS += -I$(MBEDTLS_INC)

libmbedtls_psk.a: $(MBEDTLS_OBJS)
	$(AR) crv $@ $^
	$(RANLIB) $@

mbedtls_out/%.o: $(MBEDTLS_LIB_DIR)/%.c | mbedtls_out
	$(CC) $(CFLAGS) -c -o $@ $<

mbedtls_out:
	mkdir -p $@

clean::
	$(RM) -rf mbedtls_out libmbedtls_psk.a
