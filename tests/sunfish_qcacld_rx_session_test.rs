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
use sunfish_qcacld_rx::{ParseOptions, RxDescriptor};
use sunfish_qcacld_rx_session::{Error, RxGroupSyncError, RxMsduChain, SunfishQcacldRxSession};

fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn completed_msdu(payload: &[u8], more_buffers: u8) -> Vec<u8> {
    completed_msdu_with_flags(payload, more_buffers, true, true)
}

fn completed_msdu_with_flags(
    payload: &[u8],
    more_buffers: u8,
    first_msdu: bool,
    last_msdu: bool,
) -> Vec<u8> {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let mut buffer = vec![0; profile.rx_buffer_bytes() as usize];
    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let frag_info = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.fragment_info_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    let msdu_end = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, frag_info, u32::from(more_buffers) << 16);
    put_word(&mut buffer, msdu_start, payload.len() as u32);
    put_word(&mut buffer, msdu_start + 8, 2 << 8); // Captured 802.3 decapsulation.
    put_word(
        &mut buffer,
        msdu_end + 16,
        (u32::from(first_msdu) << 14) | (u32::from(last_msdu) << 15),
    );
    let start = SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes as usize;
    buffer[start..start + payload.len()].copy_from_slice(payload);
    buffer
}

fn completed_msdu_declared_length(msdu_length: usize, more_buffers: u8, padding: u8) -> Vec<u8> {
    let mut buffer = completed_msdu_with_flags(b"", more_buffers, true, true);
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    let msdu_end = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words) * 4;
    put_word(&mut buffer, msdu_start, msdu_length as u32);
    put_word(&mut buffer, msdu_end + 8 * 4, u32::from(padding) << 10);
    buffer
}

#[test]
fn one_empty_slot_fifo_leases_and_repost_follow_ring_order() {
    assert_eq!(
        SunfishQcacldRxSession::<1>::new(),
        Err(Error::InvalidCapacity)
    );
    assert_eq!(
        SunfishQcacldRxSession::<3>::new(),
        Err(Error::InvalidCapacity)
    );
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    assert_eq!(session.post(10), Ok(0));
    assert_eq!(session.post(11), Ok(1));
    assert_eq!(session.post(12), Ok(2));
    assert_eq!(session.available_slots(), 0);
    assert_eq!(session.post(13), Err(Error::RingFull));

    let first = session.take_next().unwrap();
    let second = session.take_next().unwrap();
    assert_eq!(first.buffer_id(), 10);
    assert_eq!(second.buffer_id(), 11);
    assert_eq!(session.posted_count(), 1);
    assert_eq!(session.occupied_count(), 3);

    session.release(first).unwrap();
    assert_eq!(session.available_slots(), 1);
    assert_eq!(session.post(13), Ok(3));
    assert_eq!(session.take_next().unwrap().buffer_id(), 12);
    assert_eq!(session.take_next().unwrap().buffer_id(), 13);
}

#[test]
fn copy_is_capacity_checked_atomic_and_keeps_lease_until_release() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0x42).unwrap();
    let lease = session.take_next().unwrap();
    let buffer = completed_msdu(b"ethernet-frame", 0);
    let mut short = [0xa5; 4];
    assert_eq!(
        session.copy_single_buffer(lease, 0x42, &buffer, ParseOptions::default(), &mut short),
        Err(Error::OutputTooSmall {
            needed: b"ethernet-frame".len(),
            available: 4,
        })
    );
    assert_eq!(short, [0xa5; 4]);
    assert_eq!(session.occupied_count(), 1);

    let mut output = [0; 32];
    let copied = session
        .copy_single_buffer(lease, 0x42, &buffer, ParseOptions::default(), &mut output)
        .unwrap();
    assert_eq!(copied.bytes_copied, b"ethernet-frame".len());
    assert!(copied.descriptor.first_msdu);
    assert!(copied.descriptor.last_msdu);
    assert_eq!(&output[..copied.bytes_copied], b"ethernet-frame");
    assert_eq!(session.occupied_count(), 1);
    session.release(lease).unwrap();
    assert_eq!(session.occupied_count(), 0);
}

#[test]
fn ethernet_copy_requires_captured_decap_format_and_respects_firmware_discard() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0x42).unwrap();
    let lease = session.take_next().unwrap();
    let buffer = completed_msdu(b"ethernet-frame", 0);
    let mut output = [0xa5; 32];
    let copied = session
        .copy_ethernet_single_buffer(lease, 0x42, &buffer, ParseOptions::default(), &mut output)
        .unwrap();
    assert_eq!(copied.descriptor.decap_format, 2);
    assert_eq!(&output[..copied.bytes_copied], b"ethernet-frame");

    let mut native_wifi = buffer.clone();
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    put_word(&mut native_wifi, msdu_start + 8, 1 << 8);
    output.fill(0xa5);
    assert_eq!(
        session.copy_ethernet_single_buffer(
            lease,
            0x42,
            &native_wifi,
            ParseOptions::default(),
            &mut output,
        ),
        Err(Error::WrongDecapFormat {
            expected: 2,
            observed: 1,
        })
    );
    assert_eq!(output, [0xa5; 32]);

    let mut discarded = buffer;
    discarded[0] = 1; // FW_RX_DESC_DISCARD_M
    assert_eq!(
        session.copy_ethernet_single_buffer(
            lease,
            0x42,
            &discarded,
            ParseOptions::default(),
            &mut output,
        ),
        Err(Error::FirmwareDiscard)
    );
    assert_eq!(output, [0xa5; 32]);
    session.release(lease).unwrap();
}

#[test]
fn ethernet_chain_copy_stitches_source_sized_fragments_and_strips_first_padding() {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let buffer_bytes = profile.rx_buffer_bytes() as usize;
    let descriptor_bytes = SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes as usize;
    let padding = 4usize;
    let msdu_length = 3004usize;
    let first_bytes = buffer_bytes - descriptor_bytes - padding;
    let final_bytes = descriptor_bytes + msdu_length - buffer_bytes;

    let mut first = completed_msdu_declared_length(msdu_length, 1, padding as u8);
    first[descriptor_bytes..descriptor_bytes + padding].fill(0xee);
    for (index, byte) in first[descriptor_bytes + padding..].iter_mut().enumerate() {
        *byte = (index % 251) as u8;
    }
    let mut continuation = vec![0x7b; buffer_bytes];
    for (index, byte) in continuation[..final_bytes].iter_mut().enumerate() {
        *byte = (index % 239) as u8;
    }

    let buffers = [first.as_slice(), continuation.as_slice()];
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    session.post(1).unwrap();
    let mut msdus = [None; 1];
    let mut leases = [None; 2];
    let group = session
        .take_next_mpdu_group(
            &buffers,
            ParseOptions {
                l3_header_padding_active: true,
            },
            &mut msdus,
            &mut leases,
        )
        .unwrap();
    assert_eq!(group.msdu_count, 1);
    let chain = msdus[0].unwrap();

    let mut output = vec![0xa5; msdu_length - padding + 16];
    let copied = session
        .copy_ethernet_msdu_chain(
            chain,
            &leases,
            &buffers,
            ParseOptions {
                l3_header_padding_active: true,
            },
            &mut output,
        )
        .unwrap();
    assert_eq!(copied.bytes_copied, msdu_length - padding);
    assert_eq!(copied.descriptor.l3_header_padding, padding as u8);
    assert_eq!(&output[..first_bytes], &first[descriptor_bytes + padding..]);
    assert_eq!(
        &output[first_bytes..copied.bytes_copied],
        &continuation[..final_bytes]
    );
    assert_eq!(&output[copied.bytes_copied..], vec![0xa5; 16]);
    assert_eq!(session.occupied_count(), 2);
    for lease in leases.iter().flatten().copied() {
        session.release(lease).unwrap();
    }
}

#[test]
fn ethernet_chain_copy_supports_multiple_continuations_and_fails_atomically() {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let buffer_bytes = profile.rx_buffer_bytes() as usize;
    let descriptor_bytes = SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes as usize;
    let msdu_length = 4500usize;
    let first_bytes = buffer_bytes - descriptor_bytes;
    let final_bytes = descriptor_bytes + msdu_length - 2 * buffer_bytes;
    let mut first = completed_msdu_declared_length(msdu_length, 2, 0);
    first[descriptor_bytes..].fill(0x11);
    let middle = vec![0x22; buffer_bytes];
    let mut final_buffer = vec![0x33; buffer_bytes];
    final_buffer[..final_bytes].fill(0x44);
    let buffers = [first.as_slice(), middle.as_slice(), final_buffer.as_slice()];

    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    for id in 0..3 {
        session.post(id).unwrap();
    }
    let mut msdus = [None; 1];
    let mut leases = [None; 3];
    let group = session
        .take_next_mpdu_group(&buffers, ParseOptions::default(), &mut msdus, &mut leases)
        .unwrap();
    let chain = msdus[0].unwrap();

    let mut short = [0xa5; 32];
    assert_eq!(
        session.copy_ethernet_msdu_chain(
            chain,
            &leases,
            &buffers,
            ParseOptions::default(),
            &mut short,
        ),
        Err(Error::OutputTooSmall {
            needed: msdu_length,
            available: short.len(),
        })
    );
    assert_eq!(short, [0xa5; 32]);

    let mut output = vec![0xa5; msdu_length];
    let copied = session
        .copy_ethernet_msdu_chain(
            chain,
            &leases,
            &buffers,
            ParseOptions::default(),
            &mut output,
        )
        .unwrap();
    assert_eq!(group.buffer_count, 3);
    assert_eq!(copied.bytes_copied, msdu_length);
    assert_eq!(&output[..first_bytes], &first[descriptor_bytes..]);
    assert_eq!(&output[first_bytes..first_bytes + buffer_bytes], &middle);
    assert_eq!(
        &output[first_bytes + buffer_bytes..],
        &final_buffer[..final_bytes]
    );

    let invalid_head = completed_msdu_declared_length(2000, 1, 0);
    let invalid_buffers = [invalid_head.as_slice(), middle.as_slice()];
    // Reuse the currently leased slots and reject the impossible source count
    // before touching output or releasing ownership.
    let invalid_descriptor = RxDescriptor::decode(&invalid_head, ParseOptions::default()).unwrap();
    let invalid_chain = RxMsduChain {
        descriptor: invalid_descriptor,
        first_buffer_index: 0,
        buffer_count: 2,
    };
    output.fill(0xa5);
    assert_eq!(
        session.copy_ethernet_msdu_chain(
            invalid_chain,
            &leases,
            &invalid_buffers,
            ParseOptions::default(),
            &mut output,
        ),
        Err(Error::InvalidChainLength)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));
    assert_eq!(session.occupied_count(), 3);
    for lease in leases.iter().flatten().copied() {
        session.release(lease).unwrap();
    }
}

#[test]
fn mismatched_buffer_and_unsupported_chain_do_not_release_the_lease() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0x100).unwrap();
    let lease = session.take_next().unwrap();
    let buffer = completed_msdu(b"payload", 1);
    let mut output = [0; 64];
    assert_eq!(
        session.copy_single_buffer(lease, 0x101, &buffer, ParseOptions::default(), &mut output),
        Err(Error::BufferIdMismatch {
            expected: 0x100,
            actual: 0x101,
        })
    );
    assert!(matches!(
        session.copy_single_buffer(lease, 0x100, &buffer, ParseOptions::default(), &mut output),
        Err(Error::Descriptor(sunfish_qcacld_rx::Error::ChainedMsdu))
    ));
    assert_eq!(session.occupied_count(), 1);
    session.release(lease).unwrap();
}

#[test]
fn duplicate_live_ids_and_stale_generation_leases_are_rejected() {
    let mut session = SunfishQcacldRxSession::<2>::new().unwrap();
    session.post(7).unwrap();
    let old = session.take_next().unwrap();
    assert_eq!(session.post(7), Err(Error::DuplicateBufferId(7)));
    session.release(old).unwrap();

    session.post(8).unwrap();
    let middle = session.take_next().unwrap();
    session.release(middle).unwrap();

    session.post(9).unwrap();
    let new = session.take_next().unwrap();
    assert_ne!(old, new);
    assert_eq!(session.release(old), Err(Error::InvalidLease));
    session.release(new).unwrap();
}

#[test]
fn cpu_sync_preflights_every_head_and_continuation_without_claiming_buffers() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    session.post(1).unwrap();
    let head = completed_msdu(b"fragmented", 1);
    let continuation = vec![0; 2048];
    let buffers = [head.as_slice(), continuation.as_slice()];
    let mut synchronized = Vec::new();

    let count = session
        .sync_next_mpdu_group_for_cpu(&buffers, ParseOptions::default(), &[], 0, |buffer_id| {
            synchronized.push(buffer_id);
            Ok::<(), ()>(())
        })
        .unwrap();

    assert_eq!(count, 2);
    assert_eq!(synchronized, [0, 1]);
    assert_eq!(session.posted_count(), 2);
    assert_eq!(session.occupied_count(), 2);

    let result = session.sync_next_mpdu_group_for_cpu(
        &buffers,
        ParseOptions::default(),
        &[],
        0,
        |buffer_id| {
            if buffer_id == 1 {
                Err("sync failed")
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(result, Err(RxGroupSyncError::Sync("sync failed")));
    assert_eq!(session.posted_count(), 2);
    assert_eq!(session.occupied_count(), 2);
}

#[test]
fn mpdu_group_claims_ring2_fragments_and_msdu_heads_through_last_flag() {
    let mut session = SunfishQcacldRxSession::<8>::new().unwrap();
    for id in 0..7 {
        session.post(id).unwrap();
    }

    let mut owned = vec![vec![0; 2048]; 7];
    owned[0] = completed_msdu_with_flags(b"first MSDU", 1, true, false);
    owned[2] = completed_msdu_with_flags(b"second MSDU", 0, true, true);
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut msdus = [None; 2];
    let mut leases = [None; 4];
    let group = session
        .take_next_mpdu_group(&buffers, ParseOptions::default(), &mut msdus, &mut leases)
        .unwrap();

    assert_eq!(group.msdu_count, 2);
    assert_eq!(group.buffer_count, 3);
    assert_eq!(group.first_descriptor.more_buffers, 1);
    assert!(!group.first_descriptor.last_msdu);
    assert_eq!(
        msdus,
        [
            Some(RxMsduChain {
                descriptor: group.first_descriptor,
                first_buffer_index: 0,
                buffer_count: 2,
            }),
            Some(RxMsduChain {
                descriptor: RxDescriptor::decode(&owned[2], ParseOptions::default()).unwrap(),
                first_buffer_index: 2,
                buffer_count: 1,
            }),
        ]
    );
    assert_eq!(
        leases[..3]
            .iter()
            .map(|lease| lease.unwrap().buffer_id())
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(session.posted_count(), 4);
    for lease in leases[..3].iter().flatten().copied() {
        session.release(lease).unwrap();
    }
}

#[test]
fn incomplete_or_oversized_mpdu_group_fails_without_partial_leases() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    for id in 0..3 {
        session.post(id).unwrap();
    }
    let owned = vec![
        completed_msdu_with_flags(b"one", 0, true, false),
        completed_msdu_with_flags(b"two", 0, false, false),
        completed_msdu_with_flags(b"three", 0, false, false),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut msdus = [None; 3];
    let mut short = [None; 2];
    assert_eq!(
        session.take_next_mpdu_group(&buffers, ParseOptions::default(), &mut msdus, &mut short,),
        Err(Error::MissingLastMsdu)
    );
    assert_eq!(session.posted_count(), 3);
    assert_eq!(session.occupied_count(), 3);
    assert_eq!(short, [None; 2]);
    assert_eq!(msdus, [None; 3]);

    let oversized = completed_msdu_with_flags(b"oversized", 3, true, true);
    let mut available = vec![&[][..]; 3];
    available[0] = &oversized;
    assert_eq!(
        session.take_next_mpdu_group(
            &available,
            ParseOptions::default(),
            &mut [None; 3],
            &mut [None; 3],
        ),
        Err(Error::ChainExceedsPosted)
    );
    assert_eq!(session.posted_count(), 3);
}

#[test]
fn mpdu_group_output_capacity_is_checked_before_claiming_buffers() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    session.post(1).unwrap();
    session.post(2).unwrap();
    let owned = [
        completed_msdu_with_flags(b"one", 1, true, false),
        vec![0; 2048],
        completed_msdu_with_flags(b"two", 0, true, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut msdus = [None; 2];
    let mut short = [None; 2];
    assert_eq!(
        session.take_next_mpdu_group(&buffers, ParseOptions::default(), &mut msdus, &mut short,),
        Err(Error::OutputTooSmall {
            needed: 3,
            available: 2,
        })
    );
    assert_eq!(session.posted_count(), 3);
    assert_eq!(short, [None; 2]);
    assert_eq!(msdus, [None; 2]);
}

#[test]
fn mpdu_group_requires_capacity_for_each_msdu_chain_before_claiming() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    session.post(1).unwrap();
    let owned = [
        completed_msdu_with_flags(b"one", 0, true, false),
        completed_msdu_with_flags(b"two", 0, false, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut msdus = [None; 1];
    let mut leases = [None; 2];

    assert_eq!(
        session.take_next_mpdu_group(&buffers, ParseOptions::default(), &mut msdus, &mut leases,),
        Err(Error::OutputTooSmall {
            needed: 2,
            available: 1,
        })
    );
    assert_eq!(session.posted_count(), 2);
    assert_eq!(session.occupied_count(), 2);
    assert_eq!(msdus, [None; 1]);
    assert_eq!(leases, [None; 2]);
}

#[test]
fn rx_ind_firmware_descriptor_bytes_override_ring_bytes_for_group_and_copy() {
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    session.post(1).unwrap();
    let mut first = completed_msdu_with_flags(b"first-frame", 0, true, false);
    let mut second = completed_msdu_with_flags(b"discard-frame", 0, false, true);
    // QCACLD takes these status bytes from HTT_RX_IND and copies them over
    // the firmware-descriptor byte in each HTT RX buffer.
    first[0] = 1;
    second[0] = 1;
    let owned = [first, second];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut msdus = [None; 2];
    let mut leases = [None; 2];

    let group = session
        .take_next_mpdu_group_with_firmware_descriptors(
            &buffers,
            ParseOptions::default(),
            &[0, 1],
            0,
            &mut msdus,
            &mut leases,
        )
        .unwrap();
    assert_eq!(group.msdu_count, 2);
    let first_chain = msdus[0].unwrap();
    let discard_chain = msdus[1].unwrap();
    assert!(!first_chain.descriptor.firmware.discard);
    assert!(discard_chain.descriptor.firmware.discard);

    let mut output = [0xaa; 64];
    let copied = session
        .copy_ethernet_msdu_chain_with_firmware_descriptor(
            first_chain,
            &leases,
            &buffers,
            0,
            ParseOptions::default(),
            &mut output,
        )
        .unwrap();
    assert_eq!(&output[..copied.bytes_copied], b"first-frame");
    assert_eq!(
        session.copy_ethernet_msdu_chain_with_firmware_descriptor(
            discard_chain,
            &leases,
            &buffers,
            1,
            ParseOptions::default(),
            &mut output,
        ),
        Err(Error::FirmwareDiscard)
    );
}
