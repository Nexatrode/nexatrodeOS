# Personal OS roadmap: UTM to Pixel 4a

Planning baseline: 26 September 2026. Sizes are engineering judgments, not delivery
commitments. U0–U2 are complete. U3 now has a disk-backed vault in the regular
QEMU system, with reboot recovery covered by both standalone and full-system tests.

The goal is a small personal OS with explicit component permissions, a simple
interface, and a fixed set of useful apps. We reuse seL4, Microkit, and selected
LionsOS/sDDF components. We build the personal environment and its policies, then
investigate whether the Pixel 4a can support it.

Three separate outcomes make this manageable:

1. **UTM personal environment:** a real seL4 system with a GUI, private persistent
   data, useful local apps, and simulated phone/camera events.
2. **Pixel handheld:** the same application interfaces backed by real display,
   touch, storage, and power support; connectivity can arrive incrementally.
3. **Cellular phone:** reliable calls, messaging, audio, network recovery, and
   wake from sleep, followed by camera support and sustained reliability work.

The first outcome remains valuable if Pixel hardware support stalls. A first
cellular milestone can omit the camera, browser, Bluetooth, NFC, and app store.

## What we reuse and what we build

| Layer | Starting point | Our responsibility |
| --- | --- | --- |
| Kernel | seL4 | Configure it, test our use of it, and port the platform if needed; do not design a new kernel |
| Component framework | Microkit | Define components, memory, communication, CPU budgets, and fault policy |
| Drivers and I/O services | Selected sDDF/LionsOS components | Integrate compatible drivers; implement missing devices and service boundaries |
| Personal OS services | Our project | Time, private data, app permissions, notifications, recovery, update policy |
| Interface | Evaluate an existing embedded GUI library | Display/input integration, launcher, keyboard, focus, trusted permission surfaces |
| Applications | Our project | Calculator, notes, contacts, calendar, dialer, messages, camera interface |
| Hardware and firmware | QEMU first; Pixel later | Boot integration, device access, firmware interactions, power and thermal behavior |

LionsOS provides composable pieces and remains experimental; it is not a ready
phone distribution. Its drivers are supplied through sDDF. We should test and pin
each selected upstream revision rather than assume every component fits our
current configuration. [LionsOS](https://lionsos.org/),
[I/O components](https://lionsos.org/docs/components/io/),
[sDDF](https://github.com/au-ts/sddf).

Initially the app set is built into the system image. Installing arbitrary apps,
dynamic process loading, and Android compatibility are separate major projects.
The static starting point fits Microkit's architecture.
[Microkit manual](https://docs.sel4.systems/projects/microkit/manual/2.3.1/).

## Size legend

- **Small:** a bounded feature after its dependencies exist; usually a few focused
  development sessions.
- **Medium:** a service or integration requiring several iterations and tests;
  often weeks of effort.
- **Large:** multiple interacting subsystems; potentially months of effort.
- **Research:** we must first establish feasibility. More time alone may not
  resolve missing firmware access or undocumented hardware behavior.

These are relative sizes for a deliberately limited prototype. Reliability,
security review, and product polish can exceed the initial implementation effort.
Calendar duration depends heavily on available time and prior hardware experience.

## UTM development track

| Stage | Work | Completion evidence | Size |
| --- | --- | --- | --- |
| U0: foundation — done | Reproducible build, QEMU/UTM boot, C and Rust isolated service calls, intentional child fault | Twenty-five ordered supervisor checks pass; independent service still works; two negative controls are rejected | Complete |
| U1: interactive system — done | IRQ-driven RTC and serial input; `help`, `status`, `calc`, `crash`; Rust supervisor-mediated child restart | QEMU tests commands before and after the deliberate fault, heartbeat, and two consecutive restarts | Complete |
| U2: service architecture — done | MCS CPU budgets for active domains; malformed console input checks; explicit child restart policy | A spinning child cannot block other services; malformed inputs are rejected; the attempt counter proves private state persists across restarts | Complete |
| U3: private persistent data — in progress | The Rust vault stores system, Notes, and Contacts records in fixed endpoint-mapped namespaces. New slots use AES-256-GCM-SIV; QEMU tests verify encrypted records survive reboot and reject tampering. PVLTBAK1 remains readable; new PVLTBAK2 archives carry full active blocks, with host export/restore commands and QEMU restore coverage. The vault waits for a negotiated VirtIO flush after staging a record and again after committing it. Record encoding now uses a crypto-operation interface and preserves provider outages as storage failures; the feature-gated QEMU provider still keeps its fixed software key in the vault process. Pixel StrongBox supports the tested AES-GCM path, while the current format requires AES-GCM-SIV, so a trusted compatible service or versioned format change is needed. Default library tests use a separate test-only provider. Restore refuses older authenticated generations relative to the selected source image unless `--allow-rollback` is passed. The storage suite injects guest resets at all four write/flush boundaries for both scalar and PVLTBLB1 app records and checks old-or-new recovery. Remaining: validate a trusted device crypto path, add a trusted monotonic rollback anchor, establish durability under actual host or device power loss, and add a guest backup UI if needed | All three records survive repeated QEMU cold boots and UTM warm resets; interrupted writes leave a defined valid state; one app cannot read or alter another app's record; the archive restores authenticated records without storing the raw key in the archive; provider/storage errors and missing flush support fail vault operations | Medium–large |
| U4: graphics and input — QEMU integration passed | `./dev test-input` verifies the shared input ring and overflow handling, Rust event validation, isolated app endpoints, keyboard and touch navigation, Notes and Contacts editing, read-only Clock navigation, exact rendered frames, encrypted persistence across a cold boot, and all 26 isolation checks. Notes supports four records with independent titles and bodies; old raw notes and PNT1 collections migrate to PNT2 by deriving titles from the first body line. Contacts supports four entries and touchscreen text/number input. UTM boot/render and keyboard navigation are manually verified; pointer click comfort is still unconfirmed. Broader keymaps/accessibility and production VirtIO input drivers remain future work | QEMU renders title, body, list, scroll, Contacts, Calculator, and Clock states exactly; Notes and Contacts restore after cold boot | Medium integration complete; polish remains |
| U5: local applications — started | Notes, Contacts, Calculator, Clock, Calendar, Messages, Phone, and Settings share the Home launcher (2×3 cards plus Phone/Settings half-row). Contacts store an optional 8-character hex key-fingerprint stub (PCT2 vault blob; PCT1 migrates with empty KEY). Calculator keeps checked `i64` arithmetic with touch/keyboard QEMU checks. Clock stays a read-only UTC snapshot from the PL031 service. Calendar is an isolated PD with up to three reminders (title plus optional MMDD day and HHMM time) persisted in an encrypted vault namespace (PCL2; PCAL migrates with empty when). Messages is an interactive peer-key shell with vaulted PMS2 peers and local PSL1 sealed stubs (`SEALED N`; ns 4; `PEER KEYS` / `ADD KEY n OF 8` / `COMPOSE KEY <id>`). Phone is a read-only fake-modem alert shell (`ALERT\nRING N\nSMS M`). Settings remains a read-only stub whose fourth line shows modem alert counts (`NO ALERTS YET` or `ALERTS RING N SMS M`; see [modem-spike.md](modem-spike.md)). Refresh `./dev utm-graphics` after input changes. Pointer click comfort is still a manual UTM check. Richer notifications remain | QEMU checks arithmetic, clock formatting, calendar day/time frames, Messages compose→`SEALED 1` frames, Phone/Settings alert frames, Contacts KEY frames, routing, and exact frames; manual UTM Keyboard navigation works; pointer click behavior still needs a reliable manual check | Small per simple app; medium for vaulted reminders |
| U6: connectivity and simulated phone devices — started | `./dev test-net` proves standalone sDDF echo_server DHCP+UDP and an integrated Personal OS merge (`tests/net` serial-free `dhcp_client`, Messages→net sealed TX with configurable peer defaulting to `10.0.2.2:1237` for the fixed demo (UDP) and UI compose→seal path (UDP + TLS `MSG TLS TX`), host toy router to a second guest `SEAL OK`, `DNS OK`, TLS 1.2 ECDHE-ECDSA TOFU-pinned `TLS OK` + PSL1-over-TLS `TLS SEAL OK` on 1239, UDP echo on 1235, TCP echo on 1238, PSL1 `SEAL OK` on 1236). Console `netpeer` / `netseal` retarget sealed TX. `./dev utm-net` exports dual-UTM Shared Network bundles; **2026-09-29 manual PASS** (`SEAL OK` on Net B after `netpeer`/`netseal`). `./dev test-input-net` merges VirtIO net into graphics+input by sharing the GPU `timer_driver` (isolation + DHCP); `./dev utm-graphics-net` exports `Personal OS Graphics Net.utm`. Serial-image fake modem stub: [modem-spike.md](modem-spike.md) (`MODEM|RING` / `MODEM|SMS IN`); Settings shows `ALERTS RING N SMS M` after supervisor push. Default and offline graphics UTM images stay offline. Remaining: dialer UI, audio/camera backends; dual-UTM automation still blocked on UTM serial attach / vmnet privileges | Standalone and merged QEMU DHCP + DNS + TLS seal + Messages demo/UI TX (UDP+TLS) + toy router + UDP/TCP echo + SEAL OK pass; graphics+input+net DHCP smoke; later scripted calls/SMS and permissions once those backends exist | Medium–large |
| U7: hardening and release discipline | Entropy/key handling, encrypted data, authenticated updates, failed-update recovery, backups, parser fuzzing and endurance tests | Defined attacks are rejected; update interruption and restore work; long runs remain responsive | Large and ongoing |

U2 establishes security boundaries before adding more devices. U3 and U4 can
progress independently once those interfaces are stable. Pixel reconnaissance
should begin around U1/U2, before committing heavily to a hardware-specific plan.

The QEMU-only entropy service now returns 128-bit VirtIO RNG samples to the
supervisor. The regular boot tests exercise repeated requests, and the storage
suite checks delivery after each QMP system reset, after restoring a full pre-boot
QEMU snapshot, and in two independent QEMU instances cloned from the same saved
snapshot; the recorded short fingerprints differed. The vault does not consume
the samples; v1 remains the only writer. The proposed
[PVLTREC2 AES-GCM migration design](vault-record-v2-design.md) preserves v1
records and the slot size, but v2 implementation waits for a trusted entropy
policy, broader replay/clone analysis, and a trusted device crypto path.
Security requirements and recovery tests belong in every stage; U7 completes and
reviews them rather than introducing them for the first time.

Component restart means restoring the intended memory, device, and protocol
state. Moving the instruction pointer back to startup is not sufficient for a
general recovery mechanism. Likewise, an app must have its own protection domain
to claim kernel-enforced isolation from another app; several screens inside one
GUI process share that process's authority.

The [graphics feasibility test](graphics-spike.md) renders through sDDF's GPU
driver both alone and alongside the regular OS services on QEMU ARM `virt`. The
default Personal OS image and UTM profile remain serial-only. `./dev utm-graphics`
exports a separate test profile with the integrated image and a disposable
vault disk. QEMU ARM `virt` has no default graphics device, so the guest needs an
explicitly configured device and a matching driver. A bootloader-provided
framebuffer must not be assumed. [QEMU virt documentation](https://www.qemu.org/docs/master/system/arm/virt.html).

For networking, UTM presents an emulated interface such as Ethernet even when
the Mac itself uses Wi-Fi. This tests the network service and apps, not a Pixel
Wi-Fi driver. A test image supplied to a fake camera similarly tests app behavior,
not sensor acquisition.

### The routine development loop

1. Build a small change and run the automated QEMU tests with `./dev test`.
2. Stop the **Personal OS** VM, run `./dev utm`, then start it in UTM.
3. Exercise the visible behavior using the same guest image.
4. Add a meaningful failure case for each new security or recovery boundary.
5. Keep platform-specific driver code behind stable service interfaces.

The current scripts will grow with the system: disks, display devices, input, and
test scenarios need explicit configuration. The UTM exporter attaches the block
device through virtio-mmio because the generated seL4 system expects that transport.
Current VM settings use software emulation; their speed and host power consumption
do not predict Pixel behavior.

## Pixel 4a track

The Pixel 4a is not listed in the current official seL4 or Microkit supported
platform tables. Treat it as a new port with unknown peripheral coverage.
Existing Android/Linux source is valuable documentation and reusable code under
its licenses, but Linux drivers depend on Linux services and cannot simply be
linked into a native Microkit component.
[seL4 platform list](https://docs.sel4.systems/Hardware/).

Confirm the exact model first: this roadmap targets the non-5G Pixel 4a,
`sunfish`. The Pixel 4a 5G is a different hardware target. Existing sunfish device
sources illustrate the many separate display, touch, thermal, camera, and power
dependencies that must be accounted for.
[Sunfish device sources](https://github.com/GrapheneOS-Archive/kernel_msm-coral/blob/13/arch/arm64/boot/dts/google/sm7150-sunfish-common.dtsi).

| Stage | Work | Decision or completion evidence | Size |
| --- | --- | --- | --- |
| P0: feasibility | Identify device/firmware, recovery route and boot-image format; inspect sources; establish available CPU privilege, debug output, memory ownership and firmware constraints. Checklist: [pixel-p0-boot-feasibility.md](pixel-p0-boot-feasibility.md) | Written feasibility report and a minimally invasive boot/debug experiment where supported | Medium research |
| P1: minimal platform port | Correct loader handoff, memory map and reserved regions, interrupts, kernel timer, debug channel, seL4/Microkit platform configuration | Hello-world and isolation tests run repeatedly on the actual phone | Large research |
| P2: handheld basics | Display, touch, buttons, storage and USB/debug access; charging/thermal safeguards before prolonged use | Launcher and local apps work on the device; data survives reboot | Large |
| P3: audio and connectivity | Speaker/microphone/earpiece routing, Wi-Fi firmware and driver, association/authentication, network integration | Recording/playback and Wi-Fi recover from disconnects and service failures | Large research |
| P4: cellular phone | Modem startup/control/data paths, SIM, registration, SMS, voice/IMS, call audio, carrier behavior and modem recovery | Incoming/outgoing calls and SMS work repeatedly, including screen-off operation and relevant sleep states | Very large research |
| P5: camera | Sensor controls, clocks/regulators, capture buffers, image processing, focus/exposure, preview and storage | Preview and repeatable still capture with correct permissions and acceptable image quality | Very large research |
| P6: dependable handheld | Deep sleep/wake, alarm and modem wake sources, thermal behavior, battery accounting, updates, keys and recovery | Measured idle/call/camera use, reliable wake, interruption recovery, sustained real use | Very large and ongoing |

Basic power and thermal handling starts in P1/P2; final battery optimization does
not. Charging correctly, waking for a call, and recovering after device suspend
are functional requirements. We cannot infer them from low idle CPU utilization.

### Correction about EL2

Our existing QEMU target runs seL4 at EL2, the Arm virtualization privilege level.
Microkit 2.3.1 also documents a QEMU EL1 target. Therefore lack of EL2 is not an
automatic rejection of every native seL4 design. A Pixel EL1 port would still need
platform work and assessment of the firmware above it. Hosting a hardware-assisted
Linux driver VM under seL4 needs suitable virtualization access, normally EL2 on
AArch64. Pixel access to that level has not been established here.
[Microkit EL1/EL2 targets](https://docs.sel4.systems/projects/microkit/manual/2.3.1/#qemu-virt-aarch64-in-el1).

### Hardware strategy decision after P0

| Route | Benefit | Cost or prerequisite |
| --- | --- | --- |
| Native seL4 with native drivers | Direct control of component boundaries and a potentially smaller trusted software base | Extensive device porting and firmware research |
| seL4 with a Linux driver VM | Possible reuse of Linux drivers behind narrow service interfaces | EL2/platform support, guest bring-up, device assignment and DMA isolation; Android vendor services may still be needed |
| Our interface on an Android/Linux host | Earlier use of existing phone functionality | Different security architecture: the host kernel controls the system |
| Different hardware target | Can preserve native OS goals when Pixel access blocks them | Select only after checking exact device/driver/debug/power support |

A driver VM is an option to investigate, not a proven Pixel shortcut. Its device
DMA access must be contained with working hardware isolation or included in the
trust model. Existing LionsOS VM infrastructure gives us a starting point.
[LionsOS virtual machines](https://lionsos.org/docs/components/virtual_machines/).

### Why calls and camera are especially large

The dialer is a small client of a complex telephony service. Qualcomm's integrated
modems use SoC-specific control and data transports; planning for a generic UART
and an AT dial command is insufficient.
[ModemManager's Qualcomm description](https://modemmanager.org/docs/modemmanager/wwan-device-types/#qualcomm-socs).

LTE data success does not establish voice success. IMS/VoLTE integration and
carrier configuration must be investigated separately, along with audio and wake
behavior. Android's IMS architecture itself has vendor/carrier implementations
and potentially modem-specific offload interfaces.
[AOSP IMS architecture](https://source.android.com/docs/core/connect/ims).

The camera screen and shutter button are relatively small. Sensor acquisition,
image processing, exposure/focus, calibration and reliable buffering are separate
systems. A first raw image, a usable photograph, and stock-camera quality are
different milestones. [AOSP camera architecture](https://source.android.com/docs/core/camera).

## What carries over

| Mostly reusable with stable interfaces | Must be adapted or established on Pixel |
| --- | --- |
| App logic, UI layout, service protocols and permission policy | Loader and platform configuration |
| Storage record formats and much of the storage service | Physical storage driver and hardware key access |
| Telephony state machine and dialer/messages UI | Modem transport, IMS integration, carrier behavior and audio paths |
| Camera app and image-storage workflow | Sensor/ISP/capture driver stack |
| Fault tests, malformed-message tests and recovery policy | Real interrupt, DMA, power and device-reset behavior |

Keeping the CPU architecture ARM64 helps, but it does not make the QEMU boot image
a Pixel image. The two targets need separate builds and hardware descriptions.

## Home screen and app set (today vs planned)

Today the graphics profile is a **six-card home** (not a full phone springboard):

| Slot | Status |
| --- | --- |
| Notes | Done — isolated UI + encrypted vault |
| Contacts | Done — isolated UI + encrypted vault; optional hex key-fingerprint stub |
| Calculator | Done — isolated UI; history still transient |
| Clock | Done — read-only UTC date/time from the RTC service |
| Calendar / reminders | Done — isolated UI + encrypted vault blob (title + optional MMDD/HHMM) |
| Messages | Interactive peer-key shell with vaulted PMS2 sealed stubs (ns 4, blocks 8–9); on-wire peer delivery / toy router remain U6 |
| Settings | Done as read-only stub — isolated UI, local/vault/net/alerts status; real options come later |
| Phone / dialer | Planned UI later; real calls need U6 fake modem then Pixel P4 |

Home is `FocusTarget::None`: tap a card or use Tab / the top app bar. Escape returns
Home. There is no app drawer and no Phone icon yet.

**Messaging north star (not scheduled as the next spike):** identity by explicit
QR / PGP-style key exchange over a data pipe, not by phone number as social ID.
See [explicit-contact-messaging.md](explicit-contact-messaging.md).

**Calendar** occupies one slot in the 2×3 launcher: an isolated protection domain
and up to three reminders with title plus optional `MMDD` day and `HHMM` time,
persisted in an encrypted vault blob with QEMU cold-boot checks. Google Calendar
sync, time zones, push notifications, and backup-archive inclusion remain later
work.

**Messages** is an interactive peer-key shell: isolated PD with routed input,
up to three local 8-hex KEY peers with PSL1 sealed stubs persisted in vault
namespace 4 (PMS2). Browse shows `SEALED N` only (ciphertext stays opaque).
On the merged `./dev test-net` image, Messages can hand a sealed demo datagram
to `dhcp_client` for host delivery (peer IP/port on the shared TX page; demo
default `10.0.2.2:1237`); a host toy router forwards to a second guest. Graphics
UTM stays offline. Dual-UTM Shared Network LAN is a manual sketch (see
[network-spike.md](network-spike.md)); not part of `./dev test-net`.

**Settings** is a seventh Clock-style workspace under the 2×3 grid: isolated PD,
static local-only status (`LOCAL ONLY` / `VAULT LOCAL` / `NO NET YET` /
`NO ALERTS YET` or `ALERTS RING N SMS M` after modem inject), no input endpoint
and no vault.

### How we “port” apps on seL4

seL4/Microkit does not run Linux binaries. The benefit is explicit isolation and
a small trusted kernel, so apps are **rewritten** against our vault/input/display
(and later net) interfaces. Porting SimpleX Chat means a new native client or a
future Linux guest VM—not installing the Haskell Linux build. See
[utm-vs-hardware.md](utm-vs-hardware.md#messaging-and-simplex-chat-long-term).

## Scope controls and security claims

Start with offline calculator, notes, contacts, and a modest calendar. Defer cloud
sync, a browser engine, multimedia editing, Android apps, an app store, Bluetooth,
NFC, biometrics, and GPS until there is a reason to add them. Simple apps become
small once graphics, input, storage and time are available; recurring reminders,
time zones, synchronization and background wake make them larger.

Use established cryptographic implementations. Security work includes entropy,
hardware-backed key lifecycle, encrypted import/export, trusted input/display,
authenticated updates, rollback rules, and recovery. A bootloader-unlocked development device does not
by itself provide an authenticated production boot chain; whether our own images
can be securely booted on Pixel needs a separate device-specific assessment.
[AOSP bootloader locking](https://source.android.com/docs/core/architecture/bootloader/locking_unlocking).

seL4's proofs apply to specified configurations and assumptions. Our custom apps,
drivers and policy do not inherit whole-system verification, and the current
Microkit manual describes ongoing verification work for its MCS configuration.
The current 25 ordered supervisor checks plus eight namespace assertions
demonstrate specific isolation and service behaviors, not a fully secure phone.
[Microkit verification status](https://docs.sel4.systems/projects/microkit/manual/2.3.1/#proofs).

A dependable phone release also needs ordinary and emergency-call behavior,
network-loss recovery, charging/thermal validation and a realistic firmware
maintenance plan. Any required carrier or product qualification belongs in a
distribution plan. A lab call demonstration does not establish those properties.

## Next concrete milestone

Treat **UTM as the product shell** before Pixel bring-up. Refresh
`./dev utm-graphics`, confirm tablet + amber guest cursor, and walk the seven
workspaces (see [utm-prep.md](utm-prep.md)). Keep the default serial profile
separate.

QEMU already covers launcher/Tab/app-bar navigation and app frames. Direct
pointer comfort is a manual UTM check. U6 net/TLS stays on `./dev test-net`
(graphics UTM remains offline). Pixel P0 stays a separate track
([pixel-p0-boot-feasibility.md](pixel-p0-boot-feasibility.md)): UART + backup
before any flash.

Optional next software slices on UTM/QEMU: fake modem / camera backends, or
production TLS/PKI beyond the smoke pin. Messages UI sealed TX over UDP + TLS is
already on the merged net image. See
[utm-vs-hardware.md](utm-vs-hardware.md) for what the emulator cannot prove.

Begin P0 as a bounded feasibility study alongside U3. Follow the
[P0 boot/debug checklist](pixel-p0-boot-feasibility.md); keep partition writes
out of scope until that report’s go/no-go table is filled. As of 29 September
2026 the desk half of P0 is largely done (boot v2 anatomy, DT map, Lineage
20260924 rollback kit on disk, sunfish scaffold) with a **conditional go**:
ADB refresh still shows `console=null` / UART disabled path; still need a USB-C
SBU UART capture and a confirmed userdata backup before any flash. A desk platform
scaffold lives in [`platforms/sunfish/`](../platforms/sunfish/README.md) (overlay
DTS + GENI putchar stubs; not linked into the QEMU build). The [Pixel key
feasibility note](pixel-key-feasibility.md) finds Titan M/StrongBox support in
the Android `sunfish` stack, but no demonstrated direct seL4 service path. The
first Pixel deliverable remains a documented boot/debug/driver strategy, not a
promised daily-driver date.
Our planning expectation is months for a modest UTM personal environment, with a
fully native, dependable Pixel phone potentially requiring years or a team and
remaining conditional on hardware feasibility.
