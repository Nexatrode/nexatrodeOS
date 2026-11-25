# One ELF queue opportunity before TFTP refills normal capacity

[IMG_9725](sunfish-elf-tx-slots-IMG_9725.md) shows the first legitimate normal slot release being refilled by a different TFTP ERROR before ELF admits in another slot. At the latest blocked snapshot five packets await GLINK completion and three are unsent, including IPA's 0035 response and a registry QRTR credit. It does not establish an early-retirement opportunity or the proprietary WLAN worker's deadline.

## Offline source comparison

Runtime's checked Owner transport pass can return after a completion unblocks a retained TFTP reply. In `service_pass`, the early TFTP step precedes normal registry dispatch. Therefore a held TFTP response can consume a newly free normal slot before the waiting ELF reply repeats admission. The previous first-slot handoff was tied to an exact **registry** receipt; it did not cover the observed TFTP completion. The queued-ELF handoff begins only **after** admission. This change addresses the gap between those two boundaries, not another change to the queued reply's writer priority.

Pinned Linux fd78d17954a86ea6ceb0c535e618ea129e09f8dd, `drivers/net/ipa/ipa_qmi.c::ipa_server_driver_init_complete`, calls `qmi_send_response` for0035 before setting `uc_ready` after a successful return. Android kernel28f9290ae067d3e7857126eb6312720dbe9c7c80, `drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c::ipa3_handle_modem_init_cmplt_req`, sets its modem-init flag before its response send. Neither requires an exact GLINK RX_DONE as a general condition for a readiness flag. These sources therefore do not justify redefining native IPA COMPLETE or waiting for every outbound receipt. The unsent0035 response remains relevant, but this experiment leaves IPA and writer policy unchanged and isolates ELF admission competition.

## Operational boundary

Feature `sunfish-elf-capacity-retry` arms one private opaque-handle opportunity only after the **actual normal ELF dispatch** returns NoCapacity and the live ordinary pool is full. Diagnostic history, transaction-number guesses, clock values and snapshot state do not grant this opportunity.

Before the early TFTP service step, after existing early IPA handling, Router can lend one normal registry dispatch when its live continuation/owner/generation binding is valid and no other logical owner is pending. Owner requires that same live first-ELF handle still be pending and first in normal registry order, with a currently free ordinary TX slot. It consumes the opportunity before invoking the unchanged registry handler and queue path. Those repeat lifecycle, request, encoding, normal-slot, destination, reference-credit and third-reply checks. A full pool or another logical owner does not consume it. A blocked/failed normal admission consumes the single attempt and falls back to the existing schedule; it does not loop or reserve credit speculatively.

Successful queueing uses the existing queued-ELF handoff at its original point. No extra writer call is inserted by this new Runtime hook. Published owners are retained until their real protocol completions, all pool sizes stay fixed, and grant advertisement-before-ACK, QRTR credit priority, FIFO/table/cursor checks, close cleanup and fault handling remain intact. TFTP service still runs afterward and retains its response normally if no slot remains.

## Evidence on the panel

Title **ELF CAPACITY RETRY**. Existing eight-slot capture, first actual free, admission index, ELF timing and WLAN/WLFW outcomes remain. The old phase rows are replaced by:

- `EARLY ARM ENTER END FLAGS`: ages of arming, entering, returning and event bits.
- `EARLY SLOT RESULT ARMS FULL`: free index seen at entry, result, arm count and full-pool deferrals.

Flags:01 armed,02 entered,04 returned,08 queued,10 normal WaitingReply,20 error,40 abandoned because the exact request was no longer pending. Result1 queued,2 WaitingReply,3 Idle,4 Progress,5 error; zero means no attempted result. FULL counts only eligible Router calls that reached Owner while its pool was still full, not every service pass. Operational one-shot fields are separate from these diagnostic values.

A useful success is flags0F/result1 with admission in the first freed slot before TFTP refills it. A different free slot or a normal queue rejection is still possible under varying arrivals; compare the actual first-free and entry clocks. Successful early queueing does not prove publication met a modem deadline or that WLAN firmware will be requested.

## Verification and delivery

Actual Router/Owner schedules exercise live full-pool arming, slot release, one normal queue attempt before later TFTP admission, preservation of older ownership, clockless operation, confirmed-request credit ordering, full-pool/other-owner deferral, identity/lifecycle faults, and ordinary writer fallback without a fitting intent. Existing exact Core pressure, credit, grant, completion and transfer tests remain. The renderer uses the actual dual framebuffer, including missing and clockless fields. A disabled-entry negative control must fail the new admission replay. Native verification includes selected stack, actual capability mutations, boot envelope and packed initializer mappings; pinned Normal-NC SDK reuse is explicitly distinguished from fresh compilation. No physical outcome is claimed before boot.

An initial private preparation helper selected an older diagnostic directory. Its boot images and manifests were unchanged. The old profile was restored only to its exact original delivered artifact hashes; all three recent delivery manifests and replay pins were rechecked. The replacement helper creates a fresh directory exclusively, and restoration evidence is retained in the new private profile.

Cold temporary boot, after a full power-off:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-elf-capacity-retry-v1/fastboot.img
```

Wait at least one minute, then photograph the full settled panel. This boots temporarily; no partition is flashed and no automatic Pixel action is performed.
