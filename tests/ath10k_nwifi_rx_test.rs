#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;

use ath10k_nwifi_rx::{
    BeaconParseError, BssAdvertisementError, CryptoParameters, Error, FourAddressPadding,
    FrameHeaderError, FrameKind, InformationElementError, RX_HEADER_STATUS_BYTES, RebuiltFrame,
    inspect_frame, parse_beacon_probe_response, parse_bss_advertisement, rebuild_data_msdu,
};

const LLC_SNAP_IPV4: [u8; 8] = [0xaa, 0xaa, 0x03, 0, 0, 0, 0x08, 0];
const DEST: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
const AP: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
const SOURCE: [u8; 6] = [0x02, 0, 0, 0, 0, 3];
const FOURTH: [u8; 6] = [0x02, 0, 0, 0, 0, 4];

fn put_fc(header: &mut [u8], fc: u16) {
    header[..2].copy_from_slice(&fc.to_le_bytes());
}

fn put_address(header: &mut [u8], offset: usize, address: &[u8; 6]) {
    header[offset..offset + 6].copy_from_slice(address);
}

fn put_three_address_header(header: &mut [u8], fc: u16) {
    put_fc(header, fc);
    put_address(header, 4, &DEST);
    put_address(header, 10, &AP);
    put_address(header, 16, &SOURCE);
}

fn beacon_with_ies(ies: &[u8]) -> Vec<u8> {
    let mut frame = vec![0; 24 + 12 + ies.len()];
    put_fc(&mut frame, 0x0080);
    put_address(&mut frame, 16, &AP);
    frame[24 + 12..].copy_from_slice(ies);
    frame
}

fn header_status(fc: u16, qos: bool) -> [u8; RX_HEADER_STATUS_BYTES] {
    let mut header = [0x5a; RX_HEADER_STATUS_BYTES];
    put_three_address_header(&mut header, fc);
    if qos {
        header[24..26].copy_from_slice(&[0x34, 0x12]);
    }
    header
}

#[test]
fn restores_qos_data_header_addresses_crypto_and_llc_body() {
    let first = header_status(0x0288, true); // QoS data, FromDS
    let mut decapped = [0; 24 + 8 + 4];
    put_three_address_header(&mut decapped, 0x0208); // Native WiFi omits QoS
    decapped[24..32].copy_from_slice(&LLC_SNAP_IPV4);
    decapped[32..].copy_from_slice(&[1, 2, 3, 4]);
    let mut output = [0xa5; 64];
    let mut expected_crypto = [0; 8];
    expected_crypto.copy_from_slice(&[0x71; 8]);
    let mut first_with_crypto = first;
    first_with_crypto[26..34].copy_from_slice(&expected_crypto);

    let rebuilt = rebuild_data_msdu(
        FourAddressPadding::PaddedToFour,
        &first_with_crypto,
        &decapped,
        false,
        CryptoParameters::TkipOrAesIv,
        &mut output,
    )
    .unwrap();

    assert_eq!(
        rebuilt,
        RebuiltFrame {
            frame_bytes: 26 + 8 + 12,
            header_bytes: 26,
            crypto_parameter_bytes: 8,
            body_bytes: 12,
        }
    );
    assert_eq!(u16::from_le_bytes([output[0], output[1]]), 0x0288);
    assert_eq!(&output[4..10], &DEST);
    assert_eq!(&output[10..16], &AP);
    assert_eq!(&output[16..22], &SOURCE);
    assert_eq!(&output[26..34], &[0x71; 8]);
    assert_eq!(&output[34..46], &decapped[24..]);
    assert!(output[46..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn restores_four_byte_wep_crypto_parameters() {
    let first = header_status(0x0008, false);
    let mut first_with_crypto = first;
    first_with_crypto[24..28].copy_from_slice(&[0x10, 0x20, 0x30, 0x40]);
    let mut decapped = [0u8; 24 + 8 + 2];
    put_three_address_header(&mut decapped, 0x0008);
    decapped[24..32].copy_from_slice(&LLC_SNAP_IPV4);
    decapped[32..].copy_from_slice(&[0xaa, 0xbb]);
    let mut output = [0; 48];

    let rebuilt = rebuild_data_msdu(
        FourAddressPadding::PaddedToFour,
        &first_with_crypto,
        &decapped,
        false,
        CryptoParameters::WepIv,
        &mut output,
    )
    .unwrap();

    assert_eq!(rebuilt.crypto_parameter_bytes, 4);
    assert_eq!(rebuilt.frame_bytes, 24 + 4 + 10);
    assert_eq!(&output[24..28], &[0x10, 0x20, 0x30, 0x40]);
    assert_eq!(&output[28..38], &decapped[24..]);
}

#[test]
fn reconstructs_four_address_header_with_source_padding() {
    let mut first = [0u8; RX_HEADER_STATUS_BYTES];
    put_fc(&mut first, 0x0308);
    put_address(&mut first, 4, &AP);
    put_address(&mut first, 10, &DEST);
    put_address(&mut first, 16, &SOURCE);
    put_address(&mut first, 24, &FOURTH);

    let mut decapped = [0x77; 32 + 8];
    put_fc(&mut decapped, 0x0308);
    put_address(&mut decapped, 4, &DEST);
    put_address(&mut decapped, 10, &AP);
    put_address(&mut decapped, 16, &SOURCE);
    put_address(&mut decapped, 24, &FOURTH);
    decapped[32..].copy_from_slice(&LLC_SNAP_IPV4);
    let mut output = [0; 64];

    let rebuilt = rebuild_data_msdu(
        FourAddressPadding::PaddedToFour,
        &first,
        &decapped,
        true,
        CryptoParameters::TkipOrAesIv,
        &mut output,
    )
    .unwrap();

    assert_eq!(rebuilt.frame_bytes, 30 + 8);
    assert_eq!(rebuilt.header_bytes, 30);
    assert_eq!(rebuilt.crypto_parameter_bytes, 0);
    assert_eq!(&output[16..22], &SOURCE);
    assert_eq!(&output[24..30], &FOURTH);
    assert_eq!(&output[30..38], &LLC_SNAP_IPV4);
}

#[test]
fn honors_firmware_feature_that_disables_four_address_padding() {
    let mut first = [0u8; RX_HEADER_STATUS_BYTES];
    put_three_address_header(&mut first, 0x0308);
    put_address(&mut first, 24, &FOURTH);
    let mut decapped = [0u8; 30 + 2];
    put_fc(&mut decapped, 0x0308);
    put_address(&mut decapped, 4, &DEST);
    put_address(&mut decapped, 10, &AP);
    put_address(&mut decapped, 16, &SOURCE);
    put_address(&mut decapped, 24, &FOURTH);
    decapped[30..].copy_from_slice(&[0xaa, 0xbb]);
    let mut output = [0; 40];

    let rebuilt = rebuild_data_msdu(
        FourAddressPadding::Unpadded,
        &first,
        &decapped,
        true,
        CryptoParameters::None,
        &mut output,
    )
    .unwrap();

    assert_eq!(rebuilt.frame_bytes, 30 + 2);
    assert_eq!(&output[30..32], &[0xaa, 0xbb]);
}

#[test]
fn rejects_unexpected_qos_native_header_without_changing_output() {
    let first = header_status(0x0088, true);
    let mut decapped = [0u8; 32];
    put_three_address_header(&mut decapped, 0x0088);
    let mut output = [0xa5; 64];

    assert_eq!(
        rebuild_data_msdu(
            FourAddressPadding::PaddedToFour,
            &first,
            &decapped,
            true,
            CryptoParameters::None,
            &mut output,
        ),
        Err(Error::UnexpectedQosInDecappedHeader)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn rejects_non_data_header_and_preserves_output() {
    let first = header_status(0x0008, false);
    let mut decapped = [0u8; 32];
    put_three_address_header(&mut decapped, 0x0000); // Management frame
    let mut output = [0xa5; 64];

    assert_eq!(
        rebuild_data_msdu(
            FourAddressPadding::PaddedToFour,
            &first,
            &decapped,
            true,
            CryptoParameters::None,
            &mut output,
        ),
        Err(Error::NonDataFrame)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn output_and_crypto_bounds_fail_before_writing() {
    let first = header_status(0x0288, true);
    let mut decapped = [0u8; 24 + 8 + 4];
    put_three_address_header(&mut decapped, 0x0208);
    decapped[24..32].copy_from_slice(&LLC_SNAP_IPV4);
    let mut output = [0xa5; 37];

    assert_eq!(
        rebuild_data_msdu(
            FourAddressPadding::PaddedToFour,
            &first,
            &decapped,
            true,
            CryptoParameters::None,
            &mut output,
        ),
        Err(Error::OutputTooSmall)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    let mut encrypted_output = [0xa5; 45];
    assert_eq!(
        rebuild_data_msdu(
            FourAddressPadding::PaddedToFour,
            &first,
            &decapped,
            false,
            CryptoParameters::TkipOrAesIv,
            &mut encrypted_output,
        ),
        Err(Error::OutputTooSmall)
    );
    assert!(encrypted_output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn inspects_management_control_data_and_extension_headers() {
    let mut management = [0; 26];
    put_fc(&mut management, 0x0080); // Beacon: management subtype 8.
    assert_eq!(
        inspect_frame(&management),
        Ok(ath10k_nwifi_rx::FrameView {
            frame_control: 0x0080,
            kind: FrameKind::Management,
            subtype: 8,
            header: &management[..24],
            body: &management[24..],
        })
    );

    let mut ordered_management = [0; 28];
    put_fc(&mut ordered_management, 0x8080);
    assert_eq!(inspect_frame(&ordered_management).unwrap().header.len(), 28);

    let mut ordinary_data = [0; 24];
    put_fc(&mut ordinary_data, 0x0008);
    assert_eq!(inspect_frame(&ordinary_data).unwrap().header.len(), 24);

    let mut four_address_data = [0; 30];
    put_fc(&mut four_address_data, 0x0308);
    assert_eq!(inspect_frame(&four_address_data).unwrap().header.len(), 30);

    let mut qos_data = [0; 30];
    put_fc(&mut qos_data, 0x8088); // QoS data with HT Control.
    assert_eq!(inspect_frame(&qos_data).unwrap().header.len(), 30);

    let mut ack = [0; 10];
    put_fc(&mut ack, 0x00d4);
    let ack_view = inspect_frame(&ack).unwrap();
    assert_eq!(ack_view.kind, FrameKind::Control);
    assert_eq!(ack_view.subtype, 13);
    assert_eq!(ack_view.header.len(), 10);

    let mut rts = [0; 16];
    put_fc(&mut rts, 0x00b4);
    assert_eq!(inspect_frame(&rts).unwrap().header.len(), 16);

    let mut extension = [0; 4];
    put_fc(&mut extension, 0x000c);
    assert_eq!(
        inspect_frame(&extension).unwrap().kind,
        FrameKind::Extension
    );
}

#[test]
fn frame_inspector_rejects_short_invalid_and_truncated_headers() {
    assert_eq!(
        inspect_frame(&[0]),
        Err(FrameHeaderError::FrameControlTooShort)
    );
    assert_eq!(
        inspect_frame(&[0x01, 0x00]),
        Err(FrameHeaderError::UnsupportedVersion)
    );

    let mut beacon = [0; 23];
    put_fc(&mut beacon, 0x0080);
    assert_eq!(
        inspect_frame(&beacon),
        Err(FrameHeaderError::HeaderTruncated)
    );

    let mut ack = [0; 9];
    put_fc(&mut ack, 0x00d4);
    assert_eq!(inspect_frame(&ack), Err(FrameHeaderError::HeaderTruncated));
}

#[test]
fn parses_beacon_fixed_fields_and_borrowed_information_elements() {
    let mut frame = [0; 24 + 12 + 8];
    put_fc(&mut frame, 0x0080); // Beacon.
    let body = &mut frame[24..];
    body[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    body[8..10].copy_from_slice(&100u16.to_le_bytes());
    body[10..12].copy_from_slice(&0x0431u16.to_le_bytes());
    body[12..20].copy_from_slice(&[0, 3, b'l', b'a', b'b', 3, 1, 6]);

    let mut beacon = parse_beacon_probe_response(&frame).unwrap();
    assert_eq!(beacon.timestamp, 0x0807_0605_0403_0201);
    assert_eq!(beacon.beacon_interval, 100);
    assert_eq!(beacon.capabilities, 0x0431);
    let ssid = beacon.information_elements.next().unwrap().unwrap();
    assert_eq!(ssid.id, 0);
    assert_eq!(ssid.value, b"lab");
    let channel = beacon.information_elements.next().unwrap().unwrap();
    assert_eq!(channel.id, 3);
    assert_eq!(channel.value, &[6]);
    assert!(beacon.information_elements.next().is_none());
}

#[test]
fn parses_probe_response_and_rejects_other_or_truncated_management_bodies() {
    let mut probe_response = [0; 24 + 12];
    put_fc(&mut probe_response, 0x0050); // Probe response subtype 5.
    assert!(parse_beacon_probe_response(&probe_response).is_ok());

    let mut probe_request = [0; 24];
    put_fc(&mut probe_request, 0x0040); // Probe request.
    assert!(matches!(
        parse_beacon_probe_response(&probe_request),
        Err(BeaconParseError::NotBeaconOrProbeResponse)
    ));

    let mut short_beacon = [0; 24 + 11];
    put_fc(&mut short_beacon, 0x0080);
    assert!(matches!(
        parse_beacon_probe_response(&short_beacon),
        Err(BeaconParseError::FixedFieldsTruncated)
    ));

    let mut malformed_ie = [0; 24 + 12 + 3];
    put_fc(&mut malformed_ie, 0x0080);
    malformed_ie[24 + 12..].copy_from_slice(&[0, 4, b'x']);
    assert!(matches!(
        parse_beacon_probe_response(&malformed_ie),
        Err(BeaconParseError::MalformedInformationElements(
            InformationElementError::TruncatedValue
        ))
    ));
}

#[test]
fn information_element_iterator_fails_closed_on_truncated_records() {
    let mut partial_header = ath10k_nwifi_rx::InformationElementIter::new(&[0]);
    assert_eq!(
        partial_header.next(),
        Some(Err(InformationElementError::TruncatedHeader))
    );
    assert!(partial_header.next().is_none());

    let mut partial_value = ath10k_nwifi_rx::InformationElementIter::new(&[48, 4, 1, 2, 3]);
    assert_eq!(
        partial_value.next(),
        Some(Err(InformationElementError::TruncatedValue))
    );
    assert!(partial_value.next().is_none());

    let mut zero_length = ath10k_nwifi_rx::InformationElementIter::new(&[221, 0]);
    let element = zero_length.next().unwrap().unwrap();
    assert_eq!(element.id, 221);
    assert!(element.value.is_empty());
}

#[test]
fn extracts_borrowed_bss_identity_rates_channel_and_rsn() {
    let mut ht_operation = [0; 22];
    ht_operation[0] = 6;
    let mut ies = vec![0, 0]; // Hidden SSID.
    ies.extend_from_slice(&[1, 3, 0x82, 0x84, 0x8b]);
    ies.extend_from_slice(&[50, 2, 0x96, 0x0c]);
    ies.extend_from_slice(&[3, 1, 6]);
    ies.extend_from_slice(&[61, ht_operation.len() as u8]);
    ies.extend_from_slice(&ht_operation);
    ies.extend_from_slice(&[48, 4, 1, 0, 0xaa, 0xbb]);
    ies.extend_from_slice(&[221, 0]); // Unknown vendor IE remains accepted.
    let frame = beacon_with_ies(&ies);

    let bss = parse_bss_advertisement(&frame).unwrap();
    assert_eq!(bss.bssid, AP);
    assert_eq!(bss.ssid, Some(&[][..]));
    assert_eq!(bss.supported_rates, Some(&[0x82, 0x84, 0x8b][..]));
    assert_eq!(bss.extended_rates, Some(&[0x96, 0x0c][..]));
    assert_eq!(bss.primary_channel, Some(6));
    assert_eq!(bss.rsn, Some(&[1, 0, 0xaa, 0xbb][..]));
}

#[test]
fn bss_parser_uses_ht_channel_and_rejects_bad_identity_or_duplicate_ies() {
    let mut ht_operation = [0; 22];
    ht_operation[0] = 44;
    let mut ies = vec![61, ht_operation.len() as u8];
    ies.extend_from_slice(&ht_operation);
    let frame = beacon_with_ies(&ies);
    assert_eq!(
        parse_bss_advertisement(&frame).unwrap().primary_channel,
        Some(44)
    );

    let mut invalid_bssid = frame.clone();
    invalid_bssid[16] |= 1;
    assert_eq!(
        parse_bss_advertisement(&invalid_bssid),
        Err(BssAdvertisementError::InvalidBssid)
    );

    let duplicate_ssid = beacon_with_ies(&[0, 1, b'a', 0, 1, b'b']);
    assert_eq!(
        parse_bss_advertisement(&duplicate_ssid),
        Err(BssAdvertisementError::DuplicateElement(0))
    );
}

#[test]
fn bss_parser_rejects_invalid_rates_element_sizes_and_conflicting_channels() {
    let invalid_rates = beacon_with_ies(&[1, 9, 2, 4, 11, 22, 12, 18, 24, 36, 48]);
    assert_eq!(
        parse_bss_advertisement(&invalid_rates),
        Err(BssAdvertisementError::InvalidElementLength(1))
    );

    let mut ht_operation = [0; 22];
    ht_operation[0] = 6;
    let mut conflicting_ies = vec![3, 1, 1, 61, ht_operation.len() as u8];
    conflicting_ies.extend_from_slice(&ht_operation);
    let conflicting = beacon_with_ies(&conflicting_ies);
    assert_eq!(
        parse_bss_advertisement(&conflicting),
        Err(BssAdvertisementError::ConflictingPrimaryChannels)
    );

    let invalid_rsn = beacon_with_ies(&[48, 1, 1]);
    assert_eq!(
        parse_bss_advertisement(&invalid_rsn),
        Err(BssAdvertisementError::InvalidElementLength(48))
    );
}
