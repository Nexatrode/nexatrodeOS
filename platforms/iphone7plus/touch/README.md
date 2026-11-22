# iPhone 7 Plus touch input

The hardware reference is the D11 device tree and touch stack in Corellium's
Project Sandcastle Linux tree at commit
`0c2f7dda13d67bb7e06123df516f8bdb1000a79b`. This is a reference for
porting a dedicated Microkit driver, not a Linux binary to run inside seL4.
The relevant source files are
[`hx-h9p-d11.dts`](https://github.com/corellium/linux-sandcastle/blob/0c2f7dda13d67bb7e06123df516f8bdb1000a79b/arch/arm64/boot/dts/hx/hx-h9p-d11.dts),
[`spi-hx.c`](https://github.com/corellium/linux-sandcastle/blob/0c2f7dda13d67bb7e06123df516f8bdb1000a79b/drivers/spi/spi-hx.c), and
[`hx-touch.c`](https://github.com/corellium/linux-sandcastle/blob/0c2f7dda13d67bb7e06123df516f8bdb1000a79b/drivers/input/touchscreen/hx-touch.c).
The separate firmware loader is
[`hx-touchd`](https://github.com/corellium/projectsandcastle/tree/03db9c6ae04141eb940f3b9f56d446f50d57fadf/hx-touchd).
The phone that booted our shell is `iPhone9,2` / D11. A newer HoolockLinux
port [reports working touch on `iPhone9,4` / D111](https://github.com/Pauli1Go/HoolockLinux),
which is another iPhone 7 Plus variant. Its firmware and calibration cannot
be assumed to match D11; its A10 SPI and touch driver are useful references.

The D11 tree identifies SPI2 at physical `0x20a088000` (interrupt 216,
3 MHz maximum for the touch device), the SPI2 clock gate at `0x20e0801d8`,
and the touch clock at `0x20e078000` (32,768 Hz output). The touch node uses
GPIO 161 for reset, 81 for chip select, and 166 for interrupt. The GPIO
controller is at `0x20f100000`. Two power paths are named: `touch_ldo`
through the display PMU on I2C0 and `touch_pwrsw` through the PMU on I2C1.
These values are source-tree claims; they have not been verified against the
phone's live Apple device tree or tested as seL4 device mappings.

The Sandcastle touchscreen stack initializes power, clocks, GPIO, SPI, and
IRQ delivery before its daemon loads a controller-specific `.mtprops`
firmware personality using calibration data from the device's SysCfg partition.
The iOS 15.8.8 (19H422) IPSW supplied for this work contains both D11 and D111
identities. Its D11 DeviceTree states `iPhone9,2` / `D11AP`, identifies the
controller as `hid-transport,k1` on SPI2, and names personality `C1F5E,2`.
`ipsw extract --files --pattern '.*\.mtprops$' --device iPhone9,2` extracted
`usr/share/firmware/multitouch/D11.mtprops` from the OS filesystem. The file
is 116,503 bytes, SHA-256
`7f9b16b749dd0f3e12633ce5140220ea89317006dc0278b24cf2d815578e3ff6`,
and contains exactly that personality with two constructed firmware payloads.
These Apple assets remain in ignored `.cache/iphone7plus-touch-fw/` and must
not be committed or redistributed. `validate-assets.py` checks the D11
DeviceTree and `.mtprops` pair and rejects the D111 DeviceTree.

The matching firmware's calibration sequence requests `MtCl`, `OrbG`,
`OFCl`, and `SDAC`. These are device-specific SysCfg records, not in the IPSW.
On the iPhone 7 Plus running iOS 15.8.8, the paired lockdown query
`ideviceinfo -q com.apple.mobile.internal -k SysCfgData -x` returned the
device's 131,072-byte SysCfg as a plist data value. Two independent reads
matched byte for byte. `syscfg-calibration.py` validated the `gfCS` index and
extracted `MtCl` (852 bytes), `OrbG` (992 bytes), `OFCl` (1,474 bytes), and
`SDAC` (208 bytes). The four private files are in ignored
`.cache/iphone7plus-touch-fw/calibration/` with mode `0600`; the full SysCfg
dump and its plist wrappers were removed after validation. This paired-device
query did not require the jailbreak's SSH service. The parser prints sizes and
hashes, never the record contents, and refuses to overwrite prior output.
These calibration files must stay outside Git and must only be used with this
physical phone. The app shell still cannot receive live touch events until
the controller service and input routing are implemented and hardware-tested.

`report.c` is the first transport-independent piece. It decodes a
checksum-verified touch payload, limits it to ten contacts, handles signed
coordinates and calibration spans, and returns normalized 0–4096 positions.
`packet.c` validates the SPI reply header, tag, maximum size, and 16-bit
payload checksum before any report is decoded. Both functions are independent
of MMIO. `sh platforms/iphone7plus/touch/verify.sh` runs host tests, and the
packet, report, and boot-stream readers compile for freestanding AArch64.

`build-boot-stream.py` now converts the D11 `C1F5E,2` firmware and this phone's
four calibration files into a private, bounded boot stream. It follows the
operation order and wire encoding of Project Sandcastle's `mtfw.c`: generation
2, a wake write, two constructed firmware writes, four calibration loads,
register writes, a calibration request, then an IRQ wait. The output has 16
operations and is 80,230 bytes, SHA-256
`c4157ef64c0901f7e8aa14768e8173ab76ac406fc54e668f1f64b2eb3d74a0b3`.
It is stored at ignored `.cache/iphone7plus-touch-fw/boot-stream.bin` with
mode `0600`. The format is `NTFW`, version 1, operation count, total length,
then type/length/payload records. `boot_stream.c` validates every record and
its ordering before exposing any operation to a driver. It does not perform
SPI transfers by itself. The earlier stream hash
`a063e426b030fc106ebdd8939de601a9244f9570ccd81c5db2c8c6bd224a636a`
was incorrect: its XML parser treated `IDREF` integers as zero, altering five
boot-sequence register writes. The fixed stream matches the original
Sandcastle `mtfw.c` builder byte for byte with this D11 firmware and phone's
four SysCfg calibration records. The previous stream is retained only as an
ignored, clearly named diagnostic backup and must not be uploaded again.

The next hardware check is `spi2-read.system`, an isolated image that maps
the 16 KiB D11 SPI2 page at `0x20a088000` read-only and displays CLKCFG,
CONFIG, STATUS, PIN, and CLKDIV. `build-spi2-read.sh` successfully produced
`build/iphone7plus/spi2-read/loader.bin`. On the first phone run, the user
reported `SPI2 READ CLKCFG` with no hex value. The initial register read did
not complete. The Pongo USB device remained enumerated but would not answer a
second control request, so the next upload requires a new DFU/Pongo session.
The follow-up `spi2-diagnostic.system` adds a parent fault handler. On hardware
it drew `FAULT` with label `6` (seL4 VMFault), displayed address low word
`0A088000`, and FSR `92000018`. The ARM exception class is a data abort from
lower EL; DFSC `0x18` denotes a synchronous parity/ECC error. The display
omitted the address high word. The rebuilt diagnostic confirmed pointer
`0x41000000`, clock-gate register `0x00000A00` at `0x20e0801d8`, full fault
address `0x20A088000`, and the same FSR `92000018`. In Sandcastle's
`clk-hx-pmgr.c`, an enabled gate has bits 4–7 all set; `0xA00` reports them
zero. `spi2-enable.system` is the next bounded probe: it uses that driver's
gate enable sequence, waits up to 10,000 reads for enabled status, sets its
post-enable bit, and only then reads SPI2. Its gate mapping is writable, SPI2
remains read-only, and it does not reset the touch controller. Build it with
`build-spi2-enable.sh`; its image is at
`build/iphone7plus/spi2-enable/loader.bin`. On the first hardware run, the
user reported gate `BEFORE A00`, `AFTER FF`, and all five SPI2 registers:
CLKCFG `0`, CONFIG `8`, STATUS `0`, PIN `3`, and CLKDIV `1`, with no fault.
This confirms the MMIO map and that the gated controller becomes readable.
`spi2_transport.c` is a freestanding, host-tested PIO layer built from the
Sandcastle register sequence. It prepares SPI2 for a caller-selected rate
(the D11 tree gives SPI2 a 24 MHz parent and caps touch at 3 MHz), clocks
full-duplex transfers through the 16-byte FIFOs, and always stops PIO on a
bounded timeout or malformed FIFO state. Its interface deliberately leaves
the PMGR gate and touch chip-select GPIO to the device service. It has not
yet been linked into a phone image or used to transmit to the controller.
The isolated `spi2-setup.system` image now links the prepare routine. It
enables the tested PMGR gate, writes only SPI2's controller setup registers
for a 3 MHz rate, and reads CLKCFG, CONFIG, STATUS, PIN, and CLKDIV back. It
does not assert the touch chip select, switch its power rails, or send data.
Build with `sh platforms/iphone7plus/touch/build-spi2-setup.sh`; the flat
image is `build/iphone7plus/spi2-setup/loader.bin` (902,684 bytes, SHA-256
`7dc3563439a005405f477bc74a71ee4fe5246d598029f52e16d3c550f2426b30`).
On the first phone run, the user reported gate BEFORE `0xA00`, AFTER `0xFF`,
CLKCFG `0x1`, CONFIG `0x0010401E`, STATUS `0x30`, PIN `0`, and CLKDIV `8`,
without a fault or failure. The expected setup configuration and divider
were accepted. CLKCFG did not read back the written `0xD`; Sandcastle's
driver reads CONFIG after that write, not CLKCFG, so the meaning of this
readback remains unresolved. No SPI data byte has been transmitted.

`hw_control.c` now contains bounded PMGR gate and 32,768 Hz touch-clock
enable routines plus D11 GPIO output/peripheral configuration. Its fake-MMIO
tests verify gate completion/timeout, clock divider/timeout, and GPIO register
updates. The separate `touch-hw-probe.system` image uses only the gate part:
it enables the I2C0/I2C1 controller clocks and reads each controller's
revision, the untouched touch-clock register, and reset/CS/IRQ GPIO registers.
It performs no I2C bus transfer or PMU write. Build with
`sh platforms/iphone7plus/touch/build-touch-hw-probe.sh`; the image is
`build/iphone7plus/touch-hw-probe/loader.bin` (886,524 bytes, SHA-256
`55948371bf03343376310deac9e4e2a791c6adf74f6573fd707da01da474064c`).
On the phone, both I2C gates read `0xFF`, both revision registers read `3`,
and the mapped pointer was `0x43000000`. The untouched clock register read
`0x800002EE` (disabled). RESET, CS, and IRQ GPIOs read `0x78A02`, `0x78A21`,
and `0x7CB80`, respectively; there was no fault. I2C0 (`0x20a110000`) and I2C1 (`0x20a111000`)
share one 16 KiB Microkit mapping, with I2C1 at offset `0x1000`.

`i2c_pmu.c` now implements a bounded, read-only PMU register transaction:
initialize the Hx I2C controller at the D11 100 kHz default, select an 8- or
16-bit PMU register, then read one byte. It handles NACK, bus error, and
timeout without writing a PMU register value. The standalone
`touch-pmu-probe.system` image configures I2C0/1 pinmux and reads display-PMU
register `0x05` at address `0x27` plus core-PMU register `0x031F` at `0x74`.
It does not change either rail. Build with
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh`; the image is
`build/iphone7plus/touch-pmu-probe/loader.bin` (886,412 bytes, SHA-256
`d62645f0f79548daa082d9e88b926bfaaf0cc6093e4eef774427bf59b00a9ee6`).
The phone read completed without ERR or FAULT: HV register `0x05` returned
`0x0F`, and core register `0x031F` returned `0x01`. On D11, bit `0x10` in
the HV byte enables touch while the lower nibble controls the display; the
touch HV bit is currently off and the core switch bit is already on. A masked
read/modify/write must preserve the lower nibble.
The bounded write helper is covered by the host test. Build the separate
rail-control check with
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh hv` and upload
`build/iphone7plus/touch-hv-probe/loader.bin` from Pongo. It requires core bit
`1` and exact HV byte `0x0F`, holds touch reset low, updates only bit `0x10`,
reads the enabled byte, and attempts to restore the bit even if the first
update reports an error. The screen should show `HV BEFORE 0000000F`,
`HV ON 0000001F`, `HV OFF 0000000F`, and `DONE`. It sends no touch SPI data.
This image is 886,412 bytes, SHA-256
`c320b262e2b7b0ad891f6dc6b27814ea4f8387b5e2bfc70f4755644d5658e388`.
On the phone it displayed core `0x01`, HV before `0x0F`, HV on `0x1F`, and
HV off `0x0F`. The Pongo boot request timed out, as in earlier successful
diagnostic runs; the on-screen values establish that the PMU write and
readback worked and that the bit was restored.
The next isolated `touch-startup-probe.system` image combines these controls
with the 32,768 Hz touch clock, reset/CS GPIO sequence, SPI2 setup, and a
single four-byte zero exchange from the firmware daemon's initial handshake.
It checks for the observed core `0x01`, HV `0x0F`, and disabled clock
`0x800002EE` before changing touch hardware. It displays the received four
bytes, then puts reset low, disables the touch clock, and restores HV `0x0F`,
including after a bounded SPI failure. It does not send firmware. Build with
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh startup`; the image
is `build/iphone7plus/touch-startup-probe/loader.bin` (886,636 bytes,
SHA-256 `29fe429d9558f109ef15bb41566bb1a15d8a79772183bdb8dc0fe053da344cc2`).
The first on-phone build faulted with seL4 UserException `3`; it used
`CNTVCT_EL0` for short delays while this kernel build disables EL0 virtual
counter access. The rebuilt image uses a conservative instruction loop and
the fault monitor now displays the fault IP and number for user exceptions.
On the corrected run, the phone displayed touch clock `0x000802DC`, SPI2
`READY`, four received bytes `1F 01 1F 01`, and HV off `0x0F`, with no fault.
The SPI2 controller completed the first four-byte exchange and the rail
cleanup ran. These bytes alone do not establish that the touch firmware is
loaded or that input reports are available.
The separate `firmware` build mode links the private, SHA-256-checked D11
boot stream into an ignored image. `boot_execute.c` validates the entire
stream before transmission, holds CS across 16 KiB chunks, sends each ACK
command, releases CS on transfer failure, and performs a bounded final IRQ
wait. Its host test covers chunk boundaries and failure cleanup.
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh firmware` builds
`build/iphone7plus/touch-firmware-probe/loader.bin` (969,116 bytes, SHA-256
`841e8cf1d13ff38ccbf15932b611ad23d0e83a579f9a75ad3f8b100d6113025f`).
The image repeats the successful power/clock/SPI startup, sends the 16-record
firmware and calibration stream, displays the current record and result,
then puts reset low and restores clock and HV. `STREAM SENT` means all
transfers completed and the IRQ GPIO asserted; it does not yet verify a
valid touch report. The first on-phone firmware image reached record `0xF`
(the final `WAIT_IRQ`) after all SPI writes, then returned `0xFFFFFFFD`
(`TOUCH_BOOT_EXEC_IRQ`, timeout). Boot IRQ GPIO read `0x7CB80`, and the
second sync returned `0x1F011F01`; HV restored to `0x0F`. This is not a
confirmed firmware boot. The revised image above configures GPIO 166 as a
falling-edge IRQ in group zero and checks the pinctrl block's latched pending
bit at `0x814`, with raw active-low level as a fallback. It clears the boot
edge before streaming and displays `IRQ` (`1` means latched edge, `2` means
active-low level, `0` means timeout). The first revision faulted because this
extra result row was placed beyond the mapped framebuffer. The current image
moves it into the mapped area. On the corrected phone run, `FW RESULT` was
`0`, `IRQ` was `1` (a latched falling edge), and the final status was
`STREAM SENT`. This confirms all records completed and the controller
produced a falling edge during the stream. Because the IRQ latch was cleared
before the stream rather than immediately before the final wait, the edge
does not by itself prove firmware completion. D9 report and touch contact
delivery remain unverified.
The `report` build adds the generation-2 control protocol: a 16-byte wake
command, two D9 report-information commands, a bounded 16-byte long-read
setup, and a 21-byte transfer for the first 16 D9 bytes. It checks the
controller status and length before reading, displays its stage/result and
raw calibration bounds, then performs the same touch-power cleanup. Its
transport-independent command sequence is host-tested. Build with
`sh platforms/iphone7plus/touch/build-touch-pmu-probe.sh report`; the image
is `build/iphone7plus/touch-report-probe/loader.bin` (969,116 bytes,
SHA-256 `5cdce73744741ac6a875543e51194c388da01fb592ac5247819012d91fbd6d57`).
The first phone run retained `FW IRQ 1`, but D9 stopped at stage `3` while
checking the second report-information response. The next run displayed
the raw response as `4F814F81` repeated four times
(`4F814F81 4F814F81 4F814F81 4F814F81`), with stage `3`, status `0x4F`,
length `0x4F81`, and HV off `0x0F`. This is not a valid D9 report. The
next build re-ran SPI2 controller preparation before each generation-2
command, matching Sandcastle's per-message `hx_spi_prepare` behavior. The
firmware path remained unchanged. The phone returned the exact same D9 error
and `4F814F81` pattern, so this setup difference did not explain the failure.
An older Sandcastle debug daemon in the local source cache also explicitly
detects repeated `4F 81`, although it does not explain its cause. The current
probe preserves the first eight RX bytes from wake, first E3/D9, and second
E3/D9 separately. This will show where the repeated pattern starts rather
than repeating the same second-response observation. The image is
`build/iphone7plus/touch-report-probe/loader.bin` (SHA-256
`7f1f4745ce4e7473bd5162845e92c098aeca325fa15ea6f9b48883b86e471d06`)
and passed host checks. The phone showed `4F814F81` repeatedly on both E3/D9
responses and already on most of the WAKE response. Therefore the pattern
starts before the second D9 request. The current build records the first
firmware record boundary with a pending touch IRQ (`FIRST`), whether the IRQ
was pending before the final wait (`IRQ PRE`), and the two received bytes from
the final record's two-byte acknowledgement (`ACK14`). This distinguishes an
early edge from a firmware-ready edge and checks the last boot exchange. The
phone run showed `IRQ PRE 1`, so the final wait consumed an edge already
pending before it began. `FIRST` appeared to be zero but its last digit was
clipped in the photo; the user read `ACK14` as `0x4AD1`. The current revision
places `FIRST` where it can be read fully and adds `POST ACK`, the pending
status immediately after clearing the boot IRQ and before streaming. Its
SHA-256 is
`5cdce73744741ac6a875543e51194c388da01fb592ac5247819012d91fbd6d57`.
The phone showed `POST ACK 0`, `FIRST 3`, `ACK14 0x4AD1`, `FW IRQ 1`, and the
same invalid `D9 RESULT 0xFFFFFFFD` with repeated `4F814F81` RX. Thus the
initial boot IRQ was actually cleared, and a new edge appeared during the
large firmware write at record 2, before record 3 began. This timing does
not establish firmware readiness. Comparing the implementation with the
Sandcastle SPI controller driver revealed that its `spi_sync_transfer` calls
prepare the controller before every firmware chunk and ACK transfer, even
when the external CS remains asserted across chunks of one record. The
current source follows that sequence and captures ACK responses from records
2-5 and 13-14 for a focused phone comparison. This revision has not yet
been run on the phone. Its ignored loader is
`build/iphone7plus/touch-report-probe/loader.bin` (SHA-256
`c293c033e8b5c924adca049a402f06293ba9966177aa3f371b1e337e79a85bb4`);
the host checks and image build pass. The phone run again showed
`D9 RESULT 0xFFFFFFFD` and repeated `4F814F81` in the control responses,
even with per-transfer SPI2 preparation. The early and late ACK responses
were nonzero and varied, unlike the repeating D9 response. Therefore this
SPI setup correction did not produce a valid report, while the controller
was still responding during the firmware stream. The current image makes
two D9 reads during one powered boot, with a longer wait between them, and
displays immediate and late response words plus the late IRQ state. This
tests whether the invalid control reply is a readiness-timing issue. Its
ignored loader SHA-256 is
`d82415f058391c47f7732bed005110f388960b10b6e2dcd28d8c791c3b3fcdb1`;
the host checks and build pass, but this timed revision has not yet been
run on the phone. The timed phone run showed the same immediate and late
`D9 RESULT 0xFFFFFFFD`, `STATUS 0x4F`, `LENGTH 0x4F81`, and repeated
`4F814F81` RX, while `LATE IRQ` was `1`. This rules out a short readiness
delay as the explanation for the invalid reply. A host-side byte comparison
against the original Sandcastle firmware builder then exposed the `IDREF`
parser bug described above. The current report image embeds the corrected
stream; its SHA-256 is
`479c1a1243e38e347a64a19a49065a583651f814d6e7365279e6e0ae4bc3d51a`
when tested on the phone. Host tests and the build pass. The corrected stream changed
`D9 RESULT` from `0xFFFFFFFD` to `0`, with `FW IRQ 1`, `D9 STAGE 5`,
`D9 STATUS 0`, and `D9 LENGTH 16`. The first 16 D9 bytes were read and the
PMU high-voltage register was restored to `0x0F`. This establishes that the
firmware setup and D9 control read now work on this device. Contact IRQs and
touch packets still need a hardware test before live input can be enabled.
The event probe now sends Sandcastle's generation-2 `AF` reporting command,
waits for a contact IRQ while displaying `TAP SCREEN`, reads one bounded
packet, validates its header and checksum, and decodes the contact against
the signed D9 bounds. It always powers down the panel afterward. Its ignored
image is `build/iphone7plus/touch-event-probe/loader.bin`, SHA-256
`e0a899e61baf66fb022cef019c444fd8fd066eeabad32a030cc3bbf8c2be82ab`.
The first on-phone event revision again reached D9 successfully but finished
with `EVENT RESULT 0xFFFFFFFA` (`NO_EVENT`), `IRQ COUNT 0`, and no packet.
Its 15,000-iteration wait was much longer in real time than intended. The
revised probe shortens the wait, shows progress, and checks the active-low
IRQ line as well as the latched falling edge. Host tests and image build pass.
The revised event image produced `D9 RESULT 0`, `EVENT RESULT 0`, one IRQ,
and a checksum-valid 14-byte packet with zero contacts. The user was unsure
whether they tapped during the capture window, so active contact is still
unverified. A repeat with a deliberate tap is needed before changing the
contact protocol. A deliberate repeat showed the probe ended before the user
could tap: the first IRQ delivered a valid 14-byte status packet, whose
12-byte payload is too short to be a contact report. The event loop now skips
that packet and continues waiting for an active contact. On the phone, the
revised image reported `D9 RESULT 0`, `EVENT RESULT 0`, three IRQs, a
checksum-valid 56-byte packet, `CONTACTS 1`, normalized `X 0x03C8` (968)
and `Y 0x0109` (265), with HV restored to `0x0F`. This confirms the D11
firmware, interrupt, packet, and contact-decode path on this iPhone.

`sh platforms/iphone7plus/build-nexatrode-shell.sh touch` now builds a separate
touch-enabled shell image. A low-priority touch protection domain performs the
proven D11 startup once, polls GPIO 166 because the current seL4 iPhone port
exposes no external user IRQs, validates packets, and publishes a seqlock
protected touch snapshot through a shared 16 KiB region. It notifies the shell
on contact changes; a fresh press in the left or right half selects the
Settings or Phone demo view. The original `static` shell mode remains
available. Host touch tests and both shell image builds pass. On the iPhone,
the integrated image booted the Settings and Phone demo shell and displayed
`TOUCH READY`. A left-half tap switched to the single Settings view; after
release, a right-half tap switched to the single Phone demo view. This
confirms the first live touch-to-app path on hardware. That first image's
SHA-256 was
`47dc1e8c9264e00a9a256b8eafdbcac31c4928c21f016b8f1e42051f8cc43a38`.

The next touch shell maps the full 1080 x 1920 framebuffer and renders a
launcher, Settings page, and large Phone demo keypad. Digit, CLR, and DEL
buttons send the existing bounded routed-key request to the Rust Phone PD;
the Phone app still owns and publishes its text view. That first full-screen
keypad image had no call button or modem PD and was 1,153,628 bytes,
SHA-256 `dd6ec8f1c7abacff9ad19acb6ed4d6ccf7b4afc9f573ae6ec59b31943d014258`.
On the iPhone it filled the screen and showed `TOUCH READY`. Opening Phone
and tapping 5 changed its Rust-owned view to `DIAL`, `5`, `RING 0 SMS 0`;
DEL returned it to `IDLE`, and HOME and SETTINGS navigation also worked.
The next image adds a `CALL DEMO` button and the existing Rust fake-modem PD
on a Phone-only dial channel. Its current ignored
`build/iphone7plus/nexatrode-touch-shell/loader.bin` is 1,221,676 bytes,
SHA-256 `23fb95b0fe314f10698587fecd3cdb93c2a5ae277b9de8966f49c636deeaa4bb`.
Both static and touch builds pass. On the iPhone, entering `56` and tapping
`CALL DEMO` changed the Rust Phone view to `CALLING`, confirming the touch
shell -> Phone PD -> fake modem PD -> Phone view path. This is a simulated
dial only; the fake modem has no cellular hardware access.
No firmware or calibration bytes belong in Git. The current service polls the
GPIO IRQ state; user-space external IRQ delivery and broader input routing
remain future work.
Firmware comes from the OS filesystem in this IPSW; the HoolockLinux
kernelcache extractor is for other embedded firmware and does not extract
`D11.mtprops`.
