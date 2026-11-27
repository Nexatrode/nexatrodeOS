fn finish_blocker(r:&mut LoRig){let four=r.held_host.remove(0);r.peer.push(&control(6,2,four.0));r.owner.notified(8,&mut r.irqs).unwrap();}
#[test]fn ready_owner_exact_blocker_after_three_lends_one_real_service_attempt(){
 for clock in [false,true]{let mut r=retry_owner_setup(clock);assert_eq!(r.owner.ready_handoff_trace().flags,1);finish_blocker(&mut r);let t=r.owner.ready_handoff_trace();assert_eq!((t.flags,t.receipts,t.services),(3,1,0));assert_eq!(r.owner.elf_retry_trace().slot_transaction,Some(3));
 assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(Some(qcom_modem_owner::ServiceProgress::ReplyQueued)));let t=r.owner.ready_handoff_trace();assert_eq!((t.flags,t.services,t.result),(31,1,1));assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(None));r.owner.notified(8,&mut r.irqs).unwrap();r.drain();let elf=r.held_host.remove(0);assert_eq!(&elf.1[33..35],&5u16.to_le_bytes());r.peer.push(&control(6,2,elf.0));r.owner.notified(8,&mut r.irqs).unwrap();r.drain();assert!(r.owner.service_registry_trace().elf_exchange.unwrap().qmi_matches_reference);assert_eq!(r.owner.ready_handoff_trace(),t);}
}
#[test]fn ready_owner_unrelated_record_and_invalid_receipt_cannot_arm_ready(){
 let mut r=retry_owner_setup(false);let before=r.owner.ready_handoff_trace();let mut ad=control(5,2,1);ad.extend(4096u32.to_le_bytes());ad.extend(0x10000u32.to_le_bytes());r.peer.push(&ad);r.owner.notified(8,&mut r.irqs).unwrap();assert_eq!(r.owner.ready_handoff_trace(),before);assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(None));
 r.peer.push(&control(6,2,0xffff));assert!(r.owner.notified(8,&mut r.irqs).is_err());assert_eq!(r.owner.ready_handoff_trace(),before);assert!(r.owner.dispatch_ready_blocker(&mut r.irqs).is_err());
}
#[test]fn ready_owner_live_validation_fault_cannot_publish_or_reuse_service(){
 let mut r=retry_owner_setup(false);finish_blocker(&mut r);r.peer.0.borrow_mut().observation.ready=false;r.peer.clear();assert!(r.owner.dispatch_ready_blocker(&mut r.irqs).is_err());assert!(r.held_host.is_empty());assert!(r.owner.error().is_some());assert!(r.owner.dispatch_ready_blocker(&mut r.irqs).is_err());
}
#[test]fn ready_owner_invalid_backlog_is_still_parsed_before_queued_reply_publication(){
 let mut r=retry_owner_setup(false);let four=r.held_host.remove(0);r.peer.push(&control(6,2,four.0));r.peer.push(&control(6,2,0xffff));r.owner.notified(8,&mut r.irqs).unwrap();assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(Some(qcom_modem_owner::ServiceProgress::ReplyQueued)));assert!(r.owner.notified(8,&mut r.irqs).is_err());assert!(r.held_host.is_empty());
}
#[test]fn ready_owner_ordinary_service_wins_without_duplicate_admission(){
 let mut r=retry_owner_setup(false);finish_blocker(&mut r);assert_eq!(r.owner.dispatch_service_registry_step(&mut r.irqs),Ok(qcom_modem_owner::ServiceProgress::ReplyQueued));assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(None));assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(None));assert_eq!(r.owner.ready_handoff_trace().services,1);assert_ne!(r.owner.ready_handoff_trace().flags&32,0);
}
#[test]fn ready_owner_fresh_owner_and_getter_do_not_inherit_authority(){
 let mut r=retry_owner_setup(false);finish_blocker(&mut r);let t=r.owner.ready_handoff_trace();r.peer.clear();for _ in 0..4{assert_eq!(r.owner.ready_handoff_trace(),t);}assert!(r.peer.log().is_empty());let mut fresh=LoRig::new();assert_eq!(fresh.owner.ready_handoff_trace(),qcom_modem_owner::ReadyHandoffTrace::default());assert_eq!(fresh.owner.dispatch_ready_blocker(&mut fresh.irqs),Ok(None));
}
#[derive(Default)]struct ReadyKernel;
impl crate::elf_blocker_irq::Kernel for ReadyKernel{fn acknowledge(&mut self,_:u8)->Result<(),()>{Ok(())}}
#[test]fn ready_router_retains_other_owner_and_generation_guards(){
 let mut r=retry_owner_setup(false);finish_blocker(&mut r);let mut router=crate::elf_blocker_irq::Router::new(ReadyKernel,1);qcom_modem_owner::Interrupts::schedule(&mut router).unwrap();router.capture(4).unwrap();assert_eq!(router.service_ready_blocker(&mut r.owner),Ok(None));assert_eq!(r.owner.ready_handoff_trace().services,0);assert_eq!(router.take_other(),4);assert_eq!(router.service_ready_blocker(&mut r.owner),Ok(Some(qcom_modem_owner::ServiceProgress::ReplyQueued)));assert_eq!(router.service_ready_blocker(&mut r.owner),Ok(None));
 let mut r=retry_owner_setup(false);finish_blocker(&mut r);let mut wrong=crate::elf_blocker_irq::Router::new(ReadyKernel,2);qcom_modem_owner::Interrupts::schedule(&mut wrong).unwrap();r.peer.clear();assert_eq!(wrong.service_ready_blocker(&mut r.owner),Err(crate::elf_blocker_irq::Error::Identity));assert!(r.peer.log().is_empty());assert!(r.owner.error().is_some());
}
#[test]fn ready_owner_full_capacity_remains_a_normal_wait_after_handoff(){
 let mut session=lo_session();session.prepare_tftp_server(1,0x5000).unwrap();let mut r=retry_owner_setup_rig(LoRig::from_session(0,false,session),false);finish_blocker(&mut r);let mut full=false;
 for i in 0..16{let route=qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5001},destination:qcom_qmi_router::Address{node:0,port:0x70+i}};match r.owner.queue_tftp_response(route,b"\0\x04\0\x01",&mut r.irqs){Ok(())=>{},Err(qcom_modem_owner::Error::Transport(qcom_glink_fifo::Error::Session(qcom_glink_qmi::Error::NoCapacity)))=>{full=true;break;},other=>panic!("unexpected queue {other:?}")}}
 assert!(full);assert!(r.owner.error().is_none());assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(Some(qcom_modem_owner::ServiceProgress::WaitingReply)));assert_eq!(r.owner.ready_handoff_trace().result,2);assert_eq!(r.owner.dispatch_ready_blocker(&mut r.irqs),Ok(None));
}
#[test]fn ready_owner_clock_unwind_consumes_attempt_and_stays_stopped(){
 use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static AT:AtomicU64=AtomicU64::new(u64::MAX);static N:AtomicU64=AtomicU64::new(0);
 fn clock()->u64{let n=N.fetch_add(1,Relaxed)+1;if n==AT.load(Relaxed){panic!("injected ready service clock");}n}
 let mut reference=retry_owner_setup(false);finish_blocker(&mut reference);N.store(0,Relaxed);AT.store(u64::MAX,Relaxed);reference.owner.set_rmtfs_event_clock(clock);assert_eq!(reference.owner.dispatch_ready_blocker(&mut reference.irqs),Ok(Some(qcom_modem_owner::ServiceProgress::ReplyQueued)));let count=N.load(Relaxed);assert!(count>=4);
 for point in 1..=count{let mut r=retry_owner_setup(false);finish_blocker(&mut r);N.store(0,Relaxed);AT.store(point,Relaxed);r.owner.set_rmtfs_event_clock(clock);assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||r.owner.dispatch_ready_blocker(&mut r.irqs))).is_err(),"clock {point}/{count}");assert_eq!(r.owner.ready_handoff_trace().services,1);assert!(r.owner.dispatch_ready_blocker(&mut r.irqs).is_err(),"clock {point}/{count}");assert!(r.owner.notified(8,&mut r.irqs).is_err());assert!(r.held_host.is_empty());}
}
