//! One additional ordinary service step for the exact first held EFS request.
use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct EfsServiceTrace {
    pub calls: u32,
    pub outcome: u32,
}

impl<I: Io> Owner<I> {
    pub fn enable_efs_service_handoff(&mut self) {
        self.enable_efs_error_turn();
        self.efs_service_enabled = true;
    }

    pub fn efs_service_trace(&self) -> EfsServiceTrace { self.efs_service_trace }

    /// Called after the normal TFTP service and its existing router/close work.
    /// No request is selected here: the ordinary oldest eligible request must
    /// already be the original EFS handle, with no retained reply ahead of it.
    pub fn service_efs_handoff(
        &mut self,
        server: &mut crate::qcom_tftp_server_check::ServerCheck,
        #[cfg(feature="sunfish-tftp-wlan-mux")]
        firmware: Option<&mut crate::qcom_qrtr_tftp::Server<'_>>,
        interrupts: &mut impl Interrupts,
    ) -> Result<Option<TftpServiceProgress>, Error> {
        self.check()?;
        let Some((handle, _)) = self.first_efs_error_request else { return Ok(None); };
        if !self.efs_service_enabled || self.efs_service_used || !self.efs_ruleset_queued
            || self.first_efs_error_closed || self.has_pending_tftp_reply()
            || self.pending_tftp_request() != Some(handle)
        { return Ok(None); }
        #[cfg(feature="sunfish-mcfg-close-cleanup")]
        if self.pending_mcfg_close.is_some() { return Ok(None); }
        #[cfg(feature="sunfish-tftp-wlan-close-handoff")]
        if self.pending_firmware_close.is_some() { return Ok(None); }
        // Spend before fallible dispatch/I/O. Neither a veto nor an uncertain
        // write may repeat this opportunity. Normal continuation remains live.
        self.efs_service_used = true;
        self.efs_service_trace.calls += 1;
        #[cfg(feature="sunfish-tftp-wlan-mux")]
        let result = if let Some(firmware) = firmware {
            self.service_combined_tftp_step(server, firmware, interrupts)
        } else { self.service_tftp_server_check_step(server, interrupts) };
        #[cfg(not(feature="sunfish-tftp-wlan-mux"))]
        let result = self.service_tftp_server_check_step(server, interrupts);
        self.efs_service_trace.outcome = match result {
            Ok(TftpServiceProgress::Idle) => 1,
            Ok(TftpServiceProgress::ResponseQueued) => 2,
            Ok(TftpServiceProgress::WaitingReply) => 3,
            Ok(TftpServiceProgress::Closed(_)) => 4,
            Ok(TftpServiceProgress::Ignored(_)) => 5,
            Err(_) => 6,
        };
        result.map(Some)
    }
}
