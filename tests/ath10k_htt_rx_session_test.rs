#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/ath10k_firmware.rs"]
mod ath10k_firmware;
#[path = "../rust/src/ath10k_htt_rx.rs"]
mod ath10k_htt_rx;
#[path = "../rust/src/ath10k_htt_rx_owner.rs"]
mod ath10k_htt_rx_owner;
#[path = "../rust/src/ath10k_htt_rx_ring.rs"]
mod ath10k_htt_rx_ring;
#[path = "../rust/src/ath10k_htt_rx_session.rs"]
mod ath10k_htt_rx_session;
#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/ath10k_wmi.rs"]
mod ath10k_wmi;

mod sunfish_smmu_lpae {
    pub const SMMU_LPAE_PAGE_BYTES: u64 = 4096;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LpaePageMapping {
        pub iova_page: u64,
        pub physical_page: u64,
    }
}

use ath10k_htt_rx_owner::{CompletionMode, RxBufferLease, RxRingOwner};
use ath10k_htt_rx_ring::{RxRingMemory, plan_rx_ring};
use ath10k_htt_rx_session::{
    Disposition, DropReason, complete_fifo_msdu, complete_fifo_native_wifi_data_msdu,
};
use ath10k_nwifi_rx::{CryptoParameters, FourAddressPadding};

const RX_DESCRIPTOR_BYTES: usize = 360;
const RX_BUFFER_BYTES: usize = 2048;

fn owner() -> RxRingOwner {
    let plan = plan_rx_ring(RxRingMemory {
        ring_iova: 0x1000_0000,
        alloc_index_iova: 0x1000_4000,
        buffers_iova: 0x1001_0000,
        iova_bits: 39,
    })
    .unwrap();
    RxRingOwner::new_with_mode(plan, 7, CompletionMode::Fifo).unwrap()
}

fn blank_buffer() -> Vec<u8> {
    vec![0; RX_BUFFER_BYTES]
}

fn put_word(bytes: &mut [u8], offset: usize, word: u32) {
    bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn descriptor(
    buffer: &mut [u8],
    more_buffers: u8,
    msdu_length: u16,
    decap_format: u8,
    last_msdu: bool,
) {
    put_word(buffer, 4, 1 << 31); // MSDU_DONE
    put_word(buffer, 8, u32::from(more_buffers) << 16);
    put_word(buffer, 28, u32::from(msdu_length));
    put_word(buffer, 36, u32::from(decap_format) << 8);
    put_word(buffer, 64, (1 << 14) | if last_msdu { 1 << 15 } else { 0 });
}

#[test]
fn completes_and_copies_one_fifo_msdu_with_its_exact_buffer_lease() {
    let mut owner = owner();
    let mut buffer = blank_buffer();
    descriptor(&mut buffer, 0, 8, 1, true);
    let payload_start = RX_DESCRIPTOR_BYTES;
    buffer[payload_start..payload_start + 8].copy_from_slice(b"wifi-msd");
    let buffers: [&[u8]; 1] = [&buffer];
    let mut leases: [Option<RxBufferLease>; 1] = [None];
    let mut payload = [0xaa; 16];

    let result = complete_fifo_msdu(&mut owner, 1, &buffers, &mut leases, &mut payload).unwrap();

    assert_eq!(
        result.disposition,
        Disposition::Delivered { payload_bytes: 8 }
    );
    assert_eq!(leases[0].unwrap().buffer_id(), 0);
    assert_eq!(&payload[..8], b"wifi-msd");
    assert_eq!(owner.fill_count(), 2046);
}

#[test]
fn assembles_a_source_supported_raw_chain_and_returns_every_lease() {
    let mut owner = owner();
    let mut first = blank_buffer();
    let mut second = blank_buffer();
    let mut third = blank_buffer();
    descriptor(&mut first, 2, 4000, 0, true);
    first[RX_DESCRIPTOR_BYTES..].fill(0x11);
    second.fill(0x22);
    third.fill(0x33);
    let buffers: [&[u8]; 3] = [&first, &second, &third];
    let mut leases: [Option<RxBufferLease>; 3] = [None; 3];
    let mut payload = [0; 4000];

    let result = complete_fifo_msdu(
        &mut owner,
        buffers.len(),
        &buffers,
        &mut leases,
        &mut payload,
    )
    .unwrap();

    assert_eq!(
        result.disposition,
        Disposition::Delivered {
            payload_bytes: 4000
        }
    );
    assert_eq!(leases.map(|lease| lease.unwrap().buffer_id()), [0, 1, 2]);
    assert!(
        payload[..RX_BUFFER_BYTES - RX_DESCRIPTOR_BYTES]
            .iter()
            .all(|byte| *byte == 0x11)
    );
    assert!(
        payload[RX_BUFFER_BYTES - RX_DESCRIPTOR_BYTES
            ..RX_BUFFER_BYTES - RX_DESCRIPTOR_BYTES + RX_BUFFER_BYTES]
            .iter()
            .all(|byte| *byte == 0x22)
    );
    assert!(
        payload[RX_BUFFER_BYTES - RX_DESCRIPTOR_BYTES + RX_BUFFER_BYTES..]
            .iter()
            .all(|byte| *byte == 0x33)
    );
    assert_eq!(owner.fill_count(), 2044);
}

#[test]
fn unsupported_chain_is_dropped_after_all_completed_buffers_are_leased() {
    let mut owner = owner();
    let mut first = blank_buffer();
    let second = blank_buffer();
    descriptor(&mut first, 1, 100, 1, true);
    let buffers: [&[u8]; 2] = [&first, &second];
    let mut leases: [Option<RxBufferLease>; 2] = [None; 2];
    let mut payload = [0xaa; 128];

    let result = complete_fifo_msdu(&mut owner, 2, &buffers, &mut leases, &mut payload).unwrap();

    assert_eq!(
        result.disposition,
        Disposition::Dropped(DropReason::Unchain(
            ath10k_htt_rx::DropReason::NonRawDecapsulation
        ))
    );
    assert!(leases.iter().all(Option::is_some));
    assert!(payload.iter().all(|byte| *byte == 0xaa));
}

#[test]
fn malformed_chain_count_is_dropped_without_losing_the_returned_lease() {
    let mut owner = owner();
    let mut first = blank_buffer();
    let second = blank_buffer();
    descriptor(&mut first, 2, 100, 0, true);
    let buffers: [&[u8]; 2] = [&first, &second];
    let mut leases: [Option<RxBufferLease>; 2] = [None; 2];
    let mut payload = [0; 128];

    let result = complete_fifo_msdu(&mut owner, 2, &buffers, &mut leases, &mut payload).unwrap();

    assert_eq!(
        result.disposition,
        Disposition::Dropped(DropReason::ChainCountMismatch {
            expected: 3,
            completed: 2
        })
    );
    assert!(leases.iter().all(Option::is_some));
    assert_eq!(owner.fill_count(), 2045);
}

#[test]
fn undersized_output_fails_after_completion_but_preserves_recovery_leases() {
    let mut owner = owner();
    let mut buffer = blank_buffer();
    descriptor(&mut buffer, 0, 8, 1, true);
    let buffers: [&[u8]; 1] = [&buffer];
    let mut leases: [Option<RxBufferLease>; 1] = [None];
    let mut payload = [0xaa; 7];

    assert_eq!(
        complete_fifo_msdu(&mut owner, 1, &buffers, &mut leases, &mut payload,),
        Err(ath10k_htt_rx_session::Error::OutputTooSmall)
    );
    assert!(leases[0].is_some());
    assert!(payload.iter().all(|byte| *byte == 0xaa));
}

#[test]
fn retires_and_reconstructs_native_wifi_frame_from_the_same_rx_descriptor() {
    const RX_HEADER_STATUS_OFFSET: usize = 296;
    const PAYLOAD_OFFSET: usize = 360;
    let mut owner = owner();
    let mut buffer = blank_buffer();
    let msdu_length = 24 + 8 + 4;
    descriptor(&mut buffer, 0, msdu_length as u16, 1, true);

    // RX header status carries the original QoS 802.11 header.
    let status = &mut buffer[RX_HEADER_STATUS_OFFSET
        ..RX_HEADER_STATUS_OFFSET + ath10k_nwifi_rx::RX_HEADER_STATUS_BYTES];
    put_word(status, 0, 0x0288); // QoS data, FromDS
    status[4..10].copy_from_slice(&[0x02, 0, 0, 0, 0, 0x10]);
    status[10..16].copy_from_slice(&[0x02, 0, 0, 0, 0, 0x11]);
    status[16..22].copy_from_slice(&[0x02, 0, 0, 0, 0, 0x12]);

    // The MSDU contains a non-QoS Native-WiFi header followed by LLC/SNAP.
    let payload = &mut buffer[PAYLOAD_OFFSET..PAYLOAD_OFFSET + msdu_length];
    put_word(payload, 0, 0x0208); // firmware strips QoS Control
    payload[4..10].copy_from_slice(&[0x02, 0, 0, 0, 0, 1]); // DA
    payload[10..16].copy_from_slice(&[0x02, 0, 0, 0, 0, 2]); // BSSID
    payload[16..22].copy_from_slice(&[0x02, 0, 0, 0, 0, 3]); // SA
    payload[24..32].copy_from_slice(&[0xaa, 0xaa, 3, 0, 0, 0, 8, 0]);
    payload[32..36].copy_from_slice(&[0x45, 0x00, 0, 20]);

    let buffers: [&[u8]; 1] = [&buffer];
    let mut leases: [Option<RxBufferLease>; 1] = [None];
    let mut frame = [0xa5; 64];
    let completion = complete_fifo_native_wifi_data_msdu(
        &mut owner,
        1,
        &buffers,
        &mut leases,
        FourAddressPadding::PaddedToFour,
        true,
        CryptoParameters::None,
        &mut frame,
    )
    .unwrap();

    assert_eq!(
        completion.disposition,
        Disposition::Delivered { payload_bytes: 38 }
    );
    assert_eq!(leases[0].unwrap().buffer_id(), 0);
    assert_eq!(u16::from_le_bytes([frame[0], frame[1]]), 0x0288);
    assert_eq!(&frame[4..10], &[0x02, 0, 0, 0, 0, 1]);
    assert_eq!(&frame[16..22], &[0x02, 0, 0, 0, 0, 3]);
    assert_eq!(
        &frame[26..38],
        &buffer[PAYLOAD_OFFSET + 24..PAYLOAD_OFFSET + msdu_length]
    );
    assert!(frame[38..].iter().all(|byte| *byte == 0xa5));
    assert_eq!(owner.fill_count(), 2046);
}

#[test]
fn native_wifi_reconstruction_drops_wrong_decap_and_retains_lease() {
    let mut owner = owner();
    let mut buffer = blank_buffer();
    descriptor(&mut buffer, 0, 8, 2, true); // Ethernet-II, not Native WiFi
    let buffers: [&[u8]; 1] = [&buffer];
    let mut leases: [Option<RxBufferLease>; 1] = [None];
    let mut frame = [0xa5; 64];

    let completion = complete_fifo_native_wifi_data_msdu(
        &mut owner,
        1,
        &buffers,
        &mut leases,
        FourAddressPadding::PaddedToFour,
        true,
        CryptoParameters::None,
        &mut frame,
    )
    .unwrap();

    assert_eq!(
        completion.disposition,
        Disposition::Dropped(DropReason::WrongDecapsulationFormat {
            expected: 1,
            observed: 2
        })
    );
    assert!(leases[0].is_some());
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn native_wifi_reconstruction_drops_partial_amsdu_group() {
    let mut owner = owner();
    let mut buffer = blank_buffer();
    descriptor(&mut buffer, 0, 8, 1, true);
    put_word(&mut buffer, 64, 1 << 15); // not the first MSDU in the MPDU
    let buffers: [&[u8]; 1] = [&buffer];
    let mut leases: [Option<RxBufferLease>; 1] = [None];
    let mut frame = [0xa5; 64];

    let completion = complete_fifo_native_wifi_data_msdu(
        &mut owner,
        1,
        &buffers,
        &mut leases,
        FourAddressPadding::PaddedToFour,
        true,
        CryptoParameters::None,
        &mut frame,
    )
    .unwrap();

    assert_eq!(
        completion.disposition,
        Disposition::Dropped(DropReason::NativeWifiMsduBoundary {
            first: false,
            last: true
        })
    );
    assert!(leases[0].is_some());
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn native_wifi_output_shortage_preserves_retired_buffer_lease() {
    const RX_HEADER_STATUS_OFFSET: usize = 296;
    const PAYLOAD_OFFSET: usize = 360;
    let mut owner = owner();
    let mut buffer = blank_buffer();
    let msdu_length = 24 + 8;
    descriptor(&mut buffer, 0, msdu_length as u16, 1, true);
    put_word(
        &mut buffer[RX_HEADER_STATUS_OFFSET..],
        0,
        0x0008, // ordinary data frame, no DS bits
    );
    put_word(&mut buffer[PAYLOAD_OFFSET..], 0, 0x0008);
    buffer[PAYLOAD_OFFSET + 24..PAYLOAD_OFFSET + msdu_length]
        .copy_from_slice(&[0xaa, 0xaa, 3, 0, 0, 0, 8, 0]);
    let buffers: [&[u8]; 1] = [&buffer];
    let mut leases: [Option<RxBufferLease>; 1] = [None];
    let mut frame = [0xa5; 31]; // reconstructed frame needs 32 bytes

    assert_eq!(
        complete_fifo_native_wifi_data_msdu(
            &mut owner,
            1,
            &buffers,
            &mut leases,
            FourAddressPadding::PaddedToFour,
            true,
            CryptoParameters::None,
            &mut frame,
        ),
        Err(ath10k_htt_rx_session::Error::OutputTooSmall)
    );
    assert!(leases[0].is_some());
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}
