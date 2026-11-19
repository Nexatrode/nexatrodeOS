//! Selected Linux IPA QMI lifecycle inside the existing serialized FIFO owner.
//! Its outer native caller must retain actual IPA, PAS, domain, pool and host
//! service owners. Protocol completion is diagnostic, never a hardware permit.
#![forbid(unsafe_code)]
use super::*;
use crate::qcom_glink_qmi::LinuxIpaHandle;
#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[path = "sunfish_ipa_linux_independent.rs"]
mod independent;
use crate::sunfish_ipa_linux_qmi::{self as ipa, ColdSession, Prepared, State};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxIpaError {
    Configuration,
    Clock,
    Timeout,
    UnexpectedResponse,
    UnexpectedReceipt,
    Protocol(ipa::Error),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxIpaProgress {
    WaitingPeer,
    Discovered,
    Prepared,
    Queued,
    WaitingQueue,
    WaitingCompletion,
    Committed,
    ResponseAccepted,
    WaitingHandshake,
    Complete,
}
#[cfg(feature = "sunfish-ipa-registration-owner-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpaRegistrationOwnerTrace {
    /// Bits 0–8: core, init started, pending record, record queued, transport
    /// receipt, held init response, registration next, eligible, modem ready.
    pub route_flags: u32,
    pub route_pending_id: Option<u16>,
    pub first_step: Option<IpaRegistrationOwnerStep>,
}
#[cfg(feature = "sunfish-ipa-registration-owner-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpaRegistrationOwnerStep {
    pub started_at: u64,
    pub ended_at: Option<u64>,
    /// Protocol result, recorded before scheduling any continuation.
    pub progress: Option<LinuxIpaProgress>,
}
#[cfg(feature = "sunfish-ipa-init-after-registration")]
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PostRegistrationTurn {
    #[default]
    AwaitingPublication = 0,
    DifferentClient = 1,
    ClientClosed = 2,
    InitIneligible = 3,
    Deferred = 4,
    Queued = 5,
    #[cfg(feature = "sunfish-ipa-init-empty-rx")]
    NoEmptyRxTurn = 6,
}

#[cfg(feature = "sunfish-ipa-init-continuation")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IpaInitOwnerTrace {
    pub discovered_at: Option<u64>,
    pub prepared_at: Option<u64>,
    pub committed_at: Option<u64>,
    #[cfg(feature = "sunfish-ipa-independent-server-runtime")]
    pub response_at: Option<u64>,
    #[cfg(feature = "sunfish-ipa-init-after-registration")]
    pub post_registration: PostRegistrationTurn,
    pub extra_steps: u32,
    pub queue_waits: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxIpaReport {
    pub protocol: Option<State>,
    pub protocol_complete: bool,
    pub pending_record: bool,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    pub server_pending_record: bool,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    pub client_pending_record: bool,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    pub client_record_queued: bool,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    pub server_record_queued: bool,
    pub record_queued: bool,
    pub held_response: bool,
    pub publications_committed: u32,
    pub responses_accepted: u32,
    /// Validated server requests successfully placed in the owner queue.
    pub registration_requests: u32,
    pub driver_requests: u32,
    /// GLINK receive-intent requests observed on the IPCRTR channel.
    pub peer_intent_requests: u32,
    pub peer_intent_denied: u32,
    pub ignored_config_requests: u32,
    pub ignored_unhandled_requests: u32,
    pub last_ignored_unhandled_id: Option<u16>,
    pub ignored_init_responses: u32,
    /// At timeout, bit 0 means a pending publication expired; bit 1 means
    /// the initialization deadline expired before modem-ready. Both may apply.
    pub timeout_causes: u32,
    /// Header metadata of the pending prepared record, if available.
    pub pending_message_id: Option<u16>,
    /// Host-observed response age since init preparation, in counter ticks.
    pub response_observed_age: Option<u64>,
    pub timeout_ticks: u64,
    pub next_deadline: Option<u64>,
    pub failed: bool,
}
struct Pending {
    prepared: Prepared,
    handle: Option<RequestHandle>,
    queued: bool,
    since: u64,
}
impl Pending {
    fn message_id(&self) -> Option<u16> {
        let bytes = self.prepared.bytes();
        Some(u16::from_le_bytes([*bytes.get(3)?, *bytes.get(4)?]))
    }
}
pub(super) struct LinuxRuntime {
    generation: u64,
    shared: u32,
    timeout: u64,
    core: Option<ColdSession>,
    peer: Option<ipa::Address>,
    pending: Option<Pending>,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    server_pending: Option<Pending>,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    server_turn: bool,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    last_progress_client: bool,
    response: Option<LinuxIpaHandle>,
    response_observed: Option<u64>,
    init_since: Option<u64>,
    last_now: Option<u64>,
    timeout_causes: u32,
    committed: u32,
    accepted: u32,
    registration_requests: u32,
    driver_requests: u32,
    peer_intent_requests: u32,
    peer_intent_denied: u32,
    ignored_config_requests: u32,
    ignored_unhandled_requests: u32,
    last_ignored_unhandled_id: Option<u16>,
    ignored_init_responses: u32,
    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    registration_turn_times: [Option<u64>; 2],
    #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
    cold_registration_handoff: Option<bool>,
    #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
    registration_owner_trace: Option<IpaRegistrationOwnerTrace>,
    #[cfg(feature = "sunfish-ipa-init-continuation")]
    init_trace: IpaInitOwnerTrace,
    failed: bool,
}
impl LinuxRuntime {
    fn new(generation: u64, shared: u32, hz: u64) -> Result<Self, LinuxIpaError> {
        ipa::InitRequest::new(0, shared, false).map_err(LinuxIpaError::Protocol)?;
        let timeout = hz
            .checked_mul(u64::from(ipa::INIT_DRIVER_TIMEOUT_MS))
            .filter(|v| hz != 0 && *v / 1000 > 0 && *v / 1000 < (1u64 << 63))
            .ok_or(LinuxIpaError::Clock)?
            / 1000;
        Ok(Self {
            generation,
            shared,
            timeout,
            core: None,
            peer: None,
            pending: None,
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            server_pending: None,
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            server_turn: true,
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            last_progress_client: false,
            response: None,
            response_observed: None,
            init_since: None,
            last_now: None,
            timeout_causes: 0,
            committed: 0,
            accepted: 0,
            registration_requests: 0,
            driver_requests: 0,
            peer_intent_requests: 0,
            peer_intent_denied: 0,
            ignored_config_requests: 0,
            ignored_unhandled_requests: 0,
            last_ignored_unhandled_id: None,
            ignored_init_responses: 0,
            #[cfg(feature = "sunfish-ipa-registration-early-turn")]
            registration_turn_times: [None; 2],
            #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
            cold_registration_handoff: None,
            #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
            registration_owner_trace: None,
            #[cfg(feature = "sunfish-ipa-init-continuation")]
            init_trace: IpaInitOwnerTrace::default(),
            failed: false,
        })
    }
    pub(super) fn fault(&mut self) {
        self.failed = true;
        if let Some(core) = self.core.as_mut() {
            let _ = core.fail(self.generation);
        }
    }
    pub(super) fn record_server_request(&mut self, message_id: u16) {
        match message_id {
            ipa::INDICATION_REGISTER => self.registration_requests = self.registration_requests.saturating_add(1),
            ipa::DRIVER_INIT_COMPLETE => self.driver_requests = self.driver_requests.saturating_add(1),
            _ => {},
        }
    }
    fn deadline(&self) -> Option<u64> {
        if self.failed {
            return None;
        }
        let init = self
            .init_since
            .filter(|_| !self.core.as_ref().is_some_and(|c| c.state().modem_ready));
        let pending = self.pending.as_ref().map(|p| p.since);
        let now = self.last_now.unwrap_or(0);
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let starts = [init, pending, self.server_pending.as_ref().map(|p| p.since)];
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        let starts = [init, pending];
        starts.into_iter()
            .flatten()
            .map(|start| start.wrapping_add(self.timeout))
            .min_by_key(|deadline| deadline.wrapping_sub(now))
    }
    fn report(&self, stopped: bool) -> LinuxIpaReport {
        let failed = self.failed || stopped;
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let server = self.server_pending.as_ref();
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        let server: Option<&Pending> = None;
        LinuxIpaReport {
            protocol: self.core.as_ref().map(ColdSession::state),
            protocol_complete: !failed
                && self
                    .core
                    .as_ref()
                    .is_some_and(ColdSession::protocol_complete),
            pending_record: self.pending.is_some() || server.is_some(),
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            server_pending_record: self.server_pending.is_some(),
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            client_pending_record: self.pending.is_some(),
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            client_record_queued: self.pending.as_ref().is_some_and(|p| p.queued),
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            server_record_queued: self.server_pending.as_ref().is_some_and(|p| p.queued),
            record_queued: self.pending.as_ref().is_some_and(|p| p.queued) || server.is_some_and(|p| p.queued),
            held_response: self.response.is_some(),
            publications_committed: self.committed,
            responses_accepted: self.accepted,
            registration_requests: self.registration_requests,
            driver_requests: self.driver_requests,
            peer_intent_requests: self.peer_intent_requests,
            peer_intent_denied: self.peer_intent_denied,
            ignored_config_requests: self.ignored_config_requests,
            ignored_unhandled_requests: self.ignored_unhandled_requests,
            last_ignored_unhandled_id: self.last_ignored_unhandled_id,
            ignored_init_responses: self.ignored_init_responses,
            timeout_causes: self.timeout_causes,
            pending_message_id: self.pending.as_ref().or(server).and_then(Pending::message_id),
            response_observed_age: self.init_since.zip(self.response_observed)
                .map(|(start, observed)| observed.wrapping_sub(start))
                .filter(|age| *age < (1u64 << 63)),
            timeout_ticks: self.timeout,
            next_deadline: if failed { None } else { self.deadline() },
            failed,
        }
    }
    fn step<I: Io>(
        &mut self,
        adapter: &mut Adapter<I>,
        requests: &mut [Option<RequestHandle>; MAX_REQUESTS],
        now: u64,
    ) -> Result<LinuxIpaProgress, Error> {
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        { self.last_progress_client = false; }
        let failure = |e| Error::LinuxIpa(e);
        let protocol = |e| failure(LinuxIpaError::Protocol(e));
        if self.failed {
            return Err(Error::Stopped);
        }
        if self
            .last_now
            .is_some_and(|last| now.wrapping_sub(last) > (u64::MAX / 2))
        {
            return Err(failure(LinuxIpaError::Clock));
        }
        self.last_now = Some(now);
        adapter.revalidate(self.generation)?;
        let peer = adapter.linux_ipa_modem(self.generation)?;
        if self.peer.is_some() && peer != self.peer {
            return Err(failure(LinuxIpaError::Configuration));
        }
        let expired = |since: u64| now.wrapping_sub(since) >= self.timeout;
        let pending_expired = self.pending.as_ref().is_some_and(|p| expired(p.since));
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let pending_expired = pending_expired || self.server_pending.as_ref().is_some_and(|p| expired(p.since));
        let init_expired = self.init_since.is_some_and(|start| {
                expired(start) && !self.core.as_ref().is_some_and(|c| c.state().modem_ready)
            });
        #[cfg(feature = "sunfish-ipa-independent-response-experiment")]
        {
            // INIT_DRIVER's validated response is independent of a later
            // server reply. Preserve the server publication and its receipt.
            // At/after the deadline accept only a response already observed
            // strictly before it, using the same counter; unknown is late.
            let response_timely = !init_expired || self.init_since.zip(self.response_observed)
                .is_some_and(|(start, observed)| observed.wrapping_sub(start) < self.timeout
                    && now.wrapping_sub(observed) < (1u64 << 63));
            let eligible = !pending_expired && response_timely && self.response.is_some()
                && self.core.as_ref().is_some_and(ColdSession::init_response_can_accept);
            if eligible {
                let handle = self.response.ok_or(failure(LinuxIpaError::Configuration))?;
                let (source, bytes) = adapter.linux_ipa_response(self.generation, &handle)?;
                self.core.as_mut().ok_or(failure(LinuxIpaError::Configuration))?
                    .receive_init_response(self.generation, source, bytes).map_err(protocol)?;
                adapter.finish_linux_ipa_response(self.generation, &handle)?;
                self.response = None;
                self.accepted = self.accepted.saturating_add(1);
                return Ok(LinuxIpaProgress::ResponseAccepted);
            }
        }
        if pending_expired || init_expired {
            self.timeout_causes = u32::from(pending_expired) | (u32::from(init_expired) << 1);
            return Err(failure(LinuxIpaError::Timeout));
        }
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        { self.step_independent(adapter, requests, now, peer) }
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        {
        if let Some(pending) = self.pending.as_mut() {
            if !pending.queued {
                match adapter.queue_linux_ipa(
                    self.generation,
                    &pending.prepared,
                    pending.handle.as_ref(),
                ) {
                    Ok(()) => {}
                    Err(fifo::Error::Session(
                        crate::qcom_glink_qmi::Error::FlowBusy
                        | crate::qcom_glink_qmi::Error::NoCapacity,
                    )) => return Ok(LinuxIpaProgress::WaitingQueue),
                    Err(error) => return Err(error.into()),
                }
                pending.queued = true;
                if let Some(handle) = pending.handle {
                    for slot in requests {
                        if *slot == Some(handle) {
                            *slot = None;
                        }
                    }
                }
                return Ok(LinuxIpaProgress::Queued);
            }
            let Some(receipt) = adapter.take_linux_ipa_receipt(self.generation)? else {
                return Ok(LinuxIpaProgress::WaitingCompletion);
            };
            adapter.commit_linux_ipa(
                self.generation,
                self.core
                    .as_mut()
                    .ok_or(failure(LinuxIpaError::Configuration))?,
                &pending.prepared,
                receipt,
            )?;
            self.pending = None;
            self.committed = self.committed.saturating_add(1);
            return Ok(LinuxIpaProgress::Committed);
        }
        if adapter.take_linux_ipa_receipt(self.generation)?.is_some() {
            return Err(failure(LinuxIpaError::UnexpectedReceipt));
        }
        if self.core.is_none() {
            let Some(peer) = peer else {
                return Ok(LinuxIpaProgress::WaitingPeer);
            };
            self.core = Some(ColdSession::new(self.generation, peer).map_err(protocol)?);
            self.peer = Some(peer);
            return Ok(LinuxIpaProgress::Discovered);
        }
        let core = self.core.as_mut().unwrap();
        if self.init_since.is_none() {
            // Separate pristine Linux client/server handles each allocate ID 0
            // first. Remote request IDs do not consume the server allocator.
            let prepared = core
                .prepare_init_driver(self.generation, 0, self.shared)
                .map_err(protocol)?;
            self.init_since = Some(now);
            self.pending = Some(Pending {
                prepared,
                handle: None,
                queued: false,
                since: now,
            });
            return Ok(LinuxIpaProgress::Prepared);
        }
        if let Some(handle) = self.response {
            let (source, bytes) = adapter.linux_ipa_response(self.generation, &handle)?;
            core.receive_init_response(self.generation, source, bytes)
                .map_err(protocol)?;
            adapter.finish_linux_ipa_response(self.generation, &handle)?;
            self.response = None;
            self.accepted = self.accepted.saturating_add(1);
            return Ok(LinuxIpaProgress::ResponseAccepted);
        }
        for handle in requests.iter().flatten() {
            let (context, bytes) = adapter.request_qmi(self.generation, handle)?;
            if context.service == Service::IpaServer {
                let peer = ipa::Address {
                    node: context.peer.node,
                    port: context.peer.port,
                };
                let prepared = core
                    .prepare_response(self.generation, peer, bytes)
                    .map_err(protocol)?;
                #[cfg(feature = "sunfish-ipa-registration-trace")]
                adapter.note_ipa_registration_prepared(*handle);
                self.pending = Some(Pending {
                    prepared,
                    handle: Some(*handle),
                    queued: false,
                    since: now,
                });
                return Ok(LinuxIpaProgress::Prepared);
            }
        }
        if let Some(prepared) = core
            .prepare_indication(self.generation, 0)
            .map_err(protocol)?
        {
            self.pending = Some(Pending {
                prepared,
                handle: None,
                queued: false,
                since: now,
            });
            return Ok(LinuxIpaProgress::Prepared);
        }
        Ok(if core.protocol_complete() {
            LinuxIpaProgress::Complete
        } else {
            LinuxIpaProgress::WaitingHandshake
        })
        }
    }
}
impl<I: Io> Owner<I> {
    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub fn ipa_registration_trace(&self) -> Option<crate::qcom_glink_qmi::IpaRegistrationTrace> {
        self.adapter.ipa_registration_trace()
    }

    #[cfg(feature = "sunfish-ipa-init-close-visibility")]
    pub fn init_close_fifo(&self) -> crate::qcom_glink_fifo::InitCloseFifo {
        self.adapter.init_close_fifo()
    }

    #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
    pub fn registration_close_fifo(&self) -> Option<crate::qcom_glink_fifo::RegistrationCloseFifo> {
        self.adapter.registration_close_fifo()
    }

    #[cfg(feature = "sunfish-ipa-registration-reply-path")]
    pub fn registration_reply_fifo(&self) -> Option<crate::qcom_glink_fifo::RegistrationReplyFifo> {
        self.adapter.registration_reply_fifo()
    }

    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    fn ipa_registration_service_due(&self) -> bool {
        let Some(runtime) = self.linux_ipa.as_ref() else { return false; };
        let Some(trace) = self.adapter.ipa_registration_trace() else { return false; };
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        {
            // An early response held behind INIT's own receipt cannot block
            // the server. An acceptable response still gets the common prelude.
            if runtime.response.is_some()
                && runtime.core.as_ref().is_some_and(ColdSession::init_response_can_accept)
            { return false; }
            // Respect the alternating scheduler when the client may progress.
            // A blocked client gets its ordinary turn, then yields to server.
            if !runtime.server_turn && (runtime.pending.is_some() || runtime.init_since.is_none()) {
                return false;
            }
            if let Some(pending) = &runtime.server_pending {
                return !pending.queued && pending.handle == Some(trace.request);
            }
            runtime.core.is_some()
                && self.adapter.next_ipa_server_request(&self.requests) == Some(trace.request)
        }
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        {
            if runtime.response.is_some() { return false; }
            if let Some(pending) = &runtime.pending {
                return !pending.queued && pending.handle == Some(trace.request);
            }
            runtime.core.is_some()
                && runtime.init_since.is_some()
                && self.adapter.next_ipa_server_request(&self.requests) == Some(trace.request)
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    pub(super) fn yield_for_ipa_registration(&mut self, handle: RequestHandle) -> bool {
        if !self.adapter.ipa_registration_trace().is_some_and(|trace| trace.request == handle)
            || !self.ipa_registration_service_due()
        {
            return false;
        }
        if let (Some(runtime), Some(clock)) = (&mut self.linux_ipa, self.rmtfs_event_clock) {
            runtime.registration_turn_times[0].get_or_insert_with(clock);
        }
        true
    }

    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    pub fn ipa_registration_turn_times(&self) -> [Option<u64>; 2] {
        self.linux_ipa.as_ref().map_or([None; 2], |runtime| runtime.registration_turn_times)
    }

    #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
    pub fn ipa_registration_owner_trace(&self) -> Option<IpaRegistrationOwnerTrace> {
        self.linux_ipa.as_ref()?.registration_owner_trace
    }

    #[cfg(feature = "sunfish-ipa-announcement-timeline")]
    pub fn first_ipa_announcement(&self) -> Option<(u32, Option<u64>)> {
        self.adapter.first_ipa_announcement()
    }

    #[cfg(feature = "sunfish-ipa-init-continuation")]
    pub fn ipa_init_trace(&self) -> (Option<IpaInitOwnerTrace>, Option<crate::qcom_glink_qmi::IpaInitTransportTrace>) {
        (self.linux_ipa.as_ref().map(|runtime| runtime.init_trace), self.adapter.ipa_init_transport_trace())
    }

    /// After discovery, use at most two ordinary local steps to prepare and
    /// queue INIT_DRIVER if the tracked registration is already waiting.
    /// One local admission opportunity after the first registration reply's checked
    /// publication. Leave GLINK writer selection and receipt retirement to dispatch.
    #[cfg(feature = "sunfish-ipa-init-after-registration")]
    pub fn admit_init_after_registration(
        &mut self,
        #[cfg(feature = "sunfish-ipa-init-empty-rx")]
        empty_rx_turn: bool,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        let Some(runtime) = self.linux_ipa.as_mut() else { return Ok(()); };
        if runtime.init_trace.post_registration != PostRegistrationTurn::AwaitingPublication
            || !self.adapter.ipa_registration_trace().is_some_and(|reg| reg.published_at.is_some())
        {
            return Ok(());
        }
        let write = self.adapter.first_server_check();
        runtime.init_trace.post_registration = match write {
            Some(w) if w.peer.port == 0x1f && w.exact_wrq && w.timing.closes != 0 => PostRegistrationTurn::ClientClosed,
            #[cfg(feature = "sunfish-ipa-init-empty-rx")]
            Some(w) if w.peer.port == 0x1f && w.exact_wrq && !empty_rx_turn => PostRegistrationTurn::NoEmptyRxTurn,
            Some(w) if w.peer.port == 0x1f && w.exact_wrq => PostRegistrationTurn::InitIneligible,
            _ => PostRegistrationTurn::DifferentClient,
        };
        if runtime.init_trace.post_registration != PostRegistrationTurn::InitIneligible
            || runtime.core.is_none() || runtime.init_since.is_some() || runtime.pending.is_some()
            || !self.adapter.ipa_modem_discovered(self.generation)
        {
            return Ok(());
        }
        let Some(clock) = self.rmtfs_event_clock else { return Ok(()); };
        // Consume the turn before either fallible step. Flow/slot pressure returns
        // control to the ordinary scheduler with the original prepared record.
        runtime.init_trace.post_registration = PostRegistrationTurn::Deferred;
        if self.service_linux_ipa_step(clock(), interrupts)? == LinuxIpaProgress::Prepared
            && self.linux_ipa.as_ref().is_some_and(|r| r.last_progress_client
                && r.pending.as_ref().and_then(Pending::message_id) == Some(ipa::INIT_DRIVER))
            && self.service_linux_ipa_step(clock(), interrupts)? == LinuxIpaProgress::Queued
            && let Some(r) = self.linux_ipa.as_mut()
            && r.last_progress_client
        {
            r.init_trace.post_registration = PostRegistrationTurn::Queued;
        }
        Ok(())
    }

    #[cfg(feature = "sunfish-ipa-init-continuation")]
    pub fn continue_ipa_init_after_discovery(
        &mut self,
        progress: LinuxIpaProgress,
        clock: fn() -> u64,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        if progress != LinuxIpaProgress::Discovered {
            return Ok(());
        }
        let Some(trace) = self.adapter.ipa_registration_trace() else { return Ok(()); };
        if self.adapter.next_ipa_server_request(&self.requests) != Some(trace.request) {
            return Ok(());
        }
        let Some(runtime) = self.linux_ipa.as_mut() else { return Ok(()); };
        if runtime.core.is_none() || runtime.init_since.is_some() || runtime.pending.is_some() {
            return Ok(());
        }
        runtime.init_trace.extra_steps = 1;
        // Preserve the existing bounded INIT admission after discovery. These
        // two local attempts never wait for INIT retirement before server work.
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        { runtime.server_turn = false; }
        if self.service_linux_ipa_step(clock(), interrupts)? == LinuxIpaProgress::Prepared {
            if let Some(runtime) = self.linux_ipa.as_mut() {
                runtime.init_trace.extra_steps = 2;
                #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
                { runtime.server_turn = false; }
            }
            self.service_linux_ipa_step(clock(), interrupts)?;
        }
        Ok(())
    }

    #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
    pub(super) fn observe_ipa_registration_route(&mut self, handle: RequestHandle) {
        if !self.adapter.ipa_registration_trace().is_some_and(|trace| trace.request == handle) {
            return;
        }
        let eligible = self.ipa_registration_service_due();
        let Some(runtime) = self.linux_ipa.as_mut().filter(|runtime| runtime.registration_owner_trace.is_none())
        else { return; };
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let pending = runtime.pending.as_ref().or(runtime.server_pending.as_ref());
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        let pending = runtime.pending.as_ref();
        let next = self.adapter.next_ipa_server_request(&self.requests) == Some(handle);
        let flags = [
            runtime.core.is_some(),
            runtime.init_since.is_some(),
            pending.is_some(),
            pending.is_some_and(|record| record.queued),
            self.adapter.linux_ipa_receipt_pending() == Some(true),
            runtime.response.is_some(),
            next,
            eligible,
            runtime.core.as_ref().is_some_and(|core| core.state().modem_ready),
        ];
        runtime.registration_owner_trace = Some(IpaRegistrationOwnerTrace {
            route_flags: flags.into_iter().enumerate().fold(0, |mask, (bit, set)| {
                mask | (u32::from(set) << bit)
            }),
            route_pending_id: pending.and_then(Pending::message_id),
            first_step: None,
        });
    }

    #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
    pub(super) fn yield_for_cold_ipa_registration(&mut self) -> bool {
        let Some(runtime) = &mut self.linux_ipa else { return false; };
        let Some(trace) = self.adapter.ipa_registration_trace() else { return false; };
        if runtime.core.is_some() || runtime.cold_registration_handoff.is_some()
            || self.adapter.next_ipa_server_request(&self.requests) != Some(trace.request)
            || !self.adapter.ipa_modem_discovered(self.generation)
        {
            return false;
        }
        runtime.cold_registration_handoff = Some(true);
        if let Some(clock) = self.rmtfs_event_clock {
            runtime.registration_turn_times[0].get_or_insert_with(clock);
        }
        true
    }

    #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
    pub fn service_cold_ipa_registration(
        &mut self, clock: fn() -> u64, interrupts: &mut impl Interrupts,
    ) -> Result<bool, Error> {
        self.check()?;
        let Some(runtime) = &mut self.linux_ipa else { return Ok(false); };
        if runtime.cold_registration_handoff != Some(true) {
            return Ok(false);
        }
        runtime.cold_registration_handoff = Some(false);
        if runtime.core.is_some() {
            return Ok(false);
        }
        let now = clock();
        runtime.registration_turn_times[1].get_or_insert(now);
        if self.service_linux_ipa_step(now, interrupts)? == LinuxIpaProgress::Discovered
            && self.service_linux_ipa_step(clock(), interrupts)? == LinuxIpaProgress::Prepared
        {
            // The ordinary scheduler alternates after preparation. Borrow
            // exactly one more server step for admission before yielding.
            self.linux_ipa.as_mut().unwrap().server_turn = true;
            self.service_linux_ipa_step(clock(), interrupts)?;
        }
        Ok(true)
    }

    /// Run one ordinary IPA service step at the start of the service pass
    /// when the tracked registration is the next work it would select.
    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    pub fn service_ipa_registration_early(
        &mut self,
        now: u64,
        interrupts: &mut impl Interrupts,
    ) -> Result<bool, Error> {
        self.check()?;
        if !self.ipa_registration_service_due() {
            return Ok(false);
        }
        if let Some(runtime) = &mut self.linux_ipa {
            runtime.registration_turn_times[1].get_or_insert(now);
        }
        self.service_linux_ipa_step(now, interrupts)?;
        Ok(true)
    }

    /// Attach the real FIFO adapter to an already prepared Linux-profile
    /// session. The native outer owner must retain the IPA startup and host
    /// service resources used to prepare that session; no old readiness permit
    /// is converted or manufactured here. Start QRTR control separately.
    pub fn attach_linux_ipa(
        adapter: Adapter<I>,
        generation: u64,
        shared: u32,
        counter_hz: u64,
    ) -> Result<Self, Error> {
        Self::attach_linux_ipa_retained(adapter, generation, shared, counter_hz)
            .map_err(|(error, _retained)| error)
    }

    /// Attach while returning the quarantined adapter on failure. The native
    /// startup owner uses this form so an invalid session or generation never
    /// drops the sole retained SMMU/SMEM gate during a phase transition.
    pub fn attach_linux_ipa_retained(
        mut adapter: Adapter<I>,
        generation: u64,
        shared: u32,
        counter_hz: u64,
    ) -> Result<Self, (Error, Adapter<I>)> {
        let runtime = match LinuxRuntime::new(generation, shared, counter_hz) {
            Ok(runtime) => runtime,
            Err(error) => return Err((Error::LinuxIpa(error), adapter)),
        };
        let prepared = match adapter.linux_ipa_prepared(generation) {
            Ok(prepared) => prepared,
            Err(error) => {
                adapter.suspend();
                return Err((error.into(), adapter));
            }
        };
        if !prepared {
            adapter.suspend();
            return Err((Error::LinuxIpa(LinuxIpaError::Configuration), adapter));
        }
        if let Err(error) = adapter.revalidate(generation) {
            return Err((error.into(), adapter));
        }
        Ok(Self {
            #[cfg(feature = "sunfish-linux-rmtfs")]
            linux_rmtfs_id: None,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_large_reply: None,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_large_reply_flags: 0,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_event_clock: None,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_event_times: [None; 5],
            #[cfg(feature = "sunfish-tftp-timing-observer")]
            tftp_timing: super::TftpTiming::default(),
            linux_ipa: Some(runtime),
            adapter,
            generation,
            stopped: false,
            error: None,
            pending_irqs: 0,
            scheduled: false,
            requests: [None; MAX_REQUESTS],
            tftp_requests: [None; MAX_REQUESTS],
            #[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
            tftp_yield_debt: false,
            #[cfg(feature = "sunfish-ipa-registration-rx-first")]
            first_oack_rx_passes: 0,
            #[cfg(feature = "sunfish-first-oack-owner-turn")]
            first_oack_initial_turn_used: false,
            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
            pending_tftp_close: None,
            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
            tftp_close_commits: 0,
            #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
            test_fail_close_owner_stage: 0,
            #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
            tftp_owner_snapshot: [(None, 0); crate::qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS],
            #[cfg(feature = "sunfish-tftp-close-owner-fence")]
            firmware_peers: [None; crate::qcom_qrtr_tftp::MAX_ACTIVE_TRANSFERS],
            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
            pending_firmware_close: None,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            queue_full_kind: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wrq_coalesced: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wire_counts: [0; 4],
            #[cfg(feature = "sunfish-wlan-read-sequence")]
            read_sequence: Default::default(),
            #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
            first_wlan_rrq_at: None,
            #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
            first_wlfw_server_at: None,
            #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
            first_rrq: None,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wait_reason: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wait_tx: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wait_flows: 0,
            tftp_output: [0; crate::qcom_tftp::MAX_REQUEST_BYTES],
            pending_tftp_reply: None,
            #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
            peer_tftp_replies: [None; MAX_REQUESTS],
            #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
            peer_tftp_cursor: 0,
            handlers_id: None,
            service_registry_trace: ServiceRegistryTrace::new(),
            first_service_registry_reply_handle: None,
            #[cfg(feature = "sunfish-wlan-startup-trace")]
            first_registry_replies_by_name: [None; crate::qcom_servreg::NAME_COUNTER_COUNT],
            first_service_registry_reply_qrtr_resume_handle: None,
            #[cfg(feature = "sunfish-wlan-elf-timing")]
            elf_lookup_handle: None,
            #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
            registry_txn4_handle: None,
            tftp_trace: TftpObserverTrace::new(),
        })
    }
    pub fn linux_ipa_report(&self) -> Option<LinuxIpaReport> {
        self.linux_ipa
            .as_ref()
            .map(|runtime| runtime.report(self.stopped))
    }
    pub(super) fn observe_linux_ipa(&mut self, event: Event) -> Result<(), Error> {
        #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
        if matches!(event, Event::LinuxIpaInitResponseIgnored) {
            if let Some(runtime) = self.linux_ipa.as_mut() {
                runtime.ignored_init_responses = runtime.ignored_init_responses.saturating_add(1);
            }
        }
        #[cfg(feature = "sunfish-ipa-ignore-unhandled-reference")]
        if let Event::LinuxIpaUnhandledIgnored { message_id } = event {
            if let Some(runtime) = self.linux_ipa.as_mut() {
                runtime.ignored_unhandled_requests = runtime.ignored_unhandled_requests.saturating_add(1);
                runtime.last_ignored_unhandled_id = Some(message_id);
            }
        }
        #[cfg(feature = "sunfish-ipa-ignore-config-reference")]
        if matches!(event, Event::LinuxIpaConfigIgnored) {
            if let Some(runtime) = self.linux_ipa.as_mut() {
                runtime.ignored_config_requests = runtime.ignored_config_requests.saturating_add(1);
            }
        }
        if let Event::ReceiveIntentRequest { granted } = event {
            if let Some(runtime) = self.linux_ipa.as_mut() {
                runtime.peer_intent_requests = runtime.peer_intent_requests.saturating_add(1);
                if !granted { runtime.peer_intent_denied = runtime.peer_intent_denied.saturating_add(1); }
            }
        }
        if let Event::LinuxIpaResponse(handle) = event {
            #[cfg(feature = "sunfish-linux-rmtfs")]
            let observed = self.rmtfs_event_clock.map(|clock| clock());
            #[cfg(not(feature = "sunfish-linux-rmtfs"))]
            let observed = None;
            let runtime = self
                .linux_ipa
                .as_mut()
                .ok_or(Error::LinuxIpa(LinuxIpaError::Configuration))?;
            if runtime.response.is_some() {
                return Err(Error::LinuxIpa(LinuxIpaError::UnexpectedResponse));
            }
            runtime.response = Some(handle);
            runtime.response_observed = observed;
            #[cfg(feature = "sunfish-ipa-independent-server-runtime")]
            { runtime.init_trace.response_at = observed; }
        }
        Ok(())
    }
    /// One bounded protocol action after native transport/handler work. `now`
    /// must come from the same physical counter described by counter_hz.
    /// Arrange a timer wake for report.next_deadline when the peer is silent;
    /// waiting on completion does not reschedule a busy-spin continuation.
    pub fn service_linux_ipa_step(
        &mut self,
        now: u64,
        interrupts: &mut impl Interrupts,
    ) -> Result<LinuxIpaProgress, Error> {
        self.check()?;
        self.stopped = true;
        let result = match self.linux_ipa.as_mut() {
            Some(runtime) => {
                #[cfg(feature = "sunfish-ipa-init-continuation")]
                let pending_init = runtime.pending.as_ref()
                    .is_some_and(|pending| pending.message_id() == Some(ipa::INIT_DRIVER));
                let result = runtime.step(&mut self.adapter, &mut self.requests, now);
                #[cfg(feature = "sunfish-ipa-init-continuation")]
                match result.as_ref() {
                    Ok(LinuxIpaProgress::Discovered) => {
                        runtime.init_trace.discovered_at = self.rmtfs_event_clock.map(|clock| clock());
                    }
                    Ok(LinuxIpaProgress::Prepared) if {
                        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
                        { runtime.last_progress_client }
                        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
                        { true }
                    } && runtime.init_since == Some(now)
                        && runtime.init_trace.prepared_at.is_none()
                        && runtime.pending.as_ref().and_then(Pending::message_id) == Some(ipa::INIT_DRIVER) =>
                    {
                        runtime.init_trace.prepared_at = self.rmtfs_event_clock.map(|clock| clock());
                    }
                    Ok(LinuxIpaProgress::WaitingQueue) if pending_init && {
                        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
                        { runtime.last_progress_client }
                        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
                        { true }
                    } => {
                        runtime.init_trace.queue_waits = runtime.init_trace.queue_waits.saturating_add(1);
                    }
                    Ok(LinuxIpaProgress::Committed) if pending_init && {
                        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
                        { runtime.last_progress_client }
                        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
                        { true }
                    } => {
                        runtime.init_trace.committed_at = self.rmtfs_event_clock.map(|clock| clock());
                    }
                    _ => {}
                }
                #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
                if let Some(trace) = runtime.registration_owner_trace.as_mut()
                    && trace.first_step.is_none()
                {
                    trace.first_step = Some(IpaRegistrationOwnerStep {
                        started_at: now,
                        ended_at: self.rmtfs_event_clock.map(|clock| clock()),
                        progress: result.as_ref().ok().copied(),
                    });
                }
                result
            }
            None => Err(Error::LinuxIpa(LinuxIpaError::Configuration)),
        };
        let progress = match result {
            Ok(value) => {
                self.stopped = false;
                value
            }
            Err(error) => return Err(self.stop(error)),
        };
        if matches!(
            progress,
            LinuxIpaProgress::Discovered
                | LinuxIpaProgress::Prepared
                | LinuxIpaProgress::Queued
                | LinuxIpaProgress::Committed
                | LinuxIpaProgress::ResponseAccepted
        ) {
            self.schedule(interrupts)?;
        }
        Ok(progress)
    }
}
