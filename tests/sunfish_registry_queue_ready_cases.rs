use super::*;
#[test]fn registry_queue_ready_return_before_admission_gets_original_one_shot(){
 for clocked in [false,true]{let(mut s,h,_)=elf_ready_cases::queue_before_return_test_setup();if clocked{s.set_registry_ack_clock(||66);}let(id,b)=reply(&mut s);assert_eq!(&b[49..51],&3u16.to_le_bytes());grant(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x180000,0x180000);}
}

#[test]fn registry_queue_ready_blocked_attempt_cannot_reborrow_after_fresh_grant(){
 let(mut s,h,_)=elf_ready_cases::queue_before_return_test_setup();let mut blocked=vec![];
 assert_eq!(s.publish_next(1,|b|{blocked=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
 grant(&mut s);s.ingest(1,&control(7,108)).unwrap();grant(&mut s);let(id,b)=reply(&mut s);assert_eq!(b,blocked);
 assert_eq!(s.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x180000,0x80000);
 assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn registry_queue_ready_own_return_credit_fit_and_other_txn_stay_required(){
 let(mut s,h,id)=elf_ready_cases::queue_order_test_setup(false,3,4096,false);grant(&mut s);assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));let(i,_)=reply(&mut s);assert_eq!(s.ingest(1,&control(14,i)),Ok(Event::ReplyRxDone(h)));
 let(mut s,_,_)=elf_ready_cases::queue_order_test_setup(true,3,4096,true);grant(&mut s);let(_,b)=reply(&mut s);assert_eq!(word(&b,20),7);let _=reply(&mut s);
 let(mut s,_,_)=elf_ready_cases::queue_order_test_setup(false,4,4096,true);grant(&mut s);let _=reply(&mut s);
 let(mut s,_,_)=elf_ready_cases::queue_order_test_setup(false,3,16,true);grant(&mut s);assert!(matches!(publish(&mut s).unwrap().0,Published::RemoteIntentRequest{..}));s.ingest(1,&control(8,1)).unwrap();s.ingest(1,&intents(&[(9,4096)])).unwrap();let _=reply(&mut s);
}
#[test]fn registry_queue_ready_uncertainty_and_panic_quarantine(){
 for unwind in [false,true]{let(mut s,_,_)=elf_ready_cases::queue_before_return_test_setup();if unwind{assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||s.publish_next(1,|_|panic!("queue arm fault")))).is_err());}else{assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));}assert_eq!(s.publish_next(1,|_|panic!("no retry")),Err(Error::Quarantined));s.reset_after_transport_reset(2).unwrap();assert!(s.transport_is_pristine());}
}
