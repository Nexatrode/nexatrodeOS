// Exact third-reply admission; ordinary callers retain the two-reply limit.
fn third(s:&mut Session,h:&RequestHandle)->Result<(),Error>{
 let r=qcom_servreg::respond(qcom_servreg::Query{service_name:b"kernel/elf_loader",domain_offset:0});
 s.queue_elf_third_reply(h,Reply::Success{optional_tlvs:r.optional_tlvs()})
}
fn third_control_setup()->(Session,RequestHandle,RequestHandle,RequestHandle,u32,Option<u32>){
 let mut s=Session::new(config(),1).unwrap();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();s.ingest(1,&intents(&[(100,4096),(101,4096),(102,4096)])).unwrap();
 let id=offer(&mut s);s.ingest(1,&data(9,id,&qrtr_control(2,[0;4]),0)).unwrap();for _ in 0..3{let i=tx(&mut s);s.ingest(1,&control(14,i)).unwrap();}
 s.ingest(1,&intents(&[(7,4096),(8,4096),(9,4096),(10,4096)])).unwrap();let ids=[offer(&mut s),offer(&mut s),offer(&mut s)];let h3=request(&mut s,ids[0],3,b"avs/audio");queue(&mut s,&h3,b"avs/audio").unwrap();let i3=tx(&mut s);let h4=request(&mut s,ids[1],4,b"tms/servreg");queue(&mut s,&h4,b"tms/servreg").unwrap();let i4=tx(&mut s);let elf=request(&mut s,ids[2],5,b"kernel/elf_loader");(s,h3,h4,elf,i3,Some(i4))
}
#[test]
fn elf_third_keeps_three_distinct_owners_and_all_completion_orders(){
 for observed in [false,true]{for order in [[0,1,2],[0,2,1],[1,0,2],[1,2,0],[2,0,1],[2,1,0]]{
  let(mut s,h3,h4,elf,i3,i4)=setup(observed,true);assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));third(&mut s,&elf).unwrap();let a=s.elf_window_trace().unwrap().admitted.unwrap();assert_eq!((a.count,a.gates),(2,1));assert_eq!(s.elf_window_trace().unwrap().receipt_count,0);
  let ie=tx(&mut s);let hs=[h3,h4,elf];let ids=[i3,i4.unwrap(),ie];assert!(ids[0]!=ids[1]&&ids[0]!=ids[2]&&ids[1]!=ids[2]);
  let id=offer(&mut s);let fourth=request(&mut s,id,6,b"kernel/elf_loader");assert_eq!(third(&mut s,&fourth),Err(Error::FlowBusy),"hard limit3");
  for(n,i)in order.into_iter().enumerate(){for j in order[n..].iter().copied(){assert!(s.reply_fully_published(&hs[j]));}done(&mut s,hs[i],ids[i]);if n==0{assert_eq!(queue(&mut s,&fourth,b"kernel/elf_loader"),Err(Error::FlowBusy),"ordinary still permits only two");} }
  third(&mut s,&fourth).unwrap();assert!(!s.reply_fully_published(&fourth));
 }}
}
#[test]
fn elf_third_requires_both_older_publications_and_exact_typed_request(){
 let(mut s,_,_,elf,_,_)=setup(false,false);assert_eq!(third(&mut s,&elf),Err(Error::FlowBusy));let _=tx(&mut s);third(&mut s,&elf).unwrap();
 let(mut s,_,_,_,_,_)=setup(false,true);let id=offer(&mut s);let other=request(&mut s,id,6,b"tms/servreg");assert_eq!(third(&mut s,&other),Err(Error::InvalidReply));
 let(mut s,_,_,elf,_,_)=setup(false,true);s.reset_after_transport_reset(2).unwrap();assert!(third(&mut s,&elf).is_err());
}
#[test]
fn elf_third_preserves_credit_and_global_transmit_capacity(){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,4096),(8,4096),(9,4096)])).unwrap();
 for txn in 1..=8{let id=offer(&mut s);let h=request(&mut s,id,txn,b"tms/servreg");queue(&mut s,&h,b"tms/servreg").unwrap();let i=tx(&mut s);done(&mut s,h,i);s.ingest(1,&intents(&[(100+u32::from(txn),4096)])).unwrap();}
 let id=offer(&mut s);let h3=request(&mut s,id,9,b"tms/servreg");queue(&mut s,&h3,b"tms/servreg").unwrap();let _=tx(&mut s);
 let id=offer(&mut s);let h4=request(&mut s,id,10,b"tms/servreg");queue(&mut s,&h4,b"tms/servreg").unwrap();let _=tx(&mut s);
 let id=offer(&mut s);let elf=request(&mut s,id,11,b"kernel/elf_loader");assert_eq!(third(&mut s,&elf),Err(Error::FlowBusy));assert_eq!(s.registry_queue_pressure(&elf).unwrap().reason,4,"actual reference credit still required");
 let(mut s,h3,h4,elf,_,_)=third_control_setup();
 for peer in 10..16{s.queue_tftp_response(1,qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5001},destination:qcom_qmi_router::Address{node:0,port:peer}},b"\0\x04\0\x01").unwrap();}
 assert_eq!(third(&mut s,&elf),Err(Error::NoCapacity));assert!(s.reply_fully_published(&h3)&&s.reply_fully_published(&h4));assert_eq!(s.registry_queue_pressure(&elf).unwrap().reason,2);
}
#[test]
fn elf_third_uses_fitting_granted_intent_and_preserves_blocked_bytes(){
 let(mut s,h3,h4,elf,_,_)=third_control_setup();
 // Startup RX_DONE_REUSE leaves three available intents; occupy all seven.
 for peer in 10..15{s.queue_tftp_response(1,qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5001},destination:qcom_qmi_router::Address{node:0,port:peer}},b"\0\x04\0\x01").unwrap();let _=tx(&mut s);}
 third(&mut s,&elf).unwrap();let mut asked=false;
 for _ in 0..16{match s.publish_next(1,|_|Ok(())){Ok(Some(Published::RemoteIntentRequest{..}))=>{asked=true;break;},Ok(_)=>{},Err(Error::NoRemoteIntent)=>{},r=>panic!("{r:?}")}}
 assert!(asked);assert!(!s.reply_fully_published(&elf));s.ingest(1,&control(8,1)).unwrap();s.ingest(1,&intents(&[(99,4096)])).unwrap();
 let mut blocked=Vec::new();assert_eq!(s.publish_next(1,|b|{blocked.extend_from_slice(b);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));let mut retried=Vec::new();s.publish_next(1,|b|{retried.extend_from_slice(b);Ok(())}).unwrap();assert_eq!(blocked,retried);
 if !s.reply_fully_published(&elf){assert_eq!(tx(&mut s),99);}assert!(s.reply_fully_published(&h3)&&s.reply_fully_published(&h4));
}
#[test]
fn elf_third_uncertain_writer_and_wrong_reply_never_release_older_owners(){
 let(mut s,h3,h4,elf,_,_)=setup(false,true);assert!(s.queue_elf_third_reply(&elf,Reply::Failure{error:0}).is_err());assert!(s.reply_fully_published(&h3)&&s.reply_fully_published(&h4));third(&mut s,&elf).unwrap();
 assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));assert!(s.publish_next(1,|_|panic!("quarantined")).is_err());assert!(!s.reply_fully_published(&elf));
}
