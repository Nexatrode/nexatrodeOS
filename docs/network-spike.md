# VirtIO network feasibility spike

`./dev test-net` has two phases:

1. **Standalone** — builds the pinned sDDF `examples/echo_server` for
   `qemu_virt_aarch64` as a separate Microkit system. It boots under QEMU with
   VirtIO ethernet on `virtio-mmio-bus.0` and SLIRP user networking, waits for
   both echo clients to finish DHCP, then requires a host UDP echo through the
   published forwards. Serial: `build/sddf-net-spike/net-serial.log`.

2. **Integrated** — builds the serial-free fragment under [`tests/net/`](../tests/net/)
   (one `dhcp_client` with lwIP UDP echo on port **1235** and a fixed PSL1 sealed
   datagram check on port **1236**, logging via `libsddf_util_debug` / Microkit
   debug puts — **not** the PL011 UART).
   [`tests/merge_network_system.py`](../tests/merge_network_system.py) merges that
   fragment into Personal OS: ethernet reuses the block driver's MMIO page at
   `0xa000000` (bus.0 and bus.1 share a 4 KiB window), and net DMA buffers are
   relocated below storage/RNG reservations. QEMU keeps blk (bus.1) + RNG
   (bus.8) and attaches net on bus.0 with
   `hostfwd=udp::1235-…:1235,hostfwd=udp::1236-…:1236`.
   Pass requires `ISOLATION COMPLETE: 27 checks passed`, DHCP for `dhcp_client`,
   host UDP echo on 1235, and serial `SEAL OK` after the host sends the fixed
   PSL1 demo datagram on 1236. Artifacts:
   `build/sddf-net-spike/net-system.system`, `net-system-serial.log`.

## PSL1 sealed UDP smoke (non-production)

Wire format (binary only): `PSL1` magic (4) + 8-byte ASCII `key_id` + `pt_len`
u32 LE + 12-byte nonce + ciphertext + 16-byte AES-256-GCM-SIV tag. AAD is
`PSL1 || key_id`. Shared codec: [`rust/src/message_seal.rs`](../rust/src/message_seal.rs).

UDP demo constants (fixed, **not** for production):

- `key_id` = `DEADBEEF`
- AES key = `PERSONAL-OS-UDP-SEAL-DEMO-KEY!!!` (32 bytes)
- plaintext = `HELLO`

Host helper [`tests/net/seal_host`](../tests/net/seal_host) (or
[`seal_packet.py`](../tests/net/seal_packet.py)) emits the datagram;
[`tests/net/include/psl1_demo_packet.h`](../tests/net/include/psl1_demo_packet.h)
locks the guest `memcmp` expectation. Port **1235** stays a pure echo.

Messages UI seals with the same PSL1 layout locally into vault PMS2 stubs; it
does **not** send over this UDP path yet (no Messages↔ethernet IPC; graphics+net
still blocked on `timer_driver`).

## What passed

- Standalone: both echo clients print
  `DHCP request finished, IP address for netif …` and UDP echo returns through
  SLIRP (`1235` → `.15:1235`, `1238` → `.16:1235`).
- Integrated: Personal OS isolation completes with
  `DHCP request finished, IP address for netif dhcp_client is: …`, host UDP
  echo of a fixed payload on port 1235, and `SEAL OK` on port 1236.

## Limits

- Emulated Ethernet / NAT is not Mac Wi‑Fi and not a Pixel radio.
- Graphics+input+net is still separate: merging net into the GPU image would
  collide on the shared `timer_driver` name.
- Messages seals locally (PMS2 / `SEALED N`); peer delivery over VirtIO and a
  toy LAN router remain later U6 batches.
- Fake modem / SMS / SimpleX are out of scope.

## Next

1. Optionally assert a host↔guest TCP echo in `./dev test-net`.
2. Bridge Messages PD to ethernet / toy router toward the
   explicit-contact north star
   ([explicit-contact-messaging.md](explicit-contact-messaging.md)).
3. DNS / TLS and fake modem / audio / camera backends as needed.
