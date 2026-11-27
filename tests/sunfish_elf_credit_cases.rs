use super::*;
#[test]fn elf_credit_pair_preserves_credit_before_reply_and_ad_before_ack(){
 for before in [false,true]{for clocked in [false,true]{let(mut s,h,_)=first_elf(before,true,4096);if clocked{s.set_registry_ack_clock(||55);}
 let(cid,b)=reply(&mut s);assert_eq!(word(&b,20),7);assert_eq!(elf_publication(&s).mask&0x60000,0x60000);
 let(id,b)=reply(&mut s);assert_eq!(word(&b,20),1);assert_eq!(elf_publication(&s).mask&0x1e0000,0x1e0000);grant(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));assert!(s.ingest(1,&control(14,cid)).is_ok());}}
}
#[test]fn elf_credit_pair_blocked_credit_falls_back_to_ad_ack_without_data_loan(){
 let(mut s,h,_)=first_elf(true,true,4096);let mut original=vec![];assert_eq!(s.publish_next(1,|b|{original=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));assert_eq!(word(&original,20),7);assert_eq!(elf_publication(&s).mask&0x1e0000,0x20000);
 grant(&mut s);let(_,b)=reply(&mut s);assert_eq!(b,original);let(id,_)=reply(&mut s);assert_eq!(elf_publication(&s).mask&0x1e0000,0x20000);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn elf_credit_pair_blocked_reply_spends_both_stages_and_retries_identically(){
 let(mut s,h,_)=first_elf(true,true,4096);let _=reply(&mut s);let mut original=vec![];assert_eq!(s.publish_next(1,|b|{original=b.to_vec();Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));assert_eq!(elf_publication(&s).mask&0x1e0000,0xe0000);grant(&mut s);s.ingest(1,&control(7,108)).unwrap();grant(&mut s);let(id,b)=reply(&mut s);assert_eq!(b,original);assert_eq!(elf_publication(&s).mask&0x1e0000,0xe0000);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(h)));
}
#[test]fn elf_credit_pair_uncertain_or_unwound_stage_quarantines(){
 for stage in [0,1]{for panic_write in [false,true]{let(mut s,_,_)=first_elf(true,true,4096);if stage==1{let _=reply(&mut s);}
 if panic_write{assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||s.publish_next(1,|_|panic!("pair fault")))).is_err());}else{assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));}assert_eq!(s.publish_next(1,|_|panic!("no use after uncertain")),Err(Error::Quarantined));s.reset_after_transport_reset(2).unwrap();assert!(s.transport_is_pristine());}}
}
