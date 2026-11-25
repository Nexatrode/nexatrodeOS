// Actual Router/Owner schedules; synthetic events are not handset deadlines.
use super::*;
fn capacity_setup(confirm:bool,clock:bool,intents:u32)->(Fake,Owner<Fake>,Router<K>,u32){
 let(p,mut o)=owner();o.prepare_service_registry().unwrap();o.prepare_tftp_server(0x5000).unwrap();o.start_control().unwrap();let mut a=control(5,2,intents);for id in 0u32..intents{a.extend(4096u32.to_le_bytes());a.extend(id.to_le_bytes());}p.push(&a);let mut irq=Irqs::default();o.notified(8,&mut irq).unwrap();take_owner_datagrams(&p,&mut o,&mut irq);owner_control_peer_hello(&p,&mut o,&mut irq);take_owner_datagrams(&p,&mut o,&mut irq);for _ in 0..16{o.notified(8,&mut irq).unwrap();let bytes=p.take_tx();if bytes.is_empty()&&o.tftp_queue_snapshot().0.all_tx_used==0{break;}acknowledge_outgoing_glink(&p,&mut o,&mut irq,&bytes);}assert_eq!(o.tftp_queue_snapshot().0.all_tx_used,0);
 let rid=o.offer_receive().unwrap();o.notified(8,&mut irq).unwrap();p.take_tx();if clock{o.set_rmtfs_event_clock(||1000);}
 for n in 0..7{o.queue_tftp_response(qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5001},destination:qcom_qmi_router::Address{node:0,port:0x70+n}},b"\0\x04\0\x01",&mut irq).unwrap();}
 for _ in 0..4{o.notified(8,&mut irq).unwrap();p.take_tx();}
 let mut q=servreg_query_datagram_with_transaction(b"kernel/elf_loader",5);q[16..20].copy_from_slice(&u32::from(confirm).to_le_bytes());p.push(&data(rid,&q));let mut r=Router::new(K::default(),1);r.capture(1<<4).unwrap();r.dispatch(&mut o).unwrap();if !confirm{o.queue_tftp_response(qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5001},destination:qcom_qmi_router::Address{node:0,port:0x77}},b"\0\x04\0\x01",&mut r).unwrap();r.dispatch(&mut o).unwrap();}assert_eq!(o.dispatch_service_registry_step(&mut r),Ok(qcom_modem_owner::ServiceProgress::WaitingReply));assert_eq!(o.elf_capacity_turn_trace().flags,1);let c=o.elf_tx_slots().unwrap();let id=c.latest.slots.iter().find(|s|s.kind==6&&s.remote!=u32::MAX).unwrap().remote;(p,o,r,id)
}
fn capacity_free(p:&Fake,o:&mut Owner<Fake>,r:&mut Router<K>,id:u32){p.take_tx();p.push(&control(6,2,id));r.capture(1<<4).unwrap();r.dispatch(o).unwrap();}
#[test]
fn elf_capacity_retry_claims_free_slot_before_later_tftp_once(){
 for clock in [false,true]{for confirm in [false,true]{let(p,mut o,mut r,id)=capacity_setup(confirm,clock,16);assert_eq!(r.service_elf_capacity_retry(&mut o),Ok(None));capacity_free(&p,&mut o,&mut r,id);let freed=o.elf_tx_slots().unwrap().first_free.unwrap();assert_eq!(r.service_elf_capacity_retry(&mut o),Ok(Some(qcom_modem_owner::ServiceProgress::ReplyQueued)));let t=o.elf_capacity_turn_trace();assert_eq!((t.flags,t.result,t.arms,t.free_slot),(15,1,1,Some(freed.slot)));assert_eq!(t.entered,clock.then_some(1000));assert_eq!(o.elf_tx_slots().unwrap().admitted_slot,Some(freed.slot));
 let later=o.queue_tftp_response(qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5002},destination:qcom_qmi_router::Address{node:0,port:0x90}},b"\0\x05\0\x01not found\0",&mut r);assert!(matches!(later,Err(qcom_modem_owner::Error::Transport(qcom_glink_fifo::Error::Session(qcom_glink_qmi::Error::NoCapacity)))));
 assert_eq!(r.service_elf_capacity_retry(&mut o),Ok(None));for _ in 0..8{r.dispatch(&mut o).unwrap();if o.elf_window_trace().unwrap().writer.finished{break;}}assert!(o.elf_window_trace().unwrap().writer.finished,"writer {:?}, turn {:?}, router {:?}",o.elf_window_trace().unwrap().writer,o.elf_queued_turn_trace(),r.report());assert!(o.service_registry_trace().elf_exchange.unwrap().qmi_matches_reference);assert_eq!(o.elf_capacity_turn_trace().flags,15);
 }}
}
#[test]
fn elf_capacity_retry_full_pool_and_other_owner_keep_opportunity(){
 let(p,mut o,mut r,id)=capacity_setup(false,false,16);r.schedule().unwrap();assert_eq!(r.service_elf_capacity_retry(&mut o),Ok(None));assert_eq!(o.elf_capacity_turn_trace().flags,1);assert_eq!(o.elf_capacity_turn_trace().full_skips,1);capacity_free(&p,&mut o,&mut r,id);r.capture(4).unwrap();assert_eq!(r.service_elf_capacity_retry(&mut o),Ok(None));assert_eq!(o.elf_capacity_turn_trace().flags,1);assert_eq!(r.take_other(),4);assert_eq!(r.service_elf_capacity_retry(&mut o),Ok(Some(qcom_modem_owner::ServiceProgress::ReplyQueued)));assert_eq!(o.elf_capacity_turn_trace().arms,1);
}
#[test]
fn elf_capacity_retry_generation_and_live_fault_keep_existing_guards(){
 let(p,mut o,mut r,id)=capacity_setup(false,false,16);capacity_free(&p,&mut o,&mut r,id);let mut wrong=Router::new(K::default(),2);wrong.schedule().unwrap();p.clear();assert_eq!(wrong.service_elf_capacity_retry(&mut o),Err(Error::Identity));assert!(p.log().is_empty());assert!(o.error().is_some());
 let(p,mut o,mut r,id)=capacity_setup(false,false,16);capacity_free(&p,&mut o,&mut r,id);p.0.borrow_mut().observation.ready=false;p.clear();assert!(r.service_elf_capacity_retry(&mut o).is_err());assert!(o.error().is_some());assert!(writes(&p.log()).is_empty());assert!(o.elf_tx_slots().unwrap().admitted_slot.is_none());
}
#[test]
fn elf_capacity_retry_no_remote_intent_uses_normal_writer_fallback(){
 let(p,mut o,mut r,id)=capacity_setup(false,false,11);capacity_free(&p,&mut o,&mut r,id);assert_eq!(r.service_elf_capacity_retry(&mut o),Ok(Some(qcom_modem_owner::ServiceProgress::ReplyQueued)));assert!(r.dispatch_queued_elf_reply(&mut o).unwrap());assert!(!o.elf_window_trace().unwrap().writer.finished);assert!(!r.dispatch_queued_elf_reply(&mut o).unwrap());p.take_tx();let mut a=control(5,2,1);a.extend(4096u32.to_le_bytes());a.extend(999u32.to_le_bytes());p.push(&a);p.push(&control(8,2,1));r.capture(1<<4).unwrap();for _ in 0..8{r.dispatch(&mut o).unwrap();if o.elf_window_trace().unwrap().writer.finished{break;}}assert!(o.elf_window_trace().unwrap().writer.finished,"writer {:?}, turn {:?}, router {:?}",o.elf_window_trace().unwrap().writer,o.elf_queued_turn_trace(),r.report());assert_eq!(o.elf_capacity_turn_trace().flags,15);
}
