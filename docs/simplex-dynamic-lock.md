# SimpleX service lifecycle under the dynamic lock

This is an opt-in, disposable UTM integration of the existing dynamic-app lock.
It does not change the regular accelerated VM or migrate an existing wallet or
chat disk. Pixel drivers and Wi-Fi are outside this experiment.

The trusted lock manager now supervises a bounded group containing the real
GHC SimpleX agent, Rust Messages service, receive/transcript worker, photo reader,
photo decoder and a native test client. The existing dynamic Slint Notes panel
and trusted passphrase screen remain in the graphical fixture. The regular full
SimpleX frontend is not installed under this boundary by this test.

## Lock and unlock

The private composer starts all six service children suspended. Their parent
gets non-executable aliases of their writable ELF pages, stacks and IPC buffers,
plus receive-only notification caps. At boot, before any child can run, it saves
the nonzero public initialization pages in a bounded 16 MiB boot template. That
template contains patched addresses and initial program data, never a running
heap, conversation, session key or decrypted AgentStore.

On lock the manager closes the gate, suspends every producer, cancels blocked
IPC through seL4's suspend operation, and evicts the wrapped vault key. It covers
the application surface before the larger memory cleanup. It then overwrites
and verifies every writable image/stack/IPC byte, the decoder and reader heaps,
the client heap, transfer pages, receive/photo/transcript/navigation/snapshot
mailboxes, agent IPC and offline TCP transfer pages. Cache maintenance operates
within individual mapped page boundaries. It clears integer thread contexts and
TLS bases and drains latched notifications. Lock completion follows cleanup.

After successful passphrase verification and the manager's epoch-matched vault
commit, the manager restores only the original public template, supplies fresh
entry/stack contexts, and starts each service at its entry point. GHC reopens the
encrypted AgentStore; Messages reconstructs its history index. All pending
client state and queue payloads start empty. The existing agent startup gate
still rechecks vault readiness before entering GHC. The fixture permits that
agent endpoint only a boolean ready/denied status; it cannot arm, commit or evict
the key.

An erasure check failure or a service fault latches failure and prevents further
unlock until restart. The service child IDs are reserved separately from the
dynamic GUI instance badges.

These are curated, statically provisioned service CSpaces. Their fixed service
capabilities remain assigned to the same trusted roles across restart. This is
not general dynamic revocation/reinstallation of an untrusted application.

## Disposable test

`tests/desktop_lock_utm.py --wrapped --simplex-session GATED_AGENT` selects this
fixture. It requires the private composer built with
`experiments/desktop-integrated/build_composer.py --pool-bits 26 --simplex-session`.
Use the same prepared input, surface, storage and base paths as the existing
[dynamic-app lock fixture](dynamic-app-lock.md). The `--build-only` flag produces
the loader and source/artifact hashes; omitting it invokes the isolated UTM
runner with `--desktop-simplex-test`.

The native test driver waits for the real GHC connection-list reply. A
compile-time-only Messages seam supplies a generated decoded receive event and
an 857-byte generated JPEG. The actual receive worker persists the event through
the normal encrypted history path, the actual transcript worker retrieves it,
and the actual photo reader and decoder retrieve and decode the encrypted JPEG.
It leaves photo, transcript and receive requests queued at lock so the next
session must start without replaying them. Each unlock recovers the same first
photo; history grows to three entries and remains at three after the reboot.
The host checks a separate fixture status page directly through disposable
physical-memory reads, avoiding ambiguity from interleaved debug prints. That
page must be completely zero after lock.

The test checks enrollment, manual lock, wrong-password denial, repeated unlock,
auto-lock, locked reboot, reopened AgentStore/transcript, fresh queues and photo
decoding after each restart. Generated plaintext is checked absent from the
disposable disk. The native parent lifecycle test also runs under ASan/UBSan:

```sh
clang -std=c11 -O1 -g -Wall -Wextra -Werror -fsanitize=address,undefined \
  -I tests/desktop_simplex_stub tests/desktop_simplex_session_test.c \
  -o build/desktop-simplex/session-test
build/desktop-simplex/session-test
```

The runner reads only a sanitized configuration template, creates a new GPT disk
and a new UTM UUID, disables sharing, and deletes its own VM/disk afterward. No
existing wallet, chat database, regular VM drive or installation vault key is
used. The fixture has no ethernet stack or external SMP/XFTP connection; its
environment endpoint denies network requests. This tests local lock recovery,
not iPhone delivery or reconnection to a live relay. A regular frontend/network
rollout needs a separate disposable online test, including socket teardown.

## Verification on 2026-11-23

The four-start disposable UTM run passed all six lifecycle checks. Five complete
service-group erasures each verified 185,298,944 bytes. Unlocks recovered one,
two and three transcript entries; after reboot all three entries and the same
first photo were recovered. Wrong-passphrase entry started no service. The final
captured frame showed the locked cover with no application content.

Local evidence is in `build/desktop-simplex/complete/simplex-result.json` and
`verification.json`. The loader and every service ELF in
`build/desktop-simplex/source-verification` are byte-identical to those exercised
by the UTM run. Current source hashes match that rebuild's manifest. The parent
lifecycle, lock-probe race regression and private Notes broker tests passed with
ASan/UBSan. The ordinary wrapped-only fixture rebuilt as well; its legacy
log-based UTM callback did not start because its setup marker interleaved with
another PD's output. The new SimpleX run uses actual screen/state checks for
startup and worker progress.

## Limits

This retains the existing development-kernel, software-passphrase and wrapped-key
limits. It does not add verified boot, Titan M binding, rollback-resistant retry
counters or protection from a hostile host. It does not prove erasure of FPU/SIMD
registers, kernel/reply-object internals, other trusted services' stacks, GPU
memory, physical RAM remnants or host snapshots. A lock discards unsaved drafts
and uncommitted/in-flight work; committed data is recovered from encrypted disk.
No at-most-once remote delivery claim follows from this offline test.
