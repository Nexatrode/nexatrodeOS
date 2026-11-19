#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
pub mod sunfish_ipa_linux_gsi;

use std::collections::{BTreeMap, BTreeSet};
use sunfish_ipa_linux_buffers::{Memory, MemoryError, POOL_BYTES, Pool};
use sunfish_ipa_linux_gsi::*;

struct Ram(Vec<u32>);
impl Memory for Ram {
    fn bytes(&self) -> usize {
        self.0.len() * 4
    }
    fn read32(&mut self, offset: usize) -> Result<u32, MemoryError> {
        Ok(self.0[offset / 4])
    }
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError> {
        self.0[offset / 4] = value;
        Ok(())
    }
}
fn pool() -> Pool<Ram> {
    let mut pool = Pool::new(Ram(vec![0xa5a5a5a5; POOL_BYTES / 4])).unwrap();
    assert_eq!(pool.prepare(|| Ok(()), |_, _, _| {}).error, None);
    pool
}
fn limits() -> Limits {
    Limits::decode(0xe4046b59).unwrap()
}
fn context(op: Allocation) -> usize {
    match op {
        Allocation::Event(id) => 0x1d000 + usize::from(id) * 0x80,
        Allocation::Channel(id) => 0x1c000 + usize::from(id) * 0x80,
    }
}
fn source(op: Allocation) -> usize {
    match op {
        Allocation::Event(_) => EVENT_STATUS,
        Allocation::Channel(_) => CHANNEL_STATUS,
    }
}
fn kind(op: Allocation) -> u32 {
    match op {
        Allocation::Event(_) => 2,
        Allocation::Channel(_) => 1,
    }
}
fn id(op: Allocation) -> u8 {
    match op {
        Allocation::Event(id) | Allocation::Channel(id) => id,
    }
}

struct Fake {
    registers: BTreeMap<usize, u32>,
    writes: Vec<Word>,
    reads: usize,
    ticks: u64,
    guard_ticks: u64,
    guards: usize,
    acks: usize,
    delivered: bool,
    extra_type: u32,
    pending_command: Option<Allocation>,
    pending_modem: Option<u8>,
    modem_result: u8,
    binds: usize,
    fail_bind: bool,
    fail_read: Option<usize>,
    fail_guard: Option<usize>,
    fail_write: BTreeSet<usize>,
    suppress_write: Option<usize>,
    fail_ack: bool,
    pending_after_read: Option<(usize, usize, u32)>,
    pending_after_write: Option<(usize, usize, u32)>,
}
impl Fake {
    fn new() -> Self {
        Self {
            registers: BTreeMap::from([(TYPE_MASK, BASE_IRQ_TYPES)]),
            writes: vec![],
            reads: 0,
            ticks: 100,
            guard_ticks: 0,
            guards: 0,
            acks: 0,
            delivered: false,
            extra_type: 0,
            pending_command: None,
            pending_modem: None,
            modem_result: 1,
            binds: 0,
            fail_bind: false,
            fail_read: None,
            fail_guard: None,
            fail_write: BTreeSet::new(),
            suppress_write: None,
            fail_ack: false,
            pending_after_read: None,
            pending_after_write: None,
        }
    }
    fn value(&self, offset: usize) -> u32 {
        self.registers.get(&offset).copied().unwrap_or(0)
    }
    fn inject(&mut self, offset: usize, value: u32) {
        if offset == TYPE_STATUS {
            self.extra_type = value;
        } else {
            self.registers.insert(offset, value);
        }
    }
    fn complete(&mut self) {
        if self.pending_modem.take().is_some() {
            self.registers.insert(GLOBAL_STATUS, 2);
            self.registers.insert(
                GENERIC_SCRATCH,
                (self.value(GENERIC_SCRATCH) & !(7 << 5)) | (u32::from(self.modem_result) << 5),
            );
            self.delivered = true;
            return;
        }
        let op = self
            .pending_command
            .take()
            .expect("no allocation submitted");
        self.registers.insert(context(op), 1 << 20);
        self.registers.insert(source(op), 1 << id(op));
        self.delivered = true;
    }
    fn counts(&self) -> (usize, usize, usize, usize) {
        (self.reads, self.writes.len(), self.guards, self.acks)
    }
    fn cold() -> Self {
        let mut io = Self::new();
        io.registers
            .extend([(TYPE_MASK, 0), (GSI_STATUS, 1), (HW_PARAM_2, 0xe4046b59)]);
        io
    }
}
impl Io for Fake {
    fn ticks(&mut self) -> u64 {
        self.ticks
    }
    fn guard(&mut self) -> Result<(), Error> {
        self.ticks = self.ticks.wrapping_add(self.guard_ticks);
        let n = self.guards;
        self.guards += 1;
        if self.fail_guard == Some(n) {
            Err(Error::Guard)
        } else {
            Ok(())
        }
    }
    fn read(&mut self, offset: usize) -> Result<u32, Error> {
        let n = self.reads;
        self.reads += 1;
        if self.fail_read == Some(n) {
            return Err(Error::Io);
        }
        let value = if offset == TYPE_STATUS {
            let mut pending = self.extra_type;
            if self.value(CHANNEL_STATUS) & self.value(CHANNEL_MASK) != 0 {
                pending |= 1;
            }
            if self.value(EVENT_STATUS) & self.value(EVENT_MASK) != 0 {
                pending |= 2;
            }
            if self.value(GLOBAL_STATUS) & self.value(GLOBAL_ENABLE) != 0 {
                pending |= 4;
            }
            if self.value(GENERAL_STATUS) & self.value(0x1f120) != 0 {
                pending |= 64;
            }
            pending
        } else {
            self.value(offset)
        };
        if let Some((trigger, target, value)) = self.pending_after_read {
            if trigger == offset {
                self.pending_after_read = None;
                self.inject(target, value);
            }
        }
        Ok(value)
    }
    fn write(&mut self, request: &Request) -> Result<(), Error> {
        let word = request.word();
        let n = self.writes.len();
        self.writes.push(word);
        if self.suppress_write != Some(n) {
            match word.offset {
                CHANNEL_CLEAR => {
                    self.registers
                        .insert(CHANNEL_STATUS, self.value(CHANNEL_STATUS) & !word.value);
                }
                EVENT_CLEAR => {
                    self.registers
                        .insert(EVENT_STATUS, self.value(EVENT_STATUS) & !word.value);
                }
                GLOBAL_CLEAR => {
                    self.registers
                        .insert(GLOBAL_STATUS, self.value(GLOBAL_STATUS) & !word.value);
                }
                GENERIC_COMMAND => {
                    assert_eq!(
                        word.value & 31,
                        2,
                        "allocate only; no modem halt/flow control"
                    );
                    assert_eq!((word.value >> 10) & 15, 1, "modem EE only");
                    assert!(self.pending_command.is_none() && self.pending_modem.is_none());
                    self.pending_modem = Some(((word.value >> 5) & 31) as u8);
                }
                CHANNEL_COMMAND | EVENT_COMMAND => {
                    assert_eq!(word.value & !0xff, 0, "only ALLOCATE, never START/RESET");
                    assert!(self.pending_command.is_none());
                    self.pending_command = Some(if word.offset == EVENT_COMMAND {
                        Allocation::Event(word.value as u8)
                    } else {
                        Allocation::Channel(word.value as u8)
                    });
                }
                _ => {
                    let context0 =
                        (0x1c000..0x1d200).contains(&word.offset) && word.offset % 0x80 == 0;
                    let value = if context0 {
                        (word.value & !(15 << 20)) | (self.value(word.offset) & (15 << 20))
                    } else {
                        word.value
                    };
                    self.registers.insert(word.offset, value);
                }
            }
        }
        if let Some((trigger, target, value)) = self.pending_after_write {
            if trigger == word.offset {
                self.pending_after_write = None;
                self.inject(target, value);
            }
        }
        if self.fail_write.contains(&n) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn acknowledge(&mut self) -> Result<(), Error> {
        assert!(self.delivered, "polling cannot ACK a physical IRQ");
        self.acks += 1;
        if self.fail_ack {
            Err(Error::Acknowledge)
        } else {
            self.delivered = false;
            Ok(())
        }
    }
}
impl SetupIo for Fake {
    fn bind(&mut self) -> Result<(), Error> {
        self.binds += 1;
        for word in IRQ_CONFIG {
            assert_eq!(self.value(word.offset), word.value);
        }
        if self.fail_bind {
            Err(Error::Binding)
        } else {
            Ok(())
        }
    }
}
fn begin_and_complete(once: &mut AllocateOnce, io: &mut Fake) -> Result<(), Error> {
    once.begin(io)?;
    assert_eq!(once.report().phase, Phase::Pending);
    assert_eq!(io.acks, 0);
    io.complete();
    once.notified(io, 101)
}
fn terminal(once: &mut AllocateOnce, io: &mut Fake) {
    let before = io.counts();
    assert_eq!(once.begin(io), Err(Error::Consumed));
    assert_eq!(once.notified(io, 999), Err(Error::Consumed));
    assert_eq!(once.timeout(io, 999), Err(Error::Consumed));
    assert_eq!(once.external_fault(io), Err(Error::Consumed));
    assert_eq!(
        io.counts(),
        before,
        "terminal entry must not touch hardware"
    );
}

#[test]
fn prepared_plan_assigns_events_by_endpoint_order_then_sets_up_in_channel_order() {
    let p = Plan::from_prepared(&pool(), 0xe4046b59).unwrap();
    assert_eq!((p.limits().channels(), p.limits().events()), (11, 11));
    assert_eq!(
        p.channels()
            .map(|c| (c.id, c.event, c.endpoint, c.toward_ipa, c.command, c.tlvs)),
        [
            (0, 2, 1, true, false, 8),
            (1, 0, 6, true, true, 20),
            (2, 1, 8, false, false, 6),
            (3, 3, 9, false, false, 6)
        ]
    );
    assert_eq!(
        p.channels()
            .map(|c| (c.event_iova, c.tre_iova, c.event_bytes, c.tre_bytes)),
        [
            (0x8000a000, 0x8000c000, 8192, 8192),
            (0x80003000, 0x80004000, 4096, 4096),
            (0x80008000, 0x80009000, 4096, 4096),
            (0x8000e000, 0x8000f000, 4096, 4096)
        ]
    );
    assert_eq!(p.event_words(4), Err(Error::Layout));
    assert_eq!(p.channel_words(4, 0), Err(Error::Layout));
    assert_eq!(IRQ, 464);
}

#[test]
fn requires_prepared_retained_pool_and_decodes_counts_without_rejecting_other_fields() {
    let mut p = Pool::new(Ram(vec![0; POOL_BYTES / 4])).unwrap();
    assert_eq!(Plan::from_prepared(&p, 0xe4046b59), Err(Error::NotPrepared));
    p.prepare(|| Ok(()), |_, _, _| {});
    assert!(Plan::from_prepared(&p, 0xffff_ffff).is_ok());
    for channels in 0..4 {
        assert_eq!(
            Limits::decode((11 << 8) | (channels << 3)),
            Err(Error::HardwareLimits)
        );
    }
    for events in 0..4 {
        assert_eq!(
            Limits::decode((events << 8) | (11 << 3)),
            Err(Error::HardwareLimits)
        );
    }
    p.quarantine();
    assert_eq!(Plan::from_prepared(&p, 0xe4046b59), Err(Error::NotPrepared));
}

#[test]
fn ap_only_ids_and_bounded_wrap_safe_deadlines_are_checked_before_io() {
    for n in 4..=255 {
        for op in [Allocation::Event(n), Allocation::Channel(n)] {
            assert!(matches!(
                AllocateOnce::new(op, limits(), 100),
                Err(Error::Layout)
            ));
        }
    }
    for t in [0, 1 << 63, u64::MAX] {
        assert!(matches!(
            AllocateOnce::new(Allocation::Event(0), limits(), t),
            Err(Error::Layout)
        ));
    }
}

#[test]
fn allocation_requires_real_notification_and_allocated_state_before_ack() {
    for n in 0..4 {
        for op in [Allocation::Event(n), Allocation::Channel(n)] {
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            let mut io = Fake::new();
            assert_eq!(begin_and_complete(&mut once, &mut io), Ok(()));
            let r = once.report();
            assert_eq!(r.phase, Phase::Completed);
            assert_eq!(r.state, Some(1));
            assert_eq!(r.irq_type, Some(kind(op)));
            assert_eq!(r.irq_sources, Some(1 << n));
            assert_eq!((r.writes_sent, r.writes_done, io.acks), (7, 7, 1));
            assert!(r.ack_sent && r.ack_done && !r.mask_sent && r.error.is_none());
            assert_eq!(io.value(TYPE_MASK), BASE_IRQ_TYPES);
            terminal(&mut once, &mut io);
        }
    }
}

#[test]
fn cold_state_pending_source_and_mask_mismatches_never_submit_a_command() {
    for op in [Allocation::Event(2), Allocation::Channel(3)] {
        for (offset, value, error) in [
            (TYPE_MASK, 0, Error::Readback),
            (CHANNEL_MASK, 1, Error::Pending),
            (EVENT_MASK, 1, Error::Pending),
            (context(op), 1 << 20, Error::State),
            (source(op), 1, Error::Pending),
        ] {
            let mut io = Fake::new();
            io.registers.insert(offset, value);
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            assert_eq!(
                {
                    io.ticks = 0;
                    once.begin(&mut io)
                },
                Err(error)
            );
            assert!(io.pending_command.is_none());
            assert_eq!(io.writes.len(), 1);
            assert_eq!((io.writes[0].offset, io.writes[0].value), (TYPE_MASK, 0));
            terminal(&mut once, &mut io);
        }
        let mut io = Fake::new();
        io.extra_type = BASE_IRQ_TYPES;
        let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
        assert_eq!(
            {
                io.ticks = 0;
                once.begin(&mut io)
            },
            Err(Error::Pending)
        );
        assert!(io.pending_command.is_none());
    }
}

#[test]
fn unrelated_or_missing_completion_is_preserved_without_clear_or_ack() {
    for op in [Allocation::Event(0), Allocation::Channel(1)] {
        for extra in [0, 4, 64, 128] {
            let mut io = Fake::new();
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            {
                io.ticks = 0;
                once.begin(&mut io)
            }
            .unwrap();
            if extra != 0 {
                io.complete();
                io.extra_type = extra;
            }
            let writes = io.writes.len();
            assert_eq!(once.notified(&mut io, 1), Err(Error::UnexpectedIrq));
            assert_eq!(io.writes.len(), writes + 1);
            assert_eq!(io.acks, 0);
            terminal(&mut once, &mut io);
        }
        for wrong in [1 << ((id(op) + 1) % 4), (1 << id(op)) | 16] {
            let mut io = Fake::new();
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            {
                io.ticks = 0;
                once.begin(&mut io)
            }
            .unwrap();
            io.complete();
            io.registers.insert(source(op), wrong);
            // Keep the expected masked source present for the mixed case;
            // a wholly different source instead produces no expected type.
            assert_eq!(once.notified(&mut io, 1), Err(Error::UnexpectedIrq));
            assert_eq!(io.value(source(op)), wrong);
            assert_eq!(io.acks, 0);
        }
    }
}

#[test]
fn every_read_guard_or_uncertain_write_failure_is_terminal_and_masks_once() {
    for op in [Allocation::Event(2), Allocation::Channel(3)] {
        let mut good = Fake::new();
        let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
        begin_and_complete(&mut once, &mut good).unwrap();
        for n in 0..good.reads {
            let mut io = Fake::new();
            io.fail_read = Some(n);
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            assert_eq!(begin_and_complete(&mut once, &mut io), Err(Error::Io));
            assert_eq!(once.report().error, Some(Error::Io));
            assert_eq!(io.acks, 0);
            assert_eq!(
                io.writes.last().map(|w| (w.offset, w.value)),
                Some((TYPE_MASK, 0))
            );
            terminal(&mut once, &mut io);
        }
        for n in 0..good.guards {
            let mut io = Fake::new();
            io.fail_guard = Some(n);
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            assert_eq!(begin_and_complete(&mut once, &mut io), Err(Error::Guard));
            assert_eq!(io.acks, 0);
            terminal(&mut once, &mut io);
        }
        for n in 0..good.writes.len() {
            let mut io = Fake::new();
            io.fail_write.insert(n);
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            assert_eq!(begin_and_complete(&mut once, &mut io), Err(Error::Io));
            assert_eq!(io.writes.len(), n + 2);
            assert_eq!(io.acks, 0);
            assert_eq!(once.report().writes_done as usize, n);
            terminal(&mut once, &mut io);
        }
    }
}

#[test]
fn suppressed_masks_are_detected_before_command_or_before_ack() {
    for op in [Allocation::Event(2), Allocation::Channel(3)] {
        for n in [1, 2, 4, 5, 6] {
            let mut io = Fake::new();
            io.suppress_write = Some(n);
            let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
            let result = begin_and_complete(&mut once, &mut io);
            // Verify the raw source before disabling its mask, which could
            // otherwise hide a failed clear from the aggregate type status.
            if n == 4 {
                assert_eq!(result, Err(Error::Pending));
            } else {
                assert_eq!(result, Err(Error::Readback));
            }
            terminal(&mut once, &mut io);
        }
    }
}

#[test]
fn bad_state_ack_failure_and_containment_failure_keep_first_error() {
    let op = Allocation::Event(2);
    let mut io = Fake::new();
    let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
    {
        io.ticks = 0;
        once.begin(&mut io)
    }
    .unwrap();
    io.complete();
    io.registers.insert(context(op), 2 << 20);
    assert_eq!(once.notified(&mut io, 1), Err(Error::State));
    assert_eq!(io.acks, 0);
    terminal(&mut once, &mut io);
    let mut io = Fake::new();
    io.fail_ack = true;
    io.fail_write.insert(7);
    let mut once = AllocateOnce::new(op, limits(), 50).unwrap();
    assert_eq!(
        begin_and_complete(&mut once, &mut io),
        Err(Error::Acknowledge)
    );
    let r = once.report();
    assert!(r.ack_sent && !r.ack_done && r.mask_sent);
    assert_eq!(r.error, Some(Error::Acknowledge));
    assert_eq!(r.containment_error, Some(Error::Io));
    terminal(&mut once, &mut io);
}

#[test]
fn timeout_and_external_fault_preserve_pending_state_without_command_retry() {
    for external in [false, true] {
        let mut io = Fake::new();
        let mut once = AllocateOnce::new(Allocation::Channel(0), limits(), 50).unwrap();
        let start = u64::MAX - 20;
        {
            io.ticks = start;
            once.begin(&mut io)
        }
        .unwrap();
        io.complete();
        let before = io.counts();
        assert_eq!(once.timeout(&mut io, 28), Ok(()));
        assert_eq!(io.counts(), before);
        if external {
            assert_eq!(once.external_fault(&mut io), Err(Error::Guard));
        } else {
            assert_eq!(once.timeout(&mut io, 29), Err(Error::Timeout));
        }
        assert_eq!(io.value(CHANNEL_STATUS), 1);
        assert_eq!(io.acks, 0);
        assert_eq!(io.writes.len(), before.1 + 1);
        terminal(&mut once, &mut io);
    }
}

#[test]
fn context_words_match_compiled_pinned_c_functions() {
    let path = std::env::var("SUNFISH_GSI_C_REFERENCE")
        .expect("run test-native-ipa-linux-gsi.py for pinned C oracle");
    let expected = std::fs::read_to_string(path).unwrap();
    let p = Plan::from_prepared(&pool(), 0xe4046b59).unwrap();
    let mut actual = String::new();
    for scratch in [0, 0xffff_ffff, 0xabcd_1234, 0x0000_ffff, 0xffff_0000] {
        for i in 0..4 {
            for word in p
                .event_words(i)
                .unwrap()
                .into_iter()
                .chain(p.channel_words(i, scratch).unwrap())
            {
                actual.push_str(&format!(
                    "{scratch:08x} {i} {:05x} {:08x}\n",
                    word.offset, word.value
                ));
            }
        }
    }
    assert_eq!(actual, expected);
}

fn modem_run(once: &mut ModemAllocateOnce, io: &mut Fake) -> Result<(), Error> {
    once.begin(io)?;
    assert_eq!(io.acks, 0);
    io.complete();
    once.notified(io, 101)
}
fn modem_terminal(once: &mut ModemAllocateOnce, io: &mut Fake) {
    let before = io.counts();
    assert_eq!(once.begin(io), Err(Error::Consumed));
    assert_eq!(once.notified(io, 999), Err(Error::Consumed));
    assert_eq!(once.timeout(io, 999), Err(Error::Consumed));
    assert_eq!(once.external_fault(io), Err(Error::Consumed));
    assert_eq!(io.counts(), before);
}
#[test]
fn modem_workaround_accepts_linux_results_but_distinguishes_fresh_and_existing() {
    for channel in 0..4 {
        for result in 0..8 {
            let mut io = Fake::new();
            io.registers.insert(GLOBAL_ENABLE, 1);
            io.registers.insert(GENERIC_SCRATCH, 0xa50000ff);
            io.modem_result = result;
            let mut once = ModemAllocateOnce::new(channel, limits(), 50).unwrap();
            let actual = modem_run(&mut once, &mut io);
            if matches!(result, 1 | 2) {
                assert_eq!(actual, Ok(()));
                assert_eq!(io.acks, 1);
                assert_eq!(once.report().phase, Phase::Completed);
            } else {
                assert_eq!(actual, Err(Error::CommandResult(result)));
                assert_eq!(io.acks, 0);
                assert!(once.report().mask_sent);
            }
            assert_eq!(once.report().result, Some(result));
            assert_eq!(
                io.value(GENERIC_SCRATCH) & !(7 << 5),
                0xa500001f,
                "preserve unrelated scratch bits"
            );
            assert_eq!(
                io.writes
                    .iter()
                    .filter(|w| w.offset == GENERIC_COMMAND)
                    .map(|w| w.value)
                    .collect::<Vec<_>>(),
                [0x402 | (u32::from(channel) << 5)]
            );
            modem_terminal(&mut once, &mut io);
        }
    }
    for n in 4..=255 {
        assert!(matches!(
            ModemAllocateOnce::new(n, limits(), 50),
            Err(Error::Layout)
        ));
    }
    for timeout in [0, 1 << 63, u64::MAX] {
        assert!(matches!(
            ModemAllocateOnce::new(0, limits(), timeout),
            Err(Error::Layout)
        ));
    }
}

#[test]
fn modem_read_guard_and_uncertain_write_failures_are_consumed() {
    let mut good = Fake::new();
    good.registers.insert(GLOBAL_ENABLE, 1);
    modem_run(
        &mut ModemAllocateOnce::new(2, limits(), 50).unwrap(),
        &mut good,
    )
    .unwrap();
    for class in 0..3 {
        let count = match class {
            0 => good.reads,
            1 => good.guards,
            _ => good.writes.len(),
        };
        for n in 0..count {
            let mut io = Fake::new();
            io.registers.insert(GLOBAL_ENABLE, 1);
            let expected = if class == 1 { Error::Guard } else { Error::Io };
            match class {
                0 => io.fail_read = Some(n),
                1 => io.fail_guard = Some(n),
                _ => {
                    io.fail_write.insert(n);
                }
            }
            let mut once = ModemAllocateOnce::new(2, limits(), 50).unwrap();
            assert_eq!(modem_run(&mut once, &mut io), Err(expected));
            assert_eq!(io.acks, 0);
            assert_eq!(once.report().error, Some(expected));
            modem_terminal(&mut once, &mut io);
        }
    }
}

#[test]
fn modem_rejects_foreign_global_status_and_never_retries_on_timeout_or_ack_error() {
    for global in [0, 1, 3, 4, 6, 0x80000002] {
        let mut io = Fake::new();
        io.registers.insert(GLOBAL_ENABLE, 1);
        let mut once = ModemAllocateOnce::new(0, limits(), 50).unwrap();
        {
            io.ticks = 0;
            once.begin(&mut io)
        }
        .unwrap();
        io.complete();
        io.registers.insert(GLOBAL_STATUS, global);
        assert_eq!(once.notified(&mut io, 1), Err(Error::UnexpectedIrq));
        assert_eq!(io.value(GLOBAL_STATUS), global);
        assert_eq!(io.acks, 0);
        modem_terminal(&mut once, &mut io);
    }
    for external in [false, true] {
        let mut io = Fake::new();
        io.registers.insert(GLOBAL_ENABLE, 1);
        let mut once = ModemAllocateOnce::new(0, limits(), 50).unwrap();
        {
            io.ticks = u64::MAX - 20;
            once.begin(&mut io)
        }
        .unwrap();
        io.complete();
        let count = io.counts();
        once.timeout(&mut io, 28).unwrap();
        assert_eq!(io.counts(), count);
        let result = if external {
            once.external_fault(&mut io)
        } else {
            once.timeout(&mut io, 29)
        };
        assert_eq!(
            result,
            Err(if external {
                Error::Guard
            } else {
                Error::Timeout
            })
        );
        assert_eq!(io.value(GLOBAL_STATUS), 2);
        assert_eq!(io.acks, 0);
        modem_terminal(&mut once, &mut io);
    }
    let mut io = Fake::new();
    io.registers.insert(GLOBAL_ENABLE, 1);
    io.fail_ack = true;
    io.fail_write.insert(5);
    let mut once = ModemAllocateOnce::new(0, limits(), 50).unwrap();
    assert_eq!(modem_run(&mut once, &mut io), Err(Error::Acknowledge));
    assert_eq!(once.report().containment_error, Some(Error::Io));
    assert!(once.report().ack_sent && !once.report().ack_done);
    modem_terminal(&mut once, &mut io);
}

fn setup_plan() -> Plan {
    Plan::from_prepared(&pool(), 0xe4046b59).unwrap()
}
fn setup_run(once: &mut SetupOnce, io: &mut Fake) -> Result<(), Error> {
    once.start(io)?;
    for n in 0..12 {
        io.complete();
        once.notified(io, 101 + n)?;
    }
    assert_eq!(once.report().phase, SetupPhase::Complete);
    Ok(())
}
fn setup_terminal(once: &mut SetupOnce, io: &mut Fake) {
    let before = (io.counts(), io.binds);
    assert_eq!(once.start(io), Err(Error::Consumed));
    assert_eq!(once.notified(io, 999), Err(Error::Consumed));
    assert_eq!(once.timeout(io, 999), Err(Error::Consumed));
    assert_eq!(once.external_fault(io), Err(Error::Consumed));
    assert_eq!((io.counts(), io.binds), before);
}

#[test]
fn complete_setup_uses_eight_ap_then_four_modem_commands_and_only_event_doorbells() {
    for result in [1, 2] {
        let mut io = Fake::cold();
        io.modem_result = result;
        let mut once = SetupOnce::new(setup_plan(), 50).unwrap();
        setup_run(&mut once, &mut io).unwrap();
        let r = once.report();
        assert_eq!(
            (
                r.events_allocated,
                r.events_programmed,
                r.channels_allocated,
                r.channels_programmed,
                r.modem_accepted
            ),
            (15, 15, 15, 15, 15)
        );
        assert_eq!(
            (r.modem_fresh, r.modem_existing),
            if result == 1 { (15, 0) } else { (0, 15) }
        );
        assert_eq!(
            (
                r.writes_sent,
                r.writes_done,
                io.writes.len(),
                io.acks,
                io.binds
            ),
            (94, 94, 170, 12, 1)
        );
        assert!(r.irq_bound && r.error.is_none() && !r.mask_sent);
        assert_eq!(
            io.writes
                .iter()
                .filter(|w| [EVENT_COMMAND, CHANNEL_COMMAND, GENERIC_COMMAND].contains(&w.offset))
                .map(|w| (w.offset, w.value))
                .collect::<Vec<_>>(),
            [
                (EVENT_COMMAND, 2),
                (CHANNEL_COMMAND, 0),
                (EVENT_COMMAND, 0),
                (CHANNEL_COMMAND, 1),
                (EVENT_COMMAND, 1),
                (CHANNEL_COMMAND, 2),
                (EVENT_COMMAND, 3),
                (CHANNEL_COMMAND, 3),
                (GENERIC_COMMAND, 0x402),
                (GENERIC_COMMAND, 0x422),
                (GENERIC_COMMAND, 0x442),
                (GENERIC_COMMAND, 0x462)
            ]
        );
        assert_eq!(
            io.writes
                .iter()
                .filter(|w| (0x1e000..0x1e100).contains(&w.offset))
                .count(),
            0,
            "no transfer doorbells"
        );
        assert_eq!(
            io.writes
                .iter()
                .filter(|w| (0x1e100..0x1e200).contains(&w.offset))
                .count(),
            4
        );
        setup_terminal(&mut once, &mut io);
    }
}

#[test]
fn setup_does_not_bind_or_allocate_past_any_failed_initial_condition() {
    let plan = setup_plan();
    for (offset, value, error) in [
        (GSI_STATUS, 0, Error::State),
        (HW_PARAM_2, 0, Error::HardwareLimits),
        (GLOBAL_STATUS, 1, Error::Pending),
        (GENERAL_STATUS, 2, Error::Pending),
        (CHANNEL_STATUS, 1, Error::Pending),
        (EVENT_STATUS, 1, Error::Pending),
    ] {
        let mut io = Fake::cold();
        io.registers.insert(offset, value);
        let mut once = SetupOnce::new(plan, 50).unwrap();
        assert_eq!(once.start(&mut io), Err(error));
        assert!(io.pending_command.is_none() && io.pending_modem.is_none());
        setup_terminal(&mut once, &mut io);
    }
    let mut io = Fake::cold();
    io.fail_bind = true;
    let mut once = SetupOnce::new(plan, 50).unwrap();
    assert_eq!(once.start(&mut io), Err(Error::Binding));
    assert_eq!(io.binds, 1);
    assert!(!once.report().irq_bound);
    assert_eq!(io.acks, 0);
    setup_terminal(&mut once, &mut io);
}

#[test]
fn setup_initializes_and_records_unqualified_error_log_as_in_linux() {
    for raw in [0, 1, 0xe71132f0, 0x02003000, u32::MAX] {
        let mut io = Fake::cold();
        io.registers.insert(ERROR_LOG, raw);
        let mut once = SetupOnce::new(setup_plan(), 50).unwrap();
        setup_run(&mut once, &mut io).unwrap();
        assert_eq!(once.report().error_log_before, Some(raw));
        assert_eq!(once.report().error_log_after, Some(0));
        assert_eq!(
            io.writes
                .iter()
                .filter(|w| w.offset == ERROR_LOG)
                .map(|w| w.value)
                .collect::<Vec<_>>(),
            [0]
        );
        assert_eq!(io.writes.len(), 170);
        setup_terminal(&mut once, &mut io);
    }
    // Linux does not use the raw log as a pending-status predicate, even
    // immediately after its initialization store. Keep the sample diagnostic.
    let mut io = Fake::cold();
    io.registers.insert(ERROR_LOG, 0xe71132f0);
    io.suppress_write = Some(9);
    let mut once = SetupOnce::new(setup_plan(), 50).unwrap();
    once.start(&mut io).unwrap();
    assert_eq!(once.report().error_log_after, Some(0xe71132f0));
    assert!(io.pending_command.is_some());
}

#[test]
fn setup_preserves_new_pending_sources_around_error_log_initialization() {
    for after_write in [false, true] {
        for (target, value) in [
            (TYPE_STATUS, 4),
            (GLOBAL_STATUS, 1),
            (GENERAL_STATUS, 2),
            (CHANNEL_STATUS, 1),
            (EVENT_STATUS, 1),
        ] {
            let mut io = Fake::cold();
            io.registers.insert(ERROR_LOG, 0xe71132f0);
            let injection = Some((ERROR_LOG, target, value));
            if after_write {
                io.pending_after_write = injection;
            } else {
                io.pending_after_read = injection;
            }
            let mut once = SetupOnce::new(setup_plan(), 50).unwrap();
            assert_eq!(once.start(&mut io), Err(Error::Pending));
            assert_eq!(once.report().error_log_before, Some(0xe71132f0));
            assert_eq!(once.report().error_log_after, after_write.then_some(0));
            assert_eq!(
                io.writes.iter().filter(|w| w.offset == ERROR_LOG).count(),
                usize::from(after_write)
            );
            assert!(io.pending_command.is_none() && io.pending_modem.is_none());
            if target == TYPE_STATUS {
                assert_eq!(io.extra_type, value);
            } else {
                assert_eq!(io.value(target), value);
            }
            assert!(
                !io.writes.iter().any(
                    |w| [CHANNEL_CLEAR, EVENT_CLEAR, GLOBAL_CLEAR, 0x1f210].contains(&w.offset)
                )
            );
            assert_eq!(io.acks, 0);
            setup_terminal(&mut once, &mut io);
        }
    }
}

#[test]
fn complete_setup_quarantines_each_read_guard_and_uncertain_write_failure_without_double_mask() {
    let plan = setup_plan();
    let mut good = Fake::cold();
    setup_run(&mut SetupOnce::new(plan, 50).unwrap(), &mut good).unwrap();
    for class in 0..3 {
        let count = match class {
            0 => good.reads,
            1 => good.guards,
            _ => good.writes.len(),
        };
        for n in 0..count {
            let mut io = Fake::cold();
            let expected = if class == 1 { Error::Guard } else { Error::Io };
            match class {
                0 => io.fail_read = Some(n),
                1 => io.fail_guard = Some(n),
                _ => {
                    io.fail_write.insert(n);
                }
            }
            let mut once = SetupOnce::new(plan, 50).unwrap();
            assert_eq!(
                setup_run(&mut once, &mut io),
                Err(expected),
                "class {class} index {n}"
            );
            assert_eq!(once.report().phase, SetupPhase::Failed);
            assert_eq!(once.report().error, Some(expected));
            assert_eq!(
                io.writes.last().map(|w| (w.offset, w.value)),
                Some((TYPE_MASK, 0))
            );
            if class == 2 {
                assert_eq!(
                    io.writes.len(),
                    n + 2,
                    "uncertain write plus exactly one containment"
                );
            }
            setup_terminal(&mut once, &mut io);
        }
    }
}

#[test]
fn setup_context_readback_failure_and_pending_timeout_preserve_prior_allocations() {
    let plan = setup_plan();
    let mut baseline = Fake::cold();
    setup_run(&mut SetupOnce::new(plan, 50).unwrap(), &mut baseline).unwrap();
    for (n, word) in baseline.writes.iter().enumerate() {
        if !(0x1c000..0x1d200).contains(&word.offset) || word.value == 0 {
            continue;
        }
        let mut io = Fake::cold();
        io.suppress_write = Some(n);
        let mut once = SetupOnce::new(plan, 50).unwrap();
        assert_eq!(
            setup_run(&mut once, &mut io),
            Err(Error::Readback),
            "offset {:x}",
            word.offset
        );
        assert!(once.report().events_allocated != 0);
        setup_terminal(&mut once, &mut io);
    }
    for completed in 0..12 {
        let mut io = Fake::cold();
        let mut once = SetupOnce::new(plan, 50).unwrap();
        once.start(&mut io).unwrap();
        for n in 0..completed {
            io.complete();
            once.notified(&mut io, 101 + n).unwrap();
        }
        io.complete();
        let before = io.counts();
        assert_eq!(once.timeout(&mut io, 1000), Err(Error::Timeout));
        assert_eq!(io.acks, before.3);
        assert_eq!(io.writes.len(), before.1 + 1);
        setup_terminal(&mut once, &mut io);
    }
}

#[test]
fn complete_setup_writes_match_compiled_linux_irq_command_and_context_functions() {
    let path = std::env::var("SUNFISH_GSI_C_SETUP_REFERENCE")
        .expect("run test-native-ipa-linux-gsi.py for pinned setup oracle");
    let expected = std::fs::read_to_string(path).unwrap();
    let mut actual = String::new();
    for result in [1, 2] {
        let mut io = Fake::cold();
        io.modem_result = result;
        setup_run(&mut SetupOnce::new(setup_plan(), 50).unwrap(), &mut io).unwrap();
        for word in io.writes {
            actual.push_str(&format!(
                "setup {result} {:05x} {:08x}\n",
                word.offset, word.value
            ));
        }
    }
    assert_eq!(actual, expected);
}

#[test]
fn deadline_starts_at_each_command_store_after_slow_guards_and_context_work() {
    assert_eq!(command_timeout_ticks(19_200_000), Ok(960_000));
    assert_eq!(command_timeout_ticks(1), Ok(1));
    assert_eq!(command_timeout_ticks(21), Ok(2));
    assert_eq!(command_timeout_ticks(0), Err(Error::Layout));
    assert_eq!(command_timeout_ticks(u64::MAX), Ok(u64::MAX / 20 + 1));
    let mut io = Fake::cold();
    io.guard_ticks = 10;
    let mut once = SetupOnce::new(setup_plan(), 50).unwrap();
    assert_eq!(once.deadline(), None);
    once.start(&mut io).unwrap();
    for _ in 0..12 {
        assert_eq!(once.deadline(), Some(io.ticks + 50));
        let now = io.ticks + 1;
        io.complete();
        once.notified(&mut io, now).unwrap();
    }
    assert_eq!(once.report().phase, SetupPhase::Complete);
    assert_eq!(once.deadline(), None);
    let mut io = Fake::new();
    io.ticks = u64::MAX - 20;
    let mut ap = AllocateOnce::new(Allocation::Channel(0), limits(), 50).unwrap();
    ap.begin(&mut io).unwrap();
    assert_eq!(ap.deadline(), Some(29));
}

#[test]
fn late_delivered_irq_after_setup_is_masked_once_without_clearing_or_ack() {
    let mut io = Fake::cold();
    let mut once = SetupOnce::new(setup_plan(), 50).unwrap();
    setup_run(&mut once, &mut io).unwrap();
    let before = io.counts();
    io.extra_type = 64;
    io.delivered = true;
    assert_eq!(once.late_notification(&mut io), Err(Error::UnexpectedIrq));
    assert_eq!(io.counts(), (before.0, before.1 + 1, before.2, before.3));
    let after = io.counts();
    assert_eq!(once.late_notification(&mut io), Err(Error::Consumed));
    assert_eq!(io.counts(), after);
    setup_terminal(&mut once, &mut io);
}
