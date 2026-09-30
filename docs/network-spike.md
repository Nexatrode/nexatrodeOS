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
- Client `SEND_UI_SEALED` / live compose: same UDP path, plus
  `reserved |= TX_FLAG_TLS` so dhcp_client also queues the sealed datagram on the
  open TLS session (`MSG TLS TX` after echo).

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
`TLS SEAL OK`. The session stays open so Messages UI sealed TX can reuse it
(`MSG TLS TX`). Fragment-only guest B skips TLS (no `messages_net_tx`) so the
host stub is free for the merged guest. Host helper:
[`tls_stub.py`](../tests/net/tls_stub.py) loads the demo cert/key and echoes
multiple sealed datagrams on one connection. Entropy for the smoke is a
deterministic hardware-poll stub; validity-window checks are cleared in the
verify callback (no guest wall clock) — not a production RNG/PKI story.

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

1. Run `./dev utm-net` to build the merged net image and fragment guest B, then
   export `dist/Personal OS Net A.utm` and `dist/Personal OS Net B.utm` with
   VirtIO net on mmio `bus.0` and UTM **Shared** (`vmnet-shared`) networking.
   (Or run `./dev test-net` first if you only need the loaders under
   `build/sddf-net-spike/`.)
2. Open both bundles in UTM (aarch64 virt). Keep **serial terminals** visible.
   Do not use the graphics UTM profile for this experiment.
3. Start **Net B** first, then **Net A**. Confirm both finish DHCP on the shared
   subnet (often `192.168.64.0/24` with the host at `.1`) and that guest B still
   binds UDP **1236**.
4. On guest A’s console, lock the peer to guest B and send the fixed demo:
   `netpeer <guest-B-ip>:1236` then `netseal`. Guest B should print `SEAL OK`
   with **no** `toy_router`.
5. Optional interim: host relay with [`toy_router.py`](../tests/net/toy_router.py)
   if Shared Network DHCP or VirtIO IRQ layout fails — fall back to QEMU
   `./dev test-net`.
6. Optional: TCP echo to guest A on shared-LAN IP `:1238` from the host once
   reachable.

### Success / stop conditions

- **Pass:** guest B prints `SEAL OK` for the fixed PSL1 demo after guest A’s
  `netseal`, without relying on QEMU hostfwd alone.
- **Stop:** Shared Network DHCP missing, VirtIO net IRQ/mmio mismatch vs the
  QEMU layout, or graphics+net attempted in one VM (`timer_driver` name clash).
  Fall back to QEMU + `toy_router`; do not block U5 apps on this experiment.

Console helpers on the merged net image: `netpeer A.B.C.D:PORT` locks the
shared-page peer (survives later demo defaults); `netseal` re-sends the fixed
PSL1 demo. Boot-time `SEND_SEAL_DEMO` still targets the SLIRP demo peer unless
a peer was locked earlier.

The sealed TX peer IP/port is already on the shared `messages_net_tx` page
(demo default `10.0.2.2:1237`). Guest TLS carries the fixed PSL1 demo
(`TLS SEAL OK`) and Messages UI sealed packets (`MSG TLS TX`) on the same
pinned-cert session. Messages UI compose also still hands sealed packets to
`dhcp_client` over UDP when the net page is ready (`SEND_UI_SEALED` / live
compose).

## What passed

- Standalone: both echo clients DHCP + UDP echo through SLIRP.
- Integrated: isolation, DHCP, `DNS OK`, `TLS OK`, `TLS SEAL OK` (TOFU-pinned
  ECDHE-ECDSA), Messages
  `MSG NET TX` (demo + UI compose seal) + `MSG TLS TX` (UI over TLS), toy router → guest B `SEAL OK`, host UDP echo on 1235, host TCP
  echo on 1238, guest A `SEAL OK` on 1236.

## Limits

- Emulated Ethernet / NAT is not Mac Wi‑Fi and not a Pixel radio.
- Graphics+input+net still collide on the shared `timer_driver` name.
- Dual-UTM Shared Network: `./dev utm-net` exports Net A/B bundles; console
  `netpeer` / `netseal` enable direct peer targeting. **2026-09-29 attempt:**
  bundles register and start under UTM (`utmctl`); **automated** `SEAL OK` not
  completed — `utmctl attach` is unimplemented (serial is SPICE-only) and host
  `qemu -netdev vmnet-shared` needs elevated privileges. Manual GUI procedure
  remains: boot B then A, note B’s DHCP IP, on A run `netpeer <B-ip>:1236` then
  `netseal`, expect `SEAL OK` on B. Notes:
  `build/utm-net-experiment/README.txt`.
- TLS smoke uses a demo self-signed pin + deterministic entropy — not production PKI.
- Fake modem / SMS / SimpleX are out of scope.

## Next

1. Finish the dual-UTM Shared Network **manual** check (GUI serial) and record
   PASS (`SEAL OK`) or STOP here; automation blocked on UTM serial attach /
   vmnet privileges (2026-09-29).
2. Fake modem / audio / camera backends as needed.
3. Production TLS/PKI (real entropy, clock, pin UX) when leaving smoke territory.
