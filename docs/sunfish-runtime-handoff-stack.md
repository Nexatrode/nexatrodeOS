# Runtime handoff stack separation

The caller diagnostic boot photographed in IMG_9689 and IMG_9690 stayed on
`MPSS AUTH READY PASS`. It did not produce the intended caller panel. That
screen alone neither identifies the loaded image uniquely nor proves a CPU
stack fault. Do not interpret it as a new WLAN result.

Offline examination found a concrete compiled stack hazard on the successful
handoff reached directly from `init`. The Microkit entry and init frames remain
live while the old `pixel_mpss_runtime::start` constructs a runtime owner and
begins GLINK opening and RMTFS service. The same handoff from a later
notification has already unwound init, so these routes carry different demand.

Five selected, nested frames in the caller image sum to 5,251,632 bytes against
the unchanged 5,242,880-byte mapped stack: entry, init, start, opening, and the
RMTFS transfer frame. This exceeds the mapping by 8,752 bytes before other
descendants and TLS. The observation exposes a hazard; without a device fault
PC it does not establish the cause of the photographed stall.

The opt-in `sunfish-runtime-handoff-stack-split` feature separates construction
and begin into non-inlined helpers. Construction returns before begin reserves
its frame. The existing readiness and receipt checks, failed handoff behavior,
single-owner movement, begin behavior, and feature-off path are retained.
The Normal-NC SDK, eight table mappings, 5 MiB stack, FIFO checks, GLINK grant
ordering, QRTR credits and firmware provider are unchanged.

The new compiled image has a 32-byte start frame. Its selected construction
budget is 3,643,760 bytes and selected begin/transfer budget is 4,290,304 bytes,
leaving 952,576 bytes within the mapped stack. The regression gate requires at
least 262,144 bytes of reserve. It is explicitly a selected-frame check, not a
whole-program worst-case stack proof: TLS, indirect calls and other descendants
are outside its model. The ELF hash binds the report to the delivered binary.
Unknown SP writes, truncated prefixes and unsupported stack probes fail closed.

Actual-source host lifecycle tests cover readiness rejection, retained receipt
on failure, construction-before-begin, single ownership, repeated-start guards
and restoration after begin returns. Parser regression tests include malformed
and zero-length probes. Independent review confirmed helper separation in the
ELF and corrected a fail-open case in the first audit parser. Existing owner,
link, native fault and FIFO parity tests remain separate from these lifecycle
tests; fake-peer replay does not execute the runtime handoff itself.

The next private profile is
`.cache/sunfish-native-wifi/private/native-wrq-handoff-stack-v1`.
`RUNTIME HANDOFF START` shows step 1 before construction, step 2 after it
returns, and step 3 at begin's body. A successful handoff replaces this with
`WRQ CALLERS STACK SPLIT`, retaining the caller attribution needed to explain
the earlier increase from 40 to 77 post-return checks. Small startup paints
can affect elapsed timing and do not alter protocol message order.

The new image is not physically tested until its next cold temporary boot.
This change addresses the demonstrated handoff hazard; it makes no claim that
DATA, WLAN firmware reads, WLFW service or working Wi-Fi have been reached.
The prior Normal-NC and caller images and their device evidence remain intact.

Offline delivery verification passed: 559 owner tests in each feature arm,
212 link tests, nine handoff/parser tests, ten native fault controls, and 24
paired schedules with exact FIFO-log and peer-event parity. The packaged
authority audit rejected 304 mutations; the pinned SDK reproduced the entire
loader, CapDL and report, with all eight intended Normal-NC archive maps.
The two framebuffers rendered identically with legible, non-overlapping rows.

The temporary boot image is 68,911,104 bytes, SHA-256
`e1f3b1d57ca5e067e79cf49a30b06b82fba73e40ef50b49242e7c667b4b4e454`.
Private `manifest.json` binds its source, ELF, stack report, frozen caller
locations, SDK, firmware-input hashes and verification artifacts. Firmware
bytes and the boot image remain private and are not committed.
