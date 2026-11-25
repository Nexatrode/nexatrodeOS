# IMG_9722: third ELF admission worked; WLAN unchanged

Settled **ELF THIRD REPLY WINDOW**, image `91e35640522ac69f0ef817a81ab746431f125bb4b06312082dd5381602238259`, source `8818c7accd3793e040da9b4d100eeaaf462e219e`. Raw user photo and decoded rows are retained separately in the private delivery profile's `device-result/`; the verification manifest remains unchanged. Runtime error zero.

The admission row is limit **3**, sequence **7**, older-owner count **2**, last prior receipt **FFFFFFFF**. Thus the new third-reply capability was used before either older receipt. Exact ELF receive/queue/publication/Owner completion are `01476F02 / 01733A93 / 0174A55F / 01802C36`.

| Interval | Current boot |
| --- | ---: |
| Receive → queue | 2,870.161 ms |
| Queue → publication | 92.876 ms |
| Publication → matching Owner completion | 755.415 ms |
| Receive → matching Owner completion | 3,718.452 ms |

Core receipt3 `017580E8` and receipt4 `017603E2` occur **56.201 ms and 89.731 ms after ELF publication**, respectively. Admission itself is `01733A8E`. Neither older transmit was retired to make room. ELF completed later than both receipts, but publication and admission no longer required them.

Queued turn flags **1F** and entry/end `01733A95 / 0174A560` put publication inside the turn. Exactly one timed writer pass, no omissions, no outstanding active entry, no clock regression. `01741633 / 0174A553` spans `8F20` = **36.640 ms**. Direct queue-to-first gap `DB9F` is 56.223 ms; last-pass-to-publication is 11 us (displayed ages differ by 12 us through independent rounding). Record `09430005 / 2` identifies fitting/exact-selected registry reply transaction 5, remote intent 2; blocked and uncertain flags are absent. Publication calls1/maskA0/bytes58, selected turn gate7=1, no-pending-grant gate0=1. Other turn/grant gates and ACK counters are zero. No max interpass gap is available for one pass.

Compared with IMG_9719, receive-to-queue is 356.116 ms shorter and total completion 225.500 ms shorter. These are separate boots with varying arrivals, not a controlled speedup. The ordering result is stronger evidence than the elapsed-time comparison: third admission and publication actually preceded both old receipts.

IPA complete1, PD state7FFFFFFF, **WLAN RRQ0 / WLFW0**. Original final ACK completed at `00D90C57`; MCFG remains one committed write/one byte/zero full reads. Post-ELF MCFG peer4A/stageBF/result0/opcode5 repeats the normal missing-file path identified by pinned source in [the offline review](sunfish-elf-third-reply.md). It does not establish a WLAN startup failure caused by MCFG.

The new overlap works but does not remove the remaining **2.870161 seconds before queueing**. The writer itself again publishes promptly once queued. Next reuse the already-recorded receive-end, registry-entry/selection, first/last admission blockers, older publication timings and phase I/O totals under this third-reply policy. The older phase screenshots used different two-reply policy and cannot assign the current residual interval. A panel-only projection can distinguish backlog/dispatch time, an older not-yet-published reply, and other queue checks without new hot-path reads or another writer-priority change. The supplied settled photo is sufficient; the user may return to LineageOS while this is prepared.
