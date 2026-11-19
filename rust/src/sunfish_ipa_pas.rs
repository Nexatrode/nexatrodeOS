//! Sunfish IPA-FW PAS0xf wire sequence and legacy downstream call envelope.
//!
//! This composes the already source-matched MDT planner/stager with the PIL-TZ
//! PAS command sequence. It does not provide firmware bytes, a production
//! bandwidth-vote provider, an SMC capability, or physical-memory access. The
//! resource-envelope adapter below enforces which PAS calls must be bracketed
//! by the pinned shared vote; its provider must still implement that vote with
//! the shared platform ledger and live ownership authority.
//! Failures are terminal for this session; callers must not replay a secure
//! operation whose completion is uncertain.

#![forbid(unsafe_code)]

use crate::qcom_pas_init::{Request, Transport as PasScmTransport};
use crate::sunfish_ipa_fws::{self as fws, LoadPlan, SegmentData, StageReport, StagingMemory};

pub const PAS_ID: u32 = fws::PAS_ID;
const PAS_RESPONSE_SLOTS: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataRange {
    pub physical_address: u64,
    pub bytes: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidPlan,
    InvalidMetadata,
    WrongStage,
    MetadataChanged,
    InitUnavailable,
    MemSetupUnavailable,
    AuthUnavailable,
    Bandwidth,
    TransportPolicy,
    Kernel,
    Monitor,
    Service,
    Stage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    New,
    Checking,
    Checked,
    InitPending,
    InitReturned,
    MemSetupPending,
    MemSetupReturned,
    Staging,
    Staged,
    AuthPending,
    AuthReturned,
    Failed,
}

/// Call class shared by the two source-specific envelopes. In the older
/// downstream PIL-TZ envelope, INIT_IMAGE and AUTH_AND_RESET need the shared PAS
/// bandwidth vote; the generic auth path also checks its descriptor's
/// regulator and clock lists. The Sunfish IPA-FW descriptor declares none.
/// MEM_SETUP has no such vote in the pinned PIL-TZ path, and availability
/// queries are read-only SCM calls. The working mainline SCM profile instead
/// brackets all three mutating calls; see `sunfish_ipa_linux_pas`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallKind {
    Availability,
    InitImage,
    MemSetup,
    AuthAndReset,
}

/// Failure from the bounded SMC/resource adapter. Secure-call and vote errors
/// remain separate so diagnostics do not mislabel a failed vote as an SMC
/// invocation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError {
    SecureCall(u64),
    BandwidthAcquire(u64),
    BandwidthRelease(u64),
    SecureAndBandwidthRelease { secure: u64, bandwidth: u64 },
    RequestKindMismatch,
}

impl TransportError {
    const fn secure_code(self) -> Option<u64> {
        match self {
            Self::SecureCall(code) | Self::SecureAndBandwidthRelease { secure: code, .. } => {
                Some(code)
            }
            Self::BandwidthAcquire(_) | Self::BandwidthRelease(_) | Self::RequestKindMismatch => {
                None
            }
        }
    }

    const fn session_error(self) -> Error {
        match self {
            Self::SecureCall(_) | Self::SecureAndBandwidthRelease { .. } => Error::Kernel,
            Self::BandwidthAcquire(_) | Self::BandwidthRelease(_) => Error::Bandwidth,
            Self::RequestKindMismatch => Error::TransportPolicy,
        }
    }
}

/// Downstream PAS bandwidth-vote boundary. A production implementation must
/// add/remove only the IPA-FW client's source-derived Crypto-Core-to-EBI vote
/// in the complete shared ledger, preserving all other clients. The lease is
/// tied to that provider's current owner/generation and can be consumed once.
pub trait PasBandwidthProvider {
    type Lease;

    fn acquire_shared_pas_vote(&mut self) -> Result<Self::Lease, u64>;
    fn release_shared_pas_vote(&mut self, lease: Self::Lease) -> Result<(), u64>;
}

/// Concrete call-envelope adapter around the sealed PAS request codec and the
/// SMC transport. Availability queries and MEM_SETUP are issued directly;
/// INIT_IMAGE and AUTH_AND_RESET always acquire the shared PAS vote first and
/// release that exact lease after the SMC returns, including an SMC error.
/// A failed release is terminal for `StartOnce` and cannot be silently retried.
pub struct ResourceEnvelopeTransport<'a, S, B> {
    secure: &'a mut S,
    bandwidth: &'a mut B,
}

impl<'a, S, B> ResourceEnvelopeTransport<'a, S, B> {
    pub const fn new(secure: &'a mut S, bandwidth: &'a mut B) -> Self {
        Self { secure, bandwidth }
    }
}

pub(crate) fn request_matches(kind: CallKind, request: &Request) -> bool {
    let words = request.words();
    match kind {
        CallKind::Availability => {
            words[0] == 0x4200_0601
                && words[1] == 1
                && matches!(words[2], 0x0200_0201 | 0x0200_0202 | 0x0200_0205)
                && words[3..].iter().all(|word| *word == 0)
        }
        CallKind::InitImage => {
            words[0] == 0x4200_0201
                && words[1] == 0x82
                && words[2] == u64::from(PAS_ID)
                && words[3] != 0
                && words[4..].iter().all(|word| *word == 0)
        }
        CallKind::MemSetup => {
            words[0] == 0x4200_0202
                && words[1] == 3
                && words[2] == u64::from(PAS_ID)
                && words[3] != 0
                && words[4] != 0
                && words[5..].iter().all(|word| *word == 0)
        }
        CallKind::AuthAndReset => {
            words[0] == 0x4200_0205
                && words[1] == 1
                && words[2] == u64::from(PAS_ID)
                && words[3..].iter().all(|word| *word == 0)
        }
    }
}

impl<S, B> IpaPasTransport for ResourceEnvelopeTransport<'_, S, B>
where
    S: PasScmTransport,
    B: PasBandwidthProvider,
{
    fn invoke(&mut self, kind: CallKind, request: &Request) -> Result<[u64; 8], TransportError> {
        if !request_matches(kind, request) {
            return Err(TransportError::RequestKindMismatch);
        }
        if !matches!(kind, CallKind::InitImage | CallKind::AuthAndReset) {
            return self
                .secure
                .invoke(request)
                .map_err(TransportError::SecureCall);
        }

        let lease = self
            .bandwidth
            .acquire_shared_pas_vote()
            .map_err(TransportError::BandwidthAcquire)?;
        let secure_result = self.secure.invoke(request);
        let release_result = self.bandwidth.release_shared_pas_vote(lease);
        match (secure_result, release_result) {
            (Ok(reply), Ok(())) => Ok(reply),
            (Err(error), Ok(())) => Err(TransportError::SecureCall(error)),
            (Ok(_), Err(error)) => Err(TransportError::BandwidthRelease(error)),
            (Err(secure), Err(bandwidth)) => {
                Err(TransportError::SecureAndBandwidthRelease { secure, bandwidth })
            }
        }
    }
}

pub trait IpaPasTransport {
    /// Invoke the fixed wire request once with the selected source profile's
    /// resource envelope. `ResourceEnvelopeTransport` preserves the older
    /// downstream PIL-TZ behavior. The native Linux-reference loader requires
    /// `sunfish_ipa_linux_pas::LinuxTransport`, including SCM resources around
    /// MEM_SETUP. A generic wire-sequence receipt selects neither profile.
    fn invoke(&mut self, kind: CallKind, request: &Request) -> Result<[u64; 8], TransportError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub phase: Phase,
    pub issued_mask: u32,
    pub returned_mask: u32,
    pub replies: [Option<[u64; 8]>; PAS_RESPONSE_SLOTS],
    pub last_request: Option<Request>,
    pub stage_report: Option<StageReport>,
    pub kernel_error: Option<u64>,
    pub transport_error: Option<TransportError>,
    pub error: Option<Error>,
}

impl Report {
    pub fn passed(&self) -> bool {
        let expected_mask = if self.replies[4].is_some() {
            0x3f
        } else {
            0x2d
        };
        self.phase == Phase::AuthReturned
            && self.error.is_none()
            && self.issued_mask == expected_mask
            && self.returned_mask == expected_mask
            && self.stage_report.is_some()
            && self.replies[0].is_some_and(|reply| reply[0] == 0 && reply[1] == 1)
            && (expected_mask != 0x3f
                || self.replies[1].is_some_and(|reply| reply[0] == 0 && reply[1] == 1))
            && self.replies[2].is_some_and(|reply| reply[0] == 0 && reply[1] == 1)
            && self.replies[3].is_some_and(|reply| reply[0] == 0 && reply[1] == 0)
            && self.replies[5].is_some_and(|reply| reply[0] == 0 && reply[1] == 0)
    }
}

/// One non-retryable PIL-TZ start attempt for the Sunfish IPA firmware PAS.
pub struct StartOnce {
    plan: LoadPlan,
    metadata: MetadataRange,
    report: Report,
}

impl StartOnce {
    pub fn new(plan: LoadPlan, metadata: MetadataRange) -> Result<Self, Error> {
        if plan.pas_id != PAS_ID
            || plan.physical_base != fws::REGION_BASE
            || plan.region_bytes != fws::REGION_BYTES
            || plan.placement_count() == 0
            || metadata.physical_address == 0
            || metadata.physical_address & 0xfff != 0
            || metadata.bytes == 0
        {
            return Err(
                if metadata.bytes == 0
                    || metadata.physical_address == 0
                    || metadata.physical_address & 0xfff != 0
                {
                    Error::InvalidMetadata
                } else {
                    Error::InvalidPlan
                },
            );
        }
        let metadata_end = metadata
            .physical_address
            .checked_add(u64::from(metadata.bytes))
            .ok_or(Error::InvalidMetadata)?;
        let ipa_end = fws::REGION_BASE
            .checked_add(fws::REGION_BYTES)
            .ok_or(Error::InvalidPlan)?;
        if metadata.physical_address < ipa_end && fws::REGION_BASE < metadata_end {
            return Err(Error::InvalidMetadata);
        }
        if let Some(setup) = plan.mem_setup {
            let setup_end = setup
                .physical_base
                .checked_add(u64::from(setup.bytes))
                .ok_or(Error::InvalidPlan)?;
            if setup.pas_id != PAS_ID
                || setup.physical_base != fws::REGION_BASE
                || setup.bytes == 0
                || setup_end > ipa_end
            {
                return Err(Error::InvalidPlan);
            }
        }
        Ok(Self {
            plan,
            metadata,
            report: Report {
                phase: Phase::New,
                issued_mask: 0,
                returned_mask: 0,
                replies: [None; PAS_RESPONSE_SLOTS],
                last_request: None,
                stage_report: None,
                kernel_error: None,
                transport_error: None,
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
        io: &mut impl IpaPasTransport,
        slot: usize,
        kind: CallKind,
        phase: Phase,
        request: Request,
        before: &mut impl FnMut(&Report),
    ) -> Result<[u64; 8], Error> {
        self.report.phase = phase;
        self.report.last_request = Some(request);
        self.report.issued_mask |= 1 << slot;
        before(&self.report);
        let reply = match io.invoke(kind, &request) {
            Ok(reply) => reply,
            Err(error) => {
                self.report.kernel_error = error.secure_code();
                self.report.transport_error = Some(error);
                return Err(self.fail(error.session_error()));
            }
        };
        self.report.returned_mask |= 1 << slot;
        self.report.replies[slot] = Some(reply);
        if reply[0] != 0 {
            return Err(self.fail(Error::Monitor));
        }
        Ok(reply)
    }

    /// Check the source-defined PAS operation availability before any firmware
    /// metadata is submitted. The MEM_SETUP query is omitted for fixed MDTs.
    pub fn check(
        &mut self,
        io: &mut impl IpaPasTransport,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::New {
            return Err(self.fail(Error::WrongStage));
        }
        let mut operations = [
            (0, Request::ipa_init_query(), Error::InitUnavailable),
            (
                1,
                Request::ipa_mem_setup_query(),
                Error::MemSetupUnavailable,
            ),
            (2, Request::ipa_auth_query(), Error::AuthUnavailable),
        ];
        let count = if self.plan.mem_setup.is_some() { 3 } else { 2 };
        if self.plan.mem_setup.is_none() {
            operations[1] = operations[2];
        }
        for (slot, request, unavailable) in operations.into_iter().take(count) {
            let reply = self.call(
                io,
                slot,
                CallKind::Availability,
                Phase::Checking,
                request,
                &mut before,
            )?;
            if reply[1] != 1 {
                return Err(self.fail(unavailable));
            }
        }
        self.report.phase = Phase::Checked;
        Ok(())
    }

    /// Issue exactly one PAS_INIT_IMAGE after confirming the metadata buffer
    /// still occupies the preflighted physical range.
    pub fn initialize(
        &mut self,
        io: &mut impl IpaPasTransport,
        fresh_metadata: MetadataRange,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Checked {
            return Err(self.fail(Error::WrongStage));
        }
        if fresh_metadata != self.metadata {
            return Err(self.fail(Error::MetadataChanged));
        }
        let reply = self.call(
            io,
            3,
            CallKind::InitImage,
            Phase::InitPending,
            Request::ipa_init_image(self.metadata.physical_address),
            &mut before,
        )?;
        if reply[1] != 0 {
            return Err(self.fail(Error::Service));
        }
        self.report.phase = Phase::InitReturned;
        Ok(())
    }

    /// Issue Linux's relocatable-image PAS_MEM_SETUP before writing any
    /// segments. Fixed-address MDTs skip this operation.
    pub fn setup_memory(
        &mut self,
        io: &mut impl IpaPasTransport,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::InitReturned {
            return Err(self.fail(Error::WrongStage));
        }
        let Some(setup) = self.plan.mem_setup else {
            return Err(self.fail(Error::WrongStage));
        };
        let reply = self.call(
            io,
            4,
            CallKind::MemSetup,
            Phase::MemSetupPending,
            Request::ipa_mem_setup(setup.physical_base, setup.bytes),
            &mut before,
        )?;
        if reply[1] != 0 {
            return Err(self.fail(Error::Service));
        }
        self.report.phase = Phase::MemSetupReturned;
        Ok(())
    }

    /// Stage only after PAS_INIT_IMAGE and (for relocatable MDTs) MEM_SETUP.
    /// The staging routine preflights every segment before the first write.
    pub fn stage(
        &mut self,
        files: &[SegmentData<'_>],
        memory: &mut impl StagingMemory,
    ) -> Result<StageReport, Error> {
        let stage_allowed = (self.report.phase == Phase::InitReturned
            && self.plan.mem_setup.is_none())
            || (self.report.phase == Phase::MemSetupReturned && self.plan.mem_setup.is_some());
        if !stage_allowed {
            return Err(self.fail(Error::WrongStage));
        }
        self.report.phase = Phase::Staging;
        let report = fws::stage(&self.plan, files, memory).map_err(|_| self.fail(Error::Stage))?;
        self.report.stage_report = Some(report);
        self.report.phase = Phase::Staged;
        Ok(report)
    }

    /// Authenticate and release IPA firmware reset once, only after every
    /// planned segment was staged. Any uncertain completion is terminal.
    pub fn authenticate(
        &mut self,
        io: &mut impl IpaPasTransport,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Staged || self.report.stage_report.is_none() {
            return Err(self.fail(Error::WrongStage));
        }
        let reply = self.call(
            io,
            5,
            CallKind::AuthAndReset,
            Phase::AuthPending,
            Request::ipa_auth_reset(),
            &mut before,
        )?;
        if reply[1] != 0 {
            return Err(self.fail(Error::Service));
        }
        self.report.phase = Phase::AuthReturned;
        Ok(())
    }
}
