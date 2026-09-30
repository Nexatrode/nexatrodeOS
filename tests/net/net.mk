#
# Copyright 2026, Personal OS contributors
# SPDX-License-Identifier: BSD-2-Clause
#
# Copied into BUILD_DIR and operated on from there.
#

MICROKIT_TOOL ?= $(MICROKIT_SDK)/bin/microkit
NET_EXAMPLE := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
# When this file is copied into BUILD_DIR, NET_EXAMPLE above points at BUILD_DIR.
# The top-level Makefile sets NET_SRC to the source tree.
NET_SRC ?= $(NET_EXAMPLE)

SUPPORTED_BOARDS := qemu_virt_aarch64

TOOLCHAIN ?= clang
MICROKIT_CONFIG ?= debug
SYSTEM_FILE := net.system
IMAGE_FILE := loader.img
REPORT_FILE := report.txt

include ${SDDF}/tools/make/board/common.mk

METAPROGRAM := $(NET_SRC)/meta.py
LWIPDIR := network/ipstacks/lwip/src
UTIL := $(SDDF)/util
ETHERNET_DRIVER := $(SDDF)/drivers/network/$(NET_DRIV_DIR)
TIMER_DRIVER := $(SDDF)/drivers/timer/$(TIMER_DRIV_DIR)
NETWORK_COMPONENTS := $(SDDF)/network/components

SDDF_CUSTOM_LIBC := 1

vpath %.c $(NET_SRC) ${SDDF}

IMAGES := eth_driver.elf dhcp_client.elf \
	  network_virt_rx.elf network_virt_tx.elf network_copy.elf \
	  timer_driver.elf

CFLAGS += \
	  -Wno-unused-function \
	  -I$(BOARD_DIR)/include \
	  -I$(SDDF)/include/microkit \
	  -I$(SDDF)/include \
	  -I${NET_INCLUDE}/lwip \
	  -I$(LWIPDIR)/include \
	  -I$(LWIPDIR)/include/ipv4 \
	  -I $(NET_SRC)/include \
	  -MP

CFLAGS += -Wno-tautological-constant-out-of-range-compare

LDFLAGS := -L$(BOARD_DIR)/lib
LIBS := --start-group -lmicrokit -Tmicrokit.ld libsddf_util_debug.a --end-group

DHCP_OBJS := dhcp_client.o udp_echo_socket.o tcp_echo_socket.o dns_smoke.o
DEPS := $(DHCP_OBJS:.o=.d)

all: $(SYSTEM_FILE)

dhcp_client.elf: $(DHCP_OBJS) libsddf_util_debug.a lib_sddf_lwip.a
	$(LD) $(LDFLAGS) $^ $(LIBS) -o $@

${IMAGES}: libsddf_util_debug.a

$(SYSTEM_FILE): $(METAPROGRAM) $(IMAGES) $(DTB)
	$(PYTHON) $(METAPROGRAM) --sddf $(SDDF) --board $(MICROKIT_BOARD) \
	    --dtb $(DTB) --output . --sdf $(SYSTEM_FILE)
	$(OBJCOPY) --update-section .device_resources=ethernet_driver_device_resources.data eth_driver.elf
	$(OBJCOPY) --update-section .net_driver_config=net_driver.data eth_driver.elf
	$(OBJCOPY) --update-section .net_virt_rx_config=net_virt_rx.data network_virt_rx.elf
	$(OBJCOPY) --update-section .net_virt_tx_config=net_virt_tx.data network_virt_tx.elf
	$(OBJCOPY) --update-section .net_copy_config=net_copy_net_copier.data network_copy.elf
	$(OBJCOPY) --update-section .device_resources=timer_driver_device_resources.data timer_driver.elf
	$(OBJCOPY) --update-section .timer_client_config=timer_client_dhcp_client.data dhcp_client.elf
	$(OBJCOPY) --update-section .net_client_config=net_client_dhcp_client.data dhcp_client.elf
	$(OBJCOPY) --update-section .lib_sddf_lwip_config=lib_sddf_lwip_config_dhcp_client.data dhcp_client.elf
	touch $@

${IMAGE_FILE} $(REPORT_FILE): $(IMAGES) $(SYSTEM_FILE)
	$(MICROKIT_TOOL) $(SYSTEM_FILE) --search-path $(BUILD_DIR) \
	--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) \
	-o $(IMAGE_FILE) -r $(REPORT_FILE)

include ${SDDF}/util/util.mk
include ${SDDF}/network/components/network_components.mk
include ${SDDF}/network/lib_sddf_lwip/lib_sddf_lwip.mk
include ${ETHERNET_DRIVER}/eth_driver.mk
include ${TIMER_DRIVER}/timer_driver.mk

clean::
	${RM} -f *.elf .depend*
	find . -name \*.[do] | xargs --no-run-if-empty rm

clobber:: clean
	rm -f ${IMAGE_FILE} ${REPORT_FILE} $(SYSTEM_FILE)

-include $(DEPS)
