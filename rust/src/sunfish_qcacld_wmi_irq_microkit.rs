//! Microkit IRQ/event-loop adapter for the bounded CE2 WMI runtime.
#![cfg(target_os = "none")]
#![forbid(unsafe_code)]

use core::fmt;

use sel4_microkit::ipc::{self, Event};
use sel4_microkit::{Channel, ChannelSet, Child, Handler, MessageInfo, Never, pd_is_passive};

use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_native_io::{Gate, NativeIo};
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_htc::HtcCreditLedger;
use crate::qcom_wmi_ce::RxBufferLedger;
use crate::sunfish_qcacld_mgmt_tx::MgmtTxOutstanding;
use crate::sunfish_qcacld_wmi_irq_runtime::{DrainProgress, RuntimeError, WmiCe2IrqRuntime};
use crate::sunfish_qcacld_wmi_rx::{
    StartupObservedWmiEvent, SunfishWmiStartupSession, WmiCe2DrainReport,
};
use crate::sunfish_qcacld_wmi_rx_native::{
    NativeWmiCe2EventContext, NativeWmiCe2ServiceError, service_native_wmi_ce2_irq_with_action,
};

/// Adapter implemented by the WLAN owner around one bounded call to
/// `service_native_wmi_ce2_irq`.
pub trait BudgetedWmiCe2Drain {
    type Error: fmt::Display;

    /// Run one bounded CE2 service slice and return its native report.
    fn service_budget(&mut self) -> Result<WmiCe2DrainReport, Self::Error>;
}

/// Dispatches one fully consumed, validated WMI startup event.
pub trait WmiEventDispatcher {
    type Error: fmt::Display;

    fn dispatch(&mut self, event: &StartupObservedWmiEvent<'_>) -> Result<(), Self::Error>;

    /// Perform a source-ordered action after the event and startup transition
    /// are accepted, while CE2 remains masked and before its status ACK. A
    /// runtime dispatcher can use this to compose and publish WMI_INIT on
    /// SERVICE_AVAILABLE. The default keeps existing event-only handlers
    /// valid and performs no hardware action.
    fn after_observe<G: Gate>(
        &mut self,
        _event: &StartupObservedWmiEvent<'_>,
        _context: NativeWmiCe2EventContext<'_, G>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// The production-shaped CE2 drain adapter used by the Microkit IRQ loop.
///
/// It owns no hardware authority: `io` must already be constructed from a
/// live `qcom_ce_v3_native_io::Gate`, and every drain slice rechecks that gate
/// through `service_native_wmi_ce2_irq`. The event dispatcher runs only after
/// the WMI receive descriptor has been consumed and the startup session has
/// accepted its contents.
pub struct NativeWmiCe2Drain<'a, G: Gate, D: WmiEventDispatcher, const N: usize> {
    io: &'a mut NativeIo<G>,
    generation: u64,
    profile: Profile,
    rings: &'a RingPlan,
    cursor: &'a mut RingCursor,
    rx_ledger: &'a mut RxBufferLedger,
    expected_endpoint_id: u8,
    max_message_size: usize,
    credits: &'a mut HtcCreditLedger,
    outstanding: &'a mut MgmtTxOutstanding<N>,
    startup: &'a mut SunfishWmiStartupSession,
    scratch: &'a mut [u8],
    max_completions: usize,
    dispatcher: D,
}

impl<'a, G: Gate, D: WmiEventDispatcher, const N: usize> NativeWmiCe2Drain<'a, G, D, N> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        io: &'a mut NativeIo<G>,
        generation: u64,
        profile: Profile,
        rings: &'a RingPlan,
        cursor: &'a mut RingCursor,
        rx_ledger: &'a mut RxBufferLedger,
        expected_endpoint_id: u8,
        max_message_size: usize,
        credits: &'a mut HtcCreditLedger,
        outstanding: &'a mut MgmtTxOutstanding<N>,
        startup: &'a mut SunfishWmiStartupSession,
        scratch: &'a mut [u8],
        max_completions: usize,
        dispatcher: D,
    ) -> Self {
        Self {
            io,
            generation,
            profile,
            rings,
            cursor,
            rx_ledger,
            expected_endpoint_id,
            max_message_size,
            credits,
            outstanding,
            startup,
            scratch,
            max_completions,
            dispatcher,
        }
    }
}

impl<G: Gate, D: WmiEventDispatcher, const N: usize> BudgetedWmiCe2Drain
    for NativeWmiCe2Drain<'_, G, D, N>
{
    type Error = NativeWmiCe2ServiceError<D::Error>;

    fn service_budget(&mut self) -> Result<WmiCe2DrainReport, Self::Error> {
        service_native_wmi_ce2_irq_with_action(
            self.io,
            self.generation,
            self.profile,
            self.rings,
            self.cursor,
            self.rx_ledger,
            self.expected_endpoint_id,
            self.max_message_size,
            self.credits,
            self.outstanding,
            self.startup,
            self.scratch,
            self.max_completions,
            |event, context| {
                self.dispatcher.dispatch(event)?;
                self.dispatcher.after_observe(event, context)
            },
        )
    }
}

#[derive(Debug)]
pub enum DrainServiceError<Service> {
    Service(Service),
    InconsistentReport,
}

impl<Service: fmt::Display> fmt::Display for DrainServiceError<Service> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service(error) => write!(f, "{error}"),
            Self::InconsistentReport => f.write_str("inconsistent CE2 drain report"),
        }
    }
}

#[derive(Debug)]
pub enum HandlerError<Service> {
    PassiveProtectionDomain,
    UnexpectedChannel,
    ProtectedCall,
    ChildFault,
    Runtime(RuntimeError<Service, sel4_microkit::IrqAckError>),
}

impl<Service: fmt::Display> fmt::Display for HandlerError<Service> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PassiveProtectionDomain => {
                f.write_str("CE2 IRQ handler must run in an active protection domain")
            }
            Self::UnexpectedChannel => f.write_str("unexpected notification channel"),
            Self::ProtectedCall => f.write_str("unexpected protected procedure call"),
            Self::ChildFault => f.write_str("unexpected child protection-domain fault"),
            Self::Runtime(RuntimeError::UnexpectedNotification) => {
                f.write_str("empty CE2 notification")
            }
            Self::Runtime(RuntimeError::Service(error)) => {
                write!(f, "CE2 service failed: {error}")
            }
            Self::Runtime(RuntimeError::InterruptAcknowledge(error)) => {
                write!(f, "CE2 GIC acknowledgement failed: {error}")
            }
        }
    }
}

/// A Microkit `Handler` which never acknowledges a GIC IRQ before CE2 has
/// drained. The continuation channel must be a same-PD loopback channel: for
/// example output ID 2 -> input ID 3. Configure that channel in the system
/// description with notifications enabled at both ends.
pub struct WmiCe2MicrokitHandler<Service> {
    service: Service,
    runtime: WmiCe2IrqRuntime,
    irq_channel: Channel,
    continuation_output: Channel,
    continuation_input: Channel,
}

impl<Service> WmiCe2MicrokitHandler<Service> {
    pub fn new(
        service: Service,
        irq_channel: usize,
        continuation_output: usize,
        continuation_input: usize,
    ) -> Self {
        Self {
            service,
            runtime: WmiCe2IrqRuntime::new(),
            irq_channel: Channel::new(irq_channel),
            continuation_output: Channel::new(continuation_output),
            continuation_input: Channel::new(continuation_input),
        }
    }
}

impl<Service: BudgetedWmiCe2Drain> Handler for WmiCe2MicrokitHandler<Service> {
    type Error = HandlerError<DrainServiceError<Service::Error>>;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        let mut hardware_irq = false;
        let mut continuation = false;
        for channel in channels.iter() {
            if channel == self.irq_channel {
                hardware_irq = true;
            } else if channel == self.continuation_input {
                continuation = true;
            } else {
                return Err(HandlerError::UnexpectedChannel);
            }
        }

        let runtime = &mut self.runtime;
        let service = &mut self.service;
        let irq_channel = self.irq_channel;
        let continuation_output = self.continuation_output;
        runtime
            .notified(
                hardware_irq,
                continuation,
                || {
                    let report = service
                        .service_budget()
                        .map_err(DrainServiceError::Service)?;
                    DrainProgress::from_report_flags(
                        report.irq_acknowledged,
                        report.needs_deferred_drain,
                    )
                    .ok_or(DrainServiceError::InconsistentReport)
                },
                || continuation_output.notify(),
                || irq_channel.irq_ack(),
            )
            .map(|_| ())
            .map_err(HandlerError::Runtime)
    }

    /// The stock Microkit loop ACKs all IRQ capabilities once before its first
    /// receive. That is too early for CE2, so this active-PD loop deliberately
    /// starts receiving without that startup ACK; initial IRQs use `notified`.
    fn run(&mut self) -> Result<Never, Self::Error> {
        if pd_is_passive() {
            return Err(HandlerError::PassiveProtectionDomain);
        }

        loop {
            match ipc::recv() {
                Event::Notified(channels) => self.notified(channels)?,
                Event::Protected(_, _) => return Err(HandlerError::ProtectedCall),
                Event::Fault(_, _) => return Err(HandlerError::ChildFault),
            }
        }
    }

    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Err(HandlerError::ProtectedCall)
    }

    fn fault(&mut self, _: Child, _: MessageInfo) -> Result<Option<MessageInfo>, Self::Error> {
        Err(HandlerError::ChildFault)
    }
}
