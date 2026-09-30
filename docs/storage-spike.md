# QEMU block-storage integration

The regular `./dev build` and `./dev run` system now use an sDDF-backed Rust vault.
The build generates the virtio block driver and block virtualizer as separate
protection domains, gives the vault its client queue and private state, and
attaches a 16 MiB GPT data disk to QEMU. It stores system, Notes, Contacts, and
Calendar records in separate two-slot namespaces. Calendar reuses the former
prototype gap at blocks 2–3; Notes and Contacts remain at 4–5 and 6–7. Host
backup archives still export namespaces 0–2 only.
`build/personal-os-disk.img` is created once and preserved by later builds.
Automated main-system tests use a separate resettable disk so their sample
writes do not alter the user's CLI data.

`./dev test-storage` builds a small, separate recovery system on a fresh 16 MiB
GPT test disk. Across five fresh QEMU boots, it commits generation 1 and then
stages four invalid forms of generation 2 in the inactive slot: an encrypted body
without its commit marker, a partially written body, a committed record with a
bad authentication tag, and a record with a torn commit marker. Each following boot must
recover generation 1 before the test advances to the next case.

The same command now builds four isolated reset cases. QMP resets the guest after
the staged body write response but before its flush, after the stage flush, after
the committed-slot write response but before its flush, and after the commit flush.
The first two cases must recover generation 1; the pre-commit-flush case may
recover either valid generation because the QEMU backend can persist the completed
write before the flush; the post-flush case must recover generation 2. Each case
starts from its own clean disk image and verifies the recovered record through the
vault API after the reset.

This test ran on 27 September 2026 with Microkit 2.3.1 and sDDF commit
[`fe758631a9e99d97669f937bb6704a2f34a4a578`](https://github.com/au-ts/sddf/tree/fe758631a9e99d97669f937bb6704a2f34a4a578).
The test-only interruption request is enabled only for the separate recovery-test
build. The regular disk vault does not accept that diagnostic request.

The storage test also converts a pristine disposable raw disk into qcow2, pauses
QEMU before guest execution, and saves a whole-VM snapshot. It boots once,
restores that pre-boot snapshot, and checks that the next 64-bit test fingerprint
differs. It then copies the saved qcow2 state into two independent disks, starts
two QEMU instances from the same snapshot, and compares their fingerprints. On
the recorded QEMU 11.1.1 run, both the restore pair and clone pair differed.
This establishes different output on this QEMU backend for these restore and
clone operations; it is not an entropy-quality test and says nothing about
Pixel hardware or arbitrary cloned VM state.

Vault data covers ten 4 KiB blocks: system 0–1, Calendar 2–3, Notes 4–5, Contacts 6–7, Messages 8–9. Host backup archives still export namespaces 0–2 only. RNG DMA is pinned at `0x5fdc0000`.

These tests exercise persisted disk states through QEMU's virtual disk path,
system reset, and one whole-VM snapshot restore. QEMU and its host-side disk
backend remain running across each reset, so the test does not simulate a host
crash, dropped host write cache, physical power loss, or storage-controller
failure.
`./dev test` also boots the full regular system twice on its resettable disk and
checks the system, Notes, and Contacts records recover; the two app records
advance from generation 1 to generation 2 across those boots. At that stage it
ran 24 ordered supervisor checks plus eight app-namespace assertions. The
refreshed UTM bundle attaches its own virtio-mmio disk. A warm reset reproduced a stale virtqueue-state failure in
both UTM and standalone QEMU. The guest RAM survived while the emulated virtio
device reset, so the sDDF driver consumed a used-ring entry from the previous boot
and faulted.
`patches/sddf-personal-os.patch` clears the virtqueue, resets the block
request/response counters during driver initialization. Three consecutive boots
in one standalone QEMU process, including QMP `system_reset`, then recovered the
committed record and passed all checks each time. The earlier namespace-enabled
UTM bundle booted with all 24 supervisor checks and eight namespace assertions passing, then
survived two warm restarts. Notes and Contacts each advanced from generation 1 to
3; read-only inspection confirmed both slots per app were checksum- and
commit-valid. These results verify emulator reset recovery, not host power-loss
durability. The disk records now contain authenticated ciphertext, and the vault
decrypts only after checking the namespace, generation, commit marker, and AEAD tag.
The previous plaintext record format is recognized and migrated on first access;
its legacy checksum is not treated as cryptographic authentication.

The AES-256-GCM-SIV implementation is the no-std RustCrypto crate, with the
algorithm specified by [RFC 8452](https://www.rfc-editor.org/rfc/rfc8452.html).
The upstream crate documentation says the crate itself has not had a security
audit, so this implementation remains a prototype that needs independent review.
The nonce derives from namespace and generation because this prototype has no
trusted entropy source or rollback-resistant counter. GCM-SIV is designed to
degrade gracefully under nonce repetition, but the RFC recommends random nonces;
rollback and equality leakage still need a deliberate production policy. The
feature-gated development provider is compiled into the vault ELF and loads the
key into vault memory, so possession of the boot image reveals it. The provider
interface separates acquisition from record encoding, but this is still an
encrypted-format prototype, not device-backed key protection.

At the pinned sDDF revision, the upstream virtio block driver acknowledges
`BLK_REQ_FLUSH` without issuing a device request
([source](https://github.com/au-ts/sddf/blob/fe758631a9e99d97669f937bb6704a2f34a4a578/drivers/blk/virtio/block.c#L233-L244)).
The repository-owned driver patch now negotiates `VIRTIO_BLK_F_FLUSH`, sends a
`VIRTIO_BLK_T_FLUSH` request when the device offers it, and reports an I/O error
when flush is unavailable. The vault waits for flush completion after both the
staged slot write and the commit-marker write. Full QEMU boots exercise that path
because every runtime vault write depends on it. These checks validate the guest
protocol and QEMU backend, but do not simulate a physical power cut on the host or
establish durability on Pixel storage.

`rust/src/vault_backup.rs` keeps the original PVLTBAK1 import format (a 16-byte
header plus three 72-byte records) and adds PVLTBAK2 (a 16-byte header plus one
full 4 KiB active block per namespace). Both codecs validate record
authentication and namespace binding; legacy plaintext is rejected in an
archive. The gated host utility `vault_backup_cli` reads the latest committed
record from each namespace in a raw GPT disk image. It converts an old plaintext
slot to an encrypted record at the next generation without changing the disk;
the old checksum is not cryptographic authentication, so that conversion keeps
the legacy trust limit. Restore accepts both archive versions and creates a new
disk-image copy with one authenticated slot per namespace; neither command
overwrites its source or an existing destination.

`./dev test` exercises export, restore to a separate disk image, and QEMU boots
that recover both encrypted records and values converted from the legacy format.
The tool operates offline and requires the VM to be stopped. It uses the prototype
key from the source code, so the archive is not confidential from someone with
the source or boot image. Restore refuses an authenticated record older than the
selected source disk unless the operator passes `--allow-rollback`. This is an
offline guard against accidental rollback; an attacker can substitute an older
source image, and there is no trusted rollback-resistant counter. The vault
retains channel-based authorization and private record state;
callers do not receive the raw disk queue or a writable mapping of that state.

## Evidence boundary

The regular `./dev test` suite now exercises the disk-backed vault and remains the
security demonstration for this repository. It runs on a fresh test disk, checks
that an untrusted child cannot read or change the vault, and confirms committed
data survives a full-system reboot. The standalone `./dev test-storage` suite
covers four malformed/incomplete record states across successive fresh QEMU boots.
The UTM exporter uses the same system image with a separate persistent disk copy.
