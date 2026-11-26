# Preserve held TFTP request arrival order

## Offline replay of IMG_9773's ordering delay

IMG_9773 accepts the first modem_efs RRQ before the second, but dispatches the second first. The first is not dispatched until after the client closes, then waits behind the other reply's flow. The photograph does not retain the held-array positions, so it cannot itself prove which slot each request occupied.

The actual Owner replay reproduces a sufficient mechanism without changing FIFO validation or fabricating modem events:

1. Accept a ruleset RRQ into the first host held-request position.
2. Accept the older modem_efs RRQ into the next position.
3. Serve and normally finish the ruleset request, leaving a lower hole. Its reply uses the real queue/publication/receipt path.
4. Accept a second modem_efs RRQ from the same peer. Legacy intake reuses the first empty position, ahead of the older request.
5. The per-peer server scans held positions from the beginning, so it dispatches the newer request first. The older request then sees FlowBusy until the newer reply's actual completion.

The replay with the previous feature list prints `older_selected=false`; with arrival ordering enabled it prints `older_selected=true`. Both schedules retain normal same-peer FlowBusy, normal publication and both matching real GLINK completions. This establishes an implementation ordering defect and a plausible explanation for the photo, not a reconstruction of its unrecorded array positions or its elapsed times. A synthetic replay cannot promise a handset deadline.

## Bounded change

`sunfish-tftp-arrival-order` stably compacts the existing bounded Owner array before appending a non-coalesced incoming TFTP request. It takes and reinserts only existing opaque host handles, in increasing array order, for at most `MAX_REQUESTS` entries. At every move the destination is at or before the source; it cannot overwrite an unvisited entry. The new request goes after all remaining entries. No timestamp, wrapping sequence number, additional queue, allocation or receive-buffer copy is introduced.

The change runs after the existing exact-WRQ coalescing decision. Duplicate handling and the full-queue rejection remain in their original paths. The dispatcher still retries retained replies first and skips peers with a retained response, so an unrelated eligible peer can still progress. This fixes lower-hole overtaking; it does not remove every source of dispatch delay, prioritize these two paths over other traffic, or alter the checked writer's arbitration.

The array contains handles, not GLINK receive slots. Their actual Core datagrams, generation identities, registry state, pending replies, transmit reservations and remote intents remain in place. Finishing a request still locates its handle and performs the original checked release. MCFG and firmware close code computes its masks/snapshots from the current Owner array in the same serialized call; no close mask spans another intake/compaction. The next normal receive pass refreshes its peer snapshot. No transport lifecycle, shared cursor read, FIFO validation, grant advertisement/ACK order, QRTR credit or published-packet retirement changes.

## Offline checks and next device observation

The actual Owner tests cover the lower-hole reversal before/after, same-peer backpressure until real receipt, unchanged payload access through the moved handle, MCFG close-mask cleanup preserving the unrelated request and committed file, and exact WRQ coalescing preserving the original handle before and after compaction. Expected failing controls remove compaction or reverse its packing direction. Ordinary early negative-reply exchanges without a hole retain the same complete ordered Fake I/O and wire bytes with this feature on/off; that comparison is deliberately not a claim that hole-reuse schedules remain identical. The inherited fault, capacity, grant, credit, early return, firmware-close, registry and runtime suites remain required. Disabled production source normalizes exactly to `4f8805e1`.

The new **EARLY TFTP ARRIVAL ORDER** image retains the same four early-reply entries and original final-ACK/ELF/WLAN summaries described in [the startup capture](sunfish-early-startup-prerequisites.md). Its title distinguishes the scheduling change from the preceding observer-only image. The next question is whether the first modem_efs request is dispatched and admitted before its later same-peer request, and whether either negative reply completes before client close. The handset outcome may still expose retained-reply pressure or late receipt visibility. GLINK RX_DONE proves buffer return, not when the remote application accepted the error. Zero WLAN/WLFW remains a possible outcome; this is not a demonstrated final WLAN fix.

No Pixel command is automatic. The private ready profile is `.cache/sunfish-native-wifi/private/native-tftp-arrival-order-v1`; final verification binds committed source, native ELF, loader, capability archive and all evidence while preserving earlier delivered artifacts. The pinned SDK is reused, with the native PD rebuilt and actual packed payload reproduced and audited.
