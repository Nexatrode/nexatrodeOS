use super::*;
fn setup(returned:bool,confirm:bool,txn:u16,capacity:u32)->(Session,RequestHandle,u32){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,capacity),(8,capacity)])).unwrap();
 let id=offer(&mut s);let p=qrtr(1,9,qcom_servreg::LOCAL_PORT,confirm,&[0,txn as u8,(txn>>8)as u8,0x21,0,6,0,1,3,0,b'f',b'o',b'o']);let Event::Request(h)=s.ingest(1,&data(9,id,&p,0)).unwrap()else{panic!()};
 s.queue_reply(&h,Reply::Failure{error:3}).unwrap();if returned{let mut done=false;for _ in 0..8{if publish(&mut s).unwrap().0==Published::ReceiveDone(id){done=true;break;}}assert!(done);}
 s.ingest(1,&control(7,108)).unwrap();(s,h,id)
}
fn grant(s:&mut Session){assert!(matches!(publish(s).unwrap().0,Published::ReceiveIntent(_)));assert_eq!(publish(s).unwrap().0,Published::IntentRequestAck{granted:true});}
fn reply(s:&mut Session)->(u32,Vec<u8>){let(e,b)=publish(s).unwrap();let Published::Data{remote_intent,last_fragment:true}=e else{panic!("expected reply: {e:?}")};(remote_intent,b)}
#[test]fn ready_reply_after_own_return_precedes_fresh_ad_and_ack(){
 for clocked in [false,true]{let(mut s,h,_)=setup(true,false,3,4096);if clocked{s.set_registry_ack_clock(||77);}
 let(id,b)=reply(&mut s);assert_eq!(&b[49..51],&3u16.to_le_bytes());let p=s.registry_window_trace().predecessor.unwrap().publication.unwrap();assert_eq!(p.mask&0x180000,0x180000);grant(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));}
}
#[test]fn ready_reply_blocked_attempt_spends_loan_then_ordinary_bytes_match(){
 let(mut s,h,_)=setup(true,false,3,4096);let mut original=vec![];assert_eq!(s.publish_next(1,|b|{original.extend_from_slice(b);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));let p=s.registry_window_trace().predecessor.unwrap().publication.unwrap();assert_eq!(p.mask&0x180000,0x80000);grant(&mut s);let(id,b)=reply(&mut s);assert_eq!(b,original);assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x180000,0x80000);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn ready_reply_own_return_credit_and_other_transactions_keep_priority(){
 let(mut own,h,id)=setup(false,false,3,4096);grant(&mut own);assert_eq!(publish(&mut own).unwrap().0,Published::ReceiveDone(id));let(i,_)=reply(&mut own);assert_eq!(own.ingest(1,&control(14,i)),Ok(Event::ReplyRxDone(h)));
 let(mut credit,_,_)=setup(true,true,3,4096);grant(&mut credit);let(_,b)=reply(&mut credit);assert_eq!(word(&b,20),7);let _=reply(&mut credit);
 let(mut four,_,_)=setup(true,false,4,4096);grant(&mut four);let _=reply(&mut four);
}
#[test]fn ready_reply_missing_fit_and_bounded_followup_cannot_borrow(){
 let(mut s,_,_)=setup(true,false,3,16);grant(&mut s);s.ingest(1,&control(8,1)).unwrap();s.ingest(1,&intents(&[(9,4096)])).unwrap();let _=reply(&mut s);
 let(mut s,_,_)=setup(true,false,3,4096);let e=s.publish_oack_followup(1,qcom_glink_qmi::OackFollowup::Advertise,|_|Ok(())).unwrap();assert!(!matches!(e,Some(Published::Data{..})));assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x180000,0);let advertised=matches!(e,Some(Published::ReceiveIntent(_)));if advertised{assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});}let _=reply(&mut s);assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x180000,if advertised{0}else{0x180000});
}
#[test]fn ready_reply_uncertain_and_unwound_writes_quarantine(){
 for unwind in [false,true]{let(mut s,_,_)=setup(true,false,3,4096);if unwind{assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||s.publish_next(1,|_|panic!("injected")))).is_err());}else{assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));}assert_eq!(s.publish_next(1,|_|panic!("no retry")),Err(Error::Quarantined));s.reset_after_transport_reset(2).unwrap();assert!(s.transport_is_pristine());}
}
#[test]fn ready_dependency_getter_uses_live_unpublished_handles_not_trace(){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,4096),(8,4096)])).unwrap();let a=offer(&mut s);let b=offer(&mut s);let h=registry_turn_request(&mut s,a,9,4);assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(a));s.queue_reply(&h,Reply::Failure{error:3}).unwrap();let w=registry_turn_request(&mut s,b,9,5);assert_eq!(s.registry_unpublished_blocker(&w),Some(h));let _=publish_until_data(&mut s);assert_eq!(s.registry_unpublished_blocker(&w),None);s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_unpublished_blocker(&w),None);
}

#[cfg(feature="sunfish-elf-ready-grant-turn")]
#[path="sunfish_elf_ready_cases.rs"]
mod elf_ready_cases;

#[cfg(feature="sunfish-registry-elf-pressure")]
#[path="sunfish_registry_queue_ready_cases.rs"]
mod registry_queue_ready_cases;
