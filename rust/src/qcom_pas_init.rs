//! One Sunfish PAS4 metadata-init experiment; no memory setup/auth/reset/stop.
//!
//! Source: matched subsys-pil-tz.c::pil_init_image_trusted and scm.c. The caller
//! must exclusively own the SMC capability, keep the five metadata pages alive,
//! and complete the platform experiment and fresh buffer verification first.
//! Address/shape checks do not establish ownership or complete power readiness.
//! This supplies at most two INFO calls and one INIT. There is no BUSY retry,
//! interrupted-call resumption, SMC32 fallback, cleanup call, or time bound on
//! secure world. Any ambiguous result is terminal until physical restart.

use crate::qcom_pas_metadata::{PAGE, Prepared, validate_address};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request([u64; 8]);
impl Request {
    pub const fn words(&self) -> &[u64; 8] {
        &self.0
    }

    // Only platform modules can construct these fixed PAS4/PAS0xf envelopes.
    // No caller supplies an arbitrary PAS ID or operation ID.
    pub(crate) const fn mem_setup_query() -> Self {
        Self([0x42000601, 1, 0x02000202, 0, 0, 0, 0, 0])
    }
    pub(crate) const fn modem_mem_setup() -> Self {
        Self([0x42000202, 3, 4, 0x8b000000, 0x09800000, 0, 0, 0])
    }
    pub(crate) const fn auth_query() -> Self {
        Self([0x42000601, 1, 0x02000205, 0, 0, 0, 0, 0])
    }
    pub(crate) const fn shutdown_query() -> Self {
        Self([0x42000601, 1, 0x02000206, 0, 0, 0, 0, 0])
    }
    pub(crate) const fn modem_auth_reset() -> Self {
        Self([0x42000205, 1, 4, 0, 0, 0, 0, 0])
    }

    /// Fixed IPA-FW PAS0xf command envelopes from the Sunfish generic PIL-TZ
    /// path. The metadata address and destination extent are supplied only
    /// after the IPA-specific planner has validated them.
    pub(crate) const fn ipa_init_query() -> Self {
        Self([0x42000601, 1, 0x02000201, 0, 0, 0, 0, 0])
    }
    pub(crate) const fn ipa_mem_setup_query() -> Self {
        Self([0x42000601, 1, 0x02000202, 0, 0, 0, 0, 0])
    }
    pub(crate) const fn ipa_auth_query() -> Self {
        Self([0x42000601, 1, 0x02000205, 0, 0, 0, 0, 0])
    }
    pub(crate) const fn ipa_init_image(metadata_physical: u64) -> Self {
        Self([0x42000201, 0x82, 0x0f, metadata_physical, 0, 0, 0, 0])
    }
    pub(crate) const fn ipa_mem_setup(physical_base: u64, bytes: u32) -> Self {
        Self([0x42000202, 3, 0x0f, physical_base, bytes as u64, 0, 0, 0])
    }
    pub(crate) const fn ipa_auth_reset() -> Self {
        Self([0x42000205, 1, 0x0f, 0, 0, 0, 0, 0])
    }
}
pub trait Transport {
    /// A nonzero seL4 invocation error is separate from returned SMC registers.
    fn invoke(&mut self, request: &Request) -> Result<[u64; 8], u64>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Convention,
    Availability,
    Checked,
    CallingInit,
    Returned,
    Failed,
}
impl Phase {
    pub const fn code(self) -> u32 {
        self as u32
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::New => "SECURE CHECK START",
            Self::Convention => "CALLING SCM64 QUERY",
            Self::Availability => "CALLING INIT QUERY",
            Self::Checked => "SECURE QUERIES PASS",
            Self::CallingInit => "CALLING PAS INIT",
            Self::Returned => "PAS INIT RETURNED",
            Self::Failed => "SECURE CALL STOPPED",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Metadata,
    WrongStage,
    Kernel,
    Monitor,
    Convention,
    Availability,
    ChangedMetadata,
    Service,
}
impl Error {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub phase: Phase,
    pub issued_mask: u32,
    pub returned_mask: u32,
    pub replies: [Option<[u64; 8]>; 3],
    pub last_request: Option<Request>,
    pub kernel_error: Option<u64>,
    pub error: Option<Error>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.phase == Phase::Returned
            && self.error.is_none()
            && self.issued_mask == 7
            && self.returned_mask == 7
            && self.replies[2].is_some_and(|r| r[0] == 0 && r[1] == 0)
    }
}

pub struct InitOnce {
    prepared: Prepared,
    report: Report,
}
impl InitOnce {
    pub(crate) fn into_completed(self) -> Result<(Prepared, Report), Error> {
        if !self.report.passed() {
            return Err(Error::WrongStage);
        }
        Ok((self.prepared, self.report))
    }

    pub fn new(prepared: Prepared) -> Result<Self, Error> {
        let base = prepared
            .metadata_physical
            .checked_sub(PAGE as u64)
            .ok_or(Error::Metadata)?;
        validate_address(base).map_err(|_| Error::Metadata)?;
        // The build separately pins the installed header's SHA-256. These are
        // fixed experiment geometry checks, not cryptographic authentication.
        if prepared.metadata_length != 9100
            || prepared.program_headers != 31
            || prepared.loadable_segments != 29
            || prepared.linked_start != 0x8b000000
            || prepared.linked_end != 0x94800000
        {
            return Err(Error::Metadata);
        }
        Ok(Self {
            prepared,
            report: Report {
                phase: Phase::New,
                issued_mask: 0,
                returned_mask: 0,
                replies: [None; 3],
                last_request: None,
                kernel_error: None,
                error: None,
            },
        })
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
    fn fail(&mut self, error: Error) -> Error {
        self.report.error.get_or_insert(error);
        self.report.phase = Phase::Failed;
        error
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
        before(&self.report); // Visible before a potentially nonreturning SMC.
        let reply = match io.invoke(&request) {
            Ok(reply) => reply,
            Err(code) => {
                self.report.kernel_error = Some(code);
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
        let r = self.call(
            io,
            0,
            Phase::Convention,
            Request([0xc2000601, 1, 0x82000601, 0, 0, 0, 0, 0]),
            &mut before,
        )?;
        if r[1] != 1 {
            return Err(self.fail(Error::Convention));
        }
        let r = self.call(
            io,
            1,
            Phase::Availability,
            Request([0x42000601, 1, 0x02000201, 0, 0, 0, 0, 0]),
            &mut before,
        )?;
        if r[1] != 1 {
            return Err(self.fail(Error::Availability));
        }
        self.report.phase = Phase::Checked;
        Ok(())
    }
    pub fn initialize(
        &mut self,
        io: &mut impl Transport,
        fresh: Prepared,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Checked {
            return Err(self.fail(Error::WrongStage));
        }
        if fresh != self.prepared {
            return Err(self.fail(Error::ChangedMetadata));
        }
        let r = self.call(
            io,
            2,
            Phase::CallingInit,
            Request([0x42000201, 0x82, 4, fresh.metadata_physical, 0, 0, 0, 0]),
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
mod tests {
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
    #[derive(Default)]
    struct Fake {
        calls: Vec<[u64; 8]>,
        fault: Option<(usize, [u64; 8])>,
        kernel: Option<usize>,
    }
    impl Transport for Fake {
        fn invoke(&mut self, r: &Request) -> Result<[u64; 8], u64> {
            let index = self.calls.len();
            self.calls.push(*r.words());
            assert!(index < 3);
            if self.kernel == Some(index) {
                return Err(6);
            }
            if let Some((at, reply)) = self.fault {
                if index == at {
                    return Ok(reply);
                }
            }
            Ok([0, u64::from(index != 2), 0x1234, 0x5678, 4, 5, 6, 7])
        }
    }
    #[test]
    fn exactly_two_info_calls_and_one_metadata_init_with_raw_replies() {
        let mut f = Fake::default();
        let mut s = InitOnce::new(prepared()).unwrap();
        let mut painted = Vec::new();
        s.check(&mut f, |r| painted.push((r.issued_mask, r.returned_mask)))
            .unwrap();
        s.initialize(&mut f, prepared(), |r| {
            painted.push((r.issued_mask, r.returned_mask))
        })
        .unwrap();
        assert_eq!(painted, [(1, 0), (3, 1), (7, 3)]);
        assert_eq!(
            f.calls,
            [
                [0xc2000601, 1, 0x82000601, 0, 0, 0, 0, 0],
                [0x42000601, 1, 0x02000201, 0, 0, 0, 0, 0],
                [0x42000201, 0x82, 4, 0x80001000, 0, 0, 0, 0]
            ]
        );
        assert!(s.report().passed());
        assert_eq!(
            s.report().replies[2].unwrap()[2..],
            [0x1234, 0x5678, 4, 5, 6, 7]
        );
        assert_eq!(
            s.initialize(&mut f, prepared(), |_| {}),
            Err(Error::WrongStage)
        );
        assert_eq!(f.calls.len(), 3);
    }
    #[test]
    fn interrupted_busy_or_any_nonzero_monitor_status_never_retries_or_resumes() {
        for at in 0..3 {
            for status in [1, (-12i64) as u64, u64::MAX, 1 << 32] {
                let mut f = Fake {
                    fault: Some((at, [status, 99, 2, 3, 4, 5, 6, 7])),
                    ..Fake::default()
                };
                let mut s = InitOnce::new(prepared()).unwrap();
                let result = s
                    .check(&mut f, |_| {})
                    .and_then(|()| s.initialize(&mut f, prepared(), |_| {}));
                assert_eq!(result, Err(Error::Monitor));
                assert_eq!(f.calls.len(), at + 1);
                assert_eq!(s.report().replies[at].unwrap()[0], status);
                assert!(s.initialize(&mut f, prepared(), |_| {}).is_err());
                assert!(s.check(&mut f, |_| {}).is_err());
                assert_eq!(f.calls.len(), at + 1);
            }
        }
    }
    #[test]
    fn unavailable_or_noncanonical_queries_never_invoke_init() {
        for at in 0..2 {
            for value in [0, 2, u64::MAX, (1 << 32) | 1] {
                let mut f = Fake {
                    fault: Some((at, [0, value, 0, 0, 0, 0, 0, 0])),
                    ..Fake::default()
                };
                let mut s = InitOnce::new(prepared()).unwrap();
                assert!(s.check(&mut f, |_| {}).is_err());
                assert!(s.initialize(&mut f, prepared(), |_| {}).is_err());
                assert_eq!(f.calls.len(), at + 1);
            }
        }
    }
    #[test]
    fn service_failure_and_kernel_fault_are_distinct_and_terminal() {
        for at in 0..3 {
            let mut f = Fake {
                kernel: Some(at),
                ..Fake::default()
            };
            let mut s = InitOnce::new(prepared()).unwrap();
            assert_eq!(
                s.check(&mut f, |_| {})
                    .and_then(|()| s.initialize(&mut f, prepared(), |_| {})),
                Err(Error::Kernel)
            );
            assert_eq!(s.report().kernel_error, Some(6));
            assert_eq!(s.report().returned_mask, (1 << at) - 1);
            assert_eq!(s.report().replies[at], None);
        }
        let mut f = Fake {
            fault: Some((2, [0, u64::MAX, 0, 0, 0, 0, 0, 0])),
            ..Fake::default()
        };
        let mut s = InitOnce::new(prepared()).unwrap();
        s.check(&mut f, |_| {}).unwrap();
        assert_eq!(
            s.initialize(&mut f, prepared(), |_| {}),
            Err(Error::Service)
        );
        assert_eq!(s.report().replies[2].unwrap()[1], u64::MAX);
        assert!(!s.report().passed());
    }
    #[test]
    fn rejects_invalid_or_changed_metadata_and_out_of_order_init() {
        for address in [0, 1, 0x80001001, 0x8b001000, (1 << 48), u64::MAX] {
            let mut p = prepared();
            p.metadata_physical = address;
            assert!(matches!(InitOnce::new(p), Err(Error::Metadata)));
        }
        let mut p = prepared();
        p.metadata_length += 1;
        assert!(InitOnce::new(p).is_err());
        let mut f = Fake::default();
        let mut s = InitOnce::new(prepared()).unwrap();
        assert_eq!(
            s.initialize(&mut f, prepared(), |_| {}),
            Err(Error::WrongStage)
        );
        assert!(f.calls.is_empty());
        let mut s = InitOnce::new(prepared()).unwrap();
        s.check(&mut f, |_| {}).unwrap();
        let mut p = prepared();
        p.metadata_physical += PAGE as u64;
        assert_eq!(s.initialize(&mut f, p, |_| {}), Err(Error::ChangedMetadata));
        assert_eq!(f.calls.len(), 2);
    }
}
