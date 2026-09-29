# Pixel 4a secure-key feasibility

This combines desk research with a temporary on-device test on the Pixel 4a
(`sunfish`). The probe confirmed that Android can generate a StrongBox-backed
AES-256 key and perform AES-GCM with a 12-byte provider-generated IV, a 16-byte
tag, and associated data. A wrong-AAD check failed authentication as expected;
two encryption initializations produced different IVs in the sample. The probe
reported `security_level=STRONGBOX`, deleted its alias, and uninstalled the
temporary app. It did not test a seL4 access path, secure boot ownership, or
anti-rollback. Repeat the check with `python3 tools/pixel-keystore-probe/build.py
--run`.

Google lists the Pixel 4a as including a Titan M security module. The AOSP
`device/google/sunfish` configuration registers a StrongBox Keymaster service
provided by Citadel; a published device change upgrades that StrongBox service
to Keymaster 4.1. This establishes that the Android device stack was designed to
use the secure hardware, not that arbitrary operating systems can address it
directly. Sources: [Google Pixel 4a specifications](https://support.google.com/pixelphone/answer/16043605),
[AOSP sunfish StrongBox change](https://android.googlesource.com/device/google/sunfish/%2B/7709aa406cbbf700ddaf208c56a9075232015c87%5E%21/).

Android documents the access path as Keystore clients calling the Keystore
daemon, which stores encrypted key blobs and calls an OEM KeyMint/Keymaster HAL;
that service marshals requests to a trusted application in a secure environment.
Key material stays available to that trusted app for cryptographic operations.
The Android HAL API is platform-internal. Source: [AOSP hardware-backed
Keystore architecture](https://source.android.com/docs/security/features/keystore).

The practical inference is that a standalone seL4 image cannot use the Pixel's
StrongBox merely by adding a Rust crypto-provider implementation. The public AOSP
device tree documents an Android service path, while the secure-world transport
and device-specific firmware are not a documented seL4 API. Reusing Android's
vendor services would bring a substantial part of Android's boot and service
stack into the trusted computing base. Direct access would need a supported,
documented interface and a trusted boot/ownership story; neither is demonstrated
by the current project.

The vault now uses a `VaultCryptoProvider` operation interface, so record code
does not receive key bytes. Its QEMU implementation still performs software
crypto in the vault process with a fixed development key; it does not keep a key
inside Titan M. A real device provider would need a trusted service that accepts
AEAD operations for the current AES-256-GCM-SIV format. The Pixel probe confirmed
StrongBox-backed AES-GCM, which is a different algorithm, so integration requires
either a documented trusted GCM-SIV service or a versioned record-format change.
Android Keystore remains mediated by Android and has not demonstrated a seL4
access path.

The extended probe matches the proposed PVLTREC2 AES-GCM parameters at the
Android API boundary: 96-bit IV, 128-bit tag, and authenticated namespace-style
AAD. Its fresh-IV sample is only a compatibility check, not a proof of nonce
uniqueness across the life of a key. See the [PVLTREC2 migration proposal](vault-record-v2-design.md).

For the Pixel milestone, first boot the handset with its existing supported
software and confirm the hardware-backed Keystore/StrongBox level through
Android's Keystore APIs, optionally adding key attestation for independent
verification. Then inventory the unlocked boot path, exposed secure
services, and available source/vendor components for `sunfish`. The boot,
UART, recovery, and seL4 platform checklist lives in
[pixel-p0-boot-feasibility.md](pixel-p0-boot-feasibility.md); that study stays
read-only with respect to OS partitions. Do not claim seL4-backed key
protection or anti-rollback until a prototype demonstrates a trusted request
path under the seL4 boot chain. Until then, keep the development provider
confined to QEMU and use the Pixel for boot, display, input, and device
driver feasibility work.

## Non-destructive handset smoke test

When the phone is available over USB, start with read-only Android queries:

```sh
adb devices -l
adb shell getprop ro.product.device
adb shell getprop ro.build.version.release
adb shell getprop ro.boot.verifiedbootstate
adb shell getprop ro.boot.slot_suffix
adb shell pm has-feature android.hardware.strongbox_keystore
```

Expect product `sunfish`; save the Android release, verified-boot state, slot,
and StrongBox feature result. Do not include the device serial in project logs.
Then use a small Android test app to request an AES key with
`KeyGenParameterSpec.Builder.setIsStrongBoxBacked(true)`, retrieve its
`KeyInfo.getSecurityLevel()`, and perform an encrypt/decrypt round trip. Use a
temporary random alias and delete it after the test. A `STRONGBOX` security level
and successful operation confirm Android-mediated StrongBox use for that key;
they do not show that a separate seL4 image can access Titan M. Android documents
the StrongBox request and security-level APIs in the [KeyGenParameterSpec]
and [KeyInfo] references. StrongBox can reject unsupported algorithms or
parameters, so record the exception as a capability result rather than falling
back silently to a software key.

For the boot-path inventory, enter the bootloader using the normal key
combination or `adb reboot bootloader`, then query `fastboot getvar product`,
`fastboot getvar unlocked`, and `fastboot getvar current-slot` if supported. Do
not run `fastboot flashing unlock`, `fastboot flashing lock`, `fastboot flash`,
or `fastboot erase` as part of this inspection. Android's documented unlock
transition erases user data; unlocked boot also displays a warning because the
loaded OS is not verified by the stock root of trust. Sources: [AOSP bootloader
state](https://source.android.com/docs/security/features/verifiedboot/device-state),
[AOSP boot flow](https://source.android.com/docs/security/features/verifiedboot/boot-flow),
[Android KeyGenParameterSpec](https://developer.android.com/reference/android/security/keystore/KeyGenParameterSpec),
and [Android KeyInfo](https://developer.android.com/reference/android/security/keystore/KeyInfo).

The handset test ran on 2026-09-27. Read-only checks reported product `sunfish`,
Android 16, verified-boot state `orange`, slot `b`, and StrongBox feature `true`.
The temporary app requested StrongBox-backed AES-256, Android reported security
level `STRONGBOX`, and AES-GCM encryption/decryption passed. The app reported
`alias_cleanup=PASS`; it was then uninstalled, and Package Manager confirmed it
was no longer installed. Fastboot independently reported product `sunfish`,
`unlocked: yes`, and `current-slot: b`. The phone was rebooted back into Android
and ADB reconnected. No flash, erase, unlock, or lock operation was run.

Reproduce the Android-mediated test with:

```sh
python3 tools/pixel-keystore-probe/build.py --run
```

The probe has no Android permissions or network access. It builds a temporary
APK with the local Android SDK, installs and runs it, then uninstalls it. This
test confirms StrongBox use through Android; it does not test whether seL4 can
reach Titan M.

[KeyGenParameterSpec]: https://developer.android.com/reference/android/security/keystore/KeyGenParameterSpec
[KeyInfo]: https://developer.android.com/reference/android/security/keystore/KeyInfo
