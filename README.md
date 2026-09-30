# Personal OS

A small seL4/Microkit experiment for a future personal computing OS. Rust and C
application and service protection domains run alongside two sDDF I/O domains
and a passive QEMU entropy service on an emulated ARM64 board. The Rust
supervisor and interactive console exercise a Rust
calculator service alongside a C reference server; the calculator remains
available after the kernel contains a sibling's unauthorized
memory write. A private disk-backed vault also tests channel-based read/write
authorization and retains its committed record across QEMU boots. A separate
experimental graphics profile now includes isolated Notes, Contacts, a
four-function Calculator, a read-only UTC Clock app, a bounded Calendar reminder
list, an offline Messages peer-key shell, and a read-only Settings stub with routed navigation.

## Run

On this Mac, from any directory:

```sh
/Users/steve/github/sel4-personal-os/dev run
```

From a checkout:

```sh
./dev run
```

The command fetches the pinned Microkit SDK and sDDF source on first use, verifies
their SHA-256 digests, builds the components, assembles the system image, creates
the persistent data disk on first use, and launches QEMU with software emulation.
Exit QEMU with **Ctrl-A, then X**.

The guest first prints twenty-five ordered supervisor checks and eight
namespace-specific assertions, ending with:

```text
ISOLATION COMPLETE: 27 checks passed; crasher stopped, services alive
```

It then starts a keyboard-driven serial console. Enter `help`, `status`,
`calc 19 + 23`, or `crash`. The clock component raises an emulated RTC alarm; the
console reports a heartbeat every five seconds. `crash` restarts the contained
crasher, whose write is faulted again while the calculator service remains available.
The supervisor handles each intentional fault, so it does not appear as an
unhandled Microkit monitor error.

The boot checks also request four distinct 128-bit samples from the isolated
VirtIO RNG service. The storage recovery test repeats this check after each guest
system reset. This exercises QEMU device delivery and reinitialization; vault
records still use v1 and do not depend on this service.

## UTM

The generated VM is **Personal OS**, at `dist/Personal OS.utm`.

```sh
./dev utm
open -a /Applications/UTM.app "$(pwd)/dist/Personal OS.utm"
```

Press Play in UTM. Its built-in serial terminal shows the same demo. Stop the VM
before running `./dev utm` to refresh its image; the exporter checks for the
running bundle and refuses to overwrite it. It preserves the VM's identity and
configuration changes after its first creation. It adds a virtio-mmio data disk
for the vault and preserves that disk across exports. The CLI QEMU disk and UTM
disk are separate copies. If you move or copy the VM in UTM, the exporter
continues updating the original bundle in `dist`.

The profile uses ARM64 `virt`, Cortex-A53, one CPU, 2 GiB RAM, GICv2, and emulated
ARM virtualization extensions. Host acceleration and UEFI are disabled. The
image is supplied using UTM's **Linux Kernel** image slot because that slot maps
to QEMU's generic `-kernel` option; the guest is seL4, with no Linux guest involved.
`src/boot-image.S` wraps the unchanged Microkit image in an ELF envelope whose entry
point is `0x70000000`, so both launchers load it at the correct address.

The graphical build exports into the default experimental UTM profile with:

```sh
./dev utm-graphics
open -a /Applications/UTM.app "$(pwd)/dist/Personal OS Graphics.utm"
```

This reruns the QEMU graphics/input checks and refreshes that profile's image.
Pass `--profile <path>` only when you need a disposable side-by-side copy. The
default serial `Personal OS.utm` profile stays unchanged.

## Vault backups

Stop the VM before backing up its disk. Export the default CLI disk with:

```sh
./dev backup-export --output ./vault.pvlt
```

For the UTM disk, pass its raw disk path explicitly:

```sh
./dev backup-export --disk "dist/Personal OS.utm/Data/personal-os-disk.img" --output ./vault.pvlt
```

Restore makes a new disk-image copy and leaves the source unchanged:

```sh
./dev backup-restore --archive ./vault.pvlt --output ./restored.img
./dev run --disk ./restored.img
```

Restore compares the archive's authenticated generations with the selected source
disk and refuses older records by default. For intentional recovery, add
`--allow-rollback`; this check only sees the source image supplied to the tool
and is not a trusted anti-rollback counter.

The tool refuses to read a disk image while its QEMU/UTM VM is running and will
not overwrite an existing archive or output image. It accepts this project's raw
GPT disk layout. Legacy plaintext values are encrypted in the archive at a new
generation, but their old checksum cannot prove they were authentic. This is an
offline host tool; there is no backup screen in the guest yet. The archive
contains encrypted records but no key. The prototype key is embedded in the
source and boot image, so this does not provide confidentiality from someone
with either one.

## Commands and requirements

| Command | Result |
| --- | --- |
| `./dev setup` | Download and verify the SDK, check required tools |
| `./dev build` | Build the Rust components and sDDF block stack; produce boot images and a system report |
| `./dev run` | Build and run an interactive QEMU serial console with `build/personal-os-disk.img` |
| `./dev test` | Run AEAD and backup-format unit tests; check VirtIO entropy delivery, service permissions, encrypted vault persistence, backup export/restore, fault isolation, malformed console input, RTC, calculator, and child restart |
| `./dev test --timeout 40` | Allow a slower host more time to boot |
| `./dev test --budget-pressure` | Temporarily build a spinning crasher and verify other services progress under its CPU budget; restore the normal image afterward |
| `./dev test-storage` | Verify entropy delivery across QEMU boots, four scalar and four blob QMP write/flush resets, a full VM snapshot restore, and two cloned VM instances; check recovery from malformed encrypted slots |
| `./dev test-graphics` | Verify the sDDF GPU sample renders exactly on QEMU `virt`, then merge it with the regular OS and rerun isolation checks |
| `./dev test-input` | Test isolated app input routing, exact Notes/Contacts/Calculator/Clock/Calendar/Messages/Settings frames, calculator arithmetic by touch and keyboard, and Notes/Contacts/Calendar cold-boot persistence in QEMU |
| `./dev test-net` | Standalone sDDF echo_server DHCP+UDP, then merge serial-free `tests/net` `dhcp_client` into Personal OS; require isolation complete, DHCP, DNS OK, TLS OK, UDP/TCP echo, Messages sealed TX, and toy-router guest B `SEAL OK` (default/UTM images stay offline) |
| `./dev utm-graphics` | Verify graphics and input, then create a separate experimental `dist/Personal OS Graphics.utm` bundle; pass `--profile <path>` to create a side-by-side profile |
| `./dev backup-export --output vault.pvlt` | Export the latest authenticated records from the default CLI disk into a new archive |
| `./dev backup-restore --archive vault.pvlt --output restored.img [--allow-rollback]` | Restore into a new copy of the default CLI disk; older authenticated generations require the explicit override |
| `./dev run --disk restored.img` | Boot QEMU from a selected raw disk image |
| `./dev utm` | Build and create/update the UTM bundle |
| `python3 tests/negative_controls.py` | Verify broken isolation and an unrelated fault are rejected |

This initial bootstrap supports **Apple Silicon macOS**, Python 3.12+, Rustup/Cargo,
LLVM/LLD, and QEMU. UTM is optional for the command-line workflow. The development
machine already had the required tools; no Homebrew packages were installed by
this setup. For another Mac, install missing native tools with
`brew install python llvm lld qemu`; install Rustup from [rustup.rs](https://rustup.rs).
The first Rust build downloads the pinned nightly toolchain. Set `LLVM_BIN` if LLVM
is installed outside the usual Homebrew paths.

The build and tests need `make`, `dtc`, `gdisk`, and `mkfs.fat` (from `dosfstools`).
They download the pinned sDDF archive and install the pinned `sdfgen` package in
`.cache/sddf-venv` on first use. `./dev test-storage` additionally builds a small
standalone recovery test on disposable disks and uses `qemu-img` to prepare
temporary qcow2 disks for whole-VM snapshot and clone tests. It logs only a
64-bit test fingerprint, never the RNG samples.

`toolchain.lock.json` pins Microkit **2.3.1**, its official archive URL and digest,
the sDDF source revision and archive digest used by the regular image and tests,
and the emulated machine settings. The build installs the pinned `sdfgen` version
into a cache-local Python environment. The Rust components have been compiled and
boot-tested with that 2.3.1 SDK. `rust/Cargo.lock` pins the Rust seL4 bindings;
`rust/rust-toolchain.toml` pins the compiler. LLVM **23.1.2**, QEMU **11.1.1**, and
UTM **5.0.3** were used for initial validation. LLVM/QEMU versions are recorded,
not automatically installed or enforced. The SDK archive digest comes from the
upstream GitHub release metadata; it is an integrity check, not independent
signature verification.

## What runs

- **client/supervisor (Rust)**: tests both service implementations, triggers its
  child, validates and handles the kernel's child-fault message, and checks the
  Rust server afterward.
- **server (Rust)**: implements the calculator protocol, validates message shape
  before reading operands, rejects unsigned overflow, and keeps its counter and
  guard in a page mapped only into this server at `0x40000000`.
- **C reference server**: preserves the original C implementation on a separate
  private page; the supervisor checks that its protocol behavior still matches.
- **vault (Rust)**: stores separate system, Notes, and Contacts records, each in
  two 4 KiB slots on the data disk. The supervisor and each app hold distinct
  vault endpoints; the vault maps each incoming endpoint to one fixed namespace,
  and request payloads cannot select another. The crasher's endpoint is denied.
  New records use AES-256-GCM-SIV authenticated encryption; the tag binds the
  namespace and generation. The vault encrypts before copying data to sDDF's
  shared transfer buffers, writes a new slot before committing it, rejects stale
  updates, and recovers the last authenticated slot per namespace. It can migrate
  the previous plaintext demo format on first read. Key acquisition uses a
  crypto-operation interface; record code does not receive key bytes. The fixed
  development provider is included only when the explicit `development-crypto`
  Cargo feature is enabled, and the QEMU build enables it with a warning. Its
  software key still resides in the vault process, so the image demonstrates an
  encrypted disk format, not protection from someone who has both the disk and
  boot image. The Pixel StrongBox test established AES-GCM support, while the
  current record format requires AES-256-GCM-SIV; connecting the two needs a
  trusted service that supports GCM-SIV or a versioned format change. The sDDF
  virtio driver and block virtualizer run in separate protection domains.
- **backup format (Rust library)**: keeps the original 232-byte PVLTBAK1 format
  readable and writes PVLTBAK2 archives containing one opaque 4 KiB encrypted
  block per namespace. Both codecs validate authentication and namespace
  binding. Offline host commands export and restore raw disk images; the guest
  does not yet have a backup screen.
- **Notes and Contacts demo apps**: run in separate protection domains and
  exercise independent persistent records. They try malformed requests with
  namespace-looking payloads; the vault rejects them and their records remain
  unchanged. These are security tests, not finished user-facing apps.
- **crasher**: receives a start notification, then executes an ARM store to
  `0x40000000` without a mapping for that address. It has a vault endpoint, but the
  vault denies its read and write calls based on the kernel-assigned channel.
  It is implemented in Rust; the store uses explicit inline assembly because the
  target address is deliberately not a valid Rust pointer. Its private attempt
  counter persists across supervisor restarts, making the PC-only restart policy
  observable.
- **clock (Rust)**: owns the emulated PL031 real-time clock mapping and interrupt. It
  arms one-second alarms, serves elapsed-time queries, signals the console, and
  publishes a formatted UTC snapshot to the read-only graphical Clock screen.
- **console (Rust)**: owns the emulated PL011 keyboard-input mapping and interrupt.
  It bounds command lines, parses a small command grammar, and reaches the
  calculator and crasher only through the supervisor.
- **Microkit monitor and initializer**: upstream system-management components.

The application components have distinct address spaces. `system.system`
grants the client protected-call endpoints to the Rust service, C reference, and
vault, and a one-way notification to the crasher. The crasher receives a separate
vault endpoint so the service can apply a deny rule by kernel-assigned channel ID.
As the crasher's parent, the client also receives its kernel faults and holds
thread-control authority over it. The services are independent of that parent/child
pair. The calculator, reference service, and vault pages are private to their
owners; only generated sDDF queue and transfer buffers are shared with the block
virtualizer. No network device or host directory is attached.
The server is passive after initialization and runs requests on the caller's
scheduling budget. All components use the debug kernel's console for this demo.
Only the console domain maps PL011 input registers; only the clock domain maps RTC
registers. The supervisor alone can restart or stop the crasher. The MCS scheduler
limits the supervisor and console to 30% CPU each, and the clock and crasher to 10%
each, with 10 ms periods; 20% remains unallocated. The passive calculator servers
run on their callers' scheduling budgets.

The first seven checks exercise the Rust server protocol; the next four compare it
with the C reference. Five check vault recovery, owner updates, and stale-write
rejection. Six validate the kernel fault, both private services' state, and continued
progress.
The parent checks child identity, VMFault type/length, address, data-abort/write
syndrome, and translation-fault status. Only then does it stop the child and make
the post-fault requests. It never resumes the faulting instruction.

The host test requires those events in order and fails on missing results, an
unexpected fault, a failure marker, premature QEMU exit, or timeout. The negative
controls build disposable copies: one grants the crasher a writable mapping, and
one faults at the wrong address. Both must be rejected for the expected reason;
build failures do not count. These copies do not modify the normal image or UTM VM.

This demonstrates one concrete memory-isolation boundary and a supervisor's
bounded restart of its child. It is not a formal proof, a sustained availability
benchmark, or general device-driver recovery. The RTC is a host-controlled
simulated wall clock, and serial keyboard input is not a graphical interface.

## Files and evidence

| File | Purpose |
| --- | --- |
| `system.system` | Protection domains, private page, hierarchy, and channels |
| `src/protocol.h` | Request and reply wire format |
| `src/server.c` | C comparison server |
| `src/reference/client.c`, `src/reference/console.c` | Retained C supervisor and serial console |
| `src/reference/crasher.c`, `src/reference/clock.c` | Retained C faulting child and RTC service |
| `rust/src/main.rs`, `rust/src/crasher.rs`, `rust/src/clock.rs`, `rust/src/client.rs`, `rust/src/console.rs`, `rust/src/vault_disk.rs`, `rust/src/vault_protocol.rs` | Rust calculator, faulting child, RTC service, supervisor, console, and disk-backed private-data service |
| `rust/src/vault_record.rs`, `rust/src/vault_backup.rs`, `rust/src/vault_crypto.rs`, `rust/src/record_crypto.rs` | Authenticated records, portable backup container, crypto-operation boundary, and AEAD helpers |
| `rust/src/bin/vault_backup_cli.rs` | Offline host export and restore utility for the project's raw GPT disk images |
| `rust/src/vault.rs` | Retained RAM-backed prototype for comparison |
| `rust/src/vault_disk_test.rs`, `tests/storage_meta.py`, `tests/merge_storage_system.py` | Standalone reboot-recovery test and generated-system merger for sDDF components |
| `tests/merge_graphics_system.py` | Generated-system merger for the temporary sDDF GPU client and drivers |
| `tests/merge_network_system.py`, `tests/net/` | Serial-free VirtIO net fragment and merger into Personal OS |
| `tests/input/driver.c`, `tests/input/input.system` | Isolated VirtIO keyboard/tablet event-queue feasibility driver |
| `docs/storage-spike.md` | Storage test evidence and durability limits |
| `docs/graphics-spike.md` | QEMU VirtIO GPU feasibility test, compatibility fixes, and next integration step |
| `docs/input-spike.md` | VirtIO input event acquisition test and remaining app-routing work |
| `docs/network-spike.md` | Standalone and integrated VirtIO Ethernet spike (DHCP + UDP echo) |
| `docs/utm-vs-hardware.md` | What UTM proves vs Pixel hardware; tablet cursor vs mouse |
| `docs/pixel-p0-boot-feasibility.md` | Pixel 4a P0 boot/debug checklist; conditional go pending UART + backup |
| `docs/pixel-key-feasibility.md` | Pixel 4a Titan M/StrongBox research and the unresolved seL4 access path |
| `platforms/sunfish/` | Desk-only seL4 overlay DTS + GENI UART putchar stubs (not flashed, not in QEMU build) |
| `docs/vault-record-v2-design.md` | Proposed AES-GCM record version, IV requirements, and mixed-version migration plan |
| `tools/pixel-keystore-probe` | Temporary Android-only StrongBox probe; builds with the local SDK and uninstalls after a run |
| `tests/negative_controls.py` | Tests that prevent false-positive isolation results |
| `dev` | SDK bootstrap, build, QEMU runner, boot test, UTM exporter |
| `build/report.txt` | Microkit's generated system/capability report |
| `build/serial.log` | Most recent automated QEMU boot-test output |
| `build/persistent-reboot.log` | Regular-system second-boot vault recovery output |
| `build/interactive.log` | Automated serial-console and repeated-fault test output |
| `build/budget-pressure.log` | Output from the CPU-budget pressure test, when run |
| `build/build-info.json` | Actual compiler/QEMU versions and image hashes |
| `build/negative-controls/` | Boot logs from intentionally broken test variants |

Generated SDK files, build output, and UTM bundles are excluded from Git.

## Next milestones

1. Open `dist/Personal OS Graphics.utm` with UTM’s USB Tablet input; confirm the
   amber guest cursor follows the pointer and Clock/Calendar/Messages/Settings work as expected.
2. Continue U6 after the integrated net merge (`./dev test-net`): Messages vault
   and sealed text (explicit-contact path).
3. Keep evaluating trusted crypto, rollback protection, and storage durability for U3.
4. Pixel P0: acquire a 1.8 V USB-C SBU UART adapter, confirm userdata backup, dump
   live slot `b` boot images; desk scaffold is under `platforms/sunfish/`.
4. Investigate Pixel 4a boot and driver feasibility independently; no device flash is part of the current UTM work.

See the [UTM-to-Pixel roadmap](docs/roadmap.md) for the complete staged plan,
relative effort, hardware decision points, and acceptance criteria. EL2 is relevant
to a Linux driver VM; it is not a universal prerequisite for a native seL4 system.
[UTM vs hardware limits](docs/utm-vs-hardware.md) records what the emulator can
and cannot prove (tablet cursor vs mouse, Ethernet vs Wi‑Fi, no cellular yet).
The [QEMU block-storage integration](docs/storage-spike.md) records the regular
image's sDDF-backed vault, reboot-recovery evidence, and the remaining limits on
durability claims.

QEMU's generic board does not model the Pixel's peripherals or power system.
The graphical apps are an experimental UTM profile, separate from the default
serial image. There is no hardware-backed key service, networking, or telephony
yet. The encrypted vault uses a development key supplied by a feature-gated
provider embedded in the image.
See [the security model](docs/security-model.md) for the experiment's boundaries.

## Upstream references

- [Microkit 2.3.1 release](https://github.com/seL4/microkit/releases/tag/2.3.1)
- [Microkit manual](https://docs.sel4.systems/projects/microkit/manual/2.3.1/)
- [seL4 Rust bindings](https://github.com/seL4/rust-sel4)
- [Rust Microkit example](https://github.com/seL4/rust-microkit-demo)
- [Official macOS setup](https://docs.sel4.systems/projects/microkit/tutorial/part0.html)
- [QEMU ARM virt](https://www.qemu.org/docs/master/system/arm/virt.html)
- [LionsOS](https://lionsos.org/)

Upstream binaries and headers retain their own licenses. The SDK is downloaded
separately and is not vendored into this repository.
