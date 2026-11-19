MICROKIT_SDK ?= ../../.cache/microkit-sdk-2.3.1
MICROKIT_BOARD ?= qemu_virt_aarch64
MICROKIT_CONFIG ?= debug
SDDF ?= ../../.cache/sddf-personal-os
PYTHON ?= ../../.cache/sddf-venv/bin/python
BUILD_DIR ?= ../../build/ghc-scheduler-wait-probe
GHC_RTS_INCLUDE_FLAGS ?=
GHC_RTS_ADAPTER_OBJECTS ?=

BUILD_DIR_ABS := $(abspath $(BUILD_DIR))
MICROKIT_SDK_ABS := $(abspath $(MICROKIT_SDK))
SDDF_ABS := $(abspath $(SDDF))
TOP_ABS := $(CURDIR)

.PHONY: all clean
all: $(BUILD_DIR_ABS)/Makefile
	PATH="/opt/homebrew/opt/llvm/bin:$$PATH" $(MAKE) -C $(BUILD_DIR_ABS) \
		MICROKIT_SDK=$(MICROKIT_SDK_ABS) MICROKIT_BOARD=$(MICROKIT_BOARD) \
		MICROKIT_CONFIG=$(MICROKIT_CONFIG) SDDF=$(SDDF_ABS) TOP=$(TOP_ABS) \
		PYTHON=$(PYTHON) GHC_RTS_INCLUDE_FLAGS="$(GHC_RTS_INCLUDE_FLAGS)" \
		GHC_RTS_ADAPTER_OBJECTS="$(GHC_RTS_ADAPTER_OBJECTS)" loader.img

$(BUILD_DIR_ABS):
	mkdir -p $@

$(BUILD_DIR_ABS)/Makefile: timer.mk | $(BUILD_DIR_ABS)
	cp $< $@

clean:
	rm -rf $(BUILD_DIR_ABS)
