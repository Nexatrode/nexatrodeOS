//! Reverse-arrival counterpart of the existing first EFS bounded service turn.
use super::*;
#[derive(Clone,Copy,Debug,Default)]
pub struct RulesetHandoffTrace {pub calls:u32,pub outcome:u32,pub flags:u32,pub writer:fifo::RulesetWriterTrace}
#[derive(Default)]
pub(super) struct RulesetHandoff {
 pub(super) enabled:bool,pub(super) service_used:bool,pub(super) writer_used:bool,pub(super) closed:bool,
 pub(super) first:Option<(TftpHandle,crate::qcom_qmi_router::Address)>,pub(super) trace:RulesetHandoffTrace,
}
impl<I:Io> Owner<I> {
 pub fn enable_ruleset_service_handoff(&mut self){self.enable_efs_service_handoff();self.ruleset_handoff.enabled=true;}
 pub fn ruleset_handoff_trace(&self)->RulesetHandoffTrace{
  let s=&self.ruleset_handoff;RulesetHandoffTrace{flags:u32::from(s.enabled)|(u32::from(s.first.is_some())<<1)|(u32::from(self.efs_error_turn_used)<<2)|(u32::from(s.service_used)<<3)|(u32::from(s.writer_used)<<4)|(u32::from(s.closed)<<5)|(u32::from(self.efs_ruleset_queued)<<6),..s.trace}
 }
 /// No direct request selection: the original first ruleset must still be the
 /// normal oldest request, after an admitted first EFS ERROR, with no retained
 /// reply or close cleanup. Spend once before normal fallible service work.
 pub fn service_ruleset_handoff(&mut self,server:&mut crate::qcom_tftp_server_check::ServerCheck,
  #[cfg(feature="sunfish-tftp-wlan-mux")]firmware:Option<&mut crate::qcom_qrtr_tftp::Server<'_>>,
  interrupts:&mut impl Interrupts)->Result<Option<TftpServiceProgress>,Error>{
  self.check()?;
  let s=&self.ruleset_handoff;
  let Some((handle,_))=s.first else{return Ok(None);};
  if !s.enabled||s.service_used||s.closed||!self.efs_error_turn_used||self.has_pending_tftp_reply()||self.pending_tftp_request()!=Some(handle){return Ok(None);}
  #[cfg(feature="sunfish-mcfg-close-cleanup")]
  if self.pending_mcfg_close.is_some(){return Ok(None);}
  #[cfg(feature="sunfish-tftp-wlan-close-handoff")]
  if self.pending_firmware_close.is_some(){return Ok(None);}
  self.ruleset_handoff.service_used=true;
  self.ruleset_handoff.trace.calls=self.ruleset_handoff.trace.calls.saturating_add(1);
  #[cfg(feature="sunfish-tftp-wlan-mux")]
  let result=if let Some(firmware)=firmware{self.service_combined_tftp_step(server,firmware,interrupts)}else{self.service_tftp_server_check_step(server,interrupts)};
  #[cfg(not(feature="sunfish-tftp-wlan-mux"))]
  let result=self.service_tftp_server_check_step(server,interrupts);
  self.ruleset_handoff.trace.outcome=match result{Ok(TftpServiceProgress::Idle)=>1,Ok(TftpServiceProgress::ResponseQueued)=>2,Ok(TftpServiceProgress::WaitingReply)=>3,Ok(TftpServiceProgress::Closed(_))=>4,Ok(TftpServiceProgress::Ignored(_))=>5,Err(_)=>6};
  result.map(Some)
 }
}
