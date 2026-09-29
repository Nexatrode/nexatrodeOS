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
checks the four-card launcher and Notes, Contacts, Calculator, and Clock at
exact 300 x 400 frames, then confirms Notes and Contacts data survive a cold
boot. The isolated Clock service owns the emulated PL031 RTC and publishes only
the formatted UTC display snapshot; the GPU client maps it read-only and the
input router does not forward keys to it. The test masks the changing time
digits while comparing the rest of the Clock frame exactly. It also verifies
touch-calculator `7 + 5 = 12` and physical-keyboard `8 + 2 = 10`, while all 25 OS
checks and app-isolation constraints pass. The pinned sDDF still has no
production VirtIO input driver; this input driver is a narrow QEMU test driver.

`./dev utm-graphics` runs those verifications and exports a separate
`dist/Personal OS Graphics.utm` profile with the VirtIO GPU, keyboard, and tablet
devices plus a disposable vault disk. The profile includes the four-workspace
launcher, Notes, Contacts, the Calculator touch keypad, and the read-only Clock
screen; the default serial profile is unchanged.
It enables native resolution so the 300 x 400 touch targets map consistently.
The exported profile boots and renders in UTM; physical keyboard navigation and
Calculator arithmetic have been checked there. Pointer clicks still need a
reliable direct manual check, since desktop automation did not consistently
target the intended VM window. If UTM keeps an older bundle configuration
cached, remove the stale library entry and open the fresh bundle. QEMU `virt`
has no display by default, and the guest needs matching graphics and input drivers
([QEMU ARM `virt`](https://www.qemu.org/docs/master/system/arm/virt),
[QEMU VirtIO GPU](https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html),
[UTM display settings](https://docs.getutm.app/settings-qemu/devices/display/)).
