#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_htt.rs"]
mod ath10k_htt;
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
#[path = "../rust/src/sunfish_qcacld_key.rs"]
mod sunfish_qcacld_key;
#[path = "../rust/src/wlan_key_confirmation.rs"]
mod wlan_key_confirmation;

use qcom_ce_v3::RingCursor;
use qcom_ce_v3_profile::{Profile, plan_rings};
use qcom_wmi_ce::{
    Direction, DmaPool, SensitiveTxBufferLedger, ServiceCeMapping, TxBufferWindow,
    WMI_CONTROL_SERVICE_ID,
};
use sunfish_qcacld_key::{
    CcmpInstallKey, KeyKind, WMI_VDEV_INSTALL_KEY_BODY_BYTES, plan_qcacld_key_install_ce_tx,
};

const POOL_BASE: u64 = 0x8000_0000;
const POOL_BYTES: u64 = 0x0004_0000;
const ROUTE: [ServiceCeMapping; 2] = [
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

fn key_request<'a>(key: &'a [u8; 16]) -> CcmpInstallKey<'a> {
    CcmpInstallKey {
        vdev_id: 1,
        peer_address: [0x02, 0x12, 0x34, 0x56, 0x78, 0x9a],
        key_index: 0,
        kind: KeyKind::Pairwise,
        temporal_key: key,
        group_receive_sequence_counter: &[0; 8],
    }
}

#[test]
fn key_command_composes_into_htc_and_ce3_plan_and_clears_tlv_scratch() {
    let key = [0x5a; 16];
    let pool = DmaPool::new(POOL_BASE, POOL_BYTES).unwrap();
    let rings = plan_rings(Profile::Mission, pool.base(), pool.bytes()).unwrap();
    let buffer_iova = (pool.base() + rings.total_bytes() + 4095) & !4095;
    let buffer = TxBufferWindow {
        iova: buffer_iova,
        capacity: 512,
    };
    let cursor = RingCursor::new(32).unwrap();
    let mut body = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
    let mut frame = [0xa5; 256];

    let mut plan = plan_qcacld_key_install_ce_tx(
        9,
        key_request(&key),
        1,
        512,
        64,
        8,
        &ROUTE,
        3,
        pool,
        buffer,
        Profile::Mission,
        &rings,
        cursor,
        &mut body,
        &mut frame,
    )
    .unwrap();

    assert_eq!(plan.generation(), 9);
    assert_eq!(plan.command().command_id, 0x5009);
    assert_eq!(plan.command().body_bytes, WMI_VDEV_INSTALL_KEY_BODY_BYTES);
    assert_eq!(plan.htc_plan().credits_required, 3);
    let ce_plan = plan.ce_plan().unwrap();
    assert_eq!(ce_plan.frame_bytes(), 136);
    let decoded = qcom_htc::parse_wmi_tx_frame(ce_plan.frame(), 1, 512).unwrap();
    assert_eq!(decoded.command_id, 0x5009);
    assert_eq!(decoded.tlv_body.len(), WMI_VDEV_INSTALL_KEY_BODY_BYTES);
    assert_eq!(&decoded.tlv_body[108..124], &key);
    assert_eq!(body, [0; WMI_VDEV_INSTALL_KEY_BODY_BYTES]);
    assert_eq!(cursor.producer_index(), 0);

    let frame_bytes = plan.frame_bytes();
    let lease = plan.sensitive_buffer_lease().unwrap();
    let mut ledger = SensitiveTxBufferLedger::<2>::new();
    ledger.reserve(lease, cursor, pool).unwrap();
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.cancel_unpublished(9, lease.ticket()).unwrap(), lease);
    assert!(ledger.is_empty());
    plan.wipe_frame();
    drop(plan);
    assert_eq!(&frame[..frame_bytes], &[0; 136]);
}

#[test]
fn failed_key_transport_plan_erases_both_scratch_buffers() {
    let key = [0x5a; 16];
    let pool = DmaPool::new(POOL_BASE, POOL_BYTES).unwrap();
    let rings = plan_rings(Profile::Mission, pool.base(), pool.bytes()).unwrap();
    let buffer_iova = (pool.base() + rings.total_bytes() + 4095) & !4095;
    let cursor = RingCursor::new(32).unwrap();
    let mut body = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
    let mut frame = [0xa5; 256];
    let wrong_route = [ROUTE[0]];

    assert!(
        plan_qcacld_key_install_ce_tx(
            9,
            key_request(&key),
            1,
            512,
            64,
            8,
            &wrong_route,
            3,
            pool,
            TxBufferWindow {
                iova: buffer_iova,
                capacity: 512,
            },
            Profile::Mission,
            &rings,
            cursor,
            &mut body,
            &mut frame,
        )
        .is_err()
    );
    assert_eq!(body, [0; WMI_VDEV_INSTALL_KEY_BODY_BYTES]);
    assert_eq!(frame, [0; 256]);
    assert_eq!(cursor.producer_index(), 0);
}
