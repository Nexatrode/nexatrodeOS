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
