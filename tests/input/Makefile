MICROKIT_SDK ?= $(error MICROKIT_SDK is required)
MICROKIT_BOARD ?= qemu_virt_aarch64
MICROKIT_CONFIG ?= debug
SDDF ?= $(error SDDF is required)
BUILD_DIR ?= $(error BUILD_DIR is required)

CC := clang
LD := ld.lld
BOARD_DIR := $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)
CFLAGS := -MD -ffreestanding -std=gnu17 -mstrict-align -mcpu=cortex-a53 \
	-mtune=cortex-a53 -target aarch64-none-elf -g3 -O2 -Wall -Werror \
	-Wno-unused-command-line-argument -I$(BOARD_DIR)/include \
	-I$(SDDF)/include -I$(SDDF)/include/microkit \
	-I$(SDDF)/include/sddf/util/custom_libc
LDFLAGS := -L$(BOARD_DIR)/lib -L$(BUILD_DIR)
LIBS := --start-group -lmicrokit -Tmicrokit.ld -lsddf_util_debug --end-group

.PHONY: all
all: $(BUILD_DIR)/input_driver.elf

$(BUILD_DIR)/input_driver.o: driver.c
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/input_driver.elf: $(BUILD_DIR)/input_driver.o $(BUILD_DIR)/libsddf_util_debug.a
	$(LD) $(LDFLAGS) -o $@ $< $(LIBS)
