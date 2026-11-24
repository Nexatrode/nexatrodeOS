//! Real Session replay for accepted NEW_SERVER receive-buffer reuse.
use super::*;
#[path = "sunfish_announcement_fixture.rs"]
mod fixture;
use fixture::ready;
fn announcement(s:&mut Session,id:u32){assert!(matches!(s.ingest(1,&data(9,id,&qrtr_control(4,[0x777,1,0,0x123]),0)),Ok(Event::QrtrControl(qcom_qrtr_control::Event::Server{..}))));}
fn returned(s:&mut Session,id:u32)->Vec<u8>{for _ in 0..32{if let Some((e,b))=publish(s){if e==Published::ReceiveDone(id){return b;}}}panic!("missing announcement return")}
#[test]
fn announcement_reuse_repeats_without_new_grants_and_acks_an_available_committed_buffer(){
    let(mut s,id,_original)=ready();
    for _ in 0..32 {announcement(&mut s,id);assert_eq!(returned(&mut s,id),vec![14,0,1,0,id as u8,0,0,0]);
        assert_eq!(s.ingest(1,&control(7,52)),Ok(Event::ReceiveIntentRequest{granted:true}));
        let mut ack=false;for _ in 0..32{let Some((e,b))=publish(&mut s)else{break};assert!(!matches!(e,Published::ReceiveIntent(_)),"already committed reusable buffer needs no fresh offer");if e==(Published::IntentRequestAck{granted:true}){assert_eq!(b,[8,0,1,0,1,0,0,0]);ack=true;break;}}
        assert!(ack);
    }
}
#[test]
fn announcement_reuse_blocked_return_stays_owned_and_retry_is_identical(){
    let(mut s,id,_original)=ready();announcement(&mut s,id);let mut blocked=Vec::new();
    for _ in 0..32 {let result=s.publish_next(1,|b|{if b[0]==14{blocked=b.to_vec();Err(WriteError::WouldBlock)}else{Ok(())}});if !blocked.is_empty(){assert_eq!(result,Err(Error::WouldBlock));break;}}
    assert!(!blocked.is_empty());assert_eq!(s.ingest(1,&data(9,id,&qrtr_control(4,[0x777,1,0,0x123]),0)),Err(Error::FragmentOrder));
    assert_eq!(returned(&mut s,id),blocked);announcement(&mut s,id);assert_eq!(returned(&mut s,id)[0],14);
}
#[test]
fn announcement_reuse_clears_on_new_request_and_preserves_registry_reply_identity(){
    let(mut s,id,_original)=ready();announcement(&mut s,id);assert_eq!(returned(&mut s,id)[0],14);
    let port=qcom_servreg::LOCAL_PORT;
    let body=[0,4,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o'];
    let Event::Request(request)=s.ingest(1,&data(9,id,&qrtr(1,3,port,false,&body),0)).unwrap()else{panic!("registry request")};
    assert_eq!(s.request(&request).unwrap().0.transaction,4);s.queue_reply(&request,Reply::Failure{error:3}).unwrap();
    let original=_original;assert_ne!(original,request);assert!(s.reply_fully_published(&original),"original transaction3 is still fully published and awaiting its own receipt");
    let mut done=false;let mut reply=false;
    for _ in 0..64 {let Some((e,b))=publish(&mut s)else{break};match e{Published::ReceiveDone(x)if x==id=>{assert_eq!(b[0],6,"registry receive is retired, never announcement-reused");done=true;},Published::Data{remote_intent:0x67,last_fragment:true}=>{let q=Message::decode(&b[48..55+usize::from(u16::from_le_bytes([b[53],b[54]]))]).unwrap();assert_eq!((q.transaction,q.message_id),(4,0x21));reply=true;},_=>{}}}
    assert!(done&&reply);assert_eq!(s.ingest(1,&control(14,0x67)),Ok(Event::ReplyRxDone(request)));assert!(s.request(&request).is_err());assert!(s.reply_fully_published(&original),"original transaction3 is still fully published and awaiting its own receipt");assert_eq!(s.ingest(1,&control(14,0x66)),Ok(Event::ReplyRxDone(original)));assert!(s.request(&original).is_err());
    assert_eq!(s.ingest(1,&data(9,id,&qrtr_control(4,[0x777,1,0,0x123]),0)),Err(Error::InvalidIntent));
}
#[test]
fn announcement_reuse_never_acknowledges_oversize_or_uncommitted_buffers_and_reset_clears(){
    let(mut s,id,_original)=ready();announcement(&mut s,id);
    assert_eq!(s.ingest(1,&control(7,4097)),Ok(Event::ReceiveIntentRequest{granted:false}));
    let mut ack=false;for _ in 0..32{if let Some((e,_))=publish(&mut s){if e==(Published::IntentRequestAck{granted:false}){ack=true;break;}}}assert!(ack);
    assert_eq!(returned(&mut s,id)[0],14);s.reset_after_transport_reset(2).unwrap();
    assert_eq!(s.ingest(1,&control(7,52)),Err(Error::StaleGeneration));
    assert_eq!(s.ingest(2,&data(9,id,&qrtr_control(4,[0x777,1,0,0x123]),0)),Err(Error::InvalidIntent));
}
#[test]
fn announcement_reuse_stops_after_original_receipt_without_invalidating_advertised_buffer(){
    let(mut s,id,_original)=ready();announcement(&mut s,id);assert_eq!(returned(&mut s,id)[0],14);
    assert!(matches!(s.ingest(1,&control(14,0x66)),Ok(Event::ReplyRxDone(_))));
    // A buffer already returned reusable is still a valid grant backing. It
    // remains available until consumed; ending the window cannot revoke it.
    assert_eq!(s.ingest(1,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:true}));
    for _ in 0..32{if publish(&mut s).is_some_and(|(e,_)|e==(Published::IntentRequestAck{granted:true})){break;}}
    announcement(&mut s,id);assert_eq!(returned(&mut s,id)[0],6,"next announcement after the window retires normally");
    assert_eq!(s.ingest(1,&data(9,id,&qrtr_control(4,[0x777,1,0,0x123]),0)),Err(Error::InvalidIntent));
}
#[test]
fn announcement_reuse_rejects_malformed_new_server_without_returning_it(){
    let(mut s,id,_original)=ready();let mut invalid=qrtr_control(4,[0x777,1,0,0x123]);put(&mut invalid,32,5);
    assert!(s.ingest(1,&data(9,id,&invalid,0)).is_err());
    let mut returned=false;for _ in 0..16{if let Some((Published::ReceiveDone(x),_))=publish(&mut s){returned|=x==id;}}
    assert!(!returned,"failed control validation cannot produce a reusable return");
}
#[test]
fn announcement_reuse_full_pool_denial_converts_only_after_committed_return(){
    for blocked in [false,true]{let(mut s,id,_original)=ready();
        while let Ok(next)=s.offer_receive(1){let mut found=false;for _ in 0..32{if let Some((e,_))=publish(&mut s){if e==Published::ReceiveIntent(next){found=true;break;}}}assert!(found);}
        announcement(&mut s,id);assert_eq!(s.ingest(1,&control(7,52)),Ok(Event::ReceiveIntentRequest{granted:false}));
        if blocked{let mut seen=false;let r=s.publish_next(1,|b|{assert_eq!((b[0],word(b,4)),(14,id));seen=true;Err(WriteError::WouldBlock)});assert!(seen);assert_eq!(r,Err(Error::WouldBlock));assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:false},"blocked one-shot must retain the negative-ACK fallback");}
        let b=returned(&mut s,id);assert_eq!(b[0],14);if blocked{assert_eq!(s.ingest(1,&control(7,52)),Ok(Event::ReceiveIntentRequest{granted:true}));}let mut ack=false;
        for _ in 0..32{if let Some((e,b))=publish(&mut s){assert_ne!(b[0],5,"existing committed offer/return supplies backing");if e==(Published::IntentRequestAck{granted:true}){ack=true;break;}}}
        assert!(ack);announcement(&mut s,id);assert_eq!(returned(&mut s,id)[0],14);
    }
}
#[test]
fn announcement_reuse_uncertain_return_quarantines_and_never_replays(){
    let(mut s,id,_original)=ready();announcement(&mut s,id);let mut attempted=false;
    let r=s.publish_next(1,|b|{assert_eq!((b[0],word(b,4)),(14,id));attempted=true;Err(WriteError::Uncertain)});
    assert!(attempted);assert_eq!(r,Err(Error::PublicationUncertain));
    assert_eq!(s.publish_next(1,|_|panic!("uncertain return cannot replay")),Err(Error::Quarantined));
    assert_eq!(s.ingest(1,&control(7,52)),Err(Error::Quarantined));
}
#[test]
fn announcement_reuse_consumed_backing_cannot_get_a_stale_positive_ack(){
    let(mut s,id,_original)=ready();announcement(&mut s,id);assert_eq!(returned(&mut s,id)[0],14);
    assert_eq!(s.ingest(1,&control(7,52)),Ok(Event::ReceiveIntentRequest{granted:true}));
    let mut saw=false;assert_eq!(s.publish_next(1,|b|{assert_eq!(b[0],8);assert_eq!(word(b,4),1);saw=true;Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));assert!(saw);
    let packet=qrtr_control(4,[0x777,1,0,0x123]);
    assert_eq!(s.ingest(1,&data(9,id,&packet[..8],packet.len()-8)),Ok(Event::Fragment));
    assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:false},"fragment consumed the previous available backing");
    assert!(matches!(s.ingest(1,&data(12,id,&packet[8..],0)),Ok(Event::QrtrControl(_))));
    assert_eq!(returned(&mut s,id)[0],14);assert_eq!(s.ingest(1,&control(7,52)),Ok(Event::ReceiveIntentRequest{granted:true}));
    assert_eq!(publish(&mut s).unwrap().0,Published::IntentRequestAck{granted:true});
}
