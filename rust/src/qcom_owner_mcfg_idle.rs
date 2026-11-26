//! A retained missing-MCFG response lends one turn after normal validated idle.
use super::*;
impl<I:Io> Owner<I>{
/* MCFG_IDLE_DETAIL_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-detail")]
 pub fn mcfg_idle_detail(&self)->&fifo::McfgIdleDetail{self.adapter.mcfg_idle_detail()}
/* MCFG_IDLE_DETAIL_END */
 pub fn mcfg_idle_turn_trace(&self)->fifo::McfgIdleTurnTrace{self.adapter.mcfg_idle_turn_trace()}
 pub(super) fn lend_mcfg_idle_turn(&mut self)->Result<Option<u32>,Error>{
  if !self.mcfg_queued_enabled||self.pending_mcfg_close.is_some()||self.firmware_close_is_pending(){return Ok(None);}
  let slot=(0..MAX_REQUESTS).map(|offset|(self.peer_tftp_cursor+offset)%MAX_REQUESTS).find(|slot|
   self.peer_tftp_replies[*slot].as_ref().is_some_and(|r|!r.mcfg_idle_used
    &&r.bytes[..r.meta.bytes]==*crate::qcom_tftp_server_check::MISSING_REPLY
    &&self.mcfg_close_transfers.iter().flatten().any(|(peer,port)|*peer==r.meta.route.destination&&*port==r.meta.route.source.port)));
  let Some(slot)=slot else{return Ok(None);};
  // Consume this logical retained reply's opportunity before any fallible I/O.
  // Removing it on admission or close removes the latch with it.
/* MCFG_IDLE_DETAIL_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-detail")]
  {let r=self.peer_tftp_replies[slot].as_ref().unwrap();self.adapter.arm_idle_detail(r.meta.route,r.meta.bytes,slot);}
/* MCFG_IDLE_DETAIL_END */
  self.peer_tftp_replies[slot].as_mut().unwrap().mcfg_idle_used=true;
  self.adapter.publish_mcfg_idle_turn(self.generation).map(Some).map_err(Into::into)
 }
}

#[cfg(all(test,mpss_glink_transport_tests))]
mod tests{
 use super::*;
 fn setup()->(crate::Fake,Owner<crate::Fake>,crate::Irqs,u32){setup_slack(None)}
 fn setup_slack(slack:Option<usize>)->(crate::Fake,Owner<crate::Fake>,crate::Irqs,u32){
  let(a,p,h,close)=crate::qcom_glink_fifo::mcfg_idle_fixture(slack);let route=crate::qcom_glink_qmi::TftpReplyRoute{source:crate::qcom_qmi_router::Address{node:1,port:0x5006},destination:crate::qcom_qmi_router::Address{node:0,port:0x4a}};let mut o=Owner::attach(a,1).unwrap();o.enable_mcfg_queued_turn();o.mcfg_close_transfers[0]=Some((route.destination,route.source.port));let mut bytes=[0;crate::qcom_tftp::MAX_REQUEST_BYTES];let body=crate::qcom_tftp_server_check::MISSING_REPLY;bytes[..body.len()].copy_from_slice(body);
  o.peer_tftp_replies[0]=Some(PeerTftpReply{meta:PendingTftpReply{route,bytes:body.len()},bytes,request:h,
   #[cfg(feature="sunfish-mcfg-idle-turn")]mcfg_idle_used:false});p.clear();(p,o,crate::Irqs::default(),close)
 }
 #[test]
 fn mcfg_idle_owner_predecessor_sequence_and_real_receipt(){
  let(p,mut o,mut i,_)=setup();let progress=o.notified(8,&mut i).unwrap();let wire=p.take_tx();assert_eq!([wire[0],wire[8],wire[24],wire[32]],[6,5,8,9]);assert_eq!(progress.published,4);assert!(o.peer_tftp_replies[0].is_some());assert_eq!(o.tftp_tx_progress()[2],0);
  #[cfg(feature="sunfish-mcfg-idle-turn")]{assert_eq!(o.mcfg_idle_turn_trace().calls,1);assert_eq!(o.mcfg_idle_turn_trace().writes,4);assert!(o.peer_tftp_replies[0].unwrap().mcfg_idle_used);}
  print!("MCFG_IDLE_WIRE ");for b in &wire{print!("{b:02x}");}println!();println!("MCFG_IDLE_IO {:?}",p.log());
  o.notified(8,&mut i).unwrap();#[cfg(feature="sunfish-mcfg-idle-turn")]assert_eq!(o.mcfg_idle_turn_trace().calls,1);
  let id=crate::owner_word(&wire,36);p.push(&crate::control(14,2,id));o.notified(1,&mut i).unwrap();assert_eq!(o.tftp_tx_progress()[2],1);assert!(o.peer_tftp_replies[0].is_some());assert!(o.flush_peer_tftp_reply(0,&mut i).unwrap());let next=p.take_tx();let packets=crate::owner_datagrams(&next);assert_eq!(packets.len(),1);assert_eq!(&packets[0][32..51],crate::qcom_tftp_server_check::MISSING_REPLY);assert_eq!(o.tftp_tx_progress()[2],1);crate::acknowledge_outgoing_glink(&p,&mut o,&mut i,&next);assert_eq!(o.tftp_tx_progress()[2],2);
 }
 #[test]
 fn mcfg_idle_owner_waiting_close_gets_no_late_turn(){
  let(p,mut o,mut i,close)=setup();p.push(&crate::data(close,&crate::owner_control_frame(6,[0,0x4a,0,0])));let before=p.log().len();o.notified(1,&mut i).unwrap();assert_eq!(o.pending_mcfg_close,Some(crate::qcom_qmi_router::Address{node:0,port:0x4a}));assert_eq!(o.mcfg_idle_turn_trace().calls,0);assert!(!p.log()[before..].iter().any(|v|matches!(v,crate::Op::Head(_)|crate::Op::Notify)));assert!(!o.peer_tftp_replies[0].unwrap().mcfg_idle_used);
 }
 #[test]
 fn mcfg_idle_owner_default_and_foreign_scope_keep_old_turns(){
  for mode in 0..3{let(_,mut o,mut i,_)=setup();match mode{0=>o.mcfg_queued_enabled=false,1=>o.mcfg_close_transfers=[None;crate::qcom_tftp_server_check::TRANSFER_COUNT],_=>o.peer_tftp_replies[0].as_mut().unwrap().bytes[4]=b'X'}o.notified(8,&mut i).unwrap();assert_eq!(o.mcfg_idle_turn_trace().calls,0);}
 }
 #[test]
 fn mcfg_idle_owner_fault_is_terminal_and_does_not_spend_unvalidated_idle(){
  let(p,mut o,mut i,_)=setup();p.0.borrow_mut().observation.ready=false;let before=p.log().len();assert!(o.notified(8,&mut i).is_err());assert_eq!(o.mcfg_idle_turn_trace().calls,0);assert!(!p.log()[before..].iter().any(|v|matches!(v,crate::Op::Head(_)|crate::Op::Notify)));assert!(o.error().is_some());
 }
 #[test]
 fn mcfg_idle_owner_one_retained_opportunity_per_work_and_per_reply(){
  let(_,mut o,mut i,_)=setup();let mut second=o.peer_tftp_replies[0].unwrap();second.meta.route.destination.port=0x4b;second.meta.route.source.port=0x5007;o.mcfg_close_transfers[1]=Some((second.meta.route.destination,second.meta.route.source.port));o.peer_tftp_replies[1]=Some(second);
  o.notified(8,&mut i).unwrap();assert_eq!(o.mcfg_idle_turn_trace().calls,1);assert!(o.peer_tftp_replies[0].unwrap().mcfg_idle_used);assert!(!o.peer_tftp_replies[1].unwrap().mcfg_idle_used);o.notified(8,&mut i).unwrap();assert_eq!(o.mcfg_idle_turn_trace().calls,2);assert!(o.peer_tftp_replies[1].unwrap().mcfg_idle_used);o.notified(8,&mut i).unwrap();assert_eq!(o.mcfg_idle_turn_trace().calls,2);
 }

 #[test]
 fn mcfg_idle_owner_read_notification_is_progress_and_normal_retry_survives(){
  let(p,mut o,mut i,_)=setup_slack(Some(16));let out=o.notified(8,&mut i).unwrap();assert_eq!(out.published,1);let t=o.mcfg_idle_turn_trace();assert_eq!((t.calls,t.writes,t.blocked),(1,0,1));assert!(o.peer_tftp_replies[0].unwrap().mcfg_idle_used);assert_eq!(o.tftp_tx_progress()[1],0);let wire=p.take_tx();assert_eq!(&wire[wire.len()-8..],&[13,0,0,0,0,0,0,0]);o.notified(8,&mut i).unwrap();assert_eq!(o.mcfg_idle_turn_trace().calls,1);assert_eq!(o.tftp_tx_progress()[1],1);assert!(o.peer_tftp_replies[0].is_some());
 }

/* MCFG_IDLE_DETAIL_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-detail")]
include!("../../tests/sunfish_mcfg_idle_detail_cases.rs");
/* MCFG_IDLE_DETAIL_END */
}
