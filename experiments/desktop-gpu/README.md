# Native seL4 GPU feasibility checkpoint

This is an isolated, diskless Microkit probe. It maps only the QEMU VirtIO MMIO
transport window and its own DMA allocation. It has no block/network driver,
wallet, chat database, installation key, application surface or clipboard.
It never changes an existing VM's display configuration.

The probe negotiates modern VirtIO, discovers GPU capability sets, retrieves
them with fenced requests, and creates a VirGL context, retained until device reset for scanout. It then creates a
64×64 render target, clears it through VirGL, and checks every BGRA pixel after
a fenced transfer back to guest memory. A checkerboard texture is uploaded once;
three overlapping textured rectangles are composed at sixteen positions and
each output is compared with a CPU reference. These use VirGL blits, not a
complete OpenGL implementation or a general shader API.

The current CPU desktop is unchanged. Passing this probe does **not** prove
physical host GPU identity, full-resolution performance, presentation latency,
Slint GPU rendering, a production compositor, or SDL/wgpu compatibility.

## Reproduce

```sh
python3 tests/desktop_gpu_probe_build.py \
  --inputs /path/to/public/utm-accelerated --output build/desktop-gpu/new-probe
caffeinate -di python3 tests/desktop_gpu_probe_utm.py \
  --guest build/desktop-gpu/new-probe \
  --template /path/to/public/utm-accelerated/template.plist \
  --device virtio-gpu-gl-device
```

Run the same loader with `--device virtio-gpu-device` as a 2D control. It should
report no VirGL context and no rendering test. Build a separate output with
`--skip-clear` and run the GL case as a negative control: it must fail pixel
verification rather than report rendering success. The runner exits nonzero for
that deliberate failure; inspect its `result.json` for the pixel failure and
successful cleanup. A rendering failure must never become a software-rendered
"GPU pass".

Each run creates a uniquely identified, diskless VM, saves serial output and
results outside the bundle, and stops/deletes only that VM in its cleanup path.
Fresh output/case directories are required to avoid overwriting evidence. Loader
and source digests are verified before launch. No saved-data fixture is imported.

## Results on November 24, 2026

Installed UTM 5.0.8 accepted `virtio-gpu-gl-device` with the existing ARM64
Microkit SDK. The 2D control advertised no capability sets. The GL device exposed
capset IDs 1, 2 and 4, and accepted fenced context creation; an earlier capability-only case also
verified context destruction.
Clear/readback and the retained-texture test passed. Omitting the render command
failed on pixel zero, proving the check detects the absent draw. Every probe VM
was stopped and deleted. See ignored `build/desktop-gpu/` outputs for raw logs,
source/loader hashes and per-case results. The small readback experiment's
elapsed time includes command submission, polling, readback and verification;
it is not a desktop frame-rate benchmark.

The first render attempt exposed an uncached-memory alignment fault from merged
wide stores. The probe now uses `-mstrict-align` and explicit device barriers.
This is part of the tested compile contract, not a change to seL4 or Pixel code.

The full-resolution variant (`--desktop-size` on the builder) also passed
initial pixel verification over a 2560×1600 target and sixteen retained-texture
updates. However, scanout capture remained black after successful SET_SCANOUT
and RESOURCE_FLUSH responses, including after retaining the context and polling
for eight seconds. The full-resolution runner therefore correctly reports
`passed: false` with `GPU readback passed but scanout pixels did not match`.
This is an unresolved presentation boundary; it does not establish that UTM
cannot display this resource. The captured pixels and QEMU device tree are
retained for investigation. No accelerated desktop default was enabled.

Next: resolve and verify presentation, identify the host rendering backend,
test buffer lifetime/reset and collect equivalent CPU/GPU timings before
integrating the three real app windows. The capability result supports further GPU development;
it does not yet justify enabling accelerated desktop composition by default.

## Protocol references

- [VirtIO GPU protocol](https://docs.oasis-open.org/virtio/virtio/v1.3/virtio-v1.3.html)
- [VirGL command protocol](https://gitlab.freedesktop.org/virgl/virglrenderer/-/blob/main/src/virgl_protocol.h)
- [UTM display support](https://docs.getutm.app/settings-qemu/devices/display/)
