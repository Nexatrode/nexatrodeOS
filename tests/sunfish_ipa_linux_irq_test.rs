#[path = "../rust/src/sunfish_ipa_linux_irq.rs"]
mod sunfish_ipa_linux_irq;
use sunfish_ipa_linux_irq::*;

struct Fake {
    enabled: u32,
    status: u32,
    reads: usize,
    guards: Vec<u32>,
    writes: Vec<Step>,
    binds: usize,
    fields: Vec<Shared>,
    fail_read: Option<usize>,
    fail_write: Option<usize>,
    suppress: Option<usize>,
    fail_guard: Option<(usize, Error)>,
    fail_bind: bool,
    fail_shared: bool,
    pending_at: Option<usize>,
    changed_enable_at: Option<usize>,
}
impl Fake {
    fn new() -> Self {
        Self {
            enabled: 0,
            status: 0,
            reads: 0,
            guards: vec![],
            writes: vec![],
            binds: 0,
            fields: vec![],
            fail_read: None,
            fail_write: None,
            suppress: None,
            fail_guard: None,
            fail_bind: false,
            fail_shared: false,
            pending_at: None,
            changed_enable_at: None,
        }
    }
}
impl Io for Fake {
    fn guard(&mut self, expected: u32) -> Result<(), Error> {
        let n = self.guards.len();
        self.guards.push(expected);
        if let Some((at, error)) = self.fail_guard {
            if n == at {
                return Err(error);
            }
        }
        Ok(())
    }
    fn read(&mut self, r: Read) -> Result<u32, Error> {
        let n = self.reads;
        self.reads += 1;
        if self.pending_at == Some(n) {
            self.status |= 1 << 3;
        }
        if self.changed_enable_at == Some(n) {
            self.enabled ^= 0x4000;
        }
        if self.fail_read == Some(n) {
            Err(Error::Io)
        } else {
            Ok(match r {
                Read::Enable => self.enabled,
                Read::Status => self.status,
            })
        }
    }
    fn write(&mut self, w: &Write) -> Result<(), Error> {
        let n = self.writes.len();
        self.writes.push(w.step());
        if self.suppress != Some(n) {
            self.enabled = w.value();
        }
        if self.fail_write == Some(n) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn bind(&mut self) -> Result<(), Error> {
        self.binds += 1;
        if self.fail_bind {
            Err(Error::Binding)
        } else {
            assert_eq!(self.enabled, 0);
            assert_eq!(self.writes, vec![Step::Disable]);
            Ok(())
        }
    }
    fn shared(&mut self, f: Shared) -> Result<u8, Error> {
        self.fields.push(f);
        if self.fail_shared {
            Err(Error::Io)
        } else {
            Ok(match f {
                Shared::Event => 2,
                Shared::Response => 1,
            })
        }
    }
}
fn run(io: &mut Fake, once: &mut ArmOnce) -> Report {
    once.run(io, |_| {})
}
#[test]
fn runtime_transfer_requires_success_and_retires_old_containment() {
    let mut once = ArmOnce::new();
    assert!(!once.transfer_to_runtime());
    let mut io = Fake::new();
    assert!(run(&mut io, &mut once).error.is_none());
    assert!(once.transfer_to_runtime());
    assert!(!once.transfer_to_runtime());
    let count = io.writes.len();
    assert_eq!(once.notification(&mut io).error, Error::AlreadyStopped);
    assert_eq!(io.writes.len(), count);
}
fn no_retry(io: &mut Fake, once: &mut ArmOnce) {
    let count = (
        io.reads,
        io.guards.len(),
        io.writes.len(),
        io.binds,
        io.fields.len(),
    );
    assert_eq!(run(io, once).error, Some(Error::AlreadyStarted));
    assert_eq!(
        count,
        (
            io.reads,
            io.guards.len(),
            io.writes.len(),
            io.binds,
            io.fields.len()
        )
    );
}
#[test]
fn linux_mask_order_and_binding_precede_nonzero_enable() {
    assert_eq!((IRQ, CHANNEL), (343, 2));
    assert_eq!(
        (Read::Enable.offset(), Read::Status.offset()),
        (0x300c, 0x3008)
    );
    assert_eq!(
        (Shared::Event.offset(), Shared::Response.offset()),
        (20, 12)
    );
    assert_eq!(Error::Binding.code(), 7);
    let mut io = Fake::new();
    let mut once = ArmOnce::new();
    let r = run(&mut io, &mut once);
    assert_eq!(r.error, None);
    assert_eq!(r.submitted, COMPLETE);
    assert_eq!(r.completed, COMPLETE);
    assert!(r.bound);
    assert_eq!(
        io.writes.iter().map(|s| s.value()).collect::<Vec<_>>(),
        vec![0, 4, 12]
    );
    assert_eq!(
        r.last,
        Some(Snapshot {
            enabled: 12,
            status: 0
        })
    );
    assert_eq!(io.binds, 1);
    assert_eq!(io.guards.len(), 11);
    assert_eq!(io.reads, 22);
    assert!(io.fields.is_empty());
    no_retry(&mut io, &mut once);
}

#[test]
fn observed_disabled_status_is_preserved_but_every_other_pending_bit_stops() {
    assert_eq!(ALLOWED_DISABLED_STATUS, 0x10080);
    assert_eq!(ALLOWED_DISABLED_STATUS & ENABLED, 0);
    for status in [0, 0x80, 0x10000, 0x10080] {
        let mut io = Fake::new();
        io.status = status;
        let r = run(&mut io, &mut ArmOnce::new());
        assert_eq!(r.error, None);
        assert_eq!(r.completed, COMPLETE);
        assert!(r.bound);
        assert_eq!(
            r.last,
            Some(Snapshot {
                enabled: 12,
                status
            })
        );
        assert_eq!(io.status, status);
        assert_eq!(
            io.writes.iter().map(|s| s.value()).collect::<Vec<_>>(),
            vec![0, 4, 12]
        );
        assert!(io.fields.is_empty());
    }
    for bit in 0..32 {
        if ALLOWED_DISABLED_STATUS & (1 << bit) != 0 {
            continue;
        }
        let mut io = Fake::new();
        io.status = ALLOWED_DISABLED_STATUS | (1 << bit);
        let mut once = ArmOnce::new();
        assert_eq!(run(&mut io, &mut once).error, Some(Error::Pending));
        assert!(io.writes.is_empty());
        assert_eq!(io.binds, 0);
        no_retry(&mut io, &mut once);
    }
}
#[test]
fn nonzero_baseline_enable_or_status_stops_without_writes_or_binding() {
    for (enabled, status) in [(4, 0), (0, 1), (0, 0xc), (0, 0x80000000)] {
        let mut io = Fake::new();
        io.enabled = enabled;
        io.status = status;
        let mut once = ArmOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(
            r.error,
            Some(if enabled != 0 {
                Error::Readback
            } else {
                Error::Pending
            })
        );
        assert!(io.writes.is_empty());
        assert_eq!(io.binds, 0);
        no_retry(&mut io, &mut once);
    }
}
#[test]
fn every_read_and_guard_failure_consumes_attempt() {
    for n in 0..22 {
        let mut io = Fake::new();
        io.fail_read = Some(n);
        let mut once = ArmOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(io.reads, n + 1);
        no_retry(&mut io, &mut once);
    }
    for error in [
        Error::Domain,
        Error::Config,
        Error::Resources,
        Error::Buffers,
    ] {
        for n in 0..11 {
            let mut io = Fake::new();
            io.fail_guard = Some((n, error));
            let mut once = ArmOnce::new();
            let r = run(&mut io, &mut once);
            assert_eq!(r.error, Some(error));
            assert_eq!(io.guards.len(), n + 1);
            assert_eq!(io.reads, n * 2);
            no_retry(&mut io, &mut once);
        }
    }
}
#[test]
fn binding_failure_cannot_enable_microcontroller_interrupts() {
    let mut io = Fake::new();
    io.fail_bind = true;
    let mut once = ArmOnce::new();
    let r = run(&mut io, &mut once);
    assert_eq!(r.error, Some(Error::Binding));
    assert_eq!(io.writes, vec![Step::Disable]);
    assert!(!r.bound);
    no_retry(&mut io, &mut once);
}
#[test]
fn uncertain_or_nonsticking_write_never_advances_or_retries() {
    for n in 0..3 {
        let mut io = Fake::new();
        io.fail_write = Some(n);
        let mut once = ArmOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(r.submitted, (1 << (n + 1)) - 1);
        assert_eq!(r.completed, (1 << n) - 1);
        assert_eq!(io.writes.len(), n + 1);
        no_retry(&mut io, &mut once);
    }
    for n in 1..3 {
        let mut io = Fake::new();
        io.suppress = Some(n);
        let r = run(&mut io, &mut ArmOnce::new());
        assert_eq!(r.error, Some(Error::Readback));
        assert_eq!(io.writes.len(), n + 1);
    }
}
#[test]
fn newly_pending_status_or_unrelated_enable_bit_stops_at_every_sample() {
    for n in (0..22).step_by(2) {
        for pending in [false, true] {
            let mut io = Fake::new();
            if pending {
                io.pending_at = Some(n);
            } else {
                io.changed_enable_at = Some(n);
            }
            let r = run(&mut io, &mut ArmOnce::new());
            assert!(r.error.is_some());
            assert!(io.reads <= n + 4);
            assert!(io.fields.is_empty());
        }
    }
}
#[test]
fn notification_masks_first_and_retains_evidence_without_ack_or_clear() {
    let mut io = Fake::new();
    let mut once = ArmOnce::new();
    assert_eq!(run(&mut io, &mut once).error, None);
    let guards = io.guards.len();
    io.status = 0x8000000c;
    let n = once.notification(&mut io);
    assert_eq!(n.error, Error::Notification);
    assert!(n.mask_submitted);
    assert_eq!(
        n.last,
        Some(Snapshot {
            enabled: 0,
            status: 0x8000000c
        })
    );
    assert_eq!((n.event, n.response), (Some(2), Some(1)));
    assert_eq!(io.fields, vec![Shared::Event, Shared::Response]);
    assert_eq!(io.status, 0x8000000c);
    assert_eq!(io.binds, 1);
    assert_eq!(io.guards.len(), guards);
    assert_eq!(io.writes.last(), Some(&Step::Disable));
    let count = (io.reads, io.writes.len(), io.fields.len());
    assert_eq!(once.notification(&mut io).error, Error::AlreadyStopped);
    assert_eq!(count, (io.reads, io.writes.len(), io.fields.len()));
}
#[test]
fn containment_failures_are_terminal_and_reserved_bytes_are_never_read() {
    for pending in [0, EVENT_MASK, RESPONSE_MASK, ENABLED, 1] {
        let mut io = Fake::new();
        io.status = pending;
        let mut once = ArmOnce::new();
        let n = once.notification(&mut io);
        assert_eq!(n.error, Error::Notification);
        assert_eq!(n.event.is_some(), pending & EVENT_MASK != 0);
        assert_eq!(n.response.is_some(), pending & RESPONSE_MASK != 0);
        assert_eq!(io.binds, 0);
    }
    for failure in 0..5 {
        let mut io = Fake::new();
        io.status = ENABLED;
        match failure {
            0 => io.fail_write = Some(0),
            1 | 2 => io.fail_read = Some(failure - 1),
            3 => io.fail_shared = true,
            _ => {
                io.enabled = 12;
                io.suppress = Some(0);
            }
        }
        let mut once = ArmOnce::new();
        let n = once.notification(&mut io);
        assert!(matches!(n.error, Error::Io | Error::Readback));
        let count = (io.reads, io.writes.len());
        assert_eq!(once.notification(&mut io).error, Error::AlreadyStopped);
        assert_eq!(count, (io.reads, io.writes.len()));
        no_retry(&mut io, &mut once);
    }
}
