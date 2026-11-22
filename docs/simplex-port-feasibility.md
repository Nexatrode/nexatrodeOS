# SimpleX client feasibility spike

This is an exploratory compatibility test, not a decision to replace the
Personal OS Messages app or a claim that SimpleX is ready to run here.

**Latest SimpleX interoperability result (2026-10-16):** In QEMU, an
unmodified upstream SimpleX CLI and the upstream SimpleX Agent exchange
end-to-end encrypted messages in both directions. After shutting down and
restarting the guest with the same encrypted disk and the CLI with the same
database, the guest restores the same connection ID, sends another encrypted
message, and the CLI receives it. The guest also receives and acknowledges a
new CLI message after reboot when its restored connection is explicitly
resubscribed. On the initial connection, the incoming reply did not arrive on
the outstanding live subscription: the broker showed it queued and marked
delivered. Issuing a duplicate `SUB` caused the broker to replay the message,
which the Agent decrypted and acknowledged. The QEMU gate now requires this
recovery path. This uses a local SMP server and QEMU VirtIO networking; it does
not exercise Pixel Wi-Fi or install anything on the phone.

**Latest native milestone (2026-10-13):** The patched GHC 9.6.6 Haskell
protection domain boots under Microkit/QEMU, uses the timer and isolated RNG,
and calls a reusable `NativeNetwork` module with native `ByteString`, backed by
protected calls to the sDDF/lwIP network domain. `make -C experiments/ghc-microkit
haskell-net-qemu-smoke` checks bounded DNS against a local stub, then connects
to the returned address and checks multi-write/partial-read echo,
close/reconnect, cancel/recovery after a stalled read, and 20 KiB inbound and
outbound transfers. The service remains single-session with 128-byte calls.
A separate QEMU TLS image now completes an authenticated TLS 1.3 echo through
the same network PD using a test CA and hostname. Wrong-hostname and
expired-certificate tests reject the connection. A further QEMU check follows
SimpleXMQ's certificate-chain identity pin, accepts a different transport
hostname with the right pin, and rejects the wrong SHA-256 pin and an expired
certificate. `make -C experiments/ghc-microkit guest-smp-net-qemu-smoke`
now connects the guest to an isolated upstream SMP test server whose TCP
listener was bound to loopback for this fixture,
checks its pinned TLS chain and TLS-unique session ID, sends a version-6
client handshake and PING, receives PONG, then creates a test queue with a
guest-generated Ed25519 signature and validates the `IDS` response. It then
sends an opaque test payload, receives `OK`, and uses a signed `GET` to receive
the encrypted `MSG`. It resolves the fixture hostname through the network PD,
checks that a wrong identity pin is rejected, and uses unchanged upstream
`Simplex.Messaging.Transport.Buffer` and `Encoding` modules for its framed
wire path. This is
a wire-level relay probe, not the upstream client
or an end-to-end SimpleX message. Native SimpleX still needs
integration with upstream's client and agent modules, durable state, and
Pixel Wi-Fi. The chronological compiler/link notes below
include earlier states superseded by this milestone.

The SMP probe now opens its session through `nativeTLSBackendForHost`, which
resolves the requested hostname and opens TCP only through the protected
network PD before giving TLS the resulting backend. The same module exposes a
host/service connector matching the patched upstream client seam. The QEMU
smoke still passes pinned TLS, the SMP v6 handshake, PING/PONG, signed queue
creation, SEND/GET, encrypted-message relay, and wrong-pin rejection. This
validates the native connector beneath the upstream seam; it does not yet call
upstream `Transport.Client`.

## Native delivery gates

The target is an upstream SimpleX client running in a Microkit protection
domain, sending to a second unmodified SimpleX client through an SMP server.
A host-run client or serial relay does not count as completion.

1. **Runtime services:** native Haskell can use monotonic deadlines and fresh
   guest entropy. The QEMU smoke now passes both; general event dispatch and
   a phone hardware entropy source remain open.
2. **Network capability:** native Haskell now opens, sends, receives, closes,
   cancels a stalled read, and reconnects against a local QEMU endpoint over a
   protected network-PD capability. A reusable Haskell wrapper and 20 KiB
   transfers in both directions now pass. A QEMU TLS 1.3 echo authenticates a
   local test server and rejects a wrong hostname or expired certificate. A
   SimpleX-style identity pin also accepts the fixture and rejects a bad pin.
   A separate QEMU probe authenticates an upstream SMP server and completes a
   PING/PONG, creates a signed test queue, and sends and receives an opaque
   payload through it. The native host/service TLS-backend connector now opens
   that protected stream. The target build now compiles upstream
   `Transport.Client` and `Client.hs` with the guest path selecting this
   connector. The upstream agent now completes an invitation handshake and an
   encrypted send/receive/ack round trip in QEMU. These checks use QEMU VirtIO
   Ethernet, before Pixel Wi-Fi.
3. **Agent state:** the target now compiles the pinned SQLCipher Haskell
   bindings, the patched upstream SQLite connection wrapper, and all 53
   objects in the upstream SQLite migration catalog. The staged guest build
   lowers static SQL quasiquotes to `Query` literals because target GHC has no
   Template Haskell interpreter. The full SQLite store runner and
   `Agent.Client` now start in the guest, and the SQLCipher database migrations
   and user-record operations pass. The QEMU smoke reopens the same encrypted
   store after reboot and rejects a wrong key without mutating the disk.
   File-transfer temporaries still need a filesystem capability.
4. **Interoperability:** in QEMU, an unmodified upstream CLI joins the guest's
   invitation, receives and acknowledges an encrypted guest message, and sends
   an encrypted reply that the guest receives and acknowledges. A queued reply
   can require a duplicate `SUB` to be replayed; reliable live push delivery is
   still under investigation. Same-contact outbound and inbound exchange also
   passes after restarting the guest with its encrypted store. These results
   use a local SMP fixture and QEMU VirtIO, not Pixel Wi-Fi.
5. **Pixel transport:** bring up the Pixel's WCN3990 Wi-Fi firmware/driver and
   association, or another native IP transport, then rerun the same client
   checks on Pixel hardware. The current Pixel Slint image has no network PD.

## Source checked

The upstream `simplex-chat` stable checkout inspected for this spike is commit
`205d17e2fad68a4d121a743c7727eb48b19bf187` (2026-10-02), version `7.0.0.12`.
It is cloned under `.cache/simplex-chat-upstream/` and is not part of the OS
source tree. The mobile UI is Kotlin Multiplatform with Compose; it calls the
Haskell chat core through JNI/FFI. That core uses the SimpleXMQ Haskell client
and agent. The core's package dependencies include `network`,
`network-transport`, `tls`, `stm`, `async`, and the SQLite-backed store.

The SMP client/agent boundary is a useful reuse seam, but it is not just a
TLS-wrapped text protocol. The agent keeps queue and connection state, and
negotiates message protection, delivery receipts, and Double Ratchet state.
The Personal OS `PSL1` sealed datagram is a separate demo format and must not be
treated as SimpleX interoperability.

The resolved SimpleXMQ 7.0.0.6 source is available in the Cabal build cache.
`Simplex.Messaging.Transport.Client.runTLSTransportClient` currently resolves a
host with `Network.Socket.getAddrInfo`, opens a `Socket`, and passes it to
`Simplex.Messaging.Transport.connectTLS`; the latter calls `Network.TLS.contextNew`
and performs the handshake. SimpleXMQ defines `smpBlockSize = 16384`.
`NativeNetwork` now moves 20 KiB in each direction, exceeding one SMP block,
but it is one session. TLS 1.9 exposes a custom
`Backend` with send, exact receive, flush, and close callbacks; the local
`NativeTLSBackend.hs` adapter compiles against both the host package and the
cross-built guest package with host databases excluded. The guest
now cross-builds TLS 1.9, `crypton`, the X.509 parsing/store/validation stack,
and their dependencies as populated AArch64 static archives. The reproducible
`make guest-tls-deps` target audits every archive member and package ID
against the target databases. The cross-build disables TLS's ordinary socket
backend and uses the guest clock for TLS 1.3 ticket age; in-memory certificate
stores remain available, while filesystem loading fails explicitly. The ordinary
guest image has no trusted wall-clock service, so that clock fails closed. A separate QEMU
TLS test image reads host-controlled PL031 RTC time through an isolated clock
PD; this is not a production trusted-time solution. A freestanding UTC calendar
converter resolves the TLS/X.509 date-conversion imports without inventing a
current time. `crypton` now draws random bytes from the isolated Microkit RNG PD instead of Unix device
files. The full target ELF links with zero unresolved imports, and a separate
QEMU boot checks SHA-256, the backend's zero-length receive path, two
distinct 32-byte samples obtained through `crypton` from VirtIO RNG, and a
seeded `Network.TLS.contextNew` using the custom backend. A second QEMU image
uses `NativeTLSBackend` for a TLS 1.3 handshake and encrypted echo against a
local server with a test CA. Default X.509 validation accepts the fixture's
certificate and rejects both a different hostname and an expired certificate.
The same image follows upstream SimpleXMQ's identity-certificate fingerprint
rule and rejects an incorrect SHA-256 pin. An additional QEMU image connects
over pinned TLS to a separate upstream SimpleX SMP server, checks the framed
server handshake and TLS-unique session ID, sends a version-6 client handshake
and PING, receives PONG, sends a signed version-6 `NEW`, and validates the
resulting queue `IDS`. It uses the returned sender ID for `SEND` and signs
`GET` with the recipient key; the server accepts the send and returns an
encrypted `MSG`. The upstream `Simplex.Messaging.Transport.Buffer` module is
cross-built unchanged and reads the framed TLS blocks. Upstream
`Simplex.Messaging.Encoding` is now cross-built unchanged too. The probe uses
it to encode length-prefixed fields and to parse the server's actual block
length during the QEMU exchange. Its `parseAll` and `(<$?>)` imports use
small guest-only copies of the exact upstream definitions; the complete
upstream `Parsers` and `Util` modules are not built. This adds target
`attoparsec`, `network-transport`, and `time` dependencies plus QEMU-specific
clock and byte-order adapters. The image links without unresolved imports.
`NativeSMPTransport.hs` now centralizes the guest's TLS fixed-block framing
and batching, with upstream encoding and buffered reads. Its PING, signed
NEW, SEND, and signed GET paths pass against the upstream SMP server in QEMU.
`NativeSMPConnection.hs` now brackets pinned TLS over a network capability
opened by `nativeTLSBackendForHost`. `NativeTLSBackend.hs` also provides a
host/service connector with the shape needed by the patched upstream client.
The probe resolves a test hostname through the guest DNS service and repeats
the connection with a wrong identity pin, which must be rejected.
The next source boundary is the upstream agent. A guest-only
source overlay now removes the POSIX socket/SOCKS path from
`Transport.Client`, omits storage-only SQL serialization instances from
`Crypto`, and leaves the TLS-backend entry point intact. The target GHC
successfully typechecks this transport module and its source dependency
closure with `-DSIMPLEX_MICROKIT`. The host implementation remains the default
when that define is absent.
The pinned-source patch
[`simplexmq-microkit-tls-backend.patch`](../patches/simplexmq-microkit-tls-backend.patch)
adds backend-based TLS entry points to upstream `Transport` and
`Transport.Client`, plus an explicit `getProtocolClientWithTLSBackend` in
upstream `Client.hs`. The ordinary socket path is preserved. The caller
supplies a backend-opening action for the selected host and port; the guest
adapter resolves and opens its stream through the protected network PD. The TLS
transport owns its cleanup. This path uses the same protocol handshake,
connection queues, and client loops as the socket path, but is TLS-only and
does not use SOCKS. `make -C experiments/ghc-microkit
host-simplexmq-backend-typecheck` checks the transport modules against the
host package, then typechecks `Client.hs` and its source dependencies together.
That second check uses two unrelated GHC 9.6 compatibility fixes in a
throwaway copy of upstream `Crypto.hs`; they are not part of the patch.
The guest overlay typechecks upstream `Transport.Client` and emits 19 AArch64
objects for its dependency closure, including `Crypto`, `Transport`, and the
patched transport client. It also typechecks upstream `Client.hs` and emits
27 AArch64 objects for that source dependency closure. The guest generic
connector rejects the POSIX path and uses the supplied TLS backend, preserving
the upstream protocol handshake and client loops. This is a compile milestone,
not a running agent or message exchange. The overlay omits database
conversions from shared protocol and crypto types, supplies generic JSON for a
few shared records, and skips upstream JSON Template Haskell code generation
when the freestanding cross-compiler cannot execute it. The agent still needs
its real persistent store and complete configuration/API integration.
The target-compiler inventory registers AArch64 `unliftio-core` and
`safe-exceptions` objects. A narrow guest console-logging adapter resolves
`Control.Logger.Simple`, and upstream `Transport.Shared` now uses `String`
instead of importing the equivalent `Network.Socket.HostName` alias. Its
certificate-chain code and the logger adapter cross-compile to audited
AArch64 objects with `make -C experiments/ghc-microkit
guest-simplex-shared-compile`. The repeatable inventory command is `make -C
experiments/ghc-microkit guest-simplex-client-inventory`. The target inventory
now gets past Aeson, `Data.ByteString.Base64`, and `Data.Time.ISO8601`. The
Aeson closure, its encoding helpers, and their dependencies cross-compile to
audited AArch64 objects. Direct target builds are used for
`unordered-containers` and Aeson because Cabal's solver otherwise selects a
host GHC heap package; splitmix's POSIX entropy source is replaced by a
checked Microkit entropy hook.

The current full upstream `Client.hs` source-closure check passes target
typechecking and object generation. A rerun of
`python3 experiments/ghc-microkit/inventory_simplexmq_guest_client.py` on
2026-10-13 passes the transport and protocol-client checks (19 and 27 target
objects respectively), then reports five remaining `Agent.Client` imports:
`Network.HPACK`, HTTP/2 client/server, `Network.Socket`, and
`System.TimeManager`. Small guest adapters now cover the `async` operation,
the unlifted MVar operation, and compile-time directory calls; filesystem
operations deliberately fail because no app filesystem capability exists yet.
These adapters only close source imports and do not make file storage or the
network runtime available.
This pinned SimpleXMQ build uses `sqlcipher-simple` 0.4.18.1 over
`direct-sqlcipher` 2.3.28, with OpenSSL selected for SQLCipher on the host.
The Microkit port therefore needs a SQLCipher crypto provider plus a storage
VFS backed by the protected vault, or an intentional vault-native agent
store. The bundled SQLite C code assumes a platform VFS and pthread support;
cross-compiling its Haskell wrappers alone would not provide durable or
encrypted storage. Replacing these modules with type stubs would not make the
client usable. The upstream agent has not cross-compiled or run in the guest.
The next source milestone is to settle the persistent-store architecture and
decide which HTTP/2 features can be left out of a first SMP-only build.

## Agent-store storage decision

Keep SimpleXMQ's upstream SQLite/SQLCipher store for the first native agent
instead of reimplementing `AgentStore`. The pinned SQLite backend has 278
direct database/transaction call sites in `AgentStore.hs`, plus migrations and
typed row encoders. A vault-native replacement would need to preserve those
queries' ordering, uniqueness, transaction, and migration semantics; the
current vault API is only an encrypted, authenticated 4 KiB blob per namespace
with two alternating slots. It is useful for small app records, but cannot
hold or update a relational agent database efficiently.

The practical compatibility boundary is a dedicated SQLCipher SQLite VFS
backed by a storage capability. Keep the upstream `DBStore`, migrations,
queries, and SQLCipher keying intact, with a narrow platform patch to select
the custom VFS in `sqlite3_open_v2`. The pinned `direct-sqlcipher` binding
already exposes `open2` with its VFS-name argument; the integration can route
the store's `DB.open` through that call. Replace the store's directory/existence
checks and optional backup copy with operations supported by that capability.
Implement a bounded storage service for the database file and SQLite rollback
journal over sDDF block I/O.
Start with one agent connection, rollback-journal mode, and serialized access;
do not enable WAL or multi-connection locking until the VFS supports and tests
those semantics. The service must enforce an app-specific storage capability,
durable flush ordering, bounds, and per-agent encrypted storage. SQLCipher's
database key and the vault's at-rest key remain separate layers with explicit
key provisioning; compiling `sqlcipher-simple` alone does not supply either a
Microkit VFS or a production key source.

The first bounded vault file service is now implemented behind the Messages
app's private vault channel. It gives the app two fixed 4 MiB extents (the
SQLCipher database and SQLite rollback journal), authenticated two-slot length
metadata, bounded random reads/writes, truncation, sync, and journal deletion.
The metadata and file IDs are isolated to that channel; the C storage adapter
transfers at most 4036 bytes per IPC and maps the service onto SQLite's
`NexStorageOps`. A separate Microkit adapter compiles against the target SDK.
The vault stack was raised to 128 KiB after QEMU exposed a stack overflow in
metadata authentication, and the existing crash-recovery test now re-reads its
system record because file transfers reuse the block DMA staging buffer.

The implementation and probes live in
[`experiments/ghc-microkit/sqlite_vfs_probe/`](../experiments/ghc-microkit/sqlite_vfs_probe/).
`make -C experiments/ghc-microkit/sqlite_vfs_probe test` validates the VFS on
host SQLite and checks the IPC adapter's chunking, short-read, bounds, sync,
and journal-delete behavior against a fake vault. The QEMU `test-storage`
scenario now writes the database file, reboots, reads it back, commits journal
deletion, and still runs all scalar/blob crash-interruption and snapshot tests.
`make -C experiments/ghc-microkit/sqlite_vfs_probe microkit-compile` separately
cross-compiles the SQLite VFS, vault adapter, and IPC shim against the pinned
SQLCipher headers. These tests establish the bounded storage capability and
transport layers, not SQLCipher encryption or upstream
`AgentStore` migrations: the host SQLite test is plaintext and memory-backed,
and the QEMU file smoke test currently stores a fixed test string.

The VFS registers as SQLite's default VFS, not only under its explicit name.
SQLite's default-VFS hooks supply global randomness and clock lookups; the host
test checks default selection and dispatch through the VFS entropy callback.
This keeps those lookups on configured Microkit capabilities after VFS setup.
It is not a cryptographic test: the host fixture uses deterministic test
entropy. The codec-enabled target object is now a separate compile target.

The pinned `direct-sqlcipher` 2.3.28 amalgamation now compiles for AArch64 with
`SQLITE_HAS_CODEC` and the bundled LibTomCrypt provider enabled. A generated
build copy redirects the provider's Fortuna seed request to the default VFS
entropy callback; a missing or short response fails activation instead of
falling back to SQLite's ordinary PRNG. The LibTomCrypt 1.18.2 source also
builds into a 400 KiB AArch64 archive containing only AES-CBC, SHA-1/256/512,
HMAC, PBKDF2, and Fortuna; file I/O and self-tests are disabled. A host test
using the same provider and VFS confirms ciphertext has no SQLite plaintext
header, rejects a wrong key, and reopens with the correct key. A QEMU Microkit
PD now executes this AArch64 path end to end against an in-memory test store.
SQLCipher's Fortuna seed request reaches the Microkit virtio RNG service over
its entropy capability, and the test checks that IPC before exercising
ciphertext, wrong-key rejection, and reopen. This validates the QEMU entropy
route and target codec execution, but not persistent vault behavior or key
management. These tests also caught an initialization-order
requirement: the target disables SQLite autoinit, registers the capability VFS,
then explicitly initializes SQLCipher so the provider obtains its seed from
the VFS. The target build omits WAL, local-time conversion, and SQLCipher
logging. SQLCipher migration's POSIX `rename` remains unavailable until it is
implemented through the VFS.

`make -C experiments/ghc-microkit guest-simplex-sqlite-core-compile` remains a
codec-disabled SQLite compile check. The separate `test-sqlcipher-vault`
experiment now runs the codec-enabled C VFS against the real disk-backed vault
and RNG services in Microkit/QEMU. It creates an encrypted database, rejects a
wrong key, then reboots with the same raw vault disk and reopens the database
with the correct key while continuing to reject the wrong key.

The pinned SimpleXMQ store now has a reproducible source patch that routes its
SQLCipher connection through a named `nexatrode-storage` VFS in
`SIMPLEX_MICROKIT` builds, while preserving the host path. It determines
first-run state from the migrations table and compiles out host directory and
backup-file operations. `make -C experiments/ghc-microkit
host-simplexmq-sqlite-vfs-typecheck` typechecks both build modes against the
pinned source dependency closure. This joins the existing VFS and vault
implementation at the source/API boundary; it does not yet build or run the
complete AgentStore in Microkit. `simplex_sqlite_init_microkit.c` now
registers the VFS with the Messages app's vault endpoint, requests fresh
entropy through the RNG service, and reads Unix time through the QEMU RTC
service. `NativeSQLiteVFS.hs` exposes the initializer to Haskell, and the
Microkit store patch calls it before `DB.openWithVfs`. The Messages protection
domain already maps the transfer page and has the two additional protected-call
capabilities. The cross-compiled initializer and source typecheck do not yet
mean a complete agent image: the Haskell SimpleX process must be linked into
the Messages domain with the VFS, vault adapter, and target SQLCipher library,
then exercised through upstream store migrations. The AArch64 target now
cross-compiles and registers `direct-sqlcipher` 2.3.28 and `sqlcipher-simple`
0.4.18.1 as GHC packages, and a target compile probe checks the exact
`Database.SQLite3.open2` custom-VFS constructor used by SimpleX. The patched
upstream `Agent.Store.SQLite.DB` wrapper also typechecks for the target with
`openWithVfs`. `make guest-simplex-agentstore-migrations-check` then lowers
each static SQLite migration quotation in the staged guest source and compiles
the complete migration catalog to 53 AArch64 objects. The host upstream
source remains unchanged. A target staging experiment removes
`Agent.Protocol`'s direct import of `Client.hs`, extracts the reusable
`authTransmission` helper and protocol-facing XFTP types, and keeps the JSON
constructor tags stable. It also replaces Agent.Protocol's import of the full
`FileTransfer.Types` module with a target-local copy of its protocol error
schema. The staged Agent.Protocol now typechecks with the host compiler, and
the target compiler's dependency trace no longer reaches HTTP/2, sockets, the
full Client, or file-transfer transport. Target compilation stops at JSON
Template Haskell because this freestanding GHC has no interpreter; no target
Agent.Protocol typecheck pass is claimed yet. The next boundary is to provide
static or otherwise target-compatible JSON instances for the staged protocol.
Filesystem operations continue to fail explicitly until a storage capability
is implemented. `blaze-textual` uses a target-only base `show`
implementation because this native-bignum GHC has no `integer-gmp` package;
this affects formatting performance, not serialized values. The store runner
still pulls generated SNTRUP constants; the full agent import boundary also
needs its async and storage-service closure. The database key remains separate from the
vault metadata key. The QEMU RTC capability does not yet provide a trusted
clock on Pixel or iPhone hardware.
The minimal wire probe itself does not use SimpleXMQ's client/agent modules,
decrypt its opaque payload, or create invitations or agent messages. A separate
target inventory now compiles `Client.hs` and its dependency objects, but does
not run them in Microkit. The transport target build uses a small
guest source overlay: a minimal `Dict` carrier and `Data.Default` re-export,
the target-built `Data.Default.Class`, a guest IPv6 parser for `TransportHost`,
and skips transport JSON Template Haskell splices that the freestanding
cross-compiler cannot execute. These are compile-time accommodations; they do
not replace TLS validation or protocol behavior. The regular host build keeps
upstream's parser, socket path, SQL instances, and JSON instances.
The fixture's private keys are public test assets. `Client.hs` and its
transport dependencies now cross-compile to target objects; the SQLCipher
bindings and patched store connection wrapper also compile. The remaining
agent import boundary has narrowed to five modules: `Network.HPACK`, HTTP/2
client/server, `Network.Socket`, and `System.TimeManager`. Target shims cover
the small upstream async/MVar operations and explicitly fail filesystem calls
until a filesystem capability is implemented. The SQLite migration catalog now
compiles after replacing static SQL quasiquotes in the staged guest copy. The
full store runner still pulls in HTTP/2 and generated SNTRUP constants, so
executing schema migrations through the persistent VFS remains the next store
proof. Durable
agent state, production key lifecycle, trusted time on Pixel, and native Pixel
IP transport remain required for a real phone client. Wrapping plaintext SMP
bytes alone would not meet the authentication requirement.

## Current result

An isolated GHCup installation under `.cache/simplex-toolchain/` built the
upstream CLI and SMP server for the host's Apple Silicon architecture. The
build used GHC 9.6.3, Cabal 3.10.3.0, `simplex-chat` 7.0.0.12, and its resolved
`simplexmq` 7.0.0.6 dependency. It required the system OpenSSL development
library. No shell startup file or global package database is used by this
toolchain.

A test-only copy of the SMP server was compiled with its TCP listener changed
from wildcard bind to `127.0.0.1`; that change exists only in the ignored
`.cache/simplex-chat-upstream/` checkout. The server was initialized with
test credentials, then Alice and Bob profiles connected through it. Bob
accepted Alice's one-time invitation, and both exchanged messages. Server
logs show one message saved at shutdown. The server was stopped after the
round trip. This validates the upstream host build, SMP handshake, client
state, encryption, and message delivery on macOS; it is not a Microkit port or
evidence that SimpleX is ready to run in the guest.

A separate minimal runtime probe also passed on the Apple Silicon host. It
used `forkIO` and STM, read the system clock, obtained 32 bytes from the OS
entropy source through `crypton`, round-tripped a file, and echoed data over an
IPv4 TCP loopback socket. The executable is an ARM64 Mach-O binary linked to
macOS system libraries. GHC reports `cross compiling = NO` and target
`aarch64-apple-darwin`; this GHC install cannot produce an AArch64 Linux or
Microkit executable. The probe establishes that these runtime facilities work
on the host, not that the same Haskell runtime can run on Linux or seL4.

The Microkit target in this repository is `no_std` Rust plus statically
configured C protection domains. It has no general POSIX process runtime,
app-facing socket API, or SQLite/SQLCipher service. The existing sDDF/lwIP
network experiment proves DHCP, local-stub DNS, TCP/UDP echo, and a TLS 1.2
test connection. Its TLS test uses a pinned test certificate and deliberately
bypasses validity-window checks; it does not establish the TLS 1.3, public
resolver, entropy, clock, or server-identity behavior required for SimpleX.
The Pixel 4a UTM graphics probe is unrelated and has no SimpleX runtime or
networking.

The AArch64 Linux UTM clone passed the minimal runtime probe under Debian 13
with GHC 9.6.6. Its executable was an ELF AArch64 Linux binary, and the
threading/STM, clock, entropy, file I/O, and IPv4 loopback tests passed. The
same pinned upstream SimpleX Chat source also built there as an ARM64 Linux
CLI (`simplex-chat` 7.0.0.12 with `simplexmq` 7.0.0.6). This is a useful
reference build: it identifies the compiler, package versions, and runtime
behavior that a native port must reproduce. It is not a proposed Linux guest
architecture for the Personal OS. A separate attempt to build `smp-server`
stopped in an upstream Template Haskell asset-embedding step because the build
could not find `apps/common/Web/static/.well-known/` relative to its working
directory. The full client/server round trip remains proven on macOS only.

## GHC / Microkit boundary probe

The smallest direct-link experiment compiled a trivial Haskell `main` with the
guest's GHC 9.6.6 to an AArch64 ELF object, then passed that object to the
project's AArch64 Microkit linker script and `libmicrokit.a`. The object format
was accepted, but linking stopped on missing GHC `base`/RTS symbols (`newCAF`,
`stg_*`, and `base_*` closures), plus Microkit's required `init` and `notified`
callbacks. This is expected: the SDK contains the Microkit C entry/IPC library,
not GHC's Haskell runtime or base packages. An ABI-compatible AArch64 object is
not enough to make it a runnable protection domain.

I also inspected the Debian Linux threaded GHC RTS archive used by that build.
It imports POSIX APIs for pthread creation/synchronization and thread-local
state; `mmap`, `mprotect`, and `munmap`; signals; clocks and sleep; file
descriptors; process information; and dynamic loading. A native Microkit RTS
would need to replace or avoid those host-OS facilities. Some required
functionality can be supplied by services, but the GHC scheduler and garbage
collector also need runtime-level integration: Microkit callbacks are
event-driven PD entry points, while Haskell `forkIO`, STM waits, timers, and
allocation expect a continuously available runtime scheduler and heap.
The minimal source and exact reproduction steps are in
[`experiments/ghc-microkit/`](../experiments/ghc-microkit/README.md). GHC's own
[9.6.6 User's Guide](https://downloads.haskell.org/ghc/9.6.6/docs/users_guide/runtime_control.html)
describes the RTS as handling storage management and thread scheduling, which
matches the missing runtime functions and OS imports found in the Linux build.

A follow-up C-only boundary probe now builds and boots as a two-domain Microkit
image. It exercises the `init`, `notified`, and `protected` entry points, an
8 MiB arena of eight aligned 1 MiB megablocks (contiguous allocation, free,
reuse, and bounds checks), a 64 KiB private bump heap, and one synchronous
capability request/reply. QEMU prints the `GHCRTS|MBLOCK_ARENA_OK` and
`GHCRTS|READY` markers. This validates the OS-side shape a Haskell runtime can
use; it is not yet a Haskell executable, RTS port, or proof that this small
test arena meets GHC's garbage collector requirements.

## GHC target-platform probe

I checked the official GHC 9.6.6 source archive (SHA-256 verified against its
release checksum list). Its compiler already has an AArch64 code generator,
but its OS list does not include seL4. Stock `./configure
--target=aarch64-unknown-none` stops at `Unknown OS none`. The first
repository-owned patch is now in
[`patches/ghc-9.6.6-microkit-platform.patch`](../patches/ghc-9.6.6-microkit-platform.patch).
It adds the `OSSeL4` target, maps the normalized `none` OS to `sel4`, and marks
the target as ELF. It also skips GHC's target C++ runtime probe for seL4,
because the Microkit RTS does not use C++. The patch applies to the
checksum-verified 9.6.6 source; `autoreconf` succeeds, and configure completes
with target platform `aarch64-unknown-sel4`. The changed
`GHC.Platform.ArchOS` module also passes a no-code compile with the available
bootstrap GHC 9.6.3.

The target compiler needs a freestanding link recipe because the SDK has no
`crt0.o`, compiler-rt archive, or libc. A cache-only Clang wrapper with
`-nostdlib -Wl,-e,main`, the LLD linker, and a minimal test `inttypes.h` now
gets GHC configure through its C and atomic checks. Configure completes and
the generated Hadrian settings retain the wrapper path as the C compiler.
Configure still reports no ordinary C/POSIX headers, and the local header shim
exists only for this probe. No cross-compiler, `base`, RTS, or Haskell
protection domain has been built.

The first GHC-side memory adapter is now in
[`experiments/ghc-microkit/OSMem.c`](../experiments/ghc-microkit/OSMem.c), with
an `os(sel4)` source-selection patch in
[`patches/ghc-9.6.6-microkit-osmem.patch`](../patches/ghc-9.6.6-microkit-osmem.patch).
It maps GHC's `osGetMBlocks` and `osFreeMBlocks` calls to a statically mapped,
aligned 8 MiB arena, with explicit failure on exhaustion or invalid frees.
This is the initial bring-up limit, not a final heap budget. The adapter now
compiles as an AArch64 ELF object against headers generated from the configured
GHC 9.6.6 tree. The fixture follows Hadrian's `generateGhcAutoconfH` and
`generateGhcPlatformH` rules and calls GHC's own event-type generator. The
generated platform header identifies `aarch64-unknown-sel4`. GHC's
configure-derived feature header describes the Mac build machine, so target
branches must override host POSIX feature choices. The QEMU Microkit probe
links the memory adapter and reports `GHCRTS|GHC_OSMEM_ADAPTER_OK` after
allocation, freeing, reuse, and full-arena reset. The stage1 static RTS now
builds through Hadrian with generated `AutoApply.cmm`; the resulting archive
passes a whole-archive AArch64 relocatable link check.

The `os(sel4)` branch intentionally avoids falling through to POSIX and now
selects `sel4/OSMem.c`, `sel4/GetTime.c`, and `sel4/AwaitEvent.c`. It is still
not a complete RTS source set and cannot link a runtime. Environment, signals,
TTY, and additional thread primitives remain. Keep the build on the
freestanding target wrapper; do not use Mac headers or libraries. The tested C
probe remains the reference for the memory allocator behavior.

## First non-threaded RTS adapter surface

GHC 9.6.6's RTS source list falls through to POSIX when no dedicated OS branch
matches, so adding `OSSeL4` alone would still compile POSIX memory, time,
threading, signal, terminal, and select sources. The Microkit branch now picks
the first three dedicated adapters. Those objects compile against GHC-derived
headers, but the event broker and clocks they call are service contracts, not
implemented services yet. The important interfaces are:

| RTS area | GHC 9.6.6 entry points | Microkit port shape |
| --- | --- | --- |
| Heap blocks | `osMemInit`, `osGetMBlocks`, `osFreeMBlocks`, `osReleaseFreeMemory`, `getPageSize` in `rts/sm/OSMem.h` | Allocate aligned contiguous megablocks from a fixed PD mapping; no demand paging or per-page decommit in the first version. Keep the arena limit explicit. |
| Time | `initializeTimer`, `getMonotonicNSec`, `getUnixEpochTime`, `getPageFaults` in `rts/include/rts/GetTime.h`; `getProcessElapsedTime` in `rts/include/rts/Time.h` | `GetTime.c` requires monotonic-nanosecond and Unix-epoch service hooks. The sDDF timer-client bridge is compile-checked and passes a one-second QEMU wakeup probe using the real timer driver, but is not yet linked into GHC's runtime image. Unix epoch time still needs a trusted clock service. The QEMU PL031 clock PD reports elapsed seconds since its own start, not Unix epoch time. A direct `CNTVCT_EL0` read faulted for a PD under this seL4 configuration. CPU accounting and page-fault counters currently return zero. |
| Scheduler wait | `awaitEvent` in `rts/IOManager.h`, called by non-threaded `rts/Schedule.c` | A QEMU scheduler probe now links the actual patched GHC `awaitEvent` and `GetTime` objects plus the shared timer bridge. It queues a real GHC `StgTSO` to a future deadline and verifies the actual code moves it from `sleeping_queue` to the run queue and clears its blocked state. The harness supplies `pushOnRunQueue` and does not initialize the full RTS, so this is scheduler-hook coverage, not Haskell `threadDelay`. The initial bridge traps on unrelated badges until general callback and deferred-notification/IRQ-ack handling is integrated. Blocking file-descriptor I/O still fails fast. |
| Ticker | `startTicker` and `stopTicker` in `rts/Ticker.h` | Async ticker/preemption is disabled for seL4 because GHC's POSIX tick path depends on signals. The first RTS can only make cooperative progress at scheduler safe points; true async preemption needs an interrupt-safe Microkit design. |
| Signals | `initUserSignals` in `rts/RtsSignals.h` | No Unix signals in the PD; provide an explicit no-signal implementation and route termination/control through OS capabilities. |
| OS threads | `osThreadId` and thread synchronization in `rts/include/rts/OSThreads.h` | The patch now adds PD-local opaque types and no-op locks for the initial non-threaded build, and rejects `THREADED_RTS`; thread-source stubs and the single-PD identity still need implementation. |

This surface is small enough to guide the first port but does not yet make the
rest of GHC's common RTS freestanding. A compile will still reveal C-library
calls in shared files, so the build should be narrowed to the non-threaded RTS
and minimal runtime features before choosing whether to port a small libc
subset or remove each dependency at its call site. The host currently has
about 34 GiB free; the missing freestanding headers and RTS integration are
the immediate constraints now.

| SimpleX requirement | Current Personal OS evidence | Missing integration |
| --- | --- | --- |
| Concurrency / STM | Rust and C PDs handle bounded synchronous calls; network PDs have event loops | Haskell RTS scheduler, runnable/waiting task queues, STM wakeups, and timer callbacks inside a PD |
| Time / timers | PL031 clock PD reports seconds; sDDF timer driver supports scheduled I/O | Monotonic high-resolution time and many cancellable deadlines for the RTS, TLS, and network protocols |
| Memory | Microkit statically maps private PD memory; Rust apps use `alloc` with a fixed arena | GHC RTS heap and GC bounded inside that mapping, replacing Linux page mapping/protection calls |
| Network / DNS / TLS | Separate lwIP network PD passes DHCP, local DNS, TCP/UDP echo, and a pinned TLS 1.2 smoke | App-facing asynchronous socket capability, public DNS, TLS 1.3, real entropy/time, and SimpleX server-identity verification |
| Storage | SQLCipher, LibTomCrypt, the capability VFS, RNG IPC, and the disk-backed Messages vault work together in QEMU; an encrypted row survives a fresh boot and a wrong key is rejected | Run upstream Haskell AgentStore migrations and prove normal message-state reads/writes; WAL, file rename, and production key lifecycle remain unavailable |
| Entropy | QEMU VirtIO RNG PD now supplies two distinct 16-byte samples to native Haskell through a protected call | General bounded entropy API and a production hardware source on each phone target |

This makes a direct GHC port a runtime and service project, not a compiler flag
change. The chosen direction is to bring the Haskell runtime to Microkit and
grow the Personal OS services alongside it. Each capability added for the RTS
should also be usable by native Rust and C protection domains, so the port
work improves the OS independently of SimpleX. The ARM64 Linux build remains
a compatibility reference, not an alternate deployment target. A Rust client
would fit the existing `no_std` model better, but it would still need to
reimplement and validate SimpleX's queue protocol, cryptography, ratchets,
agent state, and persistence; that is not a small rewrite.

The first RTS target should be GHC's non-threaded runtime. GHC's own 9.6 User's
Guide says the single-threaded runtime still supports concurrency between
Haskell threads; `-threaded` is needed for multiple OS threads and parallel
execution. That lets the first port avoid pthreads while keeping `forkIO`-style
scheduling in the Haskell runtime. Its POSIX I/O manager cannot be carried over
as-is: the Microkit port needs an event path built on protected calls,
notifications, and timer capabilities. See GHC's
[9.6.1 threading option documentation](https://downloads.haskell.org/ghc/9.6.1/docs/users_guide/phases.html).

## Test sequence

1. **Complete the native compiler boundary.** The patched GHC 9.6.6 stage1
   cross-compiler now builds and reports `OSSeL4`. With small
   freestanding header shims and `quickest+native_bignum`, target RTS constants,
   `ghc-prim`, and the bundled AArch64 libffi archive now build. Compiling the
   full RTS source set now excludes the ELF runtime linker and object-unloading
   hooks for static Microkit images. The macOS-hosted cross-compiler's AArch64
   assembly printer was also fixed to select relocation syntax by target OS;
   an ELF Cmm object now assembles with the target Clang wrapper. The RTS now
   omits the runtime linker; `RtsUtils.c` also compiles directly for the target
   after dropping its POSIX wall-clock assumption and marking process-style
   sleep unsupported. All 99 C sources in Hadrian's generated dependency set
   and all 10 checked-in Cmm sources compile directly. An earlier trial archive
   accidentally included a second copy of each Cmm object. The actual Hadrian
   stage1 RTS build now includes `AutoApply.cmm`, and its 111-member ELF archive
   passes a whole-archive `ld.lld -r` check with 123 unresolved imports. These
   include `base`/`ghc-prim` closures, freestanding libc/FFI and process/file
   APIs, and the Microkit time/event hooks. The build is reproducible with
   `make -C experiments/ghc-microkit rts-trial-link`, which uses the cached
   GHC 9.6.3 host compiler and Cabal 3.10.3 to launch Hadrian. The target base
   patch ([`ghc-9.6.6-microkit-base-encoding.patch`](../patches/ghc-9.6.6-microkit-base-encoding.patch))
   sets UTF-8 as the explicit default, keeps built-in UTF and Latin-1
   encodings, reports other encodings as unsupported, and makes environment
   mutation explicitly unsupported until Microkit has that service. Hadrian
   now builds the stage1 `base` library with generated target headers. The
   fixture's minimal `Main.hs` compiles to an AArch64 object against the target
   package database. A reproducible GHC-to-LLD adapter now creates a
   diagnostic-only AArch64 ELF; it contains 68 unresolved symbols and is not
   runnable. The Microkit link patch
   ([`ghc-9.6.6-microkit-link.patch`](../patches/ghc-9.6.6-microkit-link.patch)) removes
   host-derived `libm` and `libdl` dependencies from the RTS metadata. With a
   linker inventory, a new object-merge patch
   ([`ghc-9.6.6-microkit-merge.patch`](../patches/ghc-9.6.6-microkit-merge.patch))
   configures `ld.lld -r` for GHC's target objects. Without it, GHC had nested
   an archive inside `System/Posix/Internals.o`, which left all of that module's
   FFI closures undefined. Rebuilding `base` with the merge driver produces a
   valid AArch64 object and resolves those closures. The bounded memory/string
   runtime, console write, empty environment, and explicit `ENOSYS` results for
   unavailable input and filesystem calls resolve their imports too. The QEMU
   probe checks `write(1, ...)` reaches Microkit debug output and stdin has no
   input capability. The diagnostic Haskell link still omits Microkit's
   library, leaving `microkit_dbg_putc` unresolved as a platform import.
   Remaining imports center on process, math, signal/thread hooks, and
   target timer services. The archive trace showed the stdio dependency is from
   RTS diagnostics and startup (`RtsMessages`, `RtsUtils`, and `RtsFlags`), not
   the basic Haskell text output path. `Capability.o` pulls `RtsMessages.o` for
   `barf`, and those default fatal/error hooks call stdio. An optional
   section-GC link lowers the unresolved set from 78 to 64 after the
   diagnostic stdio layer resolves RTS message and formatting calls, but math
   imports remain reachable; splitting `GHC.Float` alone did not remove them.
   The adapter writes formatted string/integer diagnostics to Microkit debug
   output and rejects file streams, stdin, and floating-point conversions.
   Its QEMU probe covers formatting, truncation, and unsupported operations.
   The restricted `poll` shim handles zero-descriptor timer waits through the RTS
   bridge; descriptor readiness returns `ENOSYS`.
   The bounded 8 MiB heap reserve/commit path passes
   its QEMU probe using the exact GHC adapter source. The
   source patches are [`ghc-9.6.6-microkit-static-rts.patch`](../patches/ghc-9.6.6-microkit-static-rts.patch)
   and [`ghc-9.6.6-microkit-platform.patch`](../patches/ghc-9.6.6-microkit-platform.patch).
   Next audit the remaining non-threaded imports; then connect Haskell `main`
   to stdout, format UTF-8 text, and return through the debug/log adapter. The allocator
   itself already compiles and passes its QEMU adapter probe.
2. **Add runtime primitives and OS services incrementally.** Bring up one
   Haskell capability at a time: monotonic time and deadlines; an event-driven
   scheduler with one lightweight thread; a blocking/wakeup primitive that
   can support STM; and a bounded allocator/GC strategy within the PD memory
   map. Keep interfaces capability-based so the app must be explicitly given
   timer, entropy, network, and storage access. Where a service is missing,
   build it for all native apps rather than hiding it in a SimpleX-specific
   shim. The current PL031 clock reports elapsed seconds since its PD starts,
   while sDDF already has a nanosecond timer interface. Unifying those behind a
   stable monotonic clock/deadline capability is an early OS improvement.
3. **Port the dependency foundation.** Build and run the Haskell `base` and
   non-threaded RTS for the bare Microkit target, then add C FFI and dependencies
   in measured layers. Start with containers, cryptographic entropy and
   primitives, then TLS 1.3 and certificate/fingerprint validation. Record
   each dependency's required OS calls and memory bounds. The Linux UTM build
   provides known-good versions and behavior for comparison, not an artifact
   to boot as a guest.
4. **Make network and persistence app services.** Evolve the existing
   isolated sDDF/lwIP experiment into a capability-safe client API with
   asynchronous TCP operations, DNS, deadlines, and errors. Add authenticated
   persistence through the vault, or port the minimum SQLite VFS needed by
   SimpleX. Test these services first from existing Rust/C apps and with small
   Haskell probes.
5. **Run the SimpleX core as a native service PD.** Add the upstream Haskell
   chat core only after its runtime, crypto, network, and store dependencies
   run natively. Connect to a local SimpleX server, create an SMP queue,
   exchange an invitation, and deliver a message to a second upstream client;
   require that client to decrypt and display it. Then expose a narrow OS app
   API and connect a native UI.

The macOS round trip and ARM64 Linux reference build are strong positives for
source reuse. The smallest Haskell Microkit domain now runs with timer,
entropy, bounded TCP, authenticated TLS, and SMP PING/PONG plus signed queue
creation and opaque relay message delivery against an upstream server in QEMU.
The next native milestone is
compiling the upstream SimpleXMQ client transport against these guest services,
followed by a message round trip. Durable agent state and a Pixel network driver remain separate
requirements before an upstream SimpleX client can send from the phone.

## Latest SimpleX protocol pass (2026-10-13)

The target overlay now substitutes Aeson generic codecs for the Agent.Protocol,
transport, file-transfer, ratchet, and error-schema JSON splices, using the
same Aeson options as upstream. A narrow `Data.Yaml` guest shim emits JSON,
which is valid YAML 1.2, and accepts JSON input only. The target compiler
successfully typechecks the staged Agent.Protocol closure and emits 43 AArch64
objects with the freestanding header path enabled. The change is reproducible
with `make guest-simplex-agent-protocol-check`. Full YAML syntax and
remote-control host/controller JSON codecs are still outside this first target
subset. This milestone is a compiled protocol dependency closure, not a linked
or running agent. The AgentStore SQLite wrapper and all 53 static migration
objects also pass; no upstream agent is running on device and no SimpleX
message has been exchanged by this OS.

The client inventory cross-builds 62 AArch64 objects for the TLS transport
closure and 27 for upstream `Simplex.Messaging.Client`. The upstream
`Network.HTTP2.Client` callback engine now also typechecks and emits 50 AArch64
objects after fencing out its POSIX socket convenience constructor and
`defaultReadN`; the HTTP/2 engine itself consumes supplied read/write callbacks.
Its supporting target packages include `network-byte-order`, `psqueues`,
`auto-update`, and `time-manager`. The guest compatibility surface supplies
the narrow STM, timeout, exception, and SOCKS-credential APIs needed to get the
agent closure past import resolution. This does not create an HTTP/2 session or
connect a client.

`compile_simplexmq_agent_client.py` now stages the real upstream
`Agent.Client` dependency closure, including a backend-injectable HTTP/2/TLS
path. The target substitutes generic Aeson instances with upstream's exact
options where the cross GHC cannot run Template Haskell, and lowers the 128
static SQLite AgentStore queries plus migration queries to `Query Text` without
changing their bound parameters. The HTTP/2 wrapper accepts an explicit
`networkBackend` capability; its default is absent and fails clearly if used,
so this compile pass does not pretend that a network service is connected.

At this compile-only checkpoint, the staged upstream `Agent.Client` dependency closure passed GHC's
Microkit-target typecheck and emits 179 AArch64 objects with
`compile_simplexmq_agent_client.py --objects`. The target routes its standard
protocol connections through `getProtocolClientWithTLSBackend` and
`NativeTLSBackend`, which is backed by the existing protected network-PD stream
API. The upstream protocol-only build (44 objects), HTTP/2 callback engine (50
objects), and integrated agent closure all passed. Later QEMU milestones below
link and run the agent against its encrypted store and a local server; this
checkpoint itself did not yet establish a session.

The QEMU stream underneath the connector is deliberately single-session and
the HTTP/2 config still has no installed backend, so simultaneous SMP/NTF/XFTP
connections and file transfer are not ready. The current message test joins an
invitation back into the same AgentClient to stay within that one-stream limit;
an independent second SimpleX client remains the interoperability gate. Pixel
Wi-Fi firmware and a seL4 WLAN driver are independent hardware bring-up work
and are not validated by these QEMU checks.

A focused `SimplexAgentStoreProbe.hs` now calls the upstream
`Agent.Env.SQLite.createAgentStore` with the application migration catalog and
patched custom-VFS SQLite wrapper. It passes the target typecheck and emits 178
AArch64 objects. The target-only link now resolves all symbols after adding
SQLCipher, the vault-backed SQLite VFS, a LibTomCrypt adapter for upstream
SNTRUP SHA-512, GHC runtime support, and bounded Microkit shims. The packager
adds the existing sDDF timer driver and configures the messages protection
domain's timer channel.

QEMU first exposed a missing monotonic timer, then a missing wall clock. With
the timer driver and a fixed QEMU-only wall-clock fixture, the real upstream
AgentStore migrations pass on a pristine disposable vault disk. The probe now
creates an upstream AgentStore user record, reads it back, and verifies the
same record ID after a separate QEMU reboot with a non-empty test key. A third
boot rejects a deliberately wrong key without changing the disk. The repeatable
three-boot smoke target passes. This validates the upstream database path and
encrypted persistence, but not full SimpleX agent or network startup. Next,
connect this store to upstream SimpleX network startup and attempt a local queue
and message round trip. The fixed clock exists only for QEMU; device builds
require the trusted OS clock service. Pixel Wi-Fi firmware and a seL4 WLAN
driver remain a separate hardware milestone.

## Upstream agent lifecycle in QEMU (2026-10-13)

`SimplexAgentRuntimeProbe.hs` now opens the persistent upstream AgentStore and
starts/disposes the real upstream `Agent.getSMPAgentClient` with an empty server
set. The combined ARM64 image links with no undefined symbols. Its repeatable
`make guest-simplex-agent-runtime-qemu-smoke` test passes a fresh boot, agent
startup and disposal, same-key store reopen with the same user record, and
wrong-key rejection without disk mutation. The first runtime boot exposed that
`messages_app` reaches the entropy PD on channel 3 in the storage system; the
existing Haskell entropy bridge was compiled for channel 1, which belongs to a
separate probe. The runtime now compiles that bridge for channel 3.

This proves the upstream agent can coexist with the encrypted store and obtain
working QEMU entropy. At this point the server set was deliberately empty; the
local-server checkpoint below extends it to a real upstream TLS/SMP session.

## Upstream SMP queue lifecycle in QEMU (2026-10-13)

The repeatable `make guest-simplex-agent-runtime-qemu-smoke` target now boots
the upstream agent with the encrypted AgentStore, QEMU VirtIO networking, the
isolated lwIP/DNS service, and the existing protected TCP stream. It connects
to a disposable local upstream SMP server using the fixture CA, runs
`Agent.testProtocolServer`, and verifies queue creation, queue authentication,
and deletion. The smoke also reboots with the same key and checks the user
record ID remains stable, then tries a wrong key and verifies the disk is
unchanged. This passes in QEMU.

Two integration fixes were needed: the messages PD's TCP protected endpoint
must use the DHCP service's reserved channel 4, and the GHC wait bridge must
dispatch all pending channel bits rather than assuming a timer-only badge. TLS
also needs the OS clock capability; QEMU uses the existing PL031 clock service
instead of the deliberately unavailable wall-clock hook. The clock and network
fixtures are test-only and do not validate Pixel Wi-Fi or hardware time.

This is a real upstream client-to-server queue lifecycle, not yet a chat
message: `testProtocolServer` creates and deletes its test queue. The latest
runtime probe goes further and starts the invitation handshake, but does not
yet reach an established connection or send a chat message.

## Upstream invitation and message round trip in QEMU (2026-10-14)

The QEMU runtime probe now creates and joins an invitation under one AgentStore
user, waits for both `CON` events, sends an end-to-end encrypted message,
receives it on the joined connection, and acknowledges it. The event collector
keeps events for the other connection instead of discarding whichever `CON`
arrives first. A direct SQLite trace exposed a 1 MiB guest C allocator
exhaustion during message insertion; the runtime now links a dedicated 64 MiB
allocator for SQLite and the message path completes.

`make guest-simplex-agent-runtime-qemu-smoke` passes the encrypted
invitation/message/ack round trip, same-key persistent-store reopen with a
stable user record, and wrong-key rejection without disk mutation. The peer is
another connection within the same AgentClient, so this demonstrates the
upstream protocol and crypto path against the local SMP server, not yet
interoperability with a second independent SimpleX installation. Pixel Wi-Fi
remains a separate transport milestone; no Pixel test or install is involved
in this QEMU result.

## Upstream CLI interoperability in QEMU (2026-10-14)

`make guest-simplex-agent-cli-interop-qemu-smoke` now joins the guest's
invitation with an unmodified upstream SimpleX CLI using a separate database.
The CLI joins the guest's invitation, receives and displays its encrypted
`x.msg.new`, then sends `hello back from upstream SimpleX CLI`; the guest
receives the reply and acknowledges it. The harness reconnects the CLI after
its one-shot invitation command so its receive queues are subscribed before
it waits for the guest's message. It also preserves out-of-order Agent events
in the probe's event backlog, so waiting for a delivery result cannot discard
an incoming chat message.

The same smoke test then shuts down and restarts the guest against the same
encrypted disk and the CLI against the same database. It confirms that the
guest restores the exact connection ID created before shutdown and sends a new
encrypted message that the CLI receives. The restored connection reports
active receive and send queues and the guest receives the prior delivery
receipt. The first version of this smoke test then ran `/help` on the CLI and
checked for the guest's outbound message; it did not actually send a
post-reboot CLI-to-guest message, so that run did not test restored reverse
delivery. The harness now sends a real message on the saved contact. Bulk
`subscribeAllConnections` reported success but the server later showed the
restored queue with no live subscriber and three queued messages. Explicitly
calling `subscribeConnection` for that restored connection after bulk restore
caused the guest to receive and acknowledge the CLI's exact post-reboot reply.
The latest clean end-to-end QEMU smoke run passes both directions before and
after reboot. A prior run reached the
same inbound-ack success but was falsely marked failed because a serial read
collected that success marker and `HASKELL_EXIT` together; the harness now
checks accumulated output before treating process exit as failure.

Two supporting fixes were needed. The guest TCP service's 8 KiB receive buffer
was smaller than SimpleX's 16 KiB transport block; increasing it to 32 KiB
allows a 20 KiB inbound and outbound burst to pass in
`make haskell-net-qemu-smoke`. The TLS backend preserves partial records. The
initial live-push path still needs investigation: after the peer's reply, the
Agent received its outgoing delivery receipt, but the broker left the reply
queued and marked delivered without a corresponding unsolicited `MSG` reaching
the Agent. A duplicate `SUB` reliably replayed that message in the correlated
response, and the Agent decrypted and acknowledged it. The smoke gate now
exercises that recovery behavior. Same-contact delivery after reboot also
passes following an explicit subscription. This establishes bidirectional
encrypted protocol and persisted state over QEMU VirtIO with a tested replay
recovery; it does not establish reliable live push, phone Wi-Fi, production key
handling, or a usable phone deployment.

The local server, SOCKS bridge, CLI database, and guest disk are disposable
QEMU fixtures. No Pixel test, install, or flash is part of this checkpoint.

## From QEMU interop to the OS Messages app

The upstream Agent now also boots as its own long-lived protection domain
beside the real Rust Messages protection domain in a QEMU service smoke. The
Messages service issues a protected `ListConnections` request over the NXSM
page, decodes the bounded snapshot, and keeps running beside the Agent. The
interop test image separately proves encrypted exchange with the unmodified
CLI, encrypted AgentStore persistence, and reboot restoration. The service
smoke currently exercises only the list operation; it does not yet boot the
Slint UI or route invitation, send, receive, and acknowledge actions through
that UI. The normal Messages model still stores peer-key metadata and PSL1 demo
packets, so the standard app is not yet a SimpleX-enabled messenger.

The next product milestone is a Slint-to-Messages-to-Agent exchange in QEMU,
using the real upstream Agent and a second SimpleX client:

1. Add the Slint UI to the service image. Keep the upstream Agent in its own
   protection domain with its encrypted store, entropy, trusted-time, and
   network capabilities; the UI keeps no network or vault capability.
2. Extend the existing bounded NXSM request/reply IPC between Messages and
   Agent to cover: list
   connections, create/join an invitation, send a UTF-8 message, poll/ack
   incoming messages, and return stable connection/message identifiers. The
   Agent service already implements these operations and the Messages model
   already has feature-gated handlers; connect and exercise them through the
   normal Slint route. Keep variable-size payloads bounded and validate every
   length and operation.
3. Connect the Slint Messages list and chat view to the Rust Messages service.
   The UI requests snapshots and sends user actions; the Messages service
   mediates those actions to the Agent, persists display preferences only in
   its own namespace, and publishes updated snapshots. The agent's SQLCipher
   store remains the source of truth for SimpleX protocol state.
4. Extend the current QEMU/CLI interop test to boot the normal OS image, open
   Messages through Slint, join a CLI invitation, exchange messages in both
   directions, reboot with the same encrypted disk, and exchange again. This
   is the first milestone that qualifies as SimpleX running as the OS's
   Messages app rather than a guest-only protocol probe.

The main integration risk is long-lived Agent event handling alongside
Microkit IPC: protected calls are synchronous, while subscriptions and incoming
events must stay live without starving UI/input or network services. Keep
message polling bounded and event-driven; retain the tested explicit
per-connection resubscription after restoring Agent state. Do not route SimpleX
messages through the existing PSL1 sealed-datagram demo.

The bounded IPC page is now defined on both sides:
`rust/src/simplex_ipc.rs` and `experiments/ghc-microkit/SimplexIPC.hs` use
`NXSM`, version 1, a 20-byte little-endian header, and a 4076-byte payload.
The operation payload primitives now match on both sides: `u8`, little-endian
`u16`, little-endian `u64`, and a byte string encoded as a little-endian `u16`
length followed by its bytes. Decoders must consume the complete payload;
truncated and trailing data are errors. Text byte strings are UTF-8 and must
be validated by the operation handler. The Rust standalone tests and
`make -C experiments/ghc-microkit simplex-ipc-host-test` cover the shared byte
vector plus request correlation, invalid operations, corrupt headers, payload
bounds, truncation, and trailing bytes.

The intended v1 operation schemas are:

| Operation | Request payload | Successful response payload |
| --- | --- | --- |
| ListConnections | empty | `u16 count`, then count records of `u64 handle`, `bytes display_name`, `u16 unread_count` |
| CreateInvitation | empty (uses the local Agent profile) | `u64 handle`, `bytes invitation_uri` |
| JoinInvitation | `bytes invitation_uri` | `u64 handle` |
| SendMessage | `u64 handle`, `bytes utf8_body` | `u64 message_id` |
| ReceiveNext | empty | `u8 event_kind` (1 = message), `u64 handle`, `u64 message_id`, `bytes utf8_body`; use `NoEvent` status when empty |
| Acknowledge | `u64 handle`, `u64 message_id` | empty |
| DeleteConnection | `u64 handle` | empty |

Handles are nonzero UI selectors derived deterministically from the Agent's
connection ID; they are not authentication tokens or SimpleX server IDs. The
mapping remains stable because the connection ID is stored in the encrypted
AgentStore. Before operations accept handles, the Agent must check for
collisions against stored connections. Text fields must be UTF-8, bounded, and
consumed completely by each operation handler.

## NXSM sidecar capability smoke (2026-10-15)

`make -C experiments/ghc-microkit simplex-ipc-sidecar-qemu-smoke` now builds
the full existing QEMU OS image with two additional test PDs: a passive
Agent-shaped service and a one-shot client. They exchange a `ListConnections`
request and an empty-list response over the versioned NXSM page using a real
Microkit protected call. QEMU prints `SIMPLEXIPC|PASS_GUEST_PROTECTED_CALL`.
The normal `rust_messages_app.elf` remains in the image. The service maps only
the NXSM page and has no vault, network, or UI mappings; the probe client is
separate from Messages so this does not falsely imply the Messages UI is
integrated.

This verifies the sidecar memory layout, endpoint priority, shared-page ABI,
and request/response path in a booted guest. The responder is a small C
protocol fixture, not the Haskell SimpleX Agent. The next integration step is
to give the real Agent a separate PD and its own encrypted-store and network
capabilities, then map the NXSM page and endpoint into the Messages service
and replace the fixture response with real Agent operations. The QEMU smoke
uses a disposable copy of the vault disk and does not modify the base OS disk.

## Messages-to-Agent QEMU vertical slice (2026-10-15)

The QEMU integration runs the real upstream Haskell SimpleX Agent in a
dedicated `simplex_agent` protection domain alongside the Rust Messages PD.
The Agent opens its SQLCipher-backed AgentStore through its own vault endpoint,
starts upstream network workers through guest network services, initializes
database migrations, and stays alive as a service. The feature-gated
`rust_messages_agent_app` is the Messages binary in this integration image; it
keeps the normal vault, view, and network-transmit capabilities and also gets
the bounded NXSM page plus one notification endpoint to the Agent.

On the existing namespace-check request, Messages asynchronously asks the
Agent for `ListConnections`. The Agent reads its connection table and replies
through the same page. Messages validates the correlated response, loads its
vault-backed model, adds the connection count to its rendered snapshot, and
logs `SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|0`. The QEMU smoke verifies
that marker, the Agent-side result, and that the Agent stays alive. The host
only launches QEMU and disposable SMP/DNS fixtures.

The request page is polled every 10 ms as well as signaled by a Microkit
notification. Its status field is authoritative, so a notification during
Agent startup cannot be lost while migrations run. Both the standalone Rust
ABI client and the actual Messages-to-Agent QEMU round trip have passed.

The Agent also handles `CreateInvitation` over that page. It calls the
upstream `Agent.createConnection` against the local SMP fixture and returns a
stable handle plus the serialized invitation URI. The Slint QEMU smoke taps
NEW LINK, observes the pending state, then refreshes after the Agent replies.
It validates the response length without printing the bearer URI. The handle
is derived from the stored connection ID; collision checks are required before
other operations accept it.

This is still not a complete messenger. The Messages screen can now request an
invitation and display its URI in a dedicated view; the QEMU smoke exercises
that action and confirms the 469-byte URI reaches Slint without printing the
bearer value. Listing still returns a count, and joining an invitation plus
message send/receive are not connected. The Agent and Messages still share the
Messages vault namespace, so storage policy needs a dedicated Agent namespace
before treating this as a hardened per-app boundary.

The next integration step is to return bounded connection records and
implement join plus message send/receive over the same request page. After the
QEMU vertical slice, replace QEMU-only network, clock, and entropy fixtures
with device services.
Pixel Wi-Fi remains the largest hardware gate: the Pixel image has no usable
WLAN network domain, so a working Pixel SimpleX client cannot be reached just
by enabling the Agent. UTM can exercise the integrated app because it already
has a QEMU network path.

## Slint Messages snapshot (2026-10-15, extended 2026-10-16)

The normal Slint UTM suite now calls a dedicated Messages endpoint and reads a
bounded local-model snapshot through `messages_transfer`. The Messages PD
loads and owns its vault-backed model, formats the snapshot, and gives the
Slint PD no vault or network capability. The Messages page displays this
service-backed state instead of placeholder sample threads in the UTM guest.
The UTM QEMU interaction test checks the Messages screen against the host
reference and requires the `SLINT MESSAGES LOAD PASS` trace before proceeding.

The Agent-enabled Slint QEMU image now combines this view with the upstream
Agent service. The Agent has a separate vault transfer page, so its SQLCipher
file I/O cannot race Messages' file operations over a shared page. On entering
Messages, the Slint service reloads the snapshot after the Agent has had time
to answer; the display then includes the returned connection count. This is
triggered by the input service because this freestanding Slint runner dispatches
window events directly and does not run Slint's timer event loop.

The integrated Slint smoke boots the real Agent, verifies the AgentStore and
Messages IPC round trip, sends QMP touches to open Messages and request an
invitation, and requires Slint to receive and display its URI. It passes in
QEMU, including the screen refresh after the touch. The test database currently
contains zero connections. Listing connection records and sending or receiving
messages through Slint remain open. The Messages input route uses its own
endpoint because the clock service already occupies the former Messages input
endpoint.

The service Make target packages a headless Agent and Messages image. A
separate cached merged image contains Slint, Messages, and Agent; its QEMU
smoke now boots the current Slint, Agent, and Messages binaries, taps Messages,
creates an upstream invitation through the UI, and refreshes the Agent-backed
connection list. The smoke harness checks for a `slint_ui` protection domain
before accepting `--slint-ui`, so the headless service image cannot be
mistaken for this UI integration test. The merged Slint smoke does not yet
exercise peer join or chat send/receive.

## Bounded invitation join RPC (2026-10-16)

The real Agent service now accepts the existing `JoinInvitation` operation.
Its C bridge copies request payloads into one reusable, fixed-size buffer, and
the Haskell service validates the length-prefixed ASCII URI before parsing it
with the upstream SimpleX URI decoder. A valid URI is passed through
`prepareConnectionToJoin` and `joinConnection`; a failed JOIN removes the
prepared connection record. A successful response contains only the stable
opaque connection handle.

The integrated QEMU smoke builds a test-only Messages variant that passes the
fresh invitation URI directly back through the join RPC. This exercises the
bounded payload copy, upstream URI parser, and `prepareConnectionToJoin` /
`joinConnection` call without printing the invitation. The test runs against a
disposable disk and local SMP server; it is a self-join integration check, not
yet a test with a second user or the Slint link-import flow.

The Slint Messages screen now has a typed invitation-import view with a
case-sensitive URL keyboard, a 2,048-byte bound, and a dedicated Messages-to-
Agent join request. The URI stays within the UI, Messages and Agent IPC path;
it is not written to debug logs. The UI reports join acceptance separately
from peer confirmation, and can query the asynchronous Agent result. The
full Slint image builds and the integration QEMU smoke still passes.

Manual entry is only a bring-up path for these long bearer links. A QR scanner
or a secure paste/input service is still needed for practical daily use. The
existing QEMU test self-joins a freshly created invitation, so a two-device
peer confirmation and encrypted send/receive through the Slint Messages view
remain the next protocol milestones.

## SimpleX chat operations in the Agent service (2026-10-16)

The Agent service now implements bounded `SendMessage`, `ReceiveNext`, and
`Acknowledge` handlers in addition to connection listing and invitation
create/join. Send validates a nonzero stored-connection handle and UTF-8 text
up to 2,048 bytes, then calls the upstream Agent. Receive scans at most 32
queued Agent events per poll, returns the decoded text event with its stable
connection handle and message ID, and reports `NoEvent` distinctly. Acknowledge
validates the handle/message-ID pair before calling the upstream Agent. The
request buffer now covers the full shared-page payload capacity.

The Slint composer and Messages PD have a send-request/status path. The Agent-
backed Messages page also has an inbox card that starts a bounded receive poll,
shows the asynchronous result, displays incoming text, and acknowledges a
received event. Acknowledgement clears the stored event identity so the next
poll can continue. The merged Slint QEMU smoke now creates an upstream
invitation, refreshes the connection list, and polls an empty inbox through the
Slint → Messages → Agent path; it passes with `RECEIVE_EMPTY`. This verifies
the no-event response and UI request path, not delivery of an actual message.

The Agent's send and acknowledge operations still need runtime coverage. The
remaining protocol checkpoint is a two-client QEMU exchange that joins a real
peer, sends text through the Slint composer, receives it on the other client,
and acknowledges the event. The merged Slint image is still assembled from a
cached system description rather than a reproducible source target, so that
packaging step should be made repeatable alongside the exchange test.

The existing standalone Agent/CLI bidirectional smoke was also rerun. The CLI
peer joined and the guest issued an SMP `SEND <MSG>`, but the guest did not
observe its expected Agent `SENT` event before the smoke timeout. This is an
unresolved transport/event confirmation issue; it does not invalidate the
passing invitation, list, and empty-inbox Slint checks.

## Slint ↔ independent SimpleX CLI encrypted exchange (2026-10-16)

The repeatable QEMU gate is
`make -C experiments/ghc-microkit BUILD_DIR=../../build guest-simplex-agent-ui-qemu-smoke`.
It boots the real upstream Agent in its own protection domain beside the Slint
and Messages domains, with the encrypted AgentStore, local SMP fixture, and
QEMU network stack. An independent upstream SimpleX CLI process joins the
invitation created through Slint. Slint then sends `HI FROM SLINT`; the CLI
receives it and replies `REPLY FROM SIMPLEX CLI`. Slint receives the reply and
acknowledges it through the Messages → Agent IPC path. The gate passed on
2026-10-16.

The Agent service processes incoming `CONF` and `CON` events to allow and
subscribe accepted connections. `ReceiveNext` now waits for the upstream Agent
message event instead of returning `NoEvent` immediately, and retries a queue
subscription when delivery is delayed. The smoke starts the Slint inbox wait
before asking the independent CLI to send its reply. The standalone upstream
Agent/CLI bidirectional probe also passes, including a same-contact encrypted
send after a QEMU guest reboot.

This is a local QEMU interoperability test. The independent CLI and disposable
SMP server are host-side test fixtures; SimpleX application logic exercised by
the test runs in the guest. The test does not establish Internet access, Pixel
4a Wi-Fi, persistent chat history in the Slint Messages UI, or production-ready
key backup and account recovery. Its invitation-export binary is test-only and
must not be used as a device image because it emits a bearer invitation over
the QEMU serial test channel. Device networking, durable UI chat history, and
real-device verification remain open.

## References

- [SimpleX mobile and desktop architecture](https://github.com/simplex-chat/simplex-chat/blob/stable/apps/multiplatform/README.md)
- [SimpleX protocol layers](https://github.com/simplex-chat/simplexmq/blob/stable/protocol/overview-tjr.md)
- [SMP agent state and connection protocol](https://github.com/simplex-chat/simplexmq/blob/stable/protocol/agent-protocol.md)
- [SimpleX CLI storage and transport details](https://simplex.chat/docs/cli.html)
- [Personal OS network feasibility experiment](network-spike.md)
