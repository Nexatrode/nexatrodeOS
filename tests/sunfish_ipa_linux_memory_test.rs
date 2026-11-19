#[allow(dead_code)]
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
mod sunfish_ipa_linux_config;
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
mod sunfish_ipa_linux_memory;
use sunfish_ipa_linux_config as config;
use sunfish_ipa_linux_memory::*;

struct Fake {
    expected: config::Snapshot,
    sram: [u32; SHARED_BYTES / 4],
    writes: Vec<CanaryWord>,
    reads: usize,
    guards: usize,
    domains: usize,
    fail_write: Option<usize>,
    suppress_write: Option<usize>,
    fail_read: Option<usize>,
    fail_guard: Option<usize>,
    fail_domain: Option<usize>,
    change_guard: Option<usize>,
    change_canary: Option<usize>,
}
impl Fake {
    fn new() -> Self {
        Self {
            expected: config::Snapshot {
                config: [0, 0x400, 0x30000000, 0xf81f8, 8, 12, 0xf0, 0, 0x100],
                guards: [0x08090811, 0x415, 0, 0, 0, 0xe4046b59],
            },
            sram: core::array::from_fn(|i| 0xa5000000 | i as u32),
            writes: vec![],
            reads: 0,
            guards: 0,
            domains: 0,
            fail_write: None,
            suppress_write: None,
            fail_read: None,
            fail_guard: None,
            fail_domain: None,
            change_guard: None,
            change_canary: None,
        }
    }
}
impl config::Io for Fake {
    fn read(&mut self, r: config::Read) -> Result<u32, config::Error> {
        let n = self.guards;
        self.guards += 1;
        if self.fail_guard == Some(n) {
            return Err(config::Error::Io);
        }
        if self.change_guard == Some(n) {
            self.expected.guards[4] = 1;
        }
        Ok(match r {
            config::Read::Config(r) => self.expected.config[r as usize],
            config::Read::Flavor => self.expected.guards[0],
            config::Read::SharedMemory => self.expected.guards[1],
            config::Read::Aggregation => self.expected.guards[2],
            config::Read::IrqEnable => self.expected.guards[3],
            config::Read::GsiStatus => self.expected.guards[4],
            config::Read::GsiParam2 => self.expected.guards[5],
        })
    }
    fn write(&mut self, _: &config::Write) -> Result<(), config::Error> {
        panic!("canary stage must not replay primitive configuration")
    }
}
impl Io for Fake {
    fn domain_quiet(&mut self) -> Result<(), Error> {
        let n = self.domains;
        self.domains += 1;
        if self.fail_domain == Some(n) {
            Err(Error::Domain)
        } else {
            Ok(())
        }
    }
    fn read_canary(&mut self, word: CanaryWord) -> Result<u32, Error> {
        let n = self.reads;
        self.reads += 1;
        if self.change_canary == Some(n) {
            self.sram[words()[0].offset() / 4] = 0;
        }
        if self.fail_read == Some(n) {
            Err(Error::Io)
        } else {
            Ok(self.sram[word.offset() / 4])
        }
    }
    fn write_canary(&mut self, word: CanaryWord) -> Result<(), Error> {
        let n = self.writes.len();
        self.writes.push(word);
        if self.suppress_write != Some(n) {
            self.sram[word.offset() / 4] = word.value();
        }
        // An I/O failure can be reported after a write reached hardware.
        if self.fail_write == Some(n) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
fn run(io: &mut Fake, once: &mut CanariesOnce) -> Report {
    let expected = io.expected;
    once.run(io, expected, |_| {})
}
fn assert_consumed(io: &mut Fake, once: &mut CanariesOnce) {
    let counts = (io.reads, io.guards, io.domains, io.writes.len());
    assert_eq!(run(io, once).error, Some(Error::AlreadyStarted));
    assert_eq!((io.reads, io.guards, io.domains, io.writes.len()), counts);
}

#[test]
fn exact_linux_canary_order_and_only_canary_words_change() {
    let mut io = Fake::new();
    let before = io.sram;
    let mut once = CanariesOnce::new();
    let r = run(&mut io, &mut once);
    assert_eq!(r.error, None);
    assert_eq!(
        (r.submitted, r.completed, r.verified_words),
        (COMPLETE, COMPLETE, 25)
    );
    // Independent offsets, including Linux's descending order within pairs.
    let offsets = [
        0x284, 0x280, 0x28c, 0x288, 0x30c, 0x308, 0x314, 0x310, 0x394, 0x390, 0x39c, 0x398, 0x41c,
        0x418, 0x424, 0x420, 0x4a4, 0x4a0, 0x5ec, 0x5e8, 0x9f4, 0x9f0, 0xa4c, 0xa48, 0x1ffc,
    ];
    assert_eq!(
        io.writes.iter().map(|w| w.offset()).collect::<Vec<_>>(),
        offsets
    );
    for (index, word) in io.writes.iter().enumerate() {
        assert_eq!(word.index(), index);
    }
    for (i, value) in io.sram.into_iter().enumerate() {
        assert_eq!(
            value,
            if offsets.contains(&(i * 4)) {
                0xdeadbeef
            } else {
                before[i]
            }
        );
    }
    assert_eq!((io.reads, io.domains, io.guards), (75, 27, 810));
    assert_eq!(r.failed, None);
    assert_consumed(&mut io, &mut once);
}

#[test]
fn measured_size_is_capped_to_dt_and_unknown_geometry_never_touches_io() {
    assert_eq!(SHARED_PHYSICAL, 0x01e47000);
    assert_eq!(
        geometry(0x415),
        Ok(Geometry {
            offset: 0,
            reported_bytes: 0x20a8,
            usable_bytes: 0x2000
        })
    );
    for raw in [0, 0x400, 0x416, 0x10415, u32::MAX] {
        let mut io = Fake::new();
        io.expected.guards[1] = raw;
        let mut once = CanariesOnce::new();
        assert_eq!(run(&mut io, &mut once).error, Some(Error::Geometry));
        assert_eq!(
            (io.reads, io.guards, io.domains, io.writes.len()),
            (0, 0, 0, 0)
        );
        assert_consumed(&mut io, &mut once);
    }
}

#[test]
fn canaries_do_not_overlap_any_region_payload_or_aperture_end() {
    for word in words() {
        assert!(word.offset() % 4 == 0 && word.offset() + 4 <= SHARED_BYTES);
        for region in REGIONS.into_iter().chain([END_MARKER]) {
            assert!(!region.name.is_empty());
            assert!(
                word.offset() + 4 <= region.offset || word.offset() >= region.offset + region.size
            );
        }
    }
    assert_eq!(Error::Readback.code(), 6);
}

#[test]
fn faulted_domain_stops_without_replay_at_each_boundary() {
    for n in 0..27 {
        let mut io = Fake::new();
        io.fail_domain = Some(n);
        let mut once = CanariesOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Domain));
        assert_eq!(io.writes.len(), n.min(25));
        assert_consumed(&mut io, &mut once);
    }
}

#[test]
fn every_uncertain_write_is_counted_and_never_retried() {
    for n in 0..WORD_COUNT {
        let mut io = Fake::new();
        io.fail_write = Some(n);
        let mut once = CanariesOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(io.writes.len(), n + 1);
        assert_eq!(
            (r.submitted, r.completed),
            ((1 << (n + 1)) - 1, (1 << n) - 1)
        );
        assert_eq!(r.failed.unwrap().index(), n);
        assert_consumed(&mut io, &mut once);
    }
}

#[test]
fn every_failed_canary_read_stops_and_retains_partial_state() {
    for n in 0..75 {
        let mut io = Fake::new();
        io.fail_read = Some(n);
        let mut once = CanariesOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Io));
        assert_eq!(io.reads, n + 1);
        assert_eq!(io.writes.len(), (n + 1).min(25));
        assert_eq!(r.verified_words, 0);
        assert_consumed(&mut io, &mut once);
    }
}

#[test]
fn every_non_sticking_write_stops_before_next_word() {
    for n in 0..WORD_COUNT {
        let mut io = Fake::new();
        io.suppress_write = Some(n);
        let mut once = CanariesOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Readback));
        assert_eq!(io.writes.len(), n + 1);
        assert_eq!(r.completed, (1 << n) - 1);
        assert_consumed(&mut io, &mut once);
    }
}

#[test]
fn corruption_after_last_write_is_caught_in_either_full_sample() {
    for n in [25, 50] {
        let mut io = Fake::new();
        io.change_canary = Some(n);
        let mut once = CanariesOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Readback));
        assert_eq!(r.completed, COMPLETE);
        assert_eq!(r.verified_words, 0);
        assert_eq!(io.writes.len(), 25);
        assert_consumed(&mut io, &mut once);
    }
}

#[test]
fn every_configuration_io_failure_stops_without_additional_writes() {
    for n in 0..810 {
        let mut io = Fake::new();
        io.fail_guard = Some(n);
        let mut once = CanariesOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Config(config::Error::Io)));
        assert_eq!(io.guards, n + 1);
        assert_eq!(io.writes.len(), (n / 30).min(25));
        assert_consumed(&mut io, &mut once);
    }
}

#[test]
fn changed_gsi_status_at_each_guard_boundary_stops_without_repair() {
    for n in (0..810).step_by(30) {
        let mut io = Fake::new();
        io.change_guard = Some(n);
        let mut once = CanariesOnce::new();
        let r = run(&mut io, &mut once);
        assert_eq!(r.error, Some(Error::Config(config::Error::Changed)));
        assert_eq!(io.writes.len(), (n / 30).min(25));
        assert_consumed(&mut io, &mut once);
    }
}
