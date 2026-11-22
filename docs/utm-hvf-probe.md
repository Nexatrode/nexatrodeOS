# UTM hardware acceleration probe

This is an isolated development experiment, not a configuration for an existing wallet or SimpleX database.

On the tested Apple Silicon Mac, UTM's HVF backend booted a QEMU `virt` guest with `virtualization=off,gic-version=3,kernel-irqchip=on`. The test used a separately built seL4 EL1 debug kernel configured for the host's 24 MHz counter. The standard EL2 guest and an EL1 guest with the original binaries did not work under HVF.

The sDDF ARM timer driver writes the physical timer, which faulted under HVF even when the kernel exported it. The disposable test replaced that driver with a counter-based polling service and a budgeted low-priority poller. This burns CPU and does not establish reliable timer behavior; it is not a production solution. The EL1 kernel's virtual timer and the sDDF timer service need a proper compatible design and isolation review.

The reduced, offline Slint keyboard fixture booted with components compiled for EL1, GICv3, and CPU-only shared buffers mapped as normal cached memory. It does not exercise the full SimpleX agent, network, wallet, or a user's vault. The fixture displayed a synthetic chat, accepted a keyboard character, and opened and closed its on-screen keyboard. No real message was sent.

In one instrumented run, the test-only F8 shortcut produced guest input-to-frame-publication intervals of approximately 4.5 ms for closing and 18.0 ms for opening the keyboard. A typed character took approximately 0.8 ms to frame publication. Instrumented CPU pixel conversion took approximately 0.3–0.5 ms for those keyboard frames. These are not end-to-end tap-to-display measurements, and the reduced fixture, timer, workload, and kernel configuration differ from the existing software-emulated build; the numbers are not a controlled production benchmark.

The next integration work is to replace the polling timer with a reliable HVF-compatible service, rebuild every protection domain for the same kernel ABI, validate memory attributes and DMA coherency, and test storage, network, SimpleX, wallet, input and sustained timing before considering any migration of an existing VM. A controlled comparison should use identical workloads and measure the time from input through visible display presentation.

## Source rebuild comparison on the 222-block range

Three alternating pairs ran against `https://rpc.nexawal.com` using HVF, the host CPU, and an independent disposable wallet disk for each boot. The timed range was 3,809,160 through 3,809,381 inclusive. Each rate covers the first request through the 222nd ordered commit.

| Build | Run 1 | Run 2 | Run 3 | Median blocks/s | Median elapsed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Pinned handoff-prime-m1-100 | 139.5 | 159.5 | 161.1 | 159.5 | 1392 ms |
| Source rebuild | 342.4 | 128.8 | 298.2 | 298.2 | 745 ms |

All six trials committed the same ordered result for 222 blocks (SHA-256 `99848eebc2035c646d6b5cacf94eefe97dcd8164c1ff18721565656ac273b080`). Each fetched three 74-block responses totaling 6,401,176 bytes. All four scan lanes participated and the fallback count was zero. Each trial continued through its live tip and verified completion.

The source rebuild’s median throughput differed from the pin by +87.0%. One-minute host load during the samples ranged from 21 to 208. Cold wallet open through first fetch ranged from 121 to 303 seconds and included retry backoff. This interval is outside the timed scan. Three trials per build on a shared host give limited evidence about repeatable timing differences.
