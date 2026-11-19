# Slint platform matrix

The shared Slint screen source and app wiring live in
[`experiments/slint-notes/ui/full_suite.slint`](../experiments/slint-notes/ui/full_suite.slint)
and [`src/suite.rs`](../experiments/slint-notes/src/suite.rs). The image and
input adapters are platform-specific. The machine-readable list of targets is
[`platforms/slint-build-matrix.json`](../platforms/slint-build-matrix.json);
run `python3 tools/check-slint-platform-matrix.py` to check its page-size and
source/build-path references.

| Target | seL4 page size | Slint path today | Platform-specific work |
| --- | ---: | --- | --- |
| UTM / QEMU `virt` | 4 KiB | Full Slint suite rendered inside a Microkit guest PD and sent through VirtIO GPU; tablet taps navigate all eight pages and exercise app controls | QEMU boot, VirtIO display/input, guest frame handoff |
| Pixel 4a `sunfish` | 4 KiB | Interactive Slint suite in a temporary Microkit RAM boot | Fastboot wrapper, reserved framebuffer, camera inset, touch service, volatile Notes vault |
| iPhone 7 Plus | 16 KiB | Interactive Slint suite in a Pongo-loaded Microkit image | Pongo entry/loader, 16 KiB kernel and SDK, D11 framebuffer and touch service |

The common `full-suite-ui` feature selects the same screen source for the
device adapters. Shared app-level behavior belongs in `suite.rs`; device
details such as framebuffer formats, touch coordinates, image dimensions,
page granules, and service channels belong behind each adapter. Select page
size and board configuration at build time. The app should learn about a
feature through the capabilities or services it receives, rather than testing
which phone it is running on.

This is a build organization in progress, not yet a unified OS release matrix.
The Pixel image currently has a volatile Notes vault and no Wi-Fi service. The
iPhone and Pixel image builders package different Microkit systems. UTM now
boots the Slint runtime inside a guest protection domain; `./dev test-slint-utm`
checks all eight app pages against host-rendered references. It enters text in
Notes, Contacts, Calendar, and Messages; calculates `1 + 2`; and verifies the
Phone demo’s dial and no-modem call states. These frames are also compared to
host-rendered references, so the test covers both input routing and visible
state updates. Notes, Contacts, and Calendar now load and save through narrow
protected calls to their owning app domains, which mediate access to their
respective vault namespaces; the guest UI receives no vault capability.
Calendar currently saves the selected reminder title while preserving date,
time, and sibling reminder data in its model. The integration test checks
readback and confirms all three records reload after a QEMU reboot using the
same disk. This uses the current development storage configuration and is not
a production encryption claim. Other Slint app data is still volatile or
demo-only. The regular UTM app screens remain available as a separate
implementation while the Slint path expands its service coverage.

To rebuild and check all three paths, run:

```sh
sh platforms/check-slint-build-matrix.sh
```

This renders and exercises the shared UI on the host, builds the iPhone Pongo
loader and the Pixel Fastboot image, and does not upload or flash either phone.
The device build steps use the local Microkit SDKs and, for Pixel image
packaging, the backed-up Lineage boot image described in
[`platforms/sunfish/README.md`](../platforms/sunfish/README.md).

The next architectural milestone is to connect the remaining stateful Slint
apps to their own protected storage services, then give each device adapter the
same service contracts while retaining its platform-specific display, input,
boot, and page-table setup.
