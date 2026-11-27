# IMG_9804: repeated intent-control competition and the exact post-blocker retry gap

Settled **STARTUP REPLY PRESSURE**, associated with the preceding manual boot of`native-startup-pressure-v1`, source`f2ea65df9bad14af1828ab5a9c7f4cd306fbfce7`, image SHA256`b47dd1f392bf7004db1dca416b878c5135aa45f273342362754b5ca4c969b8c8`. Association uses the photo and preceding instruction, not an independently measured handset hash. HEIC, full-resolution/detail crops, all rows and arithmetic are retained separately in the private profile's`device-result/`. All68 delivered artifact hashes/sizes were reverified without changing the delivery manifest or artifacts. No device command or new image build was performed for this analysis.

The adaptive display selected the correct alternative: ELF recorded one peer rejection and zero transmit-capacity, flow or credit rejections. This is the unpublished-reply path, not IMG_9802's full-pool path. Unlike the pool-only IMG_9803, this photo displays the already-retained frozen writer gates and actual retry boundaries.

## All128 transaction3 publication calls are accounted for

Transaction3 frozen snapshot2 identifies transaction3/88 bytes, calls`80`=128, mask`E0AC`. The user confirmed the first priority-gate row as`0,9,69,D` hexadecimal.

| First applicable ordinary priority gate | Count |
| --- | ---: |
| Cooldown | 0 |
| Protected TFTP priority | 9 |
| Pending intent ACK/control | **105** |
| Own receive-buffer return pending | **13** |
| No candidate / other registry candidate / unavailable ready credit | 0 / 0 / 0 |
| Selected registry priority | 1 |
| Total | **128** |

The ACK-window counters record37 committed intent acknowledgements,32 host receive-buffer returns,36 advertisements and0 selected intent-ACK WouldBlock outcomes. The105 actual control commits equal the105 ACK-gated calls. These counters do not match individual triples or identify grant sizes, request identities or positive/negative ACK outcomes. The largest completed pending-ACK age is431.320 ms (`000694D8`), supporting repeated control competition rather than one intent ACK blocked for12 seconds.

Borrowed-turn first-failing gates also total128: no pending ACK19, denial36, positive ACK without unadvertised backing37, and no ACK-yield/converted opportunity36. Gates4–B are all0. Because earlier failing predicates mask later ones, zero borrowed own-return or credit counts do not prove those conditions were simultaneously clear.

Mask`E0AC` sets control/other-DATA selection, eventual normal priority publication and conversion seen/own-return-pending/no-exact-candidate bits. Borrowed attempt/commit and conversion arm/callback/commit bits are absent, as in IMG_9803. Exact converted-grant ID/times are unavailable because no matching predecessor grant turn armed. No no-fit, outbound-intent waiting/request-selected or selected-reply FIFO-blocked bit appears. The source first-gate order confirms that13 own-return observations occur after earlier TFTP/ACK exclusions clear; the105 earlier ACK-gated observations cannot also be labeled own-return-ready. These are call/event counts, not an exclusive elapsed-time partition or proof that validation/service gaps were free.

## Dominant publication interval remains slow

| Transaction | Accepted | Queued | Published | Matching completion |
| --- | --- | --- | --- | --- |
| 3 | Not displayed | `011EA4DA` | `01DE4416` | Core`01DEA87D` (journal`01DEA87C`) |
| 4 | `01407A53` | `01E00C9E` | `01E170A1` | Core`01E1D489`, Owner timeline`01E21BD2` |
| ELF5 | `0148C4C8` | `01ED42B9` | `01EE2FFA` | Core`01EE9408`, Owner`01EEDB18` |

Transaction3 queued→publication is**12,558.140 ms**, followed by25.703 ms to its displayed Core receipt. Transaction4 accepted→queue is10,457.675 ms, queue→publication91.139 ms and publication→Owner completion43.825 ms. The dominant delay resembles IMG_9801/9803; this is not the fast converted-turn schedule of IMG_9802. Transaction4's conversion-target snapshot remains absent; its independent timeline confirms that the actual transaction completed.

ELF accepted→queue is**10,780.145 ms**, queue→publication60.737 ms and publication→Owner completion43.806 ms, total10,884.688 ms. Twenty preselection FlowBusy outcomes on transaction4 and one successful queue are encoded by21 calls (`15`), runs`00041214 / 00041101`, omissions0. Once admitted, ELF's own publication/return is prompt in this boot.

## The retry observer confirms which first-slot opportunity was spent

Actual consumed-slot observation`01DF95AC` names transaction3, Core receipt sequence1. It follows the journal's transaction3 receipt by60.720 ms; that span includes the containing checked iteration and observation sites, not remote latency. The Boolean was set by the real first-slot branch, not inferred from timing. Normal service then queued transaction4; exact ELF's failed admission at`01E0833F`, sequence2/reason1, sees one earlier owner`00042804`, transaction4 flags28 with no remote intent and gates18. Its bytes were unpublished, so admission correctly failed.

Transaction4's exact Core receipt is sequence3, matches that recorded blocker and leaves zero earlier same-peer owners. ELF admission is sequence4, count0/gates0, last receipt transaction4,2 attempts total. Target ELF receipt is sequence5. There are3 receipts and0 omissions.

Retry flags`1FF` establish original pending-ELF identity, exact blocker receipt, successful containing work return, next registry entry/selection, original ELF selected, first-slot branch already consumed, and deferred work return. Entry-before-return is clear. This is the actual Pixel sequence hypothesized in [the retry design note](sunfish-tx3-elf-retry.md); it is no longer merely an offline possible schedule.

## The approximately749 ms Core-to-admission gap is now partitioned

| Observation / span | Clock or displayed duration |
| --- | --- |
| Exact blocker Owner receipt | `01E21BD1` |
| Successful containing Owner work return | `01E51453` |
| Next registry dispatch entry | `01ECCC1A` |
| Exact ELF selection after normal checks | `01ECFB93` |
| Core admission / Owner queue | `01ED42B4 / 01ED42B9` |
| Owner receipt → containing work return | **194.690 ms** |
| Work return → registry service entry | **505.798 ms** |
| Registry entry → selection | 12.152 ms |
| Selection → Owner queue | 18.214 ms |

The displayed spans sum730.854 ms. Subtracting the independently converted absolute Owner endpoints gives730.856 ms; the2 us difference is expected independent conversion rounding. Core receipt→Core admission is749.099 ms. The leading Core→Owner-blocker observation is18.248 ms, and Core admission precedes Owner queue by5 us. These scopes reconcile; do not compare the four Owner spans alone with an older Core-to-Core interval.

Receipt progress RX/TX1/1 becomes3/3 at successful work return: two more receives and two publications occurred in that work call after the copied receipt counts. Those counts do not identify headers or exhaust every hardware operation. The505.798 ms interval includes enclosing Runtime work, scheduling/preemption and elapsed service separation; it is not evidence of505.798 ms of registry queue-check CPU cost. Actual selection/check→queue accounts for only about30.366 ms.

The exact blocker Owner receipt copied into the retry observer differs by1 us from transaction4's separate Owner timeline, and transaction3's two Core sites differ by1 us. They are separate clock observations with matched identities, not mismatched completions. The later queued handoff spans304.112 ms and ends199.572 ms after ELF Owner completion; it includes surrounding transport work and cannot replace queue→publication timing.

## Outcome and next offline work

Error/detail0, live IPA flags1F, no pending IPA message/MCFG close, cleanup stage6. Original final ACK`00D7AED8`, read count15/kind mask2E, queued/completed locator masks75, EFS pair33 with no second overlap offer/admission. Historical IPA complete1; **WLAN firmware RRQ0 / WLFW servers0**. No WLAN milestone changed.

Two distinct targets are now concrete. The dominant one is transaction3's128-call control/own-return competition, with no converted-grant arm. Replay a real own-return-clearing window and a bounded subsequent ready-registry opportunity, preserving exact live identity, fitting intents, normal credits, protected TFTP work and advertisement-before-ACK. Do not arm from diagnostic masks or retroactively bypass an already-advertised grant. The smaller target is the exact transaction4 blocker receipt after the lifetime first-slot turn was consumed on3. An offline replay can assess a separately bounded exact-blocker handoff and where regular Runtime service must still run; it must not introduce an unrestricted transport drain or bypass fresh admission checks.

The current photo is sufficient to begin those replays. No additional Pixel command or repeat photo is needed now. No new scheduling change or image was created while recording this result, and the new timing evidence does not imply that WLAN startup is the next guaranteed milestone.
