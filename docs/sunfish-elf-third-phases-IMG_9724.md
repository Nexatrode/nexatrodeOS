# IMG_9724: ordinary transmit capacity blocks ELF

Settled **ELF THIRD QUEUE PHASES**, image `e60f2c7334c8652d2ec34f7f6da354f4b0f3f8484050cac928d075b89354511d`, source `9cf72d7b2509260add4eadf29c1e77b2033ecbe2`. Raw photo/decoded rows are retained in the private profile's separate `device-result/`. Delivered verification manifest unchanged. Runtime error zero.

## Actual rejection: no normal TX slot

First rejection `0155BAC7 / 00000102`, latest `01702956 / 00000602`: sequence1/6, **reason2**. In the pinned `queue_reply_with_ipa_token`, reason2 occurs only when `free_tx_slot()` returns None. That method searches the eight normal transmit records for `len == 0`; the reserved extra OACK slot is excluded. Thus all eight ordinary TX slots were occupied at both observed checks.

Both named older registry owners are transaction3/intent4 and transaction4/intent0x10, type4, flags **3B** at both checks. This means sent, fully published, local QRTR condition satisfied, remote slot owned, reference token retained, no GLINK completion. Gate mask1 only records two owners; it is permitted by the current third-reply policy. Both older replies had published before the first failure. The same-peer gate passed; this was not the former two-reply rejection or a wait for their GLINK receipts. Reason2 occurs before reference-credit reservation, so it does not prove those credits were available at each failed check.

Seven attempts imply six failed checks and a successful seventh. Only first/latest rejection reasons and occupants are displayed; do not infer all intermediate reasons or the other six TX packets. The two rejections are 1,732.239 ms apart. Queueing follows the latest one by 202.709 ms. These snapshots do not identify which slot freed first or prove capacity persisted at every instant between checks.

## Remaining delay partition

ELF receive/queue/publication/Owner completion: `01477303 / 0173412B / 0174AC0C / 018032F7`.

| Phase | Displayed-age difference |
| --- | ---: |
| Acceptance → receive-pass end | 893.141 ms |
| Receive end → registry entry | 12.232 ms |
| Registry entry → ELF selection | 12.179 ms |
| ELF selection → queue | **1,953.272 ms** |
| Entire acceptance → queue | **2,870.824 ms** |
| Queue → publication | 92.897 ms |
| Publication → Owner completion | 755.435 ms |

Direct phase fields are 893.141 / 12.231 / 12.179 / 1,953.271 ms; one-microsecond differences follow independent integer conversion. Admission is `01734125`, with both older owners and no earlier receipt. Queued-turn flags1F. Publication precedes exact Core receipt3 `0175878F` by **56.195 ms**, and receipt4 `0176DA48` by **142.908 ms**. Matching ELF Core receipt is `017FEB83`; later Owner completion is a different boundary. Both older replies remain legitimately owned until their real receipts.

## I/O counters have an important scope limit

Acceptance→receive-end recorded OBS/DESC/READ/OTHER are **168.251 / 168.356 / 85.123 / 470.651 ms**, total **892.381 ms** within the 893.141-ms phase. This nearly accounts for that receive-pass tail with measured I/O work rather than unexplained scheduling.

Receive-end→queue recorded values are **266.190 / 266.488 / 98.088 / 679.484 ms**, total **1,310.250 ms**. These counters are enabled only inside Runtime `service_owner`'s normal dispatch, then disabled before registry service and its queued-ELF dispatch later in `service_pass`. They therefore cover a subset of elapsed work, not every I/O operation between snapshots. Do not assign the unaccounted remainder to pure CPU or idle time.

The publication I/O row is all zero although the reply took92.897 ms to publish. Source confirms the normal queued-ELF handoff runs outside that enabled timing scope. Zero here cannot establish no validation, no I/O or a free write. The previous writer trace independently measured nonzero writer work; no performance check should use these zeros as total publication cost. A future display should label this scope explicitly or show unavailable where coverage is absent, without weakening checks.

## Startup outcome and next target

IPA complete1, PD7FFFFFFF, **WLAN RRQ0 / WLFW0**. Original final ACK `00D90C33`, one MCFG write/one byte/zero full reads. MCFG peer4A/stageBF/result0/options0F repeats a completed normal response path; opcode/error bytes are not on this projection and should not be invented from this screen alone.

This capture removes an ambiguity: the first/latest admission wait is **the global eight-slot TX pool**, not either older registry reply awaiting RX_DONE. Next examine the other six owners and the first actual slot retirement, reusing the existing transmit-slot diagnostics and saved pool-pressure notes. Do not simply enlarge the pool or retire a published buffer without its exact protocol completion. Prior port0x49 slot screenshots are a different request/boot and cannot identify these current ELF occupants. No repeat of this image is needed to establish this result, and no additional Pixel command is needed for the offline review.
