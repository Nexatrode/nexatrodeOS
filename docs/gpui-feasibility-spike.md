# GPUI feasibility spike

## Question

Can GPUI-Mobile provide the UI layer for this seL4/Microkit system, and what is
the smallest UTM experiment that would answer the remaining questions?

## Result so far

The current GPUI-Mobile checkout is pinned to GPUI `0.2.2` at Zed commit
`5688167d224b5eca54875d49afb8bfd73a07915a`. Its Android backend uses Android
activity/window APIs, `wgpu`, and a native Android event loop. The platform
implementation is not a portable `no_std` renderer. GPUI itself has a broad
`Platform` contract for event loops, executors, windows, text, input, and other
OS services; it uses the standard library and is built as a GPU-accelerated UI
framework.

An Android-target compile check passed on this development Mac:

```sh
CC_aarch64_linux_android="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android26-clang" \
CXX_aarch64_linux_android="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android26-clang++" \
AR_aarch64_linux_android="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-ar" \
cargo check --locked --target aarch64-linux-android --no-default-features --lib
```

This verifies the Android backend's Rust dependencies and code compile for
Android. It does not run an app or validate UTM, seL4, or our display path.

## Fit with the current system

Our graphical UTM image is a bare seL4 system. A trusted C client renders app
snapshots into pixels and sends them to QEMU's VirtIO-GPU 2D device. A separate
Rust input service routes validated keyboard and tablet events to app protection
domains. GPUI-Mobile expects Android/iOS native platform services and a `wgpu`
renderer surface, while the guest has neither an Android runtime nor Vulkan,
OpenGL, or a `wgpu` surface. Its Android backend therefore cannot be linked into
the current image as-is.

There is also a security-boundary question before framework integration: today
each app owns its state and publishes a bounded view snapshot, while the trusted
display client controls the final pixels. Running an entire GPUI application in
one component would combine app state and UI authority. A future integration
must retain distinct app protection domains and define how a GPUI view receives
only its app's state and events.

## Smallest useful UTM experiments

1. **GPUI visual/rendering control:** run a tiny GPUI screen in a Linux guest
   under UTM/QEMU, using VirGL if the host supports it. This measures the
   framework's look, layout, and basic interaction in a GPU-capable Linux
   environment. It is a control experiment only; it does not demonstrate the
   seL4 port or Pixel graphics support.
2. **seL4 display-boundary experiment:** keep the existing protection domains
   and input router. First establish whether the pinned GPUI render pipeline
   can produce CPU-readable RGBA pixels without an Android/Linux window. Then
   prototype a bounded frame handoff to the current VirtIO-GPU client and measure
   frame time, memory, and input latency at 300 x 400. Stop if GPUI requires a
   GPU API or OS service that this system cannot provide without a major port.
3. **Architecture decision:** if the frame handoff works, estimate the custom
   `Platform`/executor/text/input work and design a per-app component boundary.
   If it does not, retain GPUI as a design reference and compare a renderer
   designed for embedded software pixels before selecting the OS toolkit.

There is already a Debian 13 + Phosh ARM64 UTM profile on this Mac configured
with `virtio-gpu-gl-pci`. It is a candidate for experiment 1, after making a
disposable copy of its VM disk and confirming the guest exposes a usable GL or
Vulkan adapter. A GPUI Linux app in that guest would test the Linux desktop
backend and visual behavior; it would not test GPUI-Mobile's Android backend or
the seL4 display path. Neither experiment should change the working `Personal
OS Graphics.utm` profile.

## Decision

Keep GPUI as the preferred UI hypothesis, not a committed platform choice. The
Android compile check is a useful positive result for the Pixel/Android
ecosystem, but direct use inside seL4 remains unproven and requires a substantial
platform and renderer integration. Do not begin a full app rewrite around GPUI
until the CPU-readable frame probe and per-app isolation design have evidence.
