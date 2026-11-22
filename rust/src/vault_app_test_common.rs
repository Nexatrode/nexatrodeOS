use core::{
    ptr::NonNull,
    sync::atomic::{AtomicU32, Ordering},
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_println, get_mr, set_mr,
};

use crate::app_view::APP_VIEW_TEXT_CAPACITY;
use crate::app_view::AppViewState;
use crate::input_protocol::{
    DEVICE_KEYBOARD, EVENT_KEY, EVENT_SYN, REPLY_ROUTED_EVENT_BAD, REPLY_ROUTED_EVENT_OK,
    REQUEST_ROUTED_EVENT, decode_routed_event,
};
use crate::notes::{NOTE_TEXT_CAPACITY, NOTES_BLOB_CAPACITY, NotesApp};
#[cfg(feature = "simplex-agent")]
use crate::simplex_ipc::{AgentPage, Operation, PayloadReader, Status};
use crate::vault_protocol::{
    APP_TEST_SUPERVISOR_CHANNEL, INITIAL_VALUE, REPLY_BAD_REQUEST, REPLY_DENIED,
    REPLY_NAMESPACE_TEST, REPLY_NET_TX_OK, REPLY_NO_BLOB, REPLY_NO_NET, REPLY_OK,
    REPLY_SEND_SEAL_DEMO_OK, REPLY_SEND_UI_SEALED_OK, REPLY_SET_NET_PEER_OK,
    REQUEST_NAMESPACE_TEST, REQUEST_NET_TX, REQUEST_READ, REQUEST_READ_BLOB,
    REQUEST_SEND_SEAL_DEMO, REQUEST_SEND_UI_SEALED, REQUEST_SET_NET_PEER, REQUEST_WRITE,
    REQUEST_WRITE_BLOB,
};
use personal_os_rust_server::calendar::{CALENDAR_BLOB_CAPACITY, Calendar};
use personal_os_rust_server::contacts::{
    CONTACTS_BLOB_CAPACITY, ContactsApp, NAME_CAPACITY, PHONE_CAPACITY,
};
use personal_os_rust_server::message_seal::{self, MAX_PACKET};
use personal_os_rust_server::messages::{MESSAGES_BLOB_CAPACITY, Messages};
use personal_os_rust_server::messages_net_tx::MessagesNetTxPage;
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;

const VAULT: Channel = Channel::new(0);
const ROUTED_INPUT: Channel = Channel::new(2);
const MESSAGES_ROUTED_INPUT: Channel = Channel::new(6);
const NET_TX: Channel = Channel::new(3);
const SLINT_APP_SERVICE: Channel = Channel::new(3);
// Messages already uses endpoint 3 for entropy and endpoint 4 for the
// optional SimpleX Agent, so its display-only Slint service has a separate
// capability endpoint.
const SLINT_MESSAGES_SERVICE: Channel = Channel::new(5);
#[cfg(feature = "simplex-agent")]
const SIMPLEX_AGENT_RESPONSE: Channel = Channel::new(4);
#[cfg(feature = "simplex-agent")]
const SIMPLEX_AGENT_REQUEST: Channel = Channel::new(4);
const REQUEST_SLINT_NOTES_LOAD: MessageLabel = 0x320;
const REQUEST_SLINT_NOTES_SAVE: MessageLabel = 0x321;
const REPLY_SLINT_NOTES_LOAD: MessageLabel = 0x322;
const REPLY_SLINT_NOTES_SAVE: MessageLabel = 0x323;
const REQUEST_SLINT_CONTACTS_LOAD: MessageLabel = 0x330;
const REQUEST_SLINT_CONTACTS_SAVE: MessageLabel = 0x331;
const REPLY_SLINT_CONTACTS_LOAD: MessageLabel = 0x332;
const REPLY_SLINT_CONTACTS_SAVE: MessageLabel = 0x333;
const REQUEST_SLINT_CALENDAR_LOAD: MessageLabel = 0x340;
const REQUEST_SLINT_CALENDAR_SAVE: MessageLabel = 0x341;
const REPLY_SLINT_CALENDAR_LOAD: MessageLabel = 0x342;
const REPLY_SLINT_CALENDAR_SAVE: MessageLabel = 0x343;
const REQUEST_SLINT_MESSAGES_LOAD: MessageLabel = 0x350;
const REPLY_SLINT_MESSAGES_LOAD: MessageLabel = 0x352;
const REQUEST_SLINT_MESSAGES_CREATE_INVITATION: MessageLabel = 0x353;
const REPLY_SLINT_MESSAGES_CREATE_INVITATION: MessageLabel = 0x354;
const REQUEST_SLINT_MESSAGES_INVITATION: MessageLabel = 0x355;
const REPLY_SLINT_MESSAGES_INVITATION: MessageLabel = 0x356;
const REQUEST_SLINT_MESSAGES_JOIN_INVITATION: MessageLabel = 0x357;
const REPLY_SLINT_MESSAGES_JOIN_INVITATION: MessageLabel = 0x358;
const REQUEST_SLINT_MESSAGES_JOIN_STATUS: MessageLabel = 0x359;
const REPLY_SLINT_MESSAGES_JOIN_STATUS: MessageLabel = 0x35a;
const REQUEST_SLINT_MESSAGES_SEND: MessageLabel = 0x35b;
const REPLY_SLINT_MESSAGES_SEND: MessageLabel = 0x35c;
const REQUEST_SLINT_MESSAGES_SEND_STATUS: MessageLabel = 0x35d;
const REPLY_SLINT_MESSAGES_SEND_STATUS: MessageLabel = 0x35e;
const REQUEST_SLINT_MESSAGES_RECEIVE: MessageLabel = 0x35f;
const REPLY_SLINT_MESSAGES_RECEIVE: MessageLabel = 0x360;
const REQUEST_SLINT_MESSAGES_RECEIVE_STATUS: MessageLabel = 0x361;
const REPLY_SLINT_MESSAGES_RECEIVE_STATUS: MessageLabel = 0x362;
const REQUEST_SLINT_MESSAGES_ACK: MessageLabel = 0x363;
const REPLY_SLINT_MESSAGES_ACK: MessageLabel = 0x364;
const REQUEST_SLINT_MESSAGES_ACK_STATUS: MessageLabel = 0x365;
const REPLY_SLINT_MESSAGES_ACK_STATUS: MessageLabel = 0x366;
#[cfg(feature = "simplex-agent")]
const SIMPLEX_INVITATION_URI_CAPACITY: usize = 2048;
#[cfg(feature = "simplex-agent")]
const SIMPLEX_MAX_CONNECTION_HANDLES: usize = 16;

fn write_u16_decimal(buffer: &mut [u8], value: u16) -> usize {
    let mut digits = [0; 5];
    let mut remaining = value;
    let mut count = 0;
    loop {
        digits[count] = b'0' + (remaining % 10) as u8;
        count += 1;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    for index in 0..count {
        buffer[index] = digits[count - index - 1];
    }
    count
}

#[cfg(feature = "simplex-agent")]
fn write_u64_hex(buffer: &mut [u8], value: u64) -> usize {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    if buffer.len() < 16 {
        return 0;
    }
    for index in 0..16 {
        let shift = (15 - index) * 4;
        buffer[index] = DIGITS[((value >> shift) & 0x0f) as usize];
    }
    16
}

pub struct NamespaceTest {
    name: &'static str,
    other_namespace: usize,
    input_events: u32,
    last_input_sequence: MessageRegisterValue,
    app_view: NonNull<AppViewState>,
    notes_touch_mode: Option<NonNull<AtomicU32>>,
    blob_transfer: NonNull<BlobTransferPage>,
    messages_net_tx: Option<NonNull<MessagesNetTxPage>>,
    vault_generation: Option<MessageRegisterValue>,
    contacts: ContactsApp,
    notes: NotesApp,
    calendar: Calendar,
    messages: Messages,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_page: Option<NonNull<AgentPage>>,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_request_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_invitation_request_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_join_probe_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_invitation_uri: [u8; SIMPLEX_INVITATION_URI_CAPACITY],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_invitation_uri_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_connection_handles: [u64; SIMPLEX_MAX_CONNECTION_HANDLES],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_connection_handles_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_join_status: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_join_handle: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_send_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_send_status: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_send_message_id: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_status: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_handle: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_message_id: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_body: [u8; 2048],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_ack_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_ack_status: u64,
    agent_connection_count: Option<u16>,
}

impl NamespaceTest {
    pub const fn new(
        name: &'static str,
        other_namespace: usize,
        app_view: NonNull<AppViewState>,
        notes_touch_mode: Option<NonNull<AtomicU32>>,
        blob_transfer: NonNull<BlobTransferPage>,
    ) -> Self {
        Self::with_net_tx(
            name,
            other_namespace,
            app_view,
            notes_touch_mode,
            blob_transfer,
            None,
        )
    }

    pub const fn with_net_tx(
        name: &'static str,
        other_namespace: usize,
        app_view: NonNull<AppViewState>,
        notes_touch_mode: Option<NonNull<AtomicU32>>,
        blob_transfer: NonNull<BlobTransferPage>,
        messages_net_tx: Option<NonNull<MessagesNetTxPage>>,
    ) -> Self {
        Self {
            name,
            other_namespace,
            input_events: 0,
            last_input_sequence: 0,
            app_view,
            notes_touch_mode,
            blob_transfer,
            messages_net_tx,
            vault_generation: None,
            contacts: ContactsApp::new(),
            notes: NotesApp::new(),
            calendar: Calendar::new(),
            messages: Messages::new(),
            #[cfg(feature = "simplex-agent")]
            simplex_agent_page: None,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_request_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_invitation_request_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_join_probe_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_invitation_uri: [0; SIMPLEX_INVITATION_URI_CAPACITY],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_invitation_uri_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_connection_handles: [0; SIMPLEX_MAX_CONNECTION_HANDLES],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_connection_handles_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_join_status: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_join_handle: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_send_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_send_status: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_send_message_id: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_status: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_handle: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_message_id: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_body: [0; 2048],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_ack_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_ack_status: 0,
            agent_connection_count: None,
        }
    }

    #[cfg(feature = "simplex-agent")]
    pub fn set_simplex_agent_page(&mut self, page: NonNull<AgentPage>) {
        self.simplex_agent_page = Some(page);
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_connection_snapshot(&mut self) {
        if self.simplex_agent_request_started {
            return;
        }
        let Some(mut page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_NO_PAGE");
            return;
        };
        if !unsafe { page.as_mut() }.begin(Operation::ListConnections, 1, &[]) {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_ENCODE");
            return;
        }
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        self.simplex_agent_request_started = true;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|REQUEST_SENT");
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_connection_snapshot(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_request_started = false;
        let Some(page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_NO_PAGE");
            return;
        };
        match unsafe { page.as_ref() }.response(1) {
            Some((Status::Ok, payload)) => {
                let mut reader = PayloadReader::new(payload);
                let Some(count) = reader.u16().map(usize::from) else {
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                    return;
                };
                if count > SIMPLEX_MAX_CONNECTION_HANDLES {
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                    return;
                }
                let mut handles = [0; SIMPLEX_MAX_CONNECTION_HANDLES];
                for handle in &mut handles[..count] {
                    let Some(value) = reader.u64() else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    };
                    if value == 0 {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    }
                    *handle = value;
                }
                if !reader.is_empty() {
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                    return;
                }
                self.agent_connection_count = Some(count as u16);
                self.simplex_agent_connection_handles = handles;
                self.simplex_agent_connection_handles_length = count;
                if self.name == "MESSAGES" {
                    if !self.ensure_loaded() || !self.render_messages_snapshot() {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RENDER");
                        return;
                    }
                }
                debug_println!(
                    "SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|{}",
                    self.agent_connection_count.unwrap_or(0)
                );
            }
            _ => debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE"),
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_invitation(&mut self) {
        if self.simplex_agent_invitation_request_started {
            return;
        }
        let Some(mut page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|INVITATION_FAIL_NO_PAGE");
            return;
        };
        if !unsafe { page.as_mut() }.begin(Operation::CreateInvitation, 2, &[]) {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|INVITATION_FAIL_ENCODE");
            return;
        }
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        self.simplex_agent_invitation_request_started = true;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_INVITATION_REQUEST_SENT");
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_invitation(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        let Some(page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|INVITATION_FAIL_NO_PAGE");
            return;
        };
        match unsafe { page.as_ref() }.response(2) {
            Some((Status::Ok, payload)) => {
                let mut reader = PayloadReader::new(payload);
                let handle = reader.u64();
                let uri = reader.bytes();
                if let (Some(handle), Some(uri)) = (handle, uri) {
                    if handle != 0
                        && !uri.is_empty()
                        && uri.len() <= SIMPLEX_INVITATION_URI_CAPACITY
                        && uri.is_ascii()
                        && reader.is_empty()
                    {
                        self.simplex_agent_invitation_uri[..uri.len()].copy_from_slice(uri);
                        self.simplex_agent_invitation_uri_length = uri.len();
                        debug_println!(
                            "SIMPLEXIPC|MESSAGES_APP|CREATE_INVITATION_PASS|HANDLE={}|URI_LEN={}",
                            handle,
                            uri.len()
                        );
                        #[cfg(feature = "simplex-agent-qemu-external-cli-test")]
                        if self.name == "MESSAGES" {
                            // Test-only bearer-link export so the independent
                            // host CLI can join this disposable QEMU invitation.
                            // Never compile this feature into a device image.
                            if let Ok(uri) = core::str::from_utf8(uri) {
                                const HEX: &[u8; 16] = b"0123456789abcdef";
                                for (index, part) in uri.as_bytes().chunks(16).enumerate() {
                                    let mut encoded = [0u8; 32];
                                    for (i, byte) in part.iter().copied().enumerate() {
                                        encoded[i * 2] = HEX[(byte >> 4) as usize];
                                        encoded[i * 2 + 1] = HEX[(byte & 0xf) as usize];
                                    }
                                    if let Ok(encoded) =
                                        core::str::from_utf8(&encoded[..part.len() * 2])
                                    {
                                        for _ in 0..2 {
                                            debug_println!(
                                                "SIMPLEX_UI_EXTERNAL_INVITE_CHUNK|{}|{}",
                                                index,
                                                encoded
                                            );
                                        }
                                    }
                                }
                            }
                            self.request_agent_connection_snapshot();
                            return;
                        }
                        #[cfg(feature = "simplex-agent-qemu-join-test")]
                        if self.name == "MESSAGES" && !self.simplex_agent_join_probe_started {
                            self.simplex_agent_join_probe_started = true;
                            let mut invitation = [0; SIMPLEX_INVITATION_URI_CAPACITY];
                            invitation[..uri.len()].copy_from_slice(uri);
                            self.request_agent_join_invitation(&invitation[..uri.len()]);
                        } else {
                            self.request_agent_connection_snapshot();
                        }
                        #[cfg(not(feature = "simplex-agent-qemu-join-test"))]
                        self.request_agent_connection_snapshot();
                        return;
                    }
                }
                debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_INVITATION_FAIL_PAYLOAD");
            }
            _ => debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_INVITATION_FAIL_RESPONSE"),
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_join_invitation(&mut self, invitation_uri: &[u8]) {
        if invitation_uri.is_empty()
            || invitation_uri.len() > SIMPLEX_INVITATION_URI_CAPACITY
            || !invitation_uri.is_ascii()
        {
            self.simplex_agent_join_status = 4;
            debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_INVALID_INPUT");
            return;
        }
        let Some(mut page) = self.simplex_agent_page else {
            self.simplex_agent_join_status = 4;
            debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_FAIL_NO_PAGE");
            return;
        };
        let mut payload = [0; personal_os_rust_server::simplex_ipc::PAYLOAD_CAPACITY];
        let mut writer = personal_os_rust_server::simplex_ipc::PayloadWriter::new(&mut payload);
        if !writer.bytes(invitation_uri) {
            self.simplex_agent_join_status = 4;
            debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_FAIL_ENCODE");
            return;
        }
        let length = writer.len();
        if !unsafe { page.as_mut() }.begin(Operation::JoinInvitation, 3, &payload[..length]) {
            self.simplex_agent_join_status = 4;
            debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_FAIL_BEGIN");
            return;
        }
        self.simplex_agent_join_status = 1;
        self.simplex_agent_join_handle = 0;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_REQUEST_SENT");
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_join_invitation(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        let Some(page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_FAIL_NO_PAGE");
            return;
        };
        match unsafe { page.as_ref() }.response(3) {
            Some((Status::Invalid, payload)) if payload.is_empty() => {
                self.simplex_agent_join_status = 3;
                debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_REJECTED");
                self.request_agent_connection_snapshot();
            }
            Some((Status::Ok, payload)) => {
                let mut reader = PayloadReader::new(payload);
                if let Some(handle) = reader.u64().filter(|handle| *handle != 0) {
                    if reader.is_empty() {
                        debug_println!(
                            "SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_PASS|HANDLE={}",
                            handle
                        );
                        self.simplex_agent_join_status = 2;
                        self.simplex_agent_join_handle = handle;
                        self.request_agent_connection_snapshot();
                        return;
                    }
                }
                debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_FAIL_PAYLOAD");
                self.simplex_agent_join_status = 4;
            }
            _ => {
                debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_FAIL_RESPONSE");
                self.simplex_agent_join_status = 4;
            }
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_send_message(&mut self, text: &[u8]) {
        if self.simplex_agent_send_started
            || text.is_empty()
            || text.len() > 2048
            || core::str::from_utf8(text).is_err()
        {
            self.simplex_agent_send_status = 4;
            return;
        }
        let Some(handle) = (if self.simplex_agent_join_handle != 0 {
            Some(self.simplex_agent_join_handle)
        } else {
            self.simplex_agent_connection_handles
                .get(..self.simplex_agent_connection_handles_length)
                .and_then(|handles| handles.first().copied())
        }) else {
            self.simplex_agent_send_status = 4;
            return;
        };
        let Some(mut page) = self.simplex_agent_page else {
            self.simplex_agent_send_status = 4;
            return;
        };
        let mut payload = [0; personal_os_rust_server::simplex_ipc::PAYLOAD_CAPACITY];
        let mut writer = personal_os_rust_server::simplex_ipc::PayloadWriter::new(&mut payload);
        if !writer.u64(handle) || !writer.bytes(text) {
            self.simplex_agent_send_status = 4;
            return;
        }
        let length = writer.len();
        if !unsafe { page.as_mut() }.begin(Operation::SendMessage, 4, &payload[..length]) {
            self.simplex_agent_send_status = 4;
            return;
        }
        self.simplex_agent_send_started = true;
        self.simplex_agent_send_status = 1;
        self.simplex_agent_send_message_id = 0;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_send_message(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_send_started = false;
        let Some(page) = self.simplex_agent_page else {
            self.simplex_agent_send_status = 4;
            return;
        };
        match unsafe { page.as_ref() }.response(4) {
            Some((Status::Ok, payload)) => {
                let mut reader = PayloadReader::new(payload);
                if let Some(message_id) = reader.u64().filter(|id| *id != 0) {
                    if reader.is_empty() {
                        self.simplex_agent_send_status = 2;
                        self.simplex_agent_send_message_id = message_id;
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|SEND_MESSAGE_PASS");
                        return;
                    }
                }
                self.simplex_agent_send_status = 4;
            }
            Some((Status::Failed | Status::Unavailable, _)) => {
                self.simplex_agent_send_status = 4;
            }
            _ => self.simplex_agent_send_status = 4,
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_receive_next(&mut self) {
        if self.simplex_agent_receive_started || self.simplex_agent_receive_status == 2 {
            return;
        }
        let Some(mut page) = self.simplex_agent_page else {
            self.simplex_agent_receive_status = 4;
            return;
        };
        if !unsafe { page.as_mut() }.begin(Operation::ReceiveNext, 5, &[]) {
            self.simplex_agent_receive_status = 4;
            return;
        }
        self.simplex_agent_receive_started = true;
        self.simplex_agent_receive_status = 1;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_next(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_receive_started = false;
        let Some(page) = self.simplex_agent_page else {
            self.simplex_agent_receive_status = 4;
            return;
        };
        match unsafe { page.as_ref() }.response(5) {
            Some((Status::NoEvent, payload)) if payload.is_empty() => {
                self.simplex_agent_receive_status = 3;
                debug_println!("SIMPLEXIPC|MESSAGES_APP|RECEIVE_EMPTY");
            }
            Some((Status::Ok, payload)) => {
                let mut reader = PayloadReader::new(payload);
                let parsed = reader
                    .u8()
                    .filter(|kind| *kind == 1)
                    .zip(reader.u64())
                    .zip(reader.u64())
                    .zip(reader.bytes())
                    .map(|(((kind, handle), message_id), body)| {
                        let _ = kind;
                        (handle, message_id, body)
                    });
                if let Some((handle, message_id, body)) = parsed {
                    if !reader.is_empty()
                        || handle == 0
                        || message_id == 0
                        || body.len() > self.simplex_agent_receive_body.len()
                        || core::str::from_utf8(body).is_err()
                    {
                        self.simplex_agent_receive_status = 4;
                        return;
                    }
                    self.simplex_agent_receive_body[..body.len()].copy_from_slice(body);
                    self.simplex_agent_receive_length = body.len();
                    self.simplex_agent_receive_handle = handle;
                    self.simplex_agent_receive_message_id = message_id;
                    self.simplex_agent_receive_status = 2;
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|RECEIVE_MESSAGE_PASS");
                } else {
                    self.simplex_agent_receive_status = 4;
                }
            }
            _ => self.simplex_agent_receive_status = 4,
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_acknowledge(&mut self) {
        if self.simplex_agent_ack_started
            || self.simplex_agent_receive_status != 2
            || self.simplex_agent_receive_handle == 0
            || self.simplex_agent_receive_message_id == 0
        {
            self.simplex_agent_ack_status = 4;
            return;
        }
        let Some(mut page) = self.simplex_agent_page else {
            self.simplex_agent_ack_status = 4;
            return;
        };
        let mut payload = [0; 16];
        payload[..8].copy_from_slice(&self.simplex_agent_receive_handle.to_le_bytes());
        payload[8..].copy_from_slice(&self.simplex_agent_receive_message_id.to_le_bytes());
        if !unsafe { page.as_mut() }.begin(Operation::Acknowledge, 6, &payload) {
            self.simplex_agent_ack_status = 4;
            return;
        }
        self.simplex_agent_ack_started = true;
        self.simplex_agent_ack_status = 1;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_acknowledge(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_ack_started = false;
        let Some(page) = self.simplex_agent_page else {
            self.simplex_agent_ack_status = 4;
            return;
        };
        match unsafe { page.as_ref() }.response(6) {
            Some((Status::Ok, payload)) if payload.is_empty() => {
                self.simplex_agent_ack_status = 2;
                self.simplex_agent_receive_status = 0;
                self.simplex_agent_receive_length = 0;
                self.simplex_agent_receive_handle = 0;
                self.simplex_agent_receive_message_id = 0;
                debug_println!("SIMPLEXIPC|MESSAGES_APP|ACKNOWLEDGE_PASS");
            }
            _ => self.simplex_agent_ack_status = 4,
        }
    }

    fn run_namespace_checks(&mut self) -> bool {
        let previous = read_record();
        let (starting_generation, current_value) = match previous {
            Some(record) => record,
            None => {
                log(
                    self.name,
                    "FAIL: own namespace has unexpected or unreadable record",
                );
                return false;
            }
        };
        log(self.name, "PASS: reads its own namespace");

        let Some(next_generation) = starting_generation.checked_add(1) else {
            log(self.name, "FAIL: namespace generation exhausted");
            return false;
        };
        // Let the vault read and rewrite whichever versioned record currently
        // owns this endpoint, without asking the app to name a namespace.
        let check = VAULT.pp_call(MessageInfo::new(REQUEST_NAMESPACE_TEST, 0));
        let checked_record = (check.label() == REPLY_NAMESPACE_TEST && check.count() == 2)
            .then(|| (get_mr(0), get_mr(1)));
        if checked_record != Some((next_generation, current_value)) {
            log(self.name, "FAIL: own namespace update did not round-trip");
            return false;
        }
        // The isolation check deliberately advances this app's vault
        // generation while preserving its active value/blob. Keep any loaded
        // app model in sync so its next save does not fail as a stale write.
        if self.vault_generation.is_some() {
            self.vault_generation = Some(next_generation);
        }
        log(self.name, "PASS: writes and reads back its own namespace");
        debug_println!("NAMESPACE|{}|GENERATION:{}", self.name, next_generation);

        // The app deliberately supplies a namespace-looking payload. The wire
        // API has no namespace selector, so both malformed attempts are rejected.
        set_mr(0, self.other_namespace as MessageRegisterValue);
        let forged_read = VAULT.pp_call(MessageInfo::new(REQUEST_READ, 1));
        set_mr(0, starting_generation);
        set_mr(1, current_value ^ 0xffff);
        set_mr(2, self.other_namespace as MessageRegisterValue);
        let forged_write = VAULT.pp_call(MessageInfo::new(REQUEST_WRITE, 3));
        let attempts_rejected = forged_read.label() == REPLY_BAD_REQUEST
            && forged_read.count() == 0
            && forged_write.label() == REPLY_BAD_REQUEST
            && forged_write.count() == 0;
        if !attempts_rejected {
            log(
                self.name,
                "FAIL: caller-selected namespace fields were accepted",
            );
            return false;
        }
        log(self.name, "PASS: rejects caller-selected namespace fields");

        if read_record() != Some((next_generation, current_value)) {
            log(self.name, "FAIL: malformed requests changed its namespace");
            false
        } else {
            log(
                self.name,
                "PASS: malformed requests left its namespace unchanged",
            );
            true
        }
    }
}

impl Handler for NamespaceTest {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        #[cfg(feature = "simplex-agent")]
        if _channels.contains(SIMPLEX_AGENT_RESPONSE) {
            if self
                .simplex_agent_page
                .and_then(|page| unsafe { page.as_ref() }.response(3))
                .is_some()
            {
                self.receive_agent_join_invitation();
            } else if self
                .simplex_agent_page
                .and_then(|page| unsafe { page.as_ref() }.response(4))
                .is_some()
            {
                self.receive_agent_send_message();
            } else if self
                .simplex_agent_page
                .and_then(|page| unsafe { page.as_ref() }.response(5))
                .is_some()
            {
                self.receive_agent_next();
            } else if self
                .simplex_agent_page
                .and_then(|page| unsafe { page.as_ref() }.response(6))
                .is_some()
            {
                self.receive_agent_acknowledge();
            } else if self
                .simplex_agent_page
                .and_then(|page| unsafe { page.as_ref() }.response(2))
                .is_some()
            {
                self.receive_agent_invitation();
            } else {
                self.receive_agent_connection_snapshot();
            }
            return Ok(());
        }
        log(
            "app",
            "FAIL: namespace test received an unexpected notification",
        );
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel == SLINT_MESSAGES_SERVICE && self.name == "MESSAGES" {
            return Ok(self.handle_slint_messages(request));
        }
        if channel == SLINT_APP_SERVICE {
            return Ok(match self.name {
                "NOTES" => self.handle_slint_notes(request),
                "CONTACTS" => self.handle_slint_contacts(request),
                "CALENDAR" => self.handle_slint_calendar(request),
                _ => MessageInfo::new(REPLY_DENIED, 0),
            });
        }
        if channel == ROUTED_INPUT || (self.name == "MESSAGES" && channel == MESSAGES_ROUTED_INPUT)
        {
            return Ok(self.handle_routed_input(request));
        }
        if channel != APP_TEST_SUPERVISOR_CHANNEL {
            return Ok(MessageInfo::new(REPLY_DENIED, 0));
        }
        if request.label() == REQUEST_SEND_SEAL_DEMO && request.count() == 0 {
            return Ok(self.handle_send_seal_demo());
        }
        if request.label() == REQUEST_SEND_UI_SEALED && request.count() == 0 {
            return Ok(self.handle_send_ui_sealed());
        }
        if request.label() == REQUEST_SET_NET_PEER && request.count() == 2 {
            return Ok(self.handle_set_net_peer());
        }
        if request.label() != REQUEST_NAMESPACE_TEST || request.count() != 0 {
            return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
        }
        let label: MessageLabel = if self.run_namespace_checks() {
            #[cfg(feature = "simplex-agent")]
            if self.name == "MESSAGES" {
                self.request_agent_connection_snapshot();
            }
            REPLY_NAMESPACE_TEST
        } else {
            REPLY_BAD_REQUEST
        };
        Ok(MessageInfo::new(label, 0))
    }
}

impl NamespaceTest {
    /// Slint receives only Messages' current local peer/thread summary. The
    /// Messages PD remains the sole owner of its vault record and Agent state.
    fn handle_slint_messages(&mut self, request: MessageInfo) -> MessageInfo {
        match (request.label(), request.count()) {
            (REQUEST_SLINT_MESSAGES_CREATE_INVITATION, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    if self.simplex_agent_invitation_uri_length == 0 {
                        self.request_agent_invitation();
                    }
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_CREATE_INVITATION, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_INVITATION, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let uri = &self.simplex_agent_invitation_uri
                        [..self.simplex_agent_invitation_uri_length];
                    if !unsafe { self.blob_transfer.as_mut() }.set_payload(uri) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, uri.len() as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_INVITATION, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_JOIN_INVITATION, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let length = get_mr(0) as usize;
                    if length == 0 || length > SIMPLEX_INVITATION_URI_CAPACITY {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    };
                    if payload.len() != length || !payload.is_ascii() {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    self.request_agent_join_invitation(payload);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_JOIN_INVITATION, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_JOIN_STATUS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    set_mr(0, self.simplex_agent_join_status);
                    set_mr(1, self.simplex_agent_join_handle);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_JOIN_STATUS, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_SEND, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let length = get_mr(0) as usize;
                    if length == 0 || length > 2048 {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    };
                    if payload.len() != length || core::str::from_utf8(payload).is_err() {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    self.request_agent_send_message(payload);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_SEND, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_SEND_STATUS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    set_mr(0, self.simplex_agent_send_status);
                    set_mr(1, self.simplex_agent_send_message_id);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_SEND_STATUS, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_RECEIVE, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    self.request_agent_receive_next();
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_RECEIVE_STATUS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let length = self.simplex_agent_receive_length;
                    if !unsafe { self.blob_transfer.as_mut() }
                        .set_payload(&self.simplex_agent_receive_body[..length])
                    {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, self.simplex_agent_receive_status);
                    set_mr(1, length as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE_STATUS, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_ACK, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    self.request_agent_acknowledge();
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_ACK, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_ACK_STATUS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    set_mr(0, self.simplex_agent_ack_status);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_ACK_STATUS, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_LOAD, 0) => {}
            _ => return MessageInfo::new(REPLY_BAD_REQUEST, 0),
        }
        // The shell is only a read-only view. Ask the isolated Agent for its
        // connection count on the first view load, then include the last
        // completed Agent snapshot alongside Messages' local peer summary.
        #[cfg(feature = "simplex-agent")]
        self.request_agent_connection_snapshot();
        if !self.ensure_messages_loaded() {
            debug_println!("SLINT MESSAGES SERVICE LOAD FAILED: ensure_messages_loaded");
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }
        let mut text = [0; APP_VIEW_TEXT_CAPACITY];
        let length = self.messages.display(&mut text).len();
        if length == 0 {
            debug_println!("SLINT MESSAGES SERVICE LOAD FAILED: transfer capacity");
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }
        let mut payload_length = length;
        #[cfg(feature = "simplex-agent")]
        {
            const LABEL: &[u8] = b"\nSIMPLEX CONNECTIONS ";
            if payload_length + LABEL.len() + 5 < text.len() {
                text[payload_length..payload_length + LABEL.len()].copy_from_slice(LABEL);
                payload_length += LABEL.len();
                if let Some(count) = self.agent_connection_count {
                    payload_length += write_u16_decimal(&mut text[payload_length..], count);
                } else {
                    const PENDING: &[u8] = b"PENDING";
                    text[payload_length..payload_length + PENDING.len()].copy_from_slice(PENDING);
                    payload_length += PENDING.len();
                }
            }
            const ID_LABEL: &[u8] = b"\nCONNECTION ID ";
            for handle in self.simplex_agent_connection_handles
                [..self.simplex_agent_connection_handles_length.min(3)]
                .iter()
            {
                if payload_length + ID_LABEL.len() + 16 >= text.len() {
                    break;
                }
                text[payload_length..payload_length + ID_LABEL.len()].copy_from_slice(ID_LABEL);
                payload_length += ID_LABEL.len();
                payload_length += write_u64_hex(&mut text[payload_length..], *handle);
            }
        }
        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&text[..payload_length]) {
            debug_println!("SLINT MESSAGES SERVICE LOAD FAILED: transfer capacity");
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }
        set_mr(0, payload_length as MessageRegisterValue);
        debug_println!("SLINT MESSAGES SERVICE LOAD PASS LEN {}", payload_length);
        MessageInfo::new(REPLY_SLINT_MESSAGES_LOAD, 1)
    }

    /// The Slint reminder editor receives only the selected title; Calendar
    /// keeps ownership of event selection and its fixed vault namespace.
    fn handle_slint_calendar(&mut self, request: MessageInfo) -> MessageInfo {
        match (request.label(), request.count()) {
            (REQUEST_SLINT_CALENDAR_LOAD, 0) => {
                if !self.ensure_calendar_loaded() {
                    debug_println!("SLINT CALENDAR LOAD FAILED: ensure_calendar_loaded");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let title = self.calendar.selected_title().unwrap_or(&[]);
                if !unsafe { self.blob_transfer.as_mut() }.set_payload(title) {
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                set_mr(0, title.len() as MessageRegisterValue);
                MessageInfo::new(REPLY_SLINT_CALENDAR_LOAD, 1)
            }
            (REQUEST_SLINT_CALENDAR_SAVE, 1) => {
                let length = get_mr(0) as usize;
                if length == 0
                    || length > personal_os_rust_server::calendar::REMINDER_TITLE_CAPACITY
                {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                if payload.len() != length {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                let mut title = [0; personal_os_rust_server::calendar::REMINDER_TITLE_CAPACITY];
                title[..length].copy_from_slice(payload);
                if !self.ensure_calendar_loaded() {
                    debug_println!("SLINT CALENDAR SAVE FAILED: ensure_calendar_loaded");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let previous = self.calendar;
                if !self.calendar.replace_selected_title(&title[..length]) {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                if !self.persist_calendar() || !self.verify_calendar_persisted() {
                    self.calendar = previous;
                    debug_println!("SLINT CALENDAR SAVE FAILED: persist/readback");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let _ = self.render_calendar_snapshot();
                debug_println!("NAMESPACE|CALENDAR|SLINT SAVE VERIFIED LEN {}", length);
                MessageInfo::new(REPLY_SLINT_CALENDAR_SAVE, 0)
            }
            _ => MessageInfo::new(REPLY_BAD_REQUEST, 0),
        }
    }

    fn verify_calendar_persisted(&mut self) -> bool {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
        if reply.label() != REPLY_OK || reply.count() != 2 {
            return false;
        }
        let generation = get_mr(0);
        let length = get_mr(1) as usize;
        let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
            return false;
        };
        if payload.len() != length {
            return false;
        }
        let Some(calendar) = Calendar::decode(payload) else {
            return false;
        };
        if calendar != self.calendar {
            return false;
        }
        self.vault_generation = Some(generation);
        true
    }

    /// The Slint contact editor can load or replace the selected name/phone
    /// pair. Contacts alone chooses the vault namespace and generation.
    fn handle_slint_contacts(&mut self, request: MessageInfo) -> MessageInfo {
        match (request.label(), request.count()) {
            (REQUEST_SLINT_CONTACTS_LOAD, 0) => {
                if !self.ensure_loaded() {
                    debug_println!("SLINT CONTACTS LOAD FAILED: ensure_loaded");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let (name, phone) = self
                    .contacts
                    .selected()
                    .map(|contact| (contact.name(), contact.phone()))
                    .unwrap_or((&[], &[]));
                let mut payload = [0; NAME_CAPACITY + PHONE_CAPACITY];
                payload[..name.len()].copy_from_slice(name);
                payload[name.len()..name.len() + phone.len()].copy_from_slice(phone);
                if !unsafe { self.blob_transfer.as_mut() }
                    .set_payload(&payload[..name.len() + phone.len()])
                {
                    debug_println!("SLINT CONTACTS LOAD FAILED: transfer capacity");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                set_mr(0, name.len() as MessageRegisterValue);
                set_mr(1, phone.len() as MessageRegisterValue);
                MessageInfo::new(REPLY_SLINT_CONTACTS_LOAD, 2)
            }
            (REQUEST_SLINT_CONTACTS_SAVE, 2) => {
                let name_length = get_mr(0) as usize;
                let phone_length = get_mr(1) as usize;
                if name_length == 0 || name_length > NAME_CAPACITY || phone_length > PHONE_CAPACITY
                {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                if payload.len() != name_length + phone_length {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                if !self.ensure_loaded() {
                    debug_println!("SLINT CONTACTS SAVE FAILED: ensure_loaded");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let previous = self.contacts;
                if !self
                    .contacts
                    .replace_selected_fields(&payload[..name_length], &payload[name_length..])
                {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                if !self.persist_contacts() {
                    self.contacts = previous;
                    debug_println!("SLINT CONTACTS SAVE FAILED: persist_contacts");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                if !self.verify_contacts_persisted() {
                    self.contacts = previous;
                    debug_println!("SLINT CONTACTS SAVE FAILED: verify_contacts_persisted");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let _ = self.render_contacts_snapshot();
                debug_println!("NAMESPACE|CONTACTS|SLINT SAVE VERIFIED");
                MessageInfo::new(REPLY_SLINT_CONTACTS_SAVE, 0)
            }
            _ => MessageInfo::new(REPLY_BAD_REQUEST, 0),
        }
    }

    fn verify_contacts_persisted(&mut self) -> bool {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
        if reply.label() != REPLY_OK || reply.count() != 2 {
            return false;
        }
        let generation = get_mr(0);
        let length = get_mr(1) as usize;
        let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
            return false;
        };
        if payload.len() != length {
            return false;
        }
        let Some(contacts) = ContactsApp::decode(payload) else {
            return false;
        };
        if contacts != self.contacts {
            return false;
        }
        self.vault_generation = Some(generation);
        true
    }

    /// Narrow service interface used by the Slint shell. The shell can ask the
    /// Notes domain to load or replace the selected body, but it has no vault
    /// endpoint and cannot choose a namespace or generation.
    fn handle_slint_notes(&mut self, request: MessageInfo) -> MessageInfo {
        if self.name != "NOTES" {
            return MessageInfo::new(REPLY_DENIED, 0);
        }
        match (request.label(), request.count()) {
            (REQUEST_SLINT_NOTES_LOAD, 0) => {
                if !self.ensure_loaded() {
                    debug_println!("SLINT NOTES SERVICE LOAD FAILED: ensure_loaded");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let text = self.notes.current_text();
                if !unsafe { self.blob_transfer.as_mut() }.set_payload(text) {
                    debug_println!("SLINT NOTES SERVICE LOAD FAILED: transfer capacity");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                set_mr(0, text.len() as MessageRegisterValue);
                MessageInfo::new(REPLY_SLINT_NOTES_LOAD, 1)
            }
            (REQUEST_SLINT_NOTES_SAVE, 1) => {
                let length = get_mr(0) as usize;
                if length > NOTE_TEXT_CAPACITY {
                    debug_println!("SLINT NOTES SERVICE SAVE FAILED: length {}", length);
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                    debug_println!("SLINT NOTES SERVICE SAVE FAILED: transfer payload");
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                if payload.len() != length {
                    debug_println!("SLINT NOTES SERVICE SAVE FAILED: length mismatch");
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                let mut text = [0; NOTE_TEXT_CAPACITY];
                text[..length].copy_from_slice(payload);
                if !self.ensure_loaded() {
                    debug_println!("SLINT NOTES SERVICE SAVE FAILED: ensure_loaded");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                let previous = self.notes;
                if !self.notes.replace_current_text(&text[..length]) {
                    debug_println!("SLINT NOTES SERVICE SAVE FAILED: model rejected text");
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                if !self.persist_notes() {
                    self.notes = previous;
                    debug_println!("SLINT NOTES SERVICE SAVE FAILED: persist_notes");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                if !self.verify_notes_persisted() {
                    self.notes = previous;
                    debug_println!("SLINT NOTES SERVICE SAVE FAILED: verify_notes_persisted");
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                self.render_notes_snapshot();
                debug_println!("NAMESPACE|NOTES|SLINT SAVE VERIFIED LEN {}", length);
                MessageInfo::new(REPLY_SLINT_NOTES_SAVE, 0)
            }
            _ => {
                debug_println!("SLINT NOTES SERVICE FAILED: unsupported request");
                MessageInfo::new(REPLY_BAD_REQUEST, 0)
            }
        }
    }

    fn verify_notes_persisted(&mut self) -> bool {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
        if reply.label() != REPLY_OK || reply.count() != 2 {
            return false;
        }
        let generation = get_mr(0);
        let length = get_mr(1) as usize;
        let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
            return false;
        };
        if payload.len() != length {
            return false;
        }
        let Some(notes) = NotesApp::decode(payload) else {
            return false;
        };
        if notes != self.notes {
            return false;
        }
        self.vault_generation = Some(generation);
        true
    }
}

impl NamespaceTest {
    fn handle_send_seal_demo(&mut self) -> MessageInfo {
        if self.name != "MESSAGES" {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let mut packet = [0u8; MAX_PACKET];
        let length = message_seal::demo_udp_packet(&mut packet);
        match self.tx_sealed_datagram(&packet[..length], false) {
            Ok(()) => {
                log(self.name, "PASS: sealed demo handed to dhcp_client");
                MessageInfo::new(REPLY_SEND_SEAL_DEMO_OK, 0)
            }
            Err(REPLY_NO_NET) => MessageInfo::new(REPLY_NO_NET, 0),
            Err(_) => {
                log(self.name, "FAIL: dhcp_client rejected sealed TX");
                MessageInfo::new(REPLY_BAD_REQUEST, 0)
            }
        }
    }

    fn handle_send_ui_sealed(&mut self) -> MessageInfo {
        if self.name != "MESSAGES" {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        // Probe net before mutating vault state so offline images stay quiet.
        if !self.net_tx_ready() {
            return MessageInfo::new(REPLY_NO_NET, 0);
        }
        if !self.ensure_loaded() {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let old = self.messages;
        self.drive_ui_compose_seal();
        let Some(packet) = self.messages.selected_sealed() else {
            self.messages = old;
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        };
        let mut copy = [0u8; MAX_PACKET];
        let length = packet.len();
        copy[..length].copy_from_slice(packet);
        if !self.persist_messages() {
            self.messages = old;
            log(self.name, "FAIL: vault could not persist UI sealed peer");
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        match self.tx_sealed_datagram(&copy[..length], true) {
            Ok(()) => {
                let _ = self.render_messages_snapshot();
                log(self.name, "PASS: UI sealed packet handed to dhcp_client");
                MessageInfo::new(REPLY_SEND_UI_SEALED_OK, 0)
            }
            Err(REPLY_NO_NET) => MessageInfo::new(REPLY_NO_NET, 0),
            Err(_) => {
                log(self.name, "FAIL: dhcp_client rejected UI sealed TX");
                MessageInfo::new(REPLY_BAD_REQUEST, 0)
            }
        }
    }

    fn handle_set_net_peer(&mut self) -> MessageInfo {
        if self.name != "MESSAGES" {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let Some(mut page) = self.messages_net_tx else {
            return MessageInfo::new(REPLY_NO_NET, 0);
        };
        let packed = get_mr(0);
        let port = get_mr(1) as u16;
        if port == 0 {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let ipv4 = [
            (packed & 0xff) as u8,
            ((packed >> 8) & 0xff) as u8,
            ((packed >> 16) & 0xff) as u8,
            ((packed >> 24) & 0xff) as u8,
        ];
        let page = unsafe { page.as_mut() };
        page.lock_peer(ipv4, port);
        log(self.name, "PASS: sealed TX peer locked");
        MessageInfo::new(REPLY_SET_NET_PEER_OK, 0)
    }

    /// Replay the Messages UI key path: add peer BE12CD34, compose HI, seal.
    fn drive_ui_compose_seal(&mut self) {
        self.messages = Messages::new();
        let key_codes = [48u16, 18, 2, 3, 46, 32, 4, 5]; // BE12CD34
        self.messages.apply_key(28, 1); // Enter → ADD KEY
        for code in key_codes {
            self.messages.apply_key(code, 1);
        }
        self.messages.apply_key(28, 1); // commit peer
        self.messages.apply_key(28, 1); // compose
        self.messages.apply_key(35, 1); // H
        self.messages.apply_key(23, 1); // I
        let _ = self.messages.apply_key(28, 1); // seal
    }

    fn net_tx_ready(&self) -> bool {
        let Some(page) = self.messages_net_tx else {
            return false;
        };
        unsafe { page.as_ref().net_ready() }
    }

    fn tx_sealed_datagram(&mut self, bytes: &[u8], over_tls: bool) -> Result<(), MessageLabel> {
        let Some(mut page) = self.messages_net_tx else {
            return Err(REPLY_NO_NET);
        };
        let page = unsafe { page.as_mut() };
        if !page.net_ready() {
            return Err(REPLY_NO_NET);
        }
        if over_tls {
            page.set_demo_peer_tls();
        } else {
            page.set_demo_peer();
        }
        if !page.set_datagram(bytes) {
            return Err(REPLY_BAD_REQUEST);
        }
        let reply = NET_TX.pp_call(MessageInfo::new(REQUEST_NET_TX, 0));
        if reply.label() != REPLY_NET_TX_OK {
            return Err(REPLY_BAD_REQUEST);
        }
        Ok(())
    }

    /// Best-effort TX after UI compose; no-op when the net page is offline.
    fn try_tx_selected_sealed(&mut self) {
        let Some(packet) = self.messages.selected_sealed() else {
            return;
        };
        let mut copy = [0u8; MAX_PACKET];
        let length = packet.len();
        copy[..length].copy_from_slice(packet);
        let _ = self.tx_sealed_datagram(&copy[..length], true);
    }

    fn handle_routed_input(&mut self, request: MessageInfo) -> MessageInfo {
        if request.label() != REQUEST_ROUTED_EVENT || request.count() != 3 {
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }

        let Some((event, sequence)) =
            decode_routed_event(get_mr(0) as u64, get_mr(1) as u64, get_mr(2) as u64)
        else {
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        };
        if sequence as MessageRegisterValue <= self.last_input_sequence {
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }

        self.last_input_sequence = sequence as MessageRegisterValue;
        self.input_events = self.input_events.saturating_add(1);
        if !self.ensure_loaded() {
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }
        if event.event_type == EVENT_SYN {
            // Focus-leave / restore SYN: persist in-RAM Notes edits.
            if self.name == "NOTES" && !self.persist_notes() {
                log(self.name, "FAIL: vault could not persist Notes collection");
                return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
            }
            return MessageInfo::new(REPLY_ROUTED_EVENT_OK, 0);
        }
        if event.device == DEVICE_KEYBOARD && event.event_type == EVENT_KEY {
            if self.name == "CONTACTS" {
                let old_contacts = self.contacts;
                let update = self.contacts.apply_key(event.code, event.value);
                if update.changed {
                    if update.persist && !self.persist_contacts() {
                        self.contacts = old_contacts;
                        self.render_contacts_snapshot();
                        log(self.name, "FAIL: vault could not persist contact list");
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    if !self.render_contacts_snapshot() {
                        self.contacts = old_contacts;
                        log(self.name, "FAIL: could not render contact list");
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                }
            } else if self.name == "CALENDAR" {
                let old_calendar = self.calendar;
                let update = self.calendar.apply_key(event.code, event.value);
                if update.changed {
                    if update.persist && !self.persist_calendar() {
                        self.calendar = old_calendar;
                        self.render_calendar_snapshot();
                        log(self.name, "FAIL: vault could not persist reminders");
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    if !self.render_calendar_snapshot() {
                        self.calendar = old_calendar;
                        log(self.name, "FAIL: could not render reminders");
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                }
            } else if self.name == "MESSAGES" {
                let old_messages = self.messages;
                let update = self.messages.apply_key(event.code, event.value);
                if update.changed {
                    if update.persist && !self.persist_messages() {
                        self.messages = old_messages;
                        self.render_messages_snapshot();
                        log(self.name, "FAIL: vault could not persist peer keys");
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    if update.tx_sealed {
                        self.try_tx_selected_sealed();
                    }
                    if !self.render_messages_snapshot() {
                        self.messages = old_messages;
                        log(self.name, "FAIL: could not render peer keys");
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                }
            } else {
                let old_notes = self.notes;
                let update = {
                    let view = unsafe { self.app_view.as_mut() };
                    self.notes.apply_key(event.code, event.value, view)
                };
                self.publish_notes_touch_mode();
                if update.changed {
                    if update.persist && !self.persist_notes() {
                        self.notes = old_notes;
                        self.render_notes_snapshot();
                        self.publish_notes_touch_mode();
                        log(self.name, "FAIL: vault could not persist Notes collection");
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                }
            }
        }
        // Hot-path EVENT/VIEW UART logs removed — they stalled the guest cursor
        // on every UTM keystroke. Frame compares still cover routing.
        let _ = sequence;
        MessageInfo::new(REPLY_ROUTED_EVENT_OK, 0)
    }

    fn ensure_loaded(&mut self) -> bool {
        if self.vault_generation.is_some() {
            return true;
        }
        if self.name == "CONTACTS" {
            return self.ensure_contacts_loaded();
        }
        if self.name == "NOTES" {
            return self.ensure_notes_loaded();
        }
        if self.name == "CALENDAR" {
            return self.ensure_calendar_loaded();
        }
        if self.name == "MESSAGES" {
            return self.ensure_messages_loaded();
        }
        log(self.name, "FAIL: unknown app namespace");
        false
    }

    fn ensure_notes_loaded(&mut self) -> bool {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
        if reply.label() == REPLY_OK && reply.count() == 2 {
            let generation = get_mr(0);
            let length = get_mr(1) as usize;
            let page = unsafe { self.blob_transfer.as_ref() };
            let Some(payload) = page.payload() else {
                log(
                    self.name,
                    "FAIL: vault returned an invalid Notes payload length",
                );
                return false;
            };
            if payload.len() != length {
                log(self.name, "FAIL: vault returned an invalid Notes payload");
                return false;
            }
            self.vault_generation = Some(generation);
            if let Some(notes) = NotesApp::decode(payload) {
                self.notes = notes;
                self.render_notes_snapshot();
                self.publish_notes_touch_mode();
                return true;
            }
            if let Some(notes) = NotesApp::from_v1_collection(payload) {
                self.notes = notes;
                self.render_notes_snapshot();
                self.publish_notes_touch_mode();
                if !self.persist_notes() {
                    log(self.name, "FAIL: could not migrate PNT1 Notes collection");
                    return false;
                }
                return true;
            }
            let Some(notes) = NotesApp::from_legacy_text(payload) else {
                log(
                    self.name,
                    "FAIL: Notes blob is neither a collection nor legacy text",
                );
                return false;
            };
            self.notes = notes;
            self.render_notes_snapshot();
            self.publish_notes_touch_mode();
            if !self.persist_notes() {
                log(self.name, "FAIL: could not migrate legacy Notes blob");
                return false;
            }
            return true;
        }
        if reply.label() != REPLY_NO_BLOB || reply.count() != 0 {
            log(self.name, "FAIL: could not load persisted Notes collection");
            return false;
        }

        let Some((generation, value)) = read_record() else {
            log(self.name, "FAIL: could not read legacy Notes field");
            return false;
        };
        let view = unsafe { self.app_view.as_mut() };
        if !view.restore_packed(value) && !view.restore_packed(INITIAL_VALUE) {
            log(self.name, "FAIL: legacy Notes field is invalid");
            return false;
        }
        let length = view.length as usize;
        let Some(notes) = NotesApp::from_legacy_text(&view.text[..length]) else {
            log(self.name, "FAIL: legacy Notes text is invalid");
            return false;
        };
        self.notes = notes;
        self.vault_generation = Some(generation);
        self.render_notes_snapshot();
        self.publish_notes_touch_mode();
        if !self.persist_notes() {
            log(self.name, "FAIL: could not migrate legacy Notes field");
            return false;
        }
        true
    }

    fn ensure_contacts_loaded(&mut self) -> bool {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
        if reply.label() == REPLY_OK && reply.count() == 2 {
            let generation = get_mr(0);
            let length = get_mr(1) as usize;
            let page = unsafe { self.blob_transfer.as_ref() };
            let Some(payload) = page.payload() else {
                log(
                    self.name,
                    "FAIL: vault returned an invalid contacts payload length",
                );
                return false;
            };
            if payload.len() != length {
                log(
                    self.name,
                    "FAIL: vault returned an invalid contacts payload",
                );
                return false;
            }
            self.vault_generation = Some(generation);
            if let Some(contacts) = ContactsApp::decode(payload) {
                self.contacts = contacts;
                return self.render_contacts_snapshot();
            }
            let Some(contacts) = ContactsApp::from_legacy_name(payload) else {
                log(
                    self.name,
                    "FAIL: contacts blob is neither a list nor a legacy name",
                );
                return false;
            };
            self.contacts = contacts;
            if !self.persist_contacts() {
                log(self.name, "FAIL: could not migrate legacy Contacts blob");
                return false;
            }
            return self.render_contacts_snapshot();
        }
        if reply.label() != REPLY_NO_BLOB || reply.count() != 0 {
            log(self.name, "FAIL: could not load persisted contact list");
            return false;
        }

        let Some((generation, value)) = read_record() else {
            log(self.name, "FAIL: could not read legacy Contacts field");
            return false;
        };
        let view = unsafe { self.app_view.as_mut() };
        if !view.restore_packed(value) && !view.restore_packed(INITIAL_VALUE) {
            log(self.name, "FAIL: legacy Contacts field is invalid");
            return false;
        }
        let name_length = view.length as usize;
        let Some(contacts) = ContactsApp::from_legacy_name(&view.text[..name_length]) else {
            log(self.name, "FAIL: legacy Contacts name is invalid");
            return false;
        };
        self.contacts = contacts;
        self.vault_generation = Some(generation);
        if !self.persist_contacts() {
            log(self.name, "FAIL: could not migrate legacy Contacts field");
            return false;
        }
        self.render_contacts_snapshot()
    }

    fn persist_contacts(&mut self) -> bool {
        let mut payload = [0; CONTACTS_BLOB_CAPACITY];
        let Some(length) = self.contacts.encode(&mut payload) else {
            return false;
        };
        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&payload[..length]) {
            return false;
        }
        let Some(expected_generation) = self.vault_generation else {
            return false;
        };
        let Some(generation) = write_blob(expected_generation, length) else {
            return false;
        };
        self.vault_generation = Some(generation);
        true
    }

    fn render_contacts_snapshot(&mut self) -> bool {
        let mut text = [0; APP_VIEW_TEXT_CAPACITY];
        let length = self.contacts.render(&mut text);
        unsafe { self.app_view.as_mut() }.set_display_text(&text[..length])
    }

    fn ensure_calendar_loaded(&mut self) -> bool {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
        if reply.label() == REPLY_OK && reply.count() == 2 {
            let generation = get_mr(0);
            let length = get_mr(1) as usize;
            let page = unsafe { self.blob_transfer.as_ref() };
            let Some(payload) = page.payload() else {
                log(
                    self.name,
                    "FAIL: vault returned an invalid calendar payload length",
                );
                return false;
            };
            if payload.len() != length {
                log(
                    self.name,
                    "FAIL: vault returned an invalid calendar payload",
                );
                return false;
            }
            self.vault_generation = Some(generation);
            let Some(calendar) = Calendar::decode(payload) else {
                log(self.name, "FAIL: calendar blob is invalid");
                return false;
            };
            self.calendar = calendar;
            return self.render_calendar_snapshot();
        }
        if reply.label() != REPLY_NO_BLOB || reply.count() != 0 {
            log(self.name, "FAIL: could not load persisted reminders");
            return false;
        }

        let Some((generation, _value)) = read_record() else {
            log(self.name, "FAIL: could not read empty calendar namespace");
            return false;
        };
        self.calendar = Calendar::new();
        self.vault_generation = Some(generation);
        if !self.persist_calendar() {
            log(self.name, "FAIL: could not initialize calendar blob");
            return false;
        }
        self.render_calendar_snapshot()
    }

    fn persist_calendar(&mut self) -> bool {
        let mut payload = [0; CALENDAR_BLOB_CAPACITY];
        let Some(length) = self.calendar.encode(&mut payload) else {
            return false;
        };
        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&payload[..length]) {
            return false;
        }
        let Some(expected_generation) = self.vault_generation else {
            return false;
        };
        let Some(generation) = write_blob(expected_generation, length) else {
            return false;
        };
        self.vault_generation = Some(generation);
        true
    }

    fn render_calendar_snapshot(&mut self) -> bool {
        let mut text = [0; APP_VIEW_TEXT_CAPACITY];
        let length = self.calendar.display(&mut text).len();
        unsafe { self.app_view.as_mut() }.set_display_text(&text[..length])
    }

    fn ensure_messages_loaded(&mut self) -> bool {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ_BLOB, 0));
        if reply.label() == REPLY_OK && reply.count() == 2 {
            let generation = get_mr(0);
            let length = get_mr(1) as usize;
            let page = unsafe { self.blob_transfer.as_ref() };
            let Some(payload) = page.payload() else {
                log(
                    self.name,
                    "FAIL: vault returned an invalid messages payload length",
                );
                return false;
            };
            if payload.len() != length {
                log(
                    self.name,
                    "FAIL: vault returned an invalid messages payload",
                );
                return false;
            }
            self.vault_generation = Some(generation);
            match Messages::decode(payload) {
                Some(messages) => {
                    self.messages = messages;
                    return self.render_messages_snapshot();
                }
                None => {
                    // PMS1 or other mismatched blobs re-init empty (no on-disk migration).
                    log(
                        self.name,
                        "LOG: messages blob format mismatch; re-initializing",
                    );
                }
            }
        } else if reply.label() != REPLY_NO_BLOB || reply.count() != 0 {
            log(self.name, "FAIL: could not load persisted peer keys");
            return false;
        }

        if self.vault_generation.is_none() {
            let Some((generation, _value)) = read_record() else {
                log(self.name, "FAIL: could not read empty messages namespace");
                return false;
            };
            self.vault_generation = Some(generation);
        }
        self.messages = Messages::new();
        if !self.persist_messages() {
            log(self.name, "FAIL: could not initialize messages blob");
            return false;
        }
        self.render_messages_snapshot()
    }

    fn persist_messages(&mut self) -> bool {
        let mut payload = [0; MESSAGES_BLOB_CAPACITY];
        let Some(length) = self.messages.encode(&mut payload) else {
            return false;
        };
        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&payload[..length]) {
            return false;
        }
        let Some(expected_generation) = self.vault_generation else {
            return false;
        };
        let Some(generation) = write_blob(expected_generation, length) else {
            return false;
        };
        self.vault_generation = Some(generation);
        true
    }

    fn render_messages_snapshot(&mut self) -> bool {
        let mut text = [0; APP_VIEW_TEXT_CAPACITY];
        let mut length = self.messages.display(&mut text).len();
        if let Some(count) = self.agent_connection_count {
            const LABEL: &[u8] = b"\nSIMPLEX CONNECTIONS ";
            if length + LABEL.len() + 5 < text.len() {
                text[length..length + LABEL.len()].copy_from_slice(LABEL);
                length += LABEL.len();
                length += write_u16_decimal(&mut text[length..], count);
            }
        }
        unsafe { self.app_view.as_mut() }.set_display_text(&text[..length])
    }

    fn persist_notes(&mut self) -> bool {
        let mut payload = [0; NOTES_BLOB_CAPACITY];
        let Some(length) = self.notes.encode(&mut payload) else {
            debug_println!("SLINT NOTES PERSIST FAILED: encode");
            return false;
        };
        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&payload[..length]) {
            debug_println!("SLINT NOTES PERSIST FAILED: transfer length {}", length);
            return false;
        }
        let Some(expected_generation) = self.vault_generation else {
            debug_println!("SLINT NOTES PERSIST FAILED: generation missing");
            return false;
        };
        let Some(generation) = write_blob(expected_generation, length) else {
            debug_println!(
                "SLINT NOTES PERSIST FAILED: vault write generation {} length {}",
                expected_generation,
                length
            );
            return false;
        };
        self.vault_generation = Some(generation);
        true
    }

    fn render_notes_snapshot(&mut self) {
        self.notes.render(unsafe { self.app_view.as_mut() });
    }

    fn publish_notes_touch_mode(&self) {
        if let Some(mode) = self.notes_touch_mode {
            unsafe {
                mode.as_ref()
                    .store(self.notes.touch_mode(), Ordering::Release)
            };
        }
    }
}

fn read_record() -> Option<(MessageRegisterValue, MessageRegisterValue)> {
    let reply = VAULT.pp_call(MessageInfo::new(REQUEST_READ, 0));
    if reply.label() != REPLY_OK || reply.count() != 2 {
        return None;
    }
    Some((get_mr(0), get_mr(1)))
}

fn write_blob(
    expected_generation: MessageRegisterValue,
    length: usize,
) -> Option<MessageRegisterValue> {
    set_mr(0, expected_generation);
    set_mr(1, length as MessageRegisterValue);
    let reply = VAULT.pp_call(MessageInfo::new(REQUEST_WRITE_BLOB, 2));
    if reply.label() != REPLY_OK || reply.count() != 1 {
        return None;
    }
    Some(get_mr(0))
}

fn log(name: &str, result: &str) {
    debug_println!("NAMESPACE|{}|{}", name, result);
}
