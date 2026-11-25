# IMG_9762: cleanup succeeds; MCFG observer has a cancel/reuse alias

Settled **MCFG MISSING CLOSE**, source `f0eed964c783153639962f8192e93f650e2036aa`, image `d31b4140211133a9485cb8586d5b5f07e0110d622273ab78c81757dcf6372c7d`. Raw photo and decoded rows/journal are in the delivered profile's separate `device-result`; the delivered manifest is unchanged.

The Pixel passed the previous runtime stop. Runtime error/detail are 0/0, no MCFG close is pending, and stage is 6. Five closes were observed and all five cleanups committed. Cleanup canceled one held request and one unstarted transmit, discarded no retained replies, and preserved two reply/credit records at cleanup. KEPT2 is cumulative historical retention, not the current outstanding count. Last successful peer is `0x4d`, KIND1 (RAM read); this does not reveal the classification history of all five cleanups.

ELF receive/queue/publication/completion: `014956E3/01671852/016B0BC5/01787C7B` (3.089816 seconds receive to completion). Body matches reference. Its independent host credit was requested/published/completed on intent0, and was published but not yet complete at ELF reply publication. The original final ACK completed at `00D84169`, 10.500882 seconds before ELF completion. Live IPA flags `1f`, registration1/driver1/commits4, no pending message, accepted response1, failed0/timeout0; IPA COMPLETE1. These are substantive completed startup milestones in this boot.

The first accepted request after completed ELF is a 73-byte MCFG RRQ from `0x4a`. At dispatch the MCFG file was absent; server result0 formed TFTP ERROR opcode5/code1, 19 bytes, transfer port`0x5006`. Later totals show one MCFG write and a one-byte file; no MCFG read completed. First other-path request is category8, exact `mbn_hw.dig`, 93-byte RRQ from `0x4f`; later retry/close peers `0x50/0x52` appear. The filtered journal has43 records, retains16, omits27, and reports no malformed record or target queue wait.

WLAN firmware requests0, WLFW services0, valid PD state`7fffffff` (UNINIT). Successful cleanup has not established WLAN firmware startup.

## MCFG timing cannot yet be trusted after cancellation

The scalar MCFG record shows flags`bf` and publication/completion after its close: RX`017D8EE6`, queue`0187E1BE`, close`0188DC56`, publication`019099AC`, completion`0190F4F1`. Those publication/completion fields are not sufficient evidence that this particular response was sent or completed.

An isolated actual Core test cancels an unstarted traced MCFG ERROR1 through `commit_mcfg_close`, then publishes a different peer's ACK through the reused transmit slot. The current observer matches only the slot index at publication, then the newly captured remote intent at completion. It consequently marks the canceled original as published/done and emits a synthetic completion naming the original peer. The transport correctly completes the real foreign packet; the defect is in diagnostic identity.

Private reproduction: `.cache/sunfish-native-wifi/private/mcfg-cancel-observer-IMG_9762/replay.log`, case `img9762_canceled_mcfg_trace_follows_a_foreign_reused_slot`. One characterization test passed. Production Rust files and images were unchanged. This proves the observer vulnerability, not the exact identity of the phone's reused slot hidden among27 omitted records.

Next: invalidate the traced slot on cancellation and bind later publication/completion observations to the original record identity; test cancellation and reuse offline before interpreting these MCFG timings. Preserve the now-successful protocol cleanup and startup behavior. Continue comparing the post-ELF sequence with saved Linux evidence. The existing exact `mbn_hw.dig` no-reply behavior gap remains documented in `sunfish-post-elf-late-reply-review.md`, but Linux requested WLAN before that digest path, so it is not established as the initial WLAN trigger. No repeat of this unchanged image is needed.
