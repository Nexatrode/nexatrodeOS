# Pixel 4a Wi-Fi feasibility

Native implementation progress: [Sunfish native Wi-Fi startup](sunfish-native-wifi-startup.md).

## Result

The Pixel 4a (`sunfish`) Wi-Fi hardware and LineageOS stack are working. This
does not mean Wi-Fi is available in NexatrodeOS. The current NexatrodeOS network
service has QEMU VirtIO Ethernet support. The repository contains native WLAN
protocol code and diagnostic probes, but no functional Pixel WLAN driver or
firmware-loading path.

The [powered-off Linux reference test](sunfish-mobian-comparison.md#powered-off-linux-wlan-run-2026-10-25)
passed on this Pixel on 2026-10-25: modem/WLFW startup, scanning, WPA2 association,
DHCP and sustained traffic in both directions (82.95 / 122.21 Mbit/s). The
temporary image used verified firmware in RAM and RAM-shadow modem storage;
all four original EFS hashes remained unchanged. This is evidence for the Linux
host-side startup path, with resident secure firmware, rather than a native
seL4 driver result. The earlier metadata-only parser findings below describe
that parser's scope; the Linux comparison separately read the owner's firmware
through read-only source mounts and private RAM copies.

### Vendor firmware source plumbing status

The mounted LineageOS vendor partition was inspected through metadata only.
Its ext4 profile uses 4 KiB blocks, 256-byte inodes, 32-byte group
descriptors, extents, and group-descriptor CRC16 checksums. The observed
`/vendor/firmware/wlanmdsp.mbn` is a sparse file of 3,751,004 bytes with six
initialized extents. Its data bytes were not read.

`rust/src/sunfish_vendor_ext4.rs` now has a fail-closed read-only parser for
that exact path and the observed ext4 feature profile. It checks group
descriptor checksums and directory/inode/extent bounds, maps sparse or
unwritten ranges to zeroes, and reads payload data only when a caller requests
a range. Synthetic tests cover the directory walk, sparse extents, malformed
metadata, and partition-length drift. This parser does not establish that the
partition is a trusted source, authorize redistribution or execution, or
provide the production UFS block-I/O and SMMU-backed source required by the
TFTP server. A subsequent live metadata-only check opened the path on the
Android-provided `dm-1` block view while rejecting every read overlapping the
firmware's known data extents. No device firmware payload has been read,
served, or loaded.

`rust/src/sunfish_lpd.rs` now adds a bounded read-only parser for Android LP
metadata v10.0/v10.1. It validates the geometry, metadata header, table
digests, slot, block-device size, and extent bounds before exposing only the
active `vendor_a` or `vendor_b` mapping as a `ReadSourceIo`. The parser has no
block-device activation, data read during open, write, firmware authorization,
or AVB verification. Eight parser tests cover mapping, zero extents,
checksum failures, backup-copy recovery, slot rejection, length drift, and
extent bounds. A live `lpdump` check confirms the Pixel reports v10.1 metadata,
65,536-byte metadata slots, slot count 3, and a three-extent `vendor_a` of
688,500,736 bytes. The raw `super` device denies reads to the current ADB shell,
so this parser has not yet been run against raw on-device LP metadata.

## Native AOP mailbox observation (2026-10-17)

The latest platform-startup measurement is the AOP QMP mailbox. The captured
runtime DT and generated Sunfish kernel DT identify `qcom,qmp-aop@c300000`:
MSGRAM at `0x0c300000`, size `0x1000`, descriptor offset zero, and the separate
doorbell at `0x17c0000c`. The build checks those properties before packaging
the probe. The descriptor wire reference is the matched Lineage
`drivers/mailbox/msm_qmp.c`; the named PIL notification comes from
`drivers/soc/qcom/peripheral-loader.c`.

`rust/src/qcom_qmp.rs` now decodes the 64-byte descriptor and validates its
version, features, state words, mailbox alignment, checked bounds, descriptor
overlap, and RX/TX overlap. Packet-length validation includes the four-byte
length word in the mailbox capacity. The module also constructs the fixed
96-byte modem `load_state` payload with zero-filled padding in ordinary RAM.
It has no MMIO, doorbell, or command-submission operation. This notification
alone does not perform the regulator/clock sequence or report modem readiness.

`platforms/sunfish/build-aop-read-probe.sh` builds a native Microkit image that
maps only the AOP MSGRAM page read-only and the existing reserved splash
buffers read/write. It has no doorbell mapping, IRQ, channel, or SMC capability.
It compares consecutive descriptor reads (up to four retries), shows all link
and channel states plus mailbox geometry on screen/UART, and reads only the
two length words if the copied layout is stable and valid. It never reads a
message payload or changes acknowledgements. The screen remains until a
physical restart. If the first read faults, the previously painted
`AOP MAILBOX / READ START` screen identifies where it stopped.

The native read-only image was temporarily booted on 2026-10-17, and the user
provided photo `664D2184-3462-4211-92D2-E328BEA240A9_4_5005_c.jpeg`. It shows
stable reads, valid magic/version/features and geometry, AOP link UP, AP link
DOWN, both channels DOWN, and both packet lengths zero. RX offset/size are
`0xc0/0x68`; TX offset/size are `0x128/0x2d8`. All acknowledgements are DOWN.
The reserved descriptor word was not displayed. The observation is recorded
in `platforms/sunfish/aop-handoff.json`. No partition was flashed, and the user
subsequently restarted to LineageOS.

### Link/channel-only handshake observed

The image builds, all 177 Rust library tests and both probe capability checks
pass, and the CapDL audit reports 48 pages, 24 fixed reserved intervals,
11 intentional splash pages, and zero overlaps. The native ELF has no
undefined symbols. `fastboot getvar product` confirmed `sunfish`, and
`fastboot boot` accepted the handshake image on 2026-10-17 (send 0.572 s,
boot 2.099 s). No partition was flashed. Photo
`E5CC16FC-D06E-489F-B5A0-923C8CA5BA78_4_5005_c.jpeg` confirms `HANDSHAKE PASS`,
`LINK PASS`, `OPEN PASS`, and `CLOSE PASS`, with three doorbell notifications.
Both link states/acknowledgements are UP. The remote channel/acknowledgement
remain UP; the AP channel and its close acknowledgement are DOWN. Both packet
lengths remain zero. This is an acknowledged AP close, not a claim that both
channel halves are DOWN. The user confirmed a return to LineageOS afterward.
Artifact: `.cache/pixel-sunfish/aop-handshake-probe/fastboot.img`, SHA-256
`0d2965fc82fb078ac4ca099e34c8fad8a68be889f9267021559dd597b9d5614d`.

`rust/src/qcom_qmp_link.rs` implements a bounded link negotiation, channel
open, and channel close. The seven host tests exercise the measured starting
state, both remote-open orderings, AP-owned write indices, nonempty-mailbox
refusal, geometry changes, remote reset, and finite poll budgets. The engine
returns only named writes to descriptor words 4, 6, 9, and 11; it has no packet
submission operation. These are protocol tests, not hardware evidence.

`platforms/sunfish/build-aop-handshake-probe.sh` packages that engine as a
standalone native image. Compared with the read-only probe, it adds write
permission for MSGRAM and the single device page containing the doorbell.
Only offset `0x0c` in that page is written, with DT-checked mask 1 after an
ordering barrier. The probe pins the observed mailbox geometry, requires
the dormant AP starting state and both empty mailboxes, and checks stable
descriptor/length snapshots before progressing. Linux is not running during
this standalone AP test; no other AP endpoint is instantiated in its system
description. The observed descriptor alone does not prove ownership.

It sends no payload, does not clear length words, performs no modem power,
firmware, SMC, or DMA operation, and does not attempt recovery writes on failure.
It polls instead of taking an IRQ. The 65,536-poll stage limit and short nop
delays bound work, not elapsed milliseconds; EL0 counter reads are unavailable
in this Pixel configuration. The result stays on screen until physical restart.
`HANDSHAKE PASS` establishes only that AOP acknowledged link/open/close,
not that the modem or Wi-Fi is ready.

Probe errors 1–8 are the link engine's descriptor, initial-state, nonempty,
geometry, remote-link, local-state, work-limit, and terminal-state errors.
Native snapshot errors `0x10`, `0x11`, and `0x12` mean unstable snapshot,
invalid descriptor, and geometry different from the observation. Length
`FFFFFFFF` means not safely read or not stable. Firmware startup, full power
sequencing, GLINK, and SMMU containment remain separate work.

### Fixed OFF packet delivery observed

With the native control handshake observed, the next isolated test sent one
fixed `{class: image, res: load_state, name: modem, val: off}` notification.
This uses the matched PIL notification format and QMP transmit contract. It
does not request `on`, vote regulators/clocks, invoke PAS, load firmware,
initialize GLINK, or grant DMA access. This narrow transport test does not
clear any full modem-startup gate below.

`rust/src/qcom_qmp_tx.rs` constructs only that named OFF packet, with 96
payload bytes including zero padding. The link engine can now hold an open
channel until an explicit disconnect. The fixed Sunfish MMIO adapter is in
`rust/src/pixel_aop_mailbox.rs`, shared by both handshake/packet probes. It
rechecks the measured geometry, empty mailboxes, and fully connected state
immediately before writing; writes the complete payload before publishing
length; reads back length 96; then rings the DT-checked doorbell. It never
clears RX or TX lengths. The sender waits for AOP to clear TX length to zero,
then requests AP channel close and waits for its acknowledgement. The probe
calls the sender once, with no automatic retry or recovery writes. Unexpected
RX data, changed geometry/state, an unexpected length, or exhausted work
budget terminates it. Publication-readback failure also terminates without
clearing the packet or trying another doorbell.

`OFF CONSUMED` means only that the peer cleared the published length. There
is no semantic command-result response in this test; it does not prove AOP
accepted the notification, modem reset/power state, or Wi-Fi readiness.
`PACKET PASS` requires publication, consumption, and acknowledged AP close.
Failure remains visible until physical restart. Transfer errors `0x21–0x29`
identify invalid descriptor, disconnected channel, changed geometry, nonempty
mailbox, insufficient capacity, wrong stage, unexpected length, unexpected
RX data, and work-limit exhaustion. Native errors `0x30`, `0x31`, and `0x32`
mean failed publication readback, changed/busy state before publication, and
an unexpected link-engine step. Existing snapshot errors `0x10–0x12` retain
their meaning. A `FFFFFFFF` length is an unavailable/unstable reading.

Validation passes: 185 Rust library tests, three system-resource checks, and
23 standalone harness tests (20 protocol tests plus three tests of the actual
MMIO adapter against canary-filled host RAM). The adapter tests check that
only the 100-byte TX packet and doorbell word change, that a busy/disconnected
or changed-layout endpoint produces no write, and that handshake writes leave
payload and geometry untouched. These do not simulate AOP firmware or prove
device-bus ordering. The packet native image builds, has no undefined symbols,
and passes the CapDL audit: 50 pages, 24 fixed reserved intervals, 11 intentional
splash pages, zero overlaps. The refactored handshake binary also compiles;
the previously observed handshake image remains unchanged in its cache path.

Artifact: `.cache/pixel-sunfish/aop-packet-probe/fastboot.img`, SHA-256
`bd1e3aeaa7a975af583d8a04a821122ea062b38c227e7d304f8c1c26a81ee6e1`.
After the user's Lineage-ready confirmation, ADB identified `sunfish` and
`fastboot getvar product` confirmed the same device. `fastboot boot` accepted
the image on 2026-10-17 (send 0.578 s, boot 2.090 s). No partition was flashed.
Photo `567B2724-DD22-4A3D-A138-75444FFEC732_4_5005_c.jpeg` confirms `PACKET
PASS`, `LINK PASS`, `OPEN PASS`, `OFF PUBLISHED`, `OFF CONSUMED`, and `CLOSE
PASS`. The payload is `0x60` bytes (96), notifications total four, and both
final mailbox lengths are zero. Links and remote channel/ack remain UP;
AP channel and its close acknowledgement are DOWN. This is now native
hardware evidence of one QMP packet's publication/consumption, with the
semantic and modem-state limitations above. The user confirmed LineageOS
returned afterward. The structured observation is in `aop-handoff.json`.

## RPMh resource bring-up (2026-10-17)

The next platform dependency is the RPMh resource/voltage mapping and Apps
RSC command bank, not another AOP link scan. The matched modem node requests
`vdd_cx` and `vdd_mss` at consumer level `0x181` (TURBO, raw 384 plus the
regulator consumer offset 1), then enables RPMh CXO. The runtime providers
name command-DB resources `cx.lvl`, `mss.lvl`, and `xo.lvl`. A voltage request
must be translated through that resource's ARC auxiliary u16 table to a
hardware selector; writing the number 385 as a selector would be wrong.

The CX provider also contains a declared reference to MX (`mx.lvl`), but its
effective Linux dependency is unresolved: its property is named
`pm6150l-s2-level-parent-supply`, whereas the matched driver's
`rpmh_regulator_init_vreg_supply()` constructs the lookup from
`regulator-name = "pm6150l_s2_level"`, with underscores. Do not assume the
declaration becomes an active parent vote. The readout includes MX so its
table is available while the runtime consumer/dependency behavior is checked.

The user enabled Rooted debugging for a read-only inventory. `/proc/iomem`
shows `85f20000-85f21078 : c3f000c.qcom,cmd-db`, and the platform device is bound
to `cmd-db`. The matched `cmd-db.c` reads address and size from the dictionary
at `0x0c3f000c` and sets the inclusive resource end to start plus that size.
At that stage `0x1078` was an inferred dictionary size; the first native photo
below subsequently confirmed both dictionary words. The
raw inventory is cached under `.cache/pixel-sunfish/rpmh-readback-20261017/`,
and the scoped facts are in `platforms/sunfish/rpmh-handoff.json`. This Lineage
kernel exposes neither debugfs nor `/dev/mem`; no database bytes were copied
through those interfaces. ADB was returned to normal shell UID 2000.

`rust/src/qcom_cmd_db.rs` now decodes bounded copies of the command database:
144-byte header, eight resource headers, 24-byte entries, relative auxiliary
data, consumer-specific address/type validation, and unambiguous named lookup. It
checks table/auxiliary bounds and overlap, resource keys, and ARC size,
ordering, and zero padding. ARC selection applies the consumer offset and
chooses the least supported level meeting the requested minimum. Header
version/checksum are reported as opaque metadata; the parser neither
authenticates the database nor authorizes a vote.

`platforms/sunfish/build-rpmh-read-probe.sh` packages a native read-only probe.
Its three hardware mappings are the dictionary page, exactly two observed
command-DB pages at `0x85f20000`, and the first Apps RSC page at `0x18220000`.
Only the reserved splash buffers are writable. No AOP doorbell, SMC, IRQ,
channel, modem/WLAN memory, or DMA resource is mapped. It refuses a dictionary
pointer different from the observed address, oversized/unaligned length, or
unstable copy; it rechecks the dictionary after copying. It shows addresses
and TURBO selectors for CX/MSS/MX, XO address/version, and RSC configuration,
IRQ status, and the two DT-selected active TCS status words. The latter are
non-clearing reads; even an IDLE value does not establish ownership. The
probe does not touch IRQ-clear, control, command-enable, or command registers.

The memory audit still rejects every XBL/AOP overlap by default. This build
opts into exactly two command-DB pages only after checking its actual manifest
maps them uncached and read-only; writable aliases, other addresses, and larger
windows are rejected. Validation passes: 190 Rust library tests, seven AOP/RPMh
capability tests, native build with no undefined symbols, and CapDL audit
(67 pages, 24 fixed reserved intervals, 11 intentional splash pages, two
read-only command-DB pages, zero violations). Regression checks confirm the
previous packet image still passes the default audit and this new image is
rejected without the explicit read-only exception.

First artifact, archived at
`.cache/pixel-sunfish/rpmh-read-probe/observed-db-error-04/fastboot.img`, SHA-256
`805faa6f0261914ba3801c6af7db6869b3977d31dee69affefc23ac8149dc7ef`.
After the user's Lineage-ready confirmation and inventory, `fastboot getvar
product` confirmed `sunfish` and `fastboot boot` accepted this image (send
0.561 s, boot 2.372 s). No partition was flashed. Photo
`4EC21F20-498B-4CB0-B6B5-926CDEEC1579_4_5005_c.jpeg` shows dictionary base
`85F20000`, size `00001078`, and `DB ERROR 00000004` (`TableLayout`). The
known code path reaches this error only after a stable bounded copy and
dictionary recheck; the photo does not display the raw header or exact rule
that failed. No resource selectors or RSC state were obtained and no votes
were sent. The user confirmed LineageOS returned.

The follow-up parser now follows the matched Linux reader's termination
rule: the first zero slave ID ends the table list. Empty nonzero-slave slots
are skipped without validating or dereferencing their unused offsets. Disjoint
populated headers may share a hardware category; named lookup still rejects
ambiguous duplicate resource IDs. The original extra restrictions were not
supported by the source. This corrects parser behavior but does not yet prove
which restriction caused the device's error. Bounds, populated-table and
auxiliary overlap, resource keys, and ARC checks remain. A subsequent revision
below corrects where slave-type validation belongs.

`parse_detailed()` now reports the original header slot and optional entry
index. On failure the read-only screen displays `FAULT SLOT`, `FAULT ENTRY`,
all eight raw header summaries, and the failing entry's address/auxiliary
length and offset when readable from the existing RAM copy. `FFFFFFFF` for
slot/entry means unavailable/not applicable. `Tn SID COUNT` packs the slave ID
in the high 16 bits and count in the low 16; `Tn HDR AUX` similarly packs header
offset then auxiliary-data offset. Slots after the terminator are displayed
as raw diagnostic bytes only and are never used as resource tables. No new
MMIO reads or permissions are added for this diagnostic.

The revised image passes 194 Rust library tests, including regressions for
empty ranges, ignored trailing slots, repeated categories, preserved original
fault indices, and retained overlap/ambiguity refusal. Four probe-capability
tests and the native build pass. Its ELF has no undefined symbols; the CapDL
audit reports 68 pages, 24 fixed reserved intervals, 11 intentional splash
pages, two read-only command-DB pages, and zero violations.
Revised artifact: `.cache/pixel-sunfish/rpmh-read-probe/fastboot.img`, SHA-256
`04facbb7fe70bc79f0ef627bc9b1be839a8c405688ef605224a838f1be4cc159`.
After the user's Lineage-ready confirmation, `fastboot getvar product`
confirmed `sunfish`; `fastboot boot` accepted the revision (send 0.564 s,
boot 2.241 s). No partition was flashed.

The next user photo (`7A0995AF-3012-477B-AD1F-FA6D606E0377_4_5005_c.jpeg`)
confirms version 1, a stable copy, and `DB ERROR 6` (`SlaveId`) at slot 1,
entry 0. That entry has address zero and auxiliary length 2/offset 0; its ID
and purpose were not displayed. The four populated table headers are:

| Slot | Category | Entries | Header offset | Auxiliary offset |
| --- | --- | --- | --- | --- |
| 0 | 3 | 20 | `0x0` | `0x1e0` |
| 1 | 3 | 10 | `0x2e0` | `0x3d0` |
| 2 | 5 | 40 | `0x3e8` | `0x7a8` |
| 3 | 4 | 64 | `0x8e8` | `0xee8` |

All displayed fields for slots 4–7 are zero. Every populated entry-table span
ends exactly at its own auxiliary offset, consistent with the matched
24-byte entry layout and 144-byte database header. This is header evidence,
not a full database dump. The observed image/ELF/report are archived in
`.cache/pixel-sunfish/rpmh-read-probe/observed-db-error-06/`. No rail selectors
or RSC state were obtained. The user confirmed LineageOS returned.

Source audit shows the remaining error was another overstrict global rule:
`cmd-db.c` derives a resource's slave type from its address, without comparing
every record to its table category. `rpmh-regulator.c` checks the selected
resource's nonzero address and type against its configured consumer. The
decoder now separates structural bounds validation from consumer validation.
Zero-address or differently typed unrelated records can be inspected, but
`command_address(expected_slave)` rejects zero addresses and wrong types.
ARC level decoding requires a usable ARC address; the probe checks XO's ARC
address too. Raw table category remains available as metadata. This does not
identify the purpose of slot 1 entry 0 or grant resource ownership.

Validation passes 197 Rust library tests and seven AOP/RPMh isolation tests.
New regressions cover zero-address records, selected-resource type refusal,
and a synthetic 134-entry database with the photographed table geometry.
Only its geometry is observed; fixture names/addresses/auxiliary data are
synthetic. Fault readout now includes both raw ID words from the same RAM
copy. The native build passes with no undefined symbols; the CapDL audit
again reports 68 pages, two read-only DB pages, and zero violations. The new
temporary artifact SHA-256 is
`03ac8a10eea004602f57b9bd77bb3d2732fd036b48b7ba236fac6c83bd630c10`.
Its device result is pending; `platforms/sunfish/rpmh-handoff.json` records
the artifact, validation, and subsequent observations.
After confirming ADB device `sunfish` and ordinary shell UID 2000, rebooted
to bootloader and verified `fastboot getvar product` as `sunfish`.
`fastboot boot` accepted the new artifact (send 0.563 s, boot 2.096 s).
No partition was flashed; upload success alone does not establish a successful
resource decode.

Photo `6A393895-011F-45E1-8FA5-7189258FC952_4_5005_c.jpeg` now confirms
`READ ONLY DONE`, version 1, 134 entries (`0x86`), and `STABLE YES`:

| Resource | Command address | TURBO selector | Consumer level |
| --- | --- | --- | --- |
| CX | `0x00030000` | 6 | `0x181` |
| MSS | `0x00030060` | 7 | `0x181` |
| MX | `0x00030010` | 4 | `0x181` |
| XO | `0x00030090` | Not requested | Auxiliary version `0x0100` |

RSC config is `0x80008104`: eight TCSs in DRV2 and 16 commands per TCS.
IRQ status is zero; active TCS0 and TCS1 status are both 1 (idle). The photo
also confirms `NO VOTES SENT`; the user confirmed LineageOS returned. The
artifact/ELF/report are archived under
`.cache/pixel-sunfish/rpmh-read-probe/observed-read-only-done/`. Idle is a
state observation, not ownership proof. Native command submission/completion
and voltage/clock votes remain untested.

### First active-controller transaction candidate

`qcom_rpmh_read.rs` and `rpmh-cx-read-probe.system` implement one fixed CX
READ at the photo-confirmed address `0x30000`. This is **not read-only MMIO**:
the probe writes active TCS0 command/control registers, sets only its bit in
IRQ enable, and clears only that completion bit after successful completion.
The resource command has message ID `0x108`: length 8, response requested,
WRITE bit clear, data zero. There is no voltage/oscillator vote, arbitrary
resource operation, modem startup, SCM, firmware, GLINK, or DMA.

The register and read-command sequence come from the matched
`drivers/mailbox/qcom-rpmh-mailbox.c` and `drivers/soc/qcom/rpmh.c::rpmh_read`.
An upstream [RSC read-length patch](https://lkml.iu.edu/hypermail/linux/kernel/2609.1/06946.html)
distinguishes newer v4.5 hardware's four-byte reads from older hardware's
eight-byte reads; this Sunfish probe retains the matched older driver's
eight-byte format. Newer TF-A has a
[polled AMC implementation](https://github.com/ARM-software/arm-trusted-firmware/blob/master/drivers/qti/rpmh/rpmh_client.c)
for a different SoC/TZ DRV; that corroborates the polling pattern but supplies
neither Sunfish offsets nor AP ownership.

Exclusive AP ownership is a separate precondition: the temporary standalone
image replaces Linux, only one PD maps the DT-selected Apps DRV2 window,
and no competing AP driver runs in this system. Build checks pin the live-
derived DT's label, DRV ID, register layout, and active-first TCS allocation.
The engine additionally requires two equal snapshots, the exact observed
config, no pending IRQ, both active banks idle with no enabled commands or
wait bits, and no unknown control bits before writing. It never chooses a
sleep/wake bank as fallback. The single 4 KiB RSC mapping ends before the
first sleep TCS; memory capabilities enforce page boundaries, while trusted
driver code and adapter tests enforce the narrower register-write footprint.

Completion requires the selected IRQ bit, command issued+complete flags,
idle status, unchanged READ/address/data, and unchanged other-bank state.
Cleanup disables command0/AMC, clears only IRQ bit0, restores the inherited
IRQ-enable value, and checks the resulting state before releasing a response.
An inherited high trigger bit is never restored, because that could retrigger
the command. A timeout or uncertain completion is terminal: no retry,
in-flight cancellation, broad clear, or invented response. Work limits are
poll counts, not milliseconds. The screen holds until physical restart.

This candidate is a transport prerequisite; a returned value does not prove
correct power sequencing, modem readiness, Wi-Fi operation, or secure DMA.
The response is opaque; do not assume an aggregate resource read identifies
this DRV's previous vote or supplies a safe voltage rollback value.

Validation: 202 Rust library tests, nine engine/actual-adapter tests (five
overlap the library suite), eight probe capability tests, native build with
no undefined symbols, and a default CapDL audit of 48 pages/zero overlaps.
The only reserved-memory exception is the existing splash; this image does
not map command-DB or other XBL/AOP RAM. Artifact:
`.cache/pixel-sunfish/rpmh-cx-read-probe/fastboot.img`, SHA-256
`04325b48ef157a488647472da88e3005dda20daa31f41beb4642b787b567a628`.
After the user's Lineage-ready confirmation, ADB reported `sunfish` and UID
2000; rebooted once to bootloader, confirmed product `sunfish`, and temporarily
booted this artifact. Fastboot accepted it (send 0.545 s, boot 2.091 s).
No partition was flashed.

Photo `CEE34521-92FB-48D7-9A5A-1F4CAE34D78A_1_102_a.jpeg` confirms
`CX READ PASS`, `RESPONSE AND CLEANUP OK`, READ message `0x108`, CX address
`0x30000`, and response `5`. Before and after: IRQ enable 1, IRQ status 0,
idle 1, command enable/wait/control 0. Final command status is 0 after
cleanup; issued+complete flags were required by the passing code path before
cleanup, not captured as nonzero flags in the photo. The other active bank
remains idle with enable/wait/control 0. No power vote or modem start occurred.
The user confirmed LineageOS returned. The observed artifact/ELF/report are
archived in `.cache/pixel-sunfish/rpmh-cx-read-probe/observed-cx-read-pass/`.

### Repeated reads and Linux regulator relationships

The follow-up `rpmh-resource-reads-probe` selects only four typed addresses
from the observed command DB: CX, MSS, MX, XO. It reads each once, then repeats
the same four reads, with a new bounded transaction and checked cleanup for
each of eight commands. It stops at the first error, preserves earlier
responses as partial results, and prints failed address/pass, error, command
status, and controller state. Missing results are explicitly `NOT COMPLETED`.
Different first/second responses are allowed; no fixed power state is assumed.
The original CX-only constructor/bin still builds for comparison. The new
manifest has the same first-page RSC/splash-only authority and no resource
WRITE/vote, sleep/wake access, SMC, modem startup, or DMA.

Validation passes 205 Rust library tests, 12 engine/actual-MMIO tests (eight
overlap the library suite), eight probe capability tests, both native bin
builds, no undefined symbols, and a CapDL audit of 48 pages with zero overlaps.
Regressions verify sample/address ordering with distinct responses, cleanup
before reuse, stopping after each possible timeout position, preservation of
unrelated pending IRQs, and both probe manifests' mapping limits. The new
artifact is `.cache/pixel-sunfish/rpmh-resource-reads-probe/fastboot.img`,
SHA-256 `525136cc7d17717bdbdee99f4f4eeb93f21038cd5385d528d2da35e7b03d7488`.
After confirming the attached Pixel as `sunfish` and ordinary ADB UID 2000,
rebooted once to bootloader, confirmed fastboot product `sunfish`, and
temporarily booted this artifact. Fastboot accepted it (send 0.564 s,
boot 2.090 s); no partition was flashed.

Photo `3655BF78-463A-4D7B-BC7F-75954FBA78D4_4_5005_c.jpeg` confirms
`RESOURCE READS PASS`, eight completed reads, and equal first/second responses
CX=5, MSS=0, MX=3, XO=3. Final command status/IRQ status are zero, both active
banks are idle, enable/wait/control are zero, and IRQ enable is restored to 1.
The screen confirms no modem start or power votes. The artifact/ELF/report
are archived in
`.cache/pixel-sunfish/rpmh-resource-reads-probe/observed-resource-reads-pass/`.
ADB subsequently confirmed booted LineageOS `23.2-20261001-NIGHTLY-sunfish`,
product `sunfish`, boot-completed 1, shell UID 2000. This confirms repeated
native controller transactions, not safe power rollback values.

A fresh rooted read-only Lineage inventory is cached at
`.cache/pixel-sunfish/rpmh-readback-20261017/regulators.txt`. ADB was returned
to UID 2000. It reports CX normal/AO providers enabled (consumer-level
attributes 129/65, one user each), MX normal/AO disabled (17, zero users), and
MSS disabled (17, zero users). These ARC `microvolts` attributes are consumer
levels, not physical voltages or native-boot hardware baselines. The MX
directories contain no CX `regulator.8-SUPPLY`/`regulator.9-SUPPLY` links;
the parent DT property names still contain hyphens while regulator names use
underscores. The source creates a parent consumer via those `SUPPLY` links.
Together these observations support no effective CX-to-MX framework parent
in this snapshot; link creation can fail nonfatally, and the inventory does
not establish or exclude an AOP/hardware-level rail coupling. A parent vote
must not be invented from the mismatched DT declaration.

### First fixed CX vote candidate

`qcom_rpmh_power.rs` implements one restricted hold experiment. It requires
two successful baseline passes with exactly the observed CX/MSS/MX/XO tuple
`5/0/3/3`; different starting values, including a higher CX response, cause
refusal before any resource WRITE. It then issues one fixed active CX TURBO
request: address `0x30000`, selector 6, message ID `0x10108`. The selector is
the command-DB mapping for consumer level 385, not a guessed voltage or raw
consumer-level write. This follows the matched `rpmh_regulator_arc_set_voltage_sel`
and aggregate request path. A public read facade still constructs only READs;
the shared transaction engine accepts just typed READs or this crate-private
fixed CX command. No arbitrary WRITE address/data API is introduced.

After checked acknowledgement and controller cleanup, the probe reads all
four resources twice more. Success requires both CX readbacks to equal 6;
the other responses are displayed without inventing cross-rail constraints.
Only one resource WRITE is possible, with no writes to MSS, MX, XO, sleep/wake
sets, modem reset, firmware, or DMA. The same single-owner first-page RSC and
splash mappings are used. Unknown completion is terminal and never retried.

The request is deliberately held until physical restart. There is no automatic
unvote or restoration from the opaque baseline reads, and no claim to preserve
a physical rail level across firmware power-state transitions. This is a
first write/ack/readback experiment, not the complete modem proxy sequence,
boot/shutdown service, safe rollback design, or a production power policy.
The existing bus-vote/dependency/PAS and DMA-containment gates remain.

Validation passes 210 Rust library tests, 13 engine/actual-MMIO tests (eight
overlap the library suite), eight capability tests, all three native probe
bin builds, no undefined symbols, and a CapDL audit of 50 pages/zero overlaps.
Tests cover every timeout position among 17 requests, including a write that
takes effect but loses its acknowledgement; no retry, compensating vote, or
later read is sent. They also cover mismatched/two-pass starting values,
unchanged readback despite acknowledgement, and the actual adapter's fixed
CX-only WRITE footprint. Artifact:
`.cache/pixel-sunfish/rpmh-cx-turbo-hold-probe/fastboot.img`, SHA-256
`18537129befc89f33bfb3c14eea8f0a4f362970c51e2f599c9012fb6383d4601`.
ADB verified booted LineageOS/product `sunfish`/UID 2000 without requiring a
new manual restart. Rebooted once to bootloader, verified fastboot product
`sunfish`, and temporarily booted the candidate (send 0.563 s, boot 2.099 s).
No partition was flashed.

Photo `578695D6-7165-4F25-BD8C-D53238E5AC8F_4_5005_c.jpeg` confirms
`CX TURBO HOLD PASS`, submitted/acknowledged 1, CX before 5 and both CX
readbacks 6. MSS/MX/XO remain `0/3/3` in both rounds. IRQ status and TCS enable,
wait/control are clear, TCS idle and other-bank idle are 1, and IRQ enable
is restored to 1. The resource command remains held; no automatic unvote or
modem start is implemented. The user restarted to LineageOS; ADB confirmed
`sunfish`, Lineage 23.2, boot-completed 1 and shell UID 2000. The observed
artifact/ELF/report are archived under
`.cache/pixel-sunfish/rpmh-cx-turbo-hold-probe/observed-cx-turbo-hold-pass/`.
This is the first observed fixed native resource WRITE/ack/readback, not a
complete power manager or modem boot.

### CX/MSS/XO resource hold: observed pass

`hold_modem_resources()` extends the observed CX-only experiment with two
fixed typed requests: MSS TURBO selector 7 at `0x30060`, then XO-on value 3
at `0x30090`. CX selector 6 and MSS selector 7 are the observed ARC mappings
for consumer level 385. XO-on 3 is the matched `clk-rpmh.c` definition for
`xo.lvl`; the build checks the live-derived modem's proxy rail/clock names
and levels, command-DB observations, and the source clock definition.

The coordinator reuses the checked CX baseline/write/readback flow. Before
MSS, both passes must show CX6/MSS0/XO3. After the MSS request it reads all
resources twice and requires CX6/MSS7/XO3 before requesting the already-on
XO3. It then reads all four resources twice again. Success means all three
requests acknowledged with checked controller cleanup and the expected
CX6/MSS7/XO3 final readbacks. MX is reported but never written or used to
invent a cross-rail rule. The maximum is 35 requests: three fixed WRITEs and
32 READs, all on active TCS0. Every first fault is terminal; no later stage,
write retry, guessed restoration, or automatic unvote is issued.

The XO response already equals 3 before this test. An acknowledgement and
unchanged readback can confirm the request path; they do not demonstrate an
oscillator off-to-on transition, unique ownership, frequency measurement, or
complete clock/power policy. The requests remain held until physical restart.
This image has the same RSC-first-page/splash-only mappings, with no SMC,
PAS, reset, firmware load, AOP-on, bus vote, sleep/wake reprogramming, or DMA.
`RESOURCE HOLD PASS` must not satisfy the lifecycle model's complete platform-
power precondition by itself.

The final live-derived Sunfish modem bus vectors were also checked directly:
`<129 512 0 0>` and `<129 512 0 7000000>` KBps (`0x6acfc0` peak). The earlier
common-DTS peak 1041408 is not the final Sunfish value. A native bus provider
and the secure/PAS bandwidth requirements remain unimplemented; these bus
IDs and bandwidth numbers are not raw RPMh register addresses or command data.

Validation: 214 Rust library tests, 13 engine/actual-MMIO tests (eight overlap
the library suite), eight capability tests, all four native probe bins,
no undefined symbols, CapDL audit 52 pages/zero overlaps. Regression tests
cover all 35 timeout positions, ambiguous applied-but-unacknowledged MSS/XO
requests, missing MSS readback, changed XO state blocking the clock request,
and MX response changes without an MX WRITE. The actual adapter is tested
against all three fixed WRITE address/data footprints.
Artifact `.cache/pixel-sunfish/rpmh-resources-hold-probe/fastboot.img`, SHA-256
`85e2e066e97f344706faad8560362d7cb9c44907cf6e86690a447a87c72f08c6`.
After the user's Lineage-ready reply and ADB confirmation of booted
`sunfish`/Lineage 23.2/UID 2000, rebooted once to bootloader, verified product
`sunfish`, and temporarily booted the candidate (send 0.563 s, boot 2.100 s).
No partition was flashed. Photo `81AD6C97-2384-48A1-84B8-F8AFA777FF5A_1_201_a.heic`
confirms `RESOURCE HOLD PASS`, all three acknowledgements, submitted mask 7,
CX 5->6, MSS 0->7, and two final CX6/MSS7/MX3/XO3 readbacks. IRQ status,
TCS enable/control are zero; both active banks are idle and IRQ enable is 1.
The matching image, ELF, report, and inspection PNG are archived in
`.cache/pixel-sunfish/rpmh-resources-hold-probe/observed-resource-hold-pass/`.
The user restarted to LineageOS; ADB confirms `sunfish`, Lineage 23.2,
`boot_completed=1`, UID 2000. This establishes the restricted active request
sequence, not modem startup or working Wi-Fi.

### Bus metadata and remaining bandwidth work

`validate-bcm-routes.py` checks the final live-derived SDK DT, including node
IDs, widths, aggregation-port counts, edges, absence of path blacklists,
BCM bindings, Apps RSC ownership references, and any modem crypto-master
override. The modem proxy route is master LLCC 129 -> EBI 512, with `MC0`
and `ACV` on EBI. The PAS reference route from crypto master 125 traverses
`mas-qxm-crypto`, `slv-qns-a2noc-snoc`, `mas-qnm-aggre2-noc`,
`slv-qns-gemnoc-sf`, `mas-qnm-snoc-sf`, `slv-qns-llcc`, `mas-llcc-mc`,
and `slv-ebi`. Its additional BCMs are `CE0`, `SN11`, `SN0`, and `SH0`.
These are checked reference paths, not a runtime Linux bus client or evidence
that hardware QoS, interconnect state, or firmware-loader requirements are met.

The matched `subsys-pil-tz.c` changes the PAS source to crypto master 125
unless overridden; its nonzero PAS vector is AB 393600000 / IB 3936000000
bytes/second. The modem's separate vector is AB 0 / IB 7000000 KBps, converted
by Linux with decimal 1000. Neither quantity is directly writable command data.

The [matched bus driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/msm_bus/msm_bus_fabric_rpmh.c)
reads eight auxiliary bytes per BCM: LE u32 unit size, LE u16 width, u8 virtual
clock domain, u8 reserved. It packs two saturating 14-bit vectors, valid bit 29,
and commit bit 30, with commit grouping by virtual clock domain. The separate
arbiter scales per-node requests by BCM width / bus width (average bandwidth
also by aggregation ports), then by the BCM unit. Its division helper floors
after preserving a positive sub-unit request as 1; it is not general ceiling
division. `ACV` uses Apps mask 8 after aggregation, rather than the numeric
bandwidth vector. Hardware integration, boot handoff votes, QoS state,
and sleep/wake preservation still need implementation before issuing BCM
WRITEs. Metadata is not ownership or a rollback baseline.

The next temporary image, `bcm-read-probe`, reads only the fixed dictionary
page and observed two-page DB window, plus writes the reserved splash buffer.
It has no RSC mapping, SMC capability, channels, or IRQs. It takes a stable
bounded DB copy and reports the six selected BCM addresses, unit sizes,
width/domain, and auxiliary-version/reserved fields. The decoder refuses a
wrong address type, non-eight-byte auxiliary record, or domain outside the
driver's 16 lists. Zero unit/width are now observable raw fields; numeric
planning separately rejects a zero unit or zero width on a non-ACV resource.
Raw version/reserved fields remain opaque.
`METADATA PASS` means structural decode only, never a vote or modem startup.
Each resource has four rows: `ADDR`, `UNIT`, `WIDTH VCD` (`width<<8 | domain`),
and `VER RSV` (`auxiliary_version<<16 | reserved`).

The first image (SHA-256
`845a54459660f086fbce36b07042201399fec7a08f292b93df5b6abccdb7bd2a`)
was accepted by temporary boot (send 0.562 s, boot 2.104 s). Photo
`C1F06CDE-07C6-41E3-A5EE-906B7D436CB1_4_5005_c.jpeg` shows `METADATA FAIL`:
our decoder's generic width check rejected ACV with error 3. That code path
establishes width zero and nonzero unit, but hid the other ACV fields; it
does not establish a failed bus transaction, because none was attempted.
Five other records decoded:

| Resource | Address | Unit size | Width | VCD |
| --- | --- | --- | --- | --- |
| MC0 | `0x50000` | `0x001b0210` | 16 | 0 |
| ACV (revised probe) | `0x5000c` | `0x00000001` | 0 | 11 |
| CE0 | `0x500a0` | `0x00001a92` | 8 | 6 |
| SN11 | `0x5006c` | `0x00081650` | 16 | 3 |
| SN0 | `0x50078` | `0x00081650` | 16 | 3 |
| SH0 | `0x5003c` | `0x001fe410` | 32 | 1 |

The first five decoded records show auxiliary version `0x1000` and reserved
byte zero. The image,
ELF, report, route reference, and photo are archived under
`.cache/pixel-sunfish/bcm-read-probe/observed-acv-width-error-03/`.
The user restarted to LineageOS and reconnected direct USB; ADB confirms
`sunfish`, Lineage 23.2, `boot_completed=1`, UID 2000.

The revised raw reader exposes zero-width ACV metadata. The separate pure
`qcom_bcm_vote` module implements checked average-sum/peak-max client
aggregation, node width/port scaling, active/dual member aggregation,
positive-subunit floor division, post-scaling ACV mask handling, 14-bit
saturation reporting, and VCD commit grouping. It has no MMIO backend and
does not discover clients, cached votes, or handoff ownership. In particular,
zero-width ACV scales to zero in isolation. The initial isolated-resource model
did not include the legacy caller's cross-BCM accumulator carry; the correction
below now handles EBI's ordered MC0 -> ACV bindings. Prior ACV masks are explicit
inputs; a synthetic zero mask is never treated as a discovered baseline. The
hardware probes preserve ACV instead of writing it from this model.

Validation: 225 Rust library tests, manifest capability test, checked final
DT routes, native build with no undefined symbols; CapDL 67 pages, two
read-only DB pages, 11 reserved splash pages, zero overlaps. New tests cover
width-zero ACV observation, observed MC0's 7 GB/s conversion to vector 15819,
context/member aggregation, overflow and invalid divisors, exact commit and
valid bits, saturation, and duplicate-resource refusal. This verifies
arithmetic/planning, not an installed bus provider or hardware bus vote.
The revised native result is `METADATA PASS`. Artifact SHA-256
`5f2be13ad6d445aae22588b41659d13d2ad9b2188a800f435760dbc4291950b1`
was temporarily booted after ADB confirmed booted Sunfish/UID 2000 and
fastboot confirmed product Sunfish (send 0.568 s, boot 2.101 s, exit 0).
No partition was flashed. Photo
`D2520B77-631F-4EC9-9468-8CDECCEBEA63_4_5005_c.jpeg` confirms all six records,
including ACV address `0x5000c`, unit 1, width 0, VCD 11, auxiliary version
`0x1000`, and reserved 0. The other five records match the earlier photo.
The matching image, ELF, report, route reference, and photo are archived in
`.cache/pixel-sunfish/bcm-read-probe/observed-metadata-pass/`. After the
user's restart, ADB confirms booted Sunfish/Lineage 23.2/UID 2000.

### Read-only TCS handoff snapshot

The next step inspects the programmed Apps DRV2 banks before implementing bus
requests. `qcom_rpmh_handoff` has a typed read-only register interface and
captures active TCS0-1, sleep TCS2-4, and wake TCS5-7. It pins observed
config `0x80008104`, checks 16-bit enable/wait masks, and retains message,
address, and data only for enabled command slots. Disabled slots are absent,
not assumed cleared. It does not read the hidden/control TCS or IRQ-clear
register. The highest possible read is command15 DATA in TCS7, offset
`0x20c4`, inside a three-page `0x18220000..0x18223000` mapping.

Two consecutive equal captures are required. At most three pairs are read
(2514 register reads in the fully populated case); changed geometry/masks
stop immediately, and an unstable result is refused. Equality is not an atomic
snapshot or proof that secure firmware can no longer change state. IRQ enable,
IRQ status, bank enable/wait/control/idle, and enabled payloads are retained.
No register can be written through this interface, and the native manifest
grants controller pages read-only. Only the reserved display is writable;
there is no SMC, modem memory, command DB, channel, or IRQ capability.

The report shows `A0/A1`, `S2/S3/S4`, and `W5/W6/W7`, with three rows per
bank: `EN WAIT` (`enable<<16 | wait`), `CONTROL`, and `IDLE`. Config and IRQ
status follow. Enabled payloads are also logged after the complete screen is
flushed. A stable report does not establish per-resource current votes,
all bus clients, QoS state, a rollback baseline, or authority to clear any
existing command. The same snapshot code can later check preservation in the
driver; this image only observes and holds until physical restart.

Validation: 232 Rust library tests, eight standalone handoff/MMIO tests (seven
overlap the library suite), manifest capability test, native build with no
undefined symbols; CapDL 65 pages, 11 intentional splash pages, zero reserved
overlaps. Tests cover changed geometry before bank reads, invalid masks,
index bounds, the last wake command, unknown payload preservation, disabled
slot omission, absence of IRQ-clear reads, changing payloads, bounded attempts,
and an actual adapter RAM test that preserves every word. Device result is
pending. Artifact SHA-256
`27aa41721ff995fc14d0b86aaf79f5204cbd2d26466ba060ececd79241a85f45`
was temporarily booted after ADB confirmed booted Sunfish/UID 2000 and
fastboot confirmed product Sunfish (send 0.564 s, boot 2.098 s, exit 0).
No partition was flashed. Photo
`4E876708-9481-4180-A7FA-50FA1C6D6E39_4_5005_c.jpeg` confirms `READ ONLY STABLE`,
config `0x80008104`, IRQ status zero, and all eight banks with enable/wait/
control zero and idle 1. The image, ELF, report, and photo are archived under
`.cache/pixel-sunfish/rpmh-handoff-probe/observed-read-only-stable/`.
The user restarted to LineageOS; ADB initially saw the boot still completing,
then confirmed Sunfish/Lineage 23.2/`boot_completed=1`/UID 2000. Empty banks
do not establish zero latched BCM votes.

### Restricted BCM resource reads

Before any bus WRITE, the next image queries the six observed BCM addresses
using the [matched RPMh resource-read path](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/rpmh.c).
That path uses one active-only payload, address plus zero data, `is_read=true`,
and waits for the mailbox reply. The native facade exposes only MC0, ACV,
CE0, SN11, SN0, SH0; no arbitrary address/data or BCM WRITE constructor.
The existing transaction engine sends message `0x108` (resource WRITE clear),
checks completion, cleans only its own active0 command/IRQ bit, and restores
IRQ enable. It retains bounded polling instead of the reference's long wait.
This is a controller transaction with first-page register writes, not a
physically read-only MMIO snapshot.

`read_bcm_series` performs two fixed passes (12 requests). Before every READ
it requires a stable empty/idle/untriggered eight-bank snapshot matching the
first preflight. After each completed transaction and cleanup it takes another
stable snapshot and requires exact equality before publishing that response
or issuing a later request. There are 24 successful preservation checks.
Every first failure stops the sequence; a timeout is never retried, cancelled,
cleared speculatively, or turned into a guessed vote/unvote. Uncompleted
responses are explicitly absent, including after a preservation failure.

The combined adapter's one pointer maps physical `0x18220000..0x18223000`
contiguously. Only the first page is writable for active0 transactions; the
two tail pages containing sleep/wake programming are read-only in the manifest.
The full-snapshot interface itself has no write method. The single standalone
PD has no SMC, other device, modem memory, DMA, channel, or IRQ capability.
Exclusive AP DRV2 ownership remains a separate caller/build precondition;
an empty controller does not create that authority.

`BCM READ PASS` will mean 12 checked raw replies with preserved bank
programming. Values remain opaque until the Sunfish response encoding and
ownership semantics are confirmed; they are not automatically an AP-local
rollback baseline or permission to replace bus votes. No bandwidth vote,
rail/clock WRITE, AOP-on, PAS/reset, firmware, or modem start is included.

Validation: 238 Rust library tests; 16 combined-adapter/core tests (15 overlap
the library suite), manifest permission/contiguity test, checked final DT bus
routes, all five old/new transaction probe bins build, no undefined symbols;
CapDL 69 pages, 11 intentional splash pages, zero reserved-memory overlaps.
Regressions exercise all 12 timeout positions, all eight banks' dirty states,
each post-reply wake-bank change, changed preflight IRQ-enable state, unstable
captures, and zero work budget. The actual combined adapter RAM test checks
each of the six READ footprints and unchanged controller tail words. Device
result is now observed as `BCM READ PASS`. Artifact SHA-256
`053c69e6cd1c2541524c7ebebd7babcf9d9fd8d4c0c7a4d3ab338c63703b022a`
was temporarily booted after ADB confirmed booted Sunfish/UID 2000 and
fastboot confirmed product Sunfish (send 0.575 s, boot 2.099 s, exit 0).
No partition was flashed. Photo
`87A6739A-D55F-4711-B379-5D8453842166_4_5005_c.jpeg` confirms 12 completed/
submitted reads, 24 preservation checks, IRQ status0, active0 idle1 with
enable/control0, IRQ enable1, and `BANKS PRESERVED`. Both passes match:

| Resource | Raw reply | Valid | X | Y |
| --- | --- | --- | --- | --- |
| MC0 | `0x29917f75` | yes | 9797 | 16245 |
| ACV | `0x20000008` | yes | 0 | 8 |
| CE0 | `0x00000000` | no | 0 | 0 |
| SN11 | `0x213c44f1` | yes | 1265 | 1265 |
| SN0 | `0x213c44f1` | yes | 1265 | 1265 |
| SH0 | `0x240d5adf` | yes | 4149 | 6879 |

Field decode uses the matched WRITE layout (valid29, commit30, X[27:14],
Y[13:0]); all six have commit0 and no other bits. This source-aligned decode
does not establish a complete local/per-client ownership inventory or an
automatic rollback value. It demonstrates why empty TCS banks cannot be
treated as zero latched requests. The matching image, ELF, report, route
reference, and photo are archived under
`.cache/pixel-sunfish/bcm-reads-probe/observed-bcm-read-pass/`.
The user restarted to LineageOS; ADB confirms Sunfish/Lineage23.2/
`boot_completed=1`/UID2000.

### Fixed MC0 reassertion: observed pass

MC0's observed peak field 16245 exceeds the modem proxy's calculated minimum
15819. Writing the isolated proxy word `0x60003dcb` would reduce the observed
X and Y fields. The first WRITE experiment instead reasserts the existing
numeric request: address `0x50000`, fixed word `0x69917f75` (observed
`0x29917f75` plus commit bit30). No caller-supplied address/data is accepted.
ACV is read but never written: readback checks require its observed Apps mask
0x8 (bit3) remain intact, avoiding a WRITE from the earlier isolated model.

`hold_mc0()` first requires two exact matches of the entire six-resource tuple
above. It then captures the same empty eight-bank programming, sends exactly
one fixed MC0 WRITE through active0, waits for issued+complete and IRQ0/idle,
and verifies cleanup. This bus commit sets TCS wait bit0 to 1, matching the
reference BCM command's completion flag; old ARC/XO/READ commands still use
wait0. Cleanup checks wait0 again. The post-WRITE programming must match the
pre-WRITE snapshot before another two full BCM read passes are attempted.

Success requires acknowledgement, unchanged MC0 valid/X/Y in both final
replies (commit is treated separately as a control flag), all other five raw
replies unchanged, and 50 successful bank-state checks. At most 25 requests
are issued: 24 READs and one WRITE. A differing initial tuple stops before the
WRITE; every first controller or preservation failure stops later requests.
There is no retry, cancellation, restoration, automatic unvote, other resource
WRITE, AOP-on, firmware, PAS/reset, or modem start. The request stays held
until physical restart. This is an unchanged-numeric-request experiment,
not proof of a bandwidth increase, complete bus aggregation/QoS policy, or
full modem power sequencing. Baseline checks are not an atomic compare-and-swap
or a complete ownership proof; standalone AP DRV2 ownership remains required.

Validation: 247 Rust library tests, 17 actual-adapter/core tests (15 overlap),
permission/contiguity checks for both bus manifests, final DT route checks,
all six transaction probe bins build, no undefined symbols; CapDL 69 pages,
11 intentional splash pages, zero reserved overlaps. Tests cover the observed
field decode/round trip, reserved-bit refusal, exact committed word and wait
footprint, all 25 timeout positions including applied-but-unacknowledged WRITE,
each changed starting resource, changed sleep/wake programming, changed other
resource readback, possible sticky commit control flag, and zero work budget.
The user's photo `EF19A397-AA3B-45F3-8251-BE54AE3BF5EC_4_5005_c.jpeg`
confirms **MC0 HOLD PASS**: WRITE submitted/acknowledged1, preservation count
0x32 (50), MC0 before/after/after-two0x29917f75, other five resources unchanged,
IRQ status0, idle1, enable/control0, IRQ enable1. The result banner also requires
both final six-resource passes to match. Artifact SHA-256
`8b95cf3a1db62bf840069a892c9083ee14c71a47b1ba6ddc8b84ba2a448e0653`
was temporarily booted after ADB confirmed booted Sunfish/UID2000 and fastboot
confirmed product Sunfish (send0.564 s, boot2.102 s, exit0). No partition was
flashed. The image, ELF, report, route reference, and photo are archived under
`.cache/pixel-sunfish/bcm-mc0-hold-probe/observed-mc0-hold-pass/`. ADB subsequently
confirmed Sunfish/Lineage23.2/boot_completed1/UID2000.

### Combined active-resource hold: observed pass

`qcom_rpmh_proxy_hold.rs` now combines the observed MC0 experiment with the
observed CX6/MSS7/XO3 sequence. It first completes the exact MC0 hold and bus
readback gates, reads the initial rail tuple twice, and then sends CX, MSS, XO
in order, checking acknowledgement/cleanup and two rail read passes after each
step. Full eight-bank programming must match the original stable handoff before
and after every request. Final BCM replies are read twice. A pass requires the
same MC0 valid/X/Y (commit flag separate) and unchanged other five replies.
This final bus comparison is a strict experimental postcondition, not a claim
that rail changes can never affect BCM state. Changed replies are retained for
diagnosis. MX is displayed but is neither written nor constrained after CX.

The bounded sequence has at most 72 requests: 68 READs and four fixed WRITEs
(MC0 at request13, CX34, MSS43, XO52), with 144 successful bank checks. Any first
timeout, controller fault, changed bank, or failed readback gate stops all later
requests. There is no retry, compensating vote, automatic unvote, AOP-on,
PAS/reset, firmware load, or DMA; requests remain held until physical restart.
The probe retains first-page RSC RW and two tail pages RO, with no SMC capability.
It is not a complete modem proxy transaction or proof of working Wi-Fi.

Validation: 255 Rust library tests, including every one of 72 timeout positions,
every bank changed after each of the 47 added requests, changed handoff before
each added request, starting/readback gates, changed final bus replies, and
observable MX coupling without an invented write. The 17 adapter/core tests and
permission checks for all three bus manifests pass; seven native transaction
bins build. Candidate hash, memory audit, and temporary-boot observations are
recorded in `platforms/sunfish/rpmh-handoff.json`. Candidate SHA-256
`80a14292e50245f61a0aed4d52cbd1440861008187c91b98fb34a4c005610cda`
was accepted by temporary `fastboot boot` after ADB confirmed booted Lineage
and fastboot confirmed product Sunfish (send0.567 s, boot2.096 s, exit0).
No partition was flashed. Photo
`CFFF0DC6-098B-4CBF-B8D5-072EFE4A4AA5_4_5005_c.jpeg` confirms **COMBINED HOLD
PASS**, phase6, ACK mask0xf, 72 requests (0x48), 144 checks (0x90), bus difference
mask0, IRQ status0 and TCS enable0. Final rails are CX6/MSS7/MX3/XO3; MC0 is
0x29917f75 in both displayed replies and all other bus fields remain unchanged.
The PASS banner also requires the submitted mask0xf and all intermediate gates.
The image/ELF/report/reference/photo are archived under
`.cache/pixel-sunfish/rpmh-proxy-hold-probe/observed-combined-hold-pass/`.
The user restarted to LineageOS; ADB confirmed Sunfish/Lineage23.2/boot_completed1/
UID2000. A subsequent read-only rooted inventory was completed and ADB returned
to UID2000.

### Legacy bus arithmetic correction and PAS-route floor candidate

Review of the [pinned arbiter's `bcm_update_bus_req`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/msm_bus/msm_bus_arb_rpmh.c)
shows its maximum accumulators are initialized before the loop over the calling
node's BCM bindings, then divided in place by each BCM unit. The next binding
therefore inherits the previous numeric result. EBI binds MC0 before ACV:
nonzero MC0 carry can set the Apps mask0x8 even though ACV itself has width0.
The pure planner now provides `legacy_update_ordered`, preserving divided
numeric carry separately from ACV's output mask. Isolated `aggregate` remains
an isolated-resource calculation, not a replacement for the ordered loop.
This correction supplies no ACV hardware WRITE and changes none of the observed
fixed requests. The selected cached source files have no local modifications.

A read-only rooted sysfs inventory of 14 Linux bus devices found crypto-master
clients `scm_pas`, `qcedev-noc`, and `qcrypto-noc`, all idle, and CE0's software
aggregates zero. ACV's reported framework peak aggregates were nonzero (active
14054, sleep5649), consistent with carry but not proof of an atomic hardware
state. Sequential snapshots are saved in
`.cache/pixel-sunfish/rpmh-readback-20261017/linux-bus-bandwidth.json`.

The isolated reference PAS request (AB393600000/IB3936000000 bytes/s) calculates
MC0 fields444/8894, SH0 188/3766, SN11/SN0 742/7426, and CE0 57865/578653.
Taking componentwise maxima with observed fields preserves existing MC0/SH0,
retains SN11/SN0 X1265 while increasing Y to7426, and saturates both CE0 fields
to16383 under the [matched encoder](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/msm_bus/msm_bus_fabric_rpmh.c).
This is a nondecreasing floor experiment, not complete client aggregation or
proof of sufficient PAS bandwidth. In particular CE0 saturation remains an
explicit unresolved sufficiency limit. No secure operation is enabled by PASS.

The new `rpmh-pas-bus-probe` first requires the combined hold to pass, then issues
three fixed separate single-resource commits: SN11 and SN0 word0x613c5d02,
followed by CE0 word0x6fffffff. Each has wait1, acknowledgement/cleanup, full-bank
preservation, and two full BCM read passes before the next step. This follows
the legacy single-resource commit interface, not an atomic two-command VCD
batch. ACV is never written; no inherited numeric field is reduced. Final rail
replies are read twice. At most 119 requests (112 READs/seven WRITEs) and 238
bank checks occur. First failure stops all later commands with no retry or
guessed rollback. Requests remain held until physical restart. No AOP-on,
PAS/reset, firmware, DMA, SMC capability, or sleep/wake programming is supplied.

Validation: 264 Rust library tests, including ordered MC0/ACV carry, numeric
carry distinct from masks, exact PAS arithmetic and saturation, every added
timeout, all eight banks changed after each added transaction, each unfulfilled
floor, final rail loss, and base-gate failures. The 17 adapter/core tests check
all four fixed BCM WRITE footprints; permission tests cover four manifests.
Eight native transaction bins build with no undefined symbols in the new ELF;
CapDL73 pages/11 intentional splash pages/zero reserved overlaps. Artifact and
native-result status are recorded separately in `rpmh-handoff.json`. Candidate
SHA-256 `718ad0a6f0f4dec9899d3d60fb42277261732b76db260443d63b3885254eec38`
was accepted by temporary fastboot boot after booted Lineage/UID2000 and
fastboot Sunfish checks (send0.564 s, boot2.088 s, exit0). No partition was
flashed. Photo `CAFCB5D4-054B-4A3A-A7D9-95D5799336FC_4_5005_c.jpeg`
confirms **BUS FLOOR HOLD PASS**, phase0xb, issued/ACK masks0x7f,
requests0x77 (119), checks0xee (238), CE0 plan saturation1, bus difference0,
IRQ status0 and TCS enable0. Final bus replies are MC0 0x29917f75,
ACV0x20000008, CE0 0x2fffffff, SN11/SN0 0x213c5d02, SH0 0x240d5adf;
MC0/CE0/SN11/SN0 second replies also appear and match. Rails are CX6/MSS7/MX3/
XO3. The banner additionally requires both complete six-resource passes after
each new WRITE and both final rail passes. Image/ELF/report/route/floor-plan/
photo are archived under
`.cache/pixel-sunfish/rpmh-pas-bus-probe/observed-bus-floor-hold-pass/`.
ADB confirms the subsequent return to Sunfish/Lineage23.2/boot_completed1/UID2000.
This result does not remove the CE0 saturation, complete aggregation/QoS,
secure-loader, firmware or DMA-isolation gates.

### Read-only PAS-call availability candidate

The actual modem caller includes `soc/qcom/scm.h` and uses
`drivers/soc/qcom/scm.c`; the cached runtime config reports CONFIG_QCOM_SCM=y,
CONFIG_QCOM_SCM_64=y. The earlier generic `drivers/firmware/qcom_scm-64.c`
trace is useful background, but the native probe now pins the actual caller's
specific envelopes. For its SMC64 convention trial, x0=0xc2000601,
x1=1, x2=0x82000601. After a successful trial, ordinary `scm_is_call_available`
uses x0=0x42000601, x1=1, and the scalar SIP identifier in x2. Other input
registers are zero. The four target identifiers are 0x02000201, 0x02000202,
0x02000205, 0x02000206 (init image, memory setup, authenticate/reset, shutdown).

`scm-pas-query-probe` sends only those five INFO queries. It never invokes any
queried PAS operation or supplies PAS ID4, a pointer, metadata, memory extent,
or reset command. It is independent of the resource-hold image and has only
reserved splash memory plus one isolated SMC-capable PD, no client channel,
RSC/AOP/SMMU mapping, or IRQ. The SMC capability itself is broad; fixed trusted
code and the absence of input channels restrict its use, not a per-function
kernel capability. Full 64-bit transport/value results are shown on screen.

The first transport error, noncanonical result (anything except 0/1), or failed
SMC64 trial stops subsequent calls. Unsupported target0 is retained and the
remaining availability queries continue. There is no BUSY retry, interrupted
resume, SMC32 fallback, PAS operation, firmware load or modem start. Progress
is painted before each SMC; five calls is a count bound, not a wall-clock bound
on secure world. Advertisement does not establish PAS ID4 support, sufficient
power/bandwidth, metadata/cache ownership, successful authentication, or DMA
containment.

Validation: four C host test groups covering fixed register envelopes, unused
zero registers, unavailable targets, failed convention, every transport error
position, retained raw results and noncanonical high bits. The capability
isolation test passes, the native C image builds with no undefined symbols,
and CapDL31 pages/11 intentional splash pages have zero reserved overlaps.
Artifact and device-result state are recorded in `rpmh-handoff.json`. Candidate SHA-256
`61d3771859de866bc74c2e063a9a2e2a2fad2043929ac353ce7e44a59c1c6418`
was accepted by temporary fastboot boot after ADB confirmed booted Lineage/
UID2000 and fastboot confirmed product Sunfish (send0.563 s, boot2.104 s,
exit0). Photo `C319A268-4C7E-45A9-A7EC-0AC2CE2BF4FE_4_5005_c.jpeg`
confirms **QUERY DONE**: convention status0/value1, all four target statuses0/
values1, PAS mask0xf, issued/done masks0x1f, error0, index4. The displayed final
query function is0x42000601 and target0x02000206. Image/ELF/report/plan/photo are
archived under `.cache/pixel-sunfish/scm-pas-query-probe/observed-query-done/`.
ADB confirms LineageOS returned. These are INFO results; no PAS operation,
pointer, PAS ID, firmware load or reset occurred.

### Owned MDT metadata preparation candidate

The installed `/vendor/firmware_mnt/image/modem.mdt` was copied read-only via
rooted ADB into ignored local cache on 2026-10-17. Its9100 bytes match the earlier
inventory SHA-256 `43a1414628153c109f06fe70385946c1c530a4ee08840b40f62649c9fa589870`.
No `.bNN` segment file was read or copied. ADB was returned to UID2000 and
verified. This is metadata for the owned phone's local staging experiment,
not a firmware release/authentication or redistribution manifest.

The [matched DMA mapping implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/mm/dma-mapping.c)
maps the loader's DMA_ATTR_STRONGLY_ORDERED buffer through `pgprot_noncached`;
the matching ARM64 pgtable header selects Device-nGnRnE and execute-never.
The Sunfish seL4 source likewise maps uncached user frames as Device-nGnRnE.
The new `pas-metadata-probe` therefore uses dedicated RW/NX uncached RAM pages
with explicit byte-volatile reads/writes and DSB barriers, avoiding unaligned
or vector copies to Device memory. This configured memory type and CPU
readback alone cannot prove future secure-world cache/ownership behavior.

Microkit's `setvar region_paddr` allocates five physically contiguous4K pages.
The generated CapDL layout places them at0x80000000..0x80005000, outside all24
fixed reservations and other frame allocations. The middle three pages start
at0x80001000 and can hold12288 metadata bytes; first/last pages are canaries.
The probe requires an initially zero mapping, validates the MDT and all29
relocatable nonoverlapping loadable segments before writing, copies only its
9100 metadata bytes, checks every payload byte and zero padding, checks both
guard pages, then reparses the staged bytes. Linked extent remains
0x8b000000..0x94800000. The actual modem reservation is never mapped or written.

The image has no SMC capability, IRQ, client channel, AOP/RSC mapping, segment
loader or reset operation. The diagnostic result is not permission to invoke
PAS. Source extraction and private asset inclusion are gated by an explicit
Cargo feature; normal builds do not require the phone's metadata cache.
Validation:269 library tests;9 standalone parser/adapter tests (including the
pinned installed MDT fixture);2 isolation/layout tests, native build with no
undefined symbols, CapDL70 pages/11 splash pages/5 metadata pages with zero
reserved overlaps. Device result and artifact identity are recorded separately
in `rpmh-handoff.json`. Candidate SHA-256
`d8f546b1e1f558f960eae11d37b43eb33e928b89252496aa1253a9d7602d4677`
was accepted by temporary fastboot boot after booted Lineage/UID2000 and
fastboot Sunfish checks (send0.564 s, boot2.380 s, exit0). No partition was
flashed. The user photo `E01F46E2-E269-4FF5-B3F0-DACD40A25F6C` confirms
`METADATA STAGE PASS`: buffer0x80000000, MDT0x80001000,9100 bytes,
31 headers/29 loadables, linked0x8b000000..0x94800000, exact readback/reparse,
both guards intact and padding zero. ADB subsequently confirmed LineageOS
boot complete and UID2000. This establishes CPU staging only, not secure-world
access or firmware authentication.

The next integrated `modem-prepare-probe` stages the same owned buffer before
platform writes, sends one fixed AOP `load_state: on` notification using the
bounded link/open/consume/close sequence, runs the previously observed strict
seven-resource hold, then verifies the buffer again without writing it.
Consumption remains transport evidence only. The old initial resource gates
remain strict: AOP-induced changes stop the sequence and retain the initial
BCM readings for diagnosis. First failure is terminal; no retry, guessed OFF
or unvote occurs. The image still has no SMC capability, executable segment
loader, modem-pool mapping, reset release, GLINK or WLAN DMA.

Candidate SHA-256
`fceb860a9f2eb40773002f55ecc7edbd0cd530edf6d54d504b5085a639319f19`
builds with no undefined symbols. Checks pass:271 library tests;
25 AOP,17 BCM and10 metadata standalone adapter/core tests;3 metadata/layout
and integrated capability tests;4 older probe isolation tests. CapDL contains
86 pages,11 intentional splash pages and5 contiguous metadata pages with
zero fixed-reserved overlaps. Fastboot accepted this image temporarily after
LineageOS boot-complete/UID2000 and product Sunfish checks (send0.570 s,
boot2.103 s, total2.683 s, exit0). No partition was flashed; the native
result is now confirmed by user photo
`49B9468F-1D0C-497E-986C-15F0B48DC240`: PREPARE PASS, error0,
AOP mask0x1f/notifies4/empty RX and TX, RSC phase0xb, ACK0x7f,
119 requests and238 full-bank checks. Final rails6/7/3/3 and six BCM replies
match the prior bus-floor experiment; the9100-byte MDT remains exact with
both guards and padding intact. ADB confirmed LineageOS boot complete/UID2000.
This is the first native integration of ON transport, the restricted holds,
and owned CPU metadata staging; it still invokes no modem PAS operation.


The separate `pas-init-probe` now implements a first native metadata handoff.
It repeats the observed preparation and refuses to call SCM on any preparation
fault. It then makes fresh SMC64-convention and INIT-availability INFO queries,
reverifies the buffer without writing, and invokes only PAS_INIT_IMAGE with
`x0=0x42000201, x1=0x82, x2=4, x3=0x80001000`, x4-x7 zero. The envelope matches
the pinned `subsys-pil-tz.c`/`scm.c` caller, whose selected files have no local
edits. A sealed request type exposes no arbitrary peripheral/operation API.
One dedicated PD has the unbadged SMC capability at Microkit slot7 and no
client channels. This is a trusted experiment, not kernel filtering of SCM
arguments or a production lifecycle service.

There are at most two INFO calls and one INIT; any seL4 invocation error,
nonzero monitor status (including INTERRUPTED/BUSY), unavailable/noncanonical
query, changed metadata or service failure is terminal. No retry, resumption,
SMC32 fallback or guessed cleanup call is supplied. The screen is painted
before each call and retains all eight raw returned registers, including high
halves. Only a monitor-status0 INIT return is followed by read-only exact
buffer/guard/padding inspection. All five pages and held resource requests
remain allocated until physical restart, even on failure. A successful INIT
return would establish acceptance of this header handoff; it would not prove
complete image authentication or firmware execution. The probe supplies no
MEM_SETUP, executable segment loading, AUTH_AND_RESET, SHUTDOWN, modem-pool
mapping or WLAN DMA. CE0 bandwidth saturation and complete aggregation/QoS
remain unresolved; this experiment does not assert a full startup contract.

Candidate SHA-256
`f556d3dc5f9ce44f062cc243b92ba0664916e50984c1c2be1f6f4aa95eba09a7`
builds with no undefined symbols;276 library tests,15 actual SMC-adapter/core,
25 AOP,17 BCM,10 metadata standalone tests and4 isolation/layout tests pass.
The shared preparation code is unchanged in sequence and both native bins
build. CapDL:86 pages,5 dedicated metadata pages,11 splash pages,zero fixed
reserved overlaps,one SMC cap in slot7. Fastboot accepted the image temporarily
after booted LineageOS/UID2000/slot a and Sunfish product checks (send0.565 s,
boot2.094 s,total2.675 s,exit0); no partition was flashed. The user photo
`F6F0BC0F-62FA-4B3F-8D68-A5F06D43EB3E` confirms INIT CALL STOPPED:
error0x00070006, AOP mask0x1f, resource ACK0x7f/119 requests, SMC issued/returned
masks7, kernel error0, last function0x42000201, and all eight returned
64-bit registers zero. Thus the INIT monitor and service status both returned
zero, while the strict post-call payload comparison failed (Error::Copy6).
The old verifier exits at the first payload mismatch, so this screen establishes
neither intact guards nor unchanged padding. It does not explain which bytes
changed or establish that this is an expected secure-consumer mutation. ADB
confirmed subsequent LineageOS boot complete and UID2000. The exact original
artifact, source hashes, photo and observation are archived separately.

The next diagnostic variant repeats precisely the same preparation and three
SMC envelopes, adding no PAS operation. After an INIT monitor-status0 return,
it snapshots all five allocated pages with byte-volatile reads, counts both
guard failures and padding failures without an early payload exit, counts
changed and zero payload bytes, and records first/last payload-relative change
offsets and two little-endian words beginning at the first change. Sentinel
0xffffffff means no changed offset. CRC32/IEEE fingerprints are diagnostic,
not authentication. A second exact full read reports bytes that changed during
this observation window; all counts/words/parse results refer to the first
snapshot. Neither scan writes or repairs any byte. An unchanged-buffer gate
remains strict; the read/write SCM argument alone does not explain the observed
mutation. No MEM_SETUP, firmware segments, reset release or WLAN DMA is added.

Candidate SHA-256
`931a7334a0c7641972e029d1fd59224074fc77e2616f703181d7d515347c913b`
builds with no undefined symbols.279 library tests,18 SMC-adapter/core,
25 AOP,17 BCM,14 metadata/parser/diagnostic tests and4 isolation/layout tests
pass. CapDL:89 pages,5 contiguous metadata pages,11 splash pages,zero fixed
reserved overlaps and one SMC capability. Installed input CRC0xb19e8d4e;
an all-zero9100-byte payload would have CRC0x7511eaf4 and8171 changed bytes.
Fastboot accepted the diagnostic image temporarily after LineageOS boot-complete/
UID2000 and Sunfish product checks (send0.564 s,boot2.106 s,total2.677 s,
exit0). No partition was flashed. The user photo
`790B966A-9EB2-4F6A-82A7-E4717651339F` confirms exactly that all-zero pattern:
8171 changed bytes,9100 zero bytes,first0/last0x238b,CRC0x7511eaf4,low/high
guard failures0,padding failures0,unstable bytes0 and actual parse error0x101.
INIT monitor/service remain0 and preparation masks/counts match. This identifies
the bounded observed contents, not the writer or undocumented secure-world
policy. No subsequent LineageOS return was requested or checked in this turn.

The next image is prepared locally only at
`.cache/pixel-sunfish/pas-mem-setup-probe/fastboot.img`, SHA-256
`719e8faa3afb459ff3bbb70da658312943ea833697018b41aea1ec55bcbbf42e`.
It explicitly consumes a successful INIT session after requiring the exact
observed zeroed payload, intact guard/padding values and matching second read.
It never reparses the zeroed buffer as a valid firmware image or re-stages it.
The matched Linux caller declares that buffer SCM_RW and frees it before the
next callback without an unchanged-input postcondition. This supports a
consumed-buffer lifecycle; it does not document why this secure implementation
zeroed the bytes. The original strict unchanged-buffer verifier remains intact.

After a fresh MEM_SETUP INFO query and another consumed-buffer scan, the image
can issue at most one `PAS_MEM_SETUP`:x0=0x42000202,x1=3,x2=4,
x3=0x8b000000,x4=0x09800000,x5-x7=0. The range matches the parsed installed MDT
and the modem's no-map reservation in both the cached runtime DT and seL4 DT.
A new build validator checks the PAS4 memory-region phandle and rejects any AP
frame, even partially intersecting, in the full modem range. No part of that
pool is CPU-mapped. The source reference places MEM_SETUP after INIT and before
segment loading. A new consumed-session type permits no transition from an
uncompleted/failed INIT; any unexpected contents, unavailable/noncanonical
query, invocation/monitor/service error or repeated attempt is terminal.
A successful returned setup transport is followed by one more read-only full
inspection; no segments,AUTH_AND_RESET,SHUTDOWN,reset release or WLAN DMA follows.
Status0 would prove request acceptance only, not full authentication,
ownership/delegation semantics, firmware execution or Wi-Fi.

Validation passes:284 library tests,24 actual SMC-adapter/core tests (including
exact five-call sequencing and installed-header inspection),25 AOP,17 BCM,
14 metadata/parser/inspection tests and6 capability/layout/reservation tests.
The native ELF has no undefined symbols;CapDL89 pages,5 contiguous metadata
pages,11 splash pages,one SMC owner,zero fixed-reserved overlaps and zero modem
pool frames. Cached source files for SCM and the two loader callbacks remain
unmodified. Initial preparation remained entirely local per the user's
instruction, with no device commands. After the later “Lineage ready” reply,
ADB confirmed Sunfish/LineageOS boot complete,slot a,UID2000;fastboot separately
confirmed product Sunfish and accepted this exact image temporarily
(send0.563 s,boot2.084 s,total2.653 s,exit0). No partition was flashed.
The later photo `B081C0D6-971E-47E9-99A2-2F9A279EE6F6` confirms **MEM SETUP
PASS**: error0, INIT returned mask7, MEM issued/returned masks3, kernel error0,
consumed-buffer check0, monitor x0=0 and service x1=0. Returned x4 is0x09800000;
all other returned registers are0. ADB confirms the subsequent LineageOS boot
completed, UID2000. The earlier reattached `790B966A` image is historical INIT
buffer evidence, not this result. The correct photo arrived before a repeat
image was booted; no duplicate boot occurred. The original image, source hashes,
and photo are archived under `pas-mem-setup-probe/observed-mem-setup-pass`.
This proves acceptance of this fixed request and the observed buffer checks;
it does not establish AP ownership, authenticated image loading, modem startup,
or WLAN DMA containment.

The next isolated experiment adds one fixed 4K RW/NX uncached mapping at
0x93200000 for installed MDT segment23 (`modem.b23`). Its declared file and
memory lengths are both45 bytes and flags0x08000004 (readable, non-executable,
relocatable). The 45-byte read-only ADB copy matches the prior in-place SHA-256
inventory: `3d8af2cf3f7151be6e0985e91b660d516c7aaba8ef1a083fb9c45304f61b12f9`.
Root ADB was returned to UID2000 afterward. No other segment contents were copied.
The immutable MDT and data hashes are pinned before compiling. The existing
preparation, INIT, MEM_SETUP and final consumed-buffer gates run before the
first modem-page access. The probe snapshots the page, writes exactly45 bytes,
then checks those bytes, preservation of the remaining4051 bytes and a second
full read for drift. It never clears the gap or repairs a mismatch. The matched
`pil_load_seg` source loads files after MEM_SETUP; the test empirically checks
that access rather than treating secure status0 as an ownership grant.
The normal reservation audit remains restrictive by default. A separate opt-in
pins the complete one-domain manifest and admits only this single page; all
other frames intersecting the modem pool are rejected. This is data staging,
with no executable segment, AUTH_AND_RESET, reset release, DMA or radio access.
The built image is `.cache/pixel-sunfish/modem-data-probe/fastboot.img`, SHA-256
`fc8f413c8187e20e5e5fbc2be7b375694283137a6a6df37085699713a9d35f14`.
Validation:287 library tests;26 actual data-adapter/core tests including the
installed MDT and segment;24 SMC,25 AOP,17 BCM and14 metadata adapter/core
tests;8 manifest/layout tests. These are host tests, with synthetic secure
responses where applicable. The native ELF has no undefined symbols. CapDL
contains90 pages,5 contiguous metadata pages,11 splash pages,exactly1 fixed
modem data page and no unexpected fixed-reserved overlap. The packaged kernel
matches the cached LZ4 payload, and its decompression matches the wrapped loader.
The prepared image and source snapshots are archived under
`modem-data-probe/prepared-data-copy`. Expected source/payload diagnostic
CRC is0x7d5d0dee;45 byte writes display as0x2d.
ADB again confirmed the connected Sunfish in LineageOS, boot-complete, slot a,
UID2000. After rebooting to Fastboot and checking product Sunfish, fastboot
accepted this exact image temporarily (send0.566 s,boot2.092 s,total2.662 s,
exit0). No partition was flashed. The photo
`F9C4B942-1FA2-4B7E-B080-2DD047EA2EA4` now confirms **DATA COPY PASS**:
segment23 at0x93200000,45 byte writes,zero payload/tail/unstable failures,
source and payload CRC0x7d5d0dee,first bad0xffffffff,INIT returned7 and MEM
returned3. Page CRC changed0xc71c0011 ->0x6687bbba; the remaining4051 bytes
were preserved and the second full read matched. ADB confirms the return to
LineageOS,boot-complete,slot a,UID2000. The image,source and photo are archived
under `modem-data-probe/observed-data-copy-pass`. This proves this small native
transfer, not a complete image load, authentication, modem start or Wi-Fi.

The next standalone image is `modem-segment-probe/fastboot.img`, SHA-256
`86d5f19a15062a560ccf30654c6142659b55b2e5a46fc5eefc3002f6db899bce`.
It repeats the same preparation and five-call INIT/MEM_SETUP envelope, then
loads exactly installed non-executable segment14 at0x8b780000:4160 file bytes
(flags0x08000006),followed by4032 zero bytes to its8192-byte memory extent.
This adds crossing a4K page boundary and the BSS zero-fill behavior traced in
`pil_load_seg`. A read-only ADB copy of `modem.b14` matches the earlier hash
inventory (`b7de02dad947996e55b72749abf9a096d96e824ad109f9cc0d77d801a175b4ba`);
ADB was unrooted and UID2000 verified afterward. The build pins MDT and segment
hashes, and runtime checks exact index,address,flags,file/memory lengths and
that no other MDT load intersects the guard window. Two adjacent4K pages at
0x8b77f000 and0x8b782000 are mapped read-only; only the middle two pages are RW.
All four are NX and uncached. The actual byte adapter rejects every guard
write. No page is touched until successful MEM_SETUP and the final exact
consumed-buffer check. The experiment compares every file/BSS/guard byte and
then a second16K read; no retry,repair,auth/reset,executable load or DMA follows.

Validation passes:290 library tests,26 segment-adapter/core tests including the
pinned installed MDT and segment,24 SMC,25 AOP,17 BCM and14 metadata adapter/core
tests,and10 manifest/layout tests. Secure replies in host tests are synthetic.
CapDL contains95 pages,4 fixed segment/guard pages,5 contiguous metadata pages,
11 splash pages,one SMC owner and zero unexpected fixed-reserved overlaps.
The default audit rejects all4 modem/guard frames without the exact opt-in;
the earlier single-page exception still passes its original image. The native
ELF has no undefined symbols,and the packaged kernel decompresses exactly to
the wrapped loader. Expected source/payload CRC is0x50d087bd,zero-tail CRC
0xe8b8467d,file writes0x1040 and zero writes0xfc0. The prepared image and source
snapshot are under `modem-segment-probe/prepared-segment-load`. Its native
result remains pending until a new MODEM SEGMENT LOAD screen is observed.
After fresh ADB checks (Sunfish,LineageOS,boot-complete,slot a,UID2000) and
Fastboot product confirmation,fastboot accepted this exact image temporarily
(send0.563 s,boot2.098 s,total2.668 s,exit0). No partition was flashed.
The later photo `8B3A36DB-8E88-4B76-BE22-0DE278787393` confirms **SEGMENT LOAD
PASS**:4160 file writes,4032 zero writes,zero payload/BSS/guard/unstable failures,
source/readback CRC0x50d087bd,zero-tail CRC0xe8b8467d,first bad0xffffffff,
INIT returned7 and MEM returned3. Guard CRC rows are not visible in this photo;
the displayed guard-failure count is0. ADB subsequently confirms LineageOS,
boot-complete,slot a,UID2000. The image,source and photo are archived under
`modem-segment-probe/observed-segment-load-pass`. This establishes the tested
cross-page copy,BSS fill and exact guard checks,not a running modem or Wi-Fi.

The next standalone **MODEM IMAGE STAGE** image plans the complete29-segment
load without AUTH_AND_RESET. Read-only root ADB copied all29 loadable `.bNN`
files (68,393,962 bytes),including the three empty files; every size/SHA-256
matches the prior in-place inventory. A fresh MDT read also matched the pinned
9100 bytes. ADB was unrooted and UID2000 verified afterward. The files remain
in the ignored private cache; they are not redistributed.

The build pins every source and the MDT. Before platform operations,the runtime
planner requires31 headers,29 unique source indices2..30,exact file lengths,
nonoverlapping relocatable loads in the fixed0x8b000000..0x94800000 pool and
exact aggregate sizes. After the same preparation/INIT/MEM_SETUP gates and a
fresh consumed-buffer check,it snapshots all1,639,435 gap bytes into a separate
owned2MiB allocation. It serially copies68,393,962 file bytes and zeroes
89,350,155 BSS bytes,using aligned32-bit volatile accesses plus bounded byte
tails. Every loaded byte is compared against the immutable source or zero in
two full157,744,117-byte passes. All gaps are then compared byte-for-byte to
the saved originals; this is not merely a CRC comparison. A final metadata
inspection must also retain the exact consumed pattern and intact guards.
No retry,repair,authentication,reset release,firmware execution,GLINK write,
WLAN DMA or radio access follows. Executable segment contents are included as
data; the AP modem mapping is RW/NX throughout.

The entire modem pool uses76 fixed2MiB frames in the sole trusted probe domain,
with no client channels. The capability exception pins the complete manifest;
default audits still reject every modem-pool frame without explicit opt-in.
The gap snapshot is separately allocated,non-executable and disjoint from all
fixed reservations and AP frames. A general library sorter introduced a missing
`memmove` link dependency; the new bounded29-element ordering and ordinary-RAM
C-ABI memmove are now provided. Overlap tests cover both directions,zero length,
identical pointers and surrounding canaries.

The larger payload exposed the old loader address0x81000000 overlapping the
initial task. The first private SDK variant placed the loader at0xb0000000. Rebuilding
the baseline reproduces the installed loader's loadable segments exactly; the
base SDK,kernel,monitor and initialiser remain unchanged. Because the loader
relocates its entire payload,the build validates the complete relocated range
[0xb0000000,0xb4660d14),not only its code. It is inside a cached live DT RAM bank,
outside all fixed reservations and disjoint from actual kernel/initial-task copy
targets and generated AP frames. That build assumed the source Image occupied
[0x80080000,0x846e1d14),below the first fixed reservation. A later native input
snapshot disproved this source assumption for the large diagnostic and exposed
an omitted full source/destination overlap check; see the correction below.
A48-byte valid empty
gzip/cpio replaces the unused Android ramdisk in this temporary artifact;
the original boot DTB is preserved. Existing packer calls retain the original
ramdisk by default. These historical range checks did not establish actual
native placement or a safe relocation. They are not a matched Pixel ABL
source contract or proof of successful native boot.

Candidate SHA-256 `20651a43a1b8a750e2019e82c0ae4fae28e423de6b40d85599799db102200956`:
`.cache/pixel-sunfish/modem-image-probe/fastboot.img` (41,119,744 bytes).
Validation:293 library tests,25 image-adapter/core tests including the complete
installed image in host RAM,2 freestanding-memory tests,and16 isolation/layout/
boot-format tests pass. Shared SMC/AOP/BCM/metadata adapter suites also pass.
The native ELF has no undefined symbols. CapDL has16,865 pages,76 fixed pool
frames,5 contiguous metadata pages and no unexpected fixed-reserved overlap.
Root CNode size is17 bits,above this initial image's page-cap count. The packaged
LZ4 kernel decompresses exactly to the wrapped loader. Prepared image,ELF,
selected sources and layout proofs are archived in
`modem-image-probe/prepared-image-stage`.
Expected loaded/verified masks:0x7ffffffc;file writes0x04139bea;zero writes
0x0553600b;bytes checked each pass0x0966fbf5;gap snapshot/comparison0x0019040b.
Native whole-image staging remains pending until a new result screen is observed.
After fresh ADB confirmation (Sunfish,LineageOS,boot-complete,slot a,UID2000),
Fastboot confirmed product Sunfish and a256MiB download limit. It accepted the
41,119,744-byte image temporarily (send1.077 s,boot4.070 s,total5.160 s,exit0).
No partition was flashed. Fastboot acceptance is not a native staging result.
The user later found the phone already in LineageOS without a result photo and
could not say whether it returned automatically or was restarted manually.
ADB confirms boot-complete/UID2000; both boot-reason properties report
`watchdog`. This is not enough to assign a reset cause or a reached probe stage.
The accessible pstore list is empty and `/proc/last_kmsg` is absent. Read-only
root diagnostics were saved locally; ADB was unrooted again. The full-image
staging result remains UNVERIFIED,with no pass inferred and no next secure
operation attempted.

A separate boot-only diagnostic retains the complete29-file payload and the
same memory-region manifest. The probe PD accesses only its owned ELF/RAM and
the existing splash framebuffer; its modem,AOP and RSC mappings remain unused.
It invokes no PAS operation or resource vote. Existing loader/kernel console
behavior remains present.
It validates the immutable image plan and reads all68,393,962 source bytes in
owned ELF RAM,comparing the diagnostic byte sum with a build-generated value.
The source files are still pinned by SHA-256 before compilation; the sum is a
memory-walk diagnostic,not firmware authentication. Final title is
`MODEM IMAGE BOOT`,status `BOOT CHECK PASS` or a failure line. This can establish
that this large-image revision reached the PD and read its payload,not that
the full firmware staging test passed.

For earlier failures,the private loader additionally displays IMAGE ENTRY,
IMAGE RELOCATED,IMAGE COPYING,IMAGE COPIED and IMAGE KERNEL markers while MMU
is off. Entry text pointers are formed with ADR at their actual runtime location
before relocation; the marker uses no absolute static pointer table. It writes
only a bounded strip in the same two splash buffers. Host tests verify both
buffers,all five labels,long-text truncation and surrounding canaries. The base
SDK and source remain untouched; markers are built in a private source copy.
The wrapper accepts only ADRP x1 as its first instruction,allowing the changed
stack-page displacement,and the boot validator compares the entry bytes with
the exact private ELF. This is a new temporary boot diagnostic,not a continuation
past an unobserved modem-load result.

The boot-only candidate is SHA-256
`52dd4f789319daa64324ac6304f430bd0a912ba6405835eb771706c4929a5bbe`
at `.cache/pixel-sunfish/modem-image-boot-check/fastboot.img` (41,070,592 bytes).
Its native build, 17 capability/layout/boot-format tests and six loader drawing
scenarios pass. After verifying the artifact and source hashes, Fastboot
confirmed Sunfish and the 256 MiB download limit, then accepted the temporary
boot (send 1.099 s, boot 4.260 s, total 5.370 s, exit 0). No partition was
flashed. The user subsequently reported that it appeared to have restarted to
LineageOS, without seeing a checkpoint. ADB confirms LineageOS boot complete
and UID 2000, with both boot-reason properties again reporting `watchdog`.
This boot check and the preceding full firmware staging result remain
unverified. Read-only root capture of `/proc/bldrlog`, current dmesg and pstore
status adds no native stage evidence; ADB was unrooted afterward. The current
bootloader log reports PS_HOLD reset information and disabled UEFI hardware
watchdog configuration, which does not establish the previous probe's reset
cause. Pstore remains empty after the vendor's normal decrypt/remount service.

The next diagnostic separates boot entry from relocation and kernel startup.
`build-entry-hold-check.py` compiles a small entry stub that masks interrupts,
draws `IMAGE ENTRY HOLD` in the existing splash strip, then branches to itself.
The complete entry and stack occupy 8 KiB; the wrapped compact Image is 12 KiB.
An optional comparison appends the previous 73,737,156-byte wrapped payload as
inert data. The two variants execute identical bytes, use the same empty
ramdisk and original DTB, and differ only in appended data and Image size.
Neither reaches relocation, seL4, PAS, resource voting or modem memory.
Both source ranges pass the cached live/kernel fixed-reservation checks, and
the builder validates the terminal self-branch and exact drawing call target.
QEMU executes both actual ARM64 Images through the wrapper to the hold at
`0x80081018`, with foreground/background checks in both buffers and untouched
surrounding rows. This is execution/layout evidence for the diagnostic only,
not evidence of native Pixel entry or firmware staging. The compact variant
was tested first, to check marker visibility and the minimal wrapper without
the large payload. Fastboot accepted the 421,888-byte compact boot image
(send 0.149 s, boot 1.910 s, total 2.060 s, exit 0). The user then typed
`IMAGE ENTRY HOLD`, confirming native entry and completion of the drawing
routine for SHA-256
`7511c353cd284eaac46c91ad4523a974f07c7c078c0a7b868bfd724d8d214a36`.
The observed artifact and report are archived in
`entry-hold-check/observed-compact-entry-hold`. This verifies the compact
wrapper/marker combination; it does not verify the relocated loader, seL4
startup or firmware staging. The 41,070,592-byte large comparison is prepared
as SHA-256
`cbd33ef3ad1417001d41366cf532f4723245b2978ca747e8196c9ace2695c21e`,
with the identical 8 KiB entry bytes and an inert 73,737,156-byte payload.
After a USB transition attempt that sent no image, the user reported possibly
returning the phone to LineageOS and requested a retry. Fresh ADB verification
confirmed Sunfish, LineageOS boot complete, slot a and UID 2000. Fastboot then
confirmed Sunfish and its 256 MiB limit and accepted the large comparison
(send 1.067 s, boot 4.112 s, total 5.210 s, exit 0). The user then typed
`IMAGE ENTRY HOLD`, confirming native entry and drawing completion for the
large comparison as well. Both entry observations are archived. This shows
that the large Image size and minimal wrapper can reach the standalone entry
stub on this Pixel. It does not verify the actual Microkit loader's relocation,
kernel startup or firmware staging. No partition was flashed.

The next candidate retains the complete boot/read diagnostic but stops the
actual private Microkit loader after relocation. Its assembly draws the entry
marker, performs the normal whole-payload relocation, then draws
`IMAGE RELOCATED` and loops only when the runtime `_start` address equals the
linked destination `0xb0000000`. It cannot reach loader `main` or seL4 startup
on that path. No PAS call, modem-memory access or resource vote is performed;
the existing debug UART and bounded splash writes remain present.

Loader preparation now creates a private board source tree and rebuilds every
object. The Sunfish GENI UART glue is shared with the board preparation script,
so it can reproduce the installed SDK even when the cached upstream source
lacks that branch. The fresh baseline's loadable segments match the base SDK
exactly; the base SDK and upstream source remain unchanged. This strengthens
build provenance and does not identify a native failure cause.
The relocation hold passes the same complete physical layout audits. QEMU
executes the actual loader through its copy and cache-maintenance routines,
without replacing the UART functions, and stops at `0xb0000038` before loader
`main`. The relocated code/data and all 73,675,700 immutable payload bytes
match exactly; both outer canaries survive and both framebuffers are drawn.
This is emulator evidence only. The relocation hold has not yet been tested
on the Pixel, and full firmware staging remains unverified.

Prepared relocation-hold image:
`.cache/pixel-sunfish/image-relocation-hold/fastboot.img` (41,070,592 bytes),
SHA-256 `9c90b18c0730f35de4b8e4d2973fdb0bc0d5fb5d1d4da7a2ade72272073fb14f`.
Its candidate record, native PD ELF, private loader source, image, audits and
emulator result are archived in `image-relocation-hold/prepared-relocation-hold`.
After the user restarted to LineageOS, ADB confirmed Sunfish, boot complete,
slot a and UID 2000. The image, source, private loader and base SDK fingerprints
matched. Fastboot confirmed Sunfish and its 256 MiB limit, then accepted the
temporary relocation check (send 1.081 s, boot 4.118 s, total 5.218 s, exit 0).
The user reported `google right now`, with no native marker. Initially both
ADB and Fastboot were absent; ADB subsequently reappeared in LineageOS with
boot complete, UID 2000 and both boot-reason properties reporting `watchdog`.
This does not identify the reached loader stage or the reset cause. Relocation
remains unverified. No partition was flashed.

To isolate actual loader entry from relocation, the next candidate stops
immediately after the loader's first `IMAGE ENTRY` marker. It explicitly masks
DAIF as the working standalone entry did, without changing the ordinary loader
or the earlier relocation variant. It never calls `relocate`, reaches the UART,
starts seL4, votes resources or accesses modem memory. In QEMU, the actual
73,737,156-byte wrapped image stops at `0x8008102c`, with all interrupt-mask bits
set, both splash buffers drawn and the entire relocation destination unchanged.
The original relocation variant also still passes the emulator test. These
checks do not show whether interrupt state contributed to the native result.
Prepared image SHA-256:
`a5f7db7d74195d8d311f8c42403da3d08bfc415a82fb2660dcee0b5dd81ea62c`,
at `loader-entry-hold/fastboot.img`, 41,070,592 bytes. Candidate, source snapshots
and proofs are archived in `loader-entry-hold/prepared-loader-entry-hold`.
Fastboot accepted this entry hold (send 1.060 s, boot 4.124 s, total 5.224 s,
exit 0), and the user typed `IMAGE ENTRY`. The actual private loader's entry,
stack and drawing path therefore reached its stop point with DAIF masked.
The observed artifact is archived in `loader-entry-hold/observed-loader-entry-hold`.
This does not prove interrupt masking caused the difference from the earlier
relocation test, or establish that any relocation occurred.

The next diagnostic separates the RAM copy from cache maintenance and the
jump to the destination. It retains the same interrupt mask, replaces the
relocation UART log with a screen marker, copies the complete 73,733,060-byte
loader image from `0x80081000` to `0xb0000000`, then stays at the source.
It compares the immutable code/data prefix and complete appended payload
against the source twice (73,700,276 bytes per pass), excluding mutable BSS
and live stacks. It displays `IMAGE COPY PASS` or `IMAGE COPY FAIL` with the
first bad offset. If already entered at the destination, it displays
`IMAGE COPY SKIP` and stops without claiming a copy result. No cache flush,
instruction-cache invalidation, destination execution, seL4, PAS, resource vote
or modem-memory access follows the copy on this path.

The verifier's host test covers mismatches in each immutable range, a fault
in the second pass, BSS exclusion, exact read bounds, alias/overflow rejection
and no repairs. Twelve drawing scenarios pass. The native build and unchanged
capability/physical layout gates pass. QEMU executes the compiled copy and
readback, displays the PASS glyphs in both buffers and stops at source
`0x80081040`; breakpoints guard cache-maintenance routines, UART logging and
loader main. External comparison also confirms the copied prefix/payload and
outer canaries. This tests CPU copy/readback only, with no claim of cache
coherency, executable relocation or native modem progress.
Candidate SHA-256
`6fa4e9424056bdecb74d14a9a6f8ccce0d0e9a4167aded6032bc0e1442eda791`
at `loader-copy-hold/fastboot.img` (41,070,592 bytes), archived with source and
proofs in `loader-copy-hold/prepared-loader-copy-hold`. Native result is pending.
After fresh Sunfish/LineageOS boot-complete/slot-a/UID-2000 verification and
artifact/source hash checks, Fastboot confirmed Sunfish and its 256 MiB limit
and accepted the temporary copy-only image (send 1.111 s, boot 4.131 s,
total 5.270 s, exit 0). No partition was flashed. Native copy completion has
not yet been observed.
The user subsequently reported LineageOS and requested the same check again,
without a native result reading. The first attempt remains unverified.
ADB reconnected and confirmed the same boot-complete Sunfish/LineageOS/slot-a/
UID-2000 state. Artifact, source and private loader hashes still matched.
Fastboot accepted the unchanged image for a second temporary boot (send
1.072 s, boot 4.118 s, total 5.208 s, exit 0). Its native screen result is
pending. Both attempts are recorded separately; neither implies a copy PASS.
The user then typed `IMAGE COPY START`. This confirms the relocation routine
reached its marker before size/bounds checks and `memmove`; it establishes no
minimum copied byte count. ADB subsequently returned with boot-complete
LineageOS, slot a, UID 2000 and both boot-reason properties `watchdog`. Neither
a copy completion nor the reset cause is established by these observations.
The user confirmed that this return to LineageOS happened by itself, without
a manual restart. No final copy marker was observed.

A separate copy-progress candidate uses aligned volatile 64-bit transfers for
ordinary AP RAM, with byte accesses for a prologue or tail. It rejects invalid,
overlapping or overflowing ranges before access. It displays `IMAGE COPY READ`
before the first load, `IMAGE COPY WRITE` before the first store, and copy
offsets after the first transfer and each 4 MiB chunk. It then displays offsets
for `IMAGE VERIFY1` and `IMAGE VERIFY2`, comparing only the immutable prefix and
appended payload. Word mismatches report the exact first bad byte from the
captured values, without a reread or repair. Copy offsets alone do not prove
readback. The same source hold precedes all cache maintenance, destination
execution, seL4, PAS, votes and modem-memory access. The original byte-copy
artifact and source snapshot are retained separately.

Host tests cover 2,048 source/destination alignment and length combinations,
all mismatch bytes in both immutable ranges, a second-pass transient fault,
BSS exclusion, exact accesses, chunk progression, canaries and invalid bounds.
Address/undefined-behavior sanitizers pass. Seventeen drawing scenarios and
the unchanged build/layout/capability checks pass. QEMU executes the complete
new copy and both readbacks, confirms PASS glyphs in both buffers, and verifies
all immutable bytes and outer canaries. Breakpoints exclude the original
byte-copy routine, cache maintenance, UART, destination entry and loader main.
The native PD ELF remains identical to the byte-copy candidate. This is still
emulator evidence; uncached transfer speed, a fault and other reset causes
remain hypotheses until measured on the Pixel.

Prepared progress image: `loader-copy-progress/fastboot.img`, 41,074,688 bytes,
SHA-256 `63cf965048056e647093d2e1f26c8578754ed06e22ecb98da300af572ab1a3a7`.
The copy covers 73,737,156 bytes, with 73,704,372 immutable bytes compared per
pass. Its candidate, source snapshots and proofs are archived in
`loader-copy-progress/prepared-loader-copy-progress`. Native result is pending.
After boot-complete Sunfish/LineageOS/slot-a/UID-2000 and source/artifact
fingerprint checks, Fastboot confirmed Sunfish and its 256 MiB limit, then
accepted this temporary image (send 1.134 s, boot 4.218 s, total 5.380 s,
exit 0). No partition was flashed. The user reported `IMAGE COPY FAIL FFFFFFFE`.
This is the explicit expected-address/bounds rejection code, not a mismatch
offset or evidence of a hardware fault. The next check will report the actual
source/destination, prefix, payload offset and total size without copying.

The input-snapshot candidate branches to a read-only argument reporter before
the copy routine. It displays complete 64-bit SRC/DST/PRE/PAY/LEN values and
ERR guard bits on nine bounded splash rows, then holds at the source. It
does not dereference the reported source or destination from C, copy any image
bytes, maintain caches, execute destination code, start seL4, call PAS or vote
resources. ERR bits 0 and 1 flag deviations from the prior `0x80081000` source
assumption and `0xb0000000` destination. Bits 2–9 cover null pointers, empty
prefix, prefix ordering, payload ordering, oversized total, source overflow,
destination overflow and overlap, respectively. This reports the reason
without weakening any existing copy guard.

Host tests cover each guard bit, full-width formatting, both complete splash
buffers and perimeter canaries; address/undefined-behavior sanitizers pass.
The unchanged build/layout checks and 12 loader/entry/layout/packer tests pass.
QEMU runs the actual complete image at `0x80080000` and `0x80280000`. It decodes
all nine framebuffer rows into the correct runtime arguments in both buffers,
reports ERR 0 and 1 respectively, and verifies that every byte of the copy
destination remains untouched. Breakpoints exclude copying, cache maintenance,
UART, destination entry and loader main. These are emulator placements, not
evidence of the native Pixel address.

Prepared input snapshot: `loader-copy-inputs/fastboot.img`, 41,074,688 bytes,
SHA-256 `f31be55655d49436a63ce276e95818a1c77912b175a6e38d2a462b3dea513f73`.
The candidate, source snapshots and proofs are archived in
`loader-copy-inputs/prepared-loader-copy-inputs`.
After a fresh ready Sunfish/LineageOS/slot-a/UID-2000 check and matching
artifact/source/private-loader fingerprints, Fastboot confirmed Sunfish and
its 256 MiB limit and accepted the input snapshot (send 1.071 s, boot 4.133 s,
total 5.226 s, exit 0). No partition was flashed.

The supplied native photo reads SRC `00000000B0081000`, DST
`00000000B0000000`, PRE `0000000000007000`, PAY `000000000000F010`, LEN
`00000000046523C4`, and ERR `0000000000000201`. The two ERR bits independently
identify the wrong source assumption and full-image overlap. The destination
is nonzero and matches the old linked address; the destination/null bits are
clear. The source Image begins at `0xb0080000`, with the loader at
`0xb0081000`, rather than the assumed `0x80081000` loader address. Source
`[0xb0081000,0xb46d33c4)` overlaps destination `[0xb0000000,0xb46523c4)`.
The original SDK's prefix-only overlap check permits this case even though
copying the whole body could overwrite live source instructions. This is a
concrete invalid relocation plan, without proof of the exact earlier reset
sequence. ADB subsequently confirmed boot-complete LineageOS, slot a, UID 2000,
and boot reason `reboot,longkey`.

Replaying the frozen input-snapshot image in QEMU at `0xb0080000` reproduces
all six native rows, including ERR `0x201`. It performs no image copy; external
comparison excludes only live source BSS, which aliases the destination in
this deliberately overlapping reproduction. The old artifacts/source snapshots
are retained unchanged. The earlier QEMU results at `0x80080000` tested an
incorrect native placement assumption.

The corrected private loader links at `0xc0000000` and pins its copy guard to
the observed `0xb0081000` source. The build now checks the complete source
Image against the complete destination, using source placement from its loader
profile. The observed-overlap regression fails before access, while the new
source `[0xb0080000,0xb46d33c4)` and destination `[0xc0000000,0xc46523c4)`
pass RAM-bank, fixed-reservation, generated-frame and actual-copy-target checks.
Runtime guards still reject changed placement or overlap; the frozen native
snapshot is evidence for the earlier image, not a guarantee for every boot.
The base SDK, kernel, monitor, initialiser and native diagnostic PD remain
unchanged. The new image still holds at the source after RAM copy/readback,
before cache maintenance, destination execution, seL4, PAS, votes or modem access.

Thirteen loader/entry/layout/packer tests and 18 build gates pass. Host
alignment/fault/bounds tests and address/undefined-behavior sanitizers pass.
QEMU executes the corrected full copy from the observed source to the new
destination, verifies all immutable bytes twice, confirms PASS in both
framebuffers and preserves both outer canaries. It cannot establish native
CPU access or executable relocation. Prepared image:
`loader-copy-native/fastboot.img`, 41,074,688 bytes, SHA-256
`e6cb734c30b7f956cb45e1158028afe87de4d6984b68fc83659fe12400ee5f53`.
The candidate and proofs are archived in
`loader-copy-native/prepared-loader-copy-native`. Native result is pending.
ADB confirmed the same boot-complete Sunfish/LineageOS/slot-a/UID-2000 state,
and artifact/source/private-loader/base-SDK fingerprints matched. Fastboot
confirmed Sunfish and its 256 MiB limit and accepted this temporary image
(send 1.209 s, boot 4.101 s, total 5.321 s, exit 0). No partition was flashed.
The user reported `IMAGE COPY PASS`. This confirms the full 73,737,156-byte
copy and both 73,704,372-byte immutable readback passes on the Pixel. It does
not establish cache maintenance, destination execution, kernel startup or
firmware staging. The next check will stop after executable relocation.

The next candidate keeps the confirmed aligned-copy engine and exact source/
destination guards, but returns the destination to assembly only after both
immutable verification passes succeed. Failure still holds at the source.
Assembly preserves the destination across framebuffer calls, executes the
unchanged SDK `flush_dcache` and `invalidate_icache`, and branches to the
linked entry. It marks `IMAGE CACHE START`, `IMAGE CACHE CLEAN` and
`IMAGE JUMP` along that path. The copied entry resets its stack, retains the
interrupt mask and displays `IMAGE RELOCATED`, then holds at `0xc000003c`
before loader main or seL4. No UART, PAS, votes, modem access or firmware start
is reached. Direct placement at the destination would also reach that hold;
the marker establishes destination execution rather than independently proving
every preceding step on an unobserved boot.

Fourteen loader/entry/layout/packer tests, 18 build gates and 20 drawing
scenarios pass. QEMU observes the actual source-side D-cache and I-cache
routine entries in sequence, then destination entry `0xc0000000` and the final
hold, with no stubs. It checks the relocated stack lies in destination BSS,
the exact final screen in both buffers, immutable prefix/body contents and
both canaries. A second run injects an oversized total at the actual returning
copy function entry; it displays `IMAGE COPY FAIL FFFFFFFE`, leaves the entire
destination unchanged and never reaches cache maintenance or destination code.
The native PD and base SDK remain identical to the previous confirmed copy.

Prepared relocation image: `loader-relocation-native/fastboot.img`, 41,074,688
bytes, SHA-256
`1d10ccf4dd06e2d9dc3a1c04cc8600ed9de1f5309a4e61140283152babc3beac`.
The candidate, source snapshots and proofs are archived in
`loader-relocation-native/prepared-loader-relocation-native`. Native result is
pending.
After fresh boot-complete Sunfish/LineageOS/slot-a/UID-2000 and fingerprint
checks, Fastboot confirmed Sunfish and its 256 MiB limit and accepted the
temporary relocation check (send 1.069 s, boot 4.094 s, total 5.173 s, exit 0).
No partition was flashed. The user reported `IMAGE COPY 40000000 and others\nand IMAGE RELOCATED`. The final `IMAGE RELOCATED` marker confirms native
execution at the linked destination and its stop before loader main. The
intermediate number is preserved verbatim; it is not interpreted as a copied
byte count, because `0x40000000` exceeds the image's bounded size.

The next candidate reaches the real loader main after the confirmed relocation
path and performs its CPU, exception-vector and MMU-off preparation. Its
`copy_data` implementation snapshots and validates all 8 header fields and
all 44 fields of the 11-region AP image plan before any task-image write. The
plan is pinned in `loader-task-regions.json`, and the build verifies it against
the actual packed loader table and unchanged native PD ELF. The copied regions
are ordinary AP kernel/initial-task data, totaling 73,675,284 bytes; no region
in modem memory is allowed. Each complete destination is compared with its
source twice. Success and failure both hold at `0xc0000040` before kernel or
secondary-CPU start.

The shared RAM transfer adds aligned 32-bit accesses for source/destination
alignments that cannot both use 64-bit words, retaining byte prologues/tails.
This matters for the large task-data region at a source offset ending in 4.
The host access-hook tests cover every relative alignment, exact bounds,
32/64-bit first-mismatch reporting and no repairs; address/undefined-behavior
sanitizers pass. Every manifest-field mutation and an invalid final destination
are rejected by the pure plan checker. Fifteen loader/entry/layout/packer tests,
18 build gates and 24 drawing scenarios pass.

The private diagnostic disables serial printing, omits the optional PSCI
version query, and aligns the loader-data table to eight bytes. This keeps the
RAM-only check independent of serial acknowledgements and platform firmware
query emulation. The native and QEMU tests use the same binary, with no UART
or firmware return-value stubs. The base SDK, kernel, monitor, initialiser and
native PD remain unchanged. No secure call, resource vote, modem-memory access,
task execution or firmware start occurs.

QEMU reaches `IMAGE LOAD PASS` through the actual loader main, compares all
11 complete target regions externally against the packed bytes, and preserves
14 canaries around task/loader ranges. Breakpoints guard kernel entry, MMU
enable and secondary CPU start. A second run changes the final destination to
the modem base after relocation but before loader main validation. It displays
`IMAGE LOAD FAIL C00001A0`, with every task destination still untouched; the
negative reference intentionally includes that one injected table mutation.
These tests do not establish native image loading or kernel execution.

Prepared image: `loader-task-images/fastboot.img`, 41,074,688 bytes, SHA-256
`f7c9a9ffbda89e312eff439be64bce52f81d354e6b353eb7d01ae55ce396a02d`.
The candidate, pinned manifest, source snapshots and proofs are archived in
`loader-task-images/prepared-loader-task-images`.
After fresh ready Sunfish/LineageOS/slot-a/UID-2000 and matching artifact,
source, private-loader and base-SDK fingerprints, Fastboot confirmed Sunfish
and its 256 MiB limit and accepted this temporary image (send 1.063 s, boot
4.069 s, total 5.148 s, exit 0). No partition was flashed. The user then reported
`IMAGE LOAD PASS`, confirming native whole-plan validation and all eleven
AP kernel/task copies with two complete readbacks each. The diagnostic stops
before seL4 or task execution; modem staging and Wi-Fi remain unproven.

The next isolated check returns from task loading only after every readback
passes, then enables the real loader MMU and enters the unchanged seL4 kernel,
monitor, initialiser and read-only large-payload diagnostic. The build remains
single CPU and rejects the firmware-staging program for this continuation.
The expected final screen is `MODEM IMAGE BOOT / BOOT CHECK PASS`, after
reading all 68,393,962 source bytes in owned ELF RAM and checking the pinned
sum. Modem mappings are retained for identical layout but are never accessed;
there are no PAS calls, resource votes, firmware authentication or reset release.

QEMU executes the actual MMU path from the relocated `0xc0000000` loader and
stops at the real kernel entry `0xffffff8080000000`, before its first
instruction. All six entry arguments and EL1h match; all eleven loaded regions
and fourteen physical canaries match, and both framebuffers show `IMAGE KERNEL`.
The injected bad last destination still produces `IMAGE LOAD FAIL C00001A0`
with all task targets untouched. QEMU does not model Pixel platform startup,
and this result does not establish kernel or diagnostic-program execution.
Sixteen source/entry/layout/packing tests and the existing build gates pass;
copy and plan sanitizer checks pass, and the loader has no undefined symbols.
The same native PD ELF hash is retained.

Prepared image: `loader-kernel-boot/fastboot.img`, 41,070,592 bytes, SHA-256
`2a5fa82494ae2f638110df061661735e4d5d23245d18b90bc10d07de3c76884b`.
Candidate, sources, image and proofs are archived under
`loader-kernel-boot/prepared-loader-kernel-boot`.
After fresh ready Sunfish/LineageOS/slot-a/UID-2000 and matching source,
artifact, private-loader and base-SDK fingerprints, Fastboot confirmed Sunfish
and its 256 MiB limit and accepted this temporary image (send 1.066 s, boot
4.117 s, total 5.197 s, exit 0). No partition was flashed. The final native
photo confirms `MODEM IMAGE BOOT / BOOT CHECK PASS`, `READ BYTES 04139BEA`
and `PAYLOAD SUM 2766D207`. This establishes native seL4, monitor, initialiser
and diagnostic-program execution with a complete 68,393,962-byte owned-payload
read. It does not establish modem-memory staging or Wi-Fi.

The next whole-image staging candidate keeps the now-confirmed guarded loader
path and unchanged seL4/kernel/initialiser/monitor. It uses the earlier staging
PD unchanged (SHA-256
`ab6d8c670d1d418780ce40c38823d3b2e182cf18eb474c3fcc6a147acd1b7a62`),
with its own pinned AP plan in `loader-task-regions-staging.json`; all 52 table
fields and the PD hash must match the rebuilt packed image. The read-only
boot-check and staging continuations are mutually exclusive and each requires
its matching program, verified relocation, task loading and a single CPU.
No firmware authentication, reset release or DMA path is introduced.

After repeating the existing AOP/resource/metadata/PAS4 INIT and MEM_SETUP
gates, the program stages all 29 files (68,393,962 bytes) and exact BSS zeros
(89,350,155 bytes) into the fixed excluded RW/NX modem pool. It compares all
157,744,117 loaded bytes twice and checks all 1,639,435 gap bytes against their
separate owned-RAM snapshot, then rechecks the consumed-metadata pattern.
The expected final status is `MODEM IMAGE STAGE / IMAGE STAGE PASS`, with all
three segment masks `7FFFFFFC` and no payload, zero-tail, gap or metadata error.
This would establish data staging only, before PAS authentication or modem start.

Seventeen loader/entry/layout/packing tests and 18 build gates pass, along with
the full-image host adapter tests and sanitizer plan checks. QEMU executes
real cache/MMU/branch code to the seL4 entry with exact arguments, verifies all
eleven AP image regions and fourteen canaries, and rejects a bad last target
before any task write. It stops before executing the kernel and does not test
native secure firmware or modem-pool access.
Prepared temporary image: `modem-image-stage-native/fastboot.img`, 41,119,744
bytes, SHA-256
`cab645181354df5bb807e4533f50d213f995c5e783534f24ebec110c20745a94`.
Candidate, source snapshots, images and proofs are archived in
`modem-image-stage-native/prepared-modem-image-stage-native`. Native full-image
staging is now confirmed by the user report and photo.
After fresh ready Sunfish/LineageOS/slot-a/UID-2000 and matching source,
artifact, private-loader and base-SDK fingerprints, Fastboot confirmed Sunfish
and its 256 MiB limit and accepted this temporary image (send 1.075 s, boot
4.117 s, total 5.207 s, exit 0). No partition was flashed. Command acceptance
alone did not establish staging. The subsequent photo confirms `IMAGE STAGE PASS`,
`ERROR 00000000`, all three segment masks `7FFFFFFC`, `FILE WRITES 04139BEA`,
`ZERO WRITES 0553600B`, both `CHECKED` rows `0966FBF5`, and gap snapshot/check
counts `0019040B`. Payload, zero-tail, gap and metadata-post errors are zero;
INIT returned mask7 and MEM returned mask3. All29 file/BSS segments therefore
passed both native verification passes and all gaps were preserved. This is
not PAS authentication, modem execution, DMA containment or working Wi-Fi.

The next standalone experiment adds exactly one fixed PAS4 AUTH_AND_RESET
request after repeating that entire preparation/staging path. Its sealed
request is `(42000205,1,4,0,0,0,0,0)`; two fresh INFO queries check AUTH and
SHUTDOWN availability first, with no SHUTDOWN executor. The authentication
session can be constructed only by consuming a completed `ImageOnce`; copied
reports or caller success flags are not accepted. Exact consumed-metadata
checks are required both before the INFO queries and immediately before AUTH.
Every transport error, interrupted/BUSY reply, unavailable operation, service
error or metadata fault stops permanently without retry or compensation.
Memory and resource holds stay in place until full platform restart.

This is the first test allowed to release modem reset. It deliberately reports
only `AUTH RETURN PASS` or a secure-call failure, with the raw monitor/service
values. It cannot report modem READY or mint a GLINK peer generation: no SMP2P
or GLINK writer, WLAN QMI exchange, SMMU write or host-DMA setup is added. Full
production lifecycle, shutdown/crash handling and WLAN ownership gates remain
open. The previous production-policy model is not relaxed by this isolated
empirical test, and a successful secure return is not an AFTER_POWERUP event.

An unrooted read of the running Lineage phone confirms that the modem binds to
`subsys-pil-tz` and SMP2P to `qcom_smp2p`; the IRQ table distinguishes the shared
SMP2P parent INTID483, modem watchdog INTID298 and the nested ready/crash/stop
bits. Both cached DTs contain the measured proxy resources but no active clock
or regulator name lists; the matched generic caller treats absent lists as
zero additional active resources. A build gate pins these inputs, PAS4, the
modem firmware name and event routing. This narrows the Linux caller's
requirements; it is not proof of a complete native power or recovery service.

The new core has 28 synthetic state/dependency tests, the SMC adapter has 25,
and the real-input host staging/adapter suite now carries a full staged image
into the authentication core with simulated replies. It checks exact envelopes
and rejects reissue. Eighteen loader/entry/layout/packing tests and 18 build
gates pass. QEMU checks all eleven AP targets, fourteen canaries and real
MMU/kernel handoff, and rejects a corrupted last target before task writes;
it stops before the kernel's first instruction and never simulates a native
secure result. Serial report printing was removed from this diagnostic so it
cannot delay the transition from the visible staging result to AUTH.

Prepared temporary image: `modem-auth-probe/fastboot.img`, 41,115,648 bytes,
SHA-256 `0217b9bedd2b7a3c624277aba259400f282ae20dca770d17decaa8c8c9cb54fb`.
Its AP plan is separately pinned in `loader-task-regions-auth.json`; sources,
images, input proof and test results are archived under
`modem-auth-probe/prepared-modem-auth-probe`. The user reported native
`AUTH RETURN PASS`; the marker requires accepted monitor/service status after
complete staging and fresh metadata/INFO gates. Individual numeric rows were
not supplied. The local seed image was used only to extract its AP layout and
was never wrapped or sent to a device.
After fresh ready Sunfish/LineageOS/slot-a/UID-2000 and matching source,
artifact, private-loader and base-SDK fingerprints, Fastboot confirmed Sunfish
and its 256 MiB limit and accepted the verified authentication/reset-return
image (send 1.120 s, boot 4.367 s, total 5.506 s, exit 0). No partition was
flashed. The native secure return is now confirmed; modem readiness and actual
firmware progress remain unobserved.


The next isolated probe, `build-modem-ready-probe.sh`, repeats that one-shot
sequence and passively inspects modem-owned SMP2P item435 for `slave-kernel`
ready bit1 and fatal bit0. The item must be absent before preparation, remain
absent with the same directory allocation boundaries before pool access and
before authentication, and be newly published afterward. Its version, PID
direction, features, entry counts and bounded allocation are checked; two
complete equal volatile snapshots are required. Fatal/stop/shutdown status
cannot produce `READY BIT SEEN`. This is status evidence, not a production READY
capability or permission to initialize a shared transport.

Only two mappings are added under an explicit exact-manifest mode: read-only
uncached SMEM at86000000 (2MiB) and TCSR semaphore3 at01f43000 (4KiB).
Writes in the observer are limited to lock acquire/release with AP owner1.
No AP SMP2P entry, GLINK ring, interrupt route or WLAN QMI client is initialized.
Both captured DTs pin the lock stride and host/item direction. The unchanged
seL4 configuration exports `CNTPCT_EL0`; the probe checks its progression before
any modem preparation and uses a ten-second post-AUTH observation budget plus
independent work limits. Secure calls themselves remain unbounded by software.
The observer consumes the successful authentication session, preserves held
resources until full restart, and has 34 host dependency/observer tests plus
three exact mapping/mode rejection tests. QEMU verifies all eleven AP targets,
fourteen outer canaries and real MMU/kernel handoff; a corrupted last target
stops before any task write. It stops before the first kernel instruction.
The 41,144,320-byte temporary image is frozen under
`modem-ready-probe/prepared-modem-ready-probe`, SHA-256
`59c59461d331a894de4e5f07aaf58112282f8c38791c048193c2acdbc0ab3e31`. After fresh Lineage UID2000 readiness and
matching frozen source, artifact, private-loader and base-SDK hashes, Fastboot
confirmed Sunfish and its256MiB limit and accepted the temporary boot
(send1.081s, boot4.098s, total5.198s, exit0). No partition was flashed.
The user reported native `READY NOT SEEN`. This marker follows successful
staging and an accepted authentication/reset return; the specific observer
error and item/status rows have not yet been supplied. This does not identify
a timeout, a modem crash or a protocol failure by itself.
The subsequent typed rows are ERROR00000010 (hex timeout), ITEM OFFSET0011D030,
VALID ENTRIES0 and SLAVE KERNELFFFFFFFF (absent). A newly published, decoded
modem-owned SMP2P header is evidence of firmware/shared-status progress, but
no slave-kernel entry or ready bit was observed. The AP outbound item remains
uninitialized in this passive variant.

The follow-on `build-modem-smp2p-probe.sh` is a separate one-shot AP status
publication experiment. The entire host0:1 partition must be empty before
preparation (offset11d000,size7c000,cacheline32,freeU20/freeC7c000). Existing
staging and fresh-metadata/authentication gates remain in place. After AUTH
returns successfully and a new valid remote435 header appears, it acquires
lock3 and allocates only the absent AP item428 in the uncached list. Its full
allocation must fit inside the partition's first4KiB page. It stages the exact
header/payload with version0, verifies every byte, commits/readbacks the free
cursor, publishes version1 last, and verifies again before one fixed APCS
notification at17c0000c bit14. Partial/ambiguous results are terminal and never
reissued or rolled back; allocations and resource holds remain until restart.

The AP payload advertises one `master-kernel` entry at value0. Optional SSR ACK
features are0: this diagnostic cannot recover a restarted modem or acknowledge
restart generations. Peer features must negotiate to0 before the observed ready
bit can produce `READY BIT SEEN`. The AP payload and allocation identity remain
exact on every subsequent sample; peer/fatal/routing/tamper errors stop the
probe. Its bounded observer adds no GLINK, WLAN QMI, IRQ route or host DMA setup.

SMEM is split into three contiguous virtual mappings:511 read-only4KiB pages
and exactly one RW page at8611d000. The exact separate manifest rejects broader
writes, aliases and added IRQs/clients. Native code writes only its new AP block
and the partition free cursor under lock, plus semaphore acquire/release and
the single fixed notification. Host tests exercise the actual adapter, changed
routes, unavailable page capacity, failed readback, unexpected AP allocations,
fatal peers and postpublication tampering. Secure replies, clocks and peer
responses in these tests are synthetic. All31 handshake dependency/adapter
tests and23 loader/mapping/mode tests pass. QEMU reaches the real kernel entry
with all eleven AP targets, exact handoff arguments and fourteen outer canaries
verified; an invalid final target stops before any task write. QEMU stops before
the first kernel instruction. The 41148416-byte temporary image is frozen under
`modem-smp2p-probe/prepared-modem-smp2p-probe`, SHA-256
`c7a41a412b17c53a3a810bf4216113faa6a509c17ba50176189cf8697cc80d00`. Fresh UID2000 Lineage readiness and frozen
source/artifact/private-loader/base-SDK hashes matched. Fastboot confirmed
Sunfish and its256MiB download limit, then accepted the temporary boot
(send1.076s, boot4.084s, total5.174s, exit0). No partition was flashed.
The user reported `HANDSHAKE STOPPED`, ERROR11 (hex Bounds), AP PUBLISHED0
and PEER NEGOTIATED0. The post-AUTH allocation guard stopped before AP block
writes. The rejected cursor was not captured by this report path; its exact
page/alignment/free-space failure remains unspecified. No AP publication or
ready-bit success was observed.

The revised `build-modem-smp2p-preauth-probe.sh` reserves and publishes AP428
while the host0:1 partition is still verified empty, after complete image
staging, both INFO queries and a fresh consumed-metadata check. An opaque,
non-copyable `StatusPermit` can be issued only once by that actual staged-image
session in its available phase. The publisher consumes it under lock3,
revalidates the exact empty partition and absent inbound/outbound items, then
uses the same version-last/readback sequence. AP data starts at0011D030 and
freeU advances only from20 to188. The writable SMEM page remains8611d000.
The leaf reinspects consumed metadata again before its one AUTH/reset call.

After accepted AUTH, the observer verifies the pre-existing AP item and waits
for a new valid peer header before sending its sole status notification. It
performs no post-start AP allocation or payload writes. Observation requires
the permitted authentication session to return successfully, and the AP identity
and payload must remain exact. A fatal, routing or tamper failure retains the
latest sampled directory/status for the visible report. Pre-auth publication
failures now also show the last directory/free offsets; any partial failure
stops without retry or cleanup until full restart. The specific rejected cursor
from the earlier Bounds11 run remains unknown; reserving the AP item in the
verified empty partition removes that later allocation dependency.

Host tests verify exact pre-auth write boundaries, zero status notifications
before successful AUTH, preserved AP bytes through subsequent remote allocations,
peer negotiation and fatal/error paths. A regression case grows the post-start
free cursor beyond the first4KiB while observing a valid negotiated ready bit;
no AP reallocation or SMEM write occurs. The authentication suite additionally
rejects premature, repeated or stale-metadata permits and requires the matching
permitted session to return successfully before observation. Native result for
this revision is pending. All30 authentication dependency/gate tests,31
handshake dependency/adapter tests,34 passive-status tests and23
loader/mapping/mode tests pass. Both actual-image QEMU handoff and injected
last-target rejection checks pass; QEMU stops before the first kernel
instruction. The 41148416-byte temporary image is archived under
`modem-smp2p-preauth-probe/prepared-modem-smp2p-preauth-probe`, SHA-256
`b028957e0df18490980b058b1230b0105a7fd9438ab7d427fa194b37aff8dfda`. Fresh Lineage boot-completed/Sunfish/slot-a/UID2000
and source/archive/artifact/private-loader/base-SDK fingerprints matched; the
earlier frozen AUTH, passive-ready and post-AUTH handshake images remained
unchanged. Fastboot confirmed Sunfish and256MiB and accepted the temporary
boot (send1.078s, boot4.083s, total5.173s, exit0). No partition was flashed.
The user reported native `READY BIT SEEN, 1, 1, 2, 6`. The marker requires
successful AP publication and identity/payload preservation, accepted permitted
AUTH/reset, negotiated peer features0 and a slave-kernel ready bit without
fatal/stop/shutdown bits. The numeric tail likely corresponds to AP PUBLISHED1,
PEER NEGOTIATED1, VALID ENTRIES2 and SLAVE KERNEL6, with ERROR omitted; that
row assignment is not independently confirmed. Native modem status readiness
is now observed. GLINK, WLAN/DMA setup and Wi-Fi connectivity remain untested.

`build-modem-glink-directory-probe.sh` adds a metadata-only snapshot after
repeating that observed-ready path. The handshake issues an opaque one-use
`ReadyObservation` only after its actual permitted AUTH return, AP identity and
payload checks, peer negotiation and ready-bit observation. This evidence is
scoped to read-only diagnostics; it is not a production lifecycle or GLINK-write
capability. The reader holds lock3, rechecks AP/peer identities and ready status,
compares two complete allocation-directory snapshots for items478/479/480,
and revalidates the same ready observation afterward. Missing items are valid
snapshot results, reported by present-mask and zero offset/size.

The bounded directory walker now has a payload-agnostic allocation-metadata
entry point; the SMP2P reader retains its separate340-byte policy. This permits
reporting a32-byte GLINK descriptor without interpreting it as SMP2P. The
inventory never dereferences descriptor indices or FIFO contents, initializes
GLINK state, allocates GLINK items, sends a GLINK notification, or binds an IRQ.
The existing AP status publication/notification is unchanged. All mappings
remain unchanged:511 RO SMEM pages plus the single RW status page. The result
is a point-in-time allocation snapshot, not a FIFO-empty or transport-ready
claim. No additional WLAN, SMMU or host DMA operation occurs.

Host tests include both cached/uncached lists,32-byte descriptors and16KiB
FIFOs, absent items, duplicate IDs, malformed headers, changing snapshots,
lost-ready/fatal/AP-tamper conditions and exact lock release. Instrumented read
views fail if any GLINK payload byte is touched. The37 dependency/handshake/
inventory tests and23 loader/mapping/mode tests pass. Native GLINK-directory
snapshot result is pending. Both actual-image QEMU handoff and bad-last-target
rejection checks pass, with eleven AP targets and fourteen outer canaries;
QEMU stops before the first kernel instruction. The 41156608-byte temporary
image is frozen under `modem-glink-directory-probe/prepared-modem-glink-directory-probe`,
SHA-256 `8ba611749a647c3f6bf3cd5735293e01c7999b356689f30b0a863e36c7a3d2f0`.
On2026-10-18 at06:06:14UTC, Fastboot accepted this temporary image after
confirming Sunfish and a256MiB download limit (send1.079s, boot4.263s,
total5.352s, exit0). The frozen image hash matched immediately before sending.
No partition was flashed. This is command acceptance only; the native directory
status and allocation readout remain pending.
The user subsequently reported `DIRECTORY SNAPSHOT PASS, 5, 11D330 and 20,
0 and 0, 11D360 and 4000`. Interpreted as present-mask and the three offset/size
pairs, this is mask5: descriptor478 at0x11d330,32bytes; no AP TX479;
AP RX480 at0x11d360,16384bytes. ERROR was not supplied separately; zero error
and the repeated ready/AP identity checks follow from the successful marker.
This establishes a native point-in-time allocation snapshot, without reading
any descriptor index or FIFO payload or establishing GLINK negotiation.

The next `build-modem-glink-receive-probe.sh` variant enables the explicit
`sunfish-glink-receive-probe` feature. It repeats the same ready path, then
revalidates the directory, AP identity and ready status around two identical
32-byte descriptor snapshots and at most one8-byte pending RX header. The
four active indices must be aligned to8bytes and below the fixed16KiB FIFO
size before any FIFO read. Empty rings are valid results; other commands,
versions and feature bits remain raw observations. Only command0/version1
gets the `VERSION OFFER SEEN` label, which does not mean version negotiation.
No TX payload is read, FIFO index is advanced/reset, GLINK item is allocated,
notification/version ACK is sent, IRQ is bound or WLAN/DMA setup is performed.
The existing512 SMEM page mappings and sole AP status write page are unchanged.
The directory-only build remains available without the new feature. Native
receive-header result is pending.
The46 dependency/handshake/inventory/receive host tests pass, including read
spies that reject any TX payload or RX byte outside the first pending header,
empty/wrapped-availability cases, invalid index/size rejection before FIFO
reads, changing descriptor/header rejection, final ready revocation, and exact
SMEM/lock preservation. All20 source/entry/layout/mapping/mode checks and18
common build gates pass. The complete image passes actual-image QEMU kernel
handoff and bad-last-task rejection checks (eleven targets, fourteen outer
canaries, stop before the first kernel instruction). All512 unique SMEM frames
are unchanged. The41160704-byte image is frozen under
`modem-glink-receive-probe/prepared-modem-glink-receive-probe`, SHA-256
`63ab49658771771948be9f3a00c660169d4c7a0d6df714b585b418861f537b4d`;
the PD ELF SHA-256 is
`886c02eac37e351752f074339fc5cddd947871e6c0c17b3f3a3427e2b1262559`.
The earlier AUTH, passive-ready, post-AUTH, pre-AUTH and directory images
remain unchanged. These local checks do not establish a native receive result.
On2026-10-18 at06:18:15UTC, Fastboot accepted the frozen receive image after
fresh Lineage readiness and source/archive/artifact/base-SDK fingerprint checks,
then Sunfish/256MiB confirmation (send1.081s, boot4.067s, total5.157s,
exit0). No partition was flashed. Native final status/header values are pending.
The user then reported `RX EMPTY SNAPSHOT PASS`, ERROR0 and RX AVAILABLE0.
The successful marker validates the32-byte descriptor,16KiB RX size, bounded
aligned indices, repeated contents and final ready/AP identity checks. The
individual indices and present-mask were not reported; head equals tail but
neither value is independently known to be zero. No version offer or packet
was observed. Empty at this instant does not establish a permanently idle ring.

The next standalone `build-modem-glink-prepare-probe.sh` test stops before
authentication. It consumes the actual complete-image/INFO/fresh-metadata
session into an opaque `GlinkMemoryPermit`, leaving no AuthOnce to execute.
Under lock3 it requires the exact AP-only partition with freeU0x188, intact
AP428 and absent435/478/479/480. It initializes fresh descriptor478
at0x11d198 (32bytes) and TX479 at0x11d1c8 (16384bytes), verifies the two
headers/data twice, then commits freeU0x41c8 once. It rechecks the directory,
AP payload and4028 untouched bytes, plus a third allocation-byte comparison.
Partial failure retains memory until full restart; no rollback/retry is added.
The separate exact manifest admits five uncached RW SMEM pages
0x8611d000..0x86122000 (four more than the earlier status-only writer) and
507 RO pages. No existing descriptor is reset, packet sent, notification/IRQ
added, authentication/reset issued, modem started or WLAN/DMA setup attempted.
This is not a production transport ownership/epoch capability. Native
memory-preparation result is pending.
All39 preparation/dependency tests and32 authentication tests pass. Native
five-page adapter tests preserve neighboring memory and lock words; rejected
or partial operations cannot retry through the same adapter. Fault tests
prevent cursor publication before data readback and detect post-commit guard
corruption without rolling back. All22 source/entry/layout/mapping/mode checks
and18 common build gates pass. Actual-image QEMU handoff and bad-last-task
rejection pass with eleven targets and fourteen outer canaries, stopping before
the first kernel instruction. The41156608-byte image is frozen under
`modem-glink-prepare-probe/prepared-modem-glink-prepare-probe`, SHA-256
`ff0eb645e93a64a7b247d86784db66d12c8b9aa90644c9fcbdce1a5bdf612abd`;
PD ELF SHA-256
`d0916601ef1668bd5e9a18ecb25b6a89b1c1d101c08d25dcd46a8909317db435`.
All512 SMEM frames remain unique; the separate manifest explicitly changes
only the four additional page permissions needed by this memory experiment.
The six earlier frozen images remain unchanged. Native result is pending.
Fastboot accepted the frozen pre-AUTH memory image on2026-10-18
at06:35:44UTC after fresh Lineage readiness, source/archive/artifact/base-SDK
fingerprint checks and Sunfish/256MiB confirmation (send1.078s, boot4.074s,
total5.164s, exit0). No partition was flashed. Native memory-preparation
status/error and optional free-cursor/guard readout are pending.
The user reported `GLINK MEMORY PREPARE PASS`, FREE AFTER41C8 and GUARD
BYTESFBC, confirming native initialization/readback and preservation of the
4028 untouched bytes. ERROR was not supplied separately; zero follows from
the successful marker. This test consumed its authentication session and did
not authenticate/start firmware or send GLINK packets.

The next separate `build-modem-glink-boot-probe.sh` variant combines the
observed fresh-memory preparation with the existing one-shot authentication
and status-ready experiment. A private `Boot` wrapper owns the actual AuthOnce
while the one-use memory grant is issued and real preparation runs. Failed
preparation drops that session. Before its single AUTH call, it revalidates
all prepared headers/data, AP identity, exact free cursor and absent peer/RX
items, and the caller supplies another fresh consumed-MDT inspection. Secure
failure returns only diagnostics; no post-start identity is issued.

Successful authentication returns an opaque, non-copyable `BootIdentity`
bound to the two freshly prepared allocation identities. After actual SMP2P
ready, the reader consumes that identity and the separate sealed ready
observation. It checks retained478/479 addresses/sizes, unchanged AP TX head
and RX tail, zero remote TX tail and reserved descriptor words, and two complete
zero-TX readbacks between matching descriptor/header/directory/ready snapshots.
The existing five-page manifest is unchanged. No GLINK write, allocation,
index reset/advance, packet, notification/ACK, IRQ, WLAN or host DMA setup
occurs after AUTH. This readout identity is not a production lifecycle,
ownership/epoch or transport-write capability. Native boot-memory result is
pending.
All46 boot/preparation/dependency tests and34 authentication tests pass,
including changed pre-AUTH memory blocking the secure call, rejected secure
replies producing no identity, bounded pending-header reads, post-start index/TX
tampering and readiness revocation during TX verification. All22 Python
source/entry/layout/mapping/mode checks and18 common build gates pass. Both
actual-image QEMU kernel-handoff and bad-final-task rejection pass, comparing
eleven targets and fourteen outer canaries before the first kernel instruction.
The five-page manifest and all512 unique SMEM frames are unchanged. The
41164800-byte image is frozen under
`modem-glink-boot-probe/prepared-modem-glink-boot-probe`, SHA-256
`7b617f2726d53f2875ce0e4b6bb640d43c94bb995870cfb7d1949a15b05c564a`;
PD ELF SHA-256
`0e7b2c6a5adb8b3f1c974c9a8edc9bfa4ea61e0453c9e7d7e43b9afb0a0645bb`.
Seven earlier frozen images remain unchanged. Native boot-memory result is
pending.
Fastboot accepted this frozen boot-memory image on2026-10-18 at06:52:37UTC
after fresh Lineage readiness and source/archive/artifact/base-SDK fingerprint
checks, then Sunfish/256MiB confirmation (send1.101s, boot4.067s,
total5.177s, exit0). No partition was flashed. The native final status,
error/present-mask and receive-availability readout remain pending.
The user reported `BOOT MEMORY PASS`, ERROR0, PRESENT MASK7 and RX AVAILABLE8.
This confirms native authentication/status-ready with retained descriptor/TX
identities, unchanged zero AP indices/remote TX tail/reserved descriptor words,
and both complete zero-TX checks. All478/479/480 are present, and the repeated
receive snapshots contain an eight-byte header. COMMAND/PARAM ONE/PARAM TWO
have not yet been reported; this must not be labeled a version offer until
those values are captured from the current screen. No GLINK packet was sent
or RX header consumed, and no post-AUTH GLINK write occurred.
The user restarted before the command/parameter rows were captured. ADB
confirmed return to boot-complete Sunfish LineageOS, slot-a, UID2000. The prior
pass/mask7/RX8 remain recorded; the packet's identity is unknown. The unchanged
frozen boot-memory image is being replayed solely to recover those fields.
The unchanged image was accepted again on2026-10-18 at07:04:23UTC after
fresh source/archive/artifact/base-SDK fingerprint checks and Sunfish/256MiB
confirmation (send1.081s, boot4.092s, total5.182s, exit0). No partition was
flashed. The second attempt's native header readout remains pending; command
acceptance does not supply those values.

The user then reported `command 0, param one 1, param two 7` on the unchanged
replay. These fields identify a GLINK VERSION v1/features7 offer. The replay's
status/error/mask rows were not separately repeated; the original
BOOT MEMORY PASS/ERROR0/MASK7/RX8 remain prior evidence. The header was read
without consuming RX or sending a packet, so version negotiation is still
unproven.

The user requested preparation only, with no reboot or new image sent. The
separate `build-modem-glink-version-probe.sh` experiment consumes fresh evidence
from its own actual preparation/authentication and a separate ready observation.
It requires the exact initial offer0/1/7 and untouched zero TX, stages two fixed
eight-byte messages (VERSION_ACK1/features0, then VERSION1/features0), verifies
the full TX buffer twice, publishes AP TX head16 and AP RX tail8 once, and sends
one APCS bit12 notification after releasing the directory lock. It polls read-only
for at most five seconds/512 samples, checking live ready state, identities,
indices, exact TX contents, complete16-byte consumption and ACK1/1/0. The reply
ACK remains pending; any unexpected state stops without retry or cleanup.
No channel, IRQ handler, QRTR/WLFW request or WLAN/DMA setup is included. The
five-page mapping is unchanged. Its separate non-copyable `FreshBoot` evidence
cannot be derived from the earlier read-only `BootIdentity` and does not replace
production lifecycle/epoch ownership or recovery. The image will remain local
until the user explicitly requests an on-device run.

All54 version/boot/preparation/dependency tests pass, including exact native
adapter stores, single submission/notification, malformed peer state, partial
publication, timeout and readiness revocation during post-publication TX
verification. The22 Python source/layout/mapping/mode checks and18 common build
gates pass. Actual-image QEMU validates all eleven task copies and the real
kernel-entry handoff, plus rejection of a bad final target before any task
write; both preserve fourteen outer canaries and stop before the first kernel
instruction. All512 unique SMEM frames, the five-page manifest and eight prior
frozen images are unchanged. The41168896-byte prepared image is archived under
`modem-glink-version-probe/prepared-modem-glink-version-probe`, SHA-256
`4cad56b81d2794b296ce70857dfae041adff249deba5d1078b991156104cfb26`;
PD ELF SHA-256
`34c9143a2ebefba98edebde3a89085b904d64c766274293daba0416998ce4216`.
Host tests use synthetic peer/secure replies; QEMU checks the loader only.
The version image has not been sent or run on the Pixel. Native negotiation
and Wi-Fi remain unproven.

The user subsequently explicitly authorized running this prepared image.
All133 workspace sources,32 private loader sources,24 saved artifacts,
four base-SDK files and eight previous images matched their frozen hashes.
ADB confirmed boot-complete Sunfish LineageOS23.2, slot-a, UID2000. The reboot
request timed out, but the user confirmed Fastboot Mode and the serial then
reappeared. Product sunfish and the256MiB download limit were checked before
sending the unchanged archived image. Fastboot accepted temporary boot on
2026-10-18 at09:33:26PDT (send1.091s, boot4.374s, total5.475s, exit0).
No partition was flashed. Native final status/error and TX-consumed/ACK rows
are pending; command acceptance does not establish version negotiation.

The user reported `VERSION EXCHANGE STOPPED ERROR 8 TX CONSUMED 1 ACK SEEN 0`,
then `COMMAND 2, RX AVAILABLE 50, COMMAND 2, PARAM ONE 1, PARAM TWO A`.
The display uses hexadecimal: Payload8, a peer OPEN for channel1 with ten name
bytes and80 receive bytes queued. TX CONSUMED1 confirms a successful native
post-submission snapshot with remote TX tail16 after the fixed version send
and sole notification. The ACK-only test stopped on the pending OPEN rather
than consuming or acknowledging it. The channel name is unknown; an expected
ACK may be behind that OPEN, so ACK SEEN0 does not prove the peer sent no ACK.
No channel was accepted, and WLAN remains unimplemented.

The separate pending-RX diagnostic repeats the exact fixed16-byte version send
and sole notification. After a validated TX-consumed/header sample it copies
up to128 pending receive bytes twice, then revalidates TX, descriptor, allocation
identities and ready state without consuming a receive byte. Its parser follows
only bounded VERSION, VERSION_ACK, OPEN and READ_NOTIF records in the owned
snapshot. OPEN names must fit32 bytes including a final NUL and contain printable
ASCII; malformed/truncated or unknown records stop parsing. An ACK1/1/0 behind
an OPEN can be reported as `ACK IN SNAPSHOT` without accepting either message
or changing the original ACK-at-front test result. A prefix cannot establish
absence in the unread remainder. The screen displays the first three records
and up to21 name characters, uppercasing letters and leaving punctuation blank
for the existing fixed font. It reports snapshot, parser and version errors
separately. No extra TX bytes, head/tail stores, notification, channel response,
IRQ, WLAN or DMA path is added. This diagnostic is being prepared locally;
no subsequent image has been sent.

All62 pending/version/boot/preparation/dependency tests pass, including
unchanged fixed TX/notification counts, an ACK behind an OPEN without RX
advance, bounded names/prefixes, unstable bytes, readiness revocation,
descriptor progress and terminal reuse across the adapter's two entrypoints.
All22 Python source/entry/layout/mapping/mode checks and18 common build gates
pass. Final-image QEMU kernel-entry handoff and bad-final-target rejection
pass with eleven targets and fourteen outer canaries, before the first kernel
instruction. The512 SMEM frames and five-page mapping are unchanged, and all
nine prior frozen images match. The41168896-byte image is archived locally
under `modem-glink-pending-probe/prepared-modem-glink-pending-probe`, SHA-256
`39368d24a93c59a70e3b57d345acef241b57590c08cceb07fa8a884e23f35c74`;
PD ELF SHA-256
`e7ba456f8861bff7327df8ec4b86051f0f3c598007ae1278ea1a7ba935e18a24`.
These are host/loader checks; the pending-RX diagnostic has not run natively.

After the requested Lineage readiness, all137 workspace sources,32 private
loader sources,25 artifacts, four base-SDK files and nine earlier images
matched their saved fingerprints. ADB confirmed boot-complete Sunfish
LineageOS23.2, slot-a, UID2000. The reboot command again timed out waiting for
its ADB reply; Fastboot subsequently enumerated the same serial without a
second reboot request. Product sunfish and the256MiB limit were checked.
Temporary boot of the unchanged pending-RX archive was accepted on2026-10-18
at09:55:49PDT (send1.091s, boot4.175s, total5.275s, exit0). No partition was
flashed. Native snapshot status/error, ACK presence and channel names remain
pending; acceptance of the boot command supplies none of those observations.

The user's photograph confirms native `RX SNAPSHOT PASS`: VERSION ERROR8,
TX CONSUMED1, ACK AT FRONT0, SNAPSHOT ERROR0, RX AVAILABLE50/CAPTURED50,
PARSED MESSAGES4, ACK IN SNAPSHOT1, PARSE ERROR0, QUEUE COMPLETE1 and
UNKNOWN COMMAND0. These are hexadecimal; the complete80-byte queue has three
OPENs followed by the exact expected VERSION_ACK1/1/features0. OPEN remote IDs
and name lengths are1/10,2/7 and3/19. The normalized displayed names are
`GLINK SSR`, `IPCRTR` and `SSM RTR MODEM APPS`; the font cannot independently
confirm wire case or punctuation. The first three aligned records occupy
24+16+32 bytes, so the source-gated exact ACK is the fourth eight-byte record.
This confirms native fixed version-message consumption and peer acknowledgement.
The earlier ACK-at-front test still reports Payload8, because OPEN is first.
No channel was accepted or acknowledged, RX tail remains8, and no QRTR payload
or WLAN operation occurred. The photo is retained alongside the native record.

The next standalone experiment sends only a fixed local IPCRTR OPEN request.
It requires a non-copyable continuation minted inside the actual fresh version
exchange, retaining the separately observed ready state and validated snapshot;
copied reports cannot mint it. Before any new byte, it rechecks the complete
80-byte queue, exact wire names/IDs, VERSION_ACK, retained allocations, AP
indices and zero unused TX. It appends sixteen fixed bytes at TX offset16,
verifies the complete prefix/remainder twice, publishes head32 once and sends
one additional bit12 notification outside the lock. RX tail stays8 and the
original80 bytes remain untouched. A bounded read-only scan of up to128 new
bytes checks for OPEN_ACK4/local1/0 and full32-byte TX consumption. Known intent
metadata may be skipped by checked size but is neither accepted nor used as
an address. No peer OPEN_ACK, RX advance, intent advertisement, QRTR payload,
WLAN/DMA, generic packet, retry or cleanup is provided. This is the local-open
request leg, not a completed channel or production lifecycle/epoch authority.

All72 open/version/pending/boot/preparation/dependency tests pass, including
exact native adapter stores, unchanged RX tail and prefix, terminal reuse,
partial publication, timeout, wrong ACK, readiness revocation before head
publication and during reply verification, and bounded/unstable new RX reads.
Stopped reply classification retains the first copied header for the screen.
The22 Python source/entry/layout/mapping/mode checks and18 common build gates
pass. Final-image QEMU validates eleven task copies, real kernel-entry handoff,
bad-final-target rejection and fourteen outer canaries, stopping before the
first kernel instruction. All512 unique SMEM frames, five-page permissions and
ten previous frozen images are unchanged. The41177088-byte image is archived
under `modem-glink-open-probe/prepared-modem-glink-open-probe`, SHA-256
`372e67f0bf0b69152c71b68101a2f36dc3cdf0d58070c1d50c2b40eb443237c2`;
PD ELF SHA-256
`7ec6050811873100a63c2cf54a459bfc59bc4ab7b60858ba1067a27b1765b403`.
The new request image is locally prepared only; no native request or completed
channel is claimed by these host/loader checks.

The user supplied readiness in response to the requested Lineage restart.
All141 workspace sources,32 private loader sources,28 artifacts, four base-SDK
files and ten prior images matched their saved fingerprints. ADB confirmed
boot-complete Sunfish LineageOS23.2, slot-a, UID2000. The ADB reboot reply
timed out; the same serial subsequently enumerated in Fastboot without another
reboot request. Product sunfish and the256MiB limit were checked before sending
the unchanged archive. Temporary boot was accepted on2026-10-18 at10:30:03PDT
(send1.081s, boot4.108s, total5.198s, exit0). No partition was flashed. Native
final status/error, request consumption and OPEN_ACK rows remain pending;
command acceptance does not establish those results or a completed channel.

The user reported `IPCRTR REQUEST STOPPED. ERROR 8. TX CONSUMED 0. OPEN ACK
SEEN 0.`, then STAGED0, HEAD STORE0 and NOTIFICATIONS0. The request was not
published or notified. STAGED is set only after verification, so zero alone
cannot exclude unpublished byte stores. The first image cannot distinguish
the cached plan guard from Payload8 during a live status check. Its actual
fresh version checkpoint was reached; this does not establish peer rejection
of an OPEN, which was never published.

Revision2 removes literal spelling checks on the two unaccepted channel names.
Their normalized photograph did not establish exact wire case/punctuation,
and their spelling does not authorize the fixed IPCRTR request. Queue lengths,
record IDs/order, parser bounds, every live queued byte, ACK, indices,
allocations, zero unused TX and ready checks are retained; the target still
requires exact raw `IPCRTR`/ID2. This is a correction to an unnecessary plan
assumption, not independent identification of the original failure site.
The request bytes and write/notification bounds are unchanged. The screen now
reports issued BYTE WRITES, GUARD STAGE and PLAN DETAIL. Stages1/2/4/6/8 locate
plan, initial live, staged live, published live and reply checks; plan details
1..6 locate queue length, indices, parser, OPEN shape, target name and ACK.
The new regression permits unrelated uppercase wire names while still rejecting
a differently cased target name, changed IDs, incomplete ACK and altered RX.

All73 open/dependency tests,22 Python source/entry/layout/mapping/mode checks
and18 common build gates pass. Final-image QEMU kernel-entry handoff and
bad-final-target rejection pass with eleven copies and fourteen outer canaries,
before the first kernel instruction. All512 unique SMEM frames, the five-page
permissions and eleven older frozen images are unchanged. The41177088-byte
revision2 image is frozen under
`modem-glink-open-v2-probe/prepared-modem-glink-open-v2-probe`, SHA-256
`d4765751956223ef64896ecf4913a83c50b550de8cb425558565c501c8e8ccfd`;
PD ELF SHA-256
`8321351a7e844d84213df172d976df545f58df3062affb2ec06313fd620d8397`.
After the user's Lineage-ready reply, fresh device checks and all frozen-file
fingerprints passed. Temporary boot of revision2 was accepted on 2026-10-18
at 11:04:37 PDT (exit0; send1.084s, boot4.119s, total5.218s), without flashing
a partition. The native status, request publication and peer reply remain
unobserved pending the screen readout. Command acceptance does not establish
channel success. The original stopped image and evidence remain saved separately.

The subsequent device photograph confirms `IPCRTR REQUEST PASS`, ERROR0,
STAGED1, HEAD STORE1, NOTIFICATIONS1, TX CONSUMED1 and OPEN ACK SEEN1.
TX tail/head are both0x20; RX tail8/head0x60, available0x58, with8 new bytes.
The reply is OPEN_ACK4/local1/0; samples1, issued byte writes0x10,
guard stage8 and plan detail0. The fixed request was consumed and acknowledged.
No peer OPEN_ACK was sent, RX remains unconsumed after the initial version
offer, no intents were advertised and no QRTR payload/WLAN/host DMA was started.
This establishes the local request leg, not a usable channel or working Wi-Fi.
Photo SHA-256: `f14d19be7484153514760055f01c9d8f967c9517e940a52ef0c97af790d33d08`.

The next prepared experiment consumes a private, non-copyable continuation of
the actual successful request execution. It revalidates the exact32/32/8/96
descriptor state, the original80 queued bytes and the eight-byte local OPEN_ACK,
all TX bytes, allocation identities and ready state. It then appends only
OPEN_ACK4/remote2/0 at TX offset32, verifies all TX twice, rechecks readiness,
publishes head40 once and sends one additional bit12 notification outside lock3.
It polls for TX tail40 and copies at most128 new control bytes twice. INTENT5
metadata for remote2 is counted but its size/ID words are not used; READ_NOTIF13
and SIGNALS15 are bounded control records. Unknown/truncated/changing replies
stop the test. RX tail remains8, all prior88 unread bytes are preserved and
no intent, QRTR payload, WLAN or host DMA is advertised/started. ACK consumption
alone does not establish that any intent arrived or that QRTR is usable.

All83 host tests and19 source/entry/layout/mapping/mode tests pass, along with
the18 common build gates. The actual final image passes QEMU kernel-entry
handoff and bad-final-task rejection, checking11 copies and14 outer canaries
before the first kernel instruction; these are loader checks, not native GLINK
evidence. The same512 unique uncached SMEM pages remain five RW and507 RO,
without aliases/IRQs. All12 older images and the base SDK are unchanged.
Prepared image: `modem-glink-channel-probe/prepared-modem-glink-channel-probe`,
41181184 bytes, SHA-256
`582ce57d51d79ca6ee0b3da09cee4f58fd6a38b17cbb912641f7e32d91c9b215`;
PD ELF SHA-256
`b631337663b261eaea3fbc6633c05da929b1a5e37b5719d65cf77b350a692bf5`.
After a fresh Lineage-ready reply, device identity and all frozen fingerprints
were verified. Temporary boot of the reciprocal-ACK image was accepted on
2026-10-18 at 12:14:14 PDT (exit0; send1.079s, boot4.139s, total5.228s),
without flashing a partition. Native status, ACK consumption and any peer
intent/control reply remain unobserved pending screen capture. Command
acceptance does not establish a complete channel or working Wi-Fi.

The subsequent photo confirms `IPCRTR ACK PASS`, ERROR0, STAGED1, HEAD STORE1,
NOTIFICATIONS1 and TX CONSUMED1. TX tail/head0x28, RX tail8/head0x248 and
available0x240. The128-byte captured prefix contains8 control records, all8
INTENT records/entries; its first header is5/remote2/count1. Samples1,
byte writes8, guard stage8. Thus the reciprocal fixed acknowledgement was
consumed. Of488 new bytes after the earlier88 unread bytes,360 remain outside
the snapshot and unclassified. Eight is a prefix count, not a total intent
count. The snapshot does not capture intent sizes/IDs. RX tail8 remains
unchanged; no receive intents, QRTR payload, WLAN or host DMA were advertised
or started. Photo SHA-256:
`8c24926a5292abc6caf8888faa2325ae48abb45f8d450306486a5513c4a3ed1e`.

The next isolated image repeats the same actual owned boot/version/OPEN/peer-ACK
path and expands only the final read-only control-copy bound from128 to512
bytes. It preserves the native writer,40 transmitted bytes, AP heads16/32/40,
RX tail8, all88 earlier unread bytes and three total GLINK notifications.
It decodes up to64 size/ID observations into at most8 size groups, reporting
ID range, duplicates, zero sizes and signal bits. These are diagnostics only:
no remote size drives allocation and no ID selects a send or buffer. Complete
inventory is displayed only when every currently queued new byte fits and
parses; otherwise only a prefix may be claimed. Double copies, ready/identity,
TX and preserved-prefix checks remain. No extra packets/index writes,
notifications, RX consumption, receive-intent advertisement or QRTR/WLAN/DMA
operation is added.

All91 host tests,19 source/entry/layout/mapping/mode tests and18 common build
gates pass. The final image passes QEMU kernel-entry handoff and bad-final-task
rejection (11 copies,14 canaries, before the first kernel instruction). No QEMU
result is native intent evidence. All13 previous images and the base SDK are
unchanged; the512 unique uncached SMEM frames still comprise five RW and507 RO
pages without aliases or IRQs. The prepared image is frozen under
`modem-glink-intents-probe/prepared-modem-glink-intents-probe`,41185280 bytes,
SHA-256 `37c94ded69b54776837279fed63c724ef375d215da6984132b3786b4f64b79e6`;
PD ELF SHA-256
`61f5dcb3d88dd3a6e9471e2a635fabf7ba6aa5d881677b7b708c983b18ddd39d`.
The Pixel had already returned to boot-complete LineageOS, confirmed over ADB
with the expected serial, Sunfish product and slot_a. Under the user's standing
instruction to continue until manual action is needed, the frozen expanded
snapshot was temporarily booted after product/download-limit and image-hash
checks. Fastboot accepted it on 2026-10-18 at 12:29:35 PDT (exit0; send1.074s,
boot4.073s, total5.164s), without flashing a partition. The native expanded
inventory status and full-queue/size/ID observations remain pending screen
readout. No new intent count or usable transport is inferred from acceptance.

The native photo shows `INTENT SNAPSHOT STOPPED`, Payload8, TX CONSUMED1,
RX head0x248/available0x240 and NEW RX PENDING/CAPTURED both0x1e8. All488
new bytes were copied, but QUEUE COMPLETE0 because classification failed.
Guard stage8, byte writes8, head store1 and notifications1 are shown. The
displayed5/remote2/count1 header is the first captured record, not the rejected
record. No inventory count/size/ID or exact failure site was captured. Frozen
source gates establish stable copies and revalidated ready/identity/TX state
before classification, plus retained RX tail8 and the earlier88 unread bytes.
The reciprocal ACK was consumed. Photo SHA-256:
`1c2b4437dc3e06d91b13d4c4390970d0b5d508730b8690f8d69a520236b98f1b`.

Revision2 retains the same512-byte copy bound and native writer, but uses one
copied-byte scanner to preserve complete-record prefix statistics and report
the exact rejected offset/header plus up to16 preview bytes. Stop offsets are
relative to the new post-ACK bytes starting at RX offset96. Detail codes1..7
identify input bounds, unknown command, channel, count, truncated record,
entry storage and size-group storage. A rejected record commits no partial
counts. At most five of the eight stored size groups are displayed.

The per-record count limit now derives from the512-byte copy: at most63
size/ID pairs can fit after the8-byte header. Keeping the old15-pair limit
from the128-byte reader was an unnecessary restriction; the exact native
failure cause remains unproven. The scanner also recognizes the documented
header-only receive-intent request7/remote2 as a passive diagnostic, recording
its size without allocating or answering it. The matched source sends the
eight-byte command/CID/size structure and defers it with zero extra payload.
No intent is selected, no RX/index/packet/notification write is added and no
QRTR payload, buffer advertisement or host DMA is enabled.

All98 host tests,19 source/entry/layout/mapping/mode tests and18 common build
gates pass. Regressions cover a63-entry record, a synthetic488-byte queue
containing a44-entry group, exact failed offsets, whole-record-only stats,
truncated previews and passive requests with unchanged memory. These fixtures
are not native metadata observations. The final image passes both QEMU loader
checks (11 copies,14 canaries, before the first kernel instruction). The same
512 unique uncached SMEM frames, five RW pages and base SDK remain unchanged;
all14 prior images are preserved. The41189376-byte image is frozen under
`modem-glink-intents-v2-probe/prepared-modem-glink-intents-v2-probe`, SHA-256
`2291f313842bebe6011141050ead963a63f32eedc472742c3f25b4d45c0ab91d`;
PD ELF SHA-256
`b0e9cca52a62a10ed69037bdd024d386d6ffbe2209708b8093b5a382b801d007`.
After the user's Lineage reply, fresh live device and frozen-file checks passed.
Fastboot confirmed Sunfish and its256MiB download limit, then accepted the
revision2 temporary boot on 2026-10-18 at 12:48:45 PDT (exit0; send1.079s,
boot4.104s, total5.194s). No partition was flashed. Native revised-scan status,
inventory and any rejected record remain unobserved pending screen capture;
no parser fix or protocol success is inferred from command acceptance.

The native photo confirms `INTENT INVENTORY PASS`, ERROR0, TX CONSUMED1,
RX head0x248 and pending/captured/parsed0x1e8, QUEUE COMPLETE1. All488 current
new bytes were parsed:31 control records,30 INTENT records/entries and one
receive-intent request of0x34(52) bytes. IDs range0..0x1d with zero duplicates
or zero sizes. The four size groups are128 bytes x16,512 bytes x8,1152 bytes x5
and8320 bytes x1. Signal bits are zero. The photo does not show each size-ID
pair; no cached individual mapping is assumed. RX tail8 remains unchanged,
no intent was selected/advertised and the request was not answered. No QRTR
packet has been received or sent. This different boot does not independently
identify the original stop cause. Photo SHA-256:
`576889e54a2c73fb2dd48b8e95328a41bc0ec21685110aed804ee3e3c540035f`.

The next first-receive image is prepared, not yet sent. An owned continuation
from the actual complete running exchange gates one fixed 64-byte local
intent (ID1) and its positive request acknowledgement. The 24 control bytes
append at TX40, publish head64 once and issue the fourth GLINK notification.
The private buffer has 32 guard bytes, is RW/NX and stays reserved through
restart; its address is never transmitted. The probe retains RX tail8 and
the prior576 unread bytes, double-copies at most80 new bytes and accepts only
one DATA9/remote2/local-intent1 frame with chunk52 and left0. It verifies two
volatile readbacks of the copied52 bytes, the guards and12 zero trailing
bytes, then displays the packet's header words. A52-byte request alone does
not establish QRTR HELLO. No RX_DONE, outgoing QRTR, peer-intent selection,
receive-tail advance, WLAN setup or host DMA is added.

The receive candidate passes111 host tests,19 source/layout/mapping/mode
tests and18 common build gates. Tests cover partial publication, bounded
headers, changed readiness before and after copying, terminal failure,
preserved queue prefixes and later unconsumed suffixes. Both QEMU loader
checks pass with11 task regions and14 outer canaries; neither executes a
kernel instruction or supplies native transport evidence. All15 earlier
images and the base SDK are preserved. The41193472-byte temporary image is
frozen under `modem-glink-rx-probe/prepared-modem-glink-rx-probe`, SHA-256
`8c96b254ff6451d3b6d69b1b8c159ba3faec77031e8dcd976eefdd0624998702`;
PD ELF SHA-256
`be6e3f7921136c5229b1dd9cb754404996776d3ba294abcaf1220bf01bae0ab2`.
Native first-packet receipt remains unobserved.

Fresh live ADB verified boot-complete LineageOS on Sunfish before this run;
all frozen source/artifact fingerprints,15 earlier images and the base SDK
matched. Fastboot confirmed Sunfish and its256MiB download limit, then accepted
the temporary receive image on 2026-10-18 at 13:18:55 PDT (exit0; send1.081s,
boot4.087s, total5.177s). No partition was flashed. Native receive status,
buffer checks and packet identity remain pending; command acceptance is not
evidence that the new controls ran or that a packet arrived.

The native photograph now confirms `RECEIVE COPY PASS`, ERROR0, stage9,
TX consumed/head0x40, RX head0x290 and available0x288. DATA9/remote2/local
intent1 carried0x34(52) bytes with left0; all52 bytes were copied and the
buffer guards passed. The QRTR words are version1/type2, source node0 and
port0xfffffffe, confirm0, body20 bytes, destination broadcast0xffffffff and
port0xfffffffe; body word0 is2. This is HELLO-shaped packet evidence, not
validation of the undisplayed remaining body words. RX tail8 stayed unchanged
and no RX_DONE or outgoing QRTR was sent. The unique buffer remains reserved.
Original HEIC SHA-256:
`e83d08e4b9084f72d9b6a75f354a81719f38781579ed0fc4c6684228b5522069`.

The separate receive-completion candidate is now prepared and frozen, not
sent. A non-copyable continuation minted only by the actual successful
receive retains its unique buffer lease. It revalidates the full648-byte
queue and copied52-byte payload before staging one fixed eight-byte
non-reusable RX_DONE6/localCID1/intent1 at TX64. After complete TX readback
and live checks, it stores RX tail656 and TX head72 once, with checks between
stores, then issues the fifth GLINK notification outside the lock. Earlier
other-channel offers are consumed as unaccepted offers, without acknowledging
or opening those channels. The copied packet and buffer remain private and
reserved through restart. Once RX tail is published, the released peer bytes
are never reread; later unread bytes are retained. No new/reused intent,
outgoing QRTR, WLAN or host DMA is added. TX consumption would confirm the
fixed control reached the peer FIFO reader, not its internal retirement logic
or a complete production channel lifecycle.

All122 dependency/completion host tests pass, including partial cursor
publication, terminal success/failure, late readiness loss, differing double
copies, immediate overwrite of released RX bytes and retention of a later
unclassified suffix. The20 source/entry/layout/mapping/mode tests and18 common
build gates pass. Both QEMU loader checks pass with11 task regions and14
canaries, stopping before the first kernel instruction. The same512 unique
SMEM frames/five RW pages, base SDK and16 prior images are preserved.
The41205760-byte image is frozen under
`modem-glink-done-probe/prepared-modem-glink-done-probe`, SHA-256
`544ba0e1c440d0226d022749f8c8bfb6d7ed15c9e5ec6da4d92c5816da170bff`;
PD ELF SHA-256
`8b198bd4687fbe4b4cf6f43f24dbc1f9f72fdb764bf1cd57b40006f7654b2015`.
Native receive completion remains unobserved.

Live ADB then verified boot-complete LineageOS on the same Sunfish, and all
161 source/32 private-loader/53 artifact fingerprints,16 prior images and
base SDK matched. Fastboot confirmed the product and256MiB download limit
and accepted the temporary completion image on 2026-10-18 at14:50:03 PDT
(exit0; send1.078s, boot4.097s, total5.186s). No partition was flashed.
Native completion status and cursor/retirement readout remain pending;
command acceptance does not establish that RX_DONE ran or was consumed.

The native photo confirms `RXDONE CONSUMED PASS`, ERROR0, stage8,
BYTE WRITES8, both cursor stores1, notification1 and TX CONSUMED1.
TX tail/head are0x48, RX tail/head0x290 and RX AVAILABLE0. Buffer guards
and AP intent-retired status are1, with one sample. The private buffer
remains reserved; no new intent or outgoing QRTR was sent. These values
confirm fixed RX_DONE consumption and release of the validated648-byte RX
queue. They do not independently reveal peer internal retirement semantics.
Original photo SHA-256:
`421fc9202ce7e51e9d564016525722c85d285423769ce334a5bdfeb0b48f6335`.

The first outgoing QRTR HELLO candidate is prepared and frozen, not sent.
It consumes an actual successful completion checkpoint and keeps the prior
private buffer/exchange. It selects peer intent0 once only after its retained
current advertisement proves capacity>=52 and unique IDs0..29; no individual
size is assumed from the earlier photo. It stages72 constant wire bytes at
TX72: GLINK DATA9/localCID1/peer-intent0, chunk52/left0, QRTR v1 HELLO from
explicit probe node1 to observed modem node0, control ports,20-byte body
command2 and zero remainder/padding. It double-verifies all TX bytes and
rechecks ready/state/private guards before one head144 store and sixth GLINK
notification. No additional RX-tail store occurs; RX tail656 stays unchanged.

The read-only reply snapshot double-copies at most512 bytes. Success requires
TXtail144 and one exact RX_DONE6 or RX_DONE-with-reuse14/remoteCID2/intent0 in
a completely parsed current snapshot. New intent advertisements, requests,
read notifications and signals are diagnostics only. A reuse reply does not
authorize reuse; no new receive intent, request response, service/WLAN QMI,
DMA, retry or cleanup is added. Peer GLINK completion does not alone establish
QRTR application acceptance or service discovery.

All132 dependency/HELLO host tests pass, including exact fixed wire and native
write bounds, missing/duplicate/undersized peer intent rejection, partial
publication, terminal success/failure, late readiness loss, unstable snapshots,
bounded control parsing and no reads of released RX bytes. The20 source/mode
tests,18 common gates and both QEMU loader checks pass (11 task copies,
14 canaries, before first kernel instruction; not native protocol evidence).
The base SDK, five RW/512 unique SMEM frames and17 prior images are unchanged.
The41205760-byte image is frozen under
`modem-qrtr-hello-probe/prepared-modem-qrtr-hello-probe`, SHA-256
`faefb744b9db49b6d91114c0c8540741da5c8683f2c03e491052ffe24711e392`;
PD ELF SHA-256
`b84cbef56dd848b8594b05f15d7e23d9dd7d39990ba37ce6973b178f990bf9c3`.
Native outgoing HELLO/peer completion remains unobserved.

Fresh live ADB verified boot-complete LineageOS on the same Sunfish. All
165 source/32 private-loader/57 artifact fingerprints,17 earlier images and
the base SDK matched. Fastboot confirmed Sunfish and its256MiB download limit,
then accepted the temporary HELLO image on 2026-10-18 at15:09:39 PDT
(exit0; send1.081s, boot4.094s, total5.194s). No partition was flashed.
Native outgoing payload/peer-completion status remains pending; command
acceptance does not establish that the send path ran or a reply arrived.

The native photograph confirms `HELLO TRANSPORT PASS`, ERROR0, stage8.
Current peer intent0 capacity is0x80(128), reserved1;72 byte stores, one
head store and notification, TX CONSUMED1, DONE SEEN1 and DONE COMMAND0xe(14).
TX tail/head are0x90, RX tail0x290/head0x2a0 with16 bytes captured/available
and complete parsing. Guards pass. The two control records comprise the
verified RX_DONE-with-reuse14/remote2/intent0 and one receive request for
0x34(52) bytes; their order is not visible. The reuse response was observed
without reusing the intent or consuming these16 bytes. No new receive buffer,
service/WLAN QMI or host DMA followed. This confirms the fixed outgoing
HELLO's GLINK transport completion, not QRTR service discovery or application
acceptance. Original photo SHA-256:
`9e747bf14af2f93fecb727efa770b54cb36474b6cdc4aab4950894a4f02eda0f`.

The next-receive candidate is prepared and frozen, not sent. The actual
successful HELLO mints a non-copyable continuation containing the complete
stable16-byte control snapshot and the retained first payload. Both control
orders are accepted, with exactly completion14/remote2/peer-intent0 and
request7/remote2/52. Revalidate descriptor144/144/656/672, full TX, that prefix,
readiness/identities and the retained first buffer before a new promise.
Reserve a separate private64-byte buffer with32 guards and fresh local intent2;
never reuse local1 or peer0. Append24 fixed advertisement/request-ACK bytes
at TX144, double-verify all TX, store head168 once and issue the seventh
GLINK notification outside the lock. RX tail656 stays unchanged.

Double-copy at most80 new bytes at RX672 and accept one72-byte
DATA9/remote2/local-intent2 frame, chunk52/left0. Verify two volatile readbacks
of the52-byte private copy, both buffers/guards/zero remainders and final
live state/TX/prefix. Preserve any later unclassified suffix. Display the QRTR
header and all five body words as observations; no packet identity, service
or endpoint authority is assumed. No further RX_DONE, receive consumption,
extra QRTR TX, intent reuse, service/WLAN QMI, host DMA, retries or cleanup.
Both private buffers and the peer-intent lease remain held through restart.

All144 dependency/next-receive tests,20 source/entry/layout/mode tests and18
common build gates pass. Tests cover both control orders, changed retained
first payload, damaged guards, partial publication/header, retired IDs,
fragmentation, late readiness loss, differing copies, suffix preservation
and exact native write bounds/terminal reuse. Both loader checks pass with
11 task copies and14 canaries, before the first kernel instruction. The
initial parallel handoff run hit the30-second emulator deadline inside the
task copy; the unchanged image passed when rerun alone. That failure log is
retained with the evidence. No native result is inferred from QEMU.
The same512 unique SMEM frames/five RW pages, base SDK and18 earlier images
are preserved. The two96-byte private buffer symbols are disjoint, RW/NX,
and contain the expected guard/zero data in the final ELF. The41218048-byte
image is frozen under `modem-qrtr-next-probe/prepared-modem-qrtr-next-probe`,
SHA-256 `484d560fe66e106ff0d9be0f78a31b83fbc6a82c0f495947f51f2b431d7463c2`;
PD ELF SHA-256
`5afcec230f2e6e8912a336c70af10c508f23da5866f7be8ca54ba670c038d59f`.
Native result is recorded below.

Live ADB verified boot-complete LineageOS on physical Sunfish serial
02281FQC200058; the separate Android emulator was not targeted. All169
source/32 private-loader/61 artifact fingerprints,18 earlier images, both
buffer reservations and base SDK matched. Fastboot confirmed Sunfish and
the256MiB download limit, then accepted the temporary next-receive image
on 2026-10-18 at15:32:46 PDT (exit0; send1.071s, boot4.138s, total5.228s).
No partition was flashed. The later user photo confirms `NEXT PACKET COPY
PASS`, error0/stage9, TXhead0xa8, RXhead0x2f0, available0x60. DATA9/remote2/
localintent2 contains52 bytes with no remainder. Both guarded buffers and
QRTR v1 shape pass. The raw body words are `[4,0x42,0xb401,0,3]`: a NEW_SERVER
announcement for service0x42, opaque instance word0xb401, node0, port3. RXtail656
and the preceding16 controls remain unchanged. Eight bytes after the72-byte
frame remain unclassified, so a later step must validate their actual type
before responding. This is announcement evidence only, with no authority
for service/WLAN QMI or host DMA. The image, photo and native observation
are saved with the frozen candidate.

The user supplied a second photo on 2026-10-21 showing the same exact
`NEXT PACKET COPY PASS` screen. The raw fifth body word is `3` (not `0xb3`);
the corrected observation is saved as
`.cache/pixel-sunfish/modem-qrtr-next-probe-20261021/native-observation.json`
and is pinned to candidate image SHA-256
`890f9e37d7edca8132fd9e277a5185be5b8aeddde6d8c68648b263bcc0187305` and the
new photo hash. It reconfirms service0x42/instance-word0xb401/node0/port3,
not the WLFW service ID0x45. This copy is still diagnostic evidence only;
the packet and trailing bytes remain unconsumed.




### Bounded passive QRTR service inventory (2026-10-18)

The next temporary image continues from the actual owned next-receive
checkpoint and its double-copied 72-byte frame. It accepts strict QRTR v1
NEW_SERVER announcements only. The initial eight-byte suffix is unclassified;
each new buffer promise requires a live, stable, sole request7/remote2/52.
For each validated frame it sends fixed RX_DONE6, then a fresh INTENT and
positive request acknowledgement. It retains 32 distinct guarded private
buffers, using local intent IDs3..34 once each. Released RX bytes are never
read again. All retained payloads, guard bytes, full TX, live readiness,
item identities and monotonic cursors are rechecked around publication.

The inventory stops on service ID0x45 or after 32 additional packets (33
records including the first). This target ID follows the pinned WLFW source;
instance values remain opaque. Finding it does not establish protocol-version
compatibility, endpoint authority or functioning Wi-Fi. The final packet is
left uncompleted and unconsumed. The test adds no outgoing QRTR payload,
service/WLAN QMI, intent reuse, host DMA, retry or cleanup. Global time is
bounded to20 seconds, with separate stopped-clock/work limits. Maximum AP TX
head is1192, remote RX head3312, additional notifications64 and additional
SMEM writes1536 bytes; the full post-auth write ceiling is1740 bytes.

All159 host dependency/discovery tests,20 source/entry/layout/mode tests and
18 common build gates pass. Tests cover fresh IDs and distinct buffers,
limit/target stops, strict suffix validation, delayed requests and partial
DATA, stopped clocks, malformed frames, lost readiness, partial publication,
released-byte overwrites, exact native write bounds and terminal single-use.
Both serial QEMU loader checks pass with11 task copies and14 canaries,
before the first kernel instruction. QEMU is not native discovery evidence.
All34 private buffers are disjoint RW/NX with their initial guards and zero
payloads verified in the final ELF. The same512 unique SMEM frames/five RW
pages, base SDK and19 prior frozen images are preserved.

The 41230336-byte image is frozen at
`modem-qrtr-discovery-probe/prepared-modem-qrtr-discovery-probe`, SHA-256
`98807d5ecefc8266e427a4cc7d4fd31c346f6a724102d0defa4cdbf1a0d1b37a`; PD SHA-256
`5ad40f60e3692ede4a3008037edef6720d83855118277710e6d55eb28c6c619f`. The archive holds173 workspace sources,
32 private loader sources and64 artifact fingerprints. The layout seed was
used only to extract and pin all52 loader-plan fields; it was never wrapped
or sent. Native service-inventory results remain pending. Live targeted ADB
verified boot-complete LineageOS on physical Sunfish02281FQC200058. All173
workspace/32 private-loader/64 artifact fingerprints and19 prior images
matched before reboot. The ADB reboot reply timed out; one request was sent,
and the physical serial then appeared in Fastboot. Product and256MiB download
limit were verified, and Fastboot accepted the frozen temporary image on
2026-10-18 at15:56:10 PDT (exit0; send1.067s, boot4.142s, total5.231s).
No partition was flashed. Native discovery execution and result remain
unobserved; command acceptance alone does not establish service discovery.

The later user photo shows `DISCOVERY STOPPED`, error0x10 (Timeout), stage7.
It captured two strict NEW_SERVER records; latest service0x2b/instance0x1202/
node0/port1, with WLFW SEEN0. One new packet was copied, two DONEs consumed,
one fresh intent advertised and three notifications sent. TXtail/head0xd0
and RXtail/head0x338 show an empty receive ring at the final sample. Both
old and pool guards pass. The program stopped waiting for another intent
request; it did not advertise a second fresh buffer. The photo does not
establish why no request arrived or whether more announcements were pending.
This is not a global absence finding for WLFW. The photo and complete native
observation are saved with the frozen candidate. No service QMI or host DMA.

### Optional-request receive credits (2026-10-18)

The empty-ring timeout showed the previous probe could wait without offering
another buffer. The revision accepts either a stable empty receive ring or
one exact request7/remote2/52 after the preceding DATA completion is consumed.
An empty ring permits one fresh16-byte INTENT only, with no request ACK and
no RX tail store. A validated request permits INTENT+ACK24 and consumption
of exactly8 request bytes. The reference driver's
[qcom_glink_announce_create](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/rpmsg/qcom_glink_native.c)
also advertises receive intents without waiting for an intent request;
this does not establish the cause of the modem's observed pause.

Closed internal plans derive dynamic TX/RX positions from already checked
AP cursors, with fixed unique local IDs3..34. No incoming value supplies a
write address. Changes observed during staging or after releasing a request
stop before publishing the next credit head. Released bytes are not reread,
and all private buffers remain leased through restart on every outcome.
The same32 new packets/33 records,20-second budget, TX1192/RX3312 bounds,
64 additional notifications and1536 additional SMEM-write ceiling apply.
The final DATA remains uncompleted. A bounded empty data wait or packet
limit does not establish global absence of WLFW. No outgoing service QMI,
extra QRTR beyond the prerequisite HELLO, intent reuse, host DMA, retry or
cleanup is added; service-ID discovery is not endpoint/version authority.

All170 dependency/credit host tests,20 source/layout/mode tests and18 common
build gates pass. New checks cover the native empty-ring case, optional
ACKs, mixed requests, late requests both during staging and after tail
publication, maximum cursors/write counts, exact native aperture, empty
DATA timeout and terminal reuse. Both serial loader checks pass with11 task
copies and14 canaries before the first kernel instruction. These emulator
checks do not establish a native service result. All34 disjoint guarded
buffers remain RW/NX, and the same512 unique SMEM frames/five RW pages,
base SDK and20 prior frozen images are unchanged.

The 41230336-byte temporary image is frozen under
`modem-qrtr-credit-probe/prepared-modem-qrtr-credit-probe`, SHA-256
`d19796b7785210117da8b7c6f1cb67baad74a7fc4d96fe6eee07cd9d25396942`; PD SHA-256
`85435bc8f122dc6918dcf57bf719be69eb5683004c37fbcd26b16b87ef814920`. It includes176 workspace-source,
32 private-loader and67 artifact fingerprints. The extraction-only layout
seed was not wrapped or sent. Live targeted ADB verified boot-complete LineageOS on physical serial
02281FQC200058 after the Pixel reappeared. All176 workspace-source/32 private
loader/67 artifact fingerprints,20 prior images and base SDK matched. One
ADB reboot request timed out in its reply; the physical serial then appeared
in Fastboot. Product and256MiB download limit were verified, and Fastboot
accepted this exact frozen image on2026-10-18 at16:18:05 PDT (exit0;
send1.073s, boot4.085s, total5.175s). No partition was flashed. Native credit
publication, further packets and service discovery remain unobserved;
command acceptance does not establish those results.

The later HEIC photo shows `QRTR CREDIT INVENTORY / DISCOVERY STOPPED`,
error0x10 (Timeout), stage8. Records2, latest service0x2b/instance0x1202/
node0/port1, WLFW SEEN0. TXtail/head0xe0 and RXtail/head0x338 are empty.
One new packet was copied, two DONEs consumed, two fresh intents advertised,
and one request acknowledged; both guard checks pass. The frozen path thus
advertised fresh intent4 at TX208 with16 bytes and consumed that offer,
without an unsolicited ACK or RX tail store. It then exhausted the DATA
wait's time/work budget. The exact elapsed time was not displayed. Four
additional notifications and84 SMEM-write bytes follow from the frozen path,
not direct screen rows. This rules out the earlier mandatory-request wait
as the only observed limitation; it does not diagnose why no DATA arrived
or prove global WLFW absence. The HEIC, inspection JPEG and observation JSON
are retained with the frozen candidate. No service QMI, DMA or reuse.

### Measured passive DATA wait (2026-10-18)

The consumed-credit photo does not show whether the earlier DATA wait ended
at its sample cap or its global time budget. The next probe measures a longer
observation window using the same optional-request credit policy. It permits
up to60 seconds per pending DATA packet,8192 data samples, and65 seconds
globally within the discovery phase. Other wait/sample limits and independent
stopped-counter work bounds remain. These are observation budgets, not a
claim about the modem's required startup time or a prediction that WLFW will
appear. The screen shows last-poll elapsed milliseconds since discovery
started, DATA polls, counter frequency and last successfully observed
slave-kernel status. The last status is not a fresh value after a failed
readiness check.

There is no added packet or write authority. A silent pending credit is not
reoffered or reused; timeout does not consume receive data or release any
buffer. The same32 new packets/33 records, TX1192/RX3312,64 additional
notifications and1536 additional SMEM-write ceilings apply, with all34
buffers retained and validated. Malformed traffic or lost readiness still
stops immediately. No new QRTR payload, service QMI, DMA, retry or cleanup.
A timeout remains a bounded sample, not a global absence result.

All176 host dependency/wait tests,20 source/layout/mode tests and18 common
build gates pass. New cases contrast a packet delayed25 seconds under the
old and measured budgets, bound an empty wait at60 seconds, enforce the
separate65-second budget across packets, stop a stalled counter, reject
late malformed data/readiness loss, and preserve the native write aperture.
Both serial loader checks pass with11 task copies and14 canaries before the
first kernel instruction. No native result is inferred from QEMU. All34
buffers are disjoint RW/NX with initial guards and zero data verified;
512 unique SMEM pages/five RW pages, the base SDK and21 prior frozen images
are unchanged.

The 41230336-byte image is frozen under
`modem-qrtr-wait-probe/prepared-modem-qrtr-wait-probe`, SHA-256
`064949cba5f45aac1a2b4f2b91174e233d7c4d5bafd6a734c7e4e8dbabad8cab`; PD SHA-256
`7cd75164b442af0d71b6cb5615dc11e85087c8a6084ea07ff834f5849d1cc6b4`. The archive holds179 workspace sources,
32 private-loader sources and71 artifact fingerprints, including the prior
HEIC, inspection JPEG and native observation. The layout seed is extraction
only and was never wrapped or sent. Live targeted ADB verified boot-complete LineageOS on physical serial
02281FQC200058 after it reappeared. All179 workspace-source/32 private-loader/
71 artifact fingerprints,21 prior images and the base SDK matched before
reboot. One ADB reboot reply timed out; Fastboot then enumerated that serial.
Product and256MiB download limit were verified, and Fastboot accepted the
frozen image on2026-10-18 at17:04:02 PDT (exit0; send1.076s, boot4.124s,
total5.220s). No partition was flashed. Native measured-wait execution,
elapsed values and any later packets remain unobserved; command acceptance
is not a native discovery result.

The measured-wait photograph shows `QRTR MEASURED WAIT / DISCOVERY STOPPED`,
error0x0e (Fatal), stage8 after38663ms,1440 DATA polls at19200000Hz.
Two records remain, latest service0x2b/instance0x1202/node0/port1; WLFW0.
TXtail/head0xe0 and RXtail/head0x338 are empty. One new copy, two DONEs,
two fresh intents and one request ACK; both buffer guards pass. The code
stopped when readiness validation saw the modem fatal-status flag. This
is not a timeout and does not identify the crash cause. `LAST SLAVE KERNEL`
was not rendered because its label exceeds the display's15-character limit;
no raw slave-kernel value is inferred from the blank row. No service QMI,
DMA, cleanup or reset followed the terminal guard. HEIC, inspection JPEG,
observation JSON and their hashes are saved beside the frozen image.

### Read-only modem failure reason (2026-10-18)

The measured-wait photo ends on Fatal0x0e after38.663seconds rather than
the observation timeout. The next diagnostic repeats that measured-wait
protocol unchanged, and captures global SMEM item421 before any secure call
and again after the terminal discovery result. The captured live DTS assigns
`qcom,smem-id=421` to the PAS4 modem. Its `qcom,pil-tz-generic` driver trace,
`drivers/soc/qcom/subsys-pil-tz.c` at kernel base28f9290, reads that item with
`QCOM_SMEM_HOST_ANY`; the SMEM v12 implementation routes that lookup to the
global partition, host pair65534/65534. The DTS and driver/SMEM references
and their hashes are archived; this is a source trace, not exact running
kernel-binary provenance.

A separate read-only decoder checks the global header/table, unique route,
partition bounds and nonoverlap, both allocation lists and padding, duplicate
item rejection, at most4096 allocation steps and item size at most4096bytes.
It captures at most256bytes twice, requires matching copies and directory
metadata, and retries only instability at most3times. A missing item is
reported explicitly by ITEM OFFSET=FFFFFFFF/ITEM BYTES=0, not fabricated text.
Baseline errors do not grant write authority or skip existing transport guards.
The current raw SMP2P status is sampled separately without requiring ready.

The final screen shows discovery error/last-poll ms, current raw status and
its read error, baseline/reason errors, bounded-prefix difference, item
identity/offset/size, text length and NUL/capping flags. All256 raw prefix
bytes are retained privately for comparison; display text preserves positions
but converts ASCII letters to uppercase and all punctuation/control/non-ASCII
bytes to spaces. Eleven24-character rows fit256bytes. PREFIX CHANGED0 means
that captured prefix is unchanged, not a new crash attribution. Unknown
comparisons show FFFFFFFF. Text is remote diagnostic data, never instructions.
The overlong earlier LAST SLAVE KERNEL label is shortened to LAST SLAVE.

The protocol capability is already terminal and all34 buffer leases remain
held when the extra snapshots run. These reads add only existing TCSR lock3
acquire/release operations, private stack copies and display output. They do
not clear the failure reason, consume more RX, send any additional packet,
change an index/doorbell, release a buffer, invoke QMI/DMA, reset or clean up.
The existing measured observation and packet/write ceilings remain unchanged.

All187 dependency/crash host tests,20 source/layout/mode tests and18 common
build gates pass. The new cases cover global/private route confusion,
malformed/overlapping/duplicate allocations, cached and uncached payloads,
short and unterminated/truncated text, metadata/payload instability, stale
baseline comparison, sanitized text, and read-only capture after a real
synthetic Fatal stop with no protocol continuation. Both serial QEMU loader
checks pass with11 task copies and14 canaries before the first kernel
instruction; QEMU is not native modem evidence. All34 buffers remain RW/NX,
guarded and disjoint; all512 unique SMEM mappings/five RW pages, the base SDK
and22 earlier frozen images are unchanged.

The 41234432-byte temporary image is frozen under
`modem-crash-probe/prepared-modem-crash-probe`, SHA-256
`b3301d7cddc154c77b126ab2b0b89570b79352cca410e36aec6d0ac1c880efda`; PD SHA-256
`13787efef703fa2159be4a0404415c6f3cb958ba5f5ebfbf90af7466c7126edb`. The archive holds184 workspace sources,
32 private-loader sources and79 artifact fingerprints, including the measured
fatal photo/observation and four reference-source artifacts. The layout seed
was extraction only, never wrapped or sent. Native reason results are pending.

The physical Pixel was subsequently verified boot-complete in LineageOS,
serial02281FQC200058, product sunfish, slot_a. All184 workspace/archived
sources,32 private-loader sources,79 artifacts,22 previous images and base
SDK hashes matched again. The ADB reboot reply timed out, but targeted
Fastboot then confirmed sunfish and max-download-size0x10000000. One
`fastboot boot` was accepted at 2026-10-18 17:23:04 PDT: send1.216s, boot4.071s,
total5.316s, exit0. The41,234,432byte image matched its frozen hash. No
partition was flashed. Command acceptance does not prove native execution;
reason/status readout remains pending and all positive native result fields
remain unknown until the screen is captured.

Subsequent driver work still needs voltage/clock transactions beyond these
restricted resource experiments, preservation of existing wake/sleep votes,
verified rail/clock dependency and rollback behavior, and the modem bus/PAS
startup path. The earlier resource-only readouts do not establish firmware
execution or SMMU/DMA containment. The later AUTH return still needs independent
modem readiness and transport evidence.

## Read-only device inventory (2026-10-09, updated 2026-10-10)

ADB identified the attached device as `sunfish`, running LineageOS
`23.2-20261001-NIGHTLY-sunfish` (Android 16) over an Android 13 vendor image
and a `4.14.357-openela` kernel. Wi-Fi was connected, `wlan0` was up, and it
had IPv4 and IPv6 addresses. The kernel reported the `wlan` module's loaded
`coresize` as 9,105,408 bytes, and the ICNSS platform device was bound to
`/sys/bus/platform/drivers/icnss`.

The vendor firmware tree lists `wlanmdsp.mbn`, `bdwlan-sunfish.bin`, and
`bdwlan-sunfish-EVT1.0.bin`; Android shell access did not permit reading these
files. No firmware blobs were copied. Google’s Sunfish configuration selects
the `qcwcn` WLAN device and `qca_cld3` driver, and Google’s Pixel security
target identifies WCN3998 for Pixel 4a.

The running kernel release suffix `g28f9290ae067` matches LineageOS kernel
commit [`28f9290ae067d3e7857126eb6312720dbe9c7c80`](https://github.com/LineageOS/android_kernel_google_msm-4.14/commit/28f9290ae067d3e7857126eb6312720dbe9c7c80).
That source snapshot includes ICNSS and a QCACLD tree at
`drivers/staging/qcacld-3.0`. Its Sunfish build config also names an external
input at `private/msm-google-modules/wlan/qcacld-3.0`, which is absent from the
public Lineage kernel checkout. A fresh root-enabled, read-only ADB check found
`/vendor/lib/modules/wlan.ko` (16,982,560 bytes), matching the already captured
`.cache/pixel-sunfish/wlan.ko` by SHA-256:
`9d7e738f1f8009059c6179c463700c536dcf224997376830b16534385290f8aa`. Its ELF
Build ID is `a7de3a571e18d97196f7763a71f721a054ed7a2f`, and its `vermagic`
matches the running kernel. Embedded source paths confirm QCACLD 3.0 and
qca-wifi-host-cmn. The loaded module has no `version` or `srcversion` metadata;
the binary does not identify its QCACLD source commit. The 16,982,560-byte
file differs from its 9,105,408-byte in-memory `coresize`, as expected for an
ELF with non-loaded sections. This confirms the exact driver binary but does
not prove a public source tree is byte-for-byte the source used to build it.

Google's Android 13 `sunfish-kernel` tag `android-13.0.0_r68` publishes a `wlan.ko`, but its
SHA-256 (`a21f6a5724a0a12a0a9d8becef1876ebbe8b154b2bbcde2eec6843439a41a50f`)
does not match the Lineage module. I also compared the captured module's Git
blob identity with the `wlan.ko` in all 82 published Android 13 Sunfish kernel prebuilt tags; none
matched. The checked-out Lineage device tree's `proprietary-files-vendor.txt`
lists WLAN firmware but not `wlan.ko`, and the public TheMuppets Sunfish
vendor tree likewise contains the firmware but no module. The kernel build
config names an external module source directory at
`private/msm-google-modules/wlan/qcacld-3.0`, while the public Lineage kernel
checkout does not contain that private directory. The captured module's Git
blob SHA-1 is `1b79436813d05bb471e709548febe63fc82e16df`. Google's Android 13
kernel manifest names the QCACLD project at that path and defaults it to a
branch, not a commit pinned in the manifest. Google publishes a Sunfish-
specific QCACLD branch, but its visible tip is not proof of the source
revision that produced this Lineage binary.

I also built the checked-in QCACLD tree against the running kernel config; it
completed, but produced a different ELF Build ID
(`cb3b797b912e2ba695c0e06010e5227302147033`). That build is only a reference:
build flags and source revisions can change IDs. The exact loaded module binary
is known, but its source commit cannot be recovered from the available module
metadata or the checked public release artifacts. Treat the matched Linux
source as a behavioral reference; exact binary-source attribution is not a
prerequisite for designing the isolated native driver boundary.

On 2026-10-11, root-enabled ADB read-only inspection recorded sizes and
SHA-256 identities for `wlanmdsp.mbn`, both Sunfish board-data variants, and
the active `WCNSS_qcom_cfg.ini`. The hashes are stored in
`platforms/sunfish/wlan-resources.json`; no firmware contents were copied.
This improves reproducibility when comparing a pinned vendor release, but does
not authenticate the files or establish which board-data variant the driver
will select at runtime. ADB was returned to shell UID 2000 after collection.
Android's read-only `dumpsys wifi` chipset report identifies the running WLAN
firmware as `3.2.2.9.558.0`, hardware version `0x40090000`, and driver
`5.2.03.9H`. This helps narrow compatibility matching, but does not
cryptographically bind that report to the local file hashes or reveal the
selected board-data file.
The reported host-driver version matches `QWLAN_VERSIONSTR` in the matched
Lineage source snapshot's `drivers/staging/qcacld-3.0/core/mac/inc/qwlan_version.h`.

## QCACLD WMI service-ready model (2026-10-11)

The reboot log confirms that WCN3998 reaches firmware service-ready, but the
normal kernel log does not expose the individual WMI host-memory requests.
`target_if_debug` details are compiled at DEBUG while the module's default
`TARGET_IF` trace level is INFO. An initial vendor trace-level request was
rejected (`attr Module ID failed`). A corrected request with a plain nested
attribute was accepted by `iw` (exit status 0, with no parser error). After a
Wi-Fi off/on cycle, the interface reconnected, but this did not reload the
firmware; no service-ready event or memory-request detail appeared. No change
to the phone's saved Wi-Fi configuration was made.

The pinned QCACLD source makes the next offline step precise. Its
`wmi_service_ready_event_fixed_param` is 132 bytes including the TLV header
(128 bytes of TLV value); the `num_mem_reqs` word is at offset 72 within the
TLV payload. The HAL-capabilities TLV is 40 bytes total (36-byte value), the
service bitmap is 32 words, and each `wlan_host_mem_req` is 20 bytes including
its TLV header (16 bytes of request fields). The service-ready event schema
orders the fixed event, HAL register capabilities, service bitmap, and a
variable array-struct of memory requests. The new host-testable
`rust/src/qcacld_wmi.rs` models this exact QCACLD framing separately from the
existing ath10k WMI model. It borrows the incoming bytes and bounds request
count at the source's `WMI_MAX_MEM_REQS`; it never derives DMA addresses,
allocates memory, or starts firmware.

The QCACLD parser still has synthetic packet tests only: no raw service-ready
TLV was captured from the Pixel. Separately, the full INFO diagnostic module
reported a valid zero request count from the live service-ready event. That is
enough to conclude the current startup requested no dynamic WMI chunks, but it
does not test the Rust parser against captured bytes. If a future firmware
reports a nonzero count, capture the raw TLV before defining allocation policy.
The later byte-for-byte module reproduction below ties the diagnostic behavior
to the integrated local source tree; it does not provide a vendor build
manifest or identify a unique upstream commit history.

The parser now has an explicit standalone test target in
`tests/sunfish_qcacld_wmi_test.rs` and is included in the shared
`test-modem-qmi.py` host and AArch64 no-std run. The truncation test rejects
every strict prefix of a complete service-ready packet carrying one memory
request. A narrow profile token accepts only firmware build `0x3226822e` with
zero declared requests; another build or a nonempty request list stops without
producing a DMA plan. The token borrows the parsed event and carries no hardware
authority. These synthetic tests still do not replace a captured service-ready
packet or provide an allocation policy for nonzero requests.

A further read-only rooted-ADB check on 2026-10-11 confirmed the live sysfs
firmware report (`3.2.2.9.558.0`, hardware `0x40090000`, board version 18) and
driver version (`5.2.03.9H`). The kernel ring held 12,430 lines but began at
about 32,601 seconds of uptime, after WLAN firmware startup; searching it found
runtime WMI power-management messages but no service-ready event or host-memory
request records. Debugfs was not mounted, and tracefs exposed cfg80211 events
but no WLAN-private event group. Thus the current running system cannot provide
the missing startup request list from its retained logs. No Wi-Fi state was
changed during that first inspection, and ADB was returned to shell UID 2000
afterward.

After a user-requested Wi-Fi off/on cycle, a short rooted live-log check on
2026-10-11 showed Android destroying and recreating the WLAN virtual interface
and then reconnecting, but did not show firmware reload, WMI service-ready, or
host-memory-request events. The interface ended `up` with carrier present. This
confirms that a settings-level Wi-Fi toggle is insufficient to capture the
firmware startup request list; obtaining it would require a firmware/driver
restart path or instrumentation during a full WLAN firmware boot. ADB was
returned to shell UID 2000 after capture.

A full LineageOS reboot on 2026-10-11 retained the WLAN startup handshake in
the kernel ring. ICNSS and the WLAN driver probed, QCACLD reported `Target
Ready`, and `wma_rx_service_ready_event` logged firmware build `3226822e` and
board ID 0 / board version `18 0 0 0 0`. The targeted log search found no
per-request allocation lines (`num_mem_reqs`, request IDs, sizes, or chunk
results); ADB was returned to shell UID 2000 immediately afterward. This
confirms the vendor firmware reached the WMI service-ready phase under
LineageOS, but it does not provide the actual host-memory request table.
The nearby 5 MiB `memshare_alloc` record is from the separate MPSS heap-memory
sharing QMI service (`0x34`, modem client 1), distinct from ICNSS's WLAN MSA
and the WMI host-memory request list. The 2026-10-20 source trace and working
Lineage capture are recorded in `pixel-modem-lifecycle-design.md`.

On a later 2026-10-11 check, the corrected trace command was followed by a
Wi-Fi off/on cycle. Unlike an interface-only toggle, this time the kernel ring
showed another `Target Ready` and WMA service-ready event, including the same
firmware build and board report. Even with the requested `TARGET_IF` debug
mask, the ring contained no `idx … req … num_units … unit size …` lines from
the pinned source's allocation path, and no raw service-ready event payload.
This narrows the issue to trace visibility, source/binary differences, or
logging behavior; it does not establish that the firmware requested zero
memory. Wi-Fi reconnected to the existing network, and ADB was returned to
shell UID 2000. No further phone action is needed for the offline work.

The exact captured `wlan.ko` contains the `idx %d req %d … actual units %d`
format string, so the allocation diagnostic exists in the binary. The phone's
tracefs exposes only the `nop` tracer, has no `kprobe_events`/`dynamic_events`,
and offers only generic cfg80211 and SMMU tracepoints; `/sys/kernel/debug` is
not mounted or present. There is therefore no available read-only runtime hook
to inspect the WMI event buffer or confirm that the trace mask took effect.

The read-only build identity check found kernel
`4.14.357-openela-g28f9290ae067` on the phone, matching the local kernel
repository's base commit `28f9290ae067d3e7857126eb6312720dbe9c7c80`. The
Lineage build config nevertheless compiles WLAN from the separate
`private/msm-google-modules/wlan/qcacld-3.0` project, which is absent from the
kernel checkout. I fetched the public Android 13 QCACLD and qca-wifi-host-cmn
branches as source references; `init_cmd_api.c`'s service-ready handler is
byte-for-byte identical to the integrated copy used for the compile check.
At that stage, this supported the code path without proving how the installed
module was built. The complete module reproduction recorded below later
established that the integrated local source tree generates the installed
bytes.

For a future controlled diagnostic build, the repository now has
[`patches/sunfish-wlan-wmi-host-mem-req-log.patch`](../patches/sunfish-wlan-wmi-host-mem-req-log.patch).
It adds INFO-level output for the count and each request's ID, unit count,
unit-info count, and unit size, without printing pointers or DMA addresses or
changing allocation behavior. `patch --dry-run` applies cleanly to the pinned
Lineage source. I compiled the patched translation unit with Clang in an
isolated Fedora container using the captured Pixel config as a starting point;
the temporary config disabled module-signature and fs-verity host helpers so
the old kernel could prepare against Fedora 41's OpenSSL. That first
translation-unit-only `wlan.ko` was not a complete or loadable driver build;
the patch was reverted after the compile. It was later superseded by the full
module build and controlled in-memory test below.

The checked-in QCACLD/qca-wifi-host-cmn source has a distinct path for those
requests: `init_deinit_handle_host_mem_req()` extracts them from the service-
ready event, rejects counts above `MAX_MEM_CHUNKS`, resolves peer-count-based
unit counts, rounds unit sizes to four bytes, and allocates DMA chunks before
preparing WMI INIT. It also treats contiguous-memory requests differently and
can split or reduce allocations according to its policy. These are Linux
driver allocations, not authority or a plan NexatrodeOS can copy directly.
The logged WMA build/board lines are only a summary; the kernel ring does not
expose the raw event payload. A later complete module rebuild reproduced the
installed module byte-for-byte before adding diagnostics, tying the relevant
compiled behavior to this local source tree. This still cannot distinguish
different source histories that produce identical bytes. In particular, the
host-only `ath10k_wmi.rs` parser models upstream ath10k's WMI-TLV layout and
must not be used to interpret the Pixel's QCACLD event as if it were an ath10k
capture.

On 2026-10-12, I built the complete QCACLD `wlan.ko` from the matching local
Lineage kernel tree with the INFO-level host-memory-request patch applied. The
unmodified cached module had the same SHA-256 as the module captured from the
Pixel (`9d7e738f…5290f8aa`), establishing that this build tree and its source
produce the installed base module byte-for-byte. The patched module was linked
as a full module, stripped of debug sections, and reports the exact running
kernel `vermagic` (`4.14.357-openela-g28f9290ae067`). Its 395 imported-symbol
CRC entries are identical to the captured Lineage module. The diagnostic
strings for the request count and bounded per-request tuple are present. The
source patch was reverted after the build; no module was loaded on the phone,
and no partition was changed. The artifact is
`.cache/sunfish-kernel-diagnostic/wlan-hostmem-log-diagnostic-matched.stripped.ko`
with SHA-256 `f3e8215592e83b426547958bdca62dee0c4cf3f5070b66f290f5bc418d0725db`.
The build temporarily disabled host-side module-signature and fs-verity
certificate helpers to work around Fedora 41's removed OpenSSL engine header;
this did not alter the recorded module release or imported-symbol CRC table.
This verifies module compatibility metadata, not successful loading or
firmware behavior.

The temporary in-memory test then loaded that diagnostic module on 2026-10-12,
restarted WLAN firmware, and captured `WMI service-ready host-mem request
count=0`. The same startup logged Target Ready, firmware build `3226822e`, and
board version 18. The source TLV extractor writes its output count from the
service-ready fixed parameter; it logs an error if that count exceeds the
parsed request-array length. Neither the `Invalid num_mem_reqs` error nor the
missing-fixed-parameter diagnostic appeared in the captured startup, so the
zero count is consistent with a valid event containing no dynamic WMI host
memory chunks. This is the observed firmware request count, not a claim that
all WLAN memory requirements are zero: the separate 2 MiB MSA reservation and
WLFW QMI-returned regions remain essential. The original
vendor module was reloaded afterward, Wi-Fi reconnected to `Vanilla`, ADB was
returned to shell UID 2000, and the staged diagnostic file was removed. No
partition or Lineage data was altered.

The installed `WCNSS_qcom_cfg.ini` is an exact SHA-256 match for the public
LineageOS Sunfish device-tree file at commit
`52074bf3c0b66efcba89f557e605535feb29aaab`. I also streamed pinned public
blobs through SHA-256 and found exact matches for `wlanmdsp.mbn` in the
community `firmware-google-sunfish` repository at commit
`fe63598e1bd9884bd68ba89e4670dae4d70b1e3e`, and for
`bdwlan-sunfish.bin` in `firmware-qcom-sm7150-ath10k` at commit
`792c716c85c06861c4d17bce5ec21fba7f0f7de6`. No firmware blob was saved
locally. These repositories are public community rehosts, not authenticated
Google release artifacts, and GitHub reports no declared license for either
repository. The EVT1.0 board-data variant remains unmatched. Exact hashes
establish byte identity with those pinned rehosts only; they do not prove the
installed release, runtime board-data selection, or permission to redistribute
or load a blob.

Google's driver-binaries page lists the Pixel 4a S1 vendor archive and publishes
SHA-256 `f82c4214524f0d1f85e9ad7b2a23ddaed0b402cabc5b01533a61dcd1adb7d80f`.
The downloaded archive stream matched that checksum. It is a self-extracting
shell package; its displayed Google Developer Software License limits use to
the authorized Android-enabled device for non-commercial purposes and restricts
incorporating the software into other software or devices. I did not extract
the archive or compare its individual firmware files. We should not assume this
license permits NexatrodeOS to load or package the blobs without explicit
permission. The firmware-use gate therefore remains closed alongside the SMMU
and device-ownership gates.

## What implementation requires

The Wi-Fi path is more than a packet interface. Sunfish describes a Qualcomm
ICNSS platform device, WLAN MSA memory, SMP2P signaling, interrupts, regulators,
and SMMU mappings. Qualcomm’s Linux ICNSS driver uses Linux platform, regulator,
interrupt, DMA, QMI, and firmware-service APIs. The `qca_cld3` WLAN driver then
provides the WLAN control and data path. A compatible supplicant/authentication
service and integration with our existing DHCP, DNS, TCP, TLS, and Messages
services are also needed.

### Upstream ath10k is a separate Linux reference path

The checked-in mainline-style Sunfish device tree provides a second driver lead:
`sm7150.dtsi` describes the WLAN block as `qcom,wcn3990-wifi`, with MMIO at
`0x18800000`, a reserved MSA pool of 2 MiB, and the same SMMU SID/mask
(`0x240/0x1`) seen in the runtime route audit. The ath10k SNOC driver itself
requests a 1 MiB MSA allocation from that pool. The Sunfish overlay enables the
device and sets the calibration variant to `google_sunfish`; the current
sm7150-mainline tree at pinned commit `fd78d17954a86ea6ceb0c535e618ea129e09f8dd`
contains the same configuration. Upstream Linux has an `ath10k_snoc` driver
for the WCN3990 SNOC interface; its source identifies that hardware family and
uses a 35-bit DMA mask. This makes upstream ath10k worth evaluating as a Linux
reference or driver-VM candidate. It does not mean the driver can run inside a
Microkit protection domain, and it does not establish that the WCN3998 in this
phone is compatible with every WCN3990 firmware build.

The host-only port map and first WCN3990 CE descriptor/ring model are in
[`sunfish-ath10k-port-plan.md`](sunfish-ath10k-port-plan.md) and
[`rust/src/ath10k_ce.rs`](../rust/src/ath10k_ce.rs). They encode no device
authority and do not enable firmware or DMA.

The current Lineage runtime instead exposes downstream `qcom,icnss`, the loaded
QCACLD module, and an active 2 MiB MSA pool at a different physical address.
The upstream DTS and runtime DT therefore describe different software
bindings and MSA layouts over the related hardware resources. A controlled
Linux reference boot would be needed to establish whether upstream ath10k
actually initializes this phone, and it must remain a separate, reversible
test. Merely enabling the node in a DTS is not evidence of a successful Sunfish
radio boot.

The ath10k SNOC source also makes clear why porting it to seL4 is not just a
register-access translation: it consumes Linux clock, regulator, power-sequence,
remoteproc, IOMMU, IRQ, reserved-memory, and DMA APIs. A native Microkit path
would need explicit managers or narrowly scoped equivalents for those shared
services, plus an sDDF packet interface. A Linux driver VM could reuse ath10k,
but only after the device can be safely assigned with exclusive MMIO/IRQ/DMA
ownership; the current evidence does not establish that hypervisor boundary.

A read-only check of the connected LineageOS system found no `/dev/kvm`, and
the checked-in Sunfish kernel config does not enable `CONFIG_VIRTUALIZATION` or
`CONFIG_KVM`. That does not prove EL2 is unavailable to a separately booted
Microkit/VMM image, but it means there is no current Android-side KVM facility
to prototype this driver-VM route. Treat the VM as a future architecture option;
the immediate engineering path remains explicit native Microkit service and
DMA ownership work.

The `linux-firmware` tree publishes WCN3990 `board-2.bin`, API-5
`firmware-5.bin` feature metadata, and `wlanmdsp.mbn` under the
[Qualcomm Atheros firmware license](https://kernel.googlesource.com/pub/scm/linux/kernel/git/firmware/linux-firmware/%2B/1221848f23764faab10ff63bc5d9ea5d5b39a506/LICENSES/LICENSE.QualcommAtheros_ath10k).
That license allows unmodified binary redistribution/use
solely with a Qualcomm Atheros chipset, requires retaining its notice, and
prohibits reverse engineering. It does not itself forbid a Linux reference test
on supported hardware, and it does not establish device compatibility. The
current bundle is a **no-go for Sunfish hardware testing**: the 60-byte
`firmware-5.bin` is feature metadata whose `NON_BMI` flag requires an
out-of-band firmware launch; the pinned ath10k/Sunfish path provides no such
Sunfish launcher; its `wlanmdsp.mbn` hash
(`92e1501254e6de78c0f2e2cf091507d488b608d07e53acd14813a82744823ec2`) differs
from the phone's; and upstream moved the base image under `qcom/sdm845` while
stating it is platform-specific to SDM845 and close relatives
([upstream commit](https://kernel.googlesource.com/pub/scm/linux/kernel/git/srini/linux-firmware/+/a0142c57045701b7557c3060af5c4246c420e4d8)).
No test has been skipped solely because of the license; the candidate is held
because the source, hash, and launch path do not match Sunfish. Reconsider a
separate, disposable Linux reference boot only after a source-backed
SM7150/Sunfish firmware and board-data match, an out-of-band WLFW launch path,
and the SMMU/MSA setup are established. The manifest and validator enforce the
no-go decision until those facts change.

The driver cannot be linked directly into a native Microkit protection domain:
it is a Linux kernel module and depends on Linux kernel interfaces. There are
two realistic routes:

1. Port the required ICNSS and WLAN driver functionality to seL4/Microkit.
   This preserves a native driver boundary but is a large, continuing porting
   effort and may be constrained by unavailable firmware protocols or blobs.
2. Run the existing Linux driver stack in an isolated Linux driver VM and
   expose a narrow packet/control interface to Microkit. This reuses more code,
   but requires a working Pixel EL2/VMM path, device assignment, and DMA/IOMMU
   isolation. The current Pixel port evidence records EL1 entry with the MMU
   disabled; EL2 access and a driver VM are not established.

## Firmware-start source trace

The exact Lineage ICNSS source shows that firmware startup is a service
protocol, not a single WLAN register write. When the WLFW QMI service arrives,
ICNSS powers the radio rails and reference clock, registers QMI indications,
sends the MSA memory description, transfers MSA ownership to WLAN/MSS VMIDs
through `hyp_assign_phys()`, and then sends MSA-ready and capability messages.
Its device-tree parsing also consumes the WLAN IRQs, MSA region, regulator
supplies, and SMMU IOVA ranges. These are the first pieces to model in a native
port; a Microkit driver would need a small, auditable QMI/transport layer and a
platform power/ownership service. The matching Linux `hyp_assign_phys()` path
packages the physical range and VMID permissions for Qualcomm SCM's
`MEM_PROT_ASSIGN` service (`SCM_SVC_MP`, function `0x16`); it is a secure
monitor-mediated ownership transition, not a normal seL4 page mapping.
The Linux helper marshals scatter/gather ranges plus aligned source-VMID and
destination-permission descriptors, flushes those descriptors from cache, and
submits them in batches to SCM. Its error contract is security-relevant:
`-EAGAIN` can be retried, but `-EADDRNOTAVAIL` means the memory may no longer be
usable and the host Linux kernel must stop accessing it. A Microkit service
therefore needs explicit ownership states and fail-closed error handling; a
thin SMC wrapper is not enough.

### WLFW control transport and first native software milestone

The transport underneath QMI is now traced in the matching Lineage source.
ICNSS looks up WLFW as QMI service `0x45`, version `1`, instance `0` over
QRTR. When QRTR reports a server, ICNSS connects to its discovered node and
port using `AF_QIPCRTR`; therefore a native client needs QRTR service discovery
and endpoint lifecycle handling before it can send WLFW messages.

On Sunfish, QRTR uses the modem GLINK edge in the runtime device tree:
`qcom,glink-label = "mpss"`, channel `IPCRTR`, remote processor PID 1, and the
SMEM transport. Its doorbell uses `apcs_glb` mailbox channel 12 and GIC SPI
449 (seL4 GIC INTID 481). GLINK’s SMEM backend uses shared item IDs 478
(32-byte descriptor), 479 (Linux allocates the 16 KiB TX FIFO), and 480 (RX
FIFO). The descriptor holds head/tail indices both processors update while
exchanging data. The captured live runtime FDT reserves SMEM at
`0x86000000`–`0x861fffff`, marked `no-map`. The matched Atoll source DTS has a
default `0x80900000` reservation, but the device's merged runtime tree relocates
it; diagnostics must use the exact live FDT rather than that source default.
While LineageOS was running, its modem reached `MPSS AFTER_POWERUP`, QRTR
reported modem readiness, and the `modem.IPCRTR` rpmsg endpoint was bound to
`qcom_smd_qrtr`; the runtime device tree reports `transport = "smem"` and
remote PID 1. A temporary read-only module then called the kernel's exported
`qcom_smem_get(1, item, &size)` for each item while Linux owned the transport.
It returned sizes 32, 16384, and 16384 bytes for items 478, 479, and 480. The
module read no payload bytes, was unloaded immediately, and its phone-side
file was removed. This confirms the Linux allocation route and sizes, but not
the item offsets, descriptor indices, FIFO contents, or a safe Microkit
handoff. The Linux backend will allocate or reuse those SMEM
items and then resets its RX-tail and TX-head indices to zero before starting
GLINK, even when the shared items already existed. Copying that initialization
in Microkit without agreement from the running modem could corrupt an active
ring. The descriptor's first four little-endian words are TX tail, TX head, RX
tail, and RX head; Linux produces into TX and consumes from RX. Words four
through seven are unused by this matched backend. Item 479 is the 16 KiB AP TX
FIFO; item 480 is acquired as the remote-produced RX FIFO.

The matched Linux integration provides a useful lifecycle reference: its GLINK
probe registers the SMEM edge after the modem subsystem's `AFTER_POWERUP`
notification and unregisters it after `AFTER_SHUTDOWN`. The native probe then
resets the AP-owned RX-tail and TX-head indices and sends VERSION. The protocol
VERSION exchange only negotiates version/features; it is not a remote-reset
acknowledgment and does not establish that either FIFO is empty. Microkit has
no modem SSR coordinator or reset/ready service today, so it cannot reproduce
that lifecycle contract yet. A `VERSION_ACK` from a running peer must not be
treated as permission to reset descriptor indices.

There is a separate prerequisite even for locating those GLINK items. The
matched `qcom_smem_get()` does not derive item addresses from a fixed offset:
it validates the SMEM header, then resolves either a global TOC entry or a
host-private partition/allocation entry. It takes TCSR hardware spinlock 3
while resolving the allocation. Atoll describes the SMEM range as `no-map`,
and wires this lock through the TCSR mutex block at `0x01f40000`. Thus a
read-only Microkit probe cannot safely assume that item IDs 478–480 are at
`SMEM base + constant`, or skip SMEM's directory and partition checks. A
future bounded reader must first define the memory mapping/cache attributes
and a compatible lock protocol, validate all header/table/partition ranges
against the 2 MiB reservation, and resolve the correct host/item entries.
Holding the allocator lock only protects metadata lookup; it does not make
the live GLINK descriptor or FIFOs quiescent and does not grant permission to
read or alter their indices. The current on-device probe therefore stops at
directory metadata and never reads the descriptor or FIFO contents.
The matched Qualcomm hwspinlock driver writes the Apps owner ID `1` to a
32-bit lock register and reads it back to acquire; unlock writes zero. For the
Atoll DTS tuple `syscon = <&tcsr_mutex_block 0 0x1000>`, lock 3 therefore maps
to `0x01f43000` (block base `0x01f40000` plus `3 × 0x1000`). This is a
coordination register, not passive memory: a live parser must acquire/release
it with bounded retry and guaranteed cleanup, and only after the boot-time
owner has authorized access to this shared lock bank.
An offline parser model in [`smem_model.py`](../platforms/sunfish/smem_model.py)
resolves synthetic v12 host-private uncached entries and can optionally check
the reverse cacheable-allocation list, with bounded table, partition, canary,
and allocation checks. Its tests use fabricated bytes only; it has no lock,
mapping, or device-read path.

A temporary directory-only Microkit probe is now built at
[`smem-directory-probe.system`](../platforms/sunfish/smem-directory-probe.system).
It maps the exact live-FDT reservation at `0x86000000` read-only, and grants a
single diagnostic PD read/write access to the TCSR lock-3 register at
`0x01f43000` solely to acquire/release the shared allocator lock. It validates
the v12 header and partition table and reports only the offsets/sizes of
items 478–480; it does not dereference the GLINK descriptor/FIFOs, write SMEM,
ring a mailbox, or start WLAN. The compiled CapDL report contains all 512
SMEM pages inside that exact no-map reservation, and the reserved-memory audit
allows only those specially named probe pages. The first temporary boot showed
all three items as not found in the uncached list. A second boot searched both
the uncached and reverse cacheable lists but found none. An exploratory
all-partitions scan then failed because a non-target partition did not match
the parser's assumptions; that was a probe limitation, not evidence of a bad
SMEM directory. The corrected probe follows Linux's actual
`qcom_smem_get(host=1)` route: it chooses the host-0:1 private partition when
present, otherwise the global private partition. On the Pixel, table entry 7
selected host 0:1 at offset `0x00110000`, size `0x0007c000`, cacheline `0x20`;
`offset_free_uncached` was `0x20` and `offset_free_cached` was `0x0007c000`.
Items 478–480 were absent from both allocation lists. This means the selected
private partition was empty when Microkit booted. It does not show that
LineageOS cannot create the items: matched Linux `qcom_glink_smem_register()`
allocates descriptor 478 and TX FIFO 479 when that GLINK edge registers, while
RX FIFO 480 is looked up lazily. A direct Microkit boot does not run that Linux
edge registration or its modem subsystem lifecycle, so absence is expected.
The latest temporary `fastboot boot` of the corrected probe reconfirmed the
same route and empty partition: host 0:1, table index 7, offset `0x00110000`,
size `0x0007c000`, cacheline `0x20`, free uncached `0x20`, free cached
`0x0007c000`; items 478, 479, and 480 were all absent. This is evidence that
the AP-side GLINK transport was not provisioned in this standalone boot. It
does not establish whether the remote modem was powered or running. The probe
remains read-only and does not inspect FIFO contents or start WLAN. The live
LineageOS checks and exact returned sizes confirm Linux's allocation route.
The resource validator checks the SMEM `reg`, `no-map`, and controller
`memory-region` phandle against both captured and generated FDTs.

The next read-only probe revision also resolves the Atoll modem SMP2P item IDs
435 (modem-to-AP inbound) and 428 (AP-to-modem outbound) from directory
metadata only. It does not read their entries or values, and item presence
would not prove the modem is currently alive; it only tests whether either
side published the SMP2P allocation in this standalone boot. The expanded
temporary boot found neither item 435 nor 428, along with the already absent
GLINK items 478–480. Item 428 is AP-owned and is expected to be absent without
Linux's SMP2P driver. Item 435 is the modem-to-AP record; its absence means no
modem-side SMP2P allocation is visible in this host 0:1 partition, but it does
not prove the modem is powered off. The probe did not read any SMP2P payload,
write SMEM, ring either mailbox, or start WLAN.

After the SMEM pipes come up, the matched GLINK implementation exchanges a
version/features message, opens the named `IPCRTR` channel and waits for its
acknowledgment, then negotiates receive intents before carrying QRTR packets.
The modem DT advertises three IPCRTR intent size/count pairs: `0x800 × 5`,
`0x2000 × 3`, and `0x4400 × 2`. The channel therefore needs more than a FIFO
reader: it must preserve GLINK channel IDs, intent IDs, completions, data
fragmentation, receive-done notifications, and mailbox wakeups. This is a
separate protocol implementation and authority boundary below the QRTR/QMI
codec.

The offline protocol harness now also has a GLINK record codec. It captures
the matched kernel's 8-byte little-endian header, 8-byte FIFO alignment,
NUL-terminated channel names, count/size/ID intent entries, and `TX_DATA` /
`TX_DATA_CONT` chunk metadata. The parser bounds channel names, intent batches,
chunk sizes, and reassembled message sizes before returning structured data.
Synthetic tests cover valid version/open/intent/data records and malformed
length, alignment, command, and bound cases. This validates byte-level framing
and fragmented-message reassembly (including channel/intent continuity and
remaining-byte accounting) only; it is not a GLINK transport, ring parser, or
evidence that Microkit may own the live modem endpoint. In particular, the
shared descriptor and live FIFO indices remain outside this model. Synthetic
ring snapshot helpers now validate head/tail bounds and copy wrapped byte
ranges without mutation; tests cover wraparound and out-of-range reads. They
do not access live indices or authorize a live ring reader. A stream-prefix
length probe handles incomplete headers/records and coalesced synthetic records.
The model also encodes the proposed policy order—verified exclusive AP owner,
verified peer reset/fresh boot, index initialization, then VERSION negotiation.
That gate is an executable design assertion, not a source of hardware evidence.

This changes the implementation order: do not begin by mapping that SMEM range
or ringing its mailbox from a new PD. First define who owns modem GLINK during
the Microkit boot and how Android/Linux and Microkit are prevented from using
the same descriptor/FIFO state concurrently. Until that contract exists,
SMEM, mailbox channel 12, and INTID 481 remain unavailable to Microkit WLAN.

### Proposed narrow broker boundary (not implemented)

Treat the modem GLINK edge as one exclusive transport resource, even though
WLFW is only one service carried over its `IPCRTR` channel. A future trusted
broker would be the sole owner of the SMEM descriptor/FIFOs, mailbox channel
12, and INTID 481. WLAN policy and application domains would receive bounded
request/reply IPC from that broker; they would never receive SMEM mappings,
mailbox capabilities, or the IRQ capability.

The first client interface should expose only the exact WLFW operations needed
for bring-up: service discovery for `(service 0x45, version 1, instance 0)`,
transaction-matched QMI requests from the parsed protocol model, and a small
set of validated status indications. The broker should reject other QRTR
services, unexpected QMI message IDs/TLVs, stale endpoint events, oversized
frames, duplicate transaction IDs, and responses that do not match an
outstanding request. It should copy bounded packets between fixed-size queues
so the WLAN policy component cannot retain or mutate shared transport buffers.
No generic QRTR socket or arbitrary modem RPC should be exposed to apps.

Acquisition needs a separate platform contract: prove that the AP-side modem
GLINK instance is unused by any concurrently running Android/Linux owner, and
establish whether ABL leaves the modem and SMEM ring live or resets them before
Microkit starts. Only a documented remote reset/ready handshake or verified
fresh-ring state may authorize descriptor initialization. If ownership or
peer state is ambiguous, the broker remains disabled. Modem crash and SMP2P
entry notifications should be mediated separately; WLAN start must remain
blocked until the SMMU/MSA and power sequencing gates are satisfied.
The first implementation milestone is represented by
[`wlfw_protocol_model.py`](../platforms/sunfish/wlfw_protocol_model.py) and
[`sunfish_wlfw_protocol_model_test.py`](../tests/sunfish_wlfw_protocol_model_test.py):
offline QRTR v1/v2 envelope and control parsing, QMI frame/TLV parsing, WLFW
MSA-info request/response codecs, service lifecycle, and a typed single-flight
`WlfwBrokerModel`. QRTR parsing checks versioned header sizes, lengths,
alignment, flags, and control-port normalization. The lookup builder and
NEW_SERVER parser mirror the matched QMI helper's 20-byte control payload. The
MSA decoder accepts only the matched message ID and transaction, validates the
common result and up-to-two-region TLVs, and rejects malformed lengths and
invalid `secure_flag` values. Pool bounds and overlap policy remain in the
separate offline MSA ownership planner; synthetic decoded regions are checked
there without invoking SCM.

The codecs also cover ICNSS's default indication registration, empty MSA-ready
and capability requests, and bounded responses. Registration `fw_status` and
capability chip/board/SoC, firmware-version, build-ID, and MAC-count fields have
field-level length checks. Unknown optional capability TLVs are retained;
malformed known fields and duplicates are rejected. Tests include synthetic
timeout, endpoint replacement, late-response, and service-departure cases, but
do not replay captured packets or open a device, map SMEM, or invoke SCM.
The production Rust core now also contains allocation-free GLINK record,
intent, and TX-data-chunk parsing, QRTR v1/v2 framing and WLFW service
discovery, QMI framing, and the bounded ICNSS startup codecs: indication
registration, MSA-info, MSA-ready, and capability requests and response
schemas. Capability strings, known field sizes, duplicate TLVs, mandatory
unknown TLVs, and operation-specific payload limits are checked without
allocation. Host tests cover valid and malformed wire layouts. The Rust
library compiles for the Sunfish seL4 Microkit target. These are codecs only;
there is still no GLINK/QRTR ring transport, broker task, memory assignment,
firmware start, or live modem access.
`WlfwStartupModel` mirrors ICNSS's call order and refuses failed, out-of-order,
or unverified MSA/SMMU prerequisites. Its boolean inputs are test claims only;
they neither prove hardware gates nor initiate device operations.

`WlfwBrokerModel` adds endpoint and transaction matching, one outstanding
request at a time, bounded deadlines, cancellation when the service departs,
and the fixed 2 MiB MSA-info request. It intentionally does not retry timed-out
operations: a late response is rejected and retry policy remains an explicit
caller decision. This is still an offline protocol/broker model, not a GLINK
or QRTR transport, a live WLFW client, or authorization to touch modem SMEM.
`WlfwPolicyQueue` now models the broker-to-policy boundary as four bounded
completion slots. It accepts only responses matching a request ticket and
session, caps each response by its operation schema, rejects overflow without
overwriting queued work, and flushes entries when the service leaves. This is
still an offline Python model; the component capability boundary must make the
broker the only writer and policy the only reader in a native implementation.
`wlfw_policy_ipc.h` defines and host-tests the corresponding fixed 224-byte
event slot: a 16-byte little-endian header and up to 207 bytes of bounded QMI
payload, with operation/session/transaction checks and zero padding. Its host
test passes events through a four-slot producer/consumer ring simulation and
checks FIFO order, backpressure, and wraparound. The simulation is single-
threaded: shared-memory synchronization, Microkit channel wiring, and
capability assignment are not implemented by that host test. A Microkit probe
maps the event page read-write in the broker and read-only in policy. On the
Pixel 4a, temporary-boot tests returned `IPC PASS` for the protected-call
route, `SHM PASS` for broker-write/client-read visibility, and `QUEUE PASS` for
four synthetic events, overflow rejection, FIFO decoding, consume
acknowledgement, and slot wraparound. A follow-up `QUEUE PASS` also verified
an explicit session rollover: the broker clears the old ring, rejects a stale
session request, and accepts a new-session event. This rollover is a test
command within the same broker process; it does not simulate a broker crash,
remote modem restart, or live GLINK endpoint replacement. The policy side
writes its PASS/FAIL label into the bootloader-reserved splash buffer, so these
results were checked on-device without UART. The probe uses only synthetic
messages and the reserved display buffer; no modem transport or device
operation is present. A separate temporary-boot recovery probe is now built:
the child broker deliberately faults after publishing a session-1 queue marker;
the supervisor validates the fault state, advances its read-only-to-child
session authority to 2, clears the stale queue, and restarts the child through
Microkit's hierarchy API. The restarted child checks that the stale marker is
gone and submits a session-2 event for supervisor validation. It also checks
its private initialization counter, confirming that `microkit_pd_restart`
restarts the child while preserving its PD memory rather than loading a fresh
image. The Pixel temporary boot displayed `BROKER RECOVERY PASS`. The build
audits CapDL physical memory against the Sunfish device tree; it includes no
modem mappings or transport actions. This verifies synthetic local recovery,
not a remote modem restart or live WLFW recovery.
Hardware integration remains blocked on verified exclusive modem ownership,
a fresh peer/ring handshake, and separate owners for power votes, SMP2P, SMEM,
and SMMU state.

The exact permission transition is narrower than “give WLAN the MSA pool.”
ICNSS first sends the MSA base and size to WLFW over QMI. WLFW returns a list of
memory subregions, each with an address, size, and `secure_flag`; ICNSS checks
that every range lies within the MSA pool and assigns those returned ranges
individually. The raw `boot_a` DTB reserves `0x180000` bytes at `0x97500000`,
while `qcom,wlan-msa-memory` says `0x100000`; because the node uses
`qcom,wlan-msa-fixed-region`, the fixed-region code uses the full reservation
from the active, merged DT as its pool. The actual LineageOS runtime DT
overrides that reservation to `0x98d00000`/`0x200000` (see the DT overlay
reconciliation below). A dynamic-debug boot observed WLFW return two regions:
`0x98d00000`/`0x4000` with secure flag 1 and `0x98d04000`/`0x1fc000` with
secure flag 0. Together they span the active 2 MiB pool. Linux then performed
the per-region secure-monitor assignments described below; this is evidence
about Android's startup, not permission to reuse that transition in
Microkit. The QMI response, not either size alone, determines which subranges
change ownership. The captured lines are in
`.cache/sunfish-kernel-diagnostic/msa-and-route-readback.txt`.

ICNSS defines these source-to-destination permission sets in the matched
kernel:

| Region type | Starting owner | Destination owners | Access |
| --- | --- | --- | --- |
| nonsecure | HLOS (`0x03`) | MSS MSA (`0x0f`), WLAN (`0x18`), WLAN CE (`0x19`) | read/write |
| secure | HLOS (`0x03`) | MSS MSA (`0x0f`), WLAN (`0x18`) | read/write |

The destination list depends on WLFW's per-region `secure_flag`; that flag is
not specified by the DT. A temporary dynamic-debug boot captured the QMI
response. Applying the matched ICNSS permission table to those observed flags
means Linux requests assignment of the secure `0x4000` segment from HLOS VMID
`0x03` to VMIDs `0x0f` and `0x18`, and the nonsecure `0x1fc000` segment from
`0x03` to `0x0f`, `0x18`, and `0x19`. This is source-derived from the observed
flags; the per-call SCM result was not preserved in the local capture. Startup
order is also significant: ICNSS requests
power, sends MSA information, applies these per-range assignments, and only
then sends MSA-ready. On a later readiness/capability failure it attempts to
return all regions to HLOS, but that error-path rollback result is ignored.
A native port should therefore track each validated QMI range and treat a
failed or ambiguous rollback as an ownership failure; it must not assume the
whole reserved pool transferred atomically.

The matched source clarifies what a native implementation must reproduce and
what it must not expose. `icnss_priv` is allocated with `devm_kzalloc`, so each
reported region begins in the `HLOS_ALL` permission state (enum value zero).
The QMI handler copies each region address, size, and secure flag only after
checking that the range lies wholly inside the fixed MSA pool. The ICNSS
permission helper then selects one of two compile-time destination lists based
on `secure_flag`; it does not let WLFW choose arbitrary VMIDs or permission
bits. For the observed pair of regions, this produces exactly two fixed
handoff requests described above.

`hyp_assign_phys()` wraps each range as a scatter-gather entry. The secure
buffer helper copies the source VMID list and packed destination VMID/permission
records into aligned kernel allocations, flushes those buffers from cache,
serializes calls with a mutex, batches physical ranges, and invokes SCM service
`MP`, function `0x16` (`MEM_PROT_ASSIGN`). Its source comment says `-EAGAIN` is
safe to retry; an SCM failure is returned as `-EADDRNOTAVAIL`, meaning the
caller must treat the range as potentially unusable by HLOS. This function
does not update ICNSS's tracked `perm`; the outer loop advances that state only
after each individual region succeeds. If a later region fails, Linux attempts
to restore earlier regions, but its rollback reuses one `old_perm` variable
and ignores rollback errors. The current observed pair both start in HLOS, so
the stale-variable detail does not change this first transition; ignored
rollback errors still mean the caller cannot assume a partial transition was
undone.

There is another ABI layer below that helper. The matched Linux `scm_call2`
places the function ID and argument-info word in `x0`/`x1`, the first three
values in `x2`–`x4`, and—because this request has seven values—puts the
remaining values in a separate physical extra-argument buffer addressed by
`x5`. Linux also chooses the SMC32/SMC64 convention based on a monitor feature
check, retries interrupted calls, retries busy results, serializes callers,
and remaps monitor errors. The seven values include physical pointers to the
range-batch, source-VMID list, and destination permission list, so cache
maintenance and the lifetime/ownership of those buffers are part of the ABI.
`seL4_ARM_SMC_Call` only forwards `x0`–`x7`; it does not implement these Linux
SCM conventions. We have not verified that the Pixel's secure monitor accepts
the Linux-specific call from a Microkit caller, and will not test it by issuing
a live ownership-changing call.

The matched ICNSS `hyp_assign_phys()` wrapper returns a status, not an owner
query. ICNSS updates its local permission enum after a zero return and assumes
the requested assignment succeeded. The Pixel kernel's separate
`qcom_scm_assign_mem()` helper documents a zero return as success and says
`srcvm` is updated. In the matched source, however, that mask is constructed
from the requested destination VMIDs and copied into `*srcvm` after the SCM
call returns; it is not read back from the secure monitor. See the
[matched `qcom_scm_assign_mem()` implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/firmware/qcom_scm.c#L1903-L2041)
and [the `hyp_assign_phys()` wrapper](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/secure_buffer.c#L1760-L1789).
No independent memory-owner query is established in these interfaces or the
other public Sunfish sources reviewed here; that does not prove that Qualcomm
has no private or firmware-specific query. A native manager can treat success
as authoritative only after the exact SCM ABI and secure-monitor success
contract are verified; absent that contract, the state remains unknown.
Failure or interrupted/ambiguous completion remains quarantined.

The matched kernel does expose an SCM `IS_CALL_AVAIL` helper. Its input is a
service/function identifier and its result is the monitor's availability
response; it can tell us whether a particular SCM function is implemented,
not the current VMID owner of memory, the QHEE delegation of SMMU registers,
or whether an EL1 caller is authorized to reconfigure them. A fixed query for
`MEM_PROT_ASSIGN` could be a read-only ABI smoke test later, but a positive
answer would not clear either ownership gate and does not justify spending
another phone boot on it yet.

This gives us a concrete stop line for the next bring-up step. The remaining
question is whether QHEE on this exact Pixel 4a boot chain explicitly delegates
the Apps SMMU's relevant registers, context banks, stream-match groups, and
their protection controls to EL1. The early-boot snapshot and an SCM assignment
status cannot answer that. We need a platform-specific ownership/delegation
contract from Qualcomm/Google boot documentation or an authorized handoff API;
until then, do not write the SMMU, issue memory-protection assignment calls,
or start WLAN firmware. If no such contract is available, the practical next
path is to keep the current evidence and pursue a supported Android/Linux
service boundary rather than take over the shared hardware.

For NexatrodeOS, the design implication is a **fixed-operation ownership
broker**, not a generic SCM syscall. The WLAN driver should submit only a
prevalidated MSA-region identifier and a lifecycle request such as
`assign-for-firmware` or `reclaim-after-quiesce`. A small trusted manager should
own the fixed physical pool, validate all ranges against the runtime DT
reservation, use immutable per-region VMID/permission policy, serialize each
transition, and report success only after its ownership state is consistent.
Neither Slint/apps nor the WLAN driver should supply physical addresses,
VMIDs, permissions, or arbitrary SCM function IDs. If the SCM result is
ambiguous, the manager must quarantine the region and keep firmware/DMA
disabled. The offline planner in
[`msa_ownership_model.py`](../platforms/sunfish/msa_ownership_model.py)
implements only the range checks and fixed VMID policy; its host tests cover
the captured regions, invalid flags, overlap, range escape, and excess region
count. It explicitly does not execute SCM or change ownership. No Microkit SCM
broker or live assignment call has been implemented or attempted.

The module also contains `resolve_msa_transaction()`, a host-only policy model
for partial assignment. It accepts explicit per-region evidence that the
operation succeeded under a reviewed ABI whose return contract confirms the
resulting owner, or that an independent readback confirms it if one exists. On
the first unverified assignment it models reverse-order rollback; a target or
rollback with unknown ownership is quarantined, while untouched regions remain
known to HLOS. Tests cover complete assignment, partial success, failed
rollback, and incomplete evidence. This is a decision-policy test, not an SCM
emulator or a claim that the Microkit success contract or an owner readback is
currently verified on Sunfish.

There is a security-sensitive difference to resolve before any firmware-start
probe: the captured live `icnss` node has `qcom,smmu-s1-bypass`, and the matched
Linux ICNSS implementation explicitly requests stage-1 SMMU bypass when that
property is present. That Linux configuration is not an acceptable default for
an isolated seL4 driver. A diagnostic kernel has now identified the active
Linux stream-match entries and context banks. The live DT ties ICNSS to the apps
QSMMU-v500 with IOMMU stream ID/mask pair `<0x240 0x01>`; the second cell is a
stream-match mask, not a context-bank number. The controller has
`qcom,skip-init`, which makes the Linux driver skip resetting its stream
mappings and context banks. Before that reset step, the matched `arm-smmu.c`
scans existing stream-match and route registers and records context-bank
indices for mappings already in translation mode. The Linux handoff code says
these bootloader-provided banks are expected to preserve ongoing traffic, and
only one DT device should overlap those bootloader stream IDs. The runtime
readback below shows Linux ultimately attaches these devices to banks 6 and
37–39. This identifies Android's post-attach state, but does not grant
NexatrodeOS authority to reuse or rewrite those shared banks. The secure
`hyp_assign_phys()` path relies on Qualcomm VMID permissions for the WLAN and
modem subsystems.

There is also a distinct IPA WLAN SMMU context in the captured DT:
`qcom,ipa-smmu-wlan-cb`, stream ID/mask `<0x521 0x00>`, also marked
`qcom,smmu-s1-bypass`. Read-only sysfs inspection shows this IPA context as
IOMMU group 3, bound to the IPA platform driver, while ICNSS remains alone in
group 7. These are separate devices/groups and stream IDs; the IPA context's
name alone would not prove which buffers traverse it, but the matched IPA
source confirms it is a separate WLAN IOMMU domain: its probe applies
`DOMAIN_ATTR_S1_BYPASS` when this DT property is present, then maps the DT's
`qcom,additional-mapping` ranges (commented as IPA microcontroller RAM) with
read/write/MMIO permissions. The captured range is `0x1e60000` for `0x80000`
bytes. The eventual policy must trace both the ICNSS control/firmware path and
the IPA/WDI packet-offload path: the captured IPA node also declares
`qcom,ipa-wdi2` and `qcom,ipa-wdi2_over_gsi`. Then decide which stream IDs the
native driver actually owns. Neither Linux group membership nor the DT bypass
property proves the live context-bank contents.

The captured DT sharpens this into a concrete isolation blocker: **all three
IPA stage-1 contexts are configured for bypass**. The AP, WLAN, and IPA
microcontroller contexts use stream IDs `0x520`, `0x521`, and `0x522`
respectively, and each node has `qcom,smmu-s1-bypass`. ICNSS's WLAN stream
`0x240` also has that property. In the matched IPA source, WDI2-over-GSI asks
whether both the AP and WLAN contexts are translated; with these DT flags the
answer is false. `ipa_create_gsi_smmu_mapping()` then returns the physical
address unchanged when both sides bypass. The Qualcomm Wi-Fi host code has an
SMMU-enabled branch that passes scatter-gather ring tables, but the bypass
configuration selects the direct-address branch instead. The runtime register
readback confirms the S1-bypass state for these attached contexts: CBAR is
`0x0001f300` and SCTLR is `0x00c000e6`, so `SCTLR.M` (bit 0, translation
enable) is clear. Thus this Linux configuration does not provide a restricted
per-buffer IOVA map for WDI2 rings; it relies on physical addresses and the
platform's surrounding DMA ownership rules. This does not show whether WDI2
was active during the capture.

For a native seL4 driver, do not copy this bypass arrangement. The first
networking milestone should keep IPA offload out of scope and give the WLAN
device only explicitly pinned ring and packet buffers through a verified
translated SMMU domain, if the stream can be safely taken over from the boot
chain. Before considering WDI2/IPA, we would need a separate design for both
the WLAN and IPA masters, their GSI event/channel rings, and their buffer
ownership and cache-coherency rules. If firmware or bootloader state forces
stage-1 bypass, stop before starting WLAN firmware; do not treat the SMMU
context-bank Linux device node as evidence of isolation.

The Microkit SDK build has `ALLOW_SMC_CALLS` enabled and supports granting an
SMC capability to an individual protection domain. Our current Pixel SMC calls
only exercise PSCI system reset; no Qualcomm SCM memory-assignment call or
SMMU driver exists in the port. The Linux SCM argument layout is visible in
the matched source, but a Microkit-safe implementation still needs the right
ABI and cache-maintenance behavior, secure-firmware acceptance, a validated
MSA permission transition, and a tested ownership/error state machine. The UI
and WLAN client domains should not receive that generic SMC capability.
The generated `seL4_ARM_SMC_Call` interface forwards caller-provided `x0`–`x7`
to the monitor; the capability is not an allowlist for a particular function
ID. The current Slint app and touch domains have no `smc="true"`. Reset stays
in the separate `slint_reset` watchdog domain; the touch service now exits its
failed probe path and leaves reset to that watchdog. A host policy test checks
that only this fixed-purpose reset domain receives SMC in the Slint system.
Any future SCM broker must remain isolated from both touch and app domains.
That reduces the trusted code that can issue arbitrary secure-monitor calls;
it does not itself constrain secure firmware's ABI.

The initial unprivileged ADB shell could not read the WLAN module's sysfs
`srcversion`/`version` attributes, `dmesg`, or `/proc/iomem`. A root-enabled,
read-only session can read `/vendor/lib/modules/wlan.ko` and `/proc/iomem`,
but the module's `version` and `srcversion` attributes are absent and dmesg
does not expose a QCACLD commit. The module hash matches the binary already
captured in the local cache, resolving file access and hash verification
without identifying the external source revision. ADB was returned to its
normal shell user after each read-only check.

Further read-only sysfs inspection found ICNSS alone in Linux IOMMU group 7 and
the separate IPA WLAN SMMU context in group 3, bound to the IPA driver. The
unprivileged shell could not read the SMMU debugfs state,
`/sys/kernel/iommu_groups/7/reserved_regions`, or `/proc/iomem`. This group
metadata does not expose active context-bank registers, prove that stage-1
bypass is disabled, or show the complete DMA policy. The running vendor
fingerprint is `google/sunfish/sunfish:13/TQ3A.230805.001.S1/10786265:user/release-keys`.
Root-enabled ADB later verified the exact module file, but its absent `version`
and `srcversion` attributes still prevent identifying a specific QCACLD source
revision.

I repeated the register-access check in the phone's already-enabled rooted
debugging mode, then restored ADB to its normal shell user. Root made
`/proc/iomem` and each group's `reserved_regions` readable: the Apps SMMU
resource is `0x15000000`–`0x150fffff`, and groups 3 and 7 each report only the
generic MSI window `0x08000000`–`0x080fffff`. The SMMU platform node exposes no
sysfs register resource, `CONFIG_DEBUG_FS` is not set, and `/dev/mem` is absent, so
the on-device `devmem` tool cannot read the live stream-match registers. The
rooted check therefore improves the resource inventory but still does not
reveal the active SMR/S2CR routes or context-bank tables.

A temporary read-only Microkit probe is now built from
[`smmu-read-probe.c`](../platforms/sunfish/smmu-read-probe.c) and
[`smmu-read-probe.system`](../platforms/sunfish/smmu-read-probe.system). It
reads the Apps SMMU ID registers, finds any routes matching SIDs `0x240`,
`0x520`, `0x521`, and `0x522`, and reports each route type/context bank plus
translation registers when applicable. The route summary is also drawn on the
known bootloader splash framebuffer, so the result can be checked without
USB-Cereal; the context-bank number on screen is hexadecimal. Its SMMU MMIO
mapping is read-only and uncached. The first temporary run reached the
display, but its route results were invalid because the probe used
`0x00`–`0x1c` as ID-register offsets, which are control-register addresses.
ARM/Linux define ID0–ID7 at `0x20`–`0x3c`. The corrected run showed `NO ROUTE`
for all four target SIDs. An early transcription reported
`ID0=0x4C017E4F` (79 groups); the later complete photo/readback
`IMG_8967.heic` shows `ID0=0x4C017E4E`, which the pinned Linux parser decodes
as 78 groups (valid slots 0–77). The same later capture reads
`ID1=0x6000003D`, or 61 context banks; earlier 80-bank and 79-group
transcriptions are superseded. That capture also confirms `NO ROUTE` for both
WLAN aliases `0x240`/`0x241` and the three IPA SIDs `0x520`–`0x522` at ABL
handoff. These are temporary-boot values; Linux may program routes later when
its drivers attach, so this does not establish the active LineageOS
configuration. The build script is
[`build-smmu-read-probe.sh`](../platforms/sunfish/build-smmu-read-probe.sh),
and the image is `.cache/pixel-sunfish/smmu-read-probe/fastboot.img`. The
image is temporary and was booted with `fastboot boot`; no partition was
flashed. Automatic reset is disabled by default so the report stays visible;
set `SMMU_PROBE_RESET_OUTER` to a positive loop count to enable it.

An expanded read-only run on 2026-10-09 summarized the full stream-match and
context-bank state at the same ABL handoff. The first photo was transcribed as
`ID0=0x4C017E4F`, `ID1=0x6000003D`, 6 valid SMRs, six translation-type S2CRs,
zero bypass/fault/other S2CRs, and six translation context-bank references.
The later complete capture corrects ID0 to `0x4C017E4E` and the group count
from 79 to 78; the six-route and 61-bank observations remain consistent.
None of the five target IDs (`0x240`, `0x241`, `0x520`–`0x522`) matched an
SMR. Across all 61 reported context banks, `SCTLR.M` was clear; the probe's
`CB NONZERO=61` count means each bank had a nonzero CBAR or SCTLR value, not
that translation was active. Decoding `ID1=0x6000003D` with the matched Linux
register definitions gives 61 context banks, zero stage-2-only banks, and
4-KiB context-bank register pages. This is hardware capacity information,
not a free-bank or ownership list.

The expanded all-bank report was then booted temporarily and photographed on
2026-10-10. I checked its address calculation against ARM IHI 0062D.c and the
matched Pixel Linux driver: `ID1=0x6000003D` selects 4-KiB pages and
`NUMPAGENDXB=6`, so the global and context-bank windows are each 128 pages
(512 KiB). The context-bank window starts at SMMU base + `0x80000`, and bank
`n` advances by 4 KiB; Linux maps the same split at half of the 1-MiB DT
resource. The probe's `CBAR` reads use the corresponding GR1 table at offset
`0x1000 + 4*n`. This agreement makes a mistaken window offset an unlikely
explanation for the captured values.

In that photo, all 61 exposed banks had `SCTLR.M=0`. CB0–CB3 had
`CBAR=0x0001F000` and `SCTLR=0x000000E0`; CB4–CB60 had `CBAR=0` but nonzero
raw `SCTLR`, `TTBR0`, and `TCR` words (for example, the photographed CB4
`SCTLR` is `0x2BF44312`). The summary's `CB NONZERO=0x3D` is only a count of
banks with a nonzero CBAR or SCTLR; it does not mean all banks have configured
translations. ARM defines `SCTLR.M` as the context bank's MMU enable, but other
SCTLR fields still affect transactions while translation is disabled, and
some register reset fields are architecturally unknown. Thus `M=0`, `CBAR=0`,
or a bank not referenced by the six visible S2CRs does not prove that bank is
unowned or safe to overwrite. This is an ABL handoff snapshot through the
current register view; it does not reveal secure-world reservations or later
LineageOS programming.

A fresh `fastboot boot` of the same read-only probe on 2026-10-12 reproduced
the five `NO ROUTE` results and the summary counts above (`VALID SMR=6`,
`TRANS S2CR=6`, `TRANS CB REFS=6`, `CB M ON=0`, `CB NONZERO=61`). The photo
also reproduced the six previously captured slot assignments below. This
confirms the result is repeatable across temporary boots; it still describes
only the ABL handoff and does not grant permission to change SMMU state.

A follow-up read-only probe displayed all six valid entries. The phone photo
showed:

| Slot | SMR ID | SMR mask | S2CR type | Context bank | CBAR | SCTLR |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | `0x0340` | `0x000F` | 0 (translation route) | 0 | `0x1F000` | `0x00000000` |
| 1 | `0x02A0` | `0x000F` | 0 (translation route) | 0 | `0x1F000` | `0x00000000` |
| 2 | `0x02C0` | `0x000F` | 0 (translation route) | 0 | `0x1F000` | `0x00000000` |
| 3 | `0x0540` | `0x0000` | 0 (translation route) | 1 | `0x1F000` | `0x00000000` |
| 4 | `0x0300` | `0x000F` | 0 (translation route) | 2 | `0x1F000` | `0x00000000` |
| 5 | `0x0800` | `0x0440` | 0 (translation route) | 3 | `0x1F000` | `0x00000000` |

The SMR mask bits are wildcard bits in the stream-ID comparison; these are
not six exact, single-SID routes. The first three entries share CB0, and the
remaining entries point to CB1, CB2, and CB3. Although all six S2CRs select
translation-type routes, every referenced context bank has `SCTLR.M=0`, so
the readback does not show active stage-1 translation. This is a snapshot at
ABL handoff and does not identify which hardware clients own those unrelated
stream IDs. It is consistent with the distinct Linux-after-attach readback
below: the handoff snapshot has no target routes, while Linux later establishes
device routes with stage-1 translation disabled for the bypass-configured
WLAN/IPA devices.

Comparing the captured live runtime DT against the raw entries identifies two
of the otherwise unrelated streams: slot 3's exact SID `0x540` is the `usb0`
device, whose DT node requests S1 bypass; slot 5's `0x0800/0x0440` masked
route matches the MDSS/SDE `iommus = <&apps_smmu 0x800 0x440>` request (and may
also cover other IDs because its mask is broad). These are source/DT matches,
not proof that those drivers were attached at the time of the ABL snapshot.
The other four SID/mask pairs have no direct
`iommus` match in the merged runtime DT, so their owning firmware or hardware
clients remain unidentified. In particular, none of the six entries matches
the WLAN or IPA target SIDs.

I added an executable collision check for this captured handoff table in
[`validate-smmu-handoff.py`](../platforms/sunfish/validate-smmu-handoff.py).
It applies the wildcard comparison used by the matched Linux
`arm_smmu_find_sme()` implementation: it confirms the candidate `<0x240,
0x01>` match covers exactly `0x240` and `0x241`, and neither SID overlaps any
of the six observed ABL patterns. That is evidence that this ABL snapshot did
not already route these WLAN IDs. It does not prove a stream-match slot or
context bank is available to NexatrodeOS: the expanded probe saw nonzero
CBAR/SCTLR contents in all 61 banks, and register values alone do not
establish ownership or whether secure firmware relies on a bank.

The matched Linux source also gives us a concrete translation model to
evaluate. ICNSS creates an IOMMU mapping for its IOVA aperture, but the live
DT's `qcom,smmu-s1-bypass` sets `DOMAIN_ATTR_S1_BYPASS` before attachment.
`arm_smmu_write_context_bank()` therefore leaves `SCTLR.M` clear for this
stage-1 context. A native design needs a real page-table context (TTBR, TCR,
MAIR, ASID, fault handling, and mapped pages) and a stream route to it; setting
only the S2CR to “translation” is insufficient. The Linux path programs these
registers during normal domain setup, but Microkit cannot inherit Linux's
IOMMU allocator or ownership assumptions. It is a register reference, not a
safe recipe for taking over the shared controller.

The same source shows why “reset the SMMU and start clean” is not a valid
handoff strategy. With `qcom,skip-init`, Linux skips resetting all stream
mapping groups and context banks, specifically preserving the bootloader
configuration; it still performs global TLB invalidation and configures global
fault reporting. The display route and other existing clients therefore make
global reinitialization unsafe without an explicit platform-wide takeover.
The initial native design must use a narrowly assigned WLAN entry and bank,
and avoid changing global controls unless the boot/secure-firmware contract
transfers ownership of the entire SMMU.

Linux's boot-handoff bookkeeping is also explicit: `arm_smmu_handoff_cbs()`
reads valid SMRs and S2CRs and marks the context banks referenced by
translation-type routes in its allocation bitmap. A later domain with no
handoff route gets a bank from that bitmap. The source comment says these
bootloader banks are expected to operate in bypass and exactly one device-tree
device must overlap their stream IDs. That policy is specific to Linux's
single kernel owner; a Microkit SMMU service needs equivalent ownership state
from the boot chain and must reserve every discovered route/bank before
allocating a WLAN context. The fact that Linux could allocate new banks after
boot is not proof that those banks are safe for a separate native boot.

The live Apps SMMU node enables `qcom,skip-init` and `qcom,use-3-lvl-tables`,
but not `qcom,enable-static-cb`; Linux therefore uses its ordinary dynamic
context-bank allocator. The driver's `msm_tz_set_cb_format()` SCM call is
specific to the optional static-CB path and changes page-table format, while
the separate `MEM_PROT_ASSIGN` operation traced for WLAN MSA memory does not
transfer SMMU register, stream-match, or context-bank ownership. The captured
DT/source path exposes no native service interface for claiming a WLAN SMMU
bank. A bootloader or secure-firmware ownership contract remains necessary;
the missing piece is not another guessed register value.

Qualcomm's public [Snapdragon access-control overview](https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/an-introduction-to-access-control-on-qualcomm-snapdragon-platforms.pdf)
describes VMID mapping, XPUs, and SMMUs as complementary enforcement points.
In its example trust hierarchy, the hypervisor controls non-secure
stream-to-context-bank mappings and stage-2 contexts while the CPU OS may
configure only its delegated stage-1 contexts. This is a general architecture
overview, not a Sunfish-specific ownership contract, so it cannot establish
which controls QHEE delegates on this Pixel. It does make the unanswered
platform question precise: before giving a Microkit protection domain SMMU MMIO,
determine whether the booted EL1 environment is authorized to program WLAN
stream routing and context-bank state, and which VMIDMT/XPU protections also
cover WLAN DMA. A stage-1 page table alone is not evidence of end-to-end DMA
confinement. A source search of the captured merged runtime DT and checked
SM7150 DT sources found no explicit VMIDMT/XPU resource or ownership node. That
absence does not show the hardware is absent; it means this DT inventory cannot
answer the delegation question. A platform-specific QHEE/boot-chain contract
or an authorized ownership query is still needed.

A further public-source check on 2026-10-12 found Qualcomm's general access-
control overview, but no public SM7150/Sunfish-specific SMMU delegation or
VMIDMT/XPU ownership contract. The overview itself cautions that trust
architectures vary by SoC and labels its hierarchy an example, so its
hypervisor ownership description is a warning to resolve, not proof of this
Pixel's exact access policy. An OEM/Qualcomm platform document or explicit
boot-chain handoff record remains the missing evidence.

The matched Lineage kernel config has `CONFIG_QCOM_QHEE_ENABLE_MEM_PROTECTION=y`.
Its Kconfig description says this option invokes SCM during kernel boot to
enable protection of kernel memory in QHEE stage-2 mappings, and
`smp_cpus_done()` calls `scm_enable_mem_protection()` after its EL2-mode check.
This is direct evidence that the Android boot path expects a QHEE-mediated
stage-2 memory-protection feature. It is a different control plane from the
Apps SMMU's stream-match/context-bank programming and does not establish that
EL1 owns those SMMU registers. Also, `smp_cpus_done()` ignores the function's
return value, so the config and call site prove the request path is compiled,
not that the monitor accepted it on this particular boot. See the matched
[Kconfig option](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/Kconfig#L952-L960),
[SCM implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/scm.c#L1290-L1334),
and [boot call site](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/kernel/smp.c#L401-L420).

A rooted, read-only ADB capture on 2026-10-12 resolved whether this request
succeeded in the current Lineage boot. `/proc/config.gz` confirms
`CONFIG_QCOM_QHEE_ENABLE_MEM_PROTECTION=y` and
`CONFIG_QCOM_SCM_QCPE` is unset, selecting the matched `drivers/soc/qcom/scm.c`
implementation. Early `dmesg` says `CPU: All CPU(s) started at EL1`, followed
by `scm_enable_mem_protection: SCM call failed` at 0.140634 seconds. Therefore
the kernel's QHEE stage-2 protection request was not confirmed on this boot.
The generic error log alone did not expose the secure-call return value. A
second read-only capture found no lower-level `scm_call failed` line for this
boot. In the matched source, that lower-level line is emitted when the SCM
transport returns a negative error; the boot-time message is also emitted
when the monitor's response register is nonzero. This points to successful
transport with a nonzero monitor response, but the response value is discarded
by the kernel and this remains a source-based inference. It does not establish
whether protection was already active, and it still supplies no evidence that
EL1 may program the Apps SMMU. ADB was returned to the normal shell user after
capture.

A temporary GPL diagnostic module built against this exact kernel release then
called only the exported `scm_is_call_available()` query for RTIC service
`0x19`/function `0x04` and MP service `0x0c`/function `0x16`. On 2026-10-12 it
reported `RTIC_MEM_PROTECTION=1 MP_ASSIGN_MEM=1`. The module was unloaded and
its `/data/local/tmp` copy removed; ADB was returned to uid 2000. This confirms
that the secure monitor advertises both call IDs as implemented. It does not
show that either call is authorized for this caller or succeeds with real
arguments, that QHEE delegates SMMU control to EL1, or that any memory has
changed owners. In particular, the earlier boot-time RTIC request still
reported a nonzero monitor response by the source-based interpretation above,
so do not use call availability as a substitute for a successful
ownership/delegation readback.

A follow-up read-only ADB inspection on 2026-10-12 checked whether the running
Lineage kernel exposes a lower-risk live SMMU ownership interface. The device
reports `CONFIG_ARM_SMMU=y`, but `CONFIG_IOMMU_DEBUG` and `CONFIG_DEBUG_FS` are
unset, and `/sys/kernel/debug` is not mounted. `/sys/kernel/iommu_groups`
contains Linux groups for `ipa_smmu_ap`, `ipa_smmu_uc`, and `ipa_smmu_wlan`
(groups 2, 4, and 3 respectively), alongside GPU, camera, display, USB, and
other clients. Those sysfs links describe Linux's device-to-IOMMU-group
associations; they do not expose the raw stream-match/context-bank registers,
secure-firmware ownership, or permission to retake a group. The matched
kernel's SMMU debugfs implementation is also unsuitable as an ownership API:
it is not enabled in this boot, and its per-device directory includes write
controls for map/unmap, fault injection, address translation, and clock
configuration. No controls were written and debugfs was not mounted. This
confirms that the running Android image has no safe built-in readback for the
missing delegation contract; obtaining a deeper Linux-side trace would require
a rebuilt temporary kernel, while proving authority still requires a secure
firmware/boot-chain contract or an authorized monitor readback.

The matched SCM source audit narrows that contract further. Qualcomm's
`secure_buffer.c` implements MP `ASSIGN_MEM` (`0x16`) as a state-changing
assignment of physical ranges to source/destination VMIDs and permissions;
the call returns a status, not a query of the current owner. The separate
`qcom_scm_assign_mem()` interface likewise builds an ownership-transfer
request and returns success/failure. The matched ath10k QMI client uses it to
assign WLAN MSA regions to `MSS_MSA` and `WLAN` (plus `WLAN_CE` for nonsecure
regions), then requests a transfer back to HLOS on teardown. Those are useful
examples of the Linux ownership transaction, but they are not an API for
asking whether a region or SMMU route is available to NexatrodeOS. Separately,
the Apps SMMU driver programs SMR/S2CR registers directly; the memory-VMID
assignment call does not itself delegate those route registers. See the
[matched secure-buffer implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/secure_buffer.c),
[SCM ownership-transfer implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/firmware/qcom_scm.c),
and [ath10k WLAN MSA client](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/net/wireless/ath/ath10k/qmi.c).
Do not probe this mutating API with guessed ranges or owners. The safe design
needs an authenticated handoff contract that names the WLAN memory ranges,
permitted VMIDs, exact SMMU register resources, and rollback behavior.

Before a native driver can make the first DMA, the SMMU manager needs an
explicit handoff contract from the boot chain or secure firmware. It must
establish exclusive control of the selected stream-match entry and context
bank, keep every unrelated stream route unchanged, program and invalidate a
restricted page-table domain while WLAN is held inactive, and only then route
both WLAN SIDs to translation. The first ring and packet buffers should be
the only mapped pages. An intentionally unmapped device address must produce
a captured context fault while adjacent CPU and display memory remains
unchanged. If the handoff cannot establish those preconditions, leave WLAN
and IPA disabled; do not infer ownership from an apparently unused ABL route
or from the number of context banks.

The host-only route planner now represents SMMU control delegation to EL1 as
its own required assertion, separate from transferred resource ownership and
the lists of verified-free slots/banks. This encodes the QHEE boundary found
above; its positive-path test uses synthetic assertions and is not evidence
that the Pixel grants those rights.

After restarting into LineageOS, a temporary root-ADB read of `dmesg` confirmed
the runtime initialization: the Apps SMMU reports 79 stream-match groups; the
IPA AP, WLAN, and UC devices were assigned to Linux IOMMU groups 2, 3, and 4;
ICNSS was assigned to group 7; and the IPA driver logged
`s1_bypass_arr[AP]=1`, `[WLAN]=1`, and `[UC]=1` with `fast_map=0`. The WLAN
firmware reached ready and the Android Wi-Fi stack connected successfully.
This confirms Linux's configured bypass policy and working firmware path, but
the logs do not expose the current SMR/S2CR entries or context-bank registers.
The kernel has no debugfs support, `/dev/mem` is absent, and the SMMU has no
sysfs register resource. Root ADB was returned to the normal shell user after
the read. The remaining direct runtime-register check requires a diagnostic
kernel path; tracefs IOMMU events can report future attach/map requests but do
not dump the live route table.

I verified the tracepoint limit with a short read-only capture: enabled the
existing `map`, `map_sg`, and `unmap` events, toggled Android Wi-Fi off and
back on, then disabled tracing. The phone reconnected, and all seven captured
events were GPU (`kgsl`) mappings; none named WLAN or IPA. These tracepoints
describe calls through the Linux IOMMU mapping API, not the SMMU's SMR/S2CR
register contents, so an empty WLAN result is consistent with the logged
`s1_bypass_arr[WLAN]=1` policy but is not proof that WLAN made no DMA. This
capture cannot reveal the active stream-to-context-bank route.
`CONFIG_KPROBES` is disabled. `CONFIG_DYNAMIC_DEBUG` is enabled, but without
debugfs it cannot be changed at runtime; the driver's dynamic-debug handoff
message runs while the SMMU driver starts and covers only routes already
present at bootloader handoff. The ABL probe found no target routes at that
point. The runtime attach state was subsequently measured with a diagnostic
kernel, as described next. Tracing was disabled and ADB was returned to the
normal shell user after capture.

The read-only runtime diagnostic is implemented in
[`lineage-sunfish-smmu-route-audit.patch`](../patches/lineage-sunfish-smmu-route-audit.patch).
It logs the target WLAN/IPA SIDs and Linux attachments matching all six ABL
stream-match patterns inside `arm_smmu_domain_add_master()` after the normal
driver attach. It reads SMR, S2CR, CBAR, and SCTLR without changing route
behavior. The kernel was built from the exact LineageOS source commit and
booted with `fastboot boot`; no partition was flashed. The full raw readback
is `.cache/sunfish-kernel-diagnostic/runtime-route-readback.log`.

| Runtime SID | Mask | Linux device | SMR slot | S2CR CB | S2CR | SCTLR |
| ---: | ---: | --- | ---: | ---: | ---: | ---: |
| `0x240` | `0x1` | ICNSS/WLAN | 13 | 6 | `0x00000306` | `0x00c000e6` |
| `0x540` | `0x0` | USB (`ssusb`) | 3 | 23 after reassignment | `0x00000317` | `0x00c000e6` |
| `0x800` | `0x440` | MDSS display | 5 | 3 | `0x00000303` | `0x00c000e6` |
| `0x520` | `0x0` | IPA AP | 8 | 37 | `0x00000325` | `0x00c000e6` |
| `0x521` | `0x0` | IPA WLAN | 9 | 38 | `0x00000326` | `0x00c000e6` |
| `0x522` | `0x0` | IPA UC | 10 | 39 | `0x00000327` | `0x00c000e6` |

The USB driver first logged CB1 and later reassigned SID `0x540` to CB23;
repeated records show the stable post-attach route. Display retains the
`0x800/0x440` match and CB3 seen at ABL handoff. The ABL entries
`0x340/0xF`, `0x2A0/0xF`, `0x2C0/0xF`, and `0x300/0xF` produced no matching
Linux attach records. Conversely, Linux creates the WLAN and IPA routes only
after those drivers attach, so the six-entry ABL table is not the complete
post-boot route state.

All captured CBAR values were `0x0001f300`; all S2CR entries were type 0
(route to a context bank), but every sampled `SCTLR.M` bit was clear. This
matches the Linux driver's S1-bypass handling: a context-bank route does not
prove page-table translation is active. The WLAN module loaded and Android
connected to Wi-Fi during this boot. This establishes Linux's runtime state,
not safe SMMU ownership or a translated-DMA design for Microkit.

Google also publishes a Sunfish Android 13 QCACLD source branch. As checked on
2026-10-09, its tip is commit `4f47232dc5c209b3137ea3ca26657e3853bf0d21`.
It is a useful comparison source for the phone's Android 13 vendor stack, but
does not establish which QCACLD revision LineageOS used for the currently
loaded module. The current Android 16 system / Android 13 vendor split makes
that distinction important; the branch is not being treated as a proven module
match.

## Candidate native seL4 design

This is a design proposal, not implemented or hardware-tested:

- Give a dedicated `sunfish_wifi` Microkit protection domain ownership of the
  WLAN MMIO ranges, WLAN IRQs, and only the DMA buffers it needs. Keep register
  access and packet parsing out of the Slint/app domain.
- Implement a narrow Wi-Fi control endpoint for enable, scan, connect,
  disconnect, and link status. Keep network credentials inside this service.
- Adapt the radio's Ethernet-frame TX/RX path to the existing sDDF network
  driver/virtualiser boundary. This lets the current lwIP client, DHCP/DNS/TLS,
  and Messages network service remain above the hardware-specific layer.
- Treat firmware boot and ICNSS as prerequisites, not as ordinary file reads:
  the driver needs a trusted source for firmware, the reserved WLAN MSA region,
  Qualcomm power/clock control, interrupt routing, and a verified DMA mapping.

The local sDDF integration already separates an Ethernet hardware driver from
network TX/RX virtualisers and clients. Current sDDF driver listings do not
include a Qualcomm WLAN device, so the WLAN-specific driver and control plane
would be new components; the network interface above them is reusable.

### Existing seL4 SMMU support is not Sunfish-ready

I checked the cached seL4 16.0/Microkit 2.3.1 source and the generated Sunfish
kernel configuration before considering a native driver. The build has
`CONFIG_ARM_SMMU` disabled, and seL4's config option is explicitly dependent
on `KernelPlatformTx2`. The existing ARM SMMUv2 code also labels its mapping
implementation as tested only on TX2, hardcodes the SMMU base at `0x12000000`,
and assumes a static register-window layout. Sunfish's Apps SMMU is instead at
`0x15000000`.

More critically, the stock boot initializer is not a safe handoff routine for
this phone: it rewrites every stream-to-context entry, invalidates every stream
match, disables every context bank, invalidates the global TLB, then enables
the controller. The Apps SMMU is shared with display and other devices, and
ABL already leaves display routes in it. Calling that initializer could break
active devices and erase bootloader state before WLAN ownership is established.

Microkit 2.3.1's SDF compiler independently has no AArch64 IOMMU device
identifier: its IOMMU address-space path accepts PCI identifiers on x86 and
returns `UnsupportedArch` for AArch64. So a normal Microkit `.system` file
cannot yet declare the WLAN stream ID and an isolated DMA address space, even
if the kernel SMMU option were made buildable. The current Pixel system file
therefore stays without WLAN/SMMU authority.

This is a toolchain gap as well as a register-driver gap. Microkit's CapDL
builder currently constructs only x86 `IOSpace` objects, and its memory
mapper has an explicit TODO for SMMU support. A Sunfish implementation must
add architecture-aware SMMU domain/stream objects and mapping generation to
the SDF-to-CapDL path, then connect those capabilities to seL4's ARM SID and
context-bank objects. Merely adding `iommu` syntax to the platform `.system`
file would not create protected DMA mappings.

The work is feasible, but the next implementation is a platform-specific,
read-only Sunfish SMMU manager design—not enabling the TX2 code. It should
first consume the captured ABL route table, identify only explicitly
transferred free SMR/context-bank resources, preserve every unrelated entry,
and refuse startup if the live register state differs from the validated
snapshot. After that, add an explicit Microkit resource representation for
SID/SMR ownership and a bounded map/unmap interface; only then can a separate
test prove that a WLAN context translates an IOVA while unrelated routes and
devices remain unchanged. Until those steps exist, keep WLAN DMA and firmware
startup disabled. This is also why the phone-side next check, when needed, is
another read-only ownership snapshot rather than a write or WLAN boot.

The source work can proceed without another phone session: first make a host
model test for route allocation, overlap, and preserve-on-update behavior;
then adapt the seL4 configuration and register-window mapping for Sunfish;
then extend Microkit's AArch64 SDF/CapDL generation and test generated
capabilities offline. The first device test should remain read-only and compare
the manager's observed state with the known ABL snapshot. Hardware writes
remain a later, explicit gate after the ownership transfer is proven.

The first host-only policy model is now in
[`smmu_handoff_model.py`](../platforms/sunfish/smmu_handoff_model.py), with
[`sunfish_smmu_handoff_model_test.py`](../tests/sunfish_smmu_handoff_model_test.py).
It refuses to plan without an explicit ownership-transfer assertion and
caller-supplied verified-free SMR/context-bank resources, rejects overlapping
routes, rejects malformed or duplicate snapshot entries and out-of-range
resource assertions, and returns an additive plan that preserves the observed
table. It cannot verify that those assertions are true and it never touches
hardware;
the current ABL capture has no proven free context bank, so it cannot by itself
authorize a real route change. Run its host tests with
`python3 -m unittest tests/sunfish_smmu_handoff_model_test.py -v`.

The previously booted read-only handoff image displayed every context bank's
`CBAR`, `SCTLR`, `TTBR0`, and `TCR` on screen and emitted the same records over
UART. That image was booted temporarily with `fastboot boot` and photographed;
no partition was flashed. Its values refine the earlier summary above, but a
register snapshot alone still cannot establish secure-firmware ownership or
authorize reuse.

On 2026-10-22, I rebuilt the probe offline to make its next screen more useful
for checking route availability. The candidate renders every valid stream
match entry (up to 80 rows, covering the recorded 78 groups), including the
slot, SID/mask, S2CR type, context-bank index, CBAR, and SCTLR. Three 64-bit
bitmaps summarize translated-route references, nonzero CBARs, and `SCTLR.M`
across CB0–CB63; UART retains the full context-bank dump. The SMMU mapping
remains read-only; the candidate includes no reset PD and no SMC capability.
It has only been built and host-validated; it has not yet been booted on the
Pixel. A complete fresh table can identify unoccupied match slots and
unreferenced/untranslated banks, but still cannot prove secure-firmware
ownership or authorize reuse.

### First-driver authority and DMA policy (proposed)

The initial target is a control-and-firmware-ready probe, followed by a minimal
packet interface. It is not a scan/connect milestone. Use this authority
contract when adding domains to a Microkit system file:

| Resource | Proposed owner | Initial policy | Evidence required before enable |
| --- | --- | --- | --- |
| WLAN control MMIO and IRQs | `sunfish_wifi` only | Grant the smallest exact register windows and WLAN IRQs after decoding the live DT; never map them into Slint or app domains | Confirm the address/size/IRQ tuple against the selected DTB and identify shared power, clock, or reset registers that need a broker |
| Apps SMMU (`0x15000000`, `0x100000`) | one trusted `sunfish_smmu` manager | Do not grant the shared controller to WLAN. Keep route-table writes behind this manager and expose narrow, validated map/unmap calls | Establish safe takeover and a verified translated stream route; current ICNSS/IPA DT requests bypass and Linux `qcom,skip-init` preserves handoff state |
| WLAN MSA pool (active Lineage range `0x98d00000`, `0x200000`) | reserved-memory/ownership manager until the native DT and assignment design are verified | Exclude the full active pool from ordinary RAM and client mappings. Assign only bounds-checked WLFW subranges through a dedicated SCM operation after its ABI is reviewed | Reconcile the Microkit input DT; review captured per-segment VMID permissions, cache rules, and successful/rollback semantics |
| IPA WLAN/AP/UC contexts and GSI/WDI2 buffers | no NexatrodeOS owner in milestone 1 | Keep IPA offload disabled; create no DMA rings or mappings | Separately prove stream ownership, translated mappings, ring layout, and buffer/cache ownership before a later milestone |
| Firmware image and calibration data | trusted firmware-loader service | No blobs are copied into the image yet; validate names, sizes, hashes, provenance, and licensing before use | Identify versions compatible with the phone's vendor stack and define an authenticated image source |
| Power, clocks, regulators, SCM | narrow platform-control service | Give the WLAN client no generic SMC capability or direct shared power-controller access | Define allowlisted operations, bounds, sequencing, rollback, and secure-firmware acceptance |
| Packet buffers | `sunfish_wifi` plus one bounded sDDF-facing queue | Share no DMA-capable buffer with Slint or arbitrary app domains; first expose copied Ethernet frames through a bounded interface | Verify IOVA-only device access, buffer pinning, ownership transitions, and cache maintenance under stress |

The current Pixel Slint system description contains no WLAN protection domain,
WLAN/IPA MMIO mapping, SMMU mapping, or MSA mapping. Its generated Microkit
report places the Slint heap and Notes view around `0x8127d000`; the active MSA
pool remains a separate `no-map` interval in the merged DT. I added
[`audit-capdl-memory.py`](../platforms/sunfish/audit-capdl-memory.py), which
checks every generated CapDL page against fixed reserved-memory ranges.

The rebuilt system has five protection domains and 15 declared memory
regions, with no WLAN, IPA, or SMMU domain/region. The audit found 4,042 page
objects, 24 fixed reserved-memory intervals, 11 intentional splash-buffer
pages, and zero other overlaps. The base DT marks `0x97500000`/`0x180000` as
`no-map`; the runtime merged tree instead marks `0x98d00000`/`0x200000` as
`no-map`. Seven reusable or dynamically sized reserved-memory nodes have no
fixed `reg` address in the runtime FDT (`adsp_region`, `cdsp_region`,
`qseecom_ta_region`, `sp_region`, `secure_display_region`, `mem_dump_region`,
and `linux,cma`); the checker reports these separately because their extents
cannot yet be compared with current physical pages. Assigning those pools to
native firmware services needs an explicit allocator/ownership policy.

That inventory is now executable policy in
[`platforms/sunfish/wlan-resources.json`](../platforms/sunfish/wlan-resources.json).
[`validate-wlan-resources.py`](../platforms/sunfish/validate-wlan-resources.py)
checks the live merged FDT and generated kernel DT for the ICNSS MMIO and IOVA
tuples, stage-1-bypass marker, raw interrupt cells, SMP2P entries, regulator
references, voltage request, active fixed MSA reservation, and all seven
dynamic node sizes, alignments, and allocation ranges. Dynamic entries are
classified as inactive Linux allocations for this Microkit boot because their
DT nodes contain no physical `reg` range and no matching Linux service is
started. The validator cannot infer a physical extent from a `size` request;
before a corresponding device or firmware service is added, that service must
receive a concrete range excluded from ordinary allocation, with documented
ownership and reclaim rules. The manifest records observations and fails on
drift; it does not grant any device authority. The separate
`validate-smmu-handoff.py` check validates the recorded ABL SID collision
math and summarizes the S2CR route types and SCTLR.M state. It still cannot
read live SMMU state or establish that a slot or context bank is free.

The live ICNSS node's `reg-names` are `membase`, `smmu_iova_base`, and
`smmu_iova_ipa`, with values `0x18800000+0x800000`, `0xa0000000+0x10000000`,
and `0xb0000000+0x10000`. Only `membase` is physical device MMIO; the other
two are address-space ranges. It has twelve CE interrupt tuples, CE0–CE11,
using raw PDC cells `<0 0x19e 0>` through `<0 0x1a9 0>`; root's
`interrupt-parent` is the Qualcomm PDC, whose parent is the GIC. The DT also
declares SMP2P force-fatal and early-crash indications through two
`interrupts-extended` entries. Preserve this hierarchy and resolve actual
Microkit IRQs from the interrupt-controller data; do not use Linux virtual IRQ
numbers. I traced the matched Lineage irqchip: the PDC domain delegates
translation to its GIC parent, and the Sunfish PDC table has no PDC-pin entry
for these CE values. The cells therefore decode as GIC SPI numbers `0x19e`
through `0x1a9`, corresponding to hardware INTIDs/seL4 IRQ IDs `0x1be` through
`0x1c9` (GIC SPI INTID is SPI number plus 32). All fit the board's `MAX_IRQ
511`. The validator checks that decode against the manifest and seL4 board
configuration. This identifies the future CE IRQ IDs; the current image grants
none of them. The fatal/early-crash child specifiers are SMP2P entry-bit
numbers, not additional hardware IRQ IDs. Their parent is the modem SMP2P
doorbell on GIC SPI 451 (hardware/seL4 INTID 483); Linux's SMP2P driver reads
the shared-memory `wlan` entry and fans bits 0 and 1 out as nested interrupts.
A native implementation must put that shared memory and parent doorbell behind
a trusted modem-IPC/SMP2P broker, then expose only validated WLAN notifications.
The WLAN domain must not receive the shared parent IRQ or raw SMP2P memory.
The resource manifest records this boundary. No such broker is implemented,
so treat crash notifications as unavailable and keep firmware startup disabled
until the failure path can be handled safely.

The ICNSS IOMMU specifier's mask cell is `0x01`, so it matches both stream IDs
`0x240` and `0x241`; a probe that checks only `0x240` would miss a route
installed specifically for the adjacent alias. The read-only handoff probe now
reports both IDs. A future isolated WLAN domain must own and confine both IDs
before DMA is enabled.

The exact node references five regulator rails: `vdd-cx-mx`, `vdd-1.8-xo`,
`vdd-1.3-rfa`, and `vdd-3.3-ch0/ch1`; it also references the optional
`cxo_ref_clk_pin` clock. The matching ICNSS driver marks these supplies and
clock optional, defines the expected voltage rails, and reads the DT override
`qcom,vdd-cx-mx-config = <0x9c400 0x9c400>` (640,000 µV min/max). These still
need an explicit Microkit owner and power-sequencing implementation. The raw
CE-to-GIC decode and SMP2P parent/child topology are now source-backed; what
remains unresolved is who owns the PMIC/clock controls, how to implement the
SMP2P broker safely, and how to take over the shared SMMU without affecting
other masters.

The matched Linux ICNSS power-on path applies regulator requests in this order:
CX/MX at 640 mV min/max (the DT override; the driver default is 752 mV), 1.8 V
XO, 1.304 V RFA, then channel 1 and channel 0. The pinned `atoll.dtsi` source
declares 3.000–3.312 V ranges for the two 3.3 V rails, but both the captured
runtime DT and the generated seL4 DT omit those properties. The pinned ICNSS
driver at pinned commit `28f9290ae067d3e7857126eb6312720dbe9c7c80` uses its
3.312 V min/max defaults when the properties are absent, so that is the
source-derived request for these DTBs if the running kernel uses that driver
source. The running driver identity has not been independently verified.
ICNSS passes both values to `regulator_set_voltage(min, max)`; the RPMh VRM
rounds the minimum up to whole millivolts and checks it against the maximum.
These are source-derived requests, not measured rail voltages, and the
source/runtime DT mismatch is recorded explicitly. The driver enables the
optional reference clock after the regulator loop; the captured DT has no
`clock-names`, so that clock is absent in this configuration. Linux disables
rails in reverse order, but its error cleanup does not establish a reliable
rollback result. The native power
service should use an explicit per-rail state machine, unwind only successfully
enabled resources in reverse order, and treat any failed rollback as an
unknown power state that blocks firmware startup. These values and order are
observations from the matched driver, not a safe-to-copy direct PMIC
programming recipe. The DT resolves the five rails to RPMh VRM resources
`ldoa9`, `ldoc1`, `ldoc2`, `ldoc10`, and `ldoc11`, all through the `apps_rsc`
mailbox. That client uses the shared Apps RSC at `0x18220000` (with a second
register window at `0x18220d00`) and GIC SPI 5; the runtime DT calls it
`qcom,tcs-drv`. Thus the required authority is a trusted RPMh service with a
reviewed shared-resource policy, not direct SPMI register writes or direct
WLAN access to the RSC registers. These rails and the RSC are shared system
resources, so ownership and vote aggregation must be settled before WLAN can
control them. At the pinned source, RPMh separately max-aggregates each valid
register across active consumers and across sleep consumers. Upward active
requests wait for acknowledgement, with an additional wait when the requested
voltage ceiling drops below a previously requested value. A read of the RSC
registers cannot reconstruct the per-client vote ledger, so the native owner
must not change rails until a trusted service arbitrates every requester and
preserves their contributions. The existing one-shot RPMh probes are not that
broker. The resource validator checks the RSC windows, interrupt, driver ID,
and TCS configuration in both device trees, alongside the recorded vote
policy.

The manager boundary matters because the Apps SMMU is shared by GPU, camera,
display, and other masters. Giving its entire MMIO window to `sunfish_wifi`
would let that driver rewrite routes for unrelated devices. SCM needs the
same discipline: grant a capability only to a small broker implementing a
fixed, audited operation, never to Slint or app clients.

The first DMA gate has two outcomes. Prefer a translated SMMU context owned by
the native boot, with a small IOVA aperture and pinned ring/packet pages. Before
firmware starts, verify the programmed route and demonstrate that an unmapped
DMA address is blocked. If the boot chain or secure monitor cannot provide
that constrained route, stop before firmware startup and continue network
work on QEMU/UTM. Android's bypass configuration is not a temporary safe mode.

### DT overlay reconciliation

The earlier “captured live DT” description conflated the `boot_a` image's base
DTB with the final tree Android runs. The base file
`.cache/pixel-sunfish/live-a.dtb` has `wlan_msa_region@97500000` at
`0x97500000`/`0x180000`. At runtime, `ro.boot.dtbo_idx` is `0`; extracted DTBO
entry 0 contains the WLAN MSA overlay fragment at `0x98d00000`/`0x200000`, and
`/sys/firmware/fdt` reports that same address and size. The same selected
overlay contains fragments 42–46 for the modem, video, CDSP, ADSP, and WLAN
MSA pools. Its modem region is `[0x8b000000, 0x94800000)`, matching the
relocatable PIL allocation and load log; video, CDSP, ADSP, and WLAN MSA
follow contiguously. The base DTB and older cached `live-a.dts` retain the
pre-overlay extents and must not be used as the current physical map. Linux's ICNSS log
independently reports `MSA pa=0x98d00000` and size `0x200000`. The current
read-only capture can be repeated with
[`capture-live-fdt.py`](../platforms/sunfish/capture-live-fdt.py); its
`--root-read-only` option temporarily switches ADB to root for the protected
sysfs read and restores the normal shell user in a `finally` block. The
2026-10-10 capture SHA-256 is
`e371f54727a89c9c76abdc5fe4c23d93f6e24925770d4519fe705cfa7f35bd35`.
The resource validator passes against that capture and the generated Microkit
kernel DT. The active
reservation is therefore confirmed by the boot property, overlay contents,
final FDT, and driver behavior. The base DTB embedded in the old Microkit
build said `0x97500000`/`0x180000`. I changed
[`prepare-microkit-sources.py`](../platforms/sunfish/prepare-microkit-sources.py)
to use `.cache/pixel-sunfish/live-runtime.dtb`, regenerate its DTS on every
run, and replace old generated Sunfish board entries instead of appending
duplicates. The rebuilt debug SDK and Slint image use the merged runtime tree;
`fdtget` confirms the generated kernel DT now has the active
`0x98d00000`/`0x200000` range.

The current Slint CapDL report has no assigned page overlapping either
candidate MSA range; its explicit display buffers are in the separate
`0x9c000000` splash reservation. That narrow check does not make the DT audit
complete for WLAN use. Before adding WLAN, the build must preserve the full
reserved-memory map and mechanically verify every allocated physical page.
The Apps SMMU policy must also be independently established.

I mechanically checked the rebuilt `kernel.dtb` with `fdtget`: it retains the
runtime `0x98d00000`/`0x200000` WLAN reservation, the ICNSS IOMMU
specifier `<0x240 0x01>`, and `qcom,smmu-s1-bypass`. The matched Linux SMMU
parser interprets those cells as a stream ID and its mask. This verifies the
current build's DT contents; it does not reveal which context bank was active
on Android or the SMMU hardware state left by ABL, and does not prove safe DMA
isolation. `validate-wlan-resources.py` now checks the post-overlay `reg`
values and `no-map` state of all five adjacent PIL/WLAN reservations in both
the captured runtime DTB and generated Microkit DTB, and rejects overlap.

The first native milestone should be a source and resource audit, plus an
explicit SMMU/VMID ownership plan. Only after protected DMA and the secure MSA
ownership transition are demonstrated should a fail-closed firmware-start
probe attempt the WLAN-ready handshake and report it over UART. Do not start
with scan/connect or user data: those depend on successful platform startup, a
working firmware control transport, and DMA isolation.

### MSA ownership state machine (design only)

Before implementing a secure call, track the full reserved MSA pool separately
from the WLFW-reported memory segments. The pool moves through
`unknown-at-entry` → `reserved-and-excluded`; each QMI range then moves through
`reported-and-bounds-checked` → `assigned-to-required-VMIDs` →
`SMMU-policy-verified`. Firmware startup is permitted only when every required
range has a known owner and the active stream mapping is independently
verified. Advance ownership only after the exact SCM ABI and its success
semantics are established; an arbitrary non-error SMC result is not evidence.
If assignment or SMMU verification fails, keep WLAN DMA and firmware startup
disabled. If a transfer or rollback reports an ambiguous ownership failure,
mark the affected range unavailable to all ordinary services until its owner
is re-established; never return it to the general allocator. Reclaim is a
separate transition and requires firmware quiescence, DMA shutdown, and
verified reassignment. This state machine is not live device behavior. The pure
transaction resolver above tests only the partial-assignment and quarantine
policy; it does not establish the secure monitor's success contract or provide
an owner readback.

## Modem lifecycle findings from the matched Linux source

The read-only Lineage and Microkit probes established that SMEM host 0:1 is
empty in the standalone Microkit boot, while Lineage's `qcom_smem_get()` finds
the modem GLINK items after the Android modem stack is running. The matched
kernel source explains why those observations differ: modem availability is
created by an explicit subsystem lifecycle, not by mapping the SMEM reservation
alone.

For Sunfish, Atoll describes the modem as `qcom,pil-tz-generic`, PAS ID 4,
firmware base name `modem`, and a fixed modem memory reservation. The source
DTS places that region at `0x86000000`; the merged runtime DT moves it to
`0x8b000000`. The Linux peripheral loader requests the AOP `image/load_state`
`on` transition, obtains the modem MDT, performs the PAS metadata and memory
setup calls through SCM, loads the MDT segments, and finally asks SCM to
authenticate and release the modem from reset. Some Linux PIL devices also
assign and reclaim memory through a hypervisor VMID path; the matched Sunfish
modem node has no `qcom,mem-protect-id`, so that optional full-pool transfer is
not evidenced for this modem. The subsystem-specific driver also owns proxy and active regulator,
clock, and bus votes, stop/stop-ack signaling, watchdog/crash handling, and
shutdown. This is a secure boot and power-management protocol; the Linux
implementation is a reference, not a recipe to reproduce blindly in a
Microkit protection domain.

A scoped filename search of the local Sunfish backup/cache did not find a
standalone modem image, so I checked the running phone's firmware mount
read-only. With rooted ADB temporarily enabled, the device exposed
`/vendor/firmware_mnt/image/modem.mdt`, `modem.b00` through `modem.b30`, and
`modemr.jsn`; the separate `/vendor/firmware/modemuw.jsn` was also present.
The vendor fingerprint is
`google/sunfish/sunfish:13/TQ3A.230805.001.S1/10786265:user/release-keys`.
Treat it as context only: LineageOS fstab mounts a slot-selected modem
partition separately at `/vendor/firmware_mnt`, so the vendor fingerprint does
not identify that partition revision ([Sunfish fstab](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/fstab.hardware)).
I then read only the text file `/vendor/firmware_mnt/verinfo/ver_info.txt`
over temporary rooted ADB. It reports modem image build
`MPSS.AT.4.4.c3-00112-SM7150_GENP_PACK-1`, metabuild
`SM7150.LA.3.1.c9.1-00003-STD.PROD-1`, and timestamp `2023-05-05 16:10:05`.
This is useful version context, not a cryptographic attestation of the
slot-selected modem partition or of the firmware bytes. ADB was returned to
unrooted shell mode immediately afterward.
The mount is read-only. With Rooted debugging temporarily enabled, I hashed
and size-checked the MDT, all 31 segment paths, and both service manifests in
place, without copying their contents; ADB was then returned to unrooted shell
mode. The complete per-segment inventory and its canonical digest are recorded
in [`wlan-resources.json`](../platforms/sunfish/wlan-resources.json). Three
segment paths (`b13`, `b18`, and `b30`) are zero-length. This local inventory
identifies the installed bytes but is not a vendor signature or firmware
authentication proof. LineageOS describes its device-tree `proprietary-files`
list and optional SHA-1 values as extraction pins; that is a reproducibility
mechanism, not evidence of Qualcomm PAS policy or a signature on this inventory
([LineageOS blob workflow](https://github.com/LineageOS/lineage_wiki/blob/main/pages/internal/working_with_blobs.md)).
The MDT is 9,100 bytes (SHA-256
`43a1414628153c109f06fe70385946c1c530a4ee08840b40f62649c9fa589870`), and
`modemr.jsn` is 562 bytes (SHA-256
`d9fb1cfc1804923699b971d0e04d858103ec71ff528d9fd151e4725e0f354eb7`). The
two service manifests describe modem `root_pd` services and `wlan_pd` services,
respectively; these declarations do not establish that either service is
currently running or that Microkit may take ownership.

I streamed the MDT through a host-side ELF-header parser without saving its
contents. It is little-endian ELF32 for machine 164 (Qualcomm Hexagon), with 31
program headers and 29 loadable segments. Every loadable segment has the
Qualcomm relocatable flag. Its linked-address extent is
`[0x8b000000, 0x94800000)` (0x09800000 bytes); that is not by itself proof of
the final destination. The matched PIL loader allocates a relocatable region
with `dma_alloc_attrs()`. The current Lineage boot log reports
`[0x8b000000, 0x94800000)`, and a fresh read of `/sys/firmware/fdt` confirms
that the live `pil_modem_mem` reservation is exactly that range (0x09800000
bytes). The neighboring live reservations now begin at the modem end:
`pil_video_mem` is `[0x94800000, 0x94d00000)`, `pil_cdsp_mem` is
`[0x94d00000, 0x96b00000)`, and `pil_adsp_mem` is `[0x96b00000, 0x98d00000)`.
There is no overlap in this fresh FDT. The older cached
`.cache/pixel-sunfish/live-a.dts` had pre-overlay sizes and is stale for this
comparison; do not use it as the current runtime map.

The allocation-free parser in [`rust/src/qcom_mdt.rs`](../rust/src/qcom_mdt.rs)
now validates ELF32/Hexagon identity, bounded program-header tables, loadable
segment sizes, overflow-safe page-rounded extent, and fit within a caller's
fixed pool. On 2026-10-11, the MDT was streamed directly from the phone through
this parser with temporary rooted ADB: it accepted 31 program headers, found
29 relocatable loadable segments, and confirmed the `[0x8b000000, 0x94800000)`
extent fits the matching `0x09800000` modem reservation. It also compared the
sizes of all 26 nonempty segment files with their program-header `p_filesz`;
all matched. Only the 9.1 KiB MDT metadata was held in host memory for parsing;
the segment files were stat'ed without reading their contents, and no firmware
was saved on the host. ADB was returned to the normal shell afterward. This
validation adds no segment loading, memory mapping, SCM, or reset operation.

The matched `drivers/base/dma-removed.c` explains the allocator: the
`removed-dma-pool` reserved-memory handler attaches a per-pool bitmap and
`removed_dma_ops` to the modem device. Its allocator returns the pool’s
physical base plus an allocated page offset, so the logged address is the
reserved physical extent in this case, not an unrelated SMMU IOVA or a CMA
allocation. The live FDT and kernel log now reconcile exactly. No modem
firmware or segment blob was copied to the host or loaded by Microkit. Rooted
ADB was returned to normal shell mode after collecting these read-only facts.

The AP-side GLINK edge follows the modem subsystem notifier. Linux registers
the SMEM transport on `SUBSYS_AFTER_POWERUP` and unregisters it on
`SUBSYS_AFTER_SHUTDOWN`. Its Atoll node uses remote PID 1, SMEM transport,
APCS mailbox channel 12, and GIC SPI 449 (seL4 INTID 481); the same edge carries
the `IPCRTR` channel used by QRTR. On registration, the matched GLINK backend
allocates or reuses descriptor item 478 and TX FIFO item 479, reads their
metadata, then writes zero to the AP-owned TX head and RX tail indices before
starting GLINK negotiation. Therefore even Linux's ordinary “register the
transport” path mutates shared state and can discard queued data. A Microkit
reader must not attach to an already-live ring or initialize indices based
only on a plausible descriptor, a successful SMEM lookup, or an SCM return.

The current read-only ADB check confirms that LineageOS 23.2 (kernel
`4.14.357-openela-g28f9290ae067`) has modem `IPCRTR`, `DS`, and `glink_ssr`
RPMsg devices registered; `IPCRTR` is bound to `qcom_smd_qrtr`. A repeat on
2026-10-11 also found `modem.glink_ssr` bound to `glink_ssr`, the platform
ICNSS device bound to its driver, `wlan` and `wglink_dlkm` loaded, and the
`modem_svc`/`wificond` Android services running. This confirms the AP-side
services exist under Linux today. It says nothing about whether ABL provides
them to a standalone Microkit boot, because Linux's modem subsystem and GLINK
registration sequence have run in the Lineage boot.

This gives us a concrete boundary for the next milestone:

1. Keep modem boot, SCM/PAS calls, AOP messages, regulators, clocks, bus votes,
   and SMP2P stop/crash signaling out of the WLAN domain. Any future lifecycle
   owner must be a separately reviewed trusted service with explicit authority.
2. Keep the GLINK broker disabled until it can prove exclusive AP ownership
   and a fresh/reset modem peer state. Only then may it initialize AP-owned
   ring indices and begin VERSION negotiation. On ambiguous state it remains
   read-only/disabled.
3. The source-backed transition checklist is documented above, and the
   offline `GlinkTransportGate` requires exclusive AP ownership and a verified
   fresh/reset peer before index initialization. Its booleans represent test
   claims; it cannot establish either fact on hardware. The read-only
   Microkit boot-state probes are complete: host 0:1 has no GLINK items
   478–480 or SMP2P items 435/428 in standalone Microkit boots, while Lineage
   allocates and registers its own AP-side transport after modem power-up.
   Therefore, do not repeat directory scans or attempt VERSION negotiation.
   The next host-side task is a modem-lifecycle feasibility audit: identify a
   trusted owner for firmware authentication/load, AOP power sequencing,
   AFTER_POWERUP/AFTER_SHUTDOWN events, and SMP2P before designing live
   transport. If that authority cannot be established from the boot chain,
   Wi-Fi remains blocked on this platform without adding the modem bootstrap
   path. Any later hardware probe must stay read-only until a reviewed
   reset/ownership service can prove a fresh peer.

### Modem-bootstrap feasibility audit

The matched source audit shows there is no small “kick modem” operation that
can safely precede GLINK. Linux's modem subsystem performs a coordinated
sequence: acquire AOP proxy votes and required clocks/regulators, initialize
the PAS image with metadata, set up the modem memory reservation, load all
MDT segments, authenticate and release the processor from reset through SCM,
then publish `SUBSYS_AFTER_POWERUP`. The MSS layer also owns stop-ack, watchdog,
crash/restart, and resource-unvote paths. ICNSS waits for its modem-ready
notifier before continuing; only then does GLINK register the SMEM edge, reset
AP-owned ring indices, and negotiate VERSION. These are distinct ownership
boundaries, not one WLAN-driver responsibility.

| Prerequisite | Lineage source/observation | NexatrodeOS status |
| --- | --- | --- |
| Modem firmware source | MDT and segment files are mounted under the vendor firmware path during Lineage; the MDT was header-parsed without saving firmware | No packaged loader input or protected firmware-reader service |
| Secure processor boot | PAS init, memory setup, segment authentication, and reset release use Qualcomm SCM | No reviewed Microkit ABI, no SCM call made |
| Power and clocks | `load_state on/off` is only an AOP mailbox notification; separate proxy votes use `vdd_cx`, `vdd_mss`, `xo`, a modem bus vote, a proxy-unvote interrupt, and a 10 s fallback | Native QMP link/open/AP-close is observed; fixed OFF packet delivery is a separate candidate. No native RPMh proxy-vote or complete power service |
| Lifecycle and failure handling | Linux publishes AFTER_POWERUP/AFTER_SHUTDOWN and handles stop-ack, watchdog, and restart | No trusted Microkit modem lifecycle manager |
| GLINK/QRTR start | Linux allocates SMEM items after AFTER_POWERUP; standalone Microkit found none | Transport remains disabled; no ring writes or VERSION negotiation |

The proposed boundary and state machine are now written in
[`pixel-modem-lifecycle-design.md`](pixel-modem-lifecycle-design.md), with an
offline policy model in
[`modem_lifecycle_model.py`](../platforms/sunfish/modem_lifecycle_model.py).
The model checks ordering and fail-closed behavior; it does not establish
hardware evidence. The Linux-side PAS/SCM marshalling and AOP/proxy-vote
sequence are now traced in the lifecycle design. The remaining engineering
gates are the secure firmware's accepted ABI and ownership contract, an
immutable compatible firmware source, and a concrete reserved-memory
ownership plan. Local hashes do not replace PAS authentication; no
authenticated project manifest exists yet.
No secure call, power request, firmware load, reset, or live transport is
enabled by this design.

Source locations for this trace are the matched Lineage kernel's
`drivers/soc/qcom/peripheral-loader.c` (`pil_boot()` / `pil_shutdown()`),
`drivers/soc/qcom/subsys-pil-tz.c` (PAS/SCM and proxy votes),
`drivers/soc/qcom/pil-q6v5-mss.c` (modem stop/power callbacks),
`drivers/soc/qcom/glink_probe.c` (SSR registration),
`drivers/rpmsg/qcom_glink_smem.c` (items and ring-index writes), and
`arch/arm64/boot/dts/qcom/atoll.dtsi` (modem and GLINK nodes).

## Recommended next gate

The Linux route, SMMU context, and WLFW MSA readbacks are complete. The
Microkit build now uses the merged runtime DT and its Slint image builds with
the active MSA reservation excluded. These results do not establish that
native Microkit can safely own or configure the shared SMMU. Do not start WLAN
firmware. The next engineering sequence is:

1. A complete build from the local integrated source reproduces the installed
   `wlan.ko` byte-for-byte, and an INFO diagnostic run observed zero dynamic WMI
   host-memory requests. The original build's precise upstream commit history
   is not cryptographically recorded, and 82 public Android 13 Sunfish
   prebuilt tags contain no byte-identical module. Use the matched source for
   Linux behavior, not as a drop-in Microkit driver. This result removes the
   need to guess WMI-requested dynamic chunk sizes; it does not clear the MSA,
   firmware-ownership, or DMA-isolation gates.
2. The CE PDC tuples are now decoded to seL4 IRQ IDs, and the SMP2P chain is
   resolved to its shared modem doorbell (INTID 483) plus two child entry bits.
   The offline GLINK/QRTR/QMI codec now has bounded framing and reassembly
   tests, and offline broker/queue models cover endpoint teardown, one
   in-flight transaction, timeout without automatic retry, service-loss
   cancellation, bounded response sizes, queue overflow, and stale sessions.
   The 2026-10-10 temporary-boot probe now passes protected IPC, broker-owned
   shared-page visibility, bounded queue overflow/FIFO/wraparound, and explicit
   stale-session invalidation. A later temporary-boot probe displayed
   `BROKER RECOVERY PASS` after a child fault, supervisor session rollover,
   stale-queue scrub, and child restart; the child also confirmed its PD-local
   state persisted across restart. This demonstrates why the supervisor must
   own the external generation and clear stale shared state. Before any live
   transport, exercise modem service-loss/recreation and establish a fresh
   peer handshake. Policy must not
   receive direct modem access. Hardware work still requires trusted evidence
   of exclusive modem ownership and a fresh peer before any SMEM initialization;
   separate services must own RPMh power votes and SMP2P notifications. Linux
   dynamic allocation requests also need explicit physical ranges or must
   remain inactive. The manifest grants no IRQ, shared memory, mailbox, SCM,
   or power-control authority.
3. The expanded runtime readback shows the six ABL entries are not the full
   Linux route table: USB and display persist or change banks, and WLAN/IPA
   routes appear after driver attach. `SCTLR.M=0` throughout, confirming this
   is not a translated-DMA model to copy. There is now a stronger reason to
   keep this as a hard gate: Qualcomm's public Snapdragon access-control guide
   says its example hypervisor controls non-secure stream-to-context-bank
   mappings and stage-2 contexts, while the CPU OS configures only delegated
   stage-1 contexts. The same guide explicitly says SoC trust architectures
   differ, so this is a risk model, not proof of Sunfish's exact policy.
   Sunfish's matched vendor DT marks the Apps SMMU `qcom,skip-init`; the
   matched Linux driver's corresponding branch skips resetting stream-match
   groups and context banks (it still updates fault/TLB/control registers).
   This is a Linux handoff-preservation setting, not a delegation to a later
   EL1 payload. The local Sunfish kernel config enables `CONFIG_ARM_SMMU` but
   does not enable the optional `CONFIG_MSM_TZ_SMMU` path. That path's source
   contains SCM helpers for ATOS coordination and page-table format, plus a
   write guard for selected SMMU registers when enabled; it does not expose a
   WLAN stream-route allocation/transfer API. The separate `qcom_scm_assign_mem`
   calls in ath10k/QMI transfer memory-region permissions and likewise do not
   allocate a stream match or context bank. Neither code path is evidence that
   this phone exposes a safe route-delegation interface. No matching
   Sunfish-specific secure handoff API or ownership contract has been found.
   Therefore neither readable registers nor Linux's successful operation
   proves that our EL1 Microkit payload may change SMRs/S2CRs or context-bank
   routing. Establish that exact authority and the other DMA protections
   before designing a manager. Do not reuse Linux banks or treat its bypass
   policy as DMA isolation.
   A read-only extraction of the active Pixel `hyp_a` partition strengthens
   this from a generic Qualcomm concern to a device-specific constraint. The
   ELF contains QHEE diagnostics for `/dev/smmu_V1`, illegal SMMU register
   writes, invalid SID/S2CR/SMR/CBAR values keyed by device ID, and an internal
   `SMMU_DynamicCfgSyscall` mode-switch path. Additional strings name
   `SMMUSecureModeSwitch`, transitions that add/remove SIDs from stage 2, and
   a failure for non-secure SIDs when dynamic route space is insufficient.
   The local string inventory also reports rejected HLOS SID-list buffers,
   missing context-bank indices, unexpected VMIDs, and rollback after a
   TrustZone failure. This indicates that the internal path has ownership,
   buffer-validation, and rollback checks; it does not identify the accepted
   syscall number, argument layout, device-ID policy, or an EL1/Microkit
   caller contract.
   These are diagnostic strings, not a callable ABI. They strengthen the
   evidence that this installed EL2 image virtualizes/checks at least some
   SMMU configuration; they do **not** establish which WLAN IDs it accepts,
   whether Microkit may request a transition, or the call ABI. The Android
   live FDT has no `/ac/smmu` node; that only describes the OS-visible tree and
   says nothing conclusive about QHEE's private configuration. The active-slot
   image was read only on
   2026-10-12 (540,672 bytes; SHA-256
   `aff7e8ac1433f9f2a01f975d9e5f6b206d0a57bb8c0de8f881d7fe7ec80553bd`).
   Treat this hash and extracted strings as local forensic evidence, not as a
   firmware artifact to redistribute. Public Lineage sources expose no
   documented Sunfish WLAN route-transfer call, so do not probe undocumented
   SMC/HVC numbers or try direct SMMU writes. The next safe path is to find a
   source-backed, supported QHEE route API or prove that the booted payload has
   explicit EL2/SMMU delegation; if neither exists, this boot chain cannot
   safely host native WLAN DMA without a different hypervisor/firmware design.
4. Review the captured WLFW segment ranges and VMID permissions against the
   secure monitor ABI, cache rules, and failure/rollback behavior before
   implementing any state-changing SCM call. Do not assign memory, change
   SMMU routes, or start WLAN firmware until the native owner and DMA policy
   are verified. The read-only `IS_CALL_AVAIL` query has been completed:
   `ASSIGN_MEM` and RTIC report available, while the matched kernel's ATOS and
   SMMU page-table-format calls report unavailable. The secure IOMMU page-table
   size and initialization queries report available. On the running Lineage
   kernel, `/proc/config.gz` reports `CONFIG_ARM_SMMU=y` and no
   `CONFIG_QCOM_IOMMU` or `CONFIG_MSM_TZ_SMMU`. In the matched source, the
   secure page-table size/init calls are used by the separate legacy
   `qcom,msm-iommu-v1` driver when it finds a secure context; Sunfish's WLAN
   uses the Apps `qcom,qsmmu-v500` ARM SMMU path instead. Therefore these YES
   results are not evidence of an active Sunfish secure-page-table path or a
   WLAN stream-route API. They are not authorization to call `ASSIGN_MEM` or
   either page-table operation.
   A source audit of the additional matched-kernel calls found `RESTORE_SEC_CFG`
   and `IOMMU_SECURE_MAP2_FLAT` / `IOMMU_SECURE_UNMAP2_FLAT`. The map/unmap
   pair is consumed by `io-pgtable-msm-secure.c` to edit TrustZone-owned secure
   IOMMU page tables; `RESTORE_SEC_CFG` is the secure-IOMMU configuration
   restore helper. None programs the Apps SMMU's non-secure stream-ID to
   context-bank routes used by WLAN. A temporary probe queried only
   `IS_CALL_AVAIL` for these three documented calls, alongside the earlier
   availability checks. On 2026-10-12, the Pixel displayed `RESTORE SEC CFG
   YES`, `SEC MAP2 NO`, and `SEC UNMAP2 NO`; it did not call any of these
   operations. No supported WLAN route-transfer call surfaced in the matched
   source audit or these results.
5. After those gates pass, implement a narrow manager and test translated DMA
   confinement with a controlled fault case; then build a UART-only
   firmware-ready probe.
6. Add scan/connect, authentication, and sDDF packet integration after the
   firmware-ready probe and isolation checks pass. The Linux driver VM remains
   a separate fallback requiring EL2/VMM feasibility work. The Pixel's
   temporary Image-shaped boot probe measured `CurrentEL=EL1` with the MMU
   off at the ABL handoff (`docs/pixel-p0-boot-feasibility.md`). That is the
   actual entry point used by the current Microkit port. A Linux driver VM
   therefore cannot simply be added to this image: seL4's ARM virtualization
   support requires the seL4 kernel to run at EL2, and the current boot path
   does not hand EL2 to it. A future VM experiment needs a documented boot
   chain that starts seL4 at EL2, plus a supported SMMU/VMID ownership
   contract for WLAN; EL2 access alone would not establish safe device
   assignment. See the official seL4 [virtualization FAQ](https://docs.sel4.systems/projects/sel4/frequently-asked-questions)
   and [verified configurations](https://docs.sel4.systems/projects/sel4/verified-configurations.html).
   A read-only check of the connected LineageOS kernel on 2026-10-12 also
   found no `/dev/kvm` and no `CONFIG_KVM` entry in `/proc/config.gz`, so the
   current Android kernel is not an available VM host either. This does not
   test or rule out seL4's own EL2 VMM path; it confirms that a guest-driver
   experiment needs a new EL2 boot design rather than a VM launched from the
   running Android system.

The diagnostic kernel was used only with `fastboot boot`; no partition was
flashed. The Pixel was rebooted back to LineageOS after the readback.

### Read-only SCM capability query

The running matched Lineage kernel's `scm_is_call_available()` path was
inspected. A matched-kernel diagnostic module queried
`SCM_SVC_MP/ASSIGN_MEM` and `SCM_SVC_RTIC/RTIC_MEM_PROTECTION`; both were
advertised as available (`1`). The module was then removed and ADB returned to
the ordinary shell user. This says only that the secure monitor recognizes
those commands. It does not prove that a Microkit caller has safe authority to
use them or that they accept the WLAN's desired memory ownership.

A separate Microkit image issued only Qualcomm's `IS_CALL_AVAIL` read-only
service for those two commands and painted the returned availability and call
status on the reserved splash framebuffer. On 2026-10-12, the Pixel showed
`MP ASSIGN YES` and `RTIC YES`; the probe displays `YES` only when the SMC
transport status is zero and the result register is one. This confirms the
Microkit-side SMC ABI/result interpretation for these capability queries. The
image had one SMC-capable PD, no WLAN/SMMU MMIO mapping, no SCM
memory-assignment call, and no automatic reset; it was used with `fastboot
boot` only. A successful query is not permission to call `ASSIGN_MEM`; that
still requires a source-backed WLAN ownership contract. A follow-on
read-only probe checked availability of the matched kernel's documented
SMMU page-table-format and ATOS-preparation calls; neither is a WLAN route
ownership API. On 2026-10-12 the Pixel displayed `MP ATOS NO` and `SMMU PT
NO`, while `MP ASSIGN YES` and `RTIC YES` remained available. This confirms
only the `IS_CALL_AVAIL` results, not the behavior of those disabled
operations. A follow-on temporary probe also queried MP secure IOMMU page-
table size/init availability (functions 3 and 4) without calling either
operation; the Pixel showed `SEC PTBL SIZE YES` and `SEC PTBL INIT YES`. A
read-only runtime config check showed
`CONFIG_ARM_SMMU=y` with no `CONFIG_QCOM_IOMMU` or `CONFIG_MSM_TZ_SMMU`; the
matched `qcom_iommu.c` driver binds `qcom,msm-iommu-v1`, while Sunfish's Apps
SMMU is the separate `qcom,qsmmu-v500` device. The advertised secure-page-table
functions therefore do not unblock WLAN route ownership on this boot.

### EL2/KVM boot-path check

The Qualcomm public Linux boot guide describes a KVM mode in which a UEFI
bootloader shuts down the Qualcomm Hypervisor and hands EL2 to Linux KVM. This
is a useful example that some Qualcomm platforms can offer a deliberate EL2
handoff, but it does not document the Pixel 4a boot chain: Sunfish uses Google's
ABL/Android boot-image flow, and our temporary Image-shaped payload was
measured entering at EL1 with the MMU off. No Sunfish source or published
bootloader interface found so far enables the Qualcomm Linux guide's UEFI KVM
mode or transfers QHEE/SMMU ownership to seL4. Therefore this is not currently
a usable workaround for the driver-VM route. Do not infer an EL2 handoff from
generic Qualcomm documentation, and do not try undocumented HVC/SMC calls.

The connected LineageOS device was checked read-only again on 2026-10-12: it
reported bootloader `s5-0.5-10252351` and active slot `_a`. The Android-side
checks recorded above found no `/dev/kvm` or `CONFIG_KVM`; these facts do not
determine what ABL or QHEE could support in a separately booted image. The
measured EL1 handoff remains the relevant evidence for the current Microkit
image. The driver-VM route stays blocked on a documented Sunfish EL2 entry and
device/SMMU delegation contract.

An exact-version source audit on 2026-10-21 rules out treating the EL2 route as
a board-config toggle. [`platforms/sunfish/sel4-config.cmake`](../platforms/sunfish/sel4-config.cmake)
and the generated Sunfish `build_sdk.py` entry both set
`KernelArmHypervisorSupport` to false. In the pinned Microkit 2.3.1 loader,
`ensure_correct_el()` accepts EL2 for a hypervisor build and reports an error
when the incoming level is EL1; its only EL3 transition is a drop to EL2. It
has no EL1-to-EL2 path. The loader's own manual documents this entry-level
requirement for the ZynqMP/U-Boot example as well. seL4 16.0 supports the
hypervisor configuration for Cortex-A55, so an EL2 kernel build is technically
available, but it cannot run through this Sunfish boot path without a supported
firmware handoff that enters at EL2. Even a valid EL2 entry would still need
documented QHEE/SMMU ownership transfer; it would not by itself authorize WLAN
DMA. Exact source checkouts: Microkit 2.3.1 commit
`ec86afdcd662b5976d11d4994acf1b11a2979882` and seL4 16.0.0 commit
`6e7c3b733d296cfd88d5fbf635c96e447a882374`; relevant files are
`.cache/microkit-src-2.3.1-sunfish/loader/src/aarch64/el.c`,
`.cache/microkit-src-2.3.1-sunfish/docs/manual.md`, and
`.cache/sel4-src-16.0-sunfish/src/arch/arm/config.cmake`.

The Qualcomm guide's KVM-mode description is platform-specific context, not a
Sunfish implementation reference: [Qualcomm Linux APSS boot flow](https://docs.qualcomm.com/bundle/publicresource/topics/80-70029-4/bootloader-architecture.html).

### No-radio target-link probe (2026-10-12)

The Rust WLAN protocol library was linked into a minimal Microkit protection
domain and temporarily booted with `fastboot boot`. On the Pixel, the screen
reported `WLAN PROTOCOL`, `TARGET LINK CHECK`, `CE DESC PASS`, `QRTR PASS`, and
`NO RADIO ACCESS`. This verifies target linking and execution of one WCN3990 CE
descriptor encoding check and one WLFW QRTR lookup encoding check. It does not
test the radio, firmware, modem transport, SMMU, or DMA. The protection domain
maps only the reserved splash framebuffer; a host-side regression test now
guards that it receives no SMC, IRQ, or IPC channel grants. The Rust library's
150 host unit tests and the 101 Sunfish host tests pass, and the CapDL/reserved-
memory audit reports zero overlaps.

The immediate hardware gate is unchanged: there is still no source-backed
handoff API for exclusive ownership of WLAN stream IDs `0x240` and `0x241` and
a dedicated translated DMA context. The read-only secure page-table map and
unmap availability queries both returned unavailable; size/init availability
does not create an Apps-SMMU routing interface. Do not load firmware or touch
WLAN registers until a documented secure-firmware ownership contract and a
negative DMA-fault test exist. No partition was flashed.

### Re-check of seL4's generic SMMU interface (2026-10-12)

The cached Microkit SDK contains generated ARM-SMMU capability declarations,
but the pinned Sunfish debug configuration sets both `CONFIG_ARM_SMMU` and
`CONFIG_SMMU_INTERRUPT_ENABLE` to false. Those headers are generated for the
SDK's broader set of configurations; their presence does not mean the running
Sunfish kernel has SID/context-bank capabilities.

The pinned seL4 16.0 source confirms why the feature is disabled here:
`KernelArmSMMU` depends on `KernelPlatformTx2`, the SMMUv2 header hardcodes the
controller base at `0x12000000`, and the TX2 platform supplies the SMMU header
and SID/context-bank limits. Sunfish's Apps SMMU is the distinct controller at
`0x15000000`. More importantly, the generic boot initializer resets shared
stream routes and context banks, so removing the build dependency or changing
the base address alone would be unsafe on this phone. The local Sunfish board
configuration deliberately does not enable that code or grant WLAN an SMMU
capability.

This confirms a possible *implementation reference*, not a usable Pixel
driver: a Sunfish port would need a controller-specific register/layout
implementation, non-destructive initialization that preserves unrelated
owners, a supported ownership-transfer contract from QHEE/ABL, and Microkit
CapDL support for ARM SID/context-bank objects and DMA mappings. Until those
pieces are source-backed and tested, the correct next work remains the
boot-chain/SMMU delegation gate; no further Pixel interaction is needed now.

The read-only Pixel register capture also narrows the driver port. The photo
readback shows `ID0=0x4c017e4f` and `ID1=0x6000003d`. Decoding those standard
SMMUv2 fields with the matched Linux [register definitions](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu-regs.h) gives stream-match
mode enabled, 79 SMR/S2CR slots, a 15-bit SID space, 4-KiB register pages, and
61 context banks. The derived context-bank window begins at `0x80000` and
fits within the mapped 1-MiB Apps-SMMU window. This supports using the standard
SMMUv2 register model for the basic route/context-bank layout; it does not
prove QHEE permits EL1 writes or establish the Qualcomm TBU/power requirements.

There is also a concrete seL4 object-model mismatch beyond the controller
base address. The pinned seL4 [`smmu_sid_bind_cb()` implementation](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/src/drivers/smmu/smmuv2.c)
uses the SID itself as the SMR/S2CR register index, assumes one SID maps
directly to one slot, and the TX2 platform configures [`MAX_SID=128`](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/src/plat/tx2/config.cmake). Pixel WLAN SIDs `0x240`/`0x241` are
576/577, while this hardware exposes only 79 stream-match slots. Simply raising
`MAX_SID` or changing the MMIO base would still write outside the SMR/S2CR
table. A Sunfish port needs a capability design that preserves the full SID
value but allocates a separately validated free match slot, along with
ownership-aware teardown and alias-mask handling. The host handoff planner
now returns explicit `SID -> match-slot` bindings: the `0x240/0x241` alias
pair maps to one masked entry in slot 6 in its test fixture, rather than
treating either SID as a register index. This is still a planning model, not
kernel code, and is not wired into seL4's objects or Microkit's CapDL generator.

The Rust library now also has a no-std, host-testable route-planning primitive
in `rust/src/sunfish_smmu.rs`. It keeps the WLAN SIDs separate from the bounded
79-entry SMR table, validates that the `0x240/0x241` alias pair is exactly the
masked match, rejects overlap with observed routes, and requires explicit
ownership-transfer and EL1-control assertions plus verified free slot/context
bank inputs. It only returns a plan: it does not map registers, allocate DMA,
write the controller, or establish that those assertions are true. Host tests
cover the high-SID/low-slot case, collision and bound rejection, and both
ownership gates. This is reusable planning logic for a later driver; it is not
yet seL4 capability or CapDL integration.

The Rust model now also exercises the capability lifetime rule for this alias:
both SID caps must bind to the same context bank, duplicate/conflicting binds
are rejected, and releasing either SID leaves the shared route referenced
until the final SID cap is unbound. The final-unbind result is only a proposed
teardown action; it does not authorize a register write. This covers the
bookkeeping edge case that the current one-SID/one-register-index kernel path
does not represent.

The Microkit boundary is a second independent integration gap. In the pinned
Microkit source, `IommuDeviceIdentifier` only accepts `X86Pci`; AArch64 SDFs
reject a `peripheral_id`, and the CapDL `create_iospace()` path assumes the
x86 PCI IOSpace object. Its source comments explicitly leave ARM SMMU support
for future work. So even after a Sunfish kernel controller/capability port,
Microkit still needs an ARM-specific SDF resource description and CapDL object
generation path; reusing the x86 IOSpace syntax would misrepresent stream
matching and secure ownership.

The seL4 cap representation adds another bound: the SID field is 12 bits and
can encode the two WLAN IDs, but the SID-cap backing CNode is currently only
8-bit indexed. A Sunfish build would need a bounded SID-cap table large enough
for the supported SID range, while keeping the independent match-slot table at
79 entries. The existing API's SID-indexed hardware writes and automatic
unbind behavior also need replacement with slot-aware ownership bookkeeping.
These are kernel/object-model tasks, distinct from the tested route planner.

The existing syscall boundary is equally narrow: `GetSID` accepts only the
SID, and `BindCB` later calls a helper that writes that SID directly into
`S2CRn(sid)` and `SMRn(sid)` with an exact-ID, zero-mask match. A real Sunfish
interface therefore needs an explicit stream-match policy (SID plus mask), a
separate allocated slot, and a recorded slot association for revoke/unbind.
The policy must reject overlapping masks and only share a route when all
matching SID caps target the same context bank. Until the kernel and CapDL
implement those semantics, the current Rust state model is only a specification
and the Pixel kernel remains SMMU-disabled.

There is also a boot-time integration hazard in the generic seL4 implementation.
`init_plat()` calls `plat_smmu_init()` early in `boot.c`; later, `init_smmu()`
calls it again before exposing the SID/CB control caps. In the pinned TX2
driver, that initializer probes the controller, maps its register windows,
then runs `smmu_dev_reset()`: it invalidates every SMR, rewrites every S2CR,
disables every context bank, invalidates TLBs, and enables global SMMU control.
It is therefore unsafe to adapt by changing only the base address or platform
dependency. Sunfish now explicitly sets `KernelArmSMMU OFF` in its platform
config, with a host regression test guarding that setting. A future port needs
a separate non-destructive initialization path and one-time ownership gate
before it can expose any SMMU cap.

The matched Linux driver's `arm_smmu_find_sme()` is a useful algorithmic
reference: it searches the bounded hardware slot table, reuses only a
containing match, and rejects any partial overlap before claiming a free slot.
That logic is performed while Linux owns the controller; it is not a handoff
or permission mechanism and cannot be copied as authority evidence.

### Live Linux IOMMU-group observation (2026-10-12)

The connected LineageOS boot exposes the Apps SMMU at
`/sys/bus/platform/devices/15000000.apps-smmu`, bound to the `arm-smmu`
platform driver. The ICNSS WLAN owner at `18800000.qcom,icnss` is attached to
Linux IOMMU group 7; the separate `soc:ipa_smmu_wlan` device, bound to the IPA
driver, is attached to group 3. Those groups list only their respective device
nodes in the accessible sysfs view. The ADB shell is unprivileged (`uid=2000`),
and the kernel exposes no readable SMMU debugfs directory there, so the device
tree `iommus` properties and live route registers were not readable in this
check.

This confirms the Linux driver model treats ICNSS and IPA WLAN as separate
IOMMU clients, which supports keeping IPA acceleration out of the first Wi-Fi
path. It does not establish that seL4 may take either stream, that group 7
corresponds only to SIDs `0x240/0x241`, or that secure firmware delegates the
Apps SMMU to EL1. Keep WLAN DMA disabled until those ownership and route facts
are proven through an authorized handoff.

A follow-up rooted-ADB capture on 2026-10-12 read the live device-tree
specifier cells directly. The ICNSS node's `iommus` property is
`<0x30 0x240 0x1>`; the three IPA child nodes point to the same Apps-SMMU
phandle and request `<0x520 0>`, `<0x521 0>`, and `<0x522 0>` respectively.
Thus the device-tree mask for ICNSS includes the `0x240/0x241` alias pair,
while IPA WLAN is a separate exact-match client. The live IOMMU groups remain
7 for ICNSS, 3 for IPA WLAN, 2 for IPA AP, and 4 for IPA UC. Each group's
readable `reserved_regions` file contained only the generic `0x08000000`–
`0x080fffff` MSI window. That list is not the SMMU route or secure ownership
table. The two IPA `qcom,additional-mapping` properties decode as three-cell
`(IOVA, physical address, size)` tuples in the matched IPA driver; the WLAN
tuple is `(0x01e60000, 0x01e60000, 0x00080000)`. Because the IPA driver maps it
with read/write/MMIO permissions and IPA remains outside the first Wi-Fi
scope, this tuple must not be copied into the WLAN domain.

Rooted ADB could read those device-tree properties, but the Apps-SMMU
provider's protected device-tree node still denied reads under enforcing
SELinux, `/sys/kernel/debug` does not exist, and the kernel has no debugfs
support or SMMU register sysfs interface. The live root capture therefore
adds no current SMR/S2CR/context-bank readback and no secure-firmware
delegation evidence. It also confirms the running image is a mixed Android
system/vendor build (`ro.lineage.version=23.2-20261001-NIGHTLY-sunfish`,
system release 16, vendor fingerprint Android 13); this is the same system / vendor split already accounted for in the source-matching notes above.

The check used the cached seL4 source at
[`6e7c3b7`](https://github.com/seL4/seL4/tree/6e7c3b733d296cfd88d5fbf635c96e447a882374)
and Microkit source at
[`ec86afd`](https://github.com/seL4/microkit/tree/ec86afdcd662b5976d11d4994acf1b11a2979882).

## References

- [Matched Qualcomm peripheral image loader (`peripheral-loader.c`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/peripheral-loader.c)
- [Matched Qualcomm removed-memory DMA allocator (`dma-removed.c`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/base/dma-removed.c)
- [Matched Qualcomm MDT ELF segment loader (`mdt_loader.c`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/mdt_loader.c)
- [Matched Qualcomm secure/PAS subsystem loader (`subsys-pil-tz.c`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/subsys-pil-tz.c)

- [Exact LineageOS kernel commit reported by the running device](https://github.com/LineageOS/android_kernel_google_msm-4.14/commit/28f9290ae067d3e7857126eb6312720dbe9c7c80)
- [Sunfish kernel build config at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/build.config.sunfish)
- [Qualcomm SMEM global/private item lookup and hardware-lock use at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/smem.c)
- [Qualcomm TCSR hardware-spinlock register operations at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/hwspinlock/qcom_hwspinlock.c)
- [QCACLD source snapshot at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/tree/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qcacld-3.0)
- [QCACLD service-ready WMA handler at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qcacld-3.0/core/wma/src/wma_main.c)
- [QCACLD host-memory request handling at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qca-wifi-host-cmn/target_if/init_deinit/src/init_cmd_api.c)
- [QCACLD service-ready event dispatch at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qca-wifi-host-cmn/target_if/init_deinit/src/init_event_handler.c)
- [Qualcomm WMI service-ready and host-memory request structures at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/fw-api/fw/wlan_defs.h)
- [QCACLD WMI fixed service-ready layout at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/fw-api/fw/wmi_unified.h)
- [QCACLD WMI event TLV ordering and tag definitions at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/fw-api/fw/wmi_tlv_defs.h)
- [QCACLD service bitmap sizing at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/fw-api/fw/wmi_services.h)
- [ICNSS source at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c)
- [Qualcomm SMP2P IRQ fan-out and shared-memory transport at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/smp2p.c)
- [ICNSS WLFW QMI MSA-region exchange at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss_qmi.c)
- [Sunfish/Atoll modem GLINK and SMEM device-tree configuration at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/qcom/atoll.dtsi)
- [Qualcomm GLINK SMEM FIFO and descriptor implementation at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/rpmsg/qcom_glink_smem.c)
- [Qualcomm GLINK edge registration and subsystem-restart lifecycle at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/glink_probe.c)
- [Qualcomm SCM VMID and permission constants at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/include/linux/qcom_scm.h)
- [ARM SMMU driver and bootloader handoff logic at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu.c)
- [ARM SMMU register definitions at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu-regs.h)
- [IPA driver WLAN SMMU-context handling at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa.c)
- [IPA WDI2 ring setup and GSI SMMU mapping at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa_uc_wdi.c)
- [QCACLD IPA ring parameters and optional SMMU tables at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qcacld-3.0/core/dp/txrx/ol_txrx_ipa.c)
- [Qualcomm secure-buffer / VMID memory-assignment source at that commit](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/secure_buffer.c)
- [Qualcomm Technologies overview of Snapdragon VMIDMT, XPU, and SMMU access control](https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/an-introduction-to-access-control-on-qualcomm-snapdragon-platforms.pdf)
- [Qualcomm-authored Linux SMMU discussion describing QHEE's typical EL2 static SID setup for a modem](https://lists.openwall.net/linux-kernel/2020/05/11/1172) (an analogous Qualcomm platform example, not Sunfish WLAN proof)
- [Matched Sunfish Apps SMMU node (`qcom,qsmmu-v500`, `qcom,skip-init`)](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/qcom/msm-arm-smmu-atoll.dtsi)
- [Matched Qualcomm secure-SMMU helper and write guard](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/include/soc/qcom/msm_tz_smmu.h)
- [Matched Qualcomm secure-SMMU helper implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/msm_tz_smmu.c)
- [Matched legacy Qualcomm IOMMU secure page-table implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/qcom_iommu.c)
- [Matched secure IOMMU page-table map/unmap implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/io-pgtable-msm-secure.c)
- [Matched Qualcomm SCM secure page-table wrappers](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/firmware/qcom_scm-64.c)
- [Matched ath10k WLFW memory assignment calls](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/net/wireless/ath/ath10k/qmi.c)
- [Matched Sunfish kernel configuration](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/configs/sunfish_defconfig)
- [Google Sunfish board WLAN configuration](https://android.googlesource.com/device/google/sunfish/%2B/0fcf4503/BoardConfig-common.mk)
- [Google Sunfish WLAN configuration files](https://android.googlesource.com/device/google/sunfish/%2B/refs/heads/main/)
- [Google Pixel driver-binaries archive and published checksum](https://developers.google.com/android/drivers)
- [Upstream Linux ath10k SNOC driver at pinned source](https://github.com/torvalds/linux/blob/d6d3455d8751854e0d1820f14184682e23ac4475/drivers/net/wireless/ath/ath10k/snoc.c)
- [SM7150 mainline Sunfish device tree at pinned commit](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts)
- [SM7150 mainline WLAN resource node at pinned commit](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150.dtsi)
- [Linux firmware WCN3990 inventory and Qualcomm license terms](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/afabaf773c4c2e2c841429933a6a084a5af4d14d/WHENCE)
- [Qualcomm Atheros ath10k firmware license](https://gitlab.com/kernel-firmware/linux-firmware/-/blob/afabaf773c4c2e2c841429933a6a084a5af4d14d/LICENSES/LICENSE.QualcommAtheros_ath10k)
- [Community Pixel 4a firmware rehost](https://github.com/sm7150-mainline/firmware-google-sunfish/tree/fe63598e1bd9884bd68ba89e4670dae4d70b1e3e)
- [Community SM7150 ath10k firmware rehost](https://github.com/sm7150-mainline/firmware-qcom-sm7150-ath10k/tree/792c716c85c06861c4d17bce5ec21fba7f0f7de6)
- [Google Android kernel ICNSS driver](https://android.googlesource.com/kernel/msm/%2B/2f25819b0c776ffd3d766b581954820b0f2f6630/drivers/soc/qcom/icnss.c)
- [Google Sunfish QCACLD source at the checked commit](https://android.googlesource.com/kernel/msm-modules/qcacld/%2B/4f47232dc5c209b3137ea3ca26657e3853bf0d21/)
- [Google Sunfish prebuilt kernel and modules](https://android.googlesource.com/device/google/sunfish-kernel/%2B/refs/heads/main/)
- [Google Android 13 r68 Sunfish prebuilt module set](https://android.googlesource.com/device/google/sunfish-kernel/%2B/refs/tags/android-13.0.0_r68/)
- [Google Android 13 Sunfish kernel manifest and external QCACLD project path](https://android.googlesource.com/kernel/manifest/%2B/refs/heads/android-msm-sunfish-4.14-android13/default.xml)
- [LineageOS Sunfish vendor blob list](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/sunfish/proprietary-files-vendor.txt)
- [TheMuppets Sunfish vendor tree](https://github.com/TheMuppets/proprietary_vendor_google_sunfish/tree/lineage-23.2)
- [sDDF supported driver families](https://github.com/au-ts/sddf/blob/main/docs/drivers.md)
- [sDDF device driver framework](https://github.com/au-ts/sddf)
- [Pixel security target identifying supported WLAN chipsets](https://www.commoncriteriaportal.org/files/epfiles/st_vid11317-st.pdf)

The first failure-reason image showed DISC ERROR0x0e, slave status7, and
Error::Partition0x4 for both the baseline and terminal reason reads. Both
guard checks passed. The second image showed CHECK STAGES00020002: the v2
decoder stopped in its non-global partition-range scan on both reads. The
screen does not identify which unrelated route or which range condition
failed, and no reason bytes were captured.

Kernel source review showed the global-reason path selects and validates the
unique global route without traversing unrelated host-pair ranges. The v3
reader removes that extra scan while retaining bounds on the selected global
route and its header, allocations, and payload. The v3 temporary boot stopped
at an earlier IPCRTR OPEN preflight: ERROR8, GUARD STAGE1, PLAN DETAIL4, with
STAGED, HEAD STORE, NOTIFICATIONS, and BYTE WRITES all zero. Detail4 means one
of the three captured OPEN records differed from the assumed offset/header/
name-length shape; the screen did not show which record or field, and the
modem-reason capture did not run. No modem crash cause or reason freshness was
established.

The v4 image adds bounded fields identifying the first rejected OPEN record's
offset, command, channel ID, and declared name length. Its 190 host tests,
input and source checks, image layout proof, and both QEMU loader checks pass.
The temporary v4 boot passed the OPEN preflight and completed the read-only
global item421 snapshot. The phone showed `REASON SNAPSHOT PASS`, discovery
`Fatal`0x0e after0x97bf ms, SMP2P slave status7, zero status/baseline/reason
read errors, `PREFIX CHANGED`1, and `CHECK STAGES`0. The selected item was at
0x12510 with size0x100; its sanitized text was NUL-terminated at0x5c bytes.
The displayed text began `DOG HAL COMMON C 173 TMR SLAVE2 DOG DETECTS STALLED
INITIALIZATION TRIAGE WITH IMAGE OWNER`. Both guard checks passed. WLFW was not
seen in this bounded discovery sample; that does not establish global absence.
The changed prefix is evidence of different contents between the two bounded
reads, not proof that the discovery failure caused the message. No partition
was flashed.

The subsequent temporary fatal-trace boot recorded one SMP2P status transition:
first status `0x6` (ready and handover/proxy-unvote), then fatal status `0x7`
(fatal added while ready and handover remained set), at `0x97f6` ms (38.902
seconds). The settled photo shows 1,295 status samples (`0x50f`) and one
change; stop-ack and shutdown-ack are clear. The fatal reason snapshot succeeded
with error0 and the same `0x12510`/`0x100` item; the displayed reason reads
`DOG HAL COMMON C 173 TMR SLAVE2 DOG DETECTS STALLED INITIALIZATION TRIAGE WITH
IMAGE OWNER`. `DISC ERROR`0x0e is the probe stopping on the observed fatal bit,
not a reason-read failure. `CHECK STAGES`0 and `GUARDS`3 show the probe checks
and both guard regions passed. It completed 1,440 DATA polls (`0x5a0`); WLFW
was not seen during this bounded sample, which does not establish global
absence. The trace confirms the modem asserted fatal after signaling ready and
handover; it does not identify why initialization stalled. No GIC
watchdog-pending state was sampled, and no partition was flashed.

After the temporary image ended, ADB was available in LineageOS again. Read-only
properties reported baseband `g7150-00112-230505-B-10075601`; a filtered
process listing showed `pd-mapper`, `modem_svc`, `qcrild`, and `qrtr-ns` running.
Temporary rooted ADB read only the two service manifests, then ADB was returned
to UID 2000. `modemr.jsn` maps `tms/servreg` and `gps/gps_service` to
`msm/modem/root_pd` instance180. `modemuw.jsn` maps `kernel/elf_loader`,
`tms/servreg`, and `wlan/fw` to `msm/modem/wlan_pd` instance180. The workspace
has no `pd-mapper` implementation. A Linux developer report
records the same `DOG detects stalled initialization, triage with IMAGE OWNER`
message when a Qualcomm remote processor starts before the PD Mapper service is
available; that report concerns different Qualcomm platforms, so this is a
candidate dependency to audit, not a diagnosis for Sunfish
([report](https://www.mail-archive.com/linux-kernel@vger.kernel.org/msg2653217.html)).
The reference userspace mapper loads these `.jsn` declarations and publishes the
SERVREG QRTR/QMI service that answers domain-list queries
([source](https://sources.debian.org/src/protection-domain-mapper/1.0-4/pd-mapper.c)).
The upstream Linux SMP2P and PAS remoteproc drivers provide the host-side status
and crash-reporting path, but do not expose the modem firmware's DOG internals
([SMP2P](https://github.com/torvalds/linux/blob/master/drivers/soc/qcom/smp2p.c),
[PAS remoteproc](https://github.com/torvalds/linux/blob/master/drivers/remoteproc/qcom_q6v5_pas.c)).

### QRTR receive snapshot and GLINK source comparison (2026-10-18; corrected 2026-10-19)

The first temporary SERVREG probe stopped at its local `Error::Payload` check
after two equal reads of an 88-byte RX snapshot. RX tail stayed unchanged; this
was not a modem-reported error. A later photo of the same screen clarified the
little-endian fields: the first 72 bytes contain a QRTR v1 `NEW_SERVER`
announcement for service `0x2b`, instance `0x00012102`, node0/port1. The last
16 bytes contain two 8-byte GLINK controls: `RX_DONE_W_REUSE` (command14,
channel2, remote intent1), then `RX_INTENT_REQ` (command7, channel2, size60).
The earlier labels `0x2010` and `0x2007` were big-endian renderings of those
little-endian command/channel fields.

The Android Qualcomm GLINK driver defines commands 6, 7 and 14, keeps an
intent after `RX_DONE_W_REUSE`, and handles `RX_INTENT_REQ` by advertising a
suitable local intent and acknowledging the request
([driver source](https://android.googlesource.com/kernel/msm/+/14d53c3a58295226c4b26bc60fca334f4be6e0a3/drivers/rpmsg/qcom_glink_native.c)).
The QRTR header defines type4 as `NEW_SERVER`
([QRTR source](https://android.googlesource.com/kernel/msm/+/f7fb80b48f802985ac40a9934520f3d0fbb0fcc4/net/qrtr/qrtr.c)). These sources corroborate the captured layout; they do not identify the service's modem-firmware role or prove what query follows.

The revised one-shot probe now accepts only this exact 88-byte prelude. After
rechecking the stable snapshot and descriptor, it retires local intent3,
advances RX tail by88, advertises a fresh 64-byte local intent4, and acknowledges
the request. It then waits for one exact 72-byte SERVREG QMI query addressed to
intent4. Other service announcements or control shapes remain rejected before
tail/head publication. This is the first revision that consumes the captured
discovery prelude; the QMI request and response have not yet been observed in
this run.

After returning to LineageOS, read-only ADB confirmed `qrtr-ns`, `pd-mapper`,
`modem_svc`, `wificond`, and `qcrild` running. The filtered kernel log showed
`MPSS AFTER_POWERUP`, modem QRTR/QMI readiness, and the `msm/modem/wlan_pd`
QMI server connecting to ICNSS. This is evidence for the normal Linux-owned
stack only; it does not establish a safe standalone GLINK peer state. The next
engineering step is source/protocol identification for the two unknown records
and the modem lifecycle boundary, not another write based on the raw values.

The shell user could not read `dmesg`, and debugfs IPC logging was not mounted.
Temporary `adb root` was used only to read the kernel ring buffer, then `adb
unroot` restored UID2000. The normal boot log has no `unhandled rx cmd` line in
the captured buffer; because the standalone probe uses a different boot and
transport session, this absence does not classify its records. The exact
numeric search and the checked public Lineage driver source yielded no
definitions for `0x2010` or `0x2007`.

In that exact kernel source, the default GLINK RX case already prints
`unhandled rx cmd: <id>` and stops dispatch without advancing RX. No such line
appeared in the captured normal-boot log. Extra kernel instrumentation is only
needed to log parameters/raw bytes for messages that the normal Linux driver
does receive; it cannot make the normal boot reproduce the standalone session.

On the connected Lineage device, tracefs is mounted but exposes no GLINK,
QRTR, RPMsg, or IPC trace events and no kprobe interface; debugfs is not
mounted. `uname -a` reports `4.14.357-openela-g28f9290ae067`; the matching
LineageOS source commit is
[`28f9290ae067d3e7857126eb6312720dbe9c7c80`](https://github.com/LineageOS/android_kernel_google_msm-4.14/commit/28f9290ae067d3e7857126eb6312720dbe9c7c80).
Therefore a live raw-header capture needs a small debug-only kernel
instrumentation change at the GLINK RX parser, built from that exact source and
booted temporarily with the original Lineage ramdisk/DTB.
The probe should log only RX headers and ring position before command dispatch,
with no shared-memory writes or altered dispatch behavior; capture dmesg, then
reboot to the normal Lineage image. A normal boot's log is a different modem
session and cannot by itself explain the standalone snapshot.

### Prepared temporary GLINK RX header diagnostic (2026-10-18)

Prepared `patches/lineage-sunfish-glink-unknown-header-log.patch` against the
matching kernel commit and built the diagnostic kernel from a clean archive of
that commit, using the phone-captured configuration. Only `LOCALVERSION` and
`LOCALVERSION_AUTO` were adjusted to preserve the reported release string.
The build completed with full LTO and reports
`4.14.357-openela-g28f9290ae067`. In the default handler for unsupported GLINK
commands, the patch adds the command, 16-bit `param1`, and 32-bit `param2` to
the existing rate-limited kernel error message. It leaves the `-EINVAL` stop
unchanged, does not advance the RX ring, and does not log payload bytes.

The resulting kernel is packaged as
`.cache/pixel-sunfish/glink-kernel-diagnostic/artifacts/fastboot.img` for
temporary boot only. The boot-v2 header checksum was verified, the embedded
kernel matches the build output, and the ramdisk and DTB match the saved
Lineage reference image byte-for-byte. Checksums and build inputs are recorded
in the adjacent `manifest.json`.

The image was booted once with `fastboot boot`; no partition was flashed. ADB
reported the expected kernel release and Android reached
`sys.boot_completed=1`. The filtered log
`.cache/pixel-sunfish/glink-kernel-diagnostic/artifacts/first-boot-glink-dmesg.txt`
contains four GLINK-related module/channel initialization lines and no
`unhandled rx cmd` messages. After capture, the phone rebooted and reached
Android again; ADB returned to UID2000. This run found no unsupported headers
in the normal Linux session, so it does not classify the two records from the
separate standalone session. The diagnostic may only report headers that the
normal Linux GLINK driver receives.

### Source audit of SERVREG and WLFW advertisement (2026-10-19)

The source trace separates three roles that earlier notes grouped together:
QRTR name service, Qualcomm's protection-domain mapper, and the modem-side
WLFW QMI server. LineageOS
[`init.hardware.rc`](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.hardware.rc)
declares `vendor.qrtr-ns`, `pd_mapper`, and `modem_svc` as core services. The
declarations do not provide a readiness barrier proving that the mapper is
initialized before the modem requests a domain lookup.

The upstream [`pd-mapper`](https://github.com/linux-msm/pd-mapper/blob/master/pd-mapper.c)
walks `/sys/class/remoteproc`, reads the remoteproc firmware path, finds the
associated `.jsn` files, and turns each `sr_service` provider/name into a
service-to-domain/instance mapping from `sr_domain`. It publishes the SERVREG
QRTR/QMI service and answers `SERVREG_LOC_GET_DOMAIN_LIST` from that table.
The code reads remoteproc metadata; it does not issue a remoteproc start or
create a WLFW QMI server. In the captured Sunfish manifests,
`modemr.jsn` maps `tms/servreg` and `gps/gps_service` to
`msm/modem/root_pd` instance 180, while `modemuw.jsn` maps
`kernel/elf_loader`, `tms/servreg`, and `wlan/fw` to
`msm/modem/wlan_pd` instance 180. Here `wlan/fw` is a SERVREG name, not the
WLFW QMI service ID `0x45`. Since `pd-mapper` builds one table from both
manifests and returns all rows matching a service name, a `tms/servreg`
GET_DOMAIN_LIST query should contain both `root_pd` and `wlan_pd` records.
That mapping response describes registered service names; it does not prove
that either protection domain is currently running.

The earlier fixed response encoded one `root_pd` result for `tms/servreg` and
omitted each entry's `service_data_valid` and `service_data` fields. The
upstream [`servreg_loc.qmi`](https://github.com/linux-msm/pd-mapper/blob/master/servreg_loc.qmi)
schema defines both fields, and [`servreg_loc.c`](https://github.com/linux-msm/pd-mapper/blob/master/servreg_loc.c)
serializes the complete entry structure. The host-only wire encoder in
[`pixel_servreg_wire.rs`](../rust/src/pixel_servreg_wire.rs) now prepares both
captured domain rows, with those service-data fields present and zeroed; its
unit test checks the 116-byte QRTR datagram and all QMI lengths and fields.
The mapping model in [`servreg_wlfw_model.py`](../platforms/sunfish/servreg_wlfw_model.py)
continues to keep static SERVREG mappings separate from QRTR WLFW
lookup/server registration. This fixes a demonstrated schema mismatch in the
local response, but does not prove it caused the earlier payload rejection.
The SERVREG probe was subsequently booted and captured the modem's exact
`tms/pdr_enabled` request, but its build profile had the PDR-reply feature
disabled. It therefore rejected the valid 80-byte GLINK candidate before
replying; the result and corrected image are recorded below. No partition has
been flashed.

### SERVREG response candidate ready for device check (2026-10-20)

The probe build wrapper now keeps prerequisite GLINK/QRTR feature checks
separate from the mutually exclusive loader-manifest mode. The native SERVREG
task-image manifest was refreshed from the rebuilt ELF and its exact
11-region table; the loader profile pins the manifest digest. The clean build
passes the offline QRTR/SERVREG wire and state tests, and the final
boot-layout validator confirms the source image and relocated loader do not
overlap, copy targets are disjoint, and all checked ranges avoid cached live
and kernel reservations while fitting the cached DT RAM banks.

The image at
`../.cache/pixel-sunfish/modem-qrtr-servreg-probe/fastboot.img` (SHA-256
`95fe415b9cd13291e4c851a5b357c4e995e9c006f70d274e79192708b8627129`) was
temporarily booted. The screen showed `QRTR SERVREG RX` / `PDR QUERY DECODE`,
ERROR `0x8`, RX available/captured `0x50`, GLINK DATA command on channel 2,
QRTR body size 25 from node 0/port 9 to node 1/port `0x4000`, QMI GET_DOMAIN_LIST
message `0x21`, transaction 1, and TLV 1 of length 15. The decoded shape
matched `tms/pdr_enabled`; RX tail stayed unchanged and no response or WLAN
DMA was sent. `metadata-profile.json` confirms
`qrtr_servreg_pdr_reply=false`, so the image treated this PDR request as an
unexpected shape. This is a feature-selection mismatch, not evidence that the
modem rejected a correct QMI response.

### PDR-enabled mapping reply candidate (2026-10-20)

The probe now has a distinct pinned Microkit task-image manifest for the PDR
reply feature. Its temporary image is
`../.cache/pixel-sunfish/modem-qrtr-pdr-reply-probe/fastboot.img` (SHA-256
`88460e65f7cce44bf4da499875c6e12c026e10754100f1c2f42145855b0ad36e`), built
from native PD ELF SHA-256
`c7367b2babc4b23b58f63e880d0e20e12474c14359b58fafaed010a90c38b638`. The
profile confirms PDR reply enabled and follow-up disabled. It accepts only the
captured exact `tms/pdr_enabled` GET_DOMAIN_LIST request, returns QMI success
with database revision 1 and zero domains, verifies the one response is
consumed, then observes RX without advancing its tail. It sends no WLAN QMI,
performs no DMA, and does not start `wlan_pd`.

Offline SERVREG/WLFW tests and the full modem/QMI regression suite pass. The
build's boot-layout proof confirms disjoint source/relocated payload/copy
ranges within the cached DT RAM banks, with no fixed-reservation overlap. The
image was temporarily booted after the Pixel reappeared in Fastboot. The result
showed `EMPTY MAP PASS`, ERROR `0`, and a consumed response. A stable,
unchanged-tail snapshot then exposed exactly 16 post-reply RX bytes:
`RX_DONE_W_REUSE` command 14 on channel 2 for remote intent 2, followed by
`RX_INTENT_REQ` command 7 on channel 2 requesting 64 bytes. The image did not
consume those control records or send a second QMI reply. This confirms the
empty-map reply was consumed and provides the exact next bounded GLINK step;
it does not establish WLFW service startup or Wi-Fi readiness. No partition is
flashed.

The next device candidate is the already bounded PDR follow-up probe. It will
consume only those two exact control records, advertise fresh local intent 5
with capacity 64, and answer only the captured `tms/pddump_disabled`
GET_DOMAIN_LIST request with an empty-map success. It then holds the RX tail
fixed while observing for a third request. This will clarify the modem's
SERVREG/PDR sequence before native WLFW startup; it still does not start WLAN
firmware or authorize DMA.

The first follow-up candidate at
`../.cache/pixel-sunfish/modem-qrtr-pdr-followup-probe/fastboot.img` (SHA-256
`647356e3aa887af53194a150a8e45f9d77631e77eb85bc45f99719750e977c46`) was
temporarily booted. The screen showed `QRTR PDR NEXT PACKET` / `PDDUMP REPLY
PASS`, ERROR `0`, PDDUMP match `1`, transaction `2`, request port `9`, and one
staged and consumed reply. Its stable RX snapshot was exactly 8 bytes at tail
`0x3f8`: `RX_DONE_W_REUSE` command 14/channel 2/intent 2. It reported no third
QMI reply and left RX tail fixed. This confirms the `tms/pddump_disabled`
empty-map reply was consumed; it does not prove WLAN service startup or
readiness.

The next bounded iteration accepts and consumes only that exact 8-byte
post-reply completion after a double copy, then holds the new RX tail while it
observes at most 64 subsequent bytes. It sends no third QMI response and does
not start WLAN firmware or DMA. The updated candidate is built at the same
temporary image path, with SHA-256
`40dbb1143969bcc4583913b7958fb1ee0a2dd7b39cc357d8f8f02958ec2a8524` and native
PD ELF SHA-256
`8cbfd08f8c2ef861c90e49ea50902c39987433a7f6acffdcbdf3b3efed110398`. Its
dedicated task-image manifest is refreshed and pins that ELF. The offline
probe checks pass, and the boot-layout proof confirms the source and relocated
payload are disjoint, copy targets do not overlap, and the checked ranges avoid
the cached kernel/live reservations while remaining inside cached DT RAM. It
is prepared for temporary boot only; no partition is flashed.

The matched LineageOS [`icnss_qmi.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss_qmi.c)
and [`qmi_interface.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/qmi_interface.c)
sources show that ICNSS is a WLFW client. It
registers a lookup for WLFW service/version/instance 0; QRTR's name service
then sends a `NEW_SERVER` notification when a matching server advertises.
ICNSS's callback consumes the announced node/port and posts its server-arrival
event. In [`icnss.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c),
ICNSS then connects and performs the WLFW startup sequence:
indication registration, MSA information exchange and validated permission
assignment, MSA-ready, capability exchange, and finally WLAN-driver probe on
firmware-ready. This means the WLFW advertisement itself must come from the
running modem WLAN protection domain; registering the ICNSS lookup does not
start that domain or publish the service.

The normal LineageOS run provides positive evidence that the Linux-owned path
reaches WLFW and WLAN readiness: the saved log shows `MPSS AFTER_POWERUP`,
modem QRTR/QMI readiness, and the `msm/modem/wlan_pd` QMI server connecting to
ICNSS. The standalone
Microkit snapshots did not observe service `0x45` in their bounded discovery
window, while seeing service `0x2b`. That is a per-session
observation, not global absence. Microkit also lacks Android's QRTR name
service and `pd-mapper`; this is a real stack difference, but open source does
not establish that adding only `pd-mapper` would start `wlan_pd` or cure the
modem's watchdog. The exact modem-firmware rule that starts `wlan_pd` and
causes its WLFW endpoint to register remains proprietary/unverified.

For the native implementation, treat SERVREG mapping, modem-domain lifecycle,
QRTR service announcement, and ICNSS QMI startup as separate gates. Linux
QRTR handles `NEW_LOOKUP` inside the local name service and then notifies the
local client when a matching server is announced; forwarding that control
packet over the modem link would be incorrect. The native bounded owner now
exposes a WLFW endpoint only when the authenticated peer has announced exactly
one service `0x45`, packed version/instance `1` (version 1, instance 0); wrong
versions and ambiguous duplicates fail closed; the native adapter repeats the
live SMEM lifecycle check before exposing the endpoint. This passive registry
lookup does not start the domain or perform QMI startup. A `NEW_SERVER` record must
still not bypass the independent MSA-ownership, SMMU-translation, power/clock,
and exclusive GLINK/QRTR ownership checks above. The bounded SERVREG
candidate above tests the unresolved domain-mapping response only; it does not
relax those later bring-up gates.

The native WLFW path now connects [`qcom_wlfw_client.rs`](../rust/src/qcom_wlfw_client.rs)
through [`qcom_glink_qmi.rs`](../rust/src/qcom_glink_qmi.rs),
[`qcom_glink_fifo.rs`](../rust/src/qcom_glink_fifo.rs), and
[`qcom_modem_owner.rs`](../rust/src/qcom_modem_owner.rs). After trusted local
port reservation and an exact WLFW `NEW_SERVER`, the session queues one
ICNSS-default indication-registration request through GLINK's bounded transmit
intent path. It routes only a response addressed to that client port and
matching the announced peer, transaction and message ID. Accepted registration
responses advance to `MsaInfoReady`; rejected responses are terminal until server
departure. The owner exposes the outcome, and transport-generation reset
clears the WLFW state. WLFW responses now preserve the validated boolean QRTR `confirm_rx` flag and
stage the exact `RESUME_TX` control acknowledgement through the bounded GLINK
TX queue. A full owner-through-FIFO test checks the source/destination and
client-address fields; invalid flag values fail closed. Offline end-to-end
tests now cover this native FIFO owner exchange, and
`python3 platforms/sunfish/test-modem-qmi.py` passes its host suites and
AArch64 native code-generation checks. No phone-side WLFW registration was
performed. The fixed-pool MSA_INFO exchange is now implemented offline; SCM
assignment, SMMU/power/clock ownership, MSA_READY, capability negotiation and
firmware start remain separately gated.

The rebuilt PDR-after-DONE candidate (`40dbb1143969bcc4583913b7958fb1ee0a2dd7b39cc357d8f8f02958ec2a8524`)
was temporarily booted on 2026-10-20 after Fastboot confirmed product `sunfish`.
The screen instead showed `IPCRTR FIRST RECEIVE` / `RECEIVE STOPPED`, error
`0x08` (`Payload`), stage 1, with zero TX consumed, zero RX available, and
zero copied bytes. In `pixel_glink_rx`, stage 1 is the exact prior intent
inventory plan, so the probe stopped before writing or consuming any packet;
the PDR follow-up and its RX_DONE_W_REUSE handler were not reached. The prior
hard-coded receive prerequisite therefore did not match the GLINK state in
that attempt. No WLAN QMI, DMA, or firmware start occurred, and no partition
was flashed. A bounded GLINK intent-inventory rerun was used to re-establish
the live prerequisite before selecting another receive step. It repeated the
fixed GLINK setup handshake, but sent no QMI payload and performed no WLAN or
host DMA.

That inventory candidate was rebuilt offline at
`../.cache/pixel-sunfish/modem-glink-intents-v2-20261020-probe/fastboot.img`
(SHA-256
`d3ccdc6edc758f475966ea23081bb9f782a83478bd6847142c8724a6ea63c277`). The
input proof, offline GLINK intent tests, and final image-placement proof pass;
the source and relocated image remain disjoint from the checked fixed
reservations, and copy targets are mutually disjoint. On 2026-10-20, Fastboot
confirmed product `sunfish` and temporarily booted this image. The screen
reported `INTENT INVENTORY PASS`, error 0, TX consumed 1, RX head `0x248`, and
`0x1e8` new RX bytes captured and parsed. It found 31 control records, 30 intent
entries, four size groups, one intent request of `0x34` bytes, IDs `0..0x1d`,
no duplicate IDs, no zero-sized intents, and no signal bits. The groups were
`(offset 0x80, count 16)`, `(0x200, 8)`, `(0x480, 5)`, and `(0x2080, 1)`.
The probe selected no intent, sent no reply or QRTR data, and stopped with the
RX tail unchanged. No WLAN QMI, DMA, firmware start, or partition flash
occurred. This refreshes the receive-intent inventory but does not establish
that a future request or reply will fit a particular live intent.

The offline implementation now includes a bounded SERVREG `GET_DOMAIN_LIST`
decoder and manifest-backed response encoder, registered as local service
`0x40`/version 1/instance 1 at port `0x4000`. The owner advertises it after
QRTR control startup and answers only the supported query; it does not start a
modem domain or perform power, DMA, SCM, or WLAN operations. An owner-through-
FIFO test covers service announcement, request routing, response identity,
and the two-domain `root_pd`/`wlan_pd` map. That test caught and fixed a
response-boundary error: QMI's standard result TLV belongs to the transport
encoder, so the service handler supplies only optional TLVs. The complete
`python3 platforms/sunfish/test-modem-qmi.py` run now passes, including host
suites, the SERVREG owner exchange, and AArch64 native code-generation checks.

The next guarded first-receive image is built at
`../.cache/pixel-sunfish/modem-glink-rx-probe/fastboot.img` (SHA-256
`c2af2e7939b882d8d940c27479a2c62babc4218f85713e0304fac428e3db0482`). Its
offline control-plan, packet-copy, and image-placement checks pass. The pinned
task manifest matches the rebuilt native ELF. This temporary probe answers
only the observed 52-byte intent request with a private guarded 64-byte buffer
and captures one bounded 52-byte packet without consuming it or sending QRTR,
QMI, WLAN, or DMA traffic. Fastboot confirmed `sunfish` and booted it on
2026-10-20; the user photo `native-first-receive-20261020.png` (SHA-256
`47e15fa4df73009627a48f9ba9229f83047c4041ed3ce6c78b63f60131ab1f13`)
shows `RECEIVE COPY PASS`, ERROR `0`, guard stage 9, TX consumed 1, TX head
`0x40`, RX head `0x290`, and `0x288` bytes available. The probe copied exactly
52 bytes from DATA command 9 / remote CID 2 / local intent 1 into its private
buffer, with zero remainder and intact guards. The displayed QRTR header is
version 1/type 2 with body command 2 and broadcast control destination, a shape
consistent with HELLO; the remaining body words were not displayed, so the
full control body is not independently verified. RX tail remains unchanged,
with no RX_DONE or QRTR payload sent. The next candidate is a one-shot,
non-reusable RX_DONE for this exact receive, gated by a second byte-for-byte
validation of the preserved prefix, packet, guards, live identities, and FIFO
state. It sends no QRTR/QMI response and does not start WLAN or DMA. No
partition was flashed.

The one-shot RX_DONE candidate is built at
`../.cache/pixel-sunfish/modem-glink-done-probe/fastboot.img` (SHA-256
`0ae65d371dd8f2774f63c9b54b189903c3724ae20a4c244cf0cd701cd2180c3b`), with
native task ELF SHA-256
`0d4859153f91a1146dfe4da8a1704623ea98fbc5ee303684545c238695c9c0d0`. Its
offline suites pass, including 101 receive/completion cases; the input proof
requires the exact captured packet and current FIFO checkpoint. The final
placement proof confirms disjoint source and relocated images, disjoint copy
targets, avoidance of cached kernel/live reservations, and preservation of the
original boot DTB. It is ready for temporary boot only. Fastboot currently sees
no device because the Pixel remains on the diagnostic result screen.


### WLFW QRTR flow-control acknowledgement (offline)

The bounded WLFW registration client previously rejected a valid response
whose QRTR v1 `confirm_rx` field was set. It now returns that flag with the
registration outcome, and the GLINK/QMI session reserves a TX slot before
committing a response that requests confirmation. After validating the QMI
transaction, it stages the matched kernel's fixed `RESUME_TX` packet (command
7, client equal to the local QRTR address). The existing queue sends
that credit packet as a high-priority QRTR datagram and retires it only after
GLINK completion. The native owner-through-FIFO test verifies the exact
header/body, and the WLFW client test rejects non-boolean flag values.

`python3 platforms/sunfish/test-modem-qmi.py` passes, including the new
confirmed-response owner test and AArch64 no-std code-generation checks. This
closes a transport-flow-control gap in the offline WLFW path; it does not
establish MSA ownership, SMMU isolation, regulator/clock sequencing, modem
service availability, firmware loading, or Wi-Fi operation.


### WLFW MSA_INFO exchange (offline)

The native owner now advances from accepted indication registration to a
bounded `MSA_INFO` request only when given a fixed Sunfish MSA reservation
token. The token checks the compiled `0x98d00000`/`0x200000` profile, boot
identity, no-map/exclusive evidence, and overlap against the trusted exclusion
inventory. It records validated geometry only; it does not prove current
ownership, install mappings, or authorize SCM.

The request carries the exact fixed base and size. The response parser matches
the peer, local client, QMI transaction, message ID, and QRTR padding, then
rejects empty, overflowing, out-of-pool, or overlapping WLFW subregions before
exposing them to the owner. An owner-through-FIFO test exercises the observed
two-region split and confirmed `RESUME_TX` response. The WLFW client exposes
the validated plan at `MsaOwnershipRequired`; the integrated native owner now
performs the bounded SCM handoff and can continue to `MSA_READY` and capability
discovery, as described below. Device-side WLFW traffic has not yet been
attempted.

### Fixed MSA transfer description (offline)

The native MSA policy now converts a successful WLFW response into an inert
handoff description only when it exactly matches the observed profile:
`0x98d00000`/`0x4000` with secure flag 1 and
`0x98d04000`/`0x1fc000` with secure flag 0. It fixes the source to HLOS VMID
`0x03`, gives the secure range RW to MSS_MSA `0x0f` and WLAN `0x18`, and gives
the nonsecure range RW to those VMIDs plus WLAN_CE `0x19`. It accepts the two
records in either response order, but rejects changed ranges, flags, lengths,
or counts. The plan carries the reservation's generation and identity so later
work can match it to the live handoff.

This is typed policy data, not an SCM invocation or proof of current memory
ownership. The matched Linux `ath10k_qmi.c` source sends each WLFW region to
`qcom_scm_assign_mem()`, then proceeds to `MSA_READY` only when that call
returns success. For secure regions it requests RW for MSS_MSA and WLAN; for
nonsecure regions it also requests RW for WLAN_CE. Reclaim requests the prior
VMID set and restores HLOS RW. The Linux SCM wrapper uses a zero transport
status plus the secure-call result register; it updates its local source-VMID
mask after success and does not query the monitor for an independent owner
readback. The fixed-plan acceptance and rejection cases pass offline.

The SCM64 call encoder now also accepts only a sealed `TransferPlan` and
serializes its source VMIDs, up to three destination/permission entries, and
the reviewed seven-argument envelope into private fixed-size scratch bytes.
Offline tests check both secure and nonsecure layouts, the three-destination
record, pointer offsets, and zero padding. The native owner now connects this
plan to the fixed private scratch mapping and its serialized SMC capability;
the target code has only been built and tested offline so far.

### Exact-once MSA handoff coordinator (offline)

[`qcom_msa_handoff.rs`](../rust/src/qcom_msa_handoff.rs) now consumes only the
sealed two-range handoff plan. It requires matching boot generation and
identity, a ready live lifecycle, the validated fresh-boot HLOS reservation,
and an explicit SCM/exclusivity contract. Before each invocation it stages the
exact SCM envelope and rechecks the live state. It advances its local transfer
ledger only after exact zero status in both SMC `a0` and SCM result `a1`,
completion ordering, and a fresh lifecycle check. This matches the pinned
Linux API, which has no independent owner-query operation. The coordinator
performs exactly two plan-ordered transfer operations; an ambiguous monitor or
completion result marks the current range unknown and stops without transfer
replay, rollback, or implicit reclaim. Within one transfer, the native SCM
adapter resumes interrupted calls using the returned command token and retries
the matched busy status after 30 ms, with 21 busy retries, 32 interrupted
retries, and 64 total low-level calls as hard bounds. It revalidates the live
owner before every low-level SMC attempt.

Host tests cover both transfer records and failure at staging, lifecycle
revalidation, monitor invocation, each SCM status register, and completion.
The full offline modem/QMI test script and AArch64 no-std core build pass.

### Native MSA owner path (offline build)

The Microkit modem owner now waits for a successful decoded `MSA_INFO` response,
checks the exact two-region plan and fixed no-map reservation, then stages each
SCM envelope in the validated uncached scratch page at `0xa2a01000`. It checks
the scratch bytes after volatile writes, runs the two transfers serially, and
issues a live SMEM/GLINK lifecycle check before every SMC attempt and after each
completed transfer. Only exact zero in both SMC `a0` and SCM result `a1` mints
the one-use permit; only that permit allows `MSA_READY`, after which the owner
can request WLFW capabilities. Any ambiguous result latches the owner stopped
and retains mappings and scratch; it does not retry a transfer, reclaim memory,
or infer ownership from local bookkeeping.

The offline owner integration test verifies the actual staged bytes, both
plan-ordered SMC words, lifecycle callbacks, and permit. `test-modem-qmi.py`
passes, and `build-modem-owner-offline.sh` generates the owner PD ELF and
validates the unmapped MSA reservation, scratch mapping, guards, aliases, and
IRQ routes. The current owner PD ELF is 68,831,264 bytes with SHA-256
`5d4ca774aca43806f8affaaba9b6e29b3106823c649684fbbf04f4a5df2bb43d`; the
Microkit owner system ELF is 74,428,108 bytes with SHA-256
`b985b5fd15a7cc69b2c9c0c1bb2675ac13262a4b4920c1c7a277bf8378bc4065`.
This offline build does not yet produce a Fastboot image; the loader manifest
and image packer still need an owner-specific build and placement proof. No
device-side SCM assignment has been attempted. Firmware startup and Wi-Fi are
still blocked by the absence of a verified production WLFW board-data source
and by unimplemented SMMU, radio power, and firmware-loading stages.

### WLFW MSA_READY and capability exchange (offline)

The bounded WLFW client now accepts `MSA_READY` only with a one-use permit
minted by the handoff coordinator after both exact SCM success acknowledgements. A
capacity failure leaves the permit usable for retry; once the request is
queued, the token and client state prevent reuse. The client validates the
peer, local address, QRTR padding, QMI transaction and message IDs, and the
empty MSA_READY response schema. Capability discovery is enabled only after a
successful MSA_READY response, and the response decoder returns bounded chip,
board, SoC, firmware, MAC-count, and unknown-optional-field details.

Both requests and responses now pass through the existing bounded GLINK/QMI
session, FIFO adapter, and owner lifecycle gate. Offline owner-through-FIFO
coverage exercises MSA_INFO, RESUME_TX handling, a permit-gated MSA_READY
request and reply, and capability request/response parsing. The MSA_READY
permit in that transport fixture is a test token; the separate coordinator
tests verify that production code can mint it only after both exact SCM success
status registers are acknowledged and lifecycle checks pass. The transport
also recognizes the exact empty unsolicited MSA_READY
and FW_READY indications, returns QRTR confirmation when requested, and leaves
any outstanding request intact. The matched ath10k MSA_READY indication is
recognized, but board-file selection and calibration are not yet available.
The client now records FW_READY from either the registration response status
bit or the bound peer's unsolicited indication, clears that fact on endpoint
loss or generation change, and refuses a redundant MSA_INFO handoff when the
server already reports firmware ready. The owner exposes this fact separately
from CE, WMI, and network-interface readiness. This follows the pinned Linux
WLFW client, which takes its already-ready path when the registration status
contains the FW_READY bit. The focused WLFW/SCM/MSA harness passes 50 tests;
the complete `test-modem-qmi.py` runner and AArch64 no-std/native codegen
checks also pass. This is protocol-state evidence only, not live firmware or
radio readiness.
The QMI codec builds one BDF_DOWNLOAD segment up to 4023 bytes, which fits in
the local 4096-byte QRTR datagram ceiling, and validates its status response.

An offline BDF transfer state machine now stages at most one segment per
request, derives its segment offsets and final flag from the source length, and
advances only after the matching successful QMI status response. The observed
26,328-byte Sunfish board-file size exercises seven segments (six 4,023-byte
segments and one 2,190-byte segment) inside the 4,096-byte QRTR datagram bound.
Tests reject board-ID or generation mismatches, stale live identity, small
output buffers, wrong peers or transactions, optional response fields, and
remote rejection without advancing the source cursor. The native modem owner
now routes `begin` and `continue` through the GLINK/QMI session, FIFO adapter,
and live lifecycle gate. An offline owner-through-FIFO test sends a 5,000-byte
test fixture in two BDF requests and advances only after each matching status
response; GLINK data records are completed against one-shot remote intents.
After BDF completion the client now sends and validates the exact empty
CAL_REPORT used by the pinned ath10k reference. The cached ath10k QMI object
has no calibration-data population path, so this reports no calibration IDs;
non-empty calibration reports and calibration downloads remain unsupported.
`VerifiedBdfImage` intentionally has no production constructor until the active
capability board ID is matched to verified device-local board-file bytes and
identity; only test fixtures can mint it.

The native SCM adapter is connected to the MSA path, but it has not been run on
the Pixel. No production WLFW board-data asset source is available, so the
device owner cannot yet mint a usable image token or send a real BDF payload.
Capability discovery, indication parsing, and the test-only BDF transfer do
not load firmware, configure SMMU or radio resources, or establish Wi-Fi
operation.

### Current integrated QRTR HELLO candidate (offline build)

The next image composes the validated receive, its one-shot RX_DONE, and one
fixed QRTR v1 HELLO in a single temporary boot. It selects peer intent 0 only
from the current, fully parsed inventory and requires capacity of at least 52
bytes; it does not reuse the intent, answer the observed request, send service
or WLFW QMI traffic, or configure WLAN/DMA. The 101 focused HELLO-chain tests,
the exact input proof, the AArch64 build, and the full image-placement proof
pass. The proof confirms disjoint source/relocated ranges and copy targets,
preserved boot DTB, and no overlap with the checked fixed reservations. During
the 2026-10-21 rebuild the ELF fingerprint needed refresh; the rebuilt ELF kept
the exact pinned 11-region copy map before the placement proof was rerun.

Current candidate: `.cache/pixel-sunfish/modem-qrtr-hello-probe/fastboot.img`,
41,205,760 bytes, SHA-256
`2fd686fa7b4d70113ec47f4841b2144bd98f2f63ad943e4eabd6fce2a4c87370`; native
PD ELF SHA-256
`4b3e23f1c12e7017e4ffd5d7345a1281897174c8875866e25fc25266d3365593`. Fastboot
temporarily booted this exact candidate on 2026-10-21; no partition was
flashed. The device photo confirms `HELLO TRANSPORT PASS`, error0, TX
consumed1, completion command `0x0e`, and intact guards. RX tail remained
`0x290` with 16 control bytes available/captured; parsing found two records,
including one 52-byte receive request. The screen confirms no service/WLAN
QMI and no host DMA. This establishes QRTR HELLO transport completion, not
service discovery or Wi-Fi operation. Photo SHA-256:
`c5c1e807003a6f8d9192d55d4258f2652e1fc6306545c044bc4669d7eb8ac4f7`.

### Latest bounded receive-copy observation (2026-10-21)

The `IPCRTR FIRST RECEIVE / RECEIVE COPY PASS` screen reports error zero,
guard stage 9, one TX descriptor consumed, RX head `0x290`, and RX available
`0x288`. It reports data command 9, remote CID 2, local intent 1, a 52-byte
chunk copied completely with one intact buffer guard, valid QRTR v1 shape,
version 1/type 2, and QRTR body size 20. The source is node 0/port
`0xfffffffe`; the destination
is broadcast node `0xffffffff`/control port `0xfffffffe`, with body word 0 equal
to 2. The screen explicitly says RX tail unchanged, no RX_DONE or QRTR TX, and
no host DMA or radio. This validates a bounded copy and parse only; it does not
authorize transmission. The integrated HELLO candidate above independently
requires a fresh owned receive/completion checkpoint and a current fully parsed
peer-intent inventory before its one fixed outgoing HELLO.

### Sunfish QCACLD Copy Engine descriptor baseline (offline, 2026-10-21)

The WLAN data-path baseline now follows the installed downstream driver rather
than reusing ath10k's WCN3990 encoding. The matched Lineage kernel source is
pinned at commit `28f9290ae067d3e7857126eb6312720dbe9c7c80`;
`sunfish_defconfig` enables `CONFIG_ICNSS`, which selects the SNOC path, and
QCACLD Kbuild defines `QCA_WIFI_3_0` plus `QCA_WIFI_3_0_ADRASTEA` for this
target. Its `CE_src_desc` and `CE_dest_desc` are 16 bytes,
8-byte ring-aligned, and encode a 37-bit DMA address, 16-bit length, and
16-bit metadata/transfer ID. The bounded, `unsafe`-free codec and software
ring cursor are in [`qcom_ce_v3.rs`](../rust/src/qcom_ce_v3.rs). It rejects
out-of-range addresses, lengths, transfer IDs and ring indexes; requires a nonzero
completion length, unchanged posted address, sufficient buffer capacity, and
current slot generation; and preserves the one-empty-slot rule. It does not
map memory, allocate DMA, access registers, acknowledge IRQs, or start WLAN.

Nine focused tests cover little-endian TX/RX descriptor words, descriptor
bounds, completion races and capacity, ring wrap/full behavior, stale tickets,
and generation exhaustion. `platforms/sunfish/test-modem-qmi.py` now includes
this module in its AArch64 `no_std` compilation harness; the full offline
suite, including its owner/GLINK/SCM/storage tests, passes. The source audit
also found that QCACLD's standalone `ce_send_single()` writes only descriptor
words 0 and 1 after filling a local structure whose length and transfer ID are
in word 2. The pinned kernel has no in-tree caller of its exported
`hif_send_single()` wrapper; the `ce_send()` send-list path uses
`ce_send_nolock_legacy()`, which copies the full descriptor. The codec follows
the declared v3 layout and that full-structure path, not the unused helper.
Possible callers outside this kernel tree remain unknown.

The next offline layer is [`qcom_ce_v3_regs.rs`](../rust/src/qcom_ce_v3_regs.rs),
a typed register map and host-testable bus adapter. The Sunfish build selects
Adrastea (`CONFIG_WIFI_3_0_ADRASTEA`); its CE0 register block begins at offset
`0x240000` within the ICNSS window, with 12 engines spaced `0x1000` bytes apart.
The adapter checks engine and register bounds, denies generic writes to
read-only and write-one-to-clear registers, and only acknowledges known host
interrupt bits present in a fresh status read. It leaves `MISC_IS` read-only
because the pinned QCACLD paths read it but do not establish a clear operation.
Seven offline tests cover the map, access classes, bounds, status filtering and
bus failures. The decoded CE IRQ identity table is also represented as
CE0–CE11 seL4 INTIDs `0x1be`–`0x1c9`; a host test checks the bounded sequential
mapping recorded in the Sunfish device-tree inventory and inherited PDC/GIC decode.
This lookup grants or acknowledges no IRQ. This is still an abstract
`RegisterIo` boundary: it has no MMIO mapping, power/lifecycle guard, IRQ
ownership, DMA authority, or hardware
implementation, and the actual CE engines remain untouched.

The source-derived host CE assignments are captured in
[`qcom_ce_v3_profile.rs`](../rust/src/qcom_ce_v3_profile.rs). Its bounded IOVA
planner lays out the configured 16-byte descriptor rings with the same
8-byte allocation allowance used by QCACLD. The Adrastea mission profile has
nine rings and needs 101,192 bytes for those descriptor allocations; the
explicit low-memory no-pktlog profile has eight rings and needs 73,536 bytes.
Four focused profile tests verify both tables, layout, bounds and failure
paths. This is not the complete WLAN DMA budget: RX packet buffers, firmware
memory, MSA, and runtime transport buffers are excluded. Epping and other
target profiles are not modeled.

The next step, [`qcom_ce_v3_init.rs`](../rust/src/qcom_ce_v3_init.rs), turns a
planner-produced ring set into a complete 12-engine register-write plan. Its
field updates follow QCACLD's `ce_legacy_src_ring_setup()` and
`ce_legacy_dest_ring_setup()` order: base low/high, ring size, source maximum
length, then low/high watermarks; high-address and control fields preserve
unrelated bits. It rejects a profile mismatch, missing/duplicate/out-of-pool
rings, overlap, stale lifecycle generation, unready DMA assertions, or any
engine whose sampled ring sizes or indexes are nonzero, or whose snapshot
generation differs from the lifecycle generation. Before the first write for
each engine, the live owner must compare the captured empty snapshot with the
current register state. The evidence values are assertions for a future live
owner, not proof that power, reset, MMIO, ownership, or translation are valid.
The `SetupApply` executor sends each write through one `SetupWriter` call and
latches closed on lease loss, a failed write, or an unwind, without retrying or
starting the engines. It does not allocate DMA or configure IRQs.

[`qcom_ce_v3_native_io.rs`](../rust/src/qcom_ce_v3_native_io.rs) adds the next
native boundary. It accepts only the exact 8 MiB ICNSS device mapping at
physical `0x18800000`, checks the live generation and lifecycle on construction
and every register access, and requires the translated masked route for both
WLAN SIDs plus an exclusive uncached DMA aperture below the CE's 37-bit limit.
The raw adapter allows only ring setup and snapshot registers; it cannot write
the CE command or interrupt registers. `SetupApply` uses this adapter without
nested gate acquisition: each write holds one live guard while it checks the
exact planned IOVA window, and the first write to each engine compares the live
register snapshot under that same guard. Six host tests exercise route/lifecycle
loss, invalid DMA windows, the register allowlist, full-plan application, and
stale-snapshot rejection. The test runner forces AArch64 volatile-I/O code
generation. The `Gate` remains an unsafe platform obligation with no
production implementation, so this code still cannot touch the Pixel or claim
WLAN readiness.

The remaining hardware gates are unchanged: production firmware/board-data
rights and provenance, WLAN power and reset sequencing, exclusive CE/MMIO/IRQ
ownership, a live translated SMMU context and bounded DMA pool, and a native CE
register backend guarded by validated mappings and a live lifecycle lease. The
latest GLINK receive-copy photo is modem transport evidence only; it does not
satisfy any of those WLAN gates.

### ICNSS power sequence executor (offline)

[`qcom_wlan_power.rs`](../rust/src/qcom_wlan_power.rs) now models the pinned
Sunfish ICNSS regulator order, source-derived voltage requests, optional rails,
and the optional `cxo_ref_clk_pin`. It snapshots each WLAN client's existing
brokered contribution before the first change, restores touched resources in
reverse order if an operation fails, and latches an unknown state if restore or
generation validation fails. Eight host tests cover plan order, omitted
optional resources, idempotent start, clean stop, every action failure,
snapshot failure, rollback failure, stale generations, and lease loss. The
module is included in the AArch64 no-std compilation harness.

This is not a production power driver: the latest Lineage baseline reports a
kernel release with the pinned source commit prefix, and its local ICNSS source
file matches a separate audit copy byte-for-byte. The exact compiled ICNSS
binary and configuration remain unverified, so production minting of the
source-match token stays disabled. The shared vote ledger and its ICNSS adapter
are implemented offline, but no live lease backend or trusted Apps RSC
transport exists. The latest phone screen
(`IPCRTR FIRST RECEIVE / RECEIVE COPY PASS`) reports a 52-byte bounded copy
into the reserved local buffer with its guard intact and the RX tail unchanged.
It reports no RX_DONE or QRTR transmission, so this confirms only the copy
stage, not service delivery. ADB on the Mac currently lists only two Android
emulators, so no fresh Lineage runtime identity was available for this step.

### Shared RPMh vote ledger (offline)

[`qcom_rpmh_vote_broker.rs`](../rust/src/qcom_rpmh_vote_broker.rs) adds a
per-command-database-resource ledger over the existing active/sleep max
aggregator. Its records represent already-resolved regulator-device requests
for one RPMh resource, not raw Bluetooth/ICNSS device-consumer requests. The
Linux regulator core first combines consumer voltage/enable state and sums
load for mode selection within each regulator device; only then does the RPMh
provider max-aggregate register values across regulator devices associated
with that same resource. The broker updates one registered device request but
publishes the new
ledger only after a one-shot, plan-and-phase-bound acceptance receipt for every
changed aggregate state. The pinned Linux RPMh API caches `SLEEP` and
`WAKE_ONLY` writes in software and does not send them to the controller until
`rpmh_flush`; active writes are submitted through TCS. The broker tracks
whether a sleep request has already been staged. On the first differing sleep
set it stages sleep and wake values and resends the active set, following
`rpmh-regulator.c`; later active changes stage a new wake value before active
TCS completion. The `qcom,use-awake-state` path stages sleep and uses an awake
write without a separate wake-only request. Sleep and wake receipts mean only
that the bounded owner cache accepted the values; active receipt requires TCS
completion. No phase claims that RPMh echoed register values. Stale plans and
unexpected phases are rejected. A transport error quarantines the ledger,
including after either deferred set was staged. Thirteen host tests cover the
sleep/wake/active ordering, first-sleep behavior, awake-state mode, hidden
lower votes, receipt correlation, failures, timeout, and malformed initial
client sets.

The system-wide `rpmh_flush` path remains unimplemented. It invalidates sleep
and wake TCS state, flushes cached passthrough commands, and then programs
sleep/wake pairs for every resource owned by the shared controller. A
per-WLAN-rail cache cannot safely perform that global operation; it requires
exclusive Apps RSC ownership and a complete bounded ledger for all resources.

The ledger still has no production constructor because all current clients of
each RPMh resource have not been enumerated. It also has no trusted transport
implementation or validated Apps RSC lease, so it does not grant
authority to the shared Apps RSC or enable the WLAN rails.

[`qcom_rpmh_vrm.rs`](../rust/src/qcom_rpmh_vrm.rs) now translates only the
ICNSS plan's fixed voltage and enable actions into the VRM's millivolt and
enable-register vote fields. It preserves the prior mode/headroom fields and
active/sleep selection, rejects any voltage tuple outside the source-derived
profile, and refuses unmodeled load or clock operations. The focused VRM test
harness passes 14 tests including the power-sequencer cases. This still does
not send a TCS command; the separate rail-mapping and owner layers validate
addresses and ownership.

### ICNSS-to-RPMh power adapter (offline)

[`qcom_wlan_rpmh_io.rs`](../rust/src/qcom_wlan_rpmh_io.rs) now composes the
verified ICNSS plan, decoded command database, VRM vote encoding, and
per-resource broker. The separate
[`qcom_wlan_rpmh_map.rs`](../rust/src/qcom_wlan_rpmh_map.rs) resolves the five
source-matched supply keys (`ldoa9`, `ldoc1`, `ldoc2`, `ldoc11`, `ldoc10`) in
ICNSS rail order, accepts only unique VRM slave-3 addresses, and rejects
missing, wrong-type, or aliased entries. Construction also requires the exact
set of mapped rails, matching profile identity and generation for every
broker, agreement between profile, decoded database, and broker addresses, and
an existing WLAN client entry. Each operation revalidates a lifecycle lease. A
changed vote stages the source-required sleep and wake aggregates before the
active aggregate using the same lease, and the broker commits only after those
cache stages and the active TCS transaction complete; transport ambiguity
quarantines that resource and causes the sequencer to stop. Tests verify all
five resource mappings, preservation of another client's contribution, phase
ordering, and fail-closed behavior after a later-phase failure. The focused
CMD-DB/power/broker/VRM harness now passes 68 tests and covers first sleep
staging, wake-cache ordering, and awake-state behavior.

[`qcom_rpmh_vrm_command.rs`](../rust/src/qcom_rpmh_vrm_command.rs) converts a
broker phase into at most four ordered VRM register writes at the matched
resource address plus `4 * register_index`, carrying the plan's identity,
generation, and ticket. It emits only the register fields changed in that
phase, except that the first differing sleep request resends the full active
and wake set as the source regulator does. It checks address arithmetic and
rejects a phase that was not changed or a removed vote field whose hardware
clearing semantics are not implemented. Four tests cover delta and first-sleep
batch contents, unchanged-phase refusal, and overflow/removal failures. This
is a command planner only; it does not touch Apps RSC or claim transport
acceptance or completion.

[`qcom_rpmh_vrm_tcs.rs`](../rust/src/qcom_rpmh_vrm_tcs.rs) maps an active VRM
batch to the pinned Linux active-TCS0 sequence: bounded command slots, wait and
enable masks, then the synchronized control clear/mode/trigger writes. It
requires the observed RSC geometry, an enabled active-TCS0 interrupt, no
pending IRQ status, and an idle TCS0 with empty command/wait masks. Sleep and
wake batches are rejected because they belong to RPMh's deferred owner cache;
the planner never changes the shared IRQ mask, W1C IRQ status, or sleep/wake
banks. Its final trigger has no readback/retry value. Two new tests check the
register offsets, exact write order, deferred-phase refusal, and invalid
controller snapshots.

[`qcom_rpmh_vrm_tcs_io.rs`](../rust/src/qcom_rpmh_vrm_tcs_io.rs) adds a
host-testable transaction state machine on top of that plan. It requires an
exact fresh snapshot and validates the live identity/generation before every
register access. It bounds write readback and IRQ polling, checks the TCS0
issued/complete bits, clears only TCS0's command masks and W1C IRQ bit after a
recognized completion, and never retries a write or trigger. Ambiguous
post-trigger failures stop without guessed cleanup; an observed incomplete
command is reported only after bounded source-matched cleanup. Eleven failure
and success-path tests cover stale snapshots, lease loss, readback failure,
trigger ambiguity, timeout, unrelated IRQs, command status, cleanup, and broker
commit timing.

[`qcom_rpmh_vrm_tcs_transport.rs`](../rust/src/qcom_rpmh_vrm_tcs_transport.rs)
implements the broker's exact-phase interface: deferred sleep/wake batches go
to an injected cache, and an active batch is planned, submitted once, and
accepted only after TCS0 completion. Offline integration tests confirm the
broker does not commit before that completion and deferred phases do not touch
TCS registers. The cache and backend are still injected abstractions; no global
`rpmh_flush`, volatile MMIO backend, shared-controller inventory, production
lease, or IRQ capability is connected.

This is still an offline composition boundary. The active ICNSS identity and
resource map have not been freshly verified, the complete regulator-device
vote sets for each RPMh resource have not been inventoried, and there is no
live exclusive lease or Apps RSC transport implementation. The adapter therefore cannot
authorize or perform a rail vote, and Wi-Fi bring-up remains gated on those
platform facts plus reset, firmware, SMMU, DMA, CE, and IRQ ownership.

After adding the TCS transaction and transport layers, `python3
platforms/sunfish/test-modem-qmi.py` passed. This includes the host routing,
GLINK/FIFO, and storage suites plus the AArch64 `no_std` core/native build and
code-generation checks. The focused RPMh harness passes all 68 tests; rustfmt
and `git diff --check` also pass. These checks use test backends and do not
verify access to the phone's Apps RSC.

### QCACLD WMI INIT body codec (offline, 2026-10-21)

[`qcacld_wmi.rs`](../rust/src/qcacld_wmi.rs) now requires the captured Sunfish
WCN3998 build, an empty host-memory request list, and the pinned QCACLD ABI
major/minor and namespace before it can format WMI INIT. It follows the
source's `init_cmd_send_tlv()` layout: fixed parameters with zero host-memory
chunks, the fixed-size resource-config TLV, an explicit empty host-memory
array, and an optional hardware-mode record with an empty band map. The
encoder parses the resource-config TLV through a borrowed typed view before
copying it into the command body. The view checks the exact 276-byte TLV,
resource-config tag, and 272-byte value length, and exposes pinned wire indices
for vdev/peer counts, chain masks, MSDU/fragment limits, flags, packet filters,
and STA vdevs. Its test uses a distinct value in every one of the 68 wire words
to verify those mappings. The encoder checks output capacity before writing,
and emits only the 312-byte base body or 332-byte single-mode form. The WMI
command ID is transport metadata and is not part of this body. Its bounded
WMI_READY_EVENTID decoder checks the matching ABI, zero initialization status,
and the count and byte length of up to two extra MAC records.

Fourteen focused tests cover the startup identity and ABI gate, exact body
offsets, both optional-mode forms, resource-config wire-field mappings,
malformed resource-config TLVs, undersized output, and truncation and failure
cases for both WMI event decoders. The
resource-config values still need a separately validated Sunfish policy. The
encoder does not send WMI INIT, create host-memory chunks, or establish DMA
ownership; no WMI command or `WMI_READY_EVENTID` has been observed through the
current seL4 path.

The checked Google Android 13 QCACLD source shows why the generic target macros
are not a safe substitute for that policy. Its [WMA resource setup](https://android.googlesource.com/kernel/msm-modules/qcacld/+/4f47232dc5c209b3137ea3ca26657e3853bf0d21/core/wma/src/wma_main.c)
starts from four vdevs, 14 peers, five offload peers, four offload reorder
buffers, two peer keys, and three-chain masks, then recomputes peer/TID counts
from `cds_config_info.max_station` and the detected target revision. Before
sending WMI INIT it also replaces offload-peer and reorder-buffer counts with
`ap_maxoffload_peers + 1` and `ap_maxoffload_reorderbuffs + 1`. Those values
come from host configuration, and this QCACLD branch is not confirmed as the
revision loaded by the current LineageOS image. The captured service-ready event
does not identify them, so no resource-config TLV is yet selected.

The startup proof now also exposes fields from the boundedly parsed captured
service-ready event: RF-chain count, maximum fragment entries, scan-channel
limit, supported MAC count, DBS hardware-mode count, and TX/RX chainmask. These
remain untrusted firmware values; they are not yet converted into a resource
configuration. The byte-matched Lineage module build uses `HIF_SNOC` and has no
`CONFIG_HL_SUPPORT`, so resource defaults from the SDIO/USB high-latency profile
cannot be reused. The final resource values also depend on effective host
configuration and service-ready capabilities, which this receive-copy screen
does not contain.

A follow-up audit used the pinned LineageOS kernel at
`28f9290ae067d3e7857126eb6312720dbe9c7c80` and the public
[`WCNSS_qcom_cfg.ini`](https://github.com/LineageOS/android_device_google_sunfish/blob/52074bf3c0b66efcba89f557e605535feb29aaab/WCNSS_qcom_cfg.ini).
The file is 14,079 bytes and its SHA-256 matches the configuration observed on
the Pixel (`60953ffde121e5349e4ea83dc6a7da135cdd9229da3bcb6303cd70654a9e5603`).
It does not set `gSoftApMaxPeers`, `gMaxOffloadPeers`,
`gMaxOffloadReorderBuffs`, or `max_scan_count`. The pinned [QCACLD config
defaults](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qcacld-3.0/core/hdd/inc/wlan_hdd_cfg.h)
are 32 peers, 2 offload peers, 2 reorder buffers, and 4 scan requests; the
[target defaults](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qcacld-3.0/core/mac/inc/wlan_tgt_def_config.h)
set four vdevs for this non-high-latency profile. The pinned [WMA setup](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/staging/qcacld-3.0/core/wma/src/wma_main.c)
applies `max_station` after target-revision capping, derives
`num_peers = supported_peers + 4 + 2` and (for this non-high-latency build)
`num_tids = 2 * (supported_peers + 4 + 2)`, and then replaces the offload
counts with the host values plus one. This yields candidate counts of 38 peers,
76 TIDs, 3 offload peers, 3 reorder buffers, and 4 scan requests if the target
uses the 32-peer path. The source has a special 14-peer cap for QCA6174
revision 1.1, so the exact target version and effective host values still need
confirmation before these candidates can be encoded into WMI INIT. This is
source-derived policy evidence only; no WMI command has been sent.

[`qcacld_wmi.rs`](../rust/src/qcacld_wmi.rs) now has
`QcacldWmaHostConfig::derive_resource_counts()`, which implements only those
WMA-derived counts. The peer-cap choice is explicit, host inputs are range
checked, and the function returns a partial count record rather than a TLV.
Four tests cover the 32-peer defaults, the QCA6174 revision-1.1 cap, host
overrides, and invalid bounds. The new
[`validate-qcacld-wmi-resource-policy.py`](../platforms/sunfish/validate-qcacld-wmi-resource-policy.py)
checks those formulas and defaults against the clean pinned Lineage tree.
Target identification, live host overrides, the other resource words, and
firmware capability matching remain required before constructing WMI INIT.

### QCACLD HTC startup and WMI transport codec (offline, 2026-10-21)

[`qcom_htc.rs`](../rust/src/qcom_htc.rs) adds the next host-only stage after
the QCACLD WMI INIT body codec. It parses the exact 12-byte HTC READY_EX
message, validates credit and endpoint limits, and models the ordered
WMI_CONTROL_SVC connect request (`0x0100`), connect response, and zeroed
SETUP_COMPLETE_EX message. Staged control messages do not advance the startup
state until the caller confirms transport submission; a failed submission
returns to the prior stage. Only after setup submission can the codec build a
WMI command header and an 8-byte HTC frame. It enforces the negotiated endpoint
and maximum message size, consumes the HTC header in credit cost, rejects
insufficient credits before writing, and marks a credit-update request at the
same low-credit threshold as the pinned driver. The initial WMI credit
allocation follows the single WMI endpoint initialized by this Sunfish QCACLD
build; it rejects a value that would narrow the driver's eight-bit allocation.

The combined harness composes a structurally accepted Sunfish service-ready
fixture, the 312-byte WMI INIT body, and one HTC-framed WMI INIT command. Its
33 tests include all 14 WMI codec tests plus 19 HTC/framing/startup cases for
field layouts, every READY truncation, malformed response fields, endpoint
bounds, credit accounting, state ordering, failed-send rollback, and output
buffer canaries. `python3 platforms/sunfish/test-modem-qmi.py` also compiles the
module in its AArch64 `no_std` core. This remains protocol-only: it does not
send a HTC control message, reserve a live credit, receive
`WMI_READY_EVENTID`, or establish DMA/IRQ ownership. Before device bring-up,
the WMI resource policy, runtime CE/service mapping, RX credit/trailer
handling, firmware/board data, power/reset ownership, and translated DMA
aperture still require validation.

### Offline WMI to CE3 transmit-plan composition (2026-10-21)

[`qcom_wmi_ce.rs`](../rust/src/qcom_wmi_ce.rs) joins one HTC-framed WMI packet
to the CE v3 primitives without publishing it. The caller supplies the
target-selected service-to-CE table, negotiated HTC endpoint and transmit
plan, DMA IOVA pool and packet window, selected CE profile and its complete
ring plan, and a software CE3 source-ring cursor. The planner requires exactly
one WMI_CONTROL_SVC host-to-target row on CE3 and one target-to-host row on
CE2; checks all profile rings for expected counts, pool bounds, and overlap;
checks HTC/WMI lengths, endpoint, reserved fields and credit-update flag;
and ensures the packet window is aligned, large enough, within the pool, and
disjoint from every ring allocation. It returns the CE3 descriptor bytes,
slot IOVA and next producer index, but does not touch packet memory, MMIO,
cache state, or the cursor. The native owner must still hold a live exclusive
DMA/CE lease, copy the descriptor, order the writes, publish CE3's producer
index, and commit the cursor only after successful publication.

The focused composition harness passes 29 tests with warnings denied. The
route values are validated inputs, not a fresh runtime observation of the
firmware-selected map; the transfer-ID policy and target-specific CE profile
also remain caller responsibilities. This does not send a WMI command or
prove that the QRTR HELLO on the latest Pixel photo triggered RX_DONE or a
modem reply.

### Gated native CE3 WMI publication (offline tests, 2026-10-21)

[`qcom_ce_v3_native_io.rs`](../rust/src/qcom_ce_v3_native_io.rs) now has a
single-packet WMI publication entry point behind its existing unsafe live
Gate. Under one pinned lease it rechecks CE3 ring size and high address bits,
hardware read/write indices against the software cursor, DMA IOVA bounds,
the descriptor slot, and packet/ring disjointness. It copies the framed HTC
packet and encoded CE descriptor through the uncached translated mapping,
issues the AArch64 system barrier, then writes only CE3's producer index and
commits the cursor. The generic register API continues to reject producer
index writes. The native plan privately borrows the already-validated frame,
so safe code cannot substitute different packet bytes after planning. The
live observation must also attest CE3/CE2 WMI routes for the same boot
generation. Six new native-harness cases verify the copy and index result,
and show lifecycle loss, stale route generations, stale cursors, and invalid
plans stop before DMA or MMIO changes; the new focused harness passes 45
tests with warnings denied.

The offline code-generation check now includes the native publisher, and the
full `python3 platforms/sunfish/test-modem-qmi.py` run passed after this
addition. The Gate still has no production implementation, and no phone-side
publication occurred. No WMI_READY_EVENTID, CE completion interrupt, RX
buffer post, firmware download, or WLAN startup is evidenced.

### Offline CE2 WMI receive-post planning (2026-10-21)

The same [`qcom_wmi_ce.rs`](../rust/src/qcom_wmi_ce.rs) module now validates a
CE2 destination-ring post for a WMI receive buffer. It checks the selected
CE2/CE3 service route, full profile ring plan, CE2 cursor size, DMA bounds,
alignment and ring overlap before returning the zero-length receive
descriptor and destination slot. A fixed 127-entry buffer ledger rejects
overlapping leases and retains each window until the caller releases it after
a validated completion. The 31-test focused harness includes duplicate and
partial overlap, ring overlap, capacity, and full-ledger cases; planning does
not write the DMA mapping or producer index.

### Gated native CE2 WMI receive posts (offline tests, 2026-10-21)

The lifecycle-gated native adapter can now post a planned CE2 receive buffer.
It rechecks the same-generation WMI route, CE2 ring geometry and live indices,
the DMA aperture, and buffer-ledger ownership; zeros the private receive
window; writes the zero-length destination descriptor; orders the stores; and
publishes only CE2's destination producer index. It records the buffer lease
before publishing and blocks generic register writes to both CE3 source and
CE2 destination producer indices. The focused native harness now passes 48
tests, including duplicate-buffer and stale-route cases that make no new DMA
or MMIO changes. AArch64 code generation and the full offline suite are the
remaining validation for this addition.

### Bounded native CE2 WMI completion and buffer retirement (offline tests, 2026-10-21)

The native CE adapter now polls CE2's live destination read index under the
same lifecycle and route gate, checks that progress remains inside the posted
descriptor window, reads the exact oldest descriptor through the bounded DMA
mapping, and accepts only its reserved IOVA with a nonzero length within the
buffer capacity. It copies only that many payload bytes to the caller and then
retires the matching cursor ticket and buffer-ledger entry. A receive buffer is
now associated with its exact descriptor ticket, so a completion cannot free a
different slot's storage. Posting another buffer also tolerates hardware
progress inside the still-unretired window. Base-low ring addresses are
rechecked on the native path while remaining unavailable through the generic
register API.

The focused offline suites pass 31 WMI/CE planning tests and 55 native adapter
tests, including no-progress, zero-length completion, undersized output,
wrong-buffer address, stale route, duplicate buffer, receive posting after
hardware progress, and successful copy/retirement cases. The successful fake
DMA case then runs the copied HTC/WMI frame through the new bounded
`parse_wmi_frame` codec; its receive-side credit-update flag and borrowed TLV
body are checked. The warning-clean full offline suite passes, including
AArch64 no-std codegen and volatile/barrier checks. No production Gate exists.
The latest Pixel screen reports `RECEIVE COPY PASS` for the 52-byte HELLO,
with RX tail unchanged and no RX_DONE or QRTR transmit; it does not demonstrate
completion progress.

The matching LineageOS kernel source was inspected at commit
[`bb17b7d`](https://github.com/LineageOS/android_kernel_google_msm-4.14/tree/bb17b7de4b570f2c0c32ddba0543cac03c58dd84). Its legacy CE receive path
computes completed destination entries from the hardware DRRI, checks the
next descriptor's `nbytes`, treats zero as a completion race, then advances
the software index only after a valid descriptor
([`ce_service.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/bb17b7de4b570f2c0c32ddba0543cac03c58dd84/drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_service.c#L1310-L1402)).
The SNOC path enables/disables the per-engine CE interrupt via its individual
host-enable register and the tasklet drains pending RX before re-enabling it
([`if_snoc.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/bb17b7de4b570f2c0c32ddba0543cac03c58dd84/drivers/staging/qca-wifi-host-cmn/hif/src/snoc/if_snoc.c#L343-L372),
[`hif_io32_snoc.h`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/bb17b7de4b570f2c0c32ddba0543cac03c58dd84/drivers/staging/qca-wifi-host-cmn/hif/src/snoc/hif_io32_snoc.h#L35-L62),
[`ce_tasklet.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/bb17b7de4b570f2c0c32ddba0543cac03c58dd84/drivers/staging/qca-wifi-host-cmn/hif/src/ce/ce_tasklet.c#L164-L180)).
That path does not show a separate CE status-register acknowledgement, so this
adapter does not add one.

### Bounded native CE2 receive interrupt control (offline tests, 2026-10-21)

`set_wmi_rx_irq` now applies that source-backed operation to CE2 only. It
requires the same live lifecycle and WMI route gate as the receive path, an
exclusive runtime binding for CE2's decoded Sunfish GIC INTID `0x1c0`, the
validated CE2 destination ring and current hardware indices, and at least one
posted descriptor before enabling. The private write path changes only CE2's
per-engine host interrupt-enable register to 1 or 0; generic register writes
cannot reach it. It neither claims nor acknowledges a seL4 IRQ. The warning-
clean native harness passes 55 cases, including exact IRQ binding,
unbound/shared IRQ rejection, enable/disable values, and no-posted-buffer
rejection. The full offline suite and AArch64 no-std code-generation checks
pass. A production Gate and actual IRQ notification handler are still absent.

The validated integrated QRTR HELLO candidate was temporarily booted with
`fastboot boot` on the connected `sunfish` device on 2026-10-21. Fastboot
accepted the 41,205,760-byte image with SHA-256
`2fd686fa7b4d70113ec47f4841b2144bd98f2f63ad943e4eabd6fce2a4c87370`; no
partition was flashed. The resulting photo confirms HELLO transport completion,
with a stable 16-byte control snapshot and one pending 52-byte receive request.
No service/WLAN QMI or host DMA was used. A refreshed next-receive candidate
was built offline from this current image/photo pair at
`.cache/pixel-sunfish/modem-qrtr-next-probe-20261021/fastboot.img` (41,218,048
bytes; SHA-256
`890f9e37d7edca8132fd9e277a5185be5b8aeddde6d8c68648b263bcc0187305`; PD ELF
SHA-256
`f6028b5932227325c4bd798845943cc4a24aa4419351f58284cfff956a06fc6d`). Its
warning-clean build passed 21 offline suites (1,229 test executions), the
task-image/canary check, and the 11-region image-placement proof with no
overlaps and the original boot DTB preserved. Fastboot accepted its temporary
boot on 2026-10-21 (send1.089s, boot4.388s); no partition was flashed. The new
photo confirms `NEXT PACKET COPY PASS`, error0/stage9, and a copied52-byte
QRTR-v1 type4 frame. Its exact raw body is `[4,0x42,0xb401,0,3]`: service
`0x42`, opaque instance word `0xb401`, node0, port3. The packet is not WLFW
service `0x45`; the preceding control bytes, frame and trailing8 bytes remain
unconsumed. The corrected observation is pinned at
`.cache/pixel-sunfish/modem-qrtr-next-probe-20261021/native-observation.json`.

The bounded service-inventory candidate was rebuilt from that dated evidence
and exact image using `SUNFISH_MODEM_QRTR_NEXT_INPUT_DIR`. Its 22 offline
suites pass (1,391 test executions), the task-image/canary check passes, and
CapDL reports zero overlaps; the packed loader proof preserves the original
boot DTB and keeps all11 copy targets disjoint. Fastboot accepted its
temporary boot on 2026-10-21 (send1.097s, boot4.272s); no partition was
flashed. The user photo shows `DISCOVERY STOPPED`, timeout `0x10`, stage7,
two service announcements, and an empty RX ring at `0x338`; both guard checks
pass. The last record is service `0x2b`/instance `0x1202`/node0/port1. The
matching LineageOS kernel source defines service `0x42` as SERVREG_NOTIF,
`0x2b` as SSCTL v2, and WLFW as `0x45`
([SERVREG_NOTIF source](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/service-notifier-private.h),
[SSCTL source](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/sysmon-qmi.c),
[WLFW source](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/wlan_firmware_service_v01.h)).
WLFW was not seen in this timed sample; this does not establish global
absence or why `wlan_pd` did not announce. The exact observation is pinned at
`.cache/pixel-sunfish/modem-qrtr-discovery-probe-20261021/native-observation.json`.
This candidate sent no service/WLAN QMI and set up no host DMA.

### Bounded SERVREG mapping response probe (2026-10-21)

The next device check uses the already implemented manifest-backed
`GET_DOMAIN_LIST` reply for the exact `tms/servreg` request. It can answer once
with the captured `root_pd` and `wlan_pd` rows; it does not answer PDR queries,
send WLAN QMI, or set up DMA. This specifically tests whether the modem asks
for and accepts the service-to-domain map before advertising WLFW. The offline
build passed the focused QMI/SERVREG and native code-generation suites, its
task-image/canary validation passed, and CapDL reports zero overlaps. The
packed image proof keeps the source, relocated loader, fixed reservations and
copy targets disjoint and preserves the original boot DTB. Fastboot confirmed
product `sunfish` and temporarily booted
`.cache/pixel-sunfish/modem-qrtr-servreg-map-probe-20261021/fastboot.img`
(SHA-256
`b682ded1fd242d3531b1b0c7ff9fad204f6ee8f80bd08b7c830f08b4e61f86e6`); no
partition was flashed. The photo shows a `PDR QUERY DECODE`: the exact request
was `GET_DOMAIN_LIST` for `tms/pdr_enabled`, source node0/port9 to node1/port
`0x4000`, QMI transaction1/message`0x21`, TLV1 length15. The profile did not
answer this request, preserved RX tail, and sent no WLAN QMI or DMA. This is an
unsupported-shape stop for this image; it does not show that the two-domain
`tms/servreg` map was rejected. The exact evidence is pinned at
`.cache/pixel-sunfish/modem-qrtr-servreg-map-probe-20261021/native-observation.json`.

The exact PDR-specific empty-map candidate at
`.cache/pixel-sunfish/modem-qrtr-pdr-reply-probe/fastboot.img` was checked
against its pinned SHA-256
`88460e65f7cce44bf4da499875c6e12c026e10754100f1c2f42145855b0ad36e`, enabled
PDR-reply profile, and passing packed-image proof, then temporarily booted on
`sunfish`. The user photo shows `QRTR PDR FOLLOW-UP / EMPTY MAP PASS`, error0,
with RX head `0x3a8`, TX head `0x180`, and RX tail `0x398`. A stable, unchanged-
tail prefix contains `RX_DONE_W_REUSE` command14/channel2/intent2 followed by
`RX_INTENT_REQ` command7/channel2/requested size64. The image sent no second
reply. This confirms the exact `tms/pdr_enabled` empty-map response was
consumed and establishes the next fresh-intent prerequisite; it does not show
WLAN startup. The screenshot and parsed fields are pinned at
`.cache/pixel-sunfish/modem-qrtr-pdr-reply-probe/native-observation.json`.

A separate user photo of `GLINK VERSION EXCHANGE / WAITING FOR VERSION ACK`
shows error0 and zero staging, head/tail stores, notifications, TX/RX progress,
and samples. Its artifact hash is unknown. It is an intermediate zero-counter
display, not evidence that a VERSION offer was submitted or that an ACK timed
out; it does not replace the earlier pinned native VERSION result.

The matching bounded PDR-follow-up image was rebuilt at
`.cache/pixel-sunfish/modem-qrtr-pdr-followup-probe/fastboot.img` with SHA-256
`de2d73692a5a988c77493509e5493ce6fd1dd6af79cb8e7bdaa4340e1392c3df`. Its 114
focused offline cases pass. The task-image check and packed-image proof pass;
all copy targets and the relocated payload are disjoint, no reservation
overlaps were found, and the original boot DTB is preserved. Fastboot confirmed
product `sunfish` and accepted a temporary boot. This image follows only the
exact 64-byte intent request, handles at most the captured
`tms/pddump_disabled` lookup, then consumes only its exact post-reply
completion and holds RX tail for a bounded observation. The follow-up result
photo shows `QRTR PDR AFTER DONE / RX DONE PASS`, error0, done intent2,
RX-available/captured0, RX head0, and RX tail`0x400`. The 16 displayed
post-completion words are zero and the screen reports no subsequent RX bytes.
This confirms the exact post-reply completion was consumed and the bounded
observation found no third QMI reply. It does not prove that no later modem
traffic exists or that WLAN started. No WLAN QMI, host DMA, partition flash, or
firmware start is part of this probe.

### Offline SERVREG_NOTIF state-query codec (2026-10-21)

Added `rust/src/qcom_servreg_notif.rs` for the source-defined read-only
`QUERY_STATE` request (`service 0x42`, version1, message `0x0021`). The
request encoder emits the QMI_STRING TLV for exactly `wlan/fw` within the
schema's 64-byte name limit. The reply decoder checks QMI response kind,
message ID, transaction, common response TLV length, and optional signed
state enum length; unknown enum values are preserved without being treated as
a known state. The schema and enum values come from the cached LineageOS
Sunfish source at commit
`28f9290ae067d3e7857126eb6312720dbe9c7c80`,
`drivers/soc/qcom/service-notifier-private.h` and
`include/soc/qcom/service-notifier.h`.

The codec and one-flight client tests cover exact request bytes, success with
and without the optional state, error preservation, malformed/duplicate TLVs,
wrong transactions and endpoints, QRTR padding, server removal, generation
changes, and name/output bounds. The client binds only service `0x42` instance
word `0xb401` (instance180/version1), emits one QRTR-v1 datagram to an
authenticated server record, and preserves the peer's `confirm_rx` bit. It is
now connected to the bounded GLINK session, FIFO adapter, and serialized modem
owner API. A dedicated offline exchange injects the exact SERVREG_NOTIF server
announcement, publishes the `wlan/fw` query through GLINK, decodes a matching
`UP` response, and emits QRTR receive confirmation. `test-modem-qmi.py` passes,
including the host suites, offline storage-owner suite, and AArch64 no-std/native
code-generation checks.

The client does not register for state notifications or prove the modem's
current `wlan/fw` state. The earlier captured SERVREG_NOTIF advertisement for
node0, port3/instance180 is historical and must be revalidated in the current
modem generation before querying.

### Dedicated live SERVREG state-query probe (2026-10-21)

Added a separate `sunfish-modem-servreg-state-probe` runtime mode that uses the
native modem owner and GLINK FIFO, waits for the exact SERVREG_NOTIF server in
the current generation, sends one `QUERY_STATE` for `wlan/fw`, accepts only the
matching response, and preserves QRTR receive confirmation. The normal modem
PAS initialize/authenticated boot path runs before this query. This profile
does not register/start WLFW, request WLAN MSA_INFO/MSA_READY, grant WLAN MSA
memory to secure world, map storage or heap pools, map the reserved WLAN MSA
pool, set up WLAN DMA, or issue a second query. It keeps only the fixed
modem-image pool, disjoint SMEM/GLINK resources, and the read-only WLAN MSA
no-map reservation in the system description. Its SMC capability is needed by
the existing modem PAS boot path; the SERVREG query itself makes no secure call.

The exact task map is pinned in
`platforms/sunfish/loader-task-regions-modem-servreg-state.json`; the system
description SHA-256 is
`25d8bfa5e99914a6c0349b2a9f12d3f8e1b0a9553f6f4013a2ae3d38c9490517`. The
temporary image at
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` has SHA-256
`afef1b0204fb49aaf6e23ef3498b1e0bba9ed1ad22f7f5655aa326d46c5ff3ed`. Its
CapDL audit found zero overlaps and allowed only the 512-page unmapped WLAN
MSA reservation. The task-image manifest, source/live memory placement proof,
DTB preservation check, `git diff --check`, and `platforms/sunfish/test-modem-qmi.py`
all pass. The first temporary boot used a system description without the SMC
capability required by PAS initialization. The displayed
`seL4_InvalidCapability` (kernel error2) occurred before the secure monitor
call executed; it is not a modem response. That prerequisite has now been
restored while WLAN MSA ownership remains unmapped and ungranted. The corrected
build passes its offline tests and layout checks, and
Fastboot accepted its temporary boot. The next photo shows `IPCRTR PEER ACK /
IPCRTR ACK PASS`, error0, one staged/consumed control record, TX tail/head
`0x28`, RX tail `0x8`, RX head `0x248`, and an unchanged RX tail. It reports no
intent advertisement, no QRTR payload, and no WLAN or host DMA. This confirms
the bounded peer ACK checkpoint only; it does not show the SERVREG query or a
`wlan/fw` state. The latest attached HEIC has the same capture ID and content
as the earlier JPEG; the user reports the phone remained on that checkpoint
before returning to Fastboot. The owner path should replace that screen with a
query-state screen after the checkpoint, so the state-query display now has
explicit `ATTACHING NATIVE OWNER` and `STARTING OWNER IRQ LOOP` checkpoints.
Its live status refresh key now includes QRTR control state and displays peer
HELLO and server counts, avoiding a stale `WAITING FOR SERVER` screen when the
control plane advances. The updated temporary image passes the offline
protocol suite and full image/layout validation. Fastboot accepted its new
temporary boot; the current screen is pending. WLAN MSA remains unmapped and
ungranted, and no WLAN DMA or WLFW start is added.

### Bounded GLINK continuation and owner attach trace (2026-10-21)

The repeated `IPCRTR PEER ACK` screen reported RX head `0x248`, tail `0x8`,
available `0x240`, and captured bytes `0x80`. The code correctly capped the
snapshot at 128 bytes, but the continuation guard compared that capture against
the uncapped `available - 88` remainder (`0x1e8`). This left the ACK marked
passed while refusing to mint the `ChannelCheckpoint`, so the SERVREG owner
path was never entered. The guard now checks the bounded expected size and the
reported capture length. A regression test reproduces the `0x240` available / 128
captured case; all 87 focused GLINK channel tests pass.

The next temporary boot advanced to `SERVREG OWNER PATH / ATTACHING NATIVE
OWNER`. That confirms the channel continuation passed, but the photo does not
show owner attachment completing or the SERVREG query starting. Added display
checkpoints inside owner attachment for channel/prelude validation, SMEM
inventory, GLINK revalidation, live gate, FIFO/owner attachment, SERVREG client
registration, QRTR control startup, and RX intent posting. The next live run
should identify the last completed setup stage instead of leaving one broad
attach marker.

The updated native ELF SHA-256 is
`726af6117c829ea54849787c4f44510ae467fc79425471db4ebc4e0f0a8cf694`; the
temporary Fastboot image SHA-256 is
`7cf028c368125db2fdd2cbd7574f72793f38196b206b620073889ca4e3ee3ea0`. The
complete image build and placement validation pass, the CapDL overlap audit
remains zero, and the 512-page WLAN MSA reservation remains unmapped. The
offline modem/QMI suite and the focused owner/GLINK tests pass. This remains a
diagnostic query image: it does not start WLFW, grant WLAN MSA, or configure WLAN
DMA.

### Owner attach failure-stage reporting (2026-10-21)

The photo most recently supplied under the existing capture ID still shows the
older `SERVREG OWNER PATH / ATTACHING NATIVE OWNER` page. It contains no
per-step labels, so it does not identify the current instrumented run's attach
stage. The caller previously replaced a returned attach error with a generic
stop page; it now retains the last stage label on that page and in the debug
log. This is diagnostic-only and does not change channel ownership, the
read-only SERVREG query, or any WLAN/MSA/DMA behavior.

`python3 platforms/sunfish/test-modem-qmi.py` passes, including the AArch64
no-std/native code-generation checks. The target probe rebuild passes its
packed-image and CapDL validation; the overlap count is zero, the original
boot DTB is preserved, and the 512-page WLAN MSA reservation remains unmapped.
`git diff --check` passes. The updated native PD ELF SHA-256 is
`296e364f6ba09df6c1f14b3eccf7bfb03cd107be603c34e09915ca9fc662d601`; the
temporary `fastboot.img` SHA-256 is
`70d4403807ab5b40a72fcc62f6ee94b3feca071449484c777e2d3ec325caf0ed`.
Fastboot confirmed product `sunfish` and accepted this image with `fastboot
boot`; no partition was flashed. The updated run's screen is pending. This
image still does not start WLFW, grant WLAN MSA, or configure WLAN DMA.

### Owner attach stack exhaustion fix (2026-10-21)

The latest photo still showed `ATTACHING NATIVE OWNER`, despite the instrumented
ELF containing the checkpoint strings. Disassembly explains why: the owner
`attach` frame probes about `0x5a0f0` bytes (roughly 360 KiB), and its `init`
caller probes about `0x29d30` bytes (roughly 170 KiB). Their combined stack
requirement exceeds the previous 512 KiB protection-domain stack before the
first attach callback can paint its checkpoint. Both the SERVREG diagnostic
system and the owner system now allocate a 1 MiB stack. The exact task-copy map
and pinned state-query system hash were updated to reflect the loader layout.

The rebuilt temporary image passes the full modem/QMI/GLINK/native suite,
AArch64 no-std compilation, CapDL audit (zero overlaps), and boot-layout
validation; the WLAN MSA reservation remains unmapped. `validate-modem-owner.py`
now requires at least 1 MiB for the native attach call chain, and the owner
offline Microkit/CapDL build passes with the 1 MiB system stack. The native ELF hash is
unchanged at `296e364f6ba09df6c1f14b3eccf7bfb03cd107be603c34e09915ca9fc662d601`;
the new temporary image hash is
`a99e30d8785dce6293534a3d456d4ded99a3600e76f25c6673619023e790cdd1`. This
run is intended to expose the first live owner-attach checkpoint. It still
performs only the bounded read-only SERVREG state query and does not start
WLFW, request/grant WLAN MSA, or configure WLAN DMA. Fastboot confirmed the
`sunfish` product and accepted the temporary boot; no partition was flashed.
The live result screen is still pending.

### Flexible prelude OPEN ordering (2026-10-21)

The new photo shows `OWNER ATTACH STOPPED / VALIDATING OPEN PRELUDE`, before
the SERVREG query or any WLAN action. Comparing the owner validator with the
upstream GLINK OPEN-plan validator exposed an ordering mismatch: the upstream
parser accepts IDs 1 and 2 with the exact `IPCRTR` offer in either position,
but `Prelude::validate` only accepted `glink_ssr` at ID 1 and `IPCRTR` at ID 2.
The owner validator now accepts both already-supported orders, while still
requiring the bounded 7/10-byte name shapes, exact `IPCRTR` name, IDs 1/2/3,
the fixed third OPEN length, and the exact version/OPEN_ACK records. The other
advertised names remain opaque as in the upstream parser. This is the likely
cause of the live stop; the next temporary boot will confirm whether the actual
captured prelude uses the alternate order.

Both order variants and malformed target/ID/name cases pass offline. The full
`python3 platforms/sunfish/test-modem-qmi.py` suite passes, including AArch64
no-std/native code generation; the SERVREG probe image build, exact packed
layout proof, zero-overlap CapDL audit, and 1 MiB owner offline build pass. All
512 WLAN MSA pages remain unmapped. The updated native PD ELF SHA-256 is
`3f44bb4d2118b5507fceee0acec7c92d36c20e71bb2f60fc1af1b69789e90645`; the
temporary `fastboot.img` SHA-256 is
`05948a442f19ff3689c67cc88fe00c228112c371fc6836a008f59e49ed55698b`.
Fastboot verified product `sunfish` and accepted the temporary boot; no
partition was flashed.

The next photo shows the owner passed attachment and reached the SERVREG state
probe, but stopped in the QRTR control loop: `HELLO SENT=1`, `PEER HELLO=0`,
and `QRTR SERVERS=0`. QMI result/error and firmware state remain `0xffffffff`,
with `NO RESPONSE YET`. The client sends its one QMI query only after seeing
the matching SERVREG server announcement, so this screen indicates the query
was not sent. It does not show a QMI response failure. The OPEN-order fix let
attachment advance, although this screen does not reveal which actual OPEN
order the peer used. The more detailed result photo later identified the
latched cause as `QRTR BAD PAYLOAD`; the SERVREG client remained `SEARCHING`.

### QRTR owner-stop diagnostics (2026-10-21)

The owner status screen did not expose the latched transport error, leaving
the missing QRTR peer HELLO unexplained. The state probe now shows a bounded
stop-reason label derived from the retained owner error and the current
SERVREG client state. This adds no receive, transmission, or WLAN authority.
The full modem/QMI suite and AArch64 no-std/native compilation pass; the
probe image build, packed-image proof, zero-overlap CapDL audit, and owner
offline build pass. The 512 WLAN MSA pages remain unmapped.

The updated native PD ELF SHA-256 is
`696f9c91ace723c47c16f3c9532277918c700cf3c8398e9f7d0637cb769d15cb`; the
temporary `fastboot.img` SHA-256 is
`d9a2be1d40ea21ee4b0420d9a1bad8344cc6a1c828014dc8f1c175a39c73fe5b`.
Fastboot verified product `sunfish` and accepted this temporary boot; no
partition was flashed. The photo later showed `QRTR BAD PAYLOAD`; the decoder
compatibility fix and its updated image are described below. No SERVREG QMI
query has yet been confirmed, and this image does not start WLFW, request/grant
WLAN MSA, or configure WLAN DMA.

### QRTR control union compatibility (2026-10-21)

The settled screen showed `QRTR BAD PAYLOAD`, `QRTR SERVERS=0`,
`PEER HELLO=0`, `HELLO SENT=1`, and `CLIENT SEARCHING`. No SERVREG QMI query
had been sent. Comparing the decoder with Linux QRTR showed that it required
unused command-union bytes to be zero for HELLO, BYE, EXIT, and PING, and for
the unused tail of DEL_CLIENT. Linux defines those bytes as a union and its
control handler reads only the command for the first four commands, and only
the node/port pair for DEL_CLIENT. The decoder now ignores those unused bytes
while keeping the fixed 52-byte datagram, source/destination checks, DEL_CLIENT
node/port validation, and all server endpoint checks. This follows the
[QRTR UAPI structure](https://codebrowser.dev/linux/linux/include/uapi/linux/qrtr.h.html)
and [kernel control handling](https://android.googlesource.com/kernel/common/+/cuttlefish-4.14/net/qrtr/qrtr.c).

Offline QRTR control tests cover nonzero unused union bytes and still reject
malformed routes and server endpoints. `python3 platforms/sunfish/test-modem-qmi.py`
passes, including host tests and AArch64 no-std/native code generation. The
SERVREG probe rebuilt; the packed-image placement proof passes, the CapDL audit
reports zero overlaps, owner mapping/alias/IRQ checks pass, and all 512 WLAN
MSA pages remain unmapped. The task manifest now pins native PD ELF SHA-256
`8ee90039e8a1f6e7b6548ec288cbe3799b1159af6590b54d714c634b825fa2dc`; the
temporary `fastboot.img` SHA-256 is
`a0e439372b4164004d16797ecf8698e58e6986ba9b3fcdffb272b788ff48109c`.
Fastboot accepted the image using a temporary boot only.

The new result photos show one `WAITING FOR SERVER` state with `NO OWNER ERROR`
and another `QUERY STOPPED` with `FIFO TRANSPORT ERROR`; both show zero QRTR
servers, no peer HELLO, `HELLO SENT=0`, and a searching SERVREG client. The
screens do not identify whether those captures are successive states of one
boot or separate runs. The former QRTR payload rejection did not recur in these
photos, but neither photo confirms that the SERVREG query was sent. The
diagnostic now labels every FIFO-layer error separately (including lifecycle,
cursor, record, and readback failures), so the next photo can distinguish the
specific stop. The image does not start WLAN, request/grant MSA, or configure
WLAN DMA.

### Loader task-manifest mismatch (2026-10-21)

The next temporary boot reported `IMAGE LOAD FAIL C0000001`. This is the
loader's own task-plan check: header word 1 differed. The loader source in the
failed image expected `0x046b6594`, while the packed task manifest contained
`0x046b5504`. The manifest had been refreshed after an earlier image build;
the complete loader and boot image had not then been rebuilt from that updated
manifest. Rebuilding the complete image synchronized the loader expectation
with the packed task table. The image-layout verifier now also checks the
generated loader manifest source against the task manifest and rejects a stale
header before boot packaging. A focused check passes for a matching header and
rejects a deliberately altered word 1.

The complete SERVREG probe build passed, including host tests, AArch64 no-std
compilation, native ELF generation, zero-overlap CapDL audit, owner mapping /
alias / IRQ checks, and the boot-image placement proof. The proof confirms
source and relocated payload ranges are disjoint, fixed reservations are
avoided, the original boot DTB is preserved, and the WLAN MSA pages remain
unmapped. The rebuilt temporary `fastboot.img` SHA-256 is
`0795b556ec6f3f21a562ff80249ad97379c85e57bd90037931d43d82cfd58e9c` and the
native PD ELF SHA-256 remains
`8ee90039e8a1f6e7b6548ec288cbe3799b1159af6590b54d714c634b825fa2dc`. Fastboot
accepted the rebuilt image using `fastboot boot`; no partition was flashed.
The new live result is pending. This image does not start WLAN, request/grant
MSA, or configure WLAN DMA.

### GLINK session-stop detail (2026-10-21)

The corrected image reached the SERVREG diagnostic. The photo shows
`PEER HELLO=1`, `HELLO SENT=1`, `QRTR SERVERS=0`, and `CLIENT SEARCHING`, then
stops with `GLINK SESSION ERROR`. No SERVREG server announcement was accepted,
so the read-only query was not sent and the WLAN firmware state is unknown.
The prior screen had grouped every non-QRTR-control session failure under one
label. The display now maps every GLINK session and QRTR control error variant
to its own bounded label so the next capture can identify the exact failure.

The modem/QMI offline suite, owner mapping/IRQ validation, full temporary image
build, and task-manifest / image-placement validation pass. The current
temporary `fastboot.img` SHA-256 is
`09a015cbfb7e95e9cee5ef422f1ae2ca9ef067533f97f76cec658628e5d534fd`; the
native PD ELF SHA-256 is
`e372517089b1695440c742468694ea57e77ed3305b2636c81b9644e2abada705`. The
probe remains read-only with respect to WLAN: no WLAN start, MSA grant, or DMA
configuration is attempted.

### GLINK RX_DONE_W_REUSE handling (2026-10-21)

The pinned Lineage kernel confirms `RX_DONE_W_REUSE` is not advertised in the
GLINK VERSION feature word. Its receiver sends command 14 when that particular
receive intent is reusable; the peer retains that known intent on command 14
and removes it on command 6. The native owner incorrectly treated command 14
as requiring a separately negotiated feature, which is absent from this GLINK
contract. That unsupported gate is removed. Both completion commands still
must match a busy, previously advertised peer intent and a fully published
transmit; a failed match still halts the session and preserves its diagnostic.

Offline QMI/GLINK/FIFO/owner and AArch64 no-std tests pass, including reuse
retirement and a negative test that rejects reuse for an idle/unknown intent.
The complete SERVREG diagnostic image build, CapDL audit, owner mapping/IRQ
check, and boot-layout proof pass; all 512 WLAN MSA pages remain unmapped. The
native PD ELF SHA-256 is
`7f335e980d9a4e40229b2d20715324bf1f3cd9ead1222e7490838c525fd9bec5`; the
temporary `fastboot.img` SHA-256 is
`cda6ef52b7e3d7a68bfc144c251e5c62a023e03714abe767c2111411006cb517`. The
phone remains on the prior temporary diagnostic boot. The new image has not
been booted yet, and no WLAN start, MSA grant, or DMA setup is performed.

### GLINK completion rejection instrumentation (2026-10-21)

The next photo showed `GLINK BAD COMPLETION` after a valid peer HELLO, while no
QRTR service was discovered. The diagnostic build retained the rejected ID,
reuse bit, branch, and bounded counts of busy intents, partial transmits, and
fully published transmits. The SERVREG screen exposes these fields without
adding protocol work. The initial decoder had three rejection paths, including
a presumed requirement for reuse negotiation. Reviewing the pinned Lineage
kernel showed that last path was based on an unsupported GLINK assumption; the
follow-up change above removes it. The remaining diagnostics distinguish an
unknown/idle peer intent from a completion before its transmit finished.

`python3 platforms/sunfish/test-modem-qmi.py`, owner mapping/IRQ validation,
the full temporary image build, and the packed image/CapDL proof pass. The
changed native PD ELF requires a refreshed task-image length and copy map; the
manifest now pins ELF SHA-256
`987d28d2e6d5c447f9270929d57dcf78936b2c733c8cf7f3a18bd39d84a8f40d`. The
temporary `fastboot.img` SHA-256 is
`21834671b04265bb0a7944ed497b11923d3482db3ce92020fb9d7191d7bf8fe5`. Fastboot
verified product `sunfish` and accepted this temporary boot; no partition was
flashed. The new live result is pending. This diagnostic still sends no WLAN
start, requests or grants no WLAN MSA, and configures no WLAN DMA.

### GLINK completion rejection diagnostic boot (2026-10-21)

After the previous note, Fastboot verified the connected device as `sunfish`
and accepted the updated SERVREG diagnostic image with SHA-256
`cda6ef52b7e3d7a68bfc144c251e5c62a023e03714abe767c2111411006cb517` using
`fastboot boot`; no partition was flashed. This image adds the rejected intent
ID, reuse bit, busy/partial/sent transmit counts, and precise completion
failure to the screen. The result photo is pending. The probe does not start
WLFW, request or grant WLAN MSA, or configure WLAN DMA.

### Conditional SERVREG WLAN-domain restart probe (2026-10-21)

The live `QUERY_STATE` response completed successfully and reported
`wlan/fw=UNINIT` (`0x7fffffff`). The next step adds the pinned
SERVREG_NOTIF `RESTART_PD` message `0x24`: it is allowed once, only after the
matching current-generation query succeeds with that exact state. The owner
accepts only a reply from the bound SERVREG endpoint with the matching QMI
transaction, and exposes its result/error on the screen. It sends no retry,
WLFW QMI startup request, WLAN MSA grant, or DMA setup.

`python3 platforms/sunfish/test-modem-qmi.py` passes, including the offline
QMI/QRTR/GLINK exchange and AArch64 no-std compilation. The dedicated build
profile `platforms/sunfish/build-modem-servreg-restart-probe.sh` passes its
CapDL and packed-image proofs with zero overlaps; the 512-page WLAN MSA pool
remains unmapped. Its refreshed task-image manifest pins native PD ELF SHA-256
`850c5ec12fb0b1d0189f4e9841fb30d4674ef2be7e5e9d356372993f138e4584`. The
temporary image SHA-256 is
`c7d02e751977445cd72fb55a7d47039a5085d7405d3eaf86312fbe259cd63725`.
Fastboot verified product `sunfish` and accepted `fastboot boot`; no partition
was flashed. The live PDR result photo is pending. A successful PDR response
would establish only that the modem accepted the WLAN-domain restart request;
it would not establish WLAN firmware readiness or Wi-Fi traffic.

### SERVREG listener prerequisite after rejected PD restart (2026-10-21)

The live reply to the one-shot `RESTART_PD` was QMI failure result `1`, error
`9`; the same capture reported `FIFO ACCESS ERROR` later. The QMI error is
`INVALID_HANDLE`. The pinned Lineage `service-notifier.c` sequence registers
the SERVREG listener when a server appears, then uses that client handle for
PD restart. At this point, the earlier diagnostic's missing listener looked
like the explanation for the rejection. A later run successfully registered
the resolved `msm/modem/wlan_pd` listener (`0/0`, `UNINIT`) and still received
`RESTART_PD` failure `1/9`; that evidence rules out listener omission as a
sufficient explanation. The specific handle or identity rejected by PDR
remains unknown. The later FIFO error is a separate live-owner failure and
remains unexplained; the response photo had no owner error, so it occurred
after the PDR reply was processed.

The restart probe now follows the observed client order: a successful
`QUERY_STATE` must report `UNINIT`, then an enabled `REGISTER_LISTENER` must
return success and still report `UNINIT`, and only then can the client publish
one `RESTART_PD`. Failure or a state change suppresses restart. Offline tests
cover the exact request/reply bytes, the transaction-bound GLINK/QRTR exchange,
changed listener state, and the invalid-handle failure path. This sequence
still does not send WLFW startup, grant WLAN MSA, or configure WLAN DMA.

The corrected image passes the full offline QMI/GLINK/FIFO suite and AArch64
no-std/native compilation. Its CapDL audit reports zero overlaps and leaves the
512-page WLAN MSA reservation unmapped; the packed boot-layout proof confirms
all copy targets are disjoint and the original DTB is preserved. The refreshed
manifest pins native PD ELF SHA-256
`bd5bb92d2128574965b4bf2a9356846f19a1dcaed98ebcfdcfb6120e49eccf41`; the
temporary image SHA-256 is
`d8e99dba4111c01cfd2d2d42f80fb4669a82f6c323a05fec53a1473b76526eac`. Fastboot
verified `sunfish` and accepted `fastboot boot`; no partition was flashed. The
live listener/PDR result is pending.

### SERVREG listener accepted; live owner gate diagnostic (2026-10-21)

The two result photos show `REGISTER_LISTENER` completed with QMI result/error
`0/0`, and the client reached `LISTENER READY`; the WLAN firmware state remains
`UNINIT`. The single PDR restart was not sent. The later screen reports
`FIFO ACCESS ERROR`, so the fresh live owner check failed before request staging.

The diagnostic previously collapsed all live SMEM validation failures into
that one transport label. The owner now preserves the specific validated-gate
error code and displays it as `OWNER DETAIL` after a FIFO access failure. Codes
1–22 are the existing `qcom_modem_status::Error::code()` values; `0x1001`
means the fixed GLINK allocation identities changed and `0x1002` means the
directory route changed. Zero means the backend could not provide a detail.
The gate remains fail-closed and still sends no PDR request after an error.

The updated offline QMI/GLINK/FIFO suite passes, including the diagnostic
propagation test and AArch64 `no_std` code generation. The build and packed boot
layout proofs pass with zero CapDL overlaps; the 512-page WLAN MSA reservation
remains unmapped and the original DTB is preserved. The native PD ELF SHA-256
is `5f89f5ece716ff79ab566870bb54546627cfb5290b41cb3f5f6604e5ed4d92f1`; the
temporary image SHA-256 is
`fab3934c6d1f1f2a944bae722a55b71efbee63a27800210abb3d7f18e661c9bf`. Fastboot
verified product `sunfish` and accepted `fastboot boot`; no partition was
flashed. The detailed live gate result is pending. No WLAN startup, MSA grant,
or DMA setup is performed.

### SERVREG listener-first PDR attempt (2026-10-21)

The live gate detail decoded as `qcom_modem_status::Error::Fatal` (`0x0e`).
The owner therefore stopped before staging another request; it did not send
`RESTART_PD`. This is consistent with the earlier passive trace, which saw
SMP2P status change from `0x6` to `0x7` at about 38.9 seconds and captured the
modem's stalled-initialization/watchdog reason. The reason names an image-owner
triage dependency, but does not identify a missing host service or prove a
root cause.

To follow the pinned Linux order and reduce one QMI round trip, the client now
registers the enabled `wlan/fw` listener as soon as the exact SERVREG server is
announced. It uses the listener response's current state and may send one
`RESTART_PD` only after a successful response says `UNINIT`; the native live
owner check must also pass immediately before publication. An optional
`QUERY_STATE` path remains available for diagnostics, while this image uses the
listener-first path. Offline QMI/GLINK/FIFO tests pass, including listener-first
restart gating, and AArch64 `no_std`/native code generation passes. CapDL has
zero overlaps, WLAN MSA remains unmapped, and the boot-layout proof preserves
the original DTB.

The temporary image SHA-256 is
`00fd64d34393bb43b8ca5b3bd2c80f27f2798f03bc2cea7ce64fd99d6b76e376`; the
native PD ELF SHA-256 is
`7edd0b9f89d5dc897fc204705eb347694cbf0eabb034b217bc69392bf2842e56`.
Fastboot verified `sunfish` and accepted `fastboot boot`; no partition was
flashed. The result screen from this listener-first run is pending. No WLAN
MSA grant, WLAN DMA setup, or WLAN firmware start is included.

### SERVREG map endpoint and clearer listener result (2026-10-21)

The two latest photos are from the prior listener-first build. The first shows
`LISTENER COMPLETE`, QMI result/error `0/0`, and service state `UNINIT`, but
`PDR REQUEST NOT SENT`. Listener detail rows collided with the safety text on
that screen, so the response fields could not be read independently. The
second photo stops at `FIFO ACCESS ERROR`; its live-gate diagnostic is `0x0e`,
which maps to `qcom_modem_status::Error::Fatal`. The owner correctly remains
stopped when that status appears.

The native owner now publishes the bounded, manifest-backed SERVREG domain-map
service after QRTR control starts and dispatches its GET_DOMAIN_LIST requests
through the existing bounded FIFO reply path. Its local QRTR port is `0x4003`,
distinct from storage (`0x4000`), heap (`0x4001`), and the SERVREG_NOTIF client
(`0x4200`). Storage/heap remain unavailable in this probe. The screen now puts
listener result, error, and state on separate rows below the safety lines,
which makes the `PDR REQUEST NOT SENT` gate explainable on the next live run.
The restart-probe startup path now evaluates both the listener-ready condition
and the one-shot restart gate after the initial receive drain, as well as after
later notifications. The gate still requires a successful listener result,
`UNINIT`, and a live owner check; it cannot issue a WLAN start or MSA grant.

`python3 platforms/sunfish/test-modem-qmi.py` passes, including the bounded
SERVREG map exchange, listener-first gating, native owner/FIFO integration,
and AArch64 `no_std` compilation. The rebuilt image passes both CapDL and
packed-image layout validation with zero overlaps; the 512-page WLAN MSA
reservation remains unmapped, and the original boot DTB is preserved. The
native PD ELF SHA-256 is
`dfd747349fb80f5e25c8809d4cba0dcefce56569b8cb8493ae82f976bf27b7c2`; the
temporary image SHA-256 is
`33f8ff97e8fc7bd13c82b7244fe0f750ddbd4a006d4c7b60b69ce07c0b8010e3`.
The current `fastboot devices -l` and USB inventory checks show no Pixel, so
this rebuilt image has not been booted. No partition was flashed, and this
image performs no WLAN MSA grant, DMA setup, or WLAN firmware start.

### WLFW registration and gated MSA_INFO probe (2026-10-21)

The restart profile now enables the bounded WLFW client alongside
SERVREG_NOTIF on separate local QRTR ports. On the exact announced WLFW
service (0x45/version 1/instance 0), the transport may publish its single
source-defined indication-registration request and accept only the matching
peer/transaction response. It may send one bounded MSA_INFO request only after
both that registration response succeeds and the one-shot PDR restart response
returns success. The screen reports each result and the decoded MSA regions.
The event loop stops before SCM memory assignment, MSA_READY, BDF transfer,
DMA setup, or firmware/radio start; the 512-page MSA reservation remains
unmapped.

The full `python3 platforms/sunfish/test-modem-qmi.py` suite passes, including
the coexisting SERVREG_NOTIF/WLFW clients, exact registration exchange,
failure handling, the successful-response gate, and AArch64 `no_std`/native
code generation. The image build passes CapDL and packed-image validation with
zero overlaps; the 512-page WLAN MSA reservation remains unmapped and the
original boot DTB is preserved. The native PD ELF SHA-256 is
`1068045d5224a0a8ac8d9044ecd44450568a3fa6f86053cec5ead75969fc834e`; the
temporary image SHA-256 is
`70a51c5bf0f27c2e7e015c033ab057a8f12589c7c8fad5b304ada45178bd7401`.
Fastboot and USB inventory still show no Pixel, so this build is not yet
device-tested. No partition was flashed.

### SERVREG state check and lease-bound TCS access (2026-10-21)

The latest user-provided SERVREG state screen shows the one `QUERY_STATE`
response succeeded (`QMI RESULT/ERROR 0/0`) and reported `UNINIT`; no PDR
restart was sent. The later live check stopped with `FIFO ACCESS ERROR` and
owner detail `0x0e`, which maps to `qcom_modem_status::Error::Fatal`. This is
the fail-closed GLINK lifecycle gate reacting to the modem's SMP2P fatal bit,
not evidence of a bad FIFO mapping. QRTR had two remote service announcements,
peer HELLO succeeded, and the listener client was ready. No WLAN start, MSA
grant, or DMA setup followed.

The read-only SERVREG state probe displays the service and instance IDs for the
first two remote QRTR announcements. The latest photo showed two services
(`0x42/0x4001` and `0x2b/0x1202`), no WLFW announcement, and stopped on
`GLINK BAD CREDIT` while the SERVREG query remained pending. The QRTR parser had
been treating `RESUME_TX` as a per-reply acknowledgment; the local Android 13
QRTR implementation shows it is a flow-level reset and silently ignores
well-formed acknowledgments for a tracked flow with no outstanding reply. The
bounded session now retains a fixed table of routes it has actually transmitted,
retires only a matching fully published reply, ignores a duplicate/delayed
acknowledgment for a known flow, and still rejects malformed, unknown-route, or
premature credits. Offline regression tests cover these cases.

The captured Lineage baseline independently contains
`/vendor/firmware/modemuw.jsn` for the `modem/wlan_pd` subdomain, listing
`kernel/elf_loader`, `tms/servreg`, and `wlan/fw`. The missing QRTR WLFW
announcement is consistent with the WLAN protection domain not yet being
initialized; it does not imply the firmware/service definition is absent from
the device image.

The full `python3 platforms/sunfish/test-modem-qmi.py` suite passes, including
the updated QRTR cases, native FIFO owner integration, composed storage/pool
tests, and AArch64 `no_std`/native code generation. Fastboot verified `sunfish`
and accepted the temporary boot; no partition was flashed. The rebuilt image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`52f398656afdbd4d25072db1f558220c92155bed7e31027bc7b6cbe18423f6fa`); native
PD ELF SHA-256 is
`d07f3a948531a6b8023614c6cabd8c2bc290be0bc394a3e6e8cdad970ba32277`. Its
packed-layout proof reports zero overlaps, leaves the 512-page WLAN MSA
reservation unmapped, and preserves the original boot DTB. The new live-screen
result is pending. No WLAN MSA grant, DMA setup, or WLAN firmware start has
been performed.

The active-TCS register interface now passes the lease, identity, and
generation into each read and write, with a typed lease-loss result. This lets
a native backend validate ownership in the same critical section as volatile
MMIO instead of leaving a validation-to-access preemption window. A new test
revokes the lease between command staging and readback and verifies execution
stops before later register access or the hardware trigger. The lease-gated
native Apps RSC adapter now validates the exact split mapping and live owner
identity/generation on every access, and restricts reads/writes to the bounded
DRV2 snapshot and active TCS0 registers. Host tests cover invalid mappings,
stale leases, read/write allowlists, and stable snapshot failure. The full
Sunfish routing/storage test script and AArch64 no_std/native code generation
pass. A production RSC owner gate and a complete shared-vote ledger are still
missing, so the adapter does not authorize live regulator writes.

### QRTR flow-credit fix and listener-first restart probe (2026-10-21)

The prior live screen stopped at `GLINK BAD CREDIT` with the SERVREG query
pending. The bounded QRTR session now tracks a fixed set of transmitted flows
and treats a well-formed delayed or duplicate `RESUME_TX` for a known flow as
an idempotent flow reset; it releases only a matching, fully published reply
and continues to reject unknown, malformed, or premature credits. The native
FIFO reply path also records pending metadata only after validating the
request and optional TLVs. Offline regression tests cover delayed credits and
failed reply validation.

The listener-first image now has a separate exact loader task manifest for its
larger native PD, fixing the earlier boot-layout manifest mismatch. It can bind
the exact SERVREG_NOTIF `wlan/fw` server, register one listener, issue at most
one `RESTART_PD` only when the successful listener response reports `UNINIT`,
bind the exact WLFW server and register indications, then request and decode
`MSA_INFO` only after both required replies succeed. Failure or changed live
ownership state stops the sequence. It performs no SCM ownership transfer,
MSA_READY, WLAN DMA, firmware transfer, or radio start.

`python3 platforms/sunfish/test-modem-qmi.py` passes, including the GLINK/QRTR
credit regression, SERVREG client gates, native owner/FIFO integration,
storage/heap failure paths, and AArch64 `no_std`/native code generation. The
loader-source, boot-layout, and modem-image profile tests also pass. The image
build reports zero CapDL overlaps, validates source and relocated payload
separation, preserves the original boot DTB, and leaves the 512-page WLAN MSA
reservation unmapped. Fastboot accepted a temporary boot (no partition was
flashed). The image SHA-256 is
`7679242cc6aa61f10e909a4fc17836aee9b89783d8d5f228b5e9327ee2753656`; the
native PD ELF SHA-256 is
`b6b1fd6dcfafffd1fd8e84e20542682883f3e27ff50b0abd15d05d1980f2f910`. The
settled live result is pending.

The next live photo did not pass the credit path: it remained at
`GLINK BAD CREDIT`, with `CLIENT LISTENER PENDING`, two QRTR servers, and no
PDR request sent. The prior delayed-credit adjustment was therefore
insufficient. `receive_credit` now retains a bounded diagnostic of the exact
13 QRTR words, packet length, tracked-flow result, and reply timing, with a
failure-bit mask. Rejection still holds the receive intent and does not publish
RX confirmation. Offline malformed-route, padding, flag, and premature-credit
tests verify the capture and strict stop behavior.

The diagnostic image rebuild passes the complete `test-modem-qmi.py` suite,
the loader tests, CapDL checks with zero overlaps, and packed-image placement
validation. The 512-page WLAN MSA pool remains unmapped and the original boot
DTB is preserved. Fastboot accepted a temporary boot (no partition was
flashed); image SHA-256 is
`9ca0fa9546c517644c3cdfa10354fa7dadd6ae6a149f68153600722963b47e2b`. Its
`QRTR RESUME DETAIL` live result is pending. No PDR request, MSA grant, WLAN
DMA, or firmware start is issued by this diagnostic.

### Accepting valid unmatched QRTR flow resumes (2026-10-21)

The exact Lineage QRTR receive path (`net/qrtr/qrtr.c`) dispatches
`RESUME_TX` by destination node, then looks up the payload's client node/port
in its transmit-flow table. A well-formed resume for an absent flow is
silently ignored. The previous Sunfish validator instead rejected an absent
locally tracked route, and also used the QRTR header destination port as part
of that decision. This could turn a harmless delayed or unrelated flow reset
into `GLINK BAD CREDIT`; this is a source-backed plausible cause, not yet
confirmed as the specific live packet because the diagnostic screen has not
been photographed.

The bounded session now accepts a structurally valid peer-to-local
`RESUME_TX` when its body client address is not tracked or its header
destination port does not match a pending response. It consumes the control
intent as a no-op and never retires unrelated transmit state. A reply is
retired only when both the body flow key and header route exactly match that
fully published response. Malformed header, command, confirm flag, padding,
and premature matching-response cases remain rejected. Offline regressions
cover unknown body node/port keys and mismatched header destination ports,
then verify a later matching credit retires only its own completed response.
A 12-credit run also verifies each unmatched control packet completes with
RX_DONE and reuses its receive intent without exhausting the bounded slots.

`python3 platforms/sunfish/test-modem-qmi.py` passes, including host protocol,
native owner/FIFO, storage/SCM failure-path, and AArch64 `no_std`/native code
generation checks. The listener-first temporary image builds with zero CapDL
overlaps and a valid packed-image layout; the 512-page WLAN MSA reservation
remains unmapped. Native PD ELF SHA-256 is
`953711967c0c819bcc6730c54b937518a794f311496c9bd529e0ecb7ce0e81c4`; image
SHA-256 is
`dde31b6de188459cfe7032578a4d018ba859177d16899bd098c14f0ebe2c9c67`. The
final `validate-image-boot-layout.py` run confirms disjoint source and
relocated images/copy targets and preservation of the original boot DTB.
Fastboot currently detects no Pixel and ADB sees only the emulator, so this
revision has not been booted. The temporary image still sends no MSA grant,
WLAN DMA setup, or firmware start.

### SERVREG listener rejection wire capture (2026-10-21)

The live captures showed matched listener responses rejected with QMI result
`1`, errors `0x2d` (`INVALID_DATA_FORMAT`) and `0x20`
(`INVALID_REGISTER_ACTION`), no valid service state, and no `RESTART_PD`
request. The screen also reported live owner error `0x0e`
(`qcom_modem_status::Error::Fatal`, peer SMP2P fatal). The bounded codec's
QMI field layout was correct, but its target string was not: it sent the
seven-byte Service Locator alias `wlan/fw` instead of the resolved protection
domain path.

The client retains a fixed-size trace of the exact QMI request and response
bytes, transaction/message IDs, and validated QRTR source/destination. The
current trace decodes to message `0x20`, transaction 1, enabled listener, and
the alias `wlan/fw`; the modem returned error `0x20`. No PDR request was sent.

### SERVREG domain-path correction (2026-10-21)

The pinned Lineage call chain resolves `wlan/fw` through SERVREG_LOC, then
passes each returned `domain_list[i].name` to
`service_notif_register_notifier()`. The listener encoder serializes that
`service_path` as its QMI service-name field, and RESTART_PD uses the same
path. The local manifest map resolves `wlan/fw` to
`msm/modem/wlan_pd`, instance 180; QRTR instance `0xB401` matches that
instance/version. The previous listener request therefore targeted the wrong
name even though its QMI headers, TLVs, service endpoint, and transaction were
valid.

The codec and native client now send `msm/modem/wlan_pd` for QUERY_STATE,
REGISTER_LISTENER, and RESTART_PD. The listener wire request is now 31 bytes
with the 17-byte domain string; buffers and the on-screen trace were expanded
to hold and display the full request. Offline tests assert the exact domain
path and encoded bytes. The next live run will determine whether the modem
accepts the source-derived request; the restart gate remains closed unless a
successful listener response reports `UNINIT`.

The full `python3 platforms/sunfish/test-modem-qmi.py` suite passes, including
the exact SERVREG request bytes, bounded QRTR/GLINK/FIFO paths, offline owner
gates, and AArch64 `no_std`/native code generation. The corrected image passes
CapDL overlap and packed-image range checks: source and relocated payloads,
loader copies, and fixed reservations are disjoint; the WLAN MSA reservation
stays unmapped; and the original boot DTB is preserved. Image SHA-256 is
`dd7065fa3f013203d1ea3bf7eea67e481249cedde6371cacb13395291e432f16`; native
PD ELF SHA-256 is
`42d62d8ed91ca39025b0a11c7f71d78ef8dc70b8b4b4ccfc7650603d706dcb80`.

Fastboot currently sees the Pixel 4a as product `sunfish`, unlocked. The next
step was a temporary `fastboot boot` of this image. Fastboot accepted it; the
result screen is pending. This tests whether the modem accepts the resolved
domain path. It sends at most one `RESTART_PD`, only if the listener succeeds
and reports `UNINIT`, then performs bounded WLFW registration and MSA_INFO
decode. It does not issue SCM ownership transfer, MSA_READY, WLAN DMA setup,
firmware transfer, or radio start; no partition is written.

### SERVREG listener accepted; PDR restart rejected (2026-10-21)

The user's result photo (`559BBA5C-D455-4A1B-8217-E6BF5A7A9A35`) confirms
that the resolved `msm/modem/wlan_pd` listener registration succeeded
(`QMI RESULT/ERROR 0/0`) and reported `UNINIT`. The one transaction-matched
`RESTART_PD` response failed (`PDR RESULT/ERROR 1/9`); standard QMI common
errors identify `0x0009` as `INVALID_HANDLE`. The owner then recorded
`PEER SMP2P FATAL`; `WLFW ERR RESULT` and `MSA ERR RESULT` remained
`0xffffffff`, so no WLFW response or MSA exchange occurred. The temporary
probe did not grant MSA, configure WLAN DMA, transfer firmware, or write a
partition.

The pinned Lineage `icnss_trigger_recovery()` calls `service_notif_pd_restart()`
from recovery paths after PDR registration; it is not the initial WLAN
power-on/start sequence. Since the exact source-derived request was accepted
as a valid QMI transaction but rejected with `INVALID_HANDLE`, repeating the
same one-shot request would reproduce the same failed recovery attempt. The
next engineering step is to trace the actual initial WLAN-domain start and
platform power/ownership path. WLFW registration and MSA_INFO can only follow
once that path brings up the WLFW service; SCM/MSA handoff, DMA, and firmware
start remain gated by the existing SMMU, power, and firmware-provenance
requirements. Fastboot sees device `02281FQC200058`.

The source audit narrows the missing step. In the pinned ICNSS driver, the
normal path waits for the QRTR WLFW server-arrival callback, then connects,
powers on the WLAN rails/clocks, registers indications, requests MSA_INFO,
and continues the firmware setup. `icnss_trigger_recovery()` invokes PDR from
recovery paths; it is not the cold-start trigger. See the pinned
[`icnss.c` server-arrival sequence](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c#L958-L1042)
and [recovery entry point](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c#L2556-L2595).

The device-derived `modemr.jsn` and `modemuw.jsn` maps served by the native
SERVREG LOC endpoint do not advertise `tms/pdr_enabled`; the offline model and
wire tests expect an empty result for that service name. As a cross-check, the
generic upstream Linux `qcom_pd_mapper` has a `sm7150` domain table that maps
`wlan/fw` and `kernel/elf_loader` to `msm/modem/wlan_pd`, while its
`tms/pdr_enabled` mapping appears only in other platform-specific tables. That
upstream table is corroboration, not an exact Sunfish vendor-daemon match, but
it reinforces that PDR restart is not established as this platform's
cold-start mechanism. The local launch-path validator now checks that the
SM7150 table selects `mpss_root_pd_gps` rather than the separate PDR-enabled
variant, while also retaining `mpss_wlan_pd`; this pins the expected empty
`tms/pdr_enabled` result and the `wlan/fw` alias against the audited source.
See [`qcom_pd_mapper.c`](https://linux.googlesource.com/linux/kernel/git/torvalds/linux/+/077ab8985ee278c3d8618182d335b0f0cd919e16/drivers/soc/qcom/qcom_pd_mapper.c#L347-L371) and its [`sm7150` selection](https://linux.googlesource.com/linux/kernel/git/torvalds/linux/+/077ab8985ee278c3d8618182d335b0f0cd919e16/drivers/soc/qcom/qcom_pd_mapper.c#L529-L537).

The Sunfish Lineage init configuration also declares `pd-mapper` in the core
class and `cnss-daemon` in `late_start`, while `wpa_supplicant` is disabled
until requested. This confirms that Android's normal boot includes a CNSS
userspace component absent from this native image; the init declaration alone
does not establish whether that daemon starts `wlan_pd`. It is now a concrete
source path to audit before designing another restart request. See the
[`sunfish init.hardware.rc` service declarations](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.hardware.rc#L2613-L2629).

### Fresh read-only WLAN-domain state check (2026-10-21)

After the PDR failure, Fastboot again detected `02281FQC200058` (`sunfish`,
unlocked). The validated no-restart image
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`772c74626a8fd446bd81ec7f38a1db007cf0e39b78cc603a3cda48db35e0c867`) passed
`validate-modem-image-layout.py` with the scope limited to one resolved
`msm/modem/wlan_pd` SERVREG_NOTIF state query. `fastboot boot` accepted it. The
image issues no listener registration or PDR restart, creates no WLFW client,
and performs no WLAN MSA mapping, SCM transfer, power, DMA, or firmware start.
The user's settled screen shows `QMI RESULT/ERROR 0/0`, `FW STATE 0x7fffffff`
(`SERVICE UNINIT`), `PDR REQUEST NOT SENT`, `LIVE ERROR CODE 0`, and two QRTR
servers (`0x42/0x0000b401` and `0x2b/0x00001202`). `WLFW CLIENT DISABLED` is
expected for this image. The screen still reports `PEER SMP2P FATAL`, so the
prior modem failure survived the transition through Fastboot into this image.
The SERVREG path is therefore responsive, but this run does not establish a
clean cold-start result or an announced WLFW service. A full device power cycle
is needed to clear the persistent peer-fatal state before the next observation.

The follow-up settled screen (`8EE907E3-EAE2-464E-AE3D-AC754F98F1F2`) still
shows `SERVICE UNINIT` and `PEER SMP2P FATAL`. Its SERVREG query reports QMI
result/error `0/0`, but the recorded live error code is now `0x0000000e`; PDR
remains explicitly `NOT SENT`, and WLFW/MSA results remain uninitialized. This
does not change the conclusion: the read-only SERVREG query works, while the
modem WLAN domain has not started. The fatal peer state remains present after
the temporary boot, so a full power-off cycle is required before another
diagnostic boot can establish a clean baseline.

### Revalidated no-restart image for the next cold-start observation (2026-10-21)

I rebuilt the same read-only state-query image from the current sources. The
first build caught that its pinned task-image header and ranges no longer
matched the generated loader; I refreshed
`loader-task-regions-modem-servreg-state.json` from the current build output and
reran the complete build. `test-modem-qmi.py` passes, as do the focused owner
manifest and image-layout tests. The final packed-layout proof reports zero
overlaps, disjoint source/relocated payload and copy targets, the original boot
DTB preserved, and the 512-page WLAN MSA reservation unmapped. Current image
SHA-256 is
`9190446eeba6abf0276135f6578eec11895f420e3063d9dc73e4281524b865da`; this is
temporary-boot only and still performs one read-only SERVREG state query with
no listener, PDR, WLFW, MSA, SCM, power, DMA, or firmware operation.

The state-probe wrapper now explicitly sets
`SUNFISH_MODEM_SERVREG_RESTART_PROBE=0`, so a stale shell export cannot silently
turn the read-only image into the listener/restart variant. I rebuilt with the
environment deliberately set to `1`; the generated profile still records
`modem_servreg_restart_probe=false`, and the output hash is unchanged. The
isolation regression test passes.

### Integrated modem-owner offline build recheck (2026-10-21)

I reran `platforms/sunfish/build-modem-owner-offline.sh` directly after making
the wrapper executable. The AArch64 owner protection domain and complete
Microkit system both build, and the owner-map validator passes. The pool
validator reports 1,799 reserved pages, six unmapped guard pages, and all 512
WLAN MSA pages reserved but unmapped; loader and copy targets remain disjoint.
The resulting offline artifacts have SHA-256 values
`db12900ad931f499e7a0ed51a3265174ef3ae5fa53247c45a83f7e2bc1c4c7de` for
`owner-system.elf` and
`04150fff01b1a75156bde74dd3f193e04054d9fbdea393b6507e572dc063b57f` for
`modem_image_probe.elf`.
This confirms compile-time integration and authority layout only. It does not
start the modem, invoke WLFW/MSA, build a bootable fastboot image, or clear the
device's observed `PEER SMP2P FATAL` state.

### Matched ICNSS WLFW message-order cross-check (2026-10-21)

I rechecked the pinned Sunfish `icnss_qmi.c` startup codecs against the native
WLFW client. The matched ICNSS path registers only its firmware-ready,
MSA-ready, and pin-connect indications; it then performs MSA_INFO, transfers
the validated MSA permission, sends MSA_READY, and requests capabilities. The
source contains no WLFW HOST_CAP transaction. HOST_CAP exists in the separate
`cnss2` driver path, so it is not a missing prerequisite for Sunfish's ICNSS
flow. The existing native client already implements the Sunfish message order;
the current live blocker remains that no WLFW QRTR server is announced while
the WLAN protection domain reports `UNINIT`.

References: [pinned Sunfish ICNSS QMI client](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss_qmi.c)
and [separate CNSS2 QMI client](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/lineage-23.2/drivers/net/wireless/cnss2/qmi.c).

### Sunfish initial WLAN-domain startup path audit (2026-10-21)

The device init files show that Android loads `wlan.ko` from the Sunfish
`init.insmod.sunfish.cfg` module list. The script processes `modprobe` entries
and `enable` entries separately; the Sunfish config's explicit `enable` actions
start ADSP, CDSP, and SLPI, with no corresponding WLAN sysfs write. The
hardware init file declares `pd_mapper` in `core` and `cnss-daemon` in
`late_start`. These are concrete Linux-side startup components, but the public
init files do not expose the proprietary daemon's internal trigger or prove
that it directly starts `wlan_pd`.

The pinned ICNSS driver uses Service Locator to resolve `wlan/fw` and registers
service-state notifiers for the returned domain list. Its
`icnss_trigger_recovery()` checks that PDR is already registered and that the
service notifier handle exists before calling `service_notif_pd_restart()`.
That confirms `RESTART_PD` belongs to an established recovery path; it does not
identify an initial WLAN-domain start transaction. The current native owner
has no equivalent of the Android CNSS daemon's private start behavior, and no
new QMI start request is justified by the available source. The next useful
device comparison remains a genuine cold power cycle followed by the bounded
read-only SERVREG state probe, to see whether the modem clears the recorded
peer-fatal state and announces WLAN on its own.

References: [Sunfish module list](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.insmod.sunfish.cfg#L5-L16), [module loader script](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.insmod.sh#L27-L44), [Sunfish service declarations](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.hardware.rc#L602-L611), [ICNSS Service Locator/PDR registration](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c#L1852-L1953), and [recovery-only PDR trigger](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c#L2414-L2457).

### PD-mapper startup-order lead (2026-10-21)

A September 2026 Qualcomm remoteproc patch reports watchdog timeouts with the
same `DOG detects stalled initialization, triage with IMAGE OWNER` text when
the DSP boots before the PD Mapper service is ready. The later Sunfish trace
now shows that exact full reason string after 38.902 seconds. The patch adds a
pre-soft dependency so `qcom_pd_mapper` is loaded before `qcom_common` starts
the remote processor. Its report concerns Agatti and Arduino UNO Q, not
Sunfish; the exact text match raises the priority of startup-order auditing but
does not prove the mapper is the cause on this device. [Patch
mail](https://lists.openwall.net/linux-kernel/2026/09/01/2243)

The patch author explicitly limits the soft dependency to module-load order:
it does not guarantee that the mapper has finished probing or registered its
service before the DSP starts. The reported devices and firmware stack also
differ from Sunfish, so the identical watchdog text is a useful lead rather
than a transferable fix. In the current native path, `PAS_AUTH_AND_RESET`
precedes GLINK bring-up; `pixel_modem_owner::attach` prepares the local
SERVREG LOC responder only after the GLINK channel checkpoint. The latest
screen shows no QRTR peers or LOC requests, so this run never tests whether the
modem can use that responder. The next offline audit is to determine whether
the required AP-side service can be made ready before reset release without
violating GLINK's fresh-generation and exclusive-ownership rules. Do not infer
that the LOC codec is missing or that a WLAN/PDR operation would resolve the
watchdog.

The native image already contains a bounded, manifest-backed SERVREG LOC
endpoint; this is not an absent protocol implementation. `pixel_modem_owner::attach`
publishes it after the GLINK channel checkpoint, posts receive intents before
advertising it, and `Runtime::start` drains pending work and dispatches a LOC
request before entering the IRQ loop. The processor is already running at that
point, but the source order makes the local service discoverable before the
owner begins normal event processing. Whether the modem attempts an earlier
lookup or whether a response is delayed remains a device question; neither is
proven by the watchdog text. Relevant code: [`qcom_servreg.rs`](../rust/src/qcom_servreg.rs),
[`pixel_modem_owner.rs`](../rust/src/pixel_modem_owner.rs), and
[`sunfish_modem_smp2p_probe.rs`](../rust/src/sunfish_modem_smp2p_probe.rs).

At that point in the investigation, the latest photo still showed `SERVICE
UNINIT`, `PDR REQUEST NOT SENT`, and `PEER SMP2P FATAL`; it did not distinguish
a stale fatal state from a fresh startup-order failure. Later cold-cycle and
fatal-trace observations are recorded in the dated sections that follow.

### Read-only stock WLAN baseline and CNSS daemon symbol recovery (2026-10-21)

The Pixel is currently running LineageOS and is reachable over ADB. Its stock
kernel reports the WLAN firmware-ready event and transitions the WLAN driver
to `ENABLED`; `wlan0` is up. This confirms the hardware and the installed
Android firmware/driver path work together, but it is not evidence that the
native seL4 path has started Wi-Fi.

I pulled only `/vendor/bin/cnss-daemon` and its embedded `.gnu_debugdata` over
rooted ADB for offline inspection. The compressed debug section contains
function symbols despite the executable itself being stripped. The recovered
`main` order is `nl_loop_init`, `cnss_nl80211_init`, `cnss_gw_update_init`,
`wlan_service_start`, `wlan_dp_service_start`, then `wlfw_start`, followed by
the netlink event loop. `wlfw_start` launches a worker running
`wlfw_service_request`; that worker obtains the WLFW v01 service object,
initializes a QMI CCI client for any available instance, registers indication
and error callbacks, and waits for the service. The binary's recovered symbol
list contains no PDR or protection-domain restart routine. This supports the
conclusion that `cnss-daemon` consumes an available WLFW service; it does not
show that it starts an uninitialized WLAN protection domain.

The inspected daemon SHA-256 is
`5ea335e2767ef1b89693f27a725aca17b743871ff8e79f609ce928d87d147772`. The
binary and extracted debug section remain under the ignored
`.cache/pixel-sunfish/vendor-investigation/` directory and are not source-tree
inputs. Device identifiers, network name, and addresses were excluded from
these notes. The outstanding work remains to establish a clean modem startup
baseline after a full power-off and to identify, from verified Sunfish source
or that baseline, how `wlan_pd` is initially made available to the WLFW
client.

I also disassembled the daemon's actual AArch64 code using the recovered mini-
debug symbol addresses. `main` directly confirms the initialization order
above. `wlan_service_start` registers netlink event IDs 262 and 263, while
`wlan_dp_service_start` registers 265, 267, 266, and 271 and reads
`ro.vendor.extension_library`; it resolves `perf_lock_acq` and `perf_lock_rel`
from that library. `wlfw_start` initializes its request worker. The inspected
main/startup functions do not call a PDR or protection-domain restart routine;
the dynamic extension is used as a performance-lock hook, not an identified
domain-start call. The initial `wlan_pd` start mechanism therefore remains
unverified.

I also checked the upstream `linux-msm/pd-mapper` reference. Its `main()` loads
the JSON service map and publishes the SERVREG LOC endpoint; its
`GET_DOMAIN_LIST` handler returns matching static service-to-domain entries.
The PFR dispatch branch only logs the request. This makes the mapper's role
clear: the `wlan/fw` mapping resolves `msm/modem/wlan_pd`, but the mapper code
does not launch that protection domain. This is an upstream reference audit,
not proof that the installed phone binary is byte-for-byte identical. The
initial start trigger remains elsewhere in the Linux/modem path and has not
been identified. See the upstream
[`pd-mapper.c`](https://github.com/linux-msm/pd-mapper/blob/master/pd-mapper.c).

### SERVREG LOC request telemetry ready (2026-10-21)

The native owner now counts incoming SERVREG LOC requests, queued replies, and
transient reply-queue waits. It records the last bounded service-name code and
domain count; codes 1 through 4 mean `tms/servreg`, `gps/gps_service`,
`kernel/elf_loader`, and `wlan/fw`, respectively. The later name-code update
adds 5 for `tms/pdr_enabled` and 6 for `tms/pddump_disabled`; only
`0xffffffff` now means a wholly unrecognized name. The state screen displays
request count, queued-reply count, last-name code, and last domain count. The
FIFO integration test verifies a `tms/servreg` request is recorded as one
request with two domains and that its reply is counted when queued.

`platforms/sunfish/test-modem-qmi.py` passes, including the host routing/GLINK/
FIFO suites and AArch64 no-std/native compilation. The rebuilt read-only
temporary image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`29b2c78eefcb2d949bf0a8758e325ff5711d04c8103b14d1930d34b7400be12b`). The
packed-image validator confirms the 74,221,708-byte relocated payload and all
11 copy targets are disjoint, the original boot DTB is preserved, and the
WLAN MSA reservation remains unmapped. This image performs the same single
read-only SERVREG_NOTIF state query; the new LOC fields only observe its local
map service. The latest photo confirms this image reached the settled state
screen; the observed state and current Fastboot enumeration are recorded below.

### Current host/device mode check (2026-10-21)

Before the cold cycle, `fastboot devices -l` listed the Pixel 4a (`sunfish`),
and `fastboot getvar product` / `unlocked` confirmed `sunfish` and `yes`. After
the user powered the phone fully off for at least ten seconds and returned it
to Fastboot, I temporarily booted the same no-restart image with `fastboot
boot`; Fastboot accepted it and no partition was written.

The settled post-cold-cycle photo confirms SERVREG QMI result/error `0/0`, WLAN
firmware state `0x7fffffff` (`SERVICE UNINIT`), and no PDR request. The modem
still reports `PEER SMP2P FATAL` with live error `0x0000000e`; QRTR has two
servers, peer HELLO and HELLO sent are both one, and the SERVREG LOC telemetry
shows two requests, one queued reply, last-name code `0xffffffff`, and zero
domains for that last request. Thus the full power-off did not clear the
peer-fatal state. The phone is now running this prior probe, not in Fastboot;
the next name-code probe requires returning to Fastboot and confirming USB
enumeration first.

### SERVREG LOC name-code diagnostic build (2026-10-21)

The LOC screen's `NAME CODE 0xffffffff` only said that the last request was
outside the four mapped service names. The mapper now assigns diagnostic code
5 to the known no-match `tms/pdr_enabled` query and code 6 to
`tms/pddump_disabled`; it does not add either name to the response map or
change any protocol behavior. Unknown names remain `0xffffffff`.

The stable-code tests and `python3 platforms/sunfish/test-modem-qmi.py` pass,
including host routing/GLINK/FIFO tests and AArch64 native code generation. The
separate temporary image at
`.cache/pixel-sunfish/modem-servreg-namecode-probe/fastboot.img` has SHA-256
`ca0e81e4f375714c8f8d860f880af5740bd9ec2ca84276af1654d4c1ef9530f8`. Its task
manifest was refreshed from the generated loader after the first packaging
check caught a stale header. The final build reports zero CapDL overlaps,
keeps all 512 WLAN MSA pages unmapped, and passes the complete source/relocation
and task-copy layout proof. The metadata profile confirms the one-query
no-restart scope. After the cold-cycle result confirmed the prior peer-fatal
state, the user returned the Pixel to Fastboot. USB enumeration, product
`sunfish`, and `unlocked: yes` were verified. I then successfully used
`fastboot boot` on this image; no partition was written. The settled photo
shows `PEER SMP2P FATAL` with live error `0x0000000e` again, but this run stopped
before QRTR initialization: server/HELLO counts are zero, and QMI, LOC request,
and service-map fields remain uninitialized. The name-code diagnostic therefore
did not run. This confirms a repeatable modem startup failure after a true
cold cycle, not a WLAN service discovery or Wi-Fi start result. The evidence
matches the earlier bounded fatal trace's watchdog text, but does not identify
the underlying dependency or prove that missing SERVREG is the cause.

### UFS modem-storage composition and confirmed SMMU readback (2026-10-21)

The user's text confirmation fixes the targeted global SMMU register value at
`0x002A0406`. Its common SMMUv2 fields indicate client processing and global
fault reporting/interrupts enabled, unidentified-stream faults enabled, and
stream-match conflicts configured to fault. The probe's `SCR0` label does not
establish which security bank was read. The same targeted screen shows no
matching route for WLAN SIDs `0x240`, `0x241`, `0x520`, `0x521`, or `0x522`; UFS
SID `0x300` is routed to CB2, whose `SCTLR.M` is clear. This neither identifies
a free route/context bank nor delegates SMMU control or DMA authority.

Offline code now composes the paired-GPT proof for `modemst1`, `modemst2`,
`fsg`, and `fsc` with the modem remote-storage I/O trait. It checks exact
partition extents, device and checkpoint identities, LUN capacity, and a live
exact translated SID `0x300` route whose target matches the enabled context
bank for every bounded operation; ambiguous writes are quarantined. The new
SCSI bridge connects the UFSHCI host core to `BlockIo` with bounded READ(10),
WRITE(10), and cache-sync commands; three synthetic host tests cover data
movement, preflight, invalid layouts, and no retry after uncertain completion. Eight storage
adapter tests also pass, including a synthetic composition with the SCM pool
backend that reads a verified GPT extent and exercises pool zero/grant/info/
reclaim. The owner-facing composite handler backend now also has two offline
tests for paired live-identity checks, disk/pool IO forwarding, and quarantine
on identity drift. This proves the software interfaces compose, not that the
device grants ownership. The full `platforms/sunfish/test-modem-qmi.py`
regression and AArch64
`no_std` codegen pass. There is still no production UFS
`PlatformIo`/live owner binding, physical GPT or partition access, SCM
ownership, or WLAN route authority, and this does not address the missing
initial `wlan_pd` start mechanism. The recovered
`cnss-daemon` path waits for WLFW and exposes no PDR/domain-start routine; the
available ICNSS PDR call is a recovery path for an already registered domain.
No source-backed initial start transaction is established, so the device
result remains WLAN `SERVICE UNINIT`, not native Wi-Fi.

### Sunfish Android service-start roles (2026-10-21)

The LineageOS 23.2 Sunfish [`init.hardware.rc`](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.hardware.rc#L44-L47)
starts `vendor.qseecomd` early with a comment tying it to keymaster so `vold`
can mount system/vendor. The same file separately declares `vendor.qrtr-ns`,
`modem_svc`, and `pd_mapper` as core services, while `cnss-daemon` is
`late_start` ([service declarations](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.hardware.rc#L504-L523),
[CNSS declaration](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.hardware.rc#L608-L611)).
This corroborates the Android-side service inventory, but the init file does
not establish that QSEE is a modem-start dependency or explain the observed
`PEER SMP2P FATAL` watchdog. Treating `qseecomd` as the missing modem fix would
be speculative. The native owner still lacks production implementations of
the selected AP-side modem/QRTR services, and their required startup ordering
must be established from protocol evidence rather than Android `class` labels
alone.

### Offline read-only TFTP policy core (2026-10-21)

The cached modem firmware names `wlanmdsp.mbn` under the `wlan_pd` loader and
contains separate errors for a missing WLAN image and an unavailable/RFS-read
failure. The saved Lineage baseline has running TFTP servers and a TFTP wake
lock, but no file-level request capture. This makes TFTP a plausible later
dependency, not an established cause of the observed startup failure: the
repeatable `PEER SMP2P FATAL` occurs before QRTR initializes.

`rust/src/qcom_tftp.rs` now contains a no-allocation policy/parser core for
read-only RRQs, octet mode, bounded option decoding/OACK and DATA/ERROR encoding.
It admits only the three firmware-observed `wlanmdsp.mbn` path aliases, denies
WRQ and arbitrary paths, and bounds seeks and read sizes against an immutable
file extent. Eight direct Rust tests pass. No firmware bytes are packaged or
served, no QRTR service-4096 raw endpoint is implemented, and the actual
request path and file authority remain unverified; this code does not explain
or clear the current modem SMP2P fatal.

### SERVREG readiness before the first QRTR exchange (2026-10-21)

The native owner now registers and stages the bounded, manifest-backed SERVREG
LOC endpoint before starting QRTR control. This is local state only: it sends
nothing until the peer HELLO is accepted. The direct-peer controller then
advertises LOC before remote-storage and heap services, so domain-map discovery
is available at the earliest point supported by the current native attach
sequence. The prior post-start registration path has been removed from this
runtime path. The read-only state probe also retains the last modem LOC service
name in a fixed 64-byte buffer, sanitizes it for display, and shows both
bounded halves on screen. This adds the exact lookup name to the next
diagnostic instead of only its numeric code, which was unknown in the previous
capture. A direct owner-trace test covers a full-length unknown name, preserves
its `NAME_UNKNOWN` code, and verifies the display split boundary. An owner/FIFO
integration test also routes an unmapped query end to end, checks the exact
sanitized trace name and empty-domain count, and verifies that one reply queues.

The focused QRTR/GLINK test passes all 68 cases, and the owner/FIFO test passes
all 112 cases; they check both the exact descriptor ordering and that the owner
stages LOC without publishing before HELLO. This does not
make `pd_mapper` available before modem boot: the native owner attaches only
after the modem has already produced the GLINK channel checkpoint. It therefore
cannot by itself explain or fix the earlier `PEER SMP2P FATAL` watchdog. The
separately reported SMMU `SCR0=0x002A0406` remains a read-only register
observation; the WLAN SID routes and free context-bank ownership are still
unproven, and no SMMU/DMA authority has been granted.

The full `platforms/sunfish/test-modem-qmi.py` regression passes, including
AArch64 `no_std`/native code generation. The rebuilt one-query, no-restart
temporary image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`9cf64b6c052e68305801f69dd40ee8b32ebd08dfd50e3a211ad2bcc7f3ac738e`). Its
loader/task-manifest hash and source placement validate, the packed boot DTB is
preserved, there are zero CapDL overlaps, and all 512 WLAN MSA pages remain
unmapped. The current Fastboot USB inventory is empty, so this build has not
been booted on the phone. The image-layout validator and all four focused image
profile tests pass.

### SERVREG name probe result (2026-10-22)

Fastboot enumeration again identified the unlocked device as product `sunfish`,
and the validated image above booted temporarily without writing a partition.
The settled screen reports QMI result/error `0/0`, WLAN firmware
`0x7fffffff` (`SERVICE UNINIT`), and no PDR request. It also reports QRTR server
count 2, peer HELLO 1, HELLO sent 1, live error `0x0000000e`, and stop reason
`PEER SMP2P FATAL`; the WLFW client is disabled in this read-only probe. LOC
telemetry shows 2 requests, 1 queued reply, zero domains for the last request,
and the sanitized last name `TMS PDUMP DISABLED`. This matches the earlier
bounded raw capture of `tms/pddump_disabled`. The zero-domain response is
intentional: that name is diagnostic-only in `qcom_servreg.rs` and is not in
the exact Sunfish modem service maps. The capture therefore confirms LOC name
decoding and the empty-map path; it does not indicate a missing `wlan/fw` map
or justify adding `tms/pddump_disabled` to the domain list.

This remains a diagnostic, not a WLAN start attempt: it deliberately does not
issue a PDR request, start WLFW, or enable WLAN DMA. `PEER SMP2P FATAL` remains
present while the native QRTR/LOC exchange completes, so the photo neither
isolates the fatal's cause nor establishes that it is the blocker to initial
domain start. The next offline step is to trace the target-specific initial
`wlan_pd` start path and its required AP-side services against the recovered
Lineage/QCACLD source; keep firmware start and SMMU writes disabled until that
contract and exclusive DMA ownership are proven. The temporary diagnostic is
complete. ADB now confirms the phone is back in LineageOS
`23.2-20261001-NIGHTLY-sunfish`. This session's ADB shell is unprivileged and
cannot read `/vendor/bin`; no additional vendor binary was pulled.

### Offline SMMUv2 context-fault decoder (2026-10-22)

The proposed `cb7`/`cb8` physical-address table was not used: the local
Sunfish Linux driver derives context-bank windows from the discovered SMMU
register layout and selected bank, so a guessed physical base or bank index is
not evidence for a WLAN fault. The cached Lineage kernel's ARM SMMUv2 register
header defines context-bank `FSR` at offset `0x58`, `FAR` at `0x60`, and
`FSYNR0` at `0x68`; it assigns `TF` to bit 1, `PF` to bit 3, and `EF` to bit 4.
Thus the pasted table's `FSR + 0x10` and `PF = bit 4` are incorrect for this
driver. The read-only probe already reads FSR and documents that it is
write-one-to-clear, so no write or polling loop was added.

`rust/src/sunfish_smmu.rs` now decodes only a caller-supplied FSR/FSYNR0
snapshot, preserving the raw values and applying the fault mask used by the
Linux IRQ handler. Its dedicated host test suite is separate from
`sunfish_modem_owner_test.rs`; it has no MMIO, polling, FSR clearing, guessed
physical base, CB selection, or SCM/VMID ownership behavior. The focused suite
passes 15 tests, and `platforms/sunfish/test-modem-qmi.py` passes, including
the AArch64 `no_std` core and native code-generation checks. This decoder only
helps interpret a future verified snapshot; the previous phone captures do
not provide a context-bank FSR value, so it does not establish an SMMU fault or
explain the peer fatal.

Reference: [pinned Sunfish Linux ARM SMMUv2 register definitions](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu-regs.h)
and [context-fault handling](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu.c).

### Current Linux-firmware candidate and Linux reference-test gate (2026-10-22)

I refreshed the upstream WCN3990 audit against the
[`WHENCE` snapshot at `95c414d44c8e60880be787822d5020a010044a49`](https://kernel.googlesource.com/pub/scm/linux/kernel/git/ath/linux-firmware/%2B/95c414d44c8e60880be787822d5020a010044a49/WHENCE).
It lists the generic WCN3990 image and a separate QCM2290 pair; QRB4210 paths
are symlinks to that same QCM2290 pair. This snapshot has no Sunfish/WCN3998
firmware or board-data entry. The QCM2290 files are assigned a different
[Qualcomm license](https://kernel.googlesource.com/pub/scm/linux/kernel/git/ath/linux-firmware/%2B/95c414d44c8e60880be787822d5020a010044a49/LICENSE.qcom)
and `qcom/NOTICE.txt`; its chipset and redistribution conditions must not be
inferred from the Qualcomm Atheros license on the generic bundle.

One path-selection detail changed the test plan. The pinned Sunfish mainline
tree at `fd78d17954a86ea6ceb0c535e618ea129e09f8dd` already reads the WLAN-node
`firmware-name` property and the ath10k core tries that subdirectory before
falling back to the generic directory ([SNOC lookup](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/snoc.c),
[core lookup](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/core.c)).
The current Sunfish DTS leaves this property unset, so a test-only `qcm2290`
overlay can select the nested path without a driver backport. The selector is
only path routing: the QCM2290 image and board data still need an exact
Sunfish match, and `wlanmdsp.mbn` still needs a separate out-of-band loader and
WLFW provider. Since the QCM2290 pair has no private `board-2.bin`, the test
must retain the documented parent board file while omitting generic root
firmware payloads to prevent accidental image fallback. The detailed
offline-first reference-test plan and stop conditions are in
[`sunfish-ath10k-port-plan.md`](sunfish-ath10k-port-plan.md). No candidate
files were staged, and no Linux or seL4 firmware boot was attempted.

The follow-up check resolved upstream `linux-firmware` main to
`00b1df13f9d0adebcb90bd110a4a22de5115e077` and streamed the generic,
QCM2290, and Shikra candidate files directly to SHA-256 without retaining
them. The recorded sizes and hashes match all nine files at that snapshot.
That snapshot had no Sunfish/WCN3998 firmware or board-data identity. Its QCM license and notice are now stored at
`LICENSES/LICENSE.qcom` and `LICENSES/NOTICE.qcom`; the historical `95c414d`
snapshot used `LICENSE.qcom` and `qcom/NOTICE.txt`. The detailed audit and
test sequence are in [`sunfish-ath10k-port-plan.md`](sunfish-ath10k-port-plan.md).
No candidate was staged and no device test was run.

The current main branch was rechecked later the same day at
`412eb7861d40b9dc3dfa01d4059d4b5c414b0307`. Its only relevant WCN3990 change
is commit `72f3b81c89fbc52677b9dbbfa92cde0ae00a36e2`, which updates the
Shikra-specific `wlanmdsp.mbn` to version
`WLAN.HL.3.3.7.c5-00226-QCAHLSWMTPL-1` to address stability issues. The new
3,808,072-byte payload hash is
`ea1787533f969c084c24292defdcc88d75d71955bea7e12ffee2b5fb16248350`. The
generic, QCM2290 and parent board-data hashes are unchanged; the updated image
still is not a Sunfish match and does not change the reference-test gate. The
updated source inventory is recorded and validated in
[`wlan-resources.json`](../platforms/sunfish/wlan-resources.json).

### QCM2290 candidate license scope review (2026-10-22)

I fetched the exact `LICENSES/LICENSE.qcom` and `LICENSES/NOTICE.qcom` text
from the pinned upstream main snapshot and verified both SHA-256 values against
the manifest. The agreement permits installation/use and binary redistribution
under its conditions: binary format only, operation only on platforms with
Qualcomm Technologies chipsets, include the agreement text when redistributing,
and preserve notices. It prohibits reverse engineering, disassembly, and
decompilation, grants no patent rights, and bars applying third-party license
terms to the materials. This records the source terms but does not accept the
agreement or decide whether it covers the intended NexatrodeOS distribution or
this Sunfish test. Firmware staging and device testing remain gated on separate
review, and the QCM2290 image still has no Sunfish compatibility evidence.

### Source-derived Sunfish IPA v4.2 request profile (2026-10-22)

The offline IPA path now has a source-derived Sunfish v4.2 profile in
[`sunfish_ipa_profile.rs`](../rust/src/sunfish_ipa_profile.rs). It maps the
matched Lineage partition offsets, MSM Android QMI platform value,
non-hashable-table behavior, and the zero-size drop-statistics fields. It keeps
the runtime `IPA_SHARED_MEM_SIZE` base/size, APPS_WAN_CONS endpoint mapping,
and IPA uC-loaded state as required caller inputs. Because the register's size
is the available partition length starting at its base, the codec validates
offsets against the exclusive end `base + size`.

Four profile tests cover exact v4.2 fields, first-boot versus SSR presence,
endpoint omission, request TLVs, and invalid runtime geometry. A client
composition test confirms the profile becomes one bounded QRTR/QMI request
with the expected endpoint, payload length, and padding. The no-radio target
probe now executes that source profile with synthetic register values;
it still has no IPA MMIO or live QRTR/GLINK transport and sends no QMI. The
As of 2026-10-23, the IPA source validator checks the Rust profile's v4.2
offsets and sizes, APPS_WAN_CONS endpoint 9, the Sunfish non-hashable
device-tree property, and the exact `INIT_MODEM_DRIVER` request formulas
against the pinned Lineage tree.
The validator and full `test-modem-qmi.py` regression pass, including the
AArch64 `no_std` and native code-generation checks. The
updated probe image has not been booted on the Pixel, and no trusted runtime
snapshot provider or AP-side IPA service is implemented. The live Lineage
baseline confirms IPA initialization precedes WLAN QMI, but contains no raw
request fields. Therefore this profile improves request fidelity without
establishing that IPA initialization is the cause of the custom modem stall.

### IPA QMI client/server handshake audit (2026-10-22)

The pinned Sunfish IPA v3 source shows that the AP side uses both roles on
service `0x31`/version 1: it publishes local instance 1 and separately looks
up the modem at instance 2. The local server handles indication registration
(`0x0020`) and modem-init completion (`0x0035`), while the AP client sends
`INIT_MODEM_DRIVER` (`0x0021`). After the AP client exchange succeeds, Linux
marks its QMI initialization complete and sends the master-driver-complete
indication (`0x0022`) when the modem has registered for indications. The
`0x0035` handler records the modem completion and returns a successful common
response; its IPA counter-register mapping is limited to IPA v4.1.

The `0x0021` response has an optional `modem_driver_init_pending` field. If it
is true, the modem schema says the AP must wait for the separate `0x0035`
completion request before using the IPA hardware. The native client now keeps
that response in an explicit `ModemDriverCompletionPending` state, with a
regression test confirming the flag, successful QMI result, and rejection of a
second request. A bounded AP-side protocol core now decodes modem `0x0020` and
`0x0035` requests, prepares their source-matched success responses, and prepares
`0x0022` only after both AP initialization and modem indication registration
are confirmed. It commits state only after the transport confirms a complete
send; an uncertain send quarantines the generation. The core is not attached
to the native QRTR service, so the owner does not advertise AP instance 1 or
run the handler on the phone. The offline host GLINK/QRTR model now also routes
a prepared `0x0022` indication to the unique modem IPA endpoint and releases
its token only after both GLINK RX_DONE and QRTR RESUME_TX; an exchange test
covers the wire route and completion ordering. Four protocol tests cover both
modem request types, exact QMI response fields, the dual gate for `0x0022`,
malformed peer/state handling, and uncertain-send quarantine. The pending state
therefore remains diagnostic until the runtime service and a trusted IPA
readiness source are integrated.
The saved Lineage trace contains only request sent/response received markers,
so it does not establish whether Sunfish used the pending branch. Passive
discovery of the modem IPA endpoint in the new observer cannot answer that
question; it records announcements, not QMI payloads. A native IPA
implementation must provide the matched server and client path and gate the
completion indication on actual IPA initialization, while keeping the pending
branch explicit until a real response is observed.

The evidence is in the pinned
[`ipa_qmi_service.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c)
and [`ipa_qmi_service_v01.h`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/include/uapi/linux/ipa_qmi_service_v01.h).

The separate IPA hardware-ready boundary is in the same pinned tree's
`drivers/platform/msm/ipa/ipa_v3/ipa.c`, in `ipa3_post_init()`. Before setting
`ipa_initialization_complete`, the normal-hardware path must initialize IPA
register access, complete `ipa3_init_hw()`, validate that the modem-required
SRAM partition fits, allocate its GSI support buffers, initialize interrupts,
register GSI, and connect the AP pipes. The function marks initialization
complete and emits its success log only after those steps. The uC interface
init is attempted in this routine but its error is logged rather than used as
the function's failure result; uC-loaded state remains a separate input to the
QMI profile. So an encoded request or successful `0x0021` response cannot stand
in for AP hardware readiness. The existing Lineage capture now also retains
the kernel's `IPA driver initialization was successful` marker, alongside the
QMI send/response markers and read-only `hw_type`/`gen_reg` values.

### Rebuilt TFTP observer image and target checks (2026-10-22)

The no-firmware observer was rebuilt from
[`build-modem-tftp-observer-probe.sh`](../platforms/sunfish/build-modem-tftp-observer-probe.sh).
It performs one transaction-matched SERVREG query and passively records
announcements for the IPA service (`0x31`, version 1, instance 2) and WLFW
(`0x45`, version 1, instance 0), as well as bounded, sanitized TFTP RRQ/WRQ
metadata. The display now shows the returned SERVREG `wlan/fw` state and a
service-flags bitmask for the local TFTP advertisement and discovered IPA and
WLFW services, with the IPA/WLFW ports when present. This can tell us whether
the modem announces IPA before WLFW becomes available. It does not send an IPA
QMI request, so finding the server does not prove that `INIT_MODEM_DRIVER`
succeeded. It sends no TFTP reply or file bytes and does not issue PDR, start
WLFW, grant MSA, configure WLAN DMA, or start radio firmware.

The current temporary-boot image is 41,299,968 bytes with SHA-256
`180be7f373ad7c03c16ab238c1b656db71ebce0cdf49fde9cac83f96a309eb5d`; its
native PD ELF is pinned by the task manifest at
`798c88179ba33ab875c0523e5d24882ddddc8751817bf81d6a829032019164bc`. These
replace the earlier observer-image hashes recorded in this section. The
modem/QMI host regression, AArch64 `no_std` and native code generation pass.
The current image-layout proof confirms zero CapDL overlaps, preserves the
original boot DTB, avoids fixed kernel/live reservations, verifies source and
relocated payload separation, and matches the task-manifest hash. All 512 WLAN
MSA pages remain unmapped. These checks prepare a bounded diagnostic boot; they
do not prove that the observer has run on the handset. ADB and Fastboot
currently list no device, so the actual modem service and TFTP observations
remain pending.

### Device-test decision points from the captured Lineage boot (2026-10-22)

The saved Lineage startup log gives a useful comparison timeline. It starts
`tftp_server`, `modem_svc`, `pd_mapper`, and `vendor.rmt_storage` before modem
power-up. After modem PAS boot, `rmt_storage` services modem filesystem reads;
the log then records a 5 MiB memshare allocation and a successful
`QMI_IPA_INIT_MODEM_DRIVER` response. It next records the `wlan_pd`
SERVREG indication and ACK, an ICNSS WLFW-server connection, and finally
`WLAN FW is ready`. These are timestamped observations of the saved Lineage
boot, not proof that any one service is the cause of the native boot failure.
The baseline has no captured TFTP request payloads, so it does not identify the
requested image or its options.

The next handset sequence starts with the phone left in LineageOS:
[`capture-lineage-ipa-state.sh`](../platforms/sunfish/capture-lineage-ipa-state.sh)
verifies Sunfish/LineageOS, reads IPA `hw_type` and `gen_reg` when ordinary
ADB permissions allow, and filters recent kernel logs for existing
`INIT_MODEM_DRIVER` markers. It does not use root, change log settings, send
QMI, restart the modem, or start WLAN. This may establish live IPA geometry
and ordering, but it cannot reveal the unavailable endpoint/uC state or raw
QMI payload.

The separate temporary observer checks whether the read-only SERVREG query
completed and what `wlan/fw` state it returned, whether the modem's IPA service
(`0x31`, version 1, instance 2) and WLFW service (`0x45`, version 1, instance
0) are announced, and bounded QRTR TFTP packet/RRQ counts plus sanitized
request path, peer, mode, and options. Its service-flags bitmask identifies
TFTP advertisement, IPA discovery, and WLFW discovery; the IPA port is shown
when present. This adds an observation of the Lineage startup step that
precedes WLFW without sending `INIT_MODEM_DRIVER` or accessing IPA registers.
The probe never replies to a TFTP request or serves bytes; its task manifest
keeps WLAN MSA unmapped and excludes PDR, IPA QMI requests, WLFW client
requests, WLAN DMA, and firmware start. A captured RRQ would establish that the
modem reached a file-request path and give the exact request metadata, but
would not establish that a candidate image is compatible or licensed. Seeing
the IPA server alone would not establish that its initialization request
succeeded. No RRQ with an unchanged early-stop state would place the observed
failure before or outside this request path; it would not prove that TFTP is
never used on a successful boot. The corresponding firmware-serving
experiment remains separately gated on source, compatibility, and license
review. The phone is not currently enumerating over ADB or Fastboot, so neither
check has run.

### AP IPA server transport integration (2026-10-22)

The bounded IPA AP service is now connected through the GLINK/QMI session,
FIFO adapter, and native-owner APIs. The owner can register the AP instance-1
endpoint without announcing it, borrow the exact routed QMI request, and queue
the service core's prepared `0x0020` or `0x0035` response. The transport
accepts only the unique announced modem IPA endpoint and exact transaction and
message response. It returns the service token only after both GLINK RX_DONE
and QRTR RESUME_TX; the `0x0022` indication has corresponding queue/completion
owner hooks. Offline exchange coverage verifies both acknowledgement arrival
orders and that service state does not commit after only one acknowledgement.

Publishing the AP service now requires a generation-bound opaque readiness
permit minted by the trusted runtime after source-matched `ipa3_post_init()`
completion. The generic service-publication API rejects IPA availability, and
the owner also requires that permit when enabling IPA or queuing the master
init indication. The native IPA hardware provider is still absent, so there is
no production permit-minting call path and AP IPA remains unpublished during
phone boot.

This closes the transport/API gap, not the hardware path: no trusted native
IPA register/SRAM/GSI/interrupt provider exists, the runtime does not mint the
permit or call these owner hooks, and AP IPA remains unpublished during phone
boot. Do not add a production permit-minting path until that provider proves
the full `ipa3_post_init()` readiness boundary. The current read-only device
checks remain the next evidence-gathering step; they do not exercise this QMI
path.

### Source-derived IPA v4.2 fixed-register initialization plan (2026-10-22)

[`sunfish_ipa_hw_init.rs`](../rust/src/sunfish_ipa_hw_init.rs) models the
fixed-register portion of `ipa3_init_hw()` from the pinned LineageOS kernel
commit `28f9290ae067d3e7857126eb6312720dbe9c7c80`. The plan requires the
board-selected IPA type 16 and a nonzero live `IPA_VERSION`. It then records
the nine writes in source order: clear BCR; open the two global clocks; rebuild
TX_CFG with PA masking disabled; set DDR QSB writes/reads to 8/12; set counter
aggregation to 500 microseconds; write COMP_CFG once with GSI in-order read
and write disabled and again with multiple AXI masters disabled; then clear
filter/routing hashing. Register offsets follow the pinned `ipahal_reg_init()`
table and its predecessor-version fallback, rather than a different SoC's
register map.

The module now also has a mockable register executor. It
preserves the source's read/write ordering, including re-reading COMP_CFG
between its two writes. Its `RegisterIo` trait does not map MMIO or create a
hardware capability, and no production implementation is wired to a device.
Six host tests cover the pure plan, exact executor sequence, hardware/version
gates, and a partial-write failure that stops immediately and reports the
confirmed prefix. The executor only performs this fixed-register stage; it
does not initialize IPA, GSI, interrupts, DMA, SRAM allocations, or the
AP-side QMI service. It is not evidence of live hardware readiness or Wi-Fi
operation. The full `test-modem-qmi.py` suite also passes, including its
AArch64 `no_std` compilation and native-owner checks.

The Lineage `gen_reg` capture remains useful for IPA type, SRAM geometry, and
existing initialization log markers, but it does not report the registers in
this fixed-init plan. The driver's `ipa_dump_regs` path only prints entries
enabled in its register-dump table; these fixed-init registers are not on that
list. A later temporary seL4 readback test will need a bounded IPA register
mapping and a source-verified observation point. The production MMIO provider
and full `ipa3_post_init()` path are not implemented, so native IPA service
publication remains unavailable.

### IPA GSI v2.2 and AP system-pipe profile (2026-10-22)

[`sunfish_ipa_gsi_profile.rs`](../rust/src/sunfish_ipa_gsi_profile.rs) now
records the source-matched static endpoint table: IPA type 16 selects GSI 2.2;
the AP command producer, LAN consumer, and LAN producer use IPA endpoints
6/8/2 and GSI channels 1/2/6. The QMI control endpoint is Apps WAN consumer
endpoint 9 on GSI channel 3. The WCN3998 IPA endpoints map to endpoints 3/14
and GSI channels 7/8. The profile also records the Linux AP-pipe order, the
0x800/0x800/0x1000 descriptor FIFO sizes, and the 0x7c00-byte common event
ring. The driver allocates GSI event rings dynamically, so the profile does
not invent fixed event-ring IDs. Its fixed-IOVA planner accounts for the
private CMD and LAN-RX event rings, the transfer rings, and LAN-TX consumption
from the shared event ring. The example now starts at 0x20000000, inside the
Sunfish AP SMMU IOVA window (`0x20000000` plus `0x40000000`) declared in the
pinned DTS, and needs 0xe000 bytes of page-mapped pool. A custom seL4 IOVA
window is possible only if its translated SMMU context covers the full plan.
The planner checks GSI's power-of-two ring-base alignment, page rounding, pool
bounds, and the channel-ring 4-GiB boundary rule. Seven focused host tests
cover endpoint data, allocation order, context words, and pool rejection.

This is still input for a future GSI provider; it does not map physical pages,
register GSI, set up interrupts, or run the IPA system pipes. The native
GSI/AP-pipe provider and isolated translated DMA context are still missing,
so the profile does not make the AP IPA service ready.

### GSI v2.2 AP context-word plan (2026-10-22)

The profile now also encodes the source-ordered GSI context-register writes for
the three AP system pipes. It produces the ten event-context writes for the
common and two private rings, then the four channel-context words and QoS
word for each AP channel. Ring IOVAs come from the checked plan above. Event
IDs are dynamically selected by `ApEventRingAllocator` in the same lowest-free
order as Linux. It emits opaque `ApEventRingIds` only after each corresponding
event IRQ and `ALLOCATED` context-state readback is confirmed. A failed or
abandoned command quarantines its reserved IDs; a reservation can be canceled
only before hardware access. The planner rejects non-v2.2 operation, duplicate
IDs, and IDs outside both the reported hardware capacity and the pinned GSI
driver's `GSI_EVT_RING_MAX` of 24. Event moderation and channel QoS match the
pinned IPA/GSI source, including the common-ring versus private-ring settings
and direct-mode channel weights/prefetch settings.
`issue_next_ap_event_ring_allocation()` writes the exact `GSI_EVT_ALLOCATE`
command to the AP event-command register. It marks the reservation before
MMIO and quarantines all three IDs if that write is uncertain; a successful
write remains pending until the event IRQ and context state are confirmed.

Register offsets are relative to the GSI aperture. Before planning context
words, the caller must read the AP execution environment's v2.2
`EE_n_GSI_HW_PARAM_2` at offset `0x1f040` and construct
`ApEventRingCapacity` by decoding `GSI_NUM_EV_PER_EE` (mask `0x1f00`, shift
8). The capacity type rejects a zero or above-24 count, and the planner
rejects any ID greater than or equal to the reported count. This follows
`gsi_get_max_event_rings()` and `gsi_register_device()` in the pinned driver,
rather than treating the five-bit ERINDEX width or static array size as the
device's live capacity. Focused tests cover raw-field decoding, event-ID
reservation/completion/quarantine, command encoding/failure, ring allocation,
context words, and rejection at the runtime boundary. This remains a partial
provider: it does not map that aperture, enable the event-control IRQ masks,
route the `gsi-irq` DT interrupt into seL4, handle other GSI IRQ types, allocate
Q6/UC channels, or grant the isolated SMMU route. The native GSI provider and
`ipa3_post_init()` readiness path are still missing.

### IPA AP translated DMA route plan (2026-10-22)

`sunfish_smmu.rs` now has an exact-route planner for the IPA AP DMA stream ID
`0x520`, matching the pinned `ipa_smmu_ap` device-tree context. It selects one
verified free stream-match slot and context bank from a fresh complete SMMU
snapshot, and refuses broad overlapping routes or missing control/ownership
gates. This is intentionally a translated seL4 route; the Lineage context is
marked `qcom,smmu-s1-bypass`, so its existing bypass state cannot authorize a
translated ring-pool mapping. The planner does not write the SMMU or create
page tables. Host tests cover exact routing, overlap rejection, and ownership
gates; installing the GSI ring mappings, configuring the translated context,
and proving a negative DMA-fault case remain required before use.

### IPA/GSI ring page-map plan (2026-10-22)

The GSI profile now turns its six ring extents into 14 page mappings. The
caller supplies an IOVA window and 14 distinct physical page addresses; the
planner pairs them in ring allocation order and checks full window coverage,
4 KiB alignment, physical-address width, duplicate frames, and zero pages.
This supports physically noncontiguous backing while keeping each GSI ring
contiguous in IOVA. The source-compatible example uses the IPA AP IOVA window
from Sunfish DTS; another window is accepted only when the selected translated
context covers every ring page.

This is still only a mapping description. It does not allocate or attest
physical frames, build SMMUv2 page tables, install translations, or perform a
DMA-fault test. The native IPA/GSI/SMMU provider and its live authority checks
remain necessary for actual Wi-Fi.

### IPA AP SMMUv2 LPAE table-image plan (2026-10-22)

[`sunfish_smmu_lpae.rs`](../rust/src/sunfish_smmu_lpae.rs) now consumes that
14-page map and emits a sparse AArch64 stage-1 LPAE table image for the IPA AP
ring IOVAs. It models the pinned Linux driver's `qcom,use-3-lvl-tables` input
address cap (at most 39 bits), the 4 KiB granule, output-size encoding, MAIR
values, ASID-tagged TTBRs, and TTBCR2 settings. The caller must supply four
distinct, aligned physical table pages, the hardware-reported input/output
widths, ASID, table-walk coherency policy, ring memory attribute, and whether
the ARM non-secure page-table quirk applies. The plan currently emits two
table descriptors and fourteen read/write, non-executable leaf descriptors;
it rejects maps that cross a level-1 or level-2 table boundary.

The module can now materialize the plan into four caller-supplied 4-KiB table
buffers: it validates the declared physical-page identity before zeroing the
buffers and writing the two table descriptors plus fourteen leaf descriptors.
This remains a caller-buffer operation; it does not prove that those slices
really back the declared physical pages. A real provider must still attest and
allocate those pages, handle CPU cache maintenance, barriers, TLB invalidation,
register ordering, interrupt/fault handling, and live ownership. The module does
not write SMMU registers, choose a free context bank or stream-match entry,
establish EL1/QHEE authority, or prove DMA containment. The SMMU ID2
input/output widths and a delegated translated context are not available in
the saved handoff evidence. Its focused host test now walks the materialized
three-level tables and checks all fourteen IOVA-to-physical-page translations
and the unmapped pages immediately before and after the ring run. Relevant
source is the pinned [Sunfish ARM SMMU driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu.c),
[ARM LPAE page-table builder](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/io-pgtable-arm.c),
and [SMMU register definitions](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu-regs.h).

[`sunfish_ipa_dma.rs`](../rust/src/sunfish_ipa_dma.rs) now composes these
pieces into one plan. It accepts only the exact IPA AP SID `0x520` route,
retains the selected stream-match slot and context bank, and binds them to the
14-page ring map and matching LPAE tables. Regression tests reject WLAN alias
and UFS routes and verify the combined context. This prevents accidental
cross-device plan composition; it does not upgrade the route planner's
caller-supplied delegation assertions into proof, or authorize any hardware
operation.

### Next device check: read-only IPA lifecycle capture (2026-10-22)

The Lineage capture now matches IPA firmware/PAS milestones, SMMU-attach and
firmware-load failures, `ipa3_post_init()`/GSI/pipes completion, driver
initialization, and the existing modem IPA QMI request markers when those
records are already present in the kernel log. The IPA debug-level progress
messages may not appear because this capture leaves dynamic debug off. When
the Pixel is attached in ordinary LineageOS with ADB authorized, run
[`capture-lineage-ipa-state.sh`](../platforms/sunfish/capture-lineage-ipa-state.sh)
once. It reads the driver's `hw_type` and SRAM geometry and filters the
existing kernel log; it does not enable dynamic debug, write debugfs, send
QMI, restart a subsystem, or start WLAN.

The saved boot log currently confirms that `ipa_fws` was loaded into its
reserved region and brought out of reset, and that the modem IPA init request
received a response. It does not show the intermediate post-init milestones or
the response contents, so it is not enough to claim that native IPA readiness
was reached. This capture can add observable live state from a normal Linux
boot; it still cannot validate the seL4 IPA provider or prove WLAN operation.

### LineageOS read-only baseline (2026-10-22)

On the connected `sunfish` running LineageOS 23.2, the capture completed with
one authorized ADB device. Its filtered kernel log showed
`Sending QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` followed by
`QMI_IPA_INIT_MODEM_DRIVER_REQ_V01 response received`. The IPA debugfs
`hw_type` and `gen_reg` nodes were unavailable to the shell account, so no
permission or debug setting was changed. A separate read-only `cmd wifi status`
reported Wi-Fi enabled and connected with supplicant state `COMPLETED`; this
confirms the stock LineageOS Wi-Fi path is a working device baseline, not that
the custom seL4 path is ready.

### IPA AP SMMUv2 context install plan (2026-10-22)

[`sunfish_smmu_program.rs`](../rust/src/sunfish_smmu_program.rs) now combines
the exact IPA AP route and LPAE context into an ordered SMMUv2 register plan.
It disables the selected context bank, sets AArch64 stage-1 format and LPAE
registers, publishes the four planned table pages, invalidates and synchronizes
the context-bank TLB, marks the required fault-handler binding point, enables
stage 1, and writes S2CR before the valid exact-SID SMR entry. The per-device
ACTLR value is an explicit input because the matched Linux driver sources it
from device-tree policy.

This is still a plan only. The pinned Linux `arm-smmu.c` and register header
provide the register semantics and ordering, but Linux configures the IPA AP
context for stage-1 bypass; this plan describes the proposed translated seL4
ring context. No SMMU MMIO path exists here, and power/ownership delegation,
verified free resources, table-cache maintenance, platform ACTLR, and a live
fault-IRQ binding must be established before an executor is appropriate. The
host mock covers the write sequence and selected values. The full Sunfish host
test and AArch64 `no_std` code-generation harness passes; the next device-side
SMMU check still needs a purpose-built read-only probe for the current
context/route registers before any translated route is attempted. Sources:
[pinned ARM SMMU driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu.c)
and [register definitions](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu-regs.h).

### Repeat Lineage IPA capture (2026-10-22)

The read-only capture was rerun on the connected Pixel 4a in LineageOS 23.2.
It again found the existing kernel messages for
`Sending QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` and
`QMI_IPA_INIT_MODEM_DRIVER_REQ_V01 response received`. Ordinary shell access
still cannot read IPA `hw_type` or `gen_reg`; no permission, debug, radio, or
subsystem setting was changed. The output contains no QMI response payload,
so it does not establish the modem's pending flag or prove that IPA startup is
the cause of the custom WLAN failure.

### WLAN CE and WMI control memory composed with the SMMU plan (2026-10-22)

[`sunfish_wlan_dma.rs`](../rust/src/sunfish_wlan_dma.rs) now composes the
existing exact `0x240/0x241` WCN alias route, either modeled QCACLD CE v3 ring
profile, a fixed 416-KiB caller-backed DMA arena, and the bounded LPAE plan.
The Mission profile's nine descriptor rings occupy 101,192 bytes; the planner
places 127 CE2 receive windows and 31 CE3 transmit windows after the rings,
then maps all 104 arena pages. The smaller MissionNoPktlog CE2 ring gets 63
receive windows. Tests feed the windows into the existing WMI post/send
planners, reserve the complete receive ring, materialize the page tables,
resolve every arena page to its supplied frame, and confirm the page after the
arena is unmapped.

These are WMI control buffers, not the target-requested HTT/network buffers
needed for actual packet traffic. The plan does not allocate or attest the
frames, install an SMMU route, write MMIO, perform cache or TLB maintenance,
acquire WLAN power, load firmware, or start the radio. The next useful device
observation remains a read-only SMMU route/context/fault snapshot; native WLAN
startup stays gated on a trusted owner and the complete HTT DMA contract.

The ordered SMMU register planner now accepts this composed WLAN plan as well
as its prior IPA AP plan. Its WLAN fixture encodes both alias SIDs in one SMR
entry by placing mask `0x1` in bits 16–30, writes the selected context's S2CR
before making the SMR valid, and publishes the four LPAE table pages before
enabling stage 1. The fixture's slot and context bank are synthetic planner
inputs; they are not measurements of the currently running phone. The mask
field layout and register definitions match the [pinned Sunfish ARM SMMU
register header](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/iommu/arm-smmu-regs.h#L880-L909).
This is still an offline write plan; the existing LineageOS driver owns the
live SMMU, and the plan must not be executed until a fresh complete route
snapshot and exclusive ownership are established.

### HTT receive ring composed with the WLAN SMMU tables (2026-10-22)

[`sunfish_htt_rx_ring.rs`](../rust/src/sunfish_htt_rx_ring.rs) now encodes the
source-matched 48-byte, one-ring HTT configuration and initializes the
2,048-entry 64-bit address ring with the 2,047 startup buffer IOVAs and
allocation index. Before posting buffer addresses, it can now clear the
source-required four-byte RX attention word at descriptor word 1 for each
initial or fallback buffer; the initial-fill helper checks the full count and
all lengths before changing any buffer. This follows `htt_rx_ring_fill_n()` in
the pinned QCACLD source, which resets attention before mapping each buffer.
Host tests verify the exact field, untouched payload bytes, invalid IDs, and
failure atomicity. It still performs no cache/DMA synchronization or ring
publication. Its validator requires page mappings for the ring, allocation
index, startup buffers, and 64-buffer low-memory fallback range. The composed
[`sunfish_wlan_dma.rs`](../rust/src/sunfish_wlan_dma.rs) plan combines that
1,126-page minimum HTT window with the separate 104-page CE/WMI arena and four
L3 tables.

The integration test materializes the combined LPAE table image, resolves each
of its 1,230 planned pages to the supplied synthetic physical frame, and checks
that the page after the CE arena and the page at the end of the HTT window are
unmapped. This verifies the software table image and address coverage; it does
not establish real physical-frame identity, buffer allocation, ownership,
cache maintenance, SMMU context availability, or an installed live route. The
actual screen evidence still reports WLAN alias SIDs `0x240` and `0x241` as
`NO ROUTE`; no planned table or route has been installed, and no WLAN DMA or
firmware start has been attempted. The full
[`test-modem-qmi.py`](../platforms/sunfish/test-modem-qmi.py) host regression
also passes with this composition enabled, including its AArch64 `no_std` and
native backend code-generation checks.

### One-time TFTP observer boot (2026-10-22)

ADB identified one attached `sunfish` running LineageOS. The existing
no-firmware observer image at
`.cache/pixel-sunfish/modem-tftp-observer-probe/fastboot.img` was rehashed as
`180be7f373ad7c03c16ab238c1b656db71ebce0cdf49fde9cac83f96a309eb5d`, matching
the reviewed build record. Fastboot reported product `sunfish`, and the
one-time `fastboot boot` command returned success; no partition was flashed.
The screen photo captured the settled observer result. It reports
`SERVREG QUERY STOPPED`, QRTR service flags `00000001`, two server records,
`PEER HELLO 00000001`, IPA port `FFFFFFFF`, WLFW server `00000000` and port
`FFFFFFFF`, and zero TFTP packets and RRQs. In this build, service-flag bit 0
means the local TFTP service was advertised; bits 1 and 2 mean IPA and WLFW
were discovered. Thus the photo shows the local TFTP advertisement and QRTR
peer hello, but no IPA or WLFW announcement among the tracked services. The
two server records are not identified on this screen, so they cannot be
attributed to IPA or WLFW.

`SERVREG QUERY STOPPED` is the probe's latched error indicator: it is set when
the owner progress step or SERVREG query step returns an error. The display
does not identify which step failed and contains no decoded `wlan/fw` state or
QMI result. The zero packet and RRQ counters mean this run observed no TFTP
request; they do not prove that a successful Linux boot never uses TFTP. The
screen also confirms that this observer sent no TFTP response, served no
firmware bytes, and attempted no PDR or WLAN DMA. This result gives no
firmware-compatibility, SMMU-fault, or radio-start evidence. No partition was
flashed, and the phone should return to LineageOS before another observation.

### Observer error-detail build (2026-10-22)

The read-only observer display now uses its otherwise blank area, when no TFTP
packet metadata is present, to show the SERVREG client state, QMI result/error,
returned WLAN state, owner stop reason, transport detail, whether the query was
started, and service/instance IDs for the first two QRTR server records. When a
TFTP packet is observed, its sanitized request metadata keeps those display
rows. No extra modem transaction or write was added.

The revised temporary image is
`.cache/pixel-sunfish/modem-tftp-observer-probe/fastboot.img`, 41,299,968 bytes,
SHA-256 `2b39dd7c506575a175ad255e0408b2fe51f230b2580c967c636bef5c7d69db23`;
its native PD ELF hash is
`f2cf59f89ade9901a266215bad3b28733eaaf05e371a0659dfb0571345a85aa9`. The
build refreshed the task image's fixed header/region manifest after the extra
display code added one page to the initial task extent. The final build passed
the exact manifest/hash check, verified zero CapDL overlaps and all fixed
reservations, and left all WLAN MSA pages unmapped. The full
`test-modem-qmi.py` regression and the modem-owner manifest test pass. This
image is built and layout-validated but has not been booted. The Mac currently
sees no ADB or Fastboot device, so a further handset observation waits until
LineageOS is restored and USB is available.

### LineageOS WLAN service and firmware baseline (2026-10-22)

After the user returned the Pixel to LineageOS, the read-only IPA capture
confirmed `sunfish` on LineageOS `23.2-20261001-NIGHTLY-sunfish`. The current
kernel log adds a complete WLAN-side baseline: `service-notifier` received a
service indication from `msm/modem/wlan_pd` with state `0x1fffffff` and ACKed
it; `icnss_qmi` then logged `QMI Server Connected: state: 0x980`. ICNSS reported
`WLAN FW is ready: 0xd87`; the QCA driver subsequently logged `FW ready event
received`, created its WLAN interfaces, and Android's Wi-Fi service reported a
connected client. The pinned SERVREG enum maps `0x1fffffff` to `UP`. The
Lineage [`service-notifier.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/service-notifier.c)
registers for service-state notifications and ACKs matching indications;
[`icnss_qmi.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss_qmi.c)
looks up the WLFW server and logs the QRTR connection before the firmware
ready event. Network names and MAC addresses from the log are omitted.

This confirms that the stock LineageOS service-notifier/ICNSS path observes
`wlan_pd`, reaches WLAN firmware readiness, and brings up a client connection
on this handset. It does not identify the underlying QMI transaction payloads
or imply that the seL4 observer currently handles the same service-notifier
indication. The earlier observer's missing IPA/WLFW records therefore remain a
separate seL4 observation; the new Lineage log rules out treating
`wlan_pd` as persistently uninitialized on the handset. The next comparison
point is to establish whether the seL4 owner sees the SERVREG state and a WLFW
server announcement before attributing the gap to SMMU programming. The same boot's IPA log still
reports `ipa3_uc_state_check: uC is not loaded`; the modem IPA init request
received a response, but its payload was not logged. IPA `hw_type` and
`gen_reg` remain unreadable to the ordinary shell account, and no permissions
or device settings were changed.

### Settled TFTP observer result and display-field fix (2026-10-22)

The error-detail observer image described above was then booted temporarily
with `fastboot boot`; Fastboot reported `sunfish` and an unlocked bootloader.
No partition was flashed. The settled photo shows `SERVREG QUERY STOPPED`,
`QRTR SVC FLAGS 00000001`, two QRTR server records, one peer HELLO, no IPA or
WLFW server/port, and zero TFTP packets or RRQs. It reports QMI result/error
`00000000`, WLAN state `7FFFFFFF` (`UNINIT`), query-started `1`, and owner stop
reason `PEER SMP2P FATAL`. The visible server instance words are `0000B401`
and `00001202`; this screen does not identify their service IDs. It also
confirms that no TFTP response or firmware bytes were served and no PDR or
WLAN DMA was attempted. The TFTP observer is simply waiting for an RRQ; waiting
longer on this settled screen is not expected to produce a different result.

The successful QMI result and returned `UNINIT` state are retained even though
the screen headline says `QUERY STOPPED`. In the owner loop, that headline is
a latched local error flag set when either the owner progress step or the
query step returns an error. The mapped `PEER SMP2P FATAL` reason indicates
that the native transport later encountered a validated peer SMP2P fatal
status; it does not identify the underlying cause or establish an SMMU fault.
Thus the query response completed successfully before a separate owner/transport
failure stopped further progress. This differs from the LineageOS baseline,
where the kernel observed `wlan_pd` UP, connected to WLFW, and reached firmware
ready. The comparison narrows the unresolved gap to the custom owner/transport
path before WLFW discovery, without establishing why that path stops.

The photo does not show the transport diagnostic word or either server service
ID, despite those values being requested by this image. Inspection found that
`Display::hex` silently omits labels longer than 15 characters; `TRANSPORT
DETAIL`, `SERVER 0 SERVICE`, and `SERVER 1 SERVICE` exceeded that limit. The
observer source now shortens those labels to `TRANS DETAIL`, `SERVER 0 SVC`,
and `SERVER 1 SVC` so the next temporary boot can expose the values. It also
distinguishes a successful QMI response followed by an owner stop in the
headline, instead of calling the completed query itself stopped. These are
display-only corrections; they do not change transport behavior or add modem
requests. The rebuilt image passed the full observer build script, tests,
native-image hash check, loader layout proof, and CapDL audit (18,182 pages,
24 fixed intervals, 512 WLAN MSA pages left unmapped, zero overlaps). Its
fastboot image SHA-256 is
`60e761e4f695114b0c2646ccd8d2e382403b6d9bfbbf0129898511eccbd107c9`; the
native PD ELF SHA-256 is
`d2ba668b99a901dd1f0f8bcda9311b52298cfb2e53d0ed284c39c37d0ec8771a`. It has
now been booted; the follow-up screen values are recorded below.

### Settled observer fields (2026-10-22)

Two photos from the corrected observer expose the fields that were previously
clipped. Both show a successful one-shot SERVREG query (`QMI RESULT 00000000`,
`QMI ERROR 00000000`) returning WLAN firmware state `7FFFFFFF` (`UNINIT`).
Both list QRTR server service IDs `00000042` and `0000002B`, with instance
values `0000B401` and `00001202`, respectively. The screen does not identify
which service owns either WLAN function, so the IDs are retained as observed
without assigning names to them.

One photo is headed `SERVREG FW UNINIT`; it shows `OWNER STOP REASON / NO
OWNER ERROR` and `TRANS DETAIL 00000000`. The other is headed `OWNER STOPPED
POST QMI`; it shows `STOP REASON / PEER SMP2P FATAL` and `TRANS DETAIL
0000000E`. Treat these as separate snapshots; they establish that a successful
QMI reply and the later owner-stop diagnostic can be observed independently.
The `0x0e` transport detail is the probe's local code for its mapped
`smem::Error::Fatal`, not the raw peer SMP2P status value. The diagnostic
classifies a peer-fatal condition but does not establish its hardware root
cause.

Both screens show no IPA port or WLFW server/port, zero TFTP packets and RRQs,
and explicitly report that no TFTP response or firmware bytes were served and
no PDR or WLAN DMA was attempted. The next source-validated, read-only capture
of raw SMP2P transitions and the bounded remote reason snapshot is recorded
below.

### Rebuilt fatal-trace diagnostic candidate (2026-10-22)

The separate fatal-trace image was rebuilt after the observer result. Its
source audit matches the pinned Atoll kernel revision and confirms the
SMP2P `slave-kernel` status bit definitions, bounded SMEM item 421 reason
snapshot (at most 256 bytes, no clear), no post-terminal protocol action, and
no new protocol/SMEM writes, IRQ capabilities, or acknowledgements. The
watchdog GIC distributor is not mapped to the diagnostic task because that
range is seL4-kernel-owned. Reason text and status transitions remain evidence
about timing/state, not proof of root cause.

The build and loader checks passed after refreshing the fatal-trace copy-map
manifest from the rebuilt native image. They verify 11 disjoint copy targets,
the complete source and relocated-loader ranges against the cached and kernel
DT reservations, and zero CapDL overlaps. The fastboot image SHA-256 is
`08b9fec220241f504ee392195dabd576620555c51f7051ea3c5a02e977693aa0`; the
native PD ELF SHA-256 is
`edc7dc0ee184565575e1d71ac6c15da24ee6c13b8c244e7032b5ac9cfe55dd39`. The
image was started on the phone with temporary `fastboot boot` after confirming
product `sunfish` and unlocked state. Fastboot reported both send and boot
`OKAY`; no partition was flashed. The settled screen photo reports
`DISC ERROR 0000000E`, first SMP2P status `0x6`, fatal status `0x7`, 1,295
status samples (`0x50f`), and exactly one status change at 38,902 ms (`0x97f6`).
The named fatal, ready, and proxy-unvote bits are all set in the final status;
stop-ack and shutdown bits are clear. Item 421 was read successfully at offset
`0x12510` with its declared 256-byte size, the bounded reason prefix changed,
both guards passed, and 1,440 DATA polls (`0x5a0`) completed before the fatal
stop. The sanitized reason reads `DOG HAL COMMON C 173 TMR SLAVE2 DOG DETECTS
STALLED INITIALIZATION TRIAGE WITH IMAGE OWNER`. WLFW was not seen in this
bounded sample, and the screen confirms no GIC access. The `0x0e` is the local
discovery error for the sampled fatal status, not the SMP2P word itself.

The exact watchdog text now observed on Sunfish also appears in a 2026 Qualcomm
remoteproc patch report describing watchdogs when `qcom_pd_mapper` is not ready
before DSP startup. That makes startup ordering a concrete lead: this native
image currently publishes its SERVREG LOC endpoint only after modem transport
attachment, whereas Lineage starts `pd_mapper` before modem power-up. The match
does not prove the mapper is the cause on Sunfish, and the status/reason trace
does not identify which initialization dependency stalled. The source-order
audit is now complete: the native image authenticates/resets the modem and
waits for SMP2P readiness, then completes GLINK OPEN/channel setup before
`pixel_modem_owner::attach` calls `prepare_service_registry()` and starts QRTR
control/HELLO. The LOC implementation is therefore staged before the first
QRTR HELLO, but only after modem startup has begun; it cannot satisfy a
pre-boot mapper dependency of the kind described in the Qualcomm report. A
prior owner screen recorded `tms/pddump_disabled` as its last LOC lookup and
returned the expected empty map; it did not establish whether the modem asked
for `kernel/elf_loader` or `wlan/fw` before the watchdog.

The read-only state-probe latches the first accepted LOC request name and keeps
saturating query counts for the six pinned names plus an `other` bucket. The
2026-10-22 settled photo (`IMG_8878.heic`) shows a successful SERVREG state
query (`QMI RESULT/ERROR 0/0`) returning WLAN `UNINIT` (`FW STATE 7FFFFFFF`),
followed by the owner stopping with `PEER SMP2P FATAL`. It shows two LOC
requests and one queued reply. The first request is `tms/pdr_enabled`; the
counts are one each for `tms/pdr_enabled` and `tms/pddump_disabled`, and zero
for `tms/servreg`, `gps/gps_service`, `kernel/elf_loader`, `wlan/fw`, and
`other`. Given two requests and those counts, the second request is
`tms/pddump_disabled`. The last query returned zero domains. This run therefore
did not observe the modem requesting either `kernel/elf_loader` or `wlan/fw`
before the peer-fatal stop; it weakens the missing-map hypothesis for this
particular failure path but does not rule out a lookup on another boot path.
The screen does not associate its one queued response with a name.

The state-probe image used for that run was built with the expected task-region
manifest, temporary-boot layout, loader ranges, and CapDL reservations. Its
fastboot image SHA-256 was
`5a441759d093c88bf6cfa39ebcdb20426087ef11506b9e647a0e1972319c8915`, and the
native PD ELF SHA-256 was
`06afaf1a6e44357c8dac5abee1b1e076f8d8b26fc4dfc2027bec33380729d611`. Fastboot
reported `sunfish`, unlocked, and send/boot `OKAY`; no partition was flashed.
The observation only shows that the QMI state query completed before the
separate owner stop. It does not identify the peer-fatal root cause. No PDR
retry, MSA handoff, WLAN DMA, or firmware bytes were used.

### LOC reply-name trace candidate (2026-10-22)

To resolve the unassociated queued reply from `IMG_8878.heic`, the owner now
latches the stable service-name code for the first LOC reply successfully
queued. It derives that code from the same validated request handle used to
build the response; it does not change reply content, ordering, or transport
behavior. In the state-probe display, row 9 shows `FIRST RSP CODE` (`1` through
`6` for the six pinned names, `FFFFFFFF` before any reply or for an unknown
name). This identifies which request produced the first reply queued into the
QRTR/GLINK transmit path; it does not prove that the modem received or
consumed that reply, and it still does not provide a complete per-request
reply log.

The Rust owner suite passes 141 tests, including integration assertions that
the first queued response is attributed to the request actually handled. The
state-probe build script passes; CapDL reports 18,184 pages, 24 fixed
reservations, 512 unmapped WLAN MSA pages, and zero overlaps. The image-layout
validator confirms 11 disjoint copy targets, disjoint source/relocated-loader
ranges, and preservation of the original boot DTB. The prior candidate fastboot
image SHA-256 was
`c3d561039e86f754e7c72674b77b1de343eaae8457f9afe08a6de2cd661a0539`; the
prior native PD ELF SHA-256 was
`15517bab1ef3b338acb38782b8a5582060c4261ba0d66139a188b2dddcf5af51`. The
layout proof passes with 11 disjoint copy targets, zero overlaps, and the
original boot DTB preserved. This candidate preserves the same read-only
SERVREG query scope and no-PDR, no-WLAN-DMA, no-firmware-transfer limits. It is
built and validated offline; the phone must return to LineageOS before any
separately requested next temporary boot.

### TFTP image authority API gate (2026-10-22)

The bounded QRTR TFTP server now accepts an opaque `AuthorizedWlanImage`
capability instead of an arbitrary byte slice. There is no production
constructor until a trusted provider verifies the image source, exact bytes,
Sunfish target match, and applicable use authorization. Synthetic fixtures are
available only to tests. This prevents accidental wiring of unverified bytes
into the native server API; it does not grant firmware authorization, resolve
license restrictions, establish compatibility, or make WLAN ready. No image
was staged or served and no device was accessed for this change.

The focused TFTP, owner, and QRTR-control suites passed (22, 141, and 95 tests).
The full `python3 platforms/sunfish/test-modem-qmi.py` regression passed after
the API change, including storage-host tests and AArch64 no-std/native codegen
checks. Rust formatting and whitespace checks also pass. The next device check,
after the Pixel returns to LineageOS, should distinguish local reply queuing
from the corresponding GLINK/QRTR transmit completion. Prepare that trace
offline before requesting another temporary boot.

### LOC reply attribution result (2026-10-22)

The rebuilt state probe was temporarily booted with `fastboot boot`; Fastboot
reported send and boot `OKAY`, and no partition was flashed. The settled screen
shows a successful WLAN state query (`QMI RESULT/ERROR 0/0`) with WLAN still
`UNINIT` (`FW STATE 7FFFFFFF`) and the owner stopped at `PEER SMP2P FATAL`.
There were two LOC requests and one queued reply. `FIRST RSP CODE 00000005`
maps to `tms/pdr_enabled`; the first-name field and counters agree: one
`tms/pdr_enabled`, one `tms/pddump_disabled`, and zero `wlan/fw`,
`kernel/elf_loader`, `tms/servreg`, GPS, or unknown lookups. The queued reply
therefore belongs to `tms/pdr_enabled`, whose current map result is zero
domains. This proves local queue attribution only, not remote receipt or
consumption; the screen has no per-reply GLINK/QRTR completion metric. The
one-shot result does not explain the peer-fatal stop or show WLAN startup.

### First LOC reply GLINK completion trace (2026-10-22)

The latest photo attributes the one locally queued LOC response to the first
query, `tms/pdr_enabled` (`FIRST RSP CODE 00000005`). It does not show whether
the peer completed the corresponding GLINK receive buffer. The updated
read-only trace distinguishes those stages: a new `ReplyRxDone(RequestHandle)`
event is emitted only when a valid peer GLINK `RX_DONE` matches a fully
published QMI reply, and the owner correlates it with the handle of the first
successfully queued SERVREG reply. Row 25, `FIRST RXDONE`, is `00000001` only
after that peer completion; otherwise it remains `00000000`.

This is evidence that the peer released the shared GLINK receive intent for
that reply. It does not prove the peer accepted or processed the QMI response,
and the independent QRTR credit is still required to retire the local
transaction. The change observes existing transport traffic only; it adds no
request or reply bytes and keeps the probe limited to the one WLAN state query
and manifest-backed LOC replies. The GLINK/owner suites and full
`test-modem-qmi.py` regression pass, including an integration test that checks
the reply handle is matched to peer `RX_DONE`. The full regression also
completed its storage-host and AArch64 no-std/native codegen checks. The
rebuilt candidate fastboot image SHA-256 is
`75da2307d28696440ff0feb1b31e487535dbaa3b95f181e08cc0923117fc5abe`; the
native PD ELF SHA-256 is
`c936ec8cf630c850a5062a040590e48a9dd93a19f4c7d71a4cc4c8c3e21939d4`. The
task-manifest digest now matches this ELF, and layout validation passes with
11 disjoint copy targets and the original boot DTB preserved. This candidate
has been built and validated offline; it has not yet been booted on the Pixel.

### GLINK completion probe result and display-row correction (2026-10-22)

The `IMG_8880.heic` screen shows the owner stopping at `PEER SMP2P FATAL`
before QRTR starts: server count, peer HELLO, HELLO sent, LOC requests, and
queued replies are all zero. QMI result/error and firmware state remain
`FFFFFFFF`, with no response. Therefore this boot did not execute the WLAN
state query or any LOC lookup; the first LOC name and first-reply completion
are not meaningful for this run. It does not show a failed SERVREG reply or a
change in WLAN state.

The photo also exposed a screen-layout defect: `FIRST RXDONE` and `LOC WLAN
FW` were both assigned row 25, so the rendered row could be overdrawn. The
state-probe now prints the bounded first LOC name on one row and places
`FIRST RXDONE` on row 24, leaving row 25 for the WLAN lookup counter. This
changes only diagnostic rendering. `python3 platforms/sunfish/test-modem-qmi.py`
passes, including the owner/GLINK suites, storage-host tests, and AArch64
no-std/native checks. The refreshed temporary image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` with SHA-256
`96340adb058d0ccad55a0e9686760447442745f03bc54d1f83e03c8f0101f805`; its
native PD ELF SHA-256 is
`b3acde65967bb9e8cd1a694f34ec79ba4cc26c7f32c7f9cd258d2da4a518a2a9`. The
11-target image-layout proof passes, including disjoint source/relocated
ranges, preserved boot DTB, and no overlap. This image has not been booted on
the Pixel. At the last host check, neither ADB nor Fastboot enumerated it.

### Linux reference boot-order clue (2026-10-22)

The cached Mobian Pixel 4a port at commit
`789679f25f859bd741d690fe221d1cc68a23444e` provides a concrete, but
community-authored, comparison. Its `work/patch.sh` puts the IPA and
`qcom_pd_mapper` modules before `qcom_q6v5_pas` remoteproc startup, while its
`work/wcn/sbin/wcn-wifi-bringup.sh` waits for the modem remoteproc to run and
for the WLFW QRTR service before loading `ath10k_snoc`. This separates a
possible pre-modem dependency (IPA and SERVREG-PD mapping) from the later
host-driver step. It is a useful order to compare against the native boot
path, not proof that either service is the cause of Sunfish's current fatal
stop: the port's comments and scripts are not a Qualcomm protocol
specification. The earlier `IMG_8880.heic` photo stops before QRTR and cannot
test the later WLFW/ath10k stages. Do not infer a missing WLAN map from that
early-stop screen.

The pinned Mobian recipe has conflicting historical notes about IPA timing.
Commit [`9b83e52`](https://github.com/miromraz/mobian-pixel4a-port/commit/9b83e52e706df02f0fc341f83b17e5de4f0f26ac)
replaces an IPA blacklist with the `qcom_q6v5_pas` soft-dependency and says
the AP IPA service must be available before MPSS. The same repository retains
[`ipa-late-load.sh`](https://github.com/miromraz/mobian-pixel4a-port/blob/789679f25f859bd741d690fe221d1cc68a23444e/work/ipa/sbin/ipa-late-load.sh),
originally committed as a workaround for a reported reset during early IPA
loading. Later `patch.sh` comments say that late-load hook is a no-op because
the soft-dependency already loaded IPA. The pinned recipe therefore requests
early IPA loading; the retained late script records an older failure mode, not
an alternative whose success has been established by this audit. These are
community-maintained reports, not a Qualcomm startup contract or a controlled
reproduction. `validate-mobian-wifi-reference.py` now checks and prints this
caveat alongside the recipe's ordering.

The pinned LineageOS source clarifies what “IPA ready” entails. Its
[`ipa3_load_ipa_fw()` path](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa.c#L6627-L6670)
attaches IPA to SMMU, loads the IPA PAS firmware, then runs `ipa3_post_init()`.
The RMNET probe waits for the IPA-ready callback, obtains the live Apps-WAN
endpoint and initializes Q6 shared memory before calling
`ipa3_qmi_service_init()`. That worker registers local IPA QMI service
`0x31`/version 1/instance 1 before looking up the modem-side instance 2 in
[`ipa_qmi_service.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa_qmi_service.c#L1816-L1900).
This means the native AP service adapter cannot safely stand in for an IPA
provider: it needs a live, source-matched hardware initialization and QMI
endpoint readiness proof. That provider is still absent from seL4, so the
existing `ApReadyPermit` remains unmintable and the default native runtime
does not advertise the IPA service. The current trace's `SERVICE UNINIT` and
zero WLAN domains are consistent with missing prerequisites, but do not prove
that IPA ordering caused the stop.

### LineageOS modem/IPA boot-order capture (2026-10-22)

The follow-up was captured read-only over ADB from the unlocked LineageOS
session. In the current boot log, the IPA kernel dispatcher initializes at
21:13:52.281; init starts `vendor.ipacm` at 21:13:59.726,
`vendor.qrtr-ns` at 21:14:02.925, and `pd_mapper` at 21:14:03.899. The
vendor init files classify `vendor.qrtr-ns` and `pd_mapper` as `core`
services, while `vendor.ipacm` is a `main` service started on `post-fs-data`.
At 21:14:06.883 the modem subsystem changes its firmware name to `modem`;
IPA receives the MPSS `BEFORE_POWERUP` callback at 21:14:06.892. `ueventd`
requests `modem.mdt` at 21:14:06.952 and reports that load took 46 ms at
21:14:06.998. The modem is brought out of reset at 21:14:07.588, the
after-power-up callback completes at 21:14:07.615, and
QRTR/QMI reports modem readiness at 21:14:07.620.

IPA then logs `QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` at 21:14:08.179 and
`response received` at 21:14:08.223; the response payload/result is not in
this log. At 21:14:07.637 the service notifier logs a QMI connection for
instance `180` (decimal). In the pinned QRTR-v1 record, that is packed as
instance/version `0x0000b401` for SERVREG_NOTIF service `0x42` version 1.
The kernel's [`service_notifier_new_server()` and `new_server_work()`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/service-notifier.c#L327-L362)
store the discovered endpoint and then send REGISTER_LISTENER for the service
path; the log does not print that request's response. At 21:14:09.936 the
notifier receives an indication for
`msm/modem/wlan_pd`, instance `0x180`, state `0x1fffffff`, and ACKs it at
21:14:09.959. The pinned SERVREG enum maps that value to `UP`. The connection
event precedes the indication by about 2.3 seconds. The subsequent indication
shows the listener path is active, but it is not a WLFW firmware-ready signal.
The vendor init definitions are in `/vendor/etc/init/hw/init.sm7150.rc` and
`/vendor/etc/init/ipacm.rc` on the handset.

The read-only logcat reread resolves an important readiness ambiguity. `init`
starts `pd_mapper` at 21:14:03.899 and reports PID 1347 at 21:14:03.940. At
21:14:03.992 the kernel logs `servloc: service_locator_new_server:
Connection established with the Service locator`; the corresponding
`init_service_locator` message says the client is initialized. The modem
firmware request for `modem.mdt` appears at 21:14:06.952, roughly three
seconds later. In the pinned Sunfish kernel,
[`service_locator_new_server()`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/service-locator.c#L1080-L1100)
is the QMI service-discovery callback: it saves the discovered server address,
sets the client `connected` flag, and emits that log message. This is direct
evidence that a SERVREG_LOC server was discoverable and connected before
modem firmware loading. The log does not include the server's node/port or
process identity, so the timestamp alone does not prove that PID 1347 was the
specific server endpoint.

This adds direct timing evidence that Linux brings up QRTR/PD mapping and IPA
around modem power-up and then observes the WLAN PD as UP. It does not expose
the WLFW QMI payload, identify the precise dependency that starts `wlan_pd`,
or explain why the seL4 temporary image stops before QRTR discovery. Ordinary
ADB access cannot read the kernel log (`dmesg` returns permission denied),
and this boot does not expose a `/sys/class/remoteproc` directory; the
sequence above comes from Android logcat and init definitions only. No
additional temporary boot or write to the Pixel was made for this capture.

### Latest settled SERVREG/LOC screen and SMP2P label (2026-10-22)

An earlier stopped run (`D33DF096-0EA0-46D7-BFA9-30E736B4DEF4_1_201_a.heic`)
showed `QUERY STOPPED`, QMI result/error and firmware state still `FFFFFFFF`,
and `NO RESPONSE YET`. QRTR discovery and LOC lookup had not run, so its empty
counters were not evidence of failed SERVREG replies or an empty LOC result.
The latest settled screen (`IMG_8881.heic`) advances past that point: the QMI
state response reports result/error `0/0`, while firmware remains
`7FFFFFFF` / `SERVICE UNINIT`. QRTR servers are `2`; peer HELLO and HELLO sent
are both `1`. The owner stops on the modem-facing err-fatal status, now labeled
`MODEM ERR-FATAL`.

This run made two LOC requests and queued one reply. The per-name counters show
one request each for `tms/pdr_enabled` and `tms/pddump_disabled`, with no
`wlan/fw` request. `FIRST LOC NAME` is `TMS PDR ENABLED`; LOC domains are zero.
`FIRST RXDONE 00000001` confirms the peer released the GLINK receive intent for
the first published reply. It does not prove that the peer accepted or
processed the QMI response. The second (`tms/pddump_disabled`) request has no
queued-reply evidence when the owner stops, so the next useful trace point is
whether dispatch or reply publication began for that request before the modem
err-fatal status appeared. The screen still shows WLAN service uninitialized,
WLFW disabled, and no WLAN start or DMA.

On this captured build, `FIRST RSP CODE 00000005` is a legacy, misleading
label: the value is the stable `qcom_servreg` service-name ID for the first
queued LOC reply (`5 = tms/pdr_enabled`), not a QMI response code. The rebuilt
offline state-probe labels this field `FIRST RSP NAME`; its hexadecimal
value remains the stable service-name ID, separate from the QMI result/error
shown near the top of the screen.

The screen's old `PEER SMP2P FATAL` label comes from bit 0 of the modem-facing
`slave-kernel` entry in SMEM item 435. In the pinned Atoll DTS, the modem
`interrupts-extended` list maps that bit to `qcom,err-fatal`, bit 1 to
`qcom,err-ready`, bit 2 to `qcom,proxy-unvote`, bit 3 to `qcom,stop-ack`, and
bit 7 to `qcom,shutdown-ack` ([modem SMP2P mapping](../.cache/sunfish-lineage-kernel/arch/arm64/boot/dts/qcom/atoll.dtsi#L1507)).
The WLAN-specific SMP2P entry is separate: its bits 0 and 1 map to ICNSS
`qcom,smp2p-force-fatal-error` and `qcom,smp2p-early-crash-ind`
([WLAN mapping](../.cache/sunfish-lineage-kernel/arch/arm64/boot/dts/qcom/atoll.dtsi#L3003)).
So the modem err-fatal status does not establish that `wlan_pd` itself asserted
a fatal bit, and it does not identify an SMMU or firmware cause. The owner
screen now names this state `MODEM ERR-FATAL` to make the distinction visible.
The feature-specific native test keeps that label distinct from GLINK fatal
and generic FIFO access errors.
The updated temporary image was rebuilt offline at
[`fastboot.img`](../.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img)
(SHA-256 `ddb0bc00a34987dc2a44626a1affed1a2f50edf31cadbef876d3c0e25cf522d4`);
the native ELF is `515e05687eb0eabbb9331e1d158ba29ec1b427d4bf65591620adc7edceebe515`.
The 11-target loader-layout proof passes, preserving the original boot DTB and
showing disjoint copy targets and source/relocated payload ranges. A temporary
`fastboot boot` of this image was accepted by the Pixel on 2026-10-22, and the
settled result is recorded above. No partition was flashed and no WLAN start or
DMA is enabled by this probe.

The follow-up display-label correction was rebuilt and validated offline at
the same artifact path. Its Fastboot image SHA-256 is
`d27fe3dbc7d9bb7928761815f02fc3eeee545f7136c662324926bbd6ecc65a31`, and its
native PD ELF SHA-256 is
`2531c7d59cc15b736e0c404b257d826bdcea4ff71ebf5d44c7687e71108e3958`. The
packed-layout check passes with 11 disjoint copy targets, the original boot
DTB preserved, and the 512-page WLAN MSA reservation unmapped. The ELF labels
the first reply field `FIRST RSP NAME` and adds `LOC REPLY WAITS` in place of
the `LOC OTHER` display counter, so the next run can distinguish a reply that
hit transport backpressure from one never attempted before the fatal status.
It retains the same read-only SERVREG scope and starts no WLAN hardware. The
settled screen (`IMG_8882.heic`) shows `QUERY COMPLETE`, QMI result/error
`0/0`, firmware state `7fffffff` (`SERVICE UNINIT`), and `NO OWNER ERROR`.
QRTR discovery completed (`2` servers; peer HELLO and HELLO sent both `1`).
The LOC trace shows two requests, one queued response, first name
`tms/pdr_enabled`, `FIRST RXDONE 1`, zero LOC domains, and one request each
for `tms/pdr_enabled` and `tms/pddump_disabled`; `wlan/fw` remains zero. The
new `LOC REPLY WAITS` counter is `3`. In the owner implementation this counter
increments only when a SERVREG reply enqueue returns `FlowBusy` or
`NoCapacity`. These three observations are aggregate: the capture does not
associate them with either service name. One of the two LOC requests has no
successfully queued reply at the snapshot, but the trace does not prove
whether the waits occurred while queueing that request or before the first
successful reply. The counter also does not distinguish an occupied per-peer
QRTR flow from an exhausted local transmit slot, and `FIRST RXDONE` does not
prove that the corresponding QRTR `RESUME_TX` arrived. The source retains a
reply until both transport completions and permits only one reply for a
destination flow at a time, so a missing or unmatched QRTR resume remains a
concrete hypothesis for the unqueued response, not a device-proven cause. The
modem remains uninitialized; this trace does not establish why or show a WLAN
firmware lookup/start.

The refreshed one-query candidate now reports whether the first reply received
a matching QRTR `RESUME_TX` and packs the separate `FlowBusy` and `NoCapacity`
counts into `LOC WAIT B/C` (high and low 16 bits, respectively). Its build
first found that the saved copy-layout manifest was one page short after the
native ELF grew; the manifest was refreshed from the generated image header,
then the full build and layout validator passed. The CapDL audit reports
18,186 pages, 24 fixed reservations, 512 unmapped WLAN MSA pages, and zero
overlaps. The temporary image SHA-256 is
`be0875cfbcf5581baf8b71bbcdd4c669541848df712e1ce4d3d2706676adc68b`; the
native PD ELF SHA-256 is
`d9c556c07bdf8252a277f46932f8caa735bdd318c9d5dea42327b2e141d15c52`. The
candidate has not yet been booted. It keeps the one-query SERVREG scope and
does not register a listener, issue PDR, transfer MSA ownership or firmware,
configure WLAN DMA, or start the radio.

### Host-service startup-order gap (2026-10-22)

The native diagnostic currently attaches its modem owner after the PAS/auth
and SMP2P/GLINK preparation, version exchange, channel-open, and receive
checkpoint steps. `pixel_modem_owner::attach_with_services()` then registers
the local storage and heap endpoints and prepares the SERVREG map endpoint;
`Runtime::start()` activates caller-supplied storage/heap handlers only after
that attachment. The diagnostic's default `NoServices` provider intentionally
publishes no storage or heap service. This is visible in
[`sunfish_modem_smp2p_probe.rs`](../rust/src/sunfish_modem_smp2p_probe.rs),
[`pixel_modem_owner.rs`](../rust/src/pixel_modem_owner.rs), and
[`qcom_modem_owner.rs`](../rust/src/qcom_modem_owner.rs).

That order differs from the read-only Lineage capture: the AP-side
`vendor.ipacm`, `vendor.qrtr-ns`, `pd_mapper`, and Service Locator connection
are available before `modem.mdt` is requested, and the working boot separately
advertises `heap_memory_sharing` before modem startup. The modem-facing IPA
`INIT_MODEM_DRIVER` exchange is later: the request appears about 591 ms after
reset release, its response about 635 ms after reset release, and the
`wlan_pd` SERVREG UP indication about 2.35 seconds after reset release. Thus
`ipa_host_ready` in the offline lifecycle preflight means the AP IPA provider
is available before PAS boot; it must not be interpreted as requiring the
modem-facing IPA QMI exchange before reset release. That exchange belongs to
post-boot progression and needs live IPA resource values. The preflight also
requires SERVREG mapper, remote-storage, heap-memory-sharing, and TFTP
providers before the modeled modem boot. The model remains synthetic; no
production backend is constructed or activated by the current native
diagnostic. This ordering is a useful comparison, not evidence that absent AP
services caused the observed modem `err-fatal`, and it does not justify
asserting modem/WLAN SMP2P bits or transferring memory. A production lifecycle
needs to establish the pre-boot providers, then connect their ready state to
post-GLINK endpoints and carry out the live IPA exchange without advertising
unbacked services.

As an additional post-channel gate, `RuntimeServices::memory_services_ready()`
is now false for `NoServices` and becomes true for `ModemHostServices` only
after the backend validates and the remote-storage/heap endpoints are
advertised. An owner error clears that readiness. WLFW MSA_INFO, MSA handoff,
and capability progression now stop while this readiness is false; the native
owner test confirms that the diagnostic path performs no SCM call or MSA/WLFW
progress without those handlers. This protects the later memory transition;
it does not satisfy the separate IPA/SERVREG/TFTP pre-PAS startup gate or
provide a production storage/SCM backend.

### AP IPA server runtime wiring (2026-10-22)

The native owner now has an opt-in `IpaServerServices` adapter for the AP-side
IPA QMI service. It registers the local instance-1 endpoint, advertises only
after the owner observes QRTR HELLO and a caller supplies a generation-matched
`ApReadyPermit`, and services only the observed modem peer tuple. It retains
responses across QRTR/GLINK backpressure and commits the `0x0020` / `0x0035`
response state and `0x0022` indication only after both transport completions.
The storage/heap handler now selects only those two services from the shared
request queue, leaving SERVREG LOC and IPA requests for their dedicated
dispatchers.

Offline tests reject a stale IPA readiness generation and confirm the service
is not announced before peer HELLO. The adapter has no production caller: the
default owner still uses `NoServices`, and no trusted live IPA provider can
currently mint `ApReadyPermit`. This advances the post-GLINK protocol path; it
does not implement AP IPA register/GSI/SMMU initialization or satisfy the
separate requirement to start AP providers before modem PAS boot.

The shared QMI request queue now selects the oldest request for each registered
dispatcher. SERVREG and AP IPA no longer treat a different service at the head
of the queue as their own; an offline owner test queues IPA before a SERVREG
lookup and verifies both service selections and the SERVREG reply.

### Fresh LineageOS modem/WLAN startup baseline (2026-10-22)

After the user returned the Pixel to LineageOS, ADB confirmed
`ro.lineage.version=23.2-20261001-NIGHTLY-sunfish`. A filtered, read-only
`logcat -b all -d` snapshot records the following boot order (times are local
device log timestamps): `vendor.ipacm` starts at 22:16:42.459;
`vendor.qrtr-ns` at 22:16:46.102; `tftp_server` at 22:16:46.106;
`pd_mapper` at 22:16:46.112; and Service Locator connects at 22:16:46.165.
`vendor.rmt_storage` and `vendor.tftp_server` start at 22:16:46.224 and
22:16:46.226. The modem firmware request begins at 22:16:47.718, with the
`modem.bNN` segments loaded over the following 256 ms. The modem reports its
power/clock-ready interrupt and QRTR readiness at 22:16:48.588 and 22:16:48.589.

Within 23 ms of QRTR readiness, `rmt_storage` opens `/boot/modem_fs1`,
`modem_fs2`, `modem_fsg`, and `modem_fsc`. It then reads 512 bytes from each
first pair, 2,096,128 bytes from `modem_fs1`, and 65,536 bytes from `modem_fsc`.
IPA logs `QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` at 22:16:48.881 and its response
at 22:16:48.912. The kernel records a SERVREG `msm/modem/wlan_pd` UP indication
with state `0x1fffffff` at 22:16:50.272 and its ACK at 22:16:50.302; ICNSS
connects to the WLFW QMI server at 22:16:50.280. `WLAN FW is ready` appears at
22:16:56.708, followed by the QCACLD firmware-ready event at 22:17:01.460. A
TFTP-server wake lock is visible, but this log does not expose a TFTP filename
or request. Thus the WLAN domain and firmware become ready on this Lineage
boot, with IPA, SERVREG, remote-storage, and TFTP services started before
modem load. This baseline does not establish which individual provider is
necessary to prevent the native seL4 modem watchdog, and it says nothing about
whether the separate Linux-firmware candidate is compatible or licensed for
Sunfish. No reboot, service action, firmware transfer, or device write was
performed for this capture.

A second filtered read-only capture on the same LineageOS build independently
confirms that order. In this boot, `vendor.ipacm`, `vendor.qrtr-ns`,
`tftp_server`, `pd_mapper`, Service Locator, `vendor.rmt_storage`, and
`vendor.tftp_server` all started before the `modem.mdt` request. The modem was
brought out of reset at device-log time `23:46:37.921`; the power/clock-ready
interrupt and QRTR readiness followed 48 ms and 51 ms later. `rmt_storage`
opened `/boot/modem_fs1`, `modem_fs2`, `modem_fsg`, and `modem_fsc`, then
served 512-byte reads from the first two, a 2,096,128-byte read from
`modem_fs1`, and a 65,536-byte read from `modem_fsc`. The IPA
`INIT_MODEM_DRIVER` request and response occurred 351 ms and 389 ms after
reset release. `wlan_pd` reported UP and ICNSS connected to WLFW about 2.23 s
after release; `WLAN FW is ready` and the QCACLD firmware-ready event followed
at 9.21 s and 10.17 s. This repeat strengthens the startup-order evidence but
still does not isolate which dependency is necessary or prove the cause of the
native failure. The capture read existing ADB/logcat state only; it did not
restart services, change logging, or transfer firmware.

The next native comparison should first establish an AP bootstrap that can
prove IPA, SERVREG mapping, remote storage, heap sharing, and TFTP providers
ready before PAS reset release, then record those readiness points beside the
existing modem/GLINK milestones. The current native probe supplies none of
those preboot providers as production services; its post-GLINK SERVREG/IPA
adapters do not satisfy this ordering. Keep WLAN-domain start, MSA ownership,
DMA, and firmware serving disabled until the provider contracts and target
authority are implemented and validated.

### ICNSS WLFW default follow-up in the bounded owner (2026-10-22)

The pinned Lineage ICNSS source enables the FW-rejuvenation indication in its
default registration, then sends MSA_INFO, performs the MSA permission handoff,
sends MSA_READY and CAP, and issues the dynamic-feature-mask request with
`ICNSS_DEFAULT_FEATURE_MASK` (`0x01`). ICNSS ignores that request's return
value, so the transaction is best-effort. The native bounded owner now matches
the registration subscription and queues QMI `0x003b` after an accepted CAP
response. A matching response or an explicitly reported bounded transport
timeout retires the step; the protocol layer does not guess timeouts from a
local clock.

Offline tests exercise the registration TLV and the mask request/response over
the owner’s QRTR/QMI and GLINK path. The complete `test-modem-qmi.py` suite and
its AArch64 no_std/native-codegen checks pass. This fills an ICNSS protocol
ordering gap only: it cannot run until a WLFW server appears, and the latest
device captures still show modem `err-fatal` before `wlan/fw` discovery. It
does not clear that fatal state or provide the pre-PAS IPA, SERVREG mapping,
remote-storage, heap-sharing, and TFTP services seen in the Lineage baseline.
The owner build containing this change was not booted. After offline validation,
a separate read-only SERVREG probe was temporarily booted to investigate the
observed LOC reply-queue discrepancy; its screen result is pending. No
partition was flashed, and no PDR, firmware transfer, DMA, or WLAN-start action
was performed.

References: the pinned [Sunfish ICNSS startup path](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c) and [ICNSS QMI codecs](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss_qmi.c).

### LOC reply QRTR-credit trace candidate (2026-10-22)

The `IMG_8882.heic` result makes reply flow-control the next useful
read-only discriminator. A matching QRTR `RESUME_TX` for a published QMI reply
now emits `ReplyQrtrResume(RequestHandle)`, parallel to the existing GLINK
`ReplyRxDone` event. The SERVREG owner correlates both events with the first
successfully queued reply. It also counts reply waits by cause instead of
merging `FlowBusy` and `NoCapacity`. In the state-probe display,
`QRTR RESUME` is `1` only when the first reply got a matching resume;
`LOC WAIT B/C`
packs `FlowBusy` attempts in the upper 16 bits and `NoCapacity` attempts in
the lower 16 bits. The latter occupies a display row previously used for the
disabled WLFW error value; the explicit `WLFW CLIENT DISABLED` state remains
visible.

The owner integration test reproduces the capture's shape: the first SERVREG
reply receives GLINK `RX_DONE`, three later reply attempts return `FlowBusy`,
and a matching QRTR resume retires the first response and permits the second
reply to queue. This validates the trace path against the transport model; it
does not prove that the handset's three waits had the same cause. A fresh
`IMG_8882.heic` photo still shows the earlier probe layout, without the new
`QRTR RESUME` or `LOC WAIT B/C` rows, so the updated trace has not yet been
observed on the handset.

The first trace revision's task manifest pinned the native ELF at
`0d541d70bfc1d4bca71c3effb72455dfbce90b0c0586e714d0978d3ec4421f0e`.
`build-modem-servreg-state-probe.sh` completes, and
`python3 platforms/sunfish/test-modem-qmi.py` passed, including AArch64
no-std/native code generation. CapDL reported 18,185 pages, 24 fixed
reservations, 512 unmapped WLAN MSA pages, and zero overlaps. That revision's
temporary Fastboot image SHA-256 was
`b6b83b20a1b1a9fe437e498d19592eb0103ea6e70c00df266133f5ac66b1fbfc`.
Packed-layout validation confirms 11 disjoint copy targets, the original boot
DTB preserved, and source/relocated ranges disjoint. The on-screen label is
`QRTR RESUME`, matching the value's meaning: a matching QRTR `RESUME_TX` for
the first queued reply. This revision was superseded by the later rebuilt
candidate below; it retained the one-query SERVREG scope and no-PDR, no-MSA,
no-WLAN-DMA, no-WLAN-firmware-transfer, no-radio-start limits.

### Per-name LOC reply-wait trace candidate (2026-10-23)

The `IMG_8882.heic` capture reports three reply-enqueue waits in aggregate
but does not identify which LOC name was being answered. The owner trace now
attributes each `FlowBusy` or `NoCapacity` enqueue wait to the requested
service name. The display adds `WAIT PDR ENABLE`, `WAIT PDDUMP OFF`, and
`WAIT OTHER NAME`; `LOC WAIT B/C` still reports the aggregate causes, with
`FlowBusy` in the upper 16 bits and `NoCapacity` in the lower 16. These views
are complementary: they identify the name and aggregate cause counts, but do
not yet provide a name-by-cause matrix. The native transport integration test
queues the `tms/pdr_enabled` reply, then drives three `FlowBusy` attempts for
`tms/pddump_disabled`; it asserts all three waits are attributed to the second
name and none to the first or an unknown name. This validates dispatcher
attribution against the mocked transport path, not the handset's actual cause
or per-name counts.

The combined one-query candidate now reports per-name wait counts, splits
`FlowBusy` from `NoCapacity`, and correlates the first queued reply with a
matching QRTR `RESUME_TX`. `build-modem-servreg-state-probe.sh` passed, including
the 11-target loader-layout proof. CapDL reports 18,187 pages, 24 fixed
reservations, all 512 WLAN MSA pages unmapped, and zero overlaps. The temporary
[`fastboot.img`](../.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img)
has SHA-256
`0030996e24cfc0b8fe6d964f079bf9eac0274ca9f6b401265c7c3576c6060edc`; its
native PD ELF SHA-256 is
`e768149c8fa1d8bac8eccf72885fde558982c6df471f88465fd641c7987af6b3`.
The image preserves the original boot DTB and verifies disjoint copy targets
and source/relocated payload ranges. Rebuilding found that the pinned native
ELF hash in `loader-task-regions-modem-servreg-state.json` was stale; after
updating that hash, the full build and layout validator passed. The validator
reports the source image at `0xb0080000`, relocated loader at `0xc0000000`,
11 disjoint copy targets, and no overlap with the cached live fixed
reservations. CapDL reports 18,187 pages, 24 fixed reservations, all 512 WLAN
MSA pages unmapped, and zero overlaps. This exact revision has not yet been
booted. It keeps the read-only one-query SERVREG scope: no listener
registration, PDR, MSA ownership or firmware transfer, WLAN DMA, or radio
start. The `IMG_8882.heic` photo is from the preceding layout, so none of the
new trace fields has yet been observed on the handset.

### Read-only IPA/GSI register probe candidate (2026-10-23)

The user-provided `IMG_8882.heic` shows the SERVREG/LOC diagnostic rather than
IPA registers. It reports `QUERY COMPLETE`, QMI result/error `0`, `SERVICE UNINIT`, `NO OWNER
ERROR`, `FIRST RSP NAME 00000005`, `FIRST RXDONE 00000001`, two LOC requests,
one queued response, three aggregate enqueue waits, zero LOC domains, and no
WLAN-FW LOC name. The image does not associate the waits with a service name.
This advances the LOC reply trace but does not identify AP-side IPA state.

The next offline candidate is a separate one-PD read-only register snapshot.
The generated Sunfish SDK DTB pins `/soc/qcom,ipa@1e00000` to
`reg = <0x1e00000 0x34000>, <0x1e04000 0x2c000>` and IPA hardware version
`0x10` (IPA v4.2); the pinned LineageOS source maps IPA v4.2 to GSI v2.2 and
uses execution environment 0. The probe maps the containing
`[0x01e00000, 0x01e34000)` window once with Microkit `perms="r"`, then reads
11 fixed IPA/GSI configuration registers twice using volatile reads. Each
sample reads `IPA_VERSION` first and stops before touching the other ten
registers if the value is zero, matching the pinned Linux driver's access
check. The build's `validate-ipa-read-probe-source.py` checks each IPA offset
against both the register header and the inherited v4.2 register table, plus
the GSI v2.2 parameter offset/mask, IPA-to-GSI version mapping, and nonzero
version gate against the unchanged Lineage kernel commit
`28f9290ae067d3e7857126eb6312720dbe9c7c80`. The two samples are
displayed as stable or changed alongside the hardware version, IPA shared
memory offset/size in 8-byte units, configuration values, and GSI event-ring
count.

The probe does not request interrupts, SMC, PDR, a power/reset transition, GSI
commands, DMA, firmware transfer, or WLAN startup. Its only writable mapping
is the existing reserved splash framebuffer used for its report. The
isolation tests pass, the Rust image and Microkit loader build, and the CapDL
audit reports 98 pages, 24 fixed reservations, 52 IPA MMIO pages, 11 splash
pages, and zero overlaps. The temporary Fastboot image SHA-256 is
`e16d0842d175a78d9c4768c3db65164e1a88db09f71b162ba2ce4c901c0496e3`.
`fastboot boot` accepted this image for a temporary run; no partition was
flashed. The attached `IMG_8882.heic` was the SERVREG/LOC screen, not the IPA
probe. After the subsequent probe boot, the phone was again reachable in
LineageOS; both `ro.boot.bootreason` and `sys.boot.reason` report `watchdog`.
That reason does not identify which stage triggered the reset; ordinary ADB
also cannot read `/sys/fs/pstore`, and `/proc/last_kmsg` is absent. No current
IPA/GSI values were captured. The next attempt needs a photo immediately after
the IPA screen settles. This snapshot can establish the AP IPA/GSI register
state; it does not by itself identify the WLAN firmware, modem, or SMMU failure
cause.

### Refreshed LineageOS IPA/WLAN baseline (2026-10-23)

The bounded `capture-lineage-ipa-state.sh` script was rerun while the Pixel
was attached in LineageOS 23.2-20261001-NIGHTLY-sunfish. Ordinary shell still
cannot read `/sys/kernel/debug/ipa/hw_type` or `gen_reg`. The kernel log records
`QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` followed 39 ms later by its response, but
does not expose the decoded response payload or the IPA register/SRAM values.

The capture was repeated after the latest SERVREG/LOC diagnostic photo. At
that time ADB identified one Sunfish running the same LineageOS build, while
Fastboot showed no device. The available kernel ring buffer had no matching
IPA messages, and the debugfs reads remained unavailable without permission
changes. The existing Android log buffers again showed the modem IPA request
and response, the `wlan_pd` ready indication (`0x1fffffff`), ICNSS connected at
state `0x980`, and both ICNSS and WMA firmware-ready events. This confirms the
stock LineageOS baseline on that boot; it does not change or explain the
custom SERVREG probe's `SERVICE UNINIT` result.

The same passive boot-log capture shows the stock path reaching the WLAN
ready event: the modem is brought out of reset, QRTR reports modem QMI
readiness, the service notifier receives `msm/modem/wlan_pd` state
`0x1fffffff` and ACKs it, `icnss_qmi` connects at state `0x980`, and the log
later reports `WLAN FW is ready: 0xd87` followed by the WMA firmware-ready
event. The `vendor.ipacm`, `vendor.qrtr-ns`, `tftp_server`, `pd_mapper`, and
service-locator startup records were also present. This is evidence that the
LineageOS modem/WLAN path completes its ready notification on this boot; it
does not prove which AP IPA values were supplied or that the custom seL4 path
has equivalent IPA/GSI initialization.

### Bounded AP GSI event/context executors (2026-10-23)

`rust/src/sunfish_ipa_gsi_exec.rs` now provides a one-shot executor for the
45 source-derived v2.2 AP event/channel context writes emitted by
`sunfish_ipa_gsi_profile`. It validates the complete aligned register-offset
set against the Sunfish GSI aperture before the first access, preserves write
order, stops on the first I/O error, reports only the confirmed prefix, and
never retries an uncertain write. Host tests cover full ordered execution and
failure at a selected write; the module is included in the AArch64 no-std
compile check.

The event-command executor writes one source-defined ring-allocation command
only after receiving the completed AP-channel proof, matching Linux's
GSI-v2.2 boot order, and permanently quarantines its reserved IDs after an
uncertain MMIO result.
The event-control reader consumes the expected event IRQ bit, clears it as the
pinned Linux ISR does, and confirms the matching event context state. The
channel executor decodes `GSI_NUM_CH_PER_EE` from AP `HW_PARAM_2`, checks EE0
`GSI_STATUS.ENABLED` at `0x12000`, and reads CHSTATE for each AP channel into
an immutable snapshot. A disabled manager is rejected before any channel
context reads or allocation commands. Every nonzero state is treated as
occupied; an undersized channel count is rejected before context reads. The
allocator consumes that fresh snapshot, then allocates the ten
EE_AP channels in the source endpoint order
`[5, 0, 6, 7, 1, 2, 3, 4, 8, 9]`. Each one-shot `GSI_CH_ALLOCATE` command uses
`0x12008`; the resulting control IRQ and CHSTATE `ALLOCATED` readback must
match before the next command. Any uncertain write or completion quarantines
the full reservation. The context planner now requires the proof token minted
only after all ten AP allocations complete. This implements the AP subset;
Q6/UC allocation and shared interrupt routing remain outside the adapter.

The context executor writes the source-ordered 45-word context plan and returns
the event-ring prime pointers and AP channel starts tied to that plan. The
prime step follows `gsi_prime_evt_ring`: for each dynamic event ID, it writes
the last-element pointer (`ring base + ring bytes - 16`) MSB then LSB. A
channel start is blocked until all three event rings are primed. It then sends
the one-shot EE0 `GSI_CH_START` word to `0x12008`, requires the exact channel
control IRQ and channel bitmap, acknowledges that bit, confirms CHSTATE 2
(`STARTED`) or 5 (`FLOW_CONTROL`), and writes the transfer-ring pointer MSB to
the channel doorbell as the kernel does. Any uncertain command, acknowledgement,
post-ack read, or doorbell result quarantines the generation instead of
replaying it.

This is still an offline-tested low-level path, not IPA bring-up. The caller
must supply a fresh complete AP channel-state snapshot, provide exclusive GSI
MMIO and IRQ delivery, back and zero/clean the DMA ring pages, configure IPA
endpoints, and install valid DMA/SMMU mappings. The executor does not enable
event interrupts, configure GSI interrupt masks or routing, create those DMA
mappings, allocate Q6/UC channels, or establish IPA/WLAN readiness. Host tests
cover AP allocation order/completion, context writes, event-prime pointers,
channel starts, and quarantine paths; no hardware execution has been
attempted.

### GSI v2.2 manager readiness boundary (2026-10-23)

The pinned LineageOS `gsi_register_device()` path reads EE0
`GSI_STATUS.ENABLED` at GSI offset `0x12000` after configuring the interrupt
masks. It considers the manager usable only when bit 0 is set ([GSI driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/gsi/gsi.c), [register definitions](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/gsi/gsi_reg_v2.h)). The AP channel
snapshot now verifies that same source-defined readiness bit before reading
channel states, and event allocation requires the token minted only after all
ten source-required AP channels have reached `ALLOCATED`. This prevents the
executor from treating readable capacity registers as proof that firmware is
running.

There is a separate global initialization path: the kernel comments that for
IPA 3.1 and later, GSI configuration is performed by TrustZone; the driver's
direct `gsi_configure_regs()` pre-firmware setup is used for IPA 3.0 or
emulation. Sunfish IPA v4.2 maps to GSI v2.2, so copying the direct
`gsi_enable_fw()` register writes into the seL4 host path would not reproduce
the production boot sequence ([IPA initialization](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa.c)). The custom system still needs a trusted IPA/GSI
firmware and secure-start path that makes the EE0 status bit true. The current
executor only verifies that boundary; it does not try to enable GSI. Focused
host tests cover enabled/disabled status and the channel/event ordering. No
device write or GSI hardware test was performed.

### Fresh LineageOS WLAN reference capture (2026-10-23)

With the Pixel attached in LineageOS 23.2-20261001-NIGHTLY-sunfish, the
read-only `capture-lineage-ipa-state.sh` capture again found the modem/QRTR
startup path and WLAN readiness sequence. `QMI_IPA_INIT_MODEM_DRIVER_REQ_V01`
received a response; the service notifier observed `msm/modem/wlan_pd` at
state `0x1fffffff`; ICNSS connected to the QMI server at state `0x980`; then
the kernel reported `WLAN FW is ready: 0xd87` and the WMA firmware-ready event.
The capture found no matching IPA/GSI lines in the available kernel log buffer,
and `/sys/kernel/debug/ipa/{hw_type,gen_reg}` remained unavailable without
permission changes. This confirms the LineageOS reference path is currently
working, but does not establish GSI `STATUS.ENABLED` or the IPA firmware
startup result. No device state or logging configuration was changed.

The post-diagnostic return capture is saved at
`.cache/pixel-sunfish/lineage-current-baseline-20261023/filtered-state.txt`
(SHA-256 `ddeae3c97bda0ce049a71b5b8e955b6774f0b715c4aa0f2b0f85c7de670fe925`).
This boot started `vendor.ipacm` at 09:30:40.624 local device time, followed
by `vendor.qrtr-ns`, TFTP, `pd_mapper`, and remote storage before modem image
loading at 09:30:45.896. The modem was released from reset at 09:30:46.947;
the IPA modem-driver QMI request received a response 38 ms after submission.
At 09:30:49.135 the service notifier received `wlan_pd` state
`0x1fffffff`; ICNSS logged server state `0x980`, then the notifier ACKed the
indication. The kernel reported WLAN firmware ready at 09:30:58.115 and the
WMA firmware-ready event at 09:30:59.071. This fresh return confirms the
read-only BCM diagnostic did not leave the phone in its temporary image and
that the stock WLAN path again reached firmware readiness. The capture still
does not supply IPA register/SRAM values or the raw IPA QMI response. It used
existing log buffers and did not change services, logging, or hardware state.

### Pinned ICNSS startup-versus-recovery cross-check (2026-10-23)

The fresh LineageOS boot reached WLAN firmware readiness, so I rechecked the
exact Sunfish Lineage kernel path before choosing another device experiment.
At pinned commit `28f9290ae067d3e7857126eb6312720dbe9c7c80`,
`icnss_driver_event_server_arrive()` runs after the WLFW QRTR server arrives:
it connects the QMI client, powers on the WLAN hardware path, registers
indications, sends MSA_INFO, assigns the MSA permission, sends MSA_READY,
requests capabilities, and sends the dynamic feature mask. This explains the
post-arrival work visible in a working Linux boot; it does not show what
initially launches `wlan_pd` or causes WLFW to appear.

The same source keeps `RESTART_PD` in a separate recovery path.
`icnss_pd_restart_enable()` resolves the service location, while
`icnss_enable_recovery()` registers the recovery machinery. Only
`icnss_trigger_recovery()` checks that PDR is registered and a service-notifier
handle exists before calling `service_notif_pd_restart()`. Therefore the failed
one-shot `RESTART_PD` experiment is not evidence that this transaction is part
of cold startup, and repeating it is not the next useful test. The
Service Notifier's new-server work registers a listener and then delivers its
current state, consistent with using SERVREG for observation and recovery
coordination.

`platforms/sunfish/validate-icnss-startup.py` checks these boundaries and the
server-arrival operation order against the clean pinned Lineage source. It
passes. This narrows what can be claimed: Lineage's running sequence is
observed, ICNSS post-arrival setup is source-backed, and the private initial
`wlan_pd` launch trigger remains unresolved. No restart request, device write,
firmware transfer, or secure call was made for this audit.

References: pinned [ICNSS server-arrival and QMI setup](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c#L958-L1042), [ICNSS recovery registration and trigger](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c#L2043-L2100), [guarded PDR trigger](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c#L2556-L2602), and [Service Notifier server-arrival handling](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/service-notifier.c#L343-L370).

### Read-only IPA/GSI EE0 status snapshot (2026-10-23)

The LOC screen `IMG_8882.heic` reports a completed SERVREG query (`QMI` result
and error both zero), `SERVICE UNINIT`, `NO OWNER ERROR`, first reply name
`tms/pdr_enabled`, first-reply GLINK `RX_DONE`, two LOC requests, one queued
reply, and three subsequent reply-backpressure waits. The wait count narrows
the SERVREG transport issue but does not explain the AP-side IPA/GSI state.

To make the next device check address that separate dependency, the existing
one-PD IPA read-only snapshot now also samples GSI EE0 `GSI_STATUS` at IPA
window offset `0x16000` (GSI base `+0x4000`, register offset `+0x12000`) and
reports the raw value plus `ENABLED` bit 0. The pinned LineageOS
`gsi_register_device()` reads this same register and treats bit 0 as the
manager-enabled gate ([driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/gsi/gsi.c), [register definition](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/gsi/gsi_reg_v2.h)). The register remains inside the single read-only IPA MMIO mapping; the probe performs only volatile reads, with no GSI command, interrupt, SMC, DMA, firmware transfer, or WLAN start.

Source validation, five probe isolation tests, formatting, the AArch64 release
build, Microkit packaging, and CapDL overlap audit pass. The audit reports 98
pages, 24 fixed reservations, 52 IPA MMIO pages, 11 splash pages, and zero
overlaps. The temporary image is
`.cache/pixel-sunfish/ipa-read-probe/fastboot.img` (SHA-256
`e1e5b075f28792aa1989a377985a13be968769c0f9cb53c2d313de0e7aaeff1b`); its
native PD ELF is SHA-256
`e5011c8b957fa30c26faf84f153b0abdac3ea1c6e85fb64ad18d68bc816daf82`. Before
each volatile MMIO read the display now shows the sample number and exact
register offset, so a stalled access can be localized from the screen. The
Pixel accepted the previous revision through `fastboot boot` on 2026-10-23;
it later returned to LineageOS with both boot-reason properties set to
`watchdog`, but no screen photo was received. That reset does not establish
whether any GSI read completed or caused a fault. The instrumented revision
was subsequently accepted through `fastboot boot`; its last-register
checkpoint screen was requested immediately. After roughly one minute, the
host no longer enumerated the Pixel over ADB, Fastboot, or USB. On the next
check ADB saw it in LineageOS again; both boot-reason properties were
`watchdog`, and uptime was about 140 seconds. No screen photo was received, so
the temporary USB loss was transient but the register stage and result remain
unknown. No partition has been flashed.

### IPA firmware startup order from pinned Sunfish source (2026-10-23)

The pinned LineageOS [`ipa3_load_ipa_fw()` implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa.c)
attaches the IPA device to its SMMU, starts `ipa_fws` through
`ipa3_pil_load_ipa_fws()` for IPA v3.5 and later, then calls
`ipa3_post_init()`. Sunfish's [Atoll device tree](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/boot/dts/qcom/atoll.dtsi)
selects IPA v4.2 and defines `ipa_fws` as a generic PIL-TZ subsystem with PAS
ID `0x0f`, firmware name `ipa_fws`, force-shutdown behavior, and the
`pil_ipa_fw_mem` reservation. The Atoll IPA node does not set
`qcom,platform-type`; the driver therefore keeps its `IPA_PLAT_TYPE_MDM`
default rather than taking the APQ `ipa_uc` path. `subsystem_get()` starts the
subsystem, and the [generic PIL-TZ loader](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/subsys-pil-tz.c)
performs secure PAS init-image, memory-setup, and authenticate/reset calls
before the IPA post-init sequence.

`validate-ipa-read-probe-source.py` now audits that source order and DTS/PAS
contract, the MDT segment-loading rules, and the active no-map firmware
reservation alongside the IPA and GSI register offsets. The new
`sunfish_ipa_fws.rs` planner applies the pinned Linux relocatable/fixed-address
placement logic to the active 64 KiB region, checks exact `.bNN` segment sizes,
and reports the optional PAS MEM_SETUP extent before any write. Its staging
adapter validates every file before the first write, copies in ELF header
order, and zeros only each segment's BSS tail; synthetic tests use fake
memory. Seven planner/staging tests and two MDT parser tests pass. The IPA/GSI
probe build also passes its five isolation tests, AArch64 release build,
Microkit packaging, and 98-page zero-overlap CapDL audit. This is source
evidence and an offline staging implementation, not a secure PIL
implementation for seL4. No IPA firmware bytes are available in the workspace;
the referenced local Sunfish backup contains only boot, dtbo, and vbmeta slot
images, with no vendor partition or IPA firmware source.
the custom path still lacks an authorized IPA firmware provider, secure PAS
start/authorization, IPA SMMU attachment, and remaining platform resource/power
integration. The read-only GSI enabled bit cannot substitute for those steps
or by itself diagnose the current WLAN service-uninitialized result. No secure
call or hardware start was attempted.

### SERVREG state-update ACK path (2026-10-23)

The pinned LineageOS [`service-notifier-private.h`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/service-notifier-private.h)
schema and [`service-notifier.c`](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/service-notifier.c)
define and handle
`STATE_UPDATED_IND` (`0x22`) with the service state, service path, and a
separate indication transaction ID, followed by `SET_ACK` (`0x23`) carrying
the path and that indication ID. The bounded Sunfish client now decodes that
exact schema, requires a successfully registered `wlan_pd` listener, updates
the latest state, emits one matching `SET_ACK`, and sends QRTR `RESUME_TX`
when the indication requests receive confirmation. A successful ACK returns
the client to listener-ready state. An explicit failed ACK latches the client
in `AckFailed`; the indication is retained, no retry or later update is
accepted, and RESTART_PD stays gated. RESTART_PD still requires the most
recent state to be `UNINIT` and, when an indication exists, its successful
ACK. This implements the Linux notifier's notification/acknowledgement
contract; it does not start a protection domain or load firmware.

The codec and client fixtures cover schema identity, listener gating, QRTR
padding and endpoints, exact ACK contents, latest-state restart gating, and
failed-ACK handling. The GLINK/QRTR integration fixture covers the inbound
indication, separate receive confirmation, ACK request publication, and
matching response. The focused client suite passes 36 tests, the GLINK/QRTR
suite passes 122 tests, and `platforms/sunfish/test-modem-qmi.py` completes its
host routing/GLINK/FIFO tests and AArch64 no-std code generation checks. No
device listener registration or `SET_ACK` was performed; the authorized
device probe remains one state query with passive observation. A device test
of this new path would be a distinct SERVREG interaction and has not been
attempted.

The existing one-query handset image was rebuilt against the updated client
and owner display. The boot-layout validator initially caught stale task-image
size pins; the SERVREG task manifest now records the rebuilt ELF hash and its
11 generated copy ranges. The final build passes its source, task-image,
CapDL, memory-range, and boot-layout checks: 18,188 pages, 24 fixed
reservations, 11 splash pages, 512 unmapped WLAN MSA pages, and zero overlaps.
The temporary image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`57ecbafadc03dd594d8a9fcf7b878fade45b987cb2651948979400fbf0e01a54`); its
native PD ELF is SHA-256
`895551ec9a81af45ae040ca59c22d5a1fa53aa5bdd039d9a84b195f2c535f0fa`. It
still performs one `QUERY_STATE` only; it does not register a listener or send
`SET_ACK`. Fastboot accepted a temporary boot of this image on 2026-10-23;
the diagnostic screen result is pending. No partition was flashed.

### IPA-FW PAS0xf sequence model (2026-10-23)

The existing [`sunfish_ipa_fws.rs`](../rust/src/sunfish_ipa_fws.rs) planner
and staging routine now compose through
[`sunfish_ipa_pas.rs`](../rust/src/sunfish_ipa_pas.rs), which encodes the
Sunfish PIL-TZ order: query PAS operation availability, issue one
`PAS_INIT_IMAGE`, issue `PAS_MEM_SETUP` only for a relocatable MDT, stage all
preflighted segments and BSS tails, then issue one `PAS_AUTH_AND_RESET`.
Metadata is required to be aligned, non-overlapping with the active IPA-FW
reservation, and non-overflowing. Physical backing and ownership must be
verified by a future allocator adapter; the sequence model does not claim that
authority. A failed or uncertain secure call, changed metadata range, or
staging error latches the session and prevents later calls. The generic
`ResourceEnvelopeTransport` now validates each sealed SCM request against its
call class, sends availability queries and MEM_SETUP without a vote, and
brackets INIT_IMAGE and AUTH_AND_RESET with one shared PAS-vote lease. It
attempts to release that exact lease after an SMC error too; release failures
are reported distinctly and latch the start session without retry. Host tests
cover call order, mismatched request rejection, vote-acquisition failure, SMC
failure cleanup, vote-release failure, and combined SMC/release failure. The
pinned PIL-TZ source takes the shared PAS bandwidth vote around INIT_IMAGE and
AUTH_AND_RESET, but not MEM_SETUP. AUTH also passes through the generic
descriptor regulator/clock helpers; the Sunfish `ipa_fws` node declares no
per-device regulators or clocks.

The source audit now pins the exact bandwidth table too. Its idle use case is
zero bandwidth; its active `MSM_BUS_MASTER_CRYPTO_CORE_0` to
`MSM_BUS_SLAVE_EBI_CH0` request is `ib = 492 * 8 * 1,000,000` and
`ab = 492 * 8 * 100,000` (3,936,000,000 and 393,600,000 in the driver's
bandwidth fields). PIL-TZ initializes that shared route from the first secure
PIL device's `qcom,mas-crypto` master, defaulting to Crypto Core 0, and writes
the chosen master into both use cases. The IPA-FW node has no `qcom,mas-crypto`
override. The SM6150 bus description maps Crypto Core 0 through A1NOC/SNOC and
associates its master with BCM CE0; that source topology does not justify
turning the PAS request into a hand-written CE0 command. seL4 still lacks a
production adapter that obtains and balances this shared vote with the other
bus clients. The expanded pinned-source validator checks these IDs, formulas,
dynamic source selection, and the Crypto Core bus-node metadata.

The focused PAS binary passes 34 tests, including exact PAS0xf request words,
relocatable and fixed-address order, bounds, fail-stop behavior, and the vote
envelope. The five IPA read-probe isolation tests, AArch64 read-probe build,
Microkit packaging, and 98-page zero-overlap CapDL audit also pass. A standalone
`aarch64-unknown-none` compile of the PAS core passes. This remains an offline
adapter contract: no production shared-vote implementation, SMC-capability
binding, authorized IPA firmware source, IPA SMMU attachment, physical-memory
provider, or device start exists. The pinned-source validator now checks both
the Linux resource wrapper and the call-class/vote ordering. No secure operation
or device test was run.

### Audit of the pasted SMP2P and WLFW QMI notes (2026-10-23)

The two pasted notes were treated as suggestions to verify against Sunfish's
pinned source, not as permission to assert SMP2P bits or serialize packed C
structures directly. The generic outbound SMP2P bit descriptions are not
established for Sunfish. The Atoll DTS maps WLAN `force-fatal-error` and
`early-crash-ind` on the WLAN SMP2P entry, separately from the modem
`slave-kernel` entry whose bit 0 is `err-fatal`; the photographed owner stop
was later identified as the modem-facing state. No bit was written in response
to the pasted recommendation.

The pinned [`ath10k qmi.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/qmi.c),
[`qmi_wlfw_v01.h`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/qmi_wlfw_v01.h),
and [`qmi_wlfw_v01.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/qmi_wlfw_v01.c)
at commit `fd78d17954a86ea6ceb0c535e618ea129e09f8dd` use a different WLFW sequence:
indication registration, optional host-cap request, MSA_INFO, MSA permission
setup, MSA_READY, then CAP. On MSA_READY indication it fetches board data,
sends BDF_DOWNLOAD, then CAL_REPORT. The startup path does not send the
declared M3_INFO message. Message ID `0x003c` is named `M3_INFO_REQ`, not
`M3_INIT_REQ`; its C input struct has `u64 addr` and `u32 size`, but the QMI
codec encodes those fields as TLVs `0x01` and `0x02` (maximum payload 18 bytes).
That C struct is not a raw QMI wire buffer. WLFW `INI_REQ` (`0x002f`) in this
source only sets `enablefwlog` through `ath10k_qmi_set_fw_log_mode`; it is not
the proposed WLAN INI configuration table.

`validate-wlan-launch-path.py` now checks the exact source revision, M3_INFO
name/fields/TLVs, the ath10k startup order, the separate firmware-log INI
request, and the absence of an M3_INFO call from startup. It passes against the
clean pinned tree. Existing Rust WLFW code continues to encode QMI TLVs rather
than exposing `repr(C, packed)` structs as wire bytes. These source corrections
do not clear the independent blockers: the seL4 runtime still has no production
IPA PAS/bandwidth provider, authorized IPA image source, live IPA SMMU attach,
or source-backed WLAN image launcher/WLFW server. No hardware or secure call
was used for this audit.

### Bounded Sunfish QCACLD HTT receive parser (2026-10-23)

`rust/src/sunfish_qcacld_rx.rs` adds the first borrowed-payload decoder for the
downstream Sunfish QCACLD HTT low-latency RX path. It checks the completed
descriptor's firmware flags, MSDU completion and length-error bits, ring-2
chain count, MSDU length, first/last markers, and target-dependent L3 padding.
The single-buffer API binds parsing and payload slicing to the same buffer,
checks the fixed ring-buffer bounds, and returns no slice for chains or
firmware extensions. This avoids reinterpreting a chained frame as one
contiguous packet while the native chain assembler and ring-reclaim owner are
not implemented.

`platforms/sunfish/validate-sunfish-qcacld-rx.py` verifies the masks and
field definitions against the pinned Lineage kernel at
`28f9290ae067d3e7857126eb6312720dbe9c7c80`; six focused tests pass, and the
module is included in the AArch64 `no_std` compile harness. The parser does
not touch DMA, cache state, interrupts, MMIO, or the device. It is data-path
progress only; modem/WLAN firmware start and isolated DMA remain outstanding.

This behavior is specific to the installed LineageOS QCACLD comparison path:
`htt_rx_amsdu_pop_ll()` reports the continuation count and `ol_rx.c` frees the
chained buffers through `chain_msdus()` instead of delivering the packet. Its
comment calls stitching future work and says the chain is discarded. The
planned native driver reference is upstream ath10k, so QCACLD behavior cannot
determine the native chain policy. Ath10k's own RX indication and buffer
rules must be checked before implementing that path. The QCACLD decoder is a
Lineage comparison primitive; it does not decode ath10k descriptors or provide
the native ring owner/reclaim path.

### Bounded WCN3990 ath10k HTT receive decode and ring preflight (2026-10-23)

`rust/src/ath10k_htt_rx.rs` now follows the pinned upstream Sunfish ath10k
WCN3990 `htt_rx_desc_v2` and the `htt_rx_amsdu_pop()` /
`ath10k_htt_rx_h_unchain()` policy. It checks completion and length-error
flags, ring-2 continuation count, MSDU length and decapsulation. The first
2048-byte ring buffer contributes up to 1688 payload bytes because its 360-byte
descriptor precedes the payload; following buffers contribute payload only.
The bounded assembler joins only a single RAW logical MSDU with an exact
queue/count match and returns explicit drop reasons for unsupported aggregate
shapes.

The same module now parses the TLV `RX_IN_ORD_PADDR_IND` form selected when
firmware advertises `WMI_SERVICE_RX_FULL_REORDER`. It bounds the indication to
the WCN3990 ring's 2047 posted buffers and decodes each 12-byte entry as a
64-bit buffer IOVA, MSDU length, firmware descriptor byte, and retained
reserved byte. It exposes any trailing bytes and never resolves or dereferences
an address. The pinned-source audit checks TLV message dispatch, service-bit
selection, the 64-bit C descriptor, and target-64 handling. This covers the
wire parser only; ring-buffer ownership, address-to-buffer lookup, and reclaim
remain separate work.

`platforms/sunfish/validate-ath10k-htt-rx.py` checks the policy and descriptor
fields against the unmodified pinned Linux files at
`fd78d17954a86ea6ceb0c535e618ea129e09f8dd`. Eight focused decoder tests pass,
and the module passes the AArch64 `no_std` compile harness. It only handles
caller-owned byte slices; it does not perform DMA/cache operations, reclaim a
ring, access the device, or establish radio/firmware compatibility.

The companion `rust/src/ath10k_htt_rx_ring.rs` now models the upstream
WCN3990 64-bit receive-ring setup: 2048 ring slots, 2047 initially posted
2048-byte buffers, a four-byte allocation index, a 360-byte first-buffer
descriptor, and the 48-byte `RX_RING_CFG` payload without its enclosing HTC
header. The source validator compiles the pinned Linux descriptor structures
and checks their size/offsets, then audits the ring geometry and setup order.
The planner writes only caller-owned buffers and checks sorted SMMU mapping
coverage plus distinct physical pages across the ring, index, and buffer pool.
Its five tests pass, bringing the focused ath10k RX total to thirteen. It does not
allocate or authorize pages, perform DMA or cache operations, send HTC/HTT,
reclaim buffers, or start WLAN.

### Bounded ath10k WMI host-memory preflight (2026-10-23)

`rust/src/ath10k_wmi.rs` now caps the service-ready host-memory request list at
the pinned ath10k `WMI_MAX_MEM_REQS` value of 16. A fixed-capacity batch
planner matches every borrowed request to one unique caller-supplied policy,
checks each request's trusted unit count and size limit, and enforces an
aggregate byte ceiling before returning any plans. Unknown, duplicate,
mismatched, or incomplete request sets fail closed. The planner creates no
addresses, allocates no memory, and does not authorize DMA; the Sunfish-specific
request allow-list remains undefined until compatible firmware inputs are
established.

The source validator checks both the 16-entry constant and the Linux
service-ready guard in the pinned Sunfish ath10k tree. The combined
API-5/WMI test binary now passes 26 tests and is included in
`platforms/sunfish/test-modem-qmi.py`, whose AArch64 `no_std` core/native
code-generation checks also pass. No device, firmware, secure call, or radio
path was used for this change.

The batch plan now also places each request in a distinct 4 KiB-aligned IOVA
span inside a caller-supplied bounded pool and encodes the ath10k WMI INIT
`{req_id, ptr, size}` array. Placement validates the 32-bit on-wire address
range, whole-page reservation, aggregate plan size, request identity, and
output capacity before writing. The source validator now checks Linux's
`dma_alloc_coherent()` rounded-size calculation and the chunk record/encoder
against the pinned source. This is still only address planning: no physical
pages, cache state, SMMU entries, or firmware authorization are supplied, and
the Sunfish-specific request allow-list remains open.

The assignment now also encodes the pinned ath10k WMI 10.4 INIT body. It copies
an exact 168-byte caller-validated resource-config block, appends the count and
ordered `{req_id, ptr, size}` records, and retains the source ABI's one-item
minimum chunk slot as zeroed tail space. The source validator checks the 42
resource words, struct order, one-item union, flexible-array sizing, and
zero-initialized skb allocation. The codec selects no resource values and
does not send a WMI command; the Sunfish policy, firmware compatibility, and
physical/SMMU DMA backing remain open.

### Rebuilt SERVREG reply-credit trace image (2026-10-23)

The read-only one-query SERVREG trace candidate was rebuilt against the current
owner and transport sources. The build correctly stopped at the boot-layout
gate when the native ELF had changed but the task manifest still pinned the
prior hash. The manifest was refreshed from that generated ELF, then the full
build passed, including the generated task-manifest comparison, Microkit
package, CapDL audit, and source/relocated image placement proof. The audit
reports 18,187 pages, 24 fixed reservations, 11 splash pages, 512 unmapped
WLAN MSA pages, and zero overlaps; the layout proof verifies 11 disjoint copy
targets, preserves the original boot DTB, and keeps source and relocated
payload ranges separate.

The current temporary image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`cb4c7f7da188925f798c078cf27915029b99ec72655d0c6d63e788156b89c4d7`), with
native PD ELF SHA-256
`337029f806d1b4dd44194f7a33aa29b0ae8e698f332e2dae79637a370d2f0439`. It
reports the first queued reply's matching QRTR `RESUME_TX`, `FlowBusy` versus
`NoCapacity` totals, and reply-wait attribution for PDR-enabled, PDDUMP-off,
and other LOC names. It retains one `QUERY_STATE` only and does not register a
listener, send `SET_ACK` or `RESTART_PD`, transfer firmware or MSA ownership,
configure WLAN DMA, or start the radio. The Mac currently sees no Pixel in ADB
or Fastboot, so this exact build has not been booted; the preceding photo is
from the earlier aggregate-only trace and does not establish these new fields.

### Exact QCACLD WMI INIT resource capture preparation (2026-10-23)

The `IMG_8882.heic` run came from the aggregate-only SERVREG image: it shows
one LOC reply reaching GLINK `RX_DONE`, but does not show QRTR `RESUME_TX` or
identify the three reply-enqueue waits by name or cause. The per-name/cause
SERVREG image is built separately and remains the next reply-path check.

For the native Wi-Fi path, the remaining WMI configuration gap is the exact
resource-config TLV that the working Lineage driver sends after QCACLD applies
its runtime target and host policy. Source-derived WMA counts are useful
candidates, but do not fill the other resource words or prove the final
runtime configuration. The new
[`sunfish-wmi-init-resource-dump.patch`](../patches/sunfish-wmi-init-resource-dump.patch)
logs the complete 276-byte resource-config TLV from the pinned QCACLD serializer
immediately before WMI INIT submission. It logs no pointers or DMA addresses
and does not change the command. The paired
[`parse-wmi-init-resource-dump.py`](../platforms/sunfish/parse-wmi-init-resource-dump.py)
accepts exactly one contiguous dump, verifies tag 75 and a 272-byte value,
then emits all 68 little-endian words and their pinned QCACLD field names. The
resource-policy validator cross-checks the decoder's full field order against
the pinned `wmi_resource_config` declaration, and verifies the host- and
service-ready-dependent WMA updates that shape the final resource TLV. Six host
tests cover decoding, field positions, framing, truncation, discontinuous
offsets, malformed input, and ambiguous repeated dumps. The patch passes
`patch --dry-run` against the clean pinned serializer.

The pinned WMA source confirms why a macro-only resource table would be
incomplete: it zeroes the target config, then replaces peer/TID counts using
the target-version cap and effective `max_station`; it takes the scan limit
from the effective host config and sets offload peer/reorder counts to the
effective host values plus one. Later WMI initialization sets keep-alive
capacity and derives `use_pdev_id`, `new_htt_msg_format`, and peer-unmap support
from service-ready capabilities and host policy. These runtime-dependent
values affect the final 68-word TLV. The capture point is after these decisions,
so the decoder can give the native policy work an observed payload rather than
a table assembled from defaults alone.

The instrumented QCACLD module was built from this pinned source with the patch
applied. The final stripped artifact is
`.cache/sunfish-kernel-diagnostic/wlan-wmi-init-resource-dump-matched.stripped.ko`
(SHA-256 `02fa32b157b197820f77f1b15b6196f94fa2c3bf33902aaf89c902645e8998c3`).
It reports the exact device vermagic
`4.14.357-openela-g28f9290ae067 SMP preempt mod_unload modversions aarch64`,
and its 25,280-byte imported-symbol version section is byte-identical to the
known matched QCACLD module. The `SUNFISH_WMI_INIT_RSRC` marker is present. The
temporary `UTS_RELEASE` build metadata was restored after the build, and the
patched source file was restored byte-for-byte; the patch still passes a dry
run. Six decoder tests and the WMI resource-policy source validator pass.

The authorized LineageOS capture was completed on 2026-10-24. The Pixel was
running kernel `4.14.357-openela-g28f9290ae067`; rooted ADB temporarily replaced
the stock `wlan` module with the matching diagnostic module, then enabled Wi-Fi
to exercise the normal QCACLD initialization path. The probe emitted two
complete 276-byte WMI INIT resource TLVs. Each independently passes the decoder
(tag 75, 272-byte value, 68 little-endian words), and the two records are
byte-for-byte identical. They also match the saved reference capture in
`.cache/sunfish-wmi-init-resource-capture.json` exactly.

The observed resource values include four vdevs, 38 peers, 76 TIDs, three TX
and RX chains, 1,056 MSDU descriptors, `rx_decap_mode = 2`, `flag1 = 0x00180600`,
and `num_packet_filters = 0xffffffff`. Full raw records and decoded JSON are
in `.cache/pixel-sunfish/wmi-init-resource-diagnostic-20261024-{1,2}.{log,json}`.
The seven host decoder tests and pinned QCACLD resource-policy validator pass.

After capture, the Pixel rebooted into LineageOS, loaded its stock-size 9,105,408
byte `wlan` module, and reconnected to the saved `Vanilla` Wi-Fi network. The
temporary module was removed from `/data/local/tmp`. No partition was flashed,
and no firmware bytes, DMA ownership, modem reset, or WLAN start path were
changed by the probe. The module only logged the already-serialized resource
TLV before WMI INIT submission.

The existing Rust regression compares its candidate to all 68 words from this
live TLV and uses the separately captured live `SERVICE_READY` and
`SERVICE_AVAILABLE` bitmaps. The no-radio target probe checks services 99, 176,
and 182 from those captured words, composes the candidate into a WMI INIT/HTC
frame, and parses it back. The full `test-modem-qmi.py` suite passes with this
regression enabled. The HTC endpoint and credit parameters remain synthetic;
there is no raw WMI READY packet capture, and the frame has never been
submitted to the Pixel. The remaining transport gate is to exercise actual INIT
submission and the READY wait through the seL4 HTC/CE path. Native WLAN
operation under seL4 remains unverified.

### Rebuilt per-name SERVREG credit probe (2026-10-23)

The current SERVREG probe was rebuilt from the worktree. Its earlier loader
manifest ELF pin no longer matched the generated native ELF, so I refreshed
only that hash and reran the complete build. The state-probe, transport, and
owner test binaries passed; CapDL reports 18,187 pages, 24 fixed reservations,
512 unmapped WLAN MSA pages, and zero overlaps. The packed-image validator
passes with 11 disjoint copy targets, the original boot DTB preserved, and the
source Image separate from the relocated loader payload. A second full build
produced the same ELF and image hashes.

The rebuilt temporary image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`2c5774369b42256e0f0b41c5b295cda0fe10ab5f2ec80fa24d771120c711e667`); its
native PD ELF SHA-256 is
`40c9bf7207f3b14e64f873cf00ab322c07abff53812b3d06def3c544e8b575f2`. It adds
per-name LOC wait counts, separate `FlowBusy`/`NoCapacity` totals, and a
matching first-reply QRTR `RESUME_TX` indicator to the earlier aggregate-only
trace. The Mac still sees no Pixel in ADB or Fastboot, so this revision remains
unbooted. Its manifest disables both modem-image staging and PAS
authentication/reset; it assumes the separate modem PAS boot has already
completed. It then performs one read-only SERVREG state query and does not
register a listener, send PDR, hand off MSA, transfer firmware, configure WLAN
DMA, or start the radio. A previous authentication-return test passed, but
that result did not prove modem READY or firmware progress.

### Source-derived WCN3990 WMI 10.4 INIT profile (2026-10-23)

`WmiWcn3990ResourceConfig10_4` now constructs the complete 42-word block in
the ath10k WMI 10.4 field order. The base policy uses the WCN3990 hardware
peer override of 33, 16 vdevs, and 96 TIDs. For API-5 metadata selecting WMI
operation version 10.4, pinned ath10k uses the non-TLV fixed
`wmi_service_ready_event`: a 160-byte fixed prefix, 16 service words at offset
20, request count at offset 156, then inline 16-byte memory requests. The raw
WMI 10.4 peer-caching bit is 37; `wmi_10_4_svc_map()` maps it to Linux's
normalized internal `WMI_SERVICE_PEER_CACHING` bit 78. The target profile reads
raw bit 37. The fixed non-TLV 20-byte READY event is decoded separately from
the generic WMI-TLV format. Peer caching changes counts to 528 peers and
66/132 active peers/TIDs, or 51/102 when API-5 metadata also advertises
`peer-flow-ctrl`. Chain masks remain `0x7/0x7`, with native-Wi-Fi decapsulation
and the target's other `TARGET_10_4_*` defaults. It preserves ath10k's
firmware-feature branch: 1,424 MSDU descriptors by default or 2,500 when
validated API-5 metadata advertises `peer-flow-ctrl`. A bounded API-5 parser now reads the padded
`QCA-ATH10K` IE container, captures feature bits and WMI/HTT operation
versions, and skips image payloads without exposing them. The resource-profile
constructor requires parsed WMI version 10.4, so an absent or different version
cannot silently select this layout. This does not establish firmware
compatibility. Its host-memory unit-count helper mirrors ath10k's `+1` slot
for vdevs/peers and active-peer fallback to peer count when active peers are
zero; fixed request counts remain subject to a trusted per-request policy.

The API-5 metadata and WMI suite tests the fixed event layouts, raw versus
normalized service bits, and the INIT command-ID prefix alongside the generic
TLV path. It also composes that command through synthetic HTC WMI_CONTROL
service negotiation and dynamic endpoint/credit state into a WCN3990 CE
descriptor. The pinned-source audit checks HTC and CE fields against the same
Linux revision. No firmware, secure call, DMA backing, or radio path was used.

### No-radio WMI 10.4 target self-test build (2026-10-23)

The existing framebuffer-only Microkit protocol probe now runs a synthetic
API-5 metadata → fixed WMI 10.4 SERVICE_READY → trusted zero-request plan →
resource profile → `0xa000` INIT command check, including raw peer-caching bit
37 and the non-TLV READY decoder. It then models HTC READY and WMI_CONTROL
service negotiation, wraps INIT using the target-assigned endpoint and credit
policy, and serializes the WCN3990 CE descriptor. The display has separate WMI
and HTC TX result lines. The build produced a temporary image with SHA-256
`490f00c61b81edac3a4154bad31fa7ac50b32b3e5fdca77a88939ae6150a0030`; its
protection domain still maps only the reserved splash framebuffer. The
isolation test passes. This image has not been booted and performs no physical
modem transport, firmware, SMC, IRQ, MMIO, DMA, or radio operation.

### `IMG_8882.heic` aggregate-only SERVREG trace (2026-10-23)

The latest screen photo shows the read-only SERVREG query completed with QMI
result/error `0/0`; the WLAN service remains `UNINIT`, the stop reason is `NO
OWNER ERROR`, and `LOC DOMAINS` is zero. It records two LOC requests, one queued
reply, `FIRST RXDONE=1`, and three reply waits. The first response name is
`0x00000005` (`tms/pdr_enabled`); the LOC counters show one request each for
`tms/pdr_enabled` and `tms/pddump_disabled`, and zero requests for `wlan/fw`.
The trace confirms the LOC reply path is active but does not identify a WLAN
firmware launch path. This photo is from the earlier aggregate-only observer;
it does not show QRTR `RESUME_TX`, split `FlowBusy`/`NoCapacity` totals, or
per-name wait attribution. The later rebuilt observer that adds those fields
has SHA-256
`0030996e24cfc0b8fe6d964f079bf9eac0274ca9f6b401265c7c3576c6060edc` and
remains unbooted. The captured probe was read-only and issued no PDR restart,
firmware transfer, DMA, or radio start.

### QCACLD full resource-config candidate builder (2026-10-23)

I added `sunfish_qcacld_resource.rs` as a host-only serializer for a candidate
68-word WMI resource-config TLV. It requires a parsed service-ready event for
the captured Sunfish firmware build, an empty dynamic-memory request list, and
the pinned QCACLD ABI. It takes target peer cap, effective host counts, and
runtime/build options explicitly; only the source-visible Lineage defaults
are offered as a convenience candidate. Firmware capability bits 99, 176,
and 182 gate PDEV-ID selection, new HTT message framing, and the host-plus-FW
peer-unmap response flag. The builder rejects the distinct high-latency layout
and inconsistent TWT inputs. The validator now checks those fields, the pinned
defaults, WMI flag serialization, and the source's zeroed WMI buffer.

The Rust host test composes a synthetic compatible service-ready event with
the candidate builder, QCACLD WMI_INIT encoder, and HTC command framer; it
round-trips the frame through the HTC parser and checks TLV framing, counts,
and capability gates (23 resource/parser tests pass). The existing WMI-CE suite
now also feeds this candidate frame through CE3 descriptor preflight; all 50
tests pass, and the ring cursor remains unpublished. The source validator
passes against pinned Lineage commit
`28f9290ae067d3e7857126eb6312720dbe9c7c80`. This remains a candidate: the
latest `IMG_8882.heic` contains only SERVREG/LOC observations, not the firmware
service-ready bitmap or Linux WMI INIT capture needed to confirm effective
target revision, runtime options, and the transmitted 68 words. The builder
has no transport, DMA, MMIO, or radio-start path and has not been used on the
Pixel. It is not called from the production modem owner yet. I added the
resource module and its host suite to `platforms/sunfish/test-modem-qmi.py`;
the full offline suite passes, including its AArch64 `no_std` core and native
code-generation checks.

### Refreshed SERVREG credit probe build (2026-10-23)

I rebuilt the per-name SERVREG probe after the current native ELF no longer
matched its loader-task manifest pin. I updated only
`loader-task-regions-modem-servreg-state.json` to the generated native ELF
SHA-256 `e768149c8fa1d8bac8eccf72885fde558982c6df471f88465fd641c7987af6b3`.
The repeat build and packed-image layout validator pass. The refreshed
temporary Fastboot image is
`.cache/pixel-sunfish/modem-servreg-state-probe/fastboot.img` (SHA-256
`0030996e24cfc0b8fe6d964f079bf9eac0274ca9f6b401265c7c3576c6060edc`); it
retains the one-query SERVREG-only scope and remains unbooted. The build's
host tests and CapDL audit pass: 18,187 pages, 24 fixed reservations, 512
unmapped WLAN MSA pages, and no overlaps. The layout check confirms 11
disjoint copy targets and preservation of the original boot DTB. This
revision is prepared to resolve the aggregate LOC reply-wait ambiguity in
`IMG_8882.heic`; it does not attempt a WLAN firmware launch.

### Refreshed IPA/GSI read-only snapshot build (2026-10-23)

I rebuilt the separate IPA/GSI hardware snapshot probe against the pinned
Sunfish Lineage kernel and generated board DTB. The source validator confirms
the IPA v4.2 register offsets, shared-SRAM fields, GSI v2.2 register and IPA
firmware-start source order. The protection domain receives a single
read-only, uncached `[0x01e00000, 0x01e34000)` IPA/GSI mapping and reads the
same 11 source-pinned registers twice; it performs no register writes, IRQ,
SMC, PAS, GSI command, DMA, firmware transfer, or WLAN start. The build and
isolation test pass; CapDL reports 98 pages, 24 fixed reservations, and zero
overlaps. The temporary image
`.cache/pixel-sunfish/ipa-read-probe/fastboot.img` was built with SHA-256
`e1e5b075f28792aa1989a377985a13be968769c0f9cb53c2d313de0e7aaeff1b`; its
native ELF SHA-256 is
`e5011c8b957fa30c26faf84f153b0abdac3ea1c6e85fb64ad18d68bc816daf82`. It has
not been booted, so the actual IPA version, SRAM geometry, and GSI enabled
state remain unobserved in seL4. After the source power audit below, the
bootable image was moved to
`.cache/pixel-sunfish/ipa-read-probe/fastboot.img.unpowered-quarantined`; do
not boot it.

### IPA register-read power prerequisite correction (2026-10-23)

The pinned Sunfish Linux driver sets `curr_ipa_clk_rate` to IPA v4.2 SVS2
(50 MHz) and calls `ipa3_enable_clks()` before it maps the register aperture
and before `ipa3_init_hw()` reads `IPA_VERSION`. On this DTS,
`qcom,bandwidth-vote-for-ipa` is set; `ipa3_enable_clks()` first submits the
`msm_bus` vote and then enables the IPA clocks. Therefore the candidate's
read-only Microkit mapping alone is not evidence that IPA MMIO is powered or
safe to access. The earlier temporary snapshot attempts ended with a
LineageOS watchdog boot reason and no readable checkpoint photo; the evidence
does not prove that an IPA read caused those resets, so the candidate must not
be retried without the power stage.

I added `sunfish_ipa_bus_vote.rs`, which records the source-matched four-path
Sunfish IPA bus table, the five MIN/SVS2/SVS/NOMINAL/TURBO cases, endpoint IDs,
and the Linux clock-rate-to-usecase mapping. Five host tests pass, and
`validate-ipa-read-probe-source.py` now checks those values and confirms that
Linux votes before enabling clocks. This is a vote description only; there is
still no seL4 bus-vote/clock provider or lease proving the aperture is powered.
The IPA snapshot build script now fails closed before packaging a bootable
image. The read-only candidate remains unbootable until that provider is
implemented and source-validated.

The pinned `msm_bus` implementation shows why the missing provider cannot be
replaced by submitting just the four IPA vectors. `msm_bus_scale_client_update_request()`
updates the client's four routed paths; the arbiter updates shared route-node
and BCM aggregates, then the fabric batches active, wake, and sleep BCM
commands through RPMh. `qcom_bcm_vote.rs` models parts of that calculation but
explicitly requires complete client contributions and does not own or submit
the transaction. The existing RPMh vote broker handles VRM regulator votes,
not this bus fabric. The next offline implementation boundary is therefore a
source-pinned route and shared-BCM ledger with an explicit owner/handoff
contract; until those inputs and the clock provider are complete, there is no
safe IPA power lease and no reason to boot the quarantined register probe.

The route validator now checks the four IPA vector paths against both the
captured live Lineage DTB and the current generated runtime DTB. It mirrors the
pinned arbiter's ordered breadth-first route selection, including its
accumulating blacklist, and checks per-node width, aggregation ports, and BCM
bindings. The IPA paths touch eleven distinct BCMs: IPA-to-EBI includes SN11,
SN0, SH0, MC0, and ACV; IPA-to-OCIMEM includes SN11 and SN1; APPS-to-IPA_CFG
includes SH5, SH2, SN15, and CN0; and IPA_CORE-to-IPA_CORE includes IP0. This
establishes the selected paths, not client aggregation or RPMh submission. The
full arbiter and clock owner remain required before an IPA readiness permit
can be issued.

### IPA-route BCM read diagnostic (2026-10-23)

I added a separate diagnostic for the eleven CMD_DB BCM resources on the four
validated IPA routes. It copies and verifies the existing stable two-page
CMD_DB snapshot, resolves only the route's fixed name set, checks the BCM
metadata and RPMh slave-5/alignment, then submits two resource READ passes
through the existing Apps RSC TCS0 controller. Every request is bracketed by
the same full stable handoff capture; a changed bank stops the series before
publishing that response. The route/name set is checked against the generated
board DT before packaging.

This probe reads current opaque resource responses; it does not calculate or
submit a vote, and a successful read is not evidence that IPA clocks are on or
that WLAN is ready. It does write the Apps RSC command/interrupt registers to
issue READ transactions, while keeping the BCM resource WRITE bit clear. The
CapDL manifest permits only the two uncached read-only CMD_DB snapshot pages,
the observed read-only dictionary page, the Apps RSC command page (needed for
the established READ protocol), read-only sleep/wake pages, and the display.
No IPA register window, modem memory, secure call, DMA, firmware transfer, or
WLAN/PDR start is present.

Host canary tests pass (25 Rust tests), the probe builds and passes the
reserved-memory CapDL audit with zero overlaps. The temporary image is
.cache/pixel-sunfish/ipa-path-bcm-probe/fastboot.img (SHA-256
9baeb0d006885c47f75d00325ee48c1d6b76b02a5d46caf5b2d8410108204bf3). On
2026-10-23 Fastboot identified the phone as `sunfish` and the temporary boot
completed successfully; no partition was flashed. Photo `IMG_8884.heic`
shows `READS PASS`, 22/22 reads submitted and completed, 44 handoff checks,
`ALL BANKS PRESERVED`, and `NO WRITE OR WLAN START`.

Both passes returned identical raw values. IPA-to-EBI: SN11 `0x213c44f1`,
SN0 `0x213c44f1`, SH0 `0x24005adf`, MC0 `0x29917f75`, ACV `0x20000008`.
IPA-to-OCIMEM: SN11 `0x213c44f1`, SN1 `0x00000000`. APPS-to-IPA_CFG:
SH5 `0x20000070`, SH2 `0x200000c0`, SN15 `0x200002f3`, CN0 `0x20003416`.
IPA_CORE-to-IPA_CORE: IP0 `0x00000000`. The new SH0 reply differs from the
2026-10-17 read (`0x240d5adf`): under the matched BCM packet decoder its X
field is 4097 rather than 4149, while Y remains 6879. This establishes a
changed resource response only; it does not identify the client or state
change behind it, prove complete vote ownership, or show the IPA is powered.
At this point in the evidence timeline, the follow-on preflight scripts still
asserted the earlier SH0 tuple; the later six-resource repeat below records the
newer tuple and updates only the software-side exact-value gates.

### IPA bus-vote context semantics (2026-10-23)

The pinned Linux source confirms that Sunfish's IPA client does not set
qcom,msm-bus,active-only. msm_bus therefore defaults it to dual-context mode,
converts the DTS vectors from KB/s to B/s, and places each client request in
the dual context while leaving the active-only contribution zero. Along each
selected route, the arbiter sums average bandwidth and takes the maximum
instantaneous bandwidth per node, scales those node results into BCM units,
then prepares active, wake-only, and sleep BCM batches.

sunfish_ipa_bus_vote.rs now exposes that source-derived per-path context input
alongside the five clock usecases. Its initial SVS2 byte/s inputs are
(80,000,000, 465,000,000), (80,000,000, 68,570,000),
(80,000,000, 30,000), and (0, 30,000), in average/instantaneous order.
The source validator pins the dual-context default, conversion, node
aggregation, and submission order, and the host suite checks the Rust
conversion.

The profile now expands those four requests into route-node contributions,
checking each supplied route endpoint pair against the selected IPA path and
rejecting duplicate routes/nodes or insufficient fixed capacity. The caller
must still supply the route nodes selected from the validated DT topology. The
`qcom_msm_bus_aggregate::MsmBusLedger` stores one generation-bound report per
configured client, replaces a client's current request atomically, withholds
totals until the entire configured set reports, and faults on stale or unknown
clients and malformed route reports. A composition test combines the IPA
usecase with another client on a shared node and checks the sum-average/max-peak
result. The four IPA route-node sequences match the current live-derived DTB
and the pinned arbiter; only the second client's vote is synthetic.

This closes the software path from an IPA clock usecase through route-node
aggregation, but it does not populate an authoritative seL4 client registry or
a complete route table for all configured clients, own RPMh handoff state, or
submit power votes. The Linux inventory is not the native owner's expected
client set. No IPA clock lease or MMIO probe is authorized by this model.

### SERVREG LOC reply checkpoint (2026-10-23)

The latest photographed one-query screen reports QMI result/error `0/0`,
`SERVICE UNINIT`, two LOC requests, one response queued, first response name
code `5` (`tms/pdr_enabled`), `FIRST RXDONE=1`, and `LOC REPLY WAITS=3`. The
observed LOC names include `TMS PDR ENABLED` and `TMS PDDUMP DISABLED`; the
WLAN firmware LOC count and domain count are both zero. The owner reports
`NO OWNER ERROR`, while WLFW remains disabled and its error result is
unavailable (`FFFFFFFF`).

This confirms that the first LOC reply reached GLINK RX_DONE; it does not
confirm QRTR `RESUME_TX` for that reply. With two requests but one queued
response, the three waits are consistent with the second reply encountering
transmit backpressure, but this screen does not split flow-busy from
no-capacity waits. The already-built follow-up one-query state probe displays
those wait causes by LOC name, the aggregate flow-busy/no-capacity counts,
and whether QRTR RESUME_TX arrived. The follow-up build and temporary boot are
recorded below; its screen result is still pending. This checkpoint itself
shows no WLAN service initialization or WLAN firmware discovery.

### Closed-set MSM bus-node aggregation model (2026-10-23)

`rust/src/qcom_msm_bus_aggregate.rs` now models the pinned
`aggregate_bus_req()` step over an immutable client/route snapshot. It requires
every expected client, generation-matched reports, and one explicit value for
every node on every declared client route, including zero-valued requests. It
then sums average bandwidth and takes maximum instantaneous bandwidth per
node and context with checked arithmetic. Its eight focused model tests pass,
and the source validator checks the model against the pinned
Linux aggregate loop. The same module now scales each complete node/context
through the pinned `bcm_update_bus_req()` width, bus-width, aggregation-port,
and CMD_DB unit-size formula, then applies the source-ordered shared BCM
accumulator and ACV mask behavior.

This closes the pure per-node aggregation, BCM-link scaling, and shared numeric
carry model. The authoritative live client universe and route reports are not
yet supplied in seL4; complete RPMh handoff ownership and transaction
submission are still required before the IPA can receive a power lease. The
44-test combined BCM/command-DB/RPMh/model harness is now part of
`test-modem-qmi.py`, which compiles the model in the AArch64 `no_std` core; the
complete offline script passes, including its host and native-codegen checks.
The source validator checks the model against the pinned Linux aggregation,
scaling, and binding-order loops.

### DT-declared MSM bus-client inventory (2026-10-23)

`platforms/sunfish/audit-msm-bus-client-inventory.py` now audits all
device-tree-declared bus clients and their selected routes using the pinned
arbiter's ordered route search. For the captured Lineage DTB (SHA-256
`e371f54727a89c9c76abdc5fe4c23d93f6e24925770d4519fe705cfa7f35bd35`), it
finds 40 client nodes and 187 case/path vectors. The selected routes resolve
for 185 vectors. The two unresolved vectors are the MIN/MAX cases for
`ufs_ice_noc` (`src=1`, `dst=650`, `MSM_BUS_SLAVE_UFS_CFG`); the DT has no
unique bus node for destination 650. The saved Lineage boot log shows the UFS
host's `ufshc_mem` client registering as handle 25 and the ICE device
initializing, but contains no `ufs_ice_noc` registration or route-failure
line. The live device reports kernel `4.14.357-openela-g28f9290ae067`, matching
the pinned source commit prefix. Its `/proc/config.gz` has
`CONFIG_QCOM_BUS_SCALING=y` but no `CONFIG_MSM_BUS_SCALING`. In that exact
source, `drivers/crypto/msm/ice.c` compiles `qcom_ice_bus_register()` only
under `CONFIG_MSM_BUS_SCALING`; the other branch returns success without
registering a bus client. This explains why the unresolved ICE DT vector is
not a runtime registration in this Lineage build. It can be excluded from
the baseline live-client set, but the DT route itself remains unresolved.

The inventory tool also maps registration paths from the archived Lineage
boot log. It finds 49 registered handles, 44 debug names, and 57 logged
source/destination paths; all 57 resolve to the pinned bus graph. Thirteen
logged names are dynamic entries absent from the DT client-name set, while
`sdcc_ice_noc`, `sdhc1`, and `sdhc2` have no matching registration lines in
that capture. (The fourth missing name, `ufs_ice_noc`, is disabled by the
build guard above.) The saved logs cover registration routes, not selected
usecases, request values, or current BCM votes.

The inventory confirms significant sharing on IPA's memory-facing BCMs:
MC0, ACV, and SH0 also appear on storage, crypto, display, camera, modem-PIL,
bus-proxy, and USB routes; SN11 and SN0 are shared with crypto and other
clients; CN0, SH2, SH5, and SN15 have multiple peripheral clients. SN1 and IP0
appear only on IPA routes in this DT inventory. This is route membership,
not proof that any listed client currently contributes bandwidth or what
usecase it selected. The 57 registered paths now provide a route map for the
captured boot, including its dynamic clients. Current request values and
live BCM votes remain unresolved, so the inventory is not a closed live vote
snapshot and cannot authorize power-up.

After returning to LineageOS on 2026-10-23, the authorized ADB shell could list
the platform's `/sys/class/devfreq` clients, including the CPU/LLCC/DDR and GPU
bandwidth devices, but reads of `cur_freq`, `target_freq`, governor, and
available-frequency attributes were denied to UID 2000. `su` is unavailable,
and `/sys/kernel/debug` (including `/d/msm_bus` and debugfs clock/regulator
views) is absent. The RPMh master-stats device is present in sysfs, but its
attributes are likewise inaccessible. Thus this capture confirms the runtime
interfaces/devices exist but does not provide selected usecases, requested
bandwidth, or live votes. A later values capture needs an already-authorized
privileged LineageOS interface; it must remain read-only and must not enable a
devfreq target or issue any bus/RPMh request.

The pinned `drivers/devfreq/devfreq_devbw.c` clarifies what that capture would
need: the devfreq target is instantaneous bandwidth in MB/s, while the average
bandwidth is held separately in the governor's private `gov_ab` value. Each
update writes those values into a double-buffered bus request before switching
the client's buffer index. Therefore a readable `target_freq` alone would not
reconstruct the complete vector; the governor average and the other registered
clients' contributions are also required. No devfreq target was changed during
this inspection.

### Six-resource BCM read repeat (2026-10-23)

The six-resource read-only image was temporarily booted after Fastboot
identified the device as `sunfish`; no partition was flashed. Photo
`IMG_8885.heic` shows `BCM READ PASS`, 12/12 replies completed, 24 handoff
checks, `BANKS PRESERVED`, and the BCM `RESOURCE WRITE BIT` held at zero. The
Apps RSC controller registers were written to issue these read transactions;
no bandwidth vote or modem start occurred.

Both passes matched for MC0 `0x29917f75`, ACV `0x20000008`, CE0 `0x00000000`,
SN11 `0x213c44f1`, SN0 `0x213c44f1`, and SH0 `0x24005adf`. The six-resource
sample confirms CE0's zero response and repeats the newer SH0 value already
seen in the IPA-route read. It differs from the 2026-10-17 six-resource SH0
response `0x240d5adf`; this does not identify which client or event explains
the change, establish a complete vote inventory, or imply WLAN readiness.

The current exact-value preflight and its offline builders now use the latest
repeated tuple, while the earlier photo record remains intact as historical
evidence. This updates only software-side comparison gates; no resource WRITE
was run or authorized by this read-only check. The user subsequently returned
the Pixel to LineageOS, where it remained during offline work until the
SERVREG follow-up below.

### SERVREG reply-backpressure follow-up build and temporary boot (2026-10-23)

The bounded follow-up state probe rebuild initially stopped at its ELF hash
check because the current native PD no longer matched the older manifest pin.
The current ELF was re-pinned only after the dedicated image/layout tests and
the generated source-manifest checks passed. The complete
`build-modem-servreg-state-probe.sh` then passed; CapDL reports 18,187 pages,
512 WLAN-MSA reservation pages unmapped, and zero overlaps. The focused probe
and loader-source suites pass 18 tests. The temporary Fastboot image is 41,320,448
bytes with SHA-256
`591cbdc9953b5d656e60676d5ae6996e01cf7f2267d1a932c277ecd081d1f79b`; its
native PD ELF SHA-256 is
`c16712201a2101199cb2740a24d1b20f29bd4268046c1b6decab55d098ddbf6f`.

Fastboot identified `sunfish` and accepted the image with `fastboot boot`; no
partition was flashed. The probe performs one transaction-matched SERVREG
state query and reports LOC wait counts by service name, FlowBusy versus
NoCapacity totals, and whether the first reply received QRTR `RESUME_TX`. It
does not register a listener, send `RESTART_PD`, start WLFW, grant WLAN MSA,
configure WLAN DMA, or transfer WLAN firmware. The image uses the established
modem PAS path as a prerequisite. Photo `IMG_8886.heic` instead shows
`MODEM IMAGE STAGE / PREPARATION FAILED`, error `0x00030001`, AOP mask
`0x0000001f`, resource ACK mask `0`, and request count `12`. It also explicitly
reports `NO SECURE CALL / AUTH NOT CALLED YET`. By the preparation wrapper,
`0x30000` identifies the strict RPMh resource-hold phase and low code `1` is
the outer MC0-stage failure; it is not a modem/QMI response. Twelve requests
are consistent with the two six-resource read passes preceding any accepted
resource hold. Since the display does not include the inner MC0/controller or
guard subreason, the SERVREG query and modem readiness remain untested. No
secure authentication, modem start, WLAN request, or partition flash occurred.

### Detailed modem-preparation failure probe (2026-10-23)

After the SERVREG prerequisite stopped at the MC0 stage, the existing
standalone `modem-prepare-probe` was rebuilt to expose its inner controller,
guard, and resource-value diagnostics. Its offline validation passed: 3
metadata/preparation tests, 25 AOP MMIO tests, 25 BCM MMIO tests, 14 metadata
MMIO tests, plus the AArch64 release build and CapDL audit (85 pages, zero
overlaps). Fastboot again identified `sunfish` and accepted the temporary
image `.cache/pixel-sunfish/modem-prepare-probe/fastboot.img` with SHA-256
`ea004a3c4c559b7e835300afa463c77ddb1ee6bcef67ab3477f6d3a1de2fb12f`.
This probe performs the same guarded MDT staging, fixed AOP ON notification,
and strict resource-hold path, then checks metadata; it stops at first fault
and does not call secure authentication, release modem reset, transfer image
segments, start WLAN, or flash a partition. Photo `IMG_8887.heic` shows
`PREPARE FAIL`, error `0x00030001`, AOP phase mask `0x1f`, AOP notifications
`4`, RSC phase `0`, write ACK mask `0`, 12 requests, 24 handoff checks,
controller error `0`, and guard/MC0 error `2`. In
`qcom_rpmh_bus_hold::Error`, code 2 is `StartingValues`: the completed two-pass
six-resource read did not match the full fixed expected tuple. The displayed
pass-0 tuple is MC0 `0x29917f75`, ACV `0x20000008`, CE0 `0`, SN11/SN0
`0x213c44f1`, and SH0 `0x24005adf`, all matching the expected first-pass
values. The screen does not display pass 1, so it does not identify which
resource differed across the complete comparison. `CTRL ERROR 0` and 24
handoff checks show that no RSC transaction or bank-preservation error was
reported; the failure is the value gate after the reads. The gate stopped the
sequence before the MC0 or downstream resource writes, consistent with ACK
mask zero. MDT metadata was staged (`0x238c` bytes at physical `0x80001000`)
but not verified, and no secure call, segment transfer, reset release, WLAN
start, or partition flash occurred. The user returned the Pixel to LineageOS.

The next discriminating capture should print both raw BCM passes after the AOP
ON step, then stop without any resource WRITE. That will identify whether and
which value changes between pass 0 and pass 1 before the preparation path is
attempted again. If the seven-resource floor preconditions pass in a later
test, those fixed checked holds would remain committed until physical restart;
the current failure did not reach them.

The separate `modem-bcm-after-aop-probe` now implements that capture. It stages
the same MDT, completes the fixed AOP ON notification, performs exactly two
six-resource BCM READ passes through the existing bank-preserving controller,
and displays both raw tuples plus pair-difference and expected-value masks. It
does not call the resource-hold code or issue a BCM resource WRITE. The builder
passed 5 Python tests, 25 AOP MMIO tests, 25 BCM MMIO tests, 14 metadata MMIO
tests, the AArch64 release build, the MDT layout audit, and CapDL (81 pages,
zero overlaps). The temporary image is
`.cache/pixel-sunfish/modem-bcm-after-aop-probe/fastboot.img`, 18,673,664 bytes,
SHA-256 `93ee69b584d6b382dafc31b04c0ee98ab36a26f50cbf9480ee8cd1b0ac111e0a`.
It had not yet been booted at that checkpoint; the subsequent temporary run is
recorded below.

### Post-diagnostic LineageOS return capture (2026-10-23)

After the user reported LineageOS ready, ADB confirmed one attached `sunfish`
running LineageOS `23.2-20261001-NIGHTLY-sunfish` as shell UID 2000. The
existing `capture-lineage-ipa-state.sh` was run unchanged; it reads only the
existing debugfs/log interfaces and makes no setting or service changes. Its
filtered output is saved at
`.cache/pixel-sunfish/lineage-ipa-state-20261023.txt` (SHA-256
`a4e238cacc022d0f3f7c1e58933247ebc4baf12efe52a8b79dadb185e63b760c`).

The boot log shows `vendor.ipacm`, QRTR name service, TFTP, `pd_mapper`, remote
storage, and service-locator startup before modem image loading and reset
release. Modem QMI readiness follows. The IPA modem-driver request has a
response marker 35 ms later, but neither payload nor result is exposed. The
service notifier receives and ACKs `msm/modem/wlan_pd` state `0x1fffffff`;
ICNSS connects at state `0x980`, followed by WLAN FW ready `0xd87` and the WMA
firmware-ready event. This reconfirms the stock LineageOS path after the
preparation failure, but supplies neither IPA register geometry nor the
private initial `wlan_pd` launch trigger. Both IPA debugfs files remain
inaccessible to the ordinary shell, and no permission change was made. At this
capture checkpoint, the paired BCM image remained prepared but unbooted and
the phone was in LineageOS; its later Fastboot run is recorded below.

### Live LineageOS Wi-Fi reference and BCM follow-up boot (2026-10-23)

Before the temporary boot, a read-only ADB `dumpsys wifi` capture on the
connected Sunfish (LineageOS `23.2-20261001-NIGHTLY-sunfish`, shell UID 2000)
showed Wi-Fi enabled on `wlan0`. The framework history includes supplicant
completion, IPv4 provisioning success, and the L3-connected state. Current
framework metrics at capture were RSSI `-50 dBm`, frequency `5200 MHz`, and
link speed `650 Mbps`. SSID, BSSID, IP, and DNS values were not retained.
Direct `ip link` / `operstate` reads were denied to the ordinary shell; no
permissions or device settings were changed. This confirms a working
LineageOS reference path, not native seL4 WLAN startup. The redacted record is
`.cache/pixel-sunfish/lineage-wifi-reference-20261023.txt` (SHA-256
`df92318cce9df35932b7316046bb41436a2e1d9874a4fe50e5ad25eb2df291cf`).

The current `modem-bcm-after-aop-probe` image was rebuilt from the checked-in
source immediately before use. Its six focused Python checks, 25 AOP MMIO
tests, 25 BCM MMIO tests, 14 metadata MMIO tests, AArch64 release build,
metadata layout audit, and CapDL audit all passed (81 pages, zero overlaps).
For the screen, bits 0 through 5 of both difference masks follow resource
order MC0, ACV, CE0, SN11, SN0, SH0. `PAIR DIFF MASK` marks values that changed
between passes; `EXPECTED DIFF` marks values where either pass differs from
the recorded six-resource tuple.
Fastboot reported product `sunfish` and accepted the temporary `fastboot boot`
of the image above. No partition was flashed. This run sends the fixed AOP ON
notification and performs two six-resource BCM READ passes. It does not issue
a BCM resource WRITE, secure call, modem reset release, modem/WLAN start, or
DMA. At this checkpoint, the probe had booted but the settled-screen capture
was pending, so the two-pass result was not yet known.

### Reported LineageOS return after the post-AOP BCM probe (2026-10-23)

The user reports that LineageOS is ready. At the follow-up check, both
`adb devices -l` and `fastboot devices -l` returned no attached device, so the
Android boot and USB connection could not be independently confirmed. The
settled photo from the temporary post-AOP BCM probe has not been received;
its paired-read result remains unknown. No additional device command was run.
Once USB is visible again, confirm the reported LineageOS session over ADB,
then use the already-built, read-only Fastboot diagnostic once to capture both
BCM passes if the prior screen photo is unavailable. Return the phone to
LineageOS afterward; do not infer pass-1 values from the earlier single-pass
preparation photo.

The offline integration check `python3 platforms/sunfish/test-modem-qmi.py`
completed successfully on this worktree. It covers the modeled QMI/QRTR,
SERVREG, WLFW, IPA, WLAN resource, and ownership paths and reports successful
AArch64 `no_std` core/native compilation. This validates those software
contracts only; it does not provide the missing live BCM pass-1 tuple or make
the native startup providers hardware-ready.

A focused regression now changes SH0 after the first six reads and before the
second pass. It confirms that the first tuple can match while pass 1 differs,
and that `StartingValues` stops the sequence after 12 READs with no MC0 WRITE.
All eight `qcom_rpmh_bus_hold` unit tests pass, as does a fresh run of the full
`test-modem-qmi.py` integration suite and AArch64 core/native compilation. The
probe’s device result remains pending; this regression validates the fail-closed
software response to that possible drift, not the phone’s actual readings.

### Paired BCM probe rebuild after reported LineageOS return (2026-10-23)

The user reports that the Pixel is back in LineageOS, but fresh
`adb devices -l` and `fastboot devices -l` checks are empty; the USB inventory
command produced no listing. Rebuilding `build-modem-bcm-after-aop-probe.sh`
from the current worktree passed six focused builder tests, 25 AOP MMIO tests,
25 BCM MMIO tests, 14 metadata MMIO tests, the AArch64 release build, and the
CapDL audit (81 pages, zero overlaps). The temporary image remains 18,673,664
bytes with SHA-256
`93ee69b584d6b382dafc31b04c0ee98ab36a26f50cbf9480ee8cd1b0ac111e0a`.

The image performs the fixed AOP ON notification and two six-resource BCM READ
passes, then displays both tuples and difference masks. It issues no BCM
resource WRITE, secure call, modem reset release, firmware transfer, WLAN start,
or DMA. The phone-side paired-read result remains unknown until the device
enumerates in Fastboot and the temporary image is run; no partition is flashed.

### Sunfish board-2 selection parser (2026-10-23)

Offline work added a bounded, allocation-free ath10k API-2 `board-2.bin`
selector. It constructs the source-ordered Sunfish SNOC QMI board-name
candidates from runtime board/chip IDs and the `google_sunfish` variant, then
returns only a borrowed payload after an exact name/data IE match. Extended
board data stays on the separate BMI eboard-identity path. Twelve synthetic
host tests and the pinned Linux source audit pass; the complete
`test-modem-qmi.py` suite also passes, including AArch64 `no_std` compilation.
This closes the parser gap only: it neither verifies the contents or license
of a candidate board file nor advances the outstanding paired-read hardware
check or WLAN firmware startup.

### Post-AOP paired BCM result and LineageOS return (2026-10-23)

The user-provided settled-screen photo
`670CF812-4D5E-40A2-8DC1-6C3CA81A4824_4_5005_c.jpeg` records the paired
`modem-bcm-after-aop-probe` result. Both six-resource passes completed after
the fixed AOP ON notification: read error `0`, AOP phase mask `0x1f`, four
notifications, 12 reads, and 24 handoff checks. Every resource matched between
passes: MC0 `0x29917f75`, ACV `0x20000008`, CE0 `0`, SN11/SN0 `0x213c44f1`,
and SH0 `0x240d5adf`. Pair difference mask `0` confirms the two samples were
identical. Expected-value difference mask `0x20` identifies only resource
index 5 (SH0): this image's comparison tuple has SH0 `0x24005adf`, the value
repeated in the earlier 2026-10-23 IPA-route and six-resource captures. The
observed SH0 value `0x240d5adf` matches the 2026-10-17 capture. This establishes
a repeatable SH0 value for the two reads in this boot, but does not identify
what changed it or attribute it to AOP, because this probe did not take a
pre-AOP sample. MDT metadata verified; the image reports no BCM resource
WRITE, secure call, or modem/WLAN start.

After the probe, ADB again identified the attached device as Sunfish running
LineageOS `23.2-20261001-NIGHTLY-sunfish`. The unchanged, read-only
`capture-lineage-ipa-state.sh` was rerun and saved at
`.cache/pixel-sunfish/lineage-ipa-state-20261023-post-pair.txt` (SHA-256
`d1c46998f9fc8c89ed84559189ec326da47bee878d5b917fbd24aecf7ad5fd20`). It again
shows the IPA modem-driver QMI response and the normal modem/WLAN service
readiness sequence; the ordinary shell still cannot read the two IPA debugfs
register files. The next discriminating hardware check is a same-boot,
read-only pair of six-resource snapshots immediately before and after the
fixed AOP ON notification. That can test whether the SH0 transition tracks
that notification without issuing a BCM resource WRITE or starting WLAN.

### Same-boot AOP comparison diagnostic built (2026-10-23)

To resolve that ambiguity, `modem-bcm-aop-delta-probe` now takes two complete
six-resource BCM READ passes before the fixed AOP ON notification and two more
after it in the same boot. It skips the AOP notification if the pre-read does
not pass. The display gives each resource's stable before/after values, each
stage's pair-difference mask, the transition mask, and read/controller/handoff
errors. The temporary image is
`.cache/pixel-sunfish/modem-bcm-aop-delta-probe/fastboot.img` (18,677,760
bytes; SHA-256
`591de328decf5b11fcb62002d48dffe78b4621863da7ac42081068f063cb78b7`). Its
builder passed ten focused Python checks, 25 AOP MMIO tests, 25 BCM MMIO tests,
14 metadata MMIO tests, the AArch64 release build, metadata layout audit, and
CapDL audit (82 pages, zero overlaps). The probe uses the same narrowly mapped
RSC, AOP mailbox, MDT metadata, and display regions; it does not map the modem
pool or WLAN MSA and does not issue a BCM resource WRITE, secure call, modem
start, firmware transfer, WLAN start, or DMA. The image is intended for a
temporary Fastboot boot only; no partition is flashed.

The temporary boot completed successfully. Photo `IMG_8891.heic` shows both
six-resource read series passed: 12 reads before and 12 after, both pair
difference masks `0`, AOP phase mask `0x1f`, four notifications, and AOP
transition mask `0`. Each resource was unchanged across AOP ON: MC0
`0x29917f75`, ACV `0x20000008`, CE0 `0`, SN11/SN0 `0x213c44f1`, and SH0
`0x240d5a0f`. Read, controller, and handoff errors were all `0`; MDT metadata
verified. This run shows no BCM response change across the fixed AOP ON
notification. In particular, SH0 was `0x240d5a0f` both before and after;
the earlier post-AOP-only observation of `0x240d5adf` did not recur, and this
comparison does not explain that between-boot difference. The display confirms
no BCM resource WRITE, secure call, or modem/WLAN start. The Pixel should now
return to LineageOS; this probe did not flash a partition.

### Fresh LineageOS WLAN readiness baseline (2026-10-23)

After the probe, ADB again identified the phone as Sunfish running LineageOS
`23.2-20261001-NIGHTLY-sunfish`. The unchanged, read-only
[`capture-lineage-ipa-state.sh`](../platforms/sunfish/capture-lineage-ipa-state.sh)
capture is saved at
`.cache/pixel-sunfish/lineage-ipa-state-20261023.txt` (SHA-256
`bbf21f516a3f63891459399d6674f67d0db793820380d9a1abdcc8295c9c25f9`). In the
captured boot-log order, `vendor.ipacm`, QRTR namespace, TFTP, PD mapper,
service locator, remote storage, and vendor TFTP services start before the
modem is brought out of reset. The modem reaches Power/Clock ready and QRTR
QMI readiness; remote storage then services reads from `/boot/modem_fs1`,
`modem_fs2`, `modem_fsg`, and `modem_fsc`. The IPA
`QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` receives a response. Next, the service
notifier reports `msm/modem/wlan_pd` state `0x1fffffff`, ICNSS connects to its
QMI server (`state: 0x980`) and ACKs the indication, and the log records
`WLAN FW is ready: 0xd87` followed by the WMA firmware-ready event.

This is direct evidence that the reference LineageOS boot reaches WLFW and
WMA firmware readiness. It narrows the startup comparison to the modem/IPA
prerequisites and the `wlan_pd` service-arrival path; it does not identify the
private initial `wlan_pd` launch trigger or prove that the native seL4 owner
has those providers. The ordinary shell could not read IPA debugfs geometry,
`wlan0` link state, or its `operstate`; association status is therefore
unknown. The filtered capture also contains no TFTP RRQ evidence, and does not
show the IPA QMI response payload. No Wi-Fi setting, service, or device state
was changed by the capture.

### BCM TCS phase and batching model (2026-10-23)

The user confirmed the Pixel is back in LineageOS after the same-boot AOP
comparison; no further device operation was performed.

Offline work now adds `qcom_bcm_vote::plan_tcs`, a pure model of the pinned
`msm_bus_fabric_rpmh.c` active, wake-only, and sleep command generation. It
preserves caller-supplied BCM insertion order within each clock domain, omits
all-zero resources only during kernel initialization, emits wake/sleep entries
only when the two contexts differ, and suppresses the sleep commit bit only for
`RPMH_AWAKE_STATE`. It groups adjacent VCDs into at-most-16-command batches and
rejects a single VCD larger than the RPMh payload, which the kernel shrink
helper cannot split. The planner produces diagnostic words only; it neither
discovers the complete live client set nor submits TCS commands or owns
handoff state.

The existing active-TCS planner previously accepted only four-register VRM
batches, although the captured RSC and pinned Linux driver support 16 commands
per TCS. `qcom_rpmh_vrm_tcs::build_bcm_command_plan` now maps a bounded 1-to-16
batch of validated BCM words (slave ID 5, aligned offsets, legal packet bits,
ascending VCD order, and one terminal commit per VCD) into TCS0. The VRM route
remains constrained to its four-register format, and its arbitrary address/data
helper is private to the module. The guarded executor checks and cleans all 16
command slots, and an integration test passes an actual encoded BCM phase word
through the typed controller plan. This catches premature/missing commit flags
that the previous generic builder would have accepted. The plan builder
performs no MMIO; execution still depends on the existing guarded RSC adapter,
a stable all-bank snapshot, and a live lease.

A fresh read-only LineageOS ADB check found no `devfreq` dumpsys service;
`cur_freq`, `target_freq`, and governor reads are denied to shell, and
`/sys/kernel/debug/msm_bus` is absent. This confirms the client-value gap
remains. The focused 108-test RPMh/BCM/TCS harness, 49-test BCM/aggregation
harness, and full Sunfish QMI runner pass; the latter includes AArch64 `no_std`
core/native compilation. A fresh read-only capture also records `wifi_on=1`
and `wlan0` present in LineageOS; the shell cannot read `operstate`, `carrier`,
or interface flags. A follow-up read-only `adb shell cmd wifi status` and
`dumpsys wifi` check confirms the reference device is associated: Wi-Fi is
enabled, the client is connected, and supplicant state is `COMPLETED`. This
resolves the LineageOS association-status gap; it does not establish that the
seL4 WLAN path can associate. Network name, BSSID, and local address are
intentionally omitted. The capture is saved at
`.cache/pixel-sunfish/lineage-ipa-state-current-20261023.txt` (SHA-256
`6647e4701d3429f626fd7eb99cc3dc88d3fd000189c2c9089375e3160a0a4f44`). The
Pixel stayed in LineageOS and no hardware vote was submitted. A complete
source-ordered active request/BCM-vote inventory, verified handoff ledger,
and production RPMh owner/provider are still absent.

### Current-boot Linux MSM-bus registration trace (2026-10-23)

The current kernel records are readable through `adb logcat -b kernel` even
though direct unprivileged `dmesg` access is denied. I saved the MSM-bus lines at
`.cache/pixel-sunfish/msm-bus-kernel-current-20261023.txt` (SHA-256
`a0a0a7a627705003a492235a835df805a336972d5835708ca21c7a63f6ac7d9e`) and
resolved them against the captured live DTB using
`audit-msm-bus-client-inventory.py`. The machine-readable result is
`.cache/pixel-sunfish/msm-bus-client-inventory-current-20261023.json`.

The current boot logged 49 client handles, 44 unique debug names, and 57
source/destination paths; every logged path resolves with zero failed or orphan
path records. Runtime-created client names include some clients absent from
`qcom,msm-bus,name` DT entries, while four DT-declared clients had no
registration line in this boot. The static DT view has 40 declared clients and
187 paths; its two unresolved UFS ICE paths both target absent bus node ID 650.
Neither UFS ICE path appears in the current runtime registration set.

The relevant current handle order and BCM routes are: IPA handle 10 uses
`SN11/SN0/SH0/MC0/ACV`, `SN11/SN1`, `SH5/SH2/SN15/CN0`, and `IP0`; the bus
proxy handle 11 uses `MM1/MM0/SH0/MC0/ACV`; `scm_pas` handle 12 uses
`CE0/SN11/SN0/SH0/MC0/ACV`; and `pil-modem` handle 13 uses `MC0/ACV`. No
registered handle named WLAN appeared. This gives a current Linux route and
registration-order reference, but it does not show usecase changes, runtime
bandwidth requests, or committed BCM vectors. It cannot supply the separate
authoritative seL4 client set or RPMh owner needed for native startup.

The follow-up LineageOS capture confirms why the missing values cannot be
recovered from the existing shell trace: `/proc/config.gz` reports
`CONFIG_DEBUG_FS` unset, while `CONFIG_DYNAMIC_DEBUG=y`. Tracefs exposes the
`msm_bus` event formats, but all bus events are disabled and `tracing_on=0`.
More importantly, the pinned source does not emit the apparent update events
for ordinary runtime requests: `bus_update_request` is called by the debug
client interfaces, while `bus_client_status` and `bus_bcm_client_status` are
called when the debugfs `dump_clients` and `dump_bcm_clients` files are read.
The normal RPMh `update_request_adhoc()` path logs through `MSM_BUS_DBG`; its
`bus_update_request_end` trace call is commented out. With debugfs absent and
the unprivileged shell unable to read `/proc/dynamic_debug/control`, this boot
has no accessible passive snapshot of current usecases or per-client BCM
vectors. The empty trace buffer therefore says nothing about whether clients
hold votes. A future capture needs an already-authorized privileged interface
or a purpose-built reference-kernel diagnostic that observes normal request
updates and commit-list order; it must remain read-only with respect to the
bus and RPMh resources. No tracepoint, dynamic-debug flag, or device setting
was changed during this follow-up.

A later read-only check found that the unprivileged kernel logcat ring had
wrapped: it retained 8,471 lines but only MSM-bus registrations 26–49. I used
the already-established temporary root-ADB procedure to read `dmesg` and then
restored adbd to UID 2000. The filtered full-ring capture is
`.cache/pixel-sunfish/msm-bus-update-trace/root-capture-20261023-131710/msm-bus-update-trace.txt`
(SHA-256 `305aad2aaf7d2d2b89413d37300678e51e5a61d1ae131bb500ce863541000db5`):
it contains all 49 baseline client registrations, handles 1–49, and zero
request updates or devfreq samples. The capture helper now uses this temporary
root read, verifies that ADB returns to the shell UID, and reports an
incomplete registration set rather than silently accepting a wrapped logcat
buffer. This does not change the conclusion that normal LineageOS boot lacks
the dynamic request values needed for the planned comparison.

The regenerated route inventory now also carries source-ordered fabric
topology, including every `qcom,bus-dev` parent target, at
`.cache/pixel-sunfish/msm-bus-client-inventory-current-20261023-fabric.json`
(SHA-256 `2e0d7fef767a2b3de3ca6283f855117e82a8df85e2964262cadad8397bffd108`).
It records 129 route, BCM-linked, and parent bus nodes, all 101 nodes with `qcom,bcms`, their 103
ordered BCM bindings, 34 BCM device nodes, 26 distinct CMD_DB names, and both
RSCs. The 49 registered client routes reach 24 physical BCM nodes with 19
unique names; five names (`ACV`, `MC0`, `MM0`, `MM1`, `SH0`) have separate
Apps and Display RSC devices on the routed paths. Every BCM-linked bus node
has nonzero effective width/aggregation geometry under the pinned driver
defaults. The trace joiner now retains the physical BCM node and RSC identity
per path, so repeated CMD_DB names cannot collapse the two controllers. This
is topology only; the separate six-resource CMD_DB metadata snapshot is recorded
above. Live requests and committed votes were still unmeasured at this capture.
The inventory parser's two tests and trace parser's ten
tests pass.

`qcom_bcm_vote::plan_tcs` is explicitly documented to plan one RSC commit list
at a time and to preserve its runtime order within each VCD. The actual
source-ordered commit list depends on which client updates touch bus nodes;
FDT registration order alone is not enough. The temporary Linux trace remains
the next check needed to reconstruct those updates.

`simulate-msm-bus-commit-order.py` reconstructs each captured update's
bus-node and BCM first-touch lists from the update order, runtime route
signatures, path-vector order, `update_path()` route walk, node-parent touches,
and ordered `qcom,bcms` bindings. It refuses route signatures that differ from
the saved same-kernel registration baseline; it joins registrations by client
name and ordered path signature because numeric handles can be reassigned
between boots. The report intentionally does not
infer votes or actual TCS commands; it also rejects a transaction whose paths
span multiple RSCs. It can now join the separately observed six-record CMD_DB
metadata snapshot in `platforms/sunfish/bcm-metadata-sunfish.json` and expose
the clock domain alongside each touched BCM. When every touched BCM has a
known clock domain, it emits a candidate VCD-grouped BCM touch list in ascending
domain order, preserving first-touch order within each domain. This is a
candidate only: it does not model numeric votes, command filtering, or the
resulting TCS words. Unknown CMD_DB names keep that event's grouping unresolved.
Its ten tests cover route verification, baseline completeness, node/BCM
deduplication, parent ordering, RSC consistency, metadata joins, VCD grouping,
and empty traces. Replaying the refreshed LineageOS baseline produced zero
update events and no mismatches at
`.cache/pixel-sunfish/msm-bus-update-trace/lineage-validation-20261023/commit-order.json`;
this ordinary boot has no update touches; the separate dynamic-debug boot is
recorded below.

To capture those request transitions without changing the running system, I
prepared a temporary boot image from the cached LineageOS fastboot image. Its
only change is the boot argument
`dyndbg="func update_request_adhoc +p; file drivers/devfreq/devfreq_devbw.c +p"`.
The reproducible builder puts a separating space in the v2 `extra_cmdline`
field and validates that the kernel, ramdisk, second stage, recovery DTBO,
DTB, and boot-image ID are byte-for-byte unchanged from the input image. An
earlier local image omitted this separator and was replaced before any device
boot. The first query enables the two
`MSM_BUS_DBG` callsites in `update_request_adhoc()` early in boot, before device
probes. Changed requests report the client handle, selected usecase index,
prior index, and path count. The unchanged-index branch prints an index-only
note without the handle; it leaves state unchanged and can be ignored. The
second query enables the `devfreq_devbw` driver's existing `dev_dbg` line,
which records dynamic AB/IB bandwidth values in MB/s when those requests
change. The source sets each new client's current index to `-1`, so the
boot-order trace can seed client state;
joining logged handles to the captured route inventory, pinned usecase
vectors, and devfreq values can reconstruct static and bandwidth-governor
requests and simulate ordered commit-list touches. Other runtime-generated
usecases or ALC-only clients still need separate value sources, and this trace
alone does not establish a complete request set or authorize a vote.

The image is
`.cache/pixel-sunfish/msm-bus-update-trace/fastboot.img` (38,932,480 bytes;
SHA-256 `15dbfe06dc9d45d9fe520759051ccf09cef4f9af152429e9f2ab37c27d8f8bde`).
The image was reparsed and its payload components and boot ID compared against
the input; the five-test boot-v2 packer suite also passes.
`audit-msm-bus-request-trace.py` is ready to turn the resulting kernel log into
a handle/state report, retain devfreq AB/IB samples, and optionally join
unambiguous request indices to the DT usecase vectors. The join converts
`vectors-KBps` using the pinned Linux `KBTOB` rule and refuses ambiguous
duplicate matches instead of guessing. It also flags prior-index, path-count,
registration, and bus-error inconsistencies. Its ten request-trace tests pass.

Two runtime clients are now covered by exact, source-backed profiles because
their request tables are compiled into drivers rather than declared as DT bus
clients. `scm_pas` is joined only for the captured route `125 -> 512`; the
pinned `subsys-pil-tz.c` table has an off case and a PAS-load case with
`AB=393,600,000 B/s` and `IB=3,936,000,000 B/s`, and its unspecified
`active_only` field means dual-context votes. The `sde_reg` rotator client is
joined only for `1 -> 590`; its four pinned cases request instantaneous
bandwidth of 0, 153,600,000, 320,000,000, or 640,000,000 B/s on the active
context. These profiles preserve their source-file attribution in the report
and do not match if either name or route differs. This adds trace decoding,
not hardware-write authority.

The inventory now also extracts `qcom,devbw` clients, whose vectors are
generated at runtime: the current Sunfish DT contains 11 such nodes, all 11
have resolved routes, and their derived `dev_name()` values match all 11
corresponding names in the saved Linux registration trace. The DTB parser was
corrected to preserve nodes carrying multiple DTC labels; without that, the
GPU bandwidth client was incorrectly indexed outside `/soc`. The request
auditor now pairs each devfreq AB/IB line with the following MSM-bus update for
the same registered handle. It applies the pinned `devfreq_devbw.c` formula
(`AB / num_paths * (1 << 20)` per path and `IB * (1 << 20)` per path) and the
DT `qcom,active-only` setting. The logged usecase index is reported as a
double-buffer slot, not misread as an OPP number. The two inventory parser
tests and ten request-trace tests pass.

A refreshed read-only LineageOS capture using the fabric inventory completed
through `capture-msm-bus-update-trace.sh` at
`.cache/pixel-sunfish/msm-bus-update-trace/lineage-validation-20261023/`.
It captured 106 filtered kernel lines and all 49 expected registration handles,
with 57 resolved paths, no failed/orphan paths, no bus errors, and no auditor
anomalies. The normal LineageOS boot emitted no `update_request_adhoc` or
`devbw` change records, so this confirms the baseline only. The helper restored
ADB from temporary root to `uid=2000(shell)`. At this stage, the dynamic-debug
boot image had not yet been booted.

The next required device check at this stage was to temporarily boot the image,
capture the resulting kernel `update_request_adhoc` and `devbw: BW MBps` lines,
and return to LineageOS. The result follows. No partition was flashed. The
added instrumentation does not write bus/RPMh resources or alter Wi-Fi
settings.

### First dynamic-debug boot trace and targeted CMD_DB follow-up (2026-10-23)

The user-authorized temporary boot completed on Fastboot product `sunfish`.
The image SHA-256 was
`15dbfe06dc9d45d9fe520759051ccf09cef4f9af152429e9f2ab37c27d8f8bde`; no
partition was flashed. After Android returned, the read-only capture helper
temporarily restarted adbd as root to read `dmesg`, then restored it to
`uid=2000(shell)`. The filtered trace at
`.cache/pixel-sunfish/msm-bus-update-trace/capture-20261023-dyndbg/` contains
570 lines, all 49 registrations, 441 matched request transitions, 23
unchanged-index notes, no devfreq samples, no failed/orphan routes, and no
kernel bus-error lines. There is one prior-index mismatch: handle 22 (`grp3d`)
reports prior index 11 where the new-registration model expected `-1`; the
simulator leaves that transition out rather than infer its state.

Numeric runtime handles changed order relative to the earlier boot, so the
simulator now joins registrations by client name and ordered path signature.
It reconstructs 440 transitions with no registration or route issues. The
saved request report joins 26 clients to static DT usecases, one client to a
pinned source profile, reports 10 inactive clients, and leaves 11 dynamic or
otherwise unmatched clients without static votes. The Linux `ipa` client
registered four paths and emitted 33 transitions; its final index is 1
(`SVS2`). No registered client is named WLAN. Because the IPA route contains
six BCM names whose CMD_DB clock domains have not been captured, its VCD-group
candidate remains unresolved; only 132 of the 440 reconstructed events have
complete metadata. A complete candidate for `scm_pas` orders first-touched
BCMs by VCD as MC0 (0), SH0 (1), SN11/SN0 (3), CE0 (6), and ACV (11). This
still does not calculate votes or TCS commands, and the missing devfreq samples
leave dynamic bandwidth requests unknown.

The fabric inventory now includes the `qcom,bus-dev` targets that the earlier
route-only inventory omitted. That exposed and fixed a simulator failure on
the valid parent touch `fab-system_noc`; the refreshed inventory has 129 bus
nodes and SHA-256
`2e0d7fef767a2b3de3ca6283f855117e82a8df85e2964262cadad8397bffd108`.
Validation passes the two inventory-parser tests, ten request-trace tests, ten
commit-order tests, and five boot-v2 packer tests.

To resolve the six missing IPA-route metadata records (`SN1`, `SH5`, `SH2`,
`SN15`, `CN0`, and `IP0`), I built a separate CMD_DB-only image at
`.cache/pixel-sunfish/ipa-bcm-metadata-probe/fastboot.img` (SHA-256
`a10d89e25a6d92ae87e081c1450926b2b222f6d66dfa8cbc52ae18ddcf8b68d7`). Its
CapDL audit grants two read-only CMD_DB pages and the reserved display pages;
it maps no RSC, modem/WLAN memory, channels, or DMA, and issues no BCM reads,
votes, secure calls, or subsystem starts. It has booted temporarily, and the
settled-screen photo was not captured before the user returned the phone to
LineageOS; no partition was flashed.

The request-trace report now also joins each logged old/new index to its exact
static usecase vector where both ends can be resolved. It classifies 324 of
441 updates as known transitions, leaves 116 without a static match, and marks
the single prior-index inconsistency separately. All 33 `ipa` transitions map
between its known `MIN` and `SVS2` profiles; its first transition is `-1` to
`SVS2`, and the final recorded transition is `MIN` to `SVS2`. This resolves the
IPA request vectors, but still does not calculate effective votes or RPMh TCS
commands. The 13 request-trace tests pass. The user reports LineageOS is back,
but `adb devices -l` currently lists no connected device, so a fresh Android
capture cannot run until USB enumeration returns.

The full `platforms/sunfish/test-modem-qmi.py` runner also completed
successfully in this checkout. It passed the QMI, TFTP, SERVREG, IPA GSI,
QCACLD CE, and composed storage/owner groups, plus AArch64 `no_std` compilation
and native code generation checks. Those remain host/build validation; no
hardware services, secure assignment, or WLAN startup ran.

### BCM snapshot bracketing the AOP notification (2026-10-23)

The user supplied the settled result from the read-only BCM/AOP bracketing
probe (`~/Downloads/IMG_8891.heic`). Both six-resource passes completed with
matching values: MC0 `0x29917f75`, ACV `0x20000008`, CE0 `0`, SN11 and SN0
`0x213c44f1`, and SH0 `0x240d5a0f`. The AOP phase mask was `0x1f`, notification
count `4`, and each pass read all 12 samples. The AOP delta mask and all read,
control, and handoff error fields were zero; MDT verification was `1`.

This establishes that the sampled BCM registers did not change across this
single AOP modem-on notification in this boot. It does not show that the
associated RPMh votes were committed, prove secure ownership, or explain IPA
readiness. The probe made no BCM resource writes, secure calls, or modem/WLAN
starts. The user has returned the phone to LineageOS, but `adb devices -l`
still lists no device, so the planned passive Android-side follow-up cannot
run until USB debugging transport is visible.

### Source-matched `qcom,devbw` OPP inventory and Lineage snapshot (2026-10-23)

The MSM-bus inventory now resolves each `qcom,devbw` client's governor and
`operating-points-v2` table from the captured live Lineage DTB. The updated
`.cache/pixel-sunfish/msm-bus-client-inventory-current-20261023-fabric.json`
has SHA-256
`a60ae7b67631a1f53646d6e8e92c151cb2a41e9e115b6507e128de8e066a09bb` and retains
the same DTB hash, fabric-topology content, and complete 49-handle registration
set, while adding all 11 dynamic-client profiles. The parser
combines the DT's two-cell 64-bit `opp-hz` values and, only for the pinned
`devfreq_devbw.c` path, labels that selected value as the requested IB in
MiB/s: Sunfish's `BW_OPP_ENTRY` macro encodes bandwidth there, and the driver
passes the selected value to `set_bw()` as `new_ib`.

The new
[`capture-msm-bus-devfreq-state.py`](../platforms/sunfish/capture-msm-bus-devfreq-state.py)
reads each client's `governor`, `cur_freq`, and `available_frequencies` over
ordinary authorized ADB. It pins the exact device serial, verifies `sunfish`,
and does not request root or write sysfs. The pinned driver and governor source
also proves `gov_ab` stays zero for `performance` and `powersave`, since the
zeroed devbw state is not modified by those governors. The one `bw_vbif`
client gets dynamic average bandwidth through KGSL and remains incomplete
from sysfs alone. The inventory now source-checks that KGSL's `gpubw_mon`
derives this average from VBIF `ram_time/total_time`, target `qcom,bus-width`,
and a 160 MiB/s rounding step; KGSL passes the result through `bw_vbif` to the
devbw client. The existing `devbw: BW MBps: AB: … IB: …` debug line records
the applied pair only when it changes, and the earlier Lineage capture had no
such samples. The read-only sysfs capture therefore reports the formula and
the missing counter input without inventing a GPU average. For the other
clients, the script maps a current IB only
when `cur_freq` matches both an enabled DT OPP and `available_frequencies`,
then emits per-path active-only/dual byte-per-second contributions using the
driver's `new_ab / num_paths` and `new_ib * 1 MiB` formulas. Unknown inputs
remain null, so those client reports cannot masquerade as complete.

The inventory host suite passes five tests and the capture suite passes nine.
Regeneration against the current Lineage DTB resolved all 11 OPP tables and
all 49 logged registrations. The capture currently fails closed because ADB
sees no device; no new handset action or reboot was performed. This remains
Linux reference data for the native bus ledger, not a submitted vote or Wi-Fi
readiness proof.

### IPA `INIT_MODEM_DRIVER` snapshot patch prepared (2026-10-23)

The current ordinary Lineage capture cannot read `/sys/kernel/debug/ipa/hw_type`
or `gen_reg`, and its saved kernel log contains only the QMI send/response
markers. I prepared
[`sunfish-lineage-ipa-init-snapshot.patch`](../patches/sunfish-lineage-ipa-init-snapshot.patch)
against the pinned IPA v3 source. It adds a compact `pr_info` snapshot of the
already-decoded IPA SRAM restricted base/size, modem memory offset/size,
APPS_WAN_CONS endpoint validity/value, uC-loaded/SSR state, and the returned
QMI result/error, endpoints, and `modem_driver_init_pending` fields. The
existing uC-loaded check is evaluated once and reused for the same branch;
there are no additional register reads, QMI messages, resource writes, DMA
operations, or subsystem starts. The Lineage capture helper now retains these
snapshot lines from the existing kernel log buffers.

`git apply --check`, `patch --dry-run`, and `sh -n` pass. The kernel patch is
not applied, compiled, or booted. The live device capture was attempted after
the reported LineageOS return, but ADB found zero devices and the USB inventory
showed only the Lexar reader, so no new phone state was read. The available
kernel build configuration has `CONFIG_IPA3=y`, which means testing this
instrumentation requires a temporary kernel image boot rather than swapping an
IPA module. First retry the unchanged read-only Lineage capture once ADB
enumerates; only if the required fields remain unavailable should we build
and separately review a temporary instrumented kernel boot.

### IPA-route CMD_DB metadata probe rebuilt (2026-10-23)

The focused read-only probe for the six IPA-route BCMs whose CMD_DB metadata is
not yet recorded was rebuilt from the current source. Its Rust unit tests pass,
and the CapDL audit reports 67 pages, two read-only CMD_DB pages, 11 intentional
splash pages, zero overlaps, no RSC or fixed modem-data mappings, and no mapped
WLAN MSA reservation. The metadata parser requires a stable copy of the full
`0x1078`-byte database and a stable CMD_DB directory before it displays the
address, unit size, width, clock domain, auxiliary version, and reserved byte
for SN1, SH5, SH2, SN15, CN0, and IP0. The only writable mapping is the splash
framebuffer; the image does not submit bus votes, start the modem/WLAN, or
transfer firmware.

The temporary Fastboot image is
`.cache/pixel-sunfish/ipa-bcm-metadata-probe/fastboot.img`, SHA-256
`a10d89e25a6d92ae87e081c1450926b2b222f6d66dfa8cbc52ae18ddcf8b68d7`. It has
not been booted in this run. The Pixel is reported back in LineageOS, but the
host currently has no ADB device; the next check requires Fastboot enumeration
before using `fastboot boot` (no partition flash).

### Runtime BCM baseline correction and modem-preparation rebuild (2026-10-23)

The paired post-AOP read and same-boot AOP delta images show that the six BCM
replies are stable within a read series, while SH0 can differ between boots
(`0x24005adf` in the older reference tuple and `0x240d5adf` in later captures).
The old `hold_mc0()` gate compared every reply against the historical tuple,
although it writes only MC0; that stale global comparison caused
`StartingValues` before any resource WRITE. Runtime validation now pins the MC0
request to its observed numeric fields, requires all six starting replies to be
stable across both passes, and checks every untouched resource against that
same boot's baseline after MC0 reassertion and subsequent rail votes.

The separate PAS-route floor phase still uses fixed SN11, SN0, and CE0 words.
It therefore requires those three starting resources to equal the exact
reference values in both passes before sending any of the three floor WRITEs;
ACV and SH0 may vary only if stable and unchanged. The test suite exercises
the observed alternate SH0 value through both the four-write combined hold and
the seven-write floor model, and confirms mismatched floor targets prevent all
three floor WRITEs. This keeps the correction limited to stable, untouched
resource baselines rather than broadening any written target.

Validation: the focused RPMh Rust suite passes 98 tests; the complete Rust
library suite passed 585 tests. The rebuilt modem-preparation image passes its
three preparation tests, 25 AOP MMIO tests, 25 BCM MMIO tests, 14 metadata MMIO
tests, AArch64 release build, MDT layout audit, and CapDL audit (85 pages, zero
overlaps). The temporary image is
`.cache/pixel-sunfish/modem-prepare-probe/fastboot.img`, SHA-256
`a652147802bb55f0f86b37b0fe405c4cda2c2eb525c081677b535b5cb85f3ce7`. It sends
the fixed AOP ON notification and can issue up to seven fixed RSC writes
(MC0, CX/MSS/XO, then SN11/SN0/CE0); those requests remain held until physical
restart. It has no SMC capability and does not authenticate, transfer segments,
release modem reset, start modem/WLAN, or use DMA. It has not been booted with
this corrected gate; current ADB and Fastboot enumeration are empty.

### Latest read-only BCM/AOP baseline photo (2026-10-23)

The newly attached `BCM AOP BEFORE AFTER` image currently available as
`~/Downloads/IMG_8891.heic` (SHA-256
`637f91479f91b61c3ea68b47c7421560b5505e48de20327104198707a6baa8e2`) reports
both passes successful, AOP error `0`, phase mask `0x1f`, four AOP
notifications, 12 reads in each pass, zero pair differences, and a zero AOP
delta mask. MC0 is `0x29917f75`, ACV `0x20000008`, CE0 `0`, SN11/SN0
`0x213c44f1`, and SH0 `0x24005adf`; every value matches across passes.
Read, controller, and handoff error counters are all zero, and MDT is
verified. The screen explicitly reports no BCM resource write, no secure call,
and no modem/WLAN start. This is a read-only baseline result; it does not
exercise the corrected write gate.

The same download basename was used in an earlier record above, which reports
SH0 `0x240d5a0f`. The current attachment's hash and visible value do not match
that earlier report; the filename alone cannot identify the capture, so the
two SH0 observations are kept as separate evidence until their original files
can be distinguished.

### Cached `cnss-daemon` Peripheral Manager/WLFW lead (2026-10-23)

Read-only inspection of the locally cached Sunfish `cnss-daemon` found an
AArch64 stripped executable with Build ID
`49da31f376b4e4ddfd8b08e16e5bad42` and SHA-256
`5ea335e2767ef1b89693f27a725aca17b743871ff8e79f609ce928d87d147772`. Its
ELF imports `pm_client_register`, `pm_client_connect`,
`pm_client_event_acknowledge`, `pm_client_unregister`, and the
`libperipheral_client.so` API, alongside QMI client entry points. Its strings
include `pm_event_notifier`, `pm_vote`, `pm_release_vote`, `wlfw_start`,
`wlfw_service_request`, and WLFW/“wlan service” status messages. This suggests
the daemon participates in modem power-event handling and the post-arrival
WLFW client path. It does not reveal the callback logic or prove that it starts
the WLAN protection domain; the binary was not executed.

The saved Lineage boot-order record
[`lineage-init-wlan-order-20261023.txt`](../.cache/pixel-sunfish/lineage-init-wlan-order-20261023.txt)
places the `wlan_pd` UP indication at `09:30:46.103`, `cnss-daemon` process
startup at `09:30:49.780`, and WLAN FW readiness at `09:30:54.483`. In that
captured boot, the modem domain is already UP before `cnss-daemon` starts, so
the daemon is not established as the initial `wlan_pd` launch trigger. The
capture and binary instead reinforce its likely role after domain arrival.

The native path must not synthesize Peripheral Manager votes or infer their
meaning from these symbol names. A production adapter needs the matching
client/service contract and a verified lifecycle placement. The next useful
source evidence is the corresponding `libperipheral_client` interface and
modem event sequence; the next live comparison, once LineageOS ADB enumerates,
is a read-only capture of the Sunfish `PerMgrSrv` and `cnss-daemon` records
around modem online, domain UP, and WLFW readiness. This does not change the
current corrected modem-preparation probe or authorize a WLAN start.

### Modem-preparation candidate revalidated for the next temporary boot (2026-10-23)

I reran `sh platforms/sunfish/build-modem-prepare-probe.sh` from the current
checkout. The metadata/preparation suite passed 3 tests, the AOP MMIO suite 25,
the BCM MMIO suite 25, and the PAS metadata MMIO suite 14. The AArch64 release
build and MDT structural checks passed; the CapDL audit reports 85 pages, 24
reserved intervals, 11 intentional splash pages, and zero overlaps. The
rebuilt temporary image remains SHA-256
`a652147802bb55f0f86b37b0fe405c4cda2c2eb525c081677b535b5cb85f3ce7`.

This candidate sends the fixed AOP ON notification, then only submits its
fixed MC0/CX/MSS/XO and SN11/SN0/CE0 RSC requests when the fresh two-pass
resource snapshots, acknowledgement checks, and bank-preservation guards pass.
It stops at the first fault and leaves applied requests held until physical
restart. It does not invoke secure firmware authentication, transfer modem
segments, release modem reset, start modem/WLAN, or perform DMA. The candidate
is still unbooted with the corrected baseline gate. Fastboot enumeration is
required before the one temporary `fastboot boot`; no partition flash is
involved. The experiment is staged: MC0/CX/MSS/XO can already be held before
the separate exact starting-value check for SN11/SN0/CE0. If that later check
fails, it suppresses all three floor writes, but the four earlier holds remain
until physical restart. The build message now distinguishes the fixed RSC
floor holds from PAS firmware authentication and reset release.

### Static Peripheral Manager control-flow check in `cnss-daemon` (2026-10-23)

I cross-checked the cached daemon's executable text against its embedded
`.gnu_debugdata` symbol map, without executing or modifying the binary. The
daemon SHA-256 remains
`5ea335e2767ef1b89693f27a725aca17b743871ff8e79f609ce928d87d147772`. The
embedded minimal ELF supplies symbol addresses but marks `.text` as `NOBITS`,
so the instruction bytes were disassembled from the original local executable
at those addresses. The symbol map identifies `main` at `0x8054`,
`wlfw_start` at `0xd860`, `pm_event_notifier` at `0xaf00`, and `pm_init` at
`0xaf5c`.

The `main` call sequence invokes `wlan_service_start`,
`wlan_dp_service_start`, then `wlfw_start`. When `wlfw_start` receives zero,
it calls `pm_init(1, NULL)` and `pm_init(0, "SDXPRAIRIE")` before it creates
the WLFW worker. `pm_init` obtains system-info entries, filters them by the
requested type and optional name, calls `pm_client_register` with
`pm_event_notifier`, a stored handle/client-data pointer, the selected
peripheral name and client name, then calls `pm_client_connect` when
registration supplies a handle. The notifier logs the event and calls
`pm_client_event_acknowledge` for a live handle. This gives a concrete PM
registration/connect/acknowledgement path for the daemon, but it does not
establish which PM entry controls modem boot or that this path launches
`wlan_pd`. The saved Lineage boot-order capture still places `wlan_pd` UP
before `cnss-daemon` starts.

The read-only capture helper now includes narrowly selected existing
`PerMgrSrv`, `PerMgrLib`, and `cnss-daemon` lines for PM registration, connect
or vote failures, event acknowledgement, and WLFW startup. `sh -n`, JSON
parsing, and `git diff --check` pass. A temporary mock-ADB harness also
verified that the capture emits the selected PM state/client lines and filters
an unrelated log line. The phone is still absent from both ADB and Fastboot,
so no new device capture or temporary boot has occurred.

### Corrected modem-preparation gate passed on Sunfish (2026-10-23)

The rebuilt temporary image
`.cache/pixel-sunfish/modem-prepare-probe/fastboot.img` (SHA-256
`a652147802bb55f0f86b37b0fe405c4cda2c2eb525c081677b535b5cb85f3ce7`)
was accepted with `fastboot boot` after Fastboot reported product `sunfish`;
no partition was flashed. The settled photo `IMG_8893.heic` (SHA-256
`81a6f4e8b8e2b032bc5eee72816074e382128d0da68ca158b973ead7e2a33fd1`)
shows `PREPARE PASS`, error `0`, AOP phase mask `0x1f`, four notifications,
RSC phase `0x0b` (`FloorsHeld`), write-ACK mask `0x7f`, 119 requests, and
238 handoff checks. MDT verification passed for the 9,100-byte MDT at physical
`0x80001000`. Controller and MC0 guard errors are both zero. The final
readbacks are CX `6`, MSS `7`, MX `3`, XO `3`, MC0 `0x29917f75`, ACV
`0x20000008`, CE0 `0x2fffffff`, SN11/SN0 `0x213c5d02`, and SH0 `0x240d5adf`.

This confirms the guarded MDT preparation, fixed AOP ON notification, and
seven fixed RSC requests on this boot. It does not establish complete
aggregation/QoS or PAS readiness. The screen explicitly confirms no secure
call, no segment files, and no reset release; the modem and WLAN were not
started. The fixed requests remain held until physical restart, so the Pixel
should return to LineageOS before any subsequent test. This is a prerequisite
milestone, not a working-Wi-Fi result.

The user returned the Pixel to LineageOS, and ADB identifies it as Sunfish
running `23.2-20261001-NIGHTLY-sunfish`. The unchanged read-only capture at
`.cache/pixel-sunfish/lineage-ipa-state-post-modem-prepare-20261023.txt`
(SHA-256
`b01b4f795b5c2becc5e3f3de7b36da1028e4b4b397d6f57500c69aacea8dad60`)
shows the ordinary boot again completed the IPA modem-driver QMI request,
modem Power/Clock readiness, `wlan_pd` state indication and ACK, ICNSS QMI
connection, `WLAN FW is ready: 0xd87`, and the WMA firmware-ready event. This
confirms return to the LineageOS reference path and that the temporary RSC
holds were cleared by restart; it is not evidence that the seL4 path started
the modem or WLAN. No matching Peripheral Manager lines were present in the
available log buffers.

### Current-tree Sunfish offline regression pass (2026-10-23)

`python3 platforms/sunfish/test-modem-qmi.py` completed successfully on the
current worktree. The host suites covering QRTR/QMI, GLINK/FIFO, SERVREG,
WLFW, modem ownership and storage, IPA/GSI, CE/WMI/HTT, WLAN DMA, and SMMU all
passed; the script also completed its AArch64 `no_std` core/native codegen
checks. This is offline validation only. It does not implement or exercise the
missing live IPA/GSI/SMMU providers, load authorized firmware, start the modem
or WLAN, or establish packet traffic.

### Lineage kernel source identity cross-check (2026-10-23)

After the modem-preparation experiment returned to LineageOS, ADB read-only
checks reported product `sunfish`, Lineage `23.2-20261001-NIGHTLY-sunfish`,
and kernel release `4.14.357-openela-g28f9290ae067`. The final 12-character
revision matches the local Lineage source tree's exact HEAD,
`28f9290ae067d3e7857126eb6312720dbe9c7c80`. `validate-icnss-startup.py`
reports the audited ICNSS/IPA/WLFW source files clean at that pinned revision,
and `validate-wlan-resources.py` passes against both the current live-derived
runtime DTB and the generated Microkit DTB. These checks strengthen the
reference-source match used by the native driver profiles; they are not
binary-level attestation and do not create seL4 hardware authority.

The cached source tree has a local instrumentation change in
`drivers/iommu/arm-smmu.c`; the audited ICNSS/IPA/ath10k files have no local
changes. A read of the latest 2,500 kernel-log records found no matching
`SUNFISH_SMMU` instrumentation messages, which leaves live SMMU route state
unverified. `qcom_wlan_power.rs` now describes this distinction: the matching
kernel revision is evidence for the Linux reference profile, while native
profile minting, complete RPMh client ledgers, and an exclusive power/RSC
lease remain absent.

### Current Lineage qcom,devbw snapshot (2026-10-23)

The read-only `capture-msm-bus-devfreq-state.py` helper initially treated
Toybox's `cat: … Permission denied` text as a successful scalar because
`adb exec-out` returned status zero. The decoder now recognizes remote `cat`
diagnostics and stderr as unreadable attributes; three regression tests cover
that behavior, and the focused capture suite passes all 12 tests. The
unprivileged attempt then correctly reported all 33 fields unavailable.

Using the already documented temporary root-ADB read path, I captured the 11
Sunfish `qcom,devbw` governor states and current IB OPPs to
`.cache/pixel-sunfish/msm-bus-devfreq-state-rooted-20261023.json` (SHA-256
`ec07d6ea362155723b8244faeadbc28fc6abefcdc92938383c3051687b28f645`). All
governor, `available_frequencies`, and `cur_freq` reads were available, and
each current value matched an enabled OPP: CPU-LLCC `9155`, CPU-DDR `2086`,
CPU0 DDR latency floor `1144`, CPU0 CPU-LLCC latency `4577`, CPU0 LLCC-DDR
latency `1144`, CPU6 DDR latency floor `1144`, CPU6 CPU-LLCC latency `7110`,
CPU6 LLCC-DDR latency `3879`, GPU `0`, NPU-DDR `0`, and SNOC-CNOC keepalive
`1` MiB/s. Only SNOC-CNOC keepalive has a source-confirmed zero average
request; the other ten `gov_ab` values remain dynamic or unavailable, so only
one full request profile is complete. This fills current devfreq IB state,
not the live aggregate vote or ordered commit list. No bus/RPMh request or
Wi-Fi setting was changed. ADB was returned to `uid=2000(shell)` and verified.

### Temporary devbw debug boot follow-up (2026-10-23)

The temporary `fastboot-devbw.img` (SHA-256
`628dc7e229b7c5110c7de72d3ad9e0e16adee0d253f19d1cfcf104c37cc745ba`)
was accepted for Sunfish with `fastboot boot`; it changes only the boot
command line and does not flash a partition. `/proc/cmdline` confirmed the
requested `dyndbg="func update_request_adhoc +p; func set_bw +p; file
drivers/devfreq/devfreq_devbw.c +p"` argument. The temporary Android boot
eventually exposed ADB, and the read-only root capture was followed by
`adb unroot`; `uid=2000(shell)` was verified before restarting.

The rooted capture `.cache/pixel-sunfish/msm-bus-devfreq-state-tempboot-20261023.json`
(SHA-256
`f53cbf705ef2a90fb44d56f9c2ad44ab093e03326818ca1eab2c6c2a8758ca7b`)
read all 11 `qcom,devbw` nodes and their governors, available OPPs, and current
IB values. The observed current IB values in MiB/s were CPU-LLCC `7110`,
CPU-DDR `2597`, CPU0 DDR latency floor `2086`, CPU0 CPU-LLCC latency `4577`,
CPU0 LLCC-DDR latency `2929`, CPU6 DDR latency floor `2086`, CPU6 CPU-LLCC
latency `9155`, CPU6 LLCC-DDR latency `2086`, GPU `0`, NPU-DDR `0`, and
SNOC-CNOC keepalive `1`. All current frequencies matched available OPPs.
Ten average-bandwidth request profiles remain partial because `gov_ab` is
dynamic; the powersave keepalive profile is complete. These are current IB
values, not committed per-client aggregate votes.

The filtered kernel log contained 2,792 `update_request_adhoc` records and no
client-registration records. Its lack of `devbw` messages is not evidence the
debug query failed: the capture filter incorrectly expected `devbw: BW MBps`,
but the pinned `devfreq_devbw.c` emits this through `dev_dbg(dev, ...)`, and
the kernel formats device-debug lines with the driver name followed by the
device name. The expected form is `devbw soc:qcom,<client>: BW MBps: AB: …`.
The old filter silently discarded those lines. The pinned source also confirms
`CONFIG_DYNAMIC_DEBUG=y`, `CONFIG_QCOM_DEVFREQ_DEVBW=y`, and that `set_bw()`
contains the AB/IB debug call ([driver](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/devfreq/devfreq_devbw.c),
[Sunfish config](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/arch/arm64/configs/sunfish_defconfig),
[device debug formatting](https://raw.githubusercontent.com/LineageOS/android_kernel_google_msm-4.14/28f9290ae067d3e7857126eb6312720dbe9c7c80/lib/dynamic_debug.c)).

The parser and log filter now accept the kernel's actual driver/device prefix;
the capture also preserves useful devbw samples when early registration logs
are missing and marks registration coverage as partial instead of discarding
the report. Host tests pass. The corrected image
`.cache/pixel-sunfish/msm-bus-update-trace/fastboot-devbw-corrected.img`
(SHA-256
`4cea0430a5c24df88a353773010778affa078e3240d0c624d04507e146594cad`) enables
only `set_bw` and enlarges the kernel log buffer to 4 MiB, avoiding the
previous high-volume bus-update trace while retaining early devbw records.
The builder verified that all source payloads are unchanged and only the
temporary boot command line differs. This image was then temporarily booted
with `fastboot boot`; the read-only capture at
`.cache/pixel-sunfish/msm-bus-update-trace/capture-20261023-corrected/`
found all 49/49 expected client registrations, 76 devbw samples (66 from the
active `gpubw` node), and zero bus-error lines. It produced no individual
`update_request_adhoc` records because this pass intentionally enabled only
`set_bw`; the registrations include `ipa` and `pil-modem`, but no client is
explicitly named WLAN. That inventory does not rule out a generic or deferred
WLAN request. The capture helper returned ADB to `uid=2000(shell)`. It did not
change a bus/RPMh request, load firmware, start modem/WLAN, or change a Wi-Fi
setting.

To join aggregate samples to per-client activity, the next temporary image
`.cache/pixel-sunfish/msm-bus-update-trace/fastboot-devbw-updates.img`
(SHA-256
`505f0969bbd6c6f9cc05733a76a8aa800ca018d8a5ae34899d849bb93ba43ee4`)
enables both `update_request_adhoc` and `set_bw`, retaining the 4 MiB log
buffer. Its builder verified that the original kernel, ramdisk, second stage,
recovery DTBO, DTB, and all boot-header fields other than the appended command
line are unchanged. It was temporarily booted with `fastboot boot`, and the
read-only capture at
`.cache/pixel-sunfish/msm-bus-update-trace/capture-20261023-updates/` recorded
49/49 client registrations, 639 request updates (all matched to registered
handles), and 191 devbw samples (all joined to request updates). It found no
failed/orphan registration paths or bus-error lines. One `grp3d` update began
from prior index `11` rather than the parser's initial `-1`; its subsequent
transition matched, so this is retained as a first-sample baseline anomaly.
The active requesters included `ipa` (46 updates), `pil-modem` (2), and
`gpubw` (181); no registered client was explicitly named WLAN. This remains a
LineageOS behavioral baseline, not evidence that the custom OS has a working
WLFW transport or Wi-Fi path. The capture returned ADB to `uid=2000(shell)`,
and the phone is now back on installed LineageOS 23.2 (`sunfish`, Android 13).
A post-return read-only check showed Wi-Fi enabled, connected, and the
supplicant in `COMPLETED` state; network identifiers were not saved.
No bus/RPMh request, firmware load, modem/WLAN start, or Wi-Fi setting was
changed.

The source-backed interpretation is narrower than “WLAN made 46 votes.” The
trace's `ipa` handle is the IPA platform client's four-route bus profile. At
the pinned kernel revision, `ipa.c` registers this shared IPA bus client;
`ipa3_enable_clks()` requests the profile selected from the current IPA clock
rate, and `ipa3_disable_clks()` returns it to index 0. Its clock plans map
indices 1–3 to SVS2/SVS/nominal and index 4 to turbo. The `icnss.c` WLAN
control driver has no direct `msm_bus_scale_*` calls. Thus these IPA updates
are shared accelerator clock votes and cannot by themselves identify a
WLAN-specific request. The installed Android image also has no mounted
`debugfs` tree (`/sys/kernel/debug` is absent), so the IPA active-client table
was not available through this read-only check. See the pinned [IPA bus vote
implementation](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/platform/msm/ipa/ipa_v3/ipa.c#L4957-L5043)
and [ICNSS source](https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/28f9290ae067d3e7857126eb6312720dbe9c7c80/drivers/soc/qcom/icnss.c).

### IPA vote timing in the boot trace (2026-10-23)

The combined bus trace reports 46 transitions for the registered `ipa` client
(handle 10, four paths). Its first transition to index 1 occurs at 0.352726 s;
the last transition back to index 0 occurs at 48.347053 s. Excluding the
initial `-1` registration state, the trace contains 23 transitions from index
1 to 0 and 22 from 0 to 1. With a 0–15 s uptime window, only the initial two
transitions appear; the other 44 occur from 17.503619 to 48.347053 s. The
available record does not mark Android boot completion, so this later interval
is not labeled steady state. The trace was collected with Wi-Fi connected, but
it has no Wi-Fi-toggle markers and the shared IPA clock-vote implementation
does not identify which client caused any individual transition. These counts
cannot be attributed to WLAN.

A useful follow-up must separate steady-state windows after Android finishes
booting: record a passive baseline, mark the Wi-Fi-off interval, then mark the
Wi-Fi-on interval and restored connection. The trace auditor now retains
monotonic kernel timestamps and accepts repeated `--window NAME:START:END`
arguments; window summaries count per-client index transitions and aggregate
devbw samples on start-inclusive/end-exclusive intervals. Compare handle-10
activity across those windows while retaining the full registration/update
trace for validation. This remains a LineageOS behavior comparison, not
evidence of custom-OS WLAN operation. No Wi-Fi state was changed during the
trace described above.

### One-shot SMMU install-plan executor (2026-10-23)

`rust/src/sunfish_smmu_exec.rs` now interprets the existing ordered SMMUv2
install plan through an injected `InstallIo` interface. It preflights table
page addresses and wait masks, publishes table pages, performs bounded TLB
status polling (at most 1,024 reads per wait), binds the context fault handler,
and then executes the plan's context-bank and route writes in plan order. A
successful installer is one-shot; any I/O error or wait timeout quarantines
that instance so it cannot replay a partially applied sequence. Invalid input
or a rejected polling budget is rejected before I/O.

The focused SMMU program suite passes 75 tests, including execution-order,
publication/fault-handler-before-enable, bounded timeout, and no-replay checks.
`python3 platforms/sunfish/test-modem-qmi.py` also passes, including the
AArch64 `no_std` core and native code-generation checks. This adds an execution
contract for the generated plan, not a production `InstallIo` hardware adapter:
it does not establish an exclusive SMMU lease, prove current route state,
provide MMIO access, or authorize enabling a Sunfish route. The Pixel stayed in
LineageOS during this work, and no hardware state was changed.

### Paired LineageOS Wi-Fi bus-trace comparison (2026-10-23)

At the user's request, the existing `fastboot-devbw-updates.img` was temporarily
booted on the confirmed `sunfish` target with `fastboot boot` only. The image
SHA-256 was
`505f0969bbd6c6f9cc05733a76a8aa800ca018d8a5ae34899d849bb93ba43ee4`; no
partition was flashed. After Android finished booting and Wi-Fi associated, a
60-second connected baseline was recorded, Wi-Fi was disabled for 60 seconds,
then Wi-Fi was enabled and observed connected for another 60 seconds. The
normal installed LineageOS boot was restored afterward, and a read-only check
confirmed LineageOS `23.2-20261001-NIGHTLY-sunfish`, Wi-Fi connected, and ADB
returned to `uid=2000(shell)`.

The captured kernel trace contains 49/49 registered MSM-bus clients, 10,725
matched request updates, 9,531 devbw samples, and zero failed-path, orphan-path,
or bus-error records. No registered client is named WLAN. The shared IPA client
handle 10 showed no updates in the connected baseline, five `0→1→0` pairs
(10 updates) during the Wi-Fi-off window, and one pair (2 updates) shortly
after Wi-Fi reconnected. The first off-phase update began during the disable
transition; the remaining pairs were brief. Since this handle is the shared
four-route IPA client and the Linux ICNSS control driver has no direct
`msm_bus_scale_*` calls, timing alone does not attribute these requests to
WLAN. The first pass was inconclusive; a later 30-second repeat reproduced
the five off-phase IPA pairs but showed a different reconnect count (details
below). This repeated timing pattern still does not attribute the updates to
WLAN. CPU/DDR devbw and unrelated clients remained active throughout, so total
update/sample counts are not a WLAN signal.

The parser reports one known baseline anomaly: `grp3d` handle 22's first
observed prior index is 11 instead of the parser's initial -1. It does not
affect matched-update accounting. Raw filtered trace, whole-trace report,
redacted phase markers, and time-window report are in
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-20261023-162222/`.
The raw trace, whole-trace report, redacted phase markers, and time-window
report have SHA-256 values `c832c02efb5b424263a26086b87e5da6471456488755cf625e01548cd878ea45`,
`b08ed39daeaab28ec14aa4df6067606f893c71ddb8e4e1ece953bf74522a1cda`,
`977a10e7a831c380e94684ae26463089c3c78f25db4f24d4e2eb8624f6198898`, and
`634663ee7554f6a2f8a37cdf6e150ab1bb0eabd4afd09187c2166fa0ee320846`, in the
same order. The markers contain device uptime and Wi-Fi state only; they do not
retain network names or addressing. The comparison is a LineageOS behavioral
observation, not a native seL4 WLAN test.

### Repeated LineageOS Wi-Fi bus-trace comparison (2026-10-23)

The user requested a repeat comparison. The temporary Fastboot image was
accepted but returned the phone to Fastboot rather than Android; `fastboot reboot`
restored the installed LineageOS. A read-only extraction of `boot_a`
confirmed the cached image's kernel, ramdisk, and DTB payloads matched the
installed boot. To avoid another reboot, the same two kernel debug callsites
were enabled at runtime through `/proc/dynamic_debug/control`, and only
`update_request_adhoc` and `devfreq_devbw` `set_bw` lines were streamed from
`dmesg`. The debug flags were disabled afterward, ADB returned to
`uid=2000(shell)`, and Wi-Fi was verified connected. No partition, bus vote,
firmware, modem, or WLAN start state was changed; Wi-Fi was toggled off and
back on for the comparison.

The repeat used 30-second windows: the connected baseline had no IPA handle-10
updates; the 30.13-second Wi-Fi-off window had ten updates, exactly five
`0→1→0` pairs. All ten occurred within 2.56 seconds after Wi-Fi-off was
confirmed, with no further IPA updates during the remaining off interval.
This exactly reproduces the off-phase count from the earlier 60-second pass.
During re-enable and association, four complete pairs occurred across a
6.7-second span, ending 0.17 seconds after the phone reported connected. The
first 60-second pass had only one pair shortly after reconnection, so the
reconnect activity count is not stable. CPU/DDR devbw activity continued
through every window.

This runtime-only trace starts after boot-time client registration. It maps
handle 10 to the four-route `ipa` client using the preceding full registration
capture and the matching installed kernel/boot payload; it does not independently
recapture that mapping. The filter also excludes general bus-error messages.
The repeated off-transition timing is a stronger correlation than the first
pass alone, but it still does not prove WLAN owns or caused the shared IPA
requests. The windowed trace, redacted state markers, and analysis are in
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-repeat-20261023-165128/`.
The normalized test-window trace SHA-256 is
`77b2e66b4bcca0e230060d91e1a3ddc99b334ff92d0103dd53b8467a6c8cbb69`. This is
LineageOS behavior evidence, not a native seL4 WLAN result.

### One-shot IPA post-init readiness gate (2026-10-23)

`sunfish_ipa_post_init.rs` now sequences the source-observed IPA post-init
operations and validates the captured SRAM, control endpoint, and uC fields
before it returns generation-bound readiness. AP service and modem-init
permit constructors now require the coordinator's opaque post-init evidence,
instead of accepting only a generation number. It passes the owner generation
into each backend stage, is one-shot, and quarantines the owner after a partial
failure, so an uncertain hardware prefix cannot be replayed. Five focused
tests and the full `test-modem-qmi.py` regression pass,
including AArch64 `no_std` and native code generation. This remains a software
ordering gate; the production IPA power/MMIO/GSI/IRQ/PAS/SMMU provider is
absent, so no hardware readiness or Wi-Fi operation is claimed. The Pixel
stayed in LineageOS for this change.

### IPA caller-attribution probe feasibility (2026-10-23)

The repeated Wi-Fi-off trace correlates five `0→1→0` transitions with the
shared IPA bus client, but the currently installed LineageOS kernel cannot
identify which IPA active client caused them through its exposed tracing
interfaces. A read-only capability check found `nop` as the only available
ftrace tracer, no `set_ftrace_filter`, and no mounted debugfs or readable
`/sys/kernel/debug/ipa/active_clients`. The unprivileged shell cannot read
`/proc/dynamic_debug/control`. The phone remains in LineageOS and Wi-Fi stays
connected; this check made no kernel or radio changes.

In the matching kernel source, `ipa3_enable_clks()` and
`ipa3_disable_clks()` issue the shared IPA bus votes. The source's per-client
history buffer is compiled only with `CONFIG_IPA_DEBUG` and is exposed via the
IPA debugfs `active_clients` file. Since debugfs and the function tracer are
not available on this running kernel, another Fastboot boot of the existing
bus-trace image would not improve attribution. A useful follow-up requires a
separate temporary kernel build that enables IPA client history plus a
read-only debugfs capture, or function tracing if supported by that build.
This would be a Linux reference observation; it would not demonstrate native
seL4 WLAN bring-up.

The cached kernel audit checkout is not a build workspace: its
`build_sunfish.sh` delegates to `build/build.sh` and
`private/msm-google/build.config.sunfish_no-cfi`, neither of which is present
in this checkout. It is also a different `android.googlesource.com` checkout;
the pinned LineageOS source revision used to match the installed boot is not
cached here. No instrumented kernel image has been built.

At the time of the user's `Fastboot ready` message, the Mac still listed the
Pixel through ADB as `sunfish` with Android boot complete, while
`fastboot devices -l` returned no device. No Fastboot command was run because
the handset was still in LineageOS and no new attribution-capable image was
ready.

At a later `Fastboot ready` prompt, Fastboot enumerated the phone and the
read-only `fastboot getvar product` check returned `sunfish`. No probe image
was booted: the previously recorded Wi-Fi-off bus trace still cannot attribute
votes to an IPA client, and the required instrumented LineageOS kernel has not
been built. The phone was returned to LineageOS with `fastboot reboot`; ADB
then confirmed `sys.boot_completed=1` and `settings get global wifi_on` = `1`.
No Wi-Fi toggle or firmware/partition write occurred during this check.

### Bounded TFTP reads from a retained source (2026-10-23)

The TFTP protocol core reads only each negotiated DATA block into the caller's
packet buffer through a fixed-extent random-access `ReadSource`. A no-allocation
`ReadSourceCell` adapts mutable storage I/O to that interface, checks the live
source length before and after reads, and fails closed on borrow conflicts,
extent drift, or uncertain I/O. The QRTR transfer server and modem-owner
service adapters are generic over that source, so a future authorized provider
can serve a file without first copying the entire image into RAM. A failed
backing read quarantines the session and retires its QRTR transfer instead of
retrying it; source faults are reported distinctly from malformed requests.
The only image constructors remain test-only; no production source capability,
firmware payload, license grant, TFTP reply path, or hardware startup was added.

The focused TFTP suite passes 28 tests, including bounded `seek`/`rsize`
chunking, mutable-provider extent validation, and source-failure retirement.
The full
`platforms/sunfish/test-modem-qmi.py` regression passes, including AArch64
`no_std` and native-code generation checks; Rust formatting passes. The Pixel
had previously been visible in LineageOS over ADB. During this source-adapter
work, Fastboot was not enumerating, so no Fastboot command or radio-state
change was made; the later read-only Fastboot verification is recorded above.

### Device-local WLAN image source check (2026-10-23)

Read-only ADB checks report `/vendor` as ext4 (`stat -f` says `ext3/4`, and
`/proc/mounts` identifies ext4). An ordinary shell `stat` of
`/vendor/firmware/wlanmdsp.mbn` is denied, so this check read no firmware bytes
and does not confirm the file's current size or hash. The existing UFS
`UfsModemStorage` adapter is limited to the modemst1/modemst2/fsg/fsc GPT
partitions and has no production block provider; it is not a source for the
WLAN image. A device-local source would require an authorized firmware
capability, the dynamic vendor-partition mapping, and a bounded read-only ext4
file provider in addition to the missing UFS/SMMU authority. The image remains
unavailable to the native TFTP service.

The WLAN resource inventory validator passes. The separate candidate UFS
Microkit profile also compiles and passes the owner permission/alias/IRQ and
pool-reservation checks, but the profile maps only UFS HCI/PHY/ICE MMIO and
IRQs: it has no DMA aperture or production `Gate`/`BlockIo` provider and is not
selected by the active owner. No image was booted.

### Live ext4 metadata-only path validation (2026-10-23)

The bounded Rust parser in `rust/src/sunfish_vendor_ext4.rs` was run against
the live `/dev/block/dm-1` view while LineageOS was running. Its read adapter
allowed metadata, extent-tree, and directory-block reads but refused any range
overlapping the six file-data extents recorded from the inode. The parser
resolved `/vendor/firmware/wlanmdsp.mbn`, validated the live superblock,
directory entries, group-descriptor CRCs, inode and extent tree, and returned
the metadata file length of 3,751,004 bytes. It read zero firmware payload
bytes and did not calculate a content hash. The synthetic ext4 suite also
passes, including sparse-range reads through `ReadSourceCell`.

ADB was returned to `uid=2000(shell)` after the temporary root metadata check;
`sys.boot_completed=1`, device `sunfish`, and Wi-Fi setting `1` were confirmed.
The device stayed in LineageOS. This validates the parser against the Android
block view only. It does not parse the `super` partition's logical-partition
metadata, establish a trusted raw UFS source, verify AVB/dm-verity, or supply a
production UFS BlockIo/SMMU lease. AOSP documents that dynamic-partition
metadata in `super` maps logical partition names and ranges to dm-linear
devices ([AOSP dynamic-partition implementation](https://source.android.com/docs/core/ota/dynamic_partitions/implement)); that mapping and production storage authority remain future work. The community Sunfish firmware repositories also still lack a declared license grant, and no firmware has been loaded.

### Sunfish logical-partition metadata parser (2026-10-23)

`rust/src/sunfish_lpd.rs` implements a fixed-memory, read-only parser for the
LP metadata versions observed on this Pixel. It verifies SHA-256 for geometry,
headers, and table bytes, checks A/B slot selection, table bounds, block-device
size, and linear/zero extent ranges, then maps only the selected vendor
partition through `ReadSourceIo`. The target name accepts the observed
`vendor_a` or `vendor_b`; unrelated snapshot COW names such as `vendor_a-cow`
are parsed but never selected. It does not read partition payload while
opening the mapping.

`adb shell lpdump --slot=0` reports metadata version 10.1, a 65,536-byte
metadata maximum, three metadata slots, current slot `_a`, and `vendor_a` with
1,344,728 logical sectors across three linear extents. The reported `super`
size is 9,755,951,104 bytes. A direct metadata-only `dd` attempt from the ADB
shell is denied by device-node permissions, so the Rust parser has only been
tested with synthetic LP images, not live raw bytes. The nine focused tests
pass, including the streaming SHA-256 vectors and extent-bound checks. These
checksums detect malformed metadata; they do not authenticate it. A production
source still needs the selected `super` mapping, verified storage ownership,
AVB/dm-verity integration, and a UFS `BlockIo`/SMMU lease before any firmware
payload access. The format layout and fallback locations are based on AOSP
`liblp`'s [metadata format](https://android.googlesource.com/platform/system/core/+/refs/heads/main/fs_mgr/liblp/include/liblp/metadata_format.h),
[reader](https://android.googlesource.com/platform/system/core/+/refs/heads/main/fs_mgr/liblp/reader.cpp),
and [offset helpers](https://android.googlesource.com/platform/system/core/+/f4cd54e51290e011bfbe9786368da42868d30844/fs_mgr/liblp/utility.cpp).

### Read-only UFS-to-vendor file source (2026-10-23)

The paired GPT validator now requires and returns the exact `super` extent in
addition to the four modem-storage partitions. `PartitionMap` checks its
bounds and separation from those modem partitions. `UfsModemStorage::super_source`
then exposes a borrow-scoped, read-only byte source for the verified extent.
Every nonempty read is split into sector-aligned UFS operations of at most
4,096 bytes, with the existing fresh-boot/exclusive/device/GPT/SMMU lease
rechecked before and after each operation. An uncertain read or changed lease
permanently disables this source for that provider. No write API is exposed.

`rust/src/sunfish_vendor_wlan_source.rs` now composes a `super` source with the
caller-selected active vendor slot and the exact ext4 path
`/vendor/firmware/wlanmdsp.mbn`. Its open operation parses LP and ext4 metadata
only; callers read file ranges through the existing bounded `ReadSourceIo`
interface. The slot remains explicit caller evidence, and this chain does not
authenticate or authorize the firmware.

Synthetic integration tests now traverse LP metadata, vendor ext4 metadata,
and the exact file source, verify open does not read file payload extents, and
then read the file through the bounded interface. The GPT and UFS storage
suites pass 24 tests; the LP suite passes nine, including the composed path.
The production UFS `Gate`/`BlockIo` provider is still absent, and this chain
has not read `super` metadata or firmware from the Pixel. No device command,
partition write, device firmware read, or radio-state change was part of this
implementation.

When the user reported Fastboot ready, host enumeration still showed the Pixel
as `sunfish` with Android boot complete over ADB; `fastboot devices -l` was
empty. It was left untouched, and no Fastboot operation was issued.

The next read-only source check ran `lpdump --slot=0` on the current LineageOS
boot. It reports metadata v10.1 (1,832 table bytes, 65,536-byte maximum, three
slots), active slot `_a`, and a 688,500,736-byte `vendor_a` mapping with three
linear extents at super sectors 2,097,152, 8,358,144, and 8,393,984. The
reported super block device is 9,755,951,104 bytes. An opt-in
`parses_live_super_metadata_prefix` host test is prepared to accept exactly a
512 KiB prefix plus the independently observed full device length. The prefix
contains the primary and backup metadata slots; the Rust parser would read the
backup if its primary-copy fallback is needed and will not read vendor payload.
The current Android shell can run `lpdump` but cannot read the raw super block
device (`blockdev` reports permission denied), so no raw prefix was captured
and that test remains ignored. The default LP suite passes nine tests; no
Fastboot command, firmware read, or radio-state change was used.

### Candidate UFS backing-pool allocation audit (2026-10-23)

The separate offline UFS profile now asks Microkit to allocate a 16 MiB pool
as 4 KiB frames, assign the physical base to the ELF symbol
`ufs_dma_pool_paddr`, and map it uncached into the candidate owner. The build
generated 4,096 contiguous frames at `0x88f00000` for this particular build;
the address is build-selected, not pinned in the manifest. The CapDL audit
verified one owner mapping per frame and no page-table aliases. The range is
within RAM and overlaps none of the 24 fixed reserved-memory ranges in either
the Microkit kernel DTB or the captured live Pixel DTB. Both DTBs also contain
seven dynamically sized reservations without physical addresses, which this
check cannot compare against.

The candidate profile builds offline and passes its focused manifest/CapDL
tests. The runtime-side `DmaBackingPool` token now checks the injected virtual
base, aligned build-selected physical base, exact 16 MiB size, address bounds,
and per-page extent before exposing backing page addresses. It deliberately
does not construct an IOVA or claim a live translation. The active owner
profile is unchanged. This pool supplies contiguous
CPU-addressable backing RAM only: no SMMU route is installed, no device DMA,
UFS start, secure memory assignment, firmware read, boot, or flash occurred.
The result is recorded in `platforms/sunfish/validate-ufs-dma-pool.py` and the
build remains opt-in through `platforms/sunfish/build-modem-ufs-profile-offline.sh`.

The safe core now has `fill_ufs_dma_backing_mappings` and
`plan_ufs_dma_from_backing_pool`: they derive an exact 4 KiB IOVA-to-physical
map from the validated pool token and compose it with the exact UFS route and
LPAE plan. They require a page-aligned, exclusive IOVA aperture exactly the
pool's size, and the full mapping slice up front; route and overflow checks
happen before entries are changed. The 46-test host harness covers all 4,096
pool pages, full route/LPAE composition, mismatched or unproven aperture
rejection, overflow without a partial map, and an invalid WLAN route without
changing the buffer. The Sunfish validation runner includes this test, and
its full host/AArch64 suite passed after the composition change. This closes
the software gap from pool geometry through SMMU planning; it does not
allocate or reserve an IOVA range, install page tables or a stream route,
provide live ownership evidence, perform cache maintenance, or start DMA. No
phone test was run for this change.

The opt-in UFS owner profile now allocates eleven additional uncached 4 KiB
frames for the SMMU page tables: TTBR0 root, L2, eight L3 pages for an aligned
16 MiB aperture, and TTBR1 root. The separate `UfsSmmuTablePool` token exposes
those page identities, and `plan_ufs_dma_from_profile_pools` rejects table
storage that overlaps the DMA-visible pool before it fills mappings. The
owner manifest and CapDL audits verify the 11-page table run is contiguous,
mapped once, and separate from the 16 MiB pool; both ranges are also checked
against fixed reservations in the kernel and live device-tree blobs. The
current candidate build placed the DMA pool at `0x88f00000..0x89f00000` and
the tables at `0x80005000..0x80010000`; addresses are selected by Microkit,
not pinned. The expanded 47-test DMA harness, four pool-profile tests, and
owner-manifest CapDL tests pass. Seven dynamic DTB reservations still have no
fixed addresses to compare, and no SMMU route, table contents, live ownership,
UFS start, or device DMA has been exercised.

A separate `ufs-pool-probe` image now maps only the candidate pool and splash
framebuffer. Its startup validates the injected virtual/physical bases and
size, then writes, reads back, and restores one word on each of the 4,096
freshly allocated pool pages. Its generated system grants no device MMIO,
IRQ, SMMU, SMC, modem, or WLAN capability. The profile authority validator
and CapDL/DTB pool audits pass. A temporary `fastboot boot` on `sunfish`, slot
`a`, produced a screen result of `PROFILE VALID`, virtual base `0x65000000`,
physical base `0x8035d000`, size `0x01000000`, `0x1000` pages checked, and
`PAGE RW PASS`. The booted image was 18,800,640 bytes with SHA-256
`32ef6b4be0abe77412e391d299b08e1db1b83ee57872e135454994b2a78c68bb`. This
confirms CPU read/write access to each page in that build-selected pool and
restores the tested word; it does not confirm any UFS or SMMU operation. The
profile had no device MMIO, IRQ, SMMU, SMC, modem, or WLAN capability, and no
partition was flashed. The memory audit still cannot compare seven dynamic
DTB reservations that have no fixed address.

After this successful-path test, the probe added a `dsb sy` after each restore
so an early mismatch also completes its restoration before returning. The
updated source builds and passes the profile and CapDL/DTB audits; its rebuilt
18,800,640-byte image has SHA-256
`96962e8524037e9d0101f7951423fc7fe1e069e806fbaad4a8d63cdab071b326` and has
not been booted. No UFS MMIO, DMA, SMMU route, secure ownership, firmware read,
or WLAN start was exercised.

### Third paired LineageOS Wi-Fi bus-trace comparison (2026-10-23)

After Fastboot identified `sunfish` on slot `_a`, the phone was returned to
installed LineageOS with `fastboot reboot`; no custom image or partition was
written. A runtime-only capture enabled the existing `update_request_adhoc`
and `devfreq_devbw::set_bw` dynamic-debug callsites, recorded a 30-second
connected baseline, held Wi-Fi off for about 30 seconds, then observed Wi-Fi
reconnect and a further 30-second connected window. Cleanup disabled the two
debug flags, restored Wi-Fi to connected, and returned ADB to
`uid=2000(shell)`.

The normalized trace has 9,392 request updates, 9,126 devbw samples, zero bus
error or failed-path lines, and no update lines without timestamps. The
connected baseline had no updates from the shared `ipa` client (handle 10).
The Wi-Fi-off window had four `0→1→0` IPA pairs, all within 1.24 seconds after
Wi-Fi-off was confirmed; there were no further IPA updates during the rest of
the off interval. The earlier 60-second comparison and first 30-second repeat
each had five off-phase pairs, so the count is not stable. During re-enable,
four more pairs occurred around reconnection; the final `1→0` landed about
0.04 seconds after Android reported connected. The first 30-second repeat saw
the same four-pair reconnect count, while the earlier 60-second pass saw only
one pair shortly after reconnect.

The runtime stream began after client registration. Handle names were joined
from the saved Sunfish registration snapshot; handle 10's four-path `ipa`
signature matched every update. Eighteen updates for handle 20 had a
path-count mismatch (`grp3d`: snapshot one path, update record two), so those
names remain flagged as unresolved. CPU/DDR devbw activity continued in every
window. The timing remains correlation with a shared IPA bus client, not
WLAN-specific attribution or native seL4 Wi-Fi evidence. The selected trace,
redacted markers, and report are under
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-repeat-20261024-012626/`;
their normalized trace SHA-256 is
`a6ae190072f73ed780ea8e0e8657022e9ecc92a165ea30c745b61d4b1ae9d721`.

### Fourth paired LineageOS Wi-Fi bus-trace comparison (2026-10-24)

With the Pixel unlocked in LineageOS, the same runtime-only trace captured a
30-second connected baseline, about 30 seconds with Wi-Fi disabled, the
enable-to-connected interval, and a further 30-second connected window. The
script restored Wi-Fi to connected and returned ADB to `uid=2000(shell)`; the
dynamic-debug flags were disabled after capture. The runtime registration
snapshot resolved all 57 logged route paths across 49 registered handles.

The normalized trace contains 10,949 request updates and 10,624 devbw samples.
There are no unknown handles, path-count mismatches, prior-index mismatches,
bus errors, or failed paths; seven unchanged-index notes have no client handle.
The connected baseline had no updates from shared `ipa` handle 10. The Wi-Fi-off
window had eight IPA updates, four `MIN→SVS2→MIN` pairs, all beginning about
1.24 seconds after Android confirmed Wi-Fi off. The 7.04-second enable-to-
connected interval had seven updates (four `MIN→SVS2`, three `SVS2→MIN`); the
30-second connected window then had one `SVS2→MIN` update within 0.01 seconds
of Android reporting connected. No additional IPA updates appeared during the
rest of that connected window.

This pass again ties transient shared-IPA bandwidth requests to Wi-Fi state
changes and reconnection, but the count varies across captures and does not
identify a WLAN-specific vote or establish native seL4 radio readiness. The
capture's registration snapshot, redacted markers, normalized trace, and window
report are under
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-repeat-20261024-211431/`;
the normalized trace SHA-256 is
`aff626161cf5b2a152e1a7018cb1b7be590d58ac841d909cf0d35e8a438715f3`.

### Sunfish UFS native-block geometry and read-only HCI probe (2026-10-23)

The current LineageOS boot reports a Micron `MT128GASAO4U21` UFS device,
revision `0302`, UFS 2.1, with six LUNs. The controller advertises 64-bit DMA
addresses and is active. The Linux block device reports 4096-byte logical
blocks. A read-only 32 KiB capture of the LUN's GPT prefix validates the
primary GPT header CRC and exact entry-array CRC. The table declares 15 entries
of 128 bytes (1,920 bytes total), and its all-zero disk GUID is retained as an
observed target quirk. The primary table names `modem_a` and `modem_b`, not the
modem NV partitions used by the separate QRTR storage API; it also places
`super` at native LBA 98,304 through 2,480,127. A follow-up read of the final
native LBA found a reciprocal backup header with a valid header CRC, but that
header points its entry array back to LBA 2, the primary array location. This is
before the last usable LBA (31,211,514), so it is not a valid backup-array
location. The four 4 KiB blocks after the last usable LBA and before the backup
header (LBAs 31,211,515–31,211,518) were also read; none matches the primary
entry array. The primary entries and the array read from the backup header's
declared LBA have matching CRCs, but they are the same primary copy. The paired
GPT verifier correctly rejects this layout as `BackupGeometry`; the parsed
`super` extent is not a paired-table proof and must not be used as one. UEFI
requires the backup array to sit after the last usable LBA and before its
header ([UEFI 2.10, GPT disk layout](https://uefi.org/specs/UEFI/2.10/05_GUID_Partition_Table_Format.html)).
A separate 512 KiB read of the `super` metadata prefix passes the existing live
LP-metadata parser for active slot `_a`; no firmware payload bytes were read.
The temporary root-ADB daemon was returned to `uid=2000(shell)` after the
captures and verified. The final optional tail read briefly reset the ADB
transport; the Pixel reappeared in normal shell mode. No device writes occurred.

This exposed a real offline implementation mismatch: the UFS discovery,
READ(10)/WRITE(10), and GPT path assumed 512-byte device blocks, while the
actual Sunfish LUN reports 4096. The bounded codec now retains native block
size and capacity, GPT parsing accepts native 512- or 4096-byte LBAs, and the
modem API's 512-byte sectors are normalized from native GPT extents. A separate
bounded `super`-only GPT path accepts the observed short entry array and zero
disk GUID, while requiring the caller to bind the result to a trusted UFS LUN
identity; modem-NV parsing remains strict. The SCSI adapter performs bounded
reads across 4 KiB boundaries and read-modify-write for sub-block service
writes, preserving neighboring bytes. The host core now retains a typed CAP
decode, and the observed `0x1587031F` fixture verifies 32 transfer slots, 4
ready-to-transfer requests, 8 task slots, and the reported feature bits; this
is a parser check, not a new raw MMIO read. The full storage-host crate test
run passes 203 tests with 2 ignored, and the main Rust library's 590 tests also
pass.

A separate `ufs-hci-read-probe` image passed its authority, CapDL, and DTB
audits, then booted temporarily through Fastboot. It maps only the UFSHCI
register window read-only plus the splash framebuffer; it has no UFS IRQ, DMA,
SMMU, SMC, reset, or radio capability. The Pixel displayed `UFS HCI READ ONLY`
and `SNAPSHOT START` but never reached the result screen, so CAP/VER/HCS values
were not obtained from that bare probe. A read-only LineageOS sysfs snapshot
reports cached UFSHCI CAP `0x1587031F`, version `0x210`, 64-bit addressing,
32 transfer slots, 8 task-management slots, and 4 ready-to-transfer slots.
These are values cached by the Linux driver during probe, not a live read of
HCS; debugfs was not mounted, so no raw HCS snapshot is available. At capture,
Linux reported runtime-PM level 3 (device sleep, link Hibern8). The pinned
Lineage source at commit `28f9290ae067d3e7857126eb6312720dbe9c7c80` shows the
exact pre-CAP sequence. `ufshcd_hba_init()` enables the HBA regulator,
controller clocks, and standard UFS device rails, then invokes
`ufs_qcom_init()`. That variant init powers the PHY (including `vdda-phy`,
`vdda-pll`, PHY interface/reference clocks, and any configured reference-pad
rail), obtains lane-clock handles, applies the UFS bus vote, and runs post-clock
setup. Only after that returns does `ufshcd_init()` read CAP and version. The
captured UFS `clock-names` list includes both `tx_lane0_sync_clk` and
`rx_lane0_sync_clk`; the generic clock setup loops over every listed entry and
enables them before CAP/VER. Before that enable loop, `ufshcd_init_clocks()`
sets nonzero DT maximum rates: `core_clk` 200 MHz, `core_clk_unipro` 150 MHz,
and `core_clk_ice` 300 MHz; the other five listed host clocks have no rate
request in the captured table. The SDM Magpie GCC source puts all three rate
sources in the shared `vdd_cx` class and caps their nominal-corner rates at
those DT requests; the runtime DT resolves `vdd_cx` to the RPMh ARC resource
`cx.lvl`. The PHY auxiliary clock's source table only reaches 19.2 MHz at the
lower corner. The offline profile now records these source-derived corner
floors and their RPMh level encodings; it does not issue or observe a live
voltage vote. Qualcomm later calls its lane-clock enable
helper again from HCE enable, after reset/calibration/SerDes/PCS setup, before
ICE initialization. That later callback is not the first activation of those
sync clocks. PHY reset/calibration/deassertion, SerDes start, PCS-ready check,
and UniPro selection do occur after CAP/VER. The temporary seL4 probe omitted the
earlier regulator/clock/PHY-power/bus-vote sequence, so its stuck first read
demonstrates a missing prerequisite in the experiment, not a failed controller.
The source validator also pins the GCC rate-to-corner tables, the
VDD-to-RPMh-level mapping, and the runtime `cx.lvl` supply link.
The offline lifecycle model now separates that pre-CAP phase from the HCE phase:
its controller-power validation does not require IRQ binding or DMA/SMMU setup,
while post-CAP preparation reserves DMA and binds the IRQ before link startup.
The synthetic tests cover this split; no native authority implements the stages,
so this change does not yet provide a way to perform the prerequisite sequence
on the Pixel from seL4.
The live runtime DT identifies the eight controller clocks, three PHY clocks,
GCC/RPMh providers, HBA GDSC, four PMIC rails, two interconnect paths, and the
UFS PHY reset as GCC reset ID 3.
The offline platform map now distinguishes that GCC provider from reset ID 3;
its prior placeholder value 1 was incorrect. In this pinned Lineage source,
`ufs_qcom_update_sec_cfg()` is a no-op, so it adds no SMC prerequisite; actual
RPMh regulator/clock votes, interconnect votes, and PHY/SerDes sequencing still
need a native implementation. The image SHA-256 is
`34f1b9926494647b449ae7fc537959c43dd57ba7576ade32085eccd6f5f00569`. The
native UFS path still lacks a production resource lease and has not performed
a custom-OS SCSI transfer, paired GPT verification from seL4, firmware read,
or WLAN start.

### Shared Sunfish WLAN regulator consumer reducer (2026-10-24)

The pinned `bluetooth-power.c`, ICNSS source, and both captured DTBs establish
three regulator devices shared by WCN3990 Bluetooth and ICNSS: `ldoc1`
(crystal), `ldoc2` (core), and `ldoc10` (PA). The offline Rust module
`qcom_rpmh_vrm.rs` now models each client's never-started, enabled, and stopped
requests and combines their voltage intersection, enabled state, and load into
one regulator-device vote before it reaches RPMh aggregation. It preserves
other base-vote fields, keeps the system load explicit, and recalculates the
source-supported LPM/HPM mode on a consumer enable/disable or load transition.
This follows regulator-core behavior: before enabling, it recomputes DRMS even
when the consumer did not call `set_load`, summing all consumer loads and the
system load. The reducer preserves the prior mode only when neither modeled
consumer has a state/load transition. It also rejects voltage conflicts and
arithmetic overflow. The RPMh provider separately max-aggregates registers
among regulator devices associated with the same RPMh resource.

The resource validator now also enumerates `qcom,rpmh-vrm-regulator` nodes
grouped by `qcom,resource-name` in both captured DTBs. For each of these three
names it confirms exactly one regulator device and checks its exact path
against the manifest. This closes the Device Tree regulator-device grouping
inventory for these rails; it does not identify runtime consumers outside the
DT or capture the provider's live request cache.

The host regression runner and Sunfish resource validator pass, including the
captured runtime and generated kernel DTBs. The model still represents only
the two known device consumers on these rails; live consumer enable state,
system-load values, the complete Apps RSC request cache, and any non-DT clients
remain unverified. It is not connected to the hardware broker and authorizes no
RPMh writes. The Pixel is currently restarting; the most recent host check
showed no ADB or Fastboot device, so no device test was attempted in this
iteration.

The reducer's 10,000 µA HPM threshold is now checked against the pinned
Sunfish board file `sdmmagpie-regulator.dtsi` as well as both captured DTBs.
This matters because the generic `atoll-regulator.dtsi` carries a different
`<0 1>` value; it is not the active Sunfish board definition. The validator
pins the SDM Magpie file hash and checks `ldoc1`, `ldoc2`, and `ldoc10`
individually. It also pins regulator-core and RPMh-provider source and checks
that enable triggers DRMS recomputation and the provider applies those
thresholds. This closes the source-provenance gap for mode calculation, not
the runtime load/enable/cache gaps above.

### Read-only SMMU ACTLR handoff snapshot candidate (2026-10-24)

The pinned Sunfish QSMMUv500 driver audit leaves the live context-bank ACTLR
unknown: the QCOM `qcom,actlr` table has no match for WLAN SIDs `0x240/0x241`,
but context-bank reuse does not reliably clear the stored ACTLR value. The
existing ABL probe already reads the complete stream-match table and all
reported context-bank state. It now also reads ACTLR at context-bank offset
`0x004`, includes ACTLR in route rows, and reports the nonzero-bank count and
the first 64 banks as a bitmap. The Apps SMMU mapping remains read-only; the
image has no SMC or reset-PD capability and contains no SMMU write path.

The temporary candidate
`.cache/pixel-sunfish/smmu-read-probe/fastboot.img` is 18,644,992 bytes with
SHA-256
`6b7964b9d2a98f6cf2d19989b87d501f220dbe4a9abff38ed80eaa67b029c2b7`. Its
CapDL build reports 313 root objects and a 2.71 MiB loader. This rebuild adds
visible framebuffer checkpoints before the first SMMU read and after the
global, stream-route, and context-bank scans, so an early stop can be localized
from the phone screen. The Apps SMMU window remains read-only; only the existing
splash framebuffer is writable. All 20 SMMU handoff tests pass, including the
checkpoint-order and capability checks, as do the four WLAN ACTLR source-policy
tests and recorded-route/source/DT resource validators. At this checkpoint
ADB and Fastboot enumerate no device, so this candidate has not been booted.
Even a successful snapshot would describe the ABL handoff only; it would not
prove ownership of a free CB or establish a safe ACTLR policy for the native
translated WLAN context.

### QCACLD native-Wi-Fi protocol self-check and TX descriptor (2026-10-24)

The latest temporary protocol screen reports `CE DESC PASS`, `QRTR PASS`,
`IPA CODEC PASS`, `WMI 10 4 PASS`, `HTC WMI TX PASS`, `SCAN FRAME PASS`, and
`QCACLD INIT PASS`, ending with `NO RADIO ACCESS`. The corrected probe was
started with `fastboot boot`; it did not flash a partition or start WLAN. This
confirms those local codec/self-check paths only. The Pixel has since returned
to LineageOS; ADB identifies `sunfish` and reports `sys.boot_completed=1`.

The next offline implementation slice follows the actual phone-side QCACLD
path, rather than the comparative ath10k path: pinned Sunfish QCACLD sets
HELIUMPLUS Wi-Fi IP 2.0, uses the 64-bit HTT MSDU descriptor, and downloads
standard data through the HTC data endpoint. `sunfish_qcacld_tx.rs` now encodes
the 20-byte native-Wi-Fi HTT descriptor and the separate 72-byte HELIUMPLUS
fragment table for one contiguous 802.11 data frame. It validates descriptor
fields and the source-defined low-32-bit fragment-table and 37-bit frame IOVA
apertures. The validator pins the LineageOS kernel snapshot and ABI source
hashes; six host tests cover the exact words, the 37-bit boundary, truncation,
frame type, and input limits. No DMA map, cache operation, HTC data frame, CE
publication, or WLAN access is implemented by this codec.

The data service is `HTT_DATA_MSG_SVC` `0x0300`; QCACLD connects its HTC
endpoint from the service response and the Adrastea map routes TX on CE4 and RX
on CE1. WMI control remains a separate `0x0100` service on CE3/CE2. The
dynamically negotiated endpoint, HTT descriptor/table, and frame buffer are
now connected to a guarded, host-tested CE4 publish/reclaim adapter. That
adapter still has no production Gate and has not run on the phone. The project
also lacks CE1 service dispatch/interrupt integration and integration with the
separate HTT MSDU address ring, the initial WLAN protection-domain launcher,
trusted firmware source, complete IPA and SERVREG providers, translated SMMU
ownership, and live resource/power leases; no radio traffic has been sent.

The offline integration now includes `sunfish_qcacld_htc.rs`: it emits the
source-matched `HTT_DATA_MSG_SVC` connect request with HTC credit flow disabled,
binds only the target-assigned endpoint from a validated response, and frames
caller-selected HTT bytes with the dynamic endpoint and HTC sequence field.
`sunfish_qcacld_ce_tx.rs` composes that header with the 20-byte native-Wi-Fi
HTT descriptor and the CE4 two-descriptor gather layout. For the pinned Sunfish
SNOC compact-TX configuration, QCACLD derives a 92-byte maximum download
(28-byte HTC/HTT host descriptor plus a 64-byte frame prefix), and the planner
now derives that limit from the pinned config and source ABI rather than
accepting a caller-selected length. Short frames are clipped to their actual
length. The planner validates the CE4 ring and IOVA ranges against a bounded
pool and reserves both descriptor slots as one batch.

`qcom_ce_v3_native_io.rs` now also has a guarded CE4 publish/reclaim adapter.
Before publication it checks the live HTT route and negotiated endpoint against
the current device generation, verifies CE4 ring geometry and cursor state,
and resolves the prefix, frame, fragment table, and descriptor slots inside the
current exclusive uncached DMA mapping. It stages the complete frame, fragment
table, HTC/HTT prefix, and both gather descriptors before one CE4 producer-index
write. Reclaim follows the live hardware read index. Host tests cover successful
staging/publication, partial and complete gather reclaim, and rejection of stale
endpoint or ring state before any DMA write; the source validator pins the
compact-SNOC download formula alongside the gather format, endpoint, and CE4/CE1
route. The adapter has no production Gate wired to it and has not been run on
the phone. No radio traffic has been sent.

### QCACLD CE1 service receive transport (2026-10-24)

The offline transport now validates the pinned `HTT_DATA_MSG_SVC` route
(`0x0300`, target-to-host CE1, host-to-target CE4) and plans bounded receive
posts for its CE1 destination ring. The guarded native adapter can stage and
publish one zeroed receive buffer/descriptor, then poll and validate a bounded
completion before copying bytes and retiring the reservation. It checks the
current runtime generation, negotiated service route, live ring geometry,
cursor/ticket ownership, address bounds, and descriptor length before any
device-visible publication or completion copy.

The pinned QCACLD configuration constrains the host CE1 receive buffer to
2,048 bytes; source validation also checks the target CE1 inbound ring
configuration. Host tests cover route rejection, undersized buffers, stale
generation rejection without writes, post/completion ordering, payload copy,
and ownership retirement. The focused WMI/service planner suite passes 57
tests and the native CE transport suite passes 51. This adapter remains
poll-driven: it does not bind or acknowledge CE1 interrupts or refill/release
the separate 2,048-slot HTT MSDU buffer ring. It has no production Gate and
has not run on the phone. The WLAN protection-domain launcher, trusted
firmware, complete IPA/SERVREG providers, translated SMMU ownership, and live
resource/power leases are still missing; no radio traffic has been sent.

### QCACLD CE1 to HTT RX indication decoding (2026-10-24)

The receive path now has a bounded decoder from a completed CE1 HTC data frame
through the dynamically assigned HTT endpoint to QCACLD's low-latency
`HTT_RX_IND`. The HTC decoder checks endpoint, payload length, reserved bits,
supported flags, and target message-size limits while preserving the endpoint
sequence byte and credit-update request. The HTT decoder validates the pinned
48-byte indication header, peer and extended TID, flush/release sequence
ranges, firmware-descriptor span, four-byte MPDU-range alignment, range count,
and each range's MPDU count/status. It can also expand the range table into
per-MPDU metadata while preserving range/index information; only the pinned
status value `1` is classified as successful, and unknown status values remain
visible as raw bytes. This expansion does not pop or consume DMA buffers. The
decoder rejects unsupported TIDs and any truncated span before returning
borrowed slices.

The RX validator now checks this wire shape and range arithmetic against the
pinned LineageOS QCACLD source, including the driver's 19-entry extended-TID
limit and the RX status enum. Five focused tests cover a complete
HTC-to-HTT indication, MPDU range expansion, endpoint and frame errors,
malformed spans, sequence ranges, and an empty release-only indication. This
is still a parser, not a complete RX data path: no CE1 IRQ
service dispatch, RX_IND-to-address-ring cursor integration, MPDU reorder,
chain assembly, or packet delivery is wired. It has not run on the phone and
does not change the hardware or start WLAN.

### CE1 completion to QCACLD frame handoff (2026-10-24)

`sunfish_qcacld_rx_transport.rs` now composes one already-validated CE1
completion copy with the negotiated HTC endpoint and HTT RX indication
decoder. It limits parsing to the CE-reported completion length, rejects empty
or out-of-scratch-range lengths, and returns the CE completion together with
the decoded endpoint sequence, credit-update request, and borrowed HTT
indication. Transport and frame errors remain distinct; there is no retry or
second ring consumption in the wrapper.

The RX ownership session now also models one QCACLD MPDU drain. It follows the
source FIFO order, reads each MSDU-head descriptor, accounts for that
descriptor's ring-2 continuation count, and continues through `last_msdu`.
Every slot and descriptor in the group is checked before leases are changed;
missing terminators, chains beyond the posted window, and short lease outputs
leave the session untouched. A second bounded output now records each MSDU's
head descriptor and its span in the flat lease array, with capacity checked
for both outputs before ownership changes. This correctly keeps HTT MPDU
counts distinct from variable MSDU-ring buffer counts and drains error-status
MPDUs through the same descriptor-defined group boundary.

The RX validator checks the pinned `htt_rx_amsdu_pop_ll` pop/count/terminator
rules, and nine session tests cover multi-MSDU group boundaries, ring-2
continuations, undersized lease/chain outputs, and malformed/incomplete groups.
This is still a software ownership model: it does not synchronize DMA memory,
assemble or deliver packet chains, write the shared allocation index, or refill
the ring. No CE1 interrupt service, production Gate, or on-device WLAN run is
added.

The captured 68-word Lineage WMI INIT resource block sets `rx_decap_mode` to
`2`. The descriptor parser now reads the corresponding `msdu_start` decap
format, and `copy_ethernet_single_buffer` permits only 802.3-decapped payloads
and rejects firmware-discarded MSDUs before writing the caller's output. This
matches the existing sDDF Ethernet bridge's input contract. The copied bytes
are not yet passed to that bridge: actual HTT MSDU DMA ownership, packet-chain
assembly, bridge queue wiring, and a production runtime Gate remain unfinished.

The RX session now also exposes `copy_ethernet_msdu_chain` for one MSDU whose
head descriptor reports ring-2 continuation buffers. It follows the pinned
`htt_rx_amsdu_pop_ll()` layout: strip the standard descriptor reservation and
target-gated L3 padding from the first buffer, copy full intermediate
continuations from byte zero, and use the descriptor-sized prefix plus the
remaining reported bytes for the last continuation. The method rejects an
inconsistent continuation count/length, stale lease, undersized buffer, wrong
decapsulation, firmware discard, or short output before modifying output. The
caller keeps leases until network delivery or drop, then releases them
explicitly. Eleven focused RX-session tests pass, including padded one- and
two-continuation assembly and failure atomicity; the source validator now pins
the fragment-length expressions to the clean Lineage kernel snapshot. This is
still a host-side copy operation: it performs no DMA/cache synchronization,
does not call `sunfish_wlan_net_rx_copy`, and is not wired to a production RX
Gate or CE1 event loop. Native radio traffic remains unverified.

The HTT DMA plan now resolves the session's buffer IDs to its already-validated
initial-ring or fallback-reserve IOVA windows. Initial IDs are the posted ring
indices; fallback IDs begin at the initial fill level. The lease resolver asks
the RX session to validate the live lease generation before returning the IOVA,
so a released or stale lease cannot be mistaken for current ownership. Eight
ring tests and 78 SMMU-plan tests pass, including initial/fallback boundaries
and stale-lease rejection. This binds software identities to a plan only: it
does not prove backing-frame ownership, perform DMA/cache synchronization,
publish addresses, or grant device access. The bridge and production RX event
loop remain unwired.

### Fresh LineageOS WLAN reference boot trace (2026-10-24)

The Pixel remained in its normal LineageOS boot throughout this check. Read-only
ADB confirmed `sys.boot_completed=1`, product `sunfish`, and running
`tftp_server`, `pd_mapper`, and `cnss-daemon` services. The unprivileged ADB
shell has no `su`, so no vendor binary was read and no device state was changed.

Filtered boot logs show the platform `tftp_server` and `pd_mapper` starting at
10:37:40, the vendor TFTP service starting at 10:37:41, and the modem being
brought out of reset with its power/clock-ready interrupt at 10:37:44. At
10:37:47 the Service Notifier reports `msm/modem/wlan_pd` UP and ICNSS connects
to the WLFW QMI server. `cnss-daemon` starts at 10:37:49, after that UP
indication. ICNSS reports WLAN firmware ready (`0xd87`) at 10:37:54 and
`ueventd` loads the requested `WCNSS_qcom_cfg.ini` from the vendor firmware
tree. Later in the same boot, the WLAN driver reports association, the log
records EAPOL messages M1 through M4, and DHCP request/ACK traffic.

This gives the native bring-up work a fresh end-to-end Linux reference: modem
boot and the already-running TFTP/PD-mapper services precede `wlan_pd` UP;
ICNSS then reaches firmware-ready and the Android network stack completes
association and address configuration. Because `cnss-daemon` starts after
`wlan_pd` UP, this trace does not support it as the initial domain-launch
trigger. The proprietary modem/PD transition remains unresolved, and the trace
does not explain the earlier seL4 `PEER SMP2P FATAL` observations. The Pixel
remains on LineageOS; no reboot, Fastboot session, or radio mutation was part
of this check.

### RX indication firmware descriptor handling (2026-10-24)

The pinned Lineage QCACLD source copies the firmware-status byte for each MSDU
from the HTT RX indication into its descriptor ring before applying discard or
forward policy. The RX parser and ownership-session copy helpers now accept that
per-MSDU byte explicitly instead of trusting a byte in the DMA buffer; absent
tail bytes use the source driver's zero default. A focused test verifies that
indication bytes override differing DMA-buffer bytes and that the discard bit
prevents packet copy. The RX source validator checks the pinned copy and cursor
semantics and runs in `test-modem-qmi.py`.

This is host-tested parser/session behavior only. It does not provide DMA/cache
synchronization, packet delivery, ring refill, a production Gate, or a device
Wi-Fi test.

### RX status policy and Ethernet handoff (2026-10-24)

The pinned `htt_common.h` marks `HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER` as
EAPOL-only. The RX indication status now exposes that exact policy, and
`sunfish_qcacld_rx_delivery.rs` composes it with the per-MSDU firmware bytes,
the HTT buffer-group lease session, and a synchronous Ethernet sink. Successful
MPDUs can deliver Ethernet-decapped MSDUs; unauthenticated-peer MPDUs can pass
only EtherType `0x888e`; other MPDU statuses are dropped. The indication-level
adapter drains MPDUs in FIFO order and advances the firmware-byte cursor by the
actual number of MSDUs in each group. It releases all claimed leases after
delivery, filtering, or sink failure. Tests cover ordinary Ethernet delivery,
EAPOL-only A-MSDU delivery, mixed-status indication cursor progression, rejected
MPDU status, and lease cleanup when the sink reports backpressure. The source
validator pins the status value/text and checks the enforcement, cursor, and
cleanup paths.

This closes a host-side policy/copy handoff only. A synchronous Rust callback
adapter now targets the existing bounded sDDF bridge RX-copy entry point, and
the C callback contract is covered by host tests. The adapter is not yet wired
into a Sunfish composition or production Gate. The caller still must provide
the source-correct DMA operation behind the CPU-sync callback, handle HTT
indication/reorder ordering and ring refill, and provide a production CE1/RX
Gate. No native radio traffic has been tested.

The new RX pipeline now composes the existing CE1 completion decoder directly
with this indication delivery. Its CPU-sync variant synchronizes every
completed head and continuation buffer by ID before reading that group's
descriptor or payload. The pinned QCACLD source confirms its low-latency path
unmaps each completed buffer before descriptor access. Host tests verify FIFO
sync order, delivery only after sync, and that a failed sync leaves the group
unclaimed; the caller must quarantine the ring generation after an uncertain
sync. An integration test feeds a synthetic bounded HTC/HTT frame through
the CE1-copy callback, drains three RX_IND MPDUs against the FIFO buffer
groups, applies OK/EAPOL/discard policy, and observes the two accepted Ethernet
frames at the sink. The host runner
[`test-wlan-net-bridge-ffi.py`](../platforms/sunfish/test-wlan-net-bridge-ffi.py)
now drives that RX pipeline through `SddfWlanNetSink`, the actual C bridge, and
the pinned sDDF RX queues. It verifies the two accepted frames are copied and
published, sDDF coalesces the wakeup notification, buffer IDs are released in
order, and backpressure is returned through the Rust sink. This closes the
software RX-to-network-queue integration gap. It still uses synthetic CE
completion and CPU-sync callbacks; no complete runtime supplies live CE1,
MMIO, DMA, IRQ, or a production Gate, and no native radio traffic has been
tested.

The collecting pipeline variant now records the exact IDs released by every
MPDU in FIFO order. It checks MSDU, lease, and recycle-ID scratch capacity
against all posted buffers before consuming the CE1 completion, then returns a
bounded ID prefix after delivery. Once the borrowed CPU-buffer view ends, a
caller can submit that prefix to the existing fill-N ring refiller in one
publication. Host tests verify the IDs match the three delivered/dropped
groups and that undersized scratch is rejected before CE1 consumption. This
connects the software ownership stages; no live refiller or DMA provider is
installed.

### HTT-data CE1 receive interrupt gate (2026-10-24)

The native CE adapter now has distinct receive-interrupt arm paths for WMI
control on CE2 and HTT data on CE1. The CE1 path requires a same-generation
`HTT_DATA_MSG_SVC` route to CE1, a negotiated HTC endpoint, the exact Sunfish
CE1 GIC binding (`INTID 0x1bf`) with exclusive IRQ ownership, and a posted CE1
destination descriptor. Under the live CE owner gate it checks the CE1 ring
geometry, producer/read indices, and current interrupt-enable value before
changing only CE1's copy-complete enable register. Host tests verify successful
enable/disable and reject the wrong interrupt, a missing binding, or a stale
service-map generation without touching the register.

This only arms the device-side interrupt source. It does not claim or
acknowledge the seL4 IRQ capability or dispatch CE1 completions. The native CE
adapter now also exposes a guarded CE1 host-status acknowledgement for the
observed copy-complete W1C bit, gated on the same-generation HTT route,
negotiated endpoint, and exclusive CE1 IRQ binding. It must be called after
the caller services the receive completions; it deliberately leaves unrelated
watermark/error bits untouched. Host tests cover active-runtime authorization,
the exact W1C value, absent pending status, and rejection of a stale IRQ
binding without a status write. No seL4 IRQ capability, event dispatcher,
production Gate, or Microkit composition supplies the runtime route, endpoint,
DMA lease, or interrupt binding, and these paths have not run on the Pixel.

### QCACLD HTT RX ring refill publication (2026-10-24)

The native RX ring adapter now supports serialized one-buffer and batched
refills. It checks the device-visible allocation index against its expected
producer index, prevalidates every released buffer in the QCACLD ownership
session without copying its fixed-capacity table, validates all IDs and buffer
lengths, clears only each RX attention word, asks the platform provider to
synchronize each buffer for device access, and writes the corresponding 64-bit
ring entries. It then issues one DMA write barrier and publishes the final
allocation index once, matching QCACLD's fill-N path. The software session is
committed only after successful publication. If the index write returns an
error, hardware may already have observed it, so the refill state fails closed
until ring reinitialization. Host tests exercise batch wrap, one final index
publication, ordering, mismatch rejection, partial-batch failure, and ambiguous
index-write failure. A host integration test also carries a three-MPDU CE1
completion through CPU synchronization and delivery, collects the released
buffer IDs, then publishes those IDs in one wrapped refill transaction. The
fixed buffer pool binds each opaque ID to a distinct
backing-slice offset, but it cannot prove that the CPU mapping names the same
physical pages as the planned IOVAs; a production allocator must establish
that identity. The pinned QCACLD validator checks both source ordering and the
adapter transaction ordering.

This is a host-testable transaction boundary, not a production device path.
`NativeIo::refill_htt_rx_batch` now binds this transaction to a live Gate lease:
it holds one authority guard through index read, all buffer and ring writes,
the DMA barrier, and final index publication. It checks the negotiated HTT
route and endpoint, validates every planned IOVA against the bounded uncached
DMA aperture, and refuses pool slices whose virtual bases differ from the
mapped initial/fallback buffers. Its private ring IO uses volatile stores and
the final system barrier. Host tests cover the exact mapped backing and reject
an unrelated pool before reading or changing the ring. This still depends on
a production Gate to establish real seL4 ownership, SMMU mappings, and physical
page identity; no live CE1 completion dispatcher or Microkit composition
installs that Gate, and no ring refill has been performed on the Pixel.

### Generation-bound QCACLD WMI startup sequencing (2026-10-24)

The WMI receive path now retains each validated event's TLV body and transport
generation for a bounded startup-session gate. The gate follows the pinned
WMA order: `SERVICE_READY` validates the captured Sunfish firmware build, ABI,
and empty dynamic-memory request set; the service-available / service-ready
extension event makes an INIT candidate eligible; the caller must report
successful WMI_INIT publication; and only then can a successful, strict
`WMI_READY` advance the session. Duplicate, out-of-order, malformed, or stale
generation events leave the phase unchanged. Advancing to a strictly newer
generation resets the gate.

The pinned-source validator now checks the event routing and confirms that
WMA emits WMI_INIT from its service-ready extension handler and records READY
status in the READY handler. Fourteen host tests cover the normal sequence,
early READY, duplicate and out-of-order events, bad target evidence, and stale
generations. The full `test-modem-qmi.py` run passed, including AArch64
`no_std` and native code generation. This gate only tracks WMI control-plane
state: it does not compose or publish WMI_INIT, establish CE/HTC hardware
ownership, start WLAN firmware, or claim that the radio is operational.

### CE register authority narrowed to the Adrastea window (2026-10-24)

`qcom_ce_v3_native_io.rs` now maps only the twelve-page CE register aperture:
physical `0x18a40000` through `0x18a4bfff` (`0xc000` bytes), derived from the
ICNSS base and the source-matched CE0 register offset/stride. Its authority
observation and constructor require that exact physical range and length. The
register specification continues to use ICNSS-relative offsets, while each
native volatile access subtracts `CE0_OFFSET` and is bounded to the mapped CE
aperture. The generic publication, interrupt-control, and W1C paths use the
same aperture-relative translation. This removes the former requirement to
map the unrelated remainder of the 8 MiB ICNSS block into the future WLAN
owner.

Host tests cover the 48 KiB geometry, reject a broad 8 MiB mapping, and exercise
the CE setup, TX/RX posting and completion, CE1 interrupt, and HTT refill paths
through this subwindow. These remain mock-backed tests: the Microkit system
still has no Wi-Fi domain or production Gate, no live IRQ dispatch, SMMU
ownership is not established, and the Pixel has not run the changed adapter.

## Retaining QCACLD startup evidence across CE2 buffer reuse

The generation-bound WMI startup session now snapshots the accepted
SERVICE_READY fixed parameters, HAL capabilities, and service bitmap, plus the
SERVICE_AVAILABLE extension bitmap. These compact values are copied only
after the pinned Sunfish build/ABI, empty-memory-request, TLV, and segment
checks pass. Later protocol code can reconstruct validated borrowed views from
the owned snapshots after the CE2 receive scratch buffer has been reused; a
new transport generation clears both snapshots.

The QCACLD resource-policy test now builds the observed 68-word WMI INIT
resource block from those owned snapshots and still matches the two
LineageOS-captured blocks byte for byte (276-byte TLV, SHA-256
`10af52bc0cf7949180219bc741775757c7e97fa2bb5fbc87604e6c58faa34fc1`). The
full serialized host/AArch64 regression and pinned-source validator pass.
This closes the event-lifetime gap between CE2 service discovery and WMI INIT
resource construction; the following section extends it through CE3
publication orchestration.

### WMI_INIT composition from retained startup session (2026-10-24)

Added `sunfish_qcacld_wmi_startup::encode_sunfish_qcacld_wmi_init_frame_from_session`
to bridge the accepted CE2 startup session into the existing resource and HTC
encoders. It requires the active transport generation to match and the session
to be exactly at `InitReady`, then builds from the session's owned
SERVICE_READY and SERVICE_AVAILABLE snapshots. Stale generations and earlier
phases fail before encoding. The helper returns the existing resource
candidate, WMI body length, and HTC credit/frame plan. A companion function
composes that frame with `qcom_wmi_ce::plan_wmi_tx`, checking the selected
WMI_CONTROL CE3 route, transfer buffer, DMA aperture, ring plan, and descriptor
slot. It remains a preflight only: it does not reserve HTC credits, write DMA
memory, publish the producer index, or mark INIT as submitted. The session
advances only when the caller separately records confirmed submission.

The focused unit test overwrites both original CE2 event frames after the
startup events have been accepted, then verifies the owned-snapshot output is
byte-for-byte identical to direct encoding from the original event contents.
Separate checks reject stale-generation and pre-SERVICE_READY calls. The
`publish_sunfish_qcacld_wmi_init` now reserves the frame's HTC credits before
calling `NativeIo::publish_wmi_tx` through its live Gate. A definite
pre-publication failure cancels the reservation; `TxPublishError::Uncertain`
retains the credits and latches the startup session in
`InitSubmissionUncertain` until a new generation; confirmed publication alone
advances the session to `AwaitReady`. Four focused tests exercise success,
definite failure, uncertain outcome, and stale-generation rejection without
hardware I/O. The serialized `test-modem-qmi.py` runner passed after the code
change, including the standalone resource-policy harness and AArch64
`no_std`/native code-generation checks. This completes the source-level
WMI_INIT-to-CE3 transaction boundary, but no production WLAN Gate is installed
in a Microkit system, no live CE2 IRQ dispatcher is present, and exclusive
SMMU/DMA ownership and firmware launch remain unresolved. No device state was
changed for this software-only step.

### Current LineageOS WLAN startup log cross-check (2026-10-24)

With the Pixel returned to LineageOS 23.2, a read-only rooted ADB log capture
confirmed the running kernel is `4.14.357-openela-g28f9290ae067`, matching the
pinned source commit used by the QCACLD validators. The current boot log shows
the SNOC HIF waking from CE2, WMI attach success, target-ready resources, and
`WMI_READY` status 0. The active target reports firmware build `0x3226822e`,
ABI words `1, 0, 0x5f414351, 0x4c4d, 0, 0`, hardware version `0x40090000`, and
board version 18; ICNSS reports WLAN firmware ready. Android also confirms
`WCNSS_qcom_cfg.ini` resolves to
`/vendor/firmware/wlan/qca_cld/WCNSS_qcom_cfg.ini`.

This corroborates the existing WMI INIT capture against the exact currently
running Lineage kernel and confirms the stock driver/firmware can complete its
control-plane startup. It does not identify or authorize the private initial
`wlan_pd` image launcher, prove that the current WLAN firmware bytes are
redistributable, or provide seL4 CE/SMMU/IRQ authority. Root access was used
only to read logs; `adbd` was returned to non-root, and the phone was not
rebooted or otherwise changed.

### Guarded CE2 WMI interrupt acknowledgement (2026-10-24)

`NativeIo::acknowledge_wmi_rx_ce2_copy_complete` now clears only CE2's
observed copy-complete W1C status after the caller drains the WMI receive
completions. It requires a live same-generation WMI_CONTROL route and the
exact exclusive CE2 GIC binding (`INTID 0x1c0`); it does not acknowledge the
seL4 GIC capability. The existing guarded status writer also rejects an
unobserved bit, and the operation leaves unrelated host-status bits untouched.

The native CE suite passes all 123 tests, including the new CE2 checks for
status-mask isolation, missing pending status, wrong IRQ, nonexclusive IRQ
binding, and stale WMI route. The serialized Sunfish host/AArch64 regression
was rerun after this change. This adds the device-status acknowledgement
primitive, but the seL4 IRQ capability handler, CE2 completion dispatcher,
production Gate, and Wi-Fi system composition are still not installed; no
device state or radio path was changed.

### Bounded native CE2 WMI interrupt service (2026-10-24)

Added a safe adapter composition in `sunfish_qcacld_wmi_rx_native.rs` that
masks CE2, drains and dispatches completed WMI events through the existing
credit/startup pipeline, acknowledges the CE2 copy-complete status only after
an empty poll, then re-enables CE2 only while receive buffers remain posted.
The drain is budgeted: when the budget expires it leaves CE2 masked and the
device status unacknowledged, returning a deferred-drain indication. Consume,
dispatch, or acknowledgement failures also leave the interrupt masked for
recovery. This function does not acknowledge the seL4 GIC capability or
schedule deferred work.

Runtime CE publish/post/consume and interrupt-mask operations now validate a
live same-generation owner, power/reset state, exact mappings, and translated
SMMU route without requiring CE quiescence; IRQ operations additionally check
the exact exclusive binding. Setup-only operations still require quiescence.
The mock-native integration test runs the CE2 service with the engine active,
verifies event dispatch and empty-ring acknowledgement, and confirms re-arming
with a posted buffer. The CE native suite passes all 148 tests, and
`test-modem-qmi.py` passes its serialized host, source, `no_std`, and AArch64
native-codegen checks with an explicit compile entry for this adapter. No
production Gate, IRQ capability/dispatcher, or Microkit Wi-Fi composition is
wired, and the Pixel was not touched.

### Rooted sysfs recheck of the current Lineage subsystem boundary (2026-10-24)

A read-only rooted ADB check found no generic `/sys/class/remoteproc` tree. The
`/sys/bus/msm_subsys/devices` inventory exposes subsystem IDs 0 through 6,
with the modem at ID 2 and IPA firmware services at ID 5; it exposes no
separate WLAN subsystem device. The modem's current control attributes report
`firmware=modem`, `state=ONLINE`, `restart_level=RELATED`, and
`crash_count=0`. ICNSS has no `subsys` child in its platform-device sysfs
directory. This supports the model that `wlan_pd` is not an APSS-managed
remoteproc, but does not prove which modem-side or secure service launches it.

The filtered kernel log shows ICNSS loading
`/vendor/firmware/wlan/qca_cld/WCNSS_qcom_cfg.ini`; WMA reports firmware build
`0x3226822e` at kernel uptimes 46.259 s and 50.838 s, with a firmware deinit
between them. No `wlan_pd`, TFTP, or modem-firmware request line appeared in
the retained filter, so this does not identify the cold-start trigger. The
inspection made no WLAN, reset, firmware, SMMU, or partition changes. After
collection, `adbd` was returned to shell UID 2000 and `sys.boot_completed`
remained `1`.

### Microkit continuation for bounded CE2 WMI IRQ service (2026-10-24)

Added `sunfish_qcacld_wmi_irq_runtime.rs` and the target-only
`sunfish_qcacld_wmi_irq_microkit.rs` adapter. The handler treats a budgeted
CE2 drain as one scheduling slice: if work remains, it signals a same-PD
loopback notification and leaves the hardware IRQ masked; after a later slice
reports that CE2 status was acknowledged, it ACKs the GIC capability exactly
once. Coalesced IRQ/continuation notifications run one slice, late
continuations after ACK are inert, and service or GIC-ACK errors stop without
releasing the pending interrupt.

The adapter overrides the stock Microkit handler loop because that loop ACKs
all configured IRQs before its first receive. The WLAN owner must run active,
initialize its device-side service before entering the loop, and configure a
same-PD channel such as output ID 2 to input ID 3 for continuations. Seven host
tests cover bounded continuation, final ACK ordering, coalesced signals, stale
kicks, inconsistent native drain reports, and fail-closed errors. The pinned
Sunfish Microkit tool accepted and built the same-PD channel, and an AArch64
seL4 target check compiled the adapter. The adapter accepts the native CE2
drain report directly and refuses mismatched `irq_acknowledged` /
`needs_deferred_drain` flags before it can ACK the GIC.

This closes the generic IRQ scheduling gap only. The handler is not installed
in a WLAN protection domain, no live CE2 Gate or SMMU/DMA ownership provider
exists, and the unresolved initial `wlan_pd` launch path and authorized
firmware input still prevent a radio start. No Pixel state was changed for
this software-only work.

### Native CE2 drain bound to the Microkit IRQ handler (2026-10-24)

`sunfish_qcacld_wmi_irq_microkit.rs` now provides `NativeWmiCe2Drain`, a
borrowed adapter that implements the handler's budgeted-service interface by
calling the existing `service_native_wmi_ce2_irq`. It keeps the CE NativeIo,
generation, profile/rings/cursor, receive ledger, HTC credits, outstanding
management requests, startup session, event scratch, and bounded completion
limit together for the lifetime of the Microkit handler. Its event dispatcher
is invoked only after the native receive path consumes and validates the WMI
event. The CE service error is now printable by the fail-stop handler without
discarding its typed cause.

This connects the actual CE2 drain implementation to the continuation loop;
the caller still must construct `NativeIo` from a valid exclusive CE Gate,
provide the device-side dispatcher, and own the matching IRQ capability. The
serialized `test-modem-qmi.py` Sunfish regression passes, and
`cargo +nightly-2026-09-25 check --locked --offline` for the AArch64 seL4
Microkit target compiles the target-only adapter. No production Gate, Wi-Fi
Microkit system, SMMU/DMA authority, or modem firmware launch path exists yet.
The Pixel remains in LineageOS with stock WLAN and Wi-Fi connected.

### Read-only vendor WLAN service-launch inspection (2026-10-24)

With rooted ADB, the installed `/vendor/etc/init/hw/init.sm7150.rc` confirms
that `tftp_server`, `modem_svc -q`, and `pd-mapper` are started as separate
services. The on-device `modemuw.jsn` registers `msm/modem/wlan_pd`, instance
180, with `kernel/elf_loader`, `tms/servreg`, and `wlan/fw`; this matches the
existing local Sunfish service map. The installed `modem_svc` binary's strings
show `-q/--qmi_server` and `-s/--modemstat`, with its QMI server and modem-stat
paths, but no evident WLAN-PD launch interface. The `pd-mapper` binary exposes
the Servloc QMI service and JSON `sr_service` parsing. This narrows the AP-side
roles to locator publication and the declared providers; opaque modem firmware
or an unobserved service can still initiate the first PD load, so the actual
cold-start trigger remains unproven.

The check streamed binary strings without saving the vendor executables and
read only the existing configuration. adbd was returned to UID 2000; no
firmware, modem, WLAN, reset, or partition state was changed.

### Public Sunfish SMMU authority source audit (2026-10-24)

The public-source audit checked Google's Sunfish kernel configuration, the
LineageOS 4.14 SM6150 device-tree and ICNSS implementation, and Qualcomm's
published Access Control guide:

* Google's sunfish_defconfig enables ARM_SMMU, ICNSS, ICNSS_QMI, and
  QCOM_SMCINVOKE. These are Linux build options; they do not grant another
  kernel or a seL4 protection domain the same authority:
  https://android.googlesource.com/kernel/msm/+/ae4f69de891bc8b8c4c673abe298646e6c15d06a/arch/arm64/configs/sunfish_defconfig
* The shared SM6150 device tree declares an ICNSS Apps-SMMU attachment and
  stage-1-bypass property. The matching ICNSS driver creates and attaches a
  Linux DMA-IOMMU mapping, selecting bypass when that property is set:
  https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/lineage-23.2/arch/arm64/boot/dts/qcom/sm6150.dtsi
  https://github.com/LineageOS/android_kernel_google_msm-4.14/blob/lineage-23.2/drivers/soc/qcom/icnss.c
  The installed Sunfish runtime-FDT capture independently records its
  ICNSS S1-bypass setting. Neither Linux configuration describes a
  translated seL4 context or transfers route ownership.
* The ICNSS source separately calls hyp_assign_phys for MSA-region permission
  changes. That is an ICNSS memory-permission operation, not a documented
  general API for EL1/seL4 to claim Apps-SMMU stream-match slots or context
  banks.
* Qualcomm's public Access Control guide describes external protection units
  controlling access to registers and memory regions, but the public material
  found does not specify a Sunfish boot-chain handoff or supported API that
  grants EL1/seL4 ownership of the Apps-SMMU route table and context banks:
  https://docs.qualcomm.com/bundle/publicresource/topics/80-70014-11/access-control.html

No public Sunfish-specific ownership contract was found. The current route
inventory and ABL snapshots remain observations, not a delegation token.
Accordingly, the native CE/SMMU Gate still has no valid production
constructor; register programming, translated DMA, and radio startup remain
disabled. Continue implementing and validating the software path, but require
an explicit platform handoff contract or equivalent owner-controlled
capability proof before wiring that Gate.

### SERVICE_AVAILABLE now drives the guarded WMI_INIT publication path (2026-10-24)

The native CE2 receive service now exposes a post-observation action context
with its live `NativeIo`, generation, WMI session, HTC credit ledger, and ring
plans. The Microkit dispatcher can use that hook after the event has been
copied, validated, and accepted by the startup state machine, but before CE2
status is acknowledged. `publish_sunfish_qcacld_wmi_init_after_observe()` uses
the accepted owned SERVICE_READY/SERVICE_AVAILABLE snapshots, the actual
endpoint credit geometry, a caller-owned CE3 cursor, and caller-selected
resource/route/buffer inputs to plan and publish WMI_INIT. An action or
publication failure therefore leaves CE2 fail-closed; uncertain CE3
publication quarantines the startup generation using the existing publisher
policy.

The native mock-hardware integration test feeds SERVICE_AVAILABLE through the
CE2 completion path, publishes the 324-byte WMI_INIT HTC frame through CE3,
checks the exact DMA copy and producer index, confirms credits are reserved
and the session reaches AwaitReady, then verifies CE2 acknowledgement. The
test deliberately supplies mock route and memory authority; it does not
establish a production Gate or authorize physical hardware. The full
serialized `platforms/sunfish/test-modem-qmi.py` runner passes, including
the 152-test native CE suite, AArch64 `no_std` checks, and host integration
tests. A separate AArch64 seL4 Microkit target check compiles the target-only
dispatcher. No Pixel test was run; the phone remains on LineageOS.

This closes the software join between SERVICE_AVAILABLE and WMI_INIT only.
The launcher/provider for the initial `wlan_pd`, compatible authorized
firmware, a production CE/IRQ/SMMU authority provider, and the WLAN Microkit
system are still absent. It does not establish post-WMI_INIT WMI_READY on real
hardware or start the radio.

### HTC control connection for the initial HTT endpoint (2026-10-24)

The pinned QCACLD attach order places the first `HTT_DATA_MSG_SVC`
(`0x0300`) connection before WMI pre-enable and its INIT/READY exchange.
`wlan_hdd_main.c` calls `cds_dp_open()` before `cds_pre_enable()`;
`cds_dp_open()` attaches the datapath, and `ol_txrx_pdev_attach()` allocates
HTT. `htt_pdev_alloc()` calls `htt_htc_attach_all()` before attaching its RX
path, and `htt_htc_attach()` issues `htc_connect_service()` for
`HTT_DATA_MSG_SVC`. The source's reserved HTC control service is service ID
`0x0001`; the Adrastea service map routes its host-to-target control traffic
through CE0 and target-to-host through CE2. The separate HTT data service is
routed through CE4/CE1. The source validator now checks this call order, IDs,
and route against pinned commit `28f9290ae067d3e7857126eb6312720dbe9c7c80`.

`HttStartupSession` in
[`sunfish_qcacld_htc.rs`](../rust/src/sunfish_qcacld_htc.rs) models this
transaction: stage the bounded control request, confirm transport submission,
then accept a same-generation response and bind only its validated dynamic
endpoint. Wrong generations, invalid phases, wrong-service responses, and
invalid endpoints leave the session unbound. The native CE adapter now has a
CE0 publisher that accepts only the exact staged HTT connect frame and checks
the live generation-bound CE0/CE2 control route, CE0 source ring, descriptor
window, DMA mapping, and producer/read indices under one gate. A definite
pre-publication failure returns the transaction to `AwaitRequest`; uncertain
publication quarantines it. The native CE adapter also consumes one CE2
control response under the fixed CE0/CE2 route and binds the dynamic endpoint
only after the copied HTC response passes the existing parser. Twelve host
tests cover the codec, transaction, exact DMA copy/descriptor, control response,
and route rejection.

This remains software-only: no production CE/SMMU authority or CE2 IRQ/system
dispatcher is wired, and no control frame has been sent to the phone.

### Source-matched WMI control-service startup (2026-10-24)

The pinned QCACLD sequence now has a bounded native implementation for the
WMI_CONTROL_SVC connection itself. HTC obtains the target credit count from
HTC_READY, divides it by the configured WMI endpoint count (one by default on
Sunfish), and stores that allocation for WMI_CONTROL_SVC. The connection
request advertises that allocation in bits 15:8. The source also keeps HTC
credit flow enabled by default, so the zeroed HTC_SETUP_COMPLETE_EX flags
match the Sunfish path. The validator checks these definitions against the
pinned kernel files, including the absence of endpoint-count overrides.

In `cds_pre_enable()`, WMA connects WMI_CONTROL_SVC before posting CFG download;
after WMA completion, `htc_start()` sends HTC_SETUP_COMPLETE_EX before waiting
for WMI_READY. HTT attaches earlier during datapath open, before this WMI
pre-enable stage, and stays on its separate CE4/CE1 route. This ordering is now
distinguished in the source audit rather than treating the two service
connections as one phase.

`SunfishWmiControlSession` binds the WMI connect request, response, setup
complete, and resulting credit ledger to one generation. Its constructor now
requires the source-earlier HTT connection to be `Connected` on that same
generation and verifies that both startup sessions accepted identical
HTC_READY geometry. The native adapter publishes only its staged WMI request
or setup-complete frame on CE0, receives the response on CE2, and binds the
dynamic endpoint only after parsing a successful same-service reply. Definite
pre-publication failures can be retried; uncertain publication quarantines the
session. Host validation passes 43 HTC codec/startup tests and 160 native CE
tests, and the pinned-source validator checks the credit-allocation and
service-order assumptions.

WMI startup now binds to the exact WMI endpoint from a
`SunfishWmiControlSession` that has reached `Started` after setup-complete
submission, retaining both the negotiated WMI and earlier HTT endpoints in the
same generation-bound session. The CE2 event pipeline rejects a mismatched
receive endpoint before consuming a completion; session-backed WMI_INIT
encoding and publication both require the bound WMI endpoint, and publication
also checks the staged CE frame against the active credit ledger. Advancing
the generation clears both endpoint bindings. The native CE suite now passes
160 tests, including a combined mock-native HTT-to-WMI sequence across CE0/CE2,
endpoint retention and reset, rejection of an unstarted control session, and
rejection of a wrong CE2 route before buffer consumption.

This still does not start the radio. The WMI_INIT-to-WMI_READY event path and
source-derived resource payload exist, but target resource values have not been
revalidated against a new live capture. The production CE/SMMU Gate, target
firmware authorization, WLAN power and reset ownership, and WLAN Microkit
system remain absent. No Pixel test was run in this step; the phone stays on
LineageOS.

The rooted ADB check confirmed root access on the connected LineageOS device,
but the exact WMI_INIT resource dump and successful stock WMI_READY log are
already captured in the existing fixtures. No module replacement was needed;
Wi-Fi stayed connected and `adb unroot` restored ADB to shell UID.

### Public protection-domain mapper role audit (2026-10-24)

Extended `validate-wlan-launch-path.py` against its pinned public Linux
reference ([`qcom_pd_mapper.c` at the audited commit](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/soc/qcom/qcom_pd_mapper.c), `fd78d17954a86ea6ceb0c535e618ea129e09f8dd`). The SM7150 table maps
`msm/modem/wlan_pd` instance 180 and its `kernel/elf_loader` / `wlan/fw`
services. The public `qcom_pd_mapper` implementation builds that static map,
registers a SERVREG_LOC QMI server, answers `GET_DOMAIN_LIST` from the map, and
acknowledges PFR reports; it has no remoteproc or PAS-auth-and-reset launch
path. The validator now checks these distinct responsibilities and passes.

I also pinned and checked the separate public userspace [`pd-mapper.c`](https://github.com/linux-msm/pd-mapper/blob/5ecd2fe926aca7abfe40724177f63b942cff3947/pd-mapper.c) implementation at `5ecd2fe926aca7abfe40724177f63b942cff3947`. It reads remoteproc firmware-path metadata to locate `.jsn` / `.jsn.xz` service maps, publishes the SERVREG endpoint, and answers `GET_DOMAIN_LIST` from those static rows. In this implementation, the PFR dispatch only logs the report, unlike the kernel mapper's PFR acknowledgement. The reviewed userspace source has no visible remoteproc start, PAS-authentication, or WLFW-server path. This is a second public reference, not proof that the proprietary `pd-mapper` binary on the Pixel is identical.

The pinned Linux [`pdr_interface.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/soc/qcom/pdr_interface.c) adds a distinct client operation: `pdr_restart_pd()` requires a previously resolved, connected service endpoint, sends `SERVREG_RESTART_PD_REQ`, waits for its correlated response, and returns a remote error if the request fails. This is an RPC to the remote PDR service, not a local firmware loader or PAS call. It could request a remote restart when that service accepts it, but the public client code does not define how the modem handles an `UNINIT` WLAN domain or which component loads its image. Thus, a valid QMI response proves transaction handling only; it does not establish a cold-start path or caller authority for seL4.

The same pinned Linux tree does document the root MPSS boot path: Sunfish's remoteproc node selects `qcom,sm7150-mpss-pas`, whose driver data uses PAS ID 4 and disables automatic boot. The PAS start callback prepares Q6V5, loads and maps the modem image, calls the secure prepare/authenticate-and-reset API, then waits for the modem-ready signal. The [`qcom_scm_pas_prepare_and_auth_reset()` implementation](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/firmware/qcom/qcom_scm.c) documents an EL1 path that relies on Gunyah at EL2 to trap the PAS call and create a shared-memory bridge; when Linux runs at EL2, Linux itself creates the bridge before calling TrustZone. This is the Linux-managed **root modem** path; Sunfish DTS has no separate WLAN remoteproc node, and this source does not say how the modem's `wlan_pd` child starts. It therefore cannot be copied into seL4 as authority to issue SCM calls or transfer SMMU ownership. The validator checks these boundaries as well as the static mapper behavior; see [`qcom_q6v5_pas.c`](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/remoteproc/qcom_q6v5_pas.c) and the [Sunfish remoteproc DTS](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts).

This narrows what the public Linux source establishes: it documents service
location, not who loads the initial protection-domain image. It is also not the
proprietary userspace `pd-mapper` binary installed on this LineageOS build, so
the kernel reference cannot stand in for that daemon's implementation. The
existing boot trace still places `wlan_pd UP` before `cnss-daemon`, while the
ICNSS server-arrival path is a client of WLFW rather than the first launcher.
The exact cold-start trigger and a source-backed EL1/SMMU ownership handoff
remain unresolved. A read-only ADB check confirmed the phone remains booted in
LineageOS with Wi-Fi connected; its filtered Android logcat exposed no
additional PDR or initial-launch record. The source audit caused no radio,
firmware, reset, or partition changes.

### Pinned ICNSS S1-bypass execution path (2026-10-24)

`validate-wlan-resources.py` now pins the Lineage `drivers/soc/qcom/icnss.c`
file as well as the DT and QSMMUv500 source. It checks that the live Sunfish
`qcom,smmu-s1-bypass` property sets ICNSS's `bypass_s1_smmu` flag, that
`icnss_smmu_init()` applies `DOMAIN_ATTR_S1_BYPASS` before attaching the
mapping, and that the matched `arm-smmu.c` leaves `SCTLR.M` clear for a
stage-1 context with this attribute. The same source configures a stage-1
context with `CBAR_TYPE_S1_TRANS_S2_BYPASS`; the observed `SCTLR.M=0` snapshot
is consistent with that driver path.

This makes the Linux reference behavior explicit: it is not a recipe for an
isolated seL4 DMA route, and it does not prove seL4 can own or program the
shared SMMU. The validator passes against the captured runtime DT and the
generated Microkit DT. This was offline-only; the Pixel stayed in LineageOS.

### Fresh rooted LineageOS identity and service check (2026-10-24)

USB ADB now enumerates the physical device as `sunfish`; `adb root` reports
`uid=0(root)`, and the device reports LineageOS
`23.2-20261001-NIGHTLY-sunfish`. The read-only
[`capture-lineage-ipa-state.sh`](../platforms/sunfish/capture-lineage-ipa-state.sh)
found no matching startup messages in the retained kernel/logcat buffers and
confirmed the running kernel does not support debugfs. A targeted current
sysfs read reports modem subsystem 2 and `ipa_fws` subsystem 5 online, with no
separate WLAN subsystem entry. Init properties report `modem_svc`, `pd_mapper`,
both TFTP services, `cnss-daemon`, QRTR name service, and remote storage
running. These observations confirm current service availability, not the
private cold-start trigger for `wlan_pd`.

Without transferring either payload, rooted `sha256sum` on the handset reports
the installed `/vendor/firmware/wlanmdsp.mbn` as 3,751,004 bytes with SHA-256
`a2ee6c86b82b42140e39964900cb9cc10ca263085a7125002fdb9c448df94f2e`, and
`/vendor/firmware/bdwlan-sunfish.bin` as 26,328 bytes with SHA-256
`b5aba8c34db97f25a8d8e9f9361b5b5365e74b58e0f4addc7b922bfe7226b188`. Both
match the existing Sunfish identity records. The firmware-path and board-data
source validators pass; their limits remain unchanged: identity and path
selection do not establish firmware-use authorization, WLFW startup, MSA
ownership, DMA, or a translated SMMU route. No blob was pulled, no radio or
service was restarted, and no partition or firmware state was changed.

A follow-up read-only rooted snapshot confirmed that the Android Wi-Fi service
is enabled and has an active client in the connected state; `wlan0` is
`UP/LOWER_UP`, the `wlan` kernel module is loaded, and the interface is bound
to the platform `icnss` driver. This confirms the stock LineageOS radio path is
currently operational on the device. It does not establish that the firmware
assets can be used by NexatrodeOS or provide a seL4 launch, memory-ownership,
or SMMU path. The check made no radio or service changes.

### Current firmware candidate and offline validation refresh (2026-10-24)

The current public `linux-firmware` main ref resolves to
`bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0`. Its WCN3990 Shikra image remains
version `WLAN.HL.3.3.7.c5-00226-QCAHLSWMTPL-1`, 3,808,072 bytes, SHA-256
`ea1787533f969c084c24292defdcc88d75d71955bea7e12ffee2b5fb16248350`; this
matches the checked-in candidate inventory. The Pixel's installed
`wlanmdsp.mbn` is 3,751,004 bytes with a different hash. Upstream identifies
the candidate as Shikra-specific, and no Sunfish match or equivalent launch
contract was found. It remains unsuitable for a Sunfish test; the image was
streamed only for hashing, not written to disk or booted. See the [current upstream inventory](https://kernel.googlesource.com/pub/scm/linux/kernel/git/firmware/linux-firmware/+/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0/WHENCE)
and the detailed [`wlan-resources.json`](../platforms/sunfish/wlan-resources.json)
record.

The current offline checks pass:
`validate-wlan-resources.py`, `validate-wlan-launch-path.py`,
`validate-ath10k-firmware-path.py`, `validate-ath10k-board-data.py`, and the
full `test-modem-qmi.py` host/AArch64 runner. This verifies the modeled driver
path and its fail-closed gates; it does not establish a usable Sunfish
firmware provider, cold-start trigger, production CE/SMMU authority, or native
Wi-Fi runtime. The Pixel stayed in LineageOS throughout these checks.

### Raw Apps-SMMU MMIO mapping feasibility (2026-10-25)

The current official Microkit 2.3.1 release adds IOMMU support for x86-64;
its SDF parser rejects AArch64 I/O address spaces. seL4's generic Arm SMMUv2
capability API is likewise disabled in the Sunfish config because the pinned
driver is TX2-specific and resets the stream-match table and context banks.
However, an AArch64 user-level manager does not require Microkit's SMMU
I/O-address-space API merely to access SMMU registers: the existing
`smmu-read-probe.system` already maps the physical Apps-SMMU aperture as
ordinary device MMIO.

To test that path without touching the phone, I built a temporary copy of the
SDF with only the `apps_smmu` mapping permission changed from read-only to
read-write. The existing probe source still performs reads only. Microkit's
Sunfish 2.3.1 tool generated a 313-root-object CapDL spec; the JSON confirms
256 uncached 4 KiB frames from `0x15000000` through the 1 MiB aperture with
read/write frame rights. The image was not booted and no SMMU register write
was performed.

This establishes a possible CPU-MMIO capability path for a dedicated manager
PD, not a safe route to the radio. It does not establish that the hardware
accepts the manager's EL1 writes, that the manager owns the active SMMU, or
that the inherited route table can be changed without disturbing other
clients. Any implementation must map only the required pages, acquire an
explicit exclusive-owner gate, take a fresh complete snapshot, preserve
existing routes, and keep the hardware installer fail-closed until the route
and fault-handling policy is reviewed. The Pixel remains in LineageOS with
Wi-Fi up.

### Fresh rooted LineageOS WLAN and IOMMU snapshot (2026-10-25)

With the Pixel connected as `sunfish`, ADB reports `uid=0(root)` on serial
`02281FQC200058`. A read-only kernel-log snapshot shows firmware build
`3226822e`, `wma_wait_for_ready_event: FW ready event received`, and
`hdd_wlan_start_modules: Wlan transitioned (now ENABLED)`. `wlan0` reports
operstate `up`, carrier `1`, and is bound to `icnss`; the active kernel is
`4.14.357-openela-g28f9290ae067`. No driver reload, radio toggle, reboot, or
firmware/module replacement was performed.

Rooted sysfs inspection resolves `18800000.qcom,icnss` to IOMMU group 7 and
`soc:ipa_smmu_wlan` to group 3, with the corresponding `icnss`, `ipa`, and
`arm-smmu` Linux drivers bound. `/sys/class/iommu` is empty, no matching SMMU
fault records appeared in the current kernel log, `/dev/mem` and debugfs are
absent. With ADB restarted as root, tracefs is mounted at
`/sys/kernel/tracing`; its current tracer is `nop`, all events are disabled,
and the trace buffer is empty. Standard `iommu` tracepoints are available,
including `io_page_fault`, `attach_device_to_domain`, and `smmu_init`, but they
do not expose the active Apps-SMMU stream-to-context-bank register table. These
are Linux-side status and grouping observations only: they expose neither a
seL4 ownership handoff nor current route preimages. The previously captured
exact WMI_INIT resource TLV and stock firmware-ready evidence remain the
relevant WLAN startup fixtures; no patched module was needed for this check.

### Typed complete SMMU route capture (2026-10-25)

Added [`sunfish_smmu_snapshot.rs`](../rust/src/sunfish_smmu_snapshot.rs), a
read-only Rust capture that checks the Sunfish ID0/ID1 geometry, validates the
register aperture, reads every implemented SMR/S2CR pair (including inactive
slot S2CR preimages), and rejects malformed active route metadata. Its
snapshot-backed WLAN planner derives overlap and inactive-slot decisions from
that complete table and retains the selected slot's raw preimage. The one-shot
executor requires that preimage,
reads both slot registers again before its first write, and returns a
retryable mismatch/read error without writing; failures after the first write
still quarantine the installer. Binding rejects a preimage whose SMR is
already valid or whose route does not match the install plan. This remains a
software/mock path: it still requires separate exclusive-control, ownership,
and context-bank evidence and is not connected to a hardware writer.

The focused route snapshot and installer tests pass, along with the full
92-test SMMU/CE integration target and the AArch64 Microkit `no_std` library
check. At the time of this entry, no temporary image had been booted and no
SMMU register had been accessed or changed. A separate read-only ADB check on
2026-10-25 identified the connected phone as `sunfish`, confirmed UID 0, and
confirmed LineageOS Wi-Fi was enabled.
The exact WMI INIT payload and stock WMI_READY status already exist in the
prior capture fixtures, so this check did not reload the WLAN module or
interrupt Wi-Fi. The remaining authority gap is unchanged: the project has no
production Sunfish owner lease or hardware read/write adapter, so this software
model cannot install the route or start WLAN.

### Context-bank compare-before-write preflight (2026-10-25)

Extended the typed snapshot to capture the selected context bank's CBAR,
CBA2R, SCTLR, ACTLR, TTBCR2, TTBR0/TTBR1 halves, TTBCR, MAIR0/MAIR1, and FSR.
The installer now requires this preimage as well as the selected SMR/S2CR
preimage, re-reads all 13 context words before its first write, rejects stale
values, and refuses to proceed when the captured FSR is nonzero. Plan binding
checks that every stateful context setup register is covered by the capture;
the TLB invalidate/sync command writes are excluded because they are
write-to-command registers rather than pre-existing configuration state. It
also refuses to replace a context whose SCTLR.M translation-enable bit is
already set.
Capture geometry, bank bounds, failed reads, stale values, pending faults, and
the no-write-on-precondition-failure path are covered by host tests.

Validation on 2026-10-25: all 93 tests in
[`sunfish_smmu_program_test.rs`](../tests/sunfish_smmu_program_test.rs) pass,
the Rust library test suite passes (746 tests), the AArch64 Pixel Microkit
`no_std` library check passes, and the route/ACTLR validators pass. This is
still a model only; no SMMU register has been written. The latest temporary
Fastboot probe is read-only and was booted without flashing; its screen result
is pending, so no new hardware route or context-bank values are claimed here.

The snapshot-backed route planner now also excludes candidate context banks
that any translation S2CR still names, even if the corresponding SMR is
invalid. This catches stale or transitional route-table state before the
caller-supplied free-bank list reaches plan generation. The focused suite covers
that case; this remains conservative planning evidence, not an ownership proof.

### Global-fault compare-before-write preflight (2026-10-25)

The complete route snapshot now also captures the visible SMMU global fault
status register (`GFSR`, offset `0x48`) and carries that value into the selected
route preimage. The executor re-reads the selected SMR/S2CR and all 13 context
preimage registers first, then reads GFSR as the final precondition immediately
before executing the install plan. A changed value fails the stale-state
check; a stable nonzero value returns `GlobalFaultPending`. The snapshot also
refuses to produce a route plan if the GFSR read itself fails. No status is
cleared, and no SMMU write was added. This tighter ordering still relies on a
platform owner serializing all SMMU access across the final read and the first
plan operation.

This covers only the GFSR bank visible to the caller. The Arm SMMUv2
specification defines the global fault status as security-banked where both
security states are implemented, so a clear visible GFSR is not proof that a
separate secure bank is clear or that seL4 owns the SMMU. See the [Arm SMMUv2
architecture specification, GFSR](https://documentation-service.arm.com/static/5f900d34f86e16515cdc08fb).

Validation on 2026-10-25: all 96 tests in
[`sunfish_smmu_program_test.rs`](../tests/sunfish_smmu_program_test.rs) pass,
the Rust library suite passes (747 tests), the AArch64 Pixel Microkit `no_std`
check passes, and the SMMU handoff and WLAN resource validators pass. The
current host still sees no ADB or Fastboot device, so no new device evidence is
claimed. This remains a software preflight only; the production owner lease,
hardware writer, and WLAN launch path are still unresolved.

### Peripheral Manager proxy boot-order capture extension (2026-10-25)

The existing passive LineageOS capture now retains the `PerMgrProxy` startup,
internal-modem discovery, and proxy-vote log lines, alongside the previously
selected `PerMgrSrv`, `PerMgrLib`, and CNSS records. Its boot-order filter also
records init start timestamps for `vendor.per_mgr` and `per_proxy`, so a
future capture can compare those events with the modem reset-release,
`wlan_pd UP`, and WLFW-ready timestamps. It now includes the existing
`tftp_server` tag records as well as the TFTP service start, so file-request
activity can be compared with the modem and domain-up events. The added filter
reads only existing logcat buffers and changes no logging or device state.

`sh -n`, the mock-ADB capture check, `validate-wlan-launch-path.py`, and
whitespace checks pass. The
current ADB host still enumerates no device and discovers no wireless-ADB
services, so the expanded capture has not yet run against the phone.

### Full offline regression and public launch-source recheck (2026-10-25)

The complete `platforms/sunfish/test-modem-qmi.py` runner now passes, including
the host protocol/integration tests, AArch64 `no_std` core build, native code
generation checks, and the storage-host test target. Its generated core module
list now includes `sunfish_smmu_snapshot`, which the compare-before-write
executor imports. The standalone WLAN RPMh adapter tests also assert that a
verified ICNSS power plan preserves the profile's boot generation.

`validate-wlan-launch-path.py`, `validate-wlan-resources.py`,
`validate-ath10k-firmware-path.py`, and `validate-ath10k-board-data.py` pass.
The launch validator still reports no production caller for
`attach_with_icnss_power` and no source-backed WLAN image launcher or WLFW
provider. The public [Sunfish init file](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/init.hardware.rc)
declares `tftp_server` and `modem_svc`; the [vendor extraction manifest](https://github.com/LineageOS/android_device_google_sunfish/blob/lineage-23.2/sunfish/proprietary-files-vendor.txt)
lists `pm-proxy`, `pm-service`, `tftp_server`, and `wlanmdsp.mbn` as vendor
files. This source surface does not expose the private cold-start trigger, so
the launcher remains unimplemented rather than inferred.

The latest host check still returns no ADB device despite USB debugging and
rooted debugging being enabled on the Pixel. Root status therefore remains
untested in this session, and the expanded passive boot-order capture has not
run. No device state was changed. The next device-side evidence remains a
LineageOS ADB connection for the existing read-only PM-proxy/TFTP/WLFW capture;
native startup still also needs an authorized SMMU owner and a valid WLAN image
launch/source path.

### Public MDM-helper startup lead and passive capture (2026-10-25)

Expanded the pinned LineageOS Sunfish device-tree audit beyond
`init.hardware.rc`: `init.mdm.sh` checks `ro.baseband` for `mdm` or `mdm2` and
then requests `start vendor.mdm_helper`; `sunfish/init.sunfish.rc` declares
`/vendor/bin/mdm_helper` as a root service. The script and service declaration
are public, but the public Sunfish product makefiles do not explicitly package
`init.mdm.sh`; the product conditionally inherits a separate private vendor
makefile. The current source therefore does not establish whether the script
is installed or invoked on this boot, nor what the vendor binary does. It is
recorded as a candidate to observe, not as a proven WLAN-PD launcher. The
pinned `validate-wlan-launch-path.py` checks the init and product-makefile
boundary and keeps the private behavior explicit.

The read-only Lineage capture now records `ro.baseband`, the
`init.svc.vendor.mdm_helper` state, its PID when present, read-only presence
checks for the likely installed script and binary paths, service-start log
records, and matching existing `mdm_helper` modem/WLAN/TFTP/QMI/PDR logs. It
does not start or stop the service, change logging, issue QMI, or touch radio
state. Shell syntax and the pinned launch audit pass. A mock-ADB run verifies
the new fields and log filter. At this point the phone did not enumerate in
ADB or Fastboot, so the current boot was not checked; a saved rooted return
boot later resolved the installed-binary question as described below.

### Saved Lineage boot log resolves the MDM-helper lead (2026-10-25)

The saved rooted return-to-Lineage dmesg captures now show that init attempted
to start `vendor.mdm_helper` but could not find `/vendor/bin/mdm_helper`. In the
same boot, `vendor.per_mgr`, `per_proxy`, `modem_svc`, both TFTP services, and
`pd_mapper` started; the kernel then loaded `modem.mdt`, released the MPSS
reset, received the modem-ready interrupt, acknowledged `wlan_pd` UP, connected
ICNSS to WLFW, and later logged the WLAN firmware-ready event. Both filtered
event sequences are preserved in
[`lineage-modem-startup-order-20261025.log`](../platforms/sunfish/fixtures/lineage-modem-startup-order-20261025.log)
and [`lineage-modem-startup-order-20261025-probe.log`](../platforms/sunfish/fixtures/lineage-modem-startup-order-20261025-probe.log)
and checked by
[`validate-lineage-modem-startup-order.py`](../platforms/sunfish/validate-lineage-modem-startup-order.py).

These two successful Lineage boots rule out the installed `mdm_helper` binary
as a necessary step because it was absent in both. At the time of that source
audit, the Mac had no ADB device, so the prepared passive capture had not yet
been rerun. The start order makes
`modem_svc` and the Peripheral Manager proxy the remaining public-init leads,
but temporal order does not prove which process requested MPSS startup or
caused the modem firmware to launch `wlan_pd`. Their implementations remain
vendor/private; no corresponding operation is inferred for seL4. The Mac still
has no ADB device, so the prepared passive capture could not be rerun against
the current boot.

### Pinned Lineage root-MPSS character-device path (2026-10-25)

The source audit now checks the pinned Lineage kernel's public root-modem
startup chain. atoll.dtsi names the trusted subsystem modem; the PIL driver
uses that firmware name as its subsystem name; subsystem_restart.c creates the
corresponding subsys_modem character device. Opening that device calls
subsystem_get_with_fwname, which enters __subsystem_get, starts the registered
subsystem, and invokes the PIL power-up callback. subsys-pil-tz.c passes the
selected name to pil_boot; the peripheral loader then requests modem.mdt,
performs image and memory setup, loads segments, and reaches the authenticated
reset operation. The check is part of validate-wlan-launch-path.py and
verifies these files against the pinned Lineage kernel commit.

This gives the Lineage log's “Changing subsys fw_name to modem”, modem.mdt
load, and reset-release records a concrete public kernel path. It does not
identify which user-space process invoked the path: both __subsystem_get and
the writable firmware_name sysfs store emit the same firmware-name log text,
and the modem services are private binaries. It also describes the root MPSS
boot path only; it neither explains how modem firmware starts wlan_pd nor
supplies an authorized WLAN image/provider or seL4 ownership of the required
secure-memory and hardware interfaces. No device operation was performed for
this source audit.

### Read-only modem subsystem caller snapshot (2026-10-25)

Added capture-lineage-subsys-owner.sh for a one-shot, rooted LineageOS
snapshot of persistent process descriptors that target /dev/subsys_modem or a
firmware_name sysfs node. It refuses multiple/unauthorized devices, non-Sunfish
or non-LineageOS builds, and non-root ADB shells; it never restarts adbd,
reboots the phone, changes tracing, or touches WLAN state. Its output is
limited to matching PID, process name, descriptor target, and a bounded
command line. The snapshot may miss a transient sysfs write, so an empty result
does not prove that no process invoked the path. The descriptor search uses
Android Toybox `find` to select matching symlinks before reading process
details; a per-descriptor `readlink` scan was still running after a minute and
was stopped on this device.

The 2026-10-25 rooted capture verified `sunfish`, LineageOS
`23.2-20261001-NIGHTLY-sunfish`, UID 0, and a character device at
`/dev/subsys_modem`. A targeted scan returned one matching open descriptor:
PID 1320, `pm-service`, fd 10, target `/dev/subsys_modem`, command line
`/vendor/bin/pm-service`. No persistent fd to a `firmware_name` node appeared.
This identifies Peripheral Manager's service process as a live holder of the
subsystem device, consistent with the public kernel open-to-start path above;
it does not reveal the private service's exact call timing or reason. A future
capture may still miss transient `firmware_name` writes.

A repeat read-only check later on 2026-10-25 again found PID 1320 holding fd 10
to `/dev/subsys_modem`. Its mapped vendor libraries include
`libperipheral_client.so`, `libmdmdetect.so`, and the QMI CCI/CSI/common
libraries. Its fd table also includes `/sys/kernel/tracing/trace_marker`; the
probe only read the fd link and did not write to the marker or alter tracing.
`init.svc.vendor.per_mgr`, `init.svc.per_proxy`, `init.svc.cnss-daemon`,
`init.svc.tftp_server`, and `init.svc.vendor.qrtr-ns` were all `running`.
These names and mappings confirm the process/library boundary and service
coexistence, but do not prove which private API or request caused modem startup.
ADB was returned to shell UID after this check.

A final read-only tracefs check found `tracing_on=0`, `current_tracer=nop`, and
a 7 KiB ring; no retained entries matched the modem/PM/WLAN filter. No trace
events or callsites were enabled. ADB returned to UID 2000, the phone remained
boot-complete in LineageOS, and Wi-Fi remained enabled.

### Rooted Lineage modem and WLAN startup snapshot (2026-10-25)

The passive capture-lineage-ipa-state.sh run on the same boot recorded
`vendor.per_mgr` starting as PID 1320, matching the process holding
`/dev/subsys_modem`. The existing kernel and logcat buffers show modem firmware
load and reset release, Modem QMI readiness, a successful
`QMI_IPA_INIT_MODEM_DRIVER_REQ_V01` response, a `wlan_pd` service-180
notification (`state=0x1fffffff`) and ACK, ICNSS QMI server connection
(`state=0x980`), `icnss: WLAN FW is ready: 0xd87`, and the WMA firmware-ready
event. This is a useful Linux baseline: in this boot, Lineage reached WLAN
firmware-ready while the capture reported `mdm_helper_service=unset`, no
`mdm_helper` PID, and the helper binary/script absent or unreadable.

The capture found no matching Peripheral Manager vote/client lifecycle lines,
and the running kernel does not support debugfs, so IPA `hw_type` and
`gen_reg` were unavailable. The logs establish ordering and successful
readiness in this boot, but they do not expose the private PM service's vote
implementation or identify a supported way for seL4 to perform the required
secure modem launch. The capture only reads existing properties and log
buffers; it changes no Wi-Fi, modem, or logging state.

### Public PeripheralManager source does not cover the modem service (2026-10-25)

The public [CodeAurora mirror](https://gitlab.com/codeaurora-mirror/quic/la/platform/system/peripheralmanager/-/tree/fe2167b737928cda28d3e83791beb1dcc2324bc7)
of `platform/system/peripheralmanager`, branch `aosp-new/master` at commit
`fe2167b737928cda28d3e83791beb1dcc2324bc7`, was checked read-only. Its
[Apache-2.0 license marker](https://gitlab.com/codeaurora-mirror/quic/la/platform/system/peripheralmanager/-/blob/fe2167b737928cda28d3e83791beb1dcc2324bc7/MODULE_LICENSE_APACHE2)
identifies source that builds the `peripheralman` daemon and Android Binder
APIs for GPIO, I2C, LED, SPI, and UART access. It contains no
`pm_client_register`, `pm-service`, modem subsystem device handling, or WLAN
launch path. This is a different service despite the shared “Peripheral
Manager” name; that source license does not establish rights to the separate
Sunfish vendor files.

On the attached phone, the live `/vendor/bin/pm-service` instead holds
`/dev/subsys_modem` and maps `libperipheral_client.so` plus Qualcomm QMI
libraries. The [public Sunfish init file](https://android.googlesource.com/device/google/sunfish/%2B/29352689/init.hardware.rc#432)
declares that vendor daemon and starts `per_proxy` after it; those declarations
establish service ordering, not the private client contract. Therefore the
Apache source cannot supply a source-backed adapter for the observed
modem-owner operation. No implementation was copied from the mirror, and the
phone remains in LineageOS with Wi-Fi connected.

### Current Linux-firmware candidate recheck (2026-10-25)

The upstream `linux-firmware` main head still resolves to
[`bf992df7`](https://gitlab.com/kernel-firmware/linux-firmware/-/commit/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0),
and the pinned [`WHENCE`](https://gitlab.com/kernel-firmware/linux-firmware/-/raw/bf992df7a95b5e19bb8f8546eb4d7c53ab2598d0/WHENCE)
lists generic WCN3990, Shikra, and QCM2290 firmware paths, with no Sunfish
entry. Current candidate blobs were streamed in memory and their sizes and
SHA-256 values still differ from the observed Sunfish image; the parent
`board-2.bin` also still lacks `sunfish` and `google_sunfish` selector strings.
No current upstream candidate passes the Sunfish identity and board-data gates,
so the separate Linux compatibility test remains planned but not ready to run.
The full hash inventory and test-entry criteria are in
[`sunfish-ath10k-port-plan.md`](sunfish-ath10k-port-plan.md) and
[`wlan-resources.json`](../platforms/sunfish/wlan-resources.json). This refresh
saved or decoded no firmware, staged none into a boot image, and made no phone
changes.

### Repeated LineageOS Wi-Fi bus-trace comparison (2026-10-25)

The previously authorized comparison ran on the connected LineageOS boot with
30-second baseline, Wi-Fi-off, and reconnected windows. The same-boot runtime
inventory mapped the shared `ipa` client to handle 10 with four resolved
routes. Across the trace, 7,483 request updates and 7,103 devbw samples were
recorded, with no unknown handles, path-count mismatches, failed paths, or bus
errors.

Handle 10 had no updates in the 30.61-second connected baseline. The
30.55-second Wi-Fi-off window had eight updates: four `MIN→SVS2` and four
`SVS2→MIN` transitions. The 30.61-second reconnected window also had eight
updates, all within the 7.58 seconds between enable and reported connection;
there were none in the remainder of that window. This resembles the earlier
off-transition correlation, but the count differs from the prior five-pair
off run and the client is still shared IPA, so the trace does not attribute
these requests to WLAN.

The script disabled both temporary dynamic-debug callsites, restored Wi-Fi to
connected, and returned ADB to UID 2000. It saved no SSID or network
identifiers. The trace, markers, registration inventory, and window report are
in
`.cache/pixel-sunfish/msm-bus-update-trace/wifi-compare-repeat-20261025-153703/`;
the filtered trace SHA-256 is
`a1c7d156464f1dc73d0a708bdeebaa3165527f2c7a270795257d7bbd276093e8`. This is
Linux behavior evidence, not a native seL4 WLAN result.

### Rooted Peripheral Manager runtime surface (2026-10-25)

With the user's rooted-debugging setup, `adb root` succeeded on the connected
Sunfish LineageOS boot. A bounded, read-only capture reconfirmed the `sunfish`
product and LineageOS 23.2 build; the vendor Peripheral Manager, proxy, CNSS,
QRTR namespace, and TFTP services were running. The existing kernel ring
contains WLAN CLOSED-to-ENABLED, WMA firmware-ready, EAPOL M1–M4, and DHCP ACK
records from this Linux boot. The helper's current logcat buffers no longer
contain Peripheral Manager client/vote lines, and debugfs is unsupported by
this running kernel, so this adds no fresh PM event ordering or IPA register
state. No SSID or network identifiers were retained.

On-device `toybox strings` inspection of the installed `/vendor/bin/pm-service`
and `/vendor/lib64/libperipheral_client.so` found the literal
`vendor.qcom.PeripheralManager`, `android::IPeripheralManager` Binder symbols,
and exported names including `pm_client_register`, `pm_client_connect`,
`pm_client_event_acknowledge`, `pm_client_unregister`, `pm_client_disconnect`,
`pm_service_shutdown`, and `pm_show_peripherals`. The daemon's other strings
refer to a `modem` peripheral, per-client vote/unvote tracking, QMI restart and
shutdown requests, and shutdown timeouts. These are runtime binary strings,
not a verified ABI or a safe native start sequence. A `service check` through
the ordinary Android service-manager context did not find
`vendor.qcom.PeripheralManager`; that result does not establish whether the
vendor service is registered in another Binder context.

This narrows the missing boundary to the Qualcomm Peripheral Manager client
contract and its vendor service context, but does not supply either one. The
public AOSP `platform/system/peripheralmanager` source is the separate GPIO,
I2C, LED, SPI, and UART API already distinguished above. Public Qualcomm audio
callers in [`pm.c`](https://android.googlesource.com/platform/hardware/qcom/audio/+/marshmallow-dr1.5-dev/msm8909/hal/audio_extn/pm.c)
and [`pm.h`](https://android.googlesource.com/platform/hardware/qcom/audio/+/marshmallow-dr1.5-dev/msm8909/hal/audio_extn/pm.h)
demonstrate registration, voting, callback acknowledgement, and disconnect
patterns, but include the private `pm-service.h` and do not define the Sunfish
service implementation or its WLAN launch authority. The binary strings were
not copied into the tree or used to construct an adapter.

A fresh root process and init inspection found `qrtr-ns`, `tftp_server`,
`modem_svc -q`, `pd-mapper`, `rmt_storage`, and `cnss-daemon` running; the
vendor service manager lists `vendor.qcom.PeripheralManager`. The current
`/vendor/firmware/modemuw.jsn` still maps `msm/modem/wlan_pd` instance 180 to
`kernel/elf_loader`, `tms/servreg`, and `wlan/fw`. Sunfish init declares
`vendor.mdm_helper` as `disabled` and `oneshot`, and it was absent from the
current process list. The retained logcat had no `pd-mapper` or TFTP request
records, so these static mappings and running daemons still do not identify
which private client initiates the WLAN-PD launch. No vendor binaries or
firmware were copied from the phone.

### Production WLAN composition audit (2026-10-25)

A fresh source-tree inventory found no Sunfish production WLAN `.system`
manifest: the repository-root `system.system` is the QEMU application, while
the Sunfish manifests are display/touch or bounded diagnostic probes. The
`qcom_ce_v3_native_io` module itself says it neither powers WLAN nor configures
SMMU, allocates DMA, or binds interrupts, and explicitly has no production
`Gate`. `pixel_modem_owner_gate::LiveGate` protects the GLINK/SMEM owner; it is
not a CE/SMMU gate. The launch-path validator likewise reports no production
caller for `attach_with_icnss_power` and no source-backed WLAN image
launcher/provider.

The resource inventory is explicitly non-authoritative, and the runtime WLAN
SMMU route observed after Lineage brings up the radio cannot be assumed to
exist when seL4 takes over at boot. So the next implementation step is not
another protocol probe: it is a Sunfish production composition with an actual
exclusive CE/DMA/SMMU/IRQ lease, connected to a lawful modem/WLFW launch and
firmware source. Until those authority and firmware inputs are established,
instantiating the CE trait with a fabricated observation would only make a
test look integrated and would not move the device toward working Wi-Fi.

### Rooted LineageOS IOMMU tracepoint Wi-Fi cycle (2026-10-25)

A bounded 10-second connected/off/reconnected cycle captured the available
generic IOMMU `map`, `map_sg`, `unmap`, `io_page_fault`, attach, and detach
tracepoints. Three records appeared, all `map_sg` calls in process
`RenderThread` for `5040000.qcom,kgsl-iommu:gfx3d_u`; they fell between the
Wi-Fi-enable request and the reported reconnection. The connected baseline
and Wi-Fi-off window had no IOMMU records. No WLAN/ICNSS/IPA domain, unmap, or
I/O page fault appeared. Wi-Fi was restored to connected, tracing was disabled,
and the ring buffer was cleared; no driver or firmware was changed.

This is an absence of observed generic-IOMMU events during the short toggle,
not proof that WLAN has no mapping: its mapping may persist across toggles or
use a path these tracepoints do not report. The raw trace and marker file are
in `.cache/pixel-sunfish/iommu-wifi-trace/wifi-cycle-20261025-160932-utc/`.
The capture does not expose or change the Apps-SMMU stream-to-context-bank
route, so the seL4 ownership gap remains.

### Fresh rooted Linux IOMMU-group snapshot (2026-10-25)

With rooted ADB on the connected Sunfish LineageOS 23.2 build, a read-only
sysfs check found `15000000.apps-smmu` bound to `arm-smmu`. Linux lists
`soc:ipa_smmu_ap`, `soc:ipa_smmu_wlan`, and `soc:ipa_smmu_uc` as the sole
members of IOMMU groups 2, 3, and 4, respectively; `18800000.qcom,icnss` is
the sole member of group 7. Each group's `reserved_regions` exposes only the
same MSI window, `0x08000000–0x080fffff`. `/sys/class/iommu` has no entries,
and the running kernel does not support debugfs. A passive IPA/startup log
capture still sees WMA firmware-ready records but no retained PM-client/vote
lines; `pm-service` continues to hold `/dev/subsys_modem`.

The same session read the raw `iommus` cells from the live flattened device
tree. All four nodes reference Apps-SMMU phandle `0x30` (`#iommu-cells = 2`):
ICNSS requests SID/mask `0x240/0x1`; the IPA AP, WLAN, and UC nodes request
`0x520/0`, `0x521/0`, and `0x522/0`. This independently confirms the input
specifier values already used by the source-derived profile and the earlier
diagnostic-kernel route capture. It does not read the current boot's active
SMR/S2CR or context-bank registers.

These Linux IOMMU group numbers are runtime group identifiers, not SMMU stream
IDs, context-bank numbers, or seL4 authority. The sysfs view does not expose
the active Apps-SMMU stream-to-context-bank/page-table route, so it cannot
populate or authorize the native Gate. The check changed no device state; Wi-Fi
remained connected, and no SSID or network identifier was recorded.
