#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/sunfish_qcacld_tx.rs"]
mod sunfish_qcacld_tx;

use sunfish_qcacld_tx::{
    Error, HELIUMPLUS_FRAGMENT_DESC_BYTES, HELIUMPLUS_TSO_PREFIX_BYTES, HTT_DATA_HOST_TO_TARGET_CE,
    HTT_DATA_SERVICE_ID, HTT_DATA_TARGET_TO_HOST_CE, MAX_FRAME_IOVA, QCACLD_WIFI_IP_VERSION,
    TxMetadata, encode_native_wifi_data,
};

fn native_data_frame() -> [u8; 24] {
    let mut frame = [0u8; 24];
    frame[0..2].copy_from_slice(&0x0008u16.to_le_bytes());
    frame
}

fn metadata() -> TxMetadata {
    TxMetadata {
        fragment_table_iova: 0x8000_2000,
        frame_iova: 0x1_2345_6789,
        msdu_id: 0x1234,
        vdev_id: 5,
        ext_tid: 16,
        hardware_encrypt: false,
        request_completion: true,
    }
}

#[test]
fn encodes_matched_htt_desc64_and_heliumplus_fragment_table() {
    assert_eq!(QCACLD_WIFI_IP_VERSION, (2, 0));
    assert_eq!(HTT_DATA_SERVICE_ID, 0x0300);
    assert_eq!(HTT_DATA_HOST_TO_TARGET_CE, 4);
    assert_eq!(HTT_DATA_TARGET_TO_HOST_CE, 1);
    let encoded = encode_native_wifi_data(&native_data_frame(), metadata()).unwrap();
    assert_eq!(&encoded.htt_descriptor[0..4], &0x8405_2401u32.to_le_bytes());
    assert_eq!(&encoded.htt_descriptor[4..8], &0x1234_0018u32.to_le_bytes());
    assert_eq!(
        &encoded.htt_descriptor[8..12],
        &0x8000_2000u32.to_le_bytes()
    );
    assert_eq!(&encoded.htt_descriptor[12..16], &0u32.to_le_bytes());
    assert_eq!(&encoded.htt_descriptor[16..18], &0xffffu16.to_le_bytes());
    assert_eq!(&encoded.htt_descriptor[18..20], &0u16.to_le_bytes());

    assert_eq!(encoded.fragment_descriptor.len(), 72);
    assert!(
        encoded.fragment_descriptor[..HELIUMPLUS_TSO_PREFIX_BYTES]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(
        &encoded.fragment_descriptor[HELIUMPLUS_TSO_PREFIX_BYTES..HELIUMPLUS_TSO_PREFIX_BYTES + 4],
        &0x2345_6789u32.to_le_bytes()
    );
    assert_eq!(
        &encoded.fragment_descriptor
            [HELIUMPLUS_TSO_PREFIX_BYTES + 4..HELIUMPLUS_TSO_PREFIX_BYTES + 8],
        &0x0018_0001u32.to_le_bytes()
    );
    assert!(
        encoded.fragment_descriptor[HELIUMPLUS_TSO_PREFIX_BYTES + 8..]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(
        encoded.fragment_descriptor.len(),
        HELIUMPLUS_FRAGMENT_DESC_BYTES
    );
}

#[test]
fn allows_firmware_encryption_without_setting_no_encrypt_bit() {
    let mut request = metadata();
    request.hardware_encrypt = true;
    request.request_completion = false;
    let encoded = encode_native_wifi_data(&native_data_frame(), request).unwrap();
    let word0 = u32::from_le_bytes(encoded.htt_descriptor[0..4].try_into().unwrap());
    assert_eq!(word0 & (1 << 10), 0);
    assert_eq!(word0 & (1 << 31), 0);
}

#[test]
fn rejects_management_control_and_malformed_frames() {
    let mut management = [0u8; 24];
    management[0..2].copy_from_slice(&0x0000u16.to_le_bytes());
    assert_eq!(
        encode_native_wifi_data(&management, metadata()),
        Err(Error::NotDataFrame)
    );
    assert_eq!(
        encode_native_wifi_data(&[0x08], metadata()),
        Err(Error::InvalidFrame(
            ath10k_nwifi_rx::FrameHeaderError::FrameControlTooShort
        ))
    );
}

#[test]
fn validates_vdev_tid_and_both_iova_apertures() {
    let frame = native_data_frame();
    let mut request = metadata();
    request.vdev_id = 64;
    assert_eq!(
        encode_native_wifi_data(&frame, request),
        Err(Error::InvalidVdevId { observed: 64 })
    );

    let mut request = metadata();
    request.ext_tid = 32;
    assert_eq!(
        encode_native_wifi_data(&frame, request),
        Err(Error::InvalidExtTid { observed: 32 })
    );

    let mut request = metadata();
    request.fragment_table_iova = 0xffff_ffc0;
    assert_eq!(
        encode_native_wifi_data(&frame, request),
        Err(Error::FragmentTableOutsideLow32 {
            observed: 0xffff_ffc0
        })
    );

    let mut request = metadata();
    request.fragment_table_iova = 0x8000_2004;
    assert_eq!(
        encode_native_wifi_data(&frame, request),
        Err(Error::UnalignedFragmentTableIova {
            observed: 0x8000_2004
        })
    );

    let mut request = metadata();
    request.frame_iova = MAX_FRAME_IOVA - 23;
    assert_eq!(
        encode_native_wifi_data(&frame, request),
        Err(Error::FrameIovaOutside37Bit {
            observed: MAX_FRAME_IOVA - 23,
            bytes: frame.len(),
        })
    );

    let mut request = metadata();
    request.frame_iova = 0;
    assert_eq!(
        encode_native_wifi_data(&frame, request),
        Err(Error::NullFrameIova)
    );

    let mut request = metadata();
    request.fragment_table_iova = 0;
    assert_eq!(
        encode_native_wifi_data(&frame, request),
        Err(Error::NullFragmentTableIova)
    );
}

#[test]
fn accepts_frame_ending_at_37_bit_boundary() {
    let mut request = metadata();
    request.frame_iova = MAX_FRAME_IOVA - 24;
    assert!(encode_native_wifi_data(&native_data_frame(), request).is_ok());
}

#[test]
fn rejects_empty_and_oversized_frames() {
    assert_eq!(
        encode_native_wifi_data(&[], metadata()),
        Err(Error::EmptyFrame)
    );
    let oversized = vec![0u8; usize::from(u16::MAX) + 1];
    assert_eq!(
        encode_native_wifi_data(&oversized, metadata()),
        Err(Error::FrameTooLong {
            observed: usize::from(u16::MAX) + 1,
        })
    );
}
