# IMG_9729: close observed, cleanup not committed

Settled **MCFG CLOSE CLEANUP**, image `4ff66ce38eab9215b24ed47f058993ec465d2db4ff133eb3c1f8f665b1d59f27`, source `b1483e6180786802a6af7c24ca3440fec80cbed4`. Raw photo and confirmed scalar decode are in the delivered profile's separate `device-result` directory. The image manifest is unchanged.

Runtime error is `0x73`, the outer `service_owner` error wrapper. The selected panel omits the nested detail and Owner error, so it does not distinguish a cleanup preflight failure from a runtime path that attempted more work before resolving the paused close.

`CLOSE OBS COMMIT HELD RETAINED = 1/0/0/0`. `UNSENT/KEPT = 0/0`, and last successfully cleaned peer/time are absent. No cleanup committed. These zeros cannot establish that no transport records were outstanding: cleanup counters update only on successful commit.

The original final ACK completed. The exact ELF host QRTR credit was requested, published and completed on intent 4. The ELF reply was published, but its matching completion is absent. Live IPA flags are `0x2e5`, with one registration, one driver request and two committed publications. INIT message `0x35` remains pending; accepted response 1, failed 1, timeout 0. These retained values were captured after the terminal runtime failure, not evidence of a modem timeout. IPA COMPLETE is 0 in this boot.

The filtered journal has 14 records, none omitted or malformed. It includes MCFG WRQs from port `0x49`, an OACK from transfer port `0x5005` to that peer, and its DEL_CLIENT. No first-after-completed-ELF MCFG request was selected because ELF never completed. RAM MCFG writes 0, file absent, read completions 0. WLAN RRQ and WLFW service counts are both 0; PD query state is unavailable (`FFFFFFFF`), so this boot does not establish a new valid UNINIT query result.

Offline source review found uncovered close-handoff paths: `service_pass` calls `dispatch_queued_elf_reply` and proceeds directly to RMTFS dispatch without resolving a close that the nested writer/receive pass may have observed. `dispatch_registry_handoff` can likewise poll a close and then retry registry work before returning to the outer resolver. The direct-router cleanup test did not exercise these chained service paths. These are concrete coverage gaps, but the photo alone cannot prove which path produced this phone's error.

Next: reproduce a close arriving in these nested paths using actual Owner/Router fixtures; preserve the pause until exclusive cleanup, then resume ordinary services. Add nested error evidence if needed. This result needs offline investigation before requesting another cold boot. It does not demonstrate successful cleanup or WLAN progress.
