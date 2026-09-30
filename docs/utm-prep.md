# UTM prep before Pixel 4a

Personal OS daily development stays on **UTM/QEMU**. Pixel work is a separate
feasibility track ([pixel-p0-boot-feasibility.md](pixel-p0-boot-feasibility.md)).
This checklist is what “ready enough on UTM” means before spending cycles on
UART cables and `sunfish` bring-up.

## What UTM is for

- Isolation, vault, seven-workspace Home, keyboard + tablet input
- Experimental graphics profile: `./dev utm-graphics` →
  `dist/Personal OS Graphics.utm`
- VirtIO net / TLS / sealed messaging on **QEMU** (`./dev test-net`); default and
  graphics UTM images stay **offline**

UTM does **not** replace Pixel boot, GENI UART, Wi‑Fi, modem, or power.

## Prep checklist

1. **Refresh graphics bundle** after UI/input changes:
   `./dev utm-graphics`
2. **Open** `dist/Personal OS Graphics.utm` in UTM with **USB Tablet** input
   (not mouse-only).
3. **Confirm amber guest cursor** follows the tablet; click launcher cards and
   app-bar buttons (Notes, Contacts, Calculator, Clock, Calendar, Messages,
   Settings).
4. **Keyboard path:** Tab between workspaces; Calculator `8 + 2` → `10`.
5. **Settings** shows local-only status (`LOCAL ONLY` / `VAULT LOCAL` /
   `NO NET YET`) and alert counts after modem smoke (`ALERTS RING N SMS M`, or
   `NO ALERTS YET` if nothing was injected).
6. **Messages** compose → `SEALED N` locally (still offline in the UTM image).
7. **Serial profile** remains separate: `./dev utm` / `dist/Personal OS.utm`
   for non-graphics work.
8. **Optional net:** `./dev test-net` on QEMU, or `./dev utm-net` for dual-UTM
   Shared Network bundles (`Personal OS Net A/B.utm`; see
   [network-spike.md](network-spike.md)). Graphics UTM stays offline.

## Still later on UTM

- Dialer UI on top of [modem-spike.md](modem-spike.md)
- Camera backends
- Graphics + net in one image (`timer_driver` name clash)
- Richer notification delivery beyond Settings alert counts
- Production TLS/PKI beyond the test-net TOFU smoke

## Pixel handoff (when UART adapter + backup ready)

Do not flash until P0 go/no-go: SBU UART capture + userdata backup (+ live slot
dumps). Until then, plain USB‑C can still do ADB/fastboot inventory and backups
only. See [pixel-p0-boot-feasibility.md](pixel-p0-boot-feasibility.md) §7.
