// Compile the actual preceding startup and DMA-owner fixtures unchanged.
include!("sunfish_ipa_linux_receive_start_test.rs");
#[path = "../rust/src/sunfish_ipa_linux_receive_events.rs"]
pub mod sunfish_ipa_linux_receive_events;
#[path = "../rust/src/sunfish_ipa_linux_receive_runtime.rs"]
pub mod sunfish_ipa_linux_receive_runtime;
use sunfish_ipa_linux_receive_runtime as rt;

struct Tracked {
    words: Vec<u32>,
    reads: Vec<usize>,
    track: bool,
    fail: Option<usize>,
}
#[derive(Clone)]
struct RxData(Rc<RefCell<Tracked>>);
impl Memory for RxData {
    fn bytes(&self) -> usize {
        rx::DATA_RESERVED_BYTES
    }
    fn read32(&mut self, o: usize) -> Result<u32, MemoryError> {
        let mut s = self.0.borrow_mut();
        if s.track {
            let n = s.reads.len();
            s.reads.push(o);
            if s.fail == Some(n) {
                return Err(MemoryError::Bounds);
            }
        }
        Ok(s.words[o / 4])
    }
    fn write32(&mut self, o: usize, v: u32) -> Result<(), MemoryError> {
        self.0.borrow_mut().words[o / 4] = v;
        Ok(())
    }
}
struct RxIo {
    base: Fake,
    data: RxData,
    operations: usize,
    fail: Option<usize>,
    writes: Vec<(rt::Register, u32)>,
    ack: usize,
    arrival: bool,
}
impl RxIo {
    fn step(&mut self) -> bool {
        let n = self.operations;
        self.operations += 1;
        self.fail != Some(n)
    }
}
impl sunfish_ipa_linux_receive_start::Io<fixture::Ram, RxData> for RxIo {
    fn ticks(&mut self) -> u64 {
        self.base.now
    }
    fn guard(
        &mut self,
        p: &Pool<fixture::Ram>,
        d: &DataPool<RxData>,
        s: GuardState,
    ) -> Result<(), Error> {
        if !self.step()
            || !p.table_setup_complete()
            || !d.prepared()
            || self.base.value(g::TYPE_MASK) != s.gsi_types()
            || self.base.value(c::IEOB_MASK) != s.gsi_ieob()
            || self.base.value(g::CHANNEL_MASK) != s.gsi_channel_mask()
            || self.base.ipav(0x300c) != s.ipa_mask()
            || self.base.ipav(0x3034) != s.suspend_mask()
        {
            return Err(Error::Guard);
        }
        Ok(())
    }
    fn read(&mut self, r: Read) -> Result<u32, Error> {
        if !self.step() {
            return Err(Error::Io);
        }
        <Fake as Io<fixture::Ram, Data>>::read(&mut self.base, r)
    }
    fn write(&mut self, r: &Request) -> Result<(), Error> {
        <Fake as Io<fixture::Ram, Data>>::write(&mut self.base, r)
    }
    fn acknowledge_start(&mut self) -> Result<(), Error> {
        <Fake as Io<fixture::Ram, Data>>::acknowledge_start(&mut self.base)
    }
}
impl rt::Io<fixture::Ram, RxData> for RxIo {
    fn acquire(&mut self) -> Result<(), rt::Error> {
        if self.step() {
            Ok(())
        } else {
            Err(rt::Error::Io)
        }
    }
    fn release(&mut self) -> Result<(), rt::Error> {
        if self.step() {
            Ok(())
        } else {
            Err(rt::Error::Io)
        }
    }
    fn write_receive(&mut self, r: &rt::Request) -> Result<(), rt::Error> {
        let success = self.step();
        self.writes.push((r.register(), r.value()));
        match r.register() {
            rt::Register::IeobMask => {
                self.base.values.insert(c::IEOB_MASK, r.value());
            }
            rt::Register::IeobClear => {
                self.base
                    .values
                    .insert(c::IEOB_STATUS, self.base.value(c::IEOB_STATUS) & !r.value());
            }
            rt::Register::ChannelDoorbell if self.arrival => {
                self.arrival = false;
                post(self, 1, 1, 1, 7);
            }
            _ => {}
        }
        // An unsuccessful store is deliberately allowed to have taken effect.
        if success { Ok(()) } else { Err(rt::Error::Io) }
    }
    fn acknowledge_receive(&mut self) -> Result<(), rt::Error> {
        self.ack += 1;
        if self.step() {
            Ok(())
        } else {
            Err(rt::Error::Acknowledge)
        }
    }
}
type RxOnce = ReceiveOnce<fixture::Ram, RxData>;
type Runtime = rt::Runtime<fixture::Ram, RxData>;
fn rx_prepared() -> (fixture::Retained, RxOnce, RxIo) {
    let mut owned = fixture::completed();
    let data = RxData(Rc::new(RefCell::new(Tracked {
        words: vec![0; rx::DATA_RESERVED_BYTES / 4],
        reads: vec![],
        track: false,
        fail: None,
    })));
    let mut pool = DataPool::new(data.clone()).unwrap();
    assert_eq!(pool.prepare(|| Ok(())).error, None);
    let io = RxIo {
        base: Fake::new(owned.ram.clone()),
        data,
        operations: 0,
        fail: None,
        writes: vec![],
        ack: 0,
        arrival: false,
    };
    let rings = owned_pool(&mut owned);
    (owned, ReceiveOnce::new(rings, pool, 1000), io)
}
fn owned_pool(owned: &mut fixture::Retained) -> Pool<fixture::Ram> {
    owned.pool.take().unwrap()
}
fn rx_begin(owned: &fixture::Retained, once: &mut RxOnce, io: &mut RxIo) {
    once.begin(
        &owned.owners.0,
        &owned.owners.1,
        &owned.owners.2,
        &owned.memory,
        &owned.tables,
        io,
    )
    .unwrap();
}
fn rx_armed() -> (Runtime, RxIo) {
    let (owned, mut once, mut io) = rx_prepared();
    rx_begin(&owned, &mut once, &mut io);
    let now = io.base.now;
    once.notified(&mut io, now).unwrap();
    io.operations = 0;
    io.data.0.borrow_mut().track = true;
    (Runtime::new(once), io)
}
fn post(io: &mut RxIo, event: usize, tre: usize, buffer: usize, len: usize) {
    let mut raw = [0u8; 16];
    raw[..8].copy_from_slice(&(0x80009000u64 + (tre * 16) as u64).to_le_bytes());
    raw[8..10].copy_from_slice(&(len as u16).to_le_bytes());
    raw[11] = 0xa7;
    raw[14] = 0xe1;
    raw[15] = 2;
    for (n, chunk) in raw.chunks_exact(4).enumerate() {
        fixture::write_word(
            &io.base.ram,
            0x8000 + event * 16 + n * 4,
            u32::from_le_bytes(chunk.try_into().unwrap()),
        );
    }
    io.base
        .values
        .insert(0x1d090, 0x80008000 + (((event + 1) % 256) * 16) as u32);
    io.base.values.insert(c::IEOB_STATUS, 2);
    let mut data = io.data.0.borrow_mut();
    for offset in (0..len.min(rx::DMA_BYTES)).step_by(4) {
        let bytes = core::array::from_fn(|i| ((buffer * 13 + offset + i) % 251) as u8);
        data.words[(buffer * 8192 + 64 + offset) / 4] = u32::from_le_bytes(bytes);
    }
}
#[derive(Default)]
struct Collect {
    buffers: Vec<usize>,
    payloads: Vec<Vec<u8>>,
    no_read: bool,
}
impl rt::Consumer for Collect {
    fn consume(&mut self, packet: &mut impl rt::PacketRead) -> Result<(), rt::Error> {
        let c = packet.completion();
        assert_eq!((c.code(), c.kind()), (0xa7, 0xe1));
        self.buffers.push(c.buffer());
        let mut bytes = vec![0; c.length()];
        if !self.no_read {
            // Unaligned/chunked reads must expose the exact byte range.
            for (offset, chunk) in bytes.chunks_mut(7).enumerate() {
                packet.read_into(offset * 7, chunk)?;
            }
        }
        self.payloads.push(bytes);
        Ok(())
    }
}
fn rx_terminal(runtime: &mut Runtime, io: &mut RxIo) {
    assert!(runtime.report().error.is_some());
    assert!(!runtime.pools().0.table_setup_complete());
    assert!(!runtime.pools().1.prepared());
    let counts = (
        io.operations,
        io.writes.len(),
        io.base.writes.len(),
        io.data.0.borrow().reads.len(),
        io.ack,
    );
    assert_eq!(
        runtime.notification(io, &mut Collect::default()),
        Err(rt::Error::Quarantined)
    );
    assert_eq!(runtime.external_fault(io), Err(rt::Error::Quarantined));
    assert_eq!(
        (
            io.operations,
            io.writes.len(),
            io.base.writes.len(),
            io.data.0.borrow().reads.len(),
            io.ack
        ),
        counts
    );
}
#[test]
fn rxruntime_actual_handoff_event_cpu_read_repost_and_irq_order() {
    let (mut runtime, mut io) = rx_armed();
    let before = fixture::snapshot(&io.base.ram);
    post(&mut io, 0, 0, 0, 17);
    let mut sink = Collect::default();
    assert_eq!(runtime.notification(&mut io, &mut sink), Ok(1));
    assert_eq!(sink.buffers, vec![0]);
    assert_eq!(
        sink.payloads[0],
        (0..17).map(|n| n as u8).collect::<Vec<_>>()
    );
    let r = runtime.report();
    assert_eq!(
        (
            r.notifications,
            r.delivered,
            r.acks_sent,
            r.acks_done,
            r.guard_words
        ),
        (1, 1, 1, 1, 14256)
    );
    assert_eq!(io.ack, 1);
    assert_eq!(io.base.acks, 1);
    let q = runtime.queue_report().unwrap();
    assert_eq!(
        (q.in_flight, q.held, q.events, q.recycled, q.publish_cursor),
        (251, 0, 1, 1, 252)
    );
    assert_eq!(
        io.writes,
        vec![
            (rt::Register::IeobMask, 1),
            (rt::Register::IeobClear, 2),
            (rt::Register::EventDoorbell, 0x80008000),
            (rt::Register::ChannelDoorbell, 0x80009fc0),
            (rt::Register::IeobMask, 3)
        ]
    );
    let c = std::fs::read_to_string(env!("SUNFISH_RECEIVE_RUNTIME_C")).unwrap();
    assert_eq!(
        io.writes
            .iter()
            .filter(|(r, _)| matches!(r, rt::Register::IeobMask | rt::Register::IeobClear))
            .map(|(r, v)| format!("{:05x} {:08x}\n", r.offset(), v))
            .collect::<String>(),
        c
    );
    let after = fixture::snapshot(&io.base.ram);
    for (i, (a, b)) in before.iter().zip(after).enumerate() {
        if !(0x8000..0x8010).contains(&(i * 4)) && !(0x9fb0..0x9fc0).contains(&(i * 4)) {
            assert_eq!(*a, b, "unowned ring word {:x}", i * 4);
        }
    }
    assert!(
        io.data
            .0
            .borrow()
            .reads
            .iter()
            .filter(|o| **o < rx::DATA_USED_BYTES && **o % 8192 >= 64)
            .all(|o| (64..84).contains(o))
    );
}
#[test]
fn rxruntime_rejects_unarmed_handoff_without_io_and_retains_failed_pools() {
    for pending in [false, true] {
        let (owned, mut once, mut io) = rx_prepared();
        if pending {
            rx_begin(&owned, &mut once, &mut io);
        }
        let mut runtime = Runtime::new(once);
        assert_eq!(runtime.report().error, Some(rt::Error::Startup));
        rx_terminal(&mut runtime, &mut io);
    }
}
#[test]
fn rxruntime_consumer_cannot_ignore_bad_packet_bounds_or_repost_after_failure() {
    struct Bad(bool);
    impl rt::Consumer for Bad {
        fn consume(&mut self, p: &mut impl rt::PacketRead) -> Result<(), rt::Error> {
            if self.0 {
                let mut byte = [0];
                assert_eq!(
                    p.read_into(p.completion().length(), &mut byte),
                    Err(rt::Error::PacketBounds)
                );
                Ok(())
            } else {
                Err(rt::Error::Consumer)
            }
        }
    }
    for ignore in [false, true] {
        let (mut r, mut io) = rx_armed();
        post(&mut io, 0, 0, 0, 1);
        assert_eq!(
            r.notification(&mut io, &mut Bad(ignore)),
            Err(if ignore {
                rt::Error::PacketBounds
            } else {
                rt::Error::Consumer
            })
        );
        assert!(
            !io.writes
                .iter()
                .any(|(r, _)| *r == rt::Register::ChannelDoorbell)
        );
        assert_eq!(io.ack, 0);
        rx_terminal(&mut r, &mut io);
    }
}
#[test]
fn rxruntime_every_guard_mmio_ordering_and_uncertain_store_failure_contains() {
    let (mut r, mut io) = rx_armed();
    post(&mut io, 0, 0, 0, 7);
    r.notification(&mut io, &mut Collect::default()).unwrap();
    let operations = io.operations;
    for n in 0..operations {
        let (mut r, mut io) = rx_armed();
        post(&mut io, 0, 0, 0, 7);
        io.fail = Some(n);
        assert!(
            r.notification(&mut io, &mut Collect::default()).is_err(),
            "operation {n}/{operations}"
        );
        assert_eq!(r.report().acks_done, 0);
        rx_terminal(&mut r, &mut io);
    }
}
#[test]
fn rxruntime_actual_ring_and_data_backend_failures_prevent_ack_and_reuse() {
    for n in 0..16 {
        let (mut r, mut io) = rx_armed();
        post(&mut io, 0, 0, 0, 7);
        fixture::fail_read_after(&io.base.ram, n);
        assert!(
            r.notification(&mut io, &mut Collect::default()).is_err(),
            "ring read {n}"
        );
        rx_terminal(&mut r, &mut io);
    }
    for n in 0..4 {
        for suppress in [false, true] {
            let (mut r, mut io) = rx_armed();
            post(&mut io, 0, 0, 0, 7);
            // Poison the free destination, including the normally-zero high
            // address word, so every suppressed store has an observable effect.
            for word in 0..4 {
                fixture::write_word(&io.base.ram, 0x9fb0 + word * 4, 0xdeadbeef);
            }
            if suppress {
                fixture::suppress_write_after(&io.base.ram, n)
            } else {
                fixture::fail_write_after(&io.base.ram, n)
            }
            assert!(
                r.notification(&mut io, &mut Collect::default()).is_err(),
                "store {n}, suppressed {suppress}"
            );
            rx_terminal(&mut r, &mut io);
        }
    }
    for n in [0, 1, 2, 17, 4020, 14255] {
        let (mut r, mut io) = rx_armed();
        post(&mut io, 0, 0, 0, 7);
        io.data.0.borrow_mut().fail = Some(n);
        assert!(
            r.notification(&mut io, &mut Collect::default()).is_err(),
            "data read {n}"
        );
        rx_terminal(&mut r, &mut io);
    }
}
#[test]
fn rxruntime_foreign_error_and_corrupt_events_never_expose_packet_data() {
    for kind in 0..9 {
        let (mut r, mut io) = rx_armed();
        post(&mut io, 0, 0, 0, 7);
        match kind {
            0 => {
                io.base.values.insert(g::CHANNEL_STATUS, 4);
            }
            1 => {
                io.base.values.insert(c::IEOB_STATUS, 1);
            }
            2 => {
                io.base.values.insert(g::GLOBAL_STATUS, 1);
            }
            3 => {
                io.base.values.insert(g::GENERAL_STATUS, 2);
            }
            4 => {
                io.base.values.insert(0x1d090, 0x80008001);
            }
            5 => fixture::write_word(&io.base.ram, 0x8000, 0x80009010),
            6 => fixture::write_word(&io.base.ram, 0x8004, 1),
            7 => fixture::write_word(&io.base.ram, 0x8008, 8129),
            _ => fixture::write_word(&io.base.ram, 0x800c, 3 << 24),
        }
        assert!(
            r.notification(&mut io, &mut Collect::default()).is_err(),
            "corruption {kind}"
        );
        assert!(io.data.0.borrow().reads.is_empty());
        assert_eq!(io.ack, 0);
        rx_terminal(&mut r, &mut io);
    }
}
#[test]
fn rxruntime_arrival_during_repost_is_left_for_next_physical_notification() {
    let (mut r, mut io) = rx_armed();
    post(&mut io, 0, 0, 0, 7);
    io.arrival = true;
    let mut sink = Collect::default();
    assert_eq!(r.notification(&mut io, &mut sink), Ok(1));
    assert_eq!(io.base.value(c::IEOB_STATUS), 2);
    assert_eq!(r.notification(&mut io, &mut sink), Ok(1));
    assert_eq!(sink.buffers, vec![0, 1]);
    assert_eq!(io.ack, 2);
    assert_eq!(r.queue_report().unwrap().in_flight, 251);
}
#[test]
fn rxruntime_wraps_both_rings_recycles_only_completed_buffers_and_accepts_zero_length() {
    let (mut r, mut io) = rx_armed();
    let mut sink = Collect::default();
    for n in 0..1025 {
        let len = if n % 11 == 0 { 0 } else { 7 };
        post(&mut io, n % 256, n % 256, n % 251, len);
        assert_eq!(r.notification(&mut io, &mut sink), Ok(1));
        assert_eq!(sink.buffers[n], n % 251);
        assert_eq!(
            sink.payloads[n],
            (0..len)
                .map(|i| (((n % 251) * 13 + i) % 251) as u8)
                .collect::<Vec<_>>()
        );
    }
    let q = r.queue_report().unwrap();
    assert_eq!(
        (q.events, q.recycled, q.held, q.in_flight),
        (1025, 1025, 0, 251)
    );
    assert_eq!((r.report().delivered, r.report().acks_done), (1025, 1025));
}
#[test]
fn rxruntime_spurious_delivery_and_external_fault_do_not_consume_any_buffer() {
    let (mut r, mut io) = rx_armed();
    let mut sink = Collect::default();
    assert_eq!(r.notification(&mut io, &mut sink), Ok(0));
    assert_eq!(r.report().spurious, 1);
    assert_eq!(io.ack, 1);
    assert!(io.writes.is_empty());
    assert!(io.data.0.borrow().reads.is_empty());
    assert_eq!(r.external_fault(&mut io), Err(rt::Error::Guard));
    rx_terminal(&mut r, &mut io);
}
#[test]
fn rxruntime_backends_reject_reads_outside_own_rings_and_completed_dma_window() {
    let mut owned = fixture::completed();
    let mut pool = owned.pool.take().unwrap();
    pool.claim_receive().unwrap();
    for offset in (0..0x20000).step_by(4) {
        for event in [false, true] {
            let base = if event { 0x8000 } else { 0x9000 };
            assert_eq!(
                pool.receive_read32(offset, event).is_ok(),
                (base..base + 0x1000).contains(&offset)
            );
        }
    }
    for offset in [1, 0x8001, 0x9001, usize::MAX] {
        assert!(pool.receive_read32(offset, true).is_err());
        assert!(pool.receive_read32(offset, false).is_err());
    }
    let (_, _, io) = rx_prepared();
    let mut data = DataPool::new(io.data.clone()).unwrap();
    data.prepare(|| Ok(()));
    for (buffer, offset) in [
        (251, 0),
        (usize::MAX, 0),
        (0, 8128),
        (0, 1),
        (0, usize::MAX),
    ] {
        assert!(data.completed_read32(buffer, offset).is_err());
    }
    assert!(data.completed_read32(250, 8124).is_ok());
}
