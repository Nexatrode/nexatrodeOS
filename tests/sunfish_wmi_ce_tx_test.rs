#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/qcacld_wmi.rs"]
mod qcacld_wmi;
#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_ce_v3_profile.rs"]
mod qcom_ce_v3_profile;
#[path = "../rust/src/qcom_ce_v3_regs.rs"]
mod qcom_ce_v3_regs;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/qcom_wmi_ce.rs"]
mod qcom_wmi_ce;
#[path = "../rust/src/sunfish_qcacld_resource.rs"]
mod sunfish_qcacld_resource;

use qcacld_wmi::{QcacldPeerLimit, ServiceAvailableView, ServiceReadyView, WmiResourceConfig};
use qcom_ce_v3::RingCursor;
use qcom_ce_v3_profile::{Profile, plan_rings};
use qcom_htc::{WmiTxPlan, encode_wmi_command_frame, parse_wmi_tx_frame};
use qcom_wmi_ce::{
    Direction, DmaPool, Error, HTT_DATA_DESTINATION_CE, HTT_DATA_SERVICE_ID, HTT_DATA_SOURCE_CE,
    RxBufferLedger, RxBufferWindow, SensitiveTxBufferLease, SensitiveTxBufferLedger,
    ServiceCeMapping, TxBufferWindow, WMI_CONTROL_SERVICE_ID, WmiRoute, plan_service_rx_post,
    plan_wmi_rx_post, plan_wmi_tx, validate_service_route, validate_wmi_route,
};
use sunfish_qcacld_resource::{SunfishQcacldResourceInputs, encode_sunfish_qcacld_wmi_init_frame};

const POOL_BASE: u64 = 0x8000_0000;
const POOL_BYTES: u64 = 0x0004_0000;
const ROUTE: [ServiceCeMapping; 3] = [
    ServiceCeMapping {
        service_id: 0x0001,
        direction: Direction::HostToTarget,
        engine: 0,
    },
    ServiceCeMapping {
        service_id: WMI_CONTROL_SERVICE_ID,
        direction: Direction::HostToTarget,
        engine: 3,
    },
    ServiceCeMapping {
        service_id: WMI_CONTROL_SERVICE_ID,
        direction: Direction::TargetToHost,
        engine: 2,
    },
];

fn setup() -> (DmaPool, qcom_ce_v3_profile::RingPlan, TxBufferWindow) {
    let pool = DmaPool::new(POOL_BASE, POOL_BYTES).unwrap();
    let rings = plan_rings(Profile::Mission, pool.base(), pool.bytes()).unwrap();
    let next_page = (pool.base() + rings.total_bytes() + 4095) & !4095;
    let buffer = TxBufferWindow {
        iova: next_page,
        capacity: 512,
    };
    (pool, rings, buffer)
}

#[test]
fn sensitive_tx_ledger_reserves_before_publish_and_cancels_only_exact_ticket() {
    let (pool, _rings, first_buffer) = setup();
    let cursor = RingCursor::new(32).unwrap();
    let ticket = cursor.prepare_next().unwrap();
    let lease = SensitiveTxBufferLease::new(
        7,
        ticket,
        TxBufferWindow {
            iova: first_buffer.iova,
            capacity: 136,
        },
    )
    .unwrap();
    let mut ledger = SensitiveTxBufferLedger::<2>::new();

    ledger.reserve(lease, cursor, pool).unwrap();
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.active().collect::<Vec<_>>(), vec![lease]);
    assert_eq!(
        ledger.cancel_unpublished(6, ticket),
        Err(Error::SensitiveTxBufferNotReserved)
    );
    assert_eq!(ledger.cancel_unpublished(7, ticket), Ok(lease));
    assert!(ledger.is_empty());
}

#[test]
fn sensitive_tx_ledger_rejects_overlapping_or_stale_reservations() {
    let (pool, _rings, first_buffer) = setup();
    let cursor = RingCursor::new(32).unwrap();
    let first_ticket = cursor.prepare_next().unwrap();
    let first = SensitiveTxBufferLease::new(
        7,
        first_ticket,
        TxBufferWindow {
            iova: first_buffer.iova,
            capacity: 136,
        },
    )
    .unwrap();
    let mut ledger = SensitiveTxBufferLedger::<2>::new();
    ledger.reserve(first, cursor, pool).unwrap();

    let overlapping = SensitiveTxBufferLease::new(
        7,
        first_ticket,
        TxBufferWindow {
            iova: first_buffer.iova + 8,
            capacity: 136,
        },
    )
    .unwrap();
    assert_eq!(
        ledger.reserve(overlapping, cursor, pool),
        Err(Error::SensitiveTxBufferOverlap)
    );

    let mut advanced = cursor;
    advanced.commit_publish(first_ticket).unwrap();
    assert_eq!(
        ledger.reserve(first, advanced, pool),
        Err(Error::Cursor(qcom_ce_v3::Error::StaleTicket))
    );
}

fn encoded_frame(available_credits: u16) -> (Vec<u8>, WmiTxPlan) {
    let mut frame = vec![0xa5; 64];
    let plan = encode_wmi_command_frame(1, 256, 64, available_credits, 0x1234, &[0x5a], &mut frame)
        .unwrap();
    frame.truncate(plan.frame_len);
    (frame, plan)
}

fn qcacld_service_ready_fixture() -> Vec<u8> {
    fn append_tlv(output: &mut Vec<u8>, tag: u16, value: &[u8]) {
        output.extend_from_slice(&(value.len() as u16).to_le_bytes());
        output.extend_from_slice(&tag.to_le_bytes());
        output.extend_from_slice(value);
    }

    let mut fixed = [0u8; 128];
    fixed[..4].copy_from_slice(&0x3226_822eu32.to_le_bytes());
    let abi: [u32; 6] = [0x0100_0000, 762, 0x5f41_4351, 0x0000_4c4d, 0, 0];
    for (index, word) in abi.into_iter().enumerate() {
        let start = 4 + index * 4;
        fixed[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }

    let mut service_bitmap = [0u8; 128];
    for service in [99usize] {
        let start = (service / 4) * 4;
        let mut word = u32::from_le_bytes(service_bitmap[start..start + 4].try_into().unwrap());
        word |= 1 << (service % 4);
        service_bitmap[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }

    let mut event = Vec::new();
    append_tlv(&mut event, 32, &fixed);
    append_tlv(&mut event, 33, &[0; 36]);
    append_tlv(&mut event, 16, &service_bitmap);
    event
}

fn qcacld_service_available_fixture() -> Vec<u8> {
    let mut fixed = [0u8; 20];
    fixed[..4].copy_from_slice(&128u32.to_le_bytes());
    let extended_word = (1u32 << (176 & 0x1f)) | (1u32 << (182 & 0x1f));
    fixed[8..12].copy_from_slice(&extended_word.to_le_bytes());
    let mut event = Vec::new();
    event.extend_from_slice(&(fixed.len() as u16).to_le_bytes());
    event.extend_from_slice(&559u16.to_le_bytes());
    event.extend_from_slice(&fixed);
    event
}

fn plan<'a>(
    mappings: &[ServiceCeMapping],
    frame: &'a [u8],
    wmi: WmiTxPlan,
    pool: DmaPool,
    buffer: TxBufferWindow,
    rings: &qcom_ce_v3_profile::RingPlan,
    cursor: RingCursor,
) -> Result<qcom_wmi_ce::WmiCeTxPlan<'a>, Error> {
    plan_wmi_tx(
        mappings,
        frame,
        wmi,
        1,
        7,
        pool,
        buffer,
        Profile::Mission,
        rings,
        cursor,
    )
}

#[test]
fn selected_wmi_service_map_has_one_ce3_tx_and_one_ce2_rx_route() {
    let route = validate_wmi_route(&ROUTE).unwrap();
    assert_eq!(route.host_to_target_engine, 3);
    assert_eq!(route.target_to_host_engine, 2);
}

#[test]
fn missing_duplicate_and_wrong_wmi_mappings_fail_closed() {
    assert_eq!(validate_wmi_route(&ROUTE[..1]), Err(Error::MissingWmiRoute));
    let duplicate = [ROUTE[1], ROUTE[1], ROUTE[2]];
    assert_eq!(
        validate_wmi_route(&duplicate),
        Err(Error::DuplicateWmiRoute)
    );
    let wrong = [
        ROUTE[0],
        ServiceCeMapping {
            engine: 2,
            ..ROUTE[1]
        },
        ROUTE[2],
    ];
    assert_eq!(validate_wmi_route(&wrong), Err(Error::WrongWmiRoute));
}

#[test]
fn framed_wmi_command_composes_with_ce3_descriptor_without_publishing() {
    let (pool, rings, buffer) = setup();
    let (frame, wmi) = encoded_frame(4);
    let cursor = RingCursor::new(32).unwrap();
    let result = plan(&ROUTE, &frame, wmi, pool, buffer, &rings, cursor).unwrap();

    assert_eq!(result.route().host_to_target_engine, 3);
    assert_eq!(result.route().target_to_host_engine, 2);
    assert_eq!(result.ticket().index(), 0);
    assert_eq!(
        result.descriptor_slot_iova(),
        rings.allocations()[3].descriptor_iova
    );
    assert_eq!(result.descriptor().address(), buffer.iova);
    assert_eq!(usize::from(result.descriptor().nbytes()), frame.len());
    assert_eq!(result.descriptor().transfer_id(), 7);
    assert_eq!(result.frame_bytes() as usize, frame.len());
    assert_eq!(result.next_producer_index(), 1);
    assert_eq!(cursor.producer_index(), 0);
    assert_eq!(cursor.pending(), 0);
    assert_eq!(result.descriptor().as_bytes()[4] & 0x01, 0);
}

#[test]
fn source_derived_qcacld_wmi_init_composes_through_htc_and_ce3_preflight() {
    let service_ready = qcacld_service_ready_fixture();
    let service_ready = ServiceReadyView::parse(&service_ready).unwrap();
    let service_available = qcacld_service_available_fixture();
    let service_available = ServiceAvailableView::parse(&service_available).unwrap();
    let mut inputs =
        SunfishQcacldResourceInputs::lineage_sunfish_candidate(QcacldPeerLimit::OtherTarget32);
    inputs.enable_peer_unmap_confirmation = true;

    let mut wmi_body = [0u8; 512];
    let mut htc_frame = [0u8; 512];
    let wmi_init = encode_sunfish_qcacld_wmi_init_frame(
        &service_ready,
        &service_available,
        inputs,
        None,
        1,
        512,
        64,
        16,
        &mut wmi_body,
        &mut htc_frame,
    )
    .unwrap();
    assert_eq!(wmi_init.body_bytes, 312);
    assert_eq!(wmi_init.htc.frame_len, 324);
    let parsed = parse_wmi_tx_frame(&htc_frame[..324], 1, 512).unwrap();
    assert_eq!(parsed.command_id, qcacld_wmi::SUNFISH_WMI_INIT_COMMAND_ID);
    assert_eq!(parsed.tlv_body, &wmi_body[..312]);
    let resource = WmiResourceConfig::parse(&parsed.tlv_body[32..308]).unwrap();
    assert_eq!(resource.num_peers(), 38);
    assert_eq!(
        resource.feature_flags(),
        (1 << 9) | (1 << 10) | (1 << 19) | (1 << 20)
    );

    let (pool, rings, buffer) = setup();
    let cursor = RingCursor::new(32).unwrap();
    let tx = plan(
        &ROUTE,
        &htc_frame[..wmi_init.htc.frame_len],
        wmi_init.htc,
        pool,
        buffer,
        &rings,
        cursor,
    )
    .unwrap();
    assert_eq!(tx.route().host_to_target_engine, 3);
    assert_eq!(tx.descriptor().address(), buffer.iova);
    assert_eq!(
        usize::from(tx.descriptor().nbytes()),
        wmi_init.htc.frame_len
    );
    assert_eq!(cursor.producer_index(), 0);
    assert_eq!(cursor.pending(), 0);
}

#[test]
fn next_descriptor_slot_tracks_existing_cursor_position() {
    let (pool, rings, buffer) = setup();
    let (frame, wmi) = encoded_frame(4);
    let mut cursor = RingCursor::new(32).unwrap();
    let first = cursor.prepare_next().unwrap();
    cursor.commit_publish(first).unwrap();
    let result = plan(&ROUTE, &frame, wmi, pool, buffer, &rings, cursor).unwrap();
    assert_eq!(result.ticket().index(), 1);
    assert_eq!(
        result.descriptor_slot_iova(),
        rings.allocations()[3].descriptor_iova + 16
    );
    assert_eq!(result.next_producer_index(), 2);
}

#[test]
fn full_ring_is_rejected_without_changing_the_cursor() {
    let (pool, rings, buffer) = setup();
    let (frame, wmi) = encoded_frame(4);
    let mut cursor = RingCursor::new(32).unwrap();
    for _ in 0..31 {
        let ticket = cursor.prepare_next().unwrap();
        cursor.commit_publish(ticket).unwrap();
    }
    let before = cursor;
    assert_eq!(
        plan(&ROUTE, &frame, wmi, pool, buffer, &rings, cursor),
        Err(Error::Cursor(qcom_ce_v3::Error::RingFull))
    );
    assert_eq!(cursor, before);
}

#[test]
fn buffer_capacity_alignment_bounds_and_ring_overlap_are_checked() {
    let (pool, rings, buffer) = setup();
    let (frame, wmi) = encoded_frame(4);
    let cursor = RingCursor::new(32).unwrap();

    assert_eq!(
        plan(
            &ROUTE,
            &frame,
            wmi,
            pool,
            TxBufferWindow {
                capacity: 1,
                ..buffer
            },
            &rings,
            cursor
        ),
        Err(Error::BufferTooSmall)
    );
    assert_eq!(
        plan(
            &ROUTE,
            &frame,
            wmi,
            pool,
            TxBufferWindow {
                iova: buffer.iova + 1,
                ..buffer
            },
            &rings,
            cursor
        ),
        Err(Error::BufferOutsideDmaPool)
    );
    assert_eq!(
        plan(
            &ROUTE,
            &frame,
            wmi,
            pool,
            TxBufferWindow {
                iova: POOL_BASE,
                ..buffer
            },
            &rings,
            cursor
        ),
        Err(Error::BufferOverlapsRing)
    );
    assert_eq!(
        plan(
            &ROUTE,
            &frame,
            wmi,
            pool,
            TxBufferWindow {
                iova: POOL_BASE + POOL_BYTES,
                ..buffer
            },
            &rings,
            cursor
        ),
        Err(Error::BufferOutsideDmaPool)
    );
    assert_eq!(
        plan(
            &ROUTE,
            &frame,
            wmi,
            pool,
            TxBufferWindow {
                capacity: POOL_BYTES as u32,
                ..buffer
            },
            &rings,
            cursor
        ),
        Err(Error::BufferOutsideDmaPool)
    );
}

#[test]
fn malformed_frame_and_plan_combinations_are_rejected() {
    let (pool, rings, buffer) = setup();
    let (frame, wmi) = encoded_frame(4);
    let cursor = RingCursor::new(32).unwrap();

    let mut malformed = frame.clone();
    malformed[4] = 1;
    assert_eq!(
        plan(&ROUTE, &malformed, wmi, pool, buffer, &rings, cursor),
        Err(Error::InvalidFrame)
    );
    let mut malformed = frame.clone();
    malformed[8 + 3] = 1;
    assert_eq!(
        plan(&ROUTE, &malformed, wmi, pool, buffer, &rings, cursor),
        Err(Error::InvalidFrame)
    );
    let mut malformed = frame.clone();
    malformed[1] = 0;
    assert_eq!(
        plan(&ROUTE, &malformed, wmi, pool, buffer, &rings, cursor),
        Err(Error::InvalidFrame)
    );
    assert_eq!(
        plan(
            &ROUTE,
            &frame,
            WmiTxPlan {
                frame_len: wmi.frame_len + 1,
                ..wmi
            },
            pool,
            buffer,
            &rings,
            cursor,
        ),
        Err(Error::InvalidPlan)
    );
    assert_eq!(
        plan(
            &ROUTE,
            &frame[..frame.len() - 1],
            wmi,
            pool,
            buffer,
            &rings,
            cursor
        ),
        Err(Error::InvalidPlan)
    );
}

#[test]
fn tx_credit_update_flag_must_match_the_htc_plan() {
    let (pool, rings, buffer) = setup();
    let (mut frame, wmi) = encoded_frame(64);
    assert!(!wmi.requested_credit_update);
    frame[1] = 1;
    assert_eq!(
        plan(
            &ROUTE,
            &frame,
            wmi,
            pool,
            buffer,
            &rings,
            RingCursor::new(32).unwrap()
        ),
        Err(Error::InvalidFrame)
    );
}

#[test]
fn invalid_pools_are_rejected_before_any_descriptor_plan() {
    assert_eq!(DmaPool::new(0, POOL_BYTES), Err(Error::InvalidDmaPool));
    assert_eq!(
        DmaPool::new(POOL_BASE + 1, POOL_BYTES),
        Err(Error::InvalidDmaPool)
    );
    assert_eq!(DmaPool::new(POOL_BASE, 1), Err(Error::InvalidDmaPool));
    assert_eq!(
        DmaPool::new(qcom_ce_v3::MAX_CE_IOVA - 4096, 8192),
        Err(Error::DmaPoolOutsideCeWindow)
    );
}

#[test]
fn wmi_receive_post_targets_ce2_and_keeps_its_buffer_reserved_by_the_ledger() {
    let (pool, rings, buffer) = setup();
    let ledger = RxBufferLedger::new();
    let cursor = RingCursor::new(128).unwrap();
    let result = plan_wmi_rx_post(
        &ROUTE,
        Profile::Mission,
        pool,
        RxBufferWindow {
            iova: buffer.iova,
            capacity: 2048,
        },
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();

    let ce2_ring = rings
        .allocations()
        .iter()
        .find(|allocation| {
            allocation.engine == 2
                && allocation.direction == qcom_ce_v3_profile::RingDirection::Destination
        })
        .unwrap();
    assert_eq!(result.route().host_to_target_engine, 3);
    assert_eq!(result.route().target_to_host_engine, 2);
    assert_eq!(result.ticket().index(), 0);
    assert_eq!(result.descriptor().address(), buffer.iova);
    assert_eq!(result.descriptor().as_bytes()[8..10], [0, 0]);
    assert_eq!(result.descriptor_slot_iova(), ce2_ring.descriptor_iova);
    assert_eq!(result.next_producer_index(), 1);
    assert_eq!(cursor.producer_index(), 0);
    assert!(ledger.is_empty());
}

#[test]
fn rx_posts_reject_overlaps_bad_ranges_and_a_full_ownership_ledger() {
    let (pool, rings, buffer) = setup();
    let mut ledger = RxBufferLedger::new();
    let cursor = RingCursor::new(128).unwrap();
    let window = RxBufferWindow {
        iova: buffer.iova,
        capacity: 512,
    };
    let mut reservations = RingCursor::new(128).unwrap();
    let first_ticket = reservations.prepare_next().unwrap();
    reservations.commit_publish(first_ticket).unwrap();
    ledger.reserve(window, first_ticket).unwrap();
    assert_eq!(
        plan_wmi_rx_post(
            &ROUTE,
            Profile::Mission,
            pool,
            window,
            &ledger,
            &rings,
            cursor
        ),
        Err(Error::ReceiveBufferOverlap)
    );
    assert_eq!(
        plan_wmi_rx_post(
            &ROUTE,
            Profile::Mission,
            pool,
            RxBufferWindow {
                iova: window.iova + 256,
                capacity: 512,
            },
            &ledger,
            &rings,
            cursor,
        ),
        Err(Error::ReceiveBufferOverlap)
    );
    assert_eq!(
        plan_wmi_rx_post(
            &ROUTE,
            Profile::Mission,
            pool,
            RxBufferWindow {
                iova: POOL_BASE,
                capacity: 512,
            },
            &RxBufferLedger::new(),
            &rings,
            cursor,
        ),
        Err(Error::BufferOverlapsRing)
    );
    assert_eq!(
        plan_wmi_rx_post(
            &ROUTE,
            Profile::Mission,
            pool,
            RxBufferWindow {
                iova: buffer.iova,
                capacity: 2049,
            },
            &RxBufferLedger::new(),
            &rings,
            cursor,
        ),
        Err(Error::InvalidDestinationRing)
    );

    let mut full = RxBufferLedger::new();
    let mut full_cursor = RingCursor::new(128).unwrap();
    let mut first_full_ticket = None;
    for index in 0..qcom_wmi_ce::MAX_POSTED_WMI_RX_BUFFERS {
        let ticket = full_cursor.prepare_next().unwrap();
        full_cursor.commit_publish(ticket).unwrap();
        first_full_ticket.get_or_insert(ticket);
        full.reserve(
            RxBufferWindow {
                iova: buffer.iova + (index as u64 * 512),
                capacity: 256,
            },
            ticket,
        )
        .unwrap();
    }
    assert_eq!(
        plan_wmi_rx_post(
            &ROUTE,
            Profile::Mission,
            pool,
            window,
            &full,
            &rings,
            cursor
        ),
        Err(Error::ReceiveLedgerFull)
    );
    assert_eq!(full.len(), qcom_wmi_ce::MAX_POSTED_WMI_RX_BUFFERS);
    assert_eq!(
        full.release(
            first_full_ticket.unwrap(),
            RxBufferWindow {
                iova: window.iova + 1,
                capacity: 512,
            }
        ),
        Err(Error::ReceiveBufferNotReserved)
    );
}

#[test]
fn htt_rx_post_uses_the_live_selected_ce1_route_and_full_receive_capacity() {
    let pool = DmaPool::new(POOL_BASE, POOL_BYTES).unwrap();
    let rings = plan_rings(Profile::Mission, pool.base(), pool.bytes()).unwrap();
    let route = [
        ServiceCeMapping {
            service_id: HTT_DATA_SERVICE_ID,
            direction: Direction::HostToTarget,
            engine: HTT_DATA_SOURCE_CE,
        },
        ServiceCeMapping {
            service_id: HTT_DATA_SERVICE_ID,
            direction: Direction::TargetToHost,
            engine: HTT_DATA_DESTINATION_CE,
        },
    ];
    assert_eq!(
        validate_service_route(&route, HTT_DATA_SERVICE_ID),
        Ok(WmiRoute {
            host_to_target_engine: 4,
            target_to_host_engine: 1,
        })
    );

    let buffer_iova = (pool.base() + rings.total_bytes() + 4095) & !4095;
    let buffer = RxBufferWindow {
        iova: buffer_iova,
        capacity: 2048,
    };
    let ledger = RxBufferLedger::new();
    let cursor = RingCursor::new(512).unwrap();
    let plan = plan_service_rx_post(
        &route,
        HTT_DATA_SERVICE_ID,
        WmiRoute {
            host_to_target_engine: 4,
            target_to_host_engine: 1,
        },
        Profile::Mission,
        pool,
        buffer,
        &ledger,
        &rings,
        cursor,
    )
    .unwrap();
    assert_eq!(plan.service_id(), HTT_DATA_SERVICE_ID);
    assert_eq!(plan.route().target_to_host_engine, 1);
    assert_eq!(plan.descriptor().address(), buffer_iova);
    assert_eq!(&plan.descriptor().as_bytes()[8..10], &[0, 0]);
    assert_eq!(
        plan.descriptor_slot_iova(),
        rings
            .allocations()
            .iter()
            .find(|allocation| allocation.engine == HTT_DATA_DESTINATION_CE)
            .unwrap()
            .descriptor_iova
    );
    assert_eq!(plan.next_producer_index(), 1);
    ledger.preview_reserve(plan.buffer()).unwrap();
}

#[test]
fn htt_rx_post_rejects_wrong_route_duplicate_service_and_short_buffers() {
    let pool = DmaPool::new(POOL_BASE, POOL_BYTES).unwrap();
    let rings = plan_rings(Profile::Mission, pool.base(), pool.bytes()).unwrap();
    let route = [
        ServiceCeMapping {
            service_id: HTT_DATA_SERVICE_ID,
            direction: Direction::HostToTarget,
            engine: 4,
        },
        ServiceCeMapping {
            service_id: HTT_DATA_SERVICE_ID,
            direction: Direction::TargetToHost,
            engine: 1,
        },
    ];
    let expected = WmiRoute {
        host_to_target_engine: 4,
        target_to_host_engine: 1,
    };
    let buffer_iova = (pool.base() + rings.total_bytes() + 4095) & !4095;
    let ledger = RxBufferLedger::new();
    let cursor = RingCursor::new(512).unwrap();
    assert_eq!(
        plan_service_rx_post(
            &route,
            HTT_DATA_SERVICE_ID,
            WmiRoute {
                host_to_target_engine: 3,
                target_to_host_engine: 2,
            },
            Profile::Mission,
            pool,
            RxBufferWindow {
                iova: buffer_iova,
                capacity: 2048
            },
            &ledger,
            &rings,
            cursor,
        ),
        Err(Error::WrongServiceRoute)
    );
    let duplicate = [route[0], route[1], route[1]];
    assert_eq!(
        validate_service_route(&duplicate, HTT_DATA_SERVICE_ID),
        Err(Error::DuplicateServiceRoute)
    );
    assert_eq!(
        plan_service_rx_post(
            &route,
            HTT_DATA_SERVICE_ID,
            expected,
            Profile::Mission,
            pool,
            RxBufferWindow {
                iova: buffer_iova,
                capacity: 1024
            },
            &ledger,
            &rings,
            cursor,
        ),
        Err(Error::InvalidDestinationRing)
    );
}
