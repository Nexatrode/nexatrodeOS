# Passphrase-gated dynamic app experiment

This is an isolated UTM integration of the dynamic Slint lifecycle and the
existing `Credential`, `LockController` and `LockSettings` policy. It does not
install anything in the regular VM or migrate a wallet or chat database.
The regular optional phone lock and the ordinary two-window experiment remain
available through their existing builds.

## Authority and transitions

The trusted Microkit manager starts locked. A separate trusted authenticator
owns the password buffer, PBKDF2 verifier, retry policy, unlock UI and settings
journal connection. The dynamic app has none of those pages or endpoints. The
static peer window is removed from this fixture; no app remains running behind
the lock screen.

Only the authenticator's capability-derived caller channel can unlock. The
request must carry the current lock epoch. Manual re-lock increments that epoch
even if verification is already running; a successful but canceled verifier
cannot subsequently launch an app. The manager controls idle locking using the
kernel-exported monotonic counter and the isolated timer driver/poller. It does
not depend on the authenticator completing a password check to stop an app.

Locking closes the launch/publication gate and publishes a covering frame,
suspends the app TCB, eagerly zeroes and checks every app-owned user frame,
clears graphical caches and input ownership, unbinds its CPU context, deletes
instance endpoint leases and revokes app objects/mappings. The frame ledger
covers the image, heap, stack, surface, input, generation and IPC pages. It is
bounded by the existing process object quota. Unlocking creates a new instance;
old keys, text, mappings and instance authority are not resumed.

In the original unwrapped mode, the verifier is saved through the vault's authenticated two-slot settings
journal, with two writes and readback. Missing settings require enrollment;
invalid, disabled or unreadable settings fail closed in this experiment.
Passwords are held in a fixed 128-byte buffer and explicitly cleared after
submission or lock transitions. They never enter a Slint text field or serial
trace. Setup alone shows a character count. Locked entry gives no length
feedback, and its keyboard stays hidden until a swipe or deliberate hardware
Enter. F9 is an experimental immediate lock shortcut. The trusted desktop also
has an on-screen lock button.

The lock mode reserves dispatcher channels 60 and 61, so its generation quota
is 59 per guest boot. Exhaustion leaves launch unavailable rather than reusing
an identity. This is a bounded process-management experiment, not an installer.

## Verification on November 23, 2026

Accelerated four-vCPU UTM used a newly created 16 MiB GPT disk, public
development-key vault service, restricted networking and no host file shares
or clipboard. Every temporary VM was stopped and unregistered after the run.
The regular saved VM was not started or changed. No Pixel source was edited.

The positive run checked:

- Enrollment persists but does not launch an app; F6 cannot bypass the lock.
- An unprivileged probe and the dynamic app cannot invoke unlock.
- Swiping reveals the entry keyboard; empty, five-character and hundred-character
  locked entries produce identical guest framebuffer pixels.
- Incorrect passwords and immediate retries are rejected. A correct password
  starts a new generation with fresh text/input and zero-on-reallocation checks.
- The host first finds a random test marker in the live app's physical heap,
  then cannot find it after lock, before any subsequent app allocation.
- The manager erases and reads back 12,369,920 bytes of app-owned user frames,
  while the existing teardown audit verifies removal of capabilities/mappings.
- Idle timeout destroys the app; re-lock during correct-password verification
  invalidates the pending unlock.
- A guest restart retains the credential, starts locked, and accepts the correct
  passphrase. The on-screen lock button destroys the newly unlocked app.
- Serial output contains no passphrase, typed test marker or per-key trace.

A separately compiled `CONTROL_SKIP_SCRUB` build deliberately omits erasure.
The same host RAM detector finds the old marker after lock, demonstrating that
the positive test does not merely observe a fresh UI or zero-on-reallocation.
The control is test-only and is recorded in its build manifest.

All nine existing lock policy/storage tests passed. Sanitized host checks passed
for input ownership/reset, compositor cache teardown and the graphical ELF
policy, including 10,000 bounded mutations.

Local evidence is in `build/desktop-lock/third-utm` (tested loader, build identity
and initial positive run), `build/desktop-lock/positive-recheck` (extended positive
checks, serial and framebuffers), and `build/desktop-lock/skip-scrub-utm`
(negative control). These ignored output directories contain disposable data.
They are not recovery material for the regular VM.

```sh
python3 tests/desktop_lock_utm.py \
  --inputs /path/to/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --base build/desktop-graphical/final-utm \
  --storage build/utm-vault-key-storage \
  --composer build/desktop-graphical/endpoint-composer/target/release/microkit \
  --output build/desktop-lock/new-run
```

Use fresh output directories. Prepared block configs/drivers and graphical
infrastructure are read only. Vault and entropy services are built from source;
the script rejects an installation-key vault section. `--control-skip-scrub`
selects the deliberate failure build. Test images shorten the default 60-second
idle timeout to 10 seconds. Framebuffer captures/OCR use settling waits and are
not input-latency measurements.

## Security scope

The wrapped-key integration below connects the random disposable vault key to
this lifecycle. It does not yet connect real SimpleX/NexaWal workers, checkpoint
their writes, or migrate/evict their installation vault key. The original
unwrapped settings mode uses a public development key. Software retry delays can reset with a reboot and are not
rollback resistant. Credential verification uses the existing 600,000-round
PBKDF2-HMAC-SHA256 policy with random salt and constant-time comparison.

Erasing app user frames and CPU compositor caches does not establish erasure of
authentication-service stack/register temporaries, kernel objects, all guest
RAM, GPU/display copies, the host, snapshots or swap. The trusted authenticator,
manager, renderer and storage service still need security review. This uses the
debug kernel, whose user-invokable debug halt prevents a hardened hostile-app
deployment claim. Verified boot, Titan M, hardware-bound keys, storage rollback
protection and physical Pixel testing are outside this result.

## Wrapped-key integration

`--wrapped` enables `desktop-wrapped-lock-ui` and the vault's separately gated
`utm-dynamic-vault-lock` feature. It enrolls a fresh random 256-bit vault key,
wraps it under the test passphrase with the existing versioned envelope, and
persists that envelope and verifier using the bootstrap journal. The bootstrap's
outer framing still uses a public development key; encrypted app records use
the random passphrase-wrapped key. No installation-key ELF or saved user disk is
read, and first enrollment still refuses a nonblank partition.

The manager alone holds the vault lifecycle endpoint. Unlock proceeds through
four phases: evicted, armed, prepared and active. Password verification and key
derivation happen in the trusted authenticator before a five-second preparation
lease begins. The authenticator can prepare the verified key for the armed
epoch but cannot commit it. App service reads and writes remain denied in the
prepared phase. Only a manager commit for that epoch activates the vault, after
which the manager launches the new app instance.

On manual or idle lock, the manager closes its launch gate, increments the lock
epoch, suspends the app producer, and synchronously orders vault eviction before
the longer framebuffer and app-frame cleanup. The provider zeroizes its key,
transfer pages, I/O scratch window, transaction/photo staging and cached record
metadata. Its acknowledgement includes the epoch, phase and an inspection of
whether the provider's actual key array is zero. Failure permanently blocks
further unlock/launch in that guest and shows a restart-required error. It is
not reported as a completed lock. An abandoned preparation is evicted by the
manager's independent timer; a stale verifier or commit cannot reopen it.

The accelerated four-vCPU UTM run on November 23, 2026 passed enrollment,
incorrect/correct passwords, retry delay, manual and idle key eviction, app RAM
erasure, guest restart and recovery of the same encrypted disposable note.
A static adversarial probe with only a Notes namespace endpoint was denied
reads/writes while locked or prepared and could not invoke vault commit. The
note was absent as plaintext from the disposable disk. Re-lock during a
correct verification prevented launch. F8 in this fixture deliberately abandons
the next successful preparation; the test confirmed access stayed denied and
the five-second lease erased the pending key.

A separate `--wrapped --control-skip-evict` build deliberately leaves the
provider key array intact while disabling app access. The manager detected the
nonzero key acknowledgement, destroyed the app, displayed the error and denied
another launch. This is a deliberate failure fixture, not an available lock
policy. Both wrapped modes refuse compilation with an installation-key build.
The three lifecycle policy tests, four envelope crypto tests, bootstrap-record
test and nine existing lock policy/storage tests passed. The unwrapped fixture
and the earlier wrapped-vault server without the dynamic-lifecycle feature also
rebuilt after the shared changes.

Merge verification exposed a diagnostic-probe race: unlock could commit between
its denied read and attempted write. The denial probe now uses a never-valid
expected generation, so it cannot overwrite a note if the vault unlocks between
the two RPCs. A bounded retry handles the phase change. A sanitized host fixture
covers stable lock, unlock between RPCs, re-lock between reads and detection of
an erroneously accepted write.

```sh
python3 tests/desktop_lock_utm.py \
  --inputs /path/to/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive-final \
  --base build/desktop-graphical/final-utm \
  --storage build/utm-vault-key-storage \
  --composer build/desktop-graphical/endpoint-composer/target/release/microkit \
  --output build/desktop-lock/new-wrapped-run --wrapped
```

Evidence, source/loader hashes, serial traces and framebuffer captures are in
`build/desktop-lock/wrapped-utm` and `build/desktop-lock/wrapped-skip-evict`.
The merged-main run with the corrected diagnostic probe is recorded in
`build/desktop-lock/main-verified-utm`.
The ordinary mode remains available without `--wrapped`. This does not make
the debugger-enabled kernel, host memory/snapshots, authentication stack
temporaries or GPU copies secure, and software retry delays still do not stop
offline envelope guessing or provide rollback-resistant counters.
