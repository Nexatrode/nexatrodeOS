# App architecture: Microkit phone, future desktop

Status: design direction, not an implemented installer or application ABI.

Keep the current phone and UTM system on seL4 and Microkit. The applications,
drivers, permissions, and channels in the current system image remain defined
at build time. A future desktop may need to install, start, stop, update, and
remove independent apps after the system boots. Dedicated root-task and
Microkit-hosted prototypes now exercise creation and destruction of native
processes at runtime. They load embedded test executables. A combined image
also has two predeclared Slint domains and isolated input focus.
Disk-based installation and integration
with the real app services remain work in the
[shared phone and desktop plan](phone-desktop-plan.md).

## What we can preserve now

Keep application logic separate from the details of how a process is started,
which channel number it uses, where a transfer page is mapped, and which
physical device provides a service. Apps should request narrowly scoped services
through adapters rather than reach into another component's internal state.
This is a direction for new work and incremental refactoring, not a claim that
every current app already has this separation.

For example, Notes can use a bounded storage interface while a phone adapter
continues to use its existing Microkit endpoint and transfer page. A future
desktop adapter could reach an equivalent service through a separately designed
app runtime. SimpleX and NexaWal also have network and background-work needs;
their existing integrations remain in place until a replacement is tested.

The accelerated UTM system currently assigns specific endpoints and shared
pages to apps in `experiments/hvf-smp/accelerated.system`. The vault associates
its fixed channels with fixed namespaces; app-supplied data cannot select a
different namespace. A dynamic desktop cannot simply reuse those numeric channel
identities as arbitrary app identities. It will need a trusted way to bind each
running app's authority to its own storage, network access, and other services.

## Design rules for shared interfaces

- Keep protocol messages bounded, validate inputs and lengths on the receiving
  side, and define errors and version compatibility explicitly. Avoid exposing
  fixed Microkit addresses and channel numbers as the application contract.
- Have platform adapters translate the same app-level operations to each
  environment. Keep the current working phone transport until its replacement
  has equivalent tests; an abstract interface alone provides no isolation.
- Treat storage and networking as requests to services, not unrestricted
  access to another app's data or devices. A desktop runtime must derive caller
  identity and grants from trusted authority, not a name or ID in a request.
- Account for asynchronous completion, cancellation, failure, and restart; a
  slow storage or network operation should not block the interface indefinitely.
- Keep presentation separate from app data and business logic where practical.
  App-facing UI contracts, a trusted shell, and window isolation remain design
  work; moving a screen into the same GUI process does not isolate it.
- Design lifecycle events for launch, foreground/background, lock, resume,
  termination, and recovery without assuming that today's phone implements
  all of them or that a background app has been securely wiped on lock.

## Desktop decisions still needed

The desktop lifecycle prototype has dedicated root-task and Microkit-hosted
modes. Both use bounded resource pools and create independent processes through
seL4 kernel interfaces. The hosted mode uses a private experimental composer
extension to grant resources to the trusted manager. The curated phone keeps
its existing Microkit system description. A shared app-service interface and
integration with the real drivers and services remain work.
A guest OS can run apps in its own environment, but does not turn them into
native NexatrodeOS apps or supply seamless graphics and storage integration.

Whichever direction we choose needs an app loader and package format; app
identity and permission policy; isolated address spaces and resource limits;
process and capability lifecycle, including revocation on exit; storage
namespaces; network and display/input mediation; fault handling; updates and
recovery. Authenticity and integrity of packages, trusted UI prompts, and
appropriate enforcement must be designed and tested. We should not claim
per-app isolation for multiple apps sharing a process or assume that installing
a package makes it safe. Existing Android, iOS, Linux, and macOS apps also need
compatibility work or a suitable guest environment; a desktop runtime alone
does not run them.

## Calculator experiment

The Slint suite's visible Calculator owns its state in the UI process and does
not currently call the separate `rust_calculator_app` Microkit service. The
latter has its own integer model and receives routed input via a static channel.
Do not assume the two implementations have identical input limits or behavior.

The Slint implementation now has a separate `calculator` module with typed
commands and bounded numeric input; the Slint callback translates key labels
to those commands. This is an internal Rust interface, not a stable ABI or a
new security boundary. Its tests cover arithmetic, limits, and error recovery.
An accelerated UTM run using a disposable disk also tapped `7 + 5 =` and
observed `12` on screen; it used the experimental wrapped-vault test image and
did not touch the regular VM or its data.

`python3 tests/desktop_calculator_host.py` builds a disposable host fixture
using that model, starts two separate macOS processes, verifies independent
calculator state, and stops both. It checks that a targeted `sandbox-exec`
rule denies each process access to the other fixture's file, with an
unrestricted read as a negative control. This policy otherwise permits host
access; it is only a narrow mechanism experiment, not a general sandbox, app
installer, or security guarantee. It does not establish equivalent behavior on
seL4 or physical hardware. No wallet, chats, or Pixel Wi-Fi code is involved.

Next, evaluate how a real desktop runtime would manage app identity, memory,
capabilities, and lifecycle, and demonstrate equivalent restrictions on seL4
before treating dynamic apps as supported.

## seL4 predeclared-slot feasibility probe

`experiments/desktop-slot` and `tests/desktop_slot_qemu.py` provide a separate
single-core QEMU/Microkit 2.3.1 experiment with no disk or network. A parent
starts with a child protection domain that was already created in the system
image. The test suspends the child, queues a notification, verifies the
higher-priority child does not process it while suspended, resumes it, and
observes the queued work. The child then attempts to read an address mapped
only to the parent; the parent checks the expected seL4 memory fault.

The test also builds two negative controls: omitting the suspend makes the
stopped-work check fail, and mapping the parent's page into the child makes the
forbidden read succeed. The pinned SDK accepts an extra TCB capability in its
system description but rejects a `cap_untyped` declaration. These checks were
run against the prepared Microkit 2.3.1 debug SDK and QEMU ARM `virt`:

```sh
python3 tests/desktop_slot_qemu.py --sdk /path/to/microkit-sdk-2.3.1 \
  --output build/desktop-slot
```

The baseline and both negative controls also passed in a disposable,
accelerated four-vCPU UTM VM, with both test domains assigned to CPU 0. The
runner reads the regular VM's configuration as a template but replaces its
loader and drive with the probe and a blank disk, restricts networking, and
removes the temporary VM. To build the UTM loaders and run the baseline:

```sh
python3 tests/desktop_slot_qemu.py --sdk /path/to/utm-accelerated/sdk \
  --output build/desktop-slot-utm --utm-build-only
python3 tests/utm_vault_key_probe.py --template /path/to/regular-vm.utm/config.plist \
  --loader build/desktop-slot-utm/baseline/loader.img \
  --expect 'SLOT|PASS|stop-resume' --expect 'SLOT|PASS|private-memory-fault' \
  --reject 'SLOT|FAIL|' --reject 'MON|ERROR'
```

The `no-stop` and `shared-private` loaders are intentional failure cases: their
expected markers are `SLOT|FAIL|ran while stopped` and
`SLOT|FAIL|private read succeeded`, respectively. Those markers mean the
negative controls detected the deliberately broken condition.

This confirms a narrow primitive for a *fixed* slot, not dynamic installation,
creation of a new address space, a clean restart, app termination and resource
reclamation, or a production security design. In particular, a suspended or
faulted domain can retain state and authorities. The SDF rejection establishes
what the pinned stock tool accepts, not that extending or replacing it is
impossible. A dynamically loaded app would require a trusted component with
appropriate object-allocation and capability-management authority, a loader,
and a safe teardown strategy. seL4 documents untyped retyping as the mechanism
used for creating kernel objects; Microkit's supported extra capabilities are
listed separately in its manual. See the [seL4 FAQ](https://docs.sel4.systems/projects/sel4/frequently-asked-questions)
and [Microkit 2.3.1 manual](https://docs.sel4.systems/projects/microkit/manual/2.3.1/).

## Native app handoff and unlocked-wallet experiment

On November 23, 2026, the baseline and three deliberate failure cases ran in
disposable, accelerated four-vCPU UTM guests. Each guest used its own newly
created 16 MiB GPT disk. The runner read the prepared template plist, replaced
the loader and disk references, disabled clipboard and directory sharing, and
restricted networking. It stopped and unregistered each temporary VM afterward.

The experiment in `experiments/desktop-handoff` builds separate native chess
and successor payloads. A manager loads the chess payload after boot into a
4 KiB executable page, then stops that domain, replaces the executable page,
clears a reused 16 KiB scratch arena, and launches the successor in a second
predeclared domain with its own capability space and private memory. Both
payloads run on CPU 0. Their bytes are embedded in the manager at image build
time. This checks runtime loading and cross-domain handoff with a bounded
memory layout.

The wallet fixture uses the existing disk-backed vault built with
`utm-wrapped-vault-experiment`. It enrolls and recovers a wrapped key with
public fixture inputs on the blank disk, verifies that a read is denied while
locked, unlocks, then writes, syncs, and reads a dummy WalletState file. Its
final read after both apps run proves that the wallet remained usable.
The fixture uses a test client and dummy bytes in place of the full NexaWal UI
and wallet core. Whole-disk confidentiality is outside this experiment.

The baseline verified the following behavior.

- Chess saved and reopened `1. e4 e5 2. Nf3 Nc6` through its own vault blob
  namespace. It poisoned its transfer buffer before reading back the saved
  value.
- The vault rejected a WalletState file read arriving on the chess channel,
  including when the app placed the name NexaWal in its transfer buffer.
- A direct chess read of the wallet client's transfer address produced the
  expected seL4 data-memory fault.
- Chess filled every reused scratch byte with a marker and left a marker at
  the end of its executable page. The successor checked every scratch byte
  and every byte beyond its shorter replacement executable.
- The successor found the previous chess endpoint slot empty using the debug
  kernel's capability inspection. Its own namespace started empty, and it
  saved and reopened a separate value. Its wallet file request was also
  rejected.
- A direct successor read of the chess transfer address produced the expected
  seL4 data-memory fault.

The failure cases established that these checks detect specific broken
handoffs. Retaining scratch bytes produced `old scratch bytes`. Retaining
the executable-page tail produced `old executable bytes`. Running the
successor in chess's original domain, with cleared shared pages but the same
capability space, let it reopen the previous game and produced
`old chess capability reopened previous game`.

To reproduce with a prepared UTM SDK and the block-driver artifacts from
`tests/build_utm_vault_key_storage.py`, run the following command. Add
`--control keep-scratch`, `--control keep-code-tail`, or
`--control keep-identity` and choose a separate output directory for each
failure case. A control succeeds when the runner detects its expected failure.

```sh
python3 tests/desktop_handoff_utm.py \
  --inputs /path/to/utm-accelerated \
  --storage /path/to/disposable-storage-artifacts \
  --output build/desktop-handoff/baseline
```

Each run saves `serial.log`, the generated system description, and the
Microkit report in its output directory. The local baseline and controls are
under `build/desktop-handoff`.

The scope remains a handoff between preallocated domains. The old domain stays
stopped with its private stack, transfer page, and capabilities still allocated.
The successor has a different private address space and capability space,
and only the explicitly reused code and scratch pages are scrubbed here.
Complete destruction of a domain, capability revocation, object reclamation,
repeated slot reuse, register cleanup, and crash/interruption recovery remain
work for a dynamic process manager. That manager will also need to obtain and
enforce fresh app identity and permissions beyond these fixed vault channels.

## Dedicated desktop root-task lifecycle

The experiment in `experiments/desktop-runtime` uses the prepared four-core
kernel and loader with a dedicated initial root task. A private SDK overlay
replaces only the initial root ELF used to compose the disposable image. The
generated Microkit specification contains an unused placeholder required by
the image composer. That specification is never executed by this root task.

The root reserves distinct 1 MiB management and process untyped pools. It
creates a CSpace, VSpace, page tables, thread, scheduling context, endpoint,
IPC page, stack, scratch page and bounded executable mappings after boot.
The process gets a badged endpoint, with its app identity held by the manager.
Read-only executable mappings have no surviving writable alias in the app.
This loader accepts a bounded, sorted, page-aligned AArch64 ET_EXEC image
with a matching executable entry point. Dynamic linking and installation
from disk remain outside this experiment.

On November 23, 2026, `tests/desktop_runtime_utm.py` passed the following
checks in disposable accelerated four-vCPU UTM and headless four-vCPU HVF.

- Reject truncated and overflowing ELF headers, invalid segment lengths,
  out-of-file data, and writable executable segments.
- Reclaim objects after construction stops at three different stages.
- Reclaim a partially created process after an expected resource-pool
  allocation failure.
- Run 100 alternating app identities with private save/load, wallet-read
  denial, malformed-message rejection and fresh private BSS, stack and
  scratch memory. Storage here is a synthetic per-app word in manager memory.
- Destroy processes after normal exit and deliberate private-memory faults.
  Verify every process-object capability slot is empty after revocation.
- Copy a process endpoint into the root task's long-lived CSpace, then verify
  revocation also removes that escaped copy.
- Reuse the same physical scratch frame across complete launches. Check that
  every newly retyped app frame is zero before loading executable data.

The `skip-revoke` control produced `object capability survived`.
The `keep-escaped` control allocated the endpoint outside the process resource
tree and produced `escaped capability survived`. The runner treats those
specific failures as successful negative controls.

```sh
python3 tests/desktop_runtime_utm.py \
  --inputs /path/to/utm-accelerated \
  --output build/desktop-runtime/baseline
```

Add `--control skip-revoke` or `--control keep-escaped` for the failure cases.
Add `--build-only` to compose without booting. A prepared four-core HVC SDK can
be supplied with `--hvc-sdk /path/to/sdk` to run the same kernel and experiment
through Homebrew QEMU/HVF without UTM's application control interface. Each
completed run saves its serial log, source hashes, kernel hash, loader hash,
backend and expected outcome in its output directory.

This is a bounded lifecycle prototype with embedded fixture code, an in-memory
test broker and expected failure paths. Integration with the real vault, app
services, package installation, arbitrary resource allocation and a production
failure policy remains. Zero-on-reallocation checks establish clean memory
for the next app. Physical erasure immediately at termination and removal of
all hardware or host copies require separate work.

## Independent Slint app surfaces

`experiments/desktop-surfaces` and `tests/desktop_surfaces_utm.py` run two
Slint processes using the existing Notes UI, separate 8 MiB heaps and separate
480 by 800 RGB surfaces. The compositor maps those surfaces read-only and
alone writes the full 1080 by 2340 output supplied to the existing VirtIO
display client. Each app has a bounded scheduling budget and only its own
surface and heap mappings. Updates are driven by scripted notifications.

The compositor validates fixed dimensions and stride, snapshots a completed
revision into private memory, and retains each app's last good snapshot. A
half-written or malformed new submission cannot block the other app's redraw.
Output publication uses the existing frame contract. These private snapshots
and the full output copy add memory and copy costs that need measurement.

The final disposable four-vCPU UTM run verified both windows in the captured
framebuffer, independent counters and revisions, and a seL4 fault when the
first app attempted to read the peer surface. The compositor retained that
window's prior image and the second app redrew after the fault. Deliberately
mapping the peer surface into the first app made the negative control print
`peer surface readable`.

```sh
python3 tests/desktop_surfaces_utm.py \
  --inputs /path/to/utm-accelerated \
  --sddf /path/to/sddf-personal-os \
  --output build/desktop-surfaces/baseline
```

Use a new output directory for each run. Add `--control-share-peer` for the
failure case or `--build-only` to compose without booting. The normal runner
saves a PPM framebuffer and checks known background and window pixels after
the final guest marker. This is a final-state check with a presentation wait,
and provides no input-latency measurement.

The host regression test includes the actual compositor source and checks
independent redraw, stalled producers, malformed dimensions, retained output
revisions and recovery under AddressSanitizer and UndefinedBehaviorSanitizer.

```sh
clang -std=c11 -O2 -Wall -Wextra -Werror -fsanitize=address,undefined \
  -I tests/surface_stub tests/desktop_surface_test.c \
  -o build/desktop-surface-host-test
build/desktop-surface-host-test
```

This compositor system uses predeclared Microkit domains. The combined image
below adds a bounded dynamic manager and interactive focus. Resizing, trusted
lock-screen integration, surface release on termination, display pacing and
the real SimpleX and NexaWal UI adapters remain integration work.

## Combined runtime, graphics and input experiment

`experiments/desktop-integrated/build_composer.py` fetches the committed
Microkit source at `ec86afdcd662b5976d11d4994acf1b11a2979882` into a private
build directory. Its checked source transformations add a 16 MiB untyped pool,
a 4096-slot management CNode and a separate ASID pool for the trusted manager.
It fixes ASID-pool object references after the composer's object sorting. The
management CNode has a guarded addressing branch and a full-depth invocation
alias. No app receives these resource capabilities.

The manager reuses the lifecycle code through `desktop-runtime/platform.h`.
It reserves separate 1 MiB manager and process pools and reassigns one
predeclared scheduling context to successive dynamic test processes. Its
initial placeholder thread is suspended. The manager creates each process's
CNode, VSpace, page tables, TCB, endpoint and frames. It releases the CPU
context and revokes process objects on termination. This is bounded dynamic
process creation, with one dynamic test process at a time. The two graphical
apps remain separately predeclared Microkit domains with their own heaps and
surfaces. All these experimental domains use CPU 0 in a four-vCPU VM.

The scripted combined image passed in accelerated UTM on November 23, 2026.
The dynamic manager completed 100 alternating launches, interrupted creation,
allocation failure, wallet-memory denial, escaped-capability revocation and
zero-on-reallocation checks. The same image displayed both Slint windows,
denied one window's attempted peer-surface read and redrew the surviving
window. The dedicated root-task baseline and both deliberate revocation
failure cases were also rerun under headless HVF after the hosted refactor.

The interactive mode adds the source-built VirtIO input driver and trusted
focus routing in the compositor. Each app gets only its own input page.
Clicking selects a window, Tab changes focus, a pointer drag retains its
original owner, and keyboard release follows the original key press across a
focus change. A border outside either app's writable surface indicates focus.
The app translates the bounded events into Slint window events. This fixture
supports a limited Latin keyboard map; layout selection, IME and complete
modifier behavior remain work.

Input queues have 64 fixed-size events per app. The producer never overwrites
unread events or modifies the consumer's tail. If an app stalls, its input is
dropped and a reset is queued once space becomes available. Invalid consumer
indices cannot redirect copy bounds or block the other app. Driver overflow
resets held state. Host tests exercise these paths under AddressSanitizer and
UndefinedBehaviorSanitizer.

The interactive checks use QMP clicks and key events, disposable app
diagnostics, OCR and framebuffer pixels. They verify separate `alpha` and
`bravo` text, visible focus changes, a held key released to its original
window, and input denial after clicking outside both windows. Successful GPU
copy traces can interrupt character-by-character app diagnostics; the runner
removes only those complete known copy traces after checking failure markers.
These checks passed in both the UTM app and headless four-vCPU HVF on
November 23, 2026. Each run is recorded separately in its result artifact.
These are final-state checks with waits, not host display latency measurements.
Both backends also emitted an sDDF
zero-status GPU interrupt diagnostic while continuing to present correct
frames; it remains a driver/presentation follow-up.

```sh
python3 experiments/desktop-integrated/build_composer.py \
  --source /path/to/committed-microkit-source \
  --output build/desktop-integrated/composer
python3 tests/desktop_surfaces_utm.py \
  --inputs /path/to/utm-accelerated --sddf /path/to/sddf-personal-os \
  --output build/desktop-surfaces/interactive --interactive --build-only
python3 tests/desktop_integrated_utm.py \
  --inputs /path/to/utm-accelerated \
  --surfaces build/desktop-surfaces/interactive \
  --composer build/desktop-integrated/composer/target/release/microkit \
  --output build/desktop-integrated/interactive --interactive
```

Use fresh output directories. The runner records source, kernel, loader and
composer hashes, guest pass markers and input-test evidence. `--hvc-sdk` selects
the prepared HVC loader and a separate headless QEMU/HVF run with no disk or
network. The normal UTM runner creates a blank disk, restricts networking and
removes its temporary VM. Neither path opens the saved wallet/chat disk or
changes the Pixel Wi-Fi checkout.

This does not yet provide dynamic Slint loading, a disk package installer,
disk-backed storage for these dynamic payloads, lock coverage, a surface
swapchain or lifecycle-safe surface/input revocation. The manager's private
saves are synthetic in-memory words. The next integration must tie a created
graphical process, its input/surface authority and teardown to one instance
generation before promoting this to real apps.
