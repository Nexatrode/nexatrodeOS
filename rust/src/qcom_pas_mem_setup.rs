//! One fixed Sunfish PAS4 MEM_SETUP after a successful, consumed metadata INIT.
//!
//! The captured MDT and merged DT agree on [0x8b000000,0x94800000). The build
//! must exclude that entire reservation from every AP allocation and mapping.
//! It is never CPU-mapped here. This checks only returned statuses; no segment
//! loader, authentication/reset, shutdown, DMA ownership proof or retry exists.
//! Consumed metadata is not parsed/reused as an image. Its observed all-zero
//! contents and preserved guards are an explicit experimental postcondition,
//! not documentation of the secure world's implementation or write policy.

use crate::qcom_pas_init::{InitOnce, Report as InitReport, Request, Transport};
use crate::qcom_pas_metadata::{Inspection, Prepared};

pub const MODEM_BASE: u64 = 0x8b000000;
pub const MODEM_BYTES: u64 = 0x09800000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InitNotPassed,
    ConsumedPattern,
    WrongStage,
    Kernel,
    Monitor,
    Availability,
    Service,
}
impl Error {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Query,
    Available,
    Calling,
    Returned,
    Failed,
}
impl Phase {
    pub const fn code(self) -> u32 {
        self as u32
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::New => "CHECK CONSUMED INIT",
            Self::Query => "CALLING MEM QUERY",
            Self::Available => "MEM QUERY PASS",
            Self::Calling => "CALLING MEM SETUP",
            Self::Returned => "MEM SETUP RETURNED",
            Self::Failed => "MEM SETUP STOPPED",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub init: InitReport,
    pub phase: Phase,
    pub issued_mask: u32,
    pub returned_mask: u32,
    pub replies: [Option<[u64; 8]>; 2],
    pub last_request: Option<Request>,
    pub kernel_error: Option<u64>,
    pub error: Option<Error>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.init.passed()
            && self.phase == Phase::Returned
            && self.error.is_none()
            && self.issued_mask == 3
            && self.returned_mask == 3
            && self.replies[1].is_some_and(|r| r[0] == 0 && r[1] == 0)
    }
}

/// Match the exact bounded zeroing observation, without accepting an arbitrary
/// modified ELF or trusting a CRC alone. `payload_zero == length` is an exact
/// byte count from the snapshot; stability is a second exact allocation read.
pub fn observed_consumed_pattern(r: &Inspection, prepared: Prepared) -> bool {
    prepared.metadata_length == 9100
        && prepared.program_headers == 31
        && prepared.loadable_segments == 29
        && prepared.linked_start as u64 == MODEM_BASE
        && prepared.linked_end as u64 == MODEM_BASE + MODEM_BYTES
        && r.low_guard_bad == 0
        && r.high_guard_bad == 0
        && r.padding_bad == 0
        && r.unstable_bytes == 0
        && r.payload_zero == 9100
        && r.payload_diff == 8171
        && r.first_diff == Some(0)
        && r.last_diff == Some(0x238b)
        && r.expected_words == [0x464c457f, 0x00010101]
        && r.actual_words == [0, 0]
        && r.input_crc32 == 0xb19e8d4e
        && r.actual_crc32 == 0x7511eaf4
        && r.actual_parse_error == 0x101
}

pub struct MemSetupOnce {
    prepared: Prepared,
    report: Report,
}
impl MemSetupOnce {
    pub(crate) fn into_completed(self) -> Result<Report, Error> {
        if !self.report.passed() {
            return Err(Error::WrongStage);
        }
        Ok(self.report)
    }
    /// Consumes the successful INIT session; does not reconstruct readiness
    /// from a caller-supplied success boolean or a copied diagnostic report.
    pub fn after_init(init: InitOnce, consumed: &Inspection) -> Result<Self, Error> {
        let (prepared, init) = init.into_completed().map_err(|_| Error::InitNotPassed)?;
        if !observed_consumed_pattern(consumed, prepared) {
            return Err(Error::ConsumedPattern);
        }
        Ok(Self {
            prepared,
            report: Report {
                init,
                phase: Phase::New,
                issued_mask: 0,
                returned_mask: 0,
                replies: [None; 2],
                last_request: None,
                kernel_error: None,
                error: None,
            },
        })
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
    fn fail(&mut self, e: Error) -> Error {
        self.report.error.get_or_insert(e);
        self.report.phase = Phase::Failed;
        e
    }
    fn call(
        &mut self,
        io: &mut impl Transport,
        index: usize,
        phase: Phase,
        request: Request,
        before: &mut impl FnMut(&Report),
    ) -> Result<[u64; 8], Error> {
        self.report.phase = phase;
        self.report.last_request = Some(request);
        self.report.issued_mask |= 1 << index;
        before(&self.report);
        let reply = match io.invoke(&request) {
            Ok(r) => r,
            Err(e) => {
                self.report.kernel_error = Some(e);
                return Err(self.fail(Error::Kernel));
            }
        };
        self.report.returned_mask |= 1 << index;
        self.report.replies[index] = Some(reply);
        if reply[0] != 0 {
            return Err(self.fail(Error::Monitor));
        }
        Ok(reply)
    }
    pub fn check(
        &mut self,
        io: &mut impl Transport,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(self.fail(Error::WrongStage));
        }
        let r = self.call(io, 0, Phase::Query, Request::mem_setup_query(), &mut before)?;
        if r[1] != 1 {
            return Err(self.fail(Error::Availability));
        }
        self.report.phase = Phase::Available;
        Ok(())
    }
    pub fn setup(
        &mut self,
        io: &mut impl Transport,
        fresh: &Inspection,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Available {
            return Err(self.fail(Error::WrongStage));
        }
        if !observed_consumed_pattern(fresh, self.prepared) {
            return Err(self.fail(Error::ConsumedPattern));
        }
        let r = self.call(
            io,
            1,
            Phase::Calling,
            Request::modem_mem_setup(),
            &mut before,
        )?;
        if r[1] != 0 {
            return Err(self.fail(Error::Service));
        }
        self.report.phase = Phase::Returned;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::vec::Vec;
    fn prepared() -> Prepared {
        Prepared {
            metadata_physical: 0x80001000,
            metadata_length: 9100,
            program_headers: 31,
            loadable_segments: 29,
            linked_start: 0x8b000000,
            linked_end: 0x94800000,
        }
    }
    pub(crate) fn consumed() -> Inspection {
        Inspection {
            low_guard_bad: 0,
            high_guard_bad: 0,
            padding_bad: 0,
            payload_diff: 8171,
            payload_zero: 9100,
            first_diff: Some(0),
            last_diff: Some(0x238b),
            expected_words: [0x464c457f, 0x00010101],
            actual_words: [0, 0],
            input_crc32: 0xb19e8d4e,
            actual_crc32: 0x7511eaf4,
            actual_parse_error: 0x101,
            unstable_bytes: 0,
        }
    }
    #[derive(Default)]
    struct Fake {
        calls: Vec<[u64; 8]>,
        fault: Option<(usize, Result<[u64; 8], u64>)>,
    }
    impl Transport for Fake {
        fn invoke(&mut self, r: &Request) -> Result<[u64; 8], u64> {
            let n = self.calls.len();
            self.calls.push(*r.words());
            assert!(n < 5);
            if let Some((at, reply)) = self.fault {
                if n == at {
                    return reply;
                }
            }
            Ok([
                0,
                u64::from(matches!(n, 0 | 1 | 3)),
                0x1234,
                0x5678,
                4,
                5,
                6,
                7,
            ])
        }
    }
    fn init(f: &mut Fake) -> InitOnce {
        let mut s = InitOnce::new(prepared()).unwrap();
        s.check(f, |_| {}).unwrap();
        s.initialize(f, prepared(), |_| {}).unwrap();
        s
    }
    pub(crate) fn completed_setup() -> MemSetupOnce {
        let mut f = Fake::default();
        let s = init(&mut f);
        let mut mem = MemSetupOnce::after_init(s, &consumed()).unwrap();
        mem.check(&mut f, |_| {}).unwrap();
        mem.setup(&mut f, &consumed(), |_| {}).unwrap();
        mem
    }
    #[test]
    fn exact_five_call_lifecycle_stops_after_fixed_mem_setup() {
        let mut f = Fake::default();
        let s = init(&mut f);
        let mut m = MemSetupOnce::after_init(s, &consumed()).unwrap();
        let mut masks = Vec::new();
        m.check(&mut f, |r| masks.push((r.issued_mask, r.returned_mask)))
            .unwrap();
        m.setup(&mut f, &consumed(), |r| {
            masks.push((r.issued_mask, r.returned_mask))
        })
        .unwrap();
        assert_eq!(masks, [(1, 0), (3, 1)]);
        assert!(m.report().passed());
        assert_eq!(f.calls[3], [0x42000601, 1, 0x02000202, 0, 0, 0, 0, 0]);
        assert_eq!(
            f.calls[4],
            [0x42000202, 3, 4, 0x8b000000, 0x09800000, 0, 0, 0]
        );
        assert_eq!(
            m.report().replies[1].unwrap()[2..],
            [0x1234, 0x5678, 4, 5, 6, 7]
        );
        assert!(m.setup(&mut f, &consumed(), |_| {}).is_err());
        assert!(m.check(&mut f, |_| {}).is_err());
        assert_eq!(f.calls.len(), 5);
    }
    #[test]
    fn init_must_have_returned_success_before_the_transition() {
        assert!(matches!(
            MemSetupOnce::after_init(InitOnce::new(prepared()).unwrap(), &consumed()),
            Err(Error::InitNotPassed)
        ));
        let mut f = Fake {
            fault: Some((2, Ok([0, 1, 0, 0, 0, 0, 0, 0]))),
            ..Fake::default()
        };
        let mut s = InitOnce::new(prepared()).unwrap();
        s.check(&mut f, |_| {}).unwrap();
        assert!(s.initialize(&mut f, prepared(), |_| {}).is_err());
        assert!(matches!(
            MemSetupOnce::after_init(s, &consumed()),
            Err(Error::InitNotPassed)
        ));
        assert_eq!(f.calls.len(), 3);
    }
    #[test]
    fn setup_without_the_fresh_availability_query_never_calls_secure_world() {
        let mut f = Fake::default();
        let s = init(&mut f);
        let mut m = MemSetupOnce::after_init(s, &consumed()).unwrap();
        assert_eq!(m.setup(&mut f, &consumed(), |_| {}), Err(Error::WrongStage));
        assert_eq!(f.calls.len(), 3);
        assert!(m.check(&mut f, |_| {}).is_err());
        assert_eq!(f.calls.len(), 3);
    }
    #[test]
    fn every_unexpected_buffer_observation_blocks_transition_and_fresh_setup() {
        let base = consumed();
        let mut variants = Vec::new();
        for n in 0..14 {
            let mut r = base;
            match n {
                0 => r.low_guard_bad = 1,
                1 => r.high_guard_bad = 1,
                2 => r.padding_bad = 1,
                3 => r.payload_diff -= 1,
                4 => r.payload_zero -= 1,
                5 => r.first_diff = Some(1),
                6 => r.last_diff = None,
                7 => r.expected_words[0] ^= 1,
                8 => r.expected_words[1] ^= 1,
                9 => r.actual_words[0] = 1,
                10 => r.input_crc32 ^= 1,
                11 => r.actual_crc32 ^= 1,
                12 => r.actual_parse_error = 0,
                _ => r.unstable_bytes = 1,
            }
            variants.push(r);
        }
        for bad in variants {
            let mut f = Fake::default();
            let s = init(&mut f);
            assert!(matches!(
                MemSetupOnce::after_init(s, &bad),
                Err(Error::ConsumedPattern)
            ));
            assert_eq!(f.calls.len(), 3);
            let mut f = Fake::default();
            let s = init(&mut f);
            let mut m = MemSetupOnce::after_init(s, &base).unwrap();
            m.check(&mut f, |_| {}).unwrap();
            assert_eq!(m.setup(&mut f, &bad, |_| {}), Err(Error::ConsumedPattern));
            assert_eq!(f.calls.len(), 4);
        }
    }
    #[test]
    fn query_or_setup_transport_kernel_and_service_errors_are_terminal() {
        for at in 3..5 {
            for reply in [
                Err(6),
                Ok([1, 0, 2, 3, 4, 5, 6, 7]),
                Ok([(-12i64) as u64, 0, 2, 3, 4, 5, 6, 7]),
                Ok([u64::MAX, 0, 2, 3, 4, 5, 6, 7]),
            ] {
                let mut f = Fake {
                    fault: Some((at, reply)),
                    ..Fake::default()
                };
                let s = init(&mut f);
                let mut m = MemSetupOnce::after_init(s, &consumed()).unwrap();
                assert!(
                    m.check(&mut f, |_| {})
                        .and_then(|()| m.setup(&mut f, &consumed(), |_| {}))
                        .is_err()
                );
                assert!(!m.report().passed());
                assert!(m.setup(&mut f, &consumed(), |_| {}).is_err());
                assert_eq!(f.calls.len(), at + 1);
                if let Ok(r) = reply {
                    assert_eq!(m.report().replies[at - 3], Some(r));
                } else {
                    assert_eq!(m.report().kernel_error, Some(6));
                    assert_eq!(m.report().replies[at - 3], None);
                }
            }
        }
        for value in [0, 2, u64::MAX, (1 << 32) | 1] {
            let mut f = Fake {
                fault: Some((3, Ok([0, value, 0, 0, 0, 0, 0, 0]))),
                ..Fake::default()
            };
            let s = init(&mut f);
            let mut m = MemSetupOnce::after_init(s, &consumed()).unwrap();
            assert_eq!(m.check(&mut f, |_| {}), Err(Error::Availability));
            assert_eq!(f.calls.len(), 4);
        }
        let mut f = Fake {
            fault: Some((4, Ok([0, u64::MAX, 0, 0, 0, 0, 0, 0]))),
            ..Fake::default()
        };
        let s = init(&mut f);
        let mut m = MemSetupOnce::after_init(s, &consumed()).unwrap();
        m.check(&mut f, |_| {}).unwrap();
        assert_eq!(m.setup(&mut f, &consumed(), |_| {}), Err(Error::Service));
        assert_eq!(f.calls.len(), 5);
    }
}
