// Actual Owner/FIFO schedule. Its clock ticks are synthetic, not Pixel deadlines.
use super::*;
fn retry_owner_setup(clock:bool)->LoRig{
 retry_owner_setup_rig(LoRig::new(),clock)
}
fn retry_owner_setup_rig(mut r:LoRig,clock:bool)->LoRig{
 use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static CLOCK:AtomicU64=AtomicU64::new(1000);
 let _ipa=r.next();r.hold_host=true;if clock{r.owner.set_rmtfs_event_clock(||CLOCK.fetch_add(1,Relaxed));}
 let query=|txn,name:&[u8]|lo_packet(1,9,qcom_servreg::LOCAL_PORT,true,&lo_message(0,0x21,txn,&[(1,name),(0x10,&0u32.to_le_bytes())]));
 r.input(&query(3,b"avs/audio")).unwrap();assert_eq!(r.held_host.len(),1);
 r.enqueue(&query(4,b"tms/servreg"));r.enqueue(&query(5,b"kernel/elf_loader"));r.owner.notified(8,&mut r.irqs).unwrap();r.drain();
 let three=r.held_host.remove(0);r.peer.push(&control(6,2,three.0));r.owner.notified(8,&mut r.irqs).unwrap();r.drain();assert!(r.owner.elf_completion_turn_taken());
 assert_eq!(r.owner.dispatch_service_registry_step(&mut r.irqs),Ok(qcom_modem_owner::ServiceProgress::ReplyQueued));
 assert_eq!(r.owner.dispatch_service_registry_step(&mut r.irqs),Ok(qcom_modem_owner::ServiceProgress::WaitingReply));
 assert_eq!(r.owner.elf_window_trace().unwrap().last_failure.unwrap().reason,1);
 r.owner.notified(8,&mut r.irqs).unwrap();r.drain();assert_eq!(r.held_host.len(),1);assert_eq!(u16::from_le_bytes([r.held_host[0].1[33],r.held_host[0].1[34]]),4);r
}
fn retry_owner_finish(r:&mut LoRig)->Vec<u8>{
 let four=r.held_host.remove(0);r.peer.push(&control(6,2,four.0));r.owner.notified(8,&mut r.irqs).unwrap();r.drain();
 assert_eq!(r.owner.dispatch_service_registry_step(&mut r.irqs),Ok(qcom_modem_owner::ServiceProgress::ReplyQueued));
 r.owner.notified(8,&mut r.irqs).unwrap();r.drain();let elf=r.held_host.remove(0);assert_eq!(u16::from_le_bytes([elf.1[33],elf.1[34]]),5);r.peer.push(&control(6,2,elf.0));r.owner.notified(8,&mut r.irqs).unwrap();r.drain();elf.1
}
#[test]
fn elf_retry_owner_wire_parity(){let mut r=retry_owner_setup(false);let p=retry_owner_finish(&mut r);print!("ELF_RETRY_WIRE ");for b in p{print!("{b:02x}");}println!();let io=format!("{:?}",r.peer.log());print!("ELF_RETRY_IO ");for b in io.bytes(){print!("{b:02x}");}println!();assert!(r.owner.service_registry_trace().elf_exchange.unwrap().qmi_matches_reference);}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_boundaries_and_first_slot_spent_on_three(){
 for clock in [false,true]{let mut r=retry_owner_setup(clock);let before=r.owner.elf_retry_trace();assert_eq!(before.slot_transaction,Some(3));assert_eq!(before.flags&32,32);assert_eq!(before.flags&2,0);
 let _=retry_owner_finish(&mut r);let t=r.owner.elf_retry_trace();assert_eq!(t.blocker_transaction,Some(4));assert_eq!(t.flags&0xff,0xff);assert_eq!(t.flags&512,0);assert_eq!(t.receipt_at.is_some(),clock);assert_eq!(t.work_returned_at.is_some(),clock);assert_eq!(t.registry_entry_at.is_some(),clock);assert_eq!(t.registry_selected_at.is_some(),clock);assert!(t.return_counts[0]>=t.receipt_counts[0]);assert!(t.return_counts[1]>=t.receipt_counts[1]);if clock{assert!(t.receipt_at<=t.work_returned_at&&t.work_returned_at<=t.registry_entry_at&&t.registry_entry_at<=t.registry_selected_at);}}
}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_getter_no_io_and_later_traffic_cannot_overwrite(){let mut r=retry_owner_setup(true);retry_owner_finish(&mut r);let t=r.owner.elf_retry_trace();r.peer.clear();for _ in 0..4{assert_eq!(r.owner.elf_retry_trace(),t);}assert!(r.peer.log().is_empty());r.owner.notified(8,&mut r.irqs).unwrap();assert_eq!(r.owner.elf_retry_trace(),t);}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_invalid_completion_does_not_latch_receipt(){let mut r=retry_owner_setup(true);let before=r.owner.elf_retry_trace();r.peer.push(&control(6,2,0xffff));assert!(r.owner.notified(8,&mut r.irqs).is_err());assert_eq!(r.owner.elf_retry_trace(),before);assert!(r.owner.error().is_some());}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_unrelated_input_does_not_latch_receipt(){let mut r=retry_owner_setup(false);let before=r.owner.elf_retry_trace();let mut ad=control(5,2,1);ad.extend(4096u32.to_le_bytes());ad.extend(0x10000u32.to_le_bytes());r.peer.push(&ad);r.owner.notified(8,&mut r.irqs).unwrap();assert_eq!(r.owner.elf_retry_trace(),before);}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_fresh_owner_starts_empty(){let mut r=retry_owner_setup(false);retry_owner_finish(&mut r);assert_ne!(r.owner.elf_retry_trace().flags,0);let fresh=LoRig::new();assert_eq!(fresh.owner.elf_retry_trace(),qcom_modem_owner::ElfRetryTrace::default());}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_fault_after_receipt_keeps_return_unknown(){let mut r=retry_owner_setup(true);let four=r.held_host.remove(0);r.peer.push(&control(6,2,four.0));r.peer.push(&control(6,2,0xffff));
 #[cfg(feature="sunfish-registry-ready-handoffs")] {r.owner.notified(8,&mut r.irqs).unwrap();assert_eq!(r.owner.elf_retry_trace().flags&6,6);assert!(r.owner.notified(8,&mut r.irqs).is_err());assert!(r.owner.dispatch_ready_blocker(&mut r.irqs).is_err());}
 #[cfg(not(feature="sunfish-registry-ready-handoffs"))] {assert!(r.owner.notified(8,&mut r.irqs).is_err());let t=r.owner.elf_retry_trace();assert_eq!(t.flags&2,2);assert_eq!(t.flags&4,0);assert_eq!(t.work_returned_at,None);assert_eq!(t.registry_entry_at,None);}assert!(r.owner.error().is_some());}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_clock_unwind_preserves_stopped_and_unknown_return(){
 use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static AT:AtomicU64=AtomicU64::new(u64::MAX);static N:AtomicU64=AtomicU64::new(0);
 fn clock()->u64{let n=N.fetch_add(1,Relaxed)+1;if n==AT.load(Relaxed){panic!("injected optional clock unwind");}n}
 let mut reference=retry_owner_setup(false);let four=reference.held_host.remove(0);N.store(0,Relaxed);AT.store(u64::MAX,Relaxed);reference.owner.set_rmtfs_event_clock(clock);reference.peer.push(&control(6,2,four.0));reference.owner.notified(8,&mut reference.irqs).unwrap();let original=reference.owner.elf_retry_trace();assert_eq!(original.flags&6,6);
 let points=[original.receipt_at.unwrap(),original.work_returned_at.unwrap()];assert!(points[1]>points[0]);
 for point in points{let mut r=retry_owner_setup(false);let four=r.held_host.remove(0);AT.store(point,Relaxed);N.store(0,Relaxed);r.owner.set_rmtfs_event_clock(clock);r.peer.push(&control(6,2,four.0));let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||r.owner.notified(8,&mut r.irqs)));assert!(result.is_err());let t=r.owner.elf_retry_trace();if point==points[1]{assert_eq!(t.flags&2,2);}assert_eq!(t.flags&4,0);assert_eq!(t.work_returned_at,None);assert!(r.owner.notified(8,&mut r.irqs).is_err());}
}
#[cfg(feature="sunfish-tx3-elf-retry")]
#[test]
fn elf_retry_owner_registry_clock_unwind_preserves_stopped(){
 use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static AT:AtomicU64=AtomicU64::new(u64::MAX);static N:AtomicU64=AtomicU64::new(0);
 fn clock()->u64{let n=N.fetch_add(1,Relaxed)+1;if n==AT.load(Relaxed){panic!("injected registry observation clock unwind");}n}
 fn prepared()->LoRig{let mut r=retry_owner_setup(false);let four=r.held_host.remove(0);r.peer.push(&control(6,2,four.0));r.owner.notified(8,&mut r.irqs).unwrap();assert_eq!(r.owner.elf_retry_trace().flags&6,6);r}
 let mut reference=prepared();N.store(0,Relaxed);AT.store(u64::MAX,Relaxed);reference.owner.set_rmtfs_event_clock(clock);reference.owner.dispatch_service_registry_step(&mut reference.irqs).unwrap();let original=reference.owner.elf_retry_trace();let points=[original.registry_entry_at.unwrap(),original.registry_selected_at.unwrap()];assert!(points[1]>points[0]);
 for point in points{let mut r=prepared();N.store(0,Relaxed);AT.store(point,Relaxed);r.owner.set_rmtfs_event_clock(clock);let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||r.owner.dispatch_service_registry_step(&mut r.irqs)));assert!(result.is_err());let t=r.owner.elf_retry_trace();if point==points[1]{assert_eq!(t.flags&8,8);}assert_eq!(t.flags&16,0);assert!(r.owner.notified(8,&mut r.irqs).is_err());}
}

#[cfg(feature="sunfish-registry-ready-handoffs")]
mod ready_handoff_cases {use super::*;include!("sunfish_ready_handoff_owner_cases.rs");}
