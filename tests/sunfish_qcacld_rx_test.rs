#![allow(dead_code)]
#![forbid(unsafe_code)]

mod sunfish_smmu_lpae {
    pub const SMMU_LPAE_PAGE_BYTES: u64 = 4096;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LpaePageMapping {
        pub iova_page: u64,
        pub physical_page: u64,
    }
}

#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;

#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;

use sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use sunfish_htt_rx_ring::SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS;
use sunfish_qcacld_rx::{Error, ParseOptions, RxDescriptor, decode_single_buffer};

fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn blank_buffer() -> Vec<u8> {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    vec![0; profile.rx_buffer_bytes() as usize]
}

fn place_word(bytes: &mut [u8], word_index: u16, value: u32) {
    put_word(bytes, usize::from(word_index) * 4, value);
}

#[test]
fn decodes_completed_sunfish_qcacld_descriptor_fields() {
    let mut buffer = blank_buffer();
    buffer[0] = 0xff;
    place_word(
        &mut buffer,
        SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words,
        (1 << 31) | (1 << 27) | (1 << 17),
    );
    place_word(
        &mut buffer,
        SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.fragment_info_words,
        2 << 16,
    );
    place_word(
        &mut buffer,
        SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words,
        4000,
    );
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    put_word(&mut buffer, msdu_start + 8, 2 << 8);
    let msdu_end = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words) * 4;
    put_word(&mut buffer, msdu_end + 16, (1 << 14) | (1 << 15));
    put_word(&mut buffer, msdu_end + 32, 5 << 10);

    let parsed = RxDescriptor::decode(
        &buffer,
        ParseOptions {
            l3_header_padding_active: true,
        },
    )
    .unwrap();
    assert!(parsed.firmware.discard);
    assert!(parsed.firmware.forward);
    assert!(parsed.firmware.any_error);
    assert!(parsed.firmware.duplicate_error);
    assert!(parsed.firmware.ipa_indication);
    assert!(parsed.firmware.inspect);
    assert_eq!(parsed.firmware.extension, 3);
    assert!(parsed.mpdu_length_error);
    assert!(parsed.msdu_length_error);
    assert_eq!(parsed.more_buffers, 2);
    assert_eq!(parsed.msdu_length, 4000);
    assert_eq!(
        parsed.decap_format,
        sunfish_qcacld_rx::SUNFISH_RX_DECAP_FORMAT_8023
    );
    assert!(parsed.first_msdu);
    assert!(parsed.last_msdu);
    assert_eq!(parsed.l3_header_padding, 5);
}

#[test]
fn single_buffer_payload_starts_after_descriptor_and_active_l3_padding() {
    let mut buffer = blank_buffer();
    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    let msdu_end = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, msdu_start, 8);
    put_word(&mut buffer, msdu_end + 32, 4 << 10);
    let start = SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes as usize + 4;
    buffer[start..start + 8].copy_from_slice(b"wifi-msd");

    let parsed = RxDescriptor::decode(
        &buffer,
        ParseOptions {
            l3_header_padding_active: true,
        },
    )
    .unwrap();
    assert_eq!(
        decode_single_buffer(
            &buffer,
            ParseOptions {
                l3_header_padding_active: true,
            },
        ),
        Ok((parsed, &buffer[start..start + 8]))
    );
}

#[test]
fn ignored_target_padding_keeps_the_descendant_offset_at_rx_reservation() {
    let mut buffer = blank_buffer();
    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    let msdu_end = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, msdu_start, 3);
    put_word(&mut buffer, msdu_end + 32, 7 << 10);
    let start = SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes as usize;
    buffer[start..start + 3].copy_from_slice(b"abc");

    let parsed = RxDescriptor::decode(&buffer, ParseOptions::default()).unwrap();
    assert_eq!(parsed.l3_header_padding, 0);
    assert_eq!(
        decode_single_buffer(&buffer, ParseOptions::default()),
        Ok((parsed, &buffer[start..start + 3]))
    );
}

#[test]
fn rejects_short_or_incomplete_descriptors_and_unmapped_payload_spans() {
    let mut buffer = blank_buffer();
    assert_eq!(
        RxDescriptor::decode(&buffer[..32], ParseOptions::default()),
        Err(Error::DescriptorTooShort)
    );
    assert_eq!(
        RxDescriptor::decode(&buffer, ParseOptions::default()),
        Err(Error::NotComplete)
    );

    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, msdu_start, 2000);
    assert_eq!(
        RxDescriptor::decode(&buffer, ParseOptions::default()),
        Err(Error::MsduLengthOutOfRange)
    );
}

#[test]
fn chained_msdu_and_extensions_never_become_a_single_slice() {
    let mut buffer = blank_buffer();
    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let frag = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.fragment_info_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, frag, 1 << 16);
    put_word(&mut buffer, msdu_start, 3000);
    let chained = RxDescriptor::decode(&buffer, ParseOptions::default()).unwrap();
    assert_eq!(chained.more_buffers, 1);
    assert_eq!(
        decode_single_buffer(&buffer, ParseOptions::default()),
        Err(Error::ChainedMsdu)
    );

    put_word(&mut buffer, frag, 0);
    put_word(&mut buffer, msdu_start, 64);
    buffer[0] = 1 << 6;
    let extended = RxDescriptor::decode(&buffer, ParseOptions::default()).unwrap();
    assert_eq!(extended.firmware.extension, 1);
    assert_eq!(
        decode_single_buffer(&buffer, ParseOptions::default()),
        Err(Error::FirmwareExtension)
    );
}

#[test]
fn payload_requires_the_full_fixed_ring_buffer() {
    let mut buffer = blank_buffer();
    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, msdu_start, 1);
    assert_eq!(
        decode_single_buffer(&buffer[..buffer.len() - 1], ParseOptions::default())
            .map(|(_, payload)| payload),
        Err(Error::BufferTooShort)
    );
}
