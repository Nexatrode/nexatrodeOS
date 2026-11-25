// Uses actual Core arbitration and immutable original QRTR reservations.
#[test]
fn post_mcfg_writer_same_peer_predecessor_is_not_queue_capacity(){
 for clock in [false,true]{let(s,_wire,_route,_close)=identity_setup_with_blocker(clock,true);let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();let a=m.admission;assert_eq!(a.attempts,4);assert_eq!(m.queue_waits,[3,0]);assert_eq!(a.block[0],1);assert_eq!(a.block[1..3],[6,5]);assert_eq!(a.block[3]&3,3);assert!(a.block_len>0);assert_eq!(a.block_sent,a.block_len);assert!(a.block_sequence.unwrap()<m.record_sequence().unwrap());assert_eq!(a.first_attempt,clock.then_some(1234));}
}
#[test]
fn post_mcfg_writer_real_callback_blocked_then_success(){
 for clock in [false,true]{let(mut s,mut wire,_,_)=identity_setup(clock);let mut blocked=false;for _ in 0..32{let r=s.publish_next(1,|bytes|{if u16::from_le_bytes([bytes[0],bytes[1]])==9{blocked=true;Err(qcom_glink_qmi::WriteError::WouldBlock)}else{Ok(())}});if blocked{assert_eq!(r,Err(qcom_glink_qmi::Error::WouldBlock));break;}r.unwrap();}assert!(blocked);
 let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();let a=m.admission;let r=a.records.iter().flatten().last().unwrap();assert_eq!((r.command,r.kind,r.source,r.destination,r.result),(9,1,m.transfer_port,0x4a,2));assert_ne!(r.gates&(1<<25),0);assert_eq!(r.gates&3,3);assert_eq!((r.start,r.end),(clock.then_some(1234),clock.then_some(1234)));
 let id=full_data(&mut s,&mut wire);s.ingest(1,&control(14,2,id)).unwrap();assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&0x30,48);
 }
}
#[test]
fn post_mcfg_writer_canceled_slot_cannot_acquire_foreign_turns(){
 let(mut s,mut wire,route,close)=identity_setup(false);identity_close(&mut s,route,close);let a=s.post_elf_mcfg().unwrap().mcfg.unwrap().admission;s.queue_tftp_response(1,TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5002},destination:qcom_qmi_router::Address{node:0,port:0x4b}},b"\0\x04\0\x01").unwrap();drain(&mut s,&mut wire);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().admission.passes,a.passes);
}
#[test]
fn post_mcfg_writer_uncertain_and_bounded_history(){
 let(mut s,_,_,_)=identity_setup(false);for _ in 0..16{assert_eq!(s.publish_next(1,|_|Err(qcom_glink_qmi::WriteError::WouldBlock)),Err(qcom_glink_qmi::Error::WouldBlock));}
 let a=s.post_elf_mcfg().unwrap().mcfg.unwrap().admission;assert_eq!((a.passes,a.omitted,a.records.iter().flatten().count()),(16,8,8));assert!(a.records.iter().flatten().all(|r|r.result==2));
 assert_eq!(s.publish_next(1,|_|Err(qcom_glink_qmi::WriteError::Uncertain)),Err(qcom_glink_qmi::Error::PublicationUncertain));let a=s.post_elf_mcfg().unwrap().mcfg.unwrap().admission;assert_eq!(a.records[7].unwrap().result,3);let before=a.passes;assert!(s.publish_next(1,|_|panic!("terminal" )).is_err());assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().admission.passes,before);
}

#[test]
fn post_mcfg_writer_pending_close_fences_writer_without_clock(){
 let(mut s,_wire,route,close)=identity_setup(false);
 assert_eq!(s.ingest(1,&data(close,&owner_control_frame(6,[0,route.destination.port,0,0]))).unwrap(),Event::McfgClosePending{peer:route.destination});
 let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!(m.close_at,None);assert_ne!(m.flags&128,0);let passes=m.admission.passes;
 assert_eq!(s.publish_next(1,|_|panic!("pending close must prevent writer callback")),Err(qcom_glink_qmi::Error::FlowBusy));
 assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().admission.passes,passes);
}
