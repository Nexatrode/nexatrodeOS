# A bounded registry reply turn after denial becomes a grant

IMG_9703 identified the trigger mismatch: 27 selector checks saw a pending denial, 26 saw a positive ACK with no unadvertised backing, and 26 saw fresh backing but no ACK-yield flag. No borrowed registry callback ran. The counters do not establish a chronological sequence for every request. See `sunfish-registry-grant-gates-IMG_9703.md` for the exact result and limits.

The public Session replay fills all eight wire slots, accepts and returns transaction 3's request while retaining its original host-only registry identity, refills the wire slot, queues the reply, receives another QRTR record and a 108-byte intent request, and observes the actual denial. The slot-releasing RX_DONE converts that denial into a positive grant. The prior scheduler then emits the backing advertisement; the negative control fails at the exact expected registry DATA opportunity. The new scheduler emits that exact reply first, then the backing advertisement and positive ACK.

## Scope and bounds

The optional `sunfish-registry-converted-grant-turn` feature records one private opportunity only when a successful RX_DONE actually converts a full-slot denial into a positive grant. Its token contains the new backing intent ID and the selected reply's opaque owner/generation/request identity. It is not keyed by a reusable TX-slot index, display history, clock value or diagnostic counter.

Arming uses the normal ready-registry selector. The request's own receive return must already be complete and a fitting modem receive intent must be usable. The next ordinary publisher rechecks the normal candidate and exact identity, pending positive ACK, still-unadvertised matching backing, live ReplyQueued state, per-transmit borrowed-attempt latch, registry cooldown, protected first-WRQ/TFTP work and ready QRTR credit. Same-flow predecessor and outbound-intent waiting checks remain inside the normal selector. Only when that exact registry candidate remains eligible and no ready credit wins is the preferred backing-advertisement turn lent to registry DATA. The global ACK-yield flag is not restored or otherwise changed by this feature.

The token is consumed before the checked writer callback, including WouldBlock or unwinding. An uncertain write keeps the existing quarantine. The exact backing advertisement, any actual intent ACK or trusted reset expires the token. The existing per-transmit borrowed-attempt latch prevents a later conversion from repeatedly lending turns to the same blocked reply. A large reply borrows only one 256-byte fragment: advertisement and positive ACK precede any continuation.

The reply stays bound to the original transaction and normal completion receipt. This change does not retire a transmit, release a modem buffer, reserve additional credit or admit another service reply by itself. Ordinary queue checks and the existing two-reply window remain in force. The FIFO cursor/domain checks, checked writer, fault handling, grant advertisement-before-ACK order and actual QRTR credit accounting are retained.

## Offline checks

Ten focused tests cover the exact conversion, blocked reply and byte-identical fallback, blocked advertisement retry, blocked/uncertain receive return, uncertain reply and same-session generation reset, ready QRTR credit, ready TFTP OACK, absent fitting modem intent, repeated conversion after a consumed attempt, two outstanding replies with reverse receipts, and a multi-fragment reply. Existing wire-slot-reuse tests now accept earlier publication under this feature while still verifying the original transaction's completion after slot reuse; the occupied-host-entry test verifies the second reply and reverse receipts under the existing bounded window. Previous profiles retain their earlier expected publication order.

The focused negative-control source, fixture and expected failure are retained privately. Full current/prior owner/Core suites, existing fault controls, source/stack/table checks, model I/O comparisons, capability mutations and actual SDK/loader/packed archive/source/boot verification are required before delivery. The Owner-only comparison matrix is not a handset timing proof or a substitute for the targeted conversion tests.

The saved Linux and Android handlers continue to constrain new grants: allocate then advertise before the positive ACK, or ACK an already-advertised reusable intent. The new DATA opportunity is before the new backing advertisement, never between that advertisement and its ACK. Neither source determines native seL4 scheduling costs or promises a WLAN outcome.

## Panel and interpretation

The panel is **REGISTRY GRANT RETRY**, retaining the same 27-field layout and transaction 3/4/ELF timestamps. OLD MASK bits 8/9 continue to identify any actual borrowed callback attempt/successful DATA commit. New bits are:

| Bit | Hex | Meaning |
|---|---|---|
| 10 | 00000400 | Exact converted-grant opportunity armed |
| 11 | 00000800 | Callback attempted for that conversion |
| 12 | 00001000 | That callback committed its DATA fragment |
| 13 | 00002000 | A denial-to-grant conversion occurred while this reply was unpublished |
| 14 | 00004000 | Own request RX_DONE still pending at a conversion |
| 15 | 00008000 | Normal selector did not choose this exact reply at a conversion |
| 16 | 00010000 | Reply had already consumed a borrowed opportunity at a conversion |

These bits accumulate occurrences until the existing predecessor snapshot freezes at full publication. They are not counts, durations, a one-to-one cycle trace or proof that the modem accepted the application response. Bit 12 is a successful fragment write, not matching RX_DONE; use OLD DONE US for the latter.

NOAD retains the positive-ACK/no-unadvertised-backing meaning. NOYIELD now means neither the existing ACK-yield opportunity nor an identity-matching converted-grant token is present. The safety guards use raw protected WRQ/TFTP work even when ordinary prefer-ACK would otherwise mask it. Remaining packed rows retain first-condition precedence. CREDIT BORROW's low half means registry selection, while mask bits 8/11 prove an actual callback. Paired counters saturate each half at 65535; an FFFF half can be a present saturated count. Absent/live/frozen state remains separately indicated.

Private delivery profile: `.cache/sunfish-native-wifi/private/native-registry-converted-grant-turn-v1`. No automatic ADB/Fastboot/flash action runs. The physical test asks whether the conversion opportunity actually arms and reduces registry/ELF startup delay, and independently whether WLAN firmware requests or WLFW announcements appear. Working Wi-Fi is not established by an earlier registry reply alone.

## Verified delivery

Image size: **68,919,296 bytes**. SHA256: `02e5a6886e055c95ee4934758398d47c41a45449534a71329d662c23a2a05e4b`. Current/prior owner suites pass 575/575 with one prior ignored each; Core/link suites pass 242/232. All ten focused conversion tests pass, with the prior scheduler's saved expected failure. The 24 paired Owner schedules retain exact I/O and peer-event order; they are controls rather than coverage of the new conversion path. All 304 negative capability cases are rejected. The actual packed initializer has 24,540 objects and exactly eight NC table mappings; correct-count, wrong-count and missing-count controls pass. Pinned SDK/whole loader reproduction and current generated bindings pass.

The generated layout has one fewer CapDL object than the previous profile. Only the 32 SMMU data pages and 512 IPA receive data pages move, each 4,096 bytes lower; their generated `region_paddr` symbols are used at runtime. The remaining 3,093 named protocol-frame physical bindings, including all eight table frames and MMIO, are unchanged. The current actual allocation, permissions and complete payload are audited. The selected compiled stack audit retains 939,744 bytes of headroom in 5 MiB; it is not whole-program worst-case stack certification.

Fully power off, then enter cold bootloader fastboot with Volume Down + Power. Run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-converted-grant-turn-v1/fastboot.img
```

Wait for **REGISTRY GRANT RETRY** and **RESTART WHEN DONE**, then photograph the whole settled panel. This temporarily boots the image without flashing a partition.
