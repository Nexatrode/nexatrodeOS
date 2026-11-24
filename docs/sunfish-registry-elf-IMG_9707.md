# IMG_9707: ELF admission waits for the older registry reply

The settled REGISTRY ELF ADMISSION panel is from source552ad335, image SHA256 `b0093080d03debb2aabe99a3fec782a9094f44f1f6181f1c22f933fa9f2e0ca9`. Original HEIC, preview, all27 decoded fields and arithmetic are saved privately in `native-registry-elf-admission-v1/device-result`. The delivery manifest is preserved unchanged. Runtime error0; repaint deferrals8; IPA COMPLETE1; WLAN RRQ/WLFW0.

ELF was already accepted and selected: receive→selection294.613ms, followed by4,762.212ms selection→queue. Nine failed admissions were same-peer gate failures; full-slot and reference-credit failures were0. Last reason1, last matching peer transaction4, flags0xB. Global slot counts0x00060107 mean7used,1unpublished,6published-awaiting-GLINK,0GLINK-done-awaiting-QRTR. These counts are historical/global, and the last matching peer is not necessarily the unique blocker.

Transaction3 matching RX_DONE precedes ELF queueing by203.120ms. ELF queues74.592ms before transaction4 matching RX_DONE, confirming the saved IMG9699 counterexample to an unconditional wait-for4 rule. The timing and actual gate failures support the two-outstanding same-peer window holding ELF until one earlier reply retires; they do not justify retiring an owned buffer at publication or increasing the window on a guess.

| Interval | Elapsed |
| --- | ---: |
| Transaction3 publication → matching RX_DONE processing | 8,061.110ms |
| Transaction4 queue → full publication | 583.460ms |
| Transaction4 publication → ELF queue | 4,190.982ms |
| ELF receive → queue | 5,056.825ms |
| Transaction3 completion → ELF queue | 203.120ms |
| ELF queue → transaction4 completion | 74.592ms |
| ELF queue → publication | 117.386ms |
| ELF publication → completion | 774.987ms |
| ELF receive → completion | 5,949.198ms |

The583.460ms transaction4 publication wait reproduces the repaint improvement from IMG9706's585.028ms. This does not imply the remaining older-reply receipt delay is solved.

The exact old receipt was last observed absent at01905EAA and first visible at0190A870,18.886ms apart. The first-visible snapshot has96bytes ahead. Visibility→matching processing took643.857ms, then ELF queued203.120ms later. Transaction3 publication→first-visible receipt was7,417.253ms; publication→last-absent was7,398.367ms. These are validated FIFO observations, not exact remote emission/application times. The96bytes are not identified by scalar history. This panel does not show the predecessor's retained peer-tail timestamp, so it cannot distinguish delayed modem consumption of the original reply from delayed receipt after consumption.

Next use the already retained exact predecessor peer-tail, FIFO visibility/history-loss and converted-grant observations alongside completion and ELF queueing. This addresses the dominant8.061s interval without skipping any FIFO records or weakening grant/credit checks. The first converted-grant snapshot is not a complete history of every grant. No additional driver capture hook or policy change is needed to display these fields.

The user requested continuing only through a built/verified image ready for cold fastboot. No device command was executed while recording this result.
