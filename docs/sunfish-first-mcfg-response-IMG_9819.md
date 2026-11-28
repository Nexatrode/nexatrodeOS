# IMG_9819 — first early MCFG read received a completed missing-file response

Settled **FIRST MCFG RESPONSE**, associated with the preceding manual boot of source `3bd9ab0a9b877224d74c5251e417111d0fcef572`, image SHA256 `c858589fc035640add0beb327962171ced07740f7f18429a3936a06c61b9060b`. Association uses instruction/title, not an independently read handset hash. Original HEIC, readable JPEG/crops, all35 physical data rows, executable decoder and arithmetic are archived separately under the immutable profile's `device-result/`. All139 delivered artifacts were reverified unchanged. No device command or image build was performed for this result analysis.

## The first request is now identified, and the zero-rsize alternative is excluded

The bound original request is peer49,73-byte RRQ, reservation19=25, flagsBF. Offered options mask0F, flags0, unknown-count0 identify **blksize7680, timeoutms1000, wsize10, tsize0**. Rsize, seek and unlink are absent; their all-ones values are unavailable, not explicit maxima. Consequently the earlier synthetic zero-rsize rejection cannot explain this actual request. This is a valid size request whose body length previously could not identify its options.

At its actual server dispatch, FILE is absent and server result0 forms **TFTP ERROR1,19 bytes**, local transfer port5005, matching remote intent1. It is not an OACK. Reply-options mask0 is expected for an ERROR. The exact original published and received its real GLINK completion, then its peer closed; it was not canceled. FlagsBF do not contain the client-ending bit40. First/last client ACK-or-ERROR and their clocks are unavailable, ACK count0, ERROR count0. Thus this particular request did not demonstrate ERROR9 stat termination. One same-peer RRQ repeat is counted; the counter does not prove equality of all option bytes.

The settled RAM state is1 write,1 committed byte,0 completed DATA reads. That later file state does not contradict absence when this first response was formed. The saved Linux cold boot likewise encounters absent MCFG, and first requests WLAN firmware8.297133s before its first MCFG read. IMG_9771 already completed a later watched MCFG response without WLAN. Current evidence does not justify fabricating a file, broadening option policy, or treating this normal negative response as the initial WLAN prerequisite.

## Response timing and actual queue pressure

| Boundary | Hex microseconds | Interval |
| --- | --- | ---: |
| Accepted → server dispatch | 015C908D → 016311E0 | 426.323ms |
| Dispatch → successful queue | 016311E0 → 01656BFA | 154.138ms |
| Accepted → successful queue | 015C908D → 01656BFA | 580.461ms |
| Queued → publication | 01656BFA → 016C6D65 | 459.115ms |
| Published → exact GLINK completion | 016C6D65 → 0173144E | 435.945ms |
| Completion → exact peer close | 0173144E → 017438C0 | **74.866ms** |
| Accepted → publication | 015C908D → 016C6D65 | **1,039.576ms** |

These are host observation clocks. Completion before close does not prove that the response beat a modem application timeout, nor establish application acceptance. No modem peer-tail/FIFO-visibility observation for this MCFG response is on the page.

Two admission attempts, zero FlowBusy and **one NoCapacity** are recorded. First attempt0163598A precedes successful queue by135.792ms. Last BLOCK check2 is global transmit-pool capacity; its kind/op/flags/length/sent are0 and reservation/publication/completion unavailable. This is not a named unsent or published predecessor, and does not identify all eight occupants or the first freed slot. It must not inherit IMG_9815/9817's different ELF pressure snapshot.

Four Core writer passes are retained,0 omitted. Only the first is printed:016723CE→0167B375,36.775ms, **command5 advertisement**, count1, result1. It starts112.596ms after queueing. Gates08830883 show fitting and normal-ready remote capacity, a pending receive return and ACK-yield at entry/callback, without outbound-intent waiting, unsent-flow-predecessor or selected-target DATA. It is not the target ERROR or evidence of a blocked callback. The advertisement's compact header does not identify its intent ID/size or prove a specific positive-grant backing. Target publication occurs309.744ms after that first pass ends; the other three stored records are not printed and this page cannot assign each interval to them.

Global queued-turn totals are14 calls,18 committed Session writes,5 RX vetoes,0 TX-backpressure stops. The RX-veto observer retained5 events and omitted0. These totals are diagnostic context shared across replies; they do not partition the426.323ms predispatch delay, identify the exact next pending frame, or prove all calls belonged to this original reservation. The capture resolves the request/response identity and outcome; it does not justify another unchecked immediate writer turn.

## Earlier transport and ELF status remain established

TX3 calls5/mask007E23AC again confirms all three bounded credit/own-return/reply stages committed. Queue→publication383.602ms; publication→exact Core receipt3,985.612ms. The current page has no TX3 peer-tail/receipt-visibility fields, so IMG_9818's6.117ms/3,892.378ms intervals cannot be copied into this boot.

ELF receive013BC5AB/publication014D33CB/Owner016E6813: request→publication1,142.304ms, publication→Owner2,176.072ms, total **3,318.376ms**. Exact Core016E207A precedes Owner by18.329ms. Body match1 and requested/published/completed host credit7 are present; ELF's watched remote intent is4. Original final ACK00D2BFBF is present. The first MCFG RRQ is accepted1,169.286ms before ELF Owner completion; its ERROR publishes129.710ms before that boundary and completes306.235ms afterward. This confirms why the old after-completion watcher could miss the original request.

## The bounded journal and startup outcome

Journal68 total,52 omitted,16 retained,0 bad. Opening records show ELF request transaction5/message21, DEL_CLIENT node0/port21, the host QRTR credit for1:4003, and the ELF response. Later opening entries show ruleset RRQs from48, a19-byte ERROR1 from5004 and that peer's close. The first-other-path latch is exact digest category8, peer4E,93-byte RRQ. The displayed late tail has further digest requests from56/58, ERROR1 replies and their closes. These later requests/transfer ports are not the original MCFG reservation. The first MCFG response itself lies in the omitted journal middle, but its original scalar state is separately bound by opaque request and reservation, not inferred from journal ports.

Runtime error/detail0, cleanup stage6,14 close observations/commits. Historical IPA complete1, no displayed pending IPA message or failure, PD7FFFFFFF. **WLAN firmware RRQ0 / WLFW servers0**. The new capture resolves the MCFG-option ambiguity and proves a completed source-compatible missing-file response for the early request. There is no new WLAN milestone.

No extra Pixel command or unchanged rerun is required to interpret this result. Next offline work should return to the earlier Linux/native startup divergence and request/response deadline/acceptance evidence; repeated MCFG-priority changes are not supported merely because this read returned a valid negative reply. Any further diagnostic should reuse the existing receipt/ELF/startup captures and state a new discriminating question.
