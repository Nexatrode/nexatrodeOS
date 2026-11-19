#[path = "../rust/src/sunfish_ipa_linux_resources.rs"]
mod sunfish_ipa_linux_resources;
use sunfish_ipa_linux_resources::*;

struct Fake {
    words: [u32; COUNT],
    reads: usize,
    guards: usize,
    writes: Vec<(usize, u32)>,
    fail_read: Option<usize>,
    fail_write: Option<usize>,
    suppress: Option<usize>,
    fail_guard: Option<(usize, Error)>,
    change: Option<(usize, usize)>,
}
impl Fake {
    fn new() -> Self {
        Self {
            words: [0x12345678; COUNT],
            reads: 0,
            guards: 0,
            writes: vec![],
            fail_read: None,
            fail_write: None,
            suppress: None,
            fail_guard: None,
            change: None,
        }
    }
}
impl Io for Fake {
    fn guard(&mut self) -> Result<(), Error> {
        let n = self.guards;
        self.guards += 1;
        if let Some((at, error)) = self.fail_guard {
            if n == at {
                return Err(error);
            }
        }
        Ok(())
    }
    fn read(&mut self, register: Register) -> Result<u32, Error> {
        let n = self.reads;
        self.reads += 1;
        if let Some((at, word)) = self.change {
            if n == at {
                self.words[word] ^= 1;
            }
        }
        if self.fail_read == Some(n) {
            Err(Error::Io)
        } else {
            Ok(self.words[register as usize])
        }
    }
    fn write(&mut self, request: &Write) -> Result<(), Error> {
        let n = self.writes.len();
        self.writes
            .push((request.register().offset(), request.value()));
        if self.suppress != Some(n) {
            self.words[request.register() as usize] = request.value();
        }
        if self.fail_write == Some(n) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
fn run(io: &mut Fake, once: &mut ConfigureOnce) -> Report {
    once.run(io, 0x08090811, |_, _| {})
}
fn no_retry(io: &mut Fake, once: &mut ConfigureOnce) {
    let before = (io.reads, io.guards, io.writes.len());
    assert_eq!(run(io, once).error, Some(Error::AlreadyStarted));
    assert_eq!(before, (io.reads, io.guards, io.writes.len()));
}

#[test]
fn independent_linux_fixture_and_complete_readbacks() {
    let mut io = Fake::new();
    let mut once = ConfigureOnce::new();
    let r = run(&mut io, &mut once);
    assert_eq!(COMPLETE, 0x7f);
    assert_eq!(Error::Io.code(), 6);
    assert_eq!(r.error, None);
    assert_eq!((r.submitted, r.completed), (COMPLETE, COMPLETE));
    assert_eq!(
        io.writes,
        vec![
            (0x400, 0x3f03),
            (0x420, 0x303),
            (0x440, 0xa0a),
            (0x460, 0x101),
            (0x480, 0x505),
            (0x500, 0x303),
            (0x520, 0x3f01)
        ]
    );
    assert_eq!(r.before, Some([0x12345678; 7]));
    assert_eq!(
        r.after,
        Some([0x3f03, 0x303, 0xa0a, 0x101, 0x505, 0x303, 0x3f01])
    );
    assert_eq!(r.failed, None);
    assert_eq!(io.guards, 18);
    assert_eq!(io.reads, 126);
    assert_eq!(
        r.topology,
        Some(Topology {
            tx_count: 8,
            rx_count: 9,
            rx_base: 8,
            available: 0x1ffff,
            defined: 0xf72
        })
    );
    for spec in PROFILE {
        assert!(spec.register.label().len() <= 15);
    }
    no_retry(&mut io, &mut once);
}
#[test]
fn topology_rejects_missing_endpoints_wrong_direction_and_overflow_without_io() {
    // Too few TX endpoints; too few RX endpoints; shifted RX window; >32 limit.
    for flavor in [0x08090611, 0x08030811, 0x0a090811, 0x0f1f0811, 0] {
        assert_eq!(topology(flavor), Err(Error::Topology));
        let mut io = Fake::new();
        let mut once = ConfigureOnce::new();
        assert_eq!(
            once.run(&mut io, flavor, |_, _| {}).error,
            Some(Error::Topology)
        );
        assert_eq!((io.reads, io.guards, io.writes.len()), (0, 0, 0));
        no_retry(&mut io, &mut once);
    }
    // A 32-bit available set must not overflow its mask calculation.
    assert_eq!(topology(0x08180811).unwrap().available, 0xffffffff);
}
#[test]
fn every_register_read_failure_consumes_attempt_without_extra_io() {
    for n in 0..126 {
        let mut io = Fake::new();
        io.fail_read = Some(n);
        let mut once = ConfigureOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(io.reads, n + 1);
        no_retry(&mut io, &mut once);
    }
}
#[test]
fn uncertain_writes_are_recorded_and_never_replayed() {
    for n in 0..7 {
        let mut io = Fake::new();
        io.fail_write = Some(n);
        let mut once = ConfigureOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(r.submitted, (1 << (n + 1)) - 1);
        assert_eq!(r.completed, (1 << n) - 1);
        assert_eq!(io.writes.len(), n + 1);
        assert_eq!(r.failed, Some(PROFILE[n].register));
        no_retry(&mut io, &mut once);
    }
}
#[test]
fn nonsticking_write_stops_before_next_register() {
    for n in 0..7 {
        let mut io = Fake::new();
        io.suppress = Some(n);
        let r = run(&mut io, &mut ConfigureOnce::new());
        assert_eq!(r.error, Some(Error::Readback));
        assert_eq!(r.completed, (1 << n) - 1);
        assert_eq!(io.writes.len(), n + 1);
    }
}
#[test]
fn every_domain_configuration_or_buffer_guard_failure_stops() {
    for error in [Error::Domain, Error::Config, Error::Buffers] {
        for n in 0..18 {
            let mut io = Fake::new();
            io.fail_guard = Some((n, error));
            let mut once = ConfigureOnce::new();
            let r = run(&mut io, &mut once);
            assert_eq!(r.error, Some(error));
            assert_eq!(io.guards, n + 1);
            assert_eq!(io.reads, n * 7);
            no_retry(&mut io, &mut once);
        }
    }
}
#[test]
fn changes_to_any_earlier_or_future_register_are_detected_in_every_sample() {
    // Both baseline equality and pre/post-write/final complete snapshots matter.
    for sample in 1..18 {
        for index in 0..7 {
            let mut io = Fake::new();
            io.change = Some((sample * 7, index));
            let r = run(&mut io, &mut ConfigureOnce::new());
            assert_eq!(
                r.error,
                Some(if sample == 1 {
                    Error::BaselineChanged
                } else if sample < 16 && sample % 2 == 0 {
                    Error::Changed
                } else {
                    Error::Readback
                })
            );
            let expected_writes = if sample < 2 {
                0
            } else {
                ((sample - 1) / 2).min(7)
            };
            assert_eq!(io.writes.len(), expected_writes);
        }
    }
}
