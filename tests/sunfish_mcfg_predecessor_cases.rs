// A captured predecessor has a separate immutable reservation, never target authority.
#[test]
fn post_mcfg_predecessor_unsent_turn_then_original_target(){
 for clock in [false,true]{let(mut s,mut wire,_,_)=identity_setup_with_order(clock,true,false);let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();let a=m.admission;
 assert_eq!((a.block_len,a.block_sent,a.block[0]),(52,0,1));assert_eq!(a.block_published,clock.then_some(1234));assert_eq!(a.block_completed,clock.then_some(1234));assert_eq!(m.flags&0x30,0);assert!(a.block_sequence.unwrap()<m.record_sequence().unwrap());
 assert!(a.records.iter().flatten().any(|r|r.gates&0x1000!=0&&r.command==9&&r.result==1&&r.destination==0x4a&&r.record_sequence==a.block_sequence));
 let id=full_data(&mut s,&mut wire);s.ingest(1,&control(14,2,id)).unwrap();let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!(m.flags&0x30,48);assert!(m.admission.records.iter().flatten().any(|r|r.gates&0x1000==0&&r.command==9&&r.result==1));
 }
}
#[test]
fn post_mcfg_predecessor_already_sent_does_not_invent_publication_clock(){
 let(mut s,mut wire,_,_)=identity_setup_with_order(true,true,true);let a=s.post_elf_mcfg().unwrap().mcfg.unwrap().admission;
 assert_eq!(a.block_sent,a.block_len);assert_eq!(a.block_published,None);assert_eq!(a.block_completed,Some(1234));assert!(!a.records.iter().flatten().any(|r|r.gates&0x1000!=0));
 s.set_registry_ack_clock(||5678);let id=full_data(&mut s,&mut wire);s.ingest(1,&control(14,2,id)).unwrap();assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().admission.block_published,None);
}
#[test]
fn post_mcfg_predecessor_old_slot_reuse_cannot_rewrite_receipts(){
 let(mut s,mut wire,route,close)=identity_setup_with_order(true,true,false);s.set_registry_ack_clock(||5678);let before=s.post_elf_mcfg().unwrap().mcfg.unwrap().admission;identity_close(&mut s,route,close);
 s.queue_tftp_response(1,TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5002},destination:qcom_qmi_router::Address{node:0,port:0x4b}},b"\0\x04\0\x01").unwrap();drain(&mut s,&mut wire);
 let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!((m.admission.block_published,m.admission.block_completed),(before.block_published,before.block_completed));assert_eq!(m.admission.passes,before.passes);assert_eq!((m.published,m.done),(None,None));
}
