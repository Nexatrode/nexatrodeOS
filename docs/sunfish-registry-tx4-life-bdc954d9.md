# Transaction 4 first opportunity: ready QRTR credit, then grant advertisement

The settled `bdc954d9` photo is from **REGISTRY TX4 TURN LIFE**, source `59cc4103`, image SHA256 `d16c88eb4e6e4597c544c2899a4ebac53010fba3b7be2f1f80c07d6ebebdb748`. Original photo, all 27 fields, decoding, timing arithmetic and the existing focused credit replay are saved privately under `native-registry-tx4-lifetime-v1/device-result`. Its build manifest remains unchanged.

Runtime error is zero. Ten opportunities armed; the trace retains the first. At that first entry, flags `0x28A3F` have bits 0,1,2,3,4,5,9,11,15,17 set. This was an ordinary entry with a positive pending ACK, the correct unadvertised backing (`0x4A`), exact request/token (`0x44`), matching live transmit and fitting remote intent. The converted registry candidate existed. There was no own-return, cooldown, prior borrowed-attempt or unsent-flow-predecessor flag. Registry priority was excluded by a ready QRTR credit, corroborated by **ENTRY GATE 10**. The token had survived into the writer; this first miss was not a lost identity or absent fitting buffer.

CHOICE `FFFF0005` means a receive-intent advertisement, without a QMI transaction. PARAM 1 is its advertised-intent count, not intent ID. WRITE RESULT 1 confirms a committed write. EXIT CAUSE 2 confirms that the matching backing advertisement ended the opportunity. Matching advertisement and positive ACK times retain correct advertisement-before-ACK order.

| Interval | Time |
|---|---:|
| Transaction 4 queue to first arming | 3,171.257 ms |
| Arming to first entry | 18.476 ms |
| First entry to selection | 0.013 ms |
| Selection to advertisement observation | 36.118 ms |
| Advertisement to matching ACK | 89.961 ms |
| First matching ACK to transaction 4 publication | 2,883.195 ms |
| Transaction 4 queue to full publication | **6,199.020 ms** |

There were 33 publication calls; the frozen mask is again `0x24AC`. The first credit exclusion and committed advertisement explain why this first opportunity did not attempt the registry callback. The trace does not show the credit's flow or publication time, nor prove that the same credit blocked all ten arms. The 3.171 seconds before first arming and 2.883 seconds after its ACK are separate unclassified intervals.

Publication is 901.501 ms earlier relative to queueing than the preceding boot's 7,100.521 ms, but these are separate boots, not a controlled causal performance test. IPA completed; WLAN firmware RRQ and WLFW counters remain zero. This panel omits transaction 3 visibility and ELF timings, so those outcomes cannot be compared from it.

**Trace limitation:** ARM LEN SENT is zero. Source selection requires a nonempty matching transmit, and entry flags corroborate one; the zero field cannot be used as evidence of a zero-byte reply or byte progress. It is retained verbatim as an observation anomaly. The host fixture captures a nonzero length; that alone does not establish why this handset field is zero. Any new length observation should have independent provenance/consistency checks and remain outside policy decisions.

Next offline target: preserve the ready QRTR credit's priority while preventing a stream of fresh grant work from repeatedly displacing its turn. Test a bounded credit callback at the exact converted transaction 4 opportunity, with normal remote-intent/FIFO/credit checks and advertisement before ACK, spent on blocked or uncertain attempts as well as success. Do not skip the credit to force the registry reply. Track the resulting credit, grant and transaction 4 publication before drawing an end-to-end startup conclusion.

No device command was executed while recording this result.
