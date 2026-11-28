use super::*;
#[test]fn tx3_own_return_opens_ready_reply_before_fresh_grant(){
 for clocked in [false,true]{
  let(mut s,h,request_id)=setup(false,false,3,4096);if clocked{s.set_registry_ack_clock(||77);}
  assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(request_id));
  let(remote,b)=reply(&mut s);assert_eq!(&b[49..51],&3u16.to_le_bytes());
  grant(&mut s);assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));
 }
}
fn full_slots(controls:bool)->(Session,RequestHandle,Vec<u32>){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,4096),(8,4096)])).unwrap();
 let ids:Vec<_>=(0..qcom_glink_qmi::RX_SLOTS).map(|_|offer(&mut s)).collect();let h=registry_turn_request(&mut s,ids[0],9,3);s.queue_reply(&h,Reply::Failure{error:3}).unwrap();
 if controls{for &id in &ids[1..]{assert_eq!(s.ingest(1,&data(9,id,&resume_route(0xdead_beef,MEMORY),0)),Ok(Event::CreditReceived));}}
 assert_eq!(s.ingest(1,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:false}));(s,h,ids)
}
#[test]fn tx3_own_return_after_full_slot_conversion_keeps_ad_before_ack(){
 let(mut s,h,ids)=full_slots(true);
 assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(ids[1]));
 assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(ids[0]));
 let(remote,b)=reply(&mut s);assert_eq!(&b[49..51],&3u16.to_le_bytes());grant(&mut s);
 let p=s.registry_window_trace().predecessor.unwrap().publication.unwrap();assert_eq!(p.mask&0x780000,0x780000);assert!(p.own_return_gates[7]>0);
 assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn tx3_own_return_blocked_attempt_falls_back_and_does_not_reborrow(){
 let(mut s,h,id)=setup(false,false,3,4096);let mut attempted=vec![];
 assert_eq!(s.publish_next(1,|b|{attempted=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
 assert_eq!(word(&attempted,0)&0xffff,6);assert_eq!(word(&attempted,4),id);grant(&mut s);
 assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));
 let(remote,b)=reply(&mut s);assert_eq!(&b[49..51],&3u16.to_le_bytes());assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x600000,0x200000);
 assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn tx3_own_return_then_blocked_data_preserves_both_spent_latches(){
 let(mut s,h,id)=setup(false,false,3,4096);assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));
 let mut blocked=vec![];assert_eq!(s.publish_next(1,|b|{blocked=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));grant(&mut s);
 s.ingest(1,&control(7,108)).unwrap();grant(&mut s);let(remote,b)=reply(&mut s);assert_eq!(b,blocked);
 assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x780000,0x680000);
 assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn tx3_own_return_denial_credit_fit_other_txn_and_bounded_call_stay_protected(){
 let(mut s,_,_)=full_slots(false);assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:false});
 for(confirm,txn,capacity)in[(true,3,4096),(false,4,4096),(false,3,16)]{let(mut s,_,id)=setup(false,confirm,txn,capacity);if confirm&&cfg!(feature="sunfish-tx3-credit-return-turn"){let(_,b)=reply(&mut s);assert_eq!(word(&b,20),7);assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));let _=reply(&mut s);}grant(&mut s);}
 let(mut s,_,_)=setup(false,false,3,4096);let e=s.publish_oack_followup(1,qcom_glink_qmi::OackFollowup::Advertise,|_|Ok(())).unwrap();assert!(!matches!(e,Some(Published::ReceiveDone(_))));assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.map_or(0,|p|p.mask)&0x600000,0);
}
#[test]fn tx3_own_return_uncertain_or_unwound_write_quarantines_and_resets(){
 for unwind in [false,true]{let(mut s,_,_)=setup(false,false,3,4096);if unwind{assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||s.publish_next(1,|_|panic!("own return fault")))).is_err());}else{assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));}
 assert_eq!(s.publish_next(1,|_|panic!("no retry")),Err(Error::Quarantined));s.reset_after_transport_reset(2).unwrap();assert!(s.transport_is_pristine());}
}
#[test]fn tx3_own_return_keeps_native_88_byte_locator_reply_identity(){
 let(mut s,h,id)=elf_ready_cases::tx3_manifest_pending();
 assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));let(remote,b)=reply(&mut s);
 assert_eq!(word(&b,8),88);assert_eq!(&b[49..51],&3u16.to_le_bytes());assert!(b.windows(b"msm/adsp/audio_pd".len()).any(|w|w==b"msm/adsp/audio_pd"));
 grant(&mut s);assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));
}

#[cfg(feature="sunfish-tx3-credit-return-turn")]
#[path="sunfish_tx3_credit_return_cases.rs"]
mod credit_return_cases;
