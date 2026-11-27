# IMG_9807: digest errors complete; slow ELF writer captured

Settled **ELF WRITER DIGEST REPLY**, source `5ad21fd762ac7cc651d3f1fef4936db88ca3f3ed`, image SHA256 `cec474f3d1792f9eaf5b449c0d0c67b8ba4ec737db60f45202d3175d7638aeb2`. All91 delivered artifact hashes reverified unchanged; raw photo, full/readable/focused PNGs, manual rows, decoder and hashes are retained separately under `device-result/`. Runtime error/detail0, cleanup stage6, no pending close. No Pixel commands ran during analysis.

## The digest behavior gap is closed in this boot

The first two exact digest records are peers4D/4E, reservations21/23, flagsBF, normal server dispatch result0, actual TFTP ERROR5/code1,19bytes, transfer ports5009/500A, remote intent0. Their independent receive/queue/publication/completion/close clocks are:

- 4D `01B4186C / 01B53609 / 01B683A3 / 01B7F9DF / 01B954A4`.
- 4E `01BE95F5 / 01BFB39C / 01C10147 / 01C1F47E / 01C34E6C`.

Both matching completions precede observed client close, by88.773ms and88.558ms. No client ACK/ERROR is observed; ERROR1 does not expect a DATA transfer. Result0 means a negative reply was prepared normally, not a successful file read. Digest total11/omitted9; do not generalize these first2 lifecycles to all11. Fourteen close cleanups committed, no runtime error. This establishes the new response/cleanup behavior without proving a WLAN dependency.

## ELF queued promptly, then waited6.819seconds

ELF accepted/queued/published/Owner completion `0143FFDA / 01451DA8 / 01AD29EB / 01ADD5CC`:73.166ms before queueing, **6,818.883ms queue→publication**,44.001ms publication→completion,6,936.050ms total. This reproduces a long post-admission wait, giving actual slow-case observations unlike earlier clean writer follow-ups.

There are61 writer calls, all61 timed, finished1/active0/regressed0. First entry24.398ms after queueing; last completed record ends13us before full-publication observation. Completed writer spans total2,230.648ms; the arithmetic remainder4,588.235ms is outside those completed Core spans, including inter-call work and observation overhead. It is not proven idle time or an exclusive CPU partition. Max observed inter-call gap311.061ms.

The first-priority gates account for all61 calls: cooldown0, TFTP6, pending GLINK intent ACK43, own-return0, no-candidate0, other-registry-candidate0, ready-credit exclusion11, selected1. These are selector exclusions, not61 distinct packets or milliseconds proportional to each count.

Independent borrowed-grant gate counts also sum61: no ACK18, negative ACK17, missing backing13, missing yield4, cooldown0, protected TFTP1, WRQ return0, own return0, used opportunity0, no exact candidate0, credit8, selected0. Do not add these to priority gates. The13 missing-backing checks exclude the existing before-advertisement loan; the17 denials retain the negative-ACK path. Publication mask24AC records conversion seen/armed but no converted callback attempt/commit; existing ready-loan bits19/20 are absent.

Eight records retain first4/latest4;53 middle records omitted. They show an advertisement, positive ACK, RX_DONE3C, reusable RX_DONE47; then RX_DONE5C, another advertisement, QRTR RESUME_TX and the exact transaction5 ELF DATA. All8 callback results1, all8 end-of-pass fitting flags set; the final exact record also has selected flag2. This proves fitting buffers at those retained checks, not continuously at every earlier point. No blocked/uncertain write is shown in these records. The ACK tracer counted13 committed ACKs, no blocked ACK,17 receive returns ahead and0 ads satisfying that tracer's predicate; the wire records independently show advertisements, so0 must not be read as no advertisements anywhere.

## Outcome and next boundary

Live IPA flags1F, no pending IPA message; ELF host-credit mask7 (requested/published/completed). Original final ACK completed `00D88361`. PD7FFFFFFF, WLAN RRQ0/WLFW0. Read count22/kind mask2E. No WLAN milestone moved.

TX3 queue/publication/Core completion `012F7B24 / 01338C5A / 0169CB0B`; TX4 queue/publication/Owner completion `01B6FADD / 01B8E8E3 / 01BAD6CF`, after ELF completed. A later transaction4 cannot be carried forward as an ELF admission prerequisite in this run.

The existing own-return-ready grant loan is deliberately bound to transaction3 and cannot arm an already-returned ELF request. Replay a separate, once-per-generation binding for the first successfully queued exact domain0 ELF response. It must use live validated request identity, not these trace counters, preserve normal own-return/fitting/flow/credit selection, and offer only one attempt before an unadvertised positive-grant backing. Advertised ACKs, negative ACKs and ready credits keep their turns. No speculative completion or larger queue follows from these observations. A repeat of the unchanged image is unnecessary.
