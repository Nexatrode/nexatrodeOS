include!("sunfish_qrtr_next_test.rs");
#[path = "../rust/src/pixel_qrtr_discovery.rs"]
mod pixel_qrtr_discovery;
use pixel_qrtr_discovery::{MAX_NEW, Packet, Record};
fn announcement(id: u32, service: u32) -> [u8; 72] {
    let mut b = next_packet();
    w32(&mut b, 4, id);
    w32(&mut b, 52, service);
    b
}
struct InitialPeer {
    base: *mut u8,
    door: *const u32,
    rx: usize,
    now: u64,
    service: u32,
    suffix: bool,
}
impl Clock for InitialPeer {
    fn frequency(&self) -> u64 {
        1_000_000
    }
    fn now(&mut self) -> u64 {
        self.now += 50000;
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            if u32::from_le_bytes(
                m[DESCRIPTOR.offset + 4..DESCRIPTOR.offset + 8]
                    .try_into()
                    .unwrap(),
            ) == 168
                && self.door.add(3).read_volatile() == 1 << 12
            {
                w32(m, DESCRIPTOR.offset, 168);
                w32(
                    m,
                    DESCRIPTOR.offset + 12,
                    if self.suffix { 752 } else { 744 },
                );
                m[self.rx + 672..self.rx + 744].copy_from_slice(&announcement(2, self.service));
                if self.suffix {
                    m[self.rx + 744..self.rx + 752].copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0]);
                }
            }
        }
        self.now
    }
}
fn discovery_fixture<'a, 'b>(
    first: &'a mut pixel_glink_rx::Buffer,
    second: &'b mut pixel_glink_rx::Buffer,
    service: u32,
    suffix: bool,
) -> (
    Vec<u8>,
    pixel_qrtr_next::DiscoveryCheckpoint<'a, 'b>,
    qcom_modem_status::Item,
) {
    let (mut m, cap, rx) = next_fixture(first);
    let mut lock = 0;
    let mut door = [0; 4];
    let mut clock = InitialPeer {
        base: m.as_mut_ptr(),
        door: door.as_ptr(),
        rx: rx.offset,
        now: 0,
        service,
        suffix,
    };
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let once = unsafe {
        pixel_qrtr_next::NextOnce::new(cap, second, m.as_mut_ptr().add(PART), door.as_mut_ptr())
    };
    let cap = match once.into_discovery_checkpoint(&mut w, &mut clock, |_| {}) {
        Ok(c) => c,
        Err(e) => panic!("discovery fixture: {:?}", e.report),
    };
    (m, cap, rx)
}
struct DiscoveryIo {
    base: *mut u8,
    lock: *const u32,
    rx: usize,
    last: Option<Packet>,
    packets: Vec<Packet>,
    tails: usize,
    heads: usize,
    notifies: usize,
    target_at: usize,
    fault: u8,
    overwrite: bool,
    pool: *mut pixel_glink_rx::Buffer,
    old: *mut pixel_glink_rx::Buffer,
}
impl DiscoveryIo {
    fn new(m: &mut [u8], lock: *const u32, rx: usize) -> Self {
        Self {
            base: m.as_mut_ptr(),
            lock,
            rx,
            last: None,
            packets: vec![],
            tails: 0,
            heads: 0,
            notifies: 0,
            target_at: 0,
            fault: 0,
            overwrite: false,
            pool: std::ptr::null_mut(),
            old: std::ptr::null_mut(),
        }
    }
}
impl pixel_qrtr_discovery::Io for DiscoveryIo {
    fn packet(&mut self, p: Packet) {
        assert_eq!(unsafe { *self.lock }, 1);
        self.last = Some(p);
        self.packets.push(p);
        let (at, n, _, _) = p.shape();
        let (b, size) = p.bytes();
        assert_eq!(n, size);
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            m[TX.offset + at..TX.offset + at + n].copy_from_slice(&b[..n]);
            if self.fault == 1 {
                m[TX.offset + at] ^= 1
            }
            if self.fault == 7 {
                let item = smem_item(m, 435).unwrap();
                w32(m, item.offset + 36, 0)
            }
            if self.fault == 13 {
                m[TX.offset + MAX_NEW * 32 + 168] = 1
            }
        }
    }
    fn tail(&mut self, p: Packet) {
        assert_eq!(unsafe { *self.lock }, 1);
        self.tails += 1;
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            let old = u32::from_le_bytes(
                m[DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12]
                    .try_into()
                    .unwrap(),
            ) as usize;
            if self.fault != 2 {
                w32(m, DESCRIPTOR.offset + 8, p.shape().2)
            }
            if self.overwrite {
                m[self.rx + old..self.rx + p.shape().2 as usize].fill(0xdd)
            }
            if self.fault == 8 {
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
            if self.fault == 9 {
                let item = smem_item(m, 435).unwrap();
                w32(m, item.offset + 36, 0)
            }
        }
    }
    fn notify(&mut self) {
        assert_eq!(unsafe { *self.lock }, 0);
        self.notifies += 1;
        if self.fault == 4 {
            return;
        }
        let p = self.last.unwrap();
        let (at, n, tail, id) = p.shape();
        unsafe {
            let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
            w32(m, DESCRIPTOR.offset, (at + n) as u32);
            match p {
                Packet::Planned(_) => panic!("legacy mock received credit plan"),
                Packet::Done(_) => {
                    m[self.rx + tail as usize..self.rx + tail as usize + 8]
                        .copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0]);
                    w32(m, DESCRIPTOR.offset + 12, tail + 8);
                    if self.fault == 5 {
                        m[self.rx + tail as usize] = 13
                    }
                    if self.fault == 6 {
                        m[self.rx + tail as usize + 4] = 64
                    }
                    if self.fault == 14 {
                        w32(m, DESCRIPTOR.offset + 12, tail + 16)
                    }
                }
                Packet::Advertise(k) => {
                    let mut b = announcement(
                        id,
                        if k == self.target_at {
                            0x45
                        } else {
                            0x42 + k as u32
                        },
                    );
                    match self.fault {
                        10 => w32(&mut b, 4, 1),
                        11 => w32(&mut b, 8, 53),
                        12 => w32(&mut b, 12, 1),
                        15 => w32(&mut b, 20, 3),
                        16 => w32(&mut b, 64, 0),
                        17 => w32(&mut b, 60, 1),
                        _ => {}
                    }
                    m[self.rx + tail as usize..self.rx + tail as usize + 72].copy_from_slice(&b);
                    m[self.rx + tail as usize + 72..self.rx + tail as usize + 80]
                        .copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0]);
                    w32(m, DESCRIPTOR.offset + 12, tail + 80);
                    if self.fault == 18 {
                        self.pool.cast::<u8>().write(0)
                    }
                    if self.fault == 19 {
                        self.old.cast::<u8>().add(16).write(0)
                    }
                }
            }
        }
    }
}
fn pool() -> [pixel_glink_rx::Buffer; MAX_NEW] {
    std::array::from_fn(|_| pixel_glink_rx::Buffer::new())
}
#[test]
fn passive_inventory_finds_target_and_retains_each_unique_buffer_without_completing_target() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
    io.target_at = 2;
    io.overwrite = true;
    let r = pixel_qrtr_discovery::run(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert!(r.passed(), "{r:?}");
    assert!(r.found && !r.limit);
    assert_eq!(r.count, 4);
    assert_eq!(r.target.service, 0x45);
    assert_eq!(
        (
            r.copied,
            r.done_consumed,
            r.requests,
            r.advertised,
            r.notifications
        ),
        (3, 3, 3, 3, 6)
    );
    assert_eq!(
        io.packets,
        vec![
            Packet::Done(0),
            Packet::Advertise(0),
            Packet::Done(1),
            Packet::Advertise(1),
            Packet::Done(2),
            Packet::Advertise(2)
        ]
    );
    assert_eq!(r.words, [264, 264, 912, 992, 0, 0, 0, 0]);
    assert!(r.old_guards && r.pool_guards);
    for i in 0..3 {
        assert!(buffers[i].verified(
            &announcement(3 + i as u32, if i == 2 { 0x45 } else { 0x42 + i as u32 })[16..68]
        ))
    }
    assert!(buffers[3..].iter().all(|b| b.empty()));
    assert_eq!(lock, 0);
}
#[test]
fn already_announced_target_stops_without_any_new_packet_or_index_store() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x45, true);
    let before = m.clone();
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
    let r = pixel_qrtr_discovery::run(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert!(r.passed() && r.found);
    assert_eq!(r.count, 1);
    assert!(io.packets.is_empty());
    assert_eq!(m, before);
}
#[test]
fn limit_is_partial_inventory_and_never_reuses_an_intent_or_completes_last_frame() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
    io.target_at = usize::MAX; // keep non-target IDs across the whole inventory
    // All background services below start0x42; avoid accidentally producing0x45 at k3.
    io.fault = 20;
    // A dedicated wrapper changes service IDs after each peer notification.
    struct NoTarget(DiscoveryIo);
    impl pixel_qrtr_discovery::Io for NoTarget {
        fn packet(&mut self, p: Packet) {
            self.0.packet(p)
        }
        fn tail(&mut self, p: Packet) {
            self.0.tail(p)
        }
        fn head(&mut self, p: Packet) {
            self.0.head(p)
        }
        fn notify(&mut self) {
            self.0.notify();
            if let Some(Packet::Advertise(_)) = self.0.last {
                unsafe {
                    let m = std::slice::from_raw_parts_mut(self.0.base, SMEM_BYTES);
                    let tail = self.0.last.unwrap().shape().2 as usize;
                    w32(m, self.0.rx + tail + 52, 0x42)
                }
            }
        }
    }
    let mut io = NoTarget(io);
    let r = pixel_qrtr_discovery::run(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert!(r.passed() && r.limit && !r.found, "{r:?}");
    assert_eq!(r.count, 33);
    assert_eq!(r.words, [1192, 1192, 3232, 3312, 0, 0, 0, 0]);
    assert_eq!(r.notifications, 64);
    assert_eq!(r.byte_writes, 1024);
    assert_eq!((r.head_stores, r.tail_stores), (64, 64));
    assert!(buffers.iter().all(|b| b.guards()));
    assert_eq!(io.0.packets.last(), Some(&Packet::Advertise(31)));
}
#[test]
fn unclassified_suffix_must_be_one_exact_request_before_advertisement() {
    for fault in [5, 6, 14] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
        let mut buffers = pool();
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
        io.fault = fault;
        let r = pixel_qrtr_discovery::run(
            &mut w,
            &mut cap,
            &mut buffers,
            &mut io,
            &mut Time { now: 0, step: 1000 },
            |_| {},
        );
        assert!(!r.passed());
        assert_eq!(io.packets, vec![Packet::Done(0)]);
        assert_eq!(r.advertised, 0);
        assert!(buffers.iter().all(|b| b.empty()));
        assert_eq!(r.words[2], 744);
    }
}
#[test]
fn request_can_arrive_after_done_and_is_not_invented_from_the_prior_suffix() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, false);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
    let r = pixel_qrtr_discovery::run(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |_| {},
    );
    assert!(r.passed() && r.found, "{r:?}");
    assert_eq!(r.requests, 1);
}
#[test]
fn partial_publication_or_ready_loss_is_terminal_with_no_later_response() {
    for fault in [1, 2, 3, 4, 7, 8, 9, 13] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
        let mut buffers = pool();
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
        io.fault = fault;
        let r = pixel_qrtr_discovery::run(
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
        assert!(!r.passed() && r.error.is_some(), "fault{fault} {r:?}");
        assert_eq!(io.packets, vec![Packet::Done(0)]);
        assert_eq!(io.notifies, usize::from(fault == 4));
        assert_eq!(r.advertised, 0);
        assert_eq!(lock, 0);
    }
}
#[test]
fn wrong_data_intent_chunk_fragmentation_or_non_server_cannot_copy_private_data() {
    for fault in [10, 11, 12, 15, 16, 17, 18, 19] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
        let mut buffers = pool();
        let old = {
            let (h, _, _, _) = cap.parts();
            let (s, _, _) = h.parts();
            let (c, _) = s.parts();
            c.parts().5 as *mut _
        };
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
        io.fault = fault;
        io.pool = buffers.as_mut_ptr();
        io.old = old;
        let r = pixel_qrtr_discovery::run(
            &mut w,
            &mut cap,
            &mut buffers,
            &mut io,
            &mut Time { now: 0, step: 1000 },
            |_| {},
        );
        assert!(!r.passed(), "fault{fault} {r:?}");
        assert_eq!(r.copied, 0);
        assert_eq!(io.packets.len(), 2);
        assert_eq!(r.count, 1);
    }
}
#[test]
fn dirty_or_corrupted_old_or_pool_buffers_and_live_identity_prevent_new_writes() {
    for fault in 0..5 {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
        let mut buffers = pool();
        match fault {
            0 => unsafe { buffers.as_mut_ptr().cast::<u8>().add(95).write(0) },
            1 => unsafe { buffers.as_mut_ptr().cast::<u8>().add(16).write(1) },
            2 => {
                let (h, b, _, _) = cap.parts();
                unsafe {
                    (b as *mut pixel_glink_rx::Buffer)
                        .cast::<u8>()
                        .add(16)
                        .write(0)
                }
                let _ = h;
            }
            3 => m[PART + 48 + 20] ^= 1,
            _ => m[TX.offset + 1192] = 1,
        }
        let before = m.clone();
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
        let r = pixel_qrtr_discovery::run(
            &mut w,
            &mut cap,
            &mut buffers,
            &mut io,
            &mut Time { now: 0, step: 1000 },
            |_| {},
        );
        assert!(r.error.is_some());
        assert!(io.packets.is_empty());
        assert_eq!(m, before);
    }
}
#[test]
fn stopped_clock_prevents_new_discovery_writes_and_global_budget_is_bounded() {
    for step in [0, 10_000_000] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
        let mut buffers = pool();
        let mut lock = 0;
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
        let r = pixel_qrtr_discovery::run(
            &mut w,
            &mut cap,
            &mut buffers,
            &mut io,
            &mut Time { now: 0, step },
            |_| {},
        );
        assert_eq!(
            r.error,
            Some(if step == 0 {
                Error::Clock
            } else {
                Error::Timeout
            })
        );
        assert!(io.packets.is_empty());
    }
}
#[test]
fn announcement_parser_bounds_all_fields_without_granting_a_service_endpoint() {
    let b = announcement(3, 0x45);
    assert_eq!(
        pixel_qrtr_discovery::service(b[16..68].try_into().unwrap()).unwrap(),
        Record {
            service: 0x45,
            instance: 0x100,
            node: 0,
            port: 0x5678
        }
    );
    for word in [0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 12] {
        let mut p: [u8; 52] = b[16..68].try_into().unwrap();
        let v = u32::from_le_bytes(p[word * 4..word * 4 + 4].try_into().unwrap());
        w32(&mut p, word * 4, if word == 12 { 0 } else { v ^ 1 });
        assert!(pixel_qrtr_discovery::service(&p).is_err(), "word{word}");
    }
}
#[test]
fn fixed_packets_have_unique_ids_and_stay_inside_the_bounded_ap_window() {
    let mut ids = std::collections::BTreeSet::new();
    for k in 0..MAX_NEW {
        let done = Packet::Done(k);
        let ad = Packet::Advertise(k);
        let (at, n, tail, id) = done.shape();
        assert_eq!(
            (at, n, tail, id),
            (168 + k * 32, 8, 744 + k as u32 * 80, 2 + k as u32)
        );
        let (at, n, tail, id) = ad.shape();
        assert_eq!(
            (at, n, tail, id),
            (176 + k * 32, 24, 752 + k as u32 * 80, 3 + k as u32)
        );
        assert!(at + n <= 1192 && tail + 80 <= 3312);
        assert!(ids.insert(id));
        assert_eq!(done.bytes().0[..4], [6, 0, 1, 0]);
        assert_eq!(ad.bytes().0[..4], [5, 0, 1, 0]);
        assert_eq!(ad.bytes().0[16..], [8, 0, 1, 0, 1, 0, 0, 0]);
    }
    assert_eq!(
        ids.into_iter().collect::<Vec<_>>(),
        (3..=34).collect::<Vec<_>>()
    );
}
#[test]
fn native_discovery_adapter_has_exact_aperture_and_single_use_for_success_or_failure() {
    for failed in [false, true] {
        let mut first = pixel_glink_rx::Buffer::new();
        let mut second = pixel_glink_rx::Buffer::new();
        let (mut m, cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
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
                        if head == 200 {
                            m[self.rx + 752..self.rx + 824].copy_from_slice(&announcement(3, 0x45));
                            m[self.rx + 824..self.rx + 832]
                                .copy_from_slice(&[7, 0, 2, 0, 52, 0, 0, 0]);
                            w32(m, DESCRIPTOR.offset + 12, 832)
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
            pixel_qrtr_discovery::DiscoveryOnce::new(
                cap,
                &mut buffers,
                m.as_mut_ptr().add(PART),
                door.as_mut_ptr(),
            )
        };
        let r = once.run(&mut w, &mut clock, |_| {});
        assert_eq!(r.passed(), !failed, "{r:?}");
        assert_eq!(r.error, if failed { Some(Error::Timeout) } else { None });
        let after = m.clone();
        assert_eq!(
            once.run(&mut w, &mut clock, |_| {}).error,
            Some(Error::Outbound)
        );
        assert_eq!(m, after);
        assert_eq!(locks, [0x11, 0, 0x33]);
        assert_eq!(door, [0x44, 0x55, 0x66, 1 << 12, 0x88]);
        for i in 0..m.len() {
            if !(TX.offset + 168..TX.offset + 200).contains(&i)
                && !(DESCRIPTOR.offset..DESCRIPTOR.offset + 16).contains(&i)
                && !(rx.offset + 752..rx.offset + 832).contains(&i)
            {
                assert_eq!(m[i], before[i], "native discovery aperture {i:x}")
            }
        }
    }
}
#[test]
fn failed_or_spent_next_receive_cannot_mint_discovery_authority() {
    for spent in [false, true] {
        let mut first = pixel_glink_rx::Buffer::new();
        let (mut m, cap, _) = next_fixture(&mut first);
        let mut second = pixel_glink_rx::Buffer::new();
        let mut lock = 0;
        let mut door = [0; 4];
        let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
        let mut once = unsafe {
            pixel_qrtr_next::NextOnce::new(
                cap,
                &mut second,
                m.as_mut_ptr().add(PART),
                door.as_mut_ptr(),
            )
        };
        if spent {
            assert_eq!(
                once.run(&mut w, &mut Time { now: 0, step: 0 }, |_| {})
                    .error,
                Some(Error::Clock)
            )
        }
        let before = m.clone();
        let stopped =
            match once.into_discovery_checkpoint(&mut w, &mut Time { now: 0, step: 0 }, |_| {}) {
                Err(e) => e,
                Ok(_) => panic!("failed next minted discovery"),
            };
        assert_eq!(
            stopped.report.error,
            Some(if spent { Error::Outbound } else { Error::Clock })
        );
        assert_eq!(m, before);
    }
}
#[test]
fn partial_new_data_waits_without_republishing_intent_or_done() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let io = DiscoveryIo::new(&mut m, &lock, rx.offset);
    struct Partial(DiscoveryIo);
    impl pixel_qrtr_discovery::Io for Partial {
        fn packet(&mut self, p: Packet) {
            self.0.packet(p)
        }
        fn tail(&mut self, p: Packet) {
            self.0.tail(p)
        }
        fn head(&mut self, p: Packet) {
            self.0.head(p)
        }
        fn notify(&mut self) {
            self.0.notify();
            if matches!(self.0.last, Some(Packet::Advertise(0))) {
                unsafe {
                    let m = std::slice::from_raw_parts_mut(self.0.base, SMEM_BYTES);
                    w32(m, DESCRIPTOR.offset + 12, 760)
                }
            }
        }
    }
    struct Finish {
        base: *mut u8,
        now: u64,
        observed: u32,
    }
    impl Clock for Finish {
        fn frequency(&self) -> u64 {
            1_000_000
        }
        fn now(&mut self) -> u64 {
            self.now += 1000;
            unsafe {
                let m = std::slice::from_raw_parts_mut(self.base, SMEM_BYTES);
                let h = u32::from_le_bytes(
                    m[DESCRIPTOR.offset + 12..DESCRIPTOR.offset + 16]
                        .try_into()
                        .unwrap(),
                );
                if h == 760 {
                    self.observed += 1;
                    if self.observed > 4 {
                        w32(m, DESCRIPTOR.offset + 12, 832)
                    }
                }
            }
            self.now
        }
    }
    let mut clock = Finish {
        base: m.as_mut_ptr(),
        now: 0,
        observed: 0,
    };
    let mut io = Partial(io);
    let r = pixel_qrtr_discovery::run(&mut w, &mut cap, &mut buffers, &mut io, &mut clock, |_| {});
    assert!(r.passed(), "{r:?}");
    assert_eq!(io.0.packets, vec![Packet::Done(0), Packet::Advertise(0)]);
    assert_eq!(r.copied, 1);
    assert!(clock.observed > 4);
}
#[test]
fn ready_revocation_after_private_copy_prevents_success_and_keeps_the_lease() {
    let mut first = pixel_glink_rx::Buffer::new();
    let mut second = pixel_glink_rx::Buffer::new();
    let (mut m, mut cap, rx) = discovery_fixture(&mut first, &mut second, 0x42, true);
    let mut buffers = pool();
    let mut lock = 0;
    let mut w = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let mut io = DiscoveryIo::new(&mut m, &lock, rx.offset);
    let base = m.as_mut_ptr();
    let mut changed = false;
    // The callback is after full copy validation; revoke before another target-free transaction.
    io.target_at = 1;
    let r = pixel_qrtr_discovery::run(
        &mut w,
        &mut cap,
        &mut buffers,
        &mut io,
        &mut Time { now: 0, step: 1000 },
        |r| {
            if r.copied == 1 && !changed {
                unsafe {
                    let m = std::slice::from_raw_parts_mut(base, SMEM_BYTES);
                    let p = smem_item(m, 435).unwrap();
                    w32(m, p.offset + 36, 0)
                }
                changed = true
            }
        },
    );
    assert_eq!(r.error, Some(Error::ReadyLost));
    assert_eq!(r.copied, 1);
    assert_eq!(io.packets.len(), 2);
    assert!(buffers[0].verified(&announcement(3, 0x42)[16..68]));
}
