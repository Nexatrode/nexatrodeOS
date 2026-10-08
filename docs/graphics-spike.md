# QEMU graphics feasibility spike

`./dev test-graphics` first builds the pinned sDDF GPU example as a separate
Microkit system. It then merges the GPU client, virtualiser, driver, and timer
driver into a generated copy of the regular Personal OS system and boots that
copy with QEMU's ARM `virt` machine. QMP captures a frame only after the exact
300 x 400 RGB source photo appears and the regular system's 25 isolation checks
finish. The screenshots are saved as
`build/sddf-gpu-spike/graphics-smoke.png` and
`build/sddf-gpu-spike/graphics-system-smoke.png`. Both runs reject guest faults.

The pinned GPU example predates parts of the pinned sDDF API. The local smoke
build supplies compatibility headers for removed VirtIO helpers and allocator
initialization, generates the timer's device-resource metadata, and caps the GPU
virtqueue at QEMU's 64-entry limit. Its uncached shared request queue can fault
when a library copy emits an unaligned wide store, so the sDDF overlay copies
those request records with byte-sized volatile stores. These changes are
recorded in `patches/sddf-personal-os.patch` and
`tools/sddf-gpu-compat/`.

This proves the GPU driver and QEMU device can render within a system that also
boots the vault and isolated apps. The integrated run uses a disposable test
disk and generated system file; the regular `./dev build` image and default
Personal OS UTM profile remain serial-only. `./dev test-input` adds a budgeted
VirtIO input driver and a separately budgeted Rust router to that system. The
32-record shared ring has bounded overflow accounting; the router validates
keyboard/tablet events and delivers them over app-specific endpoints. QEMU
checks the eight-app launcher and Notes, Contacts, Calculator, Clock,
Calendar,
Messages, and Settings at exact 300 x 400 frames, then confirms Notes, Contacts,
and
Calendar data survive a cold boot. The isolated Clock service owns the emulated
PL031 RTC and publishes only the formatted UTC date and time snapshot; Messages
is an offline peer-key shell with local PSL1 sealed stubs; Settings is a matching read-only stub PD. The GPU client maps them read-only
and the
input router does not forward keys to them. The test masks the changing date and
time digits while comparing the rest of the Clock frame exactly. It also verifies
touch-calculator `7 + 5 = 12` and physical-keyboard `8 + 2 = 10`, while all 26 OS
checks and app-isolation constraints pass. The pinned sDDF still has no
production VirtIO input driver; this input driver is a narrow QEMU test driver.

`./dev utm-graphics` runs those verifications and exports a separate
`dist/Personal OS Graphics.utm` profile with the VirtIO GPU, keyboard, and tablet
devices plus a disposable vault disk. For the same UI with VirtIO net (shared GPU
`timer_driver`), use `./dev test-input-net` / `./dev utm-graphics-net`. The profile includes the eight-app
launcher, Notes, Contacts, the Calculator touch keypad, the read-only Clock
screen, Calendar reminders, the Messages peer-key shell, and the Settings stub; the default
serial profile is
unchanged.
It enables native resolution so the 300 x 400 touch targets map consistently.
Keep UTM on USB Tablet mode; an amber guest cursor follows the absolute tablet
position so clicks are visible without switching to a mouse device. The exported
profile boots and renders in UTM; physical keyboard navigation and
Calculator arithmetic have been checked there. If UTM keeps an older bundle configuration
cached, remove the stale library entry and open the fresh bundle. QEMU `virt`
has no display by default, and the guest needs matching graphics and input drivers
([QEMU ARM `virt`](https://www.qemu.org/docs/master/system/arm/virt),
[QEMU VirtIO GPU](https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html),
[UTM display settings](https://docs.getutm.app/settings-qemu/devices/display/)).
See also [UTM vs hardware limits](utm-vs-hardware.md).

## Slint UI toolkit feasibility

The separate [Slint Notes experiment](../experiments/slint-notes/README.md)
renders a responsive Notes screen through Slint 1.18.1's software renderer at
300 x 400, 1080 x 2340 for Pixel 4a (estimated scale 2.75), and 1080 x 1920 for
iPhone 7 Plus (native scale 2.6087); it also checks a 440 x 900 portrait frame.
Its host test dispatches touch and key events through Slint and verifies text
entry and Save. A simulated bottom keyboard inset moves Save above the keyboard
and shrinks the editor; touch and text entry pass with the inset enabled at both
phone resolutions. The no-`std` UI library also passes a Rust type-check for the
custom seL4/Microkit AArch64 target. A bounded 300 x 400 RGB8 shared-frame
contract and a validated RGB-to-BGRA reader are also in place, with matching C
and Rust tests; the reader falls back to the existing view renderer when no
frame is published. Separately, the iPhone 7 Plus now has an isolated native
Slint Notes protection domain with a bounded boot allocator, a Microkit
`Platform` adapter, direct software rendering to the mapped D11 framebuffer,
and contact events from the existing touch service. Phone runs showed the
Slint UI and a tap changed the status through its Save callback. A temporary
touch marker confirmed that a center-of-Save tap mapped onto the visible Save
button. Display color tuning and first-frame presentation remain to be checked.
It is
one experimental screen, not the multi-workspace QEMU app suite.

`./dev utm-pixel4a-probe` builds a separate GPU-only Microkit loader with a
16 MiB client frame region and sends the Pixel 4a Slint preview through QEMU's
VirtIO GPU at exactly 1080 x 2340. QEMU verifies that the captured guest frame
matches every source pixel, then exports
`dist/Personal OS Pixel 4a Probe.utm`. This confirms native-size buffer
allocation and scanout in the VM. The probe shows a static Slint-rendered image;
Slint still does not execute inside the guest, and the probe does not include
the regular OS services or touch routing. The existing 300 x 400 Personal OS
and Graphics UTM profiles are left intact.
