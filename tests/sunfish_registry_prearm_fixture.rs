//! Observation-only pre-arm phases, using original opaque registry identities.
use super::*;
#[test]
fn registry_prearm_core_retains_first_gate_selection_and_freezes_at_arming_without_a_clock(){
    use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static NOW:AtomicU64=AtomicU64::new(100);
    for clocked in [false,true]{let(mut s,_,four,_,release,backing)=prepared(true);
        if clocked{s.set_registry_ack_clock(||NOW.load(Relaxed));}
        let q=s.registry_window_trace().target.unwrap().prearm;assert_eq!(q.queued_flags&1,0);assert_eq!(q.passes,0);
        arm(&mut s,release);let p=s.registry_window_trace().target.unwrap().prearm;
        assert_eq!(p.passes,1);assert_eq!(p.first_pass_at,clocked.then_some(100));assert_eq!(p.first_gate,Some(1));assert_ne!(p.first_flags.unwrap()&4,0);assert_eq!(p.own_done_at,None);
        let c=p.first_choice.unwrap();assert_eq!((c.command,c.parameter,c.result),(6,release,1));
        let(idc,_)=super::converted_credit::credit_for_prearm(&mut s);grant(&mut s,backing);let id=reply(&mut s);
        assert_eq!(s.registry_window_trace().target.unwrap().prearm,p);assert_eq!(s.ingest(1,&control(14,id)),Ok(Event::ReplyRxDone(four)));assert_eq!(s.ingest(1,&control(14,idc)),Ok(Event::TransmitDone));
        s.reset_after_transport_reset(2).unwrap();assert_eq!(s.registry_window_trace().target,None);
    }
}
#[test]
fn registry_prearm_core_marks_only_its_own_return_after_queue_and_stops_after_normal_publication(){
    use core::sync::atomic::{AtomicU64,Ordering::Relaxed};static NOW:AtomicU64=AtomicU64::new(10);
    let mut s=session();s.register(1,Service::ServiceRegistry,qcom_servreg::LOCAL_PORT).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.ingest(1,&intents(&[(17,4096),(18,4096)])).unwrap();
    let a=offer(&mut s);let b=offer(&mut s);let three=registry_turn_request(&mut s,a,9,3);let four=registry_turn_request(&mut s,b,9,4);
    s.queue_reply(&three,Reply::Failure{error:3}).unwrap();let(Published::Data{remote_intent:id3,..},_)=publish_until_data(&mut s)else{panic!()};
    s.set_registry_ack_clock(||NOW.load(Relaxed));s.queue_reply(&four,Reply::Failure{error:3}).unwrap();assert_ne!(s.registry_window_trace().target.unwrap().prearm.queued_flags&1,0);
    NOW.store(20,Relaxed);assert_eq!(publish(&mut s).unwrap().0,Published::ReceiveDone(b));let pre=s.registry_window_trace().target.unwrap().prearm;assert_eq!(pre.own_done_at,Some(20));assert_eq!(pre.first_choice.unwrap().parameter,b);
    NOW.store(30,Relaxed);let id=reply(&mut s);let done=s.registry_window_trace().target.unwrap().prearm;assert_eq!(done.passes,2);
    s.ingest(1,&control(14,id3)).unwrap();s.ingest(1,&control(14,id)).unwrap();let _=s.publish_next(1,|_|Ok(()));assert_eq!(s.registry_window_trace().target.unwrap().prearm,done);
}
#[test]
fn registry_prearm_core_preserves_blocked_uncertain_and_unwound_first_outcomes(){
    use std::panic::{catch_unwind,AssertUnwindSafe};
    for mode in 0..3{let(mut s,_,_,_,_,_)=prepared(false);
        if mode==2{assert!(catch_unwind(AssertUnwindSafe(||s.publish_next(1,|_|panic!("prearm outcome lost")))).is_err());}
        else{let expected=if mode==0{Error::WouldBlock}else{Error::PublicationUncertain};assert_eq!(s.publish_next(1,|_|Err(if mode==0{WriteError::WouldBlock}else{WriteError::Uncertain})),Err(expected));}
        let p=s.registry_window_trace().target.unwrap().prearm;assert_eq!(p.first_choice.unwrap().result,[2,3,4][mode]);
        if mode!=0{assert_eq!(s.publish_next(1,|_|panic!("no retry")),Err(Error::Quarantined));}
        else{let _=publish(&mut s);assert_eq!(s.registry_window_trace().target.unwrap().prearm.first_choice,p.first_choice);}
        assert_eq!(s.registry_window_trace().target.unwrap().conversion.arms,0);
    }
}
