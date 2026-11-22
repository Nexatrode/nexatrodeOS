# Apple HVF multicore boot probe

This experiment builds a two- or four-core AArch64 EL1 Microkit SDK and boots one active protection domain per vCPU. It uses Apple Hypervisor.framework, a GICv3 and the MCS scheduler. The guest has a serial port, no disk and no network. Each SDK can also compose another SDF with protection domains assigned using `cpu="N"`.

## Build and run

Run these commands from the repository root on Apple Silicon. Install the repository’s Homebrew LLVM, LLD and QEMU tools and the seL4 build dependencies CMake, Ninja, dtc and Python. The test used Clang/LLD 23.1.6, QEMU 11.1.3, Python 3.12.9 and seL4 Python dependencies 0.7.0. The repository’s setup command supplies the verified Microkit release SDK.

```sh
./dev setup
python3.12 -m venv .cache/hvf-smp-venv
.cache/hvf-smp-venv/bin/pip install sel4-deps==0.7.0
export PATH="$(brew --prefix llvm)/bin:$PWD/.cache/hvf-smp-venv/bin:$PATH"

for cores in 2 4; do
    python experiments/hvf-smp/build.py "$cores" || break
    dir="build/hvf-smp/$cores-hvc-fixed"
    python experiments/hvf-smp/run.py "$cores" "$dir/probe.img" --output "$dir/hvf-on" || break
done
```

`build.py` fetches the pinned commits into a new output directory. It uses the local Git object caches when available. The released host Microkit tool and released SMP CapDL initialiser are reused. The seL4 kernel, Microkit loader, monitor, runtime library and per-core PDs are built from source. All configuration and source edits apply inside the new build directory. Use a new `--output` for an additional build or capture.

The board is `qemu_virt_aarch64_el1`, with `smp-debug`, 2 GiB RAM, GICv3, the Apple host’s 24 MHz architectural counter, `KernelMaxNumNodes` equal to the vCPU count and one scheduling domain. The build uses the existing Microkit CPU-affinity SDF support. The loader starts the secondaries via PSCI. Homebrew QEMU uses `--conduit hvc`; the tested UTM 4.7.5 QEMU accepted the standard SMC conduit (`build.py 4 --conduit smc`). The runner uses Homebrew QEMU and starts only the primary at the loader entry point; the firmware starts the remaining vCPUs in response to the PSCI calls.

For a hardware interrupt-controller regression control, build with `--original-gicr` and run `probe.img` from the resulting `N-hvc-original` directory. Running the same original image with `--irqchip off` exercises QEMU’s userspace GIC while HVF still runs the vCPUs. Each capture includes the QEMU command and image SHA-256 in `result.json`, plus the serial log and per-vCPU register dumps.

## What the probe checks

Each pinned PD continually writes the architectural counter into its own cache line in a shared page. CPU 0 samples every secondary over a 50 ms interval. After observing positive deltas, the host pauses QEMU and reads `X25` separately on each vCPU. The run passes when the clock frequency matches the configured 24 MHz and every CPU index has the expected user-mode tag `0x534d5000 + index`. This demonstrates execution on the assigned guest vCPUs. Physical-core placement remains under host scheduling.

| Tested kernel and interrupt controller | 2 vCPUs | 4 vCPUs |
| --- | --- | --- |
| Original GICR read, hardware GIC | CPU 1 halted finding its redistributor | CPUs 1–3 halted finding their redistributors |
| Original GICR read, userspace GIC | All tags and counter progress passed | All tags and counter progress passed; three spurious interrupts during boot |
| Volatile GICR read, hardware GIC, clean upstream build | All tags passed, secondary advanced 1,218,104 ticks | All tags passed, secondaries advanced 1,213,542 / 1,255,080 / 1,262,111 ticks |

The original cast discards the volatile qualification of the GIC redistributor base. Clang reduced a 64-bit read of GICR_TYPER at offset 8 to a 32-bit load of its upper half at offset 12. The hardware GIC run then found CPU 0’s redistributor only. The one-line patch preserves the volatile qualification, producing `ldr x12, [x9, #0x8]` followed by a high-half comparison. Both SMP images booted with that change. The production-derived two- and four-core SDKs also passed the same test. The pinned release and the upstream master revision checked on November 18, 2026 both had the original cast. [seL4 16.0.0 source](https://github.com/seL4/seL4/blob/6e7c3b733d296cfd88d5fbf635c96e447a882374/src/arch/arm/machine/gic_v3.c#L257) · [Upstream master inspected](https://github.com/seL4/seL4/blob/08c7e5fddb3111ff377745c8eefb07feb5c4cfb7/src/arch/arm/machine/gic_v3.c#L258) · [Microkit 2.3.1](https://github.com/seL4/microkit/tree/ec86afdcd662b5976d11d4994acf1b11a2979882)

Local clean-build captures are [two cores](../../build/hvf-smp/2-hvc-fixed/hvf-on/result.json) and [four cores](../../build/hvf-smp/4-hvc-fixed/hvf-on/result.json). The production-derived controls are under `build/hvf-smp-lab/smp{2,4}/probe/{hvf-hvc,hvf-soft-gic}` and the corresponding corrected results under `build/hvf-smp-lab/mmio{2,4}/probe/hvf-hw-gic`.

## NexaWal on accelerated UTM

### Regular accelerated VM

Run the following from the repository root on the Apple Silicon Mac that holds the installation.

```sh
./dev utm-accelerated
```

The default rebuild composes a new Microkit loader with the release wallet, scan worker and DHCP/TCP service pinned by SHA-256 in [accelerated.lock.json](accelerated.lock.json). The regular profile uses four HVF vCPUs, the host CPU model, 2 GiB of RAM and the hardware GICv3. The wallet uses M1-tuned ownership scanning, 74-block requests to `rpc.nexawal.com` over HTTPS, 100 ms between healthy requests on that node, timed retry backoff and negotiated 2 KiB reads. It fetches and validates upcoming batches during scanning and commits blocks in height order.

The outputs are `build/utm-accelerated/loader.img` and `build/utm-accelerated/config.plist`. The plist targets the existing VM and preserves its disk identifier, MAC, and other installation settings. Rebuilding stages both files while UTM can keep running. For a later deployment, stop the VM, place the loader at `Data/loader.img` and the plist at `config.plist` in that same bundle, then reload its configuration in UTM. The existing data disk stays in place.

When prepared inputs predate the photo reader, the builder compiles the UI, input service, reader and decoder from the current checkout. It also rebuilds the vault with a separate owner-checked, read-only photo channel while copying and verifying the existing installation key and block configuration; it does not change the prepared vault or the VM disk. Use `./dev utm-accelerated --rebuild-ui` to force these source builds. They require the pinned Rust nightly, `llvm-objcopy`, and an available offline Cargo dependency cache. The UI keeps a 4 MiB in-memory cache of decoded thumbnails for the current run; it is cleared on chat deletion and does not persist after a restart. Photo decoding runs in a separate memory-bounded protection domain; the chat requests only visible previews. The current large-photo worker accepts JPEGs up to 3.5 million pixels, with each side at most 2,560 pixels.

When developing the wallet itself, run `./dev utm-accelerated --rebuild-wallet` to compile the coordinator and workers from the current checkout with the same profile. Use `--output DIR` for a separate comparison build. The default release build verifies and uses the exact measured binaries. The source build retains early batch requests, rate-limit backoff, and the release's first-connection and polling behavior. Experimental source builds can still pass their flags to `build_wallet_scan_workers.sh` explicitly.

The UTM status bar and lock screen display the Mac's local time. The guest clock remains UTC for network and wallet operations. The UI build embeds the Mac's timezone rules, including daylight-saving transitions, from `/etc/localtime`; if the Mac changes to a different timezone, rebuild the UI and restart the VM to pick it up. Pixel and iPhone need their own wall-clock and timezone sources.

#### Installation inputs

This Mac has a prepared, owner-private `.cache/utm-accelerated/` outside the build output. It contains the matching four-core SMC SDK, seL4 headers, configured service ELFs, the installation-specific vault ELF and a template of the VM hardware configuration. Keep this directory private. The build prefers the current bundle's settings and can use the template when the build tree has been cleaned. Wallet and chat data reside on the VM's own disk.

To prepare another copy of the same verified release, use Python 3.11 or later and the following input order. The final arguments are directories containing the compiled protection domains. The importer checks each ELF against the saved installation manifest and checks that manifest against the installed loader before publishing the inputs.

```sh
python3 experiments/hvf-smp/prepare_accelerated.py \
    "$regular_bundle" "$regular_manifest" "$four_core_smc_sdk" "$sel4_install" \
    "$vault_and_app_elf_dir" "$wallet_and_network_elf_dir" "$other_elf_dir"
```

### Low-level SMP builds

Build the SMC variant for the registered UTM profile. Use the same CPU count in UTM, the SDK, and the wallet system description. The wallet coordinator runs on CPU 0. Each secondary has an active, pinned protection domain, a private allocator, and one bounded mailbox shared only with the wallet. The worker receives a private view key and public spend key for an individual job. Only the wallet checks the chain anchor and updates the journal, outputs and scan height.

The following commands take an existing merged HTTPS-wallet system description as input. The benchmark used `build/hvf-real-apps/wallet-profile-utm/canonical-fine.system`.

```sh
cores=4
python experiments/hvf-smp/build.py "$cores" --conduit smc
platform="build/hvf-smp/$cores-smc-fixed"
wallet_build="build/utm-wallet-smp-$cores"
install="$platform/microkit/build/qemu_virt_aarch64_el1/smp-debug/sel4/install"
experiments/ghc-microkit/build_wallet_scan_workers.sh "$install" "$wallet_build" --generic
python experiments/ghc-microkit/enable_wallet_scan_workers.py \
    build/hvf-real-apps/wallet-profile-utm/canonical-fine.system \
    "$wallet_build/wallet.system" --cpus "$cores"
```

Compose the resulting system with this SDK's `microkit` tool, board `qemu_virt_aarch64_el1` and configuration `smp-debug`. Put the wallet build directory first in the existing image's ELF search path. The generic worker build omits the digest. Pass `--benchmark` in place of `--generic` to emit the per-core counts and ordered scan digest. Calling the wallet build script with just the two paths selects the regular M1 profile.

## Wallet results on the M1 Max

The full UTM guest fetched blocks 3,809,160 through 3,809,381 from `rpc.nexawal.com` over HTTPS. Each guest opened the same disposable wallet snapshot. Each timed interval started at the first binary range request and ended when the wallet committed block 3,809,381. All six comparisons transferred six accepted 37-block responses totaling 6,401,746 bytes. UTM used HVF, a hardware GICv3 and 2 GiB of guest memory. CPU counts ran in the order 1, 2, 4, 4, 2, 1.

| vCPUs | Two elapsed times (s) | Blocks/s at median elapsed time |
|---:|---:|---:|
| 1 | 3.063, 2.431 | 80.8 |
| 2 | 2.289, 2.660 | 89.7 |
| 4 | 1.702, 2.110 | 116.5 |

Each run produced the same SHA-256 transcript over committed heights, block hashes, and serialized owned outputs. Both four-core runs assigned 60 blocks to the coordinator and 54 to each secondary, with zero worker fallbacks. The separate funded, late-result and cancellation tests are in `rust/walletcore-core/tests/ownership`. Cold wallet-open to the first range request took 121–302 seconds across these trials, including DNS retry waits. The timings above cover the ensuing download, scan and commit. The first four-core trial opened its next five accepted requests 252–256 ms apart. Healthy requests currently have a 250 ms minimum interval.

[UTM trials and host load](../../build/hvf-real-apps/wallet-smp-utm/matched-runs.tsv) · [Wallet result verification](../../build/hvf-real-apps/wallet-smp-utm/runs/forward-4cpu/worker-correctness.json)

A separate four-core run lowered the healthy request interval to 50 ms for `rpc.nexawal.com` in the disposable image. It scanned the same 222 blocks in 2.091 s (106.1 blocks/s). Actual request gaps were 173–361 ms and the host was under heavier load. This trial did not establish an end-to-end benefit from shorter pacing. The worker implementation retains the original request pacing. [Timing and error counts](../../build/hvf-real-apps/wallet-smp-utm/runs/pace50-4cpu/pacing-comparison.json)

## Larger binary batches on four vCPUs

Two passes in opposite order scanned the same 222 blocks and 11,800 ordinary transactions on the same node. Timing ran from the first HTTPS range request through the ordered wallet commit. With the original 2 MiB body limit, the 75-block request exceeded the limit, and six 37-block replies completed the sample at 127.1 blocks/s. Three 74-block replies reached 183.4 blocks/s, two 111-block replies reached 161.8, and a request for 222 followed by a request for the remaining 47 reached 181.1. The node returned 175 blocks in the first reply to the 222-block request.

The median elapsed times for 74 and 222 differed by 15 ms, inside the roughly 25 ms serial polling interval. The first validated batch reached the scan workers after 0.197 s at 74 and 0.451 s at 222. All eight runs matched the reference digest, used all four scan lanes and subsequently verified the live tip. Each boot used a disposable wallet copy, the 32 MiB coordinator heap and the 250 ms healthy request interval. These experimental sizes apply only to the benchmark builds. [Full timings and verification](../../build/hvf-real-apps/wallet-batch-utm/results.md) · [Decoded memory measurements](../../build/hvf-real-apps/wallet-batch-utm/memory/REPORT.md)

## Profiling the 74-block UTM build

Pass `--utm-74` to `build_wallet_scan_workers.sh` for 74-block requests, a 4 MiB HTTPS body limit, an 8 MiB retained decode limit and negotiated receive capacity. Build the UTM `qemu_virt_aarch64` network fragment with `NEXAWAL_UTM_WIDE_RX=1` for the paired DHCP service. The current service advertises 2 KiB. The wallet also accepts the earlier 1 KiB service and falls back to 128 bytes on a service without wide reads. The profile below used the 1 KiB service, a 32 MiB wallet heap and four vCPUs.

For an ordered transcript, add `--benchmark`. For the guest phase timers, also add `--profile`. The `TRANSFER` marker measures each request through the final body byte. Prefetch lets this interval overlap scanning. At each batch boundary `PROFILE` reports active HTTPS polling, decoding and checkpoints, and `PHASE` separates scan-pool work from ledger commits and time between wallet callbacks. The fixed-prefix benchmark spans the first request through the 222nd ordered commit. Cold wallet startup and final verification of the live tip have separate timers.

The original 74-block build spent 400 ms of a 1,174 ms profile in the coordinator's scan pool, 128 ms waiting on its next worker, 347 ms polling HTTPS/TCP, 115 ms waiting on the socket and 69 ms decoding. The ledger and checkpoints took 37 ms together. The same 222 blocks contained 11,800 ordinary transactions and 29,567 outputs with view tags. Checking tags before decompressing destination keys and reusing the compressed shared secret per transaction public key reduced local and worker ownership CPU by 36.6% in three native replays of each build. The four-core guest's active scan-pool interval fell to 249 ms in the scanner-only profile. [Guest phase totals](../../build/hvf-real-apps/wallet-74-profile-utm/profile-results.md) · [Ownership measurements and funded receipt checks](../../build/hvf-real-apps/wallet-74-profile-utm/native/README.md)

At the default 250 ms request spacing, the original release took 1.141 and 1.073 s, the scanner change alone took 1.190 and 0.976 s, and the scanner with wider reads took 0.896 and 1.320 s. These correspond to 200.6, 205.0 and 200.3 blocks/s at median elapsed time. The 1.320 s run overlapped a host compile. Summed QEMU CPU per matched scan averaged 1.73 CPU-seconds originally and 1.22 with both changes. [Per-run wall time, CPU time and host load](../../build/hvf-real-apps/wallet-74-profile-utm/timings.tsv)

A fresh release built via `--utm-74 --benchmark` took 1.138 s (195.1 blocks/s) for the same prefix. It then verified 409 blocks at the live tip in 2.184 s from its first range request. All 12 runs reproduced the same ordered 222-block transcript with four active scan lanes and zero worker fallbacks. [Integrated release verification](../../build/hvf-real-apps/wallet-74-profile-utm/runs/canonical-release/batch-timing.json)

Two additional runs used the same disposable build with a 50 ms healthy request interval on this node. They took 0.919 and 1.092 s (220.8 blocks/s at median elapsed time). Actual gaps between the first three request starts were 224/260 ms and 164/369 ms. Each following request started in the same serial poll as acceptance of the preceding batch. After the third batch was accepted, its ordered scan took another 187 and 234 ms. These trials keep the ordinary and rate-limit retry policy intact. [Diagnostic profile and pacing trials](../../build/hvf-real-apps/wallet-74-profile-utm/profile-results.md)

## Validating the next range during the current scan

The coordinator dispatches scan workers before polling HTTPS. As authenticated bytes arrive for the next request, it can decode a complete block and check its transaction hashes, height and parent link. It keeps those decoded blocks alongside the bounded response buffer. At the end of the response it checks the complete envelope, cached object boundaries and output indices before releasing the batch for scanning. The wallet commits results in height order. Cancellation discards speculative work.

The benchmark's `VALIDATION` event records `early_blocks` for block checks that completed while the previous batch still had uncommitted work. Its `running_overlap` count includes only checks that observed a secondary scan worker running at entry or exit. A positive `scan_remaining` at final acceptance means the entire next batch passed validation before the previous batch finished committing.

In the two pipeline trials, all four within-prefix successor requests downloaded and checked individual blocks during the preceding scan. Across those boundaries the guest checked 79 blocks early in the first trial and 58 in the second. Of those, 43 and 26 checks respectively observed a running secondary. Three of the four complete responses passed final validation before the preceding batch committed. The other finished 46 ms later. [Guest events by boundary](../../build/hvf-real-apps/wallet-74-overlap-utm/range-boundaries.tsv)

Four-vCPU UTM fetched the same 222 blocks from `rpc.nexawal.com` in each trial. The pipeline took 1.500 and 1.043 s (148.0 and 212.9 blocks/s), or 174.6 blocks/s across both timed intervals. The original build took 0.963 and 4.476 s (230.6 and 49.6 blocks/s). The control runs spanned the fastest and slowest results while host load varied sharply, so the trials cannot establish a repeatable overall speedup. All four runs reproduced the known ordered transcript with zero worker fallbacks and went on to verify the live tip. Cold startup is excluded. [Benchmark report](../../build/hvf-real-apps/wallet-74-overlap-utm/overlap-results.md)

The 250 ms minimum between healthy requests with batches of 74 permits at most 296 blocks/s over a sustained scan. At 500 and 800 blocks/s, each new 74-block batch would need to start at most 148 ms and 92.5 ms apart respectively. Actual request gaps inside the pipeline's fixed windows were 254/413 ms and 259/291 ms. [Per-pass timings](../../build/hvf-real-apps/wallet-74-overlap-utm/release-timings.tsv)

## Healthy request pacing on the wallet's own node

Build with `--utm-74 --benchmark --healthy-ms=100` or `--healthy-ms=50` to change the minimum time between successful binary range requests to `rpc.nexawal.com:443`. The default is 250 ms. HTTP 429 still arms the existing 60, 120 and then 240 second retry deadlines. Other nodes and wallet operations use their ordinary timing. The same build script with `--healthy-ms=250` selects the control setting.

The 100 ms and 50 ms wallet images differed by one ARM64 instruction in their loadable sections. Both used the same kernel and the same 34 other protection-domain images, including the scan workers and network service. Host tests injected three consecutive rate-limited responses, delivered a notification before each retry deadline, then allowed the retries to run and verified the complete ordered height sequence. [Binary comparison](../../build/hvf-real-apps/wallet-74-pacing-utm/loaded-elf-comparison.json) · [Retry tests](../../build/hvf-real-apps/wallet-74-pacing-utm/wallet-tests.log)

Measured in the order 250, 100, 50, 50, 100, 250 ms, each fresh guest scanned the same 222 blocks from the live node and verified its tip. The combined rates at 250, 100 and 50 ms were 240.5, 288.2 and 212.0 blocks/s respectively. At 100 ms the actual request starts were 135–252 ms apart. At 50 ms they were 140–461 ms apart. Once the preceding reply was accepted and the lookahead slot was available, the median time until the next request was 3.8 ms and 2.8 ms respectively. [Live results and per-run timings](../../build/hvf-real-apps/wallet-74-pacing-utm/pacing-results.md)

On the same M1 Max, the released native NexaWal core (0.1.20) replayed the exact three 74-block responses over loopback HTTP. Holding range decoding serial, five interleaved trials per configuration gave 198.1 blocks/s with one ownership worker and 612.0 with four. With desktop parallel decoding and eight ownership workers it reached 758.7 blocks/s. These timings include decoding, scanning and in-memory wallet commits. The mobile-style configuration ran on the Mac CPU. [Native replay measurements and release provenance](../../build/hvf-real-apps/wallet-74-pacing-utm/native-replay/README.md)

Two subsequent direct native HTTPS trials against the same node used the same start height and 74-block requests. From the first binary request to observation of the 222nd ordered commit, the native core achieved 336.3 blocks/s with four ownership workers and 331.3 with eight. Both reproduced the replay's ordered block IDs. The native prefetcher also received a fourth response during these trials. [Native HTTPS timings and measurement boundaries](../../build/hvf-real-apps/wallet-74-pacing-utm/native-replay/live.tsv)

Five subsequent live runs of the released core on a physical iPhone 12 mini committed the same 222 blocks at 321.7 blocks/s median (191.3–355.6). Six fresh native M1 Max runs gave 303.7 blocks/s with four ownership workers and 433.6 with eight, across three runs per configuration. The eight-worker trials followed the interleaved four-worker Mac and phone trials. The earlier four-vCPU, 100 ms UTM median was 288.2 blocks/s. Reaching 350 blocks/s over this range would require reducing the guest median from 770 ms to 634 ms. [Physical iPhone comparison and raw measurements](../../build/hvf-real-apps/wallet-74-pacing-utm/iphone-mini/results.md)

## When the next batch is ready to scan

The wallet can reuse an authenticated HTTP/1.1 connection for consecutive `/get_blocks.bin` requests to the same node. It checks up to three complete blocks per step as the response arrives. The scanner receives the batch after the complete response, transaction hashes, chain links and output indices pass validation. A closed connection can be reopened, and rate-limited requests follow the existing backoff.

Three fresh four-vCPU control runs on `rpc.nexawal.com` took a median of 233 ms from a successor request to a validated batch. With connection reuse and earlier checks this fell to 146 ms. Both successor batches were ready before the preceding batch committed in each of the three revised runs. The median for all 222 ordered commits was 254.8 blocks/s, up from 203.5 in these controls. [Matched timings and readiness at each handoff](../../build/hvf-real-apps/wallet-74-ready-utm/ready-results.md)

The subsequent build also refills free scan workers within an eight-block window. Commits stay in height order. Its three runs reached 363.4, 238.1 and 265.0 blocks/s. The median run took 838 ms, including 158 ms to fetch and validate the first batch. Both successors were ready in time on that run. A median of 350 blocks/s would require 634 ms for the same work. Each of the nine runs reproduced the expected ordered result and verified the live tip. The rolling-dispatch runs came last, and live timings varied across boots. [All nine runs, phase timings and correctness checks](../../build/hvf-real-apps/wallet-74-ready-utm/rolling-results.md)

## Ownership scanning and ordered commits

In a fresh profile, the four-vCPU wallet spent 621 ms from its first validated batch to its 222nd ordered commit. Ledger updates and checkpoints accounted for 44 ms. Checking output ownership accounted for 96% of elapsed work inside the three remote workers. The wallet now checks compressed transaction keys for canonical encoding directly and avoids a redundant field inversion. The `--utm-m1-tune` benchmark option additionally selects M1 instruction scheduling for `curve25519-dalek` with the same guest instruction set.

Three fresh boots per build against `rpc.nexawal.com` yielded medians of 335.4 blocks/sec for the control, 332.5 for the direct check and 341.9 with the M1 option. The middle-ranked tuned run took 649.4 ms, 15.1 ms over the 350 blocks/sec budget. Its first batch took 145.0 ms, waits for later batches took 41.4 ms and processing ready batches took 463.0 ms. All runs matched the ordered result. The M1 trials followed the alternating control and direct-check runs. [Profile, all timing results and correctness checks](../../build/hvf-real-apps/wallet-74-commit-utm/commit-results.md)

## Starting the next wallet request earlier

In that 41.4 ms run, 452,156 bytes of the second response were still outstanding when the first 74 blocks committed. The revised UTM receive service uses a circular queue and transfers up to 2 KiB per call. After a batch passes validation, the wallet can send the request for its known successor on the same authenticated connection while the current scan finishes. It reads the successor's body after promoting the validated batch and preserves the existing pacing and rate-limit backoff.

The revised four-vCPU M1 build scanned the same 222 blocks at **399.8 blocks/sec median** in three fresh boots. Both successors were ready before their handoffs in each boot. Fresh controls reached 355.7 blocks/sec and the receive-only change reached 333.7. The third request finished sending 45–52 ms before the previous scheduling point. All nine boots matched the ordered digest. The early-send trials followed the six alternating control and receive-only trials. [Run timings, methodology and build evidence](../../build/hvf-real-apps/wallet-74-handoff-utm/results.md)

## First range acquisition on the M1 UTM guest

The current recommended benchmark configuration on the M1 Max is the four-vCPU HVF write-ahead build, `handoff-prime-m1-100`, with 74-block requests, a 100 ms healthy interval and 2 KiB reads. It overlaps validation and ownership scans, commits in height order and retains rate-limit backoff. Its earlier three-boot median was 399.8 blocks/sec. Three fresh boots on the same 222 blocks against `rpc.nexawal.com` gave 320.8 blocks/sec.

Two diagnostic boots spent 15–17 ms opening TCP and flushing the first TLS request. Streaming the 2,094,692-byte response took 101–135 ms. The experimental wallet reuses its authenticated metadata connection and can request a notification for an empty range read. The paired UTM network fragment supports this with `NEXAWAL_UTM_WIDE_RX=1 NEXAWAL_UTM_RX_NOTIFY=1`. This reduced median time to the first validated range from 129.0 to 124.0 ms, with a full-scan median of 317.5 blocks/sec. An optional one-window receive-ahead experiment (`NEXAWAL_UTM_RX_AHEAD=1`) reached 376.4 blocks/sec in its fastest trial and 175.2 at median during a busier period on the host. All 13 boots verified the ordered result and continued to the live tip. [Release trials, packet timings and build evidence](../../build/hvf-real-apps/wallet-74-first-utm/results.md)
