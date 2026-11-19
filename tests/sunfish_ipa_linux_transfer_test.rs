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
#[path = "../rust/src/sunfish_ipa_linux_table_transfer.rs"]
pub mod sunfish_ipa_linux_table_transfer;
#[path = "../rust/src/sunfish_ipa_linux_tables.rs"]
pub mod sunfish_ipa_linux_tables;
#[path = "../rust/src/sunfish_ipa_linux_transfer.rs"]
pub mod sunfish_ipa_linux_transfer;
mod owners {
    include!(env!("SUNFISH_TRANSFER_START_FIXTURE"));
}

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use sunfish_ipa_linux_buffers::{self as b, Memory, MemoryError, Pool};
use sunfish_ipa_linux_command as c;
use sunfish_ipa_linux_endpoint as ep;
use sunfish_ipa_linux_gsi as g;
use sunfish_ipa_linux_memory as m;
use sunfish_ipa_linux_memsetup::Plan;
use sunfish_ipa_linux_transfer::*;

#[derive(Default)]
struct RamState {
    words: Vec<u32>,
    reads: usize,
    writes: usize,
    fail_read: Option<usize>,
    fail_write: Option<usize>,
    suppress: Option<usize>,
    tear_event: bool,
}
#[derive(Clone)]
struct Ram(Rc<RefCell<RamState>>);
impl Memory for Ram {
    fn bytes(&self) -> usize {
        b::POOL_BYTES
    }
    fn read32(&mut self, offset: usize) -> Result<u32, MemoryError> {
        let mut s = self.0.borrow_mut();
        let n = s.reads;
        s.reads += 1;
        if s.fail_read == Some(n) {
            return Err(MemoryError::Io);
        }
        let value = s.words[offset / 4];
        if s.tear_event && (offset == 0x300c || offset == 0x301c) {
            s.words[offset / 4] ^= 0x1000000;
            s.tear_event = false;
        }
        Ok(value)
    }
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError> {
        let mut s = self.0.borrow_mut();
        let n = s.writes;
        s.writes += 1;
        if s.suppress != Some(n) {
            s.words[offset / 4] = value;
        }
        if s.fail_write == Some(n) {
            Err(MemoryError::Io)
        } else {
            Ok(())
        }
    }
}
fn pool() -> (Pool<Ram>, Ram) {
    let ram = Ram(Rc::new(RefCell::new(RamState {
        words: vec![0xa5a5a5a5; b::POOL_BYTES / 4],
        ..Default::default()
    })));
    let mut pool = Pool::new(ram.clone()).unwrap();
    assert!(pool.prepare(|| Ok(()), |_, _, _| {}).error.is_none());
    ram.0.borrow_mut().reads = 0;
    ram.0.borrow_mut().writes = 0;
    (pool, ram)
}
struct Fake {
    ram: Ram,
    values: BTreeMap<usize, u32>,
    shared: Vec<u32>,
    reads: usize,
    guards: usize,
    writes: Vec<(usize, u32)>,
    acks: usize,
    fail_read: Option<usize>,
    fail_guard: Option<usize>,
    fail_write: Option<usize>,
    suppress: Option<usize>,
    fail_ack: bool,
    fail_contain: bool,
    pending: bool,
    now: u64,
    guard_ticks: u64,
    table_mode: bool,
}
impl Fake {
    fn new(ram: Ram) -> Self {
        let mut values = BTreeMap::from([
            (g::GSI_STATUS, 1),
            (0x54, 0x415),
            (g::TYPE_MASK, 0x4c),
            (g::GLOBAL_ENABLE, 1),
            (0x1f120, 14),
            (c::IEOB_MASK, 1),
            (0x1d010, 0x80003000),
        ]);
        for e in ep::ENDPOINTS {
            values.insert(
                c::Read::Channel(e).offset(),
                if e.channel() == 1 { 2 << 20 } else { 1 << 20 },
            );
            values.insert(c::Read::Event(e).offset(), 1 << 20);
        }
        let mut shared = vec![0xa5a5a5a5; m::SHARED_BYTES / 4];
        for w in m::words() {
            shared[w.offset() / 4] = m::CANARY_VALUE;
        }
        Self {
            ram,
            values,
            shared,
            reads: 0,
            guards: 0,
            writes: vec![],
            acks: 0,
            fail_read: None,
            fail_guard: None,
            fail_write: None,
            suppress: None,
            fail_ack: false,
            fail_contain: false,
            pending: false,
            now: 100,
            guard_ticks: 0,
            table_mode: false,
        }
    }
    fn value(&self, o: usize) -> u32 {
        self.values.get(&o).copied().unwrap_or(0)
    }
    fn complete(&mut self) {
        if !self.pending {
            return;
        }
        self.pending = false;
        if self.table_mode {
            let p = sunfish_ipa_linux_tables::Plan::new(0x415).unwrap();
            for c in p.commands() {
                for offset in (c.local_offset()..c.local_offset() + c.size()).step_by(4) {
                    self.shared[offset / 4] = p.expected32(offset).unwrap();
                }
            }
            let mut bytes = [0; 16];
            bytes[..8].copy_from_slice(&p.commands()[5].tre_iova().to_le_bytes());
            bytes[11] = 1;
            bytes[14] = 0x23;
            bytes[15] = 1;
            let mut ram = self.ram.0.borrow_mut();
            for i in 0..4 {
                ram.words[0x3010 / 4 + i] =
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
            }
            self.values.insert(0x1d010, p.first_unfilled_event());
            self.values.insert(c::IEOB_STATUS, 1);
            return;
        }
        let p = Plan::new(0x415).unwrap();
        for c in p.commands() {
            self.shared[c.local_offset() / 4..(c.local_offset() + c.size()) / 4].fill(0);
        }
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&p.commands()[3].tre_iova().to_le_bytes());
        bytes[11] = 1;
        bytes[14] = 2;
        bytes[15] = 1;
        let mut ram = self.ram.0.borrow_mut();
        for i in 0..4 {
            ram.words[0x3000 / 4 + i] =
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        }
        self.values.insert(0x1d010, p.first_unfilled_event());
        self.values.insert(c::IEOB_STATUS, 1);
    }
    fn counts(&self) -> (usize, usize, usize, usize, usize, usize) {
        let ram = self.ram.0.borrow();
        (
            self.reads,
            self.guards,
            self.writes.len(),
            self.acks,
            ram.reads,
            ram.writes,
        )
    }
}
impl Io<Ram> for Fake {
    fn ticks(&mut self) -> u64 {
        self.now
    }
    fn guard(&mut self, pool: &Pool<Ram>) -> Result<(), Error> {
        let n = self.guards;
        self.guards += 1;
        self.now = self.now.wrapping_add(self.guard_ticks);
        if self.fail_guard == Some(n) || pool.buffer(b::Kind::Zero).is_none() {
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
        match r {
            Read::SharedMemorySize => Ok(self.value(0x54)),
            Read::Shared(o) => Ok(self.shared[o / 4]),
            Read::Gsi(c::Read::TypeStatus) => {
                let mut v = self.value(g::TYPE_STATUS);
                for (s, mask, t) in [
                    (g::CHANNEL_STATUS, g::CHANNEL_MASK, 1),
                    (g::EVENT_STATUS, g::EVENT_MASK, 2),
                    (g::GLOBAL_STATUS, g::GLOBAL_ENABLE, 4),
                    (c::IEOB_STATUS, c::IEOB_MASK, 8),
                    (g::GENERAL_STATUS, 0x1f120, 64),
                ] {
                    if self.value(s) & self.value(mask) != 0 {
                        v |= t;
                    }
                }
                Ok(v)
            }
            _ => Ok(self.value(r.offset())),
        }
    }
    fn write(&mut self, r: &Request) -> Result<(), Error> {
        let n = self.writes.len();
        let o = r.register().offset();
        let v = r.value();
        self.writes.push((o, v));
        if self.suppress != Some(n) {
            match r.register() {
                Register::ChannelDoorbell => {
                    assert_eq!(
                        v,
                        if self.table_mode {
                            0x800040a0
                        } else {
                            0x80004040
                        }
                    );
                    assert!(!self.pending);
                    self.pending = true;
                }
                Register::LocalContext if self.table_mode => panic!("tables cannot write context"),
                Register::IeobClear => {
                    self.values
                        .insert(c::IEOB_STATUS, self.value(c::IEOB_STATUS) & !v);
                }
                _ => {
                    self.values.insert(o, v);
                }
            }
        }
        if self.fail_write == Some(n) || (self.fail_contain && o == g::TYPE_MASK && v == 0) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn acknowledge(&mut self) -> Result<(), Error> {
        self.acks += 1;
        if self.fail_ack {
            Err(Error::Acknowledge)
        } else {
            Ok(())
        }
    }
}
type Owners = (g::SetupOnce, ep::ConfigureOnce, c::StartOnce);
fn run(
    once: &mut TransferOnce,
    own: &Owners,
    pool: &mut Pool<Ram>,
    io: &mut Fake,
) -> Result<(), Error> {
    once.begin(&own.0, &own.1, &own.2, pool, io)?;
    io.complete();
    once.notified(pool, io, io.now)
}
fn terminal(once: &mut TransferOnce, own: &Owners, pool: &mut Pool<Ram>, io: &mut Fake) {
    let before = io.counts();
    assert_eq!(
        once.begin(&own.0, &own.1, &own.2, pool, io),
        Err(Error::Consumed)
    );
    assert_eq!(once.notified(pool, io, 101), Err(Error::Consumed));
    assert_eq!(once.timeout(pool, io, 101), Err(Error::Consumed));
    assert_eq!(once.external_fault(pool, io), Err(Error::Consumed));
    assert_eq!(io.counts(), before);
}
#[test]
fn dma_full_memory_transaction_and_ordered_completion() {
    let own = owners::started();
    let (mut pool, ram) = pool();
    let mut io = Fake::new(ram);
    let mut once = TransferOnce::new(1000).unwrap();
    run(&mut once, &own, &mut pool, &mut io).unwrap();
    let r = once.report();
    assert_eq!(r.phase, Phase::Complete);
    assert_eq!((r.cpu_sent, r.cpu_done, r.cpu_verified), (48, 48, 48));
    assert_eq!((r.writes_sent, r.writes_done), (8, 8));
    assert!(r.published && r.ack_done && !r.containment_sent);
    assert_eq!(
        (
            r.zero_source_words,
            r.before_nonzero,
            r.target_words,
            r.canary_checks,
            r.final_samples
        ),
        (2048, 1619, 1619, 50, 2)
    );
    assert_eq!(r.event.unwrap().transfer, 0x80004030);
    assert_eq!(io.acks, 1);
    assert!(pool.memory_setup_complete());
    assert_eq!(
        io.writes,
        [
            (0x1e008, 0x80004040),
            (0x1f088, 0x44),
            (0x1f0b8, 0),
            (0x1f0c0, 1),
            (0x1e100, 0x80003000),
            (0x1f0b8, 1),
            (0x1f088, 0x4c),
            (0x1e8, 0x5f0)
        ]
    );
    let expected = std::fs::read_to_string(env!("SUNFISH_TRANSFER_C_REFERENCE")).unwrap();
    assert_eq!(
        io.writes
            .iter()
            .map(|(o, v)| format!("{o:05x} {v:08x}\n"))
            .collect::<String>(),
        expected
    );
    terminal(&mut once, &own, &mut pool, &mut io);
    let mut second = TransferOnce::new(1000).unwrap();
    let before = io.counts();
    assert_eq!(
        second.begin(&own.0, &own.1, &own.2, &mut pool, &mut io),
        Err(Error::Prerequisite)
    );
    assert_eq!(io.counts(), before);
}
#[test]
fn dma_prerequisites_and_exclusive_pool_claim_precede_io() {
    assert!(matches!(TransferOnce::new(0), Err(Error::Layout)));
    assert!(matches!(TransferOnce::new(1 << 63), Err(Error::Layout)));
    let own = owners::started();
    let (mut pool, ram) = pool();
    let mut io = Fake::new(ram);
    let mut once = TransferOnce::new(1000).unwrap();
    assert_eq!(
        once.begin(
            &own.0,
            &own.1,
            &c::StartOnce::new(50).unwrap(),
            &mut pool,
            &mut io
        ),
        Err(Error::Prerequisite)
    );
    assert_eq!(io.counts(), (0, 0, 0, 0, 0, 0));
    terminal(&mut once, &own, &mut pool, &mut io);
    let mut first = TransferOnce::new(1000).unwrap();
    first
        .begin(&own.0, &own.1, &own.2, &mut pool, &mut io)
        .unwrap();
    let before = io.counts();
    let mut second = TransferOnce::new(1000).unwrap();
    assert_eq!(
        second.begin(&own.0, &own.1, &own.2, &mut pool, &mut io),
        Err(Error::Prerequisite)
    );
    assert_eq!(io.counts(), before);
    first.external_fault(&mut pool, &mut io).unwrap_err();
    assert!(!pool.memory_setup_complete());
    assert!(pool.buffer(b::Kind::Zero).is_none());
}
#[test]
fn dma_every_mmio_read_guard_and_uncertain_register_store_is_terminal() {
    let own = owners::started();
    let (mut good_pool, ram) = pool();
    let mut good = Fake::new(ram);
    let mut once = TransferOnce::new(1000).unwrap();
    run(&mut once, &own, &mut good_pool, &mut good).unwrap();
    let counts = good.counts();
    for kind in 0..3 {
        for n in 0..[counts.0, counts.1, counts.2][kind] {
            let (mut pool, ram) = pool();
            let mut io = Fake::new(ram);
            let mut once = TransferOnce::new(1000).unwrap();
            match kind {
                0 => io.fail_read = Some(n),
                1 => io.fail_guard = Some(n),
                _ => io.fail_write = Some(n),
            }
            assert!(
                run(&mut once, &own, &mut pool, &mut io).is_err(),
                "{kind}/{n}"
            );
            assert_eq!(once.report().phase, Phase::Failed);
            assert!(once.report().containment_sent);
            assert_eq!(io.writes.last(), Some(&(g::TYPE_MASK, 0)));
            assert!(pool.buffer(b::Kind::Zero).is_none());
            terminal(&mut once, &own, &mut pool, &mut io);
        }
    }
}
#[test]
fn dma_cpu_failures_and_nonsticking_descriptor_stores_prevent_replay() {
    let own = owners::started();
    let (mut good_pool, ram) = pool();
    let mut good = Fake::new(ram);
    let mut once = TransferOnce::new(1000).unwrap();
    run(&mut once, &own, &mut good_pool, &mut good).unwrap();
    let reads = good.ram.0.borrow().reads;
    for kind in 0..3 {
        for n in 0..if kind == 0 { reads } else { 48 } {
            let (mut pool, ram) = pool();
            let mut io = Fake::new(ram);
            let mut once = TransferOnce::new(1000).unwrap();
            match kind {
                0 => io.ram.0.borrow_mut().fail_read = Some(n),
                1 => io.ram.0.borrow_mut().fail_write = Some(n),
                _ => io.ram.0.borrow_mut().suppress = Some(n),
            }
            let result = run(&mut once, &own, &mut pool, &mut io);
            // A suppressed zero store is indistinguishable from the correct
            // initial zero value. All nonzero fields must be detected.
            if kind == 2 && result.is_ok() {
                continue;
            }
            assert!(result.is_err(), "{kind}/{n}");
            assert!(once.report().containment_sent);
            assert!(pool.buffer(b::Kind::Zero).is_none());
            terminal(&mut once, &own, &mut pool, &mut io);
        }
    }
}
#[test]
fn dma_bad_initial_memory_registers_and_sources_never_publish() {
    let own = owners::started();
    for fault in 0..12 {
        let (mut pool, ram) = pool();
        let mut io = Fake::new(ram);
        let mut once = TransferOnce::new(1000).unwrap();
        match fault {
            0 => io.ram.0.borrow_mut().words[0] = 1,
            1 => io.ram.0.borrow_mut().words[0x3000 / 4] = 1,
            2 => io.shared[m::words()[0].offset() / 4] = 0,
            3 => {
                io.values.insert(0x1d010, 0x80003010);
            }
            4 => {
                io.values.insert(g::CHANNEL_STATUS, 4);
            }
            5 => {
                io.values.insert(c::IEOB_STATUS, 2);
            }
            6 => {
                io.values.insert(c::IEOB_MASK, 0);
            }
            7 => {
                io.values.insert(g::TYPE_MASK, 0x44);
            }
            8 => {
                io.values
                    .insert(c::Read::Channel(ep::Endpoint::LanRx).offset(), 2 << 20);
            }
            9 => {
                io.values
                    .insert(c::Read::Event(ep::Endpoint::CommandTx).offset(), 2 << 20);
            }
            10 => {
                io.values.insert(g::GENERAL_STATUS, 1);
            }
            _ => {
                io.values.insert(0x54, 0x416);
            }
        }
        assert!(
            once.begin(&own.0, &own.1, &own.2, &mut pool, &mut io)
                .is_err(),
            "fault{fault}"
        );
        assert!(!once.report().published);
        assert_eq!(io.writes, [(g::TYPE_MASK, 0)]);
        assert_eq!(io.acks, 0);
    }
}
#[test]
fn dma_foreign_sources_wrong_events_torn_reads_and_bad_targets_stay_retained() {
    let own = owners::started();
    for fault in 0..15 {
        let (mut pool, ram) = pool();
        let mut io = Fake::new(ram);
        let mut once = TransferOnce::new(1000).unwrap();
        once.begin(&own.0, &own.1, &own.2, &mut pool, &mut io)
            .unwrap();
        io.complete();
        match fault {
            0 => {
                io.values.insert(g::TYPE_STATUS, 1);
            }
            1 => {
                io.values.insert(g::CHANNEL_STATUS, 4);
            }
            2 => {
                io.values.insert(g::EVENT_STATUS, 4);
            }
            3 => {
                io.values.insert(g::GLOBAL_STATUS, 4);
            }
            4 => {
                io.values.insert(g::GENERAL_STATUS, 4);
            }
            5 => {
                io.values.insert(c::IEOB_STATUS, 3);
            }
            6 => {
                io.values.insert(0x1d010, 0x80003020);
            }
            7 => io.ram.0.borrow_mut().words[0x3000 / 4] = 0x80004020,
            8 => io.ram.0.borrow_mut().words[0x3004 / 4] = 1,
            9 => io.ram.0.borrow_mut().words[0x300c / 4] ^= 0x03000000,
            10 => io.ram.0.borrow_mut().tear_event = true,
            11 => io.shared[0x4a8 / 4] = 1,
            12 => io.shared[m::words()[0].offset() / 4] = 0,
            13 => {
                io.values
                    .insert(c::Read::Channel(ep::Endpoint::CommandTx).offset(), 1 << 20);
            }
            _ => {
                io.values.insert(g::TYPE_MASK, 0);
            }
        }
        assert!(
            once.notified(&mut pool, &mut io, 100).is_err(),
            "fault{fault}"
        );
        assert_eq!(io.acks, 0);
        assert_eq!(once.report().phase, Phase::Failed);
        assert!(!pool.memory_setup_complete());
        terminal(&mut once, &own, &mut pool, &mut io);
    }
}
#[test]
fn dma_missing_completions_timeout_wrap_ack_and_containment_failures() {
    let own = owners::started();
    for start in [100, u64::MAX - 50] {
        let (mut pool, ram) = pool();
        let mut io = Fake::new(ram);
        io.now = start;
        let mut once = TransferOnce::new(1000).unwrap();
        once.begin(&own.0, &own.1, &own.2, &mut pool, &mut io)
            .unwrap();
        assert!(
            once.timeout(&mut pool, &mut io, start.wrapping_add(999))
                .is_ok()
        );
        assert_eq!(
            once.timeout(&mut pool, &mut io, start.wrapping_add(1000)),
            Err(Error::Timeout)
        );
        assert_eq!(io.acks, 0);
        terminal(&mut once, &own, &mut pool, &mut io);
    }
    for ack_failure in [false, true] {
        let (mut pool, ram) = pool();
        let mut io = Fake::new(ram);
        let mut once = TransferOnce::new(1000).unwrap();
        io.fail_ack = ack_failure;
        io.fail_contain = !ack_failure;
        once.begin(&own.0, &own.1, &own.2, &mut pool, &mut io)
            .unwrap();
        if ack_failure {
            io.complete();
            assert_eq!(
                once.notified(&mut pool, &mut io, 100),
                Err(Error::Acknowledge)
            );
        } else {
            assert_eq!(once.external_fault(&mut pool, &mut io), Err(Error::Guard));
            assert_eq!(once.report().containment_error, Some(Error::Io));
        }
        terminal(&mut once, &own, &mut pool, &mut io);
    }
}
#[test]
fn dma_slow_preparation_does_not_spend_the_publication_timeout() {
    let own = owners::started();
    let (mut pool, ram) = pool();
    let mut io = Fake::new(ram);
    io.guard_ticks = 100;
    let mut once = TransferOnce::new(1000).unwrap();
    once.begin(&own.0, &own.1, &own.2, &mut pool, &mut io)
        .unwrap();
    assert!(io.now > 1000);
    let published = io.now;
    io.guard_ticks = 0;
    io.complete();
    once.notified(&mut pool, &mut io, published).unwrap();
}
#[test]
fn dma_pool_write_boundary_never_allows_zero_event_table_or_other_rings() {
    let (mut pool, _) = pool();
    pool.claim_memory_setup().unwrap();
    for offset in (0..b::POOL_BYTES).step_by(4) {
        let allowed = (0x6000..0x6080).contains(&offset) || (0x4000..0x4040).contains(&offset);
        assert_eq!(
            pool.memory_setup_write32(offset, 0).is_ok(),
            allowed,
            "{offset:x}"
        );
    }
    pool.finish_memory_setup().unwrap();
    assert!(pool.memory_setup_write32(0x6000, 0).is_err());
    assert!(pool.claim_memory_setup().is_err());
}

#[test]
fn dma_late_irq_invalidates_completion_once_without_reuse_or_ack() {
    let own = owners::started();
    let (mut pool, ram) = pool();
    let mut io = Fake::new(ram);
    let mut once = TransferOnce::new(1000).unwrap();
    run(&mut once, &own, &mut pool, &mut io).unwrap();
    let before = io.counts();
    assert_eq!(
        once.late_fault(&mut pool, &mut io),
        Err(Error::UnexpectedIrq)
    );
    assert_eq!(once.report().phase, Phase::Failed);
    assert!(!pool.memory_setup_complete());
    assert_eq!(io.acks, before.3);
    assert_eq!(io.writes.len(), before.2 + 1);
    let contained = io.counts();
    assert_eq!(once.late_fault(&mut pool, &mut io), Err(Error::Consumed));
    assert_eq!(io.counts(), contained);
}

use sunfish_ipa_linux_table_transfer::TableOnce;
fn table_fixture(own: &Owners) -> (TransferOnce, Pool<Ram>, Fake) {
    let (mut pool, ram) = pool();
    let mut io = Fake::new(ram);
    let mut memory = TransferOnce::new(1000).unwrap();
    run(&mut memory, own, &mut pool, &mut io).unwrap();
    io.table_mode = true;
    io.reads = 0;
    io.guards = 0;
    io.writes.clear();
    io.acks = 0;
    io.ram.0.borrow_mut().reads = 0;
    io.ram.0.borrow_mut().writes = 0;
    (memory, pool, io)
}
fn run_table(
    once: &mut TableOnce,
    own: &Owners,
    memory: &TransferOnce,
    pool: &mut Pool<Ram>,
    io: &mut Fake,
) -> Result<(), Error> {
    once.begin(&own.0, &own.1, &own.2, memory, pool, io)?;
    io.complete();
    once.notified(pool, io, io.now)
}
fn table_terminal(
    once: &mut TableOnce,
    own: &Owners,
    memory: &TransferOnce,
    pool: &mut Pool<Ram>,
    io: &mut Fake,
) {
    let before = io.counts();
    assert_eq!(
        once.begin(&own.0, &own.1, &own.2, memory, pool, io),
        Err(Error::Consumed)
    );
    assert_eq!(once.notified(pool, io, 101), Err(Error::Consumed));
    assert_eq!(once.timeout(pool, io, 101), Err(Error::Consumed));
    assert_eq!(once.external_fault(pool, io), Err(Error::Consumed));
    assert_eq!(io.counts(), before);
}
#[test]
fn dma_tables_continue_the_actual_memory_owner_without_reusing_slots() {
    let own = owners::started();
    let (memory, mut pool, mut io) = table_fixture(&own);
    let before = io.ram.0.borrow().words.clone();
    let mut once = TableOnce::new(1000).unwrap();
    run_table(&mut once, &own, &memory, &mut pool, &mut io).unwrap();
    let r = once.report();
    assert_eq!(r.phase, Phase::Complete);
    assert_eq!((r.cpu_sent, r.cpu_done, r.cpu_verified), (72, 72, 72));
    assert_eq!((r.writes_sent, r.writes_done), (7, 7));
    assert_eq!(
        (
            r.zero_source_words,
            r.table_source_words,
            r.target_words,
            r.canary_checks,
            r.final_samples
        ),
        (2048, 34, 120, 50, 2)
    );
    assert_eq!(r.event_pointer, Some(0x80003020));
    assert_eq!(r.event.unwrap().transfer, 0x80004090);
    assert_eq!(io.acks, 1);
    assert!(pool.memory_setup_complete() && pool.table_setup_complete());
    let expected = std::fs::read_to_string(env!("SUNFISH_TABLE_TRANSFER_C_REFERENCE")).unwrap();
    assert_eq!(
        io.writes
            .iter()
            .map(|(o, v)| format!("{o:05x} {v:08x}\n"))
            .collect::<String>(),
        expected
    );
    for (index, (old, new)) in before
        .iter()
        .zip(io.ram.0.borrow().words.iter())
        .enumerate()
    {
        let offset = index * 4;
        let may_change = (0x6080..0x6140).contains(&offset)
            || (0x4040..0x40a0).contains(&offset)
            || (0x3010..0x3020).contains(&offset);
        if !may_change {
            assert_eq!(old, new, "unowned pool word {offset:x}");
        }
    }
    table_terminal(&mut once, &own, &memory, &mut pool, &mut io);
    let mut second = TableOnce::new(1000).unwrap();
    let before = io.counts();
    assert_eq!(
        second.begin(&own.0, &own.1, &own.2, &memory, &mut pool, &mut io),
        Err(Error::Prerequisite)
    );
    assert_eq!(io.counts(), before);
}
#[test]
fn dma_tables_require_actual_completed_memory_and_exclusive_original_pool() {
    assert!(matches!(TableOnce::new(0), Err(Error::Layout)));
    let own = owners::started();
    for fault in 0..4 {
        let (mut memory, mut pool, mut io) = table_fixture(&own);
        match fault {
            0 => memory = TransferOnce::new(1000).unwrap(),
            1 => {
                let (fresh, _) = self::pool();
                pool = fresh;
            }
            2 => pool.quarantine(),
            _ => {
                memory.late_fault(&mut pool, &mut io).unwrap_err();
            }
        }
        let before = io.counts();
        let mut once = TableOnce::new(1000).unwrap();
        assert_eq!(
            once.begin(&own.0, &own.1, &own.2, &memory, &mut pool, &mut io),
            Err(Error::Prerequisite)
        );
        assert_eq!(io.counts(), before);
    }
    let (memory, mut pool, mut io) = table_fixture(&own);
    let mut first = TableOnce::new(1000).unwrap();
    first
        .begin(&own.0, &own.1, &own.2, &memory, &mut pool, &mut io)
        .unwrap();
    let before = io.counts();
    let mut second = TableOnce::new(1000).unwrap();
    assert_eq!(
        second.begin(&own.0, &own.1, &own.2, &memory, &mut pool, &mut io),
        Err(Error::Prerequisite)
    );
    assert_eq!(io.counts(), before);
}
#[test]
fn dma_tables_every_read_guard_uncertain_mmio_and_cpu_failure_is_terminal() {
    let own = owners::started();
    let (memory, mut pool, mut io) = table_fixture(&own);
    let mut once = TableOnce::new(1000).unwrap();
    run_table(&mut once, &own, &memory, &mut pool, &mut io).unwrap();
    let counts = io.counts();
    for kind in 0..6 {
        let limit = [counts.0, counts.1, counts.2, counts.4, counts.5, counts.5][kind];
        for n in 0..limit {
            let (memory, mut pool, mut io) = table_fixture(&own);
            let mut once = TableOnce::new(1000).unwrap();
            match kind {
                0 => io.fail_read = Some(n),
                1 => io.fail_guard = Some(n),
                2 => io.fail_write = Some(n),
                3 => io.ram.0.borrow_mut().fail_read = Some(n),
                4 => io.ram.0.borrow_mut().fail_write = Some(n),
                _ => io.ram.0.borrow_mut().suppress = Some(n),
            }
            let result = run_table(&mut once, &own, &memory, &mut pool, &mut io);
            // Suppressing a zero store into an already-zero fresh slot has
            // no observable effect; every nonzero field must be checked.
            if kind == 5 && result.is_ok() {
                continue;
            }
            assert!(result.is_err(), "{kind}/{n}");
            assert_eq!(once.report().phase, Phase::Failed);
            assert!(once.report().containment_sent);
            assert_eq!(io.writes.last(), Some(&(g::TYPE_MASK, 0)));
            assert!(!pool.memory_setup_complete() && !pool.table_setup_complete());
            table_terminal(&mut once, &own, &memory, &mut pool, &mut io);
        }
    }
}
#[test]
fn dma_tables_bad_sources_initial_producer_context_and_pending_status_never_publish() {
    let own = owners::started();
    for fault in 0..9 {
        let (memory, mut pool, mut io) = table_fixture(&own);
        match fault {
            0 => io.ram.0.borrow_mut().words[0] = 1,
            1 => io.ram.0.borrow_mut().words[0x2008 / 4] ^= 1,
            2 => io.ram.0.borrow_mut().words[0x2010 / 4] ^= 1,
            3 => io.ram.0.borrow_mut().words[0x3010 / 4] = 1,
            4 => {
                io.values.insert(0x1d010, 0x80003000);
            }
            5 => {
                io.values.insert(c::IEOB_STATUS, 1);
            }
            6 => {
                io.values.insert(c::IEOB_MASK, 0);
            }
            7 => io.shared[m::words()[0].offset() / 4] = 0,
            _ => {
                io.values.insert(0x1e8, 0);
            }
        }
        let mut once = TableOnce::new(1000).unwrap();
        assert!(
            once.begin(&own.0, &own.1, &own.2, &memory, &mut pool, &mut io)
                .is_err(),
            "fault{fault}"
        );
        assert!(!once.report().published);
        assert_eq!(io.writes, [(g::TYPE_MASK, 0)]);
        assert_eq!(io.acks, 0);
    }
}
#[test]
fn dma_tables_reject_stale_foreign_torn_extra_and_wrong_table_completions() {
    let own = owners::started();
    for fault in 0..13 {
        let (memory, mut pool, mut io) = table_fixture(&own);
        let mut once = TableOnce::new(1000).unwrap();
        once.begin(&own.0, &own.1, &own.2, &memory, &mut pool, &mut io)
            .unwrap();
        io.complete();
        match fault {
            0 => {
                io.values.insert(0x1d010, 0x80003010);
            }
            1 => {
                io.values.insert(0x1d010, 0x80003030);
            }
            2 => io.ram.0.borrow_mut().words[0x3010 / 4] = 0x80004030,
            3 => io.ram.0.borrow_mut().words[0x3014 / 4] = 1,
            4 => io.ram.0.borrow_mut().words[0x301c / 4] ^= 0x03000000,
            5 => io.ram.0.borrow_mut().tear_event = true,
            6 => io.shared[0x3a0 / 4] ^= 4,
            7 => io.shared[0x290 / 4] ^= 1,
            8 => io.shared[0x2a8 / 4] = 1,
            9 => io.shared[m::words()[0].offset() / 4] = 0,
            10 => {
                io.values.insert(c::IEOB_STATUS, 3);
            }
            11 => {
                io.values.insert(g::GLOBAL_STATUS, 1);
            }
            _ => {
                io.values.insert(0x1e8, 0);
            }
        }
        assert!(
            once.notified(&mut pool, &mut io, 100).is_err(),
            "fault{fault}"
        );
        assert_eq!(io.acks, 0);
        table_terminal(&mut once, &own, &memory, &mut pool, &mut io);
    }
}
#[test]
fn dma_tables_timeout_ack_failure_late_irq_and_containment_keep_pool_quarantined() {
    let own = owners::started();
    for fault in 0..5 {
        let (memory, mut pool, mut io) = table_fixture(&own);
        let mut once = TableOnce::new(1000).unwrap();
        io.now = u64::MAX - 50;
        once.begin(&own.0, &own.1, &own.2, &memory, &mut pool, &mut io)
            .unwrap();
        match fault {
            0 => {
                let now = io.now;
                once.timeout(&mut pool, &mut io, now.wrapping_add(999))
                    .unwrap();
                assert_eq!(
                    once.timeout(&mut pool, &mut io, now.wrapping_add(1000)),
                    Err(Error::Timeout)
                );
            }
            1 => {
                io.complete();
                io.fail_ack = true;
                let now = io.now;
                assert_eq!(
                    once.notified(&mut pool, &mut io, now),
                    Err(Error::Acknowledge)
                );
            }
            2 => {
                io.fail_contain = true;
                assert_eq!(once.external_fault(&mut pool, &mut io), Err(Error::Guard));
                assert_eq!(once.report().containment_error, Some(Error::Io));
            }
            3 => {
                io.complete();
                let now = io.now;
                once.notified(&mut pool, &mut io, now).unwrap();
                assert_eq!(
                    once.late_fault(&mut pool, &mut io),
                    Err(Error::UnexpectedIrq)
                );
                assert_eq!(io.acks, 1);
            }
            _ => {
                let now = io.now;
                assert_eq!(
                    once.notified(&mut pool, &mut io, now.wrapping_add(1000)),
                    Err(Error::Timeout)
                );
            }
        }
        assert!(!pool.table_setup_complete());
        table_terminal(&mut once, &own, &memory, &mut pool, &mut io);
    }
}
#[test]
fn dma_tables_pool_boundary_only_allows_the_six_fresh_payload_and_tre_slots() {
    let own = owners::started();
    let (_memory, mut pool, _) = table_fixture(&own);
    pool.claim_table_setup().unwrap();
    for offset in (0..b::POOL_BYTES).step_by(4) {
        let allowed = (0x6080..0x6140).contains(&offset) || (0x4040..0x40a0).contains(&offset);
        assert_eq!(
            pool.table_setup_write32(offset, 0).is_ok(),
            allowed,
            "{offset:x}"
        );
    }
    pool.finish_table_setup().unwrap();
    assert!(pool.table_setup_write32(0x6080, 0).is_err());
    assert!(pool.claim_table_setup().is_err());
}
