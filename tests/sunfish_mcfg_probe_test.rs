//! Synthetic alternatives for the same settled MCFG counters, not phone bytes.
#[path = "../rust/src/qcom_tftp.rs"] mod qcom_tftp;
#[path = "../rust/src/qcom_tftp_server_check.rs"] mod server;
use server::{Address, Error, Route, ServerCheck, SERVICE_PORT, MISSING_REPLY};

const WRITE: &[u8] = b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0";
const STAT: &[u8] = b"\0\x01/readwrite/mcfg.tmp\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0tsize\x000\0";
const ZERO_RSIZE: &[u8] = b"\0\x01/readwrite/mcfg.tmp\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0rsize\x000\0";
const END: &[u8] = b"\0\x05\0\x09End of Transfer\0";
fn route(peer: u32, port: u32) -> Route {
    Route { source: Address { node: 0, port: peer }, destination: Address { node: 1, port } }
}
fn commit_one(s: &mut ServerCheck, out: &mut [u8]) {
    let port = s.receive(route(0x56, SERVICE_PORT), WRITE, out).unwrap().route.source.port;
    let a = s.receive(route(0x56, port), b"\0\x03\0\x01X", out).unwrap();
    assert_eq!(&out[..a.len], b"\0\x04\0\x01");
    assert_eq!(s.mcfg_progress(), (1, Some(1), 0));
}
#[test]
fn missing_at_first_read_then_committed_file_matches_settled_snapshot() {
    let mut s=ServerCheck::new();let mut out=[0;128];
    let r=s.receive(route(0x4a,SERVICE_PORT),STAT,&mut out).unwrap();
    assert_eq!(&out[..r.len],MISSING_REPLY);
    commit_one(&mut s,&mut out);
    assert_eq!(s.mcfg_progress(),(1,Some(1),0));
}
#[test]
fn successful_size_probe_ended_by_error9_has_zero_completed_reads() {
    let mut s=ServerCheck::new();let mut out=[0;128];commit_one(&mut s,&mut out);
    let r=s.receive(route(0x4a,SERVICE_PORT),STAT,&mut out).unwrap();
    assert_eq!(&out[..r.len],b"\0\x06blksize\x00512\0timeoutms\x001000\0tsize\x001\0wsize\x001\0");
    let end=s.receive(route(0x4a,r.route.source.port),END,&mut out).unwrap();
    assert_eq!(end.len,0);assert_eq!(s.mcfg_progress(),(1,Some(1),0));
    assert_eq!(s.first_rejection(),None);
}
#[test]
fn two_different_73_byte_reads_do_not_have_the_same_admission_result() {
    assert_eq!((STAT.len(),ZERO_RSIZE.len()),(73,73));
    let mut s=ServerCheck::new();let mut out=[0;128];commit_one(&mut s,&mut out);
    assert_eq!(s.receive(route(0x4a,SERVICE_PORT),ZERO_RSIZE,&mut out),Err(Error::UnexpectedRequest));
    assert_eq!(s.mcfg_progress(),(1,Some(1),0));
    let r=s.receive(route(0x4b,SERVICE_PORT),STAT,&mut out).unwrap();assert_eq!(&out[..2],b"\0\x06");
    assert_eq!(r.route.destination.port,0x4b);
}
#[test]
fn real_data_ack_completion_is_distinct_from_stat_end() {
    let mut s=ServerCheck::new();let mut out=[0;128];commit_one(&mut s,&mut out);
    let port=s.receive(route(0x4a,SERVICE_PORT),STAT,&mut out).unwrap().route.source.port;
    let d=s.receive(route(0x4a,port),b"\0\x04\0\0",&mut out).unwrap();
    assert_eq!(&out[..d.len],b"\0\x03\0\x01X");
    assert_eq!(s.receive(route(0x4a,port),b"\0\x04\0\x01",&mut out).unwrap().len,0);
    assert_eq!(s.mcfg_progress(),(1,Some(1),1));
}
#[test]
fn stat_end_is_bound_to_the_exact_transfer_peer() {
    let mut s=ServerCheck::new();let mut out=[0;128];commit_one(&mut s,&mut out);
    let port=s.receive(route(0x4a,SERVICE_PORT),STAT,&mut out).unwrap().route.source.port;
    assert_eq!(s.receive(route(0x4b,port),END,&mut out),Err(Error::WrongRoute));
    assert_eq!(s.receive(route(0x4a,port),END,&mut out).unwrap().len,0);
    assert_eq!(s.mcfg_progress(),(1,Some(1),0));
}
#[test]
fn unknown_unlink_option_is_rejected_without_changing_committed_file() {
    let mut s=ServerCheck::new();let mut out=[0;128];commit_one(&mut s,&mut out);
    let mut w=WRITE.to_vec();w.extend_from_slice(b"unlink\x000\0");
    assert_eq!(s.receive(route(0x57,SERVICE_PORT),&w,&mut out),Err(Error::McfgMalformed));
    assert_eq!(s.mcfg_progress(),(1,Some(1),0));
    let port=s.receive(route(0x4a,SERVICE_PORT),STAT,&mut out).unwrap().route.source.port;
    let r=s.receive(route(0x4a,port),b"\0\x04\0\0",&mut out).unwrap();
    assert_eq!(&out[..r.len],b"\0\x03\0\x01X");
}
#[test]
fn zero_rsize_does_not_hide_a_wlan_request_from_the_existing_observer() {
    let request=b"\0\x01/readonly/firmware/image/wlanmdsp.mbn\0octet\0rsize\x000\0";
    assert!(qcom_tftp::decode_read_request(request).is_err());
    let observed=qcom_tftp::observe_request(request).unwrap();
    assert!(observed.path_text().ends_with("/wlanmdsp.mbn"));
    assert_eq!(observed.opcode,qcom_tftp::OP_RRQ);
    assert!(observed.error.is_some());
}
