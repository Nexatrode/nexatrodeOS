# Bounded third ELF reply

The next experiment removes a specific local admission wait, without releasing either older reply. IMG_9719 showed a clean one-pass ELF publication 93.500 ms after queueing, while 3.226277 seconds elapsed before queueing. Its first admitted snapshot still had one older reply outstanding; the two-reply cap had previously prevented admission while both transaction 3 and transaction 4 remained owned. This is a different intervention from repeating the writer trace.

## Source comparison and MCFG elimination

The delivered `native-elf-writer-publication-v1` source is pinned to b3fbfad04f66cc1af4c9dce054f346983d0890ff. Its first post-ELF MCFG observer binds the exact accepted `/readwrite/mcfg.tmp` RRQ and its builtin server dispatch. IMG_9719's successful server result zero plus prepared opcode 5 identifies the builtin constant ERROR 1, `file not found`, **by source inference**. This screen did not display the error-code bytes. Existing-file reads prepare OACK or DATA, or return an error to the caller; they do not prepare this successful ERROR reply. The firmware mux only claims the three whitelisted WLAN firmware paths and cannot take this MCFG request.

A committed byte in the settled MCFG counter does not prove presence at an earlier dispatch. The original first WRQ/final ACK is `server_check.txt`, not `mcfg.tmp`. The prior [IMG_9712 analysis](sunfish-post-elf-mcfg-IMG_9712.md) directly observed the same absent-file result and subsequent write. In the saved successful Linux journal, WLAN firmware requests began 8.297133 seconds **before** the first absent-file MCFG read. Neither filling MCFG nor changing its options follows from this error. See [the saved comparison](sunfish-mcfg-probe-comparison.md). No additional phone probe is needed merely to identify that ERROR.

Pinned Linux fd78d17954a86ea6ceb0c535e618ea129e09f8dd (`net/qrtr/af_qrtr.c`) and Android kernel 28f9290ae067d3e7857126eb6312720dbe9c7c80 (`net/qrtr/qrtr.c`) use per-destination QRTR low/high watermarks 5/10. Both `drivers/rpmsg/qcom_glink_native.c` implementations reserve a fitting available remote intent and return/remove the exact completed intent. They do not impose the native two-application-reply cap. This establishes the ownership/credit model; it does not show three simultaneous replies in the recorded successful boot or reveal the proprietary modem's application deadline.

## Implementation boundary

Feature `sunfish-elf-third-reply` lets Owner's **first accepted exact ELF request handle** call a separate checked queue method. Core revalidates the live request ticket and exact registry name `kernel/elf_loader`. Both older replies must have the same registry source and destination, be fully published, retain different owned remote slots and reference-flow tokens, have local QRTR completion, and still await GLINK completion. Only two older replies are permitted. A fourth fails; ordinary queue callers keep their prior two-reply bound.

The selected ELF still requires a free normal transmit slot, its own normal QRTR credit, valid reply encoding and a fitting remote intent at publication. Neither older transmit retires early. No pools grow, no grant advertisement/ACK order changes, no writer priority changes, and no FIFO, table, cursor, lifecycle or fault check is skipped. The adapter wrapper reproduces the normal queue boundary. Owner's normal work, registry dispatch and Runtime methods remain identical. This changes allowed reply overlap, not the interpretation of RX_DONE as application acceptance.

## Offline verification

Five new Core tests cover all six completion orders with and without clocks, exact/stale request identity, both required older publications, rejection of a fourth, the ordinary two-reply cap, real credit exhaustion, global TX exhaustion, no available fitting intent followed by a grant, byte-identical blocked-write retry, invalid replies and uncertain publication. Owner replay covers all six completion orders in both clock modes and lifecycle loss after request acceptance. Disabling only the new Owner admission makes its test fail with two published replies rather than three. Existing full Core, Owner, IRQ, fault and transfer suites remain required.

The old ordinary-queue protocol-byte fixture and owner-only replay retain their previous byte/order parity. They do **not** exercise this new Owner-selected third-admission path; its dedicated tests establish the intended behavioral difference. Native image verification includes the actual renderer, selected stack analysis, capability mutations, boot layout and actual packed initializer mappings. The existing pinned Normal-NC SDK is reused and reproduced against the newly built payload; this is not a fresh SDK compilation. Selected stack analysis excludes TLS, indirect calls and unexamined descendants.

## Cold temporary boot

Profile: `.cache/sunfish-native-wifi/private/native-elf-third-reply-v1`. Do not flash a partition. Fully power off, enter fastboot with Volume Down + Power, then run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-elf-third-reply-v1/fastboot.img
```

Wait at least one minute and photograph the entire settled **ELF THIRD REPLY WINDOW** panel. `LIMIT ADMIT SEQ N RX` shows limit 3, admission sequence, number of older owners, and last prior receipt transaction. An older-owner count of two means the new overlap was used; one/zero means a fallback or different arrival order. Compare queue/publication/completion against retained old receipts and the MCFG/startup counters. The window freezes on matching ELF completion; missing later old receipts do not establish that they never arrived. No handset timing, firmware request or WLAN/WLFW advance is claimed before this boot. Even a successful early ELF exchange would leave WLAN firmware loading and the later Wi-Fi bring-up milestones unproven.
