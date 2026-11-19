//! Sole retained UC/suspend/GSI dispatcher after actual receive startup.
//! No reset, pool extraction or interrupt retry API.
#![forbid(unsafe_code)]
use crate::{
    sunfish_ipa_linux_buffers::{Memory, Pool},
    sunfish_ipa_linux_irq as irq,
    sunfish_ipa_linux_receive_pool::DataPool,
    sunfish_ipa_linux_receive_runtime::{self as rx, Consumer},
    sunfish_ipa_linux_receive_start::{self as start, GuardState},
    sunfish_ipa_linux_uc as uc,
};

/// The concrete native handler must classify the delivered badge. Any
/// foreign/co-delivered fault takes priority over either legitimate source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Notice {
    Ipa,
    Gsi,
    Both,
    Fault,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Startup,
    Quarantined,
    Fault,
    Ipa(uc::Error),
    Receive(rx::Error),
}
pub trait Io<R: Memory, D: Memory>: rx::Io<R, D> {
    fn ipa_read(&mut self, register: irq::Read) -> Result<u32, uc::Error>;
    fn ipa_write(&mut self, request: &uc::Write) -> Result<(), uc::Error>;
    fn uc_byte(&mut self, field: irq::Shared) -> Result<u8, uc::Error>;
    fn suspend_info(&mut self) -> Result<u32, uc::Error>;
    fn clear_suspend(&mut self, request: &uc::SuspendWrite) -> Result<(), uc::Error>;
    fn acknowledge_ipa(&mut self) -> Result<(), uc::Error>;
}
struct Ipa<'a, R: Memory, D: Memory, I: Io<R, D>> {
    io: &'a mut I,
    rings: &'a Pool<R>,
    data: &'a DataPool<D>,
    expected: GuardState,
}
impl<R: Memory, D: Memory, I: Io<R, D>> uc::Io for Ipa<'_, R, D, I> {
    fn guard(&mut self) -> Result<(), uc::Error> {
        start::Io::guard(self.io, self.rings, self.data, self.expected)
            .map_err(|_| uc::Error::Guard)
    }
    fn read(&mut self, r: irq::Read) -> Result<u32, uc::Error> {
        self.io.ipa_read(r)
    }
    fn write(&mut self, r: &uc::Write) -> Result<(), uc::Error> {
        self.io.ipa_write(r)
    }
    fn shared(&mut self, f: irq::Shared) -> Result<u8, uc::Error> {
        self.io.uc_byte(f)
    }
    fn acknowledge(&mut self) -> Result<(), uc::Error> {
        self.io.acknowledge_ipa()
    }
}
impl<R: Memory, D: Memory, I: Io<R, D>> uc::ReceiveIo for Ipa<'_, R, D, I> {
    fn suspend_info(&mut self) -> Result<u32, uc::Error> {
        self.io.suspend_info()
    }
    fn clear_suspend(&mut self, r: &uc::SuspendWrite) -> Result<(), uc::Error> {
        self.io.clear_suspend(r)
    }
}
pub struct Runtime<R: Memory, D: Memory> {
    receive: rx::Runtime<R, D>,
    ipa: uc::ReceiveRuntime,
    error: Option<Error>,
}
impl<R: Memory, D: Memory> Runtime<R, D> {
    /// Both original owners are retained even when the handoff is invalid.
    pub fn new(mut receive: rx::Runtime<R, D>, uc: uc::Runtime) -> Self {
        let error = if receive.interrupt_state().is_err() || uc.report().error.is_some() {
            Some(Error::Startup)
        } else {
            None
        };
        if error.is_some() {
            receive.reject_dispatch_handoff();
        }
        Self {
            receive,
            ipa: uc::ReceiveRuntime::new(uc),
            error,
        }
    }
    pub const fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn receive_report(&self) -> rx::Report {
        self.receive.report()
    }
    pub fn startup_report(&self) -> start::Report {
        self.receive.startup_report()
    }
    pub fn ipa_reports(&self) -> (uc::Report, uc::SuspendReport) {
        self.ipa.reports()
    }
    pub fn pools(&self) -> (&Pool<R>, &DataPool<D>) {
        self.receive.pools()
    }
    /// Recheck the retained Linux receive setup against the platform's live
    /// hardware guard. This is an observation, not an IPA-service permit:
    /// the platform must also bind the check to the active owner generation.
    pub fn validate_retained_setup(&self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        if self.error.is_some() {
            return Err(Error::Quarantined);
        }
        let state = self.receive.interrupt_state().map_err(Error::Receive)?;
        if !state.rx_running() || !state.route_set() || self.ipa.reports().0.error.is_some() {
            return Err(Error::Startup);
        }
        let (rings, data) = self.receive.pools();
        start::Io::guard(io, rings, data, state).map_err(|_| Error::Startup)
    }
    /// Permit the Linux-profile AP service only after the retained hardware
    /// guard succeeds. The genuine modem-ready receipt provides the boot
    /// generation; the caller must retain the IPA and modem owners while the
    /// service is advertised. `io` is a trusted platform backend, not peer input.
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn linux_ap_service_permit(
        &self,
        io: &mut impl Io<R, D>,
        ready: &crate::sunfish_mpss_linux_smp2p::ReadyObservation,
    ) -> Result<crate::qcom_ipa_service::ApReadyPermit, Error> {
        if ready.generation() == 0 {
            return Err(Error::Startup);
        }
        self.validate_retained_setup(io)?;
        crate::qcom_ipa_service::ApReadyPermit::after_linux_setup(ready.generation())
            .map_err(|_| Error::Startup)
    }
    fn receive_fault(&mut self, io: &mut impl Io<R, D>, error: Error) -> Error {
        let _ = self.receive.external_fault(io);
        self.ipa.receive_failed();
        self.error.get_or_insert(error);
        error
    }
    pub fn notification(
        &mut self,
        notice: Notice,
        io: &mut impl Io<R, D>,
        consumer: &mut impl Consumer,
    ) -> Result<usize, Error> {
        if self.error.is_some() {
            return Err(Error::Quarantined);
        }
        if notice == Notice::Fault {
            return Err(self.receive_fault(io, Error::Fault));
        }
        if matches!(notice, Notice::Ipa | Notice::Both) {
            if let Err(e) = self.receive.check_interrupt_sources(io) {
                return Err(self.receive_fault(io, Error::Receive(e)));
            }
            let expected = match self.receive.interrupt_state() {
                Ok(v) => v,
                Err(e) => return Err(self.receive_fault(io, Error::Receive(e))),
            };
            let (rings, data) = self.receive.pools();
            let mut access = Ipa {
                io,
                rings,
                data,
                expected,
            };
            // UC errors take priority over reading/reposting any RX data.
            if let Err(e) = self.ipa.notification(&mut access) {
                self.receive.ipa_dispatch_failed(io);
                let error = Error::Ipa(e);
                self.error = Some(error);
                return Err(error);
            }
        }
        if matches!(notice, Notice::Gsi | Notice::Both) {
            match self.receive.notification(io, consumer) {
                Ok(n) => return Ok(n),
                Err(e) => {
                    self.ipa.receive_failed();
                    let error = Error::Receive(e);
                    self.error = Some(error);
                    return Err(error);
                }
            }
        }
        Ok(0)
    }
}
