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
used. The default offline fixture has no ethernet stack or external SMP/XFTP
connection; its environment endpoint denies network requests. It tests local
lock recovery. The opt-in live relay variant below separately tests socket
teardown and delivery through a real relay, without using an iPhone or a saved
conversation.

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

## Live relay test

Add `--simplex-live` to the wrapped SimpleX fixture to enable the real UTM
Ethernet/lwIP stack and a native upstream SimpleX Chat test peer. This uses the
same real GHC agent, Messages service and receive/transcript workers, with a
separate native driver. It does **not** compile the offline decoded-event
injection feature. The clock-only environment supplies build-time UTC plus
monotonic elapsed seconds for this disposable test.

Only the fresh-disk runner's explicit `--desktop-simplex-live-test` mode enables
unrestricted QEMU user networking. Other probe modes retain restricted
networking. The peer creates a new private temporary database, uses the
loopback-only upstream WebSocket API, and connects through the pinned
`smp4.simplex.im:5223` public relay. `SIMPLEX_LIVE_CLI` may select an alternative
native upstream CLI binary. Neither endpoint uses the user's saved chats.

The lock manager has a private network-control channel. After stopping all
SimpleX producers, it aborts their two TCP slots, clears their receive and DNS
state and shared protocol page, and closes their network request gate. Late DNS
callbacks carry a generation and cannot complete a lookup from a later unlock.
After the wrapped vault key is committed, the manager restores the public boot
template, reopens the empty network slots and restarts the services. The DHCP
stack and unrelated drivers remain alive. This is opt-in fixture wiring, not a
rollout to the regular VM or Pixel.

The live test checks actual, independently observed messages:

1. A fresh peer joins the guest's invitation. `host-before-lock` reaches the
   guest through the real receive worker and encrypted transcript, and
   `guest-before-lock` reaches the peer.
2. Manual lock closes the guest connections and erases the managed group. A
   wrong passphrase starts nothing. The peer sends `host-while-locked`; the
   guest's result/queue page remains zero while locked.
3. Unlocking the same running VM reopens AgentStore and restores subscriptions.
   The queued message arrives, the transcript contains both incoming messages,
   and `guest-after-unlock` reaches the peer. No UTM restart is used.
4. A final lock repeats teardown/erasure. The known message plaintext is absent
   from the fresh encrypted disk (a smoke check, not a cryptographic proof).

The runner deletes its VM and scratch disk and stops the temporary peer in its
cleanup path. It also attempts to delete the peer's disposable contact. Private
local evidence includes the peer's independent structured receive events;
invitation URLs, peer databases and disk images must not be added to Git.

The TCP receive tests include a new ASan/UBSan case for socket abort, receive
buffer erasure, epoch rejection, duplicate unlock denial and late DNS completion.
The existing parent lifecycle test still covers eight restart/scrub cycles and
failure latching. Final live-run evidence is under
`build/desktop-simplex/live-complete`: `build.json`, `serial.log`,
`simplex-live-result.json` and `peer-events.json`.

On 2026-11-23, two fresh live UTM runs passed all four checks. Each run
completed without a VM restart between the two messaging rounds. The final
source-built loader's 58 recorded source files and 28 artifacts matched their
manifest hashes. Three service-group erasures each verified 185,303,040 bytes;
the independent native peer recorded both guest replies. All disposable UTM
registrations were removed, and the regular VM was left stopped. The detailed
cross-check is `build/desktop-simplex/live-complete/verification.json`.

## Limits

This retains the existing development-kernel, software-passphrase and wrapped-key
limits. It does not add verified boot, Titan M binding, rollback-resistant retry
counters or protection from a hostile host. It does not prove erasure of FPU/SIMD
registers, kernel/reply-object internals, other trusted services' stacks, GPU
memory, physical RAM remnants or host snapshots. A lock discards unsaved drafts
and uncommitted/in-flight work; committed data is recovered from encrypted disk.
The live test covers text-message reconnection and delivery across this lock
boundary. It does not establish at-most-once delivery during an interrupted
send, live XFTP/photo-transfer recovery, or the regular full frontend rollout.
