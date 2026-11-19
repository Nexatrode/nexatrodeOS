//! Bounded UC interrupt dispatcher for the pinned working Linux IPA profile.
//!
//! Transfers a successful ArmOnce, clears each UC bit before reading its
//! defined shared byte, drains enabled sources, and ACKs the physical IRQ only
//! after dispatch. Power and all mappings must remain held by the backend.
//! INIT_COMPLETED is observed evidence only: it is not PAS authentication,
//! GSI/IPA readiness, functional DMA, or a permission to collapse power.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_irq::{self as irq, ArmOnce, Read, Shared};

pub const CLEAR_OFFSET: usize = 0x3010;
pub const MAX_DRAIN_ROUNDS: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotArmed,
    Quarantined,
    Guard,
    Io,
    EnableChanged,
    DrainLimit,
    MicrocontrollerError,
    Acknowledge,
    ExternalFault,
}
impl Error {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}

// Closed requests: no UC command, shared-memory write, arbitrary register
// access, or interrupt-enable operation is available to the dispatcher.
pub struct Write {
    clear: Option<u32>,
}
impl Write {
    pub const fn offset(&self) -> usize {
        if self.clear.is_some() {
            CLEAR_OFFSET
        } else {
            irq::ENABLE_OFFSET
        }
    }
    pub const fn value(&self) -> u32 {
        match self.clear {
            Some(mask) => mask,
            None => 0,
        }
    }
}

/// Trusted platform boundary, with the real IRQ binding, powered MMIO/SRAM
/// views and installed domain retained by the caller for the full lifetime.
/// Guard must reject lost power/mappings, SMMU faults and invalid configuration.
/// The backend must not retry a write/ACK which returns an uncertain error.
/// No method may read reserved UC bytes or release power/memory resources.
pub trait Io {
    fn guard(&mut self) -> Result<(), Error>;
    fn read(&mut self, register: Read) -> Result<u32, Error>;
    fn write(&mut self, request: &Write) -> Result<(), Error>;
    fn shared(&mut self, field: Shared) -> Result<u8, Error>;
    fn acknowledge(&mut self) -> Result<(), Error>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub notifications: u32,
    pub status_reads: u32,
    pub clear_submitted: u32,
    pub clear_completed: u32,
    pub last_clear: u32,
    pub last_status: Option<u32>,
    pub last_event: Option<u8>,
    pub last_response: Option<u8>,
    pub events: u32,
    pub responses: u32,
    pub init_observed: u32,
    pub unsupported_events: u32,
    pub unsupported_responses: u32,
    pub acknowledge_submitted: u32,
    pub acknowledge_completed: u32,
    pub mask_submitted: bool,
    pub mask_completed: bool,
    pub mask_readback: Option<u32>,
    pub error: Option<Error>,
    pub containment_error: Option<Error>,
}

pub struct Runtime {
    report: Report,
}
impl Runtime {
    pub fn after_arming(arm: &mut ArmOnce) -> Result<Self, Error> {
        if !arm.transfer_to_runtime() {
            return Err(Error::NotArmed);
        }
        Ok(Self {
            report: Report::default(),
        })
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    fn value(&mut self, field: Shared, value: u8) -> Result<(), Error> {
        match field {
            Shared::Event => {
                self.report.last_event = Some(value);
                self.report.events = self.report.events.saturating_add(1);
                match value {
                    1 => return Err(Error::MicrocontrollerError),
                    2 => (),
                    _ => {
                        self.report.unsupported_events =
                            self.report.unsupported_events.saturating_add(1)
                    }
                }
            }
            Shared::Response => {
                self.report.last_response = Some(value);
                self.report.responses = self.report.responses.saturating_add(1);
                if value == 1 {
                    self.report.init_observed = self.report.init_observed.saturating_add(1);
                } else {
                    self.report.unsupported_responses =
                        self.report.unsupported_responses.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    fn guard(&mut self, io: &mut impl Io) -> Result<(), Error> {
        io.guard()?;
        if io.read(Read::Enable)? != irq::ENABLED {
            return Err(Error::EnableChanged);
        }
        Ok(())
    }
    fn clear(&mut self, io: &mut impl Io, mask: u32) -> Result<(), Error> {
        self.guard(io)?;
        self.report.last_clear = mask;
        // Consume before the possibly uncertain W1C store. A later failure
        // cannot replay the clear or acknowledge the delivered physical IRQ.
        self.report.clear_submitted = self.report.clear_submitted.saturating_add(1);
        io.write(&Write { clear: Some(mask) })?;
        self.report.clear_completed = self.report.clear_completed.saturating_add(1);
        Ok(())
    }
    fn dispatch(&mut self, io: &mut impl Io) -> Result<(), Error> {
        let mut rounds = 0;
        loop {
            self.guard(io)?;
            self.report.status_reads = self.report.status_reads.saturating_add(1);
            let pending = io.read(Read::Status)?;
            self.report.last_status = Some(pending);
            if pending & irq::ENABLED == 0 {
                // Linux clears the captured disabled mask once after draining
                // enabled sources. Level conditions may remain; do not spin
                // waiting for disabled status to become zero.
                if pending != 0 {
                    self.clear(io, pending)?;
                }
                break;
            }
            if rounds == MAX_DRAIN_ROUNDS {
                return Err(Error::DrainLimit);
            }
            rounds += 1;
            for (mask, field) in [
                (irq::EVENT_MASK, Shared::Event),
                (irq::RESPONSE_MASK, Shared::Response),
            ] {
                if pending & mask == 0 {
                    continue;
                }
                // Clear-before-byte-read is intentional: clearing afterward
                // could discard a new firmware indication arriving meanwhile.
                self.clear(io, mask)?;
                self.guard(io)?;
                let value = io.shared(field)?;
                self.value(field, value)?;
            }
        }
        self.guard(io)?;
        self.report.acknowledge_submitted = self.report.acknowledge_submitted.saturating_add(1);
        io.acknowledge()?;
        self.report.acknowledge_completed = self.report.acknowledge_completed.saturating_add(1);
        Ok(())
    }
    /// Keep the first error and all partial evidence, mask once, and leave the
    /// delivered IRQ unacknowledged. No table/pool/power rollback or retry.
    fn stop(&mut self, io: &mut impl Io, error: Error) {
        self.report.error = Some(error);
        self.report.mask_submitted = true;
        let result = (|| {
            io.write(&Write { clear: None })?;
            self.report.mask_completed = true;
            let value = io.read(Read::Enable)?;
            self.report.mask_readback = Some(value);
            if value != 0 {
                return Err(Error::EnableChanged);
            }
            Ok(())
        })();
        self.report.containment_error = result.err();
    }
    pub fn notification(&mut self, io: &mut impl Io) -> Result<(), Error> {
        if self.report.error.is_some() {
            return Err(Error::Quarantined);
        }
        self.report.notifications = self.report.notifications.saturating_add(1);
        if let Err(error) = self.dispatch(io) {
            self.stop(io, error);
            return Err(error);
        }
        Ok(())
    }
    /// Co-delivered SMMU/foreign faults take priority over UC dispatch. Do not
    /// clear shared evidence or ACK either delivered physical interrupt.
    pub fn external_fault(&mut self, io: &mut impl Io) {
        if self.report.error.is_none() {
            self.stop(io, Error::ExternalFault);
        }
    }
}

/// Only the retained receive dispatcher may select this fixed extension.
/// The public UC-only Runtime still requires exactly irq::ENABLED.
pub const RECEIVE_ENABLED: u32 = irq::ENABLED | (1 << 14);
pub const SUSPEND_INFO: usize = 0x3030;
pub const SUSPEND_CLEAR: usize = 0x3038;
pub struct SuspendWrite {
    value: u32,
}
impl SuspendWrite {
    pub const fn value(&self) -> u32 {
        self.value
    }
}
#[allow(dead_code)] // Earlier standalone UC suites omit the receive dispatcher.
pub(crate) trait ReceiveIo: Io {
    fn suspend_info(&mut self) -> Result<u32, Error>;
    fn clear_suspend(&mut self, request: &SuspendWrite) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SuspendReport {
    pub notifications: u32,
    pub info_reads: u32,
    pub last_info: Option<u32>,
    pub clears_sent: u32,
    pub clears_done: u32,
}
#[allow(dead_code)]
pub(crate) struct ReceiveRuntime {
    uc: Runtime,
    suspend: SuspendReport,
}
#[allow(dead_code)]
impl ReceiveRuntime {
    pub(crate) fn new(uc: Runtime) -> Self {
        Self {
            uc,
            suspend: SuspendReport::default(),
        }
    }
    pub(crate) fn reports(&self) -> (Report, SuspendReport) {
        (self.uc.report(), self.suspend)
    }
    fn guard(&mut self, io: &mut impl ReceiveIo) -> Result<(), Error> {
        io.guard()?;
        if io.read(Read::Enable)? != RECEIVE_ENABLED {
            return Err(Error::EnableChanged);
        }
        Ok(())
    }
    fn clear(&mut self, io: &mut impl ReceiveIo, mask: u32) -> Result<(), Error> {
        self.guard(io)?;
        self.uc.report.last_clear = mask;
        self.uc.report.clear_submitted = self.uc.report.clear_submitted.saturating_add(1);
        io.write(&Write { clear: Some(mask) })?;
        self.uc.report.clear_completed = self.uc.report.clear_completed.saturating_add(1);
        Ok(())
    }
    fn dispatch(&mut self, io: &mut impl ReceiveIo) -> Result<(), Error> {
        let mut rounds = 0;
        loop {
            self.guard(io)?;
            self.uc.report.status_reads = self.uc.report.status_reads.saturating_add(1);
            let pending = io.read(Read::Status)?;
            self.uc.report.last_status = Some(pending);
            if pending & RECEIVE_ENABLED == 0 {
                if pending != 0 {
                    self.clear(io, pending)?;
                }
                break;
            }
            if rounds == MAX_DRAIN_ROUNDS {
                return Err(Error::DrainLimit);
            }
            rounds += 1;
            // Same ascending source order as ipa_isr_thread().
            for (mask, field) in [
                (irq::EVENT_MASK, Shared::Event),
                (irq::RESPONSE_MASK, Shared::Response),
            ] {
                if pending & mask != 0 {
                    self.clear(io, mask)?;
                    self.guard(io)?;
                    let value = io.shared(field)?;
                    self.uc.value(field, value)?;
                }
            }
            if pending & (1 << 14) != 0 {
                self.suspend.notifications = self.suspend.notifications.saturating_add(1);
                self.guard(io)?;
                self.suspend.info_reads = self.suspend.info_reads.saturating_add(1);
                // v4.2 has 17 endpoints: the source's ceil(count/32) loop
                // has exactly one unit. Never clear IPA source 14 first;
                // doing so loses the endpoint information.
                let info = io.suspend_info()?;
                self.suspend.last_info = Some(info);
                if info != 0 {
                    self.guard(io)?;
                    self.suspend.clears_sent = self.suspend.clears_sent.saturating_add(1);
                    io.clear_suspend(&SuspendWrite { value: info })?;
                    self.suspend.clears_done = self.suspend.clears_done.saturating_add(1);
                }
                self.clear(io, 1 << 14)?;
            }
        }
        self.guard(io)?;
        self.uc.report.acknowledge_submitted =
            self.uc.report.acknowledge_submitted.saturating_add(1);
        io.acknowledge()?;
        self.uc.report.acknowledge_completed =
            self.uc.report.acknowledge_completed.saturating_add(1);
        Ok(())
    }
    pub(crate) fn notification(&mut self, io: &mut impl ReceiveIo) -> Result<(), Error> {
        if self.uc.report.error.is_some() {
            return Err(Error::Quarantined);
        }
        self.uc.report.notifications = self.uc.report.notifications.saturating_add(1);
        if let Err(e) = self.dispatch(io) {
            self.uc.stop(io, e);
            return Err(e);
        }
        Ok(())
    }
    /// RX containment already attempted IPA_EN=0. Latch the shared failure
    /// without replaying that possibly uncertain store or ACKing this IRQ.
    pub(crate) fn receive_failed(&mut self) {
        self.uc.report.error.get_or_insert(Error::ExternalFault);
    }
}
