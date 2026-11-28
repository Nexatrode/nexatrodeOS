// Same real Owner schedule with both feature closures; capture must not authorize I/O.
#[test]
fn first_mcfg_early_owner_scope_wire_and_io_are_observation_only(){
 let(p,mut o)=owner();let mut irq=Irqs::default();o.prepare_tftp_server(0x5000).unwrap();o.prepare_service_registry().unwrap();o.start_control().unwrap();let mut adv=control(5,2,16);for id in 0u32..16{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}p.push(&adv);o.notified(8,&mut irq).unwrap();p.take_tx();owner_control_peer_hello(&p,&mut o,&mut irq);take_owner_datagrams(&p,&mut o,&mut irq);let mut server=qcom_tftp_server_check::ServerCheck::new();let mut out=[0;128];commit_one(&mut server,&mut out);
 let id=o.offer_receive().unwrap();o.notified(8,&mut irq).unwrap();p.take_tx();p.push(&data(id,&servreg_query_datagram_with_transaction(b"kernel/elf_loader",5)));o.notified(1,&mut irq).unwrap();
 assert!(!o.service_registry_trace().elf_exchange.is_some_and(|e|e.completed));assert!(o.post_elf_mcfg().unwrap().mcfg.is_none());
 mc_input(&p,&mut o,&mut irq,0x49,0x5000,STAT);
 assert_eq!(o.post_elf_mcfg().unwrap().mcfg.is_some(),cfg!(feature="sunfish-first-mcfg-response"));
 o.service_tftp_server_check_step(&mut server,&mut irq).unwrap();o.notified(8,&mut irq).unwrap();let wire=p.take_tx();let packets=owner_datagrams(&wire);assert_eq!(packets.len(),1);assert_eq!(lo_word(&packets[0],12),0x5002);assert_eq!(lo_word(&packets[0],28),0x49);assert_eq!(&packets[0][32..34],b"\0\x06");
 if cfg!(feature="sunfish-first-mcfg-response"){let m=o.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!((m.peer.port,m.file_before,m.server_result,m.reply_opcode,m.flags&63),(0x49,Some(1),Some(0),6,31));assert!(m.record_sequence().is_some());}
 acknowledge_outgoing_glink(&p,&mut o,&mut irq,&wire);
 if cfg!(feature="sunfish-first-mcfg-response"){let m=o.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!(m.flags&63,63);mc_input(&p,&mut o,&mut irq,0x49,m.transfer_port,END);}else{mc_input(&p,&mut o,&mut irq,0x49,0x5002,END);}
 o.service_tftp_server_check_step(&mut server,&mut irq).unwrap();assert_eq!(server.mcfg_progress(),(1,Some(1),0));assert!(o.error().is_none());
 if cfg!(feature="sunfish-first-mcfg-response"){assert_eq!(o.post_elf_mcfg().unwrap().mcfg.unwrap().first_client,Some(0x50009));}
 print!("FIRST_MCFG_WIRE ");for b in wire{print!("{b:02x}");}println!();println!("FIRST_MCFG_IO {:?}",p.log());
}
#[cfg(feature="sunfish-first-mcfg-response")]
fn first_mcfg_core()->(Session,Vec<u8>,RequestHandle){
 let mut s=session();let mut wire=vec![];s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();let mut adv=control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}s.ingest(1,&adv).unwrap();drain(&mut s,&mut wire);let id=offer(&mut s,&mut wire);s.ingest(1,&data(id,&owner_control_frame(2,[0;4]))).unwrap();drain(&mut s,&mut wire);
 let id=offer(&mut s,&mut wire);let Event::Request(elf)=s.ingest(1,&data(id,&servreg_query_datagram_with_transaction(b"kernel/elf_loader",5))).unwrap()else{panic!("ELF")};(s,wire,elf)
}
#[cfg(feature="sunfish-first-mcfg-response")]
#[test]
fn first_mcfg_pending_elf_duplicate_identity_backpressure_and_real_completion(){
 let(mut s,mut wire,elf)=first_mcfg_core();let h=receive(&mut s,&mut wire,0x49,0x5000,STAT);s.finish_tftp_request(&h).unwrap();drain(&mut s,&mut wire);let duplicate=receive(&mut s,&mut wire,0x49,0x5000,STAT);s.finish_tftp_request(&duplicate).unwrap();drain(&mut s,&mut wire);assert_ne!(h,duplicate);
 let mut server=qcom_tftp_server_check::ServerCheck::new();let mut out=[0;128];let r=server.receive(qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x49},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}},STAT,&mut out).unwrap();assert_eq!(&out[..r.len],qcom_tftp_server_check::MISSING_REPLY);
 s.observe_post_mcfg_dispatch(&duplicate,None,Ok(r),&out);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,1);s.observe_post_mcfg_dispatch(&h,None,Ok(r),&out);
 let route=qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:r.route.source.port},destination:qcom_qmi_router::Address{node:0,port:0x49}};
 s.queue_traced_tftp_response(1,&duplicate,route,&out[..r.len],false).unwrap();let other=full_data(&mut s,&mut wire);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,7);assert_eq!(s.queue_traced_tftp_response(1,&h,route,&out[..r.len],false),Err(qcom_glink_qmi::Error::FlowBusy));s.ingest(1,&control(14,2,other)).unwrap();assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,7);
 s.queue_traced_tftp_response(1,&h,route,&out[..r.len],false).unwrap();assert_eq!(s.publish_next(1,|_|Err(qcom_glink_qmi::WriteError::WouldBlock)),Err(qcom_glink_qmi::Error::WouldBlock));assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,15);let target=full_data(&mut s,&mut wire);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,31);s.ingest(1,&control(14,2,target)).unwrap();assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,63);
 assert!(!s.elf_exchange().is_some_and(|e|e.completed));assert!(s.request(&elf).is_ok());let before=s.post_elf_mcfg().unwrap().mcfg.unwrap().record_sequence();let _=receive(&mut s,&mut wire,0x4a,0x5000,STAT);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().record_sequence(),before);
 s.reset_after_transport_reset(2).unwrap();assert!(s.post_elf_mcfg().is_none());
}
#[cfg(feature="sunfish-first-mcfg-response")]
#[test]
fn first_mcfg_before_elf_is_not_retroactively_bound_or_rebound(){
 let(p,mut o,mut irq)=mc_owner();mc_input(&p,&mut o,&mut irq,0x49,0x5000,STAT);assert!(o.post_elf_mcfg().is_none());let id=mc_offer(&p,&mut o,&mut irq);p.push(&data(id,&servreg_query_datagram_with_transaction(b"kernel/elf_loader",5)));o.notified(1,&mut irq).unwrap();assert!(o.post_elf_mcfg().unwrap().mcfg.is_none());let mut server=qcom_tftp_server_check::ServerCheck::new();o.service_tftp_server_check_step(&mut server,&mut irq).unwrap();assert!(o.post_elf_mcfg().unwrap().mcfg.is_none());
 let(mut s,mut wire,_)=first_mcfg_core();let h=receive(&mut s,&mut wire,0x49,0x5000,STAT);let before=s.post_elf_mcfg().unwrap().mcfg.unwrap().received;let _=receive(&mut s,&mut wire,0x4a,0x5000,STAT);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().peer.port,0x49);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().received,before);assert!(s.tftp_datagram(&h).is_ok());
}
#[cfg(feature="sunfish-first-mcfg-response")]
#[test]
fn first_mcfg_early_close_or_uncertain_write_never_fabricates_receipt(){
 for mode in 0..3{let(mut s,mut wire,_)=first_mcfg_core();let h=receive(&mut s,&mut wire,0x49,0x5000,STAT);s.finish_tftp_request(&h).unwrap();drain(&mut s,&mut wire);
 let mut server=qcom_tftp_server_check::ServerCheck::new();let mut bytes=[0;128];let r=server.receive(qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x49},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}},STAT,&mut bytes).unwrap();let route=TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:r.route.source.port},destination:qcom_qmi_router::Address{node:0,port:0x49}};let mut cache=[None;qcom_tftp_server_check::TRANSFER_COUNT];cache[0]=Some((route.destination,route.source.port));s.set_mcfg_close_transfers(1,cache).unwrap();s.observe_post_mcfg_dispatch(&h,None,Ok(r),&bytes);s.queue_traced_tftp_response(1,&h,route,&bytes[..r.len],false).unwrap();let close=offer(&mut s,&mut wire);
 if mode==0{assert_eq!(identity_close(&mut s,route,close).unsent,1);let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_ne!(m.flags&256,0);assert_eq!(m.flags&48,0);}else if mode==1{let id=full_data(&mut s,&mut wire);identity_close(&mut s,route,close);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&0x130,16);s.ingest(1,&control(14,2,id)).unwrap();assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&0x130,48);}else{loop{let r=s.publish_next(1,|b|if b[0]==9{Err(qcom_glink_qmi::WriteError::Uncertain)}else{Ok(())});if r==Err(qcom_glink_qmi::Error::PublicationUncertain){break;}assert!(r.unwrap().is_some());}assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags&48,0);assert!(s.publish_next(1,|_|panic!("no retry")).is_err());}
 s.reset_after_transport_reset(2).unwrap();assert!(s.post_elf_mcfg().is_none());}
}
