// Legacy fixture imports include passive probe APIs unused in this gate suite.
#![allow(dead_code)]
#[path = "../rust/src/qcom_wlfw_bdf.rs"]
pub mod qcom_wlfw_bdf;
// Use the actual authentication/ready fixture, Window/lock and SMEM decoders.
include!("sunfish_glink_inventory_test.rs");
#[path = "../rust/src/pixel_modem_owner_gate.rs"]
pub mod pixel_modem_owner_gate;
#[path = "../rust/src/qcom_glink_fifo.rs"]
pub mod qcom_glink_fifo;
#[path = "../rust/src/qcom_glink_native_io.rs"]
pub mod qcom_glink_native_io;
#[allow(dead_code)]
#[path = "../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
#[path = "../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
pub mod qcom_ipa_service;
#[path = "../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../rust/src/qcom_modem_services.rs"]
pub mod qcom_modem_services;
#[path = "../rust/src/qcom_msa.rs"]
pub mod qcom_msa;
#[path = "../rust/src/qcom_qmi.rs"]
pub mod qcom_qmi;
#[path = "../rust/src/qcom_qmi_router.rs"]
pub mod qcom_qmi_router;
#[path = "../rust/src/qcom_qrtr_control.rs"]
pub mod qcom_qrtr_control;
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../rust/src/qcom_servreg_notif.rs"]
pub mod qcom_servreg_notif;
#[path = "../rust/src/qcom_servreg_notif_client.rs"]
pub mod qcom_servreg_notif_client;
#[path = "../rust/src/qcom_tftp.rs"]
pub mod qcom_tftp;
#[path = "../rust/src/qcom_wlfw_client.rs"]
pub mod qcom_wlfw_client;
#[path = "../rust/src/sunfish_ipa_profile.rs"]
pub mod sunfish_ipa_profile;
#[path = "../rust/src/wlfw_protocol.rs"]
pub mod wlfw_protocol;
use qcom_glink_native_io::Gate;

fn smem_item(memory: &[u8], id: u16) -> Option<qcom_modem_status::Item> {
    qcom_modem_status::allocation_directory(&Bytes(memory), id)
        .unwrap()
        .item
}

fn owner_gate_fixture() -> (
    Vec<u8>,
    Box<u32>,
    pixel_modem_owner_gate::LiveGate,
    [qcom_modem_status::Item; 3],
) {
    let mut m = mapped();
    let ready = ready_observation(&mut m);
    let items = [
        allocate(&mut m, 478, 32, false),
        allocate(&mut m, 479, 16384, false),
        allocate(&mut m, 480, 16384, true),
    ];
    let baseline = pixel_glink_inventory::inspect_ref(&Bytes(&m), &ready).unwrap();
    let mut lock = Box::new(0);
    let window = unsafe { Window::new(m.as_ptr(), &mut *lock) };
    (
        m,
        lock,
        pixel_modem_owner_gate::LiveGate::new(window, ready, baseline),
        items,
    )
}
#[test]
fn owner_gate_reads_live_directory_status_and_preserves_smem() {
    let (m, lock, mut gate, _) = owner_gate_fixture();
    let before = m.clone();
    assert!(qcom_modem_status::directory(&Bytes(&m))
        .unwrap()
        .item
        .is_some());
    assert!(qcom_modem_status::decode(&payload(6)).unwrap().ready_bit());
    let observation = gate.observe().unwrap();
    assert_eq!(observation.generation, 1);
    assert!(observation.ready);
    assert!(gate.observe().is_ok());
    assert_eq!(m, before);
    assert_eq!(*lock, 0);
}
#[test]
fn owner_gate_fatal_ready_loss_and_stop_bits_latch_closed() {
    for status in [0, 1, 3, 10, 130] {
        let (mut m, _lock, mut gate, _) = owner_gate_fixture();
        let peer = smem_item(&m, 435).unwrap();
        w32(&mut m, peer.offset + 36, status);
        assert!(gate.observe().is_err());
        w32(&mut m, peer.offset + 36, 6);
        assert!(gate.observe().is_err());
    }
}
#[test]
fn owner_gate_rejects_relocated_corrupted_or_duplicated_transport_items() {
    for fault in 0..3 {
        let (mut m, _lock, mut gate, items) = owner_gate_fixture();
        match fault {
            0 => w16(&mut m, items[0].offset - 16 + 2, 999),
            1 => w32(&mut m, items[1].offset - 16 + 4, 8),
            _ => {
                allocate(&mut m, 478, 32, false);
            }
        }
        assert!(gate.observe().is_err());
    }
}
#[test]
fn owner_gate_rejects_ap_status_identity_tampering_and_route_changes() {
    for fault in 0..2 {
        let (mut m, _lock, mut gate, _) = owner_gate_fixture();
        if fault == 0 {
            let ap = smem_item(&m, 428).unwrap();
            m[ap.offset + 20] ^= 1;
        } else {
            w32(&mut m, SMEM_BYTES - 4096 + 48, 64);
        }
        assert!(gate.observe().is_err());
    }
}
#[test]
fn owner_gate_allows_unrelated_well_formed_directory_growth() {
    let (mut m, _lock, mut gate, _) = owner_gate_fixture();
    allocate(&mut m, 700, 32, false);
    assert!(gate.observe().is_ok());
}
#[test]
fn owner_gate_repeated_checks_release_only_its_lock_without_smem_writes() {
    let (m, mut lock, mut gate, _) = owner_gate_fixture();
    let before = m.clone();
    // Host semaphore accepts writes, matching Window's finite acquisition.
    // Retained neighboring memory and all SMEM remain unchanged on repeated checks.
    *lock = 0;
    assert!(gate.observe().is_ok());
    assert_eq!(*lock, 0);
    assert_eq!(m, before);
}
