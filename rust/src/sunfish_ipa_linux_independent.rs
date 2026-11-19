//! Bounded independent IPA client/server publications.
//! The transport remains single-owner. Each socket keeps its own opaque record
//! and matching retirement receipt; waiting on one socket cannot consume the
//! other socket's receipt. The opt-in runtime profile composes the early turns.
use super::*;

impl LinuxRuntime {
    pub(super) fn step_independent<I: Io>(
        &mut self, adapter: &mut Adapter<I>, requests: &mut [Option<RequestHandle>; MAX_REQUESTS],
        now: u64, peer: Option<ipa::Address>,
    ) -> Result<LinuxIpaProgress, Error> {
        if self.core.is_none() {
            let Some(peer) = peer else { return Ok(LinuxIpaProgress::WaitingPeer); };
            self.core = Some(ColdSession::new(self.generation, peer)
                .map_err(|e| Error::LinuxIpa(LinuxIpaError::Protocol(e)))?);
            self.peer = Some(peer);
            return Ok(LinuxIpaProgress::Discovered);
        }
        // At most two probes, at most one advancing action. Alternate after
        // progress; a blocked socket yields its turn without changing bytes.
        let mut waiting = None;
        for server in [self.server_turn, !self.server_turn] {
            let result = if server {
                self.step_server(adapter, requests, now)?
            } else {
                self.step_client(adapter, requests, now)?
            };
            match result {
                Some(progress @ (LinuxIpaProgress::WaitingQueue | LinuxIpaProgress::WaitingCompletion)) => {
                    waiting = Some((progress, !server));
                }
                Some(progress) => {
                    self.server_turn = !server;
                    self.last_progress_client = !server;
                    return Ok(progress);
                }
                None => {}
            }
        }
        if let Some((progress, client)) = waiting {
            self.last_progress_client = client;
            return Ok(progress);
        }
        Ok(if self.core.as_ref().unwrap().protocol_complete() {
            LinuxIpaProgress::Complete
        } else { LinuxIpaProgress::WaitingHandshake })
    }

    fn advance_publication<I: Io>(
        generation: u64, core: &mut ColdSession, slot: &mut Option<Pending>,
        adapter: &mut Adapter<I>, requests: &mut [Option<RequestHandle>; MAX_REQUESTS],
        committed: &mut u32,
    ) -> Result<LinuxIpaProgress, Error> {
        let pending = slot.as_mut().ok_or(Error::LinuxIpa(LinuxIpaError::Configuration))?;
        if !pending.queued {
            match adapter.queue_linux_ipa(generation, &pending.prepared, pending.handle.as_ref()) {
                Ok(()) => {}
                Err(fifo::Error::Session(crate::qcom_glink_qmi::Error::FlowBusy
                    | crate::qcom_glink_qmi::Error::NoCapacity)) => return Ok(LinuxIpaProgress::WaitingQueue),
                Err(e) => return Err(e.into()),
            }
            pending.queued = true;
            if let Some(handle) = pending.handle {
                for request in requests {
                    if *request == Some(handle) { *request = None; }
                }
            }
            return Ok(LinuxIpaProgress::Queued);
        }
        let Some(receipt) = adapter.take_linux_ipa_receipt_for(generation, &pending.prepared)? else {
            return Ok(LinuxIpaProgress::WaitingCompletion);
        };
        adapter.commit_linux_ipa(generation, core, &pending.prepared, receipt)?;
        *slot = None;
        *committed = committed.saturating_add(1);
        Ok(LinuxIpaProgress::Committed)
    }

    fn step_client<I: Io>(
        &mut self, adapter: &mut Adapter<I>, requests: &mut [Option<RequestHandle>; MAX_REQUESTS], now: u64,
    ) -> Result<Option<LinuxIpaProgress>, Error> {
        let core = self.core.as_mut().unwrap();
        if self.pending.is_some() {
            return Self::advance_publication(self.generation, core, &mut self.pending,
                adapter, requests, &mut self.committed).map(Some);
        }
        if self.init_since.is_none() {
            let prepared = core.prepare_init_driver(self.generation, 0, self.shared)
                .map_err(|e| Error::LinuxIpa(LinuxIpaError::Protocol(e)))?;
            self.init_since = Some(now);
            self.pending = Some(Pending { prepared, handle: None, queued: false, since: now });
            return Ok(Some(LinuxIpaProgress::Prepared));
        }
        // Response acceptance is in the shared prelude, independently of the
        // server slot, but still only after INIT's real receipt was committed.
        Ok(None)
    }

    fn step_server<I: Io>(
        &mut self, adapter: &mut Adapter<I>, requests: &mut [Option<RequestHandle>; MAX_REQUESTS], now: u64,
    ) -> Result<Option<LinuxIpaProgress>, Error> {
        let core = self.core.as_mut().unwrap();
        if self.server_pending.is_some() {
            return Self::advance_publication(self.generation, core, &mut self.server_pending,
                adapter, requests, &mut self.committed).map(Some);
        }
        for handle in requests.iter().flatten() {
            let (context, bytes) = adapter.request_qmi(self.generation, handle)?;
            if context.service != Service::IpaServer { continue; }
            let peer = ipa::Address { node: context.peer.node, port: context.peer.port };
            let prepared = core.prepare_response(self.generation, peer, bytes)
                .map_err(|e| Error::LinuxIpa(LinuxIpaError::Protocol(e)))?;
            #[cfg(feature = "sunfish-ipa-registration-trace")]
            adapter.note_ipa_registration_prepared(*handle);
            self.server_pending = Some(Pending { prepared, handle: Some(*handle), queued: false, since: now });
            return Ok(Some(LinuxIpaProgress::Prepared));
        }
        if let Some(prepared) = core.prepare_indication(self.generation, 0)
            .map_err(|e| Error::LinuxIpa(LinuxIpaError::Protocol(e)))? {
            self.server_pending = Some(Pending { prepared, handle: None, queued: false, since: now });
            return Ok(Some(LinuxIpaProgress::Prepared));
        }
        Ok(None)
    }
}
