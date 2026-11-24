use super::*;
use qcom_qmi_router::Address;
const N:usize=qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS;
fn pending(rrq:bool)->(Fake,Adapter<Fake>,qcom_glink_qmi::TftpHandle){
 let (peer,mut owner)=owner();owner.prepare_tftp_server(0x5000).unwrap();owner.start_control().unwrap();
 let mut intents=control(5,2,8);for id in 0u32..8{intents.extend(4096u32.to_le_bytes());intents.extend(id.to_le_bytes());}
 let mut irqs=Irqs::default();peer.push(&intents);owner.notified(8,&mut irqs).unwrap();peer.take_tx();
 owner_control_peer_hello(&peer,&mut owner,&mut irqs);take_owner_datagrams(&peer,&mut owner,&mut irqs);
 let intent=owner.offer_receive().unwrap();owner.notified(8,&mut irqs).unwrap();peer.take_tx();
 let packet=if rrq{tftp_rrq(b"/readonly/firmware/image/wlanmdsp.mbn",&[])}else{server_check_wrq_for_backpressure()};
 peer.push(&data(intent,&tftp_qrtr(&packet,0x70,0x5000)));owner.notified(1,&mut irqs).unwrap();
 let handle=owner.pending_tftp_request().unwrap();let a=owner.into_spare_test_adapter();peer.clear();(peer,a,handle)
}
fn retained()->[(Option<Address>,u8,bool);N]{
 let mut r=[(None,0,false);N];r[0]=(Some(Address{node:0,port:0x71}),2,false);
 r[1]=(Some(Address{node:0,port:0x72}),4,true);r[2]=(Some(Address{node:0,port:0x73}),8,true);r
}
fn samples(p:&Fake)->usize{p.log().iter().filter(|op|matches!(op,Op::Snapshot)).count()}
#[test]fn metadata_batch_exact_order_one_fresh_gate_no_writes(){
 for rrq in [false,true]{let(p,mut a,h)=pending(rrq);
 let out=a.refresh_tftp_peer_snapshot(1,&[Some(h)],&retained()).unwrap();
 assert_eq!(&out[..4],&[(Some(Address{node:0,port:0x70}),1),(Some(Address{node:0,port:0x71}),2),(Some(Address{node:0,port:0x72}),4),(Some(Address{node:0,port:0x73}),8)]);
 let (peers,firmware)=a.test_peer_metadata();assert_eq!(peers,out.map(|x|x.0));
 let expected=if rrq{vec![Some(Address{node:0,port:0x70}),Some(Address{node:0,port:0x72}),Some(Address{node:0,port:0x73})]}else{vec![Some(Address{node:0,port:0x72}),Some(Address{node:0,port:0x73})]};
 assert_eq!(&firmware[..expected.len()],expected.as_slice());assert!(firmware[expected.len()..].iter().all(Option::is_none));
 assert_eq!(samples(&p),1);assert!(writes(&p.log()).is_empty());
 a.refresh_tftp_peer_snapshot(1,&[Some(h)],&retained()).unwrap();assert_eq!(samples(&p),2,"fresh gate on every update");}
}
#[test]fn metadata_batch_invalid_handles_and_capacity_are_atomic(){
 let(p,mut a,h)=pending(true);a.refresh_tftp_peer_snapshot(1,&[Some(h)],&retained()).unwrap();let before=a.test_peer_metadata();
 p.clear();assert!(a.refresh_tftp_peer_snapshot(2,&[Some(h)],&retained()).is_err());assert_eq!(samples(&p),0);assert_eq!(a.test_peer_metadata(),before);
 let(_,_,foreign)=pending(true);assert!(a.refresh_tftp_peer_snapshot(1,&[Some(foreign)],&retained()).is_err());assert_eq!(a.test_peer_metadata(),before);
 let full=[(Some(Address{node:0,port:0x79}),4,true);N];assert!(a.refresh_tftp_peer_snapshot(1,&[Some(h)],&full).is_err());assert_eq!(a.test_peer_metadata(),before);
 a.finish_tftp_request(1,&h).unwrap();assert!(a.refresh_tftp_peer_snapshot(1,&[Some(h)],&retained()).is_err());assert_eq!(a.test_peer_metadata(),before);assert!(writes(&p.log()).is_empty());
}
#[test]fn metadata_batch_validation_failure_quarantines_before_commit(){
 let(p,mut a,h)=pending(true);a.refresh_tftp_peer_snapshot(1,&[Some(h)],&retained()).unwrap();let before=a.test_peer_metadata();
 p.clear();p.0.borrow_mut().fail=Some("snapshot");assert!(a.refresh_tftp_peer_snapshot(1,&[Some(h)],&[(None,0,false);N]).is_err());assert_eq!(a.test_peer_metadata(),before);
 p.0.borrow_mut().fail=None;assert!(matches!(a.revalidate(1),Err(Error::Quarantined)));assert!(writes(&p.log()).is_empty());
}
#[test]fn metadata_batch_empty_still_validates_and_clears_old_peers(){
 let(p,mut a,h)=pending(true);a.refresh_tftp_peer_snapshot(1,&[Some(h)],&retained()).unwrap();p.clear();
 let out=a.refresh_tftp_peer_snapshot(1,&[],&[(None,0,false);N]).unwrap();assert_eq!(out,[(None,0);N]);assert_eq!(a.test_peer_metadata(),([None;N],[None;N]));assert_eq!(samples(&p),1);assert!(writes(&p.log()).is_empty());
}
