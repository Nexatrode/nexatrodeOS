#[path = "../rust/src/qcom_tftp_server_check.rs"]
mod model;
use model::{Address, Error, Route, ServerCheck, REQUEST, OACK, SERVICE_PORT, TRANSFER_PORT};

fn route(port: u32) -> Route {
    Route { source: Address { node: 0, port: 0x56 }, destination: Address { node: 1, port } }
}

#[test]
fn observed_wrq_sends_reference_oack_from_separate_port_then_acknowledges_data() {
    let mut s = ServerCheck::new(); let mut out = [0; 128];
    let reply = s.receive(route(SERVICE_PORT), REQUEST, &mut out).unwrap();
    assert_eq!(&out[..reply.len], OACK);
    assert_eq!(&out[..reply.len], b"\x00\x06blksize\x007680\x00timeoutms\x001000\x00wsize\x0010\x00");
    assert_eq!(reply.route.source.port, TRANSFER_PORT);
    assert_eq!(reply.route.destination, route(SERVICE_PORT).source);
    assert!(!reply.completed);
    let reply = s.receive(route(TRANSFER_PORT), b"\0\x03\0\x01hello", &mut out).unwrap();
    assert_eq!(&out[..reply.len], &[0, 4, 0, 1]);
    assert!(reply.completed); assert!(s.complete()); assert_eq!(s.last_block(), 1); assert_eq!(s.contents(), b"hello");
    assert!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x01hello", &mut out).unwrap().completed);
    assert_eq!(s.contents(), b"hello");
    assert_eq!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x01other", &mut out), Err(Error::ChangedRetransmission));
}

#[test]
fn request_and_route_are_strict_and_state_does_not_advance_without_output_space() {
    let mut s = ServerCheck::new(); let mut short = [0; 2]; let mut out = [0; 128];
    assert_eq!(s.receive(route(SERVICE_PORT), REQUEST, &mut short), Err(Error::OutputTooSmall));
    assert_eq!(s.receive(route(SERVICE_PORT), b"\0\x02/readwrite/other\0octet\0", &mut out), Err(Error::UnexpectedRequest));
    assert_eq!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x01A", &mut out), Err(Error::WrongRoute));
    let uppercase = b"\0\x02/readwrite/server_check.txt\0OCTET\0blksize\07680\0timeoutms\01000\0wsize\010\0";
    assert!(s.receive(route(SERVICE_PORT), uppercase, &mut out).is_ok());
    s.receive(route(SERVICE_PORT), REQUEST, &mut out).unwrap();
    assert_eq!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x01A", &mut short), Err(Error::OutputTooSmall));
    assert!(s.contents().is_empty());
    let mut wrong = route(TRANSFER_PORT); wrong.source.port += 1;
    assert_eq!(s.receive(wrong, b"\0\x03\0\x01A", &mut out), Err(Error::WrongRoute));
    assert_eq!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x02A", &mut out), Err(Error::UnexpectedPacket));
    s.receive(route(TRANSFER_PORT), b"\0\x03\0\x01A", &mut out).unwrap();
}

#[test]
fn distinct_peers_have_isolated_ports_and_bounded_ram() {
    let mut server = ServerCheck::new();
    let mut out = [0; 128];
    server.receive(route(SERVICE_PORT), REQUEST, &mut out).unwrap();
    let mut other = route(SERVICE_PORT);
    other.source.port = 0x57;
    let second = server.receive(other, REQUEST, &mut out).unwrap();
    assert_eq!(second.route.source.port, TRANSFER_PORT + 1);
    assert_eq!(server.active_count(), 2);
    let wrong = Route { source: other.source, destination: Address { node: 1, port: TRANSFER_PORT } };
    assert_eq!(server.receive(wrong, b"\0\x03\0\x01x", &mut out), Err(Error::WrongRoute));
    let right = Route { source: other.source, destination: second.route.source };
    server.receive(right, b"\0\x03\0\x01x", &mut out).unwrap();
    assert_eq!(server.total_bytes(), 1);
    assert_eq!(server.completed_count(), 1);
    assert!(server.contents().is_empty(), "second peer must not mutate the first peer's buffer");
    for port in 0x58..(0x56 + model::TRANSFER_COUNT as u32) {
        let mut next = route(SERVICE_PORT); next.source.port = port;
        let reply = server.receive(next, REQUEST, &mut out).unwrap();
        assert_eq!(reply.route.source.port, TRANSFER_PORT + port - 0x56);
        assert_eq!(reply.route.destination, next.source);
    }
    let mut fifth = route(SERVICE_PORT); fifth.source.port = 0x56 + model::TRANSFER_COUNT as u32;
    assert_eq!(server.receive(fifth, REQUEST, &mut out), Err(Error::Capacity));
    assert_eq!(server.active_count(), model::TRANSFER_COUNT);
}

#[cfg(feature = "sunfish-tftp-peer-close-experiment")]
#[test]
fn abandoning_empty_peer_preserves_other_peers_and_reserves_retired_port() {
    let mut server = ServerCheck::new(); let mut out = [0; 128];
    let first = route(SERVICE_PORT);
    let first_reply = server.receive(first, REQUEST, &mut out).unwrap();
    let mut second = first; second.source.port += 1;
    let second_reply = server.receive(second, REQUEST, &mut out).unwrap();
    let mut wrong_node = first.source; wrong_node.node = 1;
    assert_eq!(server.can_abandon_empty_peer(wrong_node), Err(Error::WrongRoute));
    assert_eq!(server.abandon_empty_peer(wrong_node), Err(Error::WrongRoute));
    assert_eq!(server.can_abandon_empty_peer(first.source), Ok(()));
    assert_eq!(server.can_abandon_empty_peer_at(first.source, first_reply.route.source.port), Ok(()));
    assert_eq!(server.can_abandon_empty_peer_at(first.source, second_reply.route.source.port), Err(Error::WrongRoute));
    assert_eq!(server.can_abandon_empty_peer(first.source), Ok(()), "preflight is read-only");
    server.abandon_empty_peer(first.source).unwrap();
    assert_eq!(server.abandon_empty_peer(first.source), Err(Error::UnexpectedPacket));
    assert_eq!(server.receive(first, REQUEST, &mut out), Err(Error::UnexpectedRequest));
    let late = Route { source: first.source, destination: first_reply.route.source };
    assert_eq!(server.receive(late, b"\0\x03\0\x01x", &mut out), Err(Error::UnexpectedPacket));
    let mut third = first; third.source.port += 2;
    assert_ne!(server.receive(third, REQUEST, &mut out).unwrap().route.source.port, first_reply.route.source.port);
    let data = Route { source: second.source, destination: second_reply.route.source };
    assert!(server.receive(data, b"\0\x03\0\x01x", &mut out).unwrap().completed);
}

#[cfg(feature = "sunfish-tftp-peer-close-experiment")]
#[test]
fn abandoning_peer_with_data_is_rejected_without_mutating_the_transfer() {
    let mut server = ServerCheck::new(); let mut out = [0; 128];
    let first = route(SERVICE_PORT);
    let reply = server.receive(first, REQUEST, &mut out).unwrap();
    let data = Route { source: first.source, destination: reply.route.source };
    let mut full = [7u8; 516]; full[..4].copy_from_slice(b"\0\x03\0\x01");
    server.receive(data, &full, &mut out).unwrap();
    assert_eq!(server.can_abandon_empty_peer(first.source), Err(Error::UnexpectedPacket));
    assert_eq!(server.abandon_empty_peer(first.source), Err(Error::UnexpectedPacket));
    assert_eq!(server.contents().len(), 512);
    assert!(server.receive(data, b"\0\x03\0\x02x", &mut out).unwrap().completed);
}

#[cfg(feature = "sunfish-tftp-peer-close-experiment")]
#[test]
fn abandonment_rechecks_if_data_arrives_after_preflight() {
    let mut server = ServerCheck::new(); let mut out = [0; 128];
    let first = route(SERVICE_PORT);
    let reply = server.receive(first, REQUEST, &mut out).unwrap();
    assert_eq!(server.can_abandon_empty_peer(first.source), Ok(()));
    let data = Route { source: first.source, destination: reply.route.source };
    server.receive(data, b"\0\x03\0\x01x", &mut out).unwrap();
    assert_eq!(server.abandon_empty_peer(first.source), Err(Error::UnexpectedPacket));
    assert_eq!(server.contents(), b"x");
}

#[test]
fn first_capacity_rejection_does_not_describe_later_requests() {
    let mut server = ServerCheck::new(); let mut out = [0; 128];
    for port in 0x56..(0x56 + model::TRANSFER_COUNT as u32) {
        let mut peer = route(SERVICE_PORT); peer.source.port = port;
        server.receive(peer, REQUEST, &mut out).unwrap();
    }
    let mut fifth = route(SERVICE_PORT); fifth.source.port = 0x56 + model::TRANSFER_COUNT as u32;
    assert_eq!(server.receive(fifth, REQUEST, &mut out), Err(Error::Capacity));
    let wlan_rrq = b"\0\x01/readonly/firmware/image/wlanmdsp.mbn\0octet\0";
    assert_eq!(server.receive(fifth, wlan_rrq, &mut out), Err(Error::UnexpectedRequest));
    assert_eq!(server.first_rejection(), Some((Error::Capacity, REQUEST.len() as u32)),
        "a first-error display cannot characterize later ignored packets");
}

#[test]
fn full_blocks_end_only_with_short_block_and_storage_is_bounded() {
    let mut s = ServerCheck::new(); let mut out = [0; 128];
    s.receive(route(SERVICE_PORT), REQUEST, &mut out).unwrap();
    for block in 1..=16u16 {
        let mut packet = vec![7; 516]; packet[..4].copy_from_slice(&[0, 3, (block >> 8) as u8, block as u8]);
        assert!(!s.receive(route(TRANSFER_PORT), &packet, &mut out).unwrap().completed);
    }
    assert_eq!(s.contents().len(), 8192);
    assert_eq!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x11x", &mut out), Err(Error::Capacity));
    assert!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x11", &mut out).unwrap().completed);
    assert_eq!(s.contents().len(), 8192);
}

#[test]
fn oversized_packet_is_not_silently_truncated() {
    let mut s = ServerCheck::new(); let mut out = [0; 128];
    s.receive(route(SERVICE_PORT), REQUEST, &mut out).unwrap();
    let mut packet = vec![0; 517]; packet[..4].copy_from_slice(&[0, 3, 0, 1]);
    assert_eq!(s.receive(route(TRANSFER_PORT), &packet, &mut out), Err(Error::Oversize));
    assert_eq!(s.first_rejection(), Some((Error::Oversize, 517)));
    assert_eq!(Error::Oversize.code(), 4);
    assert!(s.contents().is_empty());
}

#[cfg(feature = "sunfish-tftp-missing-temp-replies")]
#[test]
fn allowlisted_absent_reads_use_distinct_reserved_ports_without_file_data() {
    let mut server = ServerCheck::new(); let mut out = [0; 128];
    for (i, path) in ["/readwrite/mcfg.tmp", "/readwrite/ota_firewall/ruleset", "/readwrite/modem_efs"].iter().enumerate() {
        let mut packet = vec![0, 1]; packet.extend(path.as_bytes()); packet.extend(b"\0OCTET\0tsize\00\0timeoutms\01000\0");
        let mut incoming = route(SERVICE_PORT); incoming.source.port += i as u32;
        let reply = server.receive(incoming, &packet, &mut out).unwrap();
        assert_eq!(reply.route.source.port, TRANSFER_PORT + i as u32);
        assert_eq!(reply.route.destination, incoming.source);
        assert_eq!(reply.missing_kind, i as u8 + 1);
        assert_eq!(&out[..reply.len], b"\0\x05\0\x01file not found\0");
        assert!(!reply.completed);
        assert_eq!(server.receive(incoming, &packet, &mut out).unwrap(), reply, "retry stays on its original port");
        assert_eq!(server.receive(incoming, REQUEST, &mut out), Err(Error::UnexpectedRequest));
        assert_eq!(server.receive(Route { destination: reply.route.source, ..incoming }, b"\0\x03\0\x01bad", &mut out), Err(Error::UnexpectedPacket));
        #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
        assert_eq!(server.can_abandon_empty_peer(incoming.source), Err(Error::UnexpectedPacket));
    }
    assert_eq!((server.total_bytes(), server.completed_count(), server.active_count()), (0, 0, 3));
    assert_eq!((server.first_missing_kind(), server.missing_requests()), (1, 6));
    let mut wrq = route(SERVICE_PORT); wrq.source.port += 3;
    let reply = server.receive(wrq, REQUEST, &mut out).unwrap();
    assert_eq!(reply.missing_kind, 0);
    assert!(server.receive(Route { destination: reply.route.source, ..wrq }, b"\0\x03\0\x01ok", &mut out).unwrap().completed);
    assert_eq!((server.total_bytes(), server.completed_count()), (2, 1));
}

#[cfg(feature = "sunfish-tftp-missing-temp-replies")]
#[test]
fn missing_reads_reject_malformed_or_other_resources_without_reply_or_allocation() {
    for packet in [
        &b"\0\x01/readwrite/mcfg.tmp\0octet"[..],
        &b"\0\x01/readwrite/mcfg.tmp\0netascii\0"[..],
        &b"\0\x01/readwrite/mcfg.tmp\0octet\0key\0"[..],
        &b"\0\x01/readwrite/mcfg.tmp\0octet\0key\0\0"[..],
        &b"\0\x01/readwrite/mcfg.tmp\0octet\0key\0v\x01\0"[..],
        &b"\0\x01/readwrite/mcfg.tmp\0octet\0\0"[..],
        &b"\0\x02/readwrite/mcfg.tmp\0octet\0"[..],
        &b"\0\x01/readwrite/mcfg.tmp/other\0octet\0"[..],
        &b"\0\x01/readwrite/../mcfg.tmp\0octet\0"[..],
        &b"\0\x01/readonly/firmware/image/wlanmdsp.mbn\0octet\0"[..],
        &b"\0\x01/readwrite/server_check.txt\0octet\0"[..],
    ] {
        let mut server = ServerCheck::new(); let mut out = [0xa5; 128];
        assert_eq!(server.receive(route(SERVICE_PORT), packet, &mut out), Err(Error::UnexpectedRequest));
        assert_eq!(out, [0xa5; 128]);
        assert_eq!((server.active_count(), server.missing_requests(), server.first_missing_kind()), (0, 0, 0));
    }
    let mut server = ServerCheck::new(); let mut out = [0xa5; 128];
    let request = b"\0\x01/readwrite/mcfg.tmp\0octet\0";
    assert_eq!(server.receive(route(SERVICE_PORT), request, &mut out[..18]), Err(Error::OutputTooSmall));
    assert_eq!(server.active_count(), 0);
    let mut wrong = route(SERVICE_PORT); wrong.source.node = 1;
    assert_eq!(server.receive(wrong, request, &mut out), Err(Error::WrongRoute));
    for i in 0..model::TRANSFER_COUNT {
        let mut incoming = route(SERVICE_PORT); incoming.source.port += i as u32;
        server.receive(incoming, request, &mut out).unwrap();
    }
    let mut next = route(SERVICE_PORT); next.source.port += model::TRANSFER_COUNT as u32;
    assert_eq!(server.receive(next, request, &mut out), Err(Error::Capacity));
    assert_eq!(server.total_bytes(), 0);
}

#[cfg(feature = "sunfish-tftp-rejection-capture")]
#[test]
fn captures_first_rejected_request_without_data_payload_or_overwrite() {
    let mut s = ServerCheck::new(); let mut out = [0; 128];
    let request = b"\0\x02/readwrite/other\0octet\0";
    assert!(s.receive(route(SERVICE_PORT), request, &mut out).is_err());
    let c = s.rejection_capture().unwrap();
    assert_eq!(c.route, route(SERVICE_PORT)); assert_eq!(c.opcode, 2);
    assert_eq!(&c.bytes[..c.captured as usize], request);
    assert!(s.receive(route(TRANSFER_PORT), b"\0\x03\0\x01secret", &mut out).is_err());
    assert_eq!(s.rejection_capture().unwrap().opcode, 2);
    let mut other = ServerCheck::new();
    assert!(other.receive(route(TRANSFER_PORT), b"\0\x03\0\x01secret", &mut out).is_err());
    assert_eq!(other.rejection_capture().unwrap().captured, 0);
    let mut long = ServerCheck::new(); let mut packet = [b'x'; 100]; packet[0] = 0; packet[1] = 2;
    assert!(long.receive(route(SERVICE_PORT), &packet, &mut out).is_err());
    let captured = long.rejection_capture().unwrap();
    assert_eq!(captured.length, 100); assert_eq!(captured.captured, 60);
    assert_eq!(&captured.bytes, &packet[..60]);
}
