//! Bounded shared-page ABI between the Messages service and the guest SimpleX
//! Agent protection domain. The page is exchanged only while holding the
//! corresponding protected-call endpoint; it is not a lock-free queue.

pub const PAGE_SIZE: usize = 4096;
pub const MAGIC: [u8; 4] = *b"NXSM";
pub const VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 20;
pub const PAYLOAD_CAPACITY: usize = PAGE_SIZE - HEADER_SIZE;

/// A cursor over an operation payload. Variable byte strings are encoded as
/// a little-endian u16 length followed by exactly that many bytes.
pub struct PayloadReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> PayloadReader<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn u8(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.offset)?;
        self.offset += 1;
        Some(value)
    }

    pub fn u16(&mut self) -> Option<u16> {
        let end = self.offset.checked_add(2)?;
        let bytes = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn u64(&mut self) -> Option<u64> {
        let end = self.offset.checked_add(8)?;
        let bytes = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let length = self.u16()? as usize;
        let end = self.offset.checked_add(length)?;
        let value = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(value)
    }

    /// Require that a decoded payload contains no unparsed trailing fields.
    pub const fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

/// A cursor for constructing bounded operation payloads in a shared page.
pub struct PayloadWriter<'a> {
    bytes: &'a mut [u8],
    offset: usize,
}

impl<'a> PayloadWriter<'a> {
    pub const fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub const fn len(&self) -> usize {
        self.offset
    }

    pub fn u8(&mut self, value: u8) -> bool {
        let Some(slot) = self.bytes.get_mut(self.offset) else {
            return false;
        };
        *slot = value;
        self.offset += 1;
        true
    }

    pub fn u16(&mut self, value: u16) -> bool {
        self.fixed(&value.to_le_bytes())
    }

    pub fn u64(&mut self, value: u64) -> bool {
        self.fixed(&value.to_le_bytes())
    }

    pub fn bytes(&mut self, value: &[u8]) -> bool {
        let Ok(length) = u16::try_from(value.len()) else {
            return false;
        };
        let Some(end) = self
            .offset
            .checked_add(2)
            .and_then(|n| n.checked_add(value.len()))
        else {
            return false;
        };
        if end > self.bytes.len() {
            return false;
        }
        self.bytes[self.offset..self.offset + 2].copy_from_slice(&length.to_le_bytes());
        self.offset += 2;
        self.bytes[self.offset..end].copy_from_slice(value);
        self.offset = end;
        true
    }

    fn fixed(&mut self, value: &[u8]) -> bool {
        let Some(end) = self.offset.checked_add(value.len()) else {
            return false;
        };
        let Some(out) = self.bytes.get_mut(self.offset..end) else {
            return false;
        };
        out.copy_from_slice(value);
        self.offset = end;
        true
    }
}

#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    ListConnections = 1,
    CreateInvitation = 2,
    JoinInvitation = 3,
    SendMessage = 4,
    ReceiveNext = 5,
    Acknowledge = 6,
    DeleteConnection = 7,
}

impl TryFrom<u16> for Operation {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ListConnections),
            2 => Ok(Self::CreateInvitation),
            3 => Ok(Self::JoinInvitation),
            4 => Ok(Self::SendMessage),
            5 => Ok(Self::ReceiveNext),
            6 => Ok(Self::Acknowledge),
            7 => Ok(Self::DeleteConnection),
            _ => Err(()),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Request = 0,
    Ok = 1,
    NoEvent = 2,
    Invalid = 3,
    Unavailable = 4,
    Failed = 5,
}

#[repr(C)]
pub struct AgentPage {
    magic: [u8; 4],
    version: u16,
    operation: u16,
    request_id: u32,
    status: u32,
    payload_len: u32,
    payload: [u8; PAYLOAD_CAPACITY],
}

impl AgentPage {
    pub const fn new() -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            operation: 0,
            request_id: 0,
            status: Status::Failed as u32,
            payload_len: 0,
            payload: [0; PAYLOAD_CAPACITY],
        }
    }

    /// Begin one bounded request. Call only after acquiring the Agent endpoint.
    pub fn begin(&mut self, operation: Operation, request_id: u32, payload: &[u8]) -> bool {
        if request_id == 0 || payload.len() > PAYLOAD_CAPACITY {
            return false;
        }
        self.magic = MAGIC;
        self.version = VERSION;
        self.operation = operation as u16;
        self.request_id = request_id;
        self.status = Status::Request as u32;
        self.payload_len = payload.len() as u32;
        self.payload.fill(0);
        self.payload[..payload.len()].copy_from_slice(payload);
        true
    }

    /// Validate and view a request written by Messages. The caller must copy
    /// anything it needs before writing a response to this same page.
    pub fn request(&self) -> Option<(Operation, u32, &[u8])> {
        if self.magic != MAGIC || self.version != VERSION || self.status != Status::Request as u32 {
            return None;
        }
        let operation = Operation::try_from(self.operation).ok()?;
        let request_id = self.request_id;
        let length = self.payload_len as usize;
        if request_id == 0 || length > PAYLOAD_CAPACITY {
            return None;
        }
        Some((operation, request_id, &self.payload[..length]))
    }

    /// Publish a response for the active request. A matching nonzero request
    /// ID prevents a stale response from satisfying a later call.
    pub fn respond(&mut self, request_id: u32, status: Status, payload: &[u8]) -> bool {
        if request_id == 0
            || request_id != self.request_id
            || status == Status::Request
            || payload.len() > PAYLOAD_CAPACITY
        {
            return false;
        }
        self.payload.fill(0);
        self.payload[..payload.len()].copy_from_slice(payload);
        self.payload_len = payload.len() as u32;
        self.status = status as u32;
        true
    }

    pub fn response(&self, expected_request_id: u32) -> Option<(Status, &[u8])> {
        if self.magic != MAGIC
            || self.version != VERSION
            || expected_request_id == 0
            || self.request_id != expected_request_id
        {
            return None;
        }
        let status = match self.status {
            1 => Status::Ok,
            2 => Status::NoEvent,
            3 => Status::Invalid,
            4 => Status::Unavailable,
            5 => Status::Failed,
            _ => return None,
        };
        let length = self.payload_len as usize;
        if length > PAYLOAD_CAPACITY {
            return None;
        }
        Some((status, &self.payload[..length]))
    }
}

const _: () = assert!(core::mem::size_of::<AgentPage>() == PAGE_SIZE);
const _: () = assert!(core::mem::offset_of!(AgentPage, payload) == HEADER_SIZE);

#[cfg(test)]
mod tests {
    use super::{AgentPage, Operation, PAYLOAD_CAPACITY, Status};

    #[test]
    fn request_and_response_round_trip_is_bounded_and_correlated() {
        let mut page = AgentPage::new();
        assert!(page.begin(Operation::SendMessage, 41, b"conn-1\0hello"));
        let (operation, request_id, payload) = page.request().unwrap();
        assert_eq!(operation, Operation::SendMessage);
        assert_eq!(request_id, 41);
        assert_eq!(payload, b"conn-1\0hello");

        assert!(!page.respond(40, Status::Ok, b"stale"));
        assert!(page.respond(41, Status::Ok, b"sent"));
        assert!(page.response(40).is_none());
        assert_eq!(page.response(41).unwrap(), (Status::Ok, &b"sent"[..]));
    }

    #[test]
    fn rejects_invalid_ids_and_oversized_payloads() {
        let mut page = AgentPage::new();
        assert!(!page.begin(Operation::ReceiveNext, 0, b""));
        assert!(!page.begin(Operation::ReceiveNext, 1, &[0; PAYLOAD_CAPACITY + 1]));
        assert!(!page.respond(0, Status::Ok, b""));
        assert!(!page.respond(1, Status::Request, b""));
        assert!(!page.respond(1, Status::Ok, &[0; PAYLOAD_CAPACITY + 1]));
    }

    #[test]
    fn correlates_empty_receive_poll_as_no_event() {
        let mut page = AgentPage::new();
        assert!(page.begin(Operation::ReceiveNext, 73, b""));
        assert!(page.respond(73, Status::NoEvent, b""));
        assert_eq!(page.response(73).unwrap(), (Status::NoEvent, &b""[..]));
    }

    #[test]
    fn rejects_unknown_operations_and_corrupt_headers() {
        let mut page = AgentPage::new();
        assert!(page.begin(Operation::ListConnections, 2, b""));
        page.operation = 99;
        assert!(page.request().is_none());
        page.operation = Operation::ListConnections as u16;
        page.magic[0] ^= 1;
        assert!(page.request().is_none());
    }

    #[test]
    fn payload_codec_round_trips_and_rejects_truncation_or_trailing_data() {
        use super::{PayloadReader, PayloadWriter};

        let mut bytes = [0u8; 64];
        let mut writer = PayloadWriter::new(&mut bytes);
        assert!(writer.u8(2));
        assert!(writer.u64(0x0123_4567_89ab_cdef));
        assert!(writer.bytes(b"hello"));
        let length = writer.len();
        assert_eq!(
            &bytes[..length],
            &[
                2, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01, 5, 0, b'h', b'e', b'l', b'l',
                b'o'
            ]
        );

        let mut reader = PayloadReader::new(&bytes[..length]);
        assert_eq!(reader.u8(), Some(2));
        assert_eq!(reader.u64(), Some(0x0123_4567_89ab_cdef));
        assert_eq!(reader.bytes(), Some(&b"hello"[..]));
        assert!(reader.is_empty());

        let mut truncated = PayloadReader::new(&bytes[..length - 1]);
        assert_eq!(truncated.u8(), Some(2));
        assert_eq!(truncated.u64(), Some(0x0123_4567_89ab_cdef));
        assert_eq!(truncated.bytes(), None);

        let trailing = [0u8; 1];
        assert!(!PayloadReader::new(&trailing).is_empty());
    }

    #[test]
    fn payload_writer_stops_at_its_capacity() {
        use super::PayloadWriter;

        let mut bytes = [0u8; 3];
        let mut writer = PayloadWriter::new(&mut bytes);
        assert!(writer.u8(1));
        assert!(!writer.u64(2));
        assert!(!writer.bytes(b"x"));
    }
}
