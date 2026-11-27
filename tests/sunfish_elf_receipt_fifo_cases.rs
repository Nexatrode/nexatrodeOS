use super::*;
fn pending(clocked:bool,capacity:u32)->(Adapter<Fake>,Fake,RequestHandle) {
    use core::sync::atomic::{AtomicU64,Ordering::Relaxed};
    static NOW:AtomicU64=AtomicU64::new(100);
    let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();
    s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
    let peer=Fake::new(0x3ff8,0x3ff8);let mut a=Adapter::attach(peer.clone(),s,1).unwrap();
    if clocked{a.set_registry_ack_clock(||NOW.fetch_add(1,Relaxed));}
    let mut remote=control(5,2,2);for id in [7u32,8]{remote.extend(capacity.to_le_bytes());remote.extend(id.to_le_bytes());}
    peer.push(&remote);a.poll_receive(1).unwrap();
    let id=advertised(&mut a,&peer);let name=b"kernel/elf_loader";
    let mut q=vec![0,77,0,0x21,0,20,0,1,17,0];q.extend(name);
    let mut packet=qrtr(1,&q);packet[28..32].copy_from_slice(&qcom_servreg::LOCAL_PORT.to_le_bytes());
    peer.push(&data(id,&packet));
    let Ok(Receive::Session(Event::Request(h)))=a.poll_receive(1)else{panic!("exact ELF request")};
    let response=qcom_servreg::respond(qcom_servreg::Query{service_name:name,domain_offset:0});
    a.queue_reply(1,&h,Reply::Success{optional_tlvs:response.optional_tlvs()}).unwrap();
    a.observe_elf_reply(&h);assert!(a.elf_reply_fifo().is_none());(a,peer,h)
}
fn sent(a:&mut Adapter<Fake>,peer:&Fake)->u32 {
    for _ in 0..16{let e=a.publish_next(1).unwrap();if let Transmit::Session(Published::Data{remote_intent,last_fragment:true})=e{return remote_intent;}peer.take_tx();}
    panic!("ELF not published")
}
#[test]fn elf_receipt_fifo_exact_wrap_clockless_prefix_and_retained_identity(){
 for clocked in [false,true]{
    let(mut a,peer,h)=pending(clocked,4096);let intent=sent(&mut a,&peer);
    let start=a.elf_reply_fifo().expect("successful publication arms");assert_eq!(start.remote_intent,intent);assert!(!start.fifo.finished);
    peer.take_tx();a.revalidate(1).unwrap();assert_eq!(a.elf_reply_fifo().unwrap().fifo.peer_tail_at.is_some(),clocked);
    peer.push(&control(13,0,0));peer.push(&control(14,2,intent));a.revalidate(1).unwrap();
    assert_eq!(a.poll_receive(1),Ok(Receive::ReadNotification));assert!(!a.elf_reply_fifo().unwrap().fifo.finished);
    assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(h))));
    let done=a.elf_reply_fifo().unwrap();assert!(done.fifo.finished);assert_eq!(done.fifo.completion.unwrap().bytes_ahead,Some(8));
    assert_eq!(done.core_done_at.is_some(),clocked);assert_eq!(done.fifo.completion.unwrap().first_visible.is_some(),clocked);
    let log=peer.log();assert_eq!(a.elf_reply_fifo(),Some(done));assert_eq!(peer.log(),log);
    // An already consumed reusable remote intent and later FIFO activity do not rebind.
    peer.push(&control(13,0,0));a.poll_receive(1).unwrap();a.revalidate(1).unwrap();assert_eq!(a.elf_reply_fifo(),Some(done));
    peer.take_tx();peer.0.borrow_mut().observation.generation=2;
    a.reset_after_transport_reset(2).unwrap();assert!(a.elf_reply_fifo().is_none());
 }
}
#[test]fn elf_receipt_fifo_no_fit_or_failed_publication_cannot_arm(){
 let(mut a,peer,_)=pending(true,16);
 for _ in 0..4{let _=a.publish_next(1);peer.take_tx();assert!(a.elf_reply_fifo().is_none());}
 let(mut a,peer,_)=pending(false,4096);
 // Return the request normally, then fail the actual reply's checked writer.
 loop{let e=a.publish_next(1).unwrap();peer.take_tx();if matches!(e,Transmit::Session(Published::ReceiveDone(_))){break;}}
 peer.0.borrow_mut().fail=Some("write_tx");
 assert!(a.publish_next(1).is_err());assert!(a.elf_reply_fifo().is_none());
}
#[test]fn elf_receipt_fifo_invalid_sample_cannot_advance_observer(){
 let(mut a,peer,_)=pending(true,4096);let _=sent(&mut a,&peer);let before=a.elf_reply_fifo();
 peer.0.borrow_mut().words[4]=1;assert!(a.revalidate(1).is_err());assert_eq!(a.elf_reply_fifo(),before);
}
#[test]fn elf_receipt_fifo_unrelated_exact_receipt_does_not_finish_watch(){
 let(mut a,peer,elf)=pending(false,4096);let elf_intent=sent(&mut a,&peer);peer.take_tx();
 let id=advertised(&mut a,&peer);let mut p=qrtr(1,&[0,78,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o']);p[28..32].copy_from_slice(&qcom_servreg::LOCAL_PORT.to_le_bytes());peer.push(&data(id,&p));
 let Ok(Receive::Session(Event::Request(other)))=a.poll_receive(1)else{panic!("other registry")};
 a.queue_reply(1,&other,Reply::Failure{error:3}).unwrap();let other_intent=sent(&mut a,&peer);assert_ne!(elf_intent,other_intent);
 peer.take_tx();peer.push(&control(14,2,other_intent));assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(other))));assert!(!a.elf_reply_fifo().unwrap().fifo.finished);
 peer.push(&control(14,2,elf_intent));assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(elf))));assert!(a.elf_reply_fifo().unwrap().fifo.finished);
}
#[test]fn elf_receipt_fifo_evicted_appearance_stays_unknown(){
 let(mut a,peer,h)=pending(true,4096);let intent=sent(&mut a,&peer);peer.take_tx();
 peer.push(&control(14,2,intent));a.revalidate(1).unwrap();
 for _ in 0..40{peer.push(&control(13,0,0));a.revalidate(1).unwrap();}
 assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::ReplyRxDone(h))));
 let done=a.elf_reply_fifo().unwrap();assert!(done.fifo.finished);assert!(done.fifo.history_evictions>0);
 let bounds=done.fifo.completion.unwrap();assert_eq!(bounds.first_visible,None);assert_eq!(bounds.last_absent,None);assert_eq!(bounds.bytes_ahead,None);
}
