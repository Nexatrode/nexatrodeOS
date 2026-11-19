//! Synthetic call/state tests; no hardware acceptance or modem readiness claim.
extern crate self as personal_os_rust_server;
#[path="../rust/src/qcom_mdt.rs"] mod qcom_mdt;
#[path="../rust/src/qcom_pas_metadata.rs"] mod qcom_pas_metadata;
#[path="../rust/src/qcom_pas_init.rs"] mod qcom_pas_init;
#[path="../rust/src/qcom_pas_mem_setup.rs"] mod qcom_pas_mem_setup;
#[path="../rust/src/qcom_modem_image.rs"] mod qcom_modem_image;
#[path="../rust/src/qcom_pas_auth.rs"] mod qcom_pas_auth;
use qcom_pas_auth::{AuthOnce,Error,Phase};
use qcom_pas_init::{Request,Transport};
use qcom_pas_metadata::Prepared;
#[derive(Default)]
struct Fake { calls:Vec<[u64;8]>, fault:Option<(usize,Result<[u64;8],u64>)> }
impl Transport for Fake {
    fn invoke(&mut self,r:&Request)->Result<[u64;8],u64>{
        let n=self.calls.len();self.calls.push(*r.words());
        if let Some((i,value))=self.fault { if i==n { return value; } }
        Ok([0,u64::from(n<2),0x123456789abcdef0,3,4,5,6,7])
    }
}
fn prepared()->Prepared { Prepared { metadata_physical:0x80001000,metadata_length:9100,
    program_headers:31,loadable_segments:29,linked_start:0x8b000000,linked_end:0x94800000 } }
fn session()->AuthOnce { AuthOnce::after_image(qcom_modem_image::tests::auth_fixture(true),prepared(),&qcom_pas_mem_setup::tests::consumed()).unwrap() }
#[test]
fn exact_three_envelopes_one_auth_and_no_shutdown_or_arbitrary_arguments(){
    let mut s=session();let mut io=Fake::default();let mut phases=Vec::new();
    s.check(&mut io,|r|{assert!(r.issued_mask!=0);phases.push(r.phase);}).unwrap();
    assert!(!s.report().passed());
    s.authenticate(&mut io,&qcom_pas_mem_setup::tests::consumed(),|r|phases.push(r.phase)).unwrap();
    assert!(s.report().passed());
    assert_eq!(io.calls,vec![[0x42000601,1,0x02000205,0,0,0,0,0],
                           [0x42000601,1,0x02000206,0,0,0,0,0],
                           [0x42000205,1,4,0,0,0,0,0]]);
    assert_eq!(phases,vec![Phase::QueryAuth,Phase::QueryShutdown,Phase::Calling]);
    assert_eq!(s.report().replies[2].unwrap()[2],0x123456789abcdef0);
    assert_eq!(s.authenticate(&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}),Err(Error::WrongStage));
    assert_eq!(io.calls.len(),3);
}
#[test]
fn incomplete_image_and_changed_post_metadata_cannot_construct_an_auth_session(){
    assert!(matches!(AuthOnce::after_image(qcom_modem_image::tests::auth_fixture(false),prepared(),&qcom_pas_mem_setup::tests::consumed()),Err(Error::Image)));
    let mut fresh=qcom_pas_mem_setup::tests::consumed();fresh.unstable_bytes=1;
    assert!(matches!(AuthOnce::after_image(qcom_modem_image::tests::auth_fixture(true),prepared(),&fresh),Err(Error::Metadata)));
    let mut p=prepared();p.linked_end+=4096;
    assert!(matches!(AuthOnce::after_image(qcom_modem_image::tests::auth_fixture(true),p,&qcom_pas_mem_setup::tests::consumed()),Err(Error::Metadata)));
}
#[test]
fn ordering_and_fresh_metadata_are_terminal_gates_before_any_auth_call(){
    let mut s=session();let mut io=Fake::default();
    assert_eq!(s.authenticate(&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}),Err(Error::WrongStage));
    assert!(io.calls.is_empty());assert!(s.check(&mut io,|_|{}).is_err());
    let mut s=session();s.check(&mut io,|_|{}).unwrap();
    let mut fresh=qcom_pas_mem_setup::tests::consumed();fresh.high_guard_bad=1;
    assert_eq!(s.authenticate(&mut io,&fresh,|_|{}),Err(Error::Metadata));
    assert_eq!(io.calls.len(),2);
    assert!(s.authenticate(&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}).is_err());
    assert_eq!(io.calls.len(),2);
}
#[test]
fn every_call_fault_is_terminal_no_busy_retry_or_interrupted_resume(){
    for index in 0..3 {
        for transport in [Err(6),Ok([1,0,2,3,4,5,6,7]),Ok([u64::MAX-11,0,2,3,4,5,6,7]),Ok([0,2,2,3,4,5,6,7])] {
            let mut s=session();let mut io=Fake {calls:vec![],fault:Some((index,transport))};
            let result=s.check(&mut io,|_|{}).and_then(|()|s.authenticate(&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}));
            assert!(result.is_err());assert!(!s.report().passed());assert_eq!(io.calls.len(),index+1);
            let returned=if transport.is_err(){(1<<index)-1}else{(1<<(index+1))-1};
            assert_eq!(s.report().returned_mask,returned);
            assert!(s.check(&mut io,|_|{}).is_err());
            assert!(s.authenticate(&mut io,&qcom_pas_mem_setup::tests::consumed(),|_|{}).is_err());
            assert_eq!(io.calls.len(),index+1);
        }
    }
}
#[test]
fn status_permit_requires_both_info_replies_fresh_metadata_and_is_one_shot(){
    let fresh=qcom_pas_mem_setup::tests::consumed();let mut io=Fake::default();
    let mut s=session();assert!(matches!(s.status_prepare(&fresh),Err(Error::WrongStage)));
    assert!(s.check(&mut io,|_|{}).is_err());assert!(io.calls.is_empty());
    let mut s=session();s.check(&mut io,|_|{}).unwrap();let _permit=s.status_prepare(&fresh).unwrap();
    assert_eq!(io.calls.len(),2);assert!(matches!(s.status_prepare(&fresh),Err(Error::WrongStage)));
    assert!(s.authenticate(&mut io,&fresh,|_|{}).is_err());assert_eq!(io.calls.len(),2);
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();
    let mut changed=fresh;changed.unstable_bytes=1;
    assert!(matches!(s.status_prepare(&changed),Err(Error::Metadata)));assert_eq!(io.calls.len(),2);
    assert!(s.authenticate(&mut io,&fresh,|_|{}).is_err());assert_eq!(io.calls.len(),2);
}
#[test]
fn prepared_status_observer_requires_the_permitted_session_to_return_successfully(){
    let fresh=qcom_pas_mem_setup::tests::consumed();let mut io=Fake::default();
    let mut s=session();s.check(&mut io,|_|{}).unwrap();let _permit=s.status_prepare(&fresh).unwrap();
    s.authenticate(&mut io,&fresh,|_|{}).unwrap();assert!(s.into_status_returned().unwrap().passed());assert_eq!(io.calls.len(),3);
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();
    s.authenticate(&mut io,&fresh,|_|{}).unwrap();assert!(s.into_status_returned().is_err());
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();let _permit=s.status_prepare(&fresh).unwrap();
    assert!(s.into_status_returned().is_err());assert_eq!(io.calls.len(),2);
}
#[test]
fn glink_memory_permit_consumes_only_the_fresh_available_status_permitted_session(){
    let fresh=qcom_pas_mem_setup::tests::consumed();let mut io=Fake::default();
    assert!(matches!(session().into_glink_memory_preparation(&fresh),Err(Error::WrongStage)));
    let mut s=session();s.check(&mut io,|_|{}).unwrap();assert!(matches!(s.into_glink_memory_preparation(&fresh),Err(Error::WrongStage)));
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();let _status=s.status_prepare(&fresh).unwrap();
    let count=io.calls.len();let _memory=s.into_glink_memory_preparation(&fresh).unwrap();assert_eq!(io.calls.len(),count);
    assert!(io.calls.iter().all(|r|r[0]==0x42000601));
}
#[test]
fn glink_memory_permit_rejects_changed_metadata_and_any_post_auth_session(){
    let fresh=qcom_pas_mem_setup::tests::consumed();let mut io=Fake::default();
    let mut s=session();s.check(&mut io,|_|{}).unwrap();let _status=s.status_prepare(&fresh).unwrap();
    let mut bad=fresh;bad.unstable_bytes=1;
    assert!(matches!(s.into_glink_memory_preparation(&bad),Err(Error::Metadata)));assert_eq!(io.calls.len(),2);
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();let _status=s.status_prepare(&fresh).unwrap();s.authenticate(&mut io,&fresh,|_|{}).unwrap();
    assert!(matches!(s.into_glink_memory_preparation(&fresh),Err(Error::WrongStage)));
}
#[test]
fn boot_memory_grant_is_fresh_one_shot_and_cannot_be_reissued_as_standalone(){
    let fresh=qcom_pas_mem_setup::tests::consumed();let mut io=Fake::default();let mut s=session();
    s.check(&mut io,|_|{}).unwrap();let _status=s.status_prepare(&fresh).unwrap();
    let _memory=s.glink_boot_memory_prepare(&fresh).unwrap();assert_eq!(io.calls.len(),2);
    assert!(matches!(s.into_glink_memory_preparation(&fresh),Err(Error::WrongStage)));
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();let _status=s.status_prepare(&fresh).unwrap();
    let _memory=s.glink_boot_memory_prepare(&fresh).unwrap();assert!(matches!(s.glink_boot_memory_prepare(&fresh),Err(Error::WrongStage)));
    assert!(s.authenticate(&mut io,&fresh,|_|{}).is_err());assert_eq!(io.calls.len(),2);
}
#[test]
fn boot_memory_grant_rejects_premature_changed_or_post_auth_sessions(){
    let fresh=qcom_pas_mem_setup::tests::consumed();assert!(session().glink_boot_memory_prepare(&fresh).is_err());
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();
    assert!(s.glink_boot_memory_prepare(&fresh).is_err());assert_eq!(io.calls.len(),2);
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();let _status=s.status_prepare(&fresh).unwrap();
    let mut bad=fresh;bad.low_guard_bad=1;assert!(matches!(s.glink_boot_memory_prepare(&bad),Err(Error::Metadata)));assert_eq!(io.calls.len(),2);
    let mut io=Fake::default();let mut s=session();s.check(&mut io,|_|{}).unwrap();let _status=s.status_prepare(&fresh).unwrap();s.authenticate(&mut io,&fresh,|_|{}).unwrap();
    assert!(s.glink_boot_memory_prepare(&fresh).is_err());assert_eq!(io.calls.len(),3);
}
