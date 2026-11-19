//! Synthetic, no-hardware IPA codec checks shared by the target probe and host tests.

use crate::{
    qcom_ipa,
    sunfish_ipa_profile::{self, RuntimeSnapshot},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    InvalidProfile,
    RequestEncode,
    RequestHeader,
    RequestTlvs,
    SuccessResponse,
    FailureResponse,
}

pub fn check() -> Result<(), Failure> {
    // Synthetic register snapshot exercises the exact Sunfish IPA v4.2
    // profile. The caller has no IPA MMIO or QRTR transport capability.
    let fields = sunfish_ipa_profile::init_request_fields(RuntimeSnapshot {
        shared_mem_baddr_units: 0x10,
        shared_mem_size_units: 0x400,
        apps_wan_cons_endpoint: Some(sunfish_ipa_profile::APPS_WAN_CONS_ENDPOINT),
        ipa_uc_loaded: false,
    })
    .map_err(|_| Failure::InvalidProfile)?;

    let mut request = [0u8; qcom_ipa::MAX_QMI_BYTES];
    let request_len = fields
        .encode(0x1234, &mut request)
        .map_err(|_| Failure::RequestEncode)?;
    if request_len != 118
        || request[..qcom_ipa::QMI_HEADER_BYTES] != [0, 0x34, 0x12, 0x21, 0, 0x6f, 0]
    {
        return Err(Failure::RequestHeader);
    }

    let expected_tags = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x1f, 0x20, 0x21, 0x22,
    ];
    let mut cursor = qcom_ipa::QMI_HEADER_BYTES;
    for expected_tag in expected_tags {
        if request_len - cursor < 3 || request[cursor] != expected_tag {
            return Err(Failure::RequestTlvs);
        }
        let value_len = u16::from_le_bytes([request[cursor + 1], request[cursor + 2]]) as usize;
        let Some(next) = cursor.checked_add(3 + value_len) else {
            return Err(Failure::RequestTlvs);
        };
        if next > request_len {
            return Err(Failure::RequestTlvs);
        }
        cursor = next;
    }
    if cursor != request_len {
        return Err(Failure::RequestTlvs);
    }

    // Common response and control-endpoint TLVs are seven bytes apiece.
    let success_response_payload_bytes = 7 + 7;
    let success_response = [
        2,
        0x34,
        0x12,
        0x21,
        0,
        success_response_payload_bytes as u8,
        0,
        0x02,
        4,
        0,
        0,
        0,
        0,
        0,
        0x10,
        4,
        0,
        9,
        0,
        0,
        0,
    ];
    let success = qcom_ipa::decode_init_response(&success_response, 0x1234)
        .map_err(|_| Failure::SuccessResponse)?;
    if success.result != 0 || success.error != 0 || success.control_endpoint != Some(9) {
        return Err(Failure::SuccessResponse);
    }

    let failure_response = [2, 0x34, 0x12, 0x21, 0, 7, 0, 0x02, 4, 0, 1, 0, 0x17, 0];
    if !matches!(
        qcom_ipa::decode_init_response(&failure_response, 0x1234),
        Ok(response) if response.result == 1 && response.error == 0x17
    ) {
        return Err(Failure::FailureResponse);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn synthetic_target_ipa_codec_check_passes() {
        assert_eq!(check(), Ok(()));
    }
}
