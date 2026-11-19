#[path = "../rust/src/qcom_mcfg_ram.rs"]
mod m;
use m::{CAPACITY, Error, McfgRam};
#[test]
fn lifecycle_and_peer_isolation() {
    let mut f = McfgRam::new();
    let mut out = [0; 8];
    assert_eq!(f.read(0, &mut out), Err(Error::NoFile));
    f.begin(1).unwrap();
    f.begin(1).unwrap();
    assert_eq!(f.begin(2), Err(Error::Busy));
    f.write_block(1, 1, b"hello").unwrap();
    f.write_block(1, 1, b"hello").unwrap();
    assert_eq!(f.write_block(1, 1, b"other"), Err(Error::ChangedRetry));
    assert_eq!(f.write_block(2, 2, b"x"), Err(Error::NoWriter));
    assert_eq!(f.read(0, &mut out), Err(Error::NoFile));
    f.finish(1).unwrap();
    assert_eq!(f.read(0, &mut out), Ok(5));
    assert_eq!(&out[..5], b"hello");
    assert_eq!(f.read(4, &mut out), Ok(1));
    assert_eq!(out[0], b'o');
    assert_eq!(f.read(6, &mut out), Err(Error::BadOffset));
    f.begin(2).unwrap();
    f.write_block(2, 1, b"new").unwrap();
    f.abort(2).unwrap();
    assert_eq!(f.read(0, &mut out), Ok(5));
    assert_eq!(&out[..5], b"hello");
}
#[test]
fn capacity_and_order_fail_without_mutation() {
    let mut f = McfgRam::new();
    f.begin(7).unwrap();
    assert_eq!(f.write_block(7, 2, b"x"), Err(Error::OutOfOrder));
    f.write_block(7, 1, &[3; CAPACITY]).unwrap();
    assert_eq!(f.write_block(7, 2, b"x"), Err(Error::Capacity));
    f.finish(7).unwrap();
    let mut out = [0; 1];
    assert_eq!(f.read(CAPACITY - 1, &mut out), Ok(1));
    assert_eq!(out[0], 3);
    assert_eq!(f.read(CAPACITY, &mut out), Ok(0));
}
#[test]
fn replacement_is_visible_only_after_finish() {
    let mut f = McfgRam::new();
    let mut out = [0; 4];
    f.begin(1).unwrap();
    f.write_block(1, 1, b"old").unwrap();
    f.finish(1).unwrap();
    f.begin(2).unwrap();
    f.write_block(2, 1, b"new").unwrap();
    assert_eq!(f.read(0, &mut out), Ok(3));
    assert_eq!(&out[..3], b"old");
    f.finish(2).unwrap();
    assert_eq!(f.read(0, &mut out), Ok(3));
    assert_eq!(&out[..3], b"new");
}
#[test]
fn bounded_negotiation_rejects_unproven_or_malformed_options() {
    let wrq = b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0";
    assert!(m::decode_write_request(wrq).is_ok());
    assert_eq!(
        m::WRITE_OACK,
        b"\0\x06blksize\x00512\x00timeoutms\x001000\x00wsize\x001\x00"
    );
    assert!(m::decode_write_request(&wrq[..60]).is_err());
    assert!(
        m::decode_write_request(
            b"\0\x02/readwrite/other\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0"
        )
        .is_err()
    );
    assert!(m::decode_write_request(b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0wsize\01\0").is_err());
}
#[test]
fn wire_write_and_final_ack_retry() {
    let request = b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0";
    let mut f = McfgRam::new();
    let mut out = [0; 128];
    let (mut w, n) = m::WriteTransfer::start(&mut f, 0x54, request, &mut out).unwrap();
    assert_eq!(&out[..n], m::WRITE_OACK);
    assert_eq!(
        w.data(&mut f, 0x55, b"\0\x03\0\x01hello", &mut out),
        Err(Error::NoWriter)
    );
    assert_eq!(w.data(&mut f, 0x54, b"\0\x03\0\x01hello", &mut out), Ok(4));
    assert_eq!(&out[..4], b"\0\x04\0\x01");
    assert_eq!(w.data(&mut f, 0x54, b"\0\x03\0\x01hello", &mut out), Ok(4));
    assert_eq!(
        w.data(&mut f, 0x54, b"\0\x03\0\x01other", &mut out),
        Err(Error::ChangedRetry)
    );
    let mut contents = [0; 8];
    assert_eq!(f.read(0, &mut contents), Ok(5));
    assert_eq!(&contents[..5], b"hello");
}
#[test]
fn wire_output_bounds_do_not_mutate() {
    let request = b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\07680\0timeoutms\01000\0wsize\010\0";
    let mut f = McfgRam::new();
    let mut short = [0; 3];
    assert!(matches!(
        m::WriteTransfer::start(&mut f, 1, request, &mut short),
        Err(Error::OutputTooSmall)
    ));
    let mut out = [0; 128];
    let (mut w, _) = m::WriteTransfer::start(&mut f, 2, request, &mut out).unwrap();
    assert_eq!(
        w.data(&mut f, 2, b"\0\x03\0\x01x", &mut short),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(w.data(&mut f, 2, b"\0\x03\0\x01x", &mut out), Ok(4));
}
