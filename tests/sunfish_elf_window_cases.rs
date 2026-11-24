// Actual Core admission/receipt schedules. Synthetic events, not Pixel deadlines.
use super::*;
fn request(s:&mut Session,id:u32,txn:u16,name:&[u8])->RequestHandle{request_peer(s,id,txn,name,9)}
fn request_peer(s:&mut Session,id:u32,txn:u16,name:&[u8],peer:u32)->RequestHandle{
 let mut q=vec![0];q.extend(txn.to_le_bytes());q.extend(0x21u16.to_le_bytes());q.extend(((3+name.len())as u16).to_le_bytes());q.push(1);q.extend((name.len()as u16).to_le_bytes());q.extend(name);
 let p=qrtr(1,peer,qcom_servreg::LOCAL_PORT,false,&q);
 let Event::Request(h)=s.ingest(1,&data(9,id,&p,0)).unwrap()else{panic!("typed locator request");};h
}
fn queue(s:&mut Session,h:&RequestHandle,name:&[u8])->Result<(),Error>{
 let r=qcom_servreg::respond(qcom_servreg::Query{service_name:name,domain_offset:0});s.queue_reply(h,Reply::Success{optional_tlvs:r.optional_tlvs()})
}
fn tx(s:&mut Session)->u32{let(Published::Data{remote_intent,last_fragment:true},_)=publish_until_data(s)else{panic!("complete data");};remote_intent}
fn setup(clock:bool,four_published:bool)->(Session,RequestHandle,RequestHandle,RequestHandle,u32,Option<u32>){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.prepare_tftp_server(1,0x5000).unwrap();
 if clock{s.set_registry_ack_clock(||17);}
 s.ingest(1,&intents(&[(7,4096),(8,4096),(9,4096),(10,4096)])).unwrap();
 let ids=[offer(&mut s),offer(&mut s),offer(&mut s)];
 let h3=request(&mut s,ids[0],3,b"avs/audio");queue(&mut s,&h3,b"avs/audio").unwrap();let i3=tx(&mut s);
 let h4=request(&mut s,ids[1],4,b"tms/servreg");queue(&mut s,&h4,b"tms/servreg").unwrap();let i4=four_published.then(||tx(&mut s));
 let elf=request(&mut s,ids[2],5,b"kernel/elf_loader");#[cfg(feature="sunfish-elf-window-trace")]assert!(s.elf_window_trace().is_some());
 (s,h3,h4,elf,i3,i4)
}
fn done(s:&mut Session,h:RequestHandle,intent:u32){assert_eq!(s.ingest(1,&control(14,intent)),Ok(Event::ReplyRxDone(h)));}
#[cfg(feature="sunfish-elf-window-trace")]
#[test]
fn elf_window_either_receipt_releases_one_of_two_owners(){
 for clock in [false,true]{for four_first in [false,true]{
 let(mut s,h3,h4,elf,i3,i4)=setup(clock,true);let i4=i4.unwrap();assert_ne!(i3,i4);
 assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));
 let f=s.elf_window_trace().unwrap().first_failure.unwrap();assert_eq!((f.reason,f.count,f.gates),(1,2,1));let mut txns=f.entries.map(|e|e.unwrap().transaction);txns.sort();assert_eq!(txns,[3,4]);assert_eq!(f.at,clock.then_some(17));
 // A real registry completion on another peer cannot release either owner.
 let id=offer(&mut s);let unrelated_h=request_peer(&mut s,id,2,b"tms/servreg",10);queue(&mut s,&unrelated_h,b"tms/servreg").unwrap();let unrelated=tx(&mut s);done(&mut s,unrelated_h,unrelated);
 assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));assert_eq!(s.elf_window_trace().unwrap().receipt_count,0);
 let(winner,retained,id,other_id,txn)=if four_first{(h4,h3,i4,i3,4)}else{(h3,h4,i3,i4,3)};done(&mut s,winner,id);assert!(s.reply_fully_published(&retained));
 queue(&mut s,&elf,b"kernel/elf_loader").unwrap();let c=s.elf_window_trace().unwrap();let a=c.admitted.unwrap();let r=c.receipts[0].unwrap();assert_eq!((a.reason,a.count,a.gates),(0,1,0));assert_eq!(c.last_before_admission,Some(txn));assert_eq!((r.transaction,r.remote_intent,r.remaining,r.matched_last_failure),(txn,id,1,true));assert!(r.sequence>a.sequence.saturating_sub(2)&&r.sequence<a.sequence);
 assert_eq!(a.entries[0].unwrap().transaction,if four_first{3}else{4});assert_eq!(a.entries[0].unwrap().remote_intent,other_id);
 let ie=tx(&mut s);assert_ne!(ie,other_id);done(&mut s,elf,ie);let frozen=*s.elf_window_trace().unwrap();assert!(frozen.finished);assert_eq!(frozen.flags,31);
 done(&mut s,retained,other_id);assert_eq!(s.elf_window_trace().unwrap().receipt_count,frozen.receipt_count);assert_eq!(s.elf_window_trace().unwrap().admitted,frozen.admitted);
 }}
}
#[cfg(feature="sunfish-elf-window-trace")]
#[test]
fn elf_window_unpublished_survivor_needs_publication_after_receipt(){
 for clock in [false,true]{
 let(mut s,h3,h4,elf,i3,_)=setup(clock,false);assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));let first=s.elf_window_trace().unwrap().first_failure.unwrap();assert_eq!(first.count,2);assert_eq!(first.gates&25,25);
 done(&mut s,h3,i3);assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));let last=s.elf_window_trace().unwrap().last_failure.unwrap();assert_eq!((last.count,last.gates),(1,24));assert_eq!(last.entries[0].unwrap().transaction,4);
 let i4=tx(&mut s);assert!(s.reply_fully_published(&h4));queue(&mut s,&elf,b"kernel/elf_loader").unwrap();let c=s.elf_window_trace().unwrap();assert_eq!(c.receipt_count,1);assert_eq!((c.admitted.unwrap().count,c.admitted.unwrap().gates),(1,0));assert_eq!(c.last_before_admission,Some(3),"last receipt alone cannot identify publication as the last cleared condition");
 let ie=tx(&mut s);done(&mut s,h4,i4);done(&mut s,elf,ie);assert!(s.elf_window_trace().unwrap().finished);
 }
}
#[cfg(feature="sunfish-elf-window-trace")]
#[test]
fn elf_window_invalid_receipt_other_request_and_reset_keep_identity(){
 let(mut s,_,h4,elf,_,_)=setup(false,true);assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));let before=*s.elf_window_trace().unwrap();
 assert!(queue(&mut s,&h4,b"tms/servreg").is_err());assert_eq!(s.elf_window_trace().unwrap().attempts,before.attempts);
 assert_eq!(s.ingest(1,&control(14,0xffff)),Err(Error::InvalidCompletion));assert_eq!(s.elf_window_trace().unwrap().receipt_count,0);assert_eq!(s.elf_window_trace().unwrap().last_failure,before.last_failure);
 s.reset_after_transport_reset(2).unwrap();assert!(s.elf_window_trace().is_none());
}
#[cfg(feature="sunfish-elf-window-trace")]
#[test]
fn elf_window_receipt_history_keeps_opening_and_tail_across_intent_reuse(){
 let(mut s,h3,h4,elf,i3,i4)=setup(false,true);assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));
 let mut live=vec![(3,h3,i3),(4,h4,i4.unwrap())];
 for (old,new) in [(3,6),(4,7),(6,8)]{
  let n=live.iter().position(|(t,_,_)|*t==old).unwrap();let(_,h,i)=live.remove(n);done(&mut s,h,i);
  let id=offer(&mut s);let next=request(&mut s,id,new,b"tms/servreg");queue(&mut s,&next,b"tms/servreg").unwrap();let ri=tx(&mut s);live.push((new,next,ri));assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));
 }
 for(_,h,i)in live{done(&mut s,h,i);}queue(&mut s,&elf,b"kernel/elf_loader").unwrap();let i=tx(&mut s);done(&mut s,elf,i);
 let c=s.elf_window_trace().unwrap();assert_eq!((c.receipt_count,c.receipt_omitted,c.admitted.unwrap().count),(6,2,0));assert_eq!(c.receipts.map(|r|r.unwrap().transaction),[3,4,8,5]);assert_eq!(c.first_failure.unwrap().count,2);assert_eq!(c.attempts,5);assert!(c.finished);
}
#[cfg(feature="sunfish-elf-window-trace")]
#[test]
fn elf_window_reference_credit_pressure_is_not_a_same_peer_condition(){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,4096),(8,4096)])).unwrap();
 for txn in 1..=10{let id=offer(&mut s);let h=request(&mut s,id,txn,b"tms/servreg");queue(&mut s,&h,b"tms/servreg").unwrap();let i=tx(&mut s);done(&mut s,h,i);}
 let id=offer(&mut s);let h=request(&mut s,id,11,b"kernel/elf_loader");assert_eq!(queue(&mut s,&h,b"kernel/elf_loader"),Err(Error::FlowBusy));let c=s.elf_window_trace().unwrap();let f=c.first_failure.unwrap();assert_eq!((f.reason,f.count,f.gates),(4,0,0));assert!(c.admitted.is_none());assert_eq!(c.receipt_count,0);
}
#[test]
fn elf_window_protocol_bytes_unchanged(){
 let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(7,4096),(8,4096)])).unwrap();let ids=[offer(&mut s),offer(&mut s),offer(&mut s)];
 let mut bytes=Vec::new();let mut send=|s:&mut Session|{for _ in 0..16{let p=s.publish_next(1,|b|{bytes.extend_from_slice(b);Ok(())}).unwrap();if let Some(Published::Data{remote_intent,last_fragment:true})=p{return remote_intent;}}panic!("bounded data");};
 let h3=request(&mut s,ids[0],3,b"avs/audio");queue(&mut s,&h3,b"avs/audio").unwrap();let i3=send(&mut s);let h4=request(&mut s,ids[1],4,b"tms/servreg");queue(&mut s,&h4,b"tms/servreg").unwrap();let i4=send(&mut s);let elf=request(&mut s,ids[2],5,b"kernel/elf_loader");assert_eq!(queue(&mut s,&elf,b"kernel/elf_loader"),Err(Error::FlowBusy));done(&mut s,h3,i3);queue(&mut s,&elf,b"kernel/elf_loader").unwrap();let ie=send(&mut s);assert_ne!(i4,ie);done(&mut s,h4,i4);done(&mut s,elf,ie);
 print!("ELF_WINDOW_PROTOCOL_BYTES ");for b in bytes{print!("{b:02x}");}println!();
}
