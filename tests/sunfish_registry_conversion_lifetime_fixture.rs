//! Exact transaction4 conversion with transaction3 still outstanding and both
//! host-only registry entries occupied. Fake clocks are labels, not phone times.
use super::*;
fn prepared(confirm:bool)->(Session,RequestHandle,RequestHandle,u32,u32,u32){
    let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();
    s.ingest(1,&intents(&[(17,4096),(18,4096),(19,4096),(20,4096)])).unwrap();
    let ids:Vec<_>=(0..qcom_glink_qmi::RX_SLOTS).map(|_|offer(&mut s)).collect();
    let three=registry_turn_request(&mut s,ids[0],9,3);
    assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(ids[0]));offer(&mut s);
    s.queue_reply(&three,Reply::Failure{error:3}).unwrap();
    let(Published::Data{remote_intent:three_intent,..},_)=publish_until_data(&mut s)else{panic!()};
    let packet=qrtr(1,9,qcom_servreg::LOCAL_PORT,confirm,&[0,4,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o']);
    let Event::Request(four)=s.ingest(1,&data(9,ids[1],&packet,0)).unwrap()else{panic!()};
    assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(ids[1]));let last=offer(&mut s);
    s.queue_reply(&four,Reply::Failure{error:3}).unwrap();
    assert_eq!(s.ingest(1,&data(9,ids[2],&resume_route(0xdead_0000,STORAGE),0)),Ok(Event::CreditReceived));
    assert_eq!(s.ingest(1,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:false}));
    (s,three,four,three_intent,ids[2],last+1)
}
fn arm(s:&mut Session,release:u32){assert_eq!(publish(s).unwrap().0,Published::ReceiveDone(release));}
fn trace(s:&Session)->qcom_glink_qmi::RegistryConversionLifetime{s.registry_window_trace().target.unwrap().conversion}
fn reply(s:&mut Session)->u32{
    let(e,b)=publish(s).unwrap();let Published::Data{remote_intent,last_fragment:true}=e else{panic!("expected4: {e:?}")};
    assert_eq!(&b[49..51],&4u16.to_le_bytes());remote_intent
}
fn grant(s:&mut Session,backing:u32){assert_eq!(publish(s).unwrap().0,Published::ReceiveIntent(backing));assert_eq!(publish(s).unwrap().0,Published::IntentRequestAck{granted:true});}
#[test]
fn registry_conversion_lifetime_exact_entry_write_and_grant_are_clock_independent(){
    use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static NOW:AtomicU64=AtomicU64::new(100);
    for clocked in [false,true]{
        let(mut s,three,four,id3,release,backing)=prepared(false);
        if clocked{s.set_registry_ack_clock(||NOW.load(Relaxed));}arm(&mut s,release);
        let t=trace(&s);assert_eq!(t.arms,1);let first=t.first.unwrap();assert_eq!(first.intent,backing);assert_eq!(first.sent,0);assert_eq!(first.bytes,48);assert_eq!(first.armed_at,clocked.then_some(100));assert_eq!(first.entry,None);
        NOW.store(200,Relaxed);let id4=reply(&mut s);let first=trace(&s).first.unwrap();
        let e=first.entry.unwrap();assert_eq!(e.at,clocked.then_some(200));assert_eq!(e.gate,Some(11));assert_eq!(e.flags&0x1e3f,0x1a3f);assert_ne!(e.flags&(1<<17),0);assert_eq!(e.flags&((1<<16)|(1<<18)),0);assert_eq!(e.backing,Some(backing));assert_eq!(e.token_request,Some(first.request_intent));
        let c=first.choice.unwrap();assert_eq!((c.command,c.transaction,c.parameter,c.result),(9,Some(4),id4,1));assert_eq!(first.exit,Some((1,clocked.then_some(200))));
        NOW.store(300,Relaxed);grant(&mut s,backing);let frozen=trace(&s).first.unwrap();assert_eq!(frozen.advertised_at,clocked.then_some(300));assert_eq!(frozen.ack_at,clocked.then_some(300));
        assert_eq!(s.ingest(1,&control(14,id4)),Ok(Event::ReplyRxDone(four)));assert_eq!(trace(&s).first,Some(frozen));assert!(s.reply_fully_published(&three));assert_eq!(s.ingest(1,&control(14,id3)),Ok(Event::ReplyRxDone(three)));
        s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().target,None);NOW.store(100,Relaxed);
    }
}
#[test]
fn registry_conversion_lifetime_blocked_callback_does_not_become_success_on_retry(){
    let(mut s,_,four,_,release,backing)=prepared(false);arm(&mut s,release);
    let mut bytes=vec![];assert_eq!(s.publish_next(1,|b|{bytes.extend_from_slice(b);Err(WriteError::WouldBlock)}),Err(Error::WouldBlock));
    let first=trace(&s).first.unwrap();assert_eq!(first.choice.unwrap().result,2);assert_eq!(first.exit.unwrap().0,1);
    grant(&mut s,backing);let mut retried=vec![];let Some(Published::Data{remote_intent,..})=s.publish_next(1,|b|{retried.extend_from_slice(b);Ok(())}).unwrap()else{panic!()};assert_eq!(retried,bytes);
    assert_eq!(trace(&s).first.unwrap().choice,first.choice);assert_eq!(s.ingest(1,&control(14,remote_intent)),Ok(Event::ReplyRxDone(four)));
}
#[test]
fn registry_conversion_lifetime_credit_preempts_and_matching_advertisement_expires_token(){
    let(mut s,_,four,_,release,backing)=prepared(true);arm(&mut s,release);grant(&mut s,backing);
    let first=trace(&s).first.unwrap();assert_eq!(first.entry.unwrap().gate,Some(10));assert_ne!(first.entry.unwrap().flags&(1<<15),0);assert_eq!((first.choice.unwrap().command,first.choice.unwrap().result),(5,1));assert_eq!(first.exit.unwrap().0,2);
    let(e,b)=publish(&mut s).unwrap();let Published::Data{remote_intent:credit,..}=e else{panic!()};assert_eq!(word(&b,20),7);
    let id=reply(&mut s);assert_ne!(credit,id);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(four)));assert_eq!(trace(&s).first,Some(first));
}
#[test]
fn registry_conversion_lifetime_bounded_followup_veto_is_distinct_from_callback(){
    let(mut s,_,four,_,release,backing)=prepared(false);arm(&mut s,release);
    assert_eq!(s.publish_oack_followup(1,qcom_glink_qmi::OackFollowup::Advertise,|_|panic!("bounded veto must not call writer")),Ok(None));
    let first=trace(&s).first.unwrap();assert_eq!(first.entry.unwrap().flags&1,0);assert_eq!(first.entry.unwrap().gate,Some(9));assert_eq!((first.choice.unwrap().command,first.choice.unwrap().result),(5,0));assert_eq!(first.exit,None);
    let id=reply(&mut s);grant(&mut s,backing);let done=trace(&s).first.unwrap();assert_eq!(done.entry,first.entry);assert_eq!(done.choice,first.choice);assert_eq!(done.exit.unwrap().0,1);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(four)));
}
#[test]
fn registry_conversion_lifetime_uncertain_write_freezes_attempt_and_quarantines(){
    let(mut s,_,_,_,release,_)=prepared(false);arm(&mut s,release);
    assert_eq!(s.publish_next(1,|_|Err(WriteError::Uncertain)),Err(Error::PublicationUncertain));let t=trace(&s);assert_eq!(t.first.unwrap().choice.unwrap().result,3);
    assert_eq!(s.publish_next(1,|_|panic!("no retry after uncertainty")),Err(Error::Quarantined));assert_eq!(trace(&s),t);
    s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().target,None);
}
#[test]
fn registry_conversion_lifetime_later_arm_counts_but_cannot_rewrite_first_history(){
    let(mut s,_,four,_,release,backing)=prepared(true);arm(&mut s,release);grant(&mut s,backing);
    let first=trace(&s).first.unwrap();let(e,_)=publish(&mut s).unwrap();assert!(matches!(e,Published::Data{..})); // QRTR credit keeps its turn.
    assert_eq!(s.ingest(1,&data(9,backing,&resume_route(0xdead_0000,STORAGE),0)),Ok(Event::CreditReceived));assert_eq!(s.ingest(1,&control(7,108)),Ok(Event::ReceiveIntentRequest{granted:false}));arm(&mut s,backing);
    assert_eq!(trace(&s).arms,2);assert_eq!(trace(&s).first,Some(first));let id=reply(&mut s);grant(&mut s,backing+1);assert_eq!(trace(&s).first,Some(first));assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(four)));
}
#[test]
fn registry_conversion_lifetime_unwind_is_not_mislabeled_as_a_veto(){
    use std::panic::{catch_unwind,AssertUnwindSafe};
    let(mut s,_,_,_,release,_)=prepared(false);arm(&mut s,release);
    assert!(catch_unwind(AssertUnwindSafe(||s.publish_next(1,|_|panic!("lost callback outcome")))).is_err());
    let t=trace(&s);assert_eq!(t.first.unwrap().choice.unwrap().result,4);assert_eq!(t.first.unwrap().exit.unwrap().0,1);
    assert_eq!(s.publish_next(1,|_|panic!("unwound callback cannot retry")),Err(Error::Quarantined));assert_eq!(trace(&s),t);
    s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().target,None);
}
