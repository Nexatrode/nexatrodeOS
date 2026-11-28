use super::*;
#[test]fn tx3_credit_return_sequence_keeps_credit_before_return_reply_and_grant(){
 for clocked in [false,true]{
  let(mut s,h,id)=setup(false,true,3,4096);if clocked{s.set_registry_ack_clock(||55);}
  let(credit,b)=reply(&mut s);assert_eq!(word(&b,20),7);
  assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));
  let(remote,b)=reply(&mut s);assert_eq!(word(&b,20),1);assert_eq!(&b[49..51],&3u16.to_le_bytes());
  grant(&mut s);assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));assert!(s.ingest(1,&control(14,credit)).is_ok());
 }
}
#[test]fn tx3_credit_return_blocked_credit_falls_back_without_enabling_later_stages(){
 let(mut s,h,id)=setup(false,true,3,4096);let mut blocked=vec![];
 assert_eq!(s.publish_next(1,|b|{blocked=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));assert_eq!(word(&blocked,20),7);
 grant(&mut s);s.ingest(1,&control(7,108)).unwrap();
 assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));grant(&mut s);let(credit,b)=reply(&mut s);assert_eq!(b,blocked);
 let(remote,_)=reply(&mut s);assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x7e0000,0x20000);
 assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));assert!(s.ingest(1,&control(14,credit)).is_ok());
}
#[test]fn tx3_credit_return_blocked_own_return_spends_only_that_attempt(){
 let(mut s,h,id)=setup(false,true,3,4096);let(credit,_)=reply(&mut s);
 assert_eq!(s.publish_next(1,|b|{assert_eq!(word(b,0)&0xffff,6);assert_eq!(word(b,4),id);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
 grant(&mut s);s.ingest(1,&control(7,108)).unwrap();
 assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));let(remote,_)=reply(&mut s);grant(&mut s);
 assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x7e0000,0x3e0000);
 assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));assert!(s.ingest(1,&control(14,credit)).is_ok());
}
#[test]fn tx3_credit_return_blocked_reply_retries_identical_bytes_after_fresh_grant(){
 let(mut s,h,id)=setup(false,true,3,4096);let(credit,_)=reply(&mut s);assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));
 let mut blocked=vec![];assert_eq!(s.publish_next(1,|b|{blocked=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));assert_eq!(word(&blocked,20),1);
 grant(&mut s);s.ingest(1,&control(7,108)).unwrap();grant(&mut s);let(remote,b)=reply(&mut s);assert_eq!(b,blocked);
 assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x7e0000,0x6e0000);
 assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));assert!(s.ingest(1,&control(14,credit)).is_ok());
}
#[test]fn tx3_credit_return_uncertain_or_unwound_stage_quarantines(){
 for stage in 0..3{for unwind in [false,true]{let(mut s,_,id)=setup(false,true,3,4096);
 if stage>0{let _=reply(&mut s);}if stage>1{assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));}
 if unwind{assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||s.publish_next(1,|_|panic!("credit return stage fault")))).is_err());}else{assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));}
 assert_eq!(s.publish_next(1,|_|panic!("no retry")),Err(Error::Quarantined));s.reset_after_transport_reset(2).unwrap();assert!(s.transport_is_pristine());}}
}
fn custom(remote:&[(u32,u32)],two:bool)->(Session,RequestHandle,u32,Option<RequestHandle>){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(remote)).unwrap();
 let a=offer(&mut s);let b=if two{Some(offer(&mut s))}else{None};
 let request=|s:&mut Session,id,peer,txn:u8|{let p=qrtr(1,peer,qcom_servreg::LOCAL_PORT,true,&[0,txn,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o']);let Event::Request(h)=s.ingest(1,&data(9,id,&p,0)).unwrap()else{panic!("request")};h};
 let h=request(&mut s,a,9,3);s.queue_reply(&h,Reply::Failure{error:3}).unwrap();let other=b.map(|id|request(&mut s,id,10,4));s.ingest(1,&control(7,108)).unwrap();(s,h,a,other)
}
#[test]fn tx3_credit_return_one_modem_buffer_waits_for_actual_credit_receipt(){
 let(mut s,h,id,_)=custom(&[(7,4096)],false);let(credit,_)=reply(&mut s);assert_eq!(credit,7);assert_eq!(s.in_flight(),1);
 grant(&mut s);
 for _ in 0..4{match s.publish_next(1,|_|Ok(())){Ok(Some(Published::RemoteIntentRequest{size}))=>{assert!(size>=48);s.ingest(1,&control(8,1)).unwrap();},Ok(Some(Published::ReceiveDone(returned)))=>assert_eq!(returned,id),Ok(None)|Err(Error::NoRemoteIntent)=>{},other=>panic!("no reply before real credit receipt: {other:?}")}}
 assert!(s.ingest(1,&control(14,credit)).is_ok());assert_eq!(s.in_flight(),1);
 let(Published::Data{remote_intent:remote,..},_)=publish_until_data(&mut s)else{panic!("reply")};assert_eq!(remote,credit);assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));
 assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x7e0000,0x60000);
}
#[test]fn tx3_credit_return_another_ready_credit_keeps_priority_and_is_not_drained(){
 let(mut s,h,_,other)=custom(&[(7,4096),(8,4096),(9,4096)],true);
 let(first,b)=reply(&mut s);assert_eq!(word(&b,20),7);grant(&mut s);
 let mut second=None;let mut reply_id=None;
 for _ in 0..16{let(e,b)=publish(&mut s).unwrap();if let Published::Data{remote_intent,..}=e{if word(&b,20)==7{second=Some(remote_intent);}else{assert!(second.is_some());reply_id=Some(remote_intent);break;}}}
 let second=second.unwrap();let remote=reply_id.unwrap();assert_ne!(first,second);assert_ne!(remote,first);assert_ne!(remote,second);
 assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x7e0000,0x60000);
 assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));assert!(s.request(&other.unwrap()).is_ok());
}
#[test]fn tx3_credit_return_scope_fit_advertised_ack_and_bounded_call_are_preserved(){
 for(returned,txn,capacity)in[(true,3,4096),(false,4,4096),(false,3,16)]{let(mut s,_,_)=setup(returned,true,txn,capacity);grant(&mut s);}
 let(mut s,_,_)=setup(false,true,3,16);assert!(matches!(publish(&mut s).unwrap().0,Published::ReceiveIntent(_)));s.ingest(1,&intents(&[(9,4096)])).unwrap();assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});
 let(mut s,_,_)=setup(false,true,3,4096);let e=s.publish_oack_followup(1,qcom_glink_qmi::OackFollowup::Advertise,|_|Ok(())).unwrap();assert!(!matches!(e,Some(Published::Data{..})));assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.map_or(0,|p|p.mask)&0x7e0000,0);
}
#[test]fn tx3_credit_return_native_88_byte_reply_keeps_original_transaction(){
 let(mut s,h,id)=elf_ready_cases::tx3_manifest_pending_confirmed();let(credit,b)=reply(&mut s);assert_eq!(word(&b,8),52);assert_eq!(word(&b,20),7);
 assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));let(remote,b)=reply(&mut s);assert_eq!(word(&b,8),88);assert_eq!(&b[49..51],&3u16.to_le_bytes());assert!(b.windows(b"msm/adsp/audio_pd".len()).any(|w|w==b"msm/adsp/audio_pd"));assert_ne!(credit,remote);
 grant(&mut s);assert!(s.ingest(1,&control(14,credit)).is_ok());assert_eq!(s.in_flight(),1);assert_eq!(s.ingest(1,&control(14,remote)),Ok(Event::ReplyRxDone(h)));
 assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x7e0000,0x7e0000);
}
