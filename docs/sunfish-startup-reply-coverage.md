# Early reply coverage after IMG_9819

IMG_9819 identifies the original early MCFG request as a valid tsize probe of a file absent at dispatch. Its ERROR1 received an exact GLINK completion74.866ms before the observed close, without WLAN. The successful saved Linux boot first requests WLAN firmware8.297133s before its first MCFG read. Existing IMG_9797/9806 also completed the original server-check write, both retained EFS replies and ELF without WLAN. Thus another generic MCFG-priority change would repeat an already resolved question.

There is a narrower remaining coverage limit: the earlier startup observer reports four ruleset RRQs but retains only two. The two omitted requests cannot inherit the retained requests' outcomes, and endpoint/length alone cannot identify retries. The current accelerated TX3/ELF profile has not displayed these four current-boot lifecycles together. This diagnostic asks whether either previously omitted ruleset exchange failed to publish or return its buffer before close, alongside the EFS pair and original write/ELF outcome. A clean result deprioritizes these retained negative exchanges; it does not prove application acceptance or imply that Wi-Fi is nearly operational.

## Bounded observer increment

Feature `sunfish-startup-reply-coverage` inherits `sunfish-first-mcfg-response`. Existing six startup slots remain at their original indices: ruleset0/1, EFS2/3 and digest4/5. Two extra slots6/7 retain the third/fourth distinct ruleset request handles. They never overwrite earlier records. Further requests still increment omitted counts; duplicate hooks for retained handles remain ignored, while omitted handles are not retained for deduplication. Counts therefore describe observations under these existing rules, not an unlimited transaction ledger.

An optional numeric timeout projection is copied from the already parsed request options. Presence is independent of value, including an explicit zero; absent values remain all ones. Existing reservation, slot, route, length and remote-intent matching govern publication/completion for the extra records. All ordinary queue, receive return, close and reset observation hooks iterate the same bounded array. No new clock call site, FIFO access, validation, queue admission, scheduling opportunity, grant/credit policy, file content or firmware-provider change is added. Retaining two more entries can execute existing optional clock sites and metadata copies more often. The persistent observer grows; target stack verification is required. The fixed observation size assertion is2304bytes, rather than the parent's1536byte bound.

No transport buffer is reclaimed from a close or a cursor observation. Advertisement-before-ACK, QRTR credits and blocked/uncertain-write behavior remain inherited. The feature changes observation capacity and presentation, not the startup behavior under investigation. Timing can still vary because observation and rendering take time.

## EARLY REPLY COVERAGE

All timestamps are hexadecimal microseconds on the current runtime clock. All ones means unavailable. Each identity is the original opaque request privately bound to its real host-only transport reservation; printed sequence low/high halves are diagnostics, not capabilities.

Rows2–6 show runtime error/close state and TX3 call/mask; original peer/WRQ/OACK/DATA/final-ACK publication/completion; original close and ELF receive/queue/publication/Owner completion/credit mask; TX3 and TX4 queue/publication/completion; and ruleset/EFS/digest total/omitted counts.

Rows7–24 contain six three-row blocks, in order RULE1, RULE2, RULE3, RULE4, EFS1, EFS2:

1. Peer, flags, reservation sequence low/high, offered timeoutms, host transfer port.
2. Accepted, server dispatch, queued, publication, matching GLINK completion, exact peer close.
3. Request body length; option-presence low16/parse-flags high16; rsize; reply opcode high16/code low16; admission attempts; FlowBusy high16/NoCapacity low16 (each wait count saturates at65535).

Options use the existing presence bits: blksize1, timeoutms2, wsize4, tsize8, rsize16, seek32, unlink64. An unavailable opcode/code half printsFFFF. Request flags1/2/4/8/16/32/64/128 mean accepted/dispatched/reply formed/queued/published/matching completion/client ending/close;512 marks a missing reservation. A phase can exist without an optional clock. ERROR1 is a negative reply, and does not expect normal OACK/DATA continuation. This compact page does not print reply body or client-ending contents; use flags and do not invent those values.

Rows25–28 reuse existing ELF peer-tail/receipt-absence/visibility/ahead/Core/Owner clocks; live IPA flags/counters/pending ID; RAM MCFG writes/file/read counts and close observations/commits/first-other-path; historical IPA, PD, WLAN RRQ, WLFW, ELF body match/completion. The ELF FIFO watch is explicitly separate from the early TFTP replies: its tail/visibility must not be assigned to their reservations. Historical IPA completion remains separate from live pending state.

An early reply completing before host-observed close establishes real transport return, not application acceptance or a private timeout guarantee. Acceptance-to-publication can be compared to the offered timeout only as host timing evidence; this page does not sample the early request's first FIFO visibility or modem consumption. Any omissions still limit completeness.

## Offline checks and delivery

The actual Owner replay sends four ruleset requests and two EFS requests through the real server dispatch, normal transmit queue and matching GLINK receipts. Child/parent feature runs must have identical committed bytes and complete ordered Fake I/O. The extra-slot Core case rejects wrong reservations and remote-intent receipts, tests bounded duplicate/omitted observations, close scoping and transport reset. Expected-red controls remove extra capacity or reservation matching and must fail those functional assertions. Existing fault/backpressure, grant, credit, close, original ACK and registry/ELF regression suites remain in the delivery checks.

The real physical framebuffer renderer is tested for all six distinct identities, ordering, numeric zero versus absence, unavailable clocks, current IPA state, omissions and both buffers, and is visually inspected. The native program is rebuilt, then the packed loader/boot image and capability archive are checked, with a selected nested-stack regression audit. That stack audit is not whole-program worst-case stack certification. The pinned SDK is reused; no physical device command is run automatically.

The unrelated mainline Maps update `dc18d26e` adds gated vault-key/session functionality. Its exact Cargo/vault source overlay is recognized by inherited Wi-Fi source-boundary tests. Neither Maps feature is in the native feature closure; no Maps authority or device path is added to this image. The original `native-first-mcfg-response-v1` artifacts remain unchanged, and IMG_9819 results stay separate from its manifest.

When verified, use the temporary image in `.cache/sunfish-native-wifi/private/native-early-reply-coverage-v1/fastboot.img`. Wait at least a minute and photograph the full **EARLY REPLY COVERAGE** panel. No unchanged MCFG rerun is needed first. This experiment resolves the named omitted-exchange gap; it is not a proposed WLAN fix.

Verified regression counts for this increment: current Owner726, IRQ761 and Core334 passed; parent Owner725/Core333 and older Core297 passed. Owner/IRQ retain one existing ignored test each. All37 regression jobs passed, as did12 deliberate functional removal controls. The six actual Owner exchanges preserve identical complete I/O and committed bytes with coverage on/off. The selected native stack audit leaves693856bytes in the five-MiB mapping, above its262144byte headroom threshold; its limited scope remains as described above. The temporary image is69,001,216bytes, SHA256 `7af5ec0a9b13d7620d96de3c0fff9a9594d9096cda1499e7405537ff557c755f`. Physical startup remains untested until the next manual boot; readiness additionally requires the recorded fresh package/capability and committed rebuild checks in the private manifest.
