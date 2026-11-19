#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_command.rs"]
pub mod sunfish_ipa_linux_command;
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
pub mod sunfish_ipa_linux_config;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
pub mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
pub mod sunfish_ipa_linux_gsi;
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
pub mod sunfish_ipa_linux_memory;
#[path = "../rust/src/sunfish_ipa_linux_memsetup.rs"]
pub mod sunfish_ipa_linux_memsetup;
#[path = "../rust/src/sunfish_ipa_linux_receive.rs"]
pub mod sunfish_ipa_linux_receive;
#[path = "../rust/src/sunfish_ipa_linux_receive_pool.rs"]
pub mod sunfish_ipa_linux_receive_pool;
#[path = "../rust/src/sunfish_ipa_linux_receive_start.rs"]
pub mod sunfish_ipa_linux_receive_start;
#[path = "../rust/src/sunfish_ipa_linux_table_transfer.rs"]
pub mod sunfish_ipa_linux_table_transfer;
#[path = "../rust/src/sunfish_ipa_linux_tables.rs"]
pub mod sunfish_ipa_linux_tables;
#[path = "../rust/src/sunfish_ipa_linux_transfer.rs"]
pub mod sunfish_ipa_linux_transfer;
mod fixture {
    include!(env!("SUNFISH_RECEIVE_OWNERS_FIXTURE"));
}
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use sunfish_ipa_linux_buffers::{Memory, MemoryError, Pool};
use sunfish_ipa_linux_command as c;
use sunfish_ipa_linux_endpoint as ep;
use sunfish_ipa_linux_gsi as g;
use sunfish_ipa_linux_receive::{self as rx, Space, Word};
use sunfish_ipa_linux_receive_pool::DataPool;
use sunfish_ipa_linux_receive_start::*;

#[derive(Clone)]
struct Data(Rc<RefCell<Vec<u32>>>);
impl Memory for Data {
    fn bytes(&self) -> usize {
        rx::DATA_RESERVED_BYTES
    }
    fn read32(&mut self, o: usize) -> Result<u32, MemoryError> {
        Ok(self.0.borrow()[o / 4])
    }
    fn write32(&mut self, o: usize, v: u32) -> Result<(), MemoryError> {
        self.0.borrow_mut()[o / 4] = v;
        Ok(())
    }
}
fn data() -> (DataPool<Data>, Data) {
    let r = Data(Rc::new(RefCell::new(vec![
        0xa5a5a5a5;
        rx::DATA_RESERVED_BYTES / 4
    ])));
    let mut p = DataPool::new(r.clone()).unwrap();
    assert_eq!(p.prepare(|| Ok(())).error, None);
    (p, r)
}
struct Fake {
    ram: fixture::Ram,
    values: BTreeMap<usize, u32>,
    ipa: BTreeMap<usize, u32>,
    writes: Vec<Word>,
    reads: usize,
    guards: usize,
    acks: usize,
    now: u64,
    cpu_base: usize,
    guard_ticks: u64,
    fail_read: Option<usize>,
    fail_guard: Option<usize>,
    fail_write: Option<usize>,
    suppress_write: Option<usize>,
    fail_ack: bool,
    fail_contain: bool,
    arrival: u8,
}
impl Fake {
    fn new(ram: fixture::Ram) -> Self {
        let cpu_base = fixture::write_count(&ram);
        let mut values = BTreeMap::from([
            (g::GSI_STATUS, 1),
            (g::TYPE_MASK, 0x4c),
            (c::IEOB_MASK, 1),
            (g::GLOBAL_ENABLE, 1),
            (0x1f120, 14),
            (0x1d090, 0),
        ]);
        for e in ep::ENDPOINTS {
            values.insert(
                c::Read::Channel(e).offset(),
                if e.channel() == 1 { 2 << 20 } else { 1 << 20 },
            );
            values.insert(c::Read::Event(e).offset(), 1 << 20);
        }
        Self {
            ram,
            values,
            ipa: BTreeMap::from([(0x300c, 0xc)]),
            writes: vec![],
            reads: 0,
            guards: 0,
            acks: 0,
            now: 100,
            cpu_base,
            guard_ticks: 0,
            fail_read: None,
            fail_guard: None,
            fail_write: None,
            suppress_write: None,
            fail_ack: false,
            fail_contain: false,
            arrival: 0,
        }
    }
    fn value(&self, o: usize) -> u32 {
        self.values.get(&o).copied().unwrap_or(0)
    }
    fn ipav(&self, o: usize) -> u32 {
        self.ipa.get(&o).copied().unwrap_or(0)
    }
    fn counts(&self) -> (usize, usize, usize, usize, usize) {
        (
            self.reads,
            self.guards,
            self.writes.len(),
            fixture::write_count(&self.ram) - self.cpu_base,
            self.acks,
        )
    }
}
impl Io<fixture::Ram, Data> for Fake {
    fn ticks(&mut self) -> u64 {
        self.now
    }
    fn guard(
        &mut self,
        p: &Pool<fixture::Ram>,
        d: &DataPool<Data>,
        s: GuardState,
    ) -> Result<(), Error> {
        let n = self.guards;
        self.guards += 1;
        self.now = self.now.wrapping_add(self.guard_ticks);
        if self.fail_guard == Some(n)
            || !p.table_setup_complete()
            || !d.prepared()
            || self.value(g::TYPE_MASK) != s.gsi_types()
            || self.value(c::IEOB_MASK) != s.gsi_ieob()
            || self.value(g::CHANNEL_MASK) != s.gsi_channel_mask()
            || self.ipav(0x300c) != s.ipa_mask()
            || self.ipav(0x3034) != s.suspend_mask()
        {
            Err(Error::Guard)
        } else {
            Ok(())
        }
    }
    fn read(&mut self, r: Read) -> Result<u32, Error> {
        let n = self.reads;
        self.reads += 1;
        if self.fail_read == Some(n) {
            return Err(Error::Io);
        }
        if r == Read::Gsi(c::Read::TypeStatus) {
            let mut v = self.value(g::TYPE_STATUS);
            for (o, m, t) in [
                (g::CHANNEL_STATUS, g::CHANNEL_MASK, 1),
                (g::EVENT_STATUS, g::EVENT_MASK, 2),
                (g::GLOBAL_STATUS, g::GLOBAL_ENABLE, 4),
                (c::IEOB_STATUS, c::IEOB_MASK, 8),
                (g::GENERAL_STATUS, 0x1f120, 64),
            ] {
                if self.value(o) & self.value(m) != 0 {
                    v |= t;
                }
            }
            return Ok(v);
        }
        Ok(match r.space() {
            Space::Gsi => self.value(r.offset()),
            Space::Ipa => self.ipav(r.offset()),
        })
    }
    fn write(&mut self, r: &Request) -> Result<(), Error> {
        let n = self.writes.len();
        let w = r.word();
        self.writes.push(w);
        if self.suppress_write != Some(n) {
            match (w.space, w.offset) {
                (Space::Gsi, 0x1f0a0) => {
                    self.values
                        .insert(g::CHANNEL_STATUS, self.value(g::CHANNEL_STATUS) & !w.value);
                }
                (Space::Gsi, 0x1f008) => {
                    assert_eq!(w.value, 0x01000002);
                    self.values.insert(0x1c100, 2 << 20);
                    self.values.insert(g::CHANNEL_STATUS, 4);
                }
                (Space::Gsi, o) => {
                    self.values.insert(o, w.value);
                }
                (Space::Ipa, o) => {
                    self.ipa.insert(o, w.value);
                }
            }
            if w.offset == 0x1e010 && self.arrival != 0 {
                self.values
                    .insert(c::IEOB_STATUS, if self.arrival == 1 { 2 } else { 1 });
                self.values.insert(0x1d090, 0x80008010);
            }
        }
        if self.fail_write == Some(n)
            || (self.fail_contain && w.value == 0 && [g::TYPE_MASK, 0x300c].contains(&w.offset))
        {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn acknowledge_start(&mut self) -> Result<(), Error> {
        assert_eq!(self.value(g::CHANNEL_STATUS), 0);
        assert_eq!(self.value(c::IEOB_STATUS), 0);
        self.acks += 1;
        if self.fail_ack {
            Err(Error::Acknowledge)
        } else {
            Ok(())
        }
    }
}
type Once = ReceiveOnce<fixture::Ram, Data>;
fn prepared() -> (fixture::Retained, Once, Fake, Data) {
    let mut owned = fixture::completed();
    let p = owned.pool.take().unwrap();
    let (d, r) = data();
    let io = Fake::new(owned.ram.clone());
    (owned, Once::new(p, d, 1000), io, r)
}
fn begin(owned: &fixture::Retained, o: &mut Once, io: &mut Fake) -> Result<(), Error> {
    o.begin(
        &owned.owners.0,
        &owned.owners.1,
        &owned.owners.2,
        &owned.memory,
        &owned.tables,
        io,
    )
}
fn run(owned: &fixture::Retained, o: &mut Once, io: &mut Fake) -> Result<(), Error> {
    begin(owned, o, io)?;
    let now = io.now;
    o.notified(io, now)
}
fn terminal(owned: &fixture::Retained, o: &mut Once, io: &mut Fake) {
    let before = io.counts();
    assert_eq!(begin(owned, o, io), Err(Error::Consumed));
    assert_eq!(o.notified(io, 100), Err(Error::Consumed));
    assert_eq!(o.timeout(io, 100), Err(Error::Consumed));
    if o.report().phase == Phase::Failed {
        assert_eq!(o.external_fault(io), Err(Error::Consumed));
    }
    assert_eq!(io.counts(), before);
}
#[test]
fn rxstart_full_retained_sequence_matches_composed_linux_c_without_touching_other_rings() {
    let (owned, mut once, mut io, _) = prepared();
    let before = fixture::snapshot(&owned.ram);
    run(&owned, &mut once, &mut io).unwrap();
    let r = once.report();
    assert_eq!(r.phase, Phase::Armed);
    assert_eq!((r.cpu_sent, r.cpu_done, r.cpu_checked), (1004, 1004, 1004));
    assert_eq!((r.blank_words, r.guard_words), (2048, 28512));
    assert_eq!((r.writes_sent, r.writes_done, r.doorbells), (27, 27, 16));
    assert_eq!((r.published_buffers, r.final_samples), (251, 2));
    assert!(r.start_sent && r.ack_done && r.route_done && !r.rx_pending);
    assert_eq!(r.rx_producer, None);
    assert_eq!(io.acks, 1);
    assert_eq!(r.containment_sent, 0);
    let expected = std::fs::read_to_string(env!("SUNFISH_RECEIVE_STARTUP_C")).unwrap();
    assert_eq!(
        io.writes
            .iter()
            .map(|w| format!(
                "{} {:05x} {:08x}\n",
                if w.space == Space::Gsi { "gsi" } else { "ipa" },
                w.offset,
                w.value
            ))
            .collect::<String>(),
        expected
    );
    for (i, (old, new)) in before.iter().zip(fixture::snapshot(&owned.ram)).enumerate() {
        if !(0x9000..0x9fb0).contains(&(i * 4)) {
            assert_eq!(*old, new, "unowned word {:x}", i * 4);
        }
    }
    assert!(once.pools().0.table_setup_complete() && once.pools().1.prepared());
    terminal(&owned, &mut once, &mut io);
}
#[test]
fn rxstart_actual_table_owner_and_data_preparation_are_required_before_any_io() {
    for fault in 0..3 {
        let mut owned = fixture::completed();
        let p = owned.pool.take().unwrap();
        let (mut d, _) = data();
        if fault == 0 {
            owned.tables = sunfish_ipa_linux_table_transfer::TableOnce::new(1000).unwrap();
        }
        if fault == 1 {
            d.quarantine();
        }
        let mut once = Once::new(p, d, if fault == 2 { 0 } else { 1000 });
        let mut io = Fake::new(owned.ram.clone());
        assert_eq!(
            begin(&owned, &mut once, &mut io),
            Err(if fault == 2 {
                Error::Layout
            } else {
                Error::Prerequisite
            })
        );
        assert_eq!(io.counts(), (0, 0, 0, 0, 0));
        terminal(&owned, &mut once, &mut io);
    }
}
#[test]
fn rxstart_each_read_guard_and_uncertain_mmio_or_descriptor_failure_is_terminal() {
    let (owned, mut once, mut io, _) = prepared();
    run(&owned, &mut once, &mut io).unwrap();
    let counts = io.counts();
    for kind in 0..4 {
        for n in 0..[counts.0, counts.1, counts.2, counts.3][kind] {
            let (owned, mut once, mut io, _) = prepared();
            match kind {
                0 => io.fail_read = Some(n),
                1 => io.fail_guard = Some(n),
                2 => io.fail_write = Some(n),
                _ => fixture::fail_write_after(&owned.ram, n),
            }
            assert!(run(&owned, &mut once, &mut io).is_err(), "{kind}/{n}");
            let r = once.report();
            assert_eq!(r.phase, Phase::Failed);
            assert_eq!((r.containment_sent, r.containment_done), (2, 2));
            assert!(!once.pools().0.table_setup_complete() && !once.pools().1.prepared());
            terminal(&owned, &mut once, &mut io);
        }
    }
}

#[test]
fn rxstart_each_pool_read_and_nonsticking_descriptor_stops_before_hardware_start() {
    // Two blank 256-entry rings, immediate descriptor reads and a second
    // complete descriptor pass. All occur before START or any RX doorbell.
    for n in 0..2048 + 2 * 1004 {
        let (owned, mut once, mut io, _) = prepared();
        fixture::fail_read_after(&owned.ram, n);
        assert_eq!(
            begin(&owned, &mut once, &mut io),
            Err(Error::Pool),
            "read{n}"
        );
        assert!(!once.report().start_sent);
        assert_eq!(io.acks, 0);
        terminal(&owned, &mut once, &mut io);
    }
    for n in 0..1004 {
        let (owned, mut once, mut io, _) = prepared();
        fixture::suppress_write_after(&owned.ram, n);
        let result = begin(&owned, &mut once, &mut io);
        // High address words are already zero in the fresh ring. Omitting
        // that zero store has no observable effect; other fields must fail.
        if n % 4 == 1 {
            assert_eq!(result, Ok(()));
        } else {
            assert_eq!(result, Err(Error::Readback), "store{n}");
            assert!(!once.report().start_sent);
            terminal(&owned, &mut once, &mut io);
        }
    }
}
#[test]
fn rxstart_corrupt_ring_data_masks_and_states_stop_before_start() {
    for fault in 0..10 {
        let (owned, mut once, mut io, r) = prepared();
        match fault {
            0 => fixture::write_word(&owned.ram, 0x8000, 1),
            1 => fixture::write_word(&owned.ram, 0x9ffc, 1),
            2 => r.0.borrow_mut()[0] ^= 1,
            3 => r.0.borrow_mut()[rx::DATA_RESERVED_BYTES / 4 - 1] ^= 1,
            4 => {
                io.values.insert(c::IEOB_STATUS, 2);
            }
            5 => {
                io.values.insert(g::TYPE_MASK, 0x44);
            }
            6 => {
                io.values.insert(c::IEOB_MASK, 3);
            }
            7 => {
                io.values.insert(0x1c100, 2 << 20);
            }
            8 => {
                io.ipa.insert(0x3034, 0x100);
            }
            _ => {
                io.ipa.insert(0x300c, 0x400c);
            }
        }
        assert!(begin(&owned, &mut once, &mut io).is_err(), "fault{fault}");
        assert!(!once.report().start_sent);
        assert_eq!(once.report().containment_sent, 2);
        assert_eq!(io.acks, 0);
        terminal(&owned, &mut once, &mut io);
    }
}
#[test]
fn rxstart_foreign_or_uncleared_start_sources_and_ack_failure_never_publish_buffers() {
    for fault in 0..8 {
        let (owned, mut once, mut io, _) = prepared();
        begin(&owned, &mut once, &mut io).unwrap();
        match fault {
            0 => {
                io.values.insert(g::CHANNEL_STATUS, 6);
            }
            1 => {
                io.values.insert(g::GLOBAL_STATUS, 1);
            }
            2 => {
                io.values.insert(c::IEOB_STATUS, 2);
            }
            3 => {
                io.values.insert(0x1c100, 1 << 20);
            }
            4 => {
                io.values.insert(g::GENERAL_STATUS, 4);
            }
            5 => {
                io.values.insert(g::EVENT_STATUS, 1);
            }
            6 => io.suppress_write = Some(5),
            _ => io.fail_ack = true,
        }
        assert!(once.notified(&mut io, 100).is_err(), "fault{fault}");
        assert_eq!(once.report().published_buffers, 0);
        terminal(&owned, &mut once, &mut io);
    }
}
#[test]
fn rxstart_legitimate_receive_arrival_stays_pending_without_receive_ack_or_premature_event_reads() {
    let (owned, mut once, mut io, _) = prepared();
    io.arrival = 1;
    run(&owned, &mut once, &mut io).unwrap();
    assert_eq!(once.report().phase, Phase::Armed);
    assert!(once.report().rx_pending);
    assert_eq!(once.report().rx_producer, Some(0x80008010));
    assert_eq!(io.value(c::IEOB_STATUS), 2);
    assert_eq!(io.acks, 1);
    assert!(once.pools().1.prepared());
    let (owned, mut once, mut io, _) = prepared();
    io.arrival = 2;
    assert!(run(&owned, &mut once, &mut io).is_err());
    assert_eq!(once.report().error, Some(Error::UnexpectedIrq));
    assert_eq!(io.acks, 1);
}
#[test]
fn rxstart_timeout_wrap_slow_preparation_and_late_fault_retain_both_pools() {
    for start in [100, u64::MAX - 50] {
        let (owned, mut once, mut io, _) = prepared();
        io.now = start;
        begin(&owned, &mut once, &mut io).unwrap();
        assert_eq!(once.timeout(&mut io, start.wrapping_add(999)), Ok(()));
        assert_eq!(
            once.timeout(&mut io, start.wrapping_add(1000)),
            Err(Error::Timeout)
        );
        assert_eq!(io.acks, 0);
        terminal(&owned, &mut once, &mut io);
    }
    let (owned, mut once, mut io, _) = prepared();
    io.guard_ticks = 100;
    begin(&owned, &mut once, &mut io).unwrap();
    assert!(io.now > 1000);
    let now = io.now;
    io.guard_ticks = 0;
    once.notified(&mut io, now).unwrap();
    assert_eq!(once.external_fault(&mut io), Err(Error::Guard));
    assert!(!once.pools().0.table_setup_complete() && !once.pools().1.prepared());
    terminal(&owned, &mut once, &mut io);
    let (owned, mut once, mut io, _) = prepared();
    begin(&owned, &mut once, &mut io).unwrap();
    io.fail_contain = true;
    assert_eq!(once.external_fault(&mut io), Err(Error::Guard));
    assert_eq!(once.report().containment_error, Some(Error::Io));
    assert_eq!(once.report().containment_sent, 2);
    assert_eq!(once.report().containment_done, 0);
}

#[test]
fn rxstart_original_pool_claim_and_entire_store_boundary_are_enforced() {
    let mut incomplete = fixture::memory_only_pool();
    assert!(incomplete.memory_setup_complete());
    assert!(!incomplete.table_setup_complete());
    assert_eq!(incomplete.claim_receive(), Err(MemoryError::Layout));
    assert_eq!(
        incomplete.receive_write32(0x9000, 1),
        Err(MemoryError::Bounds)
    );
    let mut owned = fixture::completed();
    let mut fresh = Pool::new(owned.ram.clone()).unwrap();
    assert_eq!(fresh.claim_receive(), Err(MemoryError::Layout));
    let mut pool = owned.pool.take().unwrap();
    let before = fixture::snapshot(&owned.ram);
    let writes = fixture::write_count(&owned.ram);
    assert_eq!(pool.receive_write32(0x9000, 1), Err(MemoryError::Bounds));
    assert_eq!(fixture::write_count(&owned.ram), writes);
    pool.claim_receive().unwrap();
    assert_eq!(pool.claim_receive(), Err(MemoryError::Layout));
    for offset in (0..sunfish_ipa_linux_buffers::POOL_BYTES).step_by(4) {
        let result = pool.receive_write32(offset, 0xdecafbad ^ offset as u32);
        if (0x9000..0xa000).contains(&offset) {
            assert_eq!(result, Ok(()));
        } else {
            assert_eq!(result, Err(MemoryError::Bounds), "{offset:x}");
        }
    }
    for offset in [0x9001, 0x9002, 0x9fff, 0xa000, usize::MAX] {
        assert_eq!(pool.receive_write32(offset, 1), Err(MemoryError::Bounds));
    }
    assert_eq!(fixture::write_count(&owned.ram) - writes, 1024);
    let after = fixture::snapshot(&owned.ram);
    for (index, (&old, &new)) in before.iter().zip(&after).enumerate() {
        if !(0x9000..0xa000).contains(&(index * 4)) {
            assert_eq!(old, new);
        }
    }
    fixture::fail_write_after(&owned.ram, 0);
    assert_eq!(pool.receive_write32(0x9000, 2), Err(MemoryError::Io));
    assert!(!pool.table_setup_complete());
    assert_eq!(pool.receive_write32(0x9000, 3), Err(MemoryError::Bounds));
    assert_eq!(pool.claim_receive(), Err(MemoryError::Layout));
}
