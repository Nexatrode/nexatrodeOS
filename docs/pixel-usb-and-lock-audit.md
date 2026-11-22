# Pixel 4a lock and USB development audit

This is a source and documentation review, not an on-device security test.
It does not alter the phone, Wi-Fi work, bootloader state, or saved data.

## What is established

- `platforms/sunfish/slint-display.system` describes an experimental display,
  touchscreen, volatile notes service and reset domain. It does not list an
  Android `adbd` service, a USB gadget, or an authorization service. This is a
  statement about that particular system description, not every image or every
  USB interface that firmware may expose.
- `platforms/sunfish/build-slint-display.sh` uses the debug Microkit SDK. The
  bring-up README documents a debug UART and previously enabled UART access.
  The Slint adapter and the active touch service previously logged coordinates
  and raw touch packets. Those logs have been removed from source; the modified
  touch service has been compiled, but the modified image has not been tested on
  the Pixel. Other debug channels and probe images still need review before
  entering a real passphrase on a device.
- `docs/pixel-key-feasibility.md` records an earlier Pixel `sunfish` check with
  bootloader `unlocked: yes` and Android verified-boot state `orange`. It also
  records an Android-mediated StrongBox test, not a seL4 Titan M interface.
  These are historical results, not a new reading of the current device.
- Android's ADB authorization belongs to Android. A standalone seL4 image does
  not inherit it, while bootloader/recovery USB interfaces exist outside that
  image and need separate review. The Pixel bring-up README documents temporary
  `fastboot boot`; do not change bootloader state as part of this audit.

## Experimental UTM lock

The accelerated UTM build includes an optional software passphrase gate. The
Privacy settings offer enrollment, manual lock and authenticated disable; an
enabled lock also engages after 60 seconds without input. It starts locked after
reboot, does not display the passphrase or its length, and delays repeated wrong
attempts. The credential uses a fresh salt and PBKDF2-SHA256. Lock settings are
stored in authenticated redundant vault records that preserve the existing
system scalar; unreadable or interrupted records fail closed.

The disposable UTM test exercised enrollment, wrong and correct passwords,
navigation blocking, idle locking, persistence across reboot, interrupted and
corrupted records, and disable. This does not establish security against an
attacker with control of the UTM host, image, or disk, nor is it a Pixel test.
The following work remains before making stronger device-security claims:

1. Hardware-backed retry limits and anti-rollback: the current retry counters
   are in memory and reset after restart; snapshots can also be restored.
2. Move the expensive password derivation off the UI thread without allowing
   navigation or concurrent authentication to bypass the gate, and audit all
   debug paths and memory handling for the deployed image.
3. Integrate and test a trusted startup gate on Pixel; this UTM implementation
   is not wired into the Pixel display image.
4. Device-specific USB enumeration and negative access tests with an independent
   Pixel image, after a production/debug interface policy exists. Confirm the
   running image and bootloader state; do not assume lack of ADB means all USB,
   UART, DMA, fastboot, or physical access is secured.

This milestone does not provide passphrase-wrapped vault keys, secure boot,
rollback-resistant hardware rate limits, or verified removal of all secrets
from RAM. A later encrypted-vault migration needs its own recovery and data-loss
testing before it can be applied to a real wallet or chat store.
