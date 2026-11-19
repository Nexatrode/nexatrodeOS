#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_command.rs"]
pub mod sunfish_ipa_linux_command;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
pub mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
pub mod sunfish_ipa_linux_gsi;
mod fixture {
    include!(env!("SUNFISH_COMMAND_ENDPOINT_FIXTURE"));
}
use std::collections::BTreeMap;
use sunfish_ipa_linux_command::*;
use sunfish_ipa_linux_endpoint::{self as ep, ENDPOINTS};
use sunfish_ipa_linux_gsi as gsi;

struct Fake {
    values: BTreeMap<usize, u32>,
    writes: Vec<(usize, u32)>,
    reads: usize,
    guards: usize,
    acks: usize,
    now: u64,
    guard_ticks: u64,
    pending: bool,
    delivered: bool,
    extra_type: u32,
    fail_read: Option<usize>,
    fail_guard: Option<usize>,
    fail_write: Option<usize>,
    suppress: Option<usize>,
    fail_ack: bool,
    mask_failure: bool,
}
impl Fake {
    fn new() -> Self {
        let mut values = BTreeMap::from([
            (gsi::GSI_STATUS, 1),
            (gsi::TYPE_MASK, gsi::BASE_IRQ_TYPES),
            (gsi::GLOBAL_ENABLE, 1),
            (0x1f120, 14),
        ]);
        for e in ENDPOINTS {
            for r in [Read::Channel(e), Read::Event(e)] {
                values.insert(r.offset(), 1 << 20);
            }
        }
        Self {
            values,
            writes: vec![],
            reads: 0,
            guards: 0,
            acks: 0,
            now: 100,
            guard_ticks: 0,
            pending: false,
            delivered: false,
            extra_type: 0,
            fail_read: None,
            fail_guard: None,
            fail_write: None,
            suppress: None,
            fail_ack: false,
            mask_failure: false,
        }
    }
    fn value(&self, o: usize) -> u32 {
        self.values.get(&o).copied().unwrap_or(0)
    }
    fn complete(&mut self) {
        assert!(self.pending);
        self.pending = false;
        self.delivered = true;
        self.values
            .insert(Read::Channel(ep::Endpoint::CommandTx).offset(), 2 << 20);
        self.values.insert(gsi::CHANNEL_STATUS, 2);
    }
    fn counts(&self) -> (usize, usize, usize, usize) {
        (self.reads, self.writes.len(), self.guards, self.acks)
    }
}
impl Io for Fake {
    fn ticks(&mut self) -> u64 {
        self.now
    }
    fn guard(&mut self) -> Result<(), Error> {
        self.now = self.now.wrapping_add(self.guard_ticks);
        let n = self.guards;
        self.guards += 1;
        if self.fail_guard == Some(n) {
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
        if r == Read::TypeStatus {
            let mut v = self.extra_type;
            for (source, mask, kind) in [
                (gsi::CHANNEL_STATUS, gsi::CHANNEL_MASK, 1),
                (gsi::EVENT_STATUS, gsi::EVENT_MASK, 2),
                (gsi::GLOBAL_STATUS, gsi::GLOBAL_ENABLE, 4),
                (IEOB_STATUS, IEOB_MASK, 8),
                (gsi::GENERAL_STATUS, 0x1f120, 64),
            ] {
                if self.value(source) & self.value(mask) != 0 {
                    v |= kind;
                }
            }
            Ok(v)
        } else {
            Ok(self.value(r.offset()))
        }
    }
    fn write(&mut self, r: &Request) -> Result<(), Error> {
        let n = self.writes.len();
        let offset = r.register().offset();
        let value = r.value();
        self.writes.push((offset, value));
        if self.suppress != Some(n) {
            match r.register() {
                Register::ChannelClear => {
                    self.values.insert(
                        gsi::CHANNEL_STATUS,
                        self.value(gsi::CHANNEL_STATUS) & !value,
                    );
                }
                Register::Command => {
                    assert_eq!(value, START);
                    assert!(!self.pending);
                    self.pending = true;
                }
                _ => {
                    self.values.insert(offset, value);
                }
            }
        }
        if self.fail_write == Some(n)
            || (self.mask_failure && r.register() == Register::TypeMask && value == 0)
        {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn acknowledge(&mut self) -> Result<(), Error> {
        assert!(self.delivered);
        self.acks += 1;
        if self.fail_ack {
            Err(Error::Acknowledge)
        } else {
            self.delivered = false;
            Ok(())
        }
    }
}
fn run(
    once: &mut StartOnce,
    g: &gsi::SetupOnce,
    e: &ep::ConfigureOnce,
    io: &mut Fake,
) -> Result<(), Error> {
    once.begin(g, e, io)?;
    io.complete();
    once.notified(io, io.now)
}
fn terminal(once: &mut StartOnce, g: &gsi::SetupOnce, e: &ep::ConfigureOnce, io: &mut Fake) {
    let before = io.counts();
    assert_eq!(once.begin(g, e, io), Err(Error::Consumed));
    assert_eq!(once.notified(io, 999), Err(Error::Consumed));
    assert_eq!(once.timeout(io, 999), Err(Error::Consumed));
    assert_eq!(once.external_fault(io), Err(Error::Consumed));
    assert_eq!(io.counts(), before);
}
#[test]
fn cmdstart_matches_full_compiled_linux_start_including_ieob_and_irq_clear() {
    let (g, e) = fixture::owned_startup();
    let mut io = Fake::new();
    let mut once = StartOnce::new(50).unwrap();
    run(&mut once, &g, &e, &mut io).unwrap();
    let c: Vec<_> = std::fs::read_to_string(std::env::var("SUNFISH_COMMAND_C_REFERENCE").unwrap())
        .unwrap()
        .lines()
        .map(|s| {
            let mut p = s.split_whitespace();
            (
                usize::from_str_radix(p.next().unwrap(), 16).unwrap(),
                u32::from_str_radix(p.next().unwrap(), 16).unwrap(),
            )
        })
        .collect();
    assert_eq!(io.writes, c);
    assert_eq!(io.writes.len(), 9);
    let r = once.report();
    assert_eq!(r.phase, Phase::Complete);
    assert_eq!((r.writes_sent, r.writes_done), (9, 9));
    assert!(r.ack_sent && r.ack_done && !r.mask_sent);
    assert_eq!(r.state, Some(2));
    assert_eq!(io.acks, 1);
    assert_eq!(io.value(IEOB_MASK), 1);
    assert_eq!(io.value(gsi::TYPE_MASK), 0x4c);
    assert!(
        !io.writes
            .iter()
            .any(|&(o, _)| (0x1e000..0x1e200).contains(&o))
    );
    terminal(&mut once, &g, &e, &mut io);
}
#[test]
fn cmdstart_rejects_incomplete_owners_and_invalid_timeouts_without_io() {
    assert!(matches!(StartOnce::new(0), Err(Error::Layout)));
    assert!(matches!(StartOnce::new(1 << 63), Err(Error::Layout)));
    let (g, e) = fixture::owned_startup();
    for incomplete_gsi in [false, true] {
        let mut io = Fake::new();
        let mut once = StartOnce::new(50).unwrap();
        assert_eq!(once.external_fault(&mut io), Err(Error::Consumed));
        let result = if incomplete_gsi {
            once.begin(&fixture::incomplete_setup(), &e, &mut io)
        } else {
            once.begin(&g, &ep::ConfigureOnce::new(), &mut io)
        };
        assert_eq!(result, Err(Error::Prerequisite));
        assert_eq!(io.counts(), (0, 0, 0, 0));
        terminal(&mut once, &g, &e, &mut io);
    }
}
#[test]
fn cmdstart_every_read_guard_and_uncertain_write_failure_is_terminal_and_retained() {
    let (g, e) = fixture::owned_startup();
    let mut good = Fake::new();
    run(&mut StartOnce::new(50).unwrap(), &g, &e, &mut good).unwrap();
    for class in 0..3 {
        let count = match class {
            0 => good.reads,
            1 => good.guards,
            _ => good.writes.len(),
        };
        for n in 0..count {
            let mut io = Fake::new();
            match class {
                0 => io.fail_read = Some(n),
                1 => io.fail_guard = Some(n),
                _ => io.fail_write = Some(n),
            }
            let mut once = StartOnce::new(50).unwrap();
            let error = if class == 1 { Error::Guard } else { Error::Io };
            assert_eq!(run(&mut once, &g, &e, &mut io), Err(error));
            let r = once.report();
            assert_eq!(r.phase, Phase::Failed);
            assert_eq!(r.error, Some(error));
            assert!(r.mask_sent);
            assert_eq!(io.writes.last(), Some(&(gsi::TYPE_MASK, 0)));
            assert_eq!(io.acks, 0);
            assert_eq!(
                &io.writes[..io.writes.len() - 1],
                &good.writes[..io.writes.len() - 1]
            );
            terminal(&mut once, &g, &e, &mut io);
        }
    }
}
#[test]
fn cmdstart_timeout_wrap_and_slow_preparation_do_not_replay_or_ack() {
    let (g, e) = fixture::owned_startup();
    let mut io = Fake::new();
    io.guard_ticks = 100;
    let mut once = StartOnce::new(50).unwrap();
    once.begin(&g, &e, &mut io).unwrap();
    let start = io.now;
    assert!(start > 1000);
    assert_eq!(once.deadline(), Some(start + 50));
    let before = io.counts();
    once.timeout(&mut io, start + 49).unwrap();
    assert_eq!(io.counts(), before);
    assert_eq!(once.timeout(&mut io, start + 50), Err(Error::Timeout));
    assert_eq!(io.acks, 0);
    assert!(io.pending);
    terminal(&mut once, &g, &e, &mut io);
    let mut io = Fake::new();
    io.now = u64::MAX - 20;
    let mut once = StartOnce::new(50).unwrap();
    once.begin(&g, &e, &mut io).unwrap();
    io.complete();
    assert_eq!(once.deadline(), Some(29));
    once.notified(&mut io, 28).unwrap();
}
#[test]
fn cmdstart_unexpected_sources_are_not_cleared_or_acknowledged() {
    let (g, e) = fixture::owned_startup();
    for extra in [0, 4, 8, 64] {
        for source in [0, 2, 3, 4] {
            if extra == 0 && source == 2 {
                continue;
            }
            let mut io = Fake::new();
            let mut once = StartOnce::new(50).unwrap();
            once.begin(&g, &e, &mut io).unwrap();
            io.complete();
            io.extra_type = extra;
            io.values.insert(gsi::CHANNEL_STATUS, source);
            let before = io.writes.len();
            assert_eq!(once.notified(&mut io, 100), Err(Error::UnexpectedIrq));
            assert_eq!(&io.writes[before..], [(gsi::TYPE_MASK, 0)]);
            assert_eq!(io.value(gsi::CHANNEL_STATUS), source);
            assert_eq!(io.acks, 0);
            terminal(&mut once, &g, &e, &mut io);
        }
    }
    for o in [
        gsi::EVENT_STATUS,
        gsi::GLOBAL_STATUS,
        gsi::GENERAL_STATUS,
        IEOB_STATUS,
    ] {
        let mut io = Fake::new();
        let mut once = StartOnce::new(50).unwrap();
        once.begin(&g, &e, &mut io).unwrap();
        io.complete();
        io.values.insert(o, 1);
        assert_eq!(once.notified(&mut io, 100), Err(Error::UnexpectedIrq));
        assert_eq!(io.value(o), 1);
        assert_eq!(io.value(gsi::CHANNEL_STATUS), 2);
        assert_eq!(io.acks, 0);
    }
}
#[test]
fn cmdstart_pending_preconditions_and_wrong_context_states_stop_before_start() {
    let (g, e) = fixture::owned_startup();
    for r in [
        Read::ChannelStatus,
        Read::EventStatus,
        Read::GlobalStatus,
        Read::GeneralStatus,
        Read::IeobStatus,
    ] {
        let mut io = Fake::new();
        io.values.insert(r.offset(), 1);
        let mut once = StartOnce::new(50).unwrap();
        assert_eq!(once.begin(&g, &e, &mut io), Err(Error::Pending));
        assert_eq!(io.writes, [(gsi::TYPE_MASK, 0)]);
        assert_eq!(io.value(r.offset()), 1);
    }
    for ep in ENDPOINTS {
        for r in [Read::Channel(ep), Read::Event(ep)] {
            let mut io = Fake::new();
            io.values.insert(r.offset(), 0);
            let mut once = StartOnce::new(50).unwrap();
            assert_eq!(once.begin(&g, &e, &mut io), Err(Error::State));
            assert_eq!(io.writes, [(gsi::TYPE_MASK, 0)]);
        }
    }
}
#[test]
fn cmdstart_readback_lost_command_ack_and_containment_errors_preserve_evidence() {
    let (g, e) = fixture::owned_startup();
    for n in [0, 1, 3, 4, 6, 7, 8] {
        let mut io = Fake::new();
        io.suppress = Some(n);
        let mut once = StartOnce::new(50).unwrap();
        let expected = if n == 6 {
            Error::Pending
        } else {
            Error::Readback
        };
        assert_eq!(run(&mut once, &g, &e, &mut io), Err(expected));
        assert_eq!(io.acks, 0);
        terminal(&mut once, &g, &e, &mut io);
    }
    let mut io = Fake::new();
    io.suppress = Some(5);
    let mut once = StartOnce::new(50).unwrap();
    once.begin(&g, &e, &mut io).unwrap();
    assert!(!io.pending);
    assert_eq!(once.timeout(&mut io, 150), Err(Error::Timeout));
    assert_eq!(io.acks, 0);
    let mut io = Fake::new();
    io.fail_ack = true;
    let mut once = StartOnce::new(50).unwrap();
    assert_eq!(run(&mut once, &g, &e, &mut io), Err(Error::Acknowledge));
    assert!(once.report().ack_sent && !once.report().ack_done);
    assert_eq!(io.acks, 1);
    assert_eq!(
        io.value(Read::Channel(ep::Endpoint::CommandTx).offset()),
        2 << 20
    );
    terminal(&mut once, &g, &e, &mut io);
    let mut io = Fake::new();
    io.fail_guard = Some(0);
    io.mask_failure = true;
    let mut once = StartOnce::new(50).unwrap();
    assert_eq!(once.begin(&g, &e, &mut io), Err(Error::Guard));
    assert_eq!(once.report().containment_error, Some(Error::Io));
    terminal(&mut once, &g, &e, &mut io);
}

#[test]
fn cmdstart_wrong_post_state_and_external_fault_leave_allocations_retained() {
    let (g, e) = fixture::owned_startup();
    for state in [0, 1, 3, 4, 15] {
        let mut io = Fake::new();
        let mut once = StartOnce::new(50).unwrap();
        once.begin(&g, &e, &mut io).unwrap();
        io.complete();
        io.values
            .insert(Read::Channel(ep::Endpoint::CommandTx).offset(), state << 20);
        assert_eq!(once.notified(&mut io, 100), Err(Error::State));
        assert_eq!(
            io.value(Read::Channel(ep::Endpoint::CommandTx).offset()),
            state << 20
        );
        assert_eq!(io.acks, 0);
        terminal(&mut once, &g, &e, &mut io);
    }
    let mut io = Fake::new();
    let mut once = StartOnce::new(50).unwrap();
    once.begin(&g, &e, &mut io).unwrap();
    io.complete();
    let before = io.writes.len();
    assert_eq!(once.external_fault(&mut io), Err(Error::Guard));
    assert_eq!(&io.writes[before..], [(gsi::TYPE_MASK, 0)]);
    assert_eq!(io.value(gsi::CHANNEL_STATUS), 2);
    assert_eq!(io.acks, 0);
    terminal(&mut once, &g, &e, &mut io);
}
