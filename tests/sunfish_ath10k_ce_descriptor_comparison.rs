//! Cross-check the two distinct software CE descriptor interfaces.
//! These tests do not establish that either format has been accepted by hardware.

#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;

#[test]
fn raw_transfer_id_is_not_interchangeable() {
    let ath = ath10k_ce::Wcn3990Descriptor::encode(0x1234_5000, 100, 1, false, false).unwrap();
    let qcom = qcom_ce_v3::TxDescriptor::encode(0x1234_5000, 100, 1, false, false).unwrap();
    assert_eq!(&ath.as_bytes()[10..12], &[0x10, 0]);
    assert_eq!(&qcom.as_bytes()[10..12], &[0x01, 0]);
    assert_ne!(ath.as_bytes(), qcom.as_bytes());
}

#[test]
fn shifted_id_matches_only_the_plain_common_subset() {
    for id in [0, 1, 0xff, 0xfff] {
        let ath = ath10k_ce::Wcn3990Descriptor::encode(0x1234_5000, 100, id, false, false).unwrap();
        let qcom = qcom_ce_v3::TxDescriptor::encode(0x1234_5000, 100, u32::from(id) << 4, false, false).unwrap();
        assert_eq!(ath.as_bytes(), qcom.as_bytes());
    }
}

#[test]
fn shifted_id_does_not_make_gather_or_swap_equivalent() {
    for (gather, swap) in [(true, false), (false, true), (true, true)] {
        let ath = ath10k_ce::Wcn3990Descriptor::encode(0x1234_5000, 100, 1, gather, swap).unwrap();
        let qcom = qcom_ce_v3::TxDescriptor::encode(0x1234_5000, 100, 16, swap, gather).unwrap();
        assert_ne!(ath.as_bytes(), qcom.as_bytes());
    }
}

#[test]
fn qcom_address_acceptance_does_not_establish_ath10k_acceptance() {
    let address = 1_u64 << 35;
    assert!(qcom_ce_v3::TxDescriptor::encode(address, 100, 0, false, false).is_ok());
    assert!(ath10k_ce::Wcn3990Descriptor::encode(address, 100, 0, false, false).is_err());
}
