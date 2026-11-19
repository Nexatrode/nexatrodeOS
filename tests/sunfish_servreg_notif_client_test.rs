//! Offline QRTR client tests; no modem, device, or transport access.
#[path = "../rust/src/qcom_modem_services.rs"]
#[allow(dead_code)]
mod qcom_modem_services;
#[path = "../rust/src/qcom_qmi.rs"]
#[allow(dead_code)]
mod qcom_qmi;
#[path = "../rust/src/qcom_qmi_router.rs"]
#[allow(dead_code)]
mod qcom_qmi_router;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
mod qcom_ipa_service;
#[path = "../rust/src/qcom_qrtr_control.rs"]
#[allow(dead_code)]
mod qcom_qrtr_control;
#[path = "../rust/src/qcom_servreg.rs"]
#[allow(dead_code)]
mod qcom_servreg;
#[path = "../rust/src/qcom_servreg_notif.rs"]
mod qcom_servreg_notif;
#[path = "../rust/src/qcom_servreg_notif_client.rs"]
mod qcom_servreg_notif_client;

#[test]
fn captured_sunfish_notif_server_binding_matches_service_and_instance() {
    assert_eq!(qcom_servreg_notif_client::VERSION_INSTANCE, 0xb401);
    assert_eq!(qcom_servreg_notif::SERVICE_ID, 0x42);
    assert_eq!(qcom_servreg_notif::SERVICE_VERSION, 1);
    let local = qcom_qmi_router::Address { node: 1, port: 0x4002 };
    let client = qcom_servreg_notif_client::Client::new(local, 1).unwrap();
    assert_eq!(client.local_address(), local);
}
