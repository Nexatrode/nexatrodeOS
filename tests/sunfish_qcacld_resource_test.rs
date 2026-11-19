#![allow(dead_code)]

#[path = "../rust/src/qcacld_wmi.rs"]
mod qcacld_wmi;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/sunfish_qcacld_resource.rs"]
mod sunfish_qcacld_resource;

use qcacld_wmi::{
    QcacldPeerLimit, SUNFISH_WMI_INIT_COMMAND_ID, ServiceAvailableView, ServiceReadyView,
    WmiResourceConfig,
};
use sunfish_qcacld_resource::{
    RESOURCE_CONFIG_TLV_BYTES, SunfishQcacldResourceCandidate, SunfishQcacldResourceInputs,
    encode_sunfish_qcacld_wmi_init_frame,
};

const CAPTURED_LINEAGE_RESOURCE_WORDS: [u32; 68] = [
    0x00000004, 0x00000026, 0x00000003, 0x00000003, 0x00000002, 0x0000004c, 0x00000010, 0x00000003,
    0x00000003, 0x00000064, 0x00000064, 0x00000064, 0x00000028, 0x00000002, 0x00000004, 0x00000003,
    0x00000003, 0x00000008, 0x00000000, 0x00000000, 0x00000000, 0x00000400, 0x00000000, 0x00000000,
    0x00000000, 0x00000000, 0x00000000, 0x00000003, 0x00000420, 0x00000002, 0x00000001, 0x00000008,
    0x00000003, 0x00000020, 0x00000002, 0x00000006, 0x00000000, 0x00000001, 0x00000001, 0x00000000,
    0x00000001, 0x00000002, 0x00000002, 0x00180600, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
    0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000001, 0x00000007, 0x00000000,
    0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0xffffffff, 0x00000000, 0x00000000,
    0x00000000, 0x00000000, 0x00000000, 0x00000000,
];

// Raw service words logged by the kernel-matched diagnostic module on the
// Pixel 4a. SERVICE_READY uses QCACLD's four-service-byte stride; these words
// are copied from the actual active WMA handler output.
const CAPTURED_LINEAGE_BASE_SERVICE_WORDS: [u32; 32] = [
    0x0000000d, 0x00000007, 0x0000000f, 0x00000003, 0x0000000f, 0x00000009, 0x00000009, 0x0000000f,
    0x0000000a, 0x00000009, 0x00000002, 0x0000000a, 0x00000001, 0x0000000d, 0x00000007, 0x0000000f,
    0x0000000b, 0x00000004, 0x00000004, 0x0000000b, 0x00000007, 0x00000000, 0x00000008, 0x00000001,
    0x00000009, 0x00000003, 0x0000000d, 0x00000001, 0x00000005, 0x0000000e, 0x00000008, 0x00000005,
];

// SERVICE_AVAILABLE offset 128; words decoded little-endian from the live
// 16-byte bitmap record.
const CAPTURED_LINEAGE_EXT_SERVICE_WORDS: [u32; 4] =
    [0x4531e0c8, 0x00512000, 0x00002000, 0x00000000];

fn append_tlv(output: &mut Vec<u8>, tag: u16, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u16).to_le_bytes());
    output.extend_from_slice(&tag.to_le_bytes());
    output.extend_from_slice(value);
}

fn service_ready_fixture(enabled_services: &[usize]) -> Vec<u8> {
    let mut bitmap = [0u32; 32];
    for service in enabled_services
        .iter()
        .copied()
        .filter(|service| *service < 128)
    {
        bitmap[service / 4] |= 1 << (service % 4);
    }
    service_ready_words_fixture(bitmap)
}

fn service_ready_words_fixture(words: [u32; 32]) -> Vec<u8> {
    let mut fixed = [0u8; 128];
    fixed[..4].copy_from_slice(&0x3226_822eu32.to_le_bytes());
    let abi: [u32; 6] = [0x0100_0000, 762, 0x5f41_4351, 0x0000_4c4d, 0, 0];
    for (offset, word) in abi.into_iter().enumerate() {
        fixed[4 + offset * 4..8 + offset * 4].copy_from_slice(&word.to_le_bytes());
    }

    let mut bitmap = [0u8; 128];
    for (index, word) in words.into_iter().enumerate() {
        bitmap[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }

    let mut bytes = Vec::new();
    append_tlv(&mut bytes, 32, &fixed); // WMI_SERVICE_READY_EVENTID fixed parameter
    append_tlv(&mut bytes, 33, &[0; 36]); // HAL_REG_CAPABILITIES
    append_tlv(&mut bytes, 16, &bitmap); // WMI service bitmap
    bytes
}

fn service_available_fixture(enabled_services: &[usize]) -> Vec<u8> {
    let mut bitmap = [0u32; 4];
    for service in enabled_services
        .iter()
        .copied()
        .filter(|service| (128..256).contains(service))
    {
        let word = (service - 128) / 32;
        bitmap[word] |= 1 << (service & 0x1f);
    }
    service_available_words_fixture(bitmap)
}

fn service_available_words_fixture(bitmap: [u32; 4]) -> Vec<u8> {
    let mut fixed = [0u8; 20];
    fixed[..4].copy_from_slice(&128u32.to_le_bytes());
    for (index, word) in bitmap.into_iter().enumerate() {
        fixed[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes());
    }
    let mut bytes = Vec::new();
    append_tlv(&mut bytes, 559, &fixed);
    bytes
}

fn word(config: &[u8], index: usize) -> u32 {
    let start = 4 + index * 4;
    u32::from_le_bytes(config[start..start + 4].try_into().unwrap())
}

#[test]
fn candidate_serializes_source_derived_counts_and_service_gates() {
    let bytes = service_ready_fixture(&[99]);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_fixture(&[176, 182]);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    let mut inputs =
        SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
    inputs.enable_peer_unmap_confirmation = true;

    let ready_snapshot = ready.sunfish_startup_snapshot().unwrap();
    let available_snapshot = available.snapshot();
    let candidate = SunfishQcacldResourceCandidate::build(
        &ready_snapshot.view(),
        &available_snapshot.view(),
        inputs,
    )
    .unwrap();
    assert_eq!(candidate.words()[0], 4);
    let tlv = candidate.encode_tlv();
    assert_eq!(tlv.len(), RESOURCE_CONFIG_TLV_BYTES);
    let parsed = WmiResourceConfig::parse(&tlv).unwrap();
    assert_eq!(parsed.num_vdevs(), 4);
    assert_eq!(parsed.num_peers(), 38);
    assert_eq!(parsed.num_offload_peers(), 3);
    assert_eq!(parsed.num_offload_reorder_buffers(), 3);
    assert_eq!(parsed.num_peer_keys(), 2);
    assert_eq!(parsed.num_tids(), 76);
    assert_eq!(word(&tlv, 13), 2); // normal Ethernet decapsulation
    assert_eq!(word(&tlv, 14), 4); // configured max scan count
    assert_eq!(word(&tlv, 28), 1056);
    assert_eq!(word(&tlv, 35), 6);
    assert_eq!(word(&tlv, 43), (1 << 9) | (1 << 10) | (1 << 19) | (1 << 20));
    assert_eq!(word(&tlv, 53), 1); // WMI_SERVICE_DEPRECATED_REPLACE
    assert_eq!(word(&tlv, 54), 7); // CDS_DBS_SCAN_CLIENTS_MAX
    assert_eq!(word(&tlv, 67), 0); // flags2 remains zero
}

#[test]
fn observed_lineage_effective_resources_reproduce_the_captured_wmi_init_block() {
    let bytes = service_ready_words_fixture(CAPTURED_LINEAGE_BASE_SERVICE_WORDS);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_words_fixture(CAPTURED_LINEAGE_EXT_SERVICE_WORDS);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    assert_eq!(ready.service_enabled(99), Some(true));
    assert_eq!(available.service_enabled(176), Some(true));
    assert_eq!(available.service_enabled(182), Some(true));
    let inputs = SunfishQcacldResourceInputs::observed_lineage_wmi_init_2026_10_24(
        QcacldPeerLimit::OtherTarget32,
    );

    let ready_snapshot = ready.sunfish_startup_snapshot().unwrap();
    let available_snapshot = available.snapshot();
    let candidate = SunfishQcacldResourceCandidate::build(
        &ready_snapshot.view(),
        &available_snapshot.view(),
        inputs,
    )
    .unwrap();
    assert_eq!(candidate.words(), &CAPTURED_LINEAGE_RESOURCE_WORDS);
    let tlv = candidate.encode_tlv();
    assert_eq!(&tlv[..4], &[0x10, 0x01, 0x4b, 0x00]);
    let parsed = WmiResourceConfig::parse(&tlv).unwrap();
    assert_eq!(parsed.tx_chain_mask(), 3);
    assert_eq!(parsed.rx_chain_mask(), 3);
    assert_eq!(parsed.num_msdu_descriptors(), 1056);
    assert_eq!(parsed.feature_flags(), 0x0018_0600);
    assert_eq!(parsed.num_packet_filters(), u32::MAX);
}

#[test]
fn candidate_composes_into_a_parsed_qcacld_wmi_init_htc_frame() {
    let bytes = service_ready_fixture(&[99]);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_fixture(&[176, 182]);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    let mut inputs =
        SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
    inputs.enable_peer_unmap_confirmation = true;

    let mut wmi_body = [0u8; 512];
    let mut htc_frame = [0u8; 512];
    let plan = encode_sunfish_qcacld_wmi_init_frame(
        &ready,
        &available,
        inputs,
        None,
        2,
        512,
        128,
        5,
        &mut wmi_body,
        &mut htc_frame,
    )
    .unwrap();
    assert_eq!(plan.body_bytes, 312);
    assert_eq!(plan.htc.frame_len, 324);
    assert_eq!(plan.htc.credits_required, 3);
    assert_eq!(plan.htc.credits_remaining, 2);
    assert!(plan.htc.requested_credit_update);

    let parsed = qcom_htc::parse_wmi_tx_frame(&htc_frame[..324], 2, 512).unwrap();
    assert_eq!(parsed.command_id, SUNFISH_WMI_INIT_COMMAND_ID);
    assert_eq!(parsed.tlv_body, &wmi_body[..312]);
    let resource = WmiResourceConfig::parse(&parsed.tlv_body[32..308]).unwrap();
    assert_eq!(resource.num_peers(), 38);
    assert_eq!(
        resource.feature_flags(),
        (1 << 9) | (1 << 10) | (1 << 19) | (1 << 20)
    );
}

#[test]
fn captured_lineage_profile_composes_into_the_qcacld_wmi_init_htc_frame() {
    let bytes = service_ready_words_fixture(CAPTURED_LINEAGE_BASE_SERVICE_WORDS);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_words_fixture(CAPTURED_LINEAGE_EXT_SERVICE_WORDS);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    let inputs = SunfishQcacldResourceInputs::observed_lineage_wmi_init_2026_10_24(
        QcacldPeerLimit::OtherTarget32,
    );

    let mut wmi_body = [0u8; 512];
    let mut htc_frame = [0u8; 512];
    let plan = encode_sunfish_qcacld_wmi_init_frame(
        &ready,
        &available,
        inputs,
        None,
        2,
        512,
        128,
        5,
        &mut wmi_body,
        &mut htc_frame,
    )
    .unwrap();

    assert_eq!(plan.resource.words(), &CAPTURED_LINEAGE_RESOURCE_WORDS);
    assert_eq!(plan.body_bytes, 312);
    assert_eq!(plan.htc.frame_len, 324);

    let parsed = qcom_htc::parse_wmi_tx_frame(&htc_frame[..324], 2, 512).unwrap();
    assert_eq!(parsed.command_id, SUNFISH_WMI_INIT_COMMAND_ID);
    let resource = WmiResourceConfig::parse(&parsed.tlv_body[32..308]).unwrap();
    assert_eq!(resource.as_tlv()[..4], [0x10, 0x01, 0x4b, 0x00]);
    assert_eq!(
        (0..68)
            .map(|index| word(resource.as_tlv(), index))
            .collect::<Vec<_>>(),
        CAPTURED_LINEAGE_RESOURCE_WORDS
    );
}

#[test]
fn service_absence_disables_capability_flags_and_host_gate_is_required() {
    let bytes = service_ready_fixture(&[]);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_fixture(&[]);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    let inputs =
        SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
    let candidate = SunfishQcacldResourceCandidate::build(&ready, &available, inputs).unwrap();
    let tlv = candidate.encode_tlv();
    assert_eq!(word(&tlv, 43), (1 << 9) | (1 << 10));
    assert_eq!(word(&tlv, 53), 0);

    let bytes = service_ready_fixture(&[]);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_fixture(&[182]);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    let candidate = SunfishQcacldResourceCandidate::build(&ready, &available, inputs).unwrap();
    assert_eq!(word(&candidate.encode_tlv(), 43), (1 << 9) | (1 << 10));
}

#[test]
fn candidate_rejects_twt_counts_when_build_feature_is_off() {
    let bytes = service_ready_fixture(&[]);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_fixture(&[]);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    let mut inputs =
        SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
    inputs.compile.twt_ap_sta_count = 1;
    assert!(SunfishQcacldResourceCandidate::build(&ready, &available, inputs).is_err());
}

#[test]
fn candidate_rejects_the_different_high_latency_layout() {
    let bytes = service_ready_fixture(&[]);
    let ready = ServiceReadyView::parse(&bytes).unwrap();
    let available_bytes = service_available_fixture(&[]);
    let available = ServiceAvailableView::parse(&available_bytes).unwrap();
    let mut inputs =
        SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
    inputs.compile.high_latency_hif = true;
    assert!(SunfishQcacldResourceCandidate::build(&ready, &available, inputs).is_err());
}
