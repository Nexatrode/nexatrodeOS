# IMG_9705: credit and later registry turns committed

The settled **REGISTRY CREDIT TURN** panel is from source `9280fa90`, image SHA256 `b42b902f36139f97f160beac588e4fb525875ee6a1011393141d6f98f1076326`. Original HEIC, readable preview, all 27 fields and arithmetic are preserved privately in `native-registry-converted-credit-v1/device-result`. The original delivery manifest/source snapshots remain unchanged. Runtime error is zero.

## The changed path actually ran

ENTRY FLAGS `0x1A8A3F` retain the valid ordinary entry, positive grant, matching unadvertised backing/token, live transmit, fitting remote intent and converted registry candidate. Gate 10 still reports the ready-credit exclusion of registry priority. The new bit 19 shows the bounded credit candidate; bit 20 confirms agreement of the live transmit length with its queued publication size.

CREDIT NODES `00010000` and PORTS `40030009` identify QRTR credit from **host 1:0x4003 to modem 0:9**. CHOICE `FFFF0009` is GLINK DATA without a QMI transaction; its parameter 5 is the selected modem GLINK intent, not a QRTR port. WRITE RESULT 1 and publication-mask bits 17/18 establish that the borrowed credit callback was attempted and committed. EXIT CAUSE 5 marks its attempt before the checked writer; it is not the exact wire-publication timestamp.

TX4 MASK `0x63FAC` has bits 2,3,5,7,8,9,10,11,12,13,17,18 set. Besides the credit attempt/commit, the registry borrowed-attempt/commit and conversion-bound attempt/commit markers are present. With two recorded arms and the consumed first credit opportunity, this corroborates a later converted registry turn publishing the reply. TX4 SNAP STATE 2 confirms the counters froze at full publication. The first-only trace does not timestamp the second arming, and GLINK/QMI application acceptance is not established by publication alone.

ARM LEN SENT `00E00000` records **224 bytes, zero already sent** at first arming. The independent consistency bit is set in this boot. This supplies a valid nonzero size for this reply; it does not retroactively explain the preceding boot's zero field.

## Same-clock timing and comparison

| Interval / count | IMG_9705 | Previous bdc954d9 |
|---|---:|---:|
| Queue to first arm | 3,157.309 ms | 3,171.257 ms |
| First arm to full registry publication | **340.310 ms** | 3,027.763 ms |
| Queue to full registry publication | **3,497.619 ms** | 6,199.020 ms |
| Publication calls | **7** | 33 |
| Arms | **2** | 10 |

The measured queue wait is 2,701.401 ms shorter, with 2,687.453 ms of that difference after first arming. These are separate boots, not a controlled causal benchmark; the new markers independently prove that the policy was exercised on hardware.

Within IMG_9705, first arming to publisher entry is 18.337 ms, then 0.018 ms to selection. Selection precedes the advertisement observation by 91.586 ms. The checked credit write committed before that advertisement, but this panel does not record its exact publication or modem-read/receipt time.

The matching backing advertisement is `0194F7E0`, followed by positive ACK `019651D6` **88.566 ms later**. Registry publication follows that ACK by **141.803 ms**. Advertisement-before-ACK therefore remains intact. Roughly 3.157 seconds, about 90% of the remaining queue wait, precede the first arming; that interval cannot be assigned to particular service work, receive returns or writer checks from this panel alone.

## Startup result and next offline target

IPA COMPLETE is 1. WLAN firmware RRQ and WLFW server counters remain zero. Transaction 3 completion visibility and ELF timing are omitted from this panel, so their improvement cannot be inferred, nor can this be called working Wi-Fi.

The bounded credit turn and subsequent registry publication are concrete hardware progress. Next review the remaining transaction 4 queue-to-first-arm interval using saved source, traces and existing loop/I/O observations: when the reply first became eligible, whether its request RX_DONE or full-slot conversion still waited, and which work ran before that point. Also check the subsequent ELF admission/publication sequence before assuming that faster transaction 4 delivery is the last startup barrier. Preserve the verified credit/grant ordering and all FIFO/fit/credit boundaries.

No additional Pixel command is needed merely to interpret this result or start that offline review. No device command was executed while recording it.
