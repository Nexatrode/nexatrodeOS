# IMG_9812 — prompt reply consumption and receipt, earlier publication wait recurs

The settled REGISTRY RECEIPT FIFO photograph is associated with source `100b2032994cac4288b073907a16c217eaeaafb0`, image SHA256 `c6567ff49c7eac1a28ec35ccd8f11cebce5eb520d852e14174e262b5ed600805`. Original HEIC, readable JPEG, manually transcribed physical rows, decoder, arithmetic and hashes are retained separately under `.cache/sunfish-native-wifi/private/native-registry-receipt-fifo-v1/device-result/`. All 116 delivered artifacts were reverified unchanged. No device command or image build was performed for this analysis. Times below are milliseconds converted from hexadecimal runtime microseconds.

| Interval after publication | TX3 | ELF |
| --- | ---: | ---: |
| Peer-tail crossing observed | 6.203 | 6.160 |
| Matching RX_DONE first visible | 6.203 | 6.160 |
| Normal Core decoding of matching RX_DONE | 25.840 | 25.746 |
| Owner completion | Not separately displayed | 44.037 |

TX3 publication/tail/Core are 01E10810/01E1204B/01E16D00. ELF publication/tail/Core/Owner are 01EA0496/01EA1CA6/01EA6928/01EAB09B. TAIL and VIS are observed in the same first post-publication sample for each reply. This does not measure an exact simultaneous modem read/return instant. Both windows are finished, have zero history evictions and zero bytes ahead of the matching receipt. ABS is unavailable because there was no retained earlier active-window sample start for the first advance; it is not an error or evidence of lost packets. ELF armed at 01EA048F with zero unread bytes and actual remote intent 0. Its arm observation precedes the separate Owner publication observation by 7us, consistent with their different sites.

Visible-to-Core intervals are 19.637ms for TX3 and 19.586ms for ELF. ELF Core-to-Owner adds 18.291ms. These are elapsed host processing/observation boundaries, not per-function CPU measurements. Neither completion has a preceding FIFO prefix in its captured appearance sample. The prior IMG_9811 publication-to-completion waits of 4,119.376ms and 1,803.327ms did not reproduce; this capture therefore cannot retrospectively assign those waits to the modem or host.

## The dominant delay is before publication in this boot

TX3 queue01208AC1→publication01E10810 takes **12,614.991ms**, compared with 260.962ms in IMG_9811. Frozen snapshot2, calls80=128 and maskE0AC reproduce the earlier IMG_9804 control-pressure pattern. The first-applicable priority counters are 0 cooldown, 9 protected TFTP, 105 pending intent ACK/control, 13 own receive-buffer return pending, zero missing/other/credit exclusions and 1 selected registry turn. These are selector observations, not 105 distinct ACK packets.

Borrowed-grant first-failing counters independently total 128: 19 no pending ACK, 36 negative ACK, 37 no unadvertised backing and 36 no ACK-yield/conversion/ready-candidate opportunity. Later gates are all zero; earlier failures prevent interpreting those zeroes as proof that fit, own return or credit were simultaneously ready. ACK progress records 37 sent, 0 blocked, 34 returns ahead and 36 advertisements ahead. These counters must not be substituted for IMG_9804's 32 returns or assumed to partition every writer call. No converted-grant snapshot was retained and neither ready DATA attempt/success bits19/20 nor paired credit bits17/18 ran for TX3.

ELF receive014AD84B→queue01E91653 takes **10,370.568ms**; queue→publication01EA0496 takes **60.995ms**; total receive→Owner completion is **10,475.600ms**. The clean ELF maskA0 and one writer call do not exercise the paired turn, unlike IMG_9811's mask001E27AC. Completed writer span36.614ms plus queue-to-first24.363ms and last-to-Owner-publication18us reconcile the queued interval. Maximum inter-call gap is unavailable with only one call, not a hidden failure.

ELF has one recorded peer admission rejection and zero recorded transmit-capacity/flow/credit/capability rejections. Last failure01E349D1, packed sequence/reason201, one owner and gates18 retain a transaction4 registry reply whose flags28 indicate QRTR credit/reference flow but an unpublished state. ELF queues380.034ms after that failure snapshot. Only one failure was recorded: do not call all10.371 seconds a continuously attempted peer rejection. RX-end015BB7D9 and registry-entry015BE7A6 precede selection01E90262; the panel records21 preselection calls and no omitted turn records.

## Startup status and next offline question

Live IPA flags1F, no pending message, exact ELF host-credit mask7, ELF body match1, original final ACK00093E33 and historical IPA complete1 remain established. WLAN firmware RRQs and WLFW servers remain0, PD state7FFFFFFF, one RAM MCFG write/file byte and no full firmware read completion. Both retained digest replies have flagsBF and actual completion before observed close; two retained reservation IDs21/22 do not establish every outcome among9 total/7 omitted digest requests.

This boot answers its own receipt timing question, but it does not prove the previous late-receipt case fixed or reproduce the previous successful paired turn. The earlier TX3 publication pattern is already in the notes and source: use the exact eligibility/retry replays to investigate that retained path before choosing another handset probe. No additional Pixel run is required merely to interpret this panel.
