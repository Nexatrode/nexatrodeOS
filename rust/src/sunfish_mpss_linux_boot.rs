//! Retained PAS4 loader and the original RMTFS server across reset release.
//! This is an opt-in composition, not an SMP2P-ready or GLINK/WLAN permit.
#![forbid(unsafe_code)]
use crate::{
    qcom_modem_handlers::Fault, sunfish_linux_rmtfs as rmtfs, sunfish_linux_rmtfs_server::Server,
    sunfish_mpss_linux_loader as pas,
};
use core::mem::ManuallyDrop;

/// Trusted serialized reset-lifecycle boundary of the original pool provider.
/// The native implementation requires the actual completed RMTFS assignment
/// and closed AUTH request. This operation itself must not issue PAS/SMC.
pub trait AuthIo: rmtfs::Io {
    fn submit_mpss_auth(&mut self, generation: u64, request: &pas::Request) -> Result<(), Fault>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Loading,
    AuthSubmitted,
    AuthReturned,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Attach,
    Consumed,
    Resource,
    Generation,
    Loader(pas::Error),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub error: Option<Error>,
    pub auth_attempted: bool,
}

/// Owns the actual server, its original pool/native I/O and RAM shadows, plus
/// the loader's metadata/target owner. Nothing is extracted, freed or retried
/// on failure or unwind. The caller retains the actual power/SCM backend and
/// passes its serialized borrow each step; report values cannot replace it.
pub struct Boot<'a, 'image, M: pas::Memory, I: AuthIo> {
    loader: ManuallyDrop<pas::Loader<'image, M>>,
    server: ManuallyDrop<Server<'a, I>>,
    generation: u64,
    report: Report,
    busy: bool,
    receipt: Option<pas::AuthReturned>,
}
impl<'a, 'image, M: pas::Memory, I: AuthIo> Boot<'a, 'image, M, I> {
    pub fn new(generation: u64, loader: pas::Loader<'image, M>, server: Server<'a, I>) -> Self {
        let mut s = Self {
            loader: ManuallyDrop::new(loader),
            server: ManuallyDrop::new(server),
            generation,
            report: Report {
                phase: Phase::Loading,
                error: None,
                auth_attempted: false,
            },
            busy: true,
            receipt: None,
        };
        if generation == 0
            || s.server.configuration().generation != generation
            || s.loader.report().phase != pas::Phase::Metadata
            || s.loader.report().error.is_some()
            || s.server.pre_auth_shared(generation).is_err()
        {
            s.fail(Error::Attach);
        } else {
            s.busy = false;
        }
        s
    }
    fn fail(&mut self, e: Error) -> Error {
        self.report.error.get_or_insert(e);
        self.report.phase = Phase::Failed;
        self.server.fault();
        e
    }
    pub fn report(&self) -> Report {
        let mut r = self.report;
        if self.busy && r.error.is_none() {
            r.phase = Phase::Failed;
            r.error = Some(Error::Consumed);
        }
        r
    }
    pub fn loader_report(&self) -> pas::Report {
        self.loader.report()
    }
    /// External watchdog/domain/lifecycle faults retain and stop both actual
    /// owners. This performs no secure call, reset or ownership reclamation.
    pub fn quarantine(&mut self) {
        self.fail(Error::Resource);
    }
    pub fn step(&mut self, backend: &mut impl pas::Backend) -> Result<pas::Phase, Error> {
        if self.busy || self.report.error.is_some() || self.report.phase != Phase::Loading {
            return Err(self.fail(Error::Consumed));
        }
        self.busy = true;
        let result = self.loader.step(&mut Boundary {
            backend,
            server: &mut self.server,
            generation: self.generation,
            report: &mut self.report,
        });
        let phase = match result {
            Ok(p) => p,
            Err(e) => return Err(self.fail(Error::Loader(e))),
        };
        if phase == pas::Phase::AuthReturned {
            if self.report.phase != Phase::AuthSubmitted || self.server.runtime_shared().is_err() {
                return Err(self.fail(Error::Resource));
            }
            self.receipt = Some(
                self.loader
                    .take_auth_returned()
                    .map_err(|e| self.fail(Error::Loader(e)))?,
            );
            self.report.phase = Phase::AuthReturned;
        } else if self.report.phase != Phase::Loading {
            return Err(self.fail(Error::Consumed));
        }
        self.busy = false;
        Ok(phase)
    }
    /// The actual sealed loader receipt; it is not modem ready or a transport
    /// permission. Keep Boot/its server retained after taking it.
    pub fn take_auth_returned(&mut self) -> Result<pas::AuthReturned, Error> {
        if self.report().phase != Phase::AuthReturned {
            return Err(self.fail(Error::Consumed));
        }
        self.receipt
            .take()
            .ok_or_else(|| self.fail(Error::Consumed))
    }
    /// Service storage immediately after successful AUTH, without waiting for
    /// a global MPSS-ready bit. The transport still validates its own actual
    /// generation, FIFO/IRQ ownership and readiness before dispatch.
    pub fn server(&mut self) -> Result<&mut Server<'a, I>, Error> {
        if self.report().phase != Phase::AuthReturned || self.server.runtime_shared().is_err() {
            return Err(self.fail(Error::Resource));
        }
        Ok(&mut self.server)
    }
}
struct Boundary<'b, 'a, B: pas::Backend, I: AuthIo> {
    backend: &'b mut B,
    server: &'b mut Server<'a, I>,
    generation: u64,
    report: &'b mut Report,
}
impl<B: pas::Backend, I: AuthIo> pas::Backend for Boundary<'_, '_, B, I> {
    type Lease = B::Lease;
    fn guard(&mut self, g: pas::Guard) -> Result<(), pas::Error> {
        if g.generation != self.generation
            || self.report.error.is_some()
            || (self.report.phase == Phase::AuthSubmitted && !g.auth_submitted)
            || match self.report.phase {
                Phase::Loading => self.server.pre_auth_shared(self.generation).is_err(),
                Phase::AuthSubmitted => self.server.runtime_shared().is_err(),
                _ => true,
            }
        {
            return Err(pas::Error::Guard);
        }
        self.backend.guard(g)
    }
    fn acquire(&mut self) -> Result<B::Lease, pas::Error> {
        self.backend.acquire()
    }
    fn release(&mut self, l: B::Lease) -> Result<(), pas::Error> {
        self.backend.release(l)
    }
    fn invoke(&mut self, r: &pas::Request) -> Result<[u64; 8], pas::Error> {
        if self.report.phase != Phase::Loading {
            return Err(pas::Error::Consumed);
        }
        if r.kind() == pas::Kind::AuthAndReset {
            self.report.auth_attempted = true;
            self.report.phase = Phase::AuthSubmitted;
            // Change the actual pool's reset lifecycle BEFORE invoking the
            // delegate, including a failed/uncertain AUTH or monitor unwind.
            self.server
                .submit_mpss_auth(self.generation, r)
                .map_err(|_| pas::Error::Guard)?;
        }
        self.backend.invoke(r)
    }
}
