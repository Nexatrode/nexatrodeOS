# MODEM EFS EARLY ERROR: control write followed by RX veto

Two settled photos, `Photo on 11-25-26 at 11.21 PM.jpg` and its `#2` companion, correspond to the delivered `native-efs-early-error-v1` profile, revision `6b1d0beacb56373354849296998908b6946215d3`, image SHA256 `cb07fe8fc23e83a85a4acebf0dd80d1f2cff6d95d4105a9367afc9923d515eb6`. This is a panel/command association, not an independently measured handset image hash. Originals, readable crops and decoded values are in that profile's separate `device-result`; delivered artifacts and manifest remain immutable.

## What the bounded turn actually did

`EARLY CALL WRITE RX BLOCK = 1 1 1 0`, with `STOP = 2`. The opportunity ran once, committed one ordinary Session write, then yielded to pending receive work. It was not blocked by transmit capacity. From `qcom_fifo_efs_early.rs`, a DATA or any other non-control operation would end the group with STOP1. Therefore the one committed write was one of RX_DONE6, receive-intent advertisement5, or grant ACK8; it was **not** the target EFS DATA publication. The command array exists in memory but is not projected on this panel, so the photo cannot distinguish those three commands or identify the pending RX record. Do not label the counted write as the EFS ERROR itself.

The normal continuation subsequently published the first EFS ERROR. The fresh-RX veto and one-shot latch behaved as intended. Unread8 at the later FIFO-watch arm is not a contemporaneous sample of the earlier turn veto and cannot identify its cause.

## Exact first EFS timeline

Peer0x22, reservation0x13, remote intent0, flags0xbf. RRQ74 bytes; reply ERROR1,19 bytes, local port0x5003. Ordinary queue admission succeeded on the first attempt with no FlowBusy or NoCapacity. No client ACK/ERROR was retained; this is not proof of application acceptance.

| Event | Same-clock hex microseconds |
| --- | --- |
| Request accepted | `00E0D345` |
| Server dispatch | `00EF0F64` |
| Reply queued | `00EF5767` |
| Reply published | `00F537A7` |
| FIFO watch armed | `00F537B0` |
| Peer tail crossed reply | `00F54FC5` |
| Last sample excluding matching receipt | `016BF446` |
| First sample containing matching receipt | `016C3E6D` |
| Matching receipt processed | `016EBF6D` |
| Client close processed | `01072C9C` |

The host spends 932.895ms acceptance-to-dispatch, 18.435ms dispatch-to-queue, and 385.088ms queue-to-publication: **1,336.418ms acceptance-to-publication**. The prior IMG_9775 interval was 1,492.993ms, so this boot is 156.575ms shorter, largely in queue-to-publication. These are one boot each, not a controlled causal result. In particular the borrowed group yielded without sending the EFS reply, so the shorter total is not evidence that the group directly published it sooner. The dominant approximately 933ms pre-dispatch wait remains.

The modem's tail crossing was observed **6.174ms after publication**, compared with 6.200ms previously. Shared-FIFO consumption is prompt; this does not prove remote TFTP acceptance. The receipt remains absent 7,775.361ms after that observed tail crossing, first appears 18.983ms later, then is processed 164.096ms after first visibility. Total publication-to-processed-completion is **7,964.614ms**, versus 7,844.098ms previously. The long pre-visibility gap persists; this experiment did not resolve it.

Publication precedes host close processing by1,176.821ms, but the matching receipt is not visible at close and first appears 6,623.697ms afterward. RX unread at arm8, at close72; matching receipt has72 bytes ahead. Those records remain unidentified by this panel. History evictions28 do not erase the retained numeric bounds for this exact receipt; watch finished1.

## Other startup state and next question

The second EFS response is reservation0x1a,12 admission attempts,10 FlowBusy and1 NoCapacity before success, then real completion. Original peer0x1f sent DATA; its final ACK is phase4 and completed. ELF body match1, credit mask7 and completion present. Runtime error/detail0, no pending close, cleanup stage6, IPA complete1. PD remains UNINIT, **WLAN RRQ0 / WLFW0**. No new WLAN milestone occurred.

The saved Linux sequence still places the early ruleset/EFS exchanges before WLAN and the later MCFG/digest after WLAN. That host-side ordering does not supply the modem's private launch decision or application deadline, and GLINK RX_DONE remains buffer return rather than application acceptance.

The next offline investigation should follow the actual accepted request through its approximately 933ms service wait, and the bounded group's control/RX handoff. A fresh-RX veto is not permission to bypass FIFO checks. Before another probe, reuse the saved Owner/service-loop evidence to determine what is already known; the present photo cannot recover the unpublished command array or pending header. Do not claim that another boot of this same image will reveal them. No additional Pixel command is needed to interpret this result.
