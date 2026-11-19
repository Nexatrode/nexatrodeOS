//! Offline SERVREG_NOTIF QUERY_STATE codec tests; no device or transport access.
#[path = "../rust/src/qcom_qmi.rs"]
#[allow(dead_code)]
mod qcom_qmi;
#[path = "../rust/src/qcom_servreg_notif.rs"]
mod qcom_servreg_notif;

#[test]
fn pinned_wlan_domain_query_constants_are_bounded() {
    assert_eq!(qcom_servreg_notif::SERVICE_ID, 0x42);
    assert_eq!(qcom_servreg_notif::SERVICE_VERSION, 1);
    assert_eq!(
        qcom_servreg_notif::WLAN_PD_DOMAIN_NAME,
        b"msm/modem/wlan_pd"
    );
    assert_eq!(qcom_servreg_notif::WLAN_PD_QUERY_BYTES, 27);
    assert!(qcom_servreg_notif::WLAN_PD_QUERY_BYTES <= qcom_servreg_notif::MAX_QUERY_BYTES);
    assert_eq!(
        qcom_servreg_notif::STATE_UPDATED_INDICATION_MESSAGE_ID,
        0x22
    );
    assert_eq!(qcom_servreg_notif::SET_ACK_MESSAGE_ID, 0x23);
    assert_eq!(
        qcom_servreg_notif::WLAN_PD_STATE_UPDATED_INDICATION_BYTES,
        86
    );
    assert_eq!(qcom_servreg_notif::WLAN_PD_SET_ACK_REQUEST_BYTES, 32);
}
