# VirtIO network feasibility spike

`./dev test-net` has two phases:

1. **Standalone** — builds the pinned sDDF `examples/echo_server` for
   `qemu_virt_aarch64` as a separate Microkit system. It boots under QEMU with
   VirtIO ethernet on `virtio-mmio-bus.0` and SLIRP user networking, waits for
   both echo clients to finish DHCP, then requires a host UDP echo through the
   published forwards. Serial: `build/sddf-net-spike/net-serial.log`.

2. **Integrated** — builds the serial-free fragment under [`tests/net/`](../tests/net/)
   and merges it into Personal OS. The merged guest proves isolation + DHCP +
   UDP echo on **1235** + host→guest PSL1 `SEAL OK` on **1236**, plus
   Messages→`dhcp_client` sealed TX to the host on **1237**. A second QEMU
   (fragment-only guest B) receives that datagram via
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
  packet, `pp_call`s dhcp_client, which `udp_sendto`s `10.0.2.2:1237` and prints
  `MSG NET TX`. Offline builds reply `REPLY_NO_NET` when magic `NET1` is absent.

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
`memcmp`. Port **1235** stays pure echo.

## Toy LAN router

SLIRP isolates guests, so peer delivery uses a **host process** as the central
router: listen on UDP 1237, forward opaque bytes to guest B’s hostfwd. This is
not a bridged UTM LAN and not graphics+net (still blocked on `timer_driver`).

## What passed

- Standalone: both echo clients DHCP + UDP echo through SLIRP.
- Integrated: isolation, DHCP, Messages `MSG NET TX`, toy router → guest B
  `SEAL OK`, host UDP echo on 1235, guest A `SEAL OK` on 1236.

## Limits

- Emulated Ethernet / NAT is not Mac Wi‑Fi and not a Pixel radio.
- Graphics+input+net still collide on the shared `timer_driver` name.
- Dual-UTM shared LAN and real PGP/QR remain later.
- Fake modem / SMS / SimpleX are out of scope.

## Next

1. Optionally assert a host↔guest TCP echo in `./dev test-net`.
2. Dual-UTM bridged LAN experiment (manual) once the host router path is familiar.
3. DNS / TLS and fake modem / audio / camera backends as needed.
