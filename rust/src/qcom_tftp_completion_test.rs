//! Pure observer identity checks; no hardware fixture or protocol mutation.
use super::*;
fn fixture() -> Session {
 let mut s=Session::new(Config{local_node:1,peer_node:0,local_channel:1,remote_channel:2},77).unwrap();
 let peer=Address{node:0,port:0x1f};let request=TftpHandle{owner:s.owner,generation:77,intent:9};
 let mut write=ElfServerCheckTrace::new(request,peer,Some(10),true,true);write.transfer_port=Some(0x5001);
 s.first_server_check=Some(write);
 s.server_check_transfers[0]=Some(ServerCheckOackTrace{
  early_oack_attempted:false,intent:9,peer,transfer_port:0x5001,wrq_at:Some(10),confirm_requested:false,
  wrq_done_at:None,wrq_priority_published:false,wrq_reused:false,resume_at:None,oack_at:Some(20),oack_done_at:None,
  ready_at_oack:None,first_data_at:Some(30),first_repeat_at:None,first_close_at:None});
 s
}
#[test]fn original_completion_view_is_read_only_and_ignores_other_completed_peer(){
 let mut s=fixture();let expected=s.first_server_check;
 let mut other=s.server_check_transfers[0].unwrap();other.intent=10;other.peer.port=0x51;other.transfer_port=0x5002;other.wrq_at=Some(40);
 s.server_check_transfers[1]=Some(other);s.completed_server_check=Some(1);
 s.intent_ack_clock=Some(||panic!("observer must not read the clock"));
 assert_eq!(s.original_tftp_write_trace(),expected);assert_eq!(s.first_server_check,expected);
 assert_eq!(s.original_tftp_write_trace(),expected);
}
#[test]fn original_completion_view_rejects_every_identity_mismatch(){
 for bad in 0..6 {let mut s=fixture();let w=s.first_server_check.as_mut().unwrap();match bad{
  0=>w.peer.node=99,1=>w.peer.port=0x51,2=>w.transfer_port=Some(0x5002),3=>w.request.intent+=1,
  4=>w.request.owner+=1,_=>w.request.generation+=1};
  assert!(s.original_tftp_write_trace().is_none(),"mismatch {bad}");}
}
#[test]fn original_completion_view_does_not_invent_an_absent_original(){
 let mut s=fixture();s.server_check_transfers[0]=None;assert!(s.original_tftp_write_trace().is_none());
 let mut s=fixture();s.first_server_check=None;assert!(s.original_tftp_write_trace().is_none());
 let mut s=fixture();s.first_server_check.as_mut().unwrap().transfer_port=None;assert!(s.original_tftp_write_trace().is_none());
}

#[cfg(feature="sunfish-ack-elf-phase-trace")]
#[test]
fn original_ack_stages_exclude_other_data_handles_and_never_overwrite_start(){
 for bad in 0..3 {
  let mut s=fixture();let request=TftpHandle{owner:s.owner,generation:77,intent:17};let mut other=request;
  match bad{0=>other.owner+=1,1=>other.generation+=1,_=>other.intent+=1}
  s.first_server_check.as_mut().unwrap().data=Some(ElfWriteData{request:other,block:1,body_bytes:9,
   received_at:Some(30),result:None,stages:FinalAckStages::default()});
  s.intent_ack_clock=Some(||panic!("unrelated handle must not create an observation"));
  s.observe_elf_server_check_dispatch_start(&request);
  assert_eq!(s.first_server_check.unwrap().data.unwrap().stages,FinalAckStages::default());
 }
 let mut s=fixture();let request=TftpHandle{owner:s.owner,generation:77,intent:17};
 s.first_server_check.as_mut().unwrap().data=Some(ElfWriteData{request,block:1,body_bytes:9,
  received_at:Some(30),result:None,stages:FinalAckStages::default()});
 s.intent_ack_clock=Some(||41);s.observe_elf_server_check_dispatch_start(&request);
 s.intent_ack_clock=Some(||panic!("first start must not be overwritten"));s.observe_elf_server_check_dispatch_start(&request);
 assert_eq!(s.first_server_check.unwrap().data.unwrap().stages.dispatch_at,Some(41));
}

#[cfg(feature="sunfish-ack-elf-phase-trace")]
#[test]
fn original_ack_stages_record_return_of_reused_wire_intent(){
 let mut s=fixture();let request=TftpHandle{owner:s.owner,generation:77,intent:17};
 s.first_server_check.as_mut().unwrap().data=Some(ElfWriteData{request,block:1,body_bytes:9,
  received_at:Some(30),result:Some(0),stages:FinalAckStages::default()});
 s.receive[0].id=9;s.receive[0].state=RxState::Control;s.receive[0].done_pending=true;
 s.receive[0].reused_datagram_id=Some(17);
 s.intent_ack_clock=Some(||55);
 let mut offered=std::vec::Vec::new();
 assert_eq!(s.publish_next(77, |bytes|{offered.extend_from_slice(bytes);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
 assert_eq!(offered.len(),8);assert_eq!(u16::from_le_bytes([offered[0],offered[1]]),6);
 assert_eq!(u32::from_le_bytes(offered[4..8].try_into().unwrap()),9);
 assert_eq!(s.first_server_check.unwrap().data.unwrap().stages.returned_at,None);
 assert_eq!(s.publish_next(77, |bytes|{assert_eq!(bytes,&offered);Ok(())}),Ok(Some(Published::ReceiveDone(9))));
 assert_eq!(s.first_server_check.unwrap().data.unwrap().stages.returned_at,Some(55),
  "wire ID9 returns the opaque DATA17 held on the reused WRQ buffer");
}

#[cfg(feature="sunfish-ack-elf-phase-trace")]
#[test]
fn original_ack_stages_queue_requires_accepted_exact_data_even_without_clock(){
 for case in 0..8 {
  let mut s=fixture();let request=TftpHandle{owner:s.owner,generation:77,intent:17};
  s.control.prepare_tftp_available(0x5000).unwrap();s.control.start().unwrap();
  let mut hello=s.control.encode(control::Outbound::Hello);put(&mut hello,8,0);put(&mut hello,24,1);
  s.control.receive(&hello).unwrap();s.control.published(control::Outbound::Hello);
  s.control.published(control::Outbound::NewTftpServer);assert!(s.control.accepts_tftp());
  s.first_server_check.as_mut().unwrap().data=Some(ElfWriteData{request,block:1,body_bytes:9,
   received_at:Some(30),result:Some(0),stages:FinalAckStages::default()});
  let route=TftpReplyRoute{source:Address{node:1,port:0x5001},destination:Address{node:0,port:0x1f}};
  let reply=crate::qcom_tftp_server_check::Reply{route:crate::qcom_tftp_server_check::Route{
   source:crate::qcom_tftp_server_check::Address{node:1,port:0x5001},
   destination:crate::qcom_tftp_server_check::Address{node:0,port:0x1f}},len:4,completed:true,missing_kind:0};
  let ack=[0,4,0,1];
  s.observe_elf_server_check_dispatch(&request,route,false,Ok(reply),&ack);
  assert!(s.first_server_check.unwrap().data.unwrap().stages.final_prepared);
  assert_eq!(s.first_server_check.unwrap().data.unwrap().stages.prepared_at,None,"no clock");
  let mut queued_request=request;let mut queued_route=route;let mut packet=ack;
  match case{1=>queued_request.owner+=1,2=>queued_request.generation+=1,3=>queued_request.intent+=1,
   4=>queued_route.source.port+=1,5=>packet[3]=2,
   6=>{let d=s.first_server_check.as_mut().unwrap().data.as_mut().unwrap();d.result=Some(6);d.stages=FinalAckStages::default();},_=>()}
  if case==7{s.queue_tftp_response(77,route,&ack).unwrap();}
  else{s.queue_traced_tftp_response(77,&queued_request,queued_route,&packet,false).unwrap();}
  let stages=s.first_server_check.unwrap().data.unwrap().stages;
  assert_eq!(stages.final_queued,case==0,"case{case}");
  assert_eq!(stages.queued_at,None,"an absent clock must not invent a time");
 }
}
