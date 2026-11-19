//! Bounded seL4 continuation policy for the CE2 WMI receive interrupt.
//!
//! A hardware notification remains unacknowledged while the device-side drain
//! reports pending work. The runtime schedules another bounded slice through a
//! same-PD Microkit notification channel, then acknowledges the GIC capability
//! only after the device-side service has drained and W1C-acknowledged CE2.
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainProgress {
    Complete,
    Deferred,
}

impl DrainProgress {
    /// Classify the native CE2 report. Both bits must agree: otherwise the
    /// GIC handler cannot know whether it is safe to release the IRQ.
    pub const fn from_report_flags(
        irq_acknowledged: bool,
        needs_deferred_drain: bool,
    ) -> Option<Self> {
        match (irq_acknowledged, needs_deferred_drain) {
            (true, false) => Some(Self::Complete),
            (false, true) => Some(Self::Deferred),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationOutcome {
    Complete,
    Deferred,
    StaleContinuation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeError<Service, Ack> {
    UnexpectedNotification,
    Service(Service),
    InterruptAcknowledge(Ack),
}

/// Tracks whether CE2's GIC interrupt is still masked pending a bounded drain.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WmiCe2IrqRuntime {
    irq_pending: bool,
}

impl WmiCe2IrqRuntime {
    pub const fn new() -> Self {
        Self { irq_pending: false }
    }

    pub const fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    /// Process one hardware IRQ or self-notification continuation.
    ///
    /// `service` must return `Complete` only after the CE2 copy-complete W1C
    /// status has been acknowledged (and any permitted rearm has completed).
    /// `schedule_continuation` must signal a same-PD Microkit channel whose
    /// receiving end is included in the handler's notification set.
    pub fn notified<Service, Ack>(
        &mut self,
        hardware_irq: bool,
        continuation: bool,
        mut service: impl FnMut() -> Result<DrainProgress, Service>,
        mut schedule_continuation: impl FnMut(),
        mut acknowledge_irq: impl FnMut() -> Result<(), Ack>,
    ) -> Result<NotificationOutcome, RuntimeError<Service, Ack>> {
        if !hardware_irq && !continuation {
            return Err(RuntimeError::UnexpectedNotification);
        }

        if hardware_irq {
            self.irq_pending = true;
        } else if continuation && !self.irq_pending {
            // A coalesced/late self-notification after a completed ACK is inert.
            return Ok(NotificationOutcome::StaleContinuation);
        }

        let progress = service().map_err(RuntimeError::Service)?;
        match progress {
            DrainProgress::Deferred => {
                schedule_continuation();
                Ok(NotificationOutcome::Deferred)
            }
            DrainProgress::Complete => {
                acknowledge_irq().map_err(RuntimeError::InterruptAcknowledge)?;
                self.irq_pending = false;
                Ok(NotificationOutcome::Complete)
            }
        }
    }
}
