//! Bounded shared-page ABI between the Messages service and the guest SimpleX
//! Agent protection domain. The page is exchanged only while holding the
//! corresponding protected-call endpoint; it is not a lock-free queue.

pub const PAGE_SIZE: usize = 4096;
pub const MAGIC: [u8; 4] = *b"NXSM";
// Version 2 widens attachment offsets and supports extended image lengths.
pub const VERSION: u16 = 2;
pub const HEADER_SIZE: usize = 20;
pub const PAYLOAD_CAPACITY: usize = PAGE_SIZE - HEADER_SIZE;

pub fn chat_at_displayed_row(displayed: &[u64], live: &[u64], index: usize) -> Option<u64> {
    let handle = *displayed.get(index)?;
    (handle != 0 && live.get(index) == Some(&handle)).then_some(handle)
}

/// Resolve a stable owner at dispatch time and reject ambiguous identities.
pub fn live_chat_index(live: &[u64], owner: u64) -> Option<usize> {
    if owner == 0 || owner == u64::MAX { return None; }
    let mut matches = live.iter().enumerate().filter(|(_, handle)| **handle == owner);
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

/// Resolve only the explicitly selected live direct contact.
pub fn group_invitee(handles: &[u64], visible_chats: usize, group_mask: u16, index: usize) -> Option<u64> {
    if index >= visible_chats || index >= 16 || group_mask & (1u16 << index) != 0 { return None; }
    handles.get(index).copied().filter(|handle| *handle != 0)
}

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

    pub fn u32(&mut self) -> Option<u32> {
        let end = self.offset.checked_add(4)?;
        let bytes = self.bytes.get(self.offset..end)?;
        self.offset = end;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
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

    pub const fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }
}

/// CreateInvitation shares its page with the inbox. Queue the user's request
/// until the current owner completes, and consume each terminal reply once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationRequest {
    Idle,
    Queued,
    InFlight,
}

impl InvitationRequest {
    pub fn queue(&mut self, cached: bool) {
        if *self == Self::Idle && !cached {
            *self = Self::Queued;
        }
    }

    pub fn begin(&mut self, page: &mut AgentPage) -> bool {
        if *self != Self::Queued || page.request().is_some() || !page.begin(Operation::CreateInvitation, 2, &[]) {
            return false;
        }
        *self = Self::InFlight;
        true
    }

    pub fn take_response<'a>(&mut self, page: &'a AgentPage) -> Option<(Status, &'a [u8])> {
        if *self != Self::InFlight {
            return None;
        }
        let reply = page.response(2)?;
        *self = Self::Idle;
        Some(reply)
    }
}

/// Keep the destination with the body until the send reply is recorded.
pub struct OutgoingMessage {
    pub handle: u64,
    body: [u8; 2048],
    length: usize,
}

impl OutgoingMessage {
    pub fn text(&self) -> &[u8] {
        &self.body[..self.length]
    }
}

pub struct MessageOutbox {
    pending: Option<OutgoingMessage>,
    pub in_flight: Option<OutgoingMessage>,
}

impl MessageOutbox {
    pub const fn new() -> Self {
        Self { pending: None, in_flight: None }
    }

    pub fn queue(&mut self, handle: u64, live: &[u64], text: &[u8]) -> bool {
        if self.pending.is_some() || handle == 0 || !live.contains(&handle)
            || text.is_empty() || text.len() > 2048 || core::str::from_utf8(text).is_err()
        {
            return false;
        }
        let mut message = OutgoingMessage { handle, body: [0; 2048], length: text.len() };
        message.body[..text.len()].copy_from_slice(text);
        self.pending = Some(message);
        true
    }

    /// Wait for the shared page. A deleted destination fails the accepted send.
    pub fn begin(&mut self, page: &mut AgentPage, live: &[u64]) -> Result<bool, ()> {
        if self.in_flight.is_some() || page.request().is_some() {
            return Ok(false);
        }
        let Some(message) = self.pending.take() else { return Ok(false); };
        if !live.contains(&message.handle) {
            return Err(());
        }
        let mut payload = [0; 8 + 2 + 2048];
        let mut writer = PayloadWriter::new(&mut payload);
        if !writer.u64(message.handle) || !writer.bytes(message.text()) {
            return Err(());
        }
        let length = writer.len();
        if !page.begin(Operation::SendMessage, 4, &payload[..length]) {
            return Err(());
        }
        self.in_flight = Some(message);
        Ok(true)
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
    ReadImage = 8,
    GetPeerImage = 9,
    SetProfile = 10,
    CreateGroup = 11,
    CreateAddress = 12,
    /// Append one ordered chunk to the volatile outgoing attachment buffer.
    StageAttachment = 13,
    /// Send a previously staged attachment over XFTP.
    SendAttachment = 14,
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
            8 => Ok(Self::ReadImage),
            9 => Ok(Self::GetPeerImage),
            10 => Ok(Self::SetProfile),
            11 => Ok(Self::CreateGroup),
            12 => Ok(Self::CreateAddress),
            13 => Ok(Self::StageAttachment),
            14 => Ok(Self::SendAttachment),
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
        if request_id == 0 || payload.len() > PAYLOAD_CAPACITY || self.request().is_some() {
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
    #[test]
    fn background_selection_requires_a_unique_live_owner() {
        assert_eq!(super::live_chat_index(&[22, 11], 11), Some(1));
        assert_eq!(super::live_chat_index(&[22, 11], 33), None);
        assert_eq!(super::live_chat_index(&[0, 11], 0), None);
        assert_eq!(super::live_chat_index(&[u64::MAX], u64::MAX), None);
        assert_eq!(super::live_chat_index(&[11, 22, 11], 11), None);
    }
    use super::{AgentPage, InvitationRequest, Operation, PAYLOAD_CAPACITY, Status};

    #[test]
    fn queued_send_keeps_its_recipient_and_body_when_the_chat_changes() {
        let mut page = AgentPage::new();
        let mut sends = super::MessageOutbox::new();
        assert!(page.begin(Operation::ReceiveNext, 5, &[]));
        assert!(sends.queue(11, &[11, 22], b"first draft"));
        assert_eq!(sends.begin(&mut page, &[11, 22]), Ok(false));
        assert!(!sends.queue(22, &[11, 22], b"another draft"));
        assert!(page.respond(5, Status::NoEvent, &[]));
        assert_eq!(sends.begin(&mut page, &[22, 11]), Ok(true));
        let (op, id, payload) = page.request().unwrap();
        assert_eq!((op, id), (Operation::SendMessage, 4));
        let mut body = super::PayloadReader::new(payload);
        assert_eq!(body.u64(), Some(11));
        assert_eq!(body.bytes(), Some(&b"first draft"[..]));
        assert!(body.is_empty());
        assert!(sends.queue(22, &[22, 11], b"retry"));
        assert!(page.respond(4, Status::Ok, &[]));
        assert_eq!(sends.begin(&mut page, &[22, 11]), Ok(false));
        let sent = sends.in_flight.take().unwrap();
        assert_eq!((sent.handle, sent.text()), (11, &b"first draft"[..]));
        assert_eq!(sends.begin(&mut page, &[22, 11]), Ok(true));
        let sent = sends.in_flight.take().unwrap();
        assert_eq!((sent.handle, sent.text()), (22, &b"retry"[..]));
    }

    #[test]
    fn send_requires_a_live_recipient_at_admission_and_at_dispatch() {
        let mut page = AgentPage::new();
        let mut sends = super::MessageOutbox::new();
        assert!(!sends.queue(0, &[0, 22], b"no selection"));
        assert!(!sends.queue(11, &[22], b"old selection"));
        assert!(sends.queue(11, &[11, 22], b"accepted"));
        assert_eq!(sends.begin(&mut page, &[22]), Err(()));
        assert!(sends.in_flight.is_none());
        assert!(page.request().is_none());
        assert!(sends.queue(22, &[22], b"next"));
        assert_eq!(sends.begin(&mut page, &[22]), Ok(true));
    }

    #[test]
    fn selecting_a_row_requires_the_contact_shown_in_the_last_snapshot() {
        let displayed = [11, 22, 0];
        assert_eq!(super::chat_at_displayed_row(&displayed, &[22, 11], 0), None);
        assert_eq!(super::chat_at_displayed_row(&displayed, &[11], 1), None);
        assert_eq!(super::chat_at_displayed_row(&displayed, &[11, 22, 0], 2), None);
        assert_eq!(super::chat_at_displayed_row(&displayed, &[11, 22], usize::MAX), None);
        assert_eq!(super::chat_at_displayed_row(&displayed, &[11, 22], 1), Some(22));
    }

    #[test]
    fn profile_update_cannot_replace_an_active_send_and_retries_after_reply() {
        let mut page = AgentPage::new();
        let mut sends = super::MessageOutbox::new();
        assert!(sends.queue(11, &[11], b"message"));
        assert_eq!(sends.begin(&mut page, &[11]), Ok(true));
        assert!(!page.begin(Operation::SetProfile, 10, b"New Name"));
        assert_eq!(page.request().unwrap().0, Operation::SendMessage);
        assert!(page.respond(4, Status::Ok, &[]));
        sends.in_flight.take();
        assert!(page.begin(Operation::SetProfile, 10, b"New Name"));
        assert!(sends.queue(11, &[11], b"next"));
        assert_eq!(sends.begin(&mut page, &[11]), Ok(false));
        assert!(page.respond(10, Status::Ok, &[]));
        assert_eq!(sends.begin(&mut page, &[11]), Ok(true));
    }

    #[test]
    fn invitation_waits_for_receive_and_releases_the_page_after_its_reply() {
        let mut page = AgentPage::new();
        let mut invitation = InvitationRequest::Idle;
        assert!(page.begin(Operation::ReceiveNext, 5, &[]));
        invitation.queue(false);
        assert!(!invitation.begin(&mut page));
        assert_eq!(page.request(), Some((Operation::ReceiveNext, 5, &[][..])));
        assert!(page.respond(5, Status::NoEvent, &[]));
        assert!(invitation.begin(&mut page));
        invitation.queue(false);
        assert!(!invitation.begin(&mut page));
        assert!(invitation.take_response(&page).is_none());
        assert!(page.respond(2, Status::Ok, b"result"));
        assert_eq!(invitation.take_response(&page), Some((Status::Ok, &b"result"[..])));
        assert_eq!(invitation, InvitationRequest::Idle);
        assert!(invitation.take_response(&page).is_none());
        invitation.queue(true);
        assert!(!invitation.begin(&mut page));
        assert!(page.begin(Operation::ListConnections, 1, &[]));
    }

    #[test]
    fn rejected_invitation_releases_the_page_and_can_be_retried() {
        let mut page = AgentPage::new();
        let mut invitation = InvitationRequest::Idle;
        for status in [Status::Invalid, Status::Failed, Status::Unavailable] {
            invitation.queue(false);
            assert!(invitation.begin(&mut page));
            assert!(page.respond(2, status, &[]));
            assert_eq!(invitation.take_response(&page), Some((status, &[][..])));
            assert!(invitation.take_response(&page).is_none());
            assert_eq!(invitation, InvitationRequest::Idle);
        }
    }

    #[test]
    fn group_invite_requires_the_selected_live_direct_contact() {
        let handles = [11, 22, 0];
        assert_eq!(super::group_invitee(&handles, 3, 0b010, 0), Some(11));
        assert_eq!(super::group_invitee(&handles, 3, 0b010, 1), None);
        assert_eq!(super::group_invitee(&handles, 3, 0, 2), None);
        assert_eq!(super::group_invitee(&handles, 1, 0, 1), None);
        assert_eq!(super::group_invitee(&handles, 3, 0, usize::MAX), None);
    }

    #[test]
    fn attachment_upload_operations_fit_and_round_trip_on_the_shared_page() {
        let mut page = AgentPage::new();
        let mut chunk = [0u8; 3074];
        chunk[..2].copy_from_slice(&12_288u16.to_le_bytes());
        chunk[2..].fill(0xa5);
        assert!(page.begin(Operation::StageAttachment, 13, &chunk));
        let (op, id, payload) = page.request().unwrap();
        assert_eq!((op, id), (Operation::StageAttachment, 13));
        assert_eq!(payload, &chunk);
        assert!(page.respond(13, Status::Ok, &[]));
        let mut send = [0u8; 128];
        let mut writer = super::PayloadWriter::new(&mut send);
        assert!(writer.u64(42));
        assert!(writer.bytes(b"photo.jpg"));
        let len = writer.len();
        assert!(page.begin(Operation::SendAttachment, 14, &send[..len]));
        let (op, id, payload) = page.request().unwrap();
        assert_eq!((op, id), (Operation::SendAttachment, 14));
        let mut reader = super::PayloadReader::new(payload);
        assert_eq!(reader.u64(), Some(42));
        assert_eq!(reader.bytes(), Some(b"photo.jpg".as_slice()));
        assert!(reader.is_empty());
    }

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
    fn large_attachment_length_is_little_endian_and_checked() {
        let mut reader = super::PayloadReader::new(&[0x00, 0x00, 0x10, 0x00]);
        assert_eq!(reader.u32(), Some(1024 * 1024));
        assert!(reader.is_empty());
        assert_eq!(super::PayloadReader::new(&[0, 0, 0x10]).u32(), None);
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

/// The first page of a direct message. Continuation replies contain raw bytes
/// starting at the requested offset, which may fall inside a UTF-8 character.
pub struct DirectFrame<'a> {
    pub kind: u8,
    pub handle: u64,
    pub message: u64,
    pub part: u16,
    pub id: &'a str,
    pub total: usize,
    pub first: &'a [u8],
}

impl<'a> DirectFrame<'a> {
    pub fn parse(payload: &'a [u8], limit: usize) -> Option<Self> {
        let mut r = PayloadReader::new(payload);
        let kind = r.u8()?;
        let handle = r.u64()?;
        let message = r.u64()?;
        let part = r.u16()?;
        let id = core::str::from_utf8(r.bytes()?).ok()?;
        let total = r.u32()? as usize;
        let first = r.bytes()?;
        if !r.is_empty() || !(7..=9).contains(&kind) || handle == 0 || message == 0
            || total > limit || first.len() != total.min(3000)
            || id.len() > 64 || (kind != 7 && id.is_empty())
            || (kind == 9 && total != 0) {
            return None;
        }
        Some(Self { kind, handle, message, part, id, total, first })
    }
}

/// A pending invitation can gain a peer name after the first connection list.
/// Pace metadata reads while that happens; completed lists need no polling.
#[derive(Default)]
pub struct MetadataRefresh { last: Option<u32> }
impl MetadataRefresh {
    pub const fn new() -> Self { Self { last: None } }
    pub fn due(&mut self, pending: bool, busy: bool, now: u32) -> bool {
        if !pending { self.last=None; return false; }
        if busy { return false; }
        if self.last.is_some_and(|last| now>=last && now-last<2) { return false; }
        self.last=Some(now);true
    }
}
#[cfg(test)]
mod metadata_refresh_tests {
    use super::MetadataRefresh;
    #[test]
    fn pending_metadata_is_paced_and_never_steals_an_inflight_operation() {
        let mut refresh=MetadataRefresh::new();
        assert!(!refresh.due(false,false,100));
        assert!(!refresh.due(true,true,100));
        assert!(refresh.due(true,false,100));
        for _ in 0..100 { assert!(!refresh.due(true,false,101)); }
        assert!(refresh.due(true,false,102));
        assert!(!refresh.due(false,false,103));
        assert!(refresh.due(true,false,103));
        assert!(refresh.due(true,false,90)); // changed wall clock cannot strand the list
    }
}
