//! Logical modem-owner notifications on the combined native startup endpoint.
//! Caller must supply the actual audited IRQ caps/owner and service `take_other`
//! through the retained SMMU/IPA/GSI owners. This grants no hardware authority.
//! Not connected to the frozen PAS4-only image; its future profile needs IRQ481.
#![forbid(unsafe_code)]
use crate::{
    qcom_glink_fifo::Io,
    qcom_modem_owner::{self as modem, Interrupts, Owner, Progress},
};

pub const GLINK_CHANNEL: u8 = 4;
pub const SMP_CHANNEL: u8 = 5;
pub const WATCHDOG_CHANNEL: u8 = 6;
pub const CONTINUATION_SEND_CHANNEL: u8 = 7;
pub const CONTINUATION_RECEIVE_CHANNEL: u8 = 8;
const GLINK: u64 = 1 << GLINK_CHANNEL;
const SMP: u64 = 1 << SMP_CHANNEL;
const WATCHDOG: u64 = 1 << WATCHDOG_CHANNEL;
const MODEM: u64 = GLINK | SMP;
const SOFTWARE_WAKE: u64 = 1 << CONTINUATION_RECEIVE_CHANNEL;
const OTHER: u64 = 0xf;

/// Kernel adapter must ACK exactly the supplied audited channel once. An
/// error may have taken effect; this router never retries that operation.
pub trait Kernel {
    fn acknowledge(&mut self, physical_channel: u8) -> Result<(), ()>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Consumed,
    Watchdog,
    SmmuFault,
    UnexpectedNotification,
    DuplicateNotification,
    UnexpectedAck,
    Ack,
    Identity,
    Owner(modem::Error),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub pending_modem: u64,
    pub pending_other: u64,
    pub continuation_pending: bool,
    pub acknowledgements: u32,
    pub error: Option<Error>,
}

/// Keeps physical notifications until the actual serialized owner requests
/// their rearm. Continuations are a software work flag, not physical GSI bit3.
pub struct Router<K: Kernel> {
    #[cfg(feature = "sunfish-original-final-ack-turn")]
    original_final_ack_turn: bool,
    kernel: K,
    generation: u64,
    identity: Option<u64>,
    pending: u64,
    delivered: u64,
    other: u64,
    continuation: bool,
    busy: bool,
    acknowledgements: u32,
    error: Option<Error>,
    #[cfg(feature = "sunfish-wlan-elf-oack-router-turn")]
    elf_oack_turn: Option<(u64, u64)>,
    #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
    first_oack_turn: Option<(u64, u64)>,
    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    retry_oack_turn: bool,
}
impl<K: Kernel> Router<K> {
    pub fn new(kernel: K, generation: u64) -> Self {
        Self {
            #[cfg(feature = "sunfish-original-final-ack-turn")]
            original_final_ack_turn: false,
            kernel,
            generation,
            identity: None,
            pending: 0,
            delivered: 0,
            other: 0,
            continuation: false,
            busy: false,
            acknowledgements: 0,
            error: (generation == 0).then_some(Error::Identity),
            #[cfg(feature = "sunfish-wlan-elf-oack-router-turn")]
            elf_oack_turn: None,
            #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
            first_oack_turn: None,
            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
            retry_oack_turn: false,
        }
    }
    pub fn report(&self) -> Report {
        Report {
            pending_modem: self.pending,
            pending_other: self.other,
            continuation_pending: self.continuation,
            acknowledgements: self.acknowledgements,
            error: self.error.or(self.busy.then_some(Error::Consumed)),
        }
    }
    /// Permanently stop routing after any uncertain runtime operation.
    pub fn quarantine(&mut self) {
        self.fail(Error::Consumed);
    }
    fn fail(&mut self, error: Error) -> Error {
        *self.error.get_or_insert(error)
    }
    fn live(&mut self) -> Result<(), Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.busy {
            return Err(self.fail(Error::Consumed));
        }
        Ok(())
    }
    pub fn capture(&mut self, physical_badge: u64) -> Result<(), Error> {
        // Even a simultaneous modem failure must not silently discard faults
        // or completions belonging to the other retained owners.
        self.other |= physical_badge & OTHER;
        self.live()?;
        if physical_badge & WATCHDOG != 0 {
            return Err(self.fail(Error::Watchdog));
        }
        if physical_badge & 3 != 0 {
            return Err(self.fail(Error::SmmuFault));
        }
        if physical_badge & !(MODEM | OTHER | SOFTWARE_WAKE) != 0 {
            return Err(self.fail(Error::UnexpectedNotification));
        }
        let incoming = physical_badge & MODEM;
        if incoming & self.pending != 0 {
            return Err(self.fail(Error::DuplicateNotification));
        }
        self.pending |= incoming;
        Ok(())
    }
    /// Forward unchanged bits0..3 exactly once to their real startup owners.
    /// This remains available on failure; it does not ACK any interrupt.
    pub fn take_other(&mut self) -> u64 {
        core::mem::take(&mut self.other)
    }
    /// Rearm the boot-negotiation GLINK IRQ after one successful bounded
    /// BootTransport step. The normal Owner path performs this ACK itself;
    /// this narrow path covers only the pre-Owner negotiation phase.
    pub fn acknowledge_boot_glink(&mut self) -> Result<(), Error> {
        self.live()?;
        if self.pending & GLINK == 0 || self.delivered & GLINK != 0 {
            return Err(self.fail(Error::UnexpectedAck));
        }
        self.delivered |= GLINK;
        Interrupts::acknowledge(self, modem::GLINK_CHANNEL).map_err(|_| Error::Ack)
    }
    fn badge(&mut self) -> Result<u64, Error> {
        self.live()?;
        let fresh = self.pending & !self.delivered;
        self.delivered |= fresh;
        let mut logical = 0;
        if fresh & GLINK != 0 {
            logical |= 1 << modem::GLINK_CHANNEL;
        }
        if fresh & SMP != 0 {
            logical |= 1 << modem::STATUS_CHANNEL;
        }
        if core::mem::take(&mut self.continuation) {
            logical |= 1 << modem::CONTINUE_RECEIVE;
        }
        Ok(logical)
    }
    /// One bounded actual Owner pass. Its own FIFO/domain revalidation stays
    /// ahead of every translated kernel ACK. Errors quarantine this router.
    pub fn dispatch<I: Io>(&mut self, owner: &mut Owner<I>) -> Result<Progress, Error> {
        if let Err(error) = self.live() {
            owner.fault();
            return Err(error);
        }
        if self.other != 0 {
            return Ok(Progress {
                deferred: true,
                ..Progress::default()
            });
        }
        if owner.generation() != self.generation
            || self.identity.is_some_and(|id| id != owner.identity())
        {
            owner.fault();
            return Err(self.fail(Error::Identity));
        }
        self.identity = Some(owner.identity());
        let badge = self.badge()?;
        let result = owner.notified(badge, self);
        #[cfg(feature = "sunfish-ipa-init-after-registration")]
        let result = result.and_then(|progress| {
            if progress.published != 0 {
                owner.admit_init_after_registration(
                    #[cfg(feature = "sunfish-ipa-init-empty-rx")]
                    progress.init_at_registration,
                    self,
                )?;
            }
            Ok(progress)
        });
        match result {
            Ok(progress) => Ok(progress),
            Err(error) => {
                owner.fault();
                Err(self.fail(Error::Owner(error)))
            }
        }
    }

    #[cfg(feature = "sunfish-grant-rx-open")]
    pub fn dispatch_with_grant_receive_turn<I: Io>(&mut self, owner: &mut Owner<I>) -> Result<(), Error> {
        let before = owner.grant_receive_trace().ack_count;
        self.dispatch(owner)?;
        #[cfg(feature = "sunfish-wrq-grant-turns")]
        if owner.grant_receive_trace().ack_count != before { owner.mark_primary_grant(); }
        // Dispatch at most once here. An ACK in a service pass or in the extra dispatch cannot recurse.
        // Consume only a continuation earned by a pass that committed the matching ACK.
        if self.continuation && self.other == 0 && self.error.is_none() && !self.busy
            && owner.grant_receive_trace().ack_count != before && owner.take_grant_receive_turn()
        {
            self.dispatch(owner)?;
        }
        Ok(())
    }

    /// Service the cold registration and give a newly queued reply one
    /// normal dispatch before the runtime proceeds to other services.
    #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
    pub fn service_cold_ipa_registration<I: Io>(
        &mut self,
        owner: &mut Owner<I>,
        clock: fn() -> u64,
    ) -> Result<bool, Error> {
        let handled = owner.service_cold_ipa_registration(clock, self).map_err(Error::Owner)?;
        #[cfg(feature = "sunfish-ipa-reply-before-oack")]
        if handled && self.continuation && owner.ipa_registration_trace().is_some_and(|trace|
            trace.queued_at.is_some() && trace.published_at.is_none())
        {
            self.dispatch(owner)?;
        }
        Ok(handled)
    }

    /// Give a newly queued server-check OACK one existing router turn.
    /// The normal dispatch owns receive processing, publication and IRQ rearm.
    #[cfg(feature = "sunfish-wlan-elf-oack-router-turn")]
    pub fn dispatch_queued_server_check_oack<I: Io>(
        &mut self,
        owner: &mut Owner<I>,
        clock: fn() -> u64,
    ) -> Result<bool, Error> {
        if !self.continuation || self.other != 0 {
            return Ok(false);
        }
        #[cfg(not(feature = "sunfish-wlan-first-server-check-early-turn"))]
        if self.elf_oack_turn.is_some() {
            return Ok(false);
        }
        let trace = owner.service_registry_trace();
        let ready = |write: crate::qcom_glink_qmi::ElfServerCheckTrace| {
            write.wrq_result == Some(0) && !write.coalesced && write.oack.phase == 2
        };
        #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
        let first = self.first_oack_turn.is_none() && trace.first_server_check.is_some_and(|write| {
            write.before_elf && write.timing.closes == 0 && ready(write)
        });
        #[cfg(not(feature = "sunfish-wlan-first-server-check-early-turn"))]
        let first = false;
        if !first && (self.elf_oack_turn.is_some()
            || !trace.elf_exchange.and_then(|exchange| exchange.server_check).is_some_and(ready))
        {
            return Ok(false);
        }
        let started = clock();
        let result = self.dispatch(owner);
        let turn = Some((started, clock()));
        if first {
            #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
            { self.first_oack_turn = turn; }
        } else {
            self.elf_oack_turn = turn;
        }
        result.map(|_| true)
    }

    /// At most one ordinary checked dispatch for the exact original final ACK.
    /// An attempted but blocked write consumes this turn; normal continuation
    /// owns subsequent retries. Diagnostic clocks never gate the opportunity.
    #[cfg(feature = "sunfish-original-final-ack-turn")]
    pub fn dispatch_queued_original_final_ack<I: Io>(
        &mut self, owner: &mut Owner<I>,
    ) -> Result<bool, Error> {
        if self.original_final_ack_turn || !self.continuation || self.other != 0 {
            return Ok(false);
        }
        if !owner.original_final_ack_queued() { return Ok(false); }
        self.original_final_ack_turn = true;
        self.dispatch(owner).map(|_| true)
    }

/* READY_HANDOFF_BEGIN */
    #[cfg(feature="sunfish-registry-ready-handoffs")]
    pub fn service_ready_blocker<I:Io>(&mut self,owner:&mut Owner<I>)->Result<Option<modem::ServiceProgress>,Error>{
        if let Err(e)=self.live(){owner.fault();return Err(e);}
        if !self.continuation || self.other!=0{return Ok(None);}
        if owner.generation()!=self.generation || self.identity.is_some_and(|id|id!=owner.identity()){
            owner.fault();return Err(self.fail(Error::Identity));
        }
        owner.dispatch_ready_blocker(self).map_err(|e|self.fail(Error::Owner(e)))
    }
/* READY_HANDOFF_END */
    /// One normal registry service attempt before TFTP may refill a released slot.
    #[cfg(feature="sunfish-elf-capacity-retry")]
    pub fn service_elf_capacity_retry<I:Io>(&mut self,owner:&mut Owner<I>)
        ->Result<Option<modem::ServiceProgress>,Error>{
        if let Err(error)=self.live(){owner.fault();return Err(error);}
        if !self.continuation || self.other!=0{return Ok(None);}
        if owner.generation()!=self.generation || self.identity.is_some_and(|id|id!=owner.identity()){
            owner.fault();return Err(self.fail(Error::Identity));
        }
        owner.dispatch_elf_capacity_retry(self).map_err(|e|self.fail(Error::Owner(e)))
    }

    /// One normal checked dispatch immediately after the exact ELF reply queues.
    /// The capability is consumed before any callback; blocked attempts fall back
    /// to the existing continuation. Other owners and terminal faults keep priority.
    #[cfg(feature="sunfish-elf-queued-handoff")]
    pub fn dispatch_queued_elf_reply<I:Io>(&mut self,owner:&mut Owner<I>)->Result<bool,Error>{
        if let Err(error)=self.live(){owner.fault();return Err(error);}
        if !self.continuation || self.other!=0 {return Ok(false);}
        // Check the same generation/owner binding before consuming its one shot.
        if owner.generation()!=self.generation || self.identity.is_some_and(|id|id!=owner.identity()) {
            owner.fault();return Err(self.fail(Error::Identity));
        }
        if !owner.take_queued_elf_turn().map_err(|e|self.fail(Error::Owner(e)))?{return Ok(false);}
        owner.observe_queued_elf_turn(None);
        let result=self.dispatch(owner);
        owner.observe_queued_elf_turn(Some(result.map_err(|_|())));
        result.map(|_|true)
    }

    /// Complete the MCFG close boundary before returning to ordinary services.
    #[cfg(feature="sunfish-mcfg-close-handoff")]
    pub fn dispatch_queued_elf_mcfg_handoff<I:Io>(&mut self,owner:&mut Owner<I>,
        server:&mut crate::qcom_tftp_server_check::ServerCheck)->Result<bool,Error>{
        if let Err(error)=self.live(){owner.fault();return Err(error);}
        if self.other!=0{return Ok(false);}
        if owner.generation()!=self.generation || self.identity.is_some_and(|id|id!=owner.identity()){
            owner.fault();return Err(self.fail(Error::Identity));
        }
        // A pending exclusive close may not re-enter the ordinary Owner path.
        let dispatched=if owner.pending_mcfg_close().is_some(){false}else{self.dispatch_queued_elf_reply(owner)?};
        if owner.pending_mcfg_close().is_some(){
            owner.resolve_mcfg_close(server).map_err(|error|self.fail(Error::Owner(error)))?;
        }
        Ok(dispatched)
    }

    /// Consume before one ordinary writer turn; retry only the exact live pair.
    /// Other owners, grant/credit work and backpressure keep the normal path.
    #[cfg(feature = "sunfish-registry-handoff-turn")]
    pub fn dispatch_registry_handoff<I: Io>(&mut self, owner: &mut Owner<I>)
        -> Result<Option<modem::ServiceProgress>, Error>
    {
        if let Err(error) = self.live() { owner.fault(); return Err(error); }
        if !self.continuation || self.other != 0 { return Ok(None); }
        let token = owner.take_registry_handoff().map_err(|error| self.fail(Error::Owner(error)))?;
        let Some(token) = token else { return Ok(None); };
        owner.observe_registry_handoff_writer(token, false, false);
        let result = self.dispatch(owner);
        owner.observe_registry_handoff_writer(token, true, result.is_err());
        result?;
        if self.other != 0 { return Ok(None); }
        owner.retry_registry_handoff(token, self).map_err(|error| self.fail(Error::Owner(error)))
    }

    #[cfg(feature = "sunfish-original-final-ack-turn")]
    pub fn original_final_ack_turn(&self) -> bool { self.original_final_ack_turn }

    #[cfg(feature = "sunfish-wlan-first-server-check-early-turn")]
    pub fn first_oack_turn(&self) -> Option<(u64, u64)> {
        self.first_oack_turn
    }

    #[cfg(feature = "sunfish-wlan-elf-oack-router-turn")]
    pub fn elf_oack_turn(&self) -> Option<(u64, u64)> {
        self.elf_oack_turn
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    pub fn dispatch_queued_tftp_retry<I: Io>(&mut self, owner: &mut Owner<I>) -> Result<bool, Error> {
        if self.retry_oack_turn || !self.continuation || self.other != 0 {
            return Ok(false);
        }
        let queued = owner.service_registry_trace().first_server_check.and_then(|write| write.repeat)
            .is_some_and(|repeat| repeat.overlapped && repeat.oack.phase == 2 && !repeat.cancelled);
        if !queued { return Ok(false); }
        self.retry_oack_turn = true;
        self.dispatch(owner).map(|_| true)
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    pub fn retry_oack_turn(&self) -> bool { self.retry_oack_turn }
}
impl<K: Kernel> Interrupts for Router<K> {
    fn acknowledge(&mut self, logical_channel: u8) -> Result<(), modem::Error> {
        self.live().map_err(|_| modem::Error::Ack)?;
        let physical = match logical_channel {
            modem::GLINK_CHANNEL => GLINK_CHANNEL,
            modem::STATUS_CHANNEL => SMP_CHANNEL,
            _ => {
                self.fail(Error::UnexpectedAck);
                return Err(modem::Error::Ack);
            }
        };
        let bit = 1 << physical;
        if self.pending & self.delivered & bit == 0 {
            self.fail(Error::UnexpectedAck);
            return Err(modem::Error::Ack);
        }
        self.busy = true; // uncertain kernel errors/unwind remain terminal
        if self.kernel.acknowledge(physical).is_err() {
            self.fail(Error::Ack);
            return Err(modem::Error::Ack);
        }
        self.pending &= !bit;
        self.delivered &= !bit;
        self.acknowledgements = self.acknowledgements.saturating_add(1);
        self.busy = false;
        Ok(())
    }
    fn schedule(&mut self) -> Result<(), modem::Error> {
        self.live().map_err(|_| modem::Error::Schedule)?;
        self.continuation = true;
        Ok(())
    }
}
