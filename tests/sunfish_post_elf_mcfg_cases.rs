// Synthetic lifecycle schedules, not reconstructed handset packets.
mod post_mcfg_cases {
use super::*;
const STAT:&[u8]=b"\0\x01/readwrite/mcfg.tmp\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0tsize\x000\0";
const END:&[u8]=b"\0\x05\0\x09End of Transfer\0";
fn commit_one(s:&mut qcom_tftp_server_check::ServerCheck,out:&mut[u8]) {
 use qcom_tftp_server_check::{Address,Route};
 let route=|port|Route{source:Address{node:0,port:0x56},destination:Address{node:1,port}};
 let w=b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0";
 let port=s.receive(route(0x5000),w,out).unwrap().route.source.port;
 s.receive(route(port),b"\0\x03\0\x01X",out).unwrap();
 assert_eq!(s.mcfg_progress(),(1,Some(1),0));
}
fn publish(s:&mut Session,wire:&mut Vec<u8>)->Option<Published>{s.publish_next(1,|b|{wire.extend_from_slice(b);Ok(())}).unwrap()}
fn offer(s:&mut Session,wire:&mut Vec<u8>)->u32{let id=s.offer_receive(1).unwrap();assert_eq!(publish(s,wire),Some(Published::ReceiveIntent(id)));id}
fn drain(s:&mut Session,wire:&mut Vec<u8>){for _ in 0..64 {match publish(s,wire){None=>return,Some(Published::Data{remote_intent,last_fragment:true})=>{s.ingest(1,&control(14,2,remote_intent)).unwrap();},_=>{}}}panic!("drain bound");}
fn full_data(s:&mut Session,wire:&mut Vec<u8>)->u32 {for _ in 0..16 {if let Some(Published::Data{remote_intent,last_fragment:true})=publish(s,wire){return remote_intent;}}panic!("missing full data");}
fn receive(s:&mut Session,wire:&mut Vec<u8>,peer:u32,port:u32,payload:&[u8])->qcom_glink_qmi::TftpHandle {
 let id=offer(s,wire);let Event::TftpRequest(h)=s.ingest(1,&data(id,&owner_qrtr_packet(1,peer,port,false,payload))).unwrap()else{panic!("TFTP");};h
}
#[test]
fn post_mcfg_protocol_identity_blocked_write_reuse_and_clockless() {
 use qcom_glink_qmi::TftpReplyRoute;
 let mut s=session();let mut wire=Vec::new();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();
 let mut adv=control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}s.ingest(1,&adv).unwrap();
 drain(&mut s,&mut wire);let id=offer(&mut s,&mut wire);s.ingest(1,&data(id,&owner_control_frame(2,[0;4]))).unwrap();drain(&mut s,&mut wire);
 let id=offer(&mut s,&mut wire);let Event::Request(elf)=s.ingest(1,&data(id,&servreg_query_datagram_with_transaction(b"kernel/elf_loader",5))).unwrap()else{panic!("ELF");};
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]assert!(s.post_elf_mcfg().unwrap().mcfg.is_none());
 let response=qcom_servreg::respond(qcom_servreg::Query{service_name:b"kernel/elf_loader",domain_offset:0});s.queue_reply(&elf,Reply::Success{optional_tlvs:response.optional_tlvs()}).unwrap();s.observe_elf_reply(&elf);
 let elf_id=full_data(&mut s,&mut wire);
 // A request accepted before matching ELF completion cannot become the target.
 let early=receive(&mut s,&mut wire,0x48,0x5000,STAT);s.finish_tftp_request(&early).unwrap();
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]assert!(s.post_elf_mcfg().unwrap().mcfg.is_none());
 assert_eq!(s.ingest(1,&control(14,2,elf_id)).unwrap(),Event::ReplyRxDone(elf));drain(&mut s,&mut wire);
 let h=receive(&mut s,&mut wire,0x4a,0x5000,STAT);s.finish_tftp_request(&h).unwrap();drain(&mut s,&mut wire);
 let duplicate=receive(&mut s,&mut wire,0x4a,0x5000,STAT);assert_ne!(h,duplicate);s.finish_tftp_request(&duplicate).unwrap();drain(&mut s,&mut wire);
 let mut server=qcom_tftp_server_check::ServerCheck::new();let mut out=[0;128];commit_one(&mut server,&mut out);
 let r=server.receive(qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x4a},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}},STAT,&mut out).unwrap();
 let route=TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:r.route.source.port},destination:qcom_qmi_router::Address{node:0,port:0x4a}};
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]{s.observe_post_mcfg_dispatch(&duplicate,Some(1),Ok(r),&out);assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,1);s.observe_post_mcfg_dispatch(&h,Some(1),Ok(r),&out);}
 s.queue_traced_tftp_response(1,&duplicate,route,&out[..r.len],false).unwrap();let other=full_data(&mut s,&mut wire);
 assert_eq!(s.queue_traced_tftp_response(1,&h,route,&out[..r.len],false),Err(qcom_glink_qmi::Error::FlowBusy));
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]{let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!((m.flags,m.queue_waits),(7,[1,0]));}
 s.ingest(1,&control(14,2,other)).unwrap();
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,7);
 s.queue_traced_tftp_response(1,&h,route,&out[..r.len],false).unwrap();
 assert_eq!(s.publish_next(1,|_|Err(qcom_glink_qmi::WriteError::WouldBlock)),Err(qcom_glink_qmi::Error::WouldBlock));
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,15);
 let target=full_data(&mut s,&mut wire);
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]{let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!((m.flags,m.remote_intent,m.published),(31,target,None));assert_eq!(m.options.values[..4],[7680,1000,10,0]);assert_eq!(m.reply_options.values[..4],[512,1000,1,1]);}
 s.ingest(1,&control(14,2,target)).unwrap();
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().flags,63);
 // Reusing the normal transmit slot cannot replace the original completion.
 s.queue_tftp_response(1,TftpReplyRoute{destination:qcom_qmi_router::Address{node:0,port:0x4b},..route},b"\0\x04\0\x01").unwrap();drain(&mut s,&mut wire);
 for peer in [0x4b,0x4a]{let end=receive(&mut s,&mut wire,peer,route.source.port,END);s.finish_tftp_request(&end).unwrap();drain(&mut s,&mut wire);
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]assert_eq!(s.post_elf_mcfg().unwrap().mcfg.unwrap().client_counts,[0,u32::from(peer==0x4a),1]);}
 for peer in [0x4b,0x4a]{let id=offer(&mut s,&mut wire);s.ingest(1,&data(id,&owner_control_frame(6,[0,peer,0,0]))).unwrap();drain(&mut s,&mut wire);}
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]{let m=s.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!((m.flags,m.first_client,m.close_port,m.done),(255,Some(0x50009),0x4a,None));}
 assert!(s.reset_after_transport_reset(2).is_ok());
 #[cfg(feature="sunfish-post-elf-mcfg-trace")]assert!(s.post_elf_mcfg().is_none());
 print!("POSTMCFG_PROTOCOL_BYTES ");for b in wire{print!("{b:02x}");}println!();
}
#[cfg(feature="sunfish-post-elf-mcfg-trace")]
#[test]
fn post_mcfg_owner_automatic_dispatch_uses_actual_server_result(){
 let(peer,mut o)=owner();let mut irq=Irqs::default();o.prepare_tftp_server(0x5000).unwrap();o.prepare_service_registry().unwrap();o.start_control().unwrap();
 let mut adv=control(5,2,16);for id in 0u32..16{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}peer.push(&adv);o.notified(8,&mut irq).unwrap();peer.take_tx();owner_control_peer_hello(&peer,&mut o,&mut irq);take_owner_datagrams(&peer,&mut o,&mut irq);
 let id=o.offer_receive().unwrap();o.notified(8,&mut irq).unwrap();peer.take_tx();peer.push(&data(id,&servreg_query_datagram_with_transaction(b"kernel/elf_loader",5)));o.notified(1,&mut irq).unwrap();o.dispatch_service_registry_step(&mut irq).unwrap();o.notified(8,&mut irq).unwrap();take_owner_datagrams(&peer,&mut o,&mut irq);
 assert!(o.service_registry_trace().elf_exchange.unwrap().completed);
 let mut server=qcom_tftp_server_check::ServerCheck::new();let mut out=[0;128];commit_one(&mut server,&mut out);
 let id=o.offer_receive().unwrap();o.notified(8,&mut irq).unwrap();peer.take_tx();peer.push(&data(id,&owner_qrtr_packet(1,0x4a,0x5000,false,STAT)));o.notified(1,&mut irq).unwrap();
 o.service_tftp_server_check_step(&mut server,&mut irq).unwrap();o.notified(8,&mut irq).unwrap();take_owner_datagrams(&peer,&mut o,&mut irq);
 let m=o.post_elf_mcfg().unwrap().mcfg.unwrap();assert_eq!((m.file_before,m.server_result,m.reply_opcode,m.reply_bytes,m.flags&63),(Some(1),Some(0),6,45,63));
 let before=peer.0.borrow().log.len();let _=o.post_elf_mcfg();assert_eq!(peer.0.borrow().log.len(),before);
 let id=o.offer_receive().unwrap();o.notified(8,&mut irq).unwrap();peer.take_tx();peer.push(&data(id,&owner_qrtr_packet(1,0x4a,m.transfer_port,false,END)));o.notified(1,&mut irq).unwrap();o.service_tftp_server_check_step(&mut server,&mut irq).unwrap();
 assert_eq!(o.post_elf_mcfg().unwrap().mcfg.unwrap().first_client,Some(0x50009));assert_eq!(server.mcfg_progress(),(1,Some(1),0));
}
#[cfg(feature="sunfish-mcfg-close-cleanup")]
include!("sunfish_mcfg_close_cases.rs");


#[cfg(feature="sunfish-mcfg-missing-close")]
include!("sunfish_mcfg_missing_close_cases.rs");
}
