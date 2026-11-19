# Accelerated UTM Real Apps Implementation Plan

**Goal:** Run the real NexaWal and SimpleX services under HVF and verify recovery of the user's saved data on isolated copies.

**Architecture:** Build a coherent EL1/GICv3 guest using one matching seL4 ABI. Replace the diagnostic timer with a bounded, verified timing service, preserve each installation's vault key and storage, and test networking and persistent state on disposable images before touching the running VM. Treat software-only shared memory separately from device DMA mappings.

**Tech stack:** seL4 Microkit, Rust, sDDF, SimpleX agent, UTM/QEMU HVF.

- [ ] Identify source images, vault-key provenance, and safe copies without changing running disks.
- [ ] Build EL1 binaries for all protection domains and reconstruct device configuration sections from the current working image.
- [ ] Implement and test an HVF-compatible timer, including overlapping requests, cancellation/replacement, idle behavior, and scheduler load.
- [ ] Boot an isolated complete guest; verify storage, network, UI input, and absence of protection-domain faults.
- [ ] Restore copies of the saved chats and wallet, verify persistence and operation without exposing recovery words or transmitting messages or transactions.
- [ ] Measure responsiveness and document remaining limits before making the accelerated image available for user testing.
