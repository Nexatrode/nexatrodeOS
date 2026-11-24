# Settled POST ELF STARTUP result — November 23, 22:06 photos

The two supplied Desktop photographs show the same settled panel from image commit `45fbc150`, profile `native-post-elf-startup-v1`. Originals and selected verified transcription are saved separately in the profile's private `device-result` directory. The delivered manifest and image remain unchanged. Runtime error is zero.

## Original transfer now completes before ELF

The original WRQ peer is **0x1F**. Its final ACK phase/block is `00040001`: phase 4, matching GLINK RX_DONE, for final block 1. ACK publication is `00D8A019`; matching completion is `00D9049C` (14.222492 seconds), followed by exact peer close at `00DA5E97`. The separate first-completed-write row is count 1 / peer 0x1F / flags 0x1F, confirming that this is the original transfer rather than a later completed peer substituted into its place.

The compact ELF request clock is approximately 22.5 seconds, well after the original ACK and client close. The original ACK therefore completed before even the ELF request arrived. The four blurred compact ELF clocks are preserved in the photos but are not all included in the selected transcription, and no precise ELF interval is calculated from them. ELF BODY MATCH is 1 and ELF COMPLETED is 1: the locator reply matched the existing expected body and reached its matching transport completion. This is not WLAN firmware loading.

| Original-transfer interval | Time |
| --- | ---: |
| WRQ acceptance to OACK publication | 713.328 ms |
| OACK publication to DATA acceptance | 114.235 ms |
| DATA acceptance to final ACK publication | 797.668 ms |
| Final ACK publication to matching GLINK completion | 25.731 ms |
| Matching ACK completion to accepted peer close | 88.571 ms |

These use the clear large timestamps: WRQ `00BFD38A`, OACK `00CAB5FA`, DATA `00CC7435`, ACK publication/completion/close above. They are host observation boundaries, not modem application execution times. The missing original-write completion and ACK-after-ELF ordering seen in older boots do not describe this boot. They should not motivate another original-OACK priority change here.

## First post-ELF request and file state

POST ELF PATH is **2, MCFG TMP**. The request row is opcode **1 (RRQ)**, peer **0x4A**, host destination **0x5000**, body length **0x49 = 73 bytes**. It is the first RRQ/WRQ accepted after ELF's matching GLINK completion; its generation time and exact first reply outcome are not displayed.

The settled MCFG row is **1 / 1 / 0**: one admitted write session, a **committed one-byte RAM file**, and zero completed MCFG reads. Thus this boot is not stuck with an unconditionally absent MCFG file. The row does not timestamp file creation relative to that first post-ELF read or show its reply. Zero read completions alone does not establish a failed read: for example, existing read handling terminates a client ERROR/end-of-transfer without incrementing that counter. No new protocol diagnosis is inferred from the counter alone.

READ COUNT MASK is `000F002E`: 15 read observations; kinds include ruleset, modem_efs, MCFG and other, with no WLAN kind. The first global rows also show the early ruleset/modem_efs pair. Their kind/peer identities are readable, but this note does not guess every blurred small timestamp. This inventory is global, bounded to its first five entries, and must not be presented as five post-ELF reads.

## WLAN outcome and next offline target

IPA COMPLETE is **1**. QUERY PHASE is **4** and PD QUERY STATE is **7FFFFFFF (UNINIT)**. WLAN RRQ RX is **0** and WLFW SERVERS is **0**. There is no observed advance into WLAN firmware request/discovery despite the original transfer and correctly formed ELF locator exchange completing.

The successful Linux reference likewise completes its original write before the likely ELF exchange, then reaches WLAN-image requests. Current native instead first accepts another MCFG read after ELF. This confirms a later startup divergence on the current build; older failures of the original write cannot explain it by themselves. The next offline comparison should identify the post-ELF MCFG/request/termination sequence and any missing startup prerequisite against Linux, using saved source and existing observations before adding another timing experiment. Missing MCFG alone was already ruled out as a demonstrated initial-WLAN prerequisite in the reference, and the present one-byte file strengthens the reason not to fabricate/prepopulate a file as a fix.

No new image or additional Pixel command was needed to interpret and record this result. No remote application acceptance, WLAN firmware load, scan, association or working Wi-Fi is claimed.
