# VirtIO network feasibility spike

`./dev test-net` has two phases:

1. **Standalone** — builds the pinned sDDF `examples/echo_server` for
   `qemu_virt_aarch64` as a separate Microkit system. It boots under QEMU with
   VirtIO ethernet on `virtio-mmio-bus.0` and SLIRP user networking, waits for
   both echo clients to finish DHCP, then requires a host UDP echo through the
   published forwards. Serial: `build/sddf-net-spike/net-serial.log`.

2. **Integrated** — builds the serial-free fragment under [`tests/net/`](../tests/net/)
   and merges it into Personal OS. The merged guest proves isolation + DHCP +
   `DNS OK` (lwIP client → host [`dns_stub.py`](../tests/net/dns_stub.py) on UDP
   **53535**) + `TLS OK` (Mbed TLS 1.2 ECDHE-ECDSA + AES-128-GCM client with a
   pinned self-signed cert → host [`tls_stub.py`](../tests/net/tls_stub.py) on TCP
   **1239**) + UDP echo on
   **1235** + host→guest TCP echo on **1238** + host→guest PSL1 `SEAL OK` on
   **1236**, plus Messages→`dhcp_client` sealed TX to the host on **1237**. A
   second QEMU (fragment-only guest B) receives that datagram via
   [`toy_router.py`](../tests/net/toy_router.py) (`1237` → hostfwd `1246` →
   guest B `:1236`) and must also print `SEAL OK`. Artifacts:
   `build/sddf-net-spike/net-system.system`, `net-system-serial.log`,
   `guest-b-serial.log`.

## Messages → net bridge (merged image only)

Default / UTM images stay offline. The net merge injects:

- Shared MR `messages_net_tx` (also mapped in base [`system.system`](../system.system)
  for Messages)
- Channel `messages_app:3` (`pp`) → `dhcp_client:3` (dhcp priority raised to 101)
- Client post-isolation `SEND_SEAL_DEMO`: Messages seals the fixed PSL1 demo
  packet, sets the shared-page peer (default `10.0.2.2:1237`), `pp_call`s
  dhcp_client, which `udp_sendto`s that peer and prints `MSG NET TX`. Offline
  builds reply `REPLY_NO_NET` when magic `NET1` is absent. Dual-UTM can retarget
  the peer IP/port on the shared page without rebuilding dhcp_client.

## PSL1 sealed UDP smoke (non-production)

Wire format (binary only): `PSL1` magic (4) + 8-byte ASCII `key_id` + `pt_len`
u32 LE + 12-byte nonce + ciphertext + 16-byte AES-256-GCM-SIV tag. AAD is
`PSL1 || key_id`. Shared codec: [`rust/src/message_seal.rs`](../rust/src/message_seal.rs).

UDP demo constants (fixed, **not** for production):

- `key_id` = `DEADBEEF`
- AES key = `PERSONAL-OS-UDP-SEAL-DEMO-KEY!!!` (32 bytes)
- plaintext = `HELLO`

Host helper [`tests/net/seal_host`](../tests/net/seal_host) /
[`seal_packet.py`](../tests/net/seal_packet.py) emits the datagram;
[`psl1_demo_packet.h`](../tests/net/include/psl1_demo_packet.h) locks guest
`memcmp`. Port **1235** stays pure UDP echo; port **1238** is TCP echo.

## DNS smoke (non-production)

After DHCP, `dhcp_client` points lwIP at `10.0.2.2` and resolves
`personal-os.test` → `10.0.2.2`, printing `DNS OK`. The host stub listens on
UDP **53535** (`DNS_SERVER_PORT` in lwipopts) so CI does not need privileged
port 53 and does not collide with macOS mDNS on 5353. This is offline-friendly
and does not use the public Internet.

## TLS smoke (non-production)

Pinned Mbed TLS **3.6.2** (see `toolchain.lock.json`) builds into the net
fragment as a TLS 1.2 **ECDHE-ECDSA** client (`AES-128-GCM`) with a **TOFU pin**:
the guest embeds the demo leaf DER and its SHA-256
([`tls_tofu_pin.h`](../tests/net/include/tls_tofu_pin.h);
certs under [`tests/net/certs/`](../tests/net/certs/)). After DHCP the guest
connects to `10.0.2.2:1239`, verifies the pin (`TLS OK`), sends the fixed
PSL1 sealed demo datagram over that session, requires an echo, and prints
`TLS SEAL OK`. Host helper: [`tls_stub.py`](../tests/net/tls_stub.py) loads the
demo cert/key. Entropy for the smoke is a deterministic hardware-poll stub;
validity-window checks are cleared in the verify callback (no guest wall clock)
— not a production RNG/PKI story.

## Toy LAN router

SLIRP isolates guests, so peer delivery uses a **host process** as the central
router: listen on UDP 1237, forward opaque bytes to guest B’s hostfwd. This is
not a bridged UTM LAN and not graphics+net (still blocked on `timer_driver`).

## Dual-UTM bridged LAN (manual sketch)

`./dev test-net` stays on QEMU + SLIRP + `toy_router`. A true two-guest LAN is a
**manual** UTM experiment; it is not automated and does not change default /
graphics UTM images (those remain offline).

### Why this is separate from test-net

QEMU user networking gives each guest its own `10.0.2.0/24` with the host at
`10.0.2.2`. Guests never share L2, so peer UDP needs a host relay. UTM’s
**Shared Network** (vmnet) or **Bridged** mode can put two VMs on one subnet so
guest A can address guest B directly.

### Suggested experiment

1. Run `./dev test-net` once so artifacts exist under `build/sddf-net-spike/`
   (`net-system-loader.img`) and the fragment build (`loader.img` for guest B).
2. Create two UTM VMs (aarch64 virt, VirtIO net). Prefer **Shared Network** so
   both get addresses on the same private LAN without exposing the host Wi‑Fi.
   Bridged works if you want host-LAN IPs instead.
3. Attach guest A to the merged Personal OS net loader; guest B to the fragment
   loader (same CPU/memory/load address as QEMU). Keep serial consoles open.
4. Confirm both finish DHCP (or assign static IPs on the shared subnet) and that
   guest B still binds UDP **1236**.
5. Peer path options:
   - **Direct (goal):** set the shared-page peer to guest B’s shared-LAN IP
     `:1236` (Messages `set_peer` / page fields; demo defaults are
     `10.0.2.2:1237`), trigger `SEND_SEAL_DEMO`, and watch guest B print
     `SEAL OK` with **no** `toy_router`.
   - **Host relay (interim):** keep SLIRP/user net or hostfwd and run
     [`toy_router.py`](../tests/net/toy_router.py) on the Mac as in test-net.
6. Optional: TCP echo to guest A on shared-LAN IP `:1238` from the host or from
   guest B once a TCP client exists there.

### Success / stop conditions

- **Pass:** guest B prints `SEAL OK` for the fixed PSL1 demo after guest A’s
  sealed TX, without relying on QEMU hostfwd alone.
- **Stop:** Shared Network DHCP missing, VirtIO net IRQ/mmio mismatch vs the
  QEMU layout, or graphics+net attempted in one VM (`timer_driver` name clash).
  Fall back to QEMU + `toy_router`; do not block U5 apps on this experiment.

The sealed TX peer IP/port is already on the shared `messages_net_tx` page
(demo default `10.0.2.2:1237`). Guest TLS carries the PSL1 demo over a pinned-cert
session (`TLS SEAL OK`); Messages UI over TLS remains later. Messages UI compose
already hands sealed packets to `dhcp_client` over UDP when the net page is
ready (`SEND_UI_SEALED` / live compose).

## What passed

- Standalone: both echo clients DHCP + UDP echo through SLIRP.
- Integrated: isolation, DHCP, `DNS OK`, `TLS OK`, `TLS SEAL OK` (TOFU-pinned
  ECDHE-ECDSA), Messages
  `MSG NET TX` (demo + UI compose seal), toy router → guest B `SEAL OK`, host UDP echo on 1235, host TCP
  echo on 1238, guest A `SEAL OK` on 1236.

## Limits

- Emulated Ethernet / NAT is not Mac Wi‑Fi and not a Pixel radio.
- Graphics+input+net still collide on the shared `timer_driver` name.
- Dual-UTM shared LAN is documented as a manual sketch; not part of CI.
- TLS smoke uses a demo self-signed pin + deterministic entropy — not production PKI.
- Fake modem / SMS / SimpleX are out of scope.

## Next

1. Run the dual-UTM Shared Network experiment when convenient; record results here.
2. Messages UI sending sealed traffic over TLS (UDP UI TX + cert TOFU smoke are done).
3. Fake modem / audio / camera backends as needed.
4. Optional Messages UI / IPC to set peer IP without a rebuild (page fields already exist).
