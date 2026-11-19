#[path = "../rust/src/qcom_qrtr_tftp.rs"]
mod qcom_qrtr_tftp;
#[path = "../rust/src/qcom_tftp.rs"]
mod qcom_tftp;
#[path = "../rust/src/qcom_tftp_server_check.rs"]
mod qcom_tftp_server_check;
#[path = "../rust/src/qcom_tftp_mux.rs"]
mod qcom_tftp_mux;

use qcom_qrtr_tftp::{
    AuthorizedWlanImage, CloseReason, ConfigError, Dispatch, IgnoreReason, IncomingRoute, Peer,
    ReplyRoute, Server as QrtrTftpServer, MAX_ACTIVE_TRANSFERS,
};
use qcom_tftp::{
    decode_ack, decode_error, decode_read_request, encode_data, encode_error, encode_oack,
    error_code, AckOutcome, Error, ReadPlan, ReadSession, ReadSource, ReadSourceCell, ReadSourceIo,
    RemoteErrorOutcome, Resource, TransferProgress, ERROR_ACCESS, ERROR_END_OF_TRANSFER,
    ERROR_ILLEGAL_OPERATION, ERROR_OPTION_NEGOTIATION, OP_ACK, OP_DATA, OP_ERROR, OP_OACK,
};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

struct StreamingSource {
    bytes: Vec<u8>,
    fail_at: Option<u64>,
    reads: RefCell<Vec<(u64, usize)>>,
}

impl ReadSource for StreamingSource {
    fn len(&self) -> Result<u64, ()> {
        Ok(self.bytes.len() as u64)
    }

    fn read_exact_at(&self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        self.reads.borrow_mut().push((offset, out.len()));
        if self.fail_at == Some(offset) {
            return Err(());
        }
        let start = usize::try_from(offset).map_err(|_| ())?;
        let end = start.checked_add(out.len()).ok_or(())?;
        out.copy_from_slice(self.bytes.get(start..end).ok_or(())?);
        Ok(())
    }
}

impl StreamingSource {
    fn new(bytes: Vec<u8>, fail_at: Option<u64>) -> Self {
        Self {
            bytes,
            fail_at,
            reads: RefCell::new(Vec::new()),
        }
    }
}

struct MutableLengthSource(Cell<u64>);

impl ReadSource for MutableLengthSource {
    fn len(&self) -> Result<u64, ()> {
        Ok(self.0.get())
    }

    fn read_exact_at(&self, _offset: u64, out: &mut [u8]) -> Result<(), ()> {
        out.fill(0);
        Ok(())
    }
}

struct MutableSource {
    bytes: Vec<u8>,
    reads: Rc<RefCell<Vec<(u64, usize)>>>,
    grow_after_read: bool,
    len_calls: usize,
    grow_on_len_call: Option<usize>,
}

impl ReadSourceIo for MutableSource {
    fn len(&mut self) -> Result<u64, ()> {
        self.len_calls += 1;
        if self.grow_on_len_call == Some(self.len_calls) {
            self.bytes.push(0xff);
        }
        Ok(self.bytes.len() as u64)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        self.reads.borrow_mut().push((offset, out.len()));
        let start = usize::try_from(offset).map_err(|_| ())?;
        let end = start.checked_add(out.len()).ok_or(())?;
        out.copy_from_slice(self.bytes.get(start..end).ok_or(())?);
        if self.grow_after_read {
            self.bytes.push(0xff);
        }
        Ok(())
    }
}

fn request(path: &[u8], mode: &[u8], options: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    bytes.extend_from_slice(path);
    bytes.push(0);
    bytes.extend_from_slice(mode);
    bytes.push(0);
    for (name, value) in options {
        bytes.extend_from_slice(name);
        bytes.push(0);
        bytes.extend_from_slice(value);
        bytes.push(0);
    }
    bytes
}

#[test]
fn only_firmware_observed_wlan_aliases_are_readable() {
    for path in [
        b"/readonly/firmware/image/wlanmdsp.mbn".as_slice(),
        b"/readonly/vendor/firmware_mnt/image/wlanmdsp.mbn".as_slice(),
        b"/readonly/vendor/firmware/wlanmdsp.mbn".as_slice(),
    ] {
        let parsed = decode_read_request(&request(path, b"octet", &[])).unwrap();
        assert_eq!(parsed.resource, Resource::WlanMdspMbn);
        assert!(!parsed.options.has_supported_options());
    }

    for path in [
        b"/readonly/firmware/image/../wlanmdsp.mbn".as_slice(),
        b"/readonly/firmware/image/modem.mbn".as_slice(),
        b"/readwrite/wlanmdsp.mbn".as_slice(),
        b"/etc/passwd".as_slice(),
    ] {
        assert_eq!(
            decode_read_request(&request(path, b"octet", &[])),
            Err(Error::UnavailablePath)
        );
    }
}

#[test]
fn observer_keeps_only_sanitized_rrq_metadata_and_never_a_packet_payload() {
    let bytes = request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"OcTeT",
        &[
            (b"blksize", b"7680"),
            (b"timeoutms", b"1000"),
            (b"tsize", b"0"),
            (b"wsize", b"10"),
        ],
    );
    let observed = qcom_tftp::observe_request(&bytes).unwrap();
    assert_eq!(observed.opcode, 1);
    assert_eq!(
        observed.path_text(),
        "/readonly/firmware/image/wlanmdsp.mbn"
    );
    assert_eq!(observed.path_part(0), "/readonly/firmware/image/wlanmds");
    assert_eq!(observed.path_part(1), "p.mbn");
    assert_eq!(observed.mode_text(), "OcTeT");
    assert_eq!(observed.resource, Some(Resource::WlanMdspMbn));
    let options = observed.options.unwrap();
    assert_eq!(options.block_size, 7680);
    assert_eq!(options.timeout_ms, 1000);
    assert_eq!(options.window_size, 10);
    assert!(options.request_file_size);
    assert_eq!(options.offered_mask(), 0b0001_0111);
    assert_eq!(observed.error, None);
    assert!(std::mem::size_of_val(&observed) < qcom_tftp::MAX_REQUEST_BYTES);
}

#[test]
fn observer_preserves_denied_path_and_write_opcode_without_authorizing_them() {
    let denied = request(
        b"/readonly/firmware/image/modem.mbn",
        b"octet",
        &[(b"rsize", b"4096")],
    );
    let observed = qcom_tftp::observe_request(&denied).unwrap();
    assert_eq!(observed.opcode, 1);
    assert_eq!(observed.path_text(), "/readonly/firmware/image/modem.mbn");
    assert_eq!(observed.error, Some(Error::UnavailablePath));
    assert_eq!(observed.resource, None);
    assert_eq!(observed.options.unwrap().read_size, Some(4096));

    let mut write = request(b"/readwrite/probe", b"octet", &[(b"blksize", b"512")]);
    write[1] = 2;
    let observed = qcom_tftp::observe_request(&write).unwrap();
    assert_eq!(observed.opcode, 2);
    assert_eq!(observed.path_text(), "/readwrite/probe");
    assert_eq!(observed.mode_text(), "octet");
    assert_eq!(observed.option_bytes, 12);
    assert_eq!(observed.option_prefix_len, 12);
    assert_eq!(&observed.option_prefix[..12], b"blksize\x00512\x00");
    assert_eq!(observed.error, Some(Error::WriteDenied));
    assert_eq!(observed.resource, None);
}

#[test]
fn observer_bounds_text_and_does_not_copy_other_tftp_payloads() {
    let data = [0, OP_DATA as u8, 0, 1, 9, 8, 7, 6];
    let observed = qcom_tftp::observe_request(&data).unwrap();
    assert_eq!(observed.opcode, OP_DATA);
    assert_eq!(observed.path_len, 0);
    assert_eq!(observed.mode_len, 0);
    assert_eq!(observed.options, None);
    assert_eq!(observed.option_prefix_len, 0);
    assert_eq!(observed.option_prefix, [0; 40]);
    assert_eq!(observed.error, Some(Error::UnsupportedOpcode));

    let long_options = vec![b'7'; 60];
    let mut write = request(b"/readwrite/probe", b"octet", &[(b"test", &long_options)]);
    write[1] = qcom_tftp::OP_WRQ as u8;
    let observed = qcom_tftp::observe_request(&write).unwrap();
    assert_eq!(observed.option_bytes, 66);
    assert_eq!(observed.option_prefix_len, 40);
    assert_eq!(&observed.option_prefix[..5], b"test\x00");
    assert_eq!(observed.error, Some(Error::WriteDenied));

    let too_long = request(
        &vec![b'a'; qcom_tftp::MAX_OBSERVED_PATH_BYTES + 1],
        b"octet",
        &[],
    );
    assert_eq!(
        qcom_tftp::observe_request(&too_long),
        Err(Error::PacketTooLarge)
    );

    let non_ascii = request(b"/readonly/firmware/image/a\xff.mbn", b"octet", &[]);
    let observed = qcom_tftp::observe_request(&non_ascii).unwrap();
    assert_eq!(observed.path_text(), "/readonly/firmware/image/a?.mbn");
}

#[test]
fn read_request_parses_reference_modem_options_and_encodes_bounded_oack() {
    let bytes = request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"OcTeT",
        &[
            (b"blksize", b"7680"),
            (b"timeoutms", b"1000"),
            (b"tsize", b"0"),
            (b"wsize", b"10"),
        ],
    );
    let parsed = decode_read_request(&bytes).unwrap();
    assert_eq!(parsed.options.block_size, 7680);
    assert_eq!(parsed.options.timeout_ms, 1000);
    assert_eq!(parsed.options.window_size, 10);
    assert!(parsed.options.request_file_size);

    let plan = ReadPlan::new(parsed, 20_000).unwrap();
    assert_eq!(plan.block_size, qcom_tftp::MAX_BLOCK_SIZE);
    let mut out = [0; 128];
    let len = encode_oack(parsed, plan, &mut out).unwrap();
    assert_eq!(&out[..2], &OP_OACK.to_be_bytes());
    assert_eq!(
        &out[..len],
        b"\0\x06blksize\04060\0timeoutms\01000\0tsize\020000\0wsize\010\0"
    );
}

#[test]
fn size_probe_oack_can_end_with_nonfailure_transfer_marker() {
    let parsed = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"tsize", b"0"), (b"timeoutms", b"1000"), (b"wsize", b"10")],
    ))
    .unwrap();
    let source = b"synthetic immutable WLAN image";
    let mut session = ReadSession::new(parsed, source).unwrap();
    let mut packet = [0; 128];

    let TransferProgress::Packet { bytes, ack_after } = session.next_packet(&mut packet).unwrap()
    else {
        panic!("expected size OACK");
    };
    assert_eq!(ack_after, Some(0));
    assert!(packet[..bytes]
        .windows(6)
        .any(|window| window == b"tsize\0"));

    let mut end = [0; 64];
    let end_len = encode_error(ERROR_END_OF_TRANSFER, b"End of Transfer", &mut end).unwrap();
    assert_eq!(
        decode_error(&end[..end_len]),
        Ok((ERROR_END_OF_TRANSFER, b"End of Transfer".as_slice()))
    );
    assert_eq!(
        session.receive_error(&end[..end_len]),
        Ok(RemoteErrorOutcome::EndOfTransfer)
    );
    assert_eq!(
        session.next_packet(&mut packet),
        Ok(TransferProgress::Complete)
    );
}

#[test]
fn ordinary_tftp_error_terminates_as_failure_and_malformed_errors_do_not_advance() {
    let parsed = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[],
    ))
    .unwrap();
    let mut session = ReadSession::new(parsed, b"synthetic image").unwrap();
    let mut packet = [0; 64];
    session.next_packet(&mut packet).unwrap();

    assert_eq!(
        session.receive_error(&[0, OP_ERROR as u8, 0, 1]),
        Err(Error::InvalidErrorPacket)
    );

    let mut missing = [0; 32];
    let missing_len = encode_error(ERROR_ACCESS, b"file not found", &mut missing).unwrap();
    assert_eq!(
        session.receive_error(&missing[..missing_len]),
        Ok(RemoteErrorOutcome::Failure { code: ERROR_ACCESS })
    );
    assert_eq!(
        session.next_packet(&mut packet),
        Ok(TransferProgress::Complete)
    );
}

#[test]
fn negotiated_tftp_data_block_fits_one_qrtr_datagram() {
    let parsed = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"blksize", b"65464")],
    ))
    .unwrap();
    // Preserve the requested option for observation, while the transfer plan
    // negotiates down to the transport's maximum complete DATA packet size.
    assert_eq!(parsed.options.block_size, qcom_tftp::MAX_TFTP_BLOCK_SIZE);
    let plan = ReadPlan::new(parsed, u64::from(qcom_tftp::MAX_BLOCK_SIZE) + 1).unwrap();
    assert_eq!(plan.block_size, qcom_tftp::MAX_BLOCK_SIZE);

    let source = vec![0x5a; usize::from(qcom_tftp::MAX_BLOCK_SIZE) + 1];
    let mut session = ReadSession::new(parsed, &source).unwrap();
    let mut packet = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let oack = session.next_packet(&mut packet).unwrap();
    let TransferProgress::Packet { bytes, .. } = oack else {
        panic!("expected OACK packet");
    };
    assert!(bytes <= qcom_tftp::MAX_REQUEST_BYTES);
    assert_eq!(&packet[..bytes], b"\0\x06blksize\04060\0");
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 0]),
        Ok(AckOutcome::Accepted)
    );

    let data = session.next_packet(&mut packet).unwrap();
    assert_eq!(
        data,
        TransferProgress::Packet {
            bytes: qcom_tftp::MAX_REQUEST_BYTES,
            ack_after: Some(1),
        }
    );
    assert_eq!(
        qcom_tftp::QRTR_HEADER_BYTES + qcom_tftp::MAX_REQUEST_BYTES,
        qcom_tftp::MAX_QRTR_DATAGRAM_BYTES
    );
    assert_eq!(&packet[..4], &[0, OP_DATA as u8, 0, 1]);
    assert!(packet[4..].iter().all(|byte| *byte == 0x5a));
}

#[test]
fn unknown_options_are_ignored_without_authorizing_unrequested_options() {
    let bytes = request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"vendor-thing", b"ignored")],
    );
    let parsed = decode_read_request(&bytes).unwrap();
    assert!(!parsed.options.has_supported_options());
    let plan = ReadPlan::new(parsed, 4096).unwrap();
    let mut out = [0; 64];
    assert_eq!(
        encode_oack(parsed, plan, &mut out),
        Err(Error::MalformedOptions)
    );
}

#[test]
fn malformed_requests_writes_modes_and_duplicate_options_fail_closed() {
    assert_eq!(decode_read_request(&[]), Err(Error::ShortPacket));
    assert_eq!(decode_read_request(&[0]), Err(Error::ShortPacket));
    assert_eq!(decode_read_request(&[0, 2]), Err(Error::WriteDenied));
    assert_eq!(decode_read_request(&[0, 9]), Err(Error::UnsupportedOpcode));
    assert_eq!(
        decode_read_request(&vec![0; qcom_tftp::MAX_REQUEST_BYTES + 1]),
        Err(Error::PacketTooLarge)
    );

    let complete = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[]);
    for end in 0..complete.len() {
        assert!(
            decode_read_request(&complete[..end]).is_err(),
            "truncated RRQ at {end} bytes"
        );
    }

    assert_eq!(
        decode_read_request(&request(
            b"/readonly/firmware/image/wlanmdsp.mbn",
            b"netascii",
            &[]
        )),
        Err(Error::UnsupportedMode)
    );
    assert_eq!(
        decode_read_request(&request(
            b"/readonly/firmware/image/wlanmdsp.mbn",
            b"octet",
            &[(b"blksize", b"512"), (b"blksize", b"1024")]
        )),
        Err(Error::DuplicateOption)
    );

    let mut unterminated = vec![0, 1];
    unterminated.extend_from_slice(b"/readonly/firmware/image/wlanmdsp.mbn\0octet\0blksize\0512");
    assert_eq!(
        decode_read_request(&unterminated),
        Err(Error::MalformedOptions)
    );
}

#[test]
fn option_limits_are_checked_without_integer_wraparound() {
    for (name, value) in [
        (&b"blksize"[..], &b"7"[..]),
        (b"blksize", b"65465"),
        (b"timeoutms", b"0"),
        (b"timeoutms", b"255001"),
        (b"tsize", b"1"),
        (b"rsize", b"0"),
        (b"rsize", b"104857601"),
        (b"wsize", b"65536"),
        (b"seek", b"18446744073709551616"),
    ] {
        assert_eq!(
            decode_read_request(&request(
                b"/readonly/firmware/image/wlanmdsp.mbn",
                b"octet",
                &[(name, value)]
            )),
            Err(Error::InvalidOptionValue),
            "{name:?}={value:?}"
        );
    }
}

#[test]
fn read_plan_bounds_seek_and_rsize_to_the_immutable_file_extent() {
    let parsed = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"seek", b"100"), (b"rsize", b"200")],
    ))
    .unwrap();
    assert_eq!(
        ReadPlan::new(parsed, 250),
        Ok(ReadPlan {
            file_size: 250,
            start_offset: 100,
            transfer_size: 150,
            block_size: 512,
            window_size: 1,
        })
    );
    assert_eq!(ReadPlan::new(parsed, 99), Err(Error::SeekPastEnd));
}

#[test]
fn data_and_error_packets_use_network_order_and_fixed_bounds() {
    let mut out = [0; 32];
    let len = encode_data(0x1234, b"xyz", &mut out).unwrap();
    assert_eq!(len, 7);
    assert_eq!(
        &out[..len],
        &[0, OP_DATA as u8, 0x12, 0x34, b'x', b'y', b'z']
    );

    let len = encode_error(ERROR_ACCESS, b"Access violation", &mut out).unwrap();
    assert_eq!(
        &out[..len],
        &[
            0,
            OP_ERROR as u8,
            0,
            ERROR_ACCESS as u8,
            b'A',
            b'c',
            b'c',
            b'e',
            b's',
            b's',
            b' ',
            b'v',
            b'i',
            b'o',
            b'l',
            b'a',
            b't',
            b'i',
            b'o',
            b'n',
            0
        ]
    );
    assert_eq!(
        encode_data(1, b"too long", &mut [0; 4]),
        Err(Error::BufferTooSmall)
    );
    assert_eq!(
        encode_error(1, b"x", &mut [0; 5]),
        Err(Error::BufferTooSmall)
    );
}

#[test]
fn ack_decoder_and_request_error_mapping_are_bounded() {
    assert_eq!(decode_ack(&[0, OP_ACK as u8, 0x12, 0x34]), Ok(0x1234));
    assert_eq!(decode_ack(&[0, OP_ACK as u8, 0]), Err(Error::InvalidAck));
    assert_eq!(decode_ack(&[0, 3, 0, 1]), Err(Error::InvalidAck));
    assert_eq!(error_code(Error::WriteDenied), ERROR_ACCESS);
    assert_eq!(error_code(Error::DuplicateOption), ERROR_OPTION_NEGOTIATION);
    assert_eq!(error_code(Error::UnsupportedMode), ERROR_ILLEGAL_OPERATION);
}

#[test]
fn read_session_sends_data_windows_and_finishes_on_final_ack() {
    let source = (0..22u8).collect::<Vec<_>>();
    let request = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[
            (b"blksize", b"8"),
            (b"wsize", b"2"),
            (b"seek", b"2"),
            (b"rsize", b"20"),
            (b"tsize", b"0"),
        ],
    ))
    .unwrap();
    let mut session = ReadSession::new(request, &source).unwrap();
    let mut out = [0; 64];

    let oack = session.next_packet(&mut out).unwrap();
    assert_eq!(out[0..2], OP_OACK.to_be_bytes());
    assert!(matches!(
        oack,
        TransferProgress::Packet {
            ack_after: Some(0),
            ..
        }
    ));
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 0]),
        Ok(AckOutcome::Accepted)
    );

    let first = session.next_packet(&mut out).unwrap();
    assert_eq!(
        &out[..12],
        &[0, OP_DATA as u8, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(
        first,
        TransferProgress::Packet {
            bytes: 12,
            ack_after: None,
        }
    );

    let second = session.next_packet(&mut out).unwrap();
    assert_eq!(
        &out[..12],
        &[0, OP_DATA as u8, 0, 2, 10, 11, 12, 13, 14, 15, 16, 17]
    );
    assert_eq!(
        second,
        TransferProgress::Packet {
            bytes: 12,
            ack_after: Some(2),
        }
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::WaitingForAck(2))
    );
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 99]),
        Ok(AckOutcome::Unexpected)
    );
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 2]),
        Ok(AckOutcome::Accepted)
    );

    let final_packet = session.next_packet(&mut out).unwrap();
    assert_eq!(&out[..8], &[0, OP_DATA as u8, 0, 3, 18, 19, 20, 21]);
    assert_eq!(
        final_packet,
        TransferProgress::Packet {
            bytes: 8,
            ack_after: Some(3),
        }
    );
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 3]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Complete)
    );
}

#[test]
fn read_session_emits_zero_length_terminator_for_exact_block_multiple() {
    let source = [0x5a; 8];
    let request = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"blksize", b"8")],
    ))
    .unwrap();
    let mut session = ReadSession::new(request, &source).unwrap();
    let mut out = [0; qcom_tftp::MAX_REQUEST_BYTES];

    assert!(matches!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            ack_after: Some(0),
            ..
        })
    ));
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 0]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            bytes: 12,
            ack_after: Some(1),
        })
    );
    assert_eq!(
        &out[..12],
        &[
            0,
            OP_DATA as u8,
            0,
            1,
            0x5a,
            0x5a,
            0x5a,
            0x5a,
            0x5a,
            0x5a,
            0x5a,
            0x5a
        ]
    );
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 1]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            bytes: 4,
            ack_after: Some(2),
        })
    );
    assert_eq!(&out[..4], &[0, OP_DATA as u8, 0, 2]);
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 2]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Complete)
    );
}

#[test]
fn read_session_does_not_advance_on_duplicate_ack_or_small_output_buffer() {
    let source = [0x33; 520];
    let request = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[],
    ))
    .unwrap();
    let mut session = ReadSession::new(request, &source).unwrap();
    let mut too_small = [0; 4];
    assert_eq!(
        session.next_packet(&mut too_small),
        Err(Error::BufferTooSmall)
    );

    let mut out = [0; 600];
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            bytes: 516,
            ack_after: Some(1),
        })
    );
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 1]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 1]),
        Ok(AckOutcome::Duplicate)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            bytes: 12,
            ack_after: Some(2),
        })
    );
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 2]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Complete)
    );
}

#[test]
fn read_session_empty_source_sends_a_terminal_data_packet() {
    let request = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[],
    ))
    .unwrap();
    let mut session = ReadSession::new(request, &[]).unwrap();
    let mut out = [0; 8];

    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            bytes: 4,
            ack_after: Some(1),
        })
    );
    assert_eq!(&out[..4], &[0, OP_DATA as u8, 0, 1]);
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 1]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Complete)
    );
}

#[test]
fn read_session_fetches_only_the_requested_file_chunks() {
    let source = StreamingSource::new((0..24u8).collect(), None);
    let request = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[
            (b"blksize", b"8"),
            (b"wsize", b"2"),
            (b"seek", b"2"),
            (b"rsize", b"12"),
        ],
    ))
    .unwrap();
    let mut session = ReadSession::with_source(request, &source).unwrap();
    let mut out = [0; qcom_tftp::MAX_REQUEST_BYTES];

    assert!(matches!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            ack_after: Some(0),
            ..
        })
    ));
    assert_eq!(
        session.receive_ack(&[0, OP_ACK as u8, 0, 0]),
        Ok(AckOutcome::Accepted)
    );
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            bytes: 12,
            ack_after: None,
        })
    );
    assert_eq!(&out[..12], &[0, 3, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(
        session.next_packet(&mut out),
        Ok(TransferProgress::Packet {
            bytes: 8,
            ack_after: Some(2),
        })
    );
    assert_eq!(&out[..8], &[0, 3, 0, 2, 10, 11, 12, 13]);
    assert_eq!(*source.reads.borrow(), vec![(2, 8), (10, 4)]);
}

#[test]
fn read_session_quarantines_an_uncertain_source_read_without_retry() {
    let source = StreamingSource::new((0..16u8).collect(), Some(0));
    let request = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[],
    ))
    .unwrap();
    let mut session = ReadSession::with_source(request, &source).unwrap();
    let mut out = [0; qcom_tftp::MAX_REQUEST_BYTES];

    assert_eq!(session.next_packet(&mut out), Err(Error::SourceRead));
    assert_eq!(session.next_packet(&mut out), Err(Error::SourceRead));
    assert_eq!(*source.reads.borrow(), vec![(0, 16)]);
}

#[test]
fn mutable_read_source_is_bounded_and_binds_its_live_extent() {
    let reads = Rc::new(RefCell::new(Vec::new()));
    let source = ReadSourceCell::new(MutableSource {
        bytes: (0..32u8).collect(),
        reads: reads.clone(),
        grow_after_read: false,
        len_calls: 0,
        grow_on_len_call: None,
    })
    .unwrap();
    assert_eq!(source.len(), Ok(32));

    let mut out = [0; 8];
    source.read_exact_at(4, &mut out).unwrap();
    assert_eq!(out, [4, 5, 6, 7, 8, 9, 10, 11]);
    assert_eq!(source.read_exact_at(28, &mut out), Err(()));
    assert_eq!(*reads.borrow(), vec![(4, 8)]);
}

#[test]
fn mutable_read_source_extent_drift_quarantines_the_tftp_session() {
    let reads = Rc::new(RefCell::new(Vec::new()));
    let source = ReadSourceCell::new(MutableSource {
        bytes: (0..16u8).collect(),
        reads: reads.clone(),
        grow_after_read: true,
        len_calls: 0,
        grow_on_len_call: None,
    })
    .unwrap();
    let request = decode_read_request(&request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[],
    ))
    .unwrap();
    let mut session = ReadSession::with_source(request, &source).unwrap();
    let mut out = [0; qcom_tftp::MAX_REQUEST_BYTES];

    assert_eq!(session.next_packet(&mut out), Err(Error::SourceRead));
    assert_eq!(session.next_packet(&mut out), Err(Error::SourceRead));
    assert_eq!(*reads.borrow(), vec![(0, 16)]);
}

#[test]
fn qrtr_tftp_does_not_send_oack_after_live_source_extent_drift() {
    let reads = Rc::new(RefCell::new(Vec::new()));
    let source = ReadSourceCell::new(MutableSource {
        bytes: (0..16u8).collect(),
        reads: reads.clone(),
        grow_after_read: false,
        len_calls: 0,
        grow_on_len_call: Some(3),
    })
    .unwrap();
    let mut server = QrtrTftpServer::new(
        1,
        9,
        0x4300,
        0x4400,
        &[0x4000, 0x4100, 0x4200],
        AuthorizedWlanImage::test_source_fixture(&source),
    )
    .unwrap();
    let rrq = request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"blksize", b"8")],
    );
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];

    assert_eq!(
        server.receive(incoming(0x4300, 0x2345), &rrq, &mut output),
        Dispatch::Ignored(IgnoreReason::SourceReadFault)
    );
    assert_eq!(server.active_transfers(), 0);
    assert!(reads.borrow().is_empty());
}

fn qrtr_server<'a>(image: &'a [u8]) -> QrtrTftpServer<'a> {
    QrtrTftpServer::new(
        1,
        9,
        0x4300,
        0x4400,
        &[0x4000, 0x4100, 0x4200],
        AuthorizedWlanImage::test_fixture(image),
    )
    .unwrap()
}

fn incoming(destination_port: u32, source_port: u32) -> IncomingRoute {
    IncomingRoute {
        destination_node: 1,
        destination_port,
        source: Peer {
            node: 9,
            port: source_port,
        },
    }
}

fn tftp_error(code: u16) -> [u8; 5] {
    [0, OP_ERROR as u8, (code >> 8) as u8, code as u8, 0]
}

#[test]
fn qrtr_tftp_binds_size_probe_to_peer_and_fresh_transfer_port() {
    let image = b"firmware";
    let mut server = qrtr_server(image);
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"tsize", b"0")],
    );

    let action = server.receive(incoming(0x4300, 0x1234), &rrq, &mut output);
    assert_eq!(
        action,
        Dispatch::Send {
            route: ReplyRoute {
                source_node: 1,
                source_port: 0x4400,
                destination: Peer {
                    node: 9,
                    port: 0x1234,
                },
            },
            bytes: match action {
                Dispatch::Send { bytes, .. } => bytes,
                _ => unreachable!(),
            },
        }
    );
    let Dispatch::Send { bytes, .. } = action else {
        panic!("valid RRQ must receive an OACK");
    };
    assert_eq!(&output[..2], &[0, OP_OACK as u8]);
    assert!(output[..bytes].windows(6).any(|field| field == b"tsize\0"));
    assert!(output[..bytes].windows(2).any(|field| field == b"8\0"));
    assert_eq!(server.active_transfers(), 1);
    assert_eq!(server.next_transfer_port(), 0x4401);

    let malformed_error = [0, OP_ERROR as u8, 0, ERROR_END_OF_TRANSFER as u8];
    assert_eq!(
        server.receive(incoming(0x4400, 0x1234), &malformed_error, &mut output),
        Dispatch::Ignored(IgnoreReason::InvalidTransferPacket)
    );
    assert_eq!(server.active_transfers(), 1);

    assert_eq!(
        server.receive(
            incoming(0x4400, 0x1235),
            &tftp_error(ERROR_END_OF_TRANSFER),
            &mut output
        ),
        Dispatch::Ignored(IgnoreReason::WrongPeerEndpoint)
    );
    assert_eq!(server.active_transfers(), 1);

    assert_eq!(
        server.receive(
            IncomingRoute {
                destination_node: 1,
                destination_port: 0x4400,
                source: Peer {
                    node: 8,
                    port: 0x1234,
                },
            },
            &tftp_error(ERROR_END_OF_TRANSFER),
            &mut output
        ),
        Dispatch::Ignored(IgnoreReason::WrongPeerNode)
    );
    assert_eq!(server.active_transfers(), 1);

    assert_eq!(
        server.receive(
            incoming(0x4400, 0x1234),
            &tftp_error(ERROR_END_OF_TRANSFER),
            &mut output
        ),
        Dispatch::Closed {
            key: qcom_qrtr_tftp::TransferKey {
                local_port: 0x4400,
                peer: Peer {
                    node: 9,
                    port: 0x1234,
                },
            },
            reason: CloseReason::EndOfTransfer,
        }
    );
    assert_eq!(server.active_transfers(), 0);

    // A second RRQ from the same peer gets a new TID; the retired port is not reused.
    let action = server.receive(incoming(0x4300, 0x1234), &rrq, &mut output);
    assert!(matches!(
        action,
        Dispatch::Send {
            route: ReplyRoute {
                source_port: 0x4401,
                ..
            },
            ..
        }
    ));
}

#[test]
fn qrtr_tftp_accepts_only_ack_from_bound_peer_and_closes_on_final_ack() {
    let mut server = qrtr_server(b"abc");
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/vendor/firmware/wlanmdsp.mbn", b"octet", &[]);

    let action = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output);
    let Dispatch::Send { route, bytes } = action else {
        panic!("valid RRQ must receive DATA");
    };
    assert_eq!(route.source_port, 0x4400);
    assert_eq!(
        &output[..bytes],
        &[0, OP_DATA as u8, 0, 1, b'a', b'b', b'c']
    );

    assert_eq!(
        server.receive(
            incoming(0x4400, 0x2346),
            &[0, OP_ACK as u8, 0, 1],
            &mut output
        ),
        Dispatch::Ignored(IgnoreReason::WrongPeerEndpoint)
    );
    assert_eq!(server.active_transfers(), 1);

    assert_eq!(
        server.receive(
            incoming(0x4400, 0x2345),
            &[0, OP_ACK as u8, 0, 1],
            &mut output
        ),
        Dispatch::Closed {
            key: qcom_qrtr_tftp::TransferKey {
                local_port: 0x4400,
                peer: Peer {
                    node: 9,
                    port: 0x2345,
                },
            },
            reason: CloseReason::TransferComplete,
        }
    );
    assert_eq!(server.active_transfers(), 0);
}

#[test]
fn qrtr_tftp_retires_a_transfer_after_source_read_failure() {
    let source = StreamingSource::new((0..16u8).collect(), Some(8));
    let mut server = QrtrTftpServer::new(
        1,
        9,
        0x4300,
        0x4400,
        &[0x4000, 0x4100, 0x4200],
        AuthorizedWlanImage::test_source_fixture(&source),
    )
    .unwrap();
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(
        b"/readonly/firmware/image/wlanmdsp.mbn",
        b"octet",
        &[(b"blksize", b"8")],
    );

    assert!(matches!(
        server.receive(incoming(0x4300, 0x2345), &rrq, &mut output),
        Dispatch::Send { bytes, .. } if output[..bytes].starts_with(&OP_OACK.to_be_bytes())
    ));
    assert!(matches!(
        server.receive(
            incoming(0x4400, 0x2345),
            &[0, OP_ACK as u8, 0, 0],
            &mut output
        ),
        Dispatch::Send { bytes, .. } if output[..bytes].starts_with(&[0, OP_DATA as u8, 0, 1])
    ));
    assert_eq!(
        server.receive(
            incoming(0x4400, 0x2345),
            &[0, OP_ACK as u8, 0, 1],
            &mut output
        ),
        Dispatch::Closed {
            key: qcom_qrtr_tftp::TransferKey {
                local_port: 0x4400,
                peer: Peer {
                    node: 9,
                    port: 0x2345,
                },
            },
            reason: CloseReason::SourceReadFault,
        }
    );
    assert_eq!(server.active_transfers(), 0);
    assert_eq!(*source.reads.borrow(), vec![(0, 8), (8, 8)]);
}

#[test]
fn qrtr_tftp_revalidates_authorized_image_length_before_preboot_staging() {
    let source = MutableLengthSource(Cell::new(4));
    let server = QrtrTftpServer::new(
        1,
        9,
        0x4300,
        0x4400,
        &[0x4000, 0x4100, 0x4200],
        AuthorizedWlanImage::test_source_fixture(&source),
    )
    .unwrap();
    assert!(server.image_available());

    source.0.set(5);
    assert!(!server.image_available());
}

#[test]
fn qrtr_tftp_caps_sessions_skips_reserved_ports_and_never_reuses_retired_ports() {
    let mut server = QrtrTftpServer::new(
        1,
        9,
        0x4300,
        0x4400,
        &[0x4400],
        AuthorizedWlanImage::test_fixture(b"x"),
    )
    .unwrap();
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[]);

    for index in 0..MAX_ACTIVE_TRANSFERS {
        let source_port = 0x2000 + index as u32;
        let action = server.receive(incoming(0x4300, source_port), &rrq, &mut output);
        assert!(matches!(
            action,
            Dispatch::Send {
                route: ReplyRoute { source_port: port, .. },
                ..
            } if port == 0x4401 + index as u32
        ));
    }
    assert_eq!(server.active_transfers(), MAX_ACTIVE_TRANSFERS);
    assert_eq!(server.next_transfer_port(), 0x4405);
    assert_eq!(
        server.receive(incoming(0x4300, 0x2100), &rrq, &mut output),
        Dispatch::Ignored(IgnoreReason::NoSessionCapacity)
    );
    assert_eq!(server.next_transfer_port(), 0x4405);

    assert!(matches!(
        server.receive(
            incoming(0x4401, 0x2000),
            &tftp_error(ERROR_ACCESS),
            &mut output
        ),
        Dispatch::Closed {
            reason: CloseReason::RemoteFailure { code: ERROR_ACCESS },
            ..
        }
    ));
    assert!(matches!(
        server.receive(incoming(0x4300, 0x2100), &rrq, &mut output),
        Dispatch::Send {
            route: ReplyRoute {
                source_port: 0x4405,
                ..
            },
            ..
        }
    ));
}

#[test]
fn qrtr_tftp_rejects_conflicting_port_configuration_and_small_output_before_state_change() {
    assert_eq!(
        QrtrTftpServer::new(
            1,
            9,
            0x4300,
            0x4400,
            &[0x4000, 0x4000],
            AuthorizedWlanImage::test_fixture(b"x"),
        )
        .err(),
        Some(ConfigError::DuplicateReservedPort)
    );
    assert_eq!(
        QrtrTftpServer::new(
            1,
            9,
            0x4300,
            0x4400,
            &[0x4300],
            AuthorizedWlanImage::test_fixture(b"x"),
        )
        .err(),
        Some(ConfigError::ReservedPortConflict)
    );

    let mut server = qrtr_server(b"x");
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[]);
    let mut output = [0; 32];
    assert_eq!(
        server.receive(incoming(0x4300, 0x1234), &rrq, &mut output),
        Dispatch::Ignored(IgnoreReason::OutputTooSmall)
    );
    assert_eq!(server.active_transfers(), 0);
    assert_eq!(server.next_transfer_port(), 0x4400);
}

#[test]
fn qrtr_server_negotiates_one_packet_window_and_completes_multiblock_read() {
    let image: Vec<u8> = (0..19).collect();
    let mut server = qrtr_server(&image);
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet",
        &[(b"blksize", b"8"), (b"wsize", b"2")]);
    let Dispatch::Send { route, bytes } = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output)
        else { panic!("expected OACK"); };
    assert_eq!(&output[..bytes], b"\0\x06blksize\x008\0wsize\x001\0");
    for block in 0..3u16 {
        let [hi, lo] = block.to_be_bytes();
        let Dispatch::Send { bytes, .. } = server.receive(incoming(route.source_port, 0x2345),
            &[0, OP_ACK as u8, hi, lo], &mut output) else { panic!("expected DATA after ACK {block}"); };
        assert_eq!(&output[..4], &[0, OP_DATA as u8, 0, block as u8 + 1]);
        let begin = usize::from(block)*8;
        assert_eq!(&output[4..bytes], &image[begin..(begin+8).min(image.len())]);
    }
    assert!(matches!(server.receive(incoming(route.source_port, 0x2345), &[0, OP_ACK as u8, 0, 3], &mut output),
        Dispatch::Closed { reason: CloseReason::TransferComplete, .. }));
}

#[test]
fn synthetic_reference_sized_firmware_read_reaches_final_ack_without_truncation() {
    let image: Vec<u8> = (0..3_751_004usize).map(|i| (i.wrapping_mul(37) & 0xff) as u8).collect();
    let mut server = qrtr_server(&image);
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet",
        &[(b"blksize", b"4060"), (b"wsize", b"2")]);
    let Dispatch::Send { route, .. } = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output)
        else { panic!("expected OACK"); };
    let mut offset = 0usize;
    let mut last_ack = 0u16;
    loop {
        let [hi, lo] = last_ack.to_be_bytes();
        let outcome = server.receive(incoming(route.source_port, 0x2345), &[0, 4, hi, lo], &mut output);
        match outcome {
            Dispatch::Send { bytes, .. } => {
                let next = last_ack.wrapping_add(1);
                assert_eq!(&output[..4], &[0, 3, (next >> 8) as u8, next as u8]);
                let end = offset + bytes - 4;
                assert_eq!(&output[4..bytes], &image[offset..end]);
                offset = end;
                last_ack = next;
            }
            Dispatch::Closed { reason: CloseReason::TransferComplete, .. } => break,
            _ => panic!("transfer stalled at {offset} bytes"),
        }
    }
    assert_eq!(offset, image.len());
    assert_eq!(server.active_transfers(), 0);
}

#[test]
fn repeated_firmware_rrq_reuses_the_same_transfer_and_oack() {
    let mut server = qrtr_server(b"firmware bytes");
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet",
        &[(b"blksize", b"8")]);
    let Dispatch::Send { route, bytes } = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output)
        else { panic!("initial RRQ must produce an OACK"); };
    let original = output[..bytes].to_vec();
    let retry = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output);
    assert_eq!(retry, Dispatch::Send { route, bytes });
    assert_eq!(&output[..bytes], original);
    assert_eq!(server.active_transfers(), 1);
    assert_eq!(server.next_transfer_port(), route.source_port + 1);
}

#[test]
fn client_close_retires_only_matching_firmware_session_and_never_reuses_port() {
    let mut server = qrtr_server(b"firmware bytes");
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[(b"blksize", b"8")]);
    let Dispatch::Send { route, .. } = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output)
        else { panic!("initial RRQ"); };
    assert!(server.active_peers().contains(&Some(Peer { node: 9, port: 0x2345 })));
    assert!(server.owns_transfer(Peer { node: 9, port: 0x2345 }, route.source_port));
    assert!(!server.owns_transfer(Peer { node: 9, port: 0x2345 }, route.source_port + 1));
    assert_eq!(server.close_peer(Peer { node: 8, port: 0x2345 }), 0);
    assert_eq!(server.active_transfers(), 1);
    assert_eq!(server.close_peer(Peer { node: 9, port: 0x2345 }), 1);
    assert_eq!(server.active_transfers(), 0);
    assert!(!server.active_peers().contains(&Some(Peer { node: 9, port: 0x2345 })));
    assert!(!server.owns_transfer(Peer { node: 9, port: 0x2345 }, route.source_port));
    assert_eq!(server.receive(incoming(route.source_port, 0x2345), &[0, 4, 0, 0], &mut output),
        Dispatch::Ignored(IgnoreReason::UnknownLocalPort));
    let Dispatch::Send { route: next, .. } = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output)
        else { panic!("new RRQ"); };
    assert_ne!(next.source_port, route.source_port);
}

#[test]
fn repeated_optionless_rrq_repeats_the_initial_data_without_advancing() {
    let mut server = qrtr_server(b"firmware bytes");
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[]);
    let Dispatch::Send { route, bytes } = server.receive(incoming(0x4300, 0x2345), &rrq, &mut output)
        else { panic!("initial RRQ"); };
    let initial = output[..bytes].to_vec();
    assert_eq!(server.receive(incoming(0x4300, 0x2345), &rrq, &mut output), Dispatch::Send { route, bytes });
    assert_eq!(&output[..bytes], initial);
    assert_eq!(server.active_transfers(), 1);
    assert!(matches!(server.receive(incoming(route.source_port, 0x2345), &[0, 4, 0, 1], &mut output),
        Dispatch::Closed { reason: CloseReason::TransferComplete, .. }));
}

#[test]
fn shared_service_routes_server_check_and_firmware_without_crossing_transfer_ports() {
    use qcom_tftp_mux::{dispatch, MuxDispatch};
    use qcom_tftp_server_check::{Address, Route, ServerCheck, REQUEST, SERVICE_PORT};
    let mut check = ServerCheck::new();
    let mut firmware = QrtrTftpServer::new_bounded(1, 9, SERVICE_PORT, 0x5100, 0x51ff,
        &[], AuthorizedWlanImage::test_fixture(b"firmware bytes")).unwrap();
    let mut out = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let check_route = Route { source: Address { node: 0, port: 0x49 }, destination: Address { node: 1, port: SERVICE_PORT } };
    let MuxDispatch::ServerCheck(Ok(check_reply)) = dispatch(&mut check, &mut firmware, check_route, REQUEST, &mut out)
        else { panic!("server-check WRQ"); };
    assert_eq!(&out[..check_reply.len], qcom_tftp_server_check::OACK);
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[(b"blksize", b"8")]);
    let wlan_route = Route { source: Address { node: 9, port: 0x55 }, ..check_route };
    let MuxDispatch::Firmware(Dispatch::Send { route, .. }) = dispatch(&mut check, &mut firmware, wlan_route, &rrq, &mut out)
        else { panic!("firmware RRQ"); };
    assert_eq!(route.source_port, 0x5100);
    let ack_route = Route { destination: Address { node: 1, port: route.source_port }, ..wlan_route };
    assert!(matches!(dispatch(&mut check, &mut firmware, ack_route, &[0, 4, 0, 0], &mut out),
        MuxDispatch::Firmware(Dispatch::Send { .. })));
    let data_route = Route { destination: check_reply.route.source, ..check_route };
    assert!(matches!(dispatch(&mut check, &mut firmware, data_route, b"\0\x03\0\x01hello", &mut out),
        MuxDispatch::ServerCheck(Ok(reply)) if reply.completed));
}

#[test]
fn firmware_transfer_ports_stay_in_their_owned_range_without_reuse() {
    let mut server = QrtrTftpServer::new_bounded(1, 9, 0x5000, 0x5100, 0x5101,
        &[], AuthorizedWlanImage::test_fixture(b"x")).unwrap();
    let mut output = [0; qcom_tftp::MAX_REQUEST_BYTES];
    let rrq = request(b"/readonly/firmware/image/wlanmdsp.mbn", b"octet", &[]);
    for expected in [0x5100, 0x5101] {
        let Dispatch::Send { route, .. } = server.receive(incoming(0x5000, 0x2345), &rrq, &mut output)
            else { panic!("expected DATA"); };
        assert_eq!(route.source_port, expected);
        assert!(matches!(server.receive(incoming(expected, 0x2345), &[0, OP_ACK as u8, 0, 1], &mut output),
            Dispatch::Closed { reason: CloseReason::TransferComplete, .. }));
    }
    assert_eq!(server.receive(incoming(0x5000, 0x2345), &rrq, &mut output),
        Dispatch::Ignored(IgnoreReason::TransferPortExhausted));
}
