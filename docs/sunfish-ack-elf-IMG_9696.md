# IMG_9696: ACK service waits and transaction 4 before ELF

The settled ACK ELF STAGES panel has runtime error0, TRACE MATCH1, final ACK
PHASE4, WLAN RRQ0 and WLFW SERVERS0. The first accepted write's DATA reached
final ACK preparation and request-aware queueing; its matching GLINK completion
was observed. This is not evidence of remote TFTP application acceptance.

| Original DATA / ACK boundary | Runtime hexadecimal microseconds |
|---|---|
| DATA received | 00C90D01 |
| Dispatch started | 00D24105 |
| ACK prepared | 00D259D7 |
| ACK queued | 00D2A140 |
| ACK fully published | 00D9D175 |
| ACK GLINK completion | 00DE53B4 |
| Exact close first visible | 00DE0713 |
| DATA buffer RX_DONE published | 00ECFE01 |

Receipt to dispatch took603.140ms, dispatch through preparation6.354ms,
preparation to admission18.281ms, and admission to publication471.093ms.
The total is1098.868ms versus1046.324ms in IMG_9695. The new diagnostic does not
show a speed improvement; it identifies that most of this interval lies
outside the short dispatch/preparation span. It does not identify each checked
callback or writer record within the two dominant waits.

ACK publication preceded first close visibility by275.870ms. Matching GLINK
completion followed that visibility by19.617ms, repeating the prior timing
pattern. Neither timestamp supplies the modem application's timeout decision.
The successful DATA buffer return came1727.681ms after ACK admission and thus
did not gate admission or publication in this boot. Return is successful
GLINK RX_DONE publication, not the earlier request to release the slot.

| ELF boundary | Runtime hexadecimal microseconds |
|---|---|
| Request received | 014D5153 |
| Receive pass ended | 015AA881 |
| First registry entry | 015AD832 |
| ELF first selected | 023BD88C |
| ELF reply queued | 023FFB0F |
| ELF reply published | 024130F0 |
| ELF GLINK completion | 0241DCDA |
| Direct blocker transaction4 published | 023E316B |
| Direct blocker transaction4 completed | 023EDD2E |

The ELF prequeue interval was15903.164ms. Of that, receive-pass completion
took874.286ms, registry entry another12.209ms, entry to first ELF selection
14745.690ms, and selection to admission270.979ms. Queue to publication took
79.329ms and publication to GLINK completion44.010ms.

REG RUN0=00041212 decodes to transaction4, service-name1(tms/servreg),
outcome2(FlowBusy), count0x12=18. REG RUN1=00041101 is that transaction's
successful admission once. REG OMIT0 means no overflow beyond the four stored
runs; the compact panel shows only the first two. ELF PEER WAITS1 and BLOCK
TXN4 additionally identify transaction4 as the directly observed unpublished
registry blocker after ELF selection. Its publication followed ELF selection
by153.823ms, completion took43.971ms more, and ELF admission followed its
completion by73.185ms.

This boot therefore supports the saved notes' transaction4/preselection
explanation, rather than merely resembling its duration. It still does not
identify the packet holding transaction4 itself. Older notes found transaction3
in another profile; that identity must not be assigned to this boot without
matching evidence. The existing owner txn4 pressure and queue guard state are
the next offline sources to inspect, alongside DATA dispatch scheduling and
post-admission writer work. No new Pixel action is needed to interpret this
result, and no new test image or protocol change was made for this note.

ELF completion is a service-registry locator reply, not WLAN firmware loading.
The two WLAN indicators remain zero. Original HEIC, preview, complete decoded
fields, photo/image/manifest hashes and arithmetic are retained under
`native-ack-elf-stages-v1/device-result`. The delivered image/source commit
042014d8 and its historical manifest are preserved unchanged.
