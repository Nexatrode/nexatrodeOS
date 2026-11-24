# Identify why the borrowed registry turn was not selected

IMG_9702's frozen transaction 3 record had no borrowed-attempt or borrowed-commit bit. The88-byte reply still waited10.881687seconds, with29 ACKs,29 receive returns and29 advertisements while unpublished. Its existing ACK-first counters cannot distinguish a denial, an already-advertised positive grant, or a fresh grant whose prior ACK-yield flag was cleared. See `sunfish-registry-between-grants-IMG_9702.md` for the physical result and limits.

The existing offline replay demonstrates one missed window: queue transaction 3 while its own receive return is pending, complete a prior grant, receive a fresh grant, then return the request buffer. That RX_DONE clears the ACK-yield flag; the next advertisement and ACK go first, and the reply uses ordinary scheduling. The new conjunction observer identifies these different stages in the real public Session lifecycle. This establishes a possible mechanism, not which predicate failed on the Pixel.

The optional `sunfish-registry-grant-eligibility` feature adds twelve saturating observation counters to each existing registry publication record. They classify the first failed condition of the existing borrowed-turn conjunction, or its selection, using the already-computed selectors. No eligibility condition, priority, callback result, hardware access or clock observation is changed. The existing identity-bound predecessor snapshot copies the counters at actual full publication, retains them through matching completion/retirement, and clears them on reset.

**REGISTRY GRANT GATES** retains the transaction 3, transaction 4 and ELF timestamps plus original final-ACK, IPA, WLAN and WLFW milestones. Six packed rows expose the twelve buckets, checked in this order:

| Packed row | High16 bits | Low16 bits |
|---|---|---|
| NOACK DENY | No intent ACK pending; ordinary scheduling applies | Pending negative intent ACK |
| NOAD NOYIELD | Positive ACK has no still-unadvertised backing | Fresh backing exists, but prior ACK-yield flag is clear |
| COOL TFTP | Registry priority cooldown | Existing TFTP priority |
| WRQ OWNRET | Protected first-WRQ return | This registry request's own RX_DONE still pending |
| USED NOCAND | This transmit already consumed its borrowed attempt | Existing selector has no exact candidate for this transmit |
| CREDIT BORROW | Exact registry candidate yields to ready QRTR credit | Exact borrowed registry candidate selected |

Each half saturates at 65,535. FFFF can therefore mean a saturated present counter, not missing evidence. When no predecessor publication record exists the whole packed field is FFFFFFFF. OLD SNAP STATE 0/1/2 distinguishes absent/live/frozen evidence. Every counted selection entry contributes to exactly one bucket, but the counts are not a per-operation elapsed-time partition. Follow-up selection may return without calling the writer; do not assume their sum must equal OLD CALLS in every runtime path.

The low BORROW count is selection at the registry arbiter, not proof of a writer callback. OLD MASK bit 8 (`0x100`) continues to prove the actual borrowed callback attempt; bit 9 (`0x200`) proves its successful DATA commit. A later follow-up filter can still reject a selected candidate. Earlier buckets mask later conditions by design. NOYIELD means the flag was absent while a positive ACK and unadvertised backing were present; it does not prove the reply's own return, credit or TFTP checks were simultaneously clear.

Focused public-lifecycle tests distinguish request return, expired yield, already-advertised ACK, used opportunity after WouldBlock, ordinary publication, successful borrowed selection and a negative ACK. Clock-enabled/disabled cases check that reading the observation adds no clock samples, and that exact receipt matching, frozen retention and reset remain intact. Existing bounded-turn, grant/credit/fault and current/prior suites remain required. The actual renderer checks all27 fields, absent/live/frozen records, packing/saturation, duration units, final-ACK identity and both framebuffers. The title was shortened after visual preview showed clipping; the packaged image is rebuilt with the short title.

Saved Linux/Android handlers still supply the protocol constraint: a newly allocated receive intent is advertised before its positive ACK; Android can ACK a suitable already-advertised reusable intent. Those sources do not contain this native scheduler's ephemeral yield flag. A further policy change requires identifying the physical failed condition; this image observes it without weakening ordering, credits or FIFO validation.

Private delivery profile: `.cache/sunfish-native-wifi/private/native-registry-grant-eligibility-v1`. No automatic phone action is authorized by this document. No WLAN outcome is predicted before hardware evidence.

The larger diagnostic record/code changes the generated allocation: CapDL now has24,541 objects rather than24,539. Only the 32 SMMU data pages and 512 IPA receive data pages move, each 8,192 bytes higher. Runtime reads their generated `region_paddr` symbols; all other 3,093 named protocol-frame physical bindings, including the eight table frames and MMIO, remain unchanged. The current complete loader reproduction, generated capability bindings and actual packed archive/object count passed before delivery. This is not a claim of identical allocation to the previous image.

Verified delivery: **68,919,296 bytes**, SHA256 `14998939b07c56693e26d85a66f017afad470c28b5e82c567d78baf8e99730c3`. Current/prior owner suites pass575/575 with one prior ignored each; Core/link suites pass232/230. Two new eligibility tests exercise five scenarios; the seven prior bounded-turn tests retain nine scenarios. The24paired owner schedules retain exact protocol I/O and peer-event order. All304negative capability cases are rejected. Actual pinned SDK/loader reproduction, generated current physical bindings, packed24541-object initializer/eight NC table mappings and actual object-count negative controls pass. Selected compiled stack headroom is940208bytes in5MiB; this is not whole-program worst-case stack certification.

After fully powering off and entering bootloader fastboot with Volume Down + Power, run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-grant-eligibility-v1/fastboot.img
```

Wait for **REGISTRY GRANT GATES** and **RESTART WHEN DONE**, then photograph the whole settled panel. The command temporarily boots the image without flashing a partition.
