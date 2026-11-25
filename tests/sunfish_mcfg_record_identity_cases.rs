// Actual Core lifecycle; clocks are optional and never used as record identity.
use qcom_glink_qmi::TftpReplyRoute;
fn identity_setup(clock:bool)->(Session,Vec<u8>,TftpReplyRoute,u32){identity_setup_with_blocker(clock,false)}
fn identity_setup_with_blocker(clock:bool,blocker:bool)->(Session,Vec<u8>,TftpReplyRoute,u32){
 let mut s=session();if clock{s.set_registry_ack_clock(||1234);}let mut wire=Vec::new();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();
 let mut adv=control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}s.ingest(1,&adv).unwrap();drain(&mut s,&mut wire);let id=offer(&mut s,&mut wire);s.ingest(1,&data(id,&owner_control_frame(2,[0;4]))).unwrap();drain(&mut s,&mut wire);
 let id=offer(&mut s,&mut wire);let Event::Request(elf)=s.ingest(1,&data(id,&servreg_query_datagram_with_transaction(b"kernel/elf_loader",5))).unwrap()else{panic!("ELF");};let response=qcom_servreg::respond(qcom_servreg::Query{service_name:b"kernel/elf_loader",domain_offset:0});s.queue_reply(&elf,Reply::Success{optional_tlvs:response.optional_tlvs()}).unwrap();s.observe_elf_reply(&elf);let rid=full_data(&mut s,&mut wire);s.ingest(1,&control(14,2,rid)).unwrap();drain(&mut s,&mut wire);
 let h=receive(&mut s,&mut wire,0x4a,0x5000,STAT);s.finish_tftp_request(&h).unwrap();drain(&mut s,&mut wire);let close=offer(&mut s,&mut wire);
 let mut server=qcom_tftp_server_check::ServerCheck::new();let mut out=[0;128];let reply=server.receive(qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x4a},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}},STAT,&mut out).unwrap();assert_eq!(&out[..reply.len],qcom_tftp_server_check::MISSING_REPLY);
 let peer=qcom_qmi_router::Address{node:0,port:0x4a};let mut cache=[None;qcom_tftp_server_check::TRANSFER_COUNT];cache[0]=Some((peer,reply.route.source.port));s.set_mcfg_close_transfers(1,cache).unwrap();s.observe_post_mcfg_dispatch(&h,None,Ok(reply),&out);
 if blocker{let route=TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:reply.route.source.port},destination:peer};s.queue_tftp_response(1,route,&out[..reply.len]).unwrap();let id=full_data(&mut s,&mut wire);for _ in 0..3{assert_eq!(s.queue_traced_tftp_response(1,&h,route,&out[..reply.len],false),Err(qcom_glink_qmi::Error::FlowBusy));}s.ingest(1,&control(14,2,id)).unwrap();}
 s.queue_traced_tftp_response(1,&h,TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:reply.route.source.port},destination:peer},&out[..reply.len],false).unwrap();
 (s,wire,TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:reply.route.source.port},destination:peer},close)
}
fn identity_close(s:&mut Session,route:TftpReplyRoute,close:u32)->qcom_glink_qmi::McfgCloseOutcome{
 assert_eq!(s.ingest(1,&data(close,&owner_control_frame(6,[0,route.destination.port,0,0]))).unwrap(),Event::McfgClosePending{peer:route.destination});
 s.commit_mcfg_close(1,route.destination,Some(route.source.port),&[None;qcom_glink_qmi::MAX_REQUESTS]).unwrap()
}
#[test]
fn post_mcfg_identity_cancel_reuse_never_becomes_completion(){
 for clock in [false,true]{let(mut s,mut wire,route,close)=identity_setup(clock);let seq=s.post_elf_mcfg().unwrap().mcfg.unwrap().record_sequence();assert!(seq.is_some());
 assert_eq!(identity_close(&mut s,route,close).unsent,1);let before=wire.len();
 s.queue_tftp_response(1,TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5002},destination:qcom_qmi_router::Address{node:0,port:0x4b}},b"\0\x04\0\x01").unwrap();let other=full_data(&mut s,&mut wire);s.ingest(1,&control(14,2,other)).unwrap();let sent=owner_datagrams(&wire[before..]);assert_eq!(sent.len(),1);assert_eq!((lo_word(&sent[0],12),lo_word(&sent[0],28)),(0x5002,0x4b));
 let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!(m.flags&0x30,0,"foreign reuse must not publish or complete the canceled original");assert_ne!(m.flags&256,0,"cancellation must be explicit even without optional clocks");assert_eq!((m.published,m.done),(None,None));assert_eq!(m.canceled_at,clock.then_some(1234));assert_eq!(m.record_sequence(),seq);
 assert_eq!(s.post_elf_mcfg().unwrap().journal.iter().flatten().filter(|r|r.kind==253).count(),1);assert_eq!(s.tftp_tx_progress()[2],1);
 }
}
#[test]
fn post_mcfg_identity_blocked_then_real_publication_and_completion(){
 for clock in [false,true]{let(mut s,mut wire,route,close)=identity_setup(clock);let seq=s.post_elf_mcfg().unwrap().mcfg.unwrap().record_sequence();
 // Consume ordinary receive-return work; stop at the first DATA attempt.
 loop{let result=s.publish_next(1,|bytes|if u16::from_le_bytes([bytes[0],bytes[1]])==9{Err(qcom_glink_qmi::WriteError::WouldBlock)}else{Ok(())});if result==Err(qcom_glink_qmi::Error::WouldBlock){break;}assert!(result.unwrap().is_some());}
 assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&0x30,0);let id=full_data(&mut s,&mut wire);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&0x30,16);s.ingest(1,&control(14,2,id)).unwrap();
 let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!(m.flags&0x130,48);assert_eq!((m.published,m.done),(clock.then_some(1234),clock.then_some(1234)));assert_eq!(m.record_sequence(),seq);
 assert_eq!(identity_close(&mut s,route,close).published,0);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&256,0);
 }
}
#[test]
fn post_mcfg_identity_close_preserves_already_published_original(){
 let(mut s,mut wire,route,close)=identity_setup(false);let id=full_data(&mut s,&mut wire);let out=identity_close(&mut s,route,close);assert_eq!((out.unsent,out.published),(0,1));assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&256,0);s.ingest(1,&control(14,2,id)).unwrap();assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&0x30,48);
}
#[test]
fn post_mcfg_identity_uncertainty_never_invents_a_completion(){
 let(mut s,_wire,_route,_close)=identity_setup(false);loop{let result=s.publish_next(1,|bytes|if u16::from_le_bytes([bytes[0],bytes[1]])==9{Err(qcom_glink_qmi::WriteError::Uncertain)}else{Ok(())});if result==Err(qcom_glink_qmi::Error::PublicationUncertain){break;}assert!(result.unwrap().is_some());}
 assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&0x30,0);assert!(s.publish_next(1,|_|panic!("uncertain state must not publish")).is_err());
}
#[test]
fn post_mcfg_identity_transport_reset_clears_canceled_record(){
 let(mut s,_wire,route,close)=identity_setup(false);identity_close(&mut s,route,close);assert_ne!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&256,0);s.reset_after_transport_reset(2).unwrap();assert!(s.post_elf_mcfg().is_none());assert_eq!(s.publish_next(1,|_|panic!("stale generation must not publish")),Err(qcom_glink_qmi::Error::StaleGeneration));
}
