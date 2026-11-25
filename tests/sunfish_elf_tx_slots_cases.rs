fn slot_full(clock:bool)->(Session,RequestHandle,RequestHandle,RequestHandle){
 let(mut s,h3,h4,elf,_,_)=third_control_setup();if clock{s.set_registry_ack_clock(||77);}
 for peer in 10..16{s.queue_tftp_response(1,qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5001},destination:qcom_qmi_router::Address{node:0,port:peer}},b"\0\x04\0\x01").unwrap();}
 (s,h3,h4,elf)
}
#[test]
fn elf_tx_slots_capture_exact_full_pool_first_retirement_and_reuse(){
 for clock in [false,true]{
 let(mut s,h3,h4,elf)=slot_full(clock);assert!(s.elf_tx_slots().is_none());assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));let c=*s.elf_tx_slots().unwrap();assert_eq!((c.failures,c.first_free,c.admitted_slot),(1,None,None));assert_eq!(c.first.at,clock.then_some(77));
 assert_eq!(c.first.slots.iter().filter(|s|s.kind==4).count(),2);assert_eq!(c.first.slots.iter().filter(|s|s.kind==6).count(),6);for r in c.first.slots.iter().filter(|s|s.kind==6){assert_eq!((r.op,r.len,r.sent,r.source.port),(4,36,0,0x5001));assert_eq!(r.remote,u32::MAX);assert_eq!(r.flags&1,1);}
 let id=tx(&mut s);assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));let c=*s.elf_tx_slots().unwrap();assert_eq!(c.failures,2);assert_eq!(c.first.slots.iter().filter(|r|r.remote==id).count(),0);let i=c.latest.slots.iter().position(|r|r.remote==id).unwrap();assert_eq!(c.latest.slots[i].kind,6);assert!(c.first_free.is_none());
 s.ingest(1,&control(14,id)).unwrap();let freed=s.elf_tx_slots().unwrap().first_free.unwrap();assert_eq!((freed.slot as usize,freed.cause),(i,1));assert_ne!(freed.empty_mask&(1<<i),0);assert_eq!(freed.original,c.first.slots[i]);assert!(s.reply_fully_published(&h3)&&s.reply_fully_published(&h4));
 third(&mut s,&elf).unwrap();let frozen=*s.elf_tx_slots().unwrap();assert_eq!(frozen.admitted_slot,Some(i as u8));assert!(frozen.finished);assert_eq!(frozen.admitted_empty&(1<<i),0);let _=tx(&mut s);assert_eq!(s.elf_tx_slots().unwrap().first.slots,frozen.first.slots);assert_eq!(s.elf_tx_slots().unwrap().latest.slots,frozen.latest.slots);assert_eq!(s.elf_tx_slots().unwrap().failures,2);
 }
}
#[test]
fn elf_tx_slots_invalid_receipts_do_not_free_slots_and_reset_clears_capture(){
 let(mut s,_,_,elf)=slot_full(false);assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));assert!(s.ingest(1,&control(14,0xffff)).is_err());assert!(s.elf_tx_slots().unwrap().first_free.is_none());s.reset_after_transport_reset(2).unwrap();assert!(s.elf_tx_slots().is_none());
}
#[test]
fn elf_tx_slots_blocked_or_uncertain_publication_does_not_free_capacity(){
 let(mut s,_,_,elf)=slot_full(false);assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));assert_eq!(s.publish_next(1,|_|Err(WriteError::WouldBlock)),Err(Error::WouldBlock));assert!(s.elf_tx_slots().unwrap().first_free.is_none());assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));assert!(s.elf_tx_slots().unwrap().first_free.is_none());
}
#[test]
fn elf_tx_slots_glink_done_keeps_credit_owner_until_exact_resume(){
 let(mut s,_,_,elf,_,_)=third_control_setup();for peer in 10..16{s.queue_tftp_response(1,qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x6000},destination:qcom_qmi_router::Address{node:0,port:peer}},b"\0\x04\0\x01").unwrap();}
 assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));let id=tx(&mut s);s.ingest(1,&control(14,id)).unwrap();assert!(s.elf_tx_slots().unwrap().first_free.is_none(),"GLINK alone cannot retire legacy credit owner");assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));let i=s.elf_tx_slots().unwrap().latest.slots.iter().position(|r|r.kind==6&&r.flags&8!=0).unwrap();let peer=s.elf_tx_slots().unwrap().latest.slots[i].destination.port;
 let rid=offer(&mut s);s.ingest(1,&data(9,rid,&resume_route(peer,0x6000),0)).unwrap();let freed=s.elf_tx_slots().unwrap().first_free.unwrap();assert_eq!((freed.slot as usize,freed.cause),(i,2));third(&mut s,&elf).unwrap();
}
