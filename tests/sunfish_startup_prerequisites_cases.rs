use super::*;
fn setup()->Session{Session::new(Config{local_node:1,peer_node:0,local_channel:1,remote_channel:2},1).unwrap()}
fn seed(s:&mut Session,index:usize,intent:u32,path:&[u8],peer:u32)->TftpHandle{
 let mut body=b"\0\x01".to_vec();body.extend(path);body.extend(b"\0octet\0rsize\x000\0");let r=&mut s.receive[index];r.bytes[..32].fill(0);for(at,v)in[(0,1u32),(4,1),(8,0),(12,peer),(20,body.len()as u32),(24,1),(28,0x5000)]{r.bytes[at..at+4].copy_from_slice(&v.to_le_bytes());}r.bytes[32..32+body.len()].copy_from_slice(&body);r.used=32+body.len();let h=TftpHandle{owner:1,generation:1,intent};s.observe_startup_read(index,h);h
}
#[test]
fn startup_prerequisites_bounded_distinct_requests_before_elf_and_zero_rsize(){
 let mut s=setup();let paths=[b"/readwrite/ota_firewall/ruleset".as_slice(),b"/readwrite/modem_efs"];
 for(k,p)in paths.iter().enumerate(){for n in 0..5{let h=seed(&mut s,0,(k*10+n)as u32,p,0x20+k as u32);if n<2{s.observe_startup_read(0,h);}}}
 let c=s.startup_prerequisites();assert_eq!((c.total,c.omitted),([5,5],[3,3]));assert_eq!(c.reads.iter().flatten().count(),4);assert!(c.reads.iter().flatten().all(|r|r.received.is_none()&&r.options_present==16&&r.rsize==0));assert!(s.post_elf_mcfg().is_none());assert!(s.reset_after_transport_reset(2).is_ok());assert_eq!(s.startup_prerequisites().total,[0,0]);
}
#[test]
fn startup_prerequisites_exact_reservation_and_intent_reject_reused_slot(){
 let mut s=setup();seed(&mut s,0,1,b"/readwrite/modem_efs",0x21);let mut table=crate::qcom_qrtr_flow::Table::<8>::new(1).unwrap();let d=crate::qcom_qrtr_flow::Destination{node:0,port:0x21};let first=table.reserve(d).unwrap();let second=table.reserve(d).unwrap();
 let r=s.startup_prerequisites.reads[2].as_mut().unwrap();r.record=Some(first);r.slot=Some(0);r.flags=31;r.local_port=0x5001;r.reply_bytes=19;r.remote_intent=7;
 let t=&mut s.transmit[0];t.kind=TxKind::TftpReply{source:Address{node:1,port:0x5001},destination:Address{node:0,port:0x21}};t.len=51;t.reference_flow=Some(second);t.bytes[4..8].copy_from_slice(&1u32.to_le_bytes());t.bytes[20..24].copy_from_slice(&19u32.to_le_bytes());s.observe_startup_done(0,7);assert_eq!(s.startup_prerequisites.reads[2].unwrap().flags,31);
 s.transmit[0].reference_flow=Some(first);s.observe_startup_done(0,8);assert_eq!(s.startup_prerequisites.reads[2].unwrap().flags,31);s.observe_startup_done(0,7);assert_eq!(s.startup_prerequisites.reads[2].unwrap().flags,63);assert!(s.startup_prerequisites.reads[2].unwrap().done.is_none());
}
#[test]
fn startup_prerequisites_dispatch_failure_queue_waits_and_exact_close(){
 let mut s=setup();let h=seed(&mut s,0,1,b"/readwrite/modem_efs",0x21);let unrelated=TftpHandle{intent:2,..h};s.observe_startup_dispatch(&unrelated,Err(crate::qcom_tftp_server_check::Error::UnexpectedRequest),&[]);assert_eq!(s.startup_prerequisites.reads[2].unwrap().flags,1);
 s.observe_startup_dispatch(&h,Err(crate::qcom_tftp_server_check::Error::UnexpectedRequest),&[]);let r=s.startup_prerequisites.reads[2].unwrap();assert_eq!(r.flags,3);assert_ne!(r.result,0);assert_eq!(r.opcode,u32::MAX);
 s.observe_startup_close(0x22);assert_eq!(s.startup_prerequisites.reads[2].unwrap().flags,3);s.observe_startup_close(0x21);assert_eq!(s.startup_prerequisites.reads[2].unwrap().flags,131);
}
