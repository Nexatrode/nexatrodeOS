use super::*;
#[test]fn tx3_credit_return_real_fifo_preserves_three_stages_and_receipt_identity(){
 let(mut a,peer,h,id)=pending_with_confirm(true);
 let Ok(Transmit::Session(Published::Data{remote_intent:credit,last_fragment:true}))=a.publish_next(1)else{panic!("credit")};
 let b=peer.take_tx();assert_eq!(word(&b,8),52);assert_eq!(word(&b,20),7);assert!(a.registry_reply_fifo().is_none());
 assert_eq!(a.publish_next(1),Ok(Transmit::Session(Published::ReceiveDone(id))));assert_eq!(peer.take_tx(),control(6,1,id));
 let Ok(Transmit::Session(Published::Data{remote_intent:reply,last_fragment:true}))=a.publish_next(1)else{panic!("reply")};let b=peer.take_tx();assert_eq!(word(&b,8),88);assert_eq!(&b[49..51],&3u16.to_le_bytes());assert_ne!(reply,credit);
 let Ok(Transmit::Session(Published::ReceiveIntent(backing)))=a.publish_next(1)else{panic!("ad")};let ad=peer.take_tx();assert_eq!(word(&ad,0)&0xffff,5);assert_eq!(word(&ad,12),backing);
 assert_eq!(a.publish_next(1),Ok(Transmit::Session(Published::IntentRequestAck{granted:true})));assert_eq!(peer.take_tx(),control(8,1,1));
 peer.push(&control(14,2,credit));assert_ne!(a.poll_receive(1).unwrap(),Receive::Session(Event::ReplyRxDone(h)));
 peer.push(&control(14,2,reply));assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(h))));
 assert_eq!(a.registry_window_trace().predecessor.unwrap().publication.unwrap().mask&0x7e0000,0x7e0000);
}
#[test]fn tx3_credit_return_real_fifo_lost_notify_at_each_stage_quarantines(){
 for stage in 0..3{let(mut a,peer,_,_)=pending_with_confirm(true);for _ in 0..stage{a.publish_next(1).unwrap();peer.take_tx();}
 peer.0.borrow_mut().fail=Some("notify");assert!(a.publish_next(1).is_err());peer.0.borrow_mut().fail=None;
 assert_eq!(a.publish_next(1),Err(Error::Quarantined));assert!(a.registry_reply_fifo().is_none());}
}
