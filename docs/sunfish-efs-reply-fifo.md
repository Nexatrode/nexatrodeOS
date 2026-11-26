# First modem_efs ERROR FIFO visibility

IMG_9774 confirms first/second modem_efs request order after the held-list fix. The first ERROR1 publishes before the host observes client close, but its matching GLINK completion is processed7.859491s after publication. That panel has no peer TX-tail crossing or exact RX_DONE visibility clocks. It cannot say whether the modem read late, generated the receipt late, or the host processed already-visible RX late. The original final ACK and correctly formed ELF exchange completed, without WLAN/WLFW progress.

`sunfish-efs-reply-fifo` is an observer-only extension of that arrival-order image. It reuses `RegistryReplyWindow` / `ServerCheckRxWindow` and their32-sample cursor history for the exact **first accepted modem_efs RRQ's ERROR1**. The existing normal writer must change that retained record from queued to published. The observer requires kind2, opcode5/code1,19 reply bytes and a real reservation. It copies the exact sequence/remote intent, starts once after the known checked publication, and uses the cached validated cursor pair from that writer. It cannot arm on an unrelated later DATA publication or on the second EFS request.

Subsequent normal `Ring::sample` calls feed the existing observed TX consumption, RX-head advance and unread count to this private history. No new `sample`, read, cursor observation, validation, write, interrupt or scheduling call is introduced. Existing optional clock callbacks bracket the same validation and can perturb timing. Peer-tail crossing is an upper bound from normal sampling of transport consumption, not proof of remote application acceptance.

Before normal Session ingestion the observer copies only the first record's reservation/phase flags. After successful ingestion it records the first same-record close transition in that RX-head history. It freezes completion only when the same reservation's phase changes to done with the originally published remote intent and a normal8-byte command6/14 header carrying that intent. Foreign completions cannot freeze it. Normal errors remain terminal; capture does not authorize any work or alter the checked consume operation. Its processing observation precedes the existing RX-tail consume, so a later consume fault must still be read with the runtime error state.

A late history entry can be overwritten. The panel reports eviction count and leaves unavailable bounds unknown instead of inventing an arrival time. A receipt visible in the same sampled head advance as close is reported visible-at-close even if it appears later in FIFO order. Unknown clocks remain unknown, independently of the phase flags. Reset clears the observation with the normal transport lifecycle.

## MODEM EFS REPLY FIFO

The original ACK, ELF/body/credit, IPA/PD/WLAN/WLFW summaries, first ruleset block and both EFS blocks are retained from EARLY TFTP ARRIVAL ORDER. Rows13–17 replace the second ruleset display only; its original capture remains retained internally.

- Row13: original reservation sequence low/high, remote intent, observer-arm clock.
- Row14: peer TX-tail crossing clock, RX bytes visible at arm, RX bytes visible when the exact EFS close is processed.
- Row15: last observation that excluded the matching receipt, first observation containing it, and bytes ahead of the receipt in that first visible sample. Bounds refer to the whole matching8-byte GLINK record.
- Row16: receipt visible-at-close (0/1/unknown), capture finished, history evictions.
- Row17: first EFS response opcode/code/length and whether this exact FIFO watch armed. An unexpected response leaves tracking off.

The existing first EFS block at rows18–22 still gives receive/dispatch/queue/publication/completion/close on the same clock. All clocks are host observations. An ERROR1 is a negative read response; it does not call for an OACK or client DATA.

Offline checks cover prompt tail consumption followed by a late receipt, unknown clocks with history eviction, and the actual Owner's ERROR publication, known tail advance, unrelated reply/receipt, exact close and original matching completion. Expected failing controls remove arming or completion capture. Full protocol/IRQ, fault, backpressure, close, grant/credit and arrival-order regressions remain required. A new physical framebuffer render checks all fields, both buffers and clockless/absent cases. Ordinary early exchanges must preserve complete ordered Fake I/O and wire bytes with the observer on/off; that test is not a handset latency benchmark.

The next question is whether the first ERROR was consumed promptly and its receipt appeared late, or whether the host delayed already-visible RX. Keep the scheduling fix and FIFO/grant/credit/ownership checks unchanged. The private ready profile is `.cache/sunfish-native-wifi/private/native-efs-reply-fifo-v1`. No device command is automatic and no WLAN success is claimed.
