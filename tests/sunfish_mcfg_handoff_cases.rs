// Actual queued-ELF Router/Owner boundary, including a close received by its dispatch.
use super::*;
fn setup(published:bool)->(Fake,Owner<Fake>,Router<K>,qcom_tftp_server_check::ServerCheck,u32,Vec<u8>){
 let(p,mut o)=owner();let mut irqs=Irqs::default();o.prepare_service_registry().unwrap();o.prepare_tftp_server(0x5000).unwrap();o.start_control().unwrap();
 let mut ad=control(5,2,16);for id in 0u32..16 {ad.extend(4096u32.to_le_bytes());ad.extend(id.to_le_bytes());}p.push(&ad);o.notified(8,&mut irqs).unwrap();p.take_tx();owner_control_peer_hello(&p,&mut o,&mut irqs);take_owner_datagrams(&p,&mut o,&mut irqs);
 let req=o.offer_receive().unwrap();let elf=o.offer_receive().unwrap();let close=o.offer_receive().unwrap();o.notified(8,&mut irqs).unwrap();p.take_tx();
 let request=b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0";
 p.push(&data(req,&owner_qrtr_packet(1,0x49,0x5000,false,request)));o.notified(1,&mut irqs).unwrap();
 let mut server=qcom_tftp_server_check::ServerCheck::new();let wire=if published{o.service_tftp_server_check_step(&mut server,&mut irqs).unwrap();o.notified(8,&mut irqs).unwrap();p.take_tx()}else{Vec::new()};
 let mut r=Router::new(K::default(),1);r.schedule().unwrap();p.push(&data(elf,&servreg_query_datagram_with_transaction(b"kernel/elf_loader",5)));r.capture(1<<4).unwrap();r.dispatch(&mut o).unwrap();
 assert_eq!(o.dispatch_service_registry_step(&mut r),Ok(qcom_modem_owner::ServiceProgress::ReplyQueued));assert!(r.report().continuation_pending);
 (p,o,r,server,close,wire)
}
#[test]
fn mcfg_handoff_nested_elf_close_resolves_before_next_service(){
 for published in [false,true] {
  let(p,mut o,mut r,mut server,close,wire)=setup(published);p.push(&data(close,&owner_control_frame(6,[0,0x49,0,0])));
  assert!(r.dispatch_queued_elf_mcfg_handoff(&mut o,&mut server).unwrap());
  assert!(o.pending_mcfg_close().is_none(),"ordinary service must not see the paused close");assert_eq!(o.mcfg_close_trace().committed,1);assert!(o.error().is_none());
  assert!(o.dispatch_service_registry_step(&mut r).is_ok());
  let t=o.mcfg_close_trace();assert_eq!(t.published,u32::from(published));assert_eq!(t.held,u32::from(!published));
  if published{complete(&p,&mut o,&mut r,&wire);assert_eq!(o.tftp_tx_progress()[2],1);}
  let mut all=Vec::new();for _ in 0..32 {r.dispatch(&mut o).unwrap();all.extend(p.take_tx());if o.elf_window_trace().unwrap().writer.finished{break;}}
  assert!(o.elf_window_trace().unwrap().writer.finished,"ELF publication requires actual writer completion, independent of optional clocks");complete(&p,&mut o,&mut r,&all);
  assert!(o.service_registry_trace().elf_exchange.unwrap().completed);assert!(r.report().error.is_none());
 }
}
#[test]
fn mcfg_handoff_unwrapped_path_reproduces_paused_service_failure(){
 let(p,mut o,mut r,_server,close,_)=setup(false);p.push(&data(close,&owner_control_frame(6,[0,0x49,0,0])));
 assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());assert!(o.pending_mcfg_close().is_some());
 assert_eq!(o.dispatch_service_registry_step(&mut r),Err(qcom_modem_owner::Error::Stopped));assert_eq!(o.mcfg_close_trace().committed,0);
}
#[test]
fn mcfg_handoff_without_close_has_identical_fifo_operations_and_bytes(){
 let(p,mut o,mut r)=queued_elf_setup(16,false);p.clear();r.dispatch_queued_elf_reply(&mut o).unwrap();let operations=p.log();let bytes=p.take_tx();
 let(p,mut o,mut r)=queued_elf_setup(16,false);p.clear();let mut server=qcom_tftp_server_check::ServerCheck::new();r.dispatch_queued_elf_mcfg_handoff(&mut o,&mut server).unwrap();assert_eq!(p.log(),operations);assert_eq!(p.take_tx(),bytes);assert_eq!(o.mcfg_close_trace().committed,0);
}

fn complete(p:&Fake,o:&mut Owner<Fake>,r:&mut Router<K>,records:&[u8]){
 let mut at=0;
 while at<records.len(){let cmd=u16::from_le_bytes([records[at],records[at+1]]);let len=match cmd {5=>8+owner_word(records,at+4)as usize*8,9|12=>{let n=owner_word(records,at+8)as usize;16+((n+7)&!7)},_=>8};
 if matches!(cmd,9|12)&&owner_word(records,at+12)==0{p.push(&control(14,2,owner_word(records,at+4)));}
 at+=len;
 }
 r.dispatch(o).unwrap();
}
#[test]
fn mcfg_handoff_pending_close_obeys_other_owner_then_resolves_without_new_dispatch(){
 let(p,mut o,mut r,mut server,close,_)=setup(false);p.push(&data(close,&owner_control_frame(6,[0,0x49,0,0])));r.dispatch_queued_elf_reply(&mut o).unwrap();
 r.capture(4).unwrap();p.clear();assert!(!r.dispatch_queued_elf_mcfg_handoff(&mut o,&mut server).unwrap());assert!(p.log().is_empty());assert!(o.pending_mcfg_close().is_some());
 assert_eq!(r.take_other(),4);assert!(!r.dispatch_queued_elf_mcfg_handoff(&mut o,&mut server).unwrap());assert!(o.pending_mcfg_close().is_none());assert_eq!(o.mcfg_close_trace().stage,6);
}
#[test]
fn mcfg_handoff_pending_close_fresh_gate_fault_is_terminal_before_normal_work(){
 let(p,mut o,mut r,mut server,close,_)=setup(false);p.push(&data(close,&owner_control_frame(6,[0,0x49,0,0])));r.dispatch_queued_elf_reply(&mut o).unwrap();
 p.clear();p.0.borrow_mut().observation.ready=false;assert!(r.dispatch_queued_elf_mcfg_handoff(&mut o,&mut server).is_err());assert!(r.report().error.is_some());assert!(o.error().is_some());assert_eq!(o.mcfg_close_trace().committed,0);assert_eq!(o.mcfg_close_trace().stage,3);
 assert!(!p.log().iter().any(|v|matches!(v,Op::Write(..)|Op::Head(_)|Op::Tail(_)|Op::Notify)));
 p.clear();assert!(r.dispatch_queued_elf_mcfg_handoff(&mut o,&mut server).is_err());assert!(p.log().is_empty());
}
#[test]
fn mcfg_handoff_wrong_generation_does_not_resolve_a_live_close(){
 let(p,mut o,mut r,mut server,close,_)=setup(false);p.push(&data(close,&owner_control_frame(6,[0,0x49,0,0])));r.dispatch_queued_elf_reply(&mut o).unwrap();
 let mut wrong=Router::new(K::default(),2);wrong.schedule().unwrap();p.clear();assert_eq!(wrong.dispatch_queued_elf_mcfg_handoff(&mut o,&mut server),Err(Error::Identity));assert!(p.log().is_empty());assert_eq!(o.mcfg_close_trace().committed,0);assert!(o.error().is_some());
}
