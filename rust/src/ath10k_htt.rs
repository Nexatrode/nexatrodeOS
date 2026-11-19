//! Host-testable HTT version, security and peer-event wire primitives.
//!
//! These wire helpers do not touch CE rings, DMA, MMIO, or firmware. They
//! decode bounded messages after the HTC data service is connected.

const HTT_H2T_VERSION_REQUEST: u8 = 0;
const HTT_T2H_VERSION_CONFIRMATION: u8 = 0;
const HTT_VERSION_REQUEST_SIZE: usize = 4;
const HTT_VERSION_CONFIRMATION_SIZE: usize = 4;
pub const HTT_10_4_T2H_SECURITY_INDICATION: u8 = 0x0b;
pub const HTT_10_4_T2H_PEER_MAP: u8 = 0x03;
pub const HTT_10_4_T2H_PEER_UNMAP: u8 = 0x04;
pub const HTT_SECURITY_INDICATION_BYTES: usize = 28;
pub const HTT_SECURITY_TYPE_AES_CCMP: u8 = 6;
const HTT_SECURITY_TYPE_MASK: u8 = 0x7f;
const HTT_SECURITY_IS_UNICAST: u8 = 0x80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttError {
    EmptyMessage,
    TruncatedVersionConfirmation,
    UnexpectedMessageType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttSecurityIndicationError {
    EmptyMessage,
    WrongMessageType(u8),
    Truncated { expected: usize, observed: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttPeerEventError {
    Empty,
    WrongType(u8),
    Truncated { expected: usize, observed: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttPeerMap {
    pub vdev_id: u8,
    pub peer_id: u16,
    pub mac: [u8; 6],
}
impl HttPeerMap {
    pub fn parse(message: &[u8]) -> Result<Self, HttPeerEventError> {
        let kind = *message.first().ok_or(HttPeerEventError::Empty)?;
        if kind != HTT_10_4_T2H_PEER_MAP {
            return Err(HttPeerEventError::WrongType(kind));
        }
        if message.len() < 12 {
            return Err(HttPeerEventError::Truncated {
                expected: 12,
                observed: message.len(),
            });
        }
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&message[4..10]);
        Ok(Self {
            vdev_id: message[1],
            peer_id: u16::from_le_bytes([message[2], message[3]]),
            mac,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttPeerUnmap {
    pub peer_id: u16,
}
impl HttPeerUnmap {
    pub fn parse(message: &[u8]) -> Result<Self, HttPeerEventError> {
        let kind = *message.first().ok_or(HttPeerEventError::Empty)?;
        if kind != HTT_10_4_T2H_PEER_UNMAP {
            return Err(HttPeerEventError::WrongType(kind));
        }
        if message.len() < 4 {
            return Err(HttPeerEventError::Truncated {
                expected: 4,
                observed: message.len(),
            });
        }
        Ok(Self {
            peer_id: u16::from_le_bytes([message[2], message[3]]),
        })
    }
}

/// Bounded, non-secret fields from one target-to-host security indication.
/// Michael-key and WAPI-RSC bytes are deliberately not exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttSecurityIndication {
    pub peer_id: u16,
    pub security_type: u8,
    pub is_unicast: bool,
}

impl HttSecurityIndication {
    /// Decode the one-byte HTT response header and packed security indication.
    /// Extra bytes are ignored; the Michael key and WAPI RSC remain borrowed
    /// input only and are never copied into the result.
    pub fn parse(message: &[u8]) -> Result<Self, HttSecurityIndicationError> {
        let Some(&message_type) = message.first() else {
            return Err(HttSecurityIndicationError::EmptyMessage);
        };
        if message_type != HTT_10_4_T2H_SECURITY_INDICATION {
            return Err(HttSecurityIndicationError::WrongMessageType(message_type));
        }
        if message.len() < HTT_SECURITY_INDICATION_BYTES {
            return Err(HttSecurityIndicationError::Truncated {
                expected: HTT_SECURITY_INDICATION_BYTES,
                observed: message.len(),
            });
        }

        let flags = message[1];
        Ok(Self {
            peer_id: u16::from_le_bytes([message[2], message[3]]),
            security_type: flags & HTT_SECURITY_TYPE_MASK,
            is_unicast: flags & HTT_SECURITY_IS_UNICAST != 0,
        })
    }
}

/// The host's fixed four-byte HTT version request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttVersionRequest;

impl HttVersionRequest {
    pub const fn encode(self) -> [u8; HTT_VERSION_REQUEST_SIZE] {
        [HTT_H2T_VERSION_REQUEST, 0, 0, 0]
    }
}

/// Target's version response: HTT response type followed by minor, major,
/// and the reserved byte defined by `htt_ver_resp`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttVersionConfirmation {
    pub minor: u8,
    pub major: u8,
    pub reserved: u8,
}

impl HttVersionConfirmation {
    pub fn parse(message: &[u8]) -> Result<Self, HttError> {
        if message.is_empty() {
            return Err(HttError::EmptyMessage);
        }
        if message[0] != HTT_T2H_VERSION_CONFIRMATION {
            return Err(HttError::UnexpectedMessageType);
        }
        if message.len() < HTT_VERSION_CONFIRMATION_SIZE {
            return Err(HttError::TruncatedVersionConfirmation);
        }
        Ok(Self {
            minor: message[1],
            major: message[2],
            reserved: message[3],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_request_matches_four_byte_htc_payload() {
        assert_eq!(HttVersionRequest.encode(), [0, 0, 0, 0]);
    }

    #[test]
    fn version_confirmation_parses_minor_then_major() {
        assert_eq!(
            HttVersionConfirmation::parse(&[0, 2, 3, 0]),
            Ok(HttVersionConfirmation {
                minor: 2,
                major: 3,
                reserved: 0,
            })
        );
    }

    #[test]
    fn version_confirmation_rejects_empty_wrong_type_and_short_messages() {
        assert_eq!(
            HttVersionConfirmation::parse(&[]),
            Err(HttError::EmptyMessage)
        );
        assert_eq!(
            HttVersionConfirmation::parse(&[1, 2, 3, 0]),
            Err(HttError::UnexpectedMessageType)
        );
        assert_eq!(
            HttVersionConfirmation::parse(&[0, 2, 3]),
            Err(HttError::TruncatedVersionConfirmation)
        );
    }

    #[test]
    fn security_indication_parses_only_bounded_peer_cipher_and_cast_fields() {
        let mut message = [0xa5; HTT_SECURITY_INDICATION_BYTES + 3];
        message[0] = HTT_10_4_T2H_SECURITY_INDICATION;
        message[1] = HTT_SECURITY_TYPE_AES_CCMP | HTT_SECURITY_IS_UNICAST;
        message[2..4].copy_from_slice(&0x1234u16.to_le_bytes());
        assert_eq!(
            HttSecurityIndication::parse(&message),
            Ok(HttSecurityIndication {
                peer_id: 0x1234,
                security_type: HTT_SECURITY_TYPE_AES_CCMP,
                is_unicast: true,
            })
        );
    }

    #[test]
    fn security_indication_rejects_empty_wrong_type_and_truncated_messages() {
        assert_eq!(
            HttSecurityIndication::parse(&[]),
            Err(HttSecurityIndicationError::EmptyMessage)
        );
        assert_eq!(
            HttSecurityIndication::parse(&[0x0a; HTT_SECURITY_INDICATION_BYTES]),
            Err(HttSecurityIndicationError::WrongMessageType(0x0a))
        );
        assert_eq!(
            HttSecurityIndication::parse(&[HTT_10_4_T2H_SECURITY_INDICATION; 27]),
            Err(HttSecurityIndicationError::Truncated {
                expected: HTT_SECURITY_INDICATION_BYTES,
                observed: 27,
            })
        );
    }
}
