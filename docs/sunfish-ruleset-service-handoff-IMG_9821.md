# IMG_9821: first ruleset and both EFS replies complete before close; reverse handoff unused

Settled **RULESET EARLY HANDOFF**, associated with the preceding manual boot of source `f78b1c4270bff0f2e80eb62e2049505ff663694d`, image SHA256 `92b00d03f84fa64ce8265126dfdd193b966afa82a41b96ca1fbe8035d6dcde67`. Association is by delivered instruction/title, not an independently measured handset hash. Raw HEIC, readable full/cropped JPEGs, transcribed rows and executable arithmetic are retained separately in the profile's `device-result/`. All153 original delivery artifacts were reverified unchanged. No device command or new image build was performed for this analysis.

## The new opportunities did not run

Service calls0/result0, flags67, writer calls0; all writer counts/commands0. Flags67 identify enabled, first ruleset captured, first EFS error admitted, matching ruleset close known, and first ruleset normally queued. Both opportunity-spent bits remain clear. This is not a blocked writer attempt or a failed extra service call.

This boot accepted ruleset first. Original ruleset queued00E0E1D9 before EFS1 queued00E17113, a36.666ms lead. Therefore the required real EFS-admission latch was false when the first ruleset passed normal admission, excluding the new writer group. The first ruleset was no longer an oldest pending request when EFS subsequently admitted, so the reverse service opportunity could not dispatch it. This is the original arrival order, rather than IMG_9820's reverse case. The faster first reply must not be credited to an unused new policy. Different instrumented boots do not isolate a controlled timing cause.

## Exact early exchanges

Ruleset total4/omitted0, EFS total2/omitted0. All six entries have flagsBF and ERROR opcode5/code1, with publication and matching GLINK return and no client-ending bit40. Ruleset1/2 are peer21, reservations12/15, port5002, rsize4/options17/85-byte requests. Ruleset3/4 are peer23, reservations16/1C, port5004, rsize absent/options0F/85bytes. EFS1/2 are peer22, reservations13/14, port5003, rsize absent/options0F/74bytes. All offered timeout1000ms. Numeric/peer equality does not prove complete request-byte equality or application acceptance.

| Exchange | Accepted → publication | Publication → matching completion | Completion relative to observed close |
| --- | ---: | ---: | ---: |
| Ruleset1 | **725.131ms** | 996.539ms | **1,026.275ms before** |
| Ruleset2 | 457.212ms | **7,135.524ms** | 6,411.381ms after |
| Ruleset3 | 386.300ms | **6,250.907ms** | 1,169.169ms after |
| Ruleset4 | 1,700.453ms | 743.002ms | 2,195.940ms after |
| EFS1 | 771.103ms | 880.405ms | **783.382ms before** |
| EFS2 | 263.974ms | 533.840ms | **696.568ms before** |

The first ruleset dispatches279.374ms after acceptance, versus1,565.416ms in IMG_9820, and its admission succeeds first try. This is a positive current outcome but not evidence that the reverse handoff caused it. Ruleset2 dispatch-to-queue173.130ms includes one FlowBusy; its queue follows the first matching return by73.479ms. Its earlier multi-second admission stall is absent. It publishes724.143ms before close, yet processed completion follows publication by7.136seconds. Merely allowing an overlap to save this173ms wait would not establish the cause of that later gap.

Ruleset3 publishes5,081.738ms before its host-observed close, yet its processed return is late. Ruleset4 has three FlowBusy failures over1,435.040ms dispatch-to-queue and publishes1,452.938ms after close. Existing same-peer ownership explains admission waiting; these published buffers cannot be retired speculatively. No full-pool failure is printed for any of the six entries.

The compact page does not show these ruleset replies' modem-tail crossings or matching RX_DONE FIFO visibility. Consequently their late processed completion cannot distinguish delayed modem consumption, delayed receipt generation, and host receive processing. It also does not show initial FIFO visibility of the requests, so accepted→publication is not the complete modem application response time. The current data do not justify broadening duplicate admission from equal numeric projections alone or removing the EFS prerequisite merely because the new group stayed unused.

## Other startup state and remaining question

Original peer1F sent DATA; final ACK completed88.701ms before close and8,741.753ms before ELF publication. ELF accepted→queue738.444ms, queued→publication459.968ms, publication→Owner1,372.776ms; total2,571.188ms. Exact ELF-body match1/completion1/host-credit mask7. This child display omits the retained ELF FIFO rows, so no current consumption/visibility bound can be imported from IMG_9820. TX3 calls3/mask3FA4 show no new credit/own-return/reply stage bits; the previous run's five-call mask does not describe this boot.

Runtime error/detail0, cleanup stage6,14 closes observed/committed. Live IPA flags1F, registration1/driver1/publications4/response1, no pending ID. RAM MCFG1write/1byte/0 completed reads; first other path8 is digest. Historical IPA1, PD7FFFFFFF, **WLAN RRQ0 / WLFW0**. There is no new WLAN milestone.

The next unresolved current exchange is the6–7second post-publication gap for ruleset2/3. Existing EFS and ELF receipt observers and saved Linux/native notes should be reused to separate transport consumption, FIFO arrival and host processing with the original request/reservation/remote-intent identity. Another image of the same handoff rows cannot recover those missing boundaries. No unchanged boot or additional Pixel command is needed to interpret this result. The successful original ACK, EFS pair and first ruleset should not be treated as unproven again.
