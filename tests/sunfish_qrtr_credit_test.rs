include!("sunfish_qrtr_discovery_test.rs");
struct CreditIo {
    base: *mut u8,
    lock: *const u32,
    rx: usize,
    last: Option<Packet>,
    packets: Vec<Packet>,
    tails: usize,
    heads: usize,
    notifications: usize,
    requests: u64,
    target: usize,
    fault: u8,
    overwrite: bool,
}
impl CreditIo {
    fn new(m: &mut [u8], lock: *const u32, rx: usize) -> Self {
        Self {
            base: m.as_mut_ptr(),
            lock,
            rx,
            last: None,
            packets: vec![],
            tails: 0,
            heads: 0,
            notifications: 0,
            requests: 1,
            target: 1,
            fault: 0,
            overwrite: false,
        }
    }
}
impl pixel_qrtr_discovery::Io for CreditIo {
    fn packet(&mut self, p: Packet) {
        assert_eq!(unsafe { *self.lock }, 1);
        assert!(matches!(p, Packet::Planned(_)));
        let (at, n, _, _) = p.shape();
        let (b, size) = p.bytes();
        assert_eq!(n, size);
        self.last = Some(p);
        self.packets.push(p);
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            m[TX.offset + at..TX.offset + at + n].copy_from_slice(&b[..n]);
            if self.fault == 1 {
                m[TX.offset + at] ^= 1
            }
            if self.fault == 9 && !p.is_done() && !p.has_request() {
                let tail = u32::from_le_bytes(
                    m[DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12]
                        .try_into()
                        .unwrap(),
                );
                m[self.rx + tail as usize..self.rx + tail as usize + 8]
                    .copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0]);
                w32(m, DESCRIPTOR.offset + 12, tail + 8)
            }
            if self.fault == 10 {
                let item = smem_item(m, 435).unwrap();
                w32(m, item.offset + 36, 0)
            }
        }
    }
    fn tail(&mut self, p: Packet) {
        assert!(p.writes_tail());
        assert_eq!(unsafe { *self.lock }, 1);
        self.tails += 1;
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            let before = u32::from_le_bytes(
                m[DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12]
                    .try_into()
                    .unwrap(),
            );
            let after = p.shape().2;
            assert!(after > before);
            if self.fault != 2 {
                w32(m, DESCRIPTOR.offset + 8, after)
            }
            if self.overwrite {
                m[self.rx + before as usize..self.rx + after as usize].fill(0xdd)
            }
            if self.fault == 17 && !p.is_done() {
                m[self.rx + after as usize..self.rx + after as usize + 8]
                    .copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0]);
                w32(m, DESCRIPTOR.offset + 12, after + 8);
            }
            if self.fault == 11 {
                let item = smem_item(m, 435).unwrap();
                w32(m, item.offset + 36, 0)
            }
        }
    }
    fn head(&mut self, p: Packet) {
        assert_eq!(unsafe { *self.lock }, 1);
        self.heads += 1;
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            let (at, n, _, _) = p.shape();
            if self.fault != 3 {
                w32(m, DESCRIPTOR.offset + 4, (at + n) as u32)
            }
            if self.fault == 12 {
                let item = smem_item(m, 435).unwrap();
                w32(m, item.offset + 36, 0)
            }
        }
    }
    fn notify(&mut self) {
        assert_eq!(unsafe { *self.lock }, 0);
        self.notifications += 1;
        if self.fault == 4 {
            return;
        }
        let p = self.last.unwrap();
        let (at, n, tail, id) = p.shape();
        let k = p.cycle();
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            w32(m, DESCRIPTOR.offset, (at + n) as u32);
            if p.is_done() {
                let requested = self.requests & (1u64 << k) != 0;
                if requested {
                    m[self.rx + tail as usize..self.rx + tail as usize + 8]
                        .copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0])
                }
                w32(
                    m,
                    DESCRIPTOR.offset + 12,
                    tail + if requested { 8 } else { 0 },
                );
                if self.fault == 5 || self.fault == 6 {
                    m[self.rx + tail as usize..self.rx + tail as usize + 8].copy_from_slice(&[
                        if self.fault == 5 { 13 } else { 7 },
                        0,
                        2,
                        0,
                        if self.fault == 6 { 64 } else { 52 },
                        0,
                        0,
                        0,
                    ]);
                    w32(m, DESCRIPTOR.offset + 12, tail + 8)
                }
            } else {
                if self.fault == 16 {
                    w32(m, DESCRIPTOR.offset + 12, tail);
                    return;
                }
                let mut b = announcement(id, if k == self.target { 0x45 } else { 0x2b });
                if self.fault == 7 {
                    w32(&mut b, 4, 2)
                }
                if self.fault == 8 {
                    w32(&mut b, 20, 3)
                }
                m[self.rx + tail as usize..self.rx + tail as usize + 72].copy_from_slice(&b);
                let requested = self.requests & (1u64 << (k + 1)) != 0;
                if requested {
                    m[self.rx + tail as usize + 72..self.rx + tail as usize + 80]
                        .copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0])
                }
                w32(
                    m,
                    DESCRIPTOR.offset + 12,
                    tail + 72 + if requested { 8 } else { 0 },
                );
            }
        }
    }
}
#[test]
fn empty_ring_after_live_observed_second_done_gets_fresh_credit_without_an_ack_or_tail_store() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = CreditIo::new(&mut m, &lock, rx.offset);
    io.overwrite = true;
    let r = pixel_qrtr_discovery::run_credit(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert!(r.passed() && r.found, "{r:?}");
    assert_eq!(
        (
            r.count,
            r.copied,
            r.done_consumed,
            r.advertised,
            r.requests,
            r.notifications
        ),
        (3, 2, 2, 2, 1, 4)
    );
    assert_eq!(r.words, [224, 224, 824, 896, 0, 0, 0, 0]);
    assert_eq!((r.tail_stores, r.head_stores, r.byte_writes), (3, 4, 56));
    let p = io.packets[3];
    assert!(!p.is_done() && !p.has_request() && !p.writes_tail());
    assert_eq!(p.shape(), (208, 16, 824, 4));
    assert_eq!(p.bytes().0[16..], [0; 8]);
    assert!(buffers[0].verified(&announcement(3, 0x2b)[16..68]));
    assert!(buffers[1].verified(&announcement(4, 0x45)[16..68]));
    assert!(buffers[2..].iter().all(|b| b.empty()));
}
#[test]
fn all_empty_and_mixed_request_cycles_keep_cursors_exact_and_never_ack_without_a_request() {
    for requests in [0u64, 0b10101, 0xffffffff] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) =
            discovery_fixture(&mut first, &mut second, 0x42, requests & 1 != 0);
        let mut buffers = pool();
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = CreditIo::new(&mut m, &lock, rx.offset);
        io.requests = requests;
        io.target = 5;
        let r = pixel_qrtr_discovery::run_credit(
            &mut w,
            &mut cap,
            &mut buffers,
            &mut io,
            &mut Time { now: 0, step: 1000 },
            |_| {},
        );
        assert!(r.passed(), "{r:?}");
        let req = (requests & 63).count_ones();
        assert_eq!(r.requests, req);
        assert_eq!(r.words[1], 168 + 24 * 6 + 8 * req);
        assert_eq!(r.words[2], 744 + 72 * 5 + 8 * req);
        assert_eq!(r.tail_stores, 6 + req);
        assert_eq!(io.packets.len(), 12);
        for (i, p) in io.packets.iter().enumerate() {
            assert_eq!(p.cycle(), i / 2);
            assert_eq!(p.is_done(), i % 2 == 0);
            if !p.is_done() {
                let expected = requests & (1 << (i / 2)) != 0;
                assert_eq!(p.has_request(), expected);
                assert_eq!(p.shape().1, if expected { 24 } else { 16 });
                assert_eq!(p.writes_tail(), expected)
            }
        }
    }
}
#[test]
fn no_request_policy_reaches_limit_without_reuse_or_final_completion() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = CreditIo::new(&mut m, &lock, rx.offset);
    io.requests = 0;
    io.target = usize::MAX;
    io.overwrite = true;
    let r = pixel_qrtr_discovery::run_credit(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert!(r.passed() && r.limit && !r.found, "{r:?}");
    assert_eq!(r.count, 33);
    assert_eq!(r.words, [936, 936, 2976, 3048, 0, 0, 0, 0]);
    assert_eq!(
        (
            r.requests,
            r.notifications,
            r.byte_writes,
            r.tail_stores,
            r.head_stores
        ),
        (0, 64, 768, 32, 64)
    );
    assert_eq!(io.packets.last().unwrap().shape().3, 34);
    assert!(buffers.iter().all(|b| b.guards()));
}
#[test]
fn unknown_or_wrong_size_pending_control_is_never_consumed_or_acknowledged() {
    for fault in [5, 6] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
        let mut buffers = pool();
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = CreditIo::new(&mut m, &lock, rx.offset);
        io.fault = fault;
        let r = pixel_qrtr_discovery::run_credit(
            &mut w,
            &mut cap,
            &mut buffers,
            &mut io,
            &mut Time { now: 0, step: 1000 },
            |_| {},
        );
        assert_eq!(r.error, Some(Error::Payload));
        assert_eq!(io.packets.len(), 1);
        assert_eq!(r.words[2], 744);
        assert_eq!(r.advertised, 0);
    }
}
#[test]
fn late_request_while_unrequested_credit_is_staged_stops_before_head_or_ack() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = CreditIo::new(&mut m, &lock, rx.offset);
    io.requests = 0;
    io.fault = 9;
    let r = pixel_qrtr_discovery::run_credit(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert_eq!(r.error, Some(Error::Unstable));
    assert_eq!(
        (io.packets.len(), io.heads, io.tails, io.notifications),
        (2, 1, 1, 1)
    );
    assert_eq!(r.requests, 0);
    assert_eq!(r.advertised, 0);
    assert_eq!(r.words[1..4], [176, 744, 752]);
    assert_eq!(io.packets[1].shape().1, 16);
    assert!(buffers.iter().all(|b| b.empty()));
}
#[test]
fn credit_policy_does_not_retry_partial_writes_or_ready_loss_or_wrong_data() {
    for fault in [1, 2, 3, 4, 7, 8, 10, 11, 12] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
        let mut buffers = pool();
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = CreditIo::new(&mut m, &lock, rx.offset);
        io.requests = 0;
        io.fault = fault;
        let r = pixel_qrtr_discovery::run_credit(
            &mut w,
            &mut cap,
            &mut buffers,
            &mut io,
            &mut Time {
                now: 0,
                step: 50000,
            },
            |_| {},
        );
        assert!(!r.passed() && r.error.is_some(), "fault{fault}: {r:?}");
        assert_eq!(r.copied, 0);
        assert_eq!(io.packets.len(), if matches!(fault, 7 | 8) { 2 } else { 1 });
        assert!(buffers.iter().all(|b| b.empty()));
        assert_eq!(lock, 0);
    }
}
#[test]
fn credit_native_adapter_only_writes_planned_controls_and_is_terminal() {
    for failed in [false, true] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
        let before = m.clone();
        let mut buffers = pool();
        let mut locks = [0x11, 0, 0x33];
        let mut door = [0x44, 0x55, 0x66, 0, 0x88];
        struct Peer {
            base: *mut u8,
            rx: usize,
            seen: u32,
            now: u64,
            failed: bool,
        }
        impl Clock for Peer {
            fn frequency(&self) -> u64 {
                1_000_000
            }
            fn now(&mut self) -> u64 {
                self.now += 1000;
                unsafe {
                    let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
                    let head = u32::from_le_bytes(
                        m[DESCRIPTOR.offset + 4..DESCRIPTOR.offset + 8]
                            .try_into()
                            .unwrap(),
                    );
                    if !self.failed && head != self.seen {
                        self.seen = head;
                        w32(m, DESCRIPTOR.offset, head);
                        if head == 192 {
                            m[self.rx + 744..self.rx + 816].copy_from_slice(&announcement(3, 0x45));
                            w32(m, DESCRIPTOR.offset + 12, 816)
                        }
                    }
                }
                self.now
            }
        }
        let mut clock = Peer {
            base: m.as_mut_ptr(),
            rx: rx.offset,
            seen: 168,
            now: 0,
            failed,
        };
        let mut w = unsafe { Window::new(m.as_ptr(), locks.as_mut_ptr().add(1)) };
        let mut once = unsafe {
            pixel_qrtr_discovery::DiscoveryOnce::credit(
                cap,
                &mut buffers,
                m.as_mut_ptr().add(PART),
                door.as_mut_ptr(),
            )
        };
        let r = once.run(&mut w, &mut clock, |_| {});
        assert_eq!(r.passed(), !failed, "{r:?}");
        let after = m.clone();
        assert_eq!(
            once.run(&mut w, &mut clock, |_| {}).error,
            Some(Error::Outbound)
        );
        assert_eq!(m, after);
        assert_eq!(locks, [0x11, 0, 0x33]);
        assert_eq!(door, [0x44, 0x55, 0x66, 1 << 12, 0x88]);
        for i in 0..m.len() {
            if !(TX.offset + 168..TX.offset + 192).contains(&i)
                && !(DESCRIPTOR.offset..DESCRIPTOR.offset + 16).contains(&i)
                && !(rx.offset + 744..rx.offset + 816).contains(&i)
            {
                assert_eq!(m[i], before[i], "native credit aperture {i:x}")
            }
        }
    }
}
#[test]
fn credit_stopped_clock_prevents_every_new_publication() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let before = m.clone();
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = CreditIo::new(&mut m, &lock, rx.offset);
    let r = pixel_qrtr_discovery::run_credit(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 0 },
        |_| {},
    );
    assert_eq!(r.error, Some(Error::Clock));
    assert!(io.packets.is_empty());
    assert_eq!(m, before);
}
#[test]
fn credit_does_not_treat_empty_data_wait_as_evidence_of_global_service_absence() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = CreditIo::new(&mut m, &lock, rx.offset);
    io.requests = 0;
    io.fault = 16;
    let r = pixel_qrtr_discovery::run_credit(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time {
            now: 0,
            step: 50000,
        },
        |_| {},
    );
    assert_eq!(r.error, Some(Error::Timeout));
    assert_eq!(r.stage, 8);
    assert!(!r.found && !r.limit && !r.passed());
    assert_eq!(r.count, 1);
    assert_eq!((r.requests, r.advertised, r.copied), (0, 1, 0));
    assert_eq!(io.packets.len(), 2);
    assert!(buffers.iter().all(|b| b.empty()));
}
#[test]
fn requested_credits_reach_the_maximum_cursors_and_write_ceiling_without_final_done() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = CreditIo::new(&mut m, &lock, rx.offset);
    io.requests = (1u64 << 33) - 1;
    io.target = usize::MAX;
    let r = pixel_qrtr_discovery::run_credit(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert!(r.passed() && r.limit && !r.found, "{r:?}");
    assert_eq!(r.words, [1192, 1192, 3232, 3312, 0, 0, 0, 0]);
    assert_eq!(r.requests, 32);
    assert_eq!(r.byte_writes + 4 * r.tail_stores + 4 * r.head_stores, 1536);
    assert_eq!(io.notifications, 64);
    assert!(!io.packets.last().unwrap().is_done());
    assert_eq!(r.count, 33);
}
#[test]
fn a_second_request_observed_after_releasing_the_first_stops_before_credit_head() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = CreditIo::new(&mut m, &lock, rx.offset);
    io.fault = 17;
    let r = pixel_qrtr_discovery::run_credit(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert_eq!(r.error, Some(Error::Unstable));
    assert_eq!(r.stage, 4);
    assert_eq!(
        (io.packets.len(), io.tails, io.heads, io.notifications),
        (2, 2, 1, 1)
    );
    assert_eq!(r.words[1..4], [176, 752, 760]);
    assert_eq!(r.advertised, 0);
    assert_eq!(r.requests, 0);
    assert!(buffers.iter().all(|b| b.empty()));
}
