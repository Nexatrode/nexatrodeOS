// Exact-name missing-file handling and ordinary transport/close lifetime.
const DIGEST_RRQ:&[u8]=b"\0\x01/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0rsize\x000\0";
#[test]
fn digest_exact_missing_reply_is_bounded_and_retry_stable(){
 use qcom_tftp_server_check::{Address,Route,ServerCheck,Error,MISSING_REPLY};
 let r=Route{source:Address{node:0,port:0x4e},destination:Address{node:1,port:0x5000}};let mut s=ServerCheck::new();let mut out=[0;128];
 assert_eq!(DIGEST_RRQ.len(),93);assert_eq!(s.receive(r,DIGEST_RRQ,&mut[0;18]),Err(Error::OutputTooSmall));assert_eq!(s.active_count(),0);
 let first=s.receive(r,DIGEST_RRQ,&mut out).unwrap();assert_eq!((&out[..first.len],first.missing_kind,first.completed),(MISSING_REPLY,4,false));assert_eq!(s.active_count(),1);
 assert_eq!(s.receive(r,DIGEST_RRQ,&mut out).unwrap(),first);assert_eq!(s.active_count(),1);
 for b in [b"\0\x02/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig\0octet\0".as_slice(),b"\0\x01/readonly/vendor/mbn/mcfg_hw/other.dig\0octet\0",b"\0\x01/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig/extra\0octet\0",b"\0\x01/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig\0netascii\0",b"\0\x01/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig\0octet\0x\0",b"\0\x01/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig\0octet\0x\0\x01\0"]{let mut other=r;other.source.port=0x4f;assert!(s.receive(other,b,&mut out).is_err());assert_eq!(s.active_count(),1);}
 let mut bad=r;bad.source.node=2;assert_eq!(s.receive(bad,DIGEST_RRQ,&mut out),Err(Error::WrongRoute));assert_eq!(s.active_count(),1);
 assert_eq!(s.preflight_mcfg_close(r.source),Ok(Some(first.route.source.port)));s.close_mcfg_peer(r.source).unwrap();assert!(s.receive(r,DIGEST_RRQ,&mut out).is_err());
 let mut next=r;next.source.port=0x4f;let second=s.receive(next,DIGEST_RRQ,&mut out).unwrap();assert_ne!(second.route.source.port,first.route.source.port);assert_eq!(&out[..second.len],MISSING_REPLY);
}
#[test]
fn digest_owner_captures_real_dispatch_queue_and_matching_completion(){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();
 mc_input(&p,&mut o,&mut i,0x4e,0x5000,DIGEST_RRQ);assert_eq!(o.startup_prerequisites().digest_total,1);
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let r=o.startup_prerequisites().reads[4].unwrap();assert_eq!((r.kind,r.flags,r.result,r.opcode,r.number,r.reply_bytes),(3,15,0,5,1,19));assert!(r.record_sequence().is_some());
 o.notified(8,&mut i).unwrap();let wire=p.take_tx();assert_eq!(o.startup_prerequisites().reads[4].unwrap().flags,31);acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[4].unwrap().flags,63);
 // Same-peer retry has a new opaque receive/response identity and a stable transfer port.
 mc_input(&p,&mut o,&mut i,0x4e,0x5000,DIGEST_RRQ);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let b=o.startup_prerequisites().reads[5].unwrap();assert_ne!(r.record_sequence(),b.record_sequence());assert_eq!(r.local_port,b.local_port);
 o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[5].unwrap().flags,63);
 mc_input(&p,&mut o,&mut i,0x4f,0x5000,DIGEST_RRQ);assert_eq!((o.startup_prerequisites().digest_total,o.startup_prerequisites().digest_omitted),(3,1));o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);
 mc_close(&p,&mut o,&mut i,0x4e);o.resolve_mcfg_close(&mut server).unwrap();assert_eq!(o.startup_prerequisites().reads[4].unwrap().flags,191);assert_eq!(o.startup_prerequisites().reads[5].unwrap().flags,191);assert!(o.error().is_none());
}
#[test]
fn digest_close_cancels_unsent_without_lending_completion_to_reused_slot(){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();
 mc_input(&p,&mut o,&mut i,0x4e,0x5000,DIGEST_RRQ);let close=mc_offer(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();p.push(&data(close,&owner_control_frame(6,[0,0x4e,0,0])));o.notified(1,&mut i).unwrap();o.resolve_mcfg_close(&mut server).unwrap();assert_eq!(o.mcfg_close_trace().unsent,1);
 let original=o.startup_prerequisites().reads[4].unwrap();assert_eq!(original.flags,143);assert!(original.published.is_none()&&original.done.is_none());
 mc_input(&p,&mut o,&mut i,0x4f,0x5000,DIGEST_RRQ);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);
 let original=o.startup_prerequisites().reads[4].unwrap();assert_eq!(original.flags,143);assert!(original.published.is_none()&&original.done.is_none());assert_eq!(o.startup_prerequisites().reads[5].unwrap().flags,63);
}
#[test]
fn digest_blocked_retry_is_identical_and_uncertainty_never_completes(){
 for uncertain in [false,true]{
 let mut s=session();let mut wire=Vec::new();s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();let mut adv=control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}s.ingest(1,&adv).unwrap();drain(&mut s,&mut wire);let id=offer(&mut s,&mut wire);s.ingest(1,&data(id,&owner_control_frame(2,[0;4]))).unwrap();drain(&mut s,&mut wire);
 let h=receive(&mut s,&mut wire,0x4e,0x5000,DIGEST_RRQ);let mut server=qcom_tftp_server_check::ServerCheck::new();let mut out=[0;128];let reply=server.receive(qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x4e},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}},DIGEST_RRQ,&mut out).unwrap();s.observe_startup_dispatch(&h,Ok(reply),&out);s.finish_tftp_request(&h).unwrap();drain(&mut s,&mut wire);
 let route=qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:reply.route.source.port},destination:qcom_qmi_router::Address{node:0,port:0x4e}};s.queue_traced_tftp_response(1,&h,route,&out[..reply.len],false).unwrap();let seq=s.startup_prerequisites().reads[4].unwrap().record_sequence();
 let mut blocked=Vec::new();for _ in 0..32{let r=s.publish_next(1,|b|{if u16::from_le_bytes([b[0],b[1]])==9{blocked=b.to_vec();Err(qcom_glink_qmi::WriteError::WouldBlock)}else{Ok(())}});if r==Err(qcom_glink_qmi::Error::WouldBlock){break;}assert!(r.unwrap().is_some());}assert!(!blocked.is_empty());assert_eq!(s.startup_prerequisites().reads[4].unwrap().flags,15);
 if uncertain{assert_eq!(s.publish_next(1,|b|{assert_eq!(b,blocked);Err(qcom_glink_qmi::WriteError::Uncertain)}),Err(qcom_glink_qmi::Error::PublicationUncertain));assert!(s.startup_prerequisites().reads[4].unwrap().done.is_none());assert!(s.publish_next(1,|_|Ok(())).is_err());}
 else{let mut retry=Vec::new();let p=s.publish_next(1,|b|{retry=b.to_vec();Ok(())}).unwrap();assert_eq!(retry,blocked);let Some(Published::Data{remote_intent,last_fragment:true})=p else{panic!("digest publication");};assert_eq!(s.startup_prerequisites().reads[4].unwrap().flags,31);s.ingest(1,&control(14,2,remote_intent)).unwrap();let r=s.startup_prerequisites().reads[4].unwrap();assert_eq!((r.flags,r.record_sequence()),(63,seq));}
 }
}
#[test]
fn digest_close_keeps_published_owner_until_its_exact_receipt(){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x4e,0x5000,DIGEST_RRQ);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let wire=p.take_tx();let seq=o.startup_prerequisites().reads[4].unwrap().record_sequence();
 mc_close(&p,&mut o,&mut i,0x4e);o.resolve_mcfg_close(&mut server).unwrap();assert_eq!(o.mcfg_close_trace().published,1);assert_eq!(o.startup_prerequisites().reads[4].unwrap().flags,159);
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);let r=o.startup_prerequisites().reads[4].unwrap();assert_eq!((r.flags,r.record_sequence()),(191,seq));assert!(o.error().is_none());
}
