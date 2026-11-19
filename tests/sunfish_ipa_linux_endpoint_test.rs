#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
pub mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
pub mod sunfish_ipa_linux_gsi;
mod fixture {
    include!(env!("SUNFISH_ENDPOINT_GSI_FIXTURE"));
}

use std::collections::BTreeMap;
use sunfish_ipa_linux_endpoint::*;
use sunfish_ipa_linux_gsi as gsi;

struct Fake {
    values: BTreeMap<(bool, usize), u32>,
    writes: Vec<Word>,
    reads: usize,
    guards: usize,
    contains: usize,
    fail_read: Option<usize>,
    fail_guard: Option<usize>,
    fail_write: Option<usize>,
    suppress_write: Option<usize>,
    fail_contain: bool,
    reserved: bool,
    after_write: Option<(usize, (bool, usize), u32)>,
}
impl Fake {
    fn new() -> Self {
        let mut values = BTreeMap::new();
        for (r, v) in [
            (Read::GsiStatus, 1),
            (Read::TypeMask, gsi::BASE_IRQ_TYPES),
            (Read::GlobalEnable, 1),
            (Read::GeneralEnable, 14),
        ] {
            values.insert((r.is_gsi(), r.offset()), v);
        }
        for e in ENDPOINTS {
            for r in [Read::Channel(e), Read::Event(e)] {
                values.insert((true, r.offset()), 1 << 20);
            }
        }
        for w in PROFILE {
            values.insert((false, w.offset()), 0xa5a5a5a5);
        }
        Self {
            values,
            writes: vec![],
            reads: 0,
            guards: 0,
            contains: 0,
            fail_read: None,
            fail_guard: None,
            fail_write: None,
            suppress_write: None,
            fail_contain: false,
            reserved: false,
            after_write: None,
        }
    }
    fn counts(&self) -> (usize, usize, usize, usize) {
        (self.reads, self.writes.len(), self.guards, self.contains)
    }
}
impl Io for Fake {
    fn guard(&mut self) -> Result<(), Error> {
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
        let mut v = self
            .values
            .get(&(r.is_gsi(), r.offset()))
            .copied()
            .unwrap_or(0);
        if let Read::Endpoint(w) = r {
            if self.reserved {
                v |= !w.register.mask();
            }
        }
        Ok(v)
    }
    fn write(&mut self, r: &Request) -> Result<(), Error> {
        let n = self.writes.len();
        let w = r.word();
        self.writes.push(w);
        if self.suppress_write != Some(n) {
            self.values.insert((false, w.offset()), w.value);
        }
        if let Some((index, key, value)) = self.after_write {
            if index == n {
                self.values.insert(key, value);
                self.after_write = None;
            }
        }
        if self.fail_write == Some(n) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn contain(&mut self) -> Result<(), Error> {
        self.contains += 1;
        if self.fail_contain {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
fn terminal(once: &mut ConfigureOnce, setup: &gsi::SetupOnce, io: &mut Fake) {
    let before = io.counts();
    assert_eq!(once.execute(setup, io), Err(Error::Consumed));
    assert_eq!(io.counts(), before);
}
#[test]
fn epcfg_profile_matches_actual_compiled_linux_setup_and_abi() {
    let text =
        std::fs::read_to_string(std::env::var("SUNFISH_ENDPOINT_C_REFERENCE").unwrap()).unwrap();
    let c: Vec<_> = text
        .lines()
        .map(|line| {
            let mut p = line.split_whitespace();
            (
                usize::from_str_radix(p.next().unwrap(), 16).unwrap(),
                u32::from_str_radix(p.next().unwrap(), 16).unwrap(),
                u32::from_str_radix(p.next().unwrap(), 16).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        PROFILE
            .map(|w| (w.offset(), w.value, w.register.mask()))
            .as_slice(),
        c
    );
    assert_eq!(AGGREGATION_KB, 6);
    assert_eq!(
        PROFILE.map(|w| w.endpoint.id()),
        [
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 8, 8, 8, 8, 8, 9,
            9, 9, 9, 9, 9, 9, 9
        ]
    );
    assert!(
        PROFILE
            .iter()
            .all(|w| w.offset() < 4096 && w.value & !w.register.mask() == 0)
    );
}
#[test]
fn epcfg_full_configuration_needs_owned_complete_gsi_and_retains_success() {
    let setup = fixture::completed_setup();
    let mut io = Fake::new();
    let mut once = ConfigureOnce::new();
    once.execute(&setup, &mut io).unwrap();
    let r = once.report();
    assert_eq!(r.phase, Phase::Complete);
    assert_eq!(
        (r.writes_sent, r.writes_done, r.readback),
        (COMPLETE, COMPLETE, COMPLETE)
    );
    assert_eq!((r.endpoints_done, r.final_samples), (15, 2));
    assert_eq!(io.writes, PROFILE);
    assert_eq!(io.contains, 0);
    assert!(r.error.is_none() && !r.containment_sent);
    terminal(&mut once, &setup, &mut io);
    let setup = fixture::incomplete_setup();
    let mut io = Fake::new();
    let mut once = ConfigureOnce::new();
    assert_eq!(once.execute(&setup, &mut io), Err(Error::GsiNotComplete));
    assert_eq!(io.counts(), (0, 0, 0, 0));
    terminal(&mut once, &setup, &mut io);
}
#[test]
fn epcfg_every_read_guard_and_uncertain_write_failure_stops_once_without_replay() {
    let setup = fixture::completed_setup();
    let mut good = Fake::new();
    ConfigureOnce::new().execute(&setup, &mut good).unwrap();
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
            let mut once = ConfigureOnce::new();
            let e = if class == 1 { Error::Guard } else { Error::Io };
            assert_eq!(once.execute(&setup, &mut io), Err(e));
            let r = once.report();
            assert_eq!(r.error, Some(e));
            assert_eq!(r.phase, Phase::Failed);
            assert!(r.containment_sent);
            assert_eq!(io.contains, 1);
            assert_eq!(io.writes.as_slice(), &PROFILE[..io.writes.len()]);
            if class == 2 {
                assert_eq!(r.writes_sent, (1 << (n + 1)) - 1);
                assert_eq!(r.writes_done, (1 << n) - 1);
                assert_eq!(io.values[&(false, PROFILE[n].offset())], PROFILE[n].value);
            }
            terminal(&mut once, &setup, &mut io);
        }
    }
}
#[test]
fn epcfg_all_non_sticking_writes_and_defined_field_corruption_fail_readback() {
    let setup = fixture::completed_setup();
    for n in 0..COUNT {
        for suppress in [true, false] {
            let mut io = Fake::new();
            let word = PROFILE[n];
            if suppress {
                io.suppress_write = Some(n);
            } else {
                let bit = 1 << word.register.mask().trailing_zeros();
                io.after_write = Some((n, (false, word.offset()), word.value ^ bit));
            }
            let mut once = ConfigureOnce::new();
            assert_eq!(once.execute(&setup, &mut io), Err(Error::Readback));
            assert_eq!(once.report().writes_done, (1 << (n + 1)) - 1);
            assert_eq!(once.report().readback, (1 << n) - 1);
            assert_eq!(io.contains, 1);
            terminal(&mut once, &setup, &mut io);
        }
    }
}
#[test]
fn epcfg_ignores_reserved_readback_bits_and_overwrites_arbitrary_preimages() {
    let setup = fixture::completed_setup();
    let mut io = Fake::new();
    io.reserved = true;
    let mut once = ConfigureOnce::new();
    once.execute(&setup, &mut io).unwrap();
    assert_eq!(io.writes, PROFILE);
    for w in PROFILE {
        assert_eq!(io.values[&(false, w.offset())], w.value);
    }
    assert_eq!(io.contains, 0);
}
#[test]
fn epcfg_pending_sources_and_changed_allocated_states_are_preserved_without_writes() {
    let setup = fixture::completed_setup();
    let mut cases = vec![
        (Read::GsiStatus, 0, Error::State),
        (Read::TypeMask, 0, Error::State),
        (Read::GlobalEnable, 3, Error::State),
        (Read::ChannelMask, 1, Error::State),
        (Read::EventMask, 1, Error::State),
        (Read::GeneralEnable, 0, Error::State),
    ];
    for r in [
        Read::TypeStatus,
        Read::GlobalStatus,
        Read::GeneralStatus,
        Read::ChannelStatus,
        Read::EventStatus,
    ] {
        cases.push((r, 1, Error::Pending));
    }
    for e in ENDPOINTS {
        for r in [Read::Channel(e), Read::Event(e)] {
            cases.push((r, 2 << 20, Error::State));
        }
    }
    for (r, v, e) in cases {
        let mut io = Fake::new();
        io.values.insert((r.is_gsi(), r.offset()), v);
        let mut once = ConfigureOnce::new();
        assert_eq!(once.execute(&setup, &mut io), Err(e));
        assert!(io.writes.is_empty());
        assert_eq!(io.values[&(r.is_gsi(), r.offset())], v);
        assert_eq!(io.contains, 1);
        terminal(&mut once, &setup, &mut io);
    }
    // A new pending error after a store stops before that word is accepted.
    let mut io = Fake::new();
    io.after_write = Some((0, (true, gsi::GLOBAL_STATUS), 1));
    let mut once = ConfigureOnce::new();
    assert_eq!(once.execute(&setup, &mut io), Err(Error::Pending));
    assert_eq!(io.writes.len(), 1);
    assert_eq!(once.report().readback, 0);
    assert_eq!(io.values[&(true, gsi::GLOBAL_STATUS)], 1);
}
#[test]
fn epcfg_containment_error_preserves_primary_failure_and_no_second_attempt() {
    let setup = fixture::completed_setup();
    let mut io = Fake::new();
    io.fail_guard = Some(0);
    io.fail_contain = true;
    let mut once = ConfigureOnce::new();
    assert_eq!(once.execute(&setup, &mut io), Err(Error::Guard));
    assert_eq!(once.report().containment_error, Some(Error::Io));
    assert_eq!(io.contains, 1);
    terminal(&mut once, &setup, &mut io);
}
