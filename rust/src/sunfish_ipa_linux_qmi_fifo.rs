//! Live-gated access to the selected Linux IPA transport profile.
#![forbid(unsafe_code)]
use super::*;
use crate::qcom_glink_qmi::{LinuxIpaHandle, LinuxIpaReceipt};
use crate::sunfish_ipa_linux_qmi::{self as ipa, ColdSession, Prepared};

impl<I: Io> Adapter<I> {
    #[cfg(feature = "sunfish-ipa-announcement-timeline")]
    pub(crate) fn first_ipa_announcement(&self) -> Option<(u32, Option<u64>)> {
        self.session.first_ipa_announcement()
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(crate) fn ipa_registration_trace(&self) -> Option<core::IpaRegistrationTrace> {
        self.session.ipa_registration_trace()
    }

    #[cfg(feature = "sunfish-ipa-init-continuation")]
    pub(crate) fn ipa_init_transport_trace(&self) -> Option<core::IpaInitTransportTrace> {
        self.session.ipa_init_transport_trace()
    }

    #[cfg(feature = "sunfish-ipa-init-tx-trace")]
    pub(crate) fn observe_ipa_init_router(&mut self) {
        self.session.observe_ipa_init_router();
    }

    #[cfg(feature = "sunfish-ipa-registration-early-turn")]
    pub(crate) fn next_ipa_server_request(
        &self,
        requests: &[Option<RequestHandle>; core::MAX_REQUESTS],
    ) -> Option<RequestHandle> {
        self.session.next_ipa_server_request(requests)
    }

    #[cfg(feature = "sunfish-ipa-registration-trace")]
    pub(crate) fn note_ipa_registration_prepared(&mut self, handle: RequestHandle) {
        self.session.note_ipa_registration_prepared(handle);
    }

    pub(crate) fn linux_ipa_prepared(&mut self, generation: u64) -> Result<bool, Error> {
        self.gate(generation)?;
        Ok(self.session.linux_ipa_prepared(generation)?)
    }
    #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
    pub(crate) fn ipa_modem_discovered(&self, generation: u64) -> bool {
        self.session.linux_ipa_modem(generation).is_ok_and(|peer| peer.is_some())
    }

    pub(crate) fn linux_ipa_modem(
        &mut self,
        generation: u64,
    ) -> Result<Option<ipa::Address>, Error> {
        self.gate(generation)?;
        Ok(self.session.linux_ipa_modem(generation)?)
    }
    pub(crate) fn queue_linux_ipa(
        &mut self,
        generation: u64,
        p: &Prepared,
        h: Option<&RequestHandle>,
    ) -> Result<(), Error> {
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        if let Some(handle) = h {
            self.session.note_ipa_registration_queue_attempt(*handle);
        }
        self.gate(generation)?;
        let result = self.session.queue_linux_ipa(generation, p, h);
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        if let (Some(handle), Err(error)) = (h, &result) {
            self.session.note_ipa_registration_queue_wait(*handle, *error);
        }
        result?;
        Ok(())
    }
    #[cfg(not(feature = "sunfish-ipa-independent-server-experiment"))]
    pub(crate) fn take_linux_ipa_receipt(
        &mut self,
        generation: u64,
    ) -> Result<Option<LinuxIpaReceipt>, Error> {
        self.gate(generation)?;
        Ok(self.session.take_linux_ipa_receipt(generation)?)
    }
    #[cfg(feature = "sunfish-ipa-independent-server-experiment")]
    pub(crate) fn take_linux_ipa_receipt_for(&mut self, generation: u64, p: &Prepared)
        -> Result<Option<LinuxIpaReceipt>, Error>
    {
        self.gate(generation)?;
        Ok(self.session.take_linux_ipa_receipt_for(generation, p)?)
    }
    pub(crate) fn commit_linux_ipa(
        &mut self,
        generation: u64,
        core: &mut ColdSession,
        p: &Prepared,
        receipt: LinuxIpaReceipt,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .commit_linux_ipa(generation, core, p, receipt)?;
        Ok(())
    }
    pub(crate) fn linux_ipa_response(
        &mut self,
        generation: u64,
        h: &LinuxIpaHandle,
    ) -> Result<(ipa::Address, &[u8]), Error> {
        self.gate(generation)?;
        Ok(self.session.linux_ipa_response(h)?)
    }
    pub(crate) fn finish_linux_ipa_response(
        &mut self,
        generation: u64,
        h: &LinuxIpaHandle,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.finish_linux_ipa_response(h)?;
        Ok(())
    }
}
