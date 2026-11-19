//! Bounded parser for the ath10k firmware API metadata container.
//!
//! The parser copies only feature/version metadata. Firmware image payloads
//! are skipped and never exposed, staged, or executed here.

const ATH10K_FIRMWARE_MAGIC: &[u8; 11] = b"QCA-ATH10K\0";
const ATH10K_FIRMWARE_MAGIC_PADDED: usize = 12;
const ATH10K_FW_IE_HEADER_BYTES: usize = 8;
const ATH10K_FW_IE_FEATURES: u32 = 2;
const ATH10K_FW_IE_FW_IMAGE: u32 = 3;
const ATH10K_FW_IE_WMI_OP_VERSION: u32 = 5;
const ATH10K_FW_IE_HTT_OP_VERSION: u32 = 6;
const ATH10K_FW_FEATURE_COUNT: usize = 23;
const ATH10K_FW_FEATURE_BYTES: usize = ATH10K_FW_FEATURE_COUNT.div_ceil(8);
const ATH10K_FW_FEATURE_PEER_FLOW_CONTROL: usize = 13;
const ATH10K_FW_FEATURE_NON_BMI: usize = 19;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ath10kFirmwareError {
    MagicTooShort,
    InvalidMagic,
    IeHeaderTruncated,
    IeLengthOverflow,
    IePayloadTruncated,
    IePaddingTruncated,
}

/// Metadata parsed from an ath10k firmware-API container.
///
/// Feature bits use the same numbering as the pinned ath10k `core.h`. Unknown
/// IEs and the source parser's final zero-to-eight-byte tail are ignored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ath10kFirmwareMetadata {
    features: [u8; ATH10K_FW_FEATURE_BYTES],
    wmi_op_version: Option<u32>,
    htt_op_version: Option<u32>,
    embedded_image_len: Option<u32>,
    ignored_tail_bytes: u8,
}

impl Ath10kFirmwareMetadata {
    /// Parse the source format: NUL-terminated `QCA-ATH10K`, four-byte magic
    /// alignment, then little-endian `{id, len, payload, zero-to-three pad}`
    /// information elements. The final `<= sizeof(struct ath10k_fw_ie)` bytes
    /// are left uninterpreted to match ath10k's `while (len > sizeof(*hdr))`.
    pub fn parse(bytes: &[u8]) -> Result<Self, Ath10kFirmwareError> {
        if bytes.len() < ATH10K_FIRMWARE_MAGIC_PADDED {
            return Err(Ath10kFirmwareError::MagicTooShort);
        }
        if bytes.get(..ATH10K_FIRMWARE_MAGIC.len()) != Some(ATH10K_FIRMWARE_MAGIC.as_slice()) {
            return Err(Ath10kFirmwareError::InvalidMagic);
        }

        let mut metadata = Self {
            features: [0; ATH10K_FW_FEATURE_BYTES],
            wmi_op_version: None,
            htt_op_version: None,
            embedded_image_len: None,
            ignored_tail_bytes: 0,
        };
        let mut offset = ATH10K_FIRMWARE_MAGIC_PADDED;

        loop {
            let remaining = bytes.len().saturating_sub(offset);
            if remaining <= ATH10K_FW_IE_HEADER_BYTES {
                metadata.ignored_tail_bytes = remaining as u8;
                break;
            }

            let id_end = offset
                .checked_add(4)
                .ok_or(Ath10kFirmwareError::IeLengthOverflow)?;
            let len_end = id_end
                .checked_add(4)
                .ok_or(Ath10kFirmwareError::IeLengthOverflow)?;
            let header = bytes
                .get(offset..len_end)
                .ok_or(Ath10kFirmwareError::IeHeaderTruncated)?;
            let id = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let payload_len =
                u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
            let payload_end = len_end
                .checked_add(payload_len)
                .ok_or(Ath10kFirmwareError::IeLengthOverflow)?;
            let payload = bytes
                .get(len_end..payload_end)
                .ok_or(Ath10kFirmwareError::IePayloadTruncated)?;
            let padded_payload_len = payload_len
                .checked_add(3)
                .ok_or(Ath10kFirmwareError::IeLengthOverflow)?
                & !3;
            let next_offset = len_end
                .checked_add(padded_payload_len)
                .ok_or(Ath10kFirmwareError::IeLengthOverflow)?;
            if next_offset > bytes.len() {
                return Err(Ath10kFirmwareError::IePaddingTruncated);
            }

            match id {
                ATH10K_FW_IE_FEATURES => {
                    for (index, feature_byte) in metadata.features.iter_mut().enumerate() {
                        if index >= payload.len() {
                            break;
                        }
                        *feature_byte |= payload[index];
                    }
                    let known_last_bits = (1u8 << (ATH10K_FW_FEATURE_COUNT % 8)) - 1;
                    metadata.features[ATH10K_FW_FEATURE_BYTES - 1] &= known_last_bits;
                }
                ATH10K_FW_IE_WMI_OP_VERSION if payload.len() == 4 => {
                    metadata.wmi_op_version = Some(u32::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                    ]));
                }
                ATH10K_FW_IE_HTT_OP_VERSION if payload.len() == 4 => {
                    metadata.htt_op_version = Some(u32::from_le_bytes([
                        payload[0], payload[1], payload[2], payload[3],
                    ]));
                }
                ATH10K_FW_IE_FW_IMAGE => {
                    metadata.embedded_image_len = Some(payload.len() as u32);
                }
                _ => {}
            }
            offset = next_offset;
        }

        Ok(metadata)
    }

    pub const fn wmi_op_version(&self) -> Option<u32> {
        self.wmi_op_version
    }

    pub const fn htt_op_version(&self) -> Option<u32> {
        self.htt_op_version
    }

    pub const fn embedded_image_len(&self) -> Option<u32> {
        self.embedded_image_len
    }

    pub const fn ignored_tail_bytes(&self) -> u8 {
        self.ignored_tail_bytes
    }

    pub const fn has_peer_flow_control(&self) -> bool {
        self.has_feature(ATH10K_FW_FEATURE_PEER_FLOW_CONTROL)
    }

    pub const fn requires_external_firmware(&self) -> bool {
        self.has_feature(ATH10K_FW_FEATURE_NON_BMI)
    }

    pub const fn has_feature(&self, index: usize) -> bool {
        if index >= ATH10K_FW_FEATURE_COUNT {
            return false;
        }
        let byte = index / 8;
        let bit = index % 8;
        self.features[byte] & (1 << bit) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn append_ie(out: &mut Vec<u8>, id: u32, payload: &[u8]) {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    fn fixture() -> Vec<u8> {
        let mut bytes = Vec::from(*ATH10K_FIRMWARE_MAGIC);
        bytes.push(0);
        bytes
    }

    #[test]
    fn parses_padded_metadata_and_skips_embedded_firmware_bytes() {
        let mut bytes = fixture();
        let mut features = [0u8; ATH10K_FW_FEATURE_BYTES];
        features[ATH10K_FW_FEATURE_PEER_FLOW_CONTROL / 8] |=
            1 << (ATH10K_FW_FEATURE_PEER_FLOW_CONTROL % 8);
        features[ATH10K_FW_FEATURE_NON_BMI / 8] |= 1 << (ATH10K_FW_FEATURE_NON_BMI % 8);
        append_ie(&mut bytes, ATH10K_FW_IE_FEATURES, &features);
        append_ie(&mut bytes, ATH10K_FW_IE_WMI_OP_VERSION, &6u32.to_le_bytes());
        append_ie(&mut bytes, ATH10K_FW_IE_HTT_OP_VERSION, &4u32.to_le_bytes());
        append_ie(&mut bytes, ATH10K_FW_IE_FW_IMAGE, &[0x55; 5]);
        append_ie(&mut bytes, 0xfeed, &[1, 2, 3]);
        bytes.extend_from_slice(&[0xaa, 0xbb]);

        let metadata = Ath10kFirmwareMetadata::parse(&bytes).unwrap();
        assert!(metadata.has_peer_flow_control());
        assert!(metadata.requires_external_firmware());
        assert_eq!(metadata.wmi_op_version(), Some(6));
        assert_eq!(metadata.htt_op_version(), Some(4));
        assert_eq!(metadata.embedded_image_len(), Some(5));
        assert_eq!(metadata.ignored_tail_bytes(), 2);
        assert!(!metadata.has_feature(23));
    }

    #[test]
    fn feature_records_merge_and_valid_version_records_follow_source_order() {
        let mut bytes = fixture();
        append_ie(&mut bytes, ATH10K_FW_IE_FEATURES, &[0x02]);
        append_ie(&mut bytes, ATH10K_FW_IE_FEATURES, &[0x08, 0x20, 0x08]);
        append_ie(&mut bytes, ATH10K_FW_IE_WMI_OP_VERSION, &[1, 2]);
        append_ie(&mut bytes, ATH10K_FW_IE_WMI_OP_VERSION, &6u32.to_le_bytes());
        append_ie(&mut bytes, ATH10K_FW_IE_WMI_OP_VERSION, &3u32.to_le_bytes());

        let metadata = Ath10kFirmwareMetadata::parse(&bytes).unwrap();
        assert!(metadata.has_feature(1));
        assert!(metadata.has_feature(19));
        assert_eq!(metadata.wmi_op_version(), Some(3));
        assert_eq!(metadata.htt_op_version(), None);
    }

    #[test]
    fn rejects_bad_magic_truncated_payload_and_missing_ie_padding() {
        assert_eq!(
            Ath10kFirmwareMetadata::parse(b"QCA"),
            Err(Ath10kFirmwareError::MagicTooShort)
        );
        let mut bad_magic = fixture();
        bad_magic[0] = b'X';
        assert_eq!(
            Ath10kFirmwareMetadata::parse(&bad_magic),
            Err(Ath10kFirmwareError::InvalidMagic)
        );

        let mut short_payload = fixture();
        short_payload.extend_from_slice(&ATH10K_FW_IE_FEATURES.to_le_bytes());
        short_payload.extend_from_slice(&3u32.to_le_bytes());
        short_payload.extend_from_slice(&[1, 2]);
        assert_eq!(
            Ath10kFirmwareMetadata::parse(&short_payload),
            Err(Ath10kFirmwareError::IePayloadTruncated)
        );

        let mut short_padding = fixture();
        short_padding.extend_from_slice(&0xfeedu32.to_le_bytes());
        short_padding.extend_from_slice(&1u32.to_le_bytes());
        short_padding.push(0xaa);
        assert_eq!(
            Ath10kFirmwareMetadata::parse(&short_padding),
            Err(Ath10kFirmwareError::IePaddingTruncated)
        );
    }

    #[test]
    fn rejects_partial_ie_header_and_handles_empty_metadata() {
        let empty = fixture();
        let metadata = Ath10kFirmwareMetadata::parse(&empty).unwrap();
        assert_eq!(metadata.wmi_op_version(), None);
        assert!(!metadata.has_peer_flow_control());

        let mut partial = fixture();
        partial.extend_from_slice(&[1; 9]);
        assert_eq!(
            Ath10kFirmwareMetadata::parse(&partial),
            Err(Ath10kFirmwareError::IePayloadTruncated)
        );
    }
}
