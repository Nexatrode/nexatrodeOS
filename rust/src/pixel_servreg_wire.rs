//! Fixed SERVREG LOC wire messages used by the one-shot Sunfish probe.
//! This module has no hardware or shared-memory access.

pub const SERVICE_ID: u32 = 0x40;
pub const SERVICE_VERSION_INSTANCE: u32 = 0x0000_0101;
pub const LOCAL_NODE: u32 = 1;
pub const LOCAL_PORT: u32 = 0x4000;
pub const CONTROL_PORT: u32 = 0xffff_fffe;
pub const REMOTE_NODE: u32 = 0;
pub const DOMAIN_INSTANCE: u32 = 180;
pub const QUERY_NAME: &[u8] = b"tms/servreg";
pub const QUERY_BYTES: usize = 53;
pub const PDR_QUERY_NAME: &[u8] = b"tms/pdr_enabled";
pub const PDR_QUERY_BYTES: usize = QRTR_HEADER_BYTES + QMI_HEADER_BYTES + 3 + PDR_QUERY_NAME.len();
pub const PDDUMP_QUERY_NAME: &[u8] = b"tms/pddump_disabled";
pub const PDDUMP_QUERY_BYTES: usize =
    QRTR_HEADER_BYTES + QMI_HEADER_BYTES + 3 + PDDUMP_QUERY_NAME.len();
pub const GET_DOMAIN_LIST_MESSAGE_ID: u16 = 0x21;
pub const ROOT_DOMAIN_NAME: &[u8] = b"msm/modem/root_pd";
pub const WLAN_DOMAIN_NAME: &[u8] = b"msm/modem/wlan_pd";
const DOMAIN_ENTRY_FIXED_BYTES: usize = 2 + 4 + 1 + 4;
const DOMAIN_LIST_VALUE_BYTES: usize =
    1 + DOMAIN_ENTRY_FIXED_BYTES * 2 + ROOT_DOMAIN_NAME.len() + WLAN_DOMAIN_NAME.len();
const QRTR_HEADER_BYTES: usize = 32;
const QMI_HEADER_BYTES: usize = 7;
const QMI_PAYLOAD_BYTES: usize = 7 + 5 + 5 + 3 + DOMAIN_LIST_VALUE_BYTES;
pub const RESPONSE_BYTES: usize = QRTR_HEADER_BYTES + QMI_HEADER_BYTES + QMI_PAYLOAD_BYTES;
pub const RESPONSE_GLINK_BYTES: usize = (16 + RESPONSE_BYTES + 7) & !7;
const PDR_EMPTY_QMI_PAYLOAD_BYTES: usize = 7 + 5 + 5;
pub const PDR_EMPTY_RESPONSE_BYTES: usize =
    QRTR_HEADER_BYTES + QMI_HEADER_BYTES + PDR_EMPTY_QMI_PAYLOAD_BYTES;
pub const PDR_EMPTY_RESPONSE_GLINK_BYTES: usize = (16 + PDR_EMPTY_RESPONSE_BYTES + 7) & !7;

const fn put32(dst: &mut [u8], at: usize, value: u32) {
    let bytes = value.to_le_bytes();
    dst[at] = bytes[0];
    dst[at + 1] = bytes[1];
    dst[at + 2] = bytes[2];
    dst[at + 3] = bytes[3];
}

const fn put16(dst: &mut [u8], at: usize, value: u16) {
    let bytes = value.to_le_bytes();
    dst[at] = bytes[0];
    dst[at + 1] = bytes[1];
}

/// A canonical QRTR NEW_SERVER announcement for this one local endpoint.
pub const fn announcement() -> [u8; 52] {
    let mut b = [0; 52];
    let words = [
        1,
        4,
        LOCAL_NODE,
        CONTROL_PORT,
        0,
        20,
        REMOTE_NODE,
        CONTROL_PORT,
        4,
        SERVICE_ID,
        SERVICE_VERSION_INSTANCE,
        LOCAL_NODE,
        LOCAL_PORT,
    ];
    let mut i = 0;
    while i < words.len() {
        put32(&mut b, i * 4, words[i]);
        i += 1;
    }
    b
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Query {
    pub remote_port: u32,
    pub transaction: u16,
}

/// Accept only the single expected QMI GET_DOMAIN_LIST request, with no offset
/// or extra TLVs. The returned peer port is range-checked before it is used as
/// a destination in the one response.
pub fn parse_query(bytes: &[u8]) -> Option<Query> {
    parse_named_query(bytes, QUERY_NAME, QUERY_BYTES)
}

/// Accept only the exact observed Sunfish PDR-enabled domain lookup.
pub fn parse_pdr_query(bytes: &[u8]) -> Option<Query> {
    parse_named_query(bytes, PDR_QUERY_NAME, PDR_QUERY_BYTES)
}

/// Accept only the exact observed SERVREG lookup for `tms/pddump_disabled`.
pub fn parse_pddump_query(bytes: &[u8]) -> Option<Query> {
    parse_named_query(bytes, PDDUMP_QUERY_NAME, PDDUMP_QUERY_BYTES)
}

fn parse_named_query(bytes: &[u8], name: &[u8], expected_bytes: usize) -> Option<Query> {
    if bytes.len() != expected_bytes
        || u32::from_le_bytes(bytes[0..4].try_into().ok()?) != 1
        || u32::from_le_bytes(bytes[4..8].try_into().ok()?) != 1
        || u32::from_le_bytes(bytes[8..12].try_into().ok()?) != REMOTE_NODE
        || u32::from_le_bytes(bytes[12..16].try_into().ok()?) == 0
        || u32::from_le_bytes(bytes[12..16].try_into().ok()?) >= CONTROL_PORT
        || u32::from_le_bytes(bytes[16..20].try_into().ok()?) != 0
        || u32::from_le_bytes(bytes[20..24].try_into().ok()?)
            != (bytes.len() - QRTR_HEADER_BYTES) as u32
        || u32::from_le_bytes(bytes[24..28].try_into().ok()?) != LOCAL_NODE
        || u32::from_le_bytes(bytes[28..32].try_into().ok()?) != LOCAL_PORT
    {
        return None;
    }

    let qmi = &bytes[32..];
    if qmi[0] != 0
        || u16::from_le_bytes(qmi[3..5].try_into().ok()?) != GET_DOMAIN_LIST_MESSAGE_ID
        || u16::from_le_bytes(qmi[5..7].try_into().ok()?) as usize != qmi.len() - 7
        || qmi.len() != QMI_HEADER_BYTES + 3 + name.len()
        || qmi[7] != 1
        || u16::from_le_bytes(qmi[8..10].try_into().ok()?) as usize != name.len()
        || &qmi[10..] != name
    {
        return None;
    }

    Some(Query {
        remote_port: u32::from_le_bytes(bytes[12..16].try_into().ok()?),
        transaction: u16::from_le_bytes(qmi[1..3].try_into().ok()?),
    })
}

/// Encode a successful no-match result for a query absent from the captured
/// Sunfish service maps. The optional domain-list TLV is omitted and the
/// response reports zero total domains.
pub fn pdr_empty_response(query: Query, out: &mut [u8; PDR_EMPTY_RESPONSE_BYTES]) {
    out.fill(0);
    let words = [
        1,
        1,
        LOCAL_NODE,
        LOCAL_PORT,
        0,
        (QMI_HEADER_BYTES + PDR_EMPTY_QMI_PAYLOAD_BYTES) as u32,
        REMOTE_NODE,
        query.remote_port,
    ];
    for (i, word) in words.into_iter().enumerate() {
        put32(out, i * 4, word);
    }

    let qmi = &mut out[QRTR_HEADER_BYTES..];
    qmi[0] = 2;
    qmi[1..3].copy_from_slice(&query.transaction.to_le_bytes());
    qmi[3..5].copy_from_slice(&GET_DOMAIN_LIST_MESSAGE_ID.to_le_bytes());
    put16(qmi, 5, PDR_EMPTY_QMI_PAYLOAD_BYTES as u16);
    qmi[7..14].copy_from_slice(&[0x02, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
    qmi[14..19].copy_from_slice(&[0x10, 0x02, 0x00, 0x00, 0x00]);
    qmi[19..24].copy_from_slice(&[0x11, 0x02, 0x00, 0x01, 0x00]);
}

/// Encode the two captured `tms/servreg` records from modemr.jsn/modemuw.jsn.
/// `out` includes the 32-byte QRTR header and the complete QMI service response.
pub fn response(query: Query, out: &mut [u8; RESPONSE_BYTES]) {
    out.fill(0);
    let words = [
        1,
        1,
        LOCAL_NODE,
        LOCAL_PORT,
        0,
        (QMI_HEADER_BYTES + QMI_PAYLOAD_BYTES) as u32,
        REMOTE_NODE,
        query.remote_port,
    ];
    for (i, word) in words.into_iter().enumerate() {
        put32(out, i * 4, word);
    }

    let qmi = &mut out[32..];
    qmi[0] = 2;
    qmi[1..3].copy_from_slice(&query.transaction.to_le_bytes());
    qmi[3..5].copy_from_slice(&GET_DOMAIN_LIST_MESSAGE_ID.to_le_bytes());
    put16(qmi, 5, QMI_PAYLOAD_BYTES as u16);
    let mut at = 7;

    // QMI result: success, no error.
    qmi[at..at + 7].copy_from_slice(&[0x02, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
    at += 7;
    // Two matching domains, database revision one.
    qmi[at..at + 5].copy_from_slice(&[0x10, 0x02, 0x00, 0x02, 0x00]);
    at += 5;
    qmi[at..at + 5].copy_from_slice(&[0x11, 0x02, 0x00, 0x01, 0x00]);
    at += 5;

    // Each entry includes the service_data_valid byte and service_data u32,
    // even when service_data_valid is zero.
    qmi[at] = 0x12;
    put16(qmi, at + 1, DOMAIN_LIST_VALUE_BYTES as u16);
    at += 3;
    qmi[at] = 2;
    at += 1;
    for name in [ROOT_DOMAIN_NAME, WLAN_DOMAIN_NAME] {
        put16(qmi, at, name.len() as u16);
        at += 2;
        qmi[at..at + name.len()].copy_from_slice(name);
        at += name.len();
        put32(qmi, at, DOMAIN_INSTANCE);
        at += 4;
        qmi[at] = 0; // service_data_valid: no service_data is published.
        at += 1;
        put32(qmi, at, 0);
        at += 4;
    }
    debug_assert_eq!(at, qmi.len());
}

/// Wrap a QRTR datagram in the fixed GLINK packet envelope, padding only to
/// the next 8-byte boundary. The chunk field remains the unpadded datagram size.
pub fn glink_data_frame<const N: usize>(intent_id: u32, datagram: &[u8]) -> Option<[u8; N]> {
    let total = (16usize.checked_add(datagram.len())?.checked_add(7)?) & !7;
    if total != N || datagram.len() > u32::MAX as usize {
        return None;
    }
    let mut out = [0; N];
    out[..8].copy_from_slice(&[9, 0, 1, 0, 0, 0, 0, 0]);
    put32(&mut out, 4, intent_id);
    put32(&mut out, 8, datagram.len() as u32);
    out[16..16 + datagram.len()].copy_from_slice(datagram);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_frame(name: &[u8]) -> Vec<u8> {
        let mut b = vec![0; QRTR_HEADER_BYTES + QMI_HEADER_BYTES + 3 + name.len()];
        let words = [
            1,
            1,
            REMOTE_NODE,
            0x1234,
            0,
            (QMI_HEADER_BYTES + 3 + name.len()) as u32,
            LOCAL_NODE,
            LOCAL_PORT,
        ];
        for (i, word) in words.into_iter().enumerate() {
            put32(&mut b, i * 4, word);
        }
        let qmi = &mut b[32..];
        qmi[0] = 0;
        qmi[1..3].copy_from_slice(&9u16.to_le_bytes());
        qmi[3..5].copy_from_slice(&GET_DOMAIN_LIST_MESSAGE_ID.to_le_bytes());
        qmi[5..7].copy_from_slice(&((3 + name.len()) as u16).to_le_bytes());
        qmi[7] = 1;
        qmi[8..10].copy_from_slice(&(name.len() as u16).to_le_bytes());
        qmi[10..].copy_from_slice(name);
        b
    }

    #[test]
    fn announces_only_servreg_endpoint() {
        let b = announcement();
        assert_eq!(u32::from_le_bytes(b[32..36].try_into().unwrap()), 4);
        assert_eq!(
            u32::from_le_bytes(b[36..40].try_into().unwrap()),
            SERVICE_ID
        );
        assert_eq!(
            u32::from_le_bytes(b[40..44].try_into().unwrap()),
            SERVICE_VERSION_INSTANCE
        );
        assert_eq!(
            u32::from_le_bytes(b[48..52].try_into().unwrap()),
            LOCAL_PORT
        );
    }

    #[test]
    fn accepts_only_exact_tms_servreg_query() {
        let b = request_frame(QUERY_NAME);
        assert_eq!(
            parse_query(&b),
            Some(Query {
                remote_port: 0x1234,
                transaction: 9
            })
        );
        let mut wrong_message = b.clone();
        wrong_message[35..37].copy_from_slice(&0x20u16.to_le_bytes());
        assert_eq!(parse_query(&wrong_message), None);
        let mut bad = b.clone();
        bad[42] ^= 1;
        assert_eq!(parse_query(&bad), None);
        let mut bad = b.clone();
        bad[24..28].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(parse_query(&bad), None);
        let mut bad = b;
        bad[12..16].copy_from_slice(&CONTROL_PORT.to_le_bytes());
        assert_eq!(parse_query(&bad), None);
    }

    #[test]
    fn accepts_only_exact_pdr_enabled_query_and_builds_empty_success_response() {
        let request = request_frame(PDR_QUERY_NAME);
        let query = parse_pdr_query(&request).unwrap();
        assert_eq!(query.remote_port, 0x1234);
        assert_eq!(query.transaction, 9);
        assert_eq!(parse_query(&request), None);

        let mut response = [0; PDR_EMPTY_RESPONSE_BYTES];
        pdr_empty_response(query, &mut response);
        assert_eq!(PDR_QUERY_BYTES, 57);
        assert_eq!(PDR_EMPTY_RESPONSE_BYTES, 56);
        assert_eq!(PDR_EMPTY_RESPONSE_GLINK_BYTES, 72);
        assert_eq!(u32::from_le_bytes(response[20..24].try_into().unwrap()), 24);
        let qmi = &response[QRTR_HEADER_BYTES..];
        assert_eq!(qmi[0], 2);
        assert_eq!(u16::from_le_bytes(qmi[1..3].try_into().unwrap()), 9);
        assert_eq!(u16::from_le_bytes(qmi[3..5].try_into().unwrap()), 0x21);
        assert_eq!(u16::from_le_bytes(qmi[5..7].try_into().unwrap()), 17);
        assert_eq!(
            &qmi[7..],
            &[0x02, 4, 0, 0, 0, 0, 0, 0x10, 2, 0, 0, 0, 0x11, 2, 0, 1, 0]
        );

        let frame = glink_data_frame::<PDR_EMPTY_RESPONSE_GLINK_BYTES>(2, &response).unwrap();
        assert_eq!(u32::from_le_bytes(frame[8..12].try_into().unwrap()), 56);
        assert_eq!(&frame[16..72], &response);
        assert_eq!(&frame[72..], &[0; 0]);
    }

    #[test]
    fn accepts_only_exact_pddump_disabled_query() {
        let request = request_frame(PDDUMP_QUERY_NAME);
        let query = parse_pddump_query(&request).unwrap();
        assert_eq!(PDDUMP_QUERY_BYTES, 61);
        assert_eq!(query.remote_port, 0x1234);
        assert_eq!(query.transaction, 9);
        assert_eq!(parse_pdr_query(&request), None);
        assert_eq!(parse_query(&request), None);

        let mut wrong_name = request;
        let last_name_byte = QRTR_HEADER_BYTES + QMI_HEADER_BYTES + 3 + PDDUMP_QUERY_NAME.len() - 1;
        wrong_name[last_name_byte] ^= 1;
        assert_eq!(parse_pddump_query(&wrong_name), None);
    }

    #[test]
    fn response_preserves_transaction_and_returns_both_complete_mappings() {
        let query = parse_query(&request_frame(QUERY_NAME)).unwrap();
        let mut out = [0; RESPONSE_BYTES];
        response(query, &mut out);
        assert_eq!(RESPONSE_BYTES, 116);
        assert_eq!(RESPONSE_GLINK_BYTES, 136);
        assert_eq!(u32::from_le_bytes(out[20..24].try_into().unwrap()), 84);
        let qmi = &out[32..];
        assert_eq!(qmi[0], 2);
        assert_eq!(u16::from_le_bytes(qmi[1..3].try_into().unwrap()), 9);
        assert_eq!(
            u16::from_le_bytes(qmi[3..5].try_into().unwrap()),
            GET_DOMAIN_LIST_MESSAGE_ID
        );
        assert_eq!(u16::from_le_bytes(qmi[5..7].try_into().unwrap()), 77);
        assert_eq!(qmi[7], 0x02);
        assert_eq!(u16::from_le_bytes(qmi[8..10].try_into().unwrap()), 4);
        assert_eq!(qmi[14..19], [0x10, 2, 0, 2, 0]);
        assert_eq!(qmi[19..24], [0x11, 2, 0, 1, 0]);
        assert_eq!(qmi[24], 0x12);
        assert_eq!(u16::from_le_bytes(qmi[25..27].try_into().unwrap()), 57);
        assert_eq!(qmi[27], 2);

        let mut at = 28;
        for name in [ROOT_DOMAIN_NAME, WLAN_DOMAIN_NAME] {
            assert_eq!(
                u16::from_le_bytes(qmi[at..at + 2].try_into().unwrap()) as usize,
                name.len()
            );
            at += 2;
            assert_eq!(&qmi[at..at + name.len()], name);
            at += name.len();
            assert_eq!(
                u32::from_le_bytes(qmi[at..at + 4].try_into().unwrap()),
                DOMAIN_INSTANCE
            );
            at += 4;
            assert_eq!(qmi[at], 0); // service_data_valid
            at += 1;
            assert_eq!(u32::from_le_bytes(qmi[at..at + 4].try_into().unwrap()), 0);
            at += 4;
        }
        assert_eq!(at, qmi.len());

        let frame = glink_data_frame::<RESPONSE_GLINK_BYTES>(2, &out).unwrap();
        assert_eq!(
            u32::from_le_bytes(frame[8..12].try_into().unwrap()),
            RESPONSE_BYTES as u32
        );
        assert_eq!(&frame[16..16 + RESPONSE_BYTES], &out);
        assert_eq!(&frame[16 + RESPONSE_BYTES..], &[0; 4]);
    }

    #[test]
    fn glink_data_padding_does_not_change_qrtr_chunk_length() {
        let qrtr = announcement();
        let frame = glink_data_frame::<72>(1, &qrtr).unwrap();
        assert_eq!(u32::from_le_bytes(frame[8..12].try_into().unwrap()), 52);
        assert_eq!(u32::from_le_bytes(frame[4..8].try_into().unwrap()), 1);
        assert_eq!(&frame[16..68], &qrtr);
        assert_eq!(&frame[68..], &[0; 4]);
    }
}
