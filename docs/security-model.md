# Initial security model

## Objective

Explore a personal OS built from components with explicit authority. The current
experiment demonstrates native IPC, channel-based data authorization, per-app
storage namespaces, and containment of an unauthorized memory write across nine
application domains. The
Rust supervisor calls a stateful Rust calculator, a C reference service, and a
private-data vault; Rust clock and console components provide controlled device I/O.
It is a development system with debug facilities, not a hardened phone OS.

## Components and authority

| Component | Granted authority | Private state |
| --- | --- | --- |
| Client/supervisor | Call Rust server on channel 0, C reference on channel 4, and vault on channel 6; notify crasher on channel 1; receive child faults and control its thread; accept bounded console requests; debug console | Reply checks, fault-test state, failure count |
| Server (Rust) | Receive protected calls; reply; read/write its private page; debug console | Guard and accepted-addition count |
| C reference server | Receive protected calls; reply; read/write a separate private page; debug console | Guard and accepted-addition count |
| Vault (Rust) | Receive system calls on vault-local channel 1, child calls on channel 2, Notes calls on channel 3, Contacts calls on channel 4, and the sDDF block queue on channel 0; map its private state and transfer buffers; debug console | Three independent two-slot records on disk |
| Notes demo app | Call its fixed vault endpoint and receive a supervisor namespace test; in the separate graphics/input image, receive routed events from the trusted input service and write only its display snapshot | Notes namespace record, input sequence, and its own view page |
| Contacts demo app | Call its fixed vault endpoint and receive a supervisor namespace test; in the separate graphics/input image, receive routed events from the trusted input service and write only its display snapshot | Contacts namespace record, input sequence, and its own view page |
| Crasher (Rust) | Receive start notification; call vault on channel 1; debug console | Private attempt counter; no service data-page mapping |
| Clock (Rust) | Map QEMU PL031; receive its alarm interrupt; signal console; answer elapsed-time queries | Starting RTC seconds |
| Console | Map QEMU PL011 input; receive UART interrupts; send calculator/status/crash requests to supervisor | Fixed-size command line and parser state |

Channel IDs are local to each protection domain: the crasher calls the vault over
its own endpoint 1, while the vault receives that call on endpoint 2. The Notes
and Contacts domains each have a separate endpoint, mapped by the vault to a
fixed namespace; neither app has the other's endpoint or the system namespace
endpoint. Their storage requests carry no namespace ID. Each domain has the
runtime/kernel objects needed to execute. The calculator
servers have no outbound application endpoints. The client and crasher cannot map
or directly read or write their private pages. The vault page is mapped only by
the vault. The child has a vault endpoint, but the vault denies its requests using
the endpoint identity supplied by Microkit; the request carries no caller ID to
spoof. The C header defines a protocol, not shared runtime storage.

## Scheduling budgets

The QEMU target uses seL4's MCS kernel. The active supervisor and console each
receive a 3,000 microsecond budget every 10,000 microseconds; the clock and crasher
each receive 1,000 microseconds in that period. These limits reserve at most 80%
of the single CPU across those four domains, leaving 20% outside their budgets.
When a domain exhausts its budget, it cannot run again until the next replenishment.

The Rust calculator, C reference server, and vault are passive. After initialization,
Microkit runs their protected calls using the caller's scheduling context. Their
work therefore consumes the caller's budget; they do not have independent per-
service CPU caps. A future independently budgeted service would need an active
worker and an asynchronous request/reply design.

The separate graphics/input test image adds an active Rust input router with a
1,000 microsecond budget per 10,000 microseconds. The VirtIO driver can notify
it and write only the bounded shared event ring. The router alone holds protected
call capabilities to the Notes and Contacts input endpoints, validates every
record again, and sends new input to the currently focused app. Focus starts
empty; tablet presses inside the visible Notes or Contacts panel select that
app. Tablet coordinates outside the panels do not change focus. A key repeat or
release remains with the app that received its press, even across a focus
change. Physical input is user-controlled, so touch selects which app receives
input; it does not authenticate the input source. The router publishes focus
through a shared word it alone can write. The GPU client has read-only access
and receives a notification to redraw the selected panel's border. Notes and
Contacts write separate bounded text snapshots; the GPU client maps both pages
read-only and displays only the focused app's snapshot. The display client is
trusted to see these display projections, while neither app can read or modify
the sibling's page.

## Interface

- `REQUEST_ADD`: exactly two unsigned machine words; reply is one result word.
- `REQUEST_COUNT`: zero words; reply is one counter word.
- `REQUEST_GUARD`: zero words; reply is the private page's guard value.
- Unknown operations and wrong lengths return `REPLY_BAD_REQUEST`, zero words.
- Addition overflow returns `REPLY_OVERFLOW`, zero words.
- Error replies do not modify the counter or expose reply payload words.

The count and guard are intentionally readable through the calculator service by
the client. The vault stores three independent records—system, Notes, and
Contacts—in two 4 KiB disk slots apiece. The vault maps each caller endpoint to
one namespace; callers cannot name a different namespace in the request. The
crasher's channel is denied. Writes carry an expected generation, so stale updates
fail without changing that namespace's record. Each disk slot uses AES-256-GCM-SIV
with its namespace and generation authenticated as associated data, plus a commit
marker written after the record body. The nonce is derived from the namespace and
generation; AES-GCM-SIV limits the damage from a repeated nonce, while a rollback
can still restore old data. `./dev test-storage` stages four incomplete or
corrupt forms of a newer slot, rebooting the same disk between each case and
checking that the prior authenticated value remains readable. The first boot can
migrate the previous plaintext demo format, whose old checksum did not provide
authenticity. The full-system test app domains each
read and update their own namespace, submit malformed requests containing a
different namespace-looking value, and verify those requests do not change their
record. Their values survive the second QEMU boot. This proves the fixed channel
mapping used by this prototype; adding installable apps or dynamic namespace
creation requires a separate policy design. Future access rules should use
kernel-supplied channel
identity plus object-level policy; the kernel does not understand application
concepts such as calendar files. The test-only binary has an owner-only request
that advances this diagnostic sequence; the regular vault does not implement it.

The record layer uses a `VaultCryptoProvider` operation interface: it asks a
provider to encrypt or authenticate/decrypt bytes and never receives key bytes.
Provider errors remain distinct from invalid records, so the vault reports a
storage failure instead of misclassifying a service outage as disk corruption.
The feature-gated QEMU `DevelopmentCryptoProvider` keeps a fixed development key
inside its private software-provider object. That object still runs in the vault
process, so this is an API boundary, not hardware isolation or a trust anchor.
A future device provider must perform the current AES-256-GCM-SIV operation
inside its trusted service, or define a versioned record-format change if its
hardware only supports another AEAD such as AES-GCM.

The Rust server implements the full request protocol and owns the state page used
by the fault experiment. Its Rust supervisor checks valid requests, wrong lengths,
unknown operations, and overflow. The original C server runs beside it with a
separate state page; the supervisor checks that its replies match the same interface.
The Rust handler uses a narrow unsafe volatile-access wrapper for the Microkit
mapped state page; request decoding and arithmetic stay in safe Rust.

## Fault experiment

The supervisor completes the baseline protocol checks, verifies the Rust server's
guard, and notifies the Rust crasher. It emits an explicit AArch64 store at
`0x40000000` using inline assembly because that unmapped address is intentionally
not represented as a Rust pointer.
That virtual address has a mapping only in the Rust server. Knowing the address does
not grant access to its physical frame in another domain.

seL4 sends the crasher's fault to its parent through Microkit's fault entry point.
The parent accepts only an armed fault from child 1, with the VMFault
label/length, expected address, data access rather than instruction fetch,
AArch64 lower-EL data-abort syndrome, write flag, and translation-fault status.
It explicitly stops the child's thread and does not reply to resume execution.
Once the boot demonstration completes, a checked console request can restart this
contained test child at a small Rust fault trampoline. The supervisor changes the
program counter and resumes the same thread; it does not rerun the Rust initializer
or clear the child's private memory. A private attempt counter confirms that state
persists across restarts. The child again stops on its first unauthorized write,
which exercises a deliberate retained-state policy, not a generic service reset.

Only after verifying the fault does the supervisor call the Rust server again.
It checks that the guard is unchanged, the counter is still 2, a new addition
returns 123, and the counter advances to 3. This is evidence of post-fault progress
and state preservation, not just output produced before the fault.

Two disposable negative controls verify the host test rejects (1) accidentally
granting the crasher write access and (2) a genuine fault at an unrelated address.
The normal system never includes those modifications.

## Trust and limits

The host macOS, QEMU/UTM, SDK/toolchain, image loader, kernel, initializer, and
Microkit runtime/monitor are trusted for this experiment. The host can inspect
all guest memory. Debug console output is a shared observable channel, so this
configuration does not claim confidentiality between arbitrary hostile programs.

The normal boot and interactive tests demonstrate correct message exchange,
rejection of malformed console inputs, one unauthorized-write fault with continued
service operation, denied vault access, and repeated use of the crasher trampoline. A separate build
keeps the crasher in a compute loop; the test requires the console, calculator, and
RTC heartbeat to progress while that loop consumes its capped share. This tests one
CPU-exhaustion case, not arbitrary resource exhaustion or general service recovery.
The test denies a vault read through an untrusted endpoint, but does not test direct
unauthorized memory reads, sustained keyboard flooding, all protocol inputs, or side
channels.
The Rust client/supervisor is trusted to handle the fault correctly. Its authority to
stop or restart its child is intentional; the child cannot control its parent.
There is no claim that the whole OS is formally verified. Microkit's manual
documents the verification status of its MCS kernel configuration separately.

The regular CLI image has a persistent QEMU disk; the UTM exporter attaches a
separate persistent disk inside its bundle. A warm reset exposed that guest RAM
and the virtio device can reset at different times. The tracked sDDF patch clears
the virtqueue and resets shared queue counters during initialization. Three
consecutive boots in one standalone QEMU process, including QMP `system_reset`,
then passed all isolation checks and recovered the committed record. The
earlier namespace-enabled bundle passed 24 supervisor checks and eight namespace
assertions in UTM, before the entropy service was added. It was warm-restarted
twice; both boots returned to the
console with all checks passed. Notes and Contacts advanced from generation 1 to
3, and read-only inspection confirmed both slots for each app were valid. These
emulator reset tests do not simulate a host power loss. New records are encrypted
before they enter the sDDF transfer buffer, but the development key is available
only when the explicit `development-crypto` feature is enabled. QEMU builds enable
that feature and warn that a person with the image can extract the key, so this
build does not provide meaningful secrecy against its host; a hardware-backed key
service and key lifecycle remain future work. The offline restore tool also refuses
older authenticated generations relative to the selected source disk unless
`--allow-rollback` is passed. This is not a trusted counter: an older source image
can bypass the check. The repository patch now negotiates and
issues the VirtIO block flush command, and the vault waits for it at each
two-phase write boundary. The standalone storage suite now also triggers guest
system resets before and after each flush boundary and verifies recovery of the
valid old or new record. QEMU and its host disk backend stay alive during those
resets, so they do not establish durability across host power loss or a dropped
host write cache.

The emulator has a separate passive entropy service. It alone maps the VirtIO
MMIO page and DMA buffers; the supervisor receives samples through one protected
call endpoint, while the vault has no RNG-device mapping. Boot tests require four
distinct 128-bit samples, and the storage suite repeats that check after each of
four QMP guest resets. The suite also restores a full QEMU snapshot captured
before guest execution and boots two independent QEMU instances from copies of
that same snapshot. Their 64-bit test fingerprints differed in the recorded
run. This confirms different output on this QEMU backend for these restore and
clone paths; it does not establish entropy quality, Pixel behavior, or safety
across arbitrary snapshots and clones. QEMU's built-in RNG backend is an
emulator development source, not a physical entropy root. The vault continues
to use v1 records and does not consume this service.

The separate experimental QEMU graphics/input image adds an input driver and a
Rust input router, each with a 1,000 microsecond budget per 10,000 microseconds.
The driver has VirtIO MMIO/IRQ access. They share only a 32-record
single-producer/single-consumer handoff ring and a notification. The router
accepts only supported keyboard keys, tablet coordinates, left-button state,
and sync records; it rejects other combinations and tracks ring overflow. It
has separate protected endpoints for Notes and Contacts, and the apps validate
the forwarded event protocol and monotonically increasing sequence. The QEMU
test switches focus by tapping visible launcher panels and proves that neither
app receives the other app's key. This generated image is not part of the
regular build. The input driver currently maps the broader VirtIO MMIO window
used by the GPU integration, so a production device layout should narrow that
mapping. The GPU test client sees focus through a separate shared page mapped
read-only; the router sends a notification after selection, and the client
redraws only its own display surface.

The regular base build still has no display, modem, or network interface. The
emulated serial input and host-controlled RTC are development
interfaces, not phone devices. Adding physical devices requires reviewing
DMA, hardware/firmware access,
parsers, storage permissions, and the trust placed in any driver VM. Secure boot,
signed updates, rollback policy, and hardware-backed key management are later
design work.

## Acceptance for this milestone

The automated boot check starts the application domains, the entropy service and
the two sDDF block components, completes 25 ordered supervisor checks across the Rust server, C
reference, vault, app namespaces, and fault boundary, and verifies the initial
crasher fault is stopped. The entropy check obtains four distinct 128-bit
samples. Eight additional namespace assertions verify separate
Notes and Contacts records and reject caller-selected namespace fields. Vault
checks cover reads of the current committed record, authorized updates, stale-version
rejection, and a denied child write followed by an unchanged-record read. The
separate `./dev test-storage` suite covers recovery from four incomplete or
corrupt encrypted-slot states, verifies entropy delivery after four QMP guest
resets at write/flush boundaries, and compares short fingerprints across a full
QEMU snapshot restore and two independent VM clones.
Host-side crypto tests cover round trips, tag and namespace tampering, staged
records, and legacy-format recognition. Unexpected faults or monitor
errors fail the boot check. Host logs and image hashes are written to `build/`.
The refreshed UTM bundle uses the same ELF image with its own persistent disk and
interactive UART input. The previous namespace-enabled UTM image passed all 24
supervisor checks and eight app-namespace assertions before the entropy service
was added. A tracked sDDF initialization patch
clears stale virtqueue state after device reset; three consecutive boots in one
standalone QEMU process and two successive UTM warm restarts recovered the records
and passed all checks. Notes and Contacts advanced to generation 3 across the UTM
restarts, and read-only disk inspection confirmed both slots per app were valid.
The console suite exercises malformed inputs and subsequent child restarts. The
separate
`./dev test --budget-pressure` run
proves that the clock heartbeat and console calculator still respond while the
crasher spins within its CPU budget; the interactive test also verifies that the
crasher's private attempt counter advances across two trampoline restarts.
