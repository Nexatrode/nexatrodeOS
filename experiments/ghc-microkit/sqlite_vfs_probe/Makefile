CC ?= cc
MICROKIT_CC ?= clang
MICROKIT_SDK ?= ../../../.cache/microkit-sdk-2.3.1
MICROKIT_BOARD ?= qemu_virt_aarch64
MICROKIT_INCLUDE_FLAGS ?= -I$(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/debug/include
SQLITE_INCLUDE_DIR ?= ../../../.cache/simplex-toolchain/cabal/store/ghc-9.6.3/drct-sqlcphr-2.3.28-1bca52b5/lib/include
MICROKIT_BUILD_DIR ?= build/microkit
PKG_CONFIG ?= pkg-config
CFLAGS ?= -O2 -g
SQLITE_CFLAGS := $(shell $(PKG_CONFIG) --cflags sqlite3)
SQLITE_LIBS := $(shell $(PKG_CONFIG) --libs sqlite3)

.PHONY: test clean microkit-compile

test: vfs_test vault_storage_test
	./vfs_test
	./vault_storage_test

vfs_test: vfs.c test.c
	$(CC) -std=c11 -Wall -Wextra -Werror $(CFLAGS) $(SQLITE_CFLAGS) $^ $(SQLITE_LIBS) -o $@

vault_storage_test: vault_storage.c vault_storage_test.c
	$(CC) -std=c11 -Wall -Wextra -Werror $(CFLAGS) $(SQLITE_CFLAGS) $^ -o $@

microkit-compile:
	mkdir -p $(MICROKIT_BUILD_DIR)
	$(MICROKIT_CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Werror $(MICROKIT_INCLUDE_FLAGS) -I../freestanding-headers -I$(SQLITE_INCLUDE_DIR) \
		-c vfs.c -o $(MICROKIT_BUILD_DIR)/vfs.o
	$(MICROKIT_CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Werror $(MICROKIT_INCLUDE_FLAGS) -I../freestanding-headers -I$(SQLITE_INCLUDE_DIR) \
		-c vault_storage.c -o $(MICROKIT_BUILD_DIR)/vault_storage.o
	$(MICROKIT_CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Werror $(MICROKIT_INCLUDE_FLAGS) -I../freestanding-headers -I$(SQLITE_INCLUDE_DIR) \
		-c simplex_sqlite_init_microkit.c -o $(MICROKIT_BUILD_DIR)/simplex_sqlite_init_microkit.o
	$(MICROKIT_CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Werror $(MICROKIT_INCLUDE_FLAGS) \
		-c vault_storage_microkit.c -o $(MICROKIT_BUILD_DIR)/vault_storage_microkit.o

clean:
	rm -f vfs_test vault_storage_test
	rm -rf $(MICROKIT_BUILD_DIR)
