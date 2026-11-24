// Actual Router/Owner fixtures. Synthetic ordering schedules, not handset deadlines.
use super::*;
fn queued_elf_setup(intents:u32,confirm:bool)->(Fake,Owner<Fake>,Router<K>){queued_elf_clock_setup(intents,confirm,true)}
fn queued_elf_clock_setup(intents:u32,confirm:bool,clock:bool)->(Fake,Owner<Fake>,Router<K>){queued_elf_pressure_setup(intents,confirm,clock,false)}
fn queued_elf_pressure_setup(intents:u32,confirm:bool,clock:bool,fill:bool)->(Fake,Owner<Fake>,Router<K>){
 let(peer,mut o)=owner();o.prepare_service_registry().unwrap();o.prepare_tftp_server(0x5000).unwrap();o.start_control().unwrap();
 let mut a=control(5,2,intents);for id in 0u32..intents{a.extend(4096u32.to_le_bytes());a.extend(id.to_le_bytes());}peer.push(&a);
 let mut irqs=Irqs::default();o.notified(8,&mut irqs).unwrap();peer.take_tx();owner_control_peer_hello(&peer,&mut o,&mut irqs);take_owner_datagrams(&peer,&mut o,&mut irqs);
 let id=o.offer_receive().unwrap();o.notified(8,&mut irqs).unwrap();peer.take_tx();let mut q=servreg_query_datagram_with_transaction(b"kernel/elf_loader",5);q[16..20].copy_from_slice(&u32::from(confirm).to_le_bytes());
 if fill {
  let mut bytes=[0u8;4032];bytes[..4].copy_from_slice(&[0,3,0,1]);
  for n in 0..4{o.queue_tftp_response(qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5100+n},destination:qcom_qmi_router::Address{node:0,port:0x70+n}},&bytes,&mut irqs).unwrap();}
  for _ in 0..4{o.notified(8,&mut irqs).unwrap();}
  // Keep the peer tail stationary: real ordinary writes occupy the FIFO.
  let m=peer.0.borrow();assert!((m.words[1]as usize+FIFO_BYTES-m.words[0]as usize)%FIFO_BYTES>16000,"occupied {}",(m.words[1]as usize+FIFO_BYTES-m.words[0]as usize)%FIFO_BYTES);
 }
 if clock{o.set_rmtfs_event_clock(||1000);}
 let mut r=Router::new(K::default(),1);peer.push(&data(id,&q));r.capture(1<<4).unwrap();r.dispatch(&mut o).unwrap();
 assert_eq!(o.dispatch_service_registry_step(&mut r),Ok(qcom_modem_owner::ServiceProgress::ReplyQueued));
 assert!(r.report().continuation_pending);assert_eq!(o.elf_queued_turn_trace().flags,1);(peer,o,r)
}
#[test]
fn elf_queued_turn_sends_exact_reply_before_later_service_work_once(){
 let(p,mut o,mut r)=queued_elf_setup(16,false);assert!(o.service_registry_trace().elf.unwrap().published.is_none());
 assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());let t=o.elf_queued_turn_trace();assert_eq!(t.flags,31);assert!(o.service_registry_trace().elf_exchange.unwrap().qmi_matches_reference);
 let frames=p.take_tx();let replies:Vec<_>=owner_datagrams(&frames).into_iter().filter(|v|owner_word(v,12)==qcom_servreg::LOCAL_PORT&&owner_word(v,4)==1).collect();assert_eq!(replies.len(),1);let qmi=&replies[0][32..];assert_eq!((qmi[0],u16::from_le_bytes([qmi[1],qmi[2]]),u16::from_le_bytes([qmi[3],qmi[4]])),(2,5,0x21));
 p.clear();assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());assert!(p.log().is_empty());
 let mut irq=Irqs::default();acknowledge_outgoing_glink(&p,&mut o,&mut irq,&frames);assert!(o.service_registry_trace().elf_exchange.unwrap().completed);
}
#[test]
fn elf_queued_turn_already_published_keeps_ordinary_path(){
 let(p,mut o,mut r)=queued_elf_setup(16,false);r.dispatch(&mut o).unwrap();assert!(o.service_registry_trace().elf.unwrap().published.is_some());p.clear();assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());assert!(p.log().is_empty());assert_eq!(o.elf_queued_turn_trace().flags,1);
}
#[test]
fn elf_queued_turn_other_owner_defers_without_consuming(){
 let(p,mut o,mut r)=queued_elf_setup(16,false);r.capture(4).unwrap();p.clear();assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());assert_eq!(o.elf_queued_turn_trace().flags,1);assert!(p.log().is_empty());assert_eq!(r.take_other(),4);assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());assert_eq!(o.elf_queued_turn_trace().flags,31);
}
#[test]
fn elf_queued_turn_no_intent_consumes_then_normal_fallback_completes(){
 let(p,mut o,mut r)=queued_elf_setup(3,false);assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());assert_eq!(o.elf_queued_turn_trace().flags,15);assert!(o.service_registry_trace().elf.unwrap().published.is_none());assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());p.take_tx();
 let mut a=control(5,2,1);a.extend(4096u32.to_le_bytes());a.extend(123u32.to_le_bytes());p.push(&a);p.push(&control(8,2,1));r.capture(1<<4).unwrap();for _ in 0..4{r.dispatch(&mut o).unwrap();if o.service_registry_trace().elf.unwrap().published.is_some(){break;}}assert!(o.service_registry_trace().elf.unwrap().published.is_some());assert_eq!(o.elf_queued_turn_trace().flags,15,"the bounded attempt does not absorb later normal progress");
}
#[test]
fn elf_queued_turn_fault_stops_without_publication_or_irq_ack(){
 let(p,mut o,mut r)=queued_elf_setup(16,false);p.take_tx();p.clear();p.0.borrow_mut().fail=Some("observe");assert!(r.dispatch_queued_elf_reply(&mut o).is_err());assert_eq!(o.elf_queued_turn_trace().flags,39);assert!(o.error().is_some());assert!(writes(&p.log()).is_empty());assert!(r.report().error.is_some());
}
#[test]
fn elf_queued_turn_wrong_generation_faults_before_io(){
 let(p,mut o,_)=queued_elf_setup(16,false);let mut r=Router::new(K::default(),2);r.schedule().unwrap();p.clear();assert_eq!(r.dispatch_queued_elf_reply(&mut o),Err(Error::Identity));assert!(p.log().is_empty());assert_eq!(o.elf_queued_turn_trace().flags,1);
}
#[test]
fn elf_queued_turn_no_capability_does_not_run_extra_work(){
 let(mut o,p)=actor();let mut r=Router::new(K::default(),1);r.schedule().unwrap();p.clear();assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());assert!(p.log().is_empty());assert_eq!(o.elf_queued_turn_trace().flags,0);
}
#[test]
fn elf_queued_turn_preserves_backing_ad_before_grant_ack_and_credit_before_reply(){
 let(p,mut o,mut r)=queued_elf_setup(16,true);p.push(&control(7,2,108));r.capture(1<<4).unwrap();assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());let bytes=p.take_tx();let mut at=0;let mut ad=None;let mut ack=None;let mut reply=None;let mut credit=None;
 while at<bytes.len(){let cmd=u16::from_le_bytes([bytes[at],bytes[at+1]]);let n=match cmd{5=>8+owner_word(&bytes,at+4)as usize*8,9=>{let n=owner_word(&bytes,at+8)as usize;16+((n+7)&!7)},_=>8};
 if cmd==5{ad=Some(at);}if cmd==8{ack=Some(at);}if cmd==9{let q=&bytes[at+16..];if owner_word(q,4)==7{credit=Some(at);}if owner_word(q,4)==1&&owner_word(q,12)==qcom_servreg::LOCAL_PORT{reply=Some(at);}}at+=n;}
 assert!(ad.unwrap()<ack.unwrap());assert!(ack.unwrap()<reply.unwrap());if let Some(c)=credit{assert!(c<reply.unwrap());}assert_eq!(o.elf_queued_turn_trace().flags,31);
}

#[test]
fn elf_queued_turn_clockless_state_still_consumes_and_publishes(){
 let(_,mut o,mut r)=queued_elf_clock_setup(16,false,false);assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());let t=o.elf_queued_turn_trace();assert_eq!(t.flags,31);assert_eq!((t.armed_at,t.entry,t.end),(None,None,None));assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());
}

#[test]
fn elf_queued_turn_full_fifo_consumes_once_then_ordinary_write_retries(){
 let(p,mut o,mut r)=queued_elf_pressure_setup(16,false,true,true);assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());assert_eq!(o.elf_queued_turn_trace().flags,15);assert!(o.service_registry_trace().elf.unwrap().published.is_none());assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());
 let bytes=p.take_tx();assert!(bytes.len()>16000);r.dispatch(&mut o).unwrap();assert!(o.service_registry_trace().elf.unwrap().published.is_some());assert_eq!(o.elf_queued_turn_trace().flags,15);assert!(r.report().error.is_none());
}
