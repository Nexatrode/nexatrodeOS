MICROKIT_SDK ?= ../../.cache/microkit-sdk-2.3.1
MICROKIT_BOARD ?= qemu_virt_aarch64
MICROKIT_CONFIG ?= debug
BUILD_DIR ?= build
TIMER_PROBE_BUILD_DIR := $(abspath ../../build/ghc-timer-service-probe)
SCHEDULER_PROBE_BUILD_DIR := $(abspath ../../build/ghc-scheduler-wait-probe)
SDDF_SOURCE := $(abspath ../../.cache/sddf-personal-os)
MICROKIT_SDK_ABS := $(abspath $(MICROKIT_SDK))
TIMER_PROBE_TOP := $(CURDIR)/timer_probe
SDDF_PYTHON := $(abspath ../../.cache/sddf-venv/bin/python)


BOARD_DIR := $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)
CC = clang
HOST_CC ?= clang
HOST_CFLAGS ?= -O2 -g
LD = ld.lld
LLVM_AR ?= /opt/homebrew/opt/llvm/bin/llvm-ar
LLVM_NM ?= /opt/homebrew/opt/llvm/bin/llvm-nm
LLVM_OBJCOPY ?= /opt/homebrew/opt/llvm/bin/llvm-objcopy
LTC_SOURCE ?= ../../.cache/simplex-crypto/libtomcrypt
LTC_BUILD_DIR := $(BUILD_DIR)/guest-simplex-sqlcipher-crypto
LTC_ARCHIVE := $(abspath $(LTC_BUILD_DIR)/source/libtomcrypt.a)
LTC_HOST_BUILD_DIR := $(BUILD_DIR)/guest-simplex-sqlcipher-host-crypto
LTC_HOST_ARCHIVE := $(abspath $(LTC_HOST_BUILD_DIR)/source/libtomcrypt.a)
QEMU ?= qemu-system-aarch64
MICROKIT_TOOL ?= $(MICROKIT_SDK)/bin/microkit
GHC_RTS_SOURCE ?= ../../.cache/ghc-port/ghc-9.6.6
HOST_GHC ?= ../../.cache/simplex-toolchain/.ghcup/ghc/9.6.3/bin/ghc
CACHED_CABAL ?= ../../.cache/simplex-toolchain/.ghcup/cabal/3.10.3.0/cabal
HOST_TLS_PACKAGE_DB ?= ../../.cache/simplex-toolchain/cabal/store/ghc-9.6.3/package.db
GHC_CROSS ?= $(GHC_RTS_SOURCE)/_build/stage0/bin/aarch64-unknown-none-ghc
GHC_GENERATED_HEADERS := $(BUILD_DIR)/generated-rts-headers
GHC_DERIVED_CONSTANTS := $(GHC_RTS_SOURCE)/_build/stage1/rts/build/include/DerivedConstants.h
GENAPPLY_SOURCE := $(GHC_RTS_SOURCE)/utils/genapply/Main.hs
GENAPPLY_BUILD_DIR := $(BUILD_DIR)/host-tools/genapply
AUTOAPPLY_CMM := $(BUILD_DIR)/AutoApply.cmm
AUTOAPPLY_OBJECT := $(BUILD_DIR)/cmm/AutoApply.o
HADRIAN_RTS_ARCHIVE := $(GHC_RTS_SOURCE)/_build/stage1/rts/build/libHSrts-1.0.2.a
HADRIAN_RTS_RELOC := $(BUILD_DIR)/hadrian-rts-reloc.o
SDDF_INCLUDE_FLAGS := -I../../.cache/sddf/include -I../../.cache/sddf/include/microkit
CFLAGS := -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
	-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 -g \
	-Wall -Wextra -Werror -I$(BOARD_DIR)/include
LDFLAGS := -L$(BOARD_DIR)/lib
GHC_RTS_INCLUDE_FLAGS := -I$(GHC_GENERATED_HEADERS) -Ifreestanding-headers \
	-I$(GHC_RTS_SOURCE)/rts/include -I$(GHC_RTS_SOURCE)/rts
GHC_RTS_INCLUDE_FLAGS_ABS := -I$(abspath $(GHC_GENERATED_HEADERS)) \
	-I$(abspath freestanding-headers) -I$(abspath $(GHC_RTS_SOURCE)/rts/include) \
	-I$(abspath $(GHC_RTS_SOURCE)/rts)
GHC_RTS_ADAPTER_OBJECTS := $(abspath $(BUILD_DIR)/rts-adapter/AwaitEvent.o) \
	$(abspath $(BUILD_DIR)/rts-adapter/GetTime.o) \
	$(abspath $(BUILD_DIR)/rts-adapter/sddf_time_bridge.o)
OSMEM_BUILD_DIR := $(BUILD_DIR)/osmem_probe
HASKELL_PROBE_BUILD_DIR := $(abspath $(BUILD_DIR)/haskell-hello)
HASKELL_IMAGE_BUILD_DIR := $(abspath $(BUILD_DIR)/haskell-image)
SCHEDULER_PROBE_OUTPUT_DIR := $(abspath ../../build/ghc-scheduler-wait-probe)
HASKELL_LIBC_OBJECT := $(HASKELL_PROBE_BUILD_DIR)/sel4_libc.o
HASKELL_MICROKIT_ENTRY_OBJECT := $(HASKELL_PROBE_BUILD_DIR)/haskell_microkit_entry.o
HASKELL_WALLCLOCK_OBJECT := $(HASKELL_PROBE_BUILD_DIR)/wallclock_unavailable.o
HASKELL_CALENDAR_OBJECT := $(HASKELL_PROBE_BUILD_DIR)/calendar_utc.o
HASKELL_ENTROPY_OBJECT := $(HASKELL_PROBE_BUILD_DIR)/haskell_entropy_bridge.o
HASKELL_TCP_OBJECT := $(HASKELL_PROBE_BUILD_DIR)/haskell_tcp_bridge.o
HASKELL_NET_FRAGMENT_DIR := $(abspath ../../build/ghc-haskell-net-fragment)
HASKELL_NET_IMAGE_DIR := $(abspath $(BUILD_DIR)/haskell-net-image)
RUST_ENTROPY_ELF := $(abspath ../../build/rust-target/aarch64-sel4-microkit/release/rust_entropy_rng.elf)
RUST_CLOCK_ELF := $(abspath ../../build/rust-target/aarch64-sel4-microkit/release/rust_clock.elf)
RUST_VAULT_DISK_ELF := $(abspath ../../build/rust-target/aarch64-sel4-microkit/release/rust_vault_disk.elf)
HASKELL_PACKAGE_DB := $(abspath $(GHC_RTS_SOURCE)/_build/stage1/lib/package.conf.d)
GUEST_TLS_PACKAGE_DB := $(abspath ../../.cache/sel4-tls-deps/guest-package.conf.d)
GUEST_TLS_PROBE_DIR := $(abspath $(BUILD_DIR)/guest-tls-probe)
GUEST_TLS_IMAGE_DIR := $(abspath $(BUILD_DIR)/guest-tls-image)
GUEST_TLS_NET_IMAGE_DIR := $(abspath $(BUILD_DIR)/guest-tls-net-image)
GUEST_TLS_WALLCLOCK_OBJECT := $(GUEST_TLS_PROBE_DIR)/tls_qemu_wallclock.o
GUEST_TLS_CLOCK_ELF := $(GUEST_TLS_IMAGE_DIR)/tls_clock.elf
SMP_SERVER_BIN ?= ../../.cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx/ghc-9.6.3/simplexmq-7.0.0.6/x/smp-server/build/smp-server/smp-server
SIMPLEX_CHAT_BIN ?= ../../.cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx/ghc-9.6.3/simplex-chat-7.0.0.12/x/simplex-chat/build/simplex-chat/simplex-chat
SMP_FIXTURE_DIR := $(abspath ../../.cache/simplex-qemu-smp)
SIMPLEXMQ_SOURCE := $(abspath ../../.cache/simplex-chat-upstream/dist-newstyle/src/simplexmq-f058c38b0eecb6c0/src)
SQLCIPHER_AMALGAMATION := $(firstword $(wildcard ../../.cache/simplex-chat-upstream/dist-newstyle/src/direct-sq_*/cbits/sqlite3.c))
SIMPLEXMQ_SHIMS := $(CURDIR)/upstream-shims
GUEST_SMP_PROBE_DIR := $(abspath $(BUILD_DIR)/guest-smp-probe)
GUEST_SMP_PLATFORM_OBJECT := $(GUEST_SMP_PROBE_DIR)/smp_platform.o
GUEST_SMP_NET_IMAGE_DIR := $(abspath $(BUILD_DIR)/guest-smp-net-image)
AGENTSTORE_PROBE_DIR := $(abspath $(BUILD_DIR)/guest-simplex-agentstore-entry-probe)
AGENTSTORE_QEMU_IMAGE_DIR := $(AGENTSTORE_PROBE_DIR)/qemu-image
AGENTSTORE_WRONG_KEY_IMAGE_DIR := $(AGENTSTORE_PROBE_DIR)/wrong-key-image
AGENTSTORE_BASE_SYSTEM := $(AGENTSTORE_PROBE_DIR)/storage-base.system
AGENT_RUNTIME_PROBE_DIR := $(abspath $(BUILD_DIR)/guest-simplex-agent-runtime-probe)
AGENT_SERVICE_PROBE_DIR := $(abspath $(BUILD_DIR)/guest-simplex-agent-service)
SIMPLEX_AGENT_UI_PROBE_DIR := $(abspath $(BUILD_DIR)/guest-simplex-agent-ui)
GUEST_AGENT_RUNTIME_SYSTEM := $(AGENT_RUNTIME_PROBE_DIR)/network-storage-base.system
GUEST_AGENT_SERVICE_SYSTEM := $(AGENT_SERVICE_PROBE_DIR)/network-storage-base.system
AGENT_RUNTIME_QEMU_IMAGE_DIR := $(AGENT_RUNTIME_PROBE_DIR)/qemu-image
AGENT_SERVICE_QEMU_IMAGE_DIR := $(AGENT_SERVICE_PROBE_DIR)/qemu-image
SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR := $(SIMPLEX_AGENT_UI_PROBE_DIR)/qemu-image
AGENT_SERVICE_RPC_CLIENT_ELF := $(AGENT_SERVICE_PROBE_DIR)/simplex_agent_rpc_client.elf
AGENT_SERVICE_MESSAGES_ELF := $(AGENT_SERVICE_PROBE_DIR)/rust_messages_agent_app.elf
SIMPLEX_AGENT_UI_MESSAGES_ELF := $(SIMPLEX_AGENT_UI_PROBE_DIR)/rust_messages_agent_app.elf
SIMPLEX_AGENT_UI_SLINT_ELF := $(SIMPLEX_AGENT_UI_PROBE_DIR)/slint-target/aarch64-sel4-microkit/release/slint_notes_utm.elf
AGENT_SERVICE_VAULT_ELF := $(AGENT_SERVICE_PROBE_DIR)/rust_vault_disk.elf
RUST_AGENT_RPC_CLIENT_ELF := $(abspath ../../build/rust-target/aarch64-sel4-microkit/release/simplex_agent_rpc_client.elf)
AGENT_CLI_INTEROP_DIR := $(abspath $(BUILD_DIR)/guest-simplex-agent-cli-interop)
AGENT_CLI_INTEROP_QEMU_IMAGE_DIR := $(AGENT_CLI_INTEROP_DIR)/qemu-image
SIMPLEX_IPC_SIDECAR_DIR := $(abspath $(BUILD_DIR)/simplex-ipc-sidecar)
GHC_CROSS_ABS := $(abspath $(GHC_CROSS))
GHC_LD_WRAPPER := $(abspath ghc-ld-lld.py)
LLD ?= /opt/homebrew/bin/ld.lld
OPENLIBM_ARCHIVE ?= ../../.cache/ghc-port/openlibm/libopenlibm.a
PLATFORM_ALLOW_UNRESOLVED ?= 0
COMPILER_RT_BUILTINS ?= ../../.cache/ghc-port/llvm-project/compiler-rt/lib/builtins
COMPILER_RT_QUAD_DIR := $(BUILD_DIR)/compiler-rt-quad
COMPILER_RT_QUAD_ARCHIVE := $(COMPILER_RT_QUAD_DIR)/libquad-builtins.a

.PHONY: all clean osmem-adapter-probe rts-adapter-compile rts-utils-compile timer-service-probe scheduler-wait-probe autoapply-probe hadrian-rts hadrian-bytestring hadrian-tls-core guest-tls-deps guest-tls-backend-compile guest-tls-link-inventory guest-tls-qemu-image guest-tls-qemu-smoke guest-tls-net-qemu-image guest-tls-net-qemu-smoke guest-smp-fixture guest-smp-encoding-deps guest-smp-link-inventory guest-smp-net-qemu-image guest-smp-net-qemu-smoke host-simplexmq-backend-typecheck host-simplexmq-sqlite-vfs-typecheck guest-simplex-portable-deps guest-simplex-json-prereqs guest-simplex-shared-compile guest-simplex-client-inventory guest-simplex-http2-deps guest-simplex-http2-client-check guest-simplex-sqlite-core-compile guest-simplex-sqlcipher-core-compile guest-simplex-sqlcipher-crypto-compile guest-simplex-sqlcipher-link-inventory guest-simplex-sqlcipher-host-test guest-simplex-sqlcipher-rng-compile guest-simplex-sqlcipher-qemu-image guest-simplex-sqlcipher-qemu-smoke guest-simplex-sqlcipher-vault-client guest-simplex-agentstore-link guest-simplex-agentstore-qemu-image guest-simplex-agentstore-qemu-smoke guest-simplex-agent-runtime-objects guest-simplex-agent-runtime-link guest-simplex-agent-runtime-qemu-image guest-simplex-agent-runtime-qemu-smoke guest-simplex-agent-cli-interop-qemu-smoke sqlcipher-freestanding-test calendar-utc-test native-tls-backend-typecheck rts-trial-link haskell-compile-probe haskell-libc-compile haskell-microkit-entry-compile haskell-link-inventory haskell-link-gc-inventory haskell-openlibm-inventory haskell-openlibm-gc-inventory haskell-platform-inventory haskell-qemu-image haskell-qemu-run haskell-qemu-smoke haskell-net-qemu-image haskell-net-qemu-smoke
.PHONY: simplex-ipc-host-test simplex-attachment-host-test simplex-direct-host-test

simplex-ipc-host-test: simplex-attachment-host-test simplex-direct-host-test
	mkdir -p $(BUILD_DIR)/simplex-ipc-host
	$(HOST_GHC) -Wall -Werror -fforce-recomp -i. \
		-outputdir $(BUILD_DIR)/simplex-ipc-host SimplexIPCTest.hs \
		-o $(BUILD_DIR)/simplex-ipc-host/test
	$(BUILD_DIR)/simplex-ipc-host/test

simplex-direct-host-test:
	mkdir -p $(BUILD_DIR)/simplex-direct-host
	$(HOST_GHC) -Wall -Werror -i. -package-db $(HOST_TLS_PACKAGE_DB) -package aeson -package zstd \
		-outputdir $(BUILD_DIR)/simplex-direct-host SimplexDirectTest.hs \
		-o $(BUILD_DIR)/simplex-direct-host/test
	$(BUILD_DIR)/simplex-direct-host/test

simplex-attachment-host-test:
	mkdir -p $(BUILD_DIR)/simplex-attachment-host
	$(HOST_GHC) -Wall -Werror -i. -package-db $(HOST_TLS_PACKAGE_DB) \
		-package aeson -package sqlcipher-simple \
		-outputdir $(BUILD_DIR)/simplex-attachment-host SimplexAttachmentOfferTest.hs \
		-o $(BUILD_DIR)/simplex-attachment-host/test
	$(BUILD_DIR)/simplex-attachment-host/test

all: $(BUILD_DIR)/loader.img

osmem-adapter-probe: $(OSMEM_BUILD_DIR)/loader.img

$(BUILD_DIR):
	mkdir -p $@

$(BUILD_DIR)/microkit_runtime_probe.o: microkit_runtime_probe.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -Ifreestanding-headers -c $< -o $@

$(BUILD_DIR)/mblock_arena.o: mblock_arena.c mblock_arena.h | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/sel4_libc.o: sel4_libc.c sel4_libc.h | $(BUILD_DIR)
	$(CC) $(CFLAGS) -Ifreestanding-headers -c $< -o $@

$(BUILD_DIR)/microkit_runtime_probe.elf: $(BUILD_DIR)/microkit_runtime_probe.o \
		$(BUILD_DIR)/mblock_arena.o $(BUILD_DIR)/sel4_libc.o
	$(LD) $(LDFLAGS) -o $@ $^ --start-group -lmicrokit \
		-T$(BOARD_DIR)/lib/microkit.ld --end-group

$(BUILD_DIR)/capability_service.o: capability_service.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/capability_service.elf: $(BUILD_DIR)/capability_service.o
	$(LD) $(LDFLAGS) -o $@ $< --start-group -lmicrokit \
		-T$(BOARD_DIR)/lib/microkit.ld --end-group

$(BUILD_DIR)/loader.img: $(BUILD_DIR)/microkit_runtime_probe.elf \
		$(BUILD_DIR)/capability_service.elf microkit_runtime_probe.system
	$(MICROKIT_TOOL) microkit_runtime_probe.system --search-path $(BUILD_DIR) \
		--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) -o $@ \
		-r $(BUILD_DIR)/report.txt

$(OSMEM_BUILD_DIR):
	mkdir -p $@

$(GHC_GENERATED_HEADERS)/ghcplatform.h: \
		generate-ghc-headers.py $(GHC_RTS_SOURCE)/mk/config.h \
		$(GHC_RTS_SOURCE)/hadrian/cfg/system.config
	python3 generate-ghc-headers.py $(GHC_RTS_SOURCE) $(GHC_GENERATED_HEADERS)

$(GHC_GENERATED_HEADERS)/ghcautoconf.h: $(GHC_GENERATED_HEADERS)/ghcplatform.h
	@if [ ! -f $@ ]; then \
		python3 generate-ghc-headers.py $(GHC_RTS_SOURCE) $(GHC_GENERATED_HEADERS); \
	fi

$(GHC_GENERATED_HEADERS)/rts/EventLogConstants.h \
$(GHC_GENERATED_HEADERS)/rts/EventTypes.h: $(GHC_GENERATED_HEADERS)/ghcplatform.h
	@if [ ! -f $@ ]; then \
		python3 generate-ghc-headers.py $(GHC_RTS_SOURCE) $(GHC_GENERATED_HEADERS); \
	fi

$(BUILD_DIR)/rts-adapter/%.o: $(GHC_RTS_SOURCE)/rts/sel4/%.c \
		$(GHC_GENERATED_HEADERS)/ghcplatform.h \
		$(GHC_GENERATED_HEADERS)/ghcautoconf.h
	mkdir -p $(BUILD_DIR)/rts-adapter
	$(CC) $(CFLAGS) $(GHC_RTS_INCLUDE_FLAGS) -c $< -o $@

rts-adapter-compile: $(BUILD_DIR)/rts-adapter/GetTime.o \
		$(BUILD_DIR)/rts-adapter/AwaitEvent.o \
		$(BUILD_DIR)/rts-adapter/sddf_time_bridge.o

# Compile one unmodified core RTS source against the target-generated headers.
# Hadrian supplies these build identity strings in its compiler definitions.
GHC_RTS_BUILD_INFO_FLAGS := \
	-DProjectVersion=\"9.6.6\" -DRtsWay=\"\" \
	-DBuildPlatform=\"aarch64-apple-darwin\" -DBuildArch=\"aarch64\" \
	-DBuildOS=\"darwin\" -DBuildVendor=\"apple\" \
	-DHostPlatform=\"aarch64-unknown-sel4\" -DHostArch=\"aarch64\" \
	-DHostOS=\"sel4\" -DHostVendor=\"unknown\" \
	-DTargetPlatform=\"aarch64-unknown-sel4\" -DTargetArch=\"aarch64\" \
	-DTargetOS=\"sel4\" -DTargetVendor=\"unknown\" \
	-DGhcUnregisterised=\"NO\" -DTablesNextToCode=\"NO\"

rts-utils-compile: $(GHC_GENERATED_HEADERS)/ghcplatform.h \
		$(GHC_GENERATED_HEADERS)/ghcautoconf.h
	mkdir -p $(BUILD_DIR)/rts-core
	$(CC) $(CFLAGS) $(GHC_RTS_INCLUDE_FLAGS) $(GHC_RTS_BUILD_INFO_FLAGS) \
		-c $(GHC_RTS_SOURCE)/rts/RtsUtils.c -o $(BUILD_DIR)/rts-core/RtsUtils.o

# Generate GHC's missing RTS application closures with the cached host GHC,
# then compile them through the configured seL4 cross-compiler.
$(GENAPPLY_BUILD_DIR)/genapply: $(GENAPPLY_SOURCE)
	mkdir -p $(GENAPPLY_BUILD_DIR)
	$(HOST_GHC) --make -O0 -outputdir $(GENAPPLY_BUILD_DIR) -o $@ $<

$(AUTOAPPLY_CMM): $(GENAPPLY_BUILD_DIR)/genapply $(GHC_DERIVED_CONSTANTS)
	$< $(GHC_DERIVED_CONSTANTS) > $@

$(AUTOAPPLY_OBJECT): $(AUTOAPPLY_CMM) target-clang.sh
	mkdir -p $(dir $@)
	$(GHC_CROSS) -c $< -odir $(dir $@) -hidir $(dir $@) \
		-I$(GHC_RTS_SOURCE)/rts -I$(GHC_RTS_SOURCE)/rts/include \
		-I$(GHC_RTS_SOURCE)/_build/stage1/rts/build/include \
		-pgma ./target-clang.sh -o $@

autoapply-probe: $(AUTOAPPLY_OBJECT)

hadrian-rts:
	cd $(GHC_RTS_SOURCE) && GHC=$(abspath $(HOST_GHC)) \
		CABAL=$(abspath $(CACHED_CABAL)) \
		hadrian/build --flavour=quickest+native_bignum stage1:lib:rts

hadrian-bytestring:
	cd $(GHC_RTS_SOURCE) && GHC=$(abspath $(HOST_GHC)) \
		CABAL=$(abspath $(CACHED_CABAL)) \
		hadrian/build --flavour=quickest+native_bignum stage1:lib:bytestring

hadrian-tls-core:
	cd $(GHC_RTS_SOURCE) && GHC=$(abspath $(HOST_GHC)) \
		CABAL=$(abspath $(CACHED_CABAL)) \
		hadrian/build --flavour=quickest+native_bignum \
		stage1:lib:mtl stage1:lib:containers stage1:lib:stm \
		stage1:lib:filepath stage1:lib:text stage1:lib:binary stage1:lib:time

guest-tls-deps: hadrian-bytestring hadrian-tls-core
	python3 cross-build-tls-dep.py data-default-class
	python3 cross-build-tls-dep.py cereal
	python3 cross-build-tls-dep.py basement
	python3 cross-build-tls-dep.py memory
	python3 cross-build-tls-dep.py crypton
	python3 cross-build-tls-dep.py hourglass
	python3 cross-build-tls-dep.py pem
	python3 cross-build-tls-dep.py asn1-types
	python3 cross-build-tls-dep.py asn1-encoding
	python3 cross-build-tls-dep.py asn1-parse
	python3 cross-build-tls-dep.py crypton-x509
	python3 cross-build-tls-dep.py crypton-x509-store
	python3 cross-build-tls-dep.py crypton-x509-validation
	python3 cross-build-tls-dep.py hashable
	python3 cross-build-tls-dep.py async
	python3 cross-build-tls-dep.py tls

guest-tls-backend-compile: guest-tls-deps
	mkdir -p $(GUEST_TLS_PROBE_DIR)
	$(GHC_CROSS) -fforce-recomp -no-global-package-db -package-env - \
		-package-db $(HASKELL_PACKAGE_DB) -package-db $(GUEST_TLS_PACKAGE_DB) \
		-package tls -c NativeNetwork.hs -odir $(GUEST_TLS_PROBE_DIR) \
		-hidir $(GUEST_TLS_PROBE_DIR)
	$(GHC_CROSS) -fforce-recomp -no-global-package-db -package-env - \
		-package-db $(HASKELL_PACKAGE_DB) -package-db $(GUEST_TLS_PACKAGE_DB) \
		-package tls -i$(GUEST_TLS_PROBE_DIR) -c NativeTLSBackend.hs \
		-odir $(GUEST_TLS_PROBE_DIR) -hidir $(GUEST_TLS_PROBE_DIR)

# Diagnose static-link imports without changing the ordinary QEMU image. The
# linker permits imports so nm can report them, but the target rejects any.
guest-tls-link-inventory: guest-tls-backend-compile haskell-libc-compile \
		haskell-microkit-entry-compile rts-adapter-compile \
		$(OPENLIBM_ARCHIVE) $(COMPILER_RT_QUAD_ARCHIVE) \
		$(GUEST_TLS_PROBE_DIR)/TLSFixtureCA.hs \
		$(GUEST_TLS_WALLCLOCK_OBJECT) $(HASKELL_CALENDAR_OBJECT) \
		$(HASKELL_ENTROPY_OBJECT) $(HASKELL_TCP_OBJECT)
	cd $(GHC_RTS_SOURCE) && LLD=$(LLD) GHC_LLD_ALLOW_UNRESOLVED=1 \
		GHC_LLD_EXTRA_ARGS="-L$(abspath $(BOARD_DIR)/lib) -T$(abspath $(BOARD_DIR)/lib)/microkit.ld" \
		$(GHC_CROSS_ABS) -v0 -fforce-recomp -no-hs-main \
		-i$(GUEST_TLS_PROBE_DIR) \
		$(abspath NativeNetwork.hs) $(abspath NativeTLSBackend.hs) \
		$(abspath NativeSMPIdentity.hs) \
		$(GUEST_TLS_PROBE_DIR)/TLSFixtureCA.hs $(abspath TLSLinkProbe.hs) \
		$(HASKELL_MICROKIT_ENTRY_OBJECT) $(HASKELL_LIBC_OBJECT) \
		$(GUEST_TLS_WALLCLOCK_OBJECT) $(HASKELL_CALENDAR_OBJECT) \
		$(HASKELL_ENTROPY_OBJECT) $(HASKELL_TCP_OBJECT) \
		$(abspath $(OPENLIBM_ARCHIVE)) $(abspath $(COMPILER_RT_QUAD_ARCHIVE)) \
		$(abspath $(BUILD_DIR)/rts-adapter/sddf_time_bridge.o) \
		$(abspath $(BOARD_DIR)/lib/libmicrokit.a) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) \
		-package-db $(GUEST_TLS_PACKAGE_DB) -package tls -package crypton \
		-package hourglass -package crypton-x509 -package crypton-x509-store \
		-package crypton-x509-validation \
		-pgml $(GHC_LD_WRAPPER) \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,init -o $(GUEST_TLS_PROBE_DIR)/tls-link-inventory.elf \
		> $(GUEST_TLS_PROBE_DIR)/link.log 2>&1
	$(LLVM_NM) -u $(GUEST_TLS_PROBE_DIR)/tls-link-inventory.elf | sort -u \
		> $(GUEST_TLS_PROBE_DIR)/unresolved.txt
	@printf "TLS link unresolved imports: "
	@wc -l < $(GUEST_TLS_PROBE_DIR)/unresolved.txt
	@test ! -s $(GUEST_TLS_PROBE_DIR)/unresolved.txt

# Package the TLS crypto/backend smoke separately from the normal Haskell image.
# Its custom backend is exercised only with a zero-length receive and flush.
guest-tls-qemu-image: guest-tls-link-inventory haskell-qemu-image $(GUEST_TLS_CLOCK_ELF)
	mkdir -p $(GUEST_TLS_IMAGE_DIR)
	cp haskell_probe/tls_qemu.system $(GUEST_TLS_IMAGE_DIR)/haskell.system
	cp \
		$(HASKELL_IMAGE_BUILD_DIR)/timer_driver.elf \
		$(HASKELL_IMAGE_BUILD_DIR)/rust_entropy_rng.elf $(GUEST_TLS_IMAGE_DIR)/
	$(LLVM_OBJCOPY) --update-section .timer_client_config=$(SCHEDULER_PROBE_OUTPUT_DIR)/timer_client_client.data \
		$(GUEST_TLS_PROBE_DIR)/tls-link-inventory.elf $(GUEST_TLS_IMAGE_DIR)/client.elf
	$(MICROKIT_TOOL) $(GUEST_TLS_IMAGE_DIR)/haskell.system \
		--search-path $(GUEST_TLS_IMAGE_DIR) --board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) -o $(GUEST_TLS_IMAGE_DIR)/loader.img \
		-r $(GUEST_TLS_IMAGE_DIR)/report.txt

guest-tls-qemu-smoke: guest-tls-qemu-image
	python3 tls_qemu_smoke.py $(GUEST_TLS_IMAGE_DIR)/loader.img $(QEMU)

guest-tls-net-qemu-image: guest-tls-qemu-image $(HASKELL_NET_FRAGMENT_DIR)/net.system
	mkdir -p $(GUEST_TLS_NET_IMAGE_DIR)
	cp $(GUEST_TLS_IMAGE_DIR)/client.elf $(GUEST_TLS_IMAGE_DIR)/timer_driver.elf \
		$(GUEST_TLS_IMAGE_DIR)/rust_entropy_rng.elf \
		$(GUEST_TLS_IMAGE_DIR)/tls_clock.elf $(GUEST_TLS_NET_IMAGE_DIR)/
	cp $(HASKELL_NET_FRAGMENT_DIR)/eth_driver.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/dhcp_client.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_rx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_tx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_copy.elf $(GUEST_TLS_NET_IMAGE_DIR)/
	python3 ../../tests/merge_network_system.py --haskell-tcp \
		--base $(GUEST_TLS_IMAGE_DIR)/haskell.system \
		--network $(HASKELL_NET_FRAGMENT_DIR)/net.system \
		--output $(GUEST_TLS_NET_IMAGE_DIR)/haskell-tls-net.system \
		--elf-dir $(GUEST_TLS_NET_IMAGE_DIR)
	$(MICROKIT_TOOL) $(GUEST_TLS_NET_IMAGE_DIR)/haskell-tls-net.system \
		--search-path $(GUEST_TLS_NET_IMAGE_DIR) --board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) -o $(GUEST_TLS_NET_IMAGE_DIR)/loader.img \
		-r $(GUEST_TLS_NET_IMAGE_DIR)/report.txt

guest-tls-net-qemu-smoke: guest-tls-net-qemu-image
	python3 tls_net_qemu_smoke.py $(GUEST_TLS_NET_IMAGE_DIR)/loader.img $(QEMU)
	python3 tls_net_qemu_smoke.py $(GUEST_TLS_NET_IMAGE_DIR)/loader.img $(QEMU) --expired

guest-smp-fixture:
	python3 prepare_smp_qemu_fixture.py $(SMP_SERVER_BIN) $(SMP_FIXTURE_DIR)

host-simplexmq-backend-typecheck:
	python3 verify_simplexmq_backend_patch.py

host-simplexmq-sqlite-vfs-typecheck:
	python3 verify_simplexmq_sqlite_vfs_patch.py

guest-simplex-sqlite-core-compile:
	@test -n "$(SQLCIPHER_AMALGAMATION)" || { echo "pinned SQLCipher amalgamation is not present in the SimpleX source cache" >&2; exit 1; }
	mkdir -p $(BUILD_DIR)/guest-simplex-sqlite-core
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Wno-unused-parameter -Wno-unused-but-set-variable \
		-Wno-null-pointer-subtraction -DNDEBUG -DSQLITE_OS_OTHER=1 \
		-DSQLITE_THREADSAFE=0 -DSQLITE_TEMP_STORE=2 \
		-DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_WAL \
		-Ifreestanding-headers \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) -c $(SQLCIPHER_AMALGAMATION) \
		-o $(BUILD_DIR)/guest-simplex-sqlite-core/sqlite3.o
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Werror -Ifreestanding-headers \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlite_vfs_probe/sqlite_os_other.c \
		-o $(BUILD_DIR)/guest-simplex-sqlite-core/sqlite_os_other.o
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Werror -Ifreestanding-headers \
		-c calendar_utc.c \
		-o $(BUILD_DIR)/guest-simplex-sqlite-core/calendar_utc.o

# Compile the same pinned amalgamation with SQLCipher's LibTomCrypt provider.
# This is a codec-object compile milestone; linking the provider and exercising
# encrypted DB create/reopen still require the target LTC archive and entropy
# backed named VFS integration.
guest-simplex-sqlcipher-core-compile:
	@test -n "$(SQLCIPHER_AMALGAMATION)" || { echo "pinned SQLCipher amalgamation is not present in the SimpleX source cache" >&2; exit 1; }
	mkdir -p $(BUILD_DIR)/guest-simplex-sqlcipher-core
	python3 prepare_sqlcipher_entropy.py $(SQLCIPHER_AMALGAMATION) \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite3.c
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Wno-unused-parameter -Wno-unused-but-set-variable \
		-Wno-null-pointer-subtraction -Wno-sign-compare -Wno-unused-variable \
		-DNDEBUG -DSQLITE_OS_OTHER=1 -DSQLITE_THREADSAFE=0 -DSQLITE_TEMP_STORE=2 \
		-DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_AUTOINIT \
		-DSQLITE_OMIT_WAL -DSQLITE_OMIT_TRACE \
		-DSQLITE_OMIT_LOCALTIME -DSQLITE_HAS_CODEC \
		-DSQLCIPHER_CRYPTO_LIBTOMCRYPT -DSQLCIPHER_OMIT_LOG \
		-DLTC_NO_FILE -DLTC_NO_TEST -DLTC_SOURCE -DOMIT_MEMLOCK \
		-include sqlcipher_freestanding.h -Ifreestanding-headers \
		-I../../.cache/simplex-crypto/libtomcrypt/src/headers \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c $(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite3.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite3.o
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
		-ffreestanding -fno-builtin -fno-stack-protector -std=gnu11 -O2 \
		-Wall -Wextra -Werror -Ifreestanding-headers \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlcipher_freestanding.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlcipher_freestanding.o

guest-simplex-sqlcipher-crypto-compile: $(LTC_ARCHIVE)
	@test "$$(git -C $(LTC_SOURCE) rev-parse 'v1.18.2^{commit}')" = \
		7e7eb695d581782f04b24dc444cbfde86af59853 || { echo "unexpected LibTomCrypt source revision" >&2; exit 1; }
	$(LLVM_NM) --defined-only $(LTC_ARCHIVE) | rg -q ' T cbc_start$$'
	$(LLVM_NM) --defined-only $(LTC_ARCHIVE) | rg -q ' T pkcs_5_alg2$$'
	$(LLVM_NM) --defined-only $(LTC_ARCHIVE) | rg -q ' R fortuna_desc$$'
	@echo "SQLCIPHER_LTC_AARCH64|PASS|$$(du -h $(LTC_ARCHIVE) | cut -f1)"

$(LTC_ARCHIVE):
	@test "$$(git -C $(LTC_SOURCE) rev-parse 'v1.18.2^{commit}')" = \
		7e7eb695d581782f04b24dc444cbfde86af59853 || { echo "unexpected LibTomCrypt source revision" >&2; exit 1; }
	mkdir -p $(LTC_BUILD_DIR)/source
	git -C $(LTC_SOURCE) archive v1.18.2 | tar -x -C $(LTC_BUILD_DIR)/source
	$(MAKE) -C $(LTC_BUILD_DIR)/source clean
	$(MAKE) -C $(LTC_BUILD_DIR)/source -j8 library \
		CC=$(CC) AR=$(LLVM_AR) RANLIB=$(LLVM_AR:llvm-ar=llvm-ranlib) LTC_SMALL=1 \
		CFLAGS='-target aarch64-none-elf -mcpu=cortex-a53 -mstrict-align \
			-ffreestanding -fno-builtin -fno-stack-protector \
			-DLTC_NOTHING -DLTC_RIJNDAEL -DLTC_CBC_MODE \
			-DLTC_SHA1 -DLTC_SHA256 -DLTC_SHA512 -DLTC_HASH_HELPERS -DLTC_HMAC \
			-DLTC_FORTUNA -DLTC_PKCS_5 -DLTC_NO_FILE -DLTC_NO_TEST \
			-DLTC_NO_ASM -I$(abspath freestanding-headers)'

guest-simplex-sqlcipher-link-inventory: guest-simplex-sqlcipher-core-compile guest-simplex-sqlcipher-crypto-compile guest-simplex-sqlite-vfs-compile
	$(CC) $(CFLAGS) -Ifreestanding-headers -I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlite_vfs_probe/sqlite_os_other.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite_os_other.o
	$(LD) -r -o $(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlcipher-vfs-partial.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite3.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlcipher_freestanding.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite_os_other.o \
		sqlite_vfs_probe/build/microkit/vfs.o \
		sqlite_vfs_probe/build/microkit/vault_storage.o $(LTC_ARCHIVE)
	$(LLVM_NM) -u $(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlcipher-vfs-partial.o \
		> $(BUILD_DIR)/guest-simplex-sqlcipher-core/undefined.txt
	! rg -n ' (cbc_|fortuna_|pkcs_5_|rijndael_desc|sha(1|256|512)_desc|sqlite3_secure_randomness)' \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/undefined.txt
	@echo "SQLCIPHER_CRYPTO_LINK|PASS|remaining imports recorded in $(BUILD_DIR)/guest-simplex-sqlcipher-core/undefined.txt"

guest-simplex-sqlcipher-host-test:
	@test "$$(git -C $(LTC_SOURCE) rev-parse 'v1.18.2^{commit}')" = \
		7e7eb695d581782f04b24dc444cbfde86af59853 || { echo "unexpected LibTomCrypt source revision" >&2; exit 1; }
	mkdir -p $(LTC_HOST_BUILD_DIR)/source $(BUILD_DIR)/guest-simplex-sqlcipher-host
	git -C $(LTC_SOURCE) archive v1.18.2 | tar -x -C $(LTC_HOST_BUILD_DIR)/source
	$(MAKE) -C $(LTC_HOST_BUILD_DIR)/source clean
	$(MAKE) -C $(LTC_HOST_BUILD_DIR)/source -j8 library \
		CC=$(HOST_CC) AR=$(LLVM_AR) RANLIB=$(LLVM_AR:llvm-ar=llvm-ranlib) LTC_SMALL=1 \
		CFLAGS='-DLTC_NOTHING -DLTC_RIJNDAEL -DLTC_CBC_MODE \
			-DLTC_SHA1 -DLTC_SHA256 -DLTC_SHA512 -DLTC_HASH_HELPERS -DLTC_HMAC \
			-DLTC_FORTUNA -DLTC_PKCS_5 -DLTC_NO_FILE -DLTC_NO_TEST -DLTC_NO_ASM'
	python3 prepare_sqlcipher_entropy.py $(SQLCIPHER_AMALGAMATION) \
		$(BUILD_DIR)/guest-simplex-sqlcipher-host/sqlite3.c
	$(HOST_CC) -std=c11 -O2 -DNDEBUG -DSQLITE_OS_OTHER=1 -DSQLITE_THREADSAFE=0 \
		-DSQLITE_TEMP_STORE=2 -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_AUTOINIT \
		-DSQLITE_OMIT_WAL \
		-DSQLITE_OMIT_TRACE -DSQLITE_OMIT_LOCALTIME -DSQLITE_HAS_CODEC \
		-DSQLCIPHER_CRYPTO_LIBTOMCRYPT -DSQLCIPHER_OMIT_LOG \
		-DLTC_NO_FILE -DLTC_NO_TEST -DLTC_SOURCE -DOMIT_MEMLOCK \
		-include sqlcipher_freestanding.h -I../../.cache/simplex-crypto/libtomcrypt/src/headers \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c $(BUILD_DIR)/guest-simplex-sqlcipher-host/sqlite3.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-host/sqlite3.o
	$(HOST_CC) -std=c11 -Wall -Wextra -Werror $(HOST_CFLAGS) \
		-DNEXATRODE_TEST_SQLCIPHER -DSQLITE_HAS_CODEC \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		sqlite_vfs_probe/vfs.c sqlite_vfs_probe/test.c \
		sqlite_vfs_probe/sqlite_os_other.c sqlcipher_freestanding.c \
		$(BUILD_DIR)/guest-simplex-sqlcipher-host/sqlite3.o \
		$(LTC_HOST_ARCHIVE) -lm \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-host/sqlcipher_vfs_test
	$(BUILD_DIR)/guest-simplex-sqlcipher-host/sqlcipher_vfs_test

# Run SQLCipher itself as an AArch64 Microkit PD. The storage and entropy
# callbacks are deterministic in-memory fixtures, so this proves target codec
# execution only; they are deliberately not suitable for production use.
guest-simplex-sqlcipher-rng-compile:
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir ../build/rust-target --bin rust_entropy_rng
	@test -s $(RUST_ENTROPY_ELF)

guest-simplex-sqlcipher-qemu-image: guest-simplex-sqlcipher-core-compile guest-simplex-sqlcipher-crypto-compile guest-simplex-sqlcipher-rng-compile $(COMPILER_RT_QUAD_ARCHIVE)
	mkdir -p $(BUILD_DIR)/guest-simplex-sqlcipher-qemu
	cp $(RUST_ENTROPY_ELF) $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/rust_entropy_rng.elf
	$(CC) $(CFLAGS) -DSQLITE_HAS_CODEC -Ifreestanding-headers -I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlite_vfs_probe/sqlcipher_qemu_probe.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/probe.o
	$(CC) $(CFLAGS) -Ifreestanding-headers -I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlite_vfs_probe/vfs.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/vfs.o
	$(CC) $(CFLAGS) -Ifreestanding-headers -I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlite_vfs_probe/sqlite_os_other.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/sqlite_os_other.o
	$(CC) $(CFLAGS) -Ifreestanding-headers -I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-include sqlcipher_freestanding.h \
		-c sqlcipher_freestanding.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/sqlcipher_freestanding.o
	$(CC) $(CFLAGS) -Ifreestanding-headers -c sel4_libc.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/sel4_libc.o
	cp sqlite_vfs_probe/sqlcipher_qemu.system $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/system.xml
	$(LD) $(LDFLAGS) -o $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/sqlcipher_qemu_probe.elf \
		$(BUILD_DIR)/guest-simplex-sqlcipher-qemu/probe.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-qemu/vfs.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-qemu/sqlite_os_other.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-qemu/sqlcipher_freestanding.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-qemu/sel4_libc.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite3.o \
		$(LTC_ARCHIVE) $(COMPILER_RT_QUAD_ARCHIVE) \
		--start-group -lmicrokit -T$(BOARD_DIR)/lib/microkit.ld --end-group
	$(MICROKIT_TOOL) $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/system.xml \
		--search-path $(BUILD_DIR)/guest-simplex-sqlcipher-qemu \
		--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/loader.img \
		-r $(BUILD_DIR)/guest-simplex-sqlcipher-qemu/report.txt

guest-simplex-sqlcipher-qemu-smoke: guest-simplex-sqlcipher-qemu-image
	python3 sqlcipher_qemu_smoke.py \
		$(BUILD_DIR)/guest-simplex-sqlcipher-qemu/loader.img $(QEMU)

# Link a SQLCipher PD that uses the same disk vault and entropy capability
# routes as the OS storage system. dev test-sqlcipher-vault assembles the full
# sDDF system and verifies persistence across two QEMU boots.
guest-simplex-sqlcipher-vault-client: guest-simplex-sqlcipher-core-compile guest-simplex-sqlcipher-crypto-compile $(COMPILER_RT_QUAD_ARCHIVE)
	$(MAKE) -C sqlite_vfs_probe microkit-compile \
		MICROKIT_SDK=$(abspath $(MICROKIT_SDK)) \
		MICROKIT_INCLUDE_FLAGS=-I$(abspath $(BOARD_DIR)/include) \
		SQLITE_INCLUDE_DIR=$(abspath $(dir $(SQLCIPHER_AMALGAMATION))) \
		MICROKIT_BUILD_DIR=$(abspath sqlite_vfs_probe/build/microkit)
	mkdir -p $(BUILD_DIR)/guest-simplex-sqlcipher-vault
	$(CC) $(CFLAGS) -DSQLITE_HAS_CODEC -Ifreestanding-headers \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlite_vfs_probe/sqlcipher_vault_qemu_probe.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-vault/probe.o
	$(CC) $(CFLAGS) -Ifreestanding-headers -c sel4_libc.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-vault/sel4_libc.o
	$(CC) $(CFLAGS) -Ifreestanding-headers \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		-c sqlite_vfs_probe/sqlite_os_other.c \
		-o $(BUILD_DIR)/guest-simplex-sqlcipher-vault/sqlite_os_other.o
	$(LD) $(LDFLAGS) -o $(BUILD_DIR)/guest-simplex-sqlcipher-vault/sqlcipher_vault_probe.elf \
		$(BUILD_DIR)/guest-simplex-sqlcipher-vault/probe.o \
		sqlite_vfs_probe/build/microkit/vfs.o \
		sqlite_vfs_probe/build/microkit/vault_storage.o \
		sqlite_vfs_probe/build/microkit/vault_storage_microkit.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-vault/sqlite_os_other.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlcipher_freestanding.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-vault/sel4_libc.o \
		$(BUILD_DIR)/guest-simplex-sqlcipher-core/sqlite3.o \
		$(LTC_ARCHIVE) $(COMPILER_RT_QUAD_ARCHIVE) \
		--start-group -lmicrokit -T$(BOARD_DIR)/lib/microkit.ld --end-group

sqlcipher-freestanding-test:
	mkdir -p $(BUILD_DIR)
	$(HOST_CC) -std=c11 -Wall -Wextra -Werror $(HOST_CFLAGS) \
		$$(pkg-config --cflags sqlite3) \
		-I$(dir $(SQLCIPHER_AMALGAMATION)) \
		sqlcipher_freestanding.c sqlcipher_freestanding_test.c \
		$$(pkg-config --libs sqlite3) -o $(BUILD_DIR)/sqlcipher_freestanding_test
	$(BUILD_DIR)/sqlcipher_freestanding_test

guest-simplex-sqlite-vfs-compile:
	mkdir -p $(BUILD_DIR)/guest-simplex-sqlite-vfs
	$(GHC_CROSS) -fforce-recomp -no-global-package-db -package-env - \
		-package-db $(HASKELL_PACKAGE_DB) -c NativeSQLiteVFS.hs \
		-odir $(BUILD_DIR)/guest-simplex-sqlite-vfs \
		-hidir $(BUILD_DIR)/guest-simplex-sqlite-vfs
	$(MAKE) -C sqlite_vfs_probe microkit-compile

guest-simplex-portable-deps:
	python3 cross-build-tls-dep.py unliftio-core
	python3 cross-build-tls-dep.py safe-exceptions

guest-simplex-json-prereqs: guest-smp-encoding-deps
	python3 cross-build-tls-dep.py vector-stream
	python3 cross-build-tls-dep.py vector
	python3 cross-build-tls-dep.py unordered-containers
	python3 cross-build-tls-dep.py data-fix
	python3 cross-build-tls-dep.py dlist
	python3 cross-build-tls-dep.py generically
	python3 cross-build-tls-dep.py OneTuple
	python3 cross-build-tls-dep.py integer-conversion
	python3 cross-build-tls-dep.py th-abstraction
	python3 cross-build-tls-dep.py indexed-traversable
	python3 cross-build-tls-dep.py tagged
	python3 cross-build-tls-dep.py base-orphans
	python3 cross-build-tls-dep.py time-compat
	python3 cross-build-tls-dep.py text-iso8601
	python3 cross-build-tls-dep.py text-short
	python3 cross-build-tls-dep.py assoc
	python3 cross-build-tls-dep.py indexed-traversable-instances
	python3 cross-build-tls-dep.py distributive
	python3 cross-build-tls-dep.py transformers-compat
	python3 cross-build-tls-dep.py comonad
	python3 cross-build-tls-dep.py bifunctors
	python3 cross-build-tls-dep.py StateVar
	python3 cross-build-tls-dep.py contravariant
	python3 cross-build-tls-dep.py semigroupoids
	python3 cross-build-tls-dep.py these
	python3 cross-build-tls-dep.py semialign
	python3 cross-build-tls-dep.py strict
	python3 cross-build-tls-dep.py witherable
	python3 cross-build-tls-dep.py th-compat
	python3 cross-build-tls-dep.py parsec
	python3 cross-build-tls-dep.py network-uri
	python3 cross-build-tls-dep.py splitmix
	python3 cross-build-tls-dep.py random
	python3 cross-build-tls-dep.py QuickCheck
	python3 cross-build-tls-dep.py uuid-types
	python3 cross-build-tls-dep.py aeson
	python3 cross-build-tls-dep.py base64-bytestring
	python3 cross-build-tls-dep.py iso8601-time

guest-simplex-shared-compile:
	python3 compile_simplexmq_guest_shared.py

guest-simplex-client-inventory: guest-simplex-portable-deps guest-simplex-json-prereqs
	python3 inventory_simplexmq_guest_client.py

guest-simplex-sqlcipher-haskell-deps:
	python3 cross-build-tls-dep.py Only
	python3 cross-build-tls-dep.py blaze-builder
	python3 cross-build-tls-dep.py old-locale
	python3 cross-build-tls-dep.py blaze-textual

guest-simplex-sqlcipher-haskell-bindings: guest-simplex-sqlcipher-haskell-deps
	python3 compile_simplexmq_guest_sqlcipher_bindings.py

guest-simplex-agentstore-db-check: guest-simplex-sqlcipher-haskell-bindings
	python3 typecheck_simplexmq_guest_agentstore_db.py

guest-simplex-agentstore-migrations-check: guest-simplex-agentstore-db-check
	python3 typecheck_simplexmq_guest_migrations.py

guest-simplex-agent-protocol-check: guest-simplex-agentstore-migrations-check guest-simplex-json-prereqs guest-simplex-portable-deps
	python3 compile_simplexmq_guest_agent_protocol.py

guest-simplex-agentstore-entry-check: guest-simplex-agent-protocol-check guest-simplex-http2-client-check
	python3 compile_simplexmq_agentstore_probe.py

guest-simplex-agentstore-entry-objects: guest-simplex-agent-protocol-check guest-simplex-http2-client-check
	python3 compile_simplexmq_agentstore_probe.py --objects

guest-simplex-http2-deps: guest-simplex-json-prereqs guest-simplex-portable-deps
	python3 cross-build-tls-dep.py network-byte-order
	python3 cross-build-tls-dep.py psqueues
	python3 cross-build-tls-dep.py auto-update
	python3 cross-build-tls-dep.py time-manager

guest-simplex-http2-client-check: guest-simplex-http2-deps
	python3 compile_simplexmq_guest_http2.py

.PHONY: guest-simplex-sqlcipher-haskell-deps guest-simplex-sqlcipher-haskell-bindings guest-simplex-agentstore-db-check guest-simplex-agentstore-migrations-check guest-simplex-agent-protocol-check guest-simplex-agentstore-entry-check guest-simplex-agentstore-entry-objects guest-simplex-http2-deps guest-simplex-http2-client-check

# End-to-end AgentStore probe. QEMU uses a disposable copy of the test disk,
# an explicit fixed clock fixture, the same-key reopen, and a wrong-key image.
guest-simplex-agentstore-link: guest-simplex-agentstore-entry-objects \
		guest-simplex-sqlcipher-vault-client haskell-qemu-image
	python3 link_simplexmq_agentstore_probe.py
	python3 link_simplexmq_agentstore_probe.py --wrong-key

$(AGENTSTORE_BASE_SYSTEM): ../../system.system ../../build/storage-test/storage.system ../../tests/merge_storage_system.py
	python3 ../../tests/merge_storage_system.py \
		--base ../../system.system --storage ../../build/storage-test/storage.system \
		--output $@

guest-simplex-agentstore-qemu-image: guest-simplex-agentstore-link $(AGENTSTORE_BASE_SYSTEM)
	python3 package_simplex_agentstore_probe.py \
		--base-system $(AGENTSTORE_BASE_SYSTEM) \
		--agent-elf $(AGENTSTORE_PROBE_DIR)/probe.testclock.elf \
		--timer-elf ../../build/ghc-scheduler-wait-probe/timer_driver.elf \
		--output $(AGENTSTORE_QEMU_IMAGE_DIR) --microkit-tool $(MICROKIT_TOOL) \
		--extra-search-path ../../build --objcopy $(LLVM_OBJCOPY)
	python3 package_simplex_agentstore_probe.py \
		--base-system $(AGENTSTORE_BASE_SYSTEM) \
		--agent-elf $(AGENTSTORE_PROBE_DIR)/probe.wrong-key.testclock.elf \
		--timer-elf ../../build/ghc-scheduler-wait-probe/timer_driver.elf \
		--output $(AGENTSTORE_WRONG_KEY_IMAGE_DIR) --microkit-tool $(MICROKIT_TOOL) \
		--extra-search-path ../../build --objcopy $(LLVM_OBJCOPY)

guest-simplex-agentstore-qemu-smoke: guest-simplex-agentstore-qemu-image
	python3 agentstore_qemu_smoke.py \
		--correct-image $(AGENTSTORE_QEMU_IMAGE_DIR) \
		--wrong-key-image $(AGENTSTORE_WRONG_KEY_IMAGE_DIR) \
		--pristine-disk ../../build/storage-test/pristine-disk.img --qemu $(QEMU)

# Real upstream agent lifecycle on the same encrypted QEMU vault. Its
# messages_app entropy endpoint is channel 3 in system.system.
$(AGENT_RUNTIME_PROBE_DIR)/SMPFixtureCA.hs: guest-smp-fixture generate_tls_fixture_ca.py
	mkdir -p $(AGENT_RUNTIME_PROBE_DIR)
	python3 generate_tls_fixture_ca.py $(SMP_FIXTURE_DIR)/ca.der $@ SMPFixtureCA

guest-simplex-agent-runtime-objects: guest-simplex-agent-protocol-check guest-simplex-http2-client-check $(AGENT_RUNTIME_PROBE_DIR)/SMPFixtureCA.hs
	python3 compile_simplexmq_agent_runtime_probe.py

guest-simplex-agent-runtime-link: guest-simplex-agent-runtime-objects \
		guest-simplex-sqlcipher-vault-client haskell-qemu-image
	python3 link_simplexmq_agent_runtime_probe.py

$(GUEST_AGENT_RUNTIME_SYSTEM): $(AGENTSTORE_BASE_SYSTEM) $(HASKELL_NET_FRAGMENT_DIR)/net.system
	mkdir -p $(AGENT_RUNTIME_QEMU_IMAGE_DIR)
	cp $(HASKELL_NET_FRAGMENT_DIR)/eth_driver.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/dhcp_client.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_rx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_tx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_copy.elf $(AGENT_RUNTIME_QEMU_IMAGE_DIR)/
	python3 ../../tests/merge_network_system.py --haskell-tcp-pd messages_app \
		--base $(AGENTSTORE_BASE_SYSTEM) \
		--network $(HASKELL_NET_FRAGMENT_DIR)/net.system \
		--output $@ --elf-dir $(AGENT_RUNTIME_QEMU_IMAGE_DIR)

guest-simplex-agent-runtime-qemu-image: guest-simplex-agent-runtime-link $(GUEST_AGENT_RUNTIME_SYSTEM) guest-simplex-agentstore-qemu-image
	python3 package_simplex_agentstore_probe.py \
		--base-system $(GUEST_AGENT_RUNTIME_SYSTEM) \
		--agent-elf $(AGENT_RUNTIME_PROBE_DIR)/agent-runtime.elf \
		--timer-elf ../../build/ghc-scheduler-wait-probe/timer_driver.elf \
		--output $(AGENT_RUNTIME_QEMU_IMAGE_DIR) --microkit-tool $(MICROKIT_TOOL) \
		--extra-search-path ../../build --objcopy $(LLVM_OBJCOPY)

guest-simplex-agent-runtime-qemu-smoke: guest-simplex-agent-runtime-qemu-image
	python3 agent_runtime_qemu_smoke.py \
		--agent-image $(AGENT_RUNTIME_QEMU_IMAGE_DIR) \
		--wrong-key-image $(AGENTSTORE_WRONG_KEY_IMAGE_DIR) \
		--smp-server $(SMP_SERVER_BIN) --smp-fixture $(SMP_FIXTURE_DIR) \
		--dns-stub ../../tests/net/dns_stub.py \
		--pristine-disk ../../build/storage-test/pristine-disk.img --qemu $(QEMU)

# Long-lived real Agent service image with the production Messages PD making
# its first asynchronous NXSM ListConnections request.
.PHONY: guest-simplex-agent-service-qemu-smoke
$(AGENT_SERVICE_PROBE_DIR)/SMPFixtureCA.hs: $(AGENT_RUNTIME_PROBE_DIR)/SMPFixtureCA.hs
	mkdir -p $(AGENT_SERVICE_PROBE_DIR)
	cp $< $@

guest-simplex-agent-service-objects: guest-simplex-agent-protocol-check guest-simplex-http2-client-check $(AGENT_SERVICE_PROBE_DIR)/SMPFixtureCA.hs
	SIMPLEX_AGENT_RUNTIME_OUTPUT=$(AGENT_SERVICE_PROBE_DIR) \
	SIMPLEX_AGENT_RUNTIME_SOURCE=$(CURDIR)/SimplexAgentRuntimeProbe.hs \
	SIMPLEX_AGENT_SERVICE_MODE=1 python3 compile_simplexmq_agent_runtime_probe.py

guest-simplex-agent-service-link: guest-simplex-agent-service-objects guest-simplex-sqlcipher-vault-client haskell-qemu-image
	SIMPLEX_AGENT_RUNTIME_OUTPUT=$(AGENT_SERVICE_PROBE_DIR) SIMPLEX_AGENT_DEDICATED_PD=1 python3 link_simplexmq_agent_runtime_probe.py

$(AGENT_SERVICE_RPC_CLIENT_ELF): ../../rust/src/bin/simplex_agent_rpc_client.rs ../../rust/src/simplex_ipc.rs ../../rust/Cargo.toml
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir $(abspath ../../build/rust-target) --bin simplex_agent_rpc_client
	cp $(RUST_AGENT_RPC_CLIENT_ELF) $@

$(AGENT_SERVICE_MESSAGES_ELF): ../../rust/src/messages_app.rs ../../rust/src/vault_app_test_common.rs ../../rust/src/messages_history.rs ../../rust/src/vault_history.rs ../../rust/src/simplex_ipc.rs ../../rust/Cargo.toml ../../rust/Cargo.lock
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir $(abspath ../../build/rust-target) --features simplex-agent --bin rust_messages_agent_app
	cp $(abspath ../../build/rust-target/aarch64-sel4-microkit/release/rust_messages_agent_app.elf) $@

$(AGENT_SERVICE_PROBE_DIR)/dedicated-agent-base.system: $(AGENTSTORE_BASE_SYSTEM) prepare_simplex_agent_service_system.py
	python3 prepare_simplex_agent_service_system.py --base $(AGENTSTORE_BASE_SYSTEM) --output $@

$(RUST_VAULT_DISK_ELF): ../../rust/src/vault_disk.rs ../../rust/src/vault_protocol.rs ../../rust/src/vault_history.rs ../../rust/Cargo.toml ../../rust/Cargo.lock ../../build/storage-test/rust_vault_disk.elf
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir $(abspath ../../build/rust-target) --features development-crypto --bin rust_vault_disk
	$(LLVM_OBJCOPY) --dump-section .blk_client_config=$(AGENT_SERVICE_PROBE_DIR)/blk_client_config.data ../../build/storage-test/rust_vault_disk.elf
	$(LLVM_OBJCOPY) --update-section .blk_client_config=$(AGENT_SERVICE_PROBE_DIR)/blk_client_config.data $@ $(AGENT_SERVICE_PROBE_DIR)/rust_vault_disk.configured.elf
	mv $(AGENT_SERVICE_PROBE_DIR)/rust_vault_disk.configured.elf $@

# Give the AgentStore a separate vault transfer page, distinct from the
# Messages UI page. Preserve the configured sDDF block-client section from the
# storage-test image when building this feature-specific vault binary.
$(AGENT_SERVICE_VAULT_ELF): ../../build/storage-test/rust_vault_disk.elf ../../rust/src/vault_disk.rs ../../rust/src/vault_protocol.rs ../../rust/src/vault_history.rs ../../rust/Cargo.toml ../../rust/Cargo.lock
	mkdir -p $(AGENT_SERVICE_PROBE_DIR)
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir $(AGENT_SERVICE_PROBE_DIR)/rust-target \
		--features storage-test,simplex-agent-vault --bin rust_vault_disk
	$(LLVM_OBJCOPY) --dump-section .blk_client_config=$(AGENT_SERVICE_PROBE_DIR)/blk_client_config.data $<
	$(LLVM_OBJCOPY) --update-section .blk_client_config=$(AGENT_SERVICE_PROBE_DIR)/blk_client_config.data \
		$(AGENT_SERVICE_PROBE_DIR)/rust-target/aarch64-sel4-microkit/release/rust_vault_disk.elf $@

$(RUST_CLOCK_ELF): ../../rust/src/clock.rs ../../rust/Cargo.toml ../../rust/Cargo.lock
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir $(abspath ../../build/rust-target) --bin rust_clock

$(GUEST_AGENT_SERVICE_SYSTEM): $(AGENT_SERVICE_PROBE_DIR)/dedicated-agent-base.system $(HASKELL_NET_FRAGMENT_DIR)/net.system
	mkdir -p $(AGENT_SERVICE_QEMU_IMAGE_DIR)
	cp $(HASKELL_NET_FRAGMENT_DIR)/eth_driver.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/dhcp_client.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_rx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_tx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_copy.elf $(AGENT_SERVICE_QEMU_IMAGE_DIR)/
	python3 ../../tests/merge_network_system.py --haskell-tcp-pd simplex_agent \
		--base $(AGENT_SERVICE_PROBE_DIR)/dedicated-agent-base.system \
		--network $(HASKELL_NET_FRAGMENT_DIR)/net.system \
		--output $@ --elf-dir $(AGENT_SERVICE_QEMU_IMAGE_DIR)

guest-simplex-agent-service-qemu-image: guest-simplex-agent-service-link $(GUEST_AGENT_SERVICE_SYSTEM) $(AGENT_SERVICE_MESSAGES_ELF) $(AGENT_SERVICE_VAULT_ELF) $(RUST_CLOCK_ELF) guest-simplex-sqlcipher-rng-compile haskell-microkit-entry-compile
	cp $(AGENT_SERVICE_VAULT_ELF) $(RUST_CLOCK_ELF) $(RUST_ENTROPY_ELF) $(AGENT_SERVICE_QEMU_IMAGE_DIR)/
	python3 package_simplex_agentstore_probe.py \
		--base-system $(GUEST_AGENT_SERVICE_SYSTEM) --agent-pd simplex_agent \
		--agent-elf $(AGENT_SERVICE_PROBE_DIR)/agent-runtime.elf \
		--messages-agent-elf $(AGENT_SERVICE_MESSAGES_ELF) \
		--timer-elf ../../build/ghc-scheduler-wait-probe/timer_driver.elf \
		--output $(AGENT_SERVICE_QEMU_IMAGE_DIR) --microkit-tool $(MICROKIT_TOOL) \
		--extra-search-path ../../build --objcopy $(LLVM_OBJCOPY)

guest-simplex-agent-service-qemu-smoke: guest-simplex-agent-service-qemu-image
	python3 agent_service_qemu_smoke.py \
		--agent-image $(AGENT_SERVICE_QEMU_IMAGE_DIR) \
		--smp-server $(SMP_SERVER_BIN) --smp-fixture $(SMP_FIXTURE_DIR) \
		--dns-stub ../../tests/net/dns_stub.py \
		--pristine-disk ../../build/storage-test/pristine-disk.img --qemu $(QEMU)

# Reproducible UI-to-Agent messaging test. The test-only Messages binary
# exports its invitation to an independent host CLI peer through QEMU serial;
# never include this test variant in a device image.
$(SIMPLEX_AGENT_UI_MESSAGES_ELF): ../../rust/src/messages_app.rs ../../rust/src/vault_app_test_common.rs ../../rust/src/messages_history.rs ../../rust/src/vault_history.rs ../../rust/src/simplex_ipc.rs ../../rust/Cargo.toml ../../rust/Cargo.lock
	mkdir -p $(SIMPLEX_AGENT_UI_PROBE_DIR)
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir $(SIMPLEX_AGENT_UI_PROBE_DIR)/rust-target \
		--features simplex-agent-qemu-external-cli-test --bin rust_messages_agent_app
	cp $(SIMPLEX_AGENT_UI_PROBE_DIR)/rust-target/aarch64-sel4-microkit/release/rust_messages_agent_app.elf $@

# Let Cargo track the complete UI dependency graph, including modules imported
# by utm.rs. A short hand-maintained file list can silently leave a stale UI.
.PHONY: simplex-agent-ui-check-sources
simplex-agent-ui-check-sources:

SLINT_WALLET_FEATURES ?= utm-pd,full-suite-ui,simplex-agent-ui,simplex-cli-qemu-test
WALLET_BUILD_FEATURES ?= wallet-core
$(SIMPLEX_AGENT_UI_SLINT_ELF): simplex-agent-ui-check-sources
	mkdir -p $(SIMPLEX_AGENT_UI_PROBE_DIR)
	SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) cargo +nightly-2026-09-25 build \
		--locked --offline --release --manifest-path ../../experiments/slint-notes/Cargo.toml \
		--target ../../rust/targets/aarch64-sel4-microkit.json \
		--target-dir $(SIMPLEX_AGENT_UI_PROBE_DIR)/slint-target \
		--features $(SLINT_WALLET_FEATURES) --bin slint_notes_utm \
		-Z build-std=core,alloc -Z json-target-spec

.PHONY: guest-simplex-agent-ui-qemu-image guest-simplex-agent-ui-qemu-smoke
guest-simplex-agent-ui-qemu-image: guest-simplex-agent-service-qemu-image $(SIMPLEX_AGENT_UI_MESSAGES_ELF) $(SIMPLEX_AGENT_UI_SLINT_ELF) simplex-agent-slint-qemu.system
	mkdir -p $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)
	cp -R $(AGENT_SERVICE_QEMU_IMAGE_DIR)/. $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/
	cp simplex-agent-slint-qemu.system $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/system.system
	cp $(SIMPLEX_AGENT_UI_MESSAGES_ELF) $(SIMPLEX_AGENT_UI_SLINT_ELF) $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/
	# The Slint system maps the vault block queues at different addresses
	# from storage-test. Preserve sDDF metadata, but use the actual UI maps.
	$(LLVM_OBJCOPY) --dump-section .blk_client_config=$(SIMPLEX_AGENT_UI_PROBE_DIR)/blk_client_config.source \
		$(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/rust_vault_disk.elf
	python3 configure_slint_vault.py $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/system.system \
		$(SIMPLEX_AGENT_UI_PROBE_DIR)/blk_client_config.source $(SIMPLEX_AGENT_UI_PROBE_DIR)/blk_client_config.ui
	$(LLVM_OBJCOPY) --update-section .blk_client_config=$(SIMPLEX_AGENT_UI_PROBE_DIR)/blk_client_config.ui \
		$(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/rust_vault_disk.elf
	# Build the wallet without the embedded mainnet fixture; do not take a
	# potentially stale wallet binary from the generic build search path.
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) cargo +nightly-2026-09-25 build \
		--locked --offline --release --target aarch64-sel4-microkit \
		--target-dir $(abspath $(SIMPLEX_AGENT_UI_PROBE_DIR)/wallet-target) \
		--features $(WALLET_BUILD_FEATURES) --bin rust_wallet_app --bin rust_scan_worker
	cp $(SIMPLEX_AGENT_UI_PROBE_DIR)/wallet-target/aarch64-sel4-microkit/release/rust_wallet_app.elf \
		$(SIMPLEX_AGENT_UI_PROBE_DIR)/wallet-target/aarch64-sel4-microkit/release/rust_scan_worker.elf \
		$(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/
	cp ../../build/sddf-gpu-spike/gpu_driver.elf ../../build/sddf-gpu-spike/gpu_virt.elf $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) cargo +nightly-2026-09-25 build \
		--locked --offline --release --target aarch64-sel4-microkit \
		--target-dir $(abspath $(SIMPLEX_AGENT_UI_PROBE_DIR)/input-target) \
		--features utm-slint-input --bin rust_input_service
	cp $(SIMPLEX_AGENT_UI_PROBE_DIR)/input-target/aarch64-sel4-microkit/release/rust_input_service.elf \
		$(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/input_service.elf
	python3 build_slint_gpu_client.py --sdk $(MICROKIT_SDK_ABS) --sddf $(SDDF_SOURCE) \
		--build $(abspath $(SIMPLEX_AGENT_UI_PROBE_DIR)/slint-gpu-client) \
		--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG)
	cp $(SIMPLEX_AGENT_UI_PROBE_DIR)/slint-gpu-client/client.elf $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/gpu_client.elf
	$(MAKE) -C ../../tests/input MICROKIT_SDK=$(MICROKIT_SDK_ABS) SDDF=$(SDDF_SOURCE) \
		MICROKIT_BOARD=$(MICROKIT_BOARD) MICROKIT_CONFIG=$(MICROKIT_CONFIG) \
		BUILD_DIR=$(abspath $(SIMPLEX_AGENT_UI_PROBE_DIR)/slint-gpu-client) all
	cp $(SIMPLEX_AGENT_UI_PROBE_DIR)/slint-gpu-client/input_driver.elf $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/input_driver.elf
	$(MICROKIT_TOOL) $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/system.system \
		--search-path $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR) \
		--search-path ../../build \
		--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) \
		-o $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/loader.img \
		-r $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR)/report.txt

# Public-relay QEMU session. Builds the Slint image before replacing its Agent
# program, so it does not accidentally package stale wallet or UI binaries.
PUBLIC_SMP_DIR := $(abspath $(BUILD_DIR)/guest-simplex-public)
PUBLIC_SMP_NET_DIR := $(abspath $(BUILD_DIR)/guest-simplex-public-net-qemu-dns)
PUBLIC_SMP_VAULT_TARGET := $(abspath $(BUILD_DIR)/guest-simplex-public-vault-target)
# Provision from the vault ELF belonging to the UTM installation being upgraded.
# Do not silently build a public image with the unrelated storage-test key.
UTM_VAULT_SOURCE_ELF ?=
# Keep the agent and display image within the CapDL initializer's slot budget.
PUBLIC_SMP_LIBC_HEAP_SIZE ?= 50331648u
.PHONY: guest-simplex-public-image guest-simplex-public-qemu
guest-simplex-public-image: guest-simplex-agent-ui-qemu-image $(AGENT_SERVICE_PROBE_DIR)/SMPFixtureCA.hs \
		$(HASKELL_NET_FRAGMENT_DIR)/net.system
	@test -n "$(UTM_VAULT_SOURCE_ELF)" && test -f "$(UTM_VAULT_SOURCE_ELF)" || \
		{ echo "Set UTM_VAULT_SOURCE_ELF to the provisioned vault ELF for this installation" >&2; exit 1; }
	mkdir -p $(PUBLIC_SMP_DIR)
	cp $(AGENT_SERVICE_PROBE_DIR)/SMPFixtureCA.hs $(PUBLIC_SMP_DIR)/SMPFixtureCA.hs
	SIMPLEX_AGENT_RUNTIME_OUTPUT=$(PUBLIC_SMP_DIR) \
		SIMPLEX_AGENT_RUNTIME_SOURCE=$(CURDIR)/SimplexAgentRuntimeProbe.hs \
		SIMPLEX_AGENT_SERVICE_MODE=1 SIMPLEX_PUBLIC_SMP=1 AGENT_RUNTIME_FORCE_REBUILD=1 \
		python3 compile_simplexmq_agent_runtime_probe.py
	SIMPLEX_AGENT_RUNTIME_OUTPUT=$(PUBLIC_SMP_DIR) SIMPLEX_AGENT_DEDICATED_PD=1 \
		SEL4_LIBC_HEAP_SIZE=$(PUBLIC_SMP_LIBC_HEAP_SIZE) python3 link_simplexmq_agent_runtime_probe.py
	rm -rf $(PUBLIC_SMP_DIR)/qemu-image
	cp -R $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR) $(PUBLIC_SMP_DIR)/qemu-image
	# Build the installation-key vault, retain its existing key and use the
	# block mapping already checked against the UI system description.
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) cargo +nightly-2026-09-25 build \
		--locked --offline --release --target aarch64-sel4-microkit \
		--target-dir $(PUBLIC_SMP_VAULT_TARGET) \
		--features utm-installation-vault-key,simplex-agent-vault --bin rust_vault_disk
	python3 provision_utm_vault.py --objcopy $(LLVM_OBJCOPY) --source "$(UTM_VAULT_SOURCE_ELF)" \
		--input $(PUBLIC_SMP_VAULT_TARGET)/aarch64-sel4-microkit/release/rust_vault_disk.elf \
		--output $(PUBLIC_SMP_DIR)/vault-keyed.elf
	$(LLVM_OBJCOPY) --dump-section .blk_client_config=$(PUBLIC_SMP_DIR)/vault-block-config \
		$(PUBLIC_SMP_DIR)/qemu-image/rust_vault_disk.elf
	$(LLVM_OBJCOPY) --update-section .blk_client_config=$(PUBLIC_SMP_DIR)/vault-block-config \
		$(PUBLIC_SMP_DIR)/vault-keyed.elf $(PUBLIC_SMP_DIR)/qemu-image/rust_vault_disk.elf
	# QEMU user networking provides DNS at 10.0.2.3:53. Build the network
	# client for that resolver, then preserve all configs from the UI image.
	PATH="/opt/homebrew/opt/llvm/bin:$$PATH" $(MAKE) -C ../../tests/net \
		MICROKIT_SDK=$(MICROKIT_SDK_ABS) MICROKIT_BOARD=$(MICROKIT_BOARD) \
		MICROKIT_CONFIG=$(MICROKIT_CONFIG) SDDF=$(SDDF_SOURCE) PYTHON=$(SDDF_PYTHON) \
		MBEDTLS_DIR=$(abspath ../../.cache/mbedtls-3.6.2) \
		BUILD_DIR=$(PUBLIC_SMP_NET_DIR) EXTRA_CFLAGS=-DDNS_SERVER_PORT=53 all
	$(LLVM_OBJCOPY) --dump-section .net_client_config=$(PUBLIC_SMP_DIR)/net_client_config.ui \
		--dump-section .timer_client_config=$(PUBLIC_SMP_DIR)/timer_client_config.ui \
		--dump-section .lib_sddf_lwip_config=$(PUBLIC_SMP_DIR)/lwip_config.ui \
		$(PUBLIC_SMP_DIR)/qemu-image/dhcp_client.elf
	$(LLVM_OBJCOPY) --update-section .net_client_config=$(PUBLIC_SMP_DIR)/net_client_config.ui \
		--update-section .timer_client_config=$(PUBLIC_SMP_DIR)/timer_client_config.ui \
		--update-section .lib_sddf_lwip_config=$(PUBLIC_SMP_DIR)/lwip_config.ui \
		$(PUBLIC_SMP_NET_DIR)/dhcp_client.elf $(PUBLIC_SMP_DIR)/qemu-image/dhcp_client.elf
	$(LLVM_OBJCOPY) --update-section .timer_client_config=$(PUBLIC_SMP_DIR)/qemu-image/timer_client_config.data \
		$(PUBLIC_SMP_DIR)/agent-runtime.elf $(PUBLIC_SMP_DIR)/qemu-image/agentstore_probe.elf
	$(MICROKIT_TOOL) $(PUBLIC_SMP_DIR)/qemu-image/system.system \
		--search-path $(PUBLIC_SMP_DIR)/qemu-image \
		--search-path ../../build \
		--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) \
		-o $(PUBLIC_SMP_DIR)/qemu-image/loader.img \
		-r $(PUBLIC_SMP_DIR)/qemu-image/report.txt

guest-simplex-public-qemu: guest-simplex-public-image
	python3 public_simplex_qemu.py \
		--agent-image $(PUBLIC_SMP_DIR)/qemu-image \
		--dns-stub ../../tests/net/dns_stub.py \
		--pristine-disk ../../build/storage-test/pristine-disk.img \
		--qemu $(QEMU)

guest-simplex-agent-ui-qemu-smoke: guest-simplex-agent-ui-qemu-image
	python3 agent_service_qemu_smoke.py \
		--agent-image $(SIMPLEX_AGENT_UI_QEMU_IMAGE_DIR) \
		--smp-server $(SMP_SERVER_BIN) --smp-fixture $(SMP_FIXTURE_DIR) \
		--dns-stub ../../tests/net/dns_stub.py \
		--pristine-disk ../../build/storage-test/pristine-disk.img --simplex-chat $(SIMPLEX_CHAT_BIN) --qemu $(QEMU) \
		--slint-ui --external-cli --send-ui-message

guest-simplex-agent-cli-interop-qemu-smoke: guest-simplex-agent-runtime-qemu-image
	SIMPLEX_AGENT_RUNTIME_OUTPUT=$(AGENT_CLI_INTEROP_DIR) SIMPLEX_AGENT_EXTERNAL_PEER=1 \
		SIMPLEX_AGENT_EXTERNAL_BIDIRECTIONAL=1 \
		python3 compile_simplexmq_agent_runtime_probe.py
	SIMPLEX_AGENT_RUNTIME_OUTPUT=$(AGENT_CLI_INTEROP_DIR) \
		python3 link_simplexmq_agent_runtime_probe.py
	python3 package_simplex_agentstore_probe.py \
		--base-system $(GUEST_AGENT_RUNTIME_SYSTEM) \
		--agent-elf $(AGENT_CLI_INTEROP_DIR)/agent-runtime.elf \
		--timer-elf ../../build/ghc-scheduler-wait-probe/timer_driver.elf \
		--output $(AGENT_CLI_INTEROP_QEMU_IMAGE_DIR) --microkit-tool $(MICROKIT_TOOL) \
		--extra-search-path ../../build --objcopy $(LLVM_OBJCOPY)
	python3 simplex_cli_interop_qemu_smoke.py \
		--agent-image $(AGENT_CLI_INTEROP_QEMU_IMAGE_DIR) \
		--pristine-disk ../../build/storage-test/pristine-disk.img \
		--smp-server $(SMP_SERVER_BIN) --smp-fixture $(SMP_FIXTURE_DIR) \
		--dns-stub ../../tests/net/dns_stub.py --simplex-chat $(SIMPLEX_CHAT_BIN) --qemu $(QEMU) \
		--bidirectional

# Exercise the NXSM protected-call page as an isolated service alongside the
# normal Messages PD in the already-built OS image. The probe client is extra;
# the image's real rust_messages_app.elf remains unchanged.
.PHONY: simplex-ipc-sidecar-qemu-smoke
simplex-ipc-sidecar-qemu-smoke: simplex-ipc-host-test ../../build/system.system
	mkdir -p $(SIMPLEX_IPC_SIDECAR_DIR)
	cp ../../build/test-disk.img $(SIMPLEX_IPC_SIDECAR_DIR)/test-disk.img
	$(CC) $(CFLAGS) -I../../tests -c ../../tests/simplex_ipc_sidecar_service.c \
		-o $(SIMPLEX_IPC_SIDECAR_DIR)/service.o
	$(CC) $(CFLAGS) -I../../tests -c ../../tests/simplex_ipc_sidecar_client.c \
		-o $(SIMPLEX_IPC_SIDECAR_DIR)/client.o
	$(LD) $(LDFLAGS) -o $(SIMPLEX_IPC_SIDECAR_DIR)/simplex_ipc_sidecar_service.elf \
		$(SIMPLEX_IPC_SIDECAR_DIR)/service.o --start-group -lmicrokit \
		-T$(BOARD_DIR)/lib/microkit.ld --end-group
	$(LD) $(LDFLAGS) -o $(SIMPLEX_IPC_SIDECAR_DIR)/simplex_ipc_sidecar_client.elf \
		$(SIMPLEX_IPC_SIDECAR_DIR)/client.o --start-group -lmicrokit \
		-T$(BOARD_DIR)/lib/microkit.ld --end-group
	python3 ../../tests/simplex_ipc_sidecar_system.py \
		--base ../../build/system.system \
		--output $(SIMPLEX_IPC_SIDECAR_DIR)/system.system
	$(MICROKIT_TOOL) $(SIMPLEX_IPC_SIDECAR_DIR)/system.system \
		--search-path $(SIMPLEX_IPC_SIDECAR_DIR) --search-path ../../build \
		--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) \
		-o $(SIMPLEX_IPC_SIDECAR_DIR)/loader.img \
		-r $(SIMPLEX_IPC_SIDECAR_DIR)/report.txt
	python3 ../../tests/simplex_ipc_sidecar_qemu_smoke.py \
		--qemu $(QEMU) --image $(SIMPLEX_IPC_SIDECAR_DIR)/loader.img \
		--disk $(SIMPLEX_IPC_SIDECAR_DIR)/test-disk.img

guest-smp-encoding-deps: guest-tls-deps
	python3 cross-build-tls-dep.py primitive
	python3 cross-build-tls-dep.py integer-logarithms
	python3 cross-build-tls-dep.py scientific
	python3 cross-build-tls-dep.py attoparsec
	python3 cross-build-tls-dep.py network-transport

$(GUEST_SMP_PLATFORM_OBJECT): smp_platform.c $(GHC_GENERATED_HEADERS)/ghcplatform.h
	mkdir -p $(GUEST_SMP_PROBE_DIR)
	$(CC) $(CFLAGS) -Ifreestanding-headers -c $< -o $@

$(GUEST_SMP_PROBE_DIR)/SMPFixtureCA.hs: guest-smp-fixture generate_tls_fixture_ca.py
	mkdir -p $(GUEST_SMP_PROBE_DIR)
	python3 generate_tls_fixture_ca.py $(SMP_FIXTURE_DIR)/ca.der $@ SMPFixtureCA

guest-smp-link-inventory: guest-tls-link-inventory guest-smp-encoding-deps $(GUEST_SMP_PROBE_DIR)/SMPFixtureCA.hs $(GUEST_SMP_PLATFORM_OBJECT)
	cd $(GHC_RTS_SOURCE) && LLD=$(LLD) GHC_LLD_ALLOW_UNRESOLVED=1 \
		GHC_LLD_EXTRA_ARGS="-L$(abspath $(BOARD_DIR)/lib) -T$(abspath $(BOARD_DIR)/lib)/microkit.ld" \
		$(GHC_CROSS_ABS) -v0 -fforce-recomp -no-hs-main \
		-i$(GUEST_SMP_PROBE_DIR) -i$(SIMPLEXMQ_SHIMS) -i$(SIMPLEXMQ_SOURCE) \
		-outputdir $(GUEST_SMP_PROBE_DIR) \
		$(abspath NativeNetwork.hs) $(abspath NativeTLSBackend.hs) \
		$(abspath NativeSMPIdentity.hs) $(abspath NativeSMPConnection.hs) \
		$(abspath NativeSMPTransport.hs) \
		$(GUEST_SMP_PROBE_DIR)/SMPFixtureCA.hs $(abspath SMPLinkProbe.hs) \
		$(HASKELL_MICROKIT_ENTRY_OBJECT) $(HASKELL_LIBC_OBJECT) \
		$(GUEST_TLS_WALLCLOCK_OBJECT) $(HASKELL_CALENDAR_OBJECT) \
		$(HASKELL_ENTROPY_OBJECT) $(HASKELL_TCP_OBJECT) \
		$(GUEST_SMP_PLATFORM_OBJECT) \
		$(abspath $(OPENLIBM_ARCHIVE)) $(abspath $(COMPILER_RT_QUAD_ARCHIVE)) \
		$(abspath $(BUILD_DIR)/rts-adapter/sddf_time_bridge.o) \
		$(abspath $(BOARD_DIR)/lib/libmicrokit.a) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) \
		-package-db $(GUEST_TLS_PACKAGE_DB) -package tls -package crypton -package stm \
		-package attoparsec -package network-transport \
		-package hourglass -package crypton-x509 -package crypton-x509-store \
		-package crypton-x509-validation \
		-pgml $(GHC_LD_WRAPPER) \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,init -o $(GUEST_SMP_PROBE_DIR)/smp-link-inventory.elf \
		> $(GUEST_SMP_PROBE_DIR)/link.log 2>&1
	$(LLVM_NM) -u $(GUEST_SMP_PROBE_DIR)/smp-link-inventory.elf | sort -u \
		> $(GUEST_SMP_PROBE_DIR)/unresolved.txt
	@test -s $(GUEST_SMP_PROBE_DIR)/Simplex/Messaging/Transport/Buffer.o
	@test -s $(GUEST_SMP_PROBE_DIR)/Simplex/Messaging/Encoding.o
	@test -s $(GUEST_SMP_PROBE_DIR)/NativeSMPTransport.o
	@test -s $(GUEST_SMP_PROBE_DIR)/NativeSMPConnection.o
	@test ! -s $(GUEST_SMP_PROBE_DIR)/unresolved.txt

guest-smp-net-qemu-image: guest-smp-link-inventory guest-tls-net-qemu-image
	mkdir -p $(GUEST_SMP_NET_IMAGE_DIR)
	cp $(GUEST_TLS_NET_IMAGE_DIR)/*.elf $(GUEST_TLS_NET_IMAGE_DIR)/haskell-tls-net.system \
		$(GUEST_SMP_NET_IMAGE_DIR)/
	$(LLVM_OBJCOPY) --update-section .timer_client_config=$(SCHEDULER_PROBE_OUTPUT_DIR)/timer_client_client.data \
		$(GUEST_SMP_PROBE_DIR)/smp-link-inventory.elf $(GUEST_SMP_NET_IMAGE_DIR)/client.elf
	$(MICROKIT_TOOL) $(GUEST_SMP_NET_IMAGE_DIR)/haskell-tls-net.system \
		--search-path $(GUEST_SMP_NET_IMAGE_DIR) --board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) -o $(GUEST_SMP_NET_IMAGE_DIR)/loader.img \
		-r $(GUEST_SMP_NET_IMAGE_DIR)/report.txt

guest-smp-net-qemu-smoke: guest-smp-net-qemu-image
	python3 smp_net_qemu_smoke.py $(GUEST_SMP_NET_IMAGE_DIR)/loader.img \
		$(SMP_SERVER_BIN) $(SMP_FIXTURE_DIR) $(QEMU)

$(GUEST_TLS_WALLCLOCK_OBJECT): tls_qemu_wallclock.c
	mkdir -p $(GUEST_TLS_PROBE_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(GUEST_TLS_PROBE_DIR)/TLSFixtureCA.hs: fixtures/tls/ca.der generate_tls_fixture_ca.py
	mkdir -p $(GUEST_TLS_PROBE_DIR)
	python3 generate_tls_fixture_ca.py $< $@

$(GUEST_TLS_IMAGE_DIR)/tls_qemu_clock.o: tls_qemu_clock.c
	mkdir -p $(GUEST_TLS_IMAGE_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(GUEST_TLS_CLOCK_ELF): $(GUEST_TLS_IMAGE_DIR)/tls_qemu_clock.o
	$(LD) $(LDFLAGS) -o $@ $< --start-group -lmicrokit \
		-T$(BOARD_DIR)/lib/microkit.ld --end-group

calendar-utc-test:
	mkdir -p $(BUILD_DIR)
	clang -std=c11 -Wall -Wextra -Werror -Ifreestanding-headers \
		calendar_utc.c calendar_utc_test.c -o $(BUILD_DIR)/calendar_utc_test
	$(BUILD_DIR)/calendar_utc_test

# Keep a host-side API check alongside the guest compilation above.
native-tls-backend-typecheck:
	$(abspath $(HOST_GHC)) -v0 -fno-code -fforce-recomp \
		-package-db $(abspath $(HOST_TLS_PACKAGE_DB)) -package tls -package bytestring \
		-i$(CURDIR) $(abspath NativeNetwork.hs) $(abspath NativeTLSBackend.hs)

$(HADRIAN_RTS_RELOC): hadrian-rts
	$(LD) -r --whole-archive $(HADRIAN_RTS_ARCHIVE) --no-whole-archive -o $@

rts-trial-link: $(HADRIAN_RTS_RELOC)
	@printf "Unresolved imports in relocatable RTS trial: "
	@$(LLVM_NM) -u $< | wc -l

# Compile the fixture's minimal Haskell entry point against Hadrian's target
# base/RTS package database. This checks target interfaces and code generation;
# it does not link or run the resulting object.
haskell-compile-probe: hadrian-bytestring | $(BUILD_DIR)
	mkdir -p $(HASKELL_PROBE_BUILD_DIR)
	cd $(GHC_RTS_SOURCE) && $(GHC_CROSS_ABS) -v0 -fforce-recomp -c \
		$(abspath NativeNetwork.hs) \
		-odir $(HASKELL_PROBE_BUILD_DIR) -hidir $(HASKELL_PROBE_BUILD_DIR) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) -package bytestring \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-o $(HASKELL_PROBE_BUILD_DIR)/NativeNetwork.o
	cd $(GHC_RTS_SOURCE) && $(GHC_CROSS_ABS) -v0 -fforce-recomp -c \
		$(abspath Main.hs) \
		-i$(HASKELL_PROBE_BUILD_DIR) \
		-odir $(HASKELL_PROBE_BUILD_DIR) -hidir $(HASKELL_PROBE_BUILD_DIR) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) -package bytestring \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,main -o $(HASKELL_PROBE_BUILD_DIR)/Main.o

# A separate bounded libc subset supplies ordinary C memory/string/allocation
# operations without depending on a host libc. The arena is distinct from the
# RTS megablock allocator and is single-threaded in this first target.
$(HASKELL_LIBC_OBJECT): sel4_libc.c sel4_libc.h
	mkdir -p $(HASKELL_PROBE_BUILD_DIR)
	$(CC) $(CFLAGS) -Ifreestanding-headers -c sel4_libc.c -o $@

haskell-libc-compile: $(HASKELL_LIBC_OBJECT)

$(HASKELL_MICROKIT_ENTRY_OBJECT): haskell_microkit_entry.c \
		$(GHC_GENERATED_HEADERS)/ghcplatform.h \
		$(GHC_GENERATED_HEADERS)/ghcautoconf.h
	mkdir -p $(HASKELL_PROBE_BUILD_DIR)
	$(CC) $(CFLAGS) $(GHC_RTS_INCLUDE_FLAGS) -c $< -o $@

haskell-microkit-entry-compile: $(HASKELL_MICROKIT_ENTRY_OBJECT)

$(HASKELL_WALLCLOCK_OBJECT): wallclock_unavailable.c
	mkdir -p $(HASKELL_PROBE_BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(HASKELL_CALENDAR_OBJECT): calendar_utc.c freestanding-headers/time.h
	mkdir -p $(HASKELL_PROBE_BUILD_DIR)
	$(CC) $(CFLAGS) -Ifreestanding-headers -c $< -o $@

$(HASKELL_ENTROPY_OBJECT): haskell_entropy_bridge.c
	mkdir -p $(HASKELL_PROBE_BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(HASKELL_TCP_OBJECT): haskell_tcp_bridge.c ../../tests/net/include/haskell_tcp_protocol.h
	mkdir -p $(HASKELL_PROBE_BUILD_DIR)
	$(CC) $(CFLAGS) -I../../tests/net/include -c $< -o $@

$(COMPILER_RT_QUAD_DIR)/%.o: $(COMPILER_RT_BUILTINS)/%.c
	mkdir -p $(COMPILER_RT_QUAD_DIR)
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -ffreestanding \
		-fno-builtin -O2 -I$(COMPILER_RT_BUILTINS) -c $< -o $@

$(COMPILER_RT_QUAD_DIR)/aarch64_fp_mode.o: $(COMPILER_RT_BUILTINS)/aarch64/fp_mode.c
	mkdir -p $(COMPILER_RT_QUAD_DIR)
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -ffreestanding \
		-fno-builtin -O2 -I$(COMPILER_RT_BUILTINS) -c $< -o $@

$(COMPILER_RT_QUAD_DIR)/clear_cache.o: $(COMPILER_RT_BUILTINS)/clear_cache.c
	mkdir -p $(COMPILER_RT_QUAD_DIR)
	$(CC) -target aarch64-none-elf -mcpu=cortex-a53 -ffreestanding \
		-fno-builtin -O2 -I$(COMPILER_RT_BUILTINS) -c $< -o $@

$(COMPILER_RT_QUAD_ARCHIVE): $(COMPILER_RT_QUAD_DIR)/addtf3.o \
		$(COMPILER_RT_QUAD_DIR)/divtf3.o \
		$(COMPILER_RT_QUAD_DIR)/extenddftf2.o \
		$(COMPILER_RT_QUAD_DIR)/fixtfsi.o \
		$(COMPILER_RT_QUAD_DIR)/floatditf.o \
		$(COMPILER_RT_QUAD_DIR)/floatsitf.o \
		$(COMPILER_RT_QUAD_DIR)/multf3.o \
		$(COMPILER_RT_QUAD_DIR)/comparetf2.o \
		$(COMPILER_RT_QUAD_DIR)/subtf3.o \
		$(COMPILER_RT_QUAD_DIR)/trunctfdf2.o \
		$(COMPILER_RT_QUAD_DIR)/extendsftf2.o \
		$(COMPILER_RT_QUAD_DIR)/trunctfsf2.o \
		$(COMPILER_RT_QUAD_DIR)/aarch64_fp_mode.o \
		$(COMPILER_RT_QUAD_DIR)/clear_cache.o
	$(LLVM_AR) rcs $@ $^

haskell-compiler-rt-quad: $(COMPILER_RT_QUAD_ARCHIVE)

# Produce a deliberately incomplete ELF and capture every unresolved symbol.
# GHC_LLD_ALLOW_UNRESOLVED is diagnostic-only; this image cannot run.
haskell-link-inventory: haskell-compile-probe haskell-libc-compile
	cd $(GHC_RTS_SOURCE) && LLD=$(LLD) GHC_LLD_ALLOW_UNRESOLVED=1 \
		GHC_LLD_EXTRA_ARGS="--why-extract=$(HASKELL_PROBE_BUILD_DIR)/why-extract.txt" \
		$(GHC_CROSS_ABS) -v0 -fforce-recomp $(abspath Main.hs) $(HASKELL_LIBC_OBJECT) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) \
		-pgml $(GHC_LD_WRAPPER) \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,main -o $(HASKELL_PROBE_BUILD_DIR)/hello.incomplete.elf \
		> $(HASKELL_PROBE_BUILD_DIR)/link.log 2>&1
	@printf "Unresolved imports in incomplete Haskell ELF: "
	@$(LLVM_NM) -u $(HASKELL_PROBE_BUILD_DIR)/hello.incomplete.elf | wc -l

# Re-link with section garbage collection to measure how much of the target
# base/RTS closure remains after unused Haskell functions are discarded.
haskell-link-gc-inventory: haskell-compile-probe haskell-libc-compile
	cd $(GHC_RTS_SOURCE) && LLD=$(LLD) GHC_LLD_ALLOW_UNRESOLVED=1 \
		GHC_LLD_EXTRA_ARGS="--gc-sections --undefined=main --why-extract=$(HASKELL_PROBE_BUILD_DIR)/why-extract-gc.txt" \
		$(GHC_CROSS_ABS) -v0 -fforce-recomp $(abspath Main.hs) $(HASKELL_LIBC_OBJECT) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) \
		-pgml $(GHC_LD_WRAPPER) \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,main -o $(HASKELL_PROBE_BUILD_DIR)/hello.gc-incomplete.elf \
		> $(HASKELL_PROBE_BUILD_DIR)/link-gc.log 2>&1
	@printf "Unresolved imports after section GC: "
	@$(LLVM_NM) -u $(HASKELL_PROBE_BUILD_DIR)/hello.gc-incomplete.elf | wc -l

# Optional math-library experiment. OpenLibm is built separately for the
# freestanding AArch64 target; this diagnostic link still permits the
# non-math platform/libc imports to remain unresolved.
haskell-openlibm-inventory: haskell-compile-probe haskell-libc-compile $(OPENLIBM_ARCHIVE)
	cd $(GHC_RTS_SOURCE) && LLD=$(LLD) GHC_LLD_ALLOW_UNRESOLVED=1 \
		GHC_LLD_EXTRA_ARGS="--why-extract=$(HASKELL_PROBE_BUILD_DIR)/why-extract-openlibm.txt" \
		$(GHC_CROSS_ABS) -v0 -fforce-recomp $(abspath Main.hs) $(HASKELL_LIBC_OBJECT) \
		$(abspath $(OPENLIBM_ARCHIVE)) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) \
		-pgml $(GHC_LD_WRAPPER) \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,main -o $(HASKELL_PROBE_BUILD_DIR)/hello.openlibm-incomplete.elf \
		> $(HASKELL_PROBE_BUILD_DIR)/link-openlibm.log 2>&1
	@printf "Unresolved imports with OpenLibm: "
	@$(LLVM_NM) -u $(HASKELL_PROBE_BUILD_DIR)/hello.openlibm-incomplete.elf | sort -u | wc -l

haskell-openlibm-gc-inventory: haskell-compile-probe haskell-libc-compile $(OPENLIBM_ARCHIVE)
	cd $(GHC_RTS_SOURCE) && LLD=$(LLD) GHC_LLD_ALLOW_UNRESOLVED=1 \
		GHC_LLD_EXTRA_ARGS="--gc-sections --undefined=main --why-extract=$(HASKELL_PROBE_BUILD_DIR)/why-extract-openlibm-gc.txt" \
		$(GHC_CROSS_ABS) -v0 -fforce-recomp $(abspath Main.hs) $(HASKELL_LIBC_OBJECT) \
		$(abspath $(OPENLIBM_ARCHIVE)) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) \
		-pgml $(GHC_LD_WRAPPER) \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,main -o $(HASKELL_PROBE_BUILD_DIR)/hello.openlibm-gc-incomplete.elf \
		> $(HASKELL_PROBE_BUILD_DIR)/link-openlibm-gc.log 2>&1
	@printf "Unresolved imports with OpenLibm after section GC: "
	@$(LLVM_NM) -u $(HASKELL_PROBE_BUILD_DIR)/hello.openlibm-gc-incomplete.elf | sort -u | wc -l

# Add the real Microkit platform library and the sDDF timer bridge. This
# resolves hardware-independent GHC time/event hooks and debug output while
# deliberately leaving PD entry points and wall-clock time visible.
haskell-platform-inventory: haskell-compile-probe haskell-libc-compile \
		haskell-microkit-entry-compile \
		rts-adapter-compile $(OPENLIBM_ARCHIVE) $(COMPILER_RT_QUAD_ARCHIVE)

haskell-platform-inventory: $(HASKELL_WALLCLOCK_OBJECT) $(HASKELL_ENTROPY_OBJECT) $(HASKELL_TCP_OBJECT)
	cd $(GHC_RTS_SOURCE) && LLD=$(LLD) GHC_LLD_ALLOW_UNRESOLVED=$(PLATFORM_ALLOW_UNRESOLVED) \
		GHC_LLD_EXTRA_ARGS="--why-extract=$(HASKELL_PROBE_BUILD_DIR)/why-extract-platform.txt -L$(abspath $(BOARD_DIR)/lib) -T$(abspath $(BOARD_DIR)/lib)/microkit.ld" \
		$(GHC_CROSS_ABS) -v0 -fforce-recomp -no-hs-main \
		$(abspath NativeNetwork.hs) $(abspath Main.hs) \
		$(HASKELL_MICROKIT_ENTRY_OBJECT) $(HASKELL_LIBC_OBJECT) \
		$(HASKELL_WALLCLOCK_OBJECT) $(HASKELL_ENTROPY_OBJECT) $(HASKELL_TCP_OBJECT) \
		$(abspath $(OPENLIBM_ARCHIVE)) $(abspath $(COMPILER_RT_QUAD_ARCHIVE)) \
		$(abspath $(BUILD_DIR)/rts-adapter/sddf_time_bridge.o) \
		$(abspath $(BOARD_DIR)/lib/libmicrokit.a) \
		-no-global-package-db -package-env=- -package-db $(HASKELL_PACKAGE_DB) -package bytestring \
		-pgml $(GHC_LD_WRAPPER) \
		-optc-target -optcaarch64-none-elf \
		-optc-I$(abspath freestanding-headers) -optc-Dssize_t=long \
		-optc-ffreestanding -optc-fuse-ld=lld -optc-nostdlib \
		-optc-Wl,-e,init -o $(HASKELL_PROBE_BUILD_DIR)/hello.platform.elf \
		> $(HASKELL_PROBE_BUILD_DIR)/link-platform.log 2>&1
	@printf "Unresolved imports with Microkit and sDDF timer adapters: "
	@$(LLVM_NM) -u $(HASKELL_PROBE_BUILD_DIR)/hello.platform.elf | sort -u | wc -l

# Combine the complete Haskell PD with the already-built sDDF timer service.
# The timer client metadata is generated by scheduler-wait-probe; setvar data
# patches the linker's .timer_client_config section before Microkit packaging.
haskell-qemu-image: haskell-platform-inventory scheduler-wait-probe \
		$(HASKELL_IMAGE_BUILD_DIR)/rust_entropy_rng.elf
	mkdir -p $(HASKELL_IMAGE_BUILD_DIR)
	cp haskell_probe/haskell.system $(HASKELL_IMAGE_BUILD_DIR)/haskell.system
	cp $(SCHEDULER_PROBE_OUTPUT_DIR)/timer_driver.elf $(HASKELL_IMAGE_BUILD_DIR)/timer_driver.elf
	$(LLVM_OBJCOPY) --update-section .timer_client_config=$(SCHEDULER_PROBE_OUTPUT_DIR)/timer_client_client.data \
		$(HASKELL_PROBE_BUILD_DIR)/hello.platform.elf $(HASKELL_IMAGE_BUILD_DIR)/client.elf
	$(MICROKIT_TOOL) $(HASKELL_IMAGE_BUILD_DIR)/haskell.system \
		--search-path $(HASKELL_IMAGE_BUILD_DIR) --board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) -o $(HASKELL_IMAGE_BUILD_DIR)/loader.img \
		-r $(HASKELL_IMAGE_BUILD_DIR)/report.txt

haskell-qemu-run: haskell-qemu-image
	$(QEMU) -machine virt,virtualization=on,gic-version=2 -accel tcg \
		-cpu cortex-a53 -smp 1 -m 2G -nographic -nic none \
		-device virtio-rng-device,bus=virtio-mmio-bus.8 \
		-kernel $(HASKELL_IMAGE_BUILD_DIR)/loader.img \
		-global virtio-mmio.force-legacy=false -d guest_errors

haskell-qemu-smoke: haskell-qemu-image
	python3 haskell_qemu_smoke.py $(HASKELL_IMAGE_BUILD_DIR)/loader.img $(QEMU)

$(HASKELL_NET_FRAGMENT_DIR)/net.system: ../../tests/net/dhcp_client.c \
		../../tests/net/haskell_tcp_service.c ../../tests/net/include/haskell_tcp_protocol.h \
		../../tests/net/net.mk ../../tests/net/meta.py
	PATH="/opt/homebrew/opt/llvm/bin:$$PATH" $(MAKE) -C ../../tests/net MICROKIT_SDK=$(MICROKIT_SDK_ABS) \
		MICROKIT_BOARD=$(MICROKIT_BOARD) MICROKIT_CONFIG=$(MICROKIT_CONFIG) \
		SDDF=$(SDDF_SOURCE) PYTHON=$(SDDF_PYTHON) \
		MBEDTLS_DIR=$(abspath ../../.cache/mbedtls-3.6.2) \
		BUILD_DIR=$(HASKELL_NET_FRAGMENT_DIR) $@

haskell-net-qemu-image: haskell-qemu-image $(HASKELL_NET_FRAGMENT_DIR)/net.system
	mkdir -p $(HASKELL_NET_IMAGE_DIR)
	cp $(HASKELL_IMAGE_BUILD_DIR)/client.elf $(HASKELL_IMAGE_BUILD_DIR)/timer_driver.elf \
		$(HASKELL_IMAGE_BUILD_DIR)/rust_entropy_rng.elf $(HASKELL_NET_IMAGE_DIR)/
	cp $(HASKELL_NET_FRAGMENT_DIR)/eth_driver.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/dhcp_client.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_rx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_virt_tx.elf \
		$(HASKELL_NET_FRAGMENT_DIR)/network_copy.elf $(HASKELL_NET_IMAGE_DIR)/
	python3 ../../tests/merge_network_system.py --haskell-tcp \
		--base $(HASKELL_IMAGE_BUILD_DIR)/haskell.system \
		--network $(HASKELL_NET_FRAGMENT_DIR)/net.system \
		--output $(HASKELL_NET_IMAGE_DIR)/haskell-net.system \
		--elf-dir $(HASKELL_NET_IMAGE_DIR)
	$(MICROKIT_TOOL) $(HASKELL_NET_IMAGE_DIR)/haskell-net.system \
		--search-path $(HASKELL_NET_IMAGE_DIR) --board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) -o $(HASKELL_NET_IMAGE_DIR)/loader.img \
		-r $(HASKELL_NET_IMAGE_DIR)/report.txt

haskell-net-qemu-smoke: haskell-net-qemu-image
	python3 haskell_net_qemu_smoke.py $(HASKELL_NET_IMAGE_DIR)/loader.img $(QEMU)

$(HASKELL_IMAGE_BUILD_DIR)/rust_entropy_rng.elf: \
		../../rust/src/entropy_rng.rs ../../rust/src/entropy_protocol.rs \
		../../rust/src/virtio_rng.rs ../../rust/Cargo.toml ../../rust/Cargo.lock
	mkdir -p $(HASKELL_IMAGE_BUILD_DIR)
	cd ../../rust && SEL4_INCLUDE_DIRS=$(abspath $(BOARD_DIR)/include) \
		cargo build --locked --release --target aarch64-sel4-microkit \
		--target-dir $(abspath ../../build/rust-target) --bin rust_entropy_rng
	cp $(RUST_ENTROPY_ELF) $@

$(BUILD_DIR)/rts-adapter/sddf_time_bridge.o: sddf_time_bridge.c \
		$(GHC_GENERATED_HEADERS)/ghcplatform.h \
		$(GHC_GENERATED_HEADERS)/ghcautoconf.h
	mkdir -p $(BUILD_DIR)/rts-adapter
	$(CC) $(CFLAGS) $(GHC_RTS_INCLUDE_FLAGS) $(SDDF_INCLUDE_FLAGS) -c $< -o $@

timer-service-probe:
	PATH="/opt/homebrew/opt/llvm/bin:$$PATH" $(MAKE) -C ../../.cache/sddf-personal-os/examples/timer \
		BUILD_DIR=$(TIMER_PROBE_BUILD_DIR) TOP=$(TIMER_PROBE_TOP) \
		MICROKIT_SDK=$(MICROKIT_SDK_ABS) SDDF=$(SDDF_SOURCE) \
		MICROKIT_BOARD=$(MICROKIT_BOARD) MICROKIT_CONFIG=$(MICROKIT_CONFIG) \
		PYTHON=$(SDDF_PYTHON) all

# Link GHC's actual awaitEvent/GetTime objects into a timer-client PD. The
# minimal client supplies just the omitted RTS scheduler symbols and queues.
scheduler-wait-probe: $(BUILD_DIR)/rts-adapter/AwaitEvent.o \
		$(BUILD_DIR)/rts-adapter/GetTime.o \
		$(BUILD_DIR)/rts-adapter/sddf_time_bridge.o
	$(MAKE) -C scheduler_probe \
		BUILD_DIR=$(SCHEDULER_PROBE_BUILD_DIR) \
		MICROKIT_SDK=$(MICROKIT_SDK_ABS) SDDF=$(SDDF_SOURCE) \
		MICROKIT_BOARD=$(MICROKIT_BOARD) MICROKIT_CONFIG=$(MICROKIT_CONFIG) \
		PYTHON=$(SDDF_PYTHON) GHC_RTS_INCLUDE_FLAGS="$(GHC_RTS_INCLUDE_FLAGS_ABS)" \
		GHC_RTS_ADAPTER_OBJECTS="$(GHC_RTS_ADAPTER_OBJECTS)" all

$(OSMEM_BUILD_DIR)/microkit_runtime_probe.o: microkit_runtime_probe.c \
		$(GHC_GENERATED_HEADERS)/ghcplatform.h \
		$(GHC_GENERATED_HEADERS)/ghcautoconf.h | $(OSMEM_BUILD_DIR)
	$(CC) $(CFLAGS) $(GHC_RTS_INCLUDE_FLAGS) -DGHC_OSMEM_INTEGRATION -c $< -o $@

$(OSMEM_BUILD_DIR)/OSMem.o: $(GHC_RTS_SOURCE)/rts/sel4/OSMem.c \
		$(GHC_GENERATED_HEADERS)/ghcplatform.h \
		$(GHC_GENERATED_HEADERS)/ghcautoconf.h | $(OSMEM_BUILD_DIR)
	$(CC) $(CFLAGS) $(GHC_RTS_INCLUDE_FLAGS) -c $< -o $@

$(OSMEM_BUILD_DIR)/microkit_runtime_probe.elf: \
		$(OSMEM_BUILD_DIR)/microkit_runtime_probe.o $(BUILD_DIR)/sel4_libc.o \
		$(OSMEM_BUILD_DIR)/OSMem.o $(BUILD_DIR)/mblock_arena.o
	$(LD) $(LDFLAGS) -o $@ $^ --start-group -lmicrokit \
		-T$(BOARD_DIR)/lib/microkit.ld --end-group

$(OSMEM_BUILD_DIR)/capability_service.elf: $(BUILD_DIR)/capability_service.elf | $(OSMEM_BUILD_DIR)
	cp $< $@

$(OSMEM_BUILD_DIR)/loader.img: $(OSMEM_BUILD_DIR)/microkit_runtime_probe.elf \
		$(OSMEM_BUILD_DIR)/capability_service.elf microkit_runtime_probe.system
	$(MICROKIT_TOOL) microkit_runtime_probe.system --search-path $(OSMEM_BUILD_DIR) \
		--board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) -o $@ \
		-r $(OSMEM_BUILD_DIR)/report.txt

clean:
	rm -rf $(BUILD_DIR)
