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
| Cellular / calls / SMS | Phone has a digit-pad demo wired to the fake modem; no real audio or carrier | Modem/IMS (P4); very large |
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
  (DHCP + DNS OK + TLS OK + TLS SEAL OK + MSG TLS TX + UDP/TCP echo + PSL1 SEAL OK + Messages sealed TX + toy router). Dual-UTM
  Shared Network manual peer seal **PASS** (2026-09-29) in
  [network-spike.md](network-spike.md); fake telephony/camera remain later.
- **P3–P6:** Wi‑Fi, cellular, camera, battery — conditional on hardware access.

Apps still matter: they force stable UI, storage, and permission interfaces that
can carry over once hardware exists. They do not replace bring-up.

## Messaging and SimpleX Chat (long term)

The Linux/macOS/Android SimpleX Chat apps assume a POSIX-class OS (Haskell RTS,
TCP/TLS, SQLCipher). The chosen direction is a native GHC runtime and
dependency port for Microkit, with reusable time, scheduling, networking,
entropy, and storage services added to the OS along the way. The ARM64 Linux
build is a reference for package versions and behavior, not a guest deployment
plan. The port is tracked in [simplex-port-feasibility.md](simplex-port-feasibility.md).

The full SimpleX app remains a long-term target: first make the Haskell runtime
and its required capabilities work as bounded native protection domains, then
run an upstream-compatible chat core and verify it against another SimpleX
client. The current U6 network experiment and read-only Messages stub are
useful groundwork; they do not yet provide an app socket API, TLS 1.3, or the
SimpleX storage and runtime requirements. Other Phone and messaging screens can
remain non-networked stubs while the UI and capability boundaries develop.

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
