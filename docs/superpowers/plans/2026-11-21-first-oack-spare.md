# First OACK spare-buffer Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline. User has already opted out of more questions and requested autonomous progress through the next useful verified image.

**Goal:** Test fresh receive-buffer availability before the original OACK.

**Architecture:** Reuse the existing bounded priming allocator and ordinary checked publisher. Scope to the original exact port-0x1f server-check transaction, available Advertising/Free backing, and a single post-advertisement writer turn. Retain a copy of the exact offer in the existing read ledger.

**Tech Stack:** no_std Rust, existing seL4 Microkit target and native build/audit tools.

**Spec:** docs/superpowers/specs/2026-11-21-first-oack-spare-design.md

## Global constraints

RX_SLOTS remains 8. No DMA mappings, trust boundaries, volatile validation, source guards, credit ordering, request identities or receipt retirement change. No automatic device action.

## Review focus

- A positive pending grant must still get its own backing before ACK.
- Full occupancy must not steal a held or Control slot.
- A blocked advertisement must not falsely enable OACK or read evidence.
- Uncertain head effects must seal the owner without retry.
- The fresh offer must remain correlated after a slot is received or reused.

## Task 1 — scoped advertisement and follow-up

Modify rust/src/qcom_glink_qmi.rs and qcom_glink_fifo.rs, gated by a Cargo leaf. Add owner/FIFO cases for first advertisement before OACK, one-shot bounded follow-up, full pool, blocked publication, close and head uncertainty. Observe failure against the old policy, implement the guarded path, and run focused tests.

## Task 2 — exact offer observation and panel

Modify qcom_glink_grant_reads.rs and pixel_mpss_runtime.rs. Test frozen exact primed offer across release/reuse and tail advances before implementing observation. Retain the existing close, DATA and WLAN fields. Render and inspect all labels and final digits on both framebuffers.

## Task 3 — composed validation and image

Run full owner/link/source/source-guard/stack suites and conditional peer replays with independent wire and buffer assertions. Build AArch64, package a unique temporary image, complete the exact-runtime 280-mutation audit and verify embedded ELF, compressed/wrapped payload and audit hashes. Record notes and manifest, then supply the exact manual cold fastboot command. Leave existing dirty work and other worktrees intact.
