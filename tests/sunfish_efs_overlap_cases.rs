#[cfg(feature="sunfish-efs-service-handoff")]
#[test]
fn efs_overlap_owner_pair_keeps_independent_matching_receipts(){
 for reverse in [false,true]{
  let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();
  #[cfg(feature="sunfish-efs-reply-overlap")]o.enable_efs_reply_overlap();
  let req=b"\0\x01/readwrite/modem_efs\0octet\0";
  mc_input(&p,&mut o,&mut i,0x21,0x5000,req);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let first=p.take_tx();assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,31);
  mc_input(&p,&mut o,&mut i,0x21,0x5000,req);let result=o.service_tftp_server_check_step(&mut server,&mut i).unwrap();
  #[cfg(feature="sunfish-efs-reply-overlap")]{assert_eq!(result,qcom_modem_owner::TftpServiceProgress::ResponseQueued);assert_eq!(o.efs_overlap_trace().admitted,1);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,31);}
  #[cfg(not(feature="sunfish-efs-reply-overlap"))]{assert_eq!(result,qcom_modem_owner::TftpServiceProgress::WaitingReply);acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();}
  o.notified(8,&mut i).unwrap();let second=p.take_tx();
  #[cfg(feature="sunfish-efs-reply-overlap")]assert_eq!(o.efs_reply_fifo().unwrap().record,o.startup_prerequisites().reads[3].unwrap().record_sequence().unwrap());
  #[cfg(feature="sunfish-efs-reply-overlap")]{if reverse{acknowledge_outgoing_glink(&p,&mut o,&mut i,&second);assert_eq!((o.startup_prerequisites().reads[2].unwrap().flags,o.startup_prerequisites().reads[3].unwrap().flags),(31,63));acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);}else{acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);assert_eq!((o.startup_prerequisites().reads[2].unwrap().flags,o.startup_prerequisites().reads[3].unwrap().flags),(63,31));acknowledge_outgoing_glink(&p,&mut o,&mut i,&second);}}
  #[cfg(not(feature="sunfish-efs-reply-overlap"))]acknowledge_outgoing_glink(&p,&mut o,&mut i,&second);
  assert_eq!((o.startup_prerequisites().reads[2].unwrap().flags,o.startup_prerequisites().reads[3].unwrap().flags),(63,63));assert!(o.error().is_none());
  for wire in [&first,&second]{for packet in owner_datagrams(wire){print!("EFS_OVERLAP_PACKET ");for b in packet{print!("{b:02x}");}println!();}}
 }
}
#[cfg(feature="sunfish-efs-reply-overlap")]
fn efs_overlap_core()->(Session,Vec<u8>){
 let mut s=session();let mut wire=Vec::new();s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();let mut adv=control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}s.ingest(1,&adv).unwrap();drain(&mut s,&mut wire);let id=offer(&mut s,&mut wire);s.ingest(1,&data(id,&owner_control_frame(2,[0;4]))).unwrap();drain(&mut s,&mut wire);(s,wire)
}
#[cfg(feature="sunfish-efs-reply-overlap")]
fn efs_overlap_handles(s:&mut Session,w:&mut Vec<u8>,second:&[u8])->(qcom_glink_qmi::TftpHandle,qcom_glink_qmi::TftpHandle,qcom_glink_qmi::TftpReplyRoute){
 let first=receive(s,w,0x21,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");let next=receive(s,w,0x21,0x5000,second);s.finish_tftp_request(&first).unwrap();s.finish_tftp_request(&next).unwrap();drain(s,w);(first,next,qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5003},destination:qcom_qmi_router::Address{node:0,port:0x21}})
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_core_unpublished_predecessor_waits_then_exact_second_admits(){
 let(mut s,mut w)=efs_overlap_core();s.enable_efs_reply_overlap();let(a,b,r)=efs_overlap_handles(&mut s,&mut w,b"\0\x01/readwrite/modem_efs\0octet\0");let body=qcom_tftp_server_check::MISSING_REPLY;s.queue_traced_tftp_response(1,&a,r,body,false).unwrap();assert_eq!(s.queue_traced_tftp_response(1,&b,r,body,false),Err(qcom_glink_qmi::Error::FlowBusy));assert_eq!(s.efs_overlap_trace().offers,0);let first=full_data(&mut s,&mut w);s.queue_traced_tftp_response(1,&b,r,body,false).unwrap();let second=full_data(&mut s,&mut w);assert_ne!(first,second);assert_eq!(s.efs_overlap_trace().admitted,1);s.ingest(1,&control(14,2,second)).unwrap();s.ingest(1,&control(14,2,first)).unwrap();
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_core_nonidentical_request_cannot_use_exception(){
 let(mut s,mut w)=efs_overlap_core();s.enable_efs_reply_overlap();let(a,b,r)=efs_overlap_handles(&mut s,&mut w,b"\0\x01/readwrite/modem_efs\0octet\0rsize\x000\0");let body=qcom_tftp_server_check::MISSING_REPLY;s.queue_traced_tftp_response(1,&a,r,body,false).unwrap();let first=full_data(&mut s,&mut w);assert_eq!(s.queue_traced_tftp_response(1,&b,r,body,false),Err(qcom_glink_qmi::Error::FlowBusy));assert_eq!(s.efs_overlap_trace().flags&8,0);assert_eq!(s.efs_overlap_trace().admitted,0);s.ingest(1,&control(14,2,first)).unwrap();s.queue_traced_tftp_response(1,&b,r,body,false).unwrap();assert_eq!(s.efs_overlap_trace().admitted,0);
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_core_third_request_and_changed_reply_keep_serialization(){
 let(mut s,mut w)=efs_overlap_core();s.enable_efs_reply_overlap();let req=b"\0\x01/readwrite/modem_efs\0octet\0";let(a,b,r)=efs_overlap_handles(&mut s,&mut w,req);let c=receive(&mut s,&mut w,0x21,0x5000,req);s.finish_tftp_request(&c).unwrap();drain(&mut s,&mut w);let body=qcom_tftp_server_check::MISSING_REPLY;s.queue_traced_tftp_response(1,&a,r,body,false).unwrap();let first=full_data(&mut s,&mut w);assert_eq!(s.queue_traced_tftp_response(1,&b,r,b"\0\x05\0\x02denied\0",false),Err(qcom_glink_qmi::Error::FlowBusy));s.queue_traced_tftp_response(1,&b,r,body,false).unwrap();let second=full_data(&mut s,&mut w);assert_eq!(s.queue_traced_tftp_response(1,&c,r,body,false),Err(qcom_glink_qmi::Error::FlowBusy));assert_eq!(s.efs_overlap_trace().admitted,1);s.ingest(1,&control(14,2,first)).unwrap();assert_eq!(s.queue_traced_tftp_response(1,&c,r,body,false),Err(qcom_glink_qmi::Error::FlowBusy));s.ingest(1,&control(14,2,second)).unwrap();s.queue_traced_tftp_response(1,&c,r,body,false).unwrap();assert_eq!(s.efs_overlap_trace().admitted,1);
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_core_normal_slot_capacity_is_not_bypassed(){
 let(mut s,mut w)=efs_overlap_core();s.enable_efs_reply_overlap();let(a,b,r)=efs_overlap_handles(&mut s,&mut w,b"\0\x01/readwrite/modem_efs\0octet\0");let body=qcom_tftp_server_check::MISSING_REPLY;s.queue_traced_tftp_response(1,&a,r,body,false).unwrap();let first=full_data(&mut s,&mut w);for peer in 0x40..0x47{s.queue_tftp_response(1,qcom_glink_qmi::TftpReplyRoute{destination:qcom_qmi_router::Address{node:0,port:peer},..r},body).unwrap();}assert_eq!(s.queue_traced_tftp_response(1,&b,r,body,false),Err(qcom_glink_qmi::Error::NoCapacity));assert_eq!(s.efs_overlap_trace().admitted,0);let freed=full_data(&mut s,&mut w);s.ingest(1,&control(14,2,freed)).unwrap();s.queue_traced_tftp_response(1,&b,r,body,false).unwrap();assert_eq!(s.efs_overlap_trace().admitted,1);s.ingest(1,&control(14,2,first)).unwrap();
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_core_slot_reuse_does_not_rebind_original_token(){
 let(mut s,mut w)=efs_overlap_core();s.enable_efs_reply_overlap();let(a,b,r)=efs_overlap_handles(&mut s,&mut w,b"\0\x01/readwrite/modem_efs\0octet\0");let body=qcom_tftp_server_check::MISSING_REPLY;s.queue_traced_tftp_response(1,&a,r,body,false).unwrap();let first=full_data(&mut s,&mut w);s.ingest(1,&control(14,2,first)).unwrap();s.queue_tftp_response(1,r,body).unwrap();let reused=full_data(&mut s,&mut w);assert_eq!(s.queue_traced_tftp_response(1,&b,r,body,false),Err(qcom_glink_qmi::Error::FlowBusy));assert_eq!(s.efs_overlap_trace().offers,0);assert_eq!(s.efs_overlap_trace().admitted,0);s.ingest(1,&control(14,2,reused)).unwrap();s.queue_traced_tftp_response(1,&b,r,body,false).unwrap();assert_eq!(s.efs_overlap_trace().admitted,0);
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_owner_close_preserves_both_published_buffers_until_real_receipts(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_reply_overlap();let mut server=qcom_tftp_server_check::ServerCheck::new();let req=b"\0\x01/readwrite/modem_efs\0octet\0";mc_input(&p,&mut o,&mut i,0x21,0x5000,req);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let first=p.take_tx();mc_input(&p,&mut o,&mut i,0x21,0x5000,req);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let second=p.take_tx();mc_close(&p,&mut o,&mut i,0x21);assert_eq!(o.efs_overlap_trace().flags&32,32);assert_eq!((o.startup_prerequisites().reads[2].unwrap().flags&32,o.startup_prerequisites().reads[3].unwrap().flags&32),(0,0));acknowledge_outgoing_glink(&p,&mut o,&mut i,&second);acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);assert_eq!((o.startup_prerequisites().reads[2].unwrap().flags,o.startup_prerequisites().reads[3].unwrap().flags),(191,191));assert!(o.error().is_none());
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_core_normal_qrtr_high_watermark_still_requires_real_resume(){
 let(mut s,mut w)=efs_overlap_core();let r=qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5003},destination:qcom_qmi_router::Address{node:0,port:0x21}};let body=qcom_tftp_server_check::MISSING_REPLY;
 for _ in 0..9{s.queue_tftp_response(1,r,body).unwrap();let id=full_data(&mut s,&mut w);s.ingest(1,&control(14,2,id)).unwrap();}s.enable_efs_reply_overlap();let(a,b,r)=efs_overlap_handles(&mut s,&mut w,b"\0\x01/readwrite/modem_efs\0octet\0");s.queue_traced_tftp_response(1,&a,r,body,false).unwrap();let first=full_data(&mut s,&mut w);assert_eq!(s.queue_traced_tftp_response(1,&b,r,body,false),Err(qcom_glink_qmi::Error::FlowBusy));assert!(s.efs_overlap_trace().offers>0);assert_eq!(s.efs_overlap_trace().admitted,0);
 let id=offer(&mut s,&mut w);let mut resume=Vec::new();for n in [7u32,0,0x21,0,0]{resume.extend(n.to_le_bytes());}assert_eq!(s.ingest(1,&data(id,&owner_qrtr_packet(7,0x21,0x5003,false,&resume))).unwrap(),Event::CreditReceived);drain(&mut s,&mut w);s.queue_traced_tftp_response(1,&b,r,body,false).unwrap();assert_eq!(s.efs_overlap_trace().admitted,1);let second=full_data(&mut s,&mut w);s.ingest(1,&control(14,2,second)).unwrap();s.ingest(1,&control(14,2,first)).unwrap();
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_owner_uncertain_head_store_stops_with_no_fake_completion(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_reply_overlap();let mut server=qcom_tftp_server_check::ServerCheck::new();let req=b"\0\x01/readwrite/modem_efs\0octet\0";mc_input(&p,&mut o,&mut i,0x21,0x5000,req);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();p.take_tx();mc_input(&p,&mut o,&mut i,0x21,0x5000,req);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();assert_eq!(o.efs_overlap_trace().admitted,1);p.0.borrow_mut().fail_after_store=true;assert!(o.notified(8,&mut i).is_err());assert!(o.error().is_some());assert_eq!((o.startup_prerequisites().reads[2].unwrap().flags&32,o.startup_prerequisites().reads[3].unwrap().flags&32),(0,0));assert!(o.notified(8,&mut i).is_err());assert_eq!(o.efs_overlap_trace().admitted,1);
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_owner_known_close_does_not_admit_an_unsent_second_reply(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_reply_overlap();let mut server=qcom_tftp_server_check::ServerCheck::new();let req=b"\0\x01/readwrite/modem_efs\0octet\0";mc_input(&p,&mut o,&mut i,0x21,0x5000,req);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();p.take_tx();mc_input(&p,&mut o,&mut i,0x21,0x5000,req);mc_close(&p,&mut o,&mut i,0x21);let _=o.service_tftp_server_check_step(&mut server,&mut i);assert_eq!(o.efs_overlap_trace().flags&32,32);assert_eq!(o.efs_overlap_trace().admitted,0);assert!(o.error().is_none());
}
#[cfg(feature="sunfish-efs-reply-overlap")]
#[test]
fn efs_overlap_core_logical_reset_clears_pair_authority(){
 let(mut s,mut w)=efs_overlap_core();s.enable_efs_reply_overlap();let(a,b,r)=efs_overlap_handles(&mut s,&mut w,b"\0\x01/readwrite/modem_efs\0octet\0");let body=qcom_tftp_server_check::MISSING_REPLY;s.queue_traced_tftp_response(1,&a,r,body,false).unwrap();let first=full_data(&mut s,&mut w);s.queue_traced_tftp_response(1,&b,r,body,false).unwrap();let second=full_data(&mut s,&mut w);s.ingest(1,&control(14,2,second)).unwrap();s.ingest(1,&control(14,2,first)).unwrap();drain(&mut s,&mut w);s.reset_after_transport_reset(2).unwrap();let t=s.efs_overlap_trace();assert_eq!((t.flags,t.offers,t.admitted),(0,0,0));assert!(s.queue_traced_tftp_response(1,&b,r,body,false).is_err());assert_eq!(s.efs_overlap_trace().offers,0);
}
