# IMG_9703: the borrowed-turn trigger never became true

The settled **REGISTRY GRANT GATES** panel corresponds to source `20911df4` and image SHA256 `14998939b07c56693e26d85a66f017afad470c28b5e82c567d78baf8e99730c3`. Original photo, preview/detail, all 27 fields, unpacked counters and arithmetic are preserved privately in `native-registry-grant-eligibility-v1/device-result`. The original verified delivery manifest and source snapshots remain unchanged.

Runtime error is zero and snapshot state 2 confirms that the exact transaction 3 predecessor record froze at full publication. The reply is 88 bytes. OLD MASK remains 0xAC: neither borrowed callback-attempt bit 8 nor successful borrowed-commit bit 9 is set. CREDIT BORROW's low half is also zero: this captured reply was never selected for the borrowed turn.

| First-condition bucket | Count |
|---|---:|
| No intent ACK pending; ordinary scheduling applies | 2 |
| Negative intent ACK pending | 27 |
| Positive ACK, no still-unadvertised backing | 26 |
| Positive ACK and backing, but ACK-yield flag clear | 26 |
| All later borrowed-turn buckets | 0 |
| Total | 81 |

The total equals OLD CALLS 0x51 in this boot. ACK SENT is 26 and maximum completed ACK age is 440 ms. Negative-ACK-pending counts are **not** 27 negative ACKs on the wire; the source can convert a pending full-slot denial to a positive grant after returning a receive buffer. Buckets count selector entries, not unique requests, one-to-one cycles or a chronological packet trace.

The positive-ACK/fresh-backing checks occurred 26 times, and each found the yield flag clear before reaching the later checks. The other 26 positive-ACK checks had no unadvertised backing, so that grant's advertisement/ACK ordering remained protected. This identifies why the existing narrow trigger did not select the borrowed turn. Zero lower buckets do not prove TFTP, own RX_DONE, credit, fit or cooldown were simultaneously clear during an earlier failure.

## Timeline

| Interval | IMG_9703 | IMG_9702 |
|---|---:|---:|
| Transaction 3 queue to publication | 9,997.159 ms | 10,881.687 ms |
| Transaction 3 publication to matching RX_DONE | 746.063 ms | 1,554.575 ms |
| Transaction 4 receive to queue | 7,281.835 ms | 8,168.093 ms |
| Transaction 3 publication to transaction 4 queue | 55.053 ms | 55.036 ms |
| Transaction 4 queue to publication | 498.966 ms | 498.623 ms |
| ELF receive to queue | 7,785.291 ms | 9,391.275 ms |
| ELF queue to publication | 515.618 ms | 204.413 ms |
| ELF publication to matching RX_DONE | 92.971 ms | 3,800.464 ms |
| ELF receive to matching RX_DONE | 8,393.880 ms | 13,396.152 ms |

Transaction 4 publication precedes transaction 3 completion by 192.044 ms, retaining the two-outstanding-reply result. Timings improved in this boot, but this image changes observation only and the borrowed turn still did not run. Separate-boot timings do not prove a scheduling speedup. ELF's total is also close to IMG_9701's 8,411.656 ms.

ACK PHASE 4 retains the original final TFTP ACK's matching GLINK completion, and IPA COMPLETE remains 1. WLAN RRQ RX and WLFW SERVERS remain zero: no WLAN firmware request or WLFW announcement was observed by these counters. ELF is a locator response, not a firmware transfer or proof of working Wi-Fi.

## Concrete next replay

In `qcom_glink_qmi::Session::publish_inner`, every successful non-ACK publication clears `intent_ack_yield` before its commit is applied. The `Commit::Done` full-slot retry path can then reserve the newly freed wire slot, set a backing intent and convert `intent_request_ack` from false to true. It does not restore the yield flag. Thus that source path necessarily reaches the next publisher with a positive pending ACK and unadvertised backing but no ACK yield. This is a concrete mismatch between the existing borrowed-turn trigger and a full-slot denial-to-grant conversion, consistent with the observed NOYIELD bucket; the photo alone does not assign each observed check to that exact path.

Replay a full wire-slot pool, a pending registry reply, the peer's bounded-size intent request, the actual receive return and denial-to-grant conversion. Check whether a one-shot opportunity tied to that conversion can send the exact registry reply before the new backing advertisement. Preserve the reply's own receive return, protected TFTP/WRQ turns, actual QRTR credit and suitable remote intent, same-flow ordering, advertisement before positive ACK, consumption on WouldBlock, quarantine on uncertain write and normal fallback. Do not drop the ACK-polarity/backing requirements or enable an unrestricted drain.

No new Pixel command is needed to interpret this result or run that offline replay. No device command was executed while recording it.
