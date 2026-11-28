use super::*;
fn word(b:&[u8],i:usize)->u32{u32::from_le_bytes(b[i..i+4].try_into().unwrap())}
fn pending()->(Adapter<Fake>,Fake,RequestHandle,u32) {pending_with_confirm(false)}
fn pending_with_confirm(confirm:bool)->(Adapter<Fake>,Fake,RequestHandle,u32) {
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
 let peer=Fake::new(0x3ff8,0x3ff8);let mut a=Adapter::attach(peer.clone(),s,1).unwrap();
 let mut remote=control(5,2,2);for id in [7u32,8]{remote.extend(4096u32.to_le_bytes());remote.extend(id.to_le_bytes());}peer.push(&remote);a.poll_receive(1).unwrap();
 let id=advertised(&mut a,&peer);let name=b"avs/audio";let mut q=vec![0,3,0,0x21,0,12,0,1,9,0];q.extend(name);
 let mut p=qrtr(1,&q);p[16..20].copy_from_slice(&u32::from(confirm).to_le_bytes());p[28..32].copy_from_slice(&qcom_servreg::LOCAL_PORT.to_le_bytes());peer.push(&data(id,&p));
 let Ok(Receive::Session(Event::Request(h)))=a.poll_receive(1)else{panic!("TX3 request")};
 let response=qcom_servreg::respond(qcom_servreg::Query{service_name:name,domain_offset:0});a.queue_reply(1,&h,Reply::Success{optional_tlvs:response.optional_tlvs()}).unwrap();
 peer.push(&control(7,2,108));assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReceiveIntentRequest{granted:true})));(a,peer,h,id)
}
#[test]fn tx3_own_return_real_fifo_preserves_exact_88_byte_reply_and_grant_order(){
 let(mut a,peer,h,id)=pending();
 assert_eq!(a.publish_next(1),Ok(Transmit::Session(Published::ReceiveDone(id))));assert_eq!(peer.take_tx(),control(6,1,id));
 let Ok(Transmit::Session(Published::Data{remote_intent,last_fragment:true}))=a.publish_next(1)else{panic!("ready reply")};let b=peer.take_tx();assert_eq!(word(&b,0)&0xffff,9);assert_eq!(word(&b,8),88);assert_eq!(&b[49..51],&3u16.to_le_bytes());
 let Ok(Transmit::Session(Published::ReceiveIntent(backing)))=a.publish_next(1)else{panic!("grant advertisement")};let ad=peer.take_tx();assert_eq!(word(&ad,0)&0xffff,5);assert_eq!(word(&ad,12),backing);
 assert_eq!(a.publish_next(1),Ok(Transmit::Session(Published::IntentRequestAck{granted:true})));assert_eq!(peer.take_tx(),control(8,1,1));
 peer.push(&control(14,2,remote_intent));assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(h))));
 let p=a.registry_window_trace().predecessor.unwrap().publication.unwrap();assert_eq!(p.mask&0x780000,0x780000);
}
#[test]fn tx3_own_return_real_fifo_uncertain_notify_quarantines_without_reply_publication(){
 let(mut a,peer,_,_)=pending();peer.0.borrow_mut().fail=Some("notify");
 assert!(a.publish_next(1).is_err());peer.0.borrow_mut().fail=None;
 assert_eq!(a.publish_next(1),Err(Error::Quarantined));assert!(a.registry_reply_fifo().is_none());
}

#[cfg(feature="sunfish-tx3-credit-return-turn")]
#[path="sunfish_tx3_credit_return_fifo_cases.rs"]
mod credit_return_fifo_cases;
