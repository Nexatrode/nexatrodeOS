//! Opt-in Linux IPA messages over the existing bounded GLINK/QRTR session.
//! This child module can retire real transport slots but cannot create an IPA
//! hardware permit. Legacy mode requires GLINK RX_DONE and QRTR RESUME_TX.
//! The opt-in Linux flow mode treats QRTR resume as destination credit while
//! still requiring real GLINK RX_DONE. Early responses remain held.
#![forbid(unsafe_code)]

use super::*;
use crate::sunfish_ipa_linux_qmi::{self as ipa, ColdSession, Prepared, PublicationId, Socket};

pub(super) struct LinuxIpa {
    client: Address,
    last: Option<PublicationId>,
    peer: Option<Address>,
    completed: Option<LinuxIpaReceipt>,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    server_last: Option<PublicationId>,
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    server_completed: Option<LinuxIpaReceipt>,
    #[cfg(feature = "sunfish-ipa-registration-trace")]
    registration: Option<IpaRegistrationTrace>,
    #[cfg(feature = "sunfish-ipa-init-continuation")]
    init_transport: Option<IpaInitTransportTrace>,
    #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
    published_indication: Option<(Address, u16)>,
}
impl LinuxIpa {
    pub(super) fn pristine(&self) -> bool {
        #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
        if self.published_indication.is_some() { return false; }
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        if self.server_last.is_some() || self.server_completed.is_some() { return false; }
        self.last.is_none() && self.completed.is_none()
    }
    pub(super) fn reset(&mut self) {
        self.last = None;
        self.peer = None;
        self.completed = None;
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        { self.server_last = None; self.server_completed = None; }
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        { self.registration = None; }
        #[cfg(feature = "sunfish-ipa-init-continuation")]
        { self.init_transport = None; }
        #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
        { self.published_indication = None; }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxIpaHandle {
    owner: u64,
    generation: u64,
    intent: u32,
}
/// Only this session's matching transport retirement can create a receipt.
/// It is neither Clone nor a native startup/firmware/DMA readiness capability.
#[derive(Debug)]
pub struct LinuxIpaReceipt {
    owner: u64,
    generation: u64,
    id: PublicationId,
}

/// Private-buffer metadata only; it neither retires a send nor accepts a reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxIpaTxDiagnostic {
    pub len: u32,
    pub sent: u32,
    /// bit 0 GLINK RX_DONE, bit 1 QRTR RESUME, bit 2 confirm requested,
    /// bit 3 remote intent currently attached, bit 4 response held.
    pub flags: u32,
    pub remote_intent: u32,
    pub tx_transaction: u32,
    pub response_transaction: u32,
    pub response_result: u32,
    pub response_error: u32,
}

/// Host observations for the first IPA indication registration transaction.
#[cfg(feature = "sunfish-ipa-registration-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpaRegistrationTrace {
    pub(crate) request: RequestHandle,
    publication: Option<PublicationId>,
    pub peer_port: u32,
    pub local_port: u32,
    pub transaction: u16,
    pub received_at: Option<u64>,
    pub routed_at: Option<u64>,
    pub prepared_at: Option<u64>,
    pub first_queue_attempt_at: Option<u64>,
    pub queued_at: Option<u64>,
    pub published_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub remote_intent: Option<u32>,
    pub prior_message: Option<u16>,
    pub flow_waits: u32,
    pub capacity_waits: u32,
    #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
    pub tx: IpaRegistrationTxTrace,
}

/// Writer decisions between receipt of the first registration and its RX_DONE.
#[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistrationDoneTrace {
    pub first_pass_at: Option<u64>,
    pub first_eligible_at: Option<u64>,
    pub passes_at_close: Option<u32>,
    /// Overlapping counts: protected data, WRQ RX_DONE, grant, credit,
    /// registration not queued, queued registration not ready.
    pub gates: [u32; 6],
    pub passes: u32,
    /// Four words of ten 3-bit writer decisions, earliest in the low bits.
    /// 0 idle, 1 advertisement, 2 intent ACK, 3 other RX_DONE,
    /// 4 QRTR credit, 5 other DATA, 6 intent request, 7 target RX_DONE.
    pub turns: [u32; 4],
    pub failed_turns: u64,
    pub failures: u32,
    pub finished: bool,
}

/// Observations for the exact first registration response, frozen at publication.
#[cfg(feature = "sunfish-ipa-registration-tx-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IpaRegistrationTxTrace {
    pub passes: u32,
    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub(super) priority_attempted: bool,
    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub priority_deferrals: [u32; 6],
    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub request_done_at: Option<u64>,
    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub priority_selected: bool,
    #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
    pub request_done_attempted: bool,
    #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
    pub request_done: RegistrationDoneTrace,
    /// Overlapping pass counts: outbound intent request waiting, no fitting
    /// free intent, older same-flow data, target ready, pending peer ACK,
    /// advertising receive intent, pending receive completion, ready QRTR credit.
    pub gates: [u32; 8],
    /// Successful competing wire commands 5, 6, 7, 8, 14, other non-DATA.
    pub commands: [u32; 6],
    pub data_ahead: u32,
    pub idle: u32,
    pub target_blocked: u32,
    pub other_blocked: u32,
    pub uncertain: u32,
    pub first_pass_at: Option<u64>,
    pub selected_at: Option<u64>,
    #[cfg(feature = "sunfish-reg-writer-timeline")]
    pub committed_write_start: Option<u64>,
    pub last_end: Option<u64>,
    /// Measured selection/writer interval within publication calls, not the
    /// complete router callback. Writer time is a subset of active time.
    pub active_ticks: u64,
    pub writer_ticks: u64,
    pub max_gap_ticks: u64,
    pub clock_invalid: bool,
}

#[cfg(feature = "sunfish-ipa-init-continuation")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpaInitTransportTrace {
    publication: PublicationId,
    pub queued_at: Option<u64>,
    pub published_at: Option<u64>,
    pub retired_at: Option<u64>,
    #[cfg(feature = "sunfish-ipa-init-tx-trace")]
    pub tx: IpaInitTxTrace,
}

#[cfg(feature = "sunfish-ipa-init-tx-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IpaInitTxTrace {
    #[cfg(feature = "sunfish-ipa-init-open-writer")]
    pub queue_state: u32,
    #[cfg(feature = "sunfish-ipa-init-before-ad")]
    pub(super) queued_grant: Option<u32>,
    /// Passes, no-fit passes, controls, other DATA and observed gate mask
    /// at the normal decoder's first DEL_CLIENT for the original 0x1f WRQ.
    #[cfg(feature = "sunfish-ipa-init-open-writer")]
    pub at_first_close: Option<[u32; 5]>,
    pub router_at: Option<u64>,
    pub first_pass_at: Option<u64>,
    pub first_writer_at: Option<u64>,
    pub selected_at: Option<u64>,
    #[cfg(feature = "sunfish-ipa-init-writer-fit")]
    pub first_fit_at: Option<u64>,
    #[cfg(feature = "sunfish-ipa-init-writer-fit")]
    pub data_kinds: [u32; 8],
    pub passes: u32,
    pub control_ahead: u32,
    pub data_ahead: u32,
    pub no_fit: u32,
    pub target_blocked: u32,
    /// Bit 0 records the one priority attempt and bit 1 its successful write.
    #[cfg(feature = "sunfish-ipa-init-tx-turn")]
    pub priority_turn: u32,
    /// Bits 0–3: intent request waiting, older same-flow packet, other
    /// record blocked by FIFO space, uncertain writer result.
    pub flags: u32,
    /// Per-pass overlapping blockers: ACK, WRQ completion, TFTP, registry,
    /// credit, target not ready, priority already used, priority eligible.
    #[cfg(feature = "sunfish-ipa-init-writer-counts")]
    pub gates: [u32; 8],
    /// Successful competing wire commands: 5, 6, 7, 8, 14, other (not DATA).
    #[cfg(feature = "sunfish-ipa-init-writer-counts")]
    pub commands: [u32; 6],
}

impl Session {
    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub fn ipa_registration_trace(&self) -> Option<IpaRegistrationTrace> {
        self.linux_ipa.as_ref()?.registration
    }

    #[cfg(feature = "sunfish-ipa-init-continuation")]
    pub(crate) fn ipa_init_transport_trace(&self) -> Option<IpaInitTransportTrace> {
        self.linux_ipa.as_ref()?.init_transport
    }

    #[cfg(feature = "sunfish-ipa-init-tx-trace")]
    fn pending_ipa_init_trace(&mut self) -> Option<&mut IpaInitTxTrace> {
        self.linux_ipa.as_mut()?.init_transport.as_mut()
            .filter(|trace| trace.published_at.is_none() && trace.retired_at.is_none())
            .map(|trace| &mut trace.tx)
    }

    #[cfg(feature = "sunfish-ipa-init-tx-trace")]
    pub(crate) fn observe_ipa_init_router(&mut self) {
        let clock = self.intent_ack_clock;
        if let Some(trace) = self.pending_ipa_init_trace()
            && trace.router_at.is_none()
        {
            trace.router_at = clock.map(|clock| clock());
        }
    }

    #[cfg(feature = "sunfish-ipa-init-tx-trace")]
    pub(super) fn queued_ipa_init_slot(&self) -> Option<usize> {
        let trace = self.linux_ipa.as_ref()?.init_transport.as_ref()?;
        if trace.published_at.is_some() {
            return None;
        }
        self.transmit.iter().position(|tx| {
            tx.sent < tx.len && matches!(tx.kind, TxKind::LinuxIpa { id, .. } if id == trace.publication)
        })
    }

    #[cfg(feature = "sunfish-ipa-init-open-writer")]
    pub(super) fn observe_init_at_first_close(&mut self) {
        if let Some(init) = self.linux_ipa.as_mut().and_then(|profile| profile.init_transport.as_mut()) {
            let tx = &mut init.tx;
            let mut mask = 0;
            for (bit, count) in tx.gates.iter().enumerate() {
                if *count != 0 { mask |= 1 << bit; }
            }
            tx.at_first_close = Some([tx.passes, tx.no_fit, tx.control_ahead, tx.data_ahead, mask]);
        }
    }

    #[cfg(feature = "sunfish-ipa-init-writer-counts")]
    pub(super) fn observe_ipa_schedule_gates(&mut self, gates: [bool; 8]) {
        if self.queued_ipa_init_slot().is_none() { return; }
        if let Some(trace) = self.pending_ipa_init_trace() {
            for (count, gate) in trace.gates.iter_mut().zip(gates) {
                *count = count.saturating_add(u32::from(gate));
            }
        }
    }

    #[cfg(feature = "sunfish-ipa-init-writer-counts")]
    pub(super) fn observe_ipa_schedule_command(&mut self, record: &[u8], result: Result<(), WriteError>) {
        if result.is_err() || record.len() < 2 || self.queued_ipa_init_slot().is_none() { return; }
        let command = u16::from_le_bytes([record[0], record[1]]);
        if command == 9 || command == 12 { return; }
        if let Some(trace) = self.pending_ipa_init_trace() {
            let slot = match command { 5 => 0, 6 => 1, 7 => 2, 8 => 3, 14 => 4, _ => 5 };
            trace.commands[slot] = trace.commands[slot].saturating_add(1);
        }
    }

    #[cfg(feature = "sunfish-ipa-init-tx-turn")]
    pub(super) fn next_ipa_init_transmit(&self) -> Option<(usize, usize)> {
        let trace = self.linux_ipa.as_ref()?.init_transport.as_ref()?;
        if trace.tx.priority_turn != 0 {
            return None;
        }
        let slot = self.queued_ipa_init_slot()?;
        let tx = &self.transmit[slot];
        if tx.sent != 0 || tx.len > TX_CHUNK_BYTES {
            return None;
        }
        Some((slot, self.ready_remote_for_transmit(tx)?))
    }

    #[cfg(feature = "sunfish-ipa-init-tx-turn")]
    pub(super) fn observe_ipa_init_priority(&mut self, result: Result<(), WriteError>) {
        if let Some(trace) = self.pending_ipa_init_trace() {
            trace.priority_turn = 1 | (u32::from(result.is_ok()) << 1);
        }
    }

    #[cfg(feature = "sunfish-ipa-init-tx-trace")]
    pub(super) fn observe_ipa_init_publish_pass(&mut self) -> Option<usize> {
        let slot = self.queued_ipa_init_slot()?;
        let tx = &self.transmit[slot];
        let no_fit = self.remote_for_transmit(tx).is_none();
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        let intent_wait = matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. });
        #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
        let intent_wait = false;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let older_flow = self.unsent_flow_predecessor(tx);
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        let older_flow = false;
        let clock = self.intent_ack_clock;
        let trace = self.pending_ipa_init_trace()?;
        if trace.passes == 0 {
            trace.first_pass_at = clock.map(|clock| clock());
        }
        #[cfg(feature = "sunfish-ipa-init-writer-fit")]
        if !no_fit && trace.first_fit_at.is_none() {
            trace.first_fit_at = clock.map(|now| now());
        }
        trace.passes = trace.passes.saturating_add(1);
        trace.no_fit = trace.no_fit.saturating_add(u32::from(no_fit));
        trace.flags |= u32::from(intent_wait) | (u32::from(older_flow) << 1);
        Some(slot)
    }

    #[cfg(feature = "sunfish-ipa-init-tx-trace")]
    pub(super) fn observe_ipa_init_writer(
        &mut self,
        target: usize,
        choice: &PublishSelection,
        started: Option<u64>,
        result: Result<(), WriteError>,
    ) {
        #[cfg(feature = "sunfish-ipa-init-writer-fit")]
        let ahead_kind = match choice {
            PublishSelection::Data(slot) if *slot != target && result.is_ok() => {
                Some(self.transmit[*slot].kind.diagnostic_kind())
            }
            _ => None,
        };
        let Some(trace) = self.pending_ipa_init_trace() else { return; };
        #[cfg(feature = "sunfish-ipa-init-writer-fit")]
        if let Some(kind) = ahead_kind {
            let count = &mut trace.data_kinds[usize::from(kind - 1)];
            *count = count.saturating_add(1);
        }
        if trace.first_writer_at.is_none() {
            trace.first_writer_at = started;
        }
        let selected = matches!(choice, PublishSelection::Data(slot) if *slot == target);
        if selected && trace.selected_at.is_none() {
            trace.selected_at = started;
        }
        match result {
            Ok(()) if !selected => match choice {
                PublishSelection::Data(_) => trace.data_ahead = trace.data_ahead.saturating_add(1),
                _ => trace.control_ahead = trace.control_ahead.saturating_add(1),
            },
            Err(WriteError::WouldBlock) if selected => {
                trace.target_blocked = trace.target_blocked.saturating_add(1);
            }
            Err(WriteError::WouldBlock) => trace.flags |= 1 << 2,
            Err(WriteError::Uncertain) => trace.flags |= 1 << 3,
            Ok(()) => {}
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    pub(crate) fn next_ipa_server_request(
        &self,
        requests: &[Option<RequestHandle>; MAX_REQUESTS],
    ) -> Option<RequestHandle> {
        requests.iter().flatten().copied().find(|handle| {
            self.request_index(handle).ok()
                .and_then(|index| self.receive[index].context)
                .is_some_and(|context| context.service == Service::IpaServer)
        })
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(super) fn observe_ipa_registration_receive(
        &mut self, index: usize, len: usize, peer: Address, local: Address,
    ) {
        let Some(profile) = self.linux_ipa.as_mut() else { return; };
        if profile.registration.is_some()
            || !self.router.endpoint(Service::IpaServer).is_some_and(|endpoint| endpoint.address == local)
            || peer.node != self.config.peer_node || peer.port == 0 || peer.port >= control::CONTROL_PORT
        {
            return;
        }
        let rx = &self.receive[index];
        if word(&rx.bytes, 0) != 1 || word(&rx.bytes, 16) > 1 {
            return;
        }
        let Ok(message) = Message::decode(&rx.bytes[32..len]) else { return; };
        if message.message_id != ipa::INDICATION_REGISTER || ipa::validate_server_request(&message).is_err() {
            return;
        }
        let prior_message = self.transmit.iter()
            .find(|tx| tx.len >= 37 && matches!(tx.kind, TxKind::LinuxIpa { .. }))
            .map(|tx| u16::from_le_bytes([tx.bytes[35], tx.bytes[36]]));
        profile.registration = Some(IpaRegistrationTrace {
            request: RequestHandle { owner: self.owner, generation: self.generation, intent: rx.id },
            publication: None,
            peer_port: peer.port,
            local_port: local.port,
            transaction: message.transaction,
            received_at: self.intent_ack_clock.map(|clock| clock()),
            routed_at: None,
            prepared_at: None,
            first_queue_attempt_at: None,
            queued_at: None,
            published_at: None,
            completed_at: None,
            remote_intent: None,
            prior_message,
            flow_waits: 0,
            capacity_waits: 0,
            #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
            tx: IpaRegistrationTxTrace::default(),
        });
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    pub(super) fn registration_reply_in_flight(&self) -> bool {
        let Some(publication) = self.linux_ipa.as_ref()
            .and_then(|profile| profile.registration.as_ref()?.publication)
        else { return false };
        self.transmit.iter().any(|tx| matches!(tx.kind, TxKind::LinuxIpa { id, .. } if id == publication)
            && tx.len != 0 && tx.sent == tx.len && !tx.glink_done)
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
    pub(super) fn registration_tx_slot(&self) -> Option<usize> {
        let trace = self.linux_ipa.as_ref()?.registration.as_ref()?;
        let publication = trace.publication?;
        if trace.published_at.is_some() || trace.completed_at.is_some() { return None; }
        self.transmit.iter().position(|tx| tx.len > tx.sent
            && matches!(tx.kind, TxKind::LinuxIpa { id, .. } if id == publication))
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub(super) fn next_ipa_registration_transmit(&self) -> Option<(usize, usize)> {
        let registration = self.linux_ipa.as_ref()?.registration.as_ref()?;
        let slot = self.registration_tx_slot()?;
        let tx = &self.transmit[slot];
        // Preserve the receive completion for this request and send only
        // whole datagrams on the borrowed turn.
        if registration.tx.priority_attempted || tx.sent != 0 || tx.len > TX_CHUNK_BYTES
            || self.receive.iter().any(|rx| rx.id == registration.request.intent && rx.done_pending)
        {
            return None;
        }
        Some((slot, self.ready_remote_for_transmit(tx)?))
    }

    #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
    pub(super) fn next_ipa_registration_done(&self) -> Option<usize> {
        let registration = self.linux_ipa.as_ref()?.registration.as_ref()?;
        let tx = &self.transmit[self.registration_tx_slot()?];
        if registration.tx.request_done_attempted || tx.sent != 0 || tx.len > TX_CHUNK_BYTES {
            return None;
        }
        self.ready_remote_for_transmit(tx)?;
        self.receive.iter().position(|rx| rx.id == registration.request.intent
            && rx.state == RxState::ReplyQueued && rx.done_pending)
    }

    #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
    pub(super) fn observe_registration_done_pass(
        &mut self,
        gates: [bool; 4],
        eligible: bool,
    ) {
        let Some(registration) = self.ipa_registration_trace() else { return; };
        if registration.tx.request_done.finished { return; }
        let target = self.registration_tx_slot();
        let not_ready = target.is_some() && self.next_ipa_registration_done().is_none();
        let clock = self.intent_ack_clock;
        let close_seen = self.server_check_oacks()[0].is_some_and(|trace| trace.first_close_at.is_some());
        let Some(registration) = self.linux_ipa.as_mut().and_then(|ipa| ipa.registration.as_mut())
        else { return; };
        let trace = &mut registration.tx.request_done;
        if close_seen {
            trace.passes_at_close.get_or_insert(trace.passes);
        }
        if trace.passes == 0 {
            trace.first_pass_at = clock.map(|now| now());
        }
        if eligible && trace.first_eligible_at.is_none() {
            trace.first_eligible_at = clock.map(|now| now());
        }
        for (count, blocked) in trace.gates.iter_mut().zip(
            gates.into_iter().chain([target.is_none(), not_ready])
        ) {
            *count = count.saturating_add(u32::from(blocked));
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
    pub(super) fn observe_registration_done_turn(
        &mut self,
        turn: u32,
        result: Result<(), WriteError>,
    ) {
        let Some(registration) = self.linux_ipa.as_mut().and_then(|ipa| ipa.registration.as_mut())
        else { return; };
        let trace = &mut registration.tx.request_done;
        if trace.finished { return; }
        if trace.passes < 40 {
            trace.turns[(trace.passes / 10) as usize] |= turn << (3 * (trace.passes % 10));
            if result.is_err() {
                trace.failed_turns |= 1 << trace.passes;
            }
        }
        trace.passes = trace.passes.saturating_add(1);
        trace.failures = trace.failures.saturating_add(u32::from(result.is_err()));
        trace.finished = turn == 7 && result.is_ok();
    }

    #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
    pub(super) fn mark_registration_done_attempt(&mut self) {
        if let Some(registration) = self.linux_ipa.as_mut().and_then(|ipa| ipa.registration.as_mut()) {
            registration.tx.request_done_attempted = true;
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub(super) fn observe_registration_priority(&mut self, reason: Option<usize>) {
        let Some(reason) = reason else { return; };
        if let Some(registration) = self.linux_ipa.as_mut().and_then(|ipa| ipa.registration.as_mut()) {
            let count = &mut registration.tx.priority_deferrals[reason];
            *count = count.saturating_add(1);
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub(super) fn note_registration_request_done(&mut self, intent: u32) {
        if let Some(registration) = self.linux_ipa.as_mut().and_then(|ipa| ipa.registration.as_mut())
            .filter(|registration| registration.request.intent == intent)
        {
            registration.tx.request_done_at = self.intent_ack_clock.map(|clock| clock());
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
    pub(super) fn mark_registration_priority_attempt(&mut self) {
        if let Some(registration) = self.linux_ipa.as_mut().and_then(|ipa| ipa.registration.as_mut()) {
            registration.tx.priority_attempted = true;
            registration.tx.priority_selected = true;
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
    pub(super) fn observe_registration_pass(&mut self, started: Option<u64>) -> Option<usize> {
        let slot = self.registration_tx_slot()?;
        #[cfg(feature = "sunfish-ipa-registration-reply-path")]
        if !self.ipa_registration_trace()?.tx.request_done.finished {
            return None;
        }
        let tx = &self.transmit[slot];
        let waiting = matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. });
        let gates = [waiting, self.remote_for_transmit(tx).is_none(), self.unsent_flow_predecessor(tx),
            self.ready_remote_for_transmit(tx).is_some(), self.intent_request_ack.is_some(),
            self.receive.iter().any(|rx| rx.state == RxState::Advertising),
            self.receive.iter().any(|rx| rx.done_pending), self.next_transmit_in_order([1]).is_some()];
        let trace = &mut self.linux_ipa.as_mut()?.registration.as_mut()?.tx;
        trace.passes = trace.passes.saturating_add(1);
        for (count, gate) in trace.gates.iter_mut().zip(gates) { *count = count.saturating_add(u32::from(gate)); }
        if trace.first_pass_at.is_none() { trace.first_pass_at = started; }
        if let Some(last) = trace.last_end {
            if let Some(gap) = started.and_then(|now| now.checked_sub(last)) {
                trace.max_gap_ticks = trace.max_gap_ticks.max(gap);
            } else { trace.clock_invalid = true; }
        }
        Some(slot)
    }

    #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
    pub(super) fn observe_registration_write(&mut self, slot: Option<usize>, started: Option<u64>,
        writer_started: Option<u64>, choice: PublishSelection, bytes: &[u8], result: Result<(), WriteError>) {
        let Some(slot) = slot.filter(|slot| self.registration_tx_slot() == Some(*slot)) else { return; };
        let ended = self.intent_ack_clock.map(|clock| clock());
        let trace = &mut self.linux_ipa.as_mut().unwrap().registration.as_mut().unwrap().tx;
        if let Some(ticks) = started.zip(ended).and_then(|(a,b)| b.checked_sub(a)) {
            trace.active_ticks = trace.active_ticks.saturating_add(ticks);
        } else { trace.clock_invalid = true; }
        if let Some(start) = writer_started {
            if let Some(ticks) = ended.and_then(|end| end.checked_sub(start)) {
                trace.writer_ticks = trace.writer_ticks.saturating_add(ticks);
            } else { trace.clock_invalid = true; }
        }
        trace.last_end = ended;
        let selected = matches!(choice, PublishSelection::Data(index) if index == slot);
        if selected && trace.selected_at.is_none() { trace.selected_at = writer_started; }
        #[cfg(feature = "sunfish-reg-writer-timeline")]
        if selected && result.is_ok() {
            // For a fragmented reply the final successful fragment supplies this time.
            trace.committed_write_start = writer_started;
        }
        match result {
            Err(WriteError::Uncertain) => trace.uncertain = trace.uncertain.saturating_add(1),
            Err(WriteError::WouldBlock) if selected => trace.target_blocked = trace.target_blocked.saturating_add(1),
            Err(WriteError::WouldBlock) => trace.other_blocked = trace.other_blocked.saturating_add(1),
            Ok(()) if !selected => match choice {
                PublishSelection::Idle => trace.idle = trace.idle.saturating_add(1),
                PublishSelection::Data(_) => trace.data_ahead = trace.data_ahead.saturating_add(1),
                _ => {
                    let cmd = bytes.get(..2).map(|v| u16::from_le_bytes([v[0],v[1]])).unwrap_or(0);
                    let index = match cmd { 5=>0, 6=>1, 7=>2, 8=>3, 14=>4, _=>5 };
                    trace.commands[index] = trace.commands[index].saturating_add(1);
                }
            },
            _ => {},
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    fn ipa_registration_for_request(&mut self, handle: RequestHandle) -> Option<&mut IpaRegistrationTrace> {
        self.linux_ipa.as_mut()?.registration.as_mut().filter(|trace| trace.request == handle)
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(super) fn note_ipa_registration_routed(&mut self, handle: RequestHandle) {
        let clock = self.intent_ack_clock;
        if let Some(trace) = self.ipa_registration_for_request(handle) {
            trace.routed_at = clock.map(|clock| clock());
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(crate) fn note_ipa_registration_prepared(&mut self, handle: RequestHandle) {
        let clock = self.intent_ack_clock;
        if let Some(trace) = self.ipa_registration_for_request(handle) {
            trace.prepared_at = clock.map(|clock| clock());
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(crate) fn note_ipa_registration_queue_attempt(&mut self, handle: RequestHandle) {
        let clock = self.intent_ack_clock;
        if let Some(trace) = self.ipa_registration_for_request(handle)
            && let Some(clock) = clock
        {
            trace.first_queue_attempt_at.get_or_insert_with(clock);
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(crate) fn note_ipa_registration_queue_wait(&mut self, handle: RequestHandle, error: Error) {
        let Some(trace) = self.ipa_registration_for_request(handle) else { return; };
        match error {
            Error::FlowBusy => trace.flow_waits = trace.flow_waits.saturating_add(1),
            Error::NoCapacity => trace.capacity_waits = trace.capacity_waits.saturating_add(1),
            _ => {}
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(super) fn note_linux_ipa_published(&mut self, index: usize, remote: usize) {
        let TxKind::LinuxIpa { id, .. } = self.transmit[index].kind else { return; };
        let Some(profile) = self.linux_ipa.as_mut() else { return; };
        if let Some(trace) = profile.registration.as_mut().filter(|trace| trace.publication == Some(id)) {
            trace.published_at = self.intent_ack_clock.map(|clock| clock());
            trace.remote_intent = self.remote[remote].map(|intent| intent.id);
        }
        #[cfg(feature = "sunfish-ipa-init-continuation")]
        if let Some(trace) = profile.init_transport.as_mut().filter(|trace| trace.publication == id) {
            trace.published_at = self.intent_ack_clock.map(|clock| clock());
        }
    }

    /// Observe whether a retired IPA publication receipt is waiting, without
    /// consuming it or granting permission to commit it.
    pub fn linux_ipa_receipt_pending(&self) -> Option<bool> {
        self.linux_ipa.as_ref().map(|profile| {
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            if profile.server_completed.is_some() { return true; }
            profile.completed.is_some()
        })
    }
    /// Retain wire identity only after the final indication bytes were written.
    /// This is not GLINK completion or a protocol readiness receipt.
    #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
    pub(super) fn note_linux_ipa_indication_published(&mut self, index: usize) {
        let tx = &self.transmit[index];
        let destination = match tx.kind {
            TxKind::LinuxIpa { destination, .. } => destination,
            _ => return,
        };
        if tx.sent != tx.len || tx.len < 39 || tx.bytes[32] != 4
            || u16::from_le_bytes([tx.bytes[35], tx.bytes[36]]) != ipa::INIT_COMPLETE
        { return; }
        let transaction = u16::from_le_bytes([tx.bytes[33], tx.bytes[34]]);
        if let Some(profile) = self.linux_ipa.as_mut() {
            profile.published_indication = Some((destination, transaction));
        }
    }
    #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
    pub(super) fn matches_published_ipa_indication(&self, peer: Address, transaction: u16) -> bool {
        self.linux_ipa.as_ref().is_some_and(|profile|
            profile.published_indication == Some((peer, transaction)))
    }
    /// Locally advertised, unused IPCRTR receive slots at this instant.
    /// This private-state snapshot does not establish what the peer observed.
    pub fn linux_ipa_receive_available(&self) -> u32 {
        self.receive.iter().filter(|rx| rx.state == RxState::Available).count() as u32
    }
    /// Read retained private metadata even after quarantine. This does not
    /// decode protocol success or create any publication/ownership receipt.
    pub fn linux_ipa_tx_diagnostic(&self) -> Option<LinuxIpaTxDiagnostic> {
        let tx = self.transmit.iter().find(|tx| tx.len != 0 && matches!(tx.kind, TxKind::LinuxIpa { .. }))?;
        fn decode(bytes: &[u8]) -> Option<qmi::Message<'_>> {
            let header = bytes.get(..32)?;
            let len = u32::from_le_bytes(header.get(20..24)?.try_into().ok()?) as usize;
            qmi::Message::decode(bytes.get(32..32usize.checked_add(len)?)?).ok()
        }
        let tx_transaction = decode(&tx.bytes[..tx.len]).map_or(u32::MAX, |m| u32::from(m.transaction));
        let response = self.receive.iter().find(|rx| rx.state == RxState::LinuxIpa);
        let message = response.and_then(|rx| decode(&rx.bytes[..rx.used]));
        let response_transaction = message.as_ref().map_or(u32::MAX, |m| u32::from(m.transaction));
        let result = message.as_ref().and_then(|m| m.get(2)).and_then(|v| match v {
            [a, b, c, d] => Some((u32::from(u16::from_le_bytes([*a, *b])), u32::from(u16::from_le_bytes([*c, *d])))),
            _ => None,
        });
        let remote_intent = tx.remote_slot.and_then(|i| self.remote.get(i).copied().flatten())
            .map_or(u32::MAX, |intent| intent.id);
        Some(LinuxIpaTxDiagnostic {
            len: tx.len as u32,
            sent: tx.sent as u32,
            flags: u32::from(tx.glink_done) | (u32::from(tx.qrtr_done) << 1)
                | (u32::from(word(&tx.bytes, 16) == 1) << 2)
                | (u32::from(tx.remote_slot.is_some()) << 3)
                | (u32::from(response.is_some()) << 4),
            remote_intent,
            tx_transaction,
            response_transaction,
            response_result: result.map_or(u32::MAX, |v| v.0),
            response_error: result.map_or(u32::MAX, |v| v.1),
        })
    }
    /// Pristine local configuration only; this is not hardware readiness.
    pub fn linux_ipa_prepared(&self, generation: u64) -> Result<bool, Error> {
        self.check(generation)?;
        let base =
            self.linux_ipa.is_some() && self.transport_is_pristine() && !self.control.started();
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        if self.runtime_profile {
            let report = self.runtime_profile_report();
            return Ok(base
                && report.remote_storage_registered
                && !report.remote_storage_ready
                && !report.service_registry_registered
                && !report.ipa_server_registered
                && report.linux_ipa_client_enabled
                && report.tftp_port.is_none()
                && !report.tftp_server_prepared);
        }
        Ok(base
            && self.control.status().ready_services & (1 << 4) != 0
            && self.router.endpoint(Service::IpaServer).is_some())
    }
    pub(super) fn linux_ipa_port_in_use(&self, port: u32) -> bool {
        self.linux_ipa
            .as_ref()
            .is_some_and(|p| p.client.port == port)
    }
    /// Select this profile before attaching the native transport. Host IPA
    /// service registration/availability is separate and requires its real
    /// retained backend. The downstream client cannot coexist with this one.
    pub fn enable_linux_ipa(&mut self, generation: u64, port: u32) -> Result<(), Error> {
        self.check(generation)?;
        if self.control.started()
            || !self.transport_is_pristine()
            || self.linux_ipa.is_some()
            || self.ipa_client.is_some()
        {
            return Err(control::Error::Busy.into());
        }
        if port == 0
            || port >= control::CONTROL_PORT
            || self.local_client_port_in_use(port)
            || self.control.tftp_port() == Some(port)
        {
            return Err(control::Error::Address.into());
        }
        self.linux_ipa = Some(LinuxIpa {
            client: Address {
                node: self.config.local_node,
                port,
            },
            last: None,
            peer: None,
            completed: None,
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            server_last: None,
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            server_completed: None,
            #[cfg(feature = "sunfish-ipa-registration-trace")]
            registration: None,
            #[cfg(feature = "sunfish-ipa-init-continuation")]
            init_transport: None,
            #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
            published_indication: None,
        });
        Ok(())
    }
    pub fn linux_ipa_modem(&self, generation: u64) -> Result<Option<ipa::Address>, Error> {
        self.check(generation)?;
        if self.linux_ipa.is_none() {
            return Err(Error::InvalidReply);
        }
        Ok(self.control.ipa_modem_endpoint().map(|p| ipa::Address {
            node: p.node,
            port: p.port,
        }))
    }
    pub(super) fn observe_linux_ipa_control(&mut self) -> Result<(), Error> {
        if self
            .linux_ipa
            .as_ref()
            .and_then(|p| p.peer)
            .is_some_and(|expected| self.control.ipa_modem_endpoint() != Some(expected))
        {
            self.quarantined = true;
            return Err(control::Error::ClientGone.into());
        }
        Ok(())
    }
    /// Queue exactly the opaque record prepared by the Linux profile. Response
    /// records require their held router request. Indications may have any
    /// trusted transaction ID, including zero. No complete record is retried.
    pub fn queue_linux_ipa(
        &mut self,
        generation: u64,
        prepared: &Prepared,
        handle: Option<&RequestHandle>,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.observe_linux_ipa_control()?;
        if !self.control.accepts(Service::IpaServer) {
            return Err(control::Error::Unavailable.into());
        }
        let profile = self.linux_ipa.as_ref().ok_or(Error::InvalidReply)?;
        let id = prepared.publication_id();
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let (last, completed) = match prepared.socket() {
            Socket::Client => (profile.last, profile.completed.is_some()),
            Socket::Server => (profile.server_last, profile.server_completed.is_some()),
        };
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        let (last, completed) = (profile.last, profile.completed.is_some());
        // One protocol owner across both sockets; monotonic sequence per socket
        // permits the other socket to queue while this one has backpressure.
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        if [profile.last, profile.server_last].iter().flatten().any(|last|
            last.owner != id.owner || last.generation != id.generation) {
            return Err(Error::InvalidReply);
        }
        if id.generation != generation || last.is_some_and(|last| {
            last.owner != id.owner || last.generation != id.generation || id.sequence <= last.sequence
        }) { return Err(Error::InvalidReply); }
        if completed || self.transmit.iter().any(|tx| {
            if tx.len == 0 { return false; }
            let TxKind::LinuxIpa { source, .. } = tx.kind else { return false; };
            #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
            { (source == profile.client) == (prepared.socket() == Socket::Client) }
            #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
            { let _ = source; true }
        }) { return Err(Error::FlowBusy); }
        let modem = self
            .control
            .ipa_modem_endpoint()
            .ok_or(control::Error::Unavailable)?;
        let server = self
            .router
            .endpoint(Service::IpaServer)
            .ok_or(routing::Error::NoEndpoint)?
            .address;
        let destination = Address {
            node: prepared.peer().node,
            port: prepared.peer().port,
        };
        let message = Message::decode(prepared.bytes())?;
        let (source, receive) = match (prepared.socket(), message.kind, message.message_id, handle)
        {
            (Socket::Client, qmi::Kind::Request, ipa::INIT_DRIVER, None) => {
                if destination != modem || prepared.bytes().len() != ipa::INIT_REQUEST_BYTES {
                    return Err(Error::InvalidReply);
                }
                (profile.client, None)
            }
            (Socket::Server, qmi::Kind::Indication, ipa::INIT_COMPLETE, None) => {
                if destination != modem
                    || prepared.bytes().len() != ipa::COMMON_BYTES
                    || message.tlv_count() != 1
                    || message.required(2)? != [0, 0, 0, 0]
                {
                    return Err(Error::InvalidReply);
                }
                (server, None)
            }
            (
                Socket::Server,
                qmi::Kind::Response,
                ipa::INDICATION_REGISTER | ipa::DRIVER_INIT_COMPLETE,
                Some(handle),
            ) => {
                let receive = self.request_index(handle)?;
                let (context, request) = self.request_qmi(handle)?;
                let request = Message::decode(request)?;
                if context.service != Service::IpaServer
                    || context.peer != destination
                    || context.local != server
                    || context.generation != generation
                    || destination.node != self.config.peer_node
                    || request.transaction != message.transaction
                    || request.message_id != message.message_id
                    || message.tlv_count() != 1
                    || prepared.bytes().len() != ipa::COMMON_BYTES
                    || message.required(2)? != [0, 0, 0, 0]
                {
                    return Err(Error::InvalidReply);
                }
                (server, Some(receive))
            }
            _ => return Err(Error::InvalidReply),
        };
        if source.node != self.config.local_node
            || destination.node != self.config.peer_node
            || destination.port == 0
            || destination.port >= control::CONTROL_PORT
        {
            return Err(Error::InvalidReply);
        }
        if self.transmit.iter().any(|tx| {
            tx.len != 0
                && tx
                    .kind
                    .qrtr_addresses()
                    .is_some_and(|(_, peer)| peer == destination)
        }) {
            return Err(Error::FlowBusy);
        }
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_flow = self.remember_and_reserve_reference(
            source, destination, !cfg!(feature = "sunfish-qrtr-ipa-flow-experiment"),
        )?;
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        self.remember_qrtr_flow(source, destination)?;
        let tx = &mut self.transmit[slot];
        let len = 32 + prepared.bytes().len();
        header(
            &mut tx.bytes,
            1,
            source,
            destination,
            prepared.bytes().len(),
            true,
        );
        #[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
        put(&mut tx.bytes, 16, u32::from(reference_flow.confirm_rx()));
        tx.bytes[32..len].copy_from_slice(prepared.bytes());
        tx.bytes[len..align(len, 4)].fill(0);
        tx.len = align(len, 4);
        tx.sent = 0;
        tx.remote_slot = None;
        tx.glink_done = false;
        tx.qrtr_done = false;
        // Under the Linux flow experiment, RESUME_TX is destination credit,
        // not an acknowledgement of this specific packet. GLINK completion
        // is still required to retire the send and release its buffer.
        #[cfg(feature = "sunfish-qrtr-ipa-flow-experiment")]
        { tx.qrtr_done = true; }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { tx.reference_flow = Some(reference_flow); }
        tx.kind = TxKind::LinuxIpa {
            source,
            destination,
            receive,
            id,
        };
        if let Some(index) = receive {
            self.receive[index].state = RxState::ReplyQueued;
        }
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        let queue_state = if prepared.socket() == Socket::Client && message.message_id == ipa::INIT_DRIVER {
            let tx = &self.transmit[slot];
            u32::from(self.remote_for_transmit(tx).is_some())
                | (u32::from(self.ready_remote_for_transmit(tx).is_some()) << 1)
                | (u32::from(self.intent_request_ack.is_some()) << 2)
                | (u32::from(self.next_transmit_in_order([1]).is_some()) << 3)
                | (u32::from(matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. })) << 4)
                | (u32::from(self.unsent_flow_predecessor(tx)) << 5)
                | (u32::from(self.intent_ack_backing.is_some()) << 6)
                | (u32::from(self.original_1f_close_count() == Some(0)) << 7)
        } else { 0 };
        let profile = self.linux_ipa.as_mut().unwrap();
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        match prepared.socket() { Socket::Client => profile.last = Some(id), Socket::Server => profile.server_last = Some(id) }
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        { profile.last = Some(id); }
        profile.peer = Some(modem);
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        if let Some(trace) = profile.registration.as_mut().filter(|trace| Some(&trace.request) == handle) {
            trace.publication = Some(id);
            trace.queued_at = self.intent_ack_clock.map(|clock| clock());
        }
        #[cfg(feature = "sunfish-ipa-init-continuation")]
        if prepared.socket() == Socket::Client && message.message_id == ipa::INIT_DRIVER {
            profile.init_transport = Some(IpaInitTransportTrace {
                publication: id,
                queued_at: self.intent_ack_clock.map(|clock| clock()),
                published_at: None,
                retired_at: None,
                #[cfg(feature = "sunfish-ipa-init-tx-trace")]
                tx: IpaInitTxTrace {
                    #[cfg(feature = "sunfish-ipa-init-open-writer")]
                    queue_state,
                    #[cfg(feature = "sunfish-ipa-init-before-ad")]
                    queued_grant: self.intent_ack_backing,
                    ..IpaInitTxTrace::default()
                },
            });
        }
        Ok(())
    }
    pub(super) fn receive_linux_ipa(&mut self, index: usize, len: usize) -> Result<Event, Error> {
        self.observe_linux_ipa_control()?;
        let rx = &self.receive[index];
        let source = Address {
            node: word(&rx.bytes, 8),
            port: word(&rx.bytes, 12),
        };
        let local = Address {
            node: word(&rx.bytes, 24),
            port: word(&rx.bytes, 28),
        };
        let profile = self.linux_ipa.as_ref().ok_or(Error::InvalidReply)?;
        if word(&rx.bytes, 0) != 1
            || word(&rx.bytes, 4) != 1
            || word(&rx.bytes, 16) > 1
            || local != profile.client
            || self.control.ipa_modem_endpoint() != Some(source)
            || len > 32 + 7 + 25
        {
            return Err(Error::InvalidReply);
        }
        let m = Message::decode(&rx.bytes[32..len])?;
        if m.kind != qmi::Kind::Response || m.message_id != ipa::INIT_DRIVER {
            return Err(Error::InvalidReply);
        }
        // Transport credit can be returned independently of protocol acceptance.
        // Keep the complete response held if it races the request's completion.
        if word(&rx.bytes, 16) == 1 {
            let slot = self.free_tx_slot()
                .ok_or(Error::NoCapacity)?;
            stage_resume_tx(&mut self.transmit[slot], local, source);
        }
        self.receive[index].state = RxState::LinuxIpa;
        self.receive[index].done_pending = false;
        Ok(Event::LinuxIpaResponse(LinuxIpaHandle {
            owner: self.owner,
            generation: self.generation,
            intent: self.receive[index].id,
        }))
    }
    fn linux_ipa_response_index(&self, handle: &LinuxIpaHandle) -> Result<usize, Error> {
        self.check(handle.generation)?;
        if handle.owner != self.owner {
            return Err(Error::InvalidHandle);
        }
        self.receive
            .iter()
            .position(|rx| rx.id == handle.intent && rx.state == RxState::LinuxIpa)
            .ok_or(Error::InvalidHandle)
    }
    pub fn linux_ipa_response(
        &self,
        handle: &LinuxIpaHandle,
    ) -> Result<(ipa::Address, &[u8]), Error> {
        let index = self.linux_ipa_response_index(handle)?;
        let rx = &self.receive[index];
        let len = 32 + word(&rx.bytes, 20) as usize;
        Ok((
            ipa::Address {
                node: word(&rx.bytes, 8),
                port: word(&rx.bytes, 12),
            },
            &rx.bytes[32..len],
        ))
    }
    pub fn finish_linux_ipa_response(&mut self, handle: &LinuxIpaHandle) -> Result<(), Error> {
        let index = self.linux_ipa_response_index(handle)?;
        self.receive[index].state = RxState::Control;
        self.receive[index].done_pending = true;
        Ok(())
    }
    pub(super) fn retire_linux_ipa(
        &mut self,
        receive: Option<usize>,
        id: PublicationId,
    ) -> Result<(), Error> {
        let profile = self.linux_ipa.as_ref().ok_or(Error::InvalidReply)?;
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let server = profile.server_last == Some(id);
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let valid = if server { profile.server_completed.is_none() } else {
            profile.last == Some(id) && profile.completed.is_none()
        };
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        let valid = profile.completed.is_none() && profile.last == Some(id);
        if !valid { return Err(Error::FlowBusy); }
        if let Some(index) = receive {
            let ticket = self.receive[index]
                .ticket
                .take()
                .ok_or(Error::InvalidHandle)?;
            self.router.complete(ticket)?;
            let rx = &mut self.receive[index];
            if rx.done_pending {
                // The peer completed the reply while our return of its request buffer
                // was still pending. The writer frees the slot when that return commits.
                rx.context = None;
                rx.state = RxState::Control;
            } else {
                *rx = Receive::empty();
            }
        }
        let profile = self.linux_ipa.as_mut().unwrap();
        let receipt = LinuxIpaReceipt { owner: self.owner, generation: self.generation, id };
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        if server { profile.server_completed = Some(receipt); } else { profile.completed = Some(receipt); }
        #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
        { profile.completed = Some(receipt); }
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        if let Some(trace) = profile.registration.as_mut().filter(|trace| trace.publication == Some(id)) {
            trace.completed_at = self.intent_ack_clock.map(|clock| clock());
        }
        #[cfg(feature = "sunfish-ipa-init-continuation")]
        if let Some(trace) = profile.init_transport.as_mut().filter(|trace| trace.publication == id) {
            trace.retired_at = self.intent_ack_clock.map(|clock| clock());
        }
        Ok(())
    }
    pub fn take_linux_ipa_receipt(
        &mut self,
        generation: u64,
    ) -> Result<Option<LinuxIpaReceipt>, Error> {
        self.check(generation)?;
        let profile = self.linux_ipa.as_mut().ok_or(Error::InvalidReply)?;
        let receipt = profile.completed.take();
        #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
        let receipt = receipt.or_else(|| profile.server_completed.take());
        Ok(receipt)
    }
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    pub fn take_linux_ipa_receipt_for(&mut self, generation: u64, prepared: &Prepared)
        -> Result<Option<LinuxIpaReceipt>, Error>
    {
        self.check(generation)?;
        let profile = self.linux_ipa.as_mut().ok_or(Error::InvalidReply)?;
        let slot = match prepared.socket() {
            Socket::Client => &mut profile.completed, Socket::Server => &mut profile.server_completed,
        };
        if slot.as_ref().is_some_and(|receipt| receipt.id != prepared.publication_id()) {
            return Err(Error::InvalidReply);
        }
        Ok(slot.take())
    }

    /// Commit only this session's opaque transport receipt to its matching
    /// protocol record. This API still grants no hardware readiness permit.
    pub fn commit_linux_ipa(
        &self,
        generation: u64,
        core: &mut ColdSession,
        prepared: &Prepared,
        receipt: LinuxIpaReceipt,
    ) -> Result<(), Error> {
        self.check(generation)?;
        if receipt.owner != self.owner
            || receipt.generation != generation
            || receipt.id != prepared.publication_id()
        {
            return Err(Error::InvalidReply);
        }
        core.commit_publication(generation, prepared)
            .map_err(Error::LinuxIpa)
    }
}
