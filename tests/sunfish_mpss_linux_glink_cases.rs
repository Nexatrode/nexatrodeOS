use crate::smp_link::{GlinkError as GE, GlinkPoll as GP, Lifecycle, Power as SP};
fn with_authenticated_link(f: impl FnOnce(&mut crate::Link<'_>, &mut crate::LinkPeer)) {
    with_boot(|b, p, _, _, _| {
        until_auth(b, p);
        b.step(p).unwrap();
        let receipt = b.take_auth_returned().unwrap();
        crate::with_link_memory(|c, peer| {
            crate::GLINK_LIFE.set((1, Lifecycle::AuthReturned, SP::Active));
            c.claim_glink(receipt).unwrap();
            f(c, peer);
        });
    });
}
#[test]
fn glink_memory_retains_original_owner_and_waits_for_dynamically_sized_rx() {
    for bytes in [4096, 16384, 32768] {
        with_authenticated_link(|c, p| {
            let prior = c.memory().parent().report();
            assert_eq!(c.glink_poll(), Ok(GP::Waiting));
            assert!(!c.report().ready_seen, "no global-ready prerequisite");
            p.allocate_rx(bytes, 0x80);
            assert_eq!(c.glink_poll(), Ok(GP::Available));
            assert_eq!(c.glink_report().unwrap().rx, p.rx);
            assert_eq!(c.memory().parent().report(), prior);
            assert_eq!(p.free_lock(), 0);
            assert!(c.glink_report().unwrap().identity != 0);
        });
    }
}
#[test]
fn glink_rejects_duplicate_rx_allocation_before_any_write() {
    with_authenticated_link(|c, p| {
        p.allocate_rx(4096, 0);
        p.add(480, 4096);
        let before = p.read(0x11d000, 0x7c000);
        assert!(c.glink_poll().is_err());
        assert!(c.glink_write_tx(0, b"12345678").is_err());
        assert_eq!(p.read(0x11d000, 0x7c000), before);
        assert_eq!(p.free_lock(), 0);
    });
}
#[test]
fn glink_memory_uses_only_ap_cursors_and_tx_span_preserving_other_items() {
    with_authenticated_link(|c, p| {
        use crate::sunfish_mpss_linux_smem::ITEMS;
        p.allocate_rx(0x4000, 0);
        c.glink_poll().unwrap();
        let before = p.read(0x11d000, 0x7c000);
        let tx = ITEMS[2].1;
        c.glink_write_tx(0, b"12345678").unwrap();
        c.glink_store_tx_head(8).unwrap();
        p.cursor(0, 8);
        p.rx_bytes(0, b"ABCDEFGH");
        p.cursor(3, 8);
        let mut rx = [0; 8];
        c.glink_read_rx(0, &mut rx).unwrap();
        assert_eq!(&rx, b"ABCDEFGH");
        c.glink_store_rx_tail(8).unwrap();
        let mut out = [0; 8];
        c.glink_read_tx(0, &mut out).unwrap();
        assert_eq!(&out, b"12345678");
        let after = p.read(0x11d000, 0x7c000);
        for n in 0..after.len() {
            let at = 0x11d000 + n;
            let changed = (tx.offset..tx.offset + 8).contains(&at)
                || (ITEMS[1].1.offset..ITEMS[1].1.offset + 16).contains(&at)
                || (p.rx.unwrap().offset..p.rx.unwrap().offset + 8).contains(&at);
            if !changed {
                assert_eq!(after[n], before[n], "offset {at:x}");
            }
        }
        assert_eq!(
            (
                c.glink_report().unwrap().stores_sent,
                c.glink_report().unwrap().stores_done
            ),
            (3, 3)
        );
    });
}
#[test]
fn glink_memory_rejects_changed_allocation_owned_cursor_or_reserved_word() {
    for mode in 0..4 {
        with_authenticated_link(|c, p| {
            p.allocate_rx(4096, 0);
            c.glink_poll().unwrap();
            if mode == 0 {
                p.word(p.rx.unwrap().offset - 12, 8192);
                p.word(0x11d00c, (p.free + 4096) as u32);
            } else {
                p.corrupt_owned_cursor([0, 1, 2, 4][mode], 8);
            }
            assert!(c.glink_poll().is_err());
            let before = p.read(0x11d000, 0x7c000);
            assert!(c.glink_write_tx(0, b"12345678").is_err());
            assert!(c.glink_report().unwrap().error.is_some());
            assert_eq!(p.read(0x11d000, 0x7c000), before);
            assert_eq!(p.free_lock(), 0);
        });
    }
}
#[test]
fn glink_memory_rejects_bounds_alignment_missing_rx_and_forbids_retry() {
    for mode in 0..5 {
        with_authenticated_link(|c, p| {
            if mode != 0 {
                p.allocate_rx(4096, 0);
                c.glink_poll().unwrap();
            }
            let before = p.read(0x11d000, 0x7c000);
            let r = match mode {
                0 => c.glink_store_rx_tail(8),
                1 => c.glink_write_tx(16384, b"12345678"),
                2 => c.glink_store_tx_head(1),
                3 => c.glink_store_rx_tail(4096),
                _ => c.glink_read_rx(4096, &mut [0; 8]),
            };
            assert!(r.is_err());
            assert!(c.glink_write_tx(0, b"12345678").is_err());
            assert_eq!(p.read(0x11d000, 0x7c000), before);
        });
    }
}
#[test]
fn glink_memory_stops_on_peer_fatal_generation_power_or_lifecycle_loss() {
    for mode in 0..4 {
        with_authenticated_link(|c, p| {
            p.allocate_rx(4096, 0);
            c.glink_poll().unwrap();
            match mode {
                0 => p.peer_smp(true),
                1 => crate::GLINK_LIFE.set((2, Lifecycle::AuthReturned, SP::Active)),
                2 => crate::GLINK_LIFE.set((1, Lifecycle::AuthReturned, SP::Inactive)),
                _ => crate::GLINK_LIFE.set((1, Lifecycle::PreAuth, SP::Active)),
            };
            let before = p.read(0x11d000, 0x7c000);
            assert!(c.glink_write_tx(0, b"12345678").is_err());
            assert!(c.glink_report().unwrap().error.is_some());
            assert_eq!(p.read(0x11d000, 0x7c000), before);
            assert_eq!(p.free_lock(), 0);
        });
    }
}
#[test]
fn glink_memory_requires_auth_returned_lifecycle_even_with_real_receipt() {
    for phase in [Lifecycle::PreAuth, Lifecycle::AuthSubmitted] {
        with_boot(|b, p, _, _, _| {
            until_auth(b, p);
            b.step(p).unwrap();
            let receipt = b.take_auth_returned().unwrap();
            crate::with_link_memory(|c, peer| {
                crate::GLINK_LIFE.set((1, phase, SP::Active));
                let before = peer.read(0x11d000, 0x7c000);
                assert_eq!(c.claim_glink(receipt), Err(GE::Guard));
                assert_eq!(peer.read(0x11d000, 0x7c000), before);
                assert_eq!(peer.free_lock(), 0);
            });
        });
    }
}

#[test]
fn glink_memory_quarantines_uncertain_or_dropped_native_stores() {
    for mode in 1..=4 {
        with_authenticated_link(|c, p| {
            p.allocate_rx(4096, 0);
            c.glink_poll().unwrap();
            let tx = crate::sunfish_mpss_linux_smem::ITEMS[2].1;
            let before = p.read(tx.offset, 16);
            crate::GLINK_FAULT.set(mode);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                c.glink_write_tx(0, b"12345678")
            }));
            if mode == 3 {
                assert!(outcome.is_err());
                assert_eq!(p.free_lock(), 1);
            } else {
                assert!(outcome.unwrap().is_err());
                assert_eq!(p.free_lock(), 0);
            }
            let after = p.read(tx.offset, 16);
            assert_eq!(after != before, mode == 2 || mode == 3);
            assert!(c.glink_write_tx(8, b"ABCDEFGH").is_err());
            assert_eq!(p.read(tx.offset, 16), after);
            let r = c.glink_report().unwrap();
            assert_eq!((r.stores_sent, r.stores_done), (1, 0));
            assert!(r.error.is_some());
        });
    }
}

#[test]
fn glink_memory_does_not_unlock_or_write_through_a_foreign_lock() {
    with_authenticated_link(|c, p| {
        let before = p.read(0x11d000, 0x7c000);
        p.set_foreign_lock();
        assert!(c.glink_write_tx(0, b"12345678").is_err());
        assert_eq!(p.free_lock(), 2);
        assert_eq!(p.read(0x11d000, 0x7c000), before);
        assert!(c.glink_poll().is_err());
        assert_eq!(p.free_lock(), 2);
    });
}

#[test]
fn glink_memory_matches_original_linux_descriptor_and_fifo_wrap_effects() {
    let dir =
        std::env::var("SUNFISH_GLK_MEMORY_C").expect("original pinned Linux fixture directory");
    for bytes in [4096, 16384, 32768] {
        let expected = std::fs::read(format!("{dir}/rx-{bytes}.bin")).unwrap();
        with_authenticated_link(|c, p| {
            use crate::sunfish_mpss_linux_smem::ITEMS;
            assert_eq!(c.glink_poll(), Ok(GP::Waiting));
            p.allocate_rx(bytes, 0);
            c.glink_poll().unwrap();
            p.cursor(0, 128);
            c.glink_store_tx_head(16376).unwrap();
            c.glink_store_rx_tail((bytes - 8) as u32).unwrap();
            p.rx_bytes(bytes - 8, b"ABCDEFGH");
            p.rx_bytes(0, b"IJKLMNOP");
            p.cursor(3, 8);
            // This is the low-level memory boundary. FIFO space accounting,
            // framing and notification belong to the next transport layer.
            c.glink_write_tx(16376, b"12345678").unwrap();
            c.glink_write_tx(0, b"abcdefgh").unwrap();
            c.glink_store_tx_head(8).unwrap();
            let mut out = [0; 16];
            c.glink_read_rx(bytes - 8, &mut out[..8]).unwrap();
            c.glink_read_rx(0, &mut out[8..]).unwrap();
            c.glink_store_rx_tail(8).unwrap();
            assert_eq!(p.read(ITEMS[1].1.offset, 32), expected[..32]);
            assert_eq!(p.read(ITEMS[2].1.offset, 16384), expected[32..16416]);
            assert_eq!(
                p.read(p.rx.unwrap().offset, bytes),
                expected[16416..16416 + bytes]
            );
            assert_eq!(out, expected[16416 + bytes..]);
        });
    }
}

#[cfg(feature = "sunfish-glink-aligned-fifo-reads")]
#[test]
fn glink_aligned_fifo_reads_match_real_native_window_at_offsets_and_edges() {
    with_authenticated_link(|c, p| {
        use crate::sunfish_mpss_linux_smem::ITEMS;
        p.allocate_rx(4096, 0);
        c.glink_poll().unwrap();
        let pattern: Vec<u8> = (0..64).map(|i| (i as u8).wrapping_mul(37)).collect();
        p.rx_bytes(0, &pattern);
        c.glink_write_tx(0, &pattern).unwrap();
        for start in 0..8 {
            for len in 1..=24 {
                let mut rx = vec![0; len];
                let mut tx = vec![0; len];
                c.glink_read_rx(start, &mut rx).unwrap();
                c.glink_read_tx(start, &mut tx).unwrap();
                assert_eq!(rx, pattern[start..start + len]);
                assert_eq!(tx, pattern[start..start + len]);
            }
        }
        p.rx_bytes(4096 - 8, b"EDGEFIFO");
        c.glink_write_tx(ITEMS[2].1.size - 8, b"EDGEFIFO").unwrap();
        let mut edge = [0; 7];
        c.glink_read_rx(4096 - 7, &mut edge).unwrap();
        assert_eq!(&edge, b"DGEFIFO");
        c.glink_read_tx(ITEMS[2].1.size - 7, &mut edge).unwrap();
        assert_eq!(&edge, b"DGEFIFO");
        assert_eq!(p.free_lock(), 0);
    });
}

#[cfg(feature = "sunfish-glink-combined-cursor-sample")]
#[test]
fn glink_cursor_snapshot_keeps_local_length_order_and_live_checks() {
    for bytes in [4096, 16384, 32768] {
        with_authenticated_link(|c, p| {
            assert_eq!(c.glink_cursor_snapshot().unwrap(), (None, [0; 8]));
            p.allocate_rx(bytes, 0);
            c.glink_poll().unwrap();
            p.cursor(3, 8);
            let before = p.read(0x11d000, 0x7c000);
            let (length, words) = c.glink_cursor_snapshot().unwrap();
            assert_eq!(length, Some(bytes));
            assert_eq!(words, [0, 0, 0, 8, 0, 0, 0, 0]);
            assert_eq!(p.read(0x11d000, 0x7c000), before);
            assert_eq!(p.free_lock(), 0);
        });
    }
    // The helper preserves the old local-length-before-descriptor order even
    // if the checked descriptor call discovers RX. The next sample sees it.
    with_authenticated_link(|c, p| {
        p.allocate_rx(4096, 0);
        assert_eq!(c.glink_cursor_snapshot().unwrap().0, None);
        assert_eq!(c.glink_cursor_snapshot().unwrap().0, Some(4096));
    });
    for mode in 0..8 {
        with_authenticated_link(|c, p| {
            p.allocate_rx(4096, 0);
            c.glink_poll().unwrap();
            match mode {
                0 => p.peer_smp(true),
                1 => crate::GLINK_LIFE.set((2, Lifecycle::AuthReturned, SP::Active)),
                2 => crate::GLINK_LIFE.set((1, Lifecycle::AuthReturned, SP::Inactive)),
                3 => crate::GLINK_LIFE.set((1, Lifecycle::PreAuth, SP::Active)),
                4 => { p.word(p.rx.unwrap().offset - 12, 8192); p.word(0x11d00c, (p.free + 4096) as u32); },
                _ => p.corrupt_owned_cursor([1, 2, 4][mode - 5], 8),
            }
            let before = p.read(0x11d000, 0x7c000);
            assert!(c.glink_cursor_snapshot().is_err(), "mode {mode}");
            assert!(c.glink_cursor_snapshot().is_err());
            assert!(c.glink_write_tx(0, b"12345678").is_err());
            assert_eq!(p.read(0x11d000, 0x7c000), before);
            assert_eq!(p.free_lock(), 0);
        });
    }
}
