#![allow(dead_code)]
#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};

mod sunfish_smmu_lpae {
    pub const SMMU_LPAE_PAGE_BYTES: u64 = 4096;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LpaePageMapping {
        pub iova_page: u64,
        pub physical_page: u64,
    }
}

#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_delivery.rs"]
mod sunfish_qcacld_rx_delivery;
#[path = "../rust/src/sunfish_qcacld_rx_ind.rs"]
mod sunfish_qcacld_rx_ind;
#[path = "../rust/src/sunfish_qcacld_rx_pipeline.rs"]
mod sunfish_qcacld_rx_pipeline;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_qcacld_rx_transport.rs"]
mod sunfish_qcacld_rx_transport;

use qcom_ce_v3::RxCompletion;
use qcom_htc::HtcReady;
use sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use sunfish_htt_rx_ring::{
    SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS, SunfishHttRxBufferPool, SunfishHttRxRingIo,
    SunfishHttRxRingMemory, SunfishHttRxRingOptions, SunfishHttRxRingRefillState,
    plan_sunfish_htt_rx_ring,
};
use sunfish_qcacld_htc::{HttEndpoint, parse_connect_response};
use sunfish_qcacld_rx::ParseOptions;
use sunfish_qcacld_rx_delivery::{RxDeliveryError, deliver_mpdu};
use sunfish_qcacld_rx_ind::{
    HTT_RX_IND_MPDU_STATUS_OK, HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER, HttRxIndication, MpduStatus,
};
use sunfish_qcacld_rx_session::SunfishQcacldRxSession;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefillEvent {
    ReadIndex(u64),
    SyncBuffer(u64, usize),
    WriteRingEntry(u64, u64),
    Barrier,
    WriteIndex(u64, u32),
}

#[derive(Default)]
struct RefillIo {
    alloc_index: u32,
    events: Vec<RefillEvent>,
}

impl SunfishHttRxRingIo for RefillIo {
    type Error = &'static str;

    fn read_alloc_index(&mut self, address: u64) -> Result<u32, Self::Error> {
        self.events.push(RefillEvent::ReadIndex(address));
        Ok(self.alloc_index)
    }

    fn sync_buffer_for_device(&mut self, address: u64, bytes: usize) -> Result<(), Self::Error> {
        self.events.push(RefillEvent::SyncBuffer(address, bytes));
        Ok(())
    }

    fn write_ring_entry(&mut self, address: u64, buffer_iova: u64) -> Result<(), Self::Error> {
        self.events
            .push(RefillEvent::WriteRingEntry(address, buffer_iova));
        Ok(())
    }

    fn dma_write_barrier(&mut self) {
        self.events.push(RefillEvent::Barrier);
    }

    fn write_alloc_index(&mut self, address: u64, next: u32) -> Result<(), Self::Error> {
        self.events.push(RefillEvent::WriteIndex(address, next));
        self.alloc_index = next;
        Ok(())
    }
}

fn full_ring_session() -> SunfishQcacldRxSession<2048> {
    let mut session = SunfishQcacldRxSession::new().unwrap();
    for buffer_id in 0..2047 {
        assert_eq!(session.post(buffer_id).unwrap(), buffer_id as u16);
    }
    session
}

fn refill_plan() -> sunfish_htt_rx_ring::SunfishHttRxRingPlan {
    plan_sunfish_htt_rx_ring(
        plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap(),
        SunfishHttRxRingMemory {
            ring_iova: 0x8020_0000,
            alloc_index_iova: 0x8020_4000,
            initial_buffers_iova: 0x8020_4008,
            fallback_buffers_iova: 0x8064_3788,
        },
        SunfishHttRxRingOptions {
            packet_log_enabled: false,
            global_monitor_mode: false,
        },
    )
    .unwrap()
}

fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn status(raw: u8) -> MpduStatus {
    let mut bytes = [0u8; 52];
    bytes[0] = 1; // HTT_T2H_MSG_TYPE_RX_IND
    put_word(&mut bytes, 4, 1 << 24); // One MPDU range.
    put_word(&mut bytes, 48, 1 | (u32::from(raw) << 8));
    HttRxIndication::decode(&bytes)
        .unwrap()
        .mpdus()
        .next()
        .unwrap()
        .status
}

fn endpoint() -> HttEndpoint {
    let response = [
        3, 0, 0, 3, // HTC connect response, HTT service
        0, 5, 0, 2, // success, endpoint 5, 512-byte maximum
        0, 0, 0, 0,
    ];
    parse_connect_response(
        &response,
        HtcReady {
            credit_count: 4,
            credit_size: 64,
            max_endpoints: 9,
            htc_version: 1,
            max_messages_per_bundle: 1,
            alternate_data_credit_size: 0,
        },
    )
    .unwrap()
}

fn three_mpdu_indication() -> Vec<u8> {
    // Fixed header, three per-MSDU firmware bytes, one alignment byte, and
    // three single-MPDU ranges with OK, unauthenticated-peer, and OK status.
    let mut bytes = vec![0u8; 64];
    bytes[0] = 1;
    put_word(&mut bytes, 4, 3 << 24);
    put_word(&mut bytes, 44, 3);
    bytes[48..51].copy_from_slice(&[0, 0, 1]);
    put_word(
        &mut bytes,
        52,
        1 | (u32::from(HTT_RX_IND_MPDU_STATUS_OK) << 8),
    );
    put_word(
        &mut bytes,
        56,
        1 | (u32::from(HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER) << 8),
    );
    put_word(
        &mut bytes,
        60,
        1 | (u32::from(HTT_RX_IND_MPDU_STATUS_OK) << 8),
    );
    bytes
}

fn ethernet_frame(ethertype: u16) -> Vec<u8> {
    let mut frame = vec![0; 18];
    frame[12..14].copy_from_slice(&ethertype.to_be_bytes());
    frame[14..].copy_from_slice(&[1, 2, 3, 4]);
    frame
}

fn completed_msdu(payload: &[u8], first_msdu: bool, last_msdu: bool) -> Vec<u8> {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let mut buffer = vec![0; profile.rx_buffer_bytes() as usize];
    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    let msdu_end = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, msdu_start, payload.len() as u32);
    put_word(&mut buffer, msdu_start + 8, 2 << 8);
    put_word(
        &mut buffer,
        msdu_end + 16,
        (u32::from(first_msdu) << 14) | (u32::from(last_msdu) << 15),
    );
    let payload_start = SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes as usize;
    buffer[payload_start..payload_start + payload.len()].copy_from_slice(payload);
    buffer
}

#[test]
fn ok_mpdu_is_copied_to_sink_before_all_ring_leases_are_released() {
    let frame = ethernet_frame(0x0800);
    let owned = [completed_msdu(&frame, true, true)];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    let mut msdus = [None; 1];
    let mut leases = [None; 1];
    let mut scratch = [0; 64];
    let mut received = Vec::new();

    let report = deliver_mpdu(
        &mut session,
        &buffers,
        ParseOptions::default(),
        &[0],
        0,
        status(HTT_RX_IND_MPDU_STATUS_OK),
        &mut msdus,
        &mut leases,
        &mut scratch,
        &mut |packet: &[u8]| {
            received.push(packet.to_vec());
            Ok::<(), ()>(())
        },
    )
    .unwrap();

    assert_eq!(report.delivered_msdus, 1);
    assert_eq!(report.filtered_msdus, 0);
    assert_eq!(received, [frame]);
    assert_eq!(session.occupied_count(), 0);
    assert_eq!(session.posted_count(), 0);
    assert!(leases[0].is_none());
}

#[test]
fn unauthenticated_peer_delivers_eapol_only_across_an_amsdu_group() {
    let eapol = ethernet_frame(0x888e);
    let ip = ethernet_frame(0x0800);
    let owned = [
        completed_msdu(&eapol, true, false),
        completed_msdu(&ip, false, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    session.post(1).unwrap();
    let mut msdus = [None; 3];
    let mut leases = [None; 3];
    let mut scratch = [0; 64];
    let mut received = Vec::new();

    let report = deliver_mpdu(
        &mut session,
        &buffers,
        ParseOptions::default(),
        &[0, 0],
        0,
        status(HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER),
        &mut msdus,
        &mut leases,
        &mut scratch,
        &mut |packet: &[u8]| {
            received.push(packet.to_vec());
            Ok::<(), ()>(())
        },
    )
    .unwrap();

    assert_eq!(report.group.msdu_count, 2);
    assert_eq!(report.delivered_msdus, 1);
    assert_eq!(report.filtered_msdus, 1);
    assert_eq!(received, [eapol]);
    assert_eq!(session.occupied_count(), 0);
}

#[test]
fn indication_delivery_advances_status_and_firmware_cursors_by_msdu_group() {
    let ip = ethernet_frame(0x0800);
    let eapol = ethernet_frame(0x888e);
    let discarded = ethernet_frame(0x0800);
    let owned = [
        completed_msdu(&ip, true, true),
        completed_msdu(&eapol, true, true),
        completed_msdu(&discarded, true, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let indication_bytes = three_mpdu_indication();
    let mut transport_frame = vec![0u8; 8 + indication_bytes.len()];
    transport_frame[0] = 5;
    transport_frame[2..4].copy_from_slice(&(indication_bytes.len() as u16).to_le_bytes());
    transport_frame[5] = 0xa4;
    transport_frame[8..].copy_from_slice(&indication_bytes);
    let mut session = SunfishQcacldRxSession::<8>::new().unwrap();
    for buffer_id in 0..3 {
        session.post(buffer_id).unwrap();
    }
    let mut msdus = [None; 3];
    let mut leases = [None; 3];
    let mut transport_scratch = [0; 80];
    let mut scratch = [0; 64];
    let mut received = Vec::new();
    let mut synced = Vec::new();
    let mut recycled = [u32::MAX; 7];

    let report = sunfish_qcacld_rx_pipeline::consume_and_deliver_with_cpu_sync_and_collect(
        endpoint(),
        &mut transport_scratch,
        &mut session,
        &buffers,
        ParseOptions::default(),
        &mut msdus,
        &mut leases,
        &mut scratch,
        &mut |packet: &[u8]| {
            received.push(packet.to_vec());
            Ok::<(), ()>(())
        },
        |buffer_id| {
            synced.push(buffer_id);
            Ok::<(), ()>(())
        },
        &mut recycled,
        |destination| {
            destination[..transport_frame.len()].copy_from_slice(&transport_frame);
            Ok::<_, ()>(RxCompletion {
                address: 0x9000,
                nbytes: transport_frame.len() as u16,
                transfer_id: 7,
                byte_swapped: false,
            })
        },
    )
    .unwrap();

    assert_eq!(report.pipeline.sequence, 0xa4);
    assert_eq!(report.pipeline.delivery.mpdu_count, 3);
    assert_eq!(report.pipeline.delivery.delivered_msdus, 2);
    assert_eq!(report.pipeline.delivery.filtered_msdus, 1);
    assert_eq!(received, [ip, eapol]);
    assert_eq!(synced, [0, 1, 2]);
    assert_eq!(report.recycled_buffer_count, 3);
    assert_eq!(&recycled[..3], [0, 1, 2]);
    assert_eq!(session.occupied_count(), 0);
    assert_eq!(session.posted_count(), 0);
    assert_eq!(session.preview_post_batch(&recycled[..3]), Ok(3));
}

#[test]
fn delivered_rx_buffers_flow_into_one_batched_ring_refill() {
    let plan = refill_plan();
    let ip = ethernet_frame(0x0800);
    let eapol = ethernet_frame(0x888e);
    let discarded = ethernet_frame(0x0800);
    let owned = [
        completed_msdu(&ip, true, true),
        completed_msdu(&eapol, true, true),
        completed_msdu(&discarded, true, true),
    ];
    let mut initial = vec![0u8; plan.initial_buffer_bytes().unwrap() as usize];
    let mut fallback = vec![0u8; plan.fallback_buffer_bytes() as usize];
    let mut pool = SunfishHttRxBufferPool::new(plan, &mut initial, &mut fallback).unwrap();
    for (buffer_id, msdu) in owned.iter().enumerate() {
        pool.buffer_mut(buffer_id as u32).unwrap()[..msdu.len()].copy_from_slice(msdu);
    }
    let indication_bytes = three_mpdu_indication();
    let mut transport_frame = vec![0u8; 8 + indication_bytes.len()];
    transport_frame[0] = 5;
    transport_frame[2..4].copy_from_slice(&(indication_bytes.len() as u16).to_le_bytes());
    transport_frame[5] = 0xb7;
    transport_frame[8..].copy_from_slice(&indication_bytes);
    let mut session = full_ring_session();
    let mut msdus = vec![None; 2047];
    let mut leases = vec![None; 2047];
    let mut transport_scratch = [0; 80];
    let mut frame_scratch = [0; 64];
    let mut recycled = vec![u32::MAX; 2047];
    let mut synced = Vec::new();
    let mut received = Vec::new();

    let report = {
        let packet_buffers = (0..2047)
            .map(|buffer_id| pool.buffer(buffer_id).unwrap())
            .collect::<Vec<_>>();
        sunfish_qcacld_rx_pipeline::consume_and_deliver_with_cpu_sync_and_collect(
            endpoint(),
            &mut transport_scratch,
            &mut session,
            &packet_buffers,
            ParseOptions::default(),
            &mut msdus,
            &mut leases,
            &mut frame_scratch,
            &mut |packet: &[u8]| {
                received.push(packet.to_vec());
                Ok::<(), ()>(())
            },
            |buffer_id| {
                synced.push(buffer_id);
                Ok::<(), ()>(())
            },
            &mut recycled,
            |destination| {
                destination[..transport_frame.len()].copy_from_slice(&transport_frame);
                Ok::<_, ()>(RxCompletion {
                    address: 0x9000,
                    nbytes: transport_frame.len() as u16,
                    transfer_id: 9,
                    byte_swapped: false,
                })
            },
        )
        .unwrap()
    };

    assert_eq!(report.pipeline.sequence, 0xb7);
    assert_eq!(report.recycled_buffer_count, 3);
    assert_eq!(&recycled[..3], &[0, 1, 2]);
    assert_eq!(synced, [0, 1, 2]);
    assert_eq!(report.pipeline.delivery.delivered_msdus, 2);
    assert_eq!(report.pipeline.delivery.filtered_msdus, 1);
    assert_eq!(received, [ip, eapol]);
    assert_eq!(session.posted_count(), 2044);
    assert_eq!(session.occupied_count(), 2044);

    // The lexical block above ends all packet borrows before this refill.
    let mut refill = SunfishHttRxRingRefillState::new(plan);
    let mut io = RefillIo {
        alloc_index: plan.initial_alloc_index(),
        ..RefillIo::default()
    };
    assert_eq!(
        refill.refill_batch(plan, &mut session, &recycled[..3], &mut pool, &mut io),
        Ok(2047)
    );
    assert_eq!(refill.next_alloc_index(), 2);
    assert_eq!(session.posted_count(), 2047);
    assert_eq!(session.occupied_count(), 2047);
    assert_eq!(io.alloc_index, 2);
    assert_eq!(
        io.events,
        [
            RefillEvent::ReadIndex(0x8020_4000),
            RefillEvent::SyncBuffer(0x8020_4008, 2176),
            RefillEvent::WriteRingEntry(0x8020_3ff8, 0x8020_4008),
            RefillEvent::SyncBuffer(0x8020_4888, 2176),
            RefillEvent::WriteRingEntry(0x8020_0000, 0x8020_4888),
            RefillEvent::SyncBuffer(0x8020_5108, 2176),
            RefillEvent::WriteRingEntry(0x8020_0008, 0x8020_5108),
            RefillEvent::Barrier,
            RefillEvent::WriteIndex(0x8020_4000, 2),
        ]
    );
}

#[test]
fn recycle_pipeline_checks_all_id_and_group_scratch_before_ce_consume() {
    let frame = ethernet_frame(0x0800);
    let owned = [
        completed_msdu(&frame, true, true),
        completed_msdu(&frame, true, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    session.post(1).unwrap();
    let mut transport_scratch = [0; 80];
    let mut frame_scratch = [0; 64];
    let mut msdus = [None; 1];
    let mut leases = [None; 2];
    let mut recycled = [u32::MAX; 2];
    let consumed = Cell::new(false);

    let result = sunfish_qcacld_rx_pipeline::consume_and_deliver_with_cpu_sync_and_collect(
        endpoint(),
        &mut transport_scratch,
        &mut session,
        &buffers,
        ParseOptions::default(),
        &mut msdus,
        &mut leases,
        &mut frame_scratch,
        &mut |_: &[u8]| Ok::<(), ()>(()),
        |_| Ok::<(), ()>(()),
        &mut recycled,
        |destination| {
            consumed.set(true);
            destination[..8].copy_from_slice(&[5, 0, 0, 0, 0, 0, 0, 0]);
            Ok::<_, ()>(RxCompletion {
                address: 0x9000,
                nbytes: 8,
                transfer_id: 7,
                byte_swapped: false,
            })
        },
    );

    assert_eq!(
        result,
        Err(
            sunfish_qcacld_rx_pipeline::RxPipelineRecycleError::Delivery(
                sunfish_qcacld_rx_delivery::RxDeliverySyncError::Delivery(RxDeliveryError::Claim(
                    sunfish_qcacld_rx_session::Error::OutputTooSmall {
                        needed: 2,
                        available: 1,
                    },
                ),),
            ),
        )
    );
    assert!(!consumed.get());
    assert_eq!(session.posted_count(), 2);
    assert_eq!(session.occupied_count(), 2);
}

#[test]
fn cpu_sync_precedes_each_mpdu_descriptor_and_payload_delivery() {
    let ip = ethernet_frame(0x0800);
    let eapol = ethernet_frame(0x888e);
    let discarded = ethernet_frame(0x0800);
    let owned = [
        completed_msdu(&ip, true, true),
        completed_msdu(&eapol, true, true),
        completed_msdu(&discarded, true, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let indication_bytes = three_mpdu_indication();
    let indication = HttRxIndication::decode(&indication_bytes).unwrap();
    let mut session = SunfishQcacldRxSession::<8>::new().unwrap();
    for buffer_id in 0..3 {
        session.post(buffer_id).unwrap();
    }
    let mut msdus = [None; 2];
    let mut leases = [None; 2];
    let mut scratch = [0; 64];
    let events = RefCell::new(Vec::new());

    let report = sunfish_qcacld_rx_delivery::deliver_indication_with_cpu_sync(
        &mut session,
        &buffers,
        ParseOptions::default(),
        indication,
        &mut msdus,
        &mut leases,
        &mut scratch,
        &mut |_: &[u8]| {
            events.borrow_mut().push("deliver".to_owned());
            Ok::<(), ()>(())
        },
        |buffer_id| {
            events.borrow_mut().push(format!("sync-{buffer_id}"));
            Ok::<(), ()>(())
        },
    )
    .unwrap();

    assert_eq!(report.delivered_msdus, 2);
    assert_eq!(report.filtered_msdus, 1);
    assert_eq!(
        *events.borrow(),
        ["sync-0", "deliver", "sync-1", "deliver", "sync-2"]
    );
    assert_eq!(session.occupied_count(), 0);
}

#[test]
fn cpu_sync_failure_leaves_current_group_posted_and_undelivered() {
    let ip = ethernet_frame(0x0800);
    let eapol = ethernet_frame(0x888e);
    let discarded = ethernet_frame(0x0800);
    let owned = [
        completed_msdu(&ip, true, true),
        completed_msdu(&eapol, true, true),
        completed_msdu(&discarded, true, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let indication_bytes = three_mpdu_indication();
    let indication = HttRxIndication::decode(&indication_bytes).unwrap();
    let mut session = SunfishQcacldRxSession::<8>::new().unwrap();
    for buffer_id in 0..3 {
        session.post(buffer_id).unwrap();
    }
    let mut msdus = [None; 2];
    let mut leases = [None; 2];
    let mut scratch = [0; 64];
    let mut deliveries = 0;

    let result = sunfish_qcacld_rx_delivery::deliver_indication_with_cpu_sync(
        &mut session,
        &buffers,
        ParseOptions::default(),
        indication,
        &mut msdus,
        &mut leases,
        &mut scratch,
        &mut |_: &[u8]| {
            deliveries += 1;
            Ok::<(), ()>(())
        },
        |buffer_id| {
            if buffer_id == 0 {
                Err("cache sync failed")
            } else {
                Ok(())
            }
        },
    );

    assert_eq!(
        result,
        Err(sunfish_qcacld_rx_delivery::RxDeliverySyncError::BufferSync(
            "cache sync failed"
        ))
    );
    assert_eq!(deliveries, 0);
    assert_eq!(session.occupied_count(), 3);
    assert_eq!(session.posted_count(), 3);
}

#[test]
fn rejected_mpdu_status_releases_all_claimed_buffers_without_delivery() {
    let frame = ethernet_frame(0x0800);
    let owned = [completed_msdu(&frame, true, true)];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    let mut msdus = [None; 1];
    let mut leases = [None; 1];
    let mut scratch = [0; 64];
    let mut delivered = false;

    let report = deliver_mpdu(
        &mut session,
        &buffers,
        ParseOptions::default(),
        &[0],
        0,
        status(2), // HTT_RX_IND_MPDU_STATUS_ERR_FCS
        &mut msdus,
        &mut leases,
        &mut scratch,
        &mut |_packet: &[u8]| {
            delivered = true;
            Ok::<(), ()>(())
        },
    )
    .unwrap();

    assert_eq!(report.filtered_msdus, 1);
    assert_eq!(report.delivered_msdus, 0);
    assert!(!delivered);
    assert_eq!(session.occupied_count(), 0);
}

#[test]
fn sink_failure_is_reported_after_releasing_the_completed_group() {
    let frame = ethernet_frame(0x0800);
    let owned = [completed_msdu(&frame, true, true)];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let mut session = SunfishQcacldRxSession::<4>::new().unwrap();
    session.post(0).unwrap();
    let mut msdus = [None; 1];
    let mut leases = [None; 1];
    let mut scratch = [0; 64];

    assert_eq!(
        deliver_mpdu(
            &mut session,
            &buffers,
            ParseOptions::default(),
            &[0],
            0,
            status(HTT_RX_IND_MPDU_STATUS_OK),
            &mut msdus,
            &mut leases,
            &mut scratch,
            &mut |_packet: &[u8]| Err("bridge backpressure"),
        ),
        Err(RxDeliveryError::Sink("bridge backpressure"))
    );
    assert_eq!(session.occupied_count(), 0);
    assert!(leases[0].is_none());
}
