const RF_RULE:&[u8]=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0timeoutms\x001000\0rsize\x004\0";
fn rf_owner(clocked:bool)->(Fake,Owner<Fake>,Irqs){
 if clocked{return mc_owner();}
 let(p,mut o)=owner();let mut i=Irqs::default();o.prepare_tftp_server(0x5000).unwrap();o.prepare_service_registry().unwrap();o.start_control().unwrap();let mut adv=control(5,2,16);for id in 0u32..16{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}p.push(&adv);o.notified(8,&mut i).unwrap();p.take_tx();owner_control_peer_hello(&p,&mut o,&mut i);take_owner_datagrams(&p,&mut o,&mut i);(p,o,i)
}
fn rf_send(p:&Fake,o:&mut Owner<Fake>,i:&mut Irqs,s:&mut qcom_tftp_server_check::ServerCheck,peer:u32)->Vec<u8>{mc_input(p,o,i,peer,0x5000,RF_RULE);o.service_tftp_server_check_step(s,i).unwrap();o.notified(8,i).unwrap();p.take_tx()}
fn rf_pending(clocked:bool)->(Fake,Owner<Fake>,Irqs,Vec<u8>,Vec<u8>){
 let(p,mut o,mut i)=rf_owner(clocked);let mut s=qcom_tftp_server_check::ServerCheck::new();let first=rf_send(&p,&mut o,&mut i,&mut s,0x21);acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);let second=rf_send(&p,&mut o,&mut i,&mut s,0x21);let third=rf_send(&p,&mut o,&mut i,&mut s,0x23);assert_eq!((o.startup_prerequisites().reads[1].unwrap().flags,o.startup_prerequisites().reads[6].unwrap().flags),(31,31));o.revalidate().unwrap();(p,o,i,second,third)
}
#[test]
fn ruleset_fifo_owner_wire_and_complete_io_are_observation_only(){
 let(p,mut o,mut i,a,b)=rf_pending(false);
 #[cfg(feature="sunfish-ruleset-reply-fifo")]{let x=o.ruleset_reply_fifo(0).unwrap();let y=o.ruleset_reply_fifo(1).unwrap();assert_eq!(x.record,o.startup_prerequisites().reads[1].unwrap().record_sequence().unwrap());assert_eq!(y.record,o.startup_prerequisites().reads[6].unwrap().record_sequence().unwrap());assert_ne!(x.record,y.record);assert_ne!(x.intent,y.intent);assert_eq!((x.ordinal,y.ordinal),(2,3));}
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&b);acknowledge_outgoing_glink(&p,&mut o,&mut i,&a);assert_eq!((o.startup_prerequisites().reads[1].unwrap().flags,o.startup_prerequisites().reads[6].unwrap().flags),(63,63));assert!(o.error().is_none());
 print!("RULESET_FIFO_WIRE ");for w in [&a,&b]{for c in w{print!("{c:02x}");}}println!();println!("RULESET_FIFO_IO {:?}",p.log());
}
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[test]
fn ruleset_fifo_peer_read_and_prefixed_receipt_arrival_separate_from_processing(){
 let(p,mut o,mut i,_,b)=rf_pending(true);let w=o.ruleset_reply_fifo(0).unwrap();assert_eq!(w.pending_tx_bytes,0);assert!(w.fifo.peer_tail_at.is_some());assert!(!w.fifo.finished);
 p.push(&control(13,0,0));p.push(&control(14,2,w.intent));o.revalidate().unwrap();assert!(!o.ruleset_reply_fifo(0).unwrap().fifo.finished);o.notified(8,&mut i).unwrap();let done=o.ruleset_reply_fifo(0).unwrap();assert!(done.fifo.finished);let bounds=done.fifo.completion.unwrap();assert!(bounds.last_absent.is_some()&&bounds.first_visible.is_some());assert_eq!(bounds.bytes_ahead,Some(8));assert!(!o.ruleset_reply_fifo(1).unwrap().fifo.finished);
 let log=p.log();assert_eq!(o.ruleset_reply_fifo(0),Some(done));assert_eq!(p.log(),log);acknowledge_outgoing_glink(&p,&mut o,&mut i,&b);assert!(o.ruleset_reply_fifo(1).unwrap().fifo.finished);assert_eq!(o.ruleset_reply_fifo(0),Some(done));
}
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[test]
fn ruleset_fifo_unrelated_reply_and_return_order_do_not_cross_bind(){
 for reverse in [false,true]{let(p,mut o,mut i,a,b)=rf_pending(true);let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x24,0x5000,STAT);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let other=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&other);assert!(!o.ruleset_reply_fifo(0).unwrap().fifo.finished&&!o.ruleset_reply_fifo(1).unwrap().fifo.finished);
 let(first,last,n)=if reverse{(&b,&a,1)}else{(&a,&b,0)};acknowledge_outgoing_glink(&p,&mut o,&mut i,first);assert!(o.ruleset_reply_fifo(n).unwrap().fifo.finished);assert!(!o.ruleset_reply_fifo(1-n).unwrap().fifo.finished);let frozen=o.ruleset_reply_fifo(n);acknowledge_outgoing_glink(&p,&mut o,&mut i,last);assert!(o.ruleset_reply_fifo(1-n).unwrap().fifo.finished);assert_eq!(o.ruleset_reply_fifo(n),frozen);assert!(o.error().is_none());}
}
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[test]
fn ruleset_fifo_exact_close_context_does_not_reclaim_published_buffer(){
 let(p,mut o,mut i,a,b)=rf_pending(true);mc_close(&p,&mut o,&mut i,0x25);assert!(o.ruleset_reply_fifo(0).unwrap().unread_at_close.is_none());mc_close(&p,&mut o,&mut i,0x21);assert!(o.ruleset_reply_fifo(0).unwrap().unread_at_close.is_some());assert!(o.ruleset_reply_fifo(1).unwrap().unread_at_close.is_none());assert!(!o.ruleset_reply_fifo(0).unwrap().fifo.finished);acknowledge_outgoing_glink(&p,&mut o,&mut i,&a);let w=o.ruleset_reply_fifo(0).unwrap();assert!(w.fifo.finished);assert_eq!(w.fifo.completion.unwrap().visible_at_close,Some(false));assert_eq!(o.startup_prerequisites().reads[1].unwrap().flags,191);acknowledge_outgoing_glink(&p,&mut o,&mut i,&b);assert!(o.ruleset_reply_fifo(1).unwrap().fifo.finished);assert!(o.error().is_none());
}
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[test]
fn ruleset_fifo_faulted_validation_cannot_advance_cached_windows(){
 let(p,mut o,_,_,_)=rf_pending(true);let before=[o.ruleset_reply_fifo(0),o.ruleset_reply_fifo(1)];p.0.borrow_mut().observation.ready=false;assert!(o.revalidate().is_err());assert_eq!([o.ruleset_reply_fifo(0),o.ruleset_reply_fifo(1)],before);
}
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[test]
fn ruleset_fifo_evicted_arrival_stays_unknown(){
 let(p,mut o,mut i,_,_)=rf_pending(true);let id=o.ruleset_reply_fifo(0).unwrap().intent;p.push(&control(14,2,id));o.revalidate().unwrap();for _ in 0..40{p.push(&control(13,0,0));o.revalidate().unwrap();}o.notified(8,&mut i).unwrap();let w=o.ruleset_reply_fifo(0).unwrap();assert!(w.fifo.finished&&w.fifo.history_evictions>0);let b=w.fifo.completion.unwrap();assert_eq!((b.last_absent,b.first_visible,b.bytes_ahead),(None,None,None));
}
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[test]
fn ruleset_fifo_clockless_real_returns_keep_identity_and_consumption_latches(){
 let(p,mut o,mut i,a,b)=rf_pending(false);let w=o.ruleset_reply_fifo(0).unwrap();assert_eq!((w.armed,w.fifo.peer_tail_at,w.pending_tx_bytes),(None,None,0));acknowledge_outgoing_glink(&p,&mut o,&mut i,&a);acknowledge_outgoing_glink(&p,&mut o,&mut i,&b);for n in 0..2{let w=o.ruleset_reply_fifo(n).unwrap();assert!(w.fifo.finished);assert_eq!(w.fifo.completion.unwrap().first_visible,None);assert_ne!(w.record,0);}assert!(o.ruleset_reply_fifo(2).is_none());
 let(_,o,_)=rf_owner(false);assert!(o.ruleset_reply_fifo(0).is_none()&&o.ruleset_reply_fifo(1).is_none());
}
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[test]
fn ruleset_fifo_queue_or_uncertain_publication_cannot_arm(){
 let(p,mut o,mut i)=rf_owner(true);let mut server=qcom_tftp_server_check::ServerCheck::new();let first=rf_send(&p,&mut o,&mut i,&mut server,0x21);acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);mc_input(&p,&mut o,&mut i,0x21,0x5000,RF_RULE);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();assert!(o.ruleset_reply_fifo(0).is_none());p.0.borrow_mut().fail_after_store=true;assert!(o.notified(8,&mut i).is_err());assert!(o.ruleset_reply_fifo(0).is_none());assert_eq!(o.startup_prerequisites().reads[1].unwrap().flags&48,0);assert!(o.error().is_some());
}
