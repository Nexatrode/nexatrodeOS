#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
mod sunfish_ipa_linux_config;
use sunfish_ipa_linux_config::*;

struct Fake {
    state: Snapshot,
    reads: usize,
    writes: Vec<(Register, u32)>,
    fail_read: Option<usize>,
    fail_write: Option<usize>,
    suppress_write: Option<usize>,
    change_read: Option<(usize, Read, u32)>,
}
impl Fake {
    fn new() -> Self {
        Self {
            state: Snapshot {
                config: [
                    0xdead0001, 0xabcdffab, 0x24, 0x00123456, 0x47, 0x12340008, 0x80, 0x1111,
                    0x10000,
                ],
                guards: [0x08090811, 0x415, 0, 0, 0, 0xe4046b59],
            },
            reads: 0,
            writes: vec![],
            fail_read: None,
            fail_write: None,
            suppress_write: None,
            change_read: None,
        }
    }
    fn word(&mut self, r: Read) -> &mut u32 {
        match r {
            Read::Config(r) => &mut self.state.config[r as usize],
            Read::Flavor => &mut self.state.guards[0],
            Read::SharedMemory => &mut self.state.guards[1],
            Read::Aggregation => &mut self.state.guards[2],
            Read::IrqEnable => &mut self.state.guards[3],
            Read::GsiStatus => &mut self.state.guards[4],
            Read::GsiParam2 => &mut self.state.guards[5],
        }
    }
}
impl Io for Fake {
    fn read(&mut self, r: Read) -> Result<u32, Error> {
        let n = self.reads;
        self.reads += 1;
        if let Some((at, register, value)) = self.change_read {
            if n == at {
                *self.word(register) = value;
            }
        }
        if self.fail_read == Some(n) {
            Err(Error::Io)
        } else {
            Ok(*self.word(r))
        }
    }
    fn write(&mut self, w: &Write) -> Result<(), Error> {
        let n = self.writes.len();
        self.writes.push((w.register(), w.value()));
        if self.suppress_write != Some(n) {
            *self.word(Read::Config(w.register())) = w.value();
        }
        if self.fail_write == Some(n) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}

#[test]
fn uc_irq_comparison_changes_only_the_closed_interrupt_guard() {
    for enabled in [0, 4, 12] {
        let mut io = Fake::new();
        let expected = io.state;
        io.state.guards[3] = enabled;
        assert_eq!(verify_retained_uc_irq(&mut io, expected, enabled), Ok(()));
        assert!(io.writes.is_empty());
        assert_eq!(io.reads, 30);
        // Every other captured word remains part of both full comparisons.
        for n in 0..30 {
            let mut io = Fake::new();
            io.state.guards[3] = enabled;
            io.fail_read = Some(n);
            assert_eq!(
                verify_retained_uc_irq(&mut io, expected, enabled),
                Err(Error::Io)
            );
            assert_eq!(io.reads, n + 1);
            assert!(io.writes.is_empty());
        }
        let mut io = Fake::new();
        io.state.guards[3] = enabled ^ 0x4000;
        assert_eq!(
            verify_retained_uc_irq(&mut io, expected, enabled),
            Err(Error::Changed)
        );
        assert!(io.writes.is_empty());
    }
    for enabled in [1, 2, 8, 0x4000, u32::MAX] {
        let mut io = Fake::new();
        let expected = io.state;
        assert_eq!(
            verify_retained_uc_irq(&mut io, expected, enabled),
            Err(Error::ActiveHardware)
        );
        assert_eq!(io.reads, 0);
        assert!(io.writes.is_empty());
    }
    let mut io = Fake::new();
    io.state.guards[3] = 12;
    let forged = io.state;
    assert_eq!(
        verify_retained_uc_irq(&mut io, forged, 12),
        Err(Error::ActiveHardware)
    );
    assert_eq!(io.reads, 0);
}
fn run(io: &mut Fake, once: &mut ConfigureOnce) -> Report {
    once.run(io, true, |_, _| {})
}
#[test]
fn post_auth_comparison_accepts_only_gsi_enabled_and_the_existing_uc_mask() {
    let cold = Fake::new().state;
    let mut io = Fake::new();
    io.state.guards[3] = 12;
    io.state.guards[4] = 1;
    verify_retained_after_auth(&mut io, cold).unwrap();
    assert_eq!(io.reads, 30);
    assert!(io.writes.is_empty());
    for n in 0..30 {
        let mut io = Fake::new();
        io.state.guards[3] = 12;
        io.state.guards[4] = 1;
        io.fail_read = Some(n);
        assert_eq!(verify_retained_after_auth(&mut io, cold), Err(Error::Io));
        assert!(io.writes.is_empty());
    }
    for word in 0..15 {
        let mut io = Fake::new();
        io.state.guards[3] = 12;
        io.state.guards[4] = 1;
        if word < 9 {
            io.state.config[word] ^= 1;
        } else {
            io.state.guards[word - 9] ^= 1;
        }
        assert_eq!(
            verify_retained_after_auth(&mut io, cold),
            Err(Error::Changed)
        );
        assert!(io.writes.is_empty());
    }
    for status in [0, 2, 3, 0x80000001] {
        let mut io = Fake::new();
        io.state.guards[3] = 12;
        io.state.guards[4] = status;
        assert_eq!(
            verify_retained_after_auth(&mut io, cold),
            Err(Error::Changed)
        );
    }
    let mut io = Fake::new();
    io.state.guards[4] = 1;
    let forged = io.state;
    assert_eq!(
        verify_retained_after_auth(&mut io, forged),
        Err(Error::ActiveHardware)
    );
}

#[test]
fn exact_linux_order_values_preserve_only_two_rmw_registers() {
    let mut io = Fake::new();
    let before = io.state;
    let mut once = ConfigureOnce::new();
    let r = run(&mut io, &mut once);
    assert_eq!(r.error, None);
    assert_eq!((r.submitted, r.completed), (COMPLETE, COMPLETE));
    // Independent hand-derived Linux values, including 500us*32000/1e6-1=15.
    let values = [0, 0xabcdefab, 0x30000000, 0x0012b5d6, 8, 12, 0xf0, 0, 256];
    assert_eq!(
        io.writes
            .iter()
            .map(|(r, v)| (r.offset(), *v))
            .collect::<Vec<_>>(),
        [0x1d0, 0x1fc, 0x44, 0x3c, 0x74, 0x78, 0x1f0, 0x148, 0x240]
            .into_iter()
            .zip(values)
            .collect::<Vec<_>>()
    );
    assert_eq!(r.before, Some(before));
    assert_eq!(r.after.unwrap().config, values);
    assert_eq!(io.state.guards, before.guards);
    assert_eq!(r.failed, None);
    for s in PROFILE {
        assert!(!s.register.label().is_empty() && s.register.label().len() <= 15);
    }
    assert!(Read::GsiStatus.is_gsi() && Read::GsiParam2.is_gsi());
    assert!(!Read::IrqEnable.is_gsi());
    assert_eq!(Read::IrqEnable.offset(), 0x300c);
    assert_eq!(Read::GsiStatus.offset(), 0x1f000);
    assert_eq!(Read::Config(Register::Bcr).offset(), 0x1d0);
}

#[test]
fn incomplete_power_attempt_is_consumed_without_mmio() {
    let mut io = Fake::new();
    let mut once = ConfigureOnce::new();
    assert_eq!(
        once.run(&mut io, false, |_, _| {}).error,
        Some(Error::PowerNotComplete)
    );
    assert_eq!(io.reads, 0);
    assert!(io.writes.is_empty());
    assert_eq!(run(&mut io, &mut once).error, Some(Error::AlreadyStarted));
}

#[test]
fn wrong_hardware_and_active_gsi_irq_or_aggregation_stop_before_writes() {
    for guard in 0..6 {
        let mut io = Fake::new();
        io.state.guards[guard] ^= 1;
        let expected = if [0, 1, 5].contains(&guard) {
            Error::UnexpectedHardware
        } else {
            Error::ActiveHardware
        };
        assert_eq!(
            run(&mut io, &mut ConfigureOnce::new()).error,
            Some(expected)
        );
        assert!(io.writes.is_empty());
    }
}

#[test]
fn baseline_must_be_equal_before_any_write() {
    let mut io = Fake::new();
    io.change_read = Some((15, Read::Config(Register::Idle), 0));
    assert_eq!(
        run(&mut io, &mut ConfigureOnce::new()).error,
        Some(Error::BaselineChanged)
    );
    assert!(io.writes.is_empty());
}

#[test]
fn every_read_failure_ends_attempt_and_cannot_retry() {
    let mut success = Fake::new();
    run(&mut success, &mut ConfigureOnce::new());
    for n in 0..success.reads {
        let mut io = Fake::new();
        io.fail_read = Some(n);
        let mut once = ConfigureOnce::new();
        let report = run(&mut io, &mut once);
        assert_eq!(report.error, Some(Error::Io));
        assert_eq!(io.reads, n + 1);
        let writes = io.writes.len();
        let reads = io.reads;
        assert_eq!(run(&mut io, &mut once).error, Some(Error::AlreadyStarted));
        assert_eq!((io.writes.len(), io.reads), (writes, reads));
    }
}

#[test]
fn uncertain_write_is_counted_and_never_replayed() {
    for n in 0..COUNT {
        let mut io = Fake::new();
        io.fail_write = Some(n);
        let mut once = ConfigureOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(io.writes.len(), n + 1);
        assert_eq!(r.submitted, (1 << (n + 1)) - 1);
        assert_eq!(r.completed, (1 << n) - 1);
        assert_eq!(r.failed, Some(PROFILE[n].register));
        assert_eq!(run(&mut io, &mut once).error, Some(Error::AlreadyStarted));
        assert_eq!(io.writes.len(), n + 1);
    }
}

#[test]
fn non_sticking_write_stops_without_next_register() {
    for n in 0..COUNT {
        let mut io = Fake::new();
        io.suppress_write = Some(n);
        let r = run(&mut io, &mut ConfigureOnce::new());
        assert_eq!(r.error, Some(Error::Readback));
        assert_eq!(io.writes.len(), n + 1);
        assert_eq!(r.completed, (1 << n) - 1);
    }
}

#[test]
fn concurrent_config_or_guard_change_between_writes_stops() {
    for register in [
        Read::Config(Register::Bcr),
        Read::GsiStatus,
        Read::IrqEnable,
        Read::Flavor,
    ] {
        let mut io = Fake::new();
        // Baselines 30 reads; first pre-write+readback 30 reads. Mutate at
        // the start of the next pre-write capture, after exactly one write.
        io.change_read = Some((60, register, 0x55));
        let r = run(&mut io, &mut ConfigureOnce::new());
        assert_eq!(r.error, Some(Error::Changed));
        assert_eq!(io.writes.len(), 1);
        assert_eq!(r.completed, 1);
    }
}

#[test]
fn retained_check_reads_twice_without_replaying_writes() {
    let mut io = Fake::new();
    let expected = run(&mut io, &mut ConfigureOnce::new()).after.unwrap();
    let (reads, writes) = (io.reads, io.writes.len());
    assert_eq!(verify_retained(&mut io, expected), Ok(()));
    assert_eq!((io.reads - reads, io.writes.len()), (30, writes));
    for field in 0..15 {
        let mut changed = Fake::new();
        changed.state = expected;
        if field < COUNT {
            changed.state.config[field] ^= 1;
        } else {
            changed.state.guards[field - COUNT] ^= 1;
        }
        assert_eq!(verify_retained(&mut changed, expected), Err(Error::Changed));
        assert!(changed.writes.is_empty());
    }
}

#[test]
fn retained_check_catches_second_sample_change_and_each_io_failure() {
    let mut io = Fake::new();
    let expected = run(&mut io, &mut ConfigureOnce::new()).after.unwrap();
    let mut changed = Fake::new();
    changed.state = expected;
    changed.change_read = Some((15, Read::GsiStatus, 1));
    assert_eq!(verify_retained(&mut changed, expected), Err(Error::Changed));
    assert!(changed.writes.is_empty());
    for n in 0..30 {
        let mut failed = Fake::new();
        failed.state = expected;
        failed.fail_read = Some(n);
        assert_eq!(verify_retained(&mut failed, expected), Err(Error::Io));
        assert_eq!(failed.reads, n + 1);
        assert!(failed.writes.is_empty());
    }
}
