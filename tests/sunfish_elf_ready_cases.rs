use super::*;
fn queued_elf(before:bool,confirm:bool,txn:u16,name:&[u8],offset:u32,capacity:u32,success:bool,returned:bool)->(Session,RequestHandle,u32){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,capacity),(8,capacity)])).unwrap();
 let id=offer(&mut s);let mut tlv=vec![1,name.len()as u8,0];tlv.extend(name);if offset!=0{tlv.extend([0x10,4,0]);tlv.extend(offset.to_le_bytes());}let mut qmi=vec![0,txn as u8,(txn>>8)as u8,0x21,0,tlv.len()as u8,0];qmi.extend(tlv);let p=qrtr(1,9,qcom_servreg::LOCAL_PORT,confirm,&qmi);let Event::Request(h)=s.ingest(1,&data(9,id,&p,0)).unwrap()else{panic!("ELF request")};
 if before&&returned{for _ in 0..8{if publish(&mut s).unwrap().0==Published::ReceiveDone(id){break;}}}
 let response=qcom_servreg::respond(qcom_servreg::Query{service_name:name,domain_offset:offset});s.queue_reply(&h,if success{Reply::Success{optional_tlvs:response.optional_tlvs()}}else{Reply::Failure{error:3}}).unwrap();
 if !before&&returned{for _ in 0..8{if publish(&mut s).unwrap().0==Published::ReceiveDone(id){break;}}}
 s.ingest(1,&control(7,108)).unwrap();(s,h,id)
}
fn first_elf(before:bool,confirm:bool,capacity:u32)->(Session,RequestHandle,u32){queued_elf(before,confirm,77,b"kernel/elf_loader",0,capacity,true,true)}
fn elf_publication(s:&Session)->qcom_glink_qmi::RegistryPublication{s.elf_window_trace().unwrap().writer.publication.unwrap()}
#[test]fn elf_ready_real_query_borrows_after_own_return_before_fresh_ad(){
 for before in [false,true]{for clocked in [false,true]{let(mut s,h,_)=first_elf(before,false,4096);if clocked{s.set_registry_ack_clock(||123);}
 let(id,b)=reply(&mut s);assert_eq!(&b[49..51],&77u16.to_le_bytes());assert_eq!(elf_publication(&s).mask&0x180000,0x180000);grant(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));}}
}
#[test]fn elf_ready_blocked_loan_is_spent_and_ordinary_retry_is_identical(){
 let(mut s,h,_)=first_elf(true,false,4096);let mut blocked=vec![];assert_eq!(s.publish_next(1,|b|{blocked=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));assert_eq!(elf_publication(&s).mask&0x180000,0x80000);grant(&mut s);s.ingest(1,&control(7,108)).unwrap();grant(&mut s);let(id,b)=reply(&mut s);assert_eq!(b,blocked);assert_eq!(elf_publication(&s).mask&0x180000,0x80000);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn elf_ready_credit_and_already_advertised_ack_keep_their_turns(){
 #[cfg(not(feature="sunfish-elf-credit-pair-turn"))]
 {let(mut s,_,_)=first_elf(true,true,4096);grant(&mut s);let(_,b)=reply(&mut s);assert_eq!(word(&b,20),7);let _=reply(&mut s);assert_eq!(elf_publication(&s).mask&0x180000,0);}
 #[cfg(feature="sunfish-elf-credit-pair-turn")]
 {let(mut s,_,_)=first_elf(true,true,4096);let(_,b)=reply(&mut s);assert_eq!(word(&b,20),7);let _=reply(&mut s);grant(&mut s);assert_eq!(elf_publication(&s).mask&0x1e0000,0x1e0000);}
 let(mut s,_,_)=first_elf(true,false,16);assert!(matches!(publish(&mut s).unwrap().0,Published::ReceiveIntent(_)));s.ingest(1,&intents(&[(9,4096)])).unwrap();assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});let _=reply(&mut s);assert_eq!(elf_publication(&s).mask&0x180000,0);
}
#[test]fn elf_ready_only_first_successful_domain_zero_exact_name_can_bind(){
 for(name,offset,success)in [(b"other/service".as_slice(),0,true),(b"kernel/elf_loader".as_slice(),1,true),(b"kernel/elf_loader".as_slice(),0,false)]{let(mut s,_,_)=queued_elf(true,false,77,name,offset,4096,success,true);grant(&mut s);let _=reply(&mut s);}
 let(mut s,h,_)=first_elf(true,false,4096);let(id,_)=reply(&mut s);grant(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));
 let id=offer(&mut s);let name=b"kernel/elf_loader";let mut q=vec![0,78,0,0x21,0,20,0,1,17,0];q.extend(name);let p=qrtr(1,9,qcom_servreg::LOCAL_PORT,false,&q);let Event::Request(h)=s.ingest(1,&data(9,id,&p,0)).unwrap()else{panic!()};let r=qcom_servreg::respond(qcom_servreg::Query{service_name:name,domain_offset:0});s.queue_reply(&h,Reply::Success{optional_tlvs:r.optional_tlvs()}).unwrap();assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));s.ingest(1,&control(7,108)).unwrap();grant(&mut s);let _=reply(&mut s);
}
#[test]fn elf_ready_missing_fit_uncertainty_and_reset_preserve_boundaries(){
 let(mut s,_,_)=first_elf(true,false,16);grant(&mut s);assert!(matches!(publish(&mut s).unwrap().0,Published::RemoteIntentRequest{..}));s.ingest(1,&control(8,1)).unwrap();s.ingest(1,&intents(&[(9,4096)])).unwrap();let _=reply(&mut s);assert_eq!(elf_publication(&s).mask&0x180000,0);
 for panic_write in [false,true]{let(mut s,_,_)=first_elf(true,false,4096);if panic_write{assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||s.publish_next(1,|_|panic!("ELF fault")))).is_err());}else{assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));}assert_eq!(s.publish_next(1,|_|panic!("no retry")),Err(Error::Quarantined));s.reset_after_transport_reset(2).unwrap();assert!(s.transport_is_pristine());}
}

#[test]fn elf_ready_real_own_return_and_ordinary_call_are_required(){
 let(mut s,h,id)=queued_elf(false,false,77,b"kernel/elf_loader",0,4096,true,false);grant(&mut s);assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(id));let(i,_)=reply(&mut s);assert_eq!(s.ingest(1,&control(14,i)),Ok(Event::ReplyRxDone(h)));assert_eq!(elf_publication(&s).mask&0x180000,0);
 let(mut s,_,_)=first_elf(true,false,4096);let e=s.publish_oack_followup(1,qcom_glink_qmi::OackFollowup::Advertise,|_|Ok(())).unwrap();assert!(!matches!(e,Some(Published::Data{..})));assert_eq!(s.elf_window_trace().unwrap().writer.publication.map_or(0,|p|p.mask)&0x180000,0);
 if matches!(e,Some(Published::ReceiveIntent(_))){assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});}let _=reply(&mut s);
}

#[cfg(feature="sunfish-elf-credit-pair-turn")]
#[path="sunfish_elf_credit_cases.rs"]
mod elf_credit_cases;
