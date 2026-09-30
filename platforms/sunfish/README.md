# Pixel 4a (`sunfish`) seL4 platform scaffold

Desk-only P0/P1 sketch. **Not** wired into `./dev build`, Microkit, or UTM.
Do **not** flash anything produced from this tree.

Addresses and reserved regions come from the mainline-style DT under
[`docs/pixel-p0-assets/`](../../docs/pixel-p0-assets/README.md) and the notes in
[`docs/pixel-p0-boot-feasibility.md`](../../docs/pixel-p0-boot-feasibility.md).

## Contents

| File | Role |
| --- | --- |
| [`overlay.dts`](overlay.dts) | Minimal memory / GIC / timer / UART nodes for a future seL4 overlay |
| [`geni_uart.h`](geni_uart.h) | GENI SE register offsets used by early putchar |
| [`geni_uart_putchar.c`](geni_uart_putchar.c) | Polling TX stub (assumes bootloader left UART usable) |

## Assumptions (explicit)

1. First port is **EL1**; EL2 / driver-VM is out of scope here.
2. Single CPU core; PSCI multi-core later.
3. Debug UART is `uart8` @ `0x00a88000` (`qcom,geni-debug-uart`, SPI 355).
4. Early putchar **does not** program GCC clocks, interconnect, or TLMM. It only
   waits on the TX FIFO and writes bytes. That only works if ABL (after
   `fastboot oem uart enable`) already configured the SE — verify with an SBU
   UART capture before relying on it.
5. DRAM size is a planning placeholder (`0x180000000` ≈ 6 GiB). Reserved
   `no-map` regions from the sunfish DT must stay out of the free pool; the
   overlay lists the large ones, not every carve-out.

## Next real steps (still no flash)

1. Prove SBU UART capture of bootloader text.
2. Confirm userdata backup + dump live `boot_b` / `vbmeta_b` / `dtbo_b`.
3. Port this overlay into an seL4/Microkit board tree and link the putchar into
   elfloader / kernel console.
4. Pack a boot v2 image and attempt a temporary boot or controlled single-slot
   flash **only after** the P0 gates pass.
