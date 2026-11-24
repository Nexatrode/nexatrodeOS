//! Actual ready Resume callback at the exact converted transaction4 opportunity.
use super::*;
fn credit(s:&mut Session)->(u32,Vec<u8>){
    let(e,b)=publish(s).unwrap();let Published::Data{remote_intent,last_fragment:true}=e else{panic!("credit expected: {e:?}")};
    assert_eq!(word(&b,20),7);assert_eq!(word(&b,8),52);(remote_intent,b)
}
fn mask(s:&Session)->u32{s.registry_window_trace().target.unwrap().publication.unwrap().mask}
#[test]
fn converted_credit_commits_before_grant_and_original_registry_receipts_stay_distinct(){
    let(mut s,three,four,id3,release,backing)=prepared(true);arm(&mut s,release);
    let(idc,b)=credit(&mut s);assert_eq!(word(&b,28),qcom_servreg::LOCAL_PORT);assert_eq!(word(&b,44),9);
    let first=trace(&s).first.unwrap();assert_eq!(first.exit.unwrap().0,5);assert_eq!(first.choice.unwrap().command,9);assert_eq!(first.choice.unwrap().transaction,None);assert_eq!(first.credit_ports,Some((qcom_servreg::LOCAL_PORT<<16)|9));assert_eq!(first.credit_nodes,Some(1<<16));assert_eq!(first.packed_len_sent,48<<16);assert!(first.length_consistent);assert_ne!(first.entry.unwrap().flags&(1<<19),0);assert_ne!(first.entry.unwrap().flags&(1<<20),0);
    assert_eq!(mask(&s)&(3<<17),3<<17);assert!(!s.reply_fully_published(&four));grant(&mut s,backing);let id4=reply(&mut s);
    assert_ne!(idc,id4);assert_ne!(id3,id4);
    assert_eq!(s.ingest(1,&control(14,idc)),Ok(Event::TransmitDone));assert!(s.reply_fully_published(&three));assert!(s.reply_fully_published(&four));
    assert_eq!(s.ingest(1,&control(14,id4)),Ok(Event::ReplyRxDone(four)));assert!(s.reply_fully_published(&three));assert_eq!(s.ingest(1,&control(14,id3)),Ok(Event::ReplyRxDone(three)));
}
#[test]
fn converted_credit_blocked_attempt_spends_only_credit_loan_and_keeps_byte_identical_fallback(){
    let(mut s,_,four,_,release,backing)=prepared(true);arm(&mut s,release);let mut blocked=vec![];
    assert_eq!(s.publish_next(1,|b|{assert_eq!(word(b,20),7);blocked.extend_from_slice(b);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));assert_eq!(mask(&s)&(3<<17),1<<17);assert_eq!(trace(&s).first.unwrap().choice.unwrap().result,2);
    grant(&mut s,backing);let(_,b)=credit(&mut s);assert_eq!(b,blocked);assert_eq!(mask(&s)&(3<<17),1<<17,"ordinary fallback is not a committed borrowed callback");let id4=reply(&mut s);assert_eq!(s.ingest(1,&control(14,id4)),Ok(Event::ReplyRxDone(four)));
}
#[test]
fn converted_credit_uncertain_or_unwound_callback_quarantines_and_reset_drops_identity(){
    use std::panic::{catch_unwind,AssertUnwindSafe};
    for unwind in [false,true]{let(mut s,_,four,_,release,_)=prepared(true);arm(&mut s,release);
        if unwind{assert!(catch_unwind(AssertUnwindSafe(||s.publish_next(1,|_|panic!("credit outcome lost")))).is_err());}
        else{assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));}
        let t=trace(&s);assert_eq!(t.first.unwrap().choice.unwrap().result,if unwind{4}else{3});assert_eq!(mask(&s)&(3<<17),1<<17);assert_eq!(s.publish_next(1,|_|panic!("no retry")),Err(Error::Quarantined));
        s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().target,None);assert_eq!(s.queue_reply(&four,Reply::Failure{error:3}),Err(Error::StaleGeneration));
    }
}
#[test]
fn converted_credit_bounded_followup_cannot_spend_ordinary_opportunity(){
    let(mut s,_,four,_,release,backing)=prepared(true);arm(&mut s,release);
    assert_eq!(s.publish_oack_followup(1,qcom_glink_qmi::OackFollowup::Advertise,|_|panic!("veto")),Ok(None));assert_eq!(mask(&s)&(3<<17),0);let first=trace(&s).first.unwrap();assert_eq!(first.choice.unwrap().result,0);
    credit(&mut s);assert_eq!(mask(&s)&(3<<17),3<<17);assert_eq!(trace(&s).first.unwrap().choice,first.choice);assert_eq!(trace(&s).first.unwrap().exit.unwrap().0,5);grant(&mut s,backing);let id=reply(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(four)));
}
#[test]
fn converted_credit_requires_both_normal_registry_and_credit_fit(){
    let(mut s,_,four,_,release,backing)=prepared_credit_capacity(false,48,Reply::Failure{error:3});
    let extra=qrtr(1,11,qcom_servreg::LOCAL_PORT,true,&[0,7,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o']);
    assert!(matches!(s.ingest(1,&data(9,release+1,&extra,0)),Ok(Event::Request(_))));arm(&mut s,release);
    let id=reply(&mut s);assert_eq!(mask(&s)&(3<<17),0);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(four)));grant(&mut s,backing);
    let mut tlvs=vec![0x10];tlvs.extend(60u16.to_le_bytes());tlvs.extend(vec![0;60]);
    let(mut s,_,four,_,release,backing)=prepared_credit_capacity(true,64,Reply::Success{optional_tlvs:&tlvs});arm(&mut s,release);
    assert_eq!(trace(&s).arms,0,"credit cannot inherit an unfit registry reply opportunity");grant(&mut s,backing);credit(&mut s);assert!(!s.reply_fully_published(&four));assert_eq!(mask(&s)&(3<<17),0);
}
#[test]
fn converted_credit_is_one_attempt_per_original_registry_reply_even_with_two_ready_credits(){
    let(mut s,_,four,_,release,backing)=prepared(true);
    let extra=qrtr(1,11,qcom_servreg::LOCAL_PORT,true,&[0,7,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o']);
    assert!(matches!(s.ingest(1,&data(9,release+1,&extra,0)),Ok(Event::Request(_))));
    arm(&mut s,release);credit(&mut s);grant(&mut s,backing);
    s.ingest(1,&data(9,backing,&resume_route(0xdead_0000,STORAGE),0)).unwrap();assert_eq!(s.ingest(1,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:false}));arm(&mut s,backing);
    assert_eq!(trace(&s).arms,2);grant(&mut s,backing+1); // second credit cannot borrow this grant
    let mut seen=false;
    for _ in 0..8{let(e,b)=publish(&mut s).unwrap();if let Published::Data{..}=e{assert_eq!(word(&b,20),7);assert_eq!(word(&b,44),11);seen=true;break;}}
    assert!(seen);let id=reply(&mut s);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(four)));assert_eq!(mask(&s)&(3<<17),3<<17);
}

#[cfg(feature="sunfish-registry-prearm-io-trace")]
pub(super) fn credit_for_prearm(s:&mut Session)->(u32,Vec<u8>){credit(s)}
