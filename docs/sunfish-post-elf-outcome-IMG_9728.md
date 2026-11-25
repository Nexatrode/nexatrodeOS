# IMG_9728: ELF credit and IPA completed; post-ELF MCFG client closed

Settled **POST ELF OUTCOME**, source `cd36ad76e98ad80f144961c0bf4fdbb513c7419b`, image SHA256 `89c8aa2012038d66a776185a04b3d34db1ba4b355da2d528a19ee16a60bcde19`. Raw HEIC, full manual transcription and decoder are retained in the private profile's `device-result/`, outside the unchanged delivered manifest. Runtime error zero.

## Exact ELF credit and IPA

ELF BODY/DONE/HOST BEFORE/CONF is1/1/1/1. The reply matches the reference body and completed. Confirmation was requested. HOST BEFORE1 means its separate host RESUME_TX had published, but had not yet completed, when the ELF reply published. Current HOST REQUESTED/PUB/DONE/INTENT is1/1/1/0: that **exact credit subsequently completed**, using intent0. The current fields do not timestamp its completion relative to ELF completion, so that later ordering remains unknown.

Live IPA FLAGS/REG/DRIVER/COMMITS is1F/1/1/4. Pending message isFFFFFFFF, accepted response count1, failure0 and timeout0. FLAGS1F sets modem-ready, QMI uC-ready, indication-requested, indication-sent and current protocol-complete, with pending, queued, held-response, failed and initial-boot bits clear. This is one completed original handshake with four committed publications and no duplicate driver request or pending IPA record at the settled snapshot. The earlier unsent0035 pool row was transient, not a permanently outstanding IPA response. The separate historical Runtime IPA latch is also1.

## What followed ELF

ELF acceptance/queue/publication/Owner completion: `01487295 / 016EAE40 / 0170DB16 / 017D6936`. Receive-to-queue2,505.643ms, queue-to-publication142.550ms, publication-to-completion822.816ms; total3,471.009ms. The early ordinary retry flags0F/result1 show a successful attempt in slot1 with three eligible full-pool deferrals. These are host observations; this panel does not display the first physical slot-release time.

Original final ACK `00D96055`, phase4/block1, completed10,750.177ms before ELF's recorded completion. The first TFTP service request **accepted after** matching ELF completion is a73-byte RRQ for `/readwrite/mcfg.tmp`, peer4A. Its acceptance at01828379 follows ELF completion by334.403ms. This is acceptance order, not proof of when the modem generated it.

The first other post-ELF path is exact `/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig`, category8, peer4F,93bytes, RRQ. The retained journal shows that peer retrying at0277EAE9, closing at02D3F285 and peer51 requesting the same path at02D54FF5. It does not show this other path's server result or establish that its retry caused the WLAN failure.

The read summary is000F002E:15 service reads, kind bits for ruleset, modem_efs, MCFG and other. There is no WLAN-image bit. One MCFG write committed a one-byte file; zero MCFG reads completed.

## MCFG response and timing limit

Peer4A flagsBF indicate acceptance, dispatch, prepared reply, queue, publication, matching completion and close, with **no client ACK/ERROR** observed. File presence at dispatch is1byte; server result0; response opcode6 OACK,45bytes, transfer port5009. Offered selected values are block7680, timeout1000ms, window10, mask0F. Negotiated selected values are block512, window1, tsize1, mask0F. The offered tsize value and other undisplayed raw fields are not reconstructed from the request length.

Scalar RX/QUEUE/PUB/DONE is01828379/01D00928/01D15916/01D1BE2E. Close018C7A8D is653.076ms after acceptance and4,427.419ms before the scalar queue clock. First/last client ending are unavailable and target repeat count is0. It is safe to report the recorded close and lack of client ACK; it is premature to call the whole5,080.495ms acceptance-to-queue interval a single original OACK wait.

The journal contains an **earlier OACK on the same route5009 to4A at01C471F0**,759.608ms before the scalar queue observation, then another at01D15916 and the tracked completion01D1BE2E. Endpoint/port/body-length equality alone does not prove the two OACK records belong to one retained request identity. The observer's scalar queue binds the saved request handle while its journal records all relevant datagrams. Exact original-versus-later response/handle matching and close cleanup need offline reconciliation before changing scheduling or claiming this timing caused the failure. The current41-record journal retains first8/latest8 and omits25 middle records; it is not an exhaustive trace.

## WLAN outcome and next work

Live IPA complete1, PD7FFFFFFF (UNINIT), **WLAN RRQ0 / WLFW0**. This rules out a never-returned ELF host credit and an unfinished IPA handshake as persistent settled blockers in this boot. It does not show that either finished before a proprietary startup deadline. Linux's successful boot already requested WLAN before its absent MCFG probe, so the MCFG client ending alone does not establish an initial-WLAN prerequisite.

Next offline work is the original/later MCFG OACK identity and close path, plus the subsequent mbn_hw.dig service handling, using this journal and source. No repeat of the same image is needed for the four requested answers. No device command was run during decoding.
