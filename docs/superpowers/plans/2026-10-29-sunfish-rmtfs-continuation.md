# Sunfish RMTFS continuation implementation plan

> **For agentic workers:** Use the existing isolated worktree and execute with test-driven development.

**Goal:** A long RMTFS request must continue after a bounded service slice without needing an unrelated hardware IRQ.

**Architecture:** Add one explicitly audited Microkit self-channel for software continuation, with separate send and receive IDs. Coalesce pending wakes, retain the flag across bounded batches, and never acknowledge a software notification as a hardware interrupt. Preserve all real IRQ routing and storage backpressure. The single-threaded startup protection domain and existing Q6/GLINK/RMTFS ownership remain unchanged.

**Files:** `pixel_mpss_runtime_irq.rs` handles badge/schedule; `pixel_mpss_runtime.rs` binds the notification to Microkit; `native-startup-mpss-runtime.system` adds one self-channel; runtime source and CapDL audits verify exact authority; `pixel_mpss_runtime_irq_test.rs` covers continuation and simultaneous IRQs.

- [ ] Add failing host tests for coalesced software scheduling, continuation delivery without an IRQ ACK, and simultaneous IRQ plus continuation.
- [ ] Implement the router and native channel mapping, preserving failure and notification ordering.
- [ ] Extend source/system and exact CapDL audits to accept only the new narrowly scoped self-notification capability; add mutation cases.
- [ ] Run IRQ, storage, source, target and full image audits. Freeze a one-shot image only after all pass; verify device identity and cold state before the authorized temporary boot.
