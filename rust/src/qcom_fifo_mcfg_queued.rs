//! At most four ordinary checked writer attempts after an admitted MCFG error.
use super::*;
#[derive(Clone,Copy,Debug,Default,Eq,PartialEq)]
pub struct McfgQueuedTurnTrace {pub calls:u32,pub writes:u32,pub rx_veto:u32,pub blocked:u32}
impl<I:Io> Adapter<I>{
    pub(crate) fn mcfg_queued_turn_trace(&self)->McfgQueuedTurnTrace{self.mcfg_queued}
    pub(crate) fn publish_mcfg_queued_turn(&mut self,generation:u64)->Result<(),Error>{
/* MCFG_IDLE_TURN_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-turn")]
        {let mut progress=super::mcfg_idle::TurnProgress::default();return self.publish_mcfg_checked_turn(generation,&mut progress);}
/* MCFG_IDLE_TURN_END */
        self.check(generation)?;
        self.mcfg_queued.calls=self.mcfg_queued.calls.saturating_add(1);
        for _ in 0..4 {
            self.mcfg_guard=true;
            let result=self.publish_next(generation);
            self.mcfg_guard=false;
            match result? {
                Transmit::Session(p)=>{
                    self.mcfg_queued.writes=self.mcfg_queued.writes.saturating_add(1);
                    // The arbiter chooses. A credit or unrelated DATA keeps its
                    // ordinary turn and ends this burst; no packet is forced.
                    if !matches!(p,Published::ReceiveDone(_)|Published::ReceiveIntent(_)|Published::IntentRequestAck{..}){break;}
                },
                Transmit::Blocked{..}=>{self.mcfg_queued.blocked=self.mcfg_queued.blocked.saturating_add(1);break;},
                Transmit::Idle|Transmit::AwaitingIntent=>break,
            }
        }
        Ok(())
    }
}

/* MCFG_IDLE_TURN_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-turn")]
#[cfg(all(test,mpss_glink_transport_tests))]
pub(crate) fn mcfg_idle_fixture(slack:Option<usize>)->(Adapter<crate::Fake>,crate::Fake,core::TftpHandle,u32){tests::idle_fixture(slack)}
/* MCFG_IDLE_TURN_END */
#[cfg(all(test,mpss_glink_transport_tests))]
mod tests{
 use super::*;
 fn drain(a:&mut Adapter<crate::Fake>,p:&crate::Fake){for _ in 0..64{match a.publish_next(1).unwrap(){Transmit::Session(Published::Data{remote_intent,last_fragment:true})=>{a.session.ingest(1,&crate::control(14,2,remote_intent)).unwrap();},Transmit::Idle=>{p.take_tx();return;},_=>{}}}panic!("drain bound");}
 fn setup()->(Adapter<crate::Fake>,crate::Fake){setup_credit(false)}
 fn setup_credit(confirm:bool)->(Adapter<crate::Fake>,crate::Fake){
  let p=crate::Fake::new(0,0);let mut s=crate::session();s.prepare_tftp_server(1,0x5000).unwrap();let mut a=Adapter::attach(p.clone(),s,1).unwrap();a.session.start_control(1).unwrap();
  let mut adv=crate::control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}a.session.ingest(1,&adv).unwrap();drain(&mut a,&p);
  let id=a.session.offer_receive(1).unwrap();drain(&mut a,&p);a.session.ingest(1,&crate::data(id,&crate::owner_control_frame(2,[0;4]))).unwrap();drain(&mut a,&p);
  let id=a.session.offer_receive(1).unwrap();drain(&mut a,&p);while a.session.offer_receive(1).is_ok(){}drain(&mut a,&p);let Event::TftpRequest(h)=a.session.ingest(1,&crate::data(id,&crate::owner_qrtr_packet(1,0x4a,0x5000,confirm,b"\0\x01/readwrite/mcfg.tmp\0octet\0"))).unwrap()else{panic!("TFTP");};
  assert_eq!(a.session.ingest(1,&crate::control(7,2,52)).unwrap(),Event::ReceiveIntentRequest{granted:false});a.session.finish_tftp_request(&h).unwrap();
  a.session.queue_tftp_response(1,core::TftpReplyRoute{source:crate::qcom_qmi_router::Address{node:1,port:0x5006},destination:crate::qcom_qmi_router::Address{node:0,port:0x4a}},crate::qcom_tftp_server_check::MISSING_REPLY).unwrap();p.take_tx();(a,p)
 }
 #[test]
 fn mcfg_queued_adapter_return_ad_ack_error_in_normal_order(){
  let(mut a,p)=setup();a.publish_mcfg_queued_turn(1).unwrap();let b=p.take_tx();assert_eq!([b[0],b[8],b[24],b[32]],[6,5,8,9]);assert_eq!(&b[80..99],crate::qcom_tftp_server_check::MISSING_REPLY);assert_eq!(word(&b,28),1);assert_eq!(a.mcfg_queued_turn_trace(),McfgQueuedTurnTrace{calls:1,writes:4,rx_veto:0,blocked:0});assert_eq!(a.session.tftp_tx_progress()[2],0);let id=word(&b,36);a.session.ingest(1,&crate::control(14,2,id)).unwrap();assert_eq!(a.session.tftp_tx_progress()[2],1);
 }
 #[test]
 fn mcfg_queued_adapter_qrtr_credit_keeps_its_normal_turn(){
  let(mut a,p)=setup_credit(true);let(mut ordinary,q)=setup_credit(true);a.publish_mcfg_queued_turn(1).unwrap();
  for _ in 0..4 {match ordinary.publish_next(1).unwrap(){Transmit::Session(Published::ReceiveDone(_)|Published::ReceiveIntent(_)|Published::IntentRequestAck{..})=>{},_=>break}}
  let burst=p.take_tx();assert_eq!(burst,q.take_tx(),"same normal-arbiter turns with a pending QRTR credit");assert!(a.mcfg_queued_turn_trace().writes<=4);let mut offset=0;while offset<burst.len(){let command=u16::from_le_bytes([burst[offset],burst[offset+1]]);let bytes=match command{5=>16,9=>16+((word(&burst,offset+8)as usize+7)&!7),_=>8};if command==9{a.session.ingest(1,&crate::control(14,2,word(&burst,offset+4))).unwrap();}offset+=bytes;}
  let mut credit=false;for _ in 0..64 {match a.publish_next(1).unwrap(){Transmit::Session(Published::Data{remote_intent,last_fragment:true})=>{let wire=p.take_tx();credit|=crate::owner_datagrams(&wire).iter().any(|b|word(b,4)==7&&word(b,28)==0x4a);a.session.ingest(1,&crate::control(14,2,remote_intent)).unwrap();},Transmit::Idle=>break,_=>{}}}
  // The eager burst can publish the ERROR first, exactly as the ordinary
  // arbiter does; its QRTR credit remains live and completes normally.
  assert!(credit);assert_eq!(a.session.tftp_tx_progress()[2],1);

 }
 #[test]
 fn mcfg_queued_adapter_pending_rx_vetoes_before_any_write(){
  let(mut a,p)=setup();p.push(&[13,0,0,0,0,0,0,0]);let before=p.log().len();a.publish_mcfg_queued_turn(1).unwrap();assert_eq!(a.mcfg_queued_turn_trace().rx_veto,1);assert_eq!(a.mcfg_queued_turn_trace().writes,0);assert!(!p.log()[before..].iter().any(|o|matches!(o,crate::Op::Write(..)|crate::Op::Head(_)|crate::Op::Notify)));
 }
 #[test]
 fn mcfg_queued_adapter_arriving_rx_vetoes_head_and_keeps_fallback(){
  let(mut a,p)=setup();p.0.borrow_mut().append_notification_on_write=true;let before=p.log().len();a.publish_mcfg_queued_turn(1).unwrap();assert_eq!(a.mcfg_queued_turn_trace().rx_veto,1);assert_eq!(a.mcfg_queued_turn_trace().writes,0);assert!(!p.log()[before..].iter().any(|o|matches!(o,crate::Op::Head(_)|crate::Op::Notify)));a.poll_receive(1).unwrap();drain(&mut a,&p);assert_eq!(a.session.tftp_tx_progress()[2],1);
 }
 #[test]
 fn mcfg_queued_adapter_backpressure_stops_and_normal_retry_survives(){
  let(mut a,p)=setup();for _ in 0..(FIFO_BYTES-8)/8{a.ring.write(&[13,0,0,0,0,0,0,0],0).unwrap();}a.publish_mcfg_queued_turn(1).unwrap();assert_eq!(a.mcfg_queued_turn_trace().blocked,1);assert_eq!(a.mcfg_queued_turn_trace().writes,0);p.take_tx();drain(&mut a,&p);assert_eq!(a.session.tftp_tx_progress()[2],1);
 }
 #[test]
 fn mcfg_queued_adapter_faults_remain_terminal(){
  for mode in 0..4{let(mut a,p)=setup();let before=p.log().len();match mode{0=>p.0.borrow_mut().observation.ready=false,1=>p.0.borrow_mut().fail=Some("snapshot"),2=>p.0.borrow_mut().corrupt_readback=true,_=>p.0.borrow_mut().revoke_after_write=true};assert!(a.publish_mcfg_queued_turn(1).is_err());assert!(!p.log()[before..].iter().any(|o|matches!(o,crate::Op::Head(_)|crate::Op::Notify)));assert!(a.publish_next(1).is_err());assert_eq!(a.session.tftp_tx_progress()[2],0);}
 }
/* MCFG_RX_VETO_BEGIN */
#[cfg(feature="sunfish-mcfg-rx-veto-trace")]
include!("../../tests/sunfish_mcfg_rx_veto_cases.rs");
/* MCFG_RX_VETO_END */
/* MCFG_IDLE_TURN_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-turn")]
include!("../../tests/sunfish_mcfg_idle_adapter_cases.rs");
/* MCFG_IDLE_TURN_END */
/* MCFG_IDLE_TURN_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-turn")]
include!("../../tests/sunfish_mcfg_idle_fixture.rs");
/* MCFG_IDLE_TURN_END */
}
