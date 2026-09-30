# UTM limits vs real Pixel hardware

Planning notes from 28 September 2026. Keep building the personal environment on
UTM, but treat phone-class peripherals as a separate feasibility track.

## What UTM is for

UTM/QEMU proves seL4/Microkit behavior on a **virtual** ARM board: isolation,
app workflows, vault protocols, VirtIO GPU/input/block/RNG, and later **fake**
modem or camera backends. It does not prove Pixel boot, SM7150 drivers, radios,
or power.

| Capability | UTM / QEMU today | Real Pixel 4a (`sunfish`) |
| --- | --- | --- |
| GUI apps, vault, isolation | Experimental graphics profile | Needs P1 boot + P2 display/touch/storage |
| Keyboard | VirtIO keyboard | Physical keys / IME path TBD |
| Pointing | VirtIO **tablet** (absolute); optional guest-drawn cursor | Digitizer + driver (different stack) |
| Networking | Emulated Ethernet / NAT, not Mac Wi‑Fi | Wi‑Fi firmware + driver (P3) |
| Cellular / calls / SMS | Only as a future fake modem (U6) | Modem/IMS (P4); very large |
| Camera | Possible fake frames later | Sensor/ISP stack (P5); very large |
| Battery / deep sleep | Not meaningfully modeled | P2/P6; cannot infer from UTM CPU use |
| Bluetooth / NFC / USB host | Out of scope for now | Separate device work |
| Titan M / StrongBox | Software development key only | No seL4 access path yet |

## Input comfort: tablet vs mouse

The graphics VM attaches VirtIO **keyboard + tablet**, matching phone-like
absolute hit-testing. UTM’s “USB Tablet” mode often **hides** a host cursor, so
clicks are hard to aim on a Mac. “USB Mouse” shows a host cursor but speaks
**relative** motion; our guest driver does not implement that yet.

Practical approach:

1. Keep tablet absolute coordinates for launcher/app hit-testing.
2. Draw a **guest on-screen cursor** from the last tablet position so UTM tablet
   mode stays usable.
3. Optional later: VirtIO mouse support for relative pointers.
4. Do not switch the profile to mouse-only and expect clicks to work without
   guest changes.

## Deeper work (when apps are not the bottleneck)

- **U3:** trusted crypto, rollback anchor, real power-loss durability evidence.
- **P0/P1:** Pixel boot/debug (UART adapter, boot.img packaging, platform port).
- **Input/GPU:** production drivers beyond QEMU spike code; mouse; accessibility.
- **U6:** `./dev test-net` proves standalone and merged Personal OS VirtIO net
  (DHCP + DNS OK + TLS OK + UDP/TCP echo + PSL1 SEAL OK + Messages sealed TX + toy router). Dual-UTM
  Shared Network is a manual sketch in [network-spike.md](network-spike.md);
  Messages-over-TLS and fake telephony/camera remain later.
- **P3–P6:** Wi‑Fi, cellular, camera, battery — conditional on hardware access.

Apps still matter: they force stable UI, storage, and permission interfaces that
can carry over once hardware exists. They do not replace bring-up.

## Messaging and SimpleX Chat (long term)

The Linux/macOS/Android SimpleX Chat apps assume a POSIX-class OS (Haskell RTS,
TCP/TLS, SQLCipher). They will not run natively on Microkit. Options later:

1. **Companion:** keep SimpleX on Mac or Lineage; Personal OS stays offline-first.
2. **U6 toy/net messaging:** our own Messages UI + VirtIO Ethernet experiments.
3. **Linux guest under seL4** (if EL2/VMM exists): run upstream SimpleX inside Linux.
4. **Native rewrite:** a small Rust client inspired by SimpleX’s threat model—not a
   GHC port. AGPL-3 applies if we ship their code.

Do not schedule SimpleX-on-Personal-OS until networking (U6) and a conscious
VM-vs-native decision exist. A Home-screen Messages stub card is already present
(read-only, no net); Phone cards and real messaging still wait on U6.
as non-networked stubs if useful for UX layout.

The preferred product story for *our* Messages app is explicit QR / PGP-style
contact add and sealed traffic over a data pipe (central router first, relays
later)—see [explicit-contact-messaging.md](explicit-contact-messaging.md).

## Related docs

- [UTM prep checklist](utm-prep.md)
- [Network spike](network-spike.md)
- [Roadmap UTM vs Pixel tracks](roadmap.md) (includes home-screen today vs planned)
- [Input spike](input-spike.md)
- [Pixel P0 boot checklist](pixel-p0-boot-feasibility.md)
- [Pixel key / StrongBox](pixel-key-feasibility.md)
