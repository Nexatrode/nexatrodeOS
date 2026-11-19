//! One fixed PAS4 AUTH_AND_RESET after complete image staging.
//! This experimental secure-call result is not a modem-ready event or DMA
//! ownership proof. There is no retry, interrupted-call resumption, shutdown
//! executor, shared-ring initialization or WLAN operation. Any failure or
//! ambiguous completion requires a full platform restart with memory/votes held.
use crate::qcom_modem_image::{ImageOnce, Report as ImageReport};
use crate::qcom_pas_init::{Request, Transport};
use crate::qcom_pas_mem_setup::observed_consumed_pattern;
use crate::qcom_pas_metadata::{Inspection, Prepared};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Image,
    Metadata,
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
    QueryAuth,
    QueryShutdown,
    Available,
    Calling,
    Returned,
    Failed,
}
impl Phase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::New => "AUTH GATES CHECKED",
            Self::QueryAuth => "CALLING AUTH QUERY",
            Self::QueryShutdown => "CALLING STOP QUERY",
            Self::Available => "AUTH QUERIES PASS",
            Self::Calling => "CALLING AUTH RESET",
            Self::Returned => "AUTH RETURN PASS",
            Self::Failed => "AUTH CALL STOPPED",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub image: ImageReport,
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
        self.image.passed()
            && self.phase == Phase::Returned
            && self.error.is_none()
            && self.issued_mask == 7
            && self.returned_mask == 7
            && self.replies[0].is_some_and(|r| r[0] == 0 && r[1] == 1)
            && self.replies[1].is_some_and(|r| r[0] == 0 && r[1] == 1)
            && self.replies[2].is_some_and(|r| r[0] == 0 && r[1] == 0)
    }
}
/// Opaque, non-copyable permit for the isolated pre-AUTH AP status publisher.
/// It can be issued only after a real completed image and both INFO replies.
pub struct StatusPermit {
    _private: (),
}
/// Consumes the pre-AUTH session for the standalone fresh-memory experiment.
/// It cannot be turned back into AuthOnce or used to start firmware/transport.
pub struct GlinkMemoryPermit {
    _private: (),
}
pub struct AuthOnce {
    prepared: Prepared,
    report: Report,
    status_permit_issued: bool,
    glink_memory_issued: bool,
}
impl AuthOnce {
    /// Consume the actual completed image session. Caller-supplied reports or
    /// success booleans cannot authorize the operation. All mappings and held
    /// votes must stay alive, and the caller must provide fresh metadata reads.
    pub fn after_image(
        image: ImageOnce<'_>,
        prepared: Prepared,
        fresh: &Inspection,
    ) -> Result<Self, Error> {
        let image = image.into_completed().map_err(|_| Error::Image)?;
        if !observed_consumed_pattern(fresh, prepared) {
            return Err(Error::Metadata);
        }
        Ok(Self {
            prepared,
            status_permit_issued: false,
            glink_memory_issued: false,
            report: Report {
                image,
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
    pub fn into_returned(self) -> Result<Report, Error> {
        if !self.report.passed() {
            return Err(Error::WrongStage);
        }
        Ok(self.report)
    }
    pub fn status_prepare(&mut self, fresh: &Inspection) -> Result<StatusPermit, Error> {
        if self.report.phase != Phase::Available || self.status_permit_issued {
            return Err(self.fail(Error::WrongStage));
        }
        if !observed_consumed_pattern(fresh, self.prepared) {
            return Err(self.fail(Error::Metadata));
        }
        self.status_permit_issued = true;
        Ok(StatusPermit { _private: () })
    }
    pub fn into_status_returned(self) -> Result<Report, Error> {
        if !self.status_permit_issued {
            return Err(Error::WrongStage);
        }
        self.into_returned()
    }
    pub fn into_glink_memory_preparation(
        mut self,
        fresh: &Inspection,
    ) -> Result<GlinkMemoryPermit, Error> {
        self.glink_boot_memory_prepare(fresh)
    }
    /// Low-level grant for the private experimental boot wrapper. That wrapper
    /// owns this session throughout preparation and rejects AUTH if actual
    /// preparation/revalidation fails. This is not a transport/epoch capability.
    pub fn glink_boot_memory_prepare(
        &mut self,
        fresh: &Inspection,
    ) -> Result<GlinkMemoryPermit, Error> {
        if self.report.phase != Phase::Available
            || !self.status_permit_issued
            || self.glink_memory_issued
        {
            return Err(self.fail(Error::WrongStage));
        }
        if !observed_consumed_pattern(fresh, self.prepared) {
            return Err(self.fail(Error::Metadata));
        }
        self.glink_memory_issued = true;
        Ok(GlinkMemoryPermit { _private: () })
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
        for (index, phase, request) in [
            (0, Phase::QueryAuth, Request::auth_query()),
            (1, Phase::QueryShutdown, Request::shutdown_query()),
        ] {
            let r = self.call(io, index, phase, request, &mut before)?;
            if r[1] != 1 {
                return Err(self.fail(Error::Availability));
            }
        }
        self.report.phase = Phase::Available;
        Ok(())
    }
    pub fn authenticate(
        &mut self,
        io: &mut impl Transport,
        fresh: &Inspection,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Available {
            return Err(self.fail(Error::WrongStage));
        }
        if !observed_consumed_pattern(fresh, self.prepared) {
            return Err(self.fail(Error::Metadata));
        }
        let r = self.call(
            io,
            2,
            Phase::Calling,
            Request::modem_auth_reset(),
            &mut before,
        )?;
        if r[1] != 0 {
            return Err(self.fail(Error::Service));
        }
        self.report.phase = Phase::Returned;
        Ok(())
    }
}
