include!("sunfish_qrtr_credit_test.rs");
struct WaitPeer {
    base: *mut u8,
    rx: usize,
    seen: u32,
    now: u64,
    delay: Option<u64>,
    pending: Option<(u32, u32, u64)>,
    target: usize,
    loss: Option<u64>,
    malformed: bool,
    freeze: bool,
    frozen: bool,
}
impl WaitPeer {
    fn new(m: &mut [u8], rx: usize) -> Self {
        Self {
            base: m.as_mut_ptr(),
            rx,
            seen: 168,
            now: 0,
            delay: None,
            pending: None,
            target: 0,
            loss: None,
            malformed: false,
            freeze: false,
            frozen: false,
        }
    }
}
impl Clock for WaitPeer {
    fn frequency(&self) -> u64 {
        1_000_000
    }
    fn now(&mut self) -> u64 {
        if !self.frozen {
            self.now += 50_000
        }
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            let head = u32::from_le_bytes(
                m[DESCRIPTOR.offset + 4..DESCRIPTOR.offset + 8]
                    .try_into()
                    .unwrap(),
            );
            if head != self.seen {
                assert!(head > self.seen && head <= 1192);
                let cmd = u16::from_le_bytes(
                    m[TX.offset + self.seen as usize..TX.offset + self.seen as usize + 2]
                        .try_into()
                        .unwrap(),
                );
                w32(m, DESCRIPTOR.offset, head);
                if cmd == 5 {
                    let at = TX.offset + self.seen as usize;
                    assert_eq!(head - self.seen, 16);
                    let id = u32::from_le_bytes(m[at + 12..at + 16].try_into().unwrap());
                    assert!((3..=34).contains(&id));
                    let tail = u32::from_le_bytes(
                        m[DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12]
                            .try_into()
                            .unwrap(),
                    );
                    if let Some(delay) = self.delay {
                        self.pending = Some((id, tail, self.now + delay))
                    }
                    if self.freeze {
                        self.frozen = true
                    }
                } else {
                    assert_eq!(cmd, 6);
                    assert_eq!(head - self.seen, 8)
                }
                self.seen = head;
            }
            if let Some(at) = self.loss {
                if self.now >= at {
                    let item = smem_item(m, 435).unwrap();
                    w32(m, item.offset + 36, 0)
                }
            }
            if let Some((id, tail, due)) = self.pending {
                if self.now >= due {
                    let mut b = announcement(
                        id,
                        if id as usize == 3 + self.target {
                            0x45
                        } else {
                            0x2b
                        },
                    );
                    if self.malformed {
                        w32(&mut b, 20, 3)
                    }
                    m[self.rx + tail as usize..self.rx + tail as usize + 72].copy_from_slice(&b);
                    w32(m, DESCRIPTOR.offset + 12, tail + 72);
                    self.pending = None;
                }
            }
        }
        self.now
    }
}
#[test]
fn measured_wait_receives_a_25_second_packet_that_the_old_budget_rejects() {
    for extended in [false, true] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
        let mut buffers = pool();
        let mut lock = 0;
        let mut door = [0; 4];
        let mut clock = WaitPeer::new(&mut m, rx.offset);
        clock.delay = Some(25_000_000);
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut once = if extended {
            unsafe {
                pixel_qrtr_discovery::DiscoveryOnce::wait(
                    cap,
                    &mut buffers,
                    m.as_mut_ptr().add(PART),
                    door.as_mut_ptr(),
                )
            }
        } else {
            unsafe {
                pixel_qrtr_discovery::DiscoveryOnce::credit(
                    cap,
                    &mut buffers,
                    m.as_mut_ptr().add(PART),
                    door.as_mut_ptr(),
                )
            }
        };
        let r = once.run(&mut w, &mut clock, |_| {});
        assert_eq!(r.passed(), extended, "{r:?}");
        assert_eq!(r.frequency, 1_000_000);
        assert_eq!(r.slave_kernel, 6);
        if extended {
            assert!(r.found);
            assert!((25_000..30_000).contains(&r.elapsed_ms));
            assert_eq!((r.advertised, r.requests, r.copied), (1, 0, 1))
        } else {
            assert_eq!(r.error, Some(Error::Timeout));
            assert!((20_000..21_000).contains(&r.elapsed_ms));
            assert_eq!(r.copied, 0)
        }
    }
}
#[test]
fn measured_empty_wait_stops_at_60_seconds_without_reoffering_or_reusing_credit() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let mut buffers = pool();
    let mut locks = [0x11, 0, 0x33];
    let mut door = [0x44, 0x55, 0x66, 0, 0x88];
    let mut clock = WaitPeer::new(&mut m, rx.offset);
    let mut w = unsafe { Window::new(m.as_ptr(), locks.as_mut_ptr().add(1)) };
    let mut once = unsafe {
        pixel_qrtr_discovery::DiscoveryOnce::wait(
            cap,
            &mut buffers,
            m.as_mut_ptr().add(PART),
            door.as_mut_ptr(),
        )
    };
    let r = once.run(&mut w, &mut clock, |_| {});
    assert_eq!(r.error, Some(Error::Timeout));
    assert_eq!(r.stage, 8);
    assert!((59_900..61_000).contains(&r.elapsed_ms), "{r:?}");
    assert!(r.data_samples > 100 && r.data_samples <= 8192);
    assert_eq!(r.words, [192, 192, 744, 744, 0, 0, 0, 0]);
    assert_eq!(
        (
            r.advertised,
            r.requests,
            r.notifications,
            r.head_stores,
            r.tail_stores,
            r.byte_writes
        ),
        (1, 0, 2, 2, 1, 24)
    );
    assert!(!r.found && !r.limit && r.old_guards && r.pool_guards);
    let before = m.clone();
    assert_eq!(
        once.run(&mut w, &mut clock, |_| {}).error,
        Some(Error::Outbound)
    );
    assert_eq!(m, before);
    assert_eq!(locks, [0x11, 0, 0x33]);
    assert_eq!(door, [0x44, 0x55, 0x66, 1 << 12, 0x88]);
}
#[test]
fn measured_wait_retains_an_independent_65_second_global_limit_across_packets() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let mut buffers = pool();
    let mut lock = 0;
    let mut door = [0; 4];
    let mut clock = WaitPeer::new(&mut m, rx.offset);
    clock.delay = Some(6_000_000);
    clock.target = usize::MAX - 100;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut once = unsafe {
        pixel_qrtr_discovery::DiscoveryOnce::wait(
            cap,
            &mut buffers,
            m.as_mut_ptr().add(PART),
            door.as_mut_ptr(),
        )
    };
    let r = once.run(&mut w, &mut clock, |_| {});
    assert_eq!(r.error, Some(Error::Timeout));
    assert!((65_000..66_000).contains(&r.elapsed_ms), "{r:?}");
    assert!(r.count > 2 && r.count < 33);
    assert!(!r.limit && !r.found);
    assert!(r.notifications <= 64 && r.words[1] <= 1192 && r.words[3] <= 3312);
    assert!(r.byte_writes + 4 * r.head_stores + 4 * r.tail_stores <= 1536);
}
#[test]
fn a_stopped_counter_during_the_extended_wait_has_an_independent_work_bound() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let mut buffers = pool();
    let mut lock = 0;
    let mut door = [0; 4];
    let mut clock = WaitPeer::new(&mut m, rx.offset);
    clock.freeze = true;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut once = unsafe {
        pixel_qrtr_discovery::DiscoveryOnce::wait(
            cap,
            &mut buffers,
            m.as_mut_ptr().add(PART),
            door.as_mut_ptr(),
        )
    };
    let r = once.run(&mut w, &mut clock, |_| {});
    assert_eq!(r.error, Some(Error::Clock));
    assert_eq!(r.stage, 8);
    assert_eq!(r.advertised, 1);
    assert_eq!(r.copied, 0);
    assert_eq!(r.notifications, 2);
    assert!(r.data_samples < 10);
}
#[test]
fn late_malformed_data_and_readiness_loss_still_stop_instead_of_using_the_long_budget() {
    for revoke in [false, true] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
        let mut buffers = pool();
        let mut lock = 0;
        let mut door = [0; 4];
        let mut clock = WaitPeer::new(&mut m, rx.offset);
        if revoke {
            clock.loss = Some(25_000_000)
        } else {
            clock.delay = Some(25_000_000);
            clock.malformed = true
        }
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut once = unsafe {
            pixel_qrtr_discovery::DiscoveryOnce::wait(
                cap,
                &mut buffers,
                m.as_mut_ptr().add(PART),
                door.as_mut_ptr(),
            )
        };
        let r = once.run(&mut w, &mut clock, |_| {});
        assert_eq!(
            r.error,
            Some(if revoke {
                Error::ReadyLost
            } else {
                Error::Payload
            })
        );
        assert!((24_000..30_000).contains(&r.elapsed_ms));
        assert_eq!((r.advertised, r.copied, r.notifications), (1, 0, 2));
        assert_eq!(r.count, 1);
    }
}
#[test]
fn an_immediate_packet_has_the_same_native_write_aperture_in_measured_wait_mode() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let before = m.clone();
    let mut buffers = pool();
    let mut lock = 0;
    let mut door = [0; 4];
    let mut clock = WaitPeer::new(&mut m, rx.offset);
    clock.delay = Some(0);
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut once = unsafe {
        pixel_qrtr_discovery::DiscoveryOnce::wait(
            cap,
            &mut buffers,
            m.as_mut_ptr().add(PART),
            door.as_mut_ptr(),
        )
    };
    let r = once.run(&mut w, &mut clock, |_| {});
    assert!(r.passed() && r.found, "{r:?}");
    assert_eq!(r.words, [192, 192, 744, 816, 0, 0, 0, 0]);
    for i in 0..m.len() {
        if !(TX.offset + 168..TX.offset + 192).contains(&i)
            && !(DESCRIPTOR.offset..DESCRIPTOR.offset + 16).contains(&i)
            && !(rx.offset + 744..rx.offset + 816).contains(&i)
        {
            assert_eq!(m[i], before[i], "measured native aperture {i:x}")
        }
    }
}
