//! Allocation-free QMI service envelopes and bounded TLV decoding.
//!
//! Uses the seven-byte QMI header (not QMUX). Unknown optional TLVs may be
//! ignored by a service, but duplicate TLVs and malformed tails are rejected.
//! No transport, storage, memory access, or response side effects occur here.

#![forbid(unsafe_code)]

pub const HEADER_BYTES: usize = 7;
pub const MAX_MESSAGE_BYTES: usize = 4096;
pub const MAX_TLVS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    TooLarge,
    ShortHeader,
    LengthMismatch,
    UnsupportedKind,
    TruncatedTlv,
    TooManyTlvs,
    DuplicateTlv(u8),
    MissingTlv(u8),
    InvalidTlvLength(u8),
    UnknownMandatoryTlv(u8),
    UnsupportedMessage,
    InvalidValue,
    RangeOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Request,
    Response,
    Indication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Tlv<'a> {
    kind: u8,
    value: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message<'a> {
    pub kind: Kind,
    pub transaction: u16,
    pub message_id: u16,
    tlvs: [Option<Tlv<'a>>; MAX_TLVS],
    count: usize,
    bytes: &'a [u8],
}

impl<'a> Message<'a> {
    /// Decode one complete, unpadded QMI message. No borrowed slice exceeds
    /// the input lifetime, and no allocation depends on a wire value.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(Error::TooLarge);
        }
        if bytes.len() < HEADER_BYTES {
            return Err(Error::ShortHeader);
        }
        let kind = match bytes[0] {
            0 => Kind::Request,
            2 => Kind::Response,
            4 => Kind::Indication,
            _ => return Err(Error::UnsupportedKind),
        };
        let payload_bytes = u16::from_le_bytes([bytes[5], bytes[6]]) as usize;
        if payload_bytes != bytes.len() - HEADER_BYTES {
            return Err(Error::LengthMismatch);
        }
        let mut message = Self {
            kind,
            transaction: u16::from_le_bytes([bytes[1], bytes[2]]),
            message_id: u16::from_le_bytes([bytes[3], bytes[4]]),
            tlvs: [None; MAX_TLVS],
            count: 0,
            bytes,
        };
        let mut remaining = &bytes[HEADER_BYTES..];
        while !remaining.is_empty() {
            if remaining.len() < 3 {
                return Err(Error::TruncatedTlv);
            }
            if message.count == MAX_TLVS {
                return Err(Error::TooManyTlvs);
            }
            let kind = remaining[0];
            let size = u16::from_le_bytes([remaining[1], remaining[2]]) as usize;
            if size > remaining.len() - 3 {
                return Err(Error::TruncatedTlv);
            }
            if message.get(kind).is_some() {
                return Err(Error::DuplicateTlv(kind));
            }
            message.tlvs[message.count] = Some(Tlv {
                kind,
                value: &remaining[3..3 + size],
            });
            message.count += 1;
            remaining = &remaining[3 + size..];
        }
        Ok(message)
    }

    pub fn tlv_count(&self) -> usize {
        self.count
    }

    /// Original complete QMI message, retained as a borrowed view for service
    /// handlers that need to validate a schema in a separate protocol module.
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn get(&self, kind: u8) -> Option<&'a [u8]> {
        self.tlvs[..self.count]
            .iter()
            .flatten()
            .find(|tlv| tlv.kind == kind)
            .map(|tlv| tlv.value)
    }

    pub fn required(&self, kind: u8) -> Result<&'a [u8], Error> {
        self.get(kind).ok_or(Error::MissingTlv(kind))
    }

    /// QMI fields below 0x10 are mandatory. Unsupported optional fields are
    /// permitted within the same message/TLV ceilings for forward compatibility.
    pub fn check_fields(&self, known: &[u8]) -> Result<(), Error> {
        for tlv in self.tlvs[..self.count].iter().flatten() {
            if tlv.kind < 0x10 && !known.contains(&tlv.kind) {
                return Err(Error::UnknownMandatoryTlv(tlv.kind));
            }
        }
        Ok(())
    }

    pub fn u8(&self, kind: u8) -> Result<u8, Error> {
        match self.required(kind)? {
            [value] => Ok(*value),
            _ => Err(Error::InvalidTlvLength(kind)),
        }
    }

    pub fn u32(&self, kind: u8) -> Result<u32, Error> {
        match self.required(kind)? {
            [a, b, c, d] => Ok(u32::from_le_bytes([*a, *b, *c, *d])),
            _ => Err(Error::InvalidTlvLength(kind)),
        }
    }

    pub fn optional_u8(&self, kind: u8) -> Result<Option<u8>, Error> {
        self.get(kind).map(|_| self.u8(kind)).transpose()
    }

    pub fn optional_u32(&self, kind: u8) -> Result<Option<u32>, Error> {
        self.get(kind).map(|_| self.u32(kind)).transpose()
    }
}
