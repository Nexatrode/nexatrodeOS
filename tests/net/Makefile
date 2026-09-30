#
# Copyright 2026, Personal OS contributors
# SPDX-License-Identifier: BSD-2-Clause
#

ifeq ($(strip $(MICROKIT_SDK)),)
$(error MICROKIT_SDK must be specified)
endif

ifeq ($(strip $(MICROKIT_BOARD)),)
$(error MICROKIT_BOARD must be specified)
endif

BUILD_DIR ?= build
override BUILD_DIR := $(abspath ${BUILD_DIR})
export BUILD_DIR
export SDDF := $(abspath ${SDDF})
override MICROKIT_SDK := $(abspath ${MICROKIT_SDK})

NET_SRC := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
export NET_SRC
export NET_INCLUDE := $(NET_SRC)/include

IMAGE_FILE := $(BUILD_DIR)/loader.img
SYSTEM_FILE := $(BUILD_DIR)/net.system
REPORT_FILE := $(BUILD_DIR)/report.txt

all: ${SYSTEM_FILE}

${SYSTEM_FILE} ${IMAGE_FILE} ${REPORT_FILE} clean clobber: ${BUILD_DIR}/Makefile FORCE
	${MAKE} -C ${BUILD_DIR} MICROKIT_SDK=${MICROKIT_SDK} NET_SRC=${NET_SRC} \
		NET_INCLUDE=${NET_INCLUDE} $(notdir $@)

${BUILD_DIR}/Makefile: $(NET_SRC)/net.mk
	mkdir -p ${BUILD_DIR}
	cp $(NET_SRC)/net.mk $@

FORCE:
