# PVLTREC2 record format and migration design

Status: design proposal. PVLTREC1 remains the only writer. The QEMU image now has
a VirtIO RNG service for transport and reset testing, but its samples are not used
by the vault; a production entropy source and device crypto trust path remain
open.

## Decision

Keep PVLTREC1 readable and add PVLTREC2 for AES-256-GCM with a provider-generated
96-bit IV and a full 128-bit authentication tag. Keep the 72-byte slot, its two
slot recovery scheme, and the outer PVLTBAK1 archive layout. The version marker
and AEAD associated data distinguish the two record suites; a slot is never
tried under a different suite after authentication fails.

The current AES-256-GCM-SIV records can use a deterministic IV derived from
namespace and generation. Reusing that construction for AES-GCM would be unsafe:
the on-disk generation can roll back with an older disk image or repeat on a
forked copy. GCM requires IV uniqueness for each key; NIST recommends 96-bit IVs
and permits deterministic or random-bit-generator constructions with specified
limits. This design chooses a provider-generated IV rather than deriving one
from rollback-able disk state. The provider must create the IV and perform the
encryption as one operation, then return the IV with the tag. [NIST SP 800-38D](https://nvlpubs.nist.gov/nistpubs/legacy/sp/nistspecialpublication800-38d.pdf)

The Pixel StrongBox path is a reasonable algorithm match for this version:
Android's Keystore API documents AES/GCM and creates a random IV when a Cipher is
initialized for encryption without a caller-supplied IV. The IV is then read
back from the Cipher and stored with the ciphertext. This fits a provider-owned
IV contract, though it does not provide seL4 with a direct StrongBox connection.
[Android `KeyGenParameterSpec`](https://developer.android.com/reference/android/security/keystore/KeyGenParameterSpec.Builder#setRandomizedEncryptionRequired(boolean))
and [Android cryptography guidance](https://developer.android.com/privacy-and-security/cryptography)
describe that behavior.

NIST is revising SP 800-38D; the latest page currently describes a second
pre-draft call for comments and does not publish a replacement specification.
Recheck NIST's revision status and IV guidance before implementing this proposal.
[NIST revision status](https://csrc.nist.gov/pubs/sp/800/38/d/r1/2prd).

## Slot layout

PVLTREC2 keeps the existing 72-byte slot and field offsets:

| Bytes | Size | PVLTREC2 field |
| --- | ---: | --- |
| 0–7 | 8 | Magic `PVLTREC2` |
| 8–11 | 4 | Version `2`, little-endian |
| 12–19 | 8 | Namespace generation, little-endian |
| 20–31 | 12 | Provider-generated GCM IV |
| 32–39 | 8 | Ciphertext |
| 40–55 | 16 | Full GCM authentication tag |
| 56–63 | 8 | Commit marker: `generation XOR COMMIT_SALT` |
| 64–71 | 8 | Reserved; must be zero |

PVLTREC2 uses distinct associated data beginning with `PVLTAD02`, followed by
version 2, the namespace ID, and generation in the same canonical widths used by
v1. The parser requires the magic, version, AAD domain, and suite to agree. An
unknown `PVLTREC*` magic/version is rejected as unsupported/corrupt and is never
reinterpreted as a legacy plaintext record. Existing v1 bytes, nonce derivation,
AAD, tag, and commit marker remain byte-for-byte unchanged.

## Provider contract

The provider boundary should become suite-aware while keeping key bytes opaque:

- v1 sealing accepts its existing deterministic IV and performs AES-256-GCM-SIV
  exactly as it does today.
- v2 sealing asks the provider to generate a fresh 96-bit IV, perform
  AES-256-GCM, and return `{ iv, tag }`. Callers cannot supply or override the
  v2 IV.
- Opening receives the suite, stored IV, AAD, ciphertext, and tag. It returns
  authentication failure separately from provider unavailability.
- A v2 provider enforces its per-key invocation limits and key-rotation policy.
  If it cannot produce an IV or complete the operation, the vault returns a
  storage error before writing any staged bytes.

For Android Keystore, generate a StrongBox AES-256 key restricted to GCM,
NoPadding, encrypt/decrypt, and a 128-bit tag; keep randomized encryption
required. Initialize encryption without an IV, read the generated IV, apply the
record AAD, and split the returned ciphertext and 16-byte tag into the slot.
Decryption uses the stored IV and tag. The temporary Pixel probe now covers
96-bit IV length, 128-bit tags, AAD binding, and the StrongBox security level
together. Its two-encryption fresh-IV check is a compatibility sample, not
proof of lifetime uniqueness. The result is recorded in the
[Pixel feasibility notes](pixel-key-feasibility.md).

The QEMU image now includes a small passive VirtIO RNG service. It exposes a
single protected request to the supervisor, checks the device-reported buffer
length, and keeps the vault away from the MMIO device and DMA memory. Tests
request four distinct samples on normal boots and after the storage suite's four
QMP reset points. The suite also restores a full VM snapshot captured before
guest execution and checks that a short fingerprint changes. It also boots two
independent QEMU instances from copies of the same saved snapshot and compares
their fingerprints. Both checks passed on QEMU 11.1.1. QEMU documents built-in
and host-device RNG backends; these are useful for emulator transport tests but
are not physical entropy or a hardware trust anchor. [QEMU RNG options](https://www.qemu.org/docs/master/system/qemu-manpage.html#object-rng-builtin-id-id)
These limited experiments do not establish entropy quality or uniqueness across
arbitrary snapshots and clones. Keep v2 writes disabled until source properties
and rollback behavior have been addressed.
Do not substitute wall-clock time, namespace/generation, disk-image identifiers,
or a one-time seed plus a rollback-able counter. If the source is unavailable,
v2 writes must fail without changing either slot.

## Read, write, and migration behavior

The decoder recognizes three record classes: legacy plaintext, encrypted v1
(AES-256-GCM-SIV), and encrypted v2 (AES-256-GCM). It authenticates each slot
using the suite selected by its exact magic/version pair, then selects the
highest committed generation as it does today. Unsupported versions and failed
tags never become valid records.

New writes use v2 only after the selected provider advertises v2 support and has
a ready IV source. To migrate a valid v1 active record, the vault writes its
value as v2 into the inactive slot at generation `g + 1`, flushes the staged
slot, writes the commit marker, and flushes again. Legacy plaintext continues to
use the existing migration path, with its current warning that its old checksum
does not authenticate its value.

If a reset happens before v2 commit, the old v1 record remains the newest valid
record and migration is retried at next boot. If the commit is durable, the v2
record wins by generation. A crypto or entropy error leaves the old record
intact and prevents the vault from marking that namespace set loaded. A
subsequent normal write overwrites the older v1 slot through the existing
two-slot rotation. Keep v1 decoding available for old images and backups; retire
it only under a separate compatibility policy.

PVLTBAK1 remains a 232-byte container of opaque, individually versioned small
records. The app-data work adds PVLTBAK2, which stores one opaque 4 KiB block per
namespace and accepts either an encrypted small record or a PVLTBLB1 blob
record. Both formats are independent of the record cipher-suite version.
Existing PVLTBAK1 archives restore normally; newer exports use PVLTBAK2 so
app-sized records fit without changing their ciphertext.

## Implementation and acceptance steps

1. Add exact v1 and v2 parser dispatch plus distinct suite values; keep golden
   v1 fixtures to prevent an accidental compatibility change.
2. Accept the QEMU entropy source for emulator testing only after quality and
   replay assumptions are documented. Then add a software GCM v2 provider and
   test an AES-GCM known-answer vector, 96-bit provider-generated IVs, full tags,
   and the rule that unavailable entropy causes no disk mutation.
3. Add mixed v1/v2 slot recovery and two-phase migration tests, including resets
   before stage flush, after stage flush, before commit flush, and after commit
   flush. Avoid byte-for-byte equality between independently sealed v2 records;
   compare authenticated values and generations instead.
4. Extend backup tests to preserve and validate both suites, and restore a v1
   archive into a disk that migrates to v2 on its next boot.
5. The Pixel probe now checks generated IVs, AAD, and tags. Treat successful
   Android Keystore use as Android-mediated evidence only; a trusted seL4
   service and boot/ownership story remain separate requirements.

Do not change the on-disk writer to v2 until steps 1–3 pass in QEMU and a real
provider can meet the IV contract. Hardware integration remains conditional on
a documented secure-service path; the Android API alone does not establish one.
