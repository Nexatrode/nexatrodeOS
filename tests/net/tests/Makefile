# Run with make -C tests/net/tests; SDDF may point to an alternate checkout.
SDDF ?= ../../../.cache/sddf-personal-os
LWIP := $(SDDF)/network/ipstacks/lwip/src
HOST_CC ?= cc
HOST_RUSTC ?= rustc
BUILD_DIR ?= ../../../build/tests/net
CPPFLAGS += -Iinclude -iquote ../include -I../include/lwip -I$(SDDF)/include -I$(LWIP)/include \
            -DLWIP_DONT_PROVIDE_BYTEORDER_FUNCTIONS
HOST_CFLAGS := -std=gnu11 -O2 -ffunction-sections -fdata-sections
ifeq ($(shell uname -s),Darwin)
HOST_LDFLAGS := -Wl,-dead_strip
else
HOST_LDFLAGS := -Wl,--gc-sections
endif

.PHONY: test
test:
	mkdir -p "$(BUILD_DIR)"
	$(HOST_CC) $(CPPFLAGS) $(HOST_CFLAGS) $(CFLAGS) -UNDEBUG haskell_tcp_receive_test.c \
	    $(LWIP)/core/pbuf.c $(LDFLAGS) $(HOST_LDFLAGS) -o "$(BUILD_DIR)/haskell_tcp_receive_test"
	"$(BUILD_DIR)/haskell_tcp_receive_test"
	$(HOST_CC) $(CPPFLAGS) $(HOST_CFLAGS) $(CFLAGS) -UNDEBUG -DNEXAWAL_UTM_WIDE_RX=1 -DNEXAWAL_UTM_RX_NOTIFY=1 haskell_tcp_receive_test.c \
	    $(LWIP)/core/pbuf.c $(LDFLAGS) $(HOST_LDFLAGS) -o "$(BUILD_DIR)/haskell_tcp_wide_receive_test"
	"$(BUILD_DIR)/haskell_tcp_wide_receive_test"
	$(HOST_CC) $(CPPFLAGS) $(HOST_CFLAGS) $(CFLAGS) -UNDEBUG -DNEXAWAL_UTM_WIDE_RX=1 -DNEXAWAL_UTM_RX_NOTIFY=1 -DNEXAWAL_UTM_RX_AHEAD=1 haskell_tcp_receive_test.c \
	    $(LWIP)/core/pbuf.c $(LDFLAGS) $(HOST_LDFLAGS) -o "$(BUILD_DIR)/haskell_tcp_receive_ahead_test"
	"$(BUILD_DIR)/haskell_tcp_receive_ahead_test"
	$(HOST_CC) $(CPPFLAGS) $(HOST_CFLAGS) $(CFLAGS) -UNDEBUG simplex_network_lock_test.c \
	    $(LWIP)/core/pbuf.c $(LDFLAGS) $(HOST_LDFLAGS) -o "$(BUILD_DIR)/simplex_network_lock_test"
	"$(BUILD_DIR)/simplex_network_lock_test"
	$(HOST_CC) $(CPPFLAGS) $(HOST_CFLAGS) $(CFLAGS) -UNDEBUG desktop_tcp_isolation_test.c \
	    $(LWIP)/core/pbuf.c $(LDFLAGS) $(HOST_LDFLAGS) -o "$(BUILD_DIR)/desktop_tcp_isolation_test"
	"$(BUILD_DIR)/desktop_tcp_isolation_test"
	$(HOST_CC) $(CPPFLAGS) $(HOST_CFLAGS) $(CFLAGS) -UNDEBUG desktop_network_lock_test.c \
	    $(LWIP)/core/pbuf.c $(LDFLAGS) $(HOST_LDFLAGS) -o "$(BUILD_DIR)/desktop_network_lock_test"
	"$(BUILD_DIR)/desktop_network_lock_test"
	$(HOST_RUSTC) --edition=2021 --cfg 'feature="wallet-https"' wallet_tcp_compat_test.rs -o "$(BUILD_DIR)/wallet_tcp_legacy_test"
	"$(BUILD_DIR)/wallet_tcp_legacy_test"
	$(HOST_RUSTC) --edition=2021 --cfg 'feature="wallet-https"' --cfg 'feature="wallet-wide-rx"' wallet_tcp_compat_test.rs -o "$(BUILD_DIR)/wallet_tcp_wide_test"
	"$(BUILD_DIR)/wallet_tcp_wide_test"
