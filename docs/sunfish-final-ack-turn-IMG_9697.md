# IMG_9697: earlier final ACK and a published transaction3 blocker

The settled ACK TURN ELF BLOCKER panel has runtime error0, TRACE MATCH1,
ACK TURN1 and ACK PHASE4. The one-shot ran, the exact original accepted final
DATA's ACK was published, and matching GLINK completion was observed. WLAN
RRQ and WLFW SERVERS remain0.

| ACK interval | IMG_9696 | IMG_9697 |
|---|---:|---:|
| DATA receipt to dispatch | 603.140ms | 551.018ms |
| Dispatch through preparation | 6.354ms | 6.105ms |
| Preparation to queue | 18.281ms | 18.302ms |
| Queue to publication | 471.093ms | 222.972ms |
| DATA receipt to publication | 1098.868ms | 798.397ms |
| Publication to GLINK completion | 295.487ms | 25.761ms |

These compare two separate boots, not controlled causal benchmarks. The queued
interval was248.121ms shorter and overall receipt-to-publication300.471ms
shorter. The remaining551.018ms before dispatch is still substantial and was
not changed by this feature. The measured798.397ms does not establish an
application deadline: native accepted-DATA time is not the modem timer start.

ACK publication at00D6654E preceded close visibility00D67052 by2.820ms.
GLINK completion00D6C9EF followed close visibility by22.941ms. Close ordering
alone is not a failure verdict; DEL_CLIENT can also accompany normal transfer
teardown. The panel does not prove the modem application's acceptance of the
ACK or timestamp its timeout decision.

ELF request015B6984 to queue01DBA541 took8403.901ms, compared with15903.164ms
previously. Receipt through receive-pass completion took558.547ms, first
registry entry another12.191ms, entry to selection7219.963ms, and selection to
queue613.200ms. Queue to publication took172.350ms and publication to matching
GLINK completion658.036ms. Overall ELF receipt-to-completion was9234.287ms.
The shorter prequeue interval must not be attributed entirely to the ACK turn
from these two samples.

The new last-pressure fields are decisive about that snapshot:

- TX4 LAST WHY=1: the last rejection was the same-peer transmit gate.
- TX4 PEER TXN=3: the peer reply was transaction3.
- TX4 KIND FLAGS=0000040B: kind4(reply), started and fully published, GLINK done
  clear, local QRTR retirement bit set.

Transaction3 was therefore already published at transaction4's last queue
rejection and still awaiting its matching GLINK RX_DONE. The local QRTR bit is
not proof that a QRTR RESUME was received: this profile's reference SERVREG
path sets `qrtr_done` at queueing and tracks reference credits separately.
The last reason1 nevertheless distinguishes this rejection from the later
reference-flow admission check. It does not describe every prior wait or the
time spent in the exact captured state.

This differs from older snapshots of an unpublished transaction3. Its matching
completion/visibility path, rather than another assumption that it needs its
first writer turn, is now the relevant offline comparison. `retire_transmit`
requires both completion flags; reusing this slot before GLINK return would
break the existing ownership rule and is not a proposed optimization.

ELF itself recorded2same-peer waits behind transaction4. Transaction4's
publication01D71A74 followed ELF selection01D249F1 by315.523ms, and its
completion01DA570A followed publication by212.118ms. ELF queued85.559ms after
that completion. This identifies the direct blocker separately from the long
preselection wait on transaction3.

The next offline review should follow transaction3 from publication/peer read
to RX_DONE visibility and host ingestion, and investigate the remaining DATA
receive-to-dispatch interval using existing call histories first. No additional
Pixel command, runtime change or new image was needed to read this result.
The HEIC, preview, raw27fields, source/image/manifest hashes and arithmetic are
under `native-original-final-ack-turn-v1/device-result`. The historical image
and delivery manifest remain unchanged.
