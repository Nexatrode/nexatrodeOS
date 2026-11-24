# IMG_9717: both registry owners and exact ELF admission receipts

Settled **ELF WINDOW RETIREMENT**, image `f8363bbcaebd3db8cbea48688f46bb544e980059507bb1d11a792f4e73430c75`, source `e74c06f0`. The original photo and decoded rows are saved separately under that delivery profile's private `device-result/`; the delivery manifest remains unchanged. Runtime error zero.

Three normal same-peer rejections precede the fourth, successful admission. First failure `014A3E98`, sequence1/reason1: two owners. Transaction3 is fully published, owns modem intent3, has local QRTR retirement and a reference token (`3B`). Transaction4 has only local retirement/token (`28`), no remote intent and no publication. Derived gates `19` mean multiple owners, unpublished bytes and no remote slot. At the last failure `01565FE0`, sequence3/reason1, both3/4 are fully published (`3B`) and own distinct intents3/10; gates1 means multiple owners alone.

| Exact Core event | Clock hex | Sequence | Earlier owners remaining |
| --- | --- | ---: | ---: |
| Transaction3 receipt | 0159F734 | 4 | 1 |
| Transaction4 receipt | 015C9C11 | 5 | 0 |
| Successful ELF admission | 015E010A | 6 | 0 |
| Exact ELF transaction5 receipt | 0168F4C0 | 7 | 0 |

There is **no admission attempt between the first and second receipts**. Transaction3's receipt clears the multiple-owner condition while4 is fully published. Existing offline cases show that either completion can allow another normal queue attempt, retaining the other's bytes and intent. This photo does not show that both receipts were required, or prove reference credit would have passed at the earlier instant. Admission follows receipt3 by264.662ms and receipt4 by91.385ms. The Owner completion fields for4 and ELF are later observation sites than the Core receipt clocks, by18.266ms; these are not conflicting event identities.

The existing Owner completion handoff checks both the historical first named blocker and the last pressure's single `peer_request`. `record_registry_pressure` overwrites the latter while walking the active transmits. That can recognize4 while missing3 as a useful scheduling boundary. A new continuation must use exact validated receipt identity and live pending-request/window metadata, not treat the new diagnostic snapshots as authority. Normal queue admission still decides whether credits/capacity permit staging.

## Timing and remaining delay

ELF receive/queue/publication/Owner completion are `013B8619 / 015E0110 / 0163CCBE / 01693C16`. Receive-to-queue is2,259.703ms; queue-to-publication379.822ms; publication-to-completion356.184ms; receive-to-completion**2,995.709ms**, versus4,416.822ms in IMG9713 (1,421.113ms shorter across different physical boots). Selection-to-queue is1,313.237ms by displayed-age subtraction, versus2,700.034ms previously. Direct phase spans may differ by1us from independently rounded age subtraction.

Queued-handoff flags are**0F**, not1F: the turn entered and returned, but exact ELF publication was later. Entry/end `015E0112 / 016064E1` span156.623ms; full publication follows end by223.197ms. Thus this result must not be reported as publishing inside the extra turn.

Transaction3 was published at`01220971`, sampled consumed at`01222173` (**6.146ms** later), then its receipt was last absent at`01517726`, first visible at`0151C0A9`, with**152 bytes ahead**. First visibility is3,120.950ms after sampled consumption; visibility-to-Core processing538.251ms. The18.819ms absence/visibility bracket is a sampling bound, not exact remote emission time. History evictions0. These scalar fields do not identify the152 intervening bytes.

## MCFG and WLAN

Current post-ELF MCFG peer is**4B**, a different target from IMG9713's4A. Request offers block7680, timeout1000ms, window10, tsize0; mask0F, parse flags0. There is **no explicit rsize option**. The native result is0, a one-byte committed file exists, and a45-byte opcode6 OACK is prepared. StagesFF establish dispatch, queue, publication, matching completion, a client ACK/ERROR and close. This target was serviced; it was not silently rejected like the earlier target with stages83. The ending opcode/code and negotiated reply options are not displayed here, so ERROR9 is not inferred from the photo.

MCFG counters remain1 write /1 byte /0 full reads. Existing Linux/native notes already show a size probe can end without incrementing full-read count, and successful Linux WLAN reads precede its first MCFG read. Neither zero reads nor a successful MCFG OACK establishes a WLAN prerequisite. Original final ACK`00D8884F` completed6.487498s before ELF acceptance. **IPA1, PD state7FFFFFFF, WLAN RRQ0, WLFW0.** WLAN has not advanced.

The next isolated change is a bounded normal service continuation after the first eligible, exact registry receipt for the pending ELF endpoint pair. It must keep FIFO validation, advertisement-before-ACK, ordinary queue/credit checks, failure handling and original remote ownership intact. No additional phone command is needed to interpret this result.
