# GHC / Microkit native runtime probe

The initial link experiment below records how this port started. As of
2026-10-07, patched GHC builds a Haskell protection domain that boots under
Microkit/QEMU: it prints, completes `threadDelay`, and obtains two distinct
16-byte samples through a protected call to the isolated VirtIO RNG service.
Run `make -C experiments/ghc-microkit haskell-qemu-smoke` from the repository
root to rebuild and verify it. The smoke prints no sample bytes. This is a
bounded native runtime and service test, not the SimpleX client.

The network variant now also passes
`make -C experiments/ghc-microkit haskell-net-qemu-smoke`: native Haskell
resolves `personal-os.test` through the isolated network domain, uses the
returned IPv4 address to open a TCP session through protected calls,
sends a 192-byte stream in two writes, receives it in bounded reads, closes,
reconnects, and cancels a stalled read before successfully reconnecting again.
It also drains a 12 KiB unsolicited response through the 8 KiB network-PD
buffer, exercising receive backpressure across multiple reads.
The Haskell PD maps only its shared request page; the network PD owns VirtIO
Ethernet and DMA. The fixture endpoint is 10.0.2.2:1241.

The upstream SimpleX agent now also boots in QEMU with its encrypted AgentStore
and uses that protected network path to connect to an isolated local SMP
fixture. `make guest-simplex-agent-runtime-qemu-smoke` verifies the upstream
queue create/secure/delete lifecycle, persistence across a same-key reboot, and
wrong-key rejection. This is not yet a message exchange; the service still
supports one TCP session at a time, and the QEMU clock/network fixtures do not
validate Pixel Wi-Fi or device clock trust.

The separate-client checkpoint is available as
`make guest-simplex-agent-cli-interop-qemu-smoke`. It builds an isolated guest
variant, joins it from the upstream SimpleX CLI, then checks that the CLI parses,
displays, and acknowledges the guest's encrypted chat message. Reverse delivery
from the CLI to the guest and persistence across an external-peer restart remain
open; the test does not touch a phone.

## Reproduce

Compile `Main.hs` with the Debian ARM64 GHC 9.6.6 in the `SimpleX Linux Probe`
UTM guest:

```sh
ghc -c Main.hs
file Main.o
```

Copy `Main.o` to the Mac and try the project's QEMU AArch64 Microkit linker:

```sh
ld.lld -m aarch64elf \
  -L .cache/microkit-sdk-2.3.1/board/qemu_virt_aarch64/debug/lib \
  Main.o -lmicrokit \
  -T .cache/microkit-sdk-2.3.1/board/qemu_virt_aarch64/debug/lib/microkit.ld \
  -o ghc-microkit-hello.elf
```

The target SDK path is relative to the repository root. This link is expected
to fail: GHC-generated objects refer to the `base` package and GHC RTS, neither
of which the Microkit SDK contains. The Microkit library also expects the PD to
provide its `init` and `notified` callback functions. The Linux-built GHC RTS
archive imports POSIX facilities such as pthreads, `mmap`/`mprotect`, signals,
clocks, and file descriptors; satisfying the missing symbols with the Linux
RTS would still not yield a working Microkit PD.

The experiment establishes that GHC emits an AArch64 ELF relocatable object,
but not that the runtime is portable to seL4. The project direction is to
port the runtime for native Microkit protection domains. The initial C-only
proof below establishes bounded heap allocation, entry callbacks, logging,
and one capability call. A scheduler probe now links the real patched GHC
`awaitEvent` object and exercises its timer-queue wakeup with genuine GHC
`Capability`, `CapIOManager`, and `StgTSO` structures. The minimal Haskell
`main` and end-to-end `threadDelay` now pass; a single-session streaming TCP
capability also passes. Keep those OS interfaces usable by Rust and C apps
too. See
[`docs/simplex-port-feasibility.md`](../../docs/simplex-port-feasibility.md)
for the staged plan and current service gaps.

## Native Microkit boundary probe

The C probe checks the actual Microkit entry callbacks, an 8 MiB fixed arena
split into eight 1 MiB GHC-style megablocks, a 64 KiB bump heap, and one
synchronous capability call. It also tests `sel4_libc.c`, a separate bounded
1 MiB single-thread allocator with memory and string functions used by the
first GHC link. The allocator is deliberately separate from GHC's megablock
arena. Its QEMU test covers zeroed allocation, overlapping moves, string
operations, sorting, page alignment, resize, free, and coalescing. This is a small
freestanding runtime subset, not a POSIX libc. Its stdout/stderr writes go to
Microkit debug output; stdin and filesystem operations report `ENOSYS`, the
environment is empty, and stdout/stderr are byte streams rather than terminals.
A small diagnostic-only stdio layer formats strings and integers to those
streams; stdin, file-backed streams, floating-point formats, process services,
and terminal behavior remain unsupported. The megablock allocator checks
alignment, contiguous allocation, free/reuse,
double-free rejection, and capacity bounds. The bump heap checks alignment,
read/write, and bounds. The probe emits `GHCRTS|POSIX_WRITE_OK`,
`GHCRTS|SEL4_LIBC_OK`, `GHCRTS|MBLOCK_ARENA_OK`, and `GHCRTS|READY` markers.
It also checks empty `getenv`, non-terminal stdout metadata, explicit
`ENOSYS` from stdin and file open, `snprintf` formatting/truncation, and
`fprintf` output plus flush. This remains a
platform-shim proof: it does not execute Haskell code, implement GHC's RTS
heap manager, or establish the right final heap size for an application.

The first GHC-side adapter is in `OSMem.c`, with its source-selection and
single-threaded type patch in `../../patches/ghc-9.6.6-microkit-osmem.patch`.
It maps GHC's megablock and large-address-space reserve/commit hooks to the
same statically mapped, aligned 8 MiB arena; GHC's nominal 256 GiB reserve
request is clamped to that arena. The QEMU probe compiles this exact GHC source
and exercises both allocation paths. The compile fixture generates `ghcplatform.h`,
`ghcautoconf.h`, and RTS event-log headers from the configured GHC 9.6.6 tree
using the same rules and event-type generator as Hadrian. The platform header
therefore describes `aarch64-unknown-sel4`; the feature header comes from the
GHC configure output with host-only process, signal, and wall-clock features
disabled for Microkit. Run `make -C experiments/ghc-microkit
rts-adapter-compile` to compile the memory, time, and event-wait adapters as
AArch64 ELF objects.

The GHC-generated feature header also exposed a cross-build mismatch: its
autoconf probes describe the Mac that built GHC, so they report pthread and
POSIX headers that the target does not provide. The seL4 platform branches
now take precedence over those host feature macros. Do not use the host's
POSIX source files for the seL4 target.

`sel4/GetTime.c` now obtains monotonic nanoseconds and Unix epoch time through
two explicit service hooks. Process CPU time and page-fault counts return zero
because Microkit does not expose those counters to the PD. A direct
`CNTVCT_EL0` read was tested and faulted under the current seL4 configuration,
so the adapter does not assume user access to the architected timer. The
fixture's `sddf_time_bridge.c` now implements the monotonic hook through the
existing sDDF timer-client API when the PD is given a configured timer
capability. The Unix-epoch hook still needs a trusted wall-clock service and
is not connected to the image.

The timer bridge has a standalone QEMU proof using the real sDDF timer driver
and the same `sddf_time_bridge.c` source:

```sh
make -C experiments/ghc-microkit timer-service-probe
qemu-system-aarch64 \
  -machine virt,virtualization=on -cpu cortex-a53 -smp 1 -m 2G -nographic \
  -kernel build/ghc-timer-service-probe/loader.img \
  -global virtio-mmio.force-legacy=false -d guest_errors
```

The probe calls the GHC monotonic-time hook, passes an absolute deadline to
`sel4_rts_wait_for_event`, and verifies that the shared bridge arms the sDDF
timer, blocks in `seL4_Wait` on Microkit 2.3.1's notification cap slot, and
dispatches the pending channel callbacks before returning. It prints
`GHCTIMER|START ns=...`, `GHCTIMER|WAIT_DISPATCH_OK channel=...`, and
`GHCTIMER|WAKE_OK elapsed_ns=...` (about one second). This exercises the exact
platform wait hook referenced by the patched GHC `awaitEvent` source. The
bridge dispatches all pending badge bits so unrelated channel notifications
are not silently lost. Production integration still needs to preserve
Microkit's deferred notification and IRQ-acknowledgement semantics. This
standalone probe does not run GHC; the full Haskell `threadDelay` smoke test is
described below.

To exercise the actual GHC scheduler wait logic, build and run the second
probe:

```sh
make -C experiments/ghc-microkit scheduler-wait-probe
qemu-system-aarch64 \
  -machine virt,virtualization=on -cpu cortex-a53 -smp 1 -m 2G -nographic \
  -kernel build/ghc-scheduler-wait-probe/loader.img \
  -global virtio-mmio.force-legacy=false -d guest_errors
```

It links the patched `AwaitEvent.o`, `GetTime.o`, and shared timer bridge. The
probe queues a real GHC `StgTSO` at a future deadline, calls `awaitEvent`, then
checks that GHC moved it from the sleeping queue to the run queue and cleared
its blocked state. It prints `GHCSCHED|AWAIT_EVENT_OK`. The run queue insertion
is a small test stub and the rest of GHC's runtime is not initialized, so this
validates the scheduler hook and its GHC data structures. The separate full
RTS smoke test below also exercises Haskell `threadDelay`.

## Full compiler bootstrap status

The patched GHC 9.6.6 stage1 cross-compiler builds for the `sel4` target. With
GHC's `quickest+native_bignum` flavor and small freestanding header shims,
target RTS constants, `ghc-prim`, and the bundled AArch64 libffi archive now
build. The Microkit RTS now omits GHC's ELF runtime linker and object-unloading
code; static Haskell objects are expected to be resolved by the final image
link. The cross-compiler's AArch64 assembly printer now selects relocation
syntax from the target OS, rather than the macOS build host, and emits ELF
assembly that the target Clang wrapper can assemble. In the RTS, `RtsUtils.c`
now avoids the POSIX wall-clock header and reports `time unavailable` in
profiling metadata; its process-style sleep hook returns unsupported because
Microkit waits are handled by GHC's event/timer adapter. A direct target Clang
compile of `RtsUtils.c` passes with GHC's generated target headers and the
fixture's freestanding declarations. The cached GHC 9.6.3 host compiler and
Cabal 3.10.3 can launch Hadrian when passed explicitly. The real stage1 RTS
package now builds with `stage1:lib:rts`, including GHC-generated
`AutoApply.cmm`, and is registered in the stage1 package database. The source
compile remains reproducible with
`make -C experiments/ghc-microkit rts-utils-compile`. A minimal Haskell
`Main.hs` now compiles and links with no undefined imports, then boots as a
Microkit PD under QEMU. Its `threadDelay 200000` wakes through the sDDF timer
in about 202 ms, then exits with status 0.

Hadrian's stage1 static RTS archive contains 111 members and passes a
whole-archive `ld.lld -r` combination as AArch64 ELF. The combined object has
123 unresolved imports, chiefly `base`/`ghc-prim` closures, freestanding
libc/FFI functions, process/file APIs, and Microkit time/event service hooks.
The target `base` port is captured in
[`../../patches/ghc-9.6.6-microkit-base-encoding.patch`](../../patches/ghc-9.6.6-microkit-base-encoding.patch).
It makes UTF-8 the explicit locale, filesystem, and
foreign-string encoding, and rejects encodings outside its built-in UTF and
Latin-1 codecs; Microkit has no locale or iconv service yet. Its configure
probe therefore no longer demands iconv. The target's freestanding header
declarations now pass Cabal validation and Hadrian successfully builds the
stage1 `base` archive. `experiments/ghc-microkit/Main.hs` also compiles to an
AArch64 object against that target package database. The earlier full-archive
inventory reported 123 unresolved imports before the freestanding runtime and
platform closure were added; the current strict platform link and QEMU boot
pass. The fixture's LLD adapter remains useful for measuring the deliberately
incomplete baseline link, and the Microkit configure patch removes the
host-derived `libm` and `libdl` dependencies. The restricted `poll`
implementation supports only zero-descriptor timer waits through the RTS
timer bridge; descriptor readiness returns `ENOSYS`. The earlier
hand-assembled RTS archive is superseded by the successful Hadrian build.
Repeat the compile check with
`make -C experiments/ghc-microkit haskell-compile-probe`. To recreate the
diagnostic-only ELF and its unresolved-import log, run
`make -C experiments/ghc-microkit haskell-link-inventory`; the current ELF has
45 unresolved symbols and must not be run. The full platform link is separate
and strict by default; it writes `build/haskell-hello/hello.platform.elf`.
The diagnostic link writes
`build/haskell-hello/why-extract.txt`, which records the archive members LLD
selected to satisfy the fixture. `haskell-link-gc-inventory` repeats this as a
diagnostic with section garbage collection enabled; it currently leaves 44
unresolved symbols. That GC pass removes a few unused C sections, but the
current GHC `Float` module still keeps math imports reachable, so it does not
replace the target math or RTS work.
Heap-profiler source compiles, but writing profiles still needs a Microkit
file service.

An isolated AArch64 OpenLibm trial resolves the math imports without a hosted
libc and lowers the unresolved-import count to 11 (10 with section GC). The
Microkit/sDDF link reaches 8 unresolved symbols before adding compiler-rt
and the explicit unavailable-wall-clock adapter. To
reproduce, check out OpenLibm commit
`5fe399749f9276eaa0b8403e507470da05cbbb3f` under
`../../.cache/ghc-port/openlibm`, build `libopenlibm.a` with Clang's
`aarch64-none-elf` target and the fixture's freestanding headers, then run
both OpenLibm inventory targets. For example, from the repository root:

```sh
git clone https://github.com/JuliaMath/openlibm .cache/ghc-port/openlibm
git -C .cache/ghc-port/openlibm checkout 5fe399749f9276eaa0b8403e507470da05cbbb3f
make -C .cache/ghc-port/openlibm libopenlibm.a ARCH=aarch64 USECLANG=1 \
  CC=clang AR=/opt/homebrew/opt/llvm/bin/llvm-ar \
  CFLAGS="-target aarch64-none-elf -mcpu=cortex-a53 -ffreestanding -fno-builtin -O2 -I$(pwd)/experiments/ghc-microkit/freestanding-headers" -j4
make -C experiments/ghc-microkit haskell-openlibm-inventory haskell-openlibm-gc-inventory
```

This keeps the math-library experiment separate from the baseline 45/44-symbol
inventories after the target RTS option parser was removed.

The residual quad helpers and `__clear_cache` come from LLVM compiler-rt,
compiled from source revision
`9b2ad1942cb13925325c5a8e458aac88767e2c44`. Its AArch64 `__clear_cache`
implementation performs real instruction-cache maintenance; libffi references
it even though this target rejects runtime executable-memory creation. Clone
LLVM sparsely and build this subset with:

```sh
git clone --filter=blob:none --no-checkout --sparse https://github.com/llvm/llvm-project.git .cache/ghc-port/llvm-project
git -C .cache/ghc-port/llvm-project fetch --depth 1 origin 9b2ad1942cb13925325c5a8e458aac88767e2c44
git -C .cache/ghc-port/llvm-project checkout --detach FETCH_HEAD
git -C .cache/ghc-port/llvm-project sparse-checkout set compiler-rt/lib/builtins
make -C experiments/ghc-microkit haskell-compiler-rt-quad
```

`make -C experiments/ghc-microkit haskell-platform-inventory` adds the real
Microkit SDK library and the sDDF timer bridge to that link. It resolves
`microkit_dbg_putc`, the monotonic-clock hook, and the event-wait hook. The
inventory links Microkit's `init`/`notified` entry adapter and resolves the
debug, monotonic-time, and timer-wait hooks. A wall-clock adapter returns
unavailable instead of inventing a timestamp; GHC reports an error if an app
requests Unix epoch time. The fixture
libc now reports Haskell exit status and parks the PD, implements fatal abort,
and supplies the limited `strerror` strings needed by the static image. The
compiler-rt archive resolves the three quad-precision helpers and cache-flush
routine used by the selected OpenLibm and libffi objects. The
Microkit `flushExec` path fails explicitly because runtime-generated code is
forbidden. The strict platform link resolves every import, and the resulting
Haskell PD has booted successfully under QEMU.

GHC initializes tracing buffers even when event logging is not enabled. The
Microkit target bounds each event buffer to 64 KiB (configurable with
`SEL4_EVENT_LOG_SIZE`); the hosted default is 2 MiB. The Haskell PD gets a
1 MiB Microkit C stack because the 8 KiB default faulted during RTS startup.
Rebuild and run the end-to-end smoke test with:

```sh
make -C experiments/ghc-microkit haskell-qemu-run
```

The expected Haskell markers are `hello from Haskell`, a
`HASKELL_DELAY_NS|...` value near 200,000,000, `HASKELL_ENTROPY|PASS`, and
`HASKELL_TCP|SKIP` (no network service in this base image), then
`HASKELL_EXIT|0`. The network smoke instead requires `HASKELL_TCP|PASS`.
The exit callback parks the component, so stop QEMU with Ctrl-C after observing
the marker. This boot validates RTS startup, one timer-driven `threadDelay`,
a simple `IO` action, and a Haskell FFI call to the existing entropy service.
It does not yet validate general event delivery
during application execution, filesystem support, wall-clock access, or
foreign callback closures.

The TCP service is deliberately bounded: one IPv4 session, up to 128 bytes per
send/receive call, and an 8 KiB receive buffer. `NativeNetwork.hs` exposes
bounded DNS, connect, ByteString send-all and receive-some/exactly, close, and
cancel with monotonic deadlines and typed errors. Hadrian builds native
`bytestring` for the seL4 target. The QEMU smoke resolves a local fixture
name, checks reconnect and timeout recovery, and transfers 20 KiB in each
direction through the small calls. Callers must explicitly cancel a timed-out
session before reconnecting. `NativeTLSBackend.hs` adapts these calls to the
upstream TLS 1.9 `Backend` API; `make native-tls-backend-typecheck` checks the
host package and `make guest-tls-backend-compile` compiles the adapter against
the cross-built guest TLS archive with host package databases excluded.
`make guest-tls-net-qemu-smoke` now runs a TLS 1.3 handshake and encrypted echo
against a host fixture using an in-memory test CA. The wrong hostname and an
expired certificate are rejected. `NativeSMPIdentity.hs` follows the upstream
SimpleXMQ certificate-chain and SHA-256 identity-pin policy; the same QEMU
smoke accepts the fixture's expected pin even with an alternate transport
hostname and rejects an incorrect pin or expired certificate. Public
DNS behavior and compatibility with upstream GHC's
`network` package remain open. Its protected-call labels and
page format are in
`tests/net/include/haskell_tcp_protocol.h`; the network PD snapshots the
send request before returning from the call. This is a real network path, but
it is not yet a SimpleX transport.

`make guest-tls-deps` builds target `mtl`, `transformers`, `containers`,
`stm`, `filepath`, `text`, and `binary`, then cross-builds pinned
`data-default-class`, `cereal`, `basement`, `memory`, `crypton`, `hourglass`,
`pem`, `asn1-types`, `asn1-encoding`, `asn1-parse`, `crypton-x509`,
`crypton-x509-store`, `crypton-x509-validation`, `hashable`, `async`, and
`tls` from the local Hackage cache. `cross-build-tls-dep.py` uses
LLVM `ar`, checks that each static archive contains its AArch64 ELF objects,
and rejects dependencies absent from the target databases before registering each library in
`.cache/sel4-tls-deps/guest-package.conf.d` at the repository root.

The cross-build makes freestanding adaptations in its extracted cache:
`basement` uses a fixed terminal size, `memory` omits its private POSIX mmap
module, `crypton` disables Argon2 threads and obtains random bytes through the
isolated Microkit RNG protection domain, and `hourglass` calls the guest
wall-clock hook. The X.509 store keeps in-memory trust anchors while its
filesystem-loading calls fail explicitly. TLS 1.9 ticket timing uses the
same epoch clock, and its default socket backend is disabled for the custom
`Backend`. The ordinary Haskell image's wall-clock hook reports unavailable;
the `hourglass` bridge aborts rather than inventing certificate time. The
separate TLS QEMU image obtains host-controlled PL031 time through a clock PD.
This is test input, not a production trust source. A freestanding
UTC converter supplies `gmtime_r` and `localtime_r` to the TLS/X.509 dependency
graph; `make calendar-utc-test` checks epoch, negative time, and leap days.
This converts supplied timestamps and does not provide a clock. The
`guest-tls-link-inventory` target links the full AArch64 ELF with zero
unresolved imports. `make guest-tls-qemu-smoke` boots a separate image and
checks SHA-256, the custom backend's zero-length receive path, two
distinct 32-byte samples through `crypton` and the QEMU VirtIO RNG PD, and
`Network.TLS.contextNew` with a real random seed and configured cipher list.
`make guest-tls-net-qemu-smoke` adds the isolated lwIP network PD and runs a
TLS 1.3 echo with the default X.509 checks, a test CA and SAN hostname. A
second connection must reject the wrong hostname; two more connections check
the SimpleX-style identity pin and reject a wrong pin. A QEMU run dated after
the fixture's expiration must reject the certificate under both default X.509
checks and SimpleX-style pinning. The test CA/private
keys are public fixtures; the Pixel still needs its own trusted clock,
hardware entropy source, network path, and SimpleX server identity policy.

`make guest-smp-net-qemu-smoke` starts an isolated upstream SimpleX SMP test
server from the existing loopback-bound host build under ignored `.cache`, then
boots a separate Microkit/QEMU Haskell image. The guest authenticates the
server's certificate chain using its pinned SHA-256 identity, validates the
TLS-unique session ID in the 16 KiB server handshake block, sends the
version-6 client handshake and a PING, requires PONG, then signs a `NEW`
command with a guest-generated Ed25519 key and checks the returned `IDS`
without logging queue credentials. It sends an opaque test payload to the
new sender ID, requires the server's `OK`, then signs `GET` with the recipient
key and checks that the relay returns an encrypted `MSG`. Its 16 KiB TLS block
reads now use the unchanged upstream `Simplex.Messaging.Transport.Buffer`
module cross-compiled into the guest. The upstream
`Simplex.Messaging.Encoding` module also compiles unchanged and now encodes
the probe's length-prefixed fields and parses a real server block length.
`NativeSMPTransport.hs` now owns the guest's fixed-block TLS read/write,
padding validation, and single-transmission batch framing; the queue probe
uses that module for the full PING, NEW, SEND, and GET exchange. It is a
platform adapter, not upstream `Transport.Client`: that upstream module still
opens POSIX sockets (and supports SOCKS), whereas the guest opens its TCP
session through the isolated network protection domain.
`NativeSMPConnection.hs` owns the pinned TLS handoff and cleanup over that
already-open capability. The QEMU fixture now resolves `personal-os.test`
through the network PD before connecting, then reconnects with an incorrect
identity pin and requires TLS certificate rejection (`UnknownCa`).
`make host-simplexmq-backend-typecheck` applies
`patches/simplexmq-microkit-tls-backend.patch` to the pinned, ignored
SimpleXMQ 7.0.0.6 checkout (if needed) and force-typechecks its modified
`Transport` and `Transport.Client` source against the host package's 62 pinned
dependencies. It then typechecks upstream `Client.hs` with its source
dependency closure, using two GHC 9.6 compatibility fixes to `Crypto.hs` only
in a throwaway copy. The patch adds `connectTLSBackend`,
`runTLSBackendTransportClient`, and `getProtocolClientWithTLSBackend` beside
the POSIX socket entry points. The new protocol-client entry point uses the
same upstream handshake and client loops, with a platform-supplied TLS stream.
The host check is a source-level seam; separate target checks now compile
`Client.hs` and its dependency objects, but the client has not run as an SMP
client in Microkit and the agent layer remains unbuilt.
`make host-simplexmq-sqlite-vfs-typecheck` applies
`patches/simplexmq-microkit-sqlite-vfs.patch` and typechecks the pinned agent
SQLite store both with the normal host path and with `SIMPLEX_MICROKIT`. The
target path opens SQLCipher through the registered `nexatrode-storage` VFS,
derives first-run state from the migrations table, and omits directory and
backup-file operations that the app-scoped vault does not provide. The
SQLite/VFS/vault pieces compile and their host and QEMU storage tests pass;
`NativeSQLiteVFS.hs` and `simplex_sqlite_init_microkit.c` now provide the
registration call. The Messages protection domain maps the transfer page and
has bounded protected-call endpoints to its vault, entropy service, and QEMU
RTC clock. The patched Haskell store calls the initializer before
`DB.openWithVfs`. The complete SimpleX process still needs to be linked into
that domain with the VFS and remaining dependencies, then validated by running
upstream store migrations. `./dev test-sqlcipher-vault --timeout 60` now links
the pinned SQLCipher amalgamation and LibTomCrypt provider into a Microkit
client PD, connects it to the real disk-backed Messages vault and entropy PD,
and verifies encrypted create, persistence across a QEMU reboot, correct-key
readback, and wrong-key rejection. The database uses a QEMU-only test key, not
production key management.
`make guest-simplex-sqlite-vfs-compile` cross-compiles the Haskell FFI wrapper,
Microkit vault/entropy/clock initializer, and VFS adapter. `python3 dev build`
validates the protected-call topology and priorities without booting the image.
`make guest-simplex-sqlite-core-compile` also compiles the pinned SQLite
amalgamation for AArch64 with `SQLITE_OS_OTHER`, serial access, and WAL disabled.
This is a codec-disabled compile probe. The separate SQLCipher target now links
the LibTomCrypt provider with SQLite OS hooks and the target VFS; encrypted
AgentStore migrations still need the complete Haskell runtime and agent
dependency closure.
`make guest-simplex-agentstore-db-check` cross-builds and registers the pinned
`direct-sqlcipher` and `sqlcipher-simple` Haskell packages, checks the custom
VFS `open2` API, and typechecks the patched upstream `Agent.Store.SQLite.DB`
wrapper for AArch64. `make guest-simplex-agentstore-migrations-check` now
compiles the static SQLite migration catalog into 53 AArch64 objects; its
guest-only staging pass lowers SQL quasiquotes to `Query` literals because the
cross-compiler has no Template Haskell interpreter. A staged Agent Protocol
change removes its direct import of the full Client module by copying the
small proxy-error schema and XFTP error schema with their JSON tags. The
staged file-transfer protocol now uses a narrow copy of the upstream
`authTransmission` helper and protocol-facing XFTP types rather than importing
the full Client and file-transfer transport modules directly. Agent.Protocol
also carries a target-local FileErrorType schema to avoid importing the full
file-transfer data/client layer. Its source dependency trace no longer reaches
the Client, HTTP/2, or file-transfer transport modules. In the target staging
overlay, the Agent.Protocol, XFTP error, CommandError, BlockingInfo, and
FileParty, transport-error, file-description, and ratchet JSON instances use
Aeson generic codecs with the same options as the upstream splices. The
target-local YAML subset emits JSON (valid YAML 1.2) and accepts JSON only; it
does not parse arbitrary YAML. `make guest-simplex-agent-protocol-check`
cross-compiles the complete staged Agent.Protocol dependency closure into 43
AArch64 objects. This clears the earlier interpreter and missing C-header
barriers for this closure. It is still below a linked/running agent: remote
control codecs and full YAML parsing are not part of this target subset, and
the store runner and file I/O remain unavailable.
`make guest-simplex-client-inventory` cross-builds and audits the portable
`unliftio-core` and `safe-exceptions` packages, then typechecks the patched
upstream `Transport.Client` source closure with the seL4 compiler in no-code
mode and `SIMPLEX_MICROKIT` enabled, then emits target objects for that source
dependency closure. A guest-only source transform removes the
POSIX socket/SOCKS implementation and the SQLite serialization instances used
only by persistence, retaining the TLS backend path. A narrow guest
`Control.Logger.Simple` adapter writes errors and warnings to the Microkit
console. The inventory gets past the logger and
`UnliftIO` adapters. `make guest-simplex-json-prereqs` cross-builds the full
currently identified JSON closure: Aeson, base64-bytestring, iso8601-time,
and their target dependencies. The transport typecheck and object build pass
with 19 AArch64 objects, including upstream `Crypto`, `Transport`, and
`Transport.Client`. The same guest overlay now typechecks upstream `Client.hs`
and emits 27 AArch64 objects for that source dependency closure. The guest
call path rejects POSIX transport and uses the supplied TLS backend, preserving
the upstream protocol handshake and client loops. This is a compile milestone,
not yet a running agent or message exchange. The target overlay drops database
conversions from shared protocol/crypto types, provides generic JSON for a few
shared records, and skips upstream JSON Template Haskell code generation where
the freestanding compiler has no interpreter; complete client configuration
JSON remains open. The target Aeson build registers 34 audited
AArch64 objects. `unordered-containers` and Aeson use direct target GHC builds
because Cabal's solver selects host GHC heap packages; splitmix uses a
Microkit entropy hook instead of its POSIX C implementation.
`make guest-simplex-shared-compile` also
builds real AArch64 objects for upstream `Transport.Shared` and that logger
adapter. The patch removes `Transport.Shared`'s socket import, which was only
used as a `String` type alias. The target overlay fences socket code out of
`Transport` and `Transport.Client`; the full `Client.hs` call sites select the
backend entry point in the guest because the host `network` package cannot
provide a protected Microkit network capability.
The latest client inventory emits 62 transport objects and 27 protocol-client
objects. Upstream `Network.HTTP2.Client` typechecks and emits 50 AArch64 objects
with its callback-based transport API; the POSIX socket constructor and
`defaultReadN` are fenced out. The focused `Agent.Client` target typecheck now
passes across the staged upstream dependency closure and emits 179 AArch64
objects with `compile_simplexmq_agent_client.py --objects`. The guest overlay
supplies the required STM/concurrency and logger APIs, generic Aeson codecs
with upstream options, SQLite row codecs, a lowering of 128 static AgentStore
queries to `Query Text` with parameter binding preserved, and explicit
transport-to-agent error translation. The target also routes standard agent
protocol connections through the existing native TLS/network-PD connector.
This remains compile/codegen only: the agent is not yet linked to the existing
SQLCipher VFS/vault capability, the network service is single-session, and the
HTTP/2 backend is absent by default.
See the focused diagnostic script
[`compile_simplexmq_agent_client.py`](compile_simplexmq_agent_client.py) and
the current status in [`docs/simplex-port-feasibility.md`](../../docs/simplex-port-feasibility.md).
The explicit HTTP/2 backend remains unconfigured by default, and the bounded
network-PD TCP/TLS probe is not yet a persistent stream service for the agent.
Its two imported helpers, `parseAll` and `(<$?>)`, have exact, narrow
guest-only shims rather than cross-building the larger upstream `Parsers` and
`Util` modules. The target builds `primitive`, `integer-logarithms`,
`scientific`, both `attoparsec` components, `network-transport`, and GHC's
`time` library for this step; `smp_platform.c` provides the QEMU test clock
and network byte-order functions used by those packages. The image has no
unresolved imports, and the QEMU smoke requires `SMP_UPSTREAM_ENCODING|PASS`.
The generated
server keys remain under ignored `.cache/simplex-qemu-smp/` and are only
development credentials. `SMPLinkProbe.hs` is a small wire probe; it does not
decrypt that payload or exchange a SimpleX agent message. The upstream
Guest source adapters cover the small async/MVar API, and the directory
adapter fails explicitly until a filesystem capability exists.
SQLCipher bindings, the VFS-backed DB wrapper, and all 53 migration objects
cross-compile. The store decision is to preserve upstream SQLCipher and add a
VFS over an app-scoped
storage capability; the current vault blob API is too small and lacks random
access for the agent database. See
[`docs/simplex-port-feasibility.md`](../../docs/simplex-port-feasibility.md)
for the proposed storage sequence and remaining HTTP/2 scope decision.

The SQLite storage work now has three layers in
[`sqlite_vfs_probe/`](sqlite_vfs_probe/). `make -C
experiments/ghc-microkit/sqlite_vfs_probe test` checks database reopen and
capacity handling on host SQLite, plus the C storage adapter's bounded IPC
read/write, short-read, truncate, sync, and journal-delete mapping. The
VFS, vault adapter, and Microkit call shim cross-compile against the pinned
SQLCipher headers with `make -C
experiments/ghc-microkit/sqlite_vfs_probe microkit-compile`. The disk-backed
vault exposes a Messages-only database and rollback-journal capability, and
`python3 dev --timeout 60 test-storage` verifies persistence across QEMU boots
alongside the existing interrupted-write tests. A separate SQLCipher QEMU
smoke now runs the codec-enabled AArch64 library with the Microkit virtio RNG
service and an in-memory storage fixture, checking the entropy IPC, ciphertext,
wrong-key rejection, and correct-key reopen. `./dev test-sqlcipher-vault
--timeout 60` adds a real disk-vault run and confirms an encrypted row survives
a fresh Microkit/QEMU boot. The next storage milestone is running the upstream
Haskell store migrations through this working target path.

The static boundary also removes fork support and runtime writable-executable
page support. The memory-protection entry points fail explicitly if adjustor
code requests them; normal static Haskell code does not need runtime code
generation. The target disables RTS option parsing because a Microkit PD has
no process argument vector, so it no longer pulls numeric parsing routines
from a hosted C library.

Apply the GHC patches in this order: compiler platform and AArch64 assembly
printer (`../../patches/ghc-9.6.6-microkit-platform.patch`), Microkit memory,
timer, and event adapters (`../../patches/ghc-9.6.6-microkit-osmem.patch`),
the static RTS boundary (`../../patches/ghc-9.6.6-microkit-static-rts.patch`),
the base encoding and timer-poll changes
(../../patches/ghc-9.6.6-microkit-base-encoding.patch), the target
configuration filter
(../../patches/ghc-9.6.6-microkit-target-config.patch), the single-PD lifecycle hooks
(../../patches/ghc-9.6.6-microkit-single-pd-hooks.patch), the non-threaded
event-manager hooks (../../patches/ghc-9.6.6-microkit-event-manager.patch),
the no-RTS-options and no-runtime-executable-code target fixes
(../../patches/ghc-9.6.6-microkit-no-rtsopts.patch and
../../patches/ghc-9.6.6-microkit-no-dynamic-exec.patch),
the Microkit event-log buffer limit
(../../patches/ghc-9.6.6-microkit-eventlog-size.patch),
the link-library
probe fix (../../patches/ghc-9.6.6-microkit-link.patch), and the ELF
object-merge fix (../../patches/ghc-9.6.6-microkit-merge.patch). Run autoreconf after
applying the GHC patches so the merge-object configure change is regenerated.
The complete ordered patch stack has been applied successfully to a fresh copy
of the pinned GHC 9.6.6 source archive. The local Hadrian config
uses `aarch64-clang` as the generated GHC compiler/assembler driver so GHC's
Cmm assembly is assembled for `aarch64-none-elf`. The upstream
`TargetHasRTSLinker` setting alone would not omit the sources; it is used for
test selection and platform metadata. Dynamic loading and object unloading
are unavailable in this static-only RTS configuration.

`sel4/AwaitEvent.c` handles GHC's sleeping-thread deadline queue through a
third service hook that arms the timer to the next absolute monotonic deadline
and consumes its notification. The fixture now implements that hook for a
timer-only PD, and the QEMU probe calls the same hook used by `awaitEvent`.
The scheduler probe exercises the actual `awaitEvent` code with GHC's TSO and
capability structures, but does not initialize the full GHC scheduler. General
dispatch still needs to preserve unrelated channels and Microkit's deferred
notifications and IRQ acknowledgements. The adapter fails fast if GHC asks it
to wait on file-descriptor I/O.
Timer-driven async preemption is disabled for this initial seL4 RTS, so Haskell
concurrency would be cooperative until an interrupt-safe event path exists.
The current QEMU runs produced `GHCSCHED|AWAIT_EVENT_OK`,
`GHCTIMER|WAIT_DISPATCH_OK`, and `GHCTIMER|WAKE_OK` with approximately one
second elapsed.

The optional QEMU probe links that exact adapter into a Microkit protection
domain and checks alignment, contiguous allocation, free/reuse, and freeing
the complete arena:

```sh
make -C experiments/ghc-microkit osmem-adapter-probe
qemu-system-aarch64 \
  -machine virt,virtualization=on,gic-version=2 -accel tcg \
  -cpu cortex-a53 -smp 1 -m 2G -nographic -nic none \
  -kernel experiments/ghc-microkit/build/osmem_probe/loader.img \
  -global virtio-mmio.force-legacy=false -d guest_errors
```

The expected additional marker is
`GHCRTS|GHC_OSMEM_ADAPTER_OK: aligned allocation, contiguous runs, reuse,
bounded heap reserve/commit`.
The QEMU probe validates the memory adapter only. The separate scheduler wait
probe links the GHC time and event-wait objects with the timer service; neither
probe runs the full GHC RTS.
The 8 MiB arena is only a bring-up limit; a real Haskell heap needs a measured
budget and all remaining RTS adapters.

Build from the repository root:

```sh
make -C experiments/ghc-microkit
```

Boot the generated image with QEMU:

```sh
qemu-system-aarch64 \
  -machine virt,virtualization=on,gic-version=2 -accel tcg \
  -cpu cortex-a53 -smp 1 -m 2G -nographic -nic none \
  -kernel experiments/ghc-microkit/build/loader.img \
  -global virtio-mmio.force-legacy=false -d guest_errors
```

Expected probe output:

```text
GHCRTS|POSIX_WRITE_OK
GHCRTS|STDIO_FORMAT_OK:yes:7
GHCRTS|SEL4_LIBC_OK: memory/string operations, aligned allocation, realloc, coalescing
GHCRTS|MBLOCK_ARENA_OK: aligned 1 MiB blocks; free/reuse/bounds OK
GHCRTS|READY: Microkit callbacks and fixed heap; used=56 bytes; capability call OK
```

The freestanding runtime object is passed into the diagnostic GHC link using
`make -C experiments/ghc-microkit haskell-link-inventory`. That incomplete ELF
currently has 45 unresolved symbol names. A fresh `ld.lld -r` merge driver is
now configured for the GHC target, so `System.Posix.Internals.o` is a real
AArch64 ELF object and its foreign-import closures resolve. The bounded
allocator, memory/string functions, debug-console `write`, empty environment,
and explicit missing-file/input errors resolve another part of the boundary.
The baseline diagnostic link intentionally omits Microkit's library, so
`microkit_dbg_putc` remains an unresolved platform symbol there. Adding
OpenLibm resolves the base math surface; its residual compiler-runtime,
numeric-parsing, abort/exit, and explicit monotonic/wall-clock/event imports
still prevent a complete image. The target configuration filter removes host
process, signal, and terminal facilities. Microkit debug output resolves when
the real platform library is linked; the inventory intentionally leaves that
library out. Filesystem operations in the fixture libc now fail explicitly
until a filesystem service exists. The inventory is a useful link boundary,
not yet a Haskell runtime.
The stage1 RTS C objects now use function/data sections, but the normal base
archive remains compiled in its standard layout; an isolated `GHC.Float`
split-section experiment with GC did not remove its math imports. The
diagnostic-only stdio implementation writes formatted strings and integers to
Microkit debug output and rejects floating-point formats and file streams. Its
QEMU probe passes, and it reduced an earlier incomplete link from 82 to 48
unresolved imports (78 to 47 with section GC). The Microkit target disables the POSIX fd
setters in GHC base because only the non-threaded RTS is enabled; threadDelay
uses the already adapted deadline queue. Descriptor-based readiness remains
unsupported. The separate OpenLibm experiment brings those inventories to 11
and 10 symbols. This paragraph describes the earlier diagnostic link. The
strict platform link and QEMU smoke above now produce a runnable Haskell
image; wall clock remains explicitly unavailable.

The staged upstream SimpleX `Agent.Client` dependency closure now typechecks
and emits 179 AArch64 objects; the larger public startup-boundary probe emits
188. A focused `SimplexAgentStoreProbe.hs` calls the real
`Agent.Env.SQLite.createAgentStore` path with the application migration
catalog. It typechecks and emits 178 AArch64 objects. The target-only link
includes SQLCipher, the vault-backed SQLite VFS, the bundled LibTomCrypt
SNTRUP SHA-512 adapter, the GHC RTS, and bounded Microkit shims; it resolves
all symbols. `package_simplex_agentstore_probe.py` adds the existing sDDF timer
driver to a storage-merged Microkit system and patches the client timer
channel configuration. Starting from the plain system description omits the
vault's sDDF block-client mappings and faults on its first storage request.

The first QEMU start exposed two missing services in sequence: monotonic time,
then wall-clock time. With the timer driver and an explicit fixed QEMU test
clock, the real AgentStore migrations pass on a pristine disposable vault disk.
The probe now creates an upstream AgentStore user record, reads it back, and
verifies the same record ID after a separate QEMU reboot with the non-empty
test key. A third boot rejects a deliberately wrong key without changing the
disk. The repeatable smoke target passes all three boots. This validates the
upstream database path and encrypted persistence, but not the full SimpleX
agent or network startup. The fixed clock is QEMU-only; device builds must use
the trusted OS clock service.

`SimplexAgentRuntimeProbe.hs` now goes one step further: it starts and disposes
the real upstream `Agent.getSMPAgentClient` against the same encrypted store,
with an empty server set. `make guest-simplex-agent-runtime-qemu-smoke` passes
startup, disposal, same-key persistent reopen, and wrong-key rejection without
disk mutation. The runtime uses entropy channel 3, the messages protection
domain's route in the storage-merged system. No SMP connection or message is
tested yet; next, provide one upstream server through the existing network/TLS
capability.
