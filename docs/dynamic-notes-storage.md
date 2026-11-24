# Dynamic Notes private encrypted storage

The disposable dynamic Notes app now saves and reloads its own document through
an instance-scoped broker. Its existing Slint Save button drives the actual
app-to-manager-to-vault path. The wrapped-key lock fixture supplies the random
device vault key. Saved wallet/chat data and the regular VM are not imported or
migrated; Pixel Wi-Fi development remains separate.

## Storage authority

The embedded Notes program has one logical storage identity: the vault's Notes
namespace. Its process receives only its existing send-only manager endpoint
and a new private 4 KiB exchange frame. It receives no raw vault endpoint, vault
key, namespace selector, path or memory-management authority.

The manager's dedicated Notes endpoint is bound to that namespace by the system
description. Every load/save checks the kernel-derived caller instance, the
supplied instance tag, active/unlocked state, message arity and extra-cap count.
The exchange frame is allocated from the app's private untyped pool, mapped
only into that app and the manager, and included in eager frame erasure and
revocation. The manager's separate vault transfer page is shared only with the
vault. A different diagnostic app uses the Contacts namespace and a different
page; its manager endpoint cannot invoke Notes storage.

The broker supports a single document up to 2,048 UTF-8 bytes. A versioned
`NXNOTE01` record contains a bounded length, zero reserved fields and text.
Parsing rejects inconsistent versions, lengths and sizes. The client validates
UTF-8 on load. Journal generations provide optimistic save revisions: a stale
revision cannot overwrite a newer record. Process instance tags expire on
termination, while the logical Notes identity persists across fresh instances.

App records use the existing authenticated encrypted blob journal, including
namespace/generation/length binding and two-slot staged/committed writes. The
random encryption key is passphrase-wrapped in the bootstrap journal. The
bootstrap's outer framing uses the existing public development key; that
framing is not the secrecy mechanism for the Notes record.

The broker zeroes its static plaintext scratch and vault transfer page on each
completed/error path. The client clears its exchange after normal load/save.
Manual and idle lock suspend/destroy the app, erase its private frames, clear
cached pixels/input and broker state, revoke instance authority and evict the
vault key. Unlock creates a new instance that loads only the last committed
note. Unsaved working text is discarded with the old instance.

Saves use bounded synchronous RPCs. A lock request serializes after a save
already executing in the manager/vault; the Save acknowledgement follows both
flushes. The deliberate interruption hook stalls the disposable guest until a
host-issued reset, allowing recovery to be checked without falsely returning a
Save acknowledgement.

## Verification on November 23, 2026

An accelerated four-vCPU UTM run enrolled the public test passphrase on a fresh
16 MiB GPT disk. The host interacted with the actual app, captured its guest
framebuffer and inspected its private exchange frame for exact text rather than
relying on OCR alone. Only this disposable frame was inspected; no regular VM
disk or saved wallet/chat state was opened.

The run passed:

- Empty private document on first enrollment/unlock.
- A multiline note saved using the actual Slint Save button, then exactly read
  back. The note was absent as plaintext from the disk image.
- Manual lock, full exchange-frame erasure before reallocation, key eviction and
  recovery of the same note in a fresh process instance.
- Other-app read/write denial, independent Contacts storage, stale instance
  tags, forged namespace arguments, malformed pages and stale document revisions.
- Idle lock with unsaved text: the old private frame was erased and the next
  instance recovered the last acknowledged save.
- Locked startup and exact saved-note recovery after an ordinary guest reset.
- Continued saving and another locked restart after each interrupted-save case.

A one-shot manager-only test control paused the real Notes write path at four
boundaries, before the reply to the saving app. The runner reset the guest,
unlocked with the test passphrase and checked exact restored text:

| Reset point | Required recovery | Observed |
| --- | --- | --- |
| Before stage flush | Previous committed note | Old note |
| After stage flush | Previous committed note | Old note |
| Before commit flush | Complete old or new committed note | New note |
| After commit flush, before acknowledgement | New committed note | New note |

No interrupted operation reported success before the reset. After each case,
the new instance saved a different note and recovered it after another reset.
These are UTM/QEMU guest resets, not physical power-loss tests.

Sanitized host tests cover the actual broker's access checks, invalid/closed
instances, namespace-argument rejection, revision conflicts, maximum-size notes,
corrupt records and scratch erasure. The version/length parser also received
10,000 bounded mutations. Existing vault blob tests cover encrypted empty,
multiline and maximum-size records and authentication of namespace, generation,
length and ciphertext.

## Reproduction

```sh
python3 tests/desktop_lock_utm.py \
  --inputs /path/to/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --base build/desktop-graphical/final-utm \
  --storage build/utm-vault-key-storage \
  --composer build/desktop-graphical/endpoint-composer/target/release/microkit \
  --output build/desktop-notes/new-run --wrapped --private-notes

clang -std=c11 -O1 -g -fsanitize=address,undefined -Wall -Wextra -Werror \
  -Itests/desktop_lock_stub tests/desktop_notes_broker_test.c \
  -o build/desktop-notes-broker-test
build/desktop-notes-broker-test
```

Use a fresh output directory. The builder writes source/artifact hashes,
rejects installation-key vault images, and only reads prepared driver/config
inputs. The runner disables host shares/clipboard, restricts networking and
removes its disposable VM/disk afterward. This test uses a 30-second idle lock.
F2 saves and F3 exposes a private-frame snapshot for this fixture's host check;
neither prints note contents. F10/F11/F12/F4 arm the four reset points only in
the separately gated `utm-notes-interruption-test` vault build. Regular builds
have no interruption hook enabled.

Local evidence is in `build/desktop-notes/focused-utm`; the tested loader came
from `build/desktop-notes/first-utm`. The initial test-input focus issue was
corrected before the passing run. Build caches and disposable test artifacts
remain ignored by Git.

## Scope

This proves one curated Notes identity and one bounded document, using the
existing debug-kernel laboratory configuration. A general installed-app
identity registry, larger files, document sharing and asynchronous storage
workers remain future interfaces. Real SimpleX and NexaWal workers and their
installation-key migration remain separate.

The existing [wrapped-key limitations](vault-key-prototype.md) apply: no Titan M,
verified boot, trusted retry counters or rollback protection, and offline
passphrase guessing remains possible. Erasing app frames and selected broker
buffers does not prove erasure of every cryptographic temporary, register,
GPU/host copy or all RAM. The debugging syscalls also prevent a hardened
hostile-app deployment claim.
