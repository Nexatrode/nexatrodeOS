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
use crate::simplex_ipc::{AgentPage, InvitationRequest, MessageOutbox, Operation, PayloadReader, Status};
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
use personal_os_rust_server::messages::{
    ARCHIVE_CAPACITY, CHAT_SLOT_COUNT, ChatTranscripts, Messages, decode_archive,
    encode_archive, prefix_message_time, FILE_REFERENCE_PREFIX, PHOTO_REFERENCE_PREFIX,
};
use personal_os_rust_server::messages_net_tx::MessagesNetTxPage;
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;

#[cfg(feature = "simplex-agent")]
#[path = "messages_history.rs"]
mod messages_history;
#[cfg(feature = "simplex-agent")]
use personal_os_rust_server::vault_history::{self, Event, MessageKey};

const VAULT: Channel = Channel::new(0);
#[cfg(feature = "simplex-agent")]
const IMAGE_CAP: usize = crate::vault_protocol::PHOTO_MAX;
#[cfg(feature = "simplex-agent")]
const PEER_IMAGE_CAP: usize = 12 * 1024;
#[cfg(feature = "simplex-agent")]
use crate::vault_protocol::{PHOTO_CHUNK, REQUEST_PHOTO_STAGE, REQUEST_PHOTO_COMMIT, REQUEST_PHOTO_READ};
// Only the serialized Messages handler uses these buffers. Long messages do
// not grow the Microkit service stack or the stack frames of sibling apps.
#[cfg(feature = "simplex-agent")]
static mut DIRECT_BODY: [u8; vault_history::DIRECT_TEXT_LIMIT] = [0; vault_history::DIRECT_TEXT_LIMIT];
#[cfg(feature = "simplex-agent")]
static mut DIRECT_LINE: [u8; vault_history::DIRECT_LINE_LIMIT] = [0; vault_history::DIRECT_LINE_LIMIT];
#[cfg(feature = "simplex-agent")]
static mut MESSAGE_PREVIEW: [u8; IMAGE_CAP] = [0; IMAGE_CAP];
#[cfg(feature = "simplex-agent")]
static mut INCOMING_PHOTO: [u8; IMAGE_CAP] = [0; IMAGE_CAP];
#[cfg(feature = "simplex-agent")]
static mut INCOMING_PHOTO_LEN: usize = 0;
#[cfg(feature = "simplex-agent")]
static mut INCOMING_PHOTO_HANDLE: u64 = 0;
#[cfg(feature = "simplex-agent")]
static mut RECEIVED_ATTACHMENT_ID: Option<u64> = None;
#[cfg(feature = "simplex-agent")]
use personal_os_rust_server::messages::ReceivedAttachment;
#[cfg(feature = "simplex-agent")]
static mut PEER_PHOTOS: [[u8; PEER_IMAGE_CAP]; CHAT_SLOT_COUNT] = [[0; PEER_IMAGE_CAP]; CHAT_SLOT_COUNT];
#[cfg(feature = "simplex-agent")]
static mut PEER_PHOTO_LEN: [usize; CHAT_SLOT_COUNT] = [0; CHAT_SLOT_COUNT];
#[cfg(feature = "simplex-agent")]
const GROUP_URI_CAP: usize = 2048;
#[cfg(feature = "simplex-agent")]
static mut GROUP_INVITE_URI: [u8; GROUP_URI_CAP] = [0; GROUP_URI_CAP];
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
const REQUEST_WORKER_RECEIVE_STATUS: MessageLabel = 0x399;
#[cfg(feature = "utm-transcript-worker")]
const REQUEST_WORKER_SELECT_OWNER: MessageLabel = 0x39b;
const REQUEST_SLINT_MESSAGES_ACK: MessageLabel = 0x363;
const REPLY_SLINT_MESSAGES_ACK: MessageLabel = 0x364;
const REQUEST_SLINT_MESSAGES_ACK_STATUS: MessageLabel = 0x365;
const REPLY_SLINT_MESSAGES_ACK_STATUS: MessageLabel = 0x366;
const REQUEST_SLINT_MESSAGES_SELECT_CHAT: MessageLabel = 0x367;
const REQUEST_SLINT_MESSAGES_TRANSCRIPT: MessageLabel = 0x369;
const REPLY_SLINT_MESSAGES_TRANSCRIPT: MessageLabel = 0x36a;
const REQUEST_SLINT_MESSAGES_IMAGE: MessageLabel = 0x36b;
const REPLY_SLINT_MESSAGES_IMAGE: MessageLabel = 0x36c;
const REQUEST_SLINT_MESSAGES_IMAGE_CHUNK: MessageLabel = 0x36d;
const REPLY_SLINT_MESSAGES_IMAGE_CHUNK: MessageLabel = 0x36e;
const REQUEST_SLINT_MESSAGES_GROUP: MessageLabel = 0x36f;
const REPLY_SLINT_MESSAGES_GROUP: MessageLabel = 0x370;
const REQUEST_SLINT_MESSAGES_DELETE: MessageLabel = 0x371;
const REPLY_SLINT_MESSAGES_DELETE: MessageLabel = 0x372;
const REQUEST_SLINT_MESSAGES_ARCHIVE: MessageLabel = 0x373;
const REPLY_SLINT_MESSAGES_ARCHIVE: MessageLabel = 0x374;
const REQUEST_SLINT_MESSAGES_ARCHIVED_MASK: MessageLabel = 0x375;
const REPLY_SLINT_MESSAGES_ARCHIVED_MASK: MessageLabel = 0x376;
const REQUEST_SLINT_MESSAGES_PEER_PHOTO: MessageLabel = 0x377;
const REPLY_SLINT_MESSAGES_PEER_PHOTO: MessageLabel = 0x378;
const REQUEST_SLINT_MESSAGES_PEER_PHOTO_CHUNK: MessageLabel = 0x379;
const REPLY_SLINT_MESSAGES_PEER_PHOTO_CHUNK: MessageLabel = 0x37a;
const REQUEST_SLINT_MESSAGES_SET_PROFILE: MessageLabel = 0x37b;
const REPLY_SLINT_MESSAGES_SET_PROFILE: MessageLabel = 0x37c;
const REQUEST_SLINT_MESSAGES_CREATE_GROUP: MessageLabel = 0x37d;
/// Send a saved JPEG: MR0=source chat index, MR1=photo ID. Destination is the selected chat.
const REQUEST_SLINT_MESSAGES_SEND_SAVED_PHOTO: MessageLabel = 0x387;
const REPLY_SLINT_MESSAGES_SEND_SAVED_PHOTO: MessageLabel = 0x388;
const REQUEST_SLINT_LIBRARY_CLONE: MessageLabel = 0x389;
const REPLY_SLINT_LIBRARY_CLONE: MessageLabel = 0x38a;
const REQUEST_SLINT_LIBRARY_NEXT: MessageLabel = 0x38b;
const REPLY_SLINT_LIBRARY_NEXT: MessageLabel = 0x38c;
const REQUEST_SLINT_LIBRARY_READ: MessageLabel = 0x38d;
const REPLY_SLINT_LIBRARY_READ: MessageLabel = 0x38e;
const REQUEST_SLINT_LIBRARY_DELETE: MessageLabel = 0x38f;
const REPLY_SLINT_LIBRARY_DELETE: MessageLabel = 0x390;
const REQUEST_SLINT_LIBRARY_STAGE: MessageLabel = 0x391;
const REPLY_SLINT_LIBRARY_STAGE: MessageLabel = 0x392;
const REQUEST_SLINT_LIBRARY_IMPORT: MessageLabel = 0x393;
const REPLY_SLINT_LIBRARY_IMPORT: MessageLabel = 0x394;
const REPLY_SLINT_MESSAGES_CREATE_GROUP: MessageLabel = 0x37e;
const REQUEST_SLINT_MESSAGES_CREATE_ADDRESS: MessageLabel = 0x37f;
const REPLY_SLINT_MESSAGES_CREATE_ADDRESS: MessageLabel = 0x380;
const REQUEST_SLINT_MESSAGES_ADDRESS: MessageLabel = 0x381;
const REPLY_SLINT_MESSAGES_ADDRESS: MessageLabel = 0x382;
const REQUEST_SLINT_HISTORY_INFO: MessageLabel = 0x383;
const REPLY_SLINT_HISTORY_INFO: MessageLabel = 0x384;
const REQUEST_SLINT_HISTORY_PAGE: MessageLabel = 0x385;
const REPLY_SLINT_HISTORY_PAGE: MessageLabel = 0x386;
#[cfg(feature = "utm-transcript-worker")]
const REQUEST_SLINT_HISTORY_ADVANCE: MessageLabel = 0x395;
#[cfg(feature = "utm-transcript-worker")]
const REPLY_SLINT_HISTORY_PENDING: MessageLabel = 0x396;
const REQUEST_SLINT_HISTORY_PAGE_STEP: MessageLabel = 0x397;
const REPLY_SLINT_MESSAGES_SELECT_CHAT: MessageLabel = 0x368;
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
    #[cfg(feature = "utm-transcript-worker")]
    slint_transfer: Option<NonNull<BlobTransferPage>>,
    #[cfg(feature = "utm-transcript-worker")]
    transcript_transfer: Option<NonNull<BlobTransferPage>>,
    messages_net_tx: Option<NonNull<MessagesNetTxPage>>,
    vault_generation: Option<MessageRegisterValue>,
    contacts: ContactsApp,
    notes: NotesApp,
    calendar: Calendar,
    messages: Messages,
    chat_transcripts: ChatTranscripts,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_page: Option<NonNull<AgentPage>>,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_request_started: bool,
    #[cfg(feature = "simplex-agent")]
    invitation_request: InvitationRequest,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_join_probe_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_invitation_uri: [u8; SIMPLEX_INVITATION_URI_CAPACITY],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_invitation_uri_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_connection_handles: [u64; SIMPLEX_MAX_CONNECTION_HANDLES],
    #[cfg(feature = "simplex-agent")]
    slint_displayed_handles: [u64; CHAT_SLOT_COUNT],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_connection_handles_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_metadata_refresh: crate::simplex_ipc::MetadataRefresh,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_contact_name: [u8; 32],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_contact_name_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_profile_name: [u8; 48],
    #[cfg(feature = "simplex-agent")]
    simplex_profile_name_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_chat_names: [[u8; 32]; CHAT_SLOT_COUNT],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_chat_name_lengths: [usize; CHAT_SLOT_COUNT],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_chat_count: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_selected_handle: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_selected_index: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_outbox: MessageOutbox,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_join_status: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_join_handle: u64,
    #[cfg(feature = "simplex-agent")]
    upload_phase: u8,
    #[cfg(feature = "simplex-agent")]
    upload_photo_id: u64,
    #[cfg(feature = "simplex-agent")]
    upload_name: [u8; 120],
    #[cfg(feature = "simplex-agent")]
    upload_name_len: usize,
    #[cfg(feature = "simplex-agent")]
    upload_is_photo: bool,
    #[cfg(feature = "simplex-agent")]
    upload_target: u64,
    #[cfg(feature = "simplex-agent")]
    upload_total: usize,
    #[cfg(feature = "simplex-agent")]
    upload_offset: usize,
    #[cfg(feature = "simplex-agent")]
    upload_chunk_len: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_send_status: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_send_message_id: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_image_len: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_image_filled: usize,
    #[cfg(feature = "simplex-agent")]
    received_attachment: ReceivedAttachment,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_image_chat: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_image_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_group_len: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_group_chat: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_group_mask: u16,
    simplex_agent_group_create_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_address_request_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_address_uri: [u8; SIMPLEX_INVITATION_URI_CAPACITY],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_address_uri_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_photo_mask: u16,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_peer_photo_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_peer_photo_chat: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_peer_photo_filled: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_status: u64,
    #[cfg(feature = "simplex-agent")]
    worker_receive_pending: personal_os_rust_server::receive_handoff::ReceiveHandoff,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_handle: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_chat_handle: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_message_id: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_part: u16,
    #[cfg(feature = "simplex-agent")]
    direct_kind: u8,
    #[cfg(feature = "simplex-agent")]
    direct_total: usize,
    #[cfg(feature = "simplex-agent")]
    direct_filled: usize,
    #[cfg(feature = "simplex-agent")]
    direct_id: [u8; 64],
    #[cfg(feature = "simplex-agent")]
    direct_id_len: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_body: [u8; 2048],
    #[cfg(feature = "simplex-agent")]
    simplex_agent_receive_length: usize,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_ack_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_ack_status: u64,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_delete_started: bool,
    #[cfg(feature = "simplex-agent")]
    simplex_agent_delete_chat: usize,
    agent_connection_count: Option<u16>,
    #[cfg(feature = "utm-simplex-session-test")]
    fixture_receive: Option<(u64, usize, [u8; 2048])>,
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
            #[cfg(feature = "utm-transcript-worker")]
            slint_transfer: None,
            #[cfg(feature = "utm-transcript-worker")]
            transcript_transfer: None,
            messages_net_tx,
            vault_generation: None,
            contacts: ContactsApp::new(),
            notes: NotesApp::new(),
            calendar: Calendar::new(),
            messages: Messages::new(),
            chat_transcripts: ChatTranscripts::new(),
            #[cfg(feature = "simplex-agent")]
            simplex_agent_page: None,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_request_started: false,
            #[cfg(feature = "simplex-agent")]
            invitation_request: InvitationRequest::Idle,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_join_probe_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_invitation_uri: [0; SIMPLEX_INVITATION_URI_CAPACITY],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_invitation_uri_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_connection_handles: [0; SIMPLEX_MAX_CONNECTION_HANDLES],
            #[cfg(feature = "simplex-agent")]
            slint_displayed_handles: [0; CHAT_SLOT_COUNT],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_connection_handles_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_metadata_refresh: crate::simplex_ipc::MetadataRefresh::new(),
            #[cfg(feature = "simplex-agent")]
            simplex_agent_contact_name: [0; 32],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_contact_name_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_profile_name: [0; 48],
            #[cfg(feature = "simplex-agent")]
            simplex_profile_name_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_chat_names: [[0; 32]; CHAT_SLOT_COUNT],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_chat_name_lengths: [0; CHAT_SLOT_COUNT],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_chat_count: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_selected_handle: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_selected_index: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_outbox: MessageOutbox::new(),
            #[cfg(feature = "simplex-agent")]
            simplex_agent_join_status: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_join_handle: 0,
            #[cfg(feature = "simplex-agent")]
            upload_phase: 0,
            #[cfg(feature = "simplex-agent")]
            upload_photo_id: 0,
            #[cfg(feature = "simplex-agent")]
            upload_name: [0; 120],
            #[cfg(feature = "simplex-agent")]
            upload_name_len: 0,
            #[cfg(feature = "simplex-agent")]
            upload_is_photo: false,
            #[cfg(feature = "simplex-agent")]
            upload_target: 0,
            #[cfg(feature = "simplex-agent")]
            upload_total: 0,
            #[cfg(feature = "simplex-agent")]
            upload_offset: 0,
            #[cfg(feature = "simplex-agent")]
            upload_chunk_len: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_send_status: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_send_message_id: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_image_len: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_image_filled: 0,
            #[cfg(feature = "simplex-agent")]
            received_attachment: ReceivedAttachment::Preview,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_image_chat: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_image_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_group_len: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_group_chat: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_group_mask: 0,
            simplex_agent_group_create_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_address_request_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_address_uri: [0; SIMPLEX_INVITATION_URI_CAPACITY],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_address_uri_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_photo_mask: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_peer_photo_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_peer_photo_chat: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_peer_photo_filled: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_status: 0,
            #[cfg(feature = "simplex-agent")]
            worker_receive_pending: personal_os_rust_server::receive_handoff::ReceiveHandoff::new(),
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_handle: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_chat_handle: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_message_id: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_part: 0,
            #[cfg(feature = "simplex-agent")]
            direct_kind: 0,
            #[cfg(feature = "simplex-agent")]
            direct_total: 0,
            #[cfg(feature = "simplex-agent")]
            direct_filled: 0,
            #[cfg(feature = "simplex-agent")]
            direct_id: [0; 64],
            #[cfg(feature = "simplex-agent")]
            direct_id_len: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_body: [0; 2048],
            #[cfg(feature = "simplex-agent")]
            simplex_agent_receive_length: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_ack_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_ack_status: 0,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_delete_started: false,
            #[cfg(feature = "simplex-agent")]
            simplex_agent_delete_chat: 0,
            agent_connection_count: None,
            #[cfg(feature = "utm-simplex-session-test")]
            fixture_receive: None,
        }
    }

    #[cfg(feature = "simplex-agent")]
    pub fn set_simplex_agent_page(&mut self, page: NonNull<AgentPage>) {
        self.simplex_agent_page = Some(page);
    }

    #[cfg(feature = "simplex-agent")]
    fn refresh_pending_connection_metadata(&mut self) {
        let pending = self.simplex_agent_connection_handles_length > self.simplex_agent_chat_count;
        let busy = self.agent_busy();
        if pending {
            if let Some(now) = Self::read_message_clock() {
                if self.simplex_metadata_refresh.due(true, busy, now) {
                    self.request_agent_connection_snapshot();
                }
            }
        } else { self.simplex_metadata_refresh.due(false, busy, 0); }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_connection_snapshot(&mut self) {
        if self.simplex_agent_request_started || self.agent_busy() {
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
    fn poll_agent_list_response(&mut self) {
        if !self.simplex_agent_request_started {
            return;
        }
        let ready = self
            .simplex_agent_page
            .and_then(|page| unsafe { page.as_ref() }.response(1))
            .is_some();
        if !ready {
            return;
        }
        self.receive_agent_connection_snapshot();
        // Do not touch the vault here: Slint LOAD already holds large
        // stack frames, and a nested ensure_messages_loaded overflowed the
        // Messages PD stack.
        self.settle_agent(true, true);
        if !self.agent_busy() {
            self.begin_peer_photo_fetch();
        }
    }

    /// Slint talks to Messages over protected calls. Those can run for a long
    /// time without entering `notified`, so any finished Agent reply must also
    /// be taken from the Slint entry path.
    #[cfg(feature = "simplex-agent")]
    fn poll_agent_page(&mut self) {
        let Some(page) = self.simplex_agent_page else {
            return;
        };
        let page = unsafe { page.as_ref() };
        if self.simplex_agent_join_probe_started && page.response(3).is_some() {
            self.receive_agent_join_invitation();
        } else if self.upload_phase == 1 && page.response(13).is_some() {
            self.receive_upload_chunk();
        } else if self.upload_phase == 2 && page.response(14).is_some() {
            self.receive_upload_send();
        } else if self.simplex_agent_outbox.in_flight.is_some() && page.response(4).is_some() {
            self.receive_agent_send_message();
        } else if self.simplex_agent_receive_started && page.response(5).is_some() {
            self.receive_agent_next();
        } else if self.simplex_agent_ack_started && page.response(6).is_some() {
            self.receive_agent_acknowledge();
        } else if self.simplex_agent_delete_started && page.response(7).is_some() {
            self.receive_agent_delete_connection();
        } else if self.invitation_request == InvitationRequest::InFlight && page.response(2).is_some() {
            self.receive_agent_invitation();
        } else if self.simplex_agent_request_started && page.response(1).is_some() {
            self.poll_agent_list_response();
        } else if self.simplex_agent_image_started && page.response(8).is_some() {
            self.receive_image_chunk();
        } else if self.simplex_agent_peer_photo_started && page.response(9).is_some() {
            self.receive_peer_image_chunk();
            if !self.agent_busy() {
                self.settle_agent(true, true);
            }
        } else if self.simplex_agent_group_create_started && page.response(11).is_some() {
            self.receive_agent_create_group();
        } else if self.simplex_agent_address_request_started && page.response(12).is_some() {
            self.receive_agent_address();
        } else if !self.agent_busy() && self.agent_connection_count.is_some() {
            // Do not ReceiveNext before the first ListConnections: that marks
            // the page busy and skips the snapshot that carries photo_mask.
            self.settle_agent(true, true);
            if !self.agent_busy() {
                self.begin_peer_photo_fetch();
            }
        }
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
                // Named chats follow the handles, newest profile first. Each
                // name belongs to the handle at the same index.
                let Some(chat_count) = reader.u8().map(usize::from) else {
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                    return;
                };
                if chat_count > CHAT_SLOT_COUNT {
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                    return;
                }
                let mut chat_names = [[0u8; 32]; CHAT_SLOT_COUNT];
                let mut chat_name_lengths = [0usize; CHAT_SLOT_COUNT];
                for index in 0..chat_count {
                    let Some(name_length) = reader.u8().map(usize::from) else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    };
                    if name_length == 0 || name_length > chat_names[index].len() {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    }
                    for slot in &mut chat_names[index][..name_length] {
                        let Some(byte) = reader.u8() else {
                            debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                            return;
                        };
                        if !(byte == b' '
                            || byte.is_ascii_alphanumeric()
                            || matches!(byte, b'-' | b'.' | b'_'))
                        {
                            debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                            return;
                        }
                        *slot = byte;
                    }
                    chat_name_lengths[index] = name_length;
                }
                self.simplex_agent_group_len = 0;
                // List ends with group_mask+photo_mask and the 16 handles from
                // the row order used before contacts were listed first.
                // An optional group invite sits ahead of that tail.
                const LEGACY_HANDLES: usize = SIMPLEX_MAX_CONNECTION_HANDLES * 8;
                if reader.remaining() > 4 + LEGACY_HANDLES {
                    let Some(index) = reader.u8().map(usize::from) else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    };
                    let Some(uri) = reader.bytes() else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    };
                    if index >= chat_count
                        || uri.is_empty()
                        || uri.len() > GROUP_URI_CAP
                        || !uri.is_ascii()
                    {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    }
                    unsafe {
                        GROUP_INVITE_URI[..uri.len()].copy_from_slice(uri);
                    }
                    self.simplex_agent_group_len = uri.len();
                    self.simplex_agent_group_chat = index;
                }
                let group_mask = if reader.remaining() >= 2 {
                    let Some(mask) = reader.u16() else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    };
                    mask
                } else {
                    0
                };
                let photo_mask = if reader.remaining() >= 2 {
                    let Some(mask) = reader.u16() else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    };
                    mask
                } else {
                    0
                };
                let mut legacy_handles = [0u64; SIMPLEX_MAX_CONNECTION_HANDLES];
                if reader.remaining() < LEGACY_HANDLES {
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                    return;
                }
                for handle in &mut legacy_handles {
                    let Some(value) = reader.u64() else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                        return;
                    };
                    *handle = value;
                }
                if !reader.is_empty() {
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE");
                    return;
                }
                self.simplex_agent_chat_names = chat_names;
                self.simplex_agent_chat_name_lengths = chat_name_lengths;
                self.simplex_agent_chat_count = chat_count;
                self.simplex_agent_group_mask = group_mask;
                self.simplex_agent_photo_mask = photo_mask;
                self.simplex_agent_contact_name = chat_names[0];
                self.simplex_agent_contact_name_length = chat_name_lengths[0];
                if self.simplex_agent_selected_handle != 0
                    && !handles[..count].contains(&self.simplex_agent_selected_handle)
                {
                    self.simplex_agent_selected_handle = 0;
                }
                debug_println!(
                    "SIMPLEXIPC|MESSAGES_APP|CHAT_COUNT|{}|GROUP_MASK|{}|PHOTO_MASK|{}",
                    chat_count,
                    group_mask,
                    photo_mask
                );
                self.agent_connection_count = Some(count as u16);
                self.simplex_agent_connection_handles = handles;
                self.simplex_agent_connection_handles_length = count;
                if self.ensure_messages_loaded() && self.chat_transcripts.rehome(&handles, &legacy_handles)
                {
                    if self.persist_messages() {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|TRANSCRIPT_REHOME");
                    } else {
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|TRANSCRIPT_REHOME_PERSIST_FAIL");
                    }
                    debug_println!(
                        "SIMPLEXIPC|MESSAGES_APP|TRANSCRIPT_SLOT|{}|{}",
                        self.chat_transcripts.text(0).len(),
                        self.chat_transcripts.text(1).len()
                    );
                }
                debug_println!(
                    "SIMPLEXIPC|MESSAGES_APP|LIST_CONNECTIONS_PASS|{}",
                    self.agent_connection_count.unwrap_or(0)
                );
                // Push CHAT lines into the Slint snapshot now so the list and
                // unread badges are not stuck at the empty boot load.
                if self.render_messages_snapshot() {
                    self.notify_chat_ui();
                }
            }
            _ => debug_println!("SIMPLEXIPC|MESSAGES_APP|FAIL_RESPONSE"),
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_set_profile(&mut self, name: &[u8]) -> bool {
        if self.agent_busy() {
            return false;
        }
        let Some(mut page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|PROFILE_FAIL_NO_PAGE");
            return false;
        };
        if !unsafe { page.as_mut() }.begin(Operation::SetProfile, 10, name) {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|PROFILE_FAIL_ENCODE");
            return false;
        }
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        debug_println!("SIMPLEXIPC|MESSAGES_APP|PROFILE_REQUEST|{}", name.len());
        true
    }

    /// Invite the selected direct contact into a new group. The agent sends
    /// x.grp.inv on that contact and lists the group once the reply lands.
    #[cfg(feature = "simplex-agent")]
    fn request_agent_create_group(&mut self, name: &[u8], contact: usize) -> bool {
        if self.agent_busy() || name.is_empty() || name.len() > 32 {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_GROUP_BUSY");
            return false;
        }
        let chats = self.simplex_agent_chat_count.min(CHAT_SLOT_COUNT);
        let Some(handle) = crate::simplex_ipc::group_invitee(
            &self.simplex_agent_connection_handles[..self.simplex_agent_connection_handles_length],
            chats, self.simplex_agent_group_mask, contact,
        ) else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_GROUP_NO_CONTACT");
            return false;
        };
        let mut payload = [0u8; 42];
        payload[..8].copy_from_slice(&handle.to_le_bytes());
        let len = name.len() as u16;
        payload[8..10].copy_from_slice(&len.to_le_bytes());
        payload[10..10 + name.len()].copy_from_slice(name);
        let Some(mut page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_GROUP_NO_PAGE");
            return false;
        };
        if !unsafe { page.as_mut() }
            .begin(Operation::CreateGroup, 11, &payload[..10 + name.len()])
        {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_GROUP_ENCODE");
            return false;
        }
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        self.simplex_agent_group_create_started = true;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_GROUP_REQUEST|{}", name.len());
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_create_group(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_group_create_started = false;
        let Some(page) = self.simplex_agent_page else {
            return;
        };
        match unsafe { page.as_ref() }.response(11) {
            Some((Status::Ok, _)) => {
                debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_GROUP_PASS");
                self.request_agent_connection_snapshot();
            }
            _ => debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_GROUP_FAIL"),
        }
    }

    /// Create the reusable contact address, or return the one already stored
    /// by the agent. The 1-time invitation uses a different operation.
    #[cfg(feature = "simplex-agent")]
    fn request_agent_address(&mut self) -> bool {
        if self.simplex_agent_address_uri_length != 0 {
            return true;
        }
        if self.agent_busy() {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|ADDRESS_BUSY");
            return false;
        }
        let Some(mut page) = self.simplex_agent_page else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|ADDRESS_FAIL_NO_PAGE");
            return false;
        };
        if !unsafe { page.as_mut() }.begin(Operation::CreateAddress, 12, &[]) {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|ADDRESS_FAIL_ENCODE");
            return false;
        }
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        self.simplex_agent_address_request_started = true;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|ADDRESS_REQUEST_SENT");
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_address(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_address_request_started = false;
        let Some(page) = self.simplex_agent_page else {
            return;
        };
        match unsafe { page.as_ref() }.response(12) {
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
                        self.simplex_agent_address_uri[..uri.len()].copy_from_slice(uri);
                        self.simplex_agent_address_uri_length = uri.len();
                        debug_println!(
                            "SIMPLEXIPC|MESSAGES_APP|ADDRESS_PASS|{}",
                            uri.len()
                        );
                        return;
                    }
                }
                debug_println!("SIMPLEXIPC|MESSAGES_APP|ADDRESS_FAIL");
            }
            _ => debug_println!("SIMPLEXIPC|MESSAGES_APP|ADDRESS_FAIL"),
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn start_queued_invitation(&mut self) -> bool {
        if self.invitation_request != InvitationRequest::Queued || self.agent_busy() {
            return false;
        }
        let Some(mut page) = self.simplex_agent_page else {
            self.invitation_request = InvitationRequest::Idle;
            debug_println!("SIMPLEXIPC|MESSAGES_APP|INVITATION_FAIL_NO_PAGE");
            return false;
        };
        if !self.invitation_request.begin(unsafe { page.as_mut() }) {
            return false;
        }
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        debug_println!("SIMPLEXIPC|MESSAGES_APP|CREATE_INVITATION_REQUEST_SENT");
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_invitation(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        let Some(page) = self.simplex_agent_page else {
            self.invitation_request = InvitationRequest::Idle;
            debug_println!("SIMPLEXIPC|MESSAGES_APP|INVITATION_FAIL_NO_PAGE");
            return;
        };
        match self.invitation_request.take_response(unsafe { page.as_ref() }) {
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
        self.simplex_agent_join_probe_started = true;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        debug_println!("SIMPLEXIPC|MESSAGES_APP|JOIN_INVITATION_REQUEST_SENT");
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_join_invitation(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_join_probe_started = false;
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
                        self.simplex_agent_group_len = 0;
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
    fn chat_index_for_handle(&self, handle: u64) -> u64 {
        let named = self
            .simplex_agent_chat_count
            .min(CHAT_SLOT_COUNT)
            .min(self.simplex_agent_connection_handles_length);
        self.simplex_agent_connection_handles[..named]
            .iter()
            .position(|candidate| *candidate == handle)
            .map(|index| index as u64)
            .unwrap_or(255)
    }

    /// Receipts and XFTP failures identify the original outgoing offer by contact and agent ID.
    #[cfg(feature = "simplex-agent")]
    fn note_send_notice(&mut self, handle: u64, message_id: u64, failed_upload: bool) {
        use core::fmt::Write;
        use personal_os_rust_server::messages::TranscriptBuffer;

        let chat = self.chat_index_for_handle(handle) as usize;
        let Ok(owner) = self.ensure_chat_history(chat) else {
            return;
        };
        let key = MessageKey { source: u64::MAX, number: message_id, part: 0 };
        let Some(record) = (unsafe { messages_history::index() }).record_for(owner, key) else {
            return;
        };
        let Ok(Some((_, Event::Row(_, line)))) =
            messages_history::read(record, unsafe { self.blob_transfer.as_mut() })
        else {
            return;
        };
        let Some((base, suffix)) = personal_os_rust_server::messages::outgoing_notice_update(line, failed_upload) else {
            return;
        };
        let mut bytes = [0; vault_history::MAX_EVENT_SIZE - vault_history::ROW_HEADER];
        let mut updated = TranscriptBuffer::new(&mut bytes);
        if write!(updated, "{base}{suffix}").is_ok() && self.append_history_line(owner, key, updated.as_str()) {
            if failed_upload {
                if self.simplex_agent_send_status == 5 && self.upload_target == handle
                    && self.simplex_agent_send_message_id == message_id {
                    self.simplex_agent_send_status = 7;
                }
                debug_println!("SIMPLEXIPC|MESSAGES_APP|UPLOAD_FAILED|{}|{}", chat, message_id);
            } else {
                debug_println!("SIMPLEXIPC|MESSAGES_APP|DELIVERY_MARKED|{}|{}", chat, message_id);
            }
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_part(reader: &mut PayloadReader<'_>) -> Option<u16> {
        if reader.is_empty() {
            return Some(0);
        }
        reader.u16().filter(|_| reader.is_empty())
    }

    #[cfg(feature = "simplex-agent")]
    fn accept_group_member_message(&mut self, reader: &mut PayloadReader<'_>) {
        let parsed = reader
            .u64()
            .zip(reader.u64())
            .zip(reader.u64())
            .zip(reader.bytes());
        let Some((((handle, chat_handle), message_id), body)) = parsed else {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        };
        let part = Self::receive_part(reader);
        if part.is_none()
            || handle == 0
            || chat_handle == 0
            || message_id == 0
            || body.is_empty()
            || body.len() > self.simplex_agent_receive_body.len()
            || core::str::from_utf8(body).is_err()
        {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        }
        self.simplex_agent_receive_body[..body.len()].copy_from_slice(body);
        self.simplex_agent_receive_length = body.len();
        self.simplex_agent_receive_handle = handle;
        self.simplex_agent_receive_chat_handle = chat_handle;
        self.simplex_agent_receive_message_id = message_id;
        self.simplex_agent_receive_part = part.unwrap_or(0);
        self.simplex_agent_receive_status = 2;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|GROUP_MEMBER");
        self.settle_agent(false, true);
    }

    #[cfg(feature = "simplex-agent")]
    fn accept_group_invite(&mut self, reader: &mut PayloadReader<'_>) {
        let parsed = reader.u64().zip(reader.u64()).zip(reader.bytes()).zip(reader.bytes());
        let Some((((handle, message_id), name), uri)) = parsed else {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        };
        let prefix = b"Group invitation: ";
        let part = Self::receive_part(reader);
        if part.is_none()
            || handle == 0
            || message_id == 0
            || name.is_empty()
            || uri.is_empty()
            || uri.len() > GROUP_URI_CAP
            || !uri.is_ascii()
            || core::str::from_utf8(name).is_err()
            || prefix.len() + name.len() > self.simplex_agent_receive_body.len()
        {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        }
        self.simplex_agent_receive_body[..prefix.len()].copy_from_slice(prefix);
        self.simplex_agent_receive_body[prefix.len()..prefix.len() + name.len()].copy_from_slice(name);
        self.simplex_agent_receive_length = prefix.len() + name.len();
        self.simplex_agent_receive_handle = handle;
        self.simplex_agent_receive_chat_handle = handle;
        self.simplex_agent_receive_message_id = message_id;
        self.simplex_agent_receive_part = part.unwrap_or(0);
        let chat = self.chat_index_for_handle(handle);
        if chat < CHAT_SLOT_COUNT as u64 {
            unsafe {
                GROUP_INVITE_URI[..uri.len()].copy_from_slice(uri);
            }
            self.simplex_agent_group_len = uri.len();
            self.simplex_agent_group_chat = chat as usize;
        }
        self.simplex_agent_receive_status = 2;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|GROUP_INVITE|{}", name.len());
        // Same join path as the JOIN GROUP control: accept the invitation
        // once the URI is stored so groupSelf is established without a
        // second manual paste.
        if !self.agent_busy() {
            self.request_agent_join_invitation(uri);
        }
        self.settle_agent(false, true);
    }

    #[cfg(feature = "simplex-agent")]
    fn store_image_chunk(&mut self, chunk: &[u8]) -> bool {
        let end = self.simplex_agent_image_filled.saturating_add(chunk.len());
        if chunk.is_empty() || end > self.simplex_agent_image_len || end > IMAGE_CAP {
            return false;
        }
        unsafe {
            MESSAGE_PREVIEW[self.simplex_agent_image_filled..end].copy_from_slice(chunk);
        }
        self.simplex_agent_image_filled = end;
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn accept_image_preview(&mut self, reader: &mut PayloadReader<'_>, file: bool) {
        let parsed = (|| {
            let handle = reader.u64()?;
            let message_id = reader.u64()?;
            let caption = reader.bytes()?;
            let short = reader.u16()?;
            let total = if short == 0 { reader.u32()? as usize } else { short as usize };
            let kind = if file { ReceivedAttachment::File } else if short == 0 {
                ReceivedAttachment::Original
            } else { ReceivedAttachment::Preview };
            Some((handle, message_id, caption, total, kind, reader.bytes()?))
        })();
        let Some((handle, message_id, caption, total, kind, chunk)) = parsed else {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        };
        let part = Self::receive_part(reader);
        if part.is_none()
            || handle == 0
            || message_id == 0
            || total > IMAGE_CAP
            || chunk.len() > total
            || core::str::from_utf8(caption).is_err()
        {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        }
        let caption = if caption.is_empty() {
            if file { b"File".as_slice() } else { b"Photo".as_slice() }
        } else {
            caption
        };
        self.received_attachment = kind;
        if caption.len() > self.simplex_agent_receive_body.len() {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        }
        self.simplex_agent_receive_body[..caption.len()].copy_from_slice(caption);
        self.simplex_agent_receive_length = caption.len();
        self.simplex_agent_receive_handle = handle;
        self.simplex_agent_receive_chat_handle = handle;
        self.simplex_agent_receive_message_id = message_id;
        self.simplex_agent_receive_part = part.unwrap_or(0);
        let chat = self.chat_index_for_handle(handle);
        self.simplex_agent_image_chat = if chat < CHAT_SLOT_COUNT as u64 {
            chat as usize
        } else {
            0
        };
        self.simplex_agent_image_len = total;
        self.simplex_agent_image_filled = 0;
        if total == 0 {
            self.finish_image_message();
            return;
        }
        if !self.store_image_chunk(chunk) {
            self.simplex_agent_image_len = 0;
            self.finish_image_message();
            return;
        }
        if self.simplex_agent_image_filled < self.simplex_agent_image_len {
            self.request_image_chunk();
        } else {
            self.finish_image_message();
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_image_chunk(&mut self) {
        let Some(mut page) = self.simplex_agent_page else {
            self.simplex_agent_image_len = 0;
            self.finish_image_message();
            return;
        };
        let offset = (self.simplex_agent_image_filled as u32).to_le_bytes();
        if !unsafe { page.as_mut() }.begin(Operation::ReadImage, 8, &offset) {
            self.simplex_agent_image_len = 0;
            self.finish_image_message();
            return;
        }
        self.simplex_agent_image_started = true;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_image_chunk(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_image_started = false;
        let Some(page) = self.simplex_agent_page else {
            self.simplex_agent_image_len = 0;
            self.finish_image_message();
            return;
        };
        match unsafe { page.as_ref() }.response(8) {
            Some((Status::Ok, chunk)) if self.store_image_chunk(chunk) => {
                if self.simplex_agent_image_filled < self.simplex_agent_image_len {
                    self.request_image_chunk();
                } else {
                    self.finish_image_message();
                }
            }
            _ => {
                self.simplex_agent_image_len = 0;
                self.finish_image_message();
            }
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn begin_peer_photo_fetch(&mut self) {
        if self.simplex_agent_peer_photo_started
            || self.simplex_agent_image_started
            || self.simplex_agent_outbox.in_flight.is_some()
            || self.simplex_agent_receive_started
            || self.simplex_agent_ack_started
            || self.simplex_agent_delete_started
            || self.simplex_agent_request_started
            || self.invitation_request == InvitationRequest::InFlight
            || self.simplex_agent_address_request_started
        {
            return;
        }
        self.advance_peer_photo_fetch(0);
    }

    #[cfg(feature = "simplex-agent")]
    fn advance_peer_photo_fetch(&mut self, from: usize) {
        let mask = self.simplex_agent_photo_mask;
        for index in from..CHAT_SLOT_COUNT {
            if mask & (1u16 << index) == 0 {
                continue;
            }
            if unsafe { PEER_PHOTO_LEN[index] } > 0 {
                continue;
            }
            if index >= self.simplex_agent_chat_count
                || index >= self.simplex_agent_connection_handles_length
            {
                break;
            }
            self.simplex_agent_peer_photo_chat = index;
            self.simplex_agent_peer_photo_filled = 0;
            if self.request_peer_image_chunk() {
                return;
            }
        }
        self.simplex_agent_peer_photo_started = false;
    }

    #[cfg(feature = "simplex-agent")]
    fn request_peer_image_chunk(&mut self) -> bool {
        let Some(mut page) = self.simplex_agent_page else {
            return false;
        };
        let index = self.simplex_agent_peer_photo_chat;
        if index >= self.simplex_agent_connection_handles_length {
            return false;
        }
        let handle = self.simplex_agent_connection_handles[index];
        let filled = self.simplex_agent_peer_photo_filled as u16;
        let mut payload = [0u8; 10];
        payload[..8].copy_from_slice(&handle.to_le_bytes());
        payload[8..10].copy_from_slice(&filled.to_le_bytes());
        if !unsafe { page.as_mut() }.begin(Operation::GetPeerImage, 9, &payload) {
            return false;
        }
        self.simplex_agent_peer_photo_started = true;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_peer_image_chunk(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_peer_photo_started = false;
        let index = self.simplex_agent_peer_photo_chat;
        let Some(page) = self.simplex_agent_page else {
            self.advance_peer_photo_fetch(index + 1);
            return;
        };
        match unsafe { page.as_ref() }.response(9) {
            Some((Status::Ok, chunk)) if !chunk.is_empty() && index < CHAT_SLOT_COUNT => {
                let filled = self.simplex_agent_peer_photo_filled;
                let end = filled.saturating_add(chunk.len());
                if end > PEER_IMAGE_CAP {
                    unsafe {
                        PEER_PHOTO_LEN[index] = 0;
                    }
                    self.advance_peer_photo_fetch(index + 1);
                    return;
                }
                unsafe {
                    PEER_PHOTOS[index][filled..end].copy_from_slice(chunk);
                }
                self.simplex_agent_peer_photo_filled = end;
                if chunk.len() < 3000 {
                    unsafe {
                        PEER_PHOTO_LEN[index] = end;
                    }
                    debug_println!(
                        "SIMPLEXIPC|MESSAGES_APP|PEER_PHOTO_PASS|{}|{}",
                        index,
                        end
                    );
                    self.advance_peer_photo_fetch(index + 1);
                } else if !self.request_peer_image_chunk() {
                    unsafe {
                        PEER_PHOTO_LEN[index] = end;
                    }
                    self.advance_peer_photo_fetch(index + 1);
                }
            }
            _ => {
                self.advance_peer_photo_fetch(index + 1);
            }
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn finish_image_message(&mut self) {
        let len = self.simplex_agent_image_len;
        if len != 0 && len == self.simplex_agent_image_filled
            && (self.received_attachment != ReceivedAttachment::Preview
                || (len >= 4 && unsafe { MESSAGE_PREVIEW[..len].starts_with(&[0xff, 0xd8]) }))
        {
            // The file and its history reference must both commit before acknowledgement.
            unsafe { RECEIVED_ATTACHMENT_ID = Some(0); }
        } else if self.received_attachment != ReceivedAttachment::Preview {
            self.simplex_agent_receive_status = 4;
            self.settle_agent(false, false);
            return;
        }
        self.simplex_agent_receive_status = 2;
        debug_println!(
            "SIMPLEXIPC|MESSAGES_APP|IMAGE_READY|{}|{}",
            self.simplex_agent_image_chat,
            self.simplex_agent_image_len
        );
        self.settle_agent(false, true);
    }

    #[cfg(feature = "simplex-agent")]
    fn restore_photo(&mut self) -> bool {
        let mut length = 0;
        let mut handle = 0;
        for part in 0..IMAGE_CAP.div_ceil(PHOTO_CHUNK) {
            set_mr(0, 0);
            set_mr(1, part as u64);
            let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
            if reply.label() != REPLY_OK || reply.count() != if part == 0 { 2 } else { 0 } {
                return false;
            }
            if part == 0 {
                handle = get_mr(0);
                length = get_mr(1) as usize;
                if handle == 0 || !(4..=IMAGE_CAP).contains(&length) {
                    return false;
                }
            }
            let offset = part * PHOTO_CHUNK;
            let end = length.min(offset + PHOTO_CHUNK);
            let Some(bytes) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                return false;
            };
            if bytes.len() != end - offset || (part == 0 && !bytes.starts_with(&[0xff, 0xd8])) {
                return false;
            }
            unsafe { INCOMING_PHOTO[offset..end].copy_from_slice(bytes); }
            if end == length {
                unsafe {
                    INCOMING_PHOTO_HANDLE = handle;
                    INCOMING_PHOTO_LEN = length;
                }
                debug_println!("SIMPLEXIPC|MESSAGES_APP|PHOTO_RESTORED|{}", length);
                return true;
            }
        }
        false
    }

    #[cfg(feature = "simplex-agent")]
    fn incoming_photo(&mut self) -> Option<(usize, &[u8])> {
        if unsafe { INCOMING_PHOTO_LEN } == 0 {
            self.restore_photo();
        }
        let (len, handle) = unsafe { (INCOMING_PHOTO_LEN, INCOMING_PHOTO_HANDLE) };
        if len != 0 {
            let chat = self.chat_index_for_handle(handle) as usize;
            return (chat < CHAT_SLOT_COUNT).then(|| (chat, unsafe { &INCOMING_PHOTO[..len] }));
        }
        if !self.ensure_messages_loaded() {
            return None;
        }
        self.chat_transcripts.photo()
    }

    #[cfg(feature = "simplex-agent")]
    fn store_message_attachment(&mut self, handle: u64, len: usize) -> Option<u64> {
        for offset in (0..len).step_by(PHOTO_CHUNK) {
            let bytes = unsafe { &MESSAGE_PREVIEW[offset..len.min(offset + PHOTO_CHUNK)] };
            if !unsafe { self.blob_transfer.as_mut() }.set_payload(bytes) {
                return None;
            }
            set_mr(0, offset as u64);
            let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_STAGE, 1));
            if reply.label() != REPLY_OK || reply.count() != 0 {
                return None;
            }
        }
        set_mr(0, handle);
        set_mr(1, len as u64);
        let label = if self.received_attachment == ReceivedAttachment::File
            || !unsafe { MESSAGE_PREVIEW[..len].starts_with(&[0xff, 0xd8]) }
        {
            crate::vault_protocol::REQUEST_ATTACHMENT_COMMIT
        } else {
            REQUEST_PHOTO_COMMIT
        };
        let reply = VAULT.pp_call(MessageInfo::new(label, 2));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|PHOTO_COMMIT_FAIL|{}", reply.label());
            return None;
        }
        let id = get_mr(0);
        if self.received_attachment == ReceivedAttachment::File {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|FILE_DISK|{}|{}", id, len);
        } else {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|PHOTO_DISK|{}|{}", id, len);
        }
        Some(id)
    }

    #[cfg(feature = "simplex-agent")]
    fn incoming_history_key(&self) -> MessageKey {
        MessageKey {
            source: self.simplex_agent_receive_handle,
            number: self.simplex_agent_receive_message_id,
            part: self.simplex_agent_receive_part,
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn ensure_chat_history(&mut self, chat: usize) -> Result<u64, ()> {
        let owner = *self.simplex_agent_connection_handles.get(chat).ok_or(())?;
        if owner == 0 || !self.ensure_messages_loaded() {
            return Err(());
        }
        let legacy = core::str::from_utf8(self.chat_transcripts.text(chat)).map_err(|_| ())?;
        let history = unsafe { messages_history::index() };
        history.refresh(self.blob_transfer)?;
        history.migrate(owner, legacy, self.blob_transfer)?;
        Ok(owner)
    }

    #[cfg(feature = "simplex-agent")]
    fn append_history_line(&mut self, owner: u64, key: MessageKey, line: &str) -> bool {
        let mut bytes = [0; vault_history::MAX_EVENT_SIZE];
        let Some(len) = vault_history::encode_row(key, line, &mut bytes) else {
            return false;
        };
        let saved = unsafe { messages_history::index() }
            .append(owner, &bytes[..len], self.blob_transfer).is_ok();
        if saved {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|HISTORY_SAVED|{}", line.len());
        }
        saved
    }

    #[cfg(feature = "simplex-agent")]
    fn record_direct_event(&mut self, chat: usize, speaker: &[u8]) -> Result<(), ()> {
        use core::fmt::Write;
        use personal_os_rust_server::messages::{TranscriptBuffer, write_direct_text};
        use vault_history::PeerVersion;

        let owner = self.ensure_chat_history(chat)?;
        let delivery = self.incoming_history_key();
        let history = unsafe { messages_history::index() };
        if history.record_for(owner, delivery).is_some() { return Ok(()); }
        let id = core::str::from_utf8(&self.direct_id[..self.direct_id_len]).map_err(|_| ())?;
        let mut version = PeerVersion {
            key: delivery, shared_id: id, revision: (delivery.number, delivery.part), deleted: self.direct_kind == 9,
        };
        let mut line = TranscriptBuffer::new(unsafe { &mut *core::ptr::addr_of_mut!(DIRECT_LINE) });
        let mut found = false;
        if !id.is_empty() {
            for record in history.peer_records(owner, delivery.source, id) {
                let Some((_, Event::Peer(row))) = messages_history::read(record, unsafe { self.blob_transfer.as_mut() })?
                    else { continue };
                if row.version.shared_id != id || row.version.key.source != delivery.source { continue; }
                if self.direct_kind == 7 || !row.version.accepts(delivery, id) { return Ok(()); }
                let (prefix, _) = row.first.split_once(personal_os_rust_server::messages::DIRECT_PREFIX).ok_or(())?;
                line.write_str(prefix).map_err(|_| ())?;
                version.key = row.version.key;
                found = true;
                break;
            }
        }
        if !found {
            // Older rows never stored an authenticated peer SharedMsgId.
            // A display name or text marker cannot establish ownership.
            if self.direct_kind != 7 { return Ok(()); }
            let name = core::str::from_utf8(speaker).map_err(|_| ())?;
            write!(line, "{name}: ").map_err(|_| ())?;
            let mut clock = [0; 6];
            if Self::read_message_clock().and_then(|t| prefix_message_time(t, b"", &mut clock)).is_some() {
                line.write_str(core::str::from_utf8(&clock).map_err(|_| ())?).map_err(|_| ())?;
            }
        }
        let body = if version.deleted { "Message deleted" } else {
            core::str::from_utf8(unsafe { &DIRECT_BODY[..self.direct_total] }).map_err(|_| ())?
        };
        write_direct_text(&mut line, id, body).map_err(|_| ())?;
        vault_history::store_peer_line(version, line.as_str(), |bytes| {
            history.append_record(owner, bytes, self.blob_transfer)
        })?;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|HISTORY_SAVED|{}", line.as_str().len());
        Ok(())
    }

    #[cfg(feature = "simplex-agent")]
    fn record_received_attachment(&mut self, chat: usize, speaker: &[u8], caption: &[u8], id: u64) -> bool {
        use core::fmt::Write;
        use personal_os_rust_server::messages::TranscriptBuffer;

        let key = self.incoming_history_key();
        let Ok(owner) = self.ensure_chat_history(chat) else {
            return false;
        };
        // The shared transfer page is reused by blob storage. Copy the old row
        // before committing so its sender, time and caption survive an upgrade.
        let mut old_line = [0; vault_history::MAX_EVENT_SIZE];
        let mut old_len = 0;
        if let Some(record) = unsafe { messages_history::index() }.record_for(owner, key) {
            let Ok(Some((_, vault_history::Event::Row(_, line)))) =
                messages_history::read(record, unsafe { self.blob_transfer.as_mut() })
            else { return false; };
            old_len = line.len();
            old_line[..old_len].copy_from_slice(line.as_bytes());
        }
        let line = core::str::from_utf8(&old_line[..old_len]).unwrap();
        let old_body = line.split_once(": ").map_or("", |(_, body)| {
            personal_os_rust_server::messages::split_message_time(body).1
        });
        if !self.received_attachment.needs_blob(old_body) {
            return true;
        }
        let id = if id != 0 {
            id
        } else {
            let Some(id) = self.store_message_attachment(
                self.simplex_agent_receive_chat_handle,
                self.simplex_agent_image_len,
            ) else {
                return false;
            };
            unsafe { RECEIVED_ATTACHMENT_ID = Some(id); }
            id
        };
        let mut bytes = [0; vault_history::MAX_EVENT_SIZE - vault_history::ROW_HEADER];
        let mut marked = TranscriptBuffer::new(&mut bytes);
        if old_len != 0 && marked.write_str(&line[..old_len - old_body.len()]).is_err() {
            return false;
        }
        let jpeg = unsafe { MESSAGE_PREVIEW[..self.simplex_agent_image_len].starts_with(&[0xff, 0xd8]) };
        let caption = core::str::from_utf8(caption).unwrap_or("Photo");
        if self.received_attachment.write_reference(
            &mut marked, old_body, key.number, id, jpeg, caption,
        ).is_err() {
            return false;
        }
        if old_len != 0 {
            self.append_history_line(owner, key, marked.as_str())
        } else {
            self.append_display_line(owner, key, speaker, marked.as_str())
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn fail_upload(&mut self) {
        self.upload_phase = 0;
        self.simplex_agent_send_status = 4;
        self.settle_agent(true, true);
    }

    #[cfg(feature = "simplex-agent")]
    fn detach_legacy_library_references(&mut self, library_id: u64) -> bool {
        use core::fmt::Write;
        use personal_os_rust_server::messages::{split_file_reference, split_photo_reference, TranscriptBuffer};
        if unsafe { messages_history::index() }.refresh(self.blob_transfer).is_err() { return false; }
        let count = unsafe { messages_history::index() }.row_count();
        for index in 0..count {
            let Some((owner, key, record)) = (unsafe { messages_history::index() }).row_at(index) else { continue; };
            let mut line_bytes = [0u8; vault_history::MAX_EVENT_SIZE];
            let line_len = match messages_history::read(record, unsafe { self.blob_transfer.as_mut() }) {
                Ok(Some((_, vault_history::Event::Row(_, line)))) if line.len() <= line_bytes.len() => {
                    line_bytes[..line.len()].copy_from_slice(line.as_bytes()); line.len()
                }
                Ok(Some((_, Event::Peer(_)))) => continue,
                _ => return false,
            };
            let Ok(line) = core::str::from_utf8(&line_bytes[..line_len]) else { return false; };
            let found = [PHOTO_REFERENCE_PREFIX, FILE_REFERENCE_PREFIX].iter().find_map(|prefix| {
                let pos = line.find(prefix)?;
                let reference = &line[pos..];
                let parsed = if *prefix == PHOTO_REFERENCE_PREFIX { split_photo_reference(reference) }
                    else { split_file_reference(reference) }?;
                (parsed.1 == library_id).then_some((pos, *prefix, parsed.0, parsed.2))
            });
            let Some((pos, prefix, message, caption)) = found else { continue; };
            if key == MessageKey::LOCAL { return false; }
            set_mr(0, library_id); set_mr(1, owner);
            let reply = VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_LIBRARY_COPY_TO_CHAT, 2));
            if reply.label() != REPLY_OK || reply.count() != 1 { return false; }
            let copy_id = get_mr(0);
            let mut output = [0u8; vault_history::MAX_EVENT_SIZE];
            let mut updated = TranscriptBuffer::new(&mut output);
            if write!(updated, "{}{}{}\x1f{}\x1f{}", &line[..pos], prefix, message, copy_id, caption).is_err()
                || !self.append_history_line(owner, key, updated.as_str()) { return false; }
        }
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn begin_saved_photo_upload(&mut self, source_chat: usize, photo_id: u64) -> bool {
        let library = source_chat == usize::MAX;
        if self.upload_phase != 0 || (!library && source_chat >= self.simplex_agent_connection_handles_length) || photo_id == 0 {
            return false;
        }
        // The current XFTP sender creates one recipient descriptor. Do not
        // claim a group attachment was sent until multi-member key delivery
        // has been implemented and verified.
        if self.simplex_agent_selected_index >= 16
            || self.simplex_agent_group_mask & (1u16 << self.simplex_agent_selected_index) != 0
        {
            return false;
        }
        let source = if library { crate::vault_protocol::LIBRARY_OWNER } else { self.simplex_agent_connection_handles[source_chat] };
        let target = self.simplex_agent_selected_handle;
        if source == 0 || target == 0 || (!library && source != target) || !self.simplex_agent_connection_handles[..self.simplex_agent_connection_handles_length].contains(&target) {
            return false;
        }
        set_mr(0, photo_id);
        set_mr(1, 0);
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
        if reply.label() != REPLY_OK || reply.count() != 2 || get_mr(0) != source {
            return false;
        }
        let total = get_mr(1) as usize;
        let Some(first) = (unsafe { self.blob_transfer.as_ref() }).payload() else { return false; };
        if !(1..=IMAGE_CAP).contains(&total) {
            return false;
        }
        self.upload_is_photo = first.starts_with(&[0xff, 0xd8]);
        // Sending from Files creates a chat-owned copy before asynchronous
        // upload so deleting the library entry cannot invalidate the message.
        self.upload_photo_id = if library {
            set_mr(0, photo_id);
            set_mr(1, target);
            let reply = VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_LIBRARY_COPY_TO_CHAT, 2));
            if reply.label() != REPLY_OK || reply.count() != 1 { return false; }
            get_mr(0)
        } else { photo_id };
        self.upload_target = target;
        self.upload_total = total;
        self.upload_offset = 0;
        self.upload_phase = if self.agent_busy() { 3 } else { 1 };
        self.simplex_agent_send_status = 1;
        if self.upload_phase == 1 && !self.request_upload_chunk() {
            self.fail_upload();
            return false;
        }
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn request_upload_chunk(&mut self) -> bool {
        if self.upload_offset >= self.upload_total || self.upload_offset % PHOTO_CHUNK != 0 { return false; }
        set_mr(0, self.upload_photo_id);
        set_mr(1, (self.upload_offset / PHOTO_CHUNK) as u64);
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
        let expected_count = if self.upload_offset == 0 { 2 } else { 0 };
        if reply.label() != REPLY_OK || reply.count() != expected_count { return false; }
        let Some(bytes) = (unsafe { self.blob_transfer.as_ref() }).payload() else { return false; };
        let expected = (self.upload_total - self.upload_offset).min(PHOTO_CHUNK);
        if bytes.len() != expected { return false; }
        let mut payload = [0u8; PHOTO_CHUNK + 4];
        payload[..4].copy_from_slice(&(self.upload_offset as u32).to_le_bytes());
        payload[4..4 + expected].copy_from_slice(bytes);
        let Some(mut page) = self.simplex_agent_page else { return false; };
        if !unsafe { page.as_mut() }.begin(Operation::StageAttachment, 13, &payload[..4 + expected]) { return false; }
        self.upload_chunk_len = expected;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_upload_chunk(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        let ok = self.simplex_agent_page.and_then(|page| unsafe { page.as_ref() }.response(13))
            .is_some_and(|(status, bytes)| status == Status::Ok && bytes.is_empty());
        if !ok { self.fail_upload(); return; }
        self.upload_offset += self.upload_chunk_len;
        if self.upload_offset < self.upload_total {
            if !self.request_upload_chunk() { self.fail_upload(); }
            return;
        }
        let mut payload = [0u8; 140];
        let mut writer = personal_os_rust_server::simplex_ipc::PayloadWriter::new(&mut payload);
        if !writer.u64(self.upload_target) || !writer.bytes(&self.upload_name[..self.upload_name_len]) { self.fail_upload(); return; }
        let len = writer.len();
        let Some(mut page) = self.simplex_agent_page else { self.fail_upload(); return; };
        if !unsafe { page.as_mut() }.begin(Operation::SendAttachment, 14, &payload[..len]) { self.fail_upload(); return; }
        self.upload_phase = 2;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_upload_send(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        let result = self.simplex_agent_page.and_then(|page| unsafe { page.as_ref() }.response(14));
        let sent_id = result.and_then(|(status, bytes)| {
            let mut reader = PayloadReader::new(bytes);
            let id = reader.u64()?;
            reader.bytes()?;
            (status == Status::Ok && id != 0 && reader.is_empty()).then_some(id)
        });
        self.upload_phase = 0;
        self.simplex_agent_send_status = 4;
        if let Some(id) = sent_id {
            use core::fmt::Write;
            let chat = self.chat_index_for_handle(self.upload_target) as usize;
            if chat < CHAT_SLOT_COUNT {
                if let Ok(owner) = self.ensure_chat_history(chat) {
                    let mut bytes = [0u8; 256];
                    let mut marker = personal_os_rust_server::messages::TranscriptBuffer::new(&mut bytes);
                    let name = core::str::from_utf8(&self.upload_name[..self.upload_name_len]).unwrap_or("File");
                    let prefix = if self.upload_is_photo { PHOTO_REFERENCE_PREFIX } else { FILE_REFERENCE_PREFIX };
                    if write!(marker, "{}{}\x1f{}\x1f{}", prefix, id, self.upload_photo_id, name).is_ok()
                        && self.append_display_line(owner, MessageKey { source: u64::MAX, number: id, part: 0 }, b"NexatrodeOS", marker.as_str())
                    {
                        self.simplex_agent_send_message_id = id;
                        // The SMP offer is queued, but this reply does not
                        // confirm that the XFTP upload reached the peer.
                        self.simplex_agent_send_status = 5;
                    }
                }
            }
        }
        self.settle_agent(true, true);
    }

    #[cfg(feature = "simplex-agent")]
    fn agent_busy(&self) -> bool {
        matches!(self.upload_phase, 1 | 2) || self.simplex_agent_outbox.in_flight.is_some()
            || self.simplex_agent_receive_started
            || self.simplex_agent_ack_started
            || self.simplex_agent_request_started
            || self.invitation_request == InvitationRequest::InFlight
            || self.simplex_agent_join_probe_started
            || self.simplex_agent_image_started
            || self.simplex_agent_delete_started
            || self.simplex_agent_peer_photo_started
            || self.simplex_agent_group_create_started
            || self.simplex_agent_address_request_started
            || self.simplex_agent_page.is_some_and(|page| unsafe { page.as_ref() }.request().is_some())
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_delete_connection(&mut self, chat: usize) {
        if chat >= CHAT_SLOT_COUNT || self.agent_busy() {
            return;
        }
        let handle = if chat < self.simplex_agent_connection_handles_length {
            self.simplex_agent_connection_handles[chat]
        } else {
            0
        };
        if handle == 0 {
            self.forget_chat(chat);
            return;
        }
        let Some(mut page) = self.simplex_agent_page else {
            return;
        };
        let mut payload = [0; personal_os_rust_server::simplex_ipc::PAYLOAD_CAPACITY];
        let mut writer = personal_os_rust_server::simplex_ipc::PayloadWriter::new(&mut payload);
        if !writer.u64(handle) {
            return;
        }
        let length = writer.len();
        if !unsafe { page.as_mut() }.begin(Operation::DeleteConnection, 7, &payload[..length]) {
            return;
        }
        self.simplex_agent_delete_started = true;
        self.simplex_agent_delete_chat = chat;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_delete_connection(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        self.simplex_agent_delete_started = false;
        let chat = self.simplex_agent_delete_chat;
        let Some(page) = self.simplex_agent_page else {
            return;
        };
        match unsafe { page.as_ref() }.response(7) {
            Some((Status::Ok, _)) => {
                self.forget_chat(chat);
                debug_println!("SIMPLEXIPC|MESSAGES_APP|DELETE_PASS|{}", chat);
                self.request_agent_connection_snapshot();
            }
            _ => debug_println!("SIMPLEXIPC|MESSAGES_APP|DELETE_FAIL|{}", chat),
        }
        self.settle_agent(true, true);
    }

    #[cfg(feature = "simplex-agent")]
    fn forget_chat(&mut self, chat: usize) {
        if chat >= CHAT_SLOT_COUNT {
            return;
        }
        if let Ok(owner) = self.ensure_chat_history(chat) {
            if (unsafe { messages_history::index() })
                .append(owner, vault_history::CLEARED, self.blob_transfer).is_err()
            {
                debug_println!("SIMPLEXIPC|MESSAGES_APP|HISTORY_DELETE_FAIL");
                return;
            }
        }
        if self.ensure_messages_loaded() {
            self.chat_transcripts.clear(chat);
            let _ = self.persist_messages();
        }
        self.simplex_agent_chat_names[chat] = [0; 32];
        self.simplex_agent_chat_name_lengths[chat] = 0;
        if chat < self.simplex_agent_connection_handles_length {
            self.simplex_agent_connection_handles[chat] = 0;
        }
        if self.simplex_agent_selected_index == chat {
            self.simplex_agent_selected_handle = 0;
        }
        if !self.render_messages_snapshot() {
            debug_println!("SIMPLEXIPC|MESSAGES_APP|DELETE_RENDER_FAIL");
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn notify_chat_ui(&self) {
        SLINT_MESSAGES_SERVICE.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn flush_pending_send(&mut self) -> bool {
        if self.agent_busy() {
            return false;
        }
        let Some(mut page) = self.simplex_agent_page else { return false; };
        let live = &self.simplex_agent_connection_handles[..self.simplex_agent_connection_handles_length];
        match self.simplex_agent_outbox.begin(unsafe { page.as_mut() }, live) {
            Ok(true) => {
                self.simplex_agent_send_status = 1;
                self.simplex_agent_send_message_id = 0;
                core::sync::atomic::fence(Ordering::Release);
                SIMPLEX_AGENT_REQUEST.notify();
                true
            }
            Ok(false) => false,
            Err(()) => {
                self.simplex_agent_send_status = 4;
                self.notify_chat_ui();
                false
            }
        }
    }

    /// Pull one queued SMP message when the page is free. An empty result must
    /// not immediately ask again; the agent notifies when another message arrives.
    #[cfg(feature = "simplex-agent")]
    fn settle_agent(&mut self, allow_receive: bool, wake_ui: bool) {
        if self.start_queued_invitation() {
            return;
        }
        if self.upload_phase == 3 && !self.agent_busy() {
            self.upload_phase = 1;
            if !self.request_upload_chunk() { self.fail_upload(); }
            if wake_ui { self.notify_chat_ui(); }
            return;
        }
        if self.simplex_agent_receive_status == 6 && self.worker_receive_pending.may_auto_ack() && !self.agent_busy() {
            self.request_agent_acknowledge();
            if self.simplex_agent_ack_started {
                if wake_ui {
                    self.notify_chat_ui();
                }
                return;
            }
        }
        let started_send = self.flush_pending_send();
        let mut started_receive = false;
        if !started_send
            && allow_receive
            && !self.agent_busy()
            && self.simplex_agent_receive_status != 2
            && self.simplex_agent_receive_status != 6
        {
            self.request_agent_receive_next();
            started_receive = self.simplex_agent_receive_started;
        }
        // A receive that just started has no text yet. Waking the UI now
        // consumes the signal, and the later "message ready" wake is lost.
        if (wake_ui && !started_receive) || started_send {
            self.notify_chat_ui();
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_send_message(&mut self, text: &[u8]) -> bool {
        let live = &self.simplex_agent_connection_handles[..self.simplex_agent_connection_handles_length];
        let handle = self.simplex_agent_selected_handle;
        if self.simplex_agent_page.is_none()
            || live.get(self.simplex_agent_selected_index) != Some(&handle)
            || !self.simplex_agent_outbox.queue(handle, live, text)
        {
            return false;
        }
        debug_println!("SIMPLEXIPC|MESSAGES_APP|SEND_HANDLE|{}", handle);
        self.simplex_agent_send_status = 1;
        self.flush_pending_send();
        self.simplex_agent_send_status == 1
    }

    #[cfg(feature = "simplex-agent")]
    fn receive_agent_send_message(&mut self) {
        core::sync::atomic::fence(Ordering::Acquire);
        let Some(sent) = self.simplex_agent_outbox.in_flight.take() else { return; };
        let Some(page) = self.simplex_agent_page else {
            self.simplex_agent_send_status = 4;
            return;
        };
        match unsafe { page.as_ref() }.response(4) {
            Some((Status::Ok, payload)) => {
                let mut reader = PayloadReader::new(payload);
                if let Some(message_id) = reader.u64().filter(|id| *id != 0) {
                    // Optional SharedMsgId: u16 len + bytes (empty for edit/delete).
                    let shared_id = reader.bytes().unwrap_or(&[]);
                    if reader.is_empty() {
                        self.simplex_agent_send_status = 2;
                        self.simplex_agent_send_message_id = message_id;
                        debug_println!("SIMPLEXIPC|MESSAGES_APP|SEND_MESSAGE_PASS");
                        let chat = self.chat_index_for_handle(sent.handle);
                        if chat < CHAT_SLOT_COUNT as u64 {
                            self.apply_send_transcript(chat as usize, sent.text(), shared_id);
                        }
                        self.settle_agent(true, true);
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
        self.settle_agent(true, true);
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_receive_next(&mut self) {
        if self.simplex_agent_receive_started
            || self.simplex_agent_receive_status == 2
            || self.simplex_agent_receive_status == 6
            || self.agent_busy()
        {
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
        unsafe {
            RECEIVED_ATTACHMENT_ID = None;
        }
        self.direct_kind = 0;
        self.direct_total = 0;
        self.direct_filled = 0;
        self.simplex_agent_receive_length = 0;
        self.simplex_agent_receive_started = true;
        self.simplex_agent_receive_status = 1;
        core::sync::atomic::fence(Ordering::Release);
        SIMPLEX_AGENT_REQUEST.notify();
    }

    #[cfg(feature = "simplex-agent")]
    fn accept_direct_chunk(&mut self, chunk: &[u8]) {
        let remaining = self.direct_total - self.direct_filled;
        if chunk.len() != remaining.min(3000) {
            self.simplex_agent_receive_status = 4;
            return;
        }
        let end = self.direct_filled + chunk.len();
        unsafe { DIRECT_BODY[self.direct_filled..end].copy_from_slice(chunk); }
        self.direct_filled = end;
        if end < self.direct_total {
            let offset = (end as u32).to_le_bytes();
            let mut page = self.simplex_agent_page.unwrap();
            if unsafe { page.as_mut() }.begin(Operation::ReceiveNext, 5, &offset) {
                self.simplex_agent_receive_started = true;
                core::sync::atomic::fence(Ordering::Release);
                SIMPLEX_AGENT_REQUEST.notify();
            } else {
                self.simplex_agent_receive_status = 4;
            }
            return;
        }
        let Ok(text) = core::str::from_utf8(unsafe { &DIRECT_BODY[..end] }) else {
            self.simplex_agent_receive_status = 4;
            return;
        };
        // The status payload is a notification. The transcript service serves
        // the complete committed body, including text spanning several pages.
        if self.direct_kind == 7 && text.len() <= self.simplex_agent_receive_body.len() {
            self.simplex_agent_receive_body[..end].copy_from_slice(text.as_bytes());
            self.simplex_agent_receive_length = end;
        }
        self.simplex_agent_receive_status = 2;
        debug_println!("SIMPLEXIPC|MESSAGES_APP|RECEIVE_MESSAGE_PASS");
        self.settle_agent(false, true);
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
                // An empty receive releases the Agent page before a new poll
                // starts. Use that gap for paced pending-connection metadata.
                self.refresh_pending_connection_metadata();
                // Do not immediately re-poll: that spins when the inbox is
                // quiet. A message that arrived during this empty poll sets
                // pending_inbox_wake in the agent bridge; the follow-up notify
                // lands in the idle branch and starts ReceiveNext again.
                self.settle_agent(false, false);
                if !self.agent_busy() {
                    self.begin_peer_photo_fetch();
                }
            }
            Some((Status::Ok, payload)) => {
                if self.direct_kind != 0 {
                    self.accept_direct_chunk(payload);
                    return;
                }
                if matches!(payload.first(), Some(7..=9)) {
                    let Some(frame) = crate::simplex_ipc::DirectFrame::parse(payload, vault_history::DIRECT_TEXT_LIMIT)
                        .filter(|frame| vault_history::valid_shared_id(frame.id)) else {
                        self.simplex_agent_receive_status = 4;
                        return;
                    };
                    self.direct_kind = frame.kind;
                    self.direct_total = frame.total;
                    self.direct_id_len = frame.id.len();
                    self.direct_id[..frame.id.len()].copy_from_slice(frame.id.as_bytes());
                    self.simplex_agent_receive_handle = frame.handle;
                    self.simplex_agent_receive_chat_handle = frame.handle;
                    self.simplex_agent_receive_message_id = frame.message;
                    self.simplex_agent_receive_part = frame.part;
                    self.accept_direct_chunk(frame.first);
                    return;
                }
                let mut reader = PayloadReader::new(payload);
                let kind = reader.u8();
                if kind == Some(4) {
                    self.accept_group_invite(&mut reader);
                    return;
                }
                if matches!(kind, Some(3 | 6)) {
                    self.accept_image_preview(&mut reader, kind == Some(6));
                    return;
                }
                if matches!(kind, Some(2 | 12)) {
                    let delivered = reader.u64().zip(reader.u64());
                    if let Some((handle, message_id)) = delivered.filter(|_| reader.is_empty()) {
                        self.note_send_notice(handle, message_id, kind == Some(12));
                        self.simplex_agent_receive_status = 3;
                        self.settle_agent(true, true);
                        return;
                    }
                    self.simplex_agent_receive_status = 4;
                    self.settle_agent(false, false);
                    return;
                }
                if kind == Some(5) {
                    self.accept_group_member_message(&mut reader);
                    return;
                }
                if !matches!(kind, Some(1 | 11)) {
                    self.simplex_agent_receive_status = 4;
                    self.settle_agent(false, false);
                    return;
                }
                let parsed = reader
                    .u64()
                    .zip(reader.u64())
                    .zip(reader.bytes())
                    .map(|((handle, message_id), body)| (handle, message_id, body));
                if let Some((handle, message_id, body)) = parsed {
                    let part = Self::receive_part(&mut reader);
                    if part.is_none()
                        || handle == 0
                        || message_id == 0
                        || body.len() > self.simplex_agent_receive_body.len()
                        || core::str::from_utf8(body).is_err()
                    {
                        self.simplex_agent_receive_status = 4;
                        return;
                    }
                    if kind == Some(11) { self.direct_kind = 11; }
                    self.simplex_agent_receive_body[..body.len()].copy_from_slice(body);
                    self.simplex_agent_receive_length = body.len();
                    self.simplex_agent_receive_handle = handle;
                    self.simplex_agent_receive_chat_handle = handle;
                    self.simplex_agent_receive_message_id = message_id;
                    self.simplex_agent_receive_part = part.unwrap_or(0);
                    self.simplex_agent_receive_status = 2;
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|RECEIVE_MESSAGE_PASS");
                    self.settle_agent(false, true);
                } else {
                    self.simplex_agent_receive_status = 4;
                    self.settle_agent(false, false);
                }
            }
            _ => {
                self.simplex_agent_receive_status = 4;
                self.settle_agent(false, false);
            }
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn request_agent_acknowledge(&mut self) {
        if self.simplex_agent_ack_started
            || self.simplex_agent_receive_status != 6
            || self.simplex_agent_receive_handle == 0
            || self.simplex_agent_receive_message_id == 0
        {
            self.simplex_agent_ack_status = 4;
            return;
        }
        if self.simplex_agent_outbox.in_flight.is_some()
            || self.simplex_agent_receive_started
            || self.simplex_agent_request_started
            || self.invitation_request == InvitationRequest::InFlight
            || self.simplex_agent_join_probe_started
            || self.simplex_agent_address_request_started
        {
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
            _ => {
                self.simplex_agent_ack_status = 4;
                self.simplex_agent_receive_status = 0;
                self.simplex_agent_receive_handle = 0;
                self.simplex_agent_receive_message_id = 0;
            }
        }
        self.settle_agent(true, false);
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
            // A finished request leaves its status on the page. A later wake,
            // when the relay has a new message, must not be treated as that
            // old result. Only the request still in flight owns the page.
            if self.simplex_agent_join_probe_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(3))
                    .is_some()
            {
                self.receive_agent_join_invitation();
            } else if self.upload_phase == 1
                && self.simplex_agent_page.and_then(|page| unsafe { page.as_ref() }.response(13)).is_some()
            {
                self.receive_upload_chunk();
            } else if self.upload_phase == 2
                && self.simplex_agent_page.and_then(|page| unsafe { page.as_ref() }.response(14)).is_some()
            {
                self.receive_upload_send();
            } else if self.simplex_agent_outbox.in_flight.is_some()
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(4))
                    .is_some()
            {
                self.receive_agent_send_message();
            } else if self.simplex_agent_receive_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(5))
                    .is_some()
            {
                self.receive_agent_next();
            } else if self.simplex_agent_ack_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(6))
                    .is_some()
            {
                self.receive_agent_acknowledge();
            } else if self.simplex_agent_delete_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(7))
                    .is_some()
            {
                self.receive_agent_delete_connection();
            } else if self.invitation_request == InvitationRequest::InFlight
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(2))
                    .is_some()
            {
                self.receive_agent_invitation();
            } else if self.simplex_agent_request_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(1))
                    .is_some()
            {
                self.receive_agent_connection_snapshot();
                // Pull queued chat text first, then fill avatars when idle.
                self.settle_agent(true, true);
                if !self.agent_busy() {
                    self.begin_peer_photo_fetch();
                }
            } else if self.simplex_agent_image_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(8))
                    .is_some()
            {
                self.receive_image_chunk();
            } else if self.simplex_agent_peer_photo_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(9))
                    .is_some()
            {
                self.receive_peer_image_chunk();
                if !self.agent_busy() {
                    self.settle_agent(true, true);
                }
            } else if self.simplex_agent_group_create_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(11))
                    .is_some()
            {
                self.receive_agent_create_group();
            } else if self.simplex_agent_address_request_started
                && self
                    .simplex_agent_page
                    .and_then(|page| unsafe { page.as_ref() }.response(12))
                    .is_some()
            {
                self.receive_agent_address();
            } else if self.agent_busy() {
                // Inbox poke while a request is still outstanding. Do not
                // overwrite the shared page; the reply notify will settle.
            } else if self.agent_connection_count.is_some() {
                // The agent queued an SMP message and there is no request in
                // flight. Take it without waiting for a tap.
                self.settle_agent(true, true);
                if !self.agent_busy() {
                    self.begin_peer_photo_fetch();
                }
            } else {
                self.request_agent_connection_snapshot();
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
            #[cfg(feature = "utm-transcript-worker")]
            if let Some(client) = self.slint_transfer {
                return Ok(self.handle_isolated_messages(request, client));
            }
            return Ok(self.handle_slint_messages(request));
        }
        #[cfg(feature = "utm-transcript-worker")]
        if channel == Channel::new(7) && self.name == "MESSAGES" {
            if !matches!(request.label(), REQUEST_SLINT_HISTORY_ADVANCE | REQUEST_SLINT_HISTORY_PAGE_STEP | REQUEST_WORKER_RECEIVE_STATUS | REQUEST_SLINT_MESSAGES_RECEIVE | REQUEST_WORKER_SELECT_OWNER | REQUEST_SLINT_MESSAGES_LOAD) {
                return Ok(MessageInfo::new(REPLY_DENIED, 0));
            }
            return Ok(match self.transcript_transfer {
                Some(client) => self.handle_isolated_messages(request, client),
                None => MessageInfo::new(REPLY_DENIED, 0),
            });
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
    #[cfg(feature = "utm-transcript-worker")]
    pub fn set_message_client_pages(&mut self, ui: NonNull<BlobTransferPage>, worker: NonNull<BlobTransferPage>) {
        self.slint_transfer = Some(ui);
        self.transcript_transfer = Some(worker);
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn handle_isolated_messages(&mut self, request: MessageInfo, client: NonNull<BlobTransferPage>) -> MessageInfo {
        let source = unsafe { client.as_ref() };
        if !source.copy_to(unsafe { self.blob_transfer.as_mut() }) {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let reply = self.handle_slint_messages(request);
        if !unsafe { self.blob_transfer.as_ref() }.copy_to(unsafe { &mut *client.as_ptr() }) {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        reply
    }

    /// Slint receives only Messages' current local peer/thread summary. The
    /// Messages PD remains the sole owner of its vault record and Agent state.
    fn handle_slint_messages(&mut self, request: MessageInfo) -> MessageInfo {
        // Disposable-only input seam: synthesize a decoded relay event after
        // the real GHC agent has opened its fresh encrypted AgentStore. The
        // ordinary receive worker below still owns persistence and delivery.
        #[cfg(feature = "utm-simplex-session-test")]
        if request.label() == 0x7c0 && request.count() == 1 {
            let Some(body) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                return MessageInfo::new(REPLY_BAD_REQUEST, 0);
            };
            if body.is_empty() || body.len() > 2048 || core::str::from_utf8(body).is_err() {
                return MessageInfo::new(REPLY_BAD_REQUEST, 0);
            }
            let length = body.len();
            let mut saved = [0u8; 2048];
            saved[..length].copy_from_slice(body);
            let epoch = get_mr(0);
            self.poll_agent_list_response();
            if self.agent_connection_count.is_none() || self.agent_busy() {
                self.request_agent_connection_snapshot();
                return MessageInfo::new(1, 0);
            }
            if !self.ensure_messages_loaded() { return MessageInfo::new(REPLY_BAD_REQUEST, 0); }
            self.fixture_receive = Some((epoch, length, saved));
            return MessageInfo::new(0, 0);
        }
        #[cfg(feature = "utm-simplex-session-test")]
        if request.label() == 0x7c1 && request.count() == 0 {
            // A tiny generated JPEG uses the same owner-scoped encrypted
            // attachment path as a downloaded photo. No external peer exists.
            let Some(bytes) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                return MessageInfo::new(REPLY_BAD_REQUEST, 0);
            };
            let length = bytes.len();
            if length == 0 || length > 3072 { return MessageInfo::new(REPLY_BAD_REQUEST, 0); }
            set_mr(0, 0);
            let reply = VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_PHOTO_STAGE, 1));
            if reply.label() != REPLY_OK { return reply; }
            set_mr(0, 4242); set_mr(1, length as u64);
            return VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_PHOTO_COMMIT, 2));
        }
        #[cfg(feature = "utm-transcript-worker")]
        if request.label() == REQUEST_WORKER_SELECT_OWNER && request.count() == 1 {
            #[cfg(feature = "simplex-agent")]
            {
                let owner = get_mr(0);
                let live = &self.simplex_agent_connection_handles[..self.simplex_agent_connection_handles_length];
                if owner != 0 {
                    if let Some(index) = crate::simplex_ipc::live_chat_index(live, owner) {
                        self.simplex_agent_selected_index = index;
                        self.simplex_agent_selected_handle = owner;
                        return MessageInfo::new(REPLY_SLINT_MESSAGES_SELECT_CHAT, 0);
                    }
                }
            }
            return MessageInfo::new(REPLY_DENIED, 0);
        }
        #[cfg(feature = "simplex-agent")]
        if request.label() != REQUEST_SLINT_MESSAGES_CREATE_INVITATION
            && request.label() != REQUEST_SLINT_MESSAGES_SET_PROFILE
            && request.label() != REQUEST_SLINT_MESSAGES_SEND
            && request.label() != REQUEST_SLINT_MESSAGES_SELECT_CHAT
            && request.label() != REQUEST_SLINT_MESSAGES_JOIN_INVITATION
            && request.label() != REQUEST_SLINT_MESSAGES_CREATE_GROUP
            && request.label() != REQUEST_SLINT_MESSAGES_CREATE_ADDRESS
            && request.label() != REQUEST_SLINT_MESSAGES_SEND_SAVED_PHOTO
            && request.label() != REQUEST_SLINT_LIBRARY_CLONE
            && request.label() != REQUEST_SLINT_LIBRARY_NEXT
            && request.label() != REQUEST_SLINT_LIBRARY_READ
            && request.label() != REQUEST_SLINT_LIBRARY_DELETE
            && request.label() != REQUEST_SLINT_LIBRARY_STAGE
            && request.label() != REQUEST_SLINT_LIBRARY_IMPORT
        {
            // An idle poll arms ReceiveNext before the handler runs. That
            // made every New Group tap report busy. Leave the page free so
            // the group request can start; a tap during a real in-flight
            // reply still returns busy and can be repeated.
            self.poll_agent_page();
        }
        match (request.label(), request.count()) {
            (REQUEST_SLINT_LIBRARY_STAGE, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_STAGE, 1));
                    if reply.label() == REPLY_OK && reply.count() == 0 {
                        return MessageInfo::new(REPLY_SLINT_LIBRARY_STAGE, 0);
                    }
                    return MessageInfo::new(reply.label(), 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_LIBRARY_IMPORT, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let reply = VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_LIBRARY_IMPORT, 1));
                    if reply.label() == REPLY_OK && reply.count() == 1 {
                        return MessageInfo::new(REPLY_SLINT_LIBRARY_IMPORT, 1);
                    }
                    return MessageInfo::new(reply.label(), 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_LIBRARY_DELETE, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let id = get_mr(0);
                    if !self.detach_legacy_library_references(id) {
                        return MessageInfo::new(crate::vault_protocol::REPLY_STORAGE, 0);
                    }
                    set_mr(0, id);
                    let reply = VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_LIBRARY_DELETE, 1));
                    if reply.label() == REPLY_OK && reply.count() == 0 {
                        return MessageInfo::new(REPLY_SLINT_LIBRARY_DELETE, 0);
                    }
                    return MessageInfo::new(reply.label(), 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_LIBRARY_CLONE, 2) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let chat = get_mr(0) as usize;
                    let id = get_mr(1);
                    if chat >= self.simplex_agent_connection_handles_length || id == 0 {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let owner = self.simplex_agent_connection_handles[chat];
                    set_mr(0, id); set_mr(1, owner);
                    let reply = VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_LIBRARY_CLONE, 2));
                    if reply.label() == REPLY_OK && reply.count() == 1 {
                        return MessageInfo::new(REPLY_SLINT_LIBRARY_CLONE, 1);
                    }
                    return MessageInfo::new(reply.label(), 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_LIBRARY_NEXT, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let reply = VAULT.pp_call(MessageInfo::new(crate::vault_protocol::REQUEST_LIBRARY_NEXT, 1));
                    if reply.label() == REPLY_OK && reply.count() == 3 {
                        return MessageInfo::new(REPLY_SLINT_LIBRARY_NEXT, 3);
                    }
                    return MessageInfo::new(reply.label(), 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_LIBRARY_READ, 2) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let id = get_mr(0); let part = get_mr(1);
                    if id == 0 { return MessageInfo::new(REPLY_BAD_REQUEST, 0); }
                    set_mr(0, id); set_mr(1, 0);
                    let first = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
                    if first.label() != REPLY_OK || first.count() != 2 || get_mr(0) != crate::vault_protocol::LIBRARY_OWNER {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let total = get_mr(1);
                    if part != 0 {
                        set_mr(0, id); set_mr(1, part);
                        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
                        if reply.label() != REPLY_OK || reply.count() != 0 { return MessageInfo::new(REPLY_BAD_REQUEST, 0); }
                    }
                    set_mr(0, total);
                    return MessageInfo::new(REPLY_SLINT_LIBRARY_READ, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_SEND_SAVED_PHOTO, 2) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let source = get_mr(0) as usize;
                    let photo_id = get_mr(1);
                    let Some(name) = (unsafe { self.blob_transfer.as_ref() }).payload() else { return MessageInfo::new(REPLY_BAD_REQUEST, 0); };
                    if self.upload_phase != 0 || name.is_empty() || name.len() > self.upload_name.len()
                        || core::str::from_utf8(name).is_err()
                        || name.iter().any(|c| *c < 32 || *c == b'/' || *c == b'\\')
                    { return MessageInfo::new(REPLY_BAD_REQUEST, 0); }
                    self.upload_name_len = name.len();
                    self.upload_name[..name.len()].copy_from_slice(name);
                    if self.begin_saved_photo_upload(source, photo_id) {
                        return MessageInfo::new(REPLY_SLINT_MESSAGES_SEND_SAVED_PHOTO, 0);
                    }
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_CREATE_ADDRESS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    if !self.request_agent_address() {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_CREATE_ADDRESS, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_ADDRESS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let uri = &self.simplex_agent_address_uri
                        [..self.simplex_agent_address_uri_length];
                    if !unsafe { self.blob_transfer.as_mut() }.set_payload(uri) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, uri.len() as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_ADDRESS, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_CREATE_INVITATION, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    self.invitation_request.queue(self.simplex_agent_invitation_uri_length != 0);
                    self.start_queued_invitation();
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
                    if self.agent_busy() {
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
                    let mut body = [0u8; 2048];
                    body[..length].copy_from_slice(payload);
                    if body[..length].starts_with(b"data:image/")
                        && !self.store_outgoing_image(&body[..length])
                    {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    // Record on agent PASS so SharedMsgId can be embedded for
                    // later edit/delete; Slint refreshes transcript via status.
                    if !self.request_agent_send_message(&body[..length]) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_SEND, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_SEND_STATUS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let status = if matches!(self.simplex_agent_send_status, 5 | 7)
                        && self.simplex_agent_selected_handle != self.upload_target {
                        0
                    } else {
                        self.simplex_agent_send_status
                    };
                    set_mr(0, status);
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
            (REQUEST_SLINT_MESSAGES_RECEIVE_STATUS | REQUEST_WORKER_RECEIVE_STATUS, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    // Inject after polling the real agent, immediately before
                    // the normal persistence/handoff path. Its empty inbox
                    // response must not overwrite the fixture event in flight.
                    #[cfg(feature = "utm-simplex-session-test")]
                    if let Some((epoch, length, body)) = self.fixture_receive.take() {
                        self.simplex_agent_receive_body[..length].copy_from_slice(&body[..length]);
                        self.simplex_agent_receive_length = length;
                        self.simplex_agent_connection_handles[0] = 4242;
                        self.simplex_agent_connection_handles_length = 1;
                        self.simplex_agent_chat_names[0][..10].copy_from_slice(b"Disposable");
                        self.simplex_agent_chat_name_lengths[0] = 10;
                        self.simplex_agent_chat_count = 1;
                        self.agent_connection_count = Some(1);
                        self.simplex_agent_selected_handle = 4242;
                        self.simplex_agent_receive_chat_handle = 4242;
                        self.simplex_agent_receive_handle = 4242;
                        self.simplex_agent_receive_message_id = epoch;
                        self.simplex_agent_receive_part = 0;
                        self.simplex_agent_receive_status = 2;
                        self.direct_kind = 0;
                        unsafe { RECEIVED_ATTACHMENT_ID = None; }
                    }
                    let length = self.simplex_agent_receive_length;
                    let status = self.simplex_agent_receive_status;
                    let chat =
                        self.chat_index_for_handle(self.simplex_agent_receive_chat_handle);
                    let worker = request.label() == REQUEST_WORKER_RECEIVE_STATUS;
                    let owner = self.simplex_agent_connection_handles
                        .get(chat as usize).copied().unwrap_or(0);
                    if worker && status == 6 && self.worker_receive_pending.retry_owner().is_some() {
                        let owner = self.worker_receive_pending.retry_owner().unwrap();
                        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&self.simplex_agent_receive_body[..length]) {
                            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                        }
                        set_mr(0, 2); set_mr(1, length as MessageRegisterValue);
                        set_mr(2, chat); set_mr(3, owner);
                        return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE_STATUS, 4);
                    }
                    if worker && status == 2 && owner == 0 {
                        self.request_agent_connection_snapshot();
                        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&[]) {
                            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                        }
                        set_mr(0, 1); set_mr(1, 0); set_mr(2, chat); set_mr(3, 0);
                        return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE_STATUS, 4);
                    }
                    if status == 2 {
                        // The UI has this body. Keep the real handle for the
                        // acknowledge. A group member uses the host chat.
                        if chat as usize >= CHAT_SLOT_COUNT {
                            self.request_agent_connection_snapshot();
                            if !unsafe { self.blob_transfer.as_mut() }.set_payload(&[]) {
                                return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                            }
                            set_mr(0, 1);
                            set_mr(1, 0);
                            set_mr(2, chat);
                            if worker { set_mr(3, owner); }
                            return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE_STATUS, if worker { 4 } else { 3 });
                        }
                        if worker && !self.worker_receive_pending.hold(owner) {
                            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                        }
                        let chat_index = chat as usize;
                        let name_len = self.simplex_agent_chat_name_lengths[chat_index];
                        let mut speaker = [0u8; 32];
                        let speaker_len = if name_len == 0 {
                            speaker[..7].copy_from_slice(b"Contact");
                            7
                        } else {
                            speaker[..name_len].copy_from_slice(
                                &self.simplex_agent_chat_names[chat_index][..name_len],
                            );
                            name_len
                        };
                        let mut body = [0u8; 2048];
                        body[..length].copy_from_slice(&self.simplex_agent_receive_body[..length]);
                        // Only expose the acknowledgeable state after the
                        // vault commits the transcript. Leave a failed write
                        // pending for the next poll.
                        let recorded = if matches!(self.direct_kind, 7..=9) {
                            self.record_direct_event(chat_index, &speaker[..speaker_len]).is_ok()
                        } else if let Some(id) = unsafe { RECEIVED_ATTACHMENT_ID } {
                            self.record_received_attachment(chat_index, &speaker[..speaker_len], &body[..length], id)
                        } else if self.direct_kind == 11 && body[..length].starts_with(b"\x1eR\x1f") {
                            self.apply_reaction_wire(chat_index, &body[3..length])
                        } else {
                            self.record_chat_line(chat_index, &speaker[..speaker_len], &body[..length])
                        };
                        if !recorded {
                            if worker { self.worker_receive_pending.accept(owner); }
                            if !unsafe { self.blob_transfer.as_mut() }.set_payload(&[]) {
                                return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                            }
                            set_mr(0, 7);
                            set_mr(1, 0);
                            set_mr(2, chat);
                            if worker { set_mr(3, owner); }
                            return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE_STATUS, if worker { 4 } else { 3 });
                        }
                        self.simplex_agent_receive_status = 6;
                        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&body[..length]) {
                            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                        }
                        set_mr(0, 2);
                        set_mr(1, length as MessageRegisterValue);
                        set_mr(2, chat);
                        if worker { set_mr(3, owner); }
                        return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE_STATUS, if worker { 4 } else { 3 });
                    }
                    if !unsafe { self.blob_transfer.as_mut() }
                        .set_payload(&self.simplex_agent_receive_body[..length])
                    {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, status);
                    set_mr(1, length as MessageRegisterValue);
                    set_mr(2, chat);
                    if worker { set_mr(3, owner); }
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_RECEIVE_STATUS, if worker { 4 } else { 3 });
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_ACK, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    if !self.worker_receive_pending.may_auto_ack() { return MessageInfo::new(REPLY_DENIED, 0); }
                    self.request_agent_acknowledge();
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_ACK, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            #[cfg(feature = "utm-transcript-worker")]
            (REQUEST_SLINT_MESSAGES_ACK, 1) => {
                if !self.worker_receive_pending.accept(get_mr(0)) {
                    return MessageInfo::new(REPLY_DENIED, 0);
                }
                self.request_agent_acknowledge();
                return MessageInfo::new(REPLY_SLINT_MESSAGES_ACK, 0);
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
            (REQUEST_SLINT_MESSAGES_SELECT_CHAT, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let index = get_mr(0) as usize;
                    self.simplex_agent_selected_index = index;
                    let live = &self.simplex_agent_connection_handles[..self.simplex_agent_connection_handles_length];
                    self.simplex_agent_selected_handle =
                        crate::simplex_ipc::chat_at_displayed_row(&self.slint_displayed_handles, live, index)
                            .unwrap_or(0);
                    if self.simplex_agent_selected_handle != 0 {
                        debug_println!(
                            "SIMPLEXIPC|MESSAGES_APP|SELECT_CHAT|{}|{}",
                            index,
                            self.simplex_agent_selected_handle
                        );
                        return MessageInfo::new(REPLY_SLINT_MESSAGES_SELECT_CHAT, 0);
                    }
                    debug_println!("SIMPLEXIPC|MESSAGES_APP|SELECT_CHAT_FAIL|{}", index);
                    return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_HISTORY_INFO, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let chat = get_mr(0) as usize;
                    let Ok(owner) = self.ensure_chat_history(chat) else {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    };
                    set_mr(0, owner);
                    set_mr(1, unsafe { messages_history::index() }.revision());
                    return MessageInfo::new(REPLY_SLINT_HISTORY_INFO, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            #[cfg(feature = "utm-transcript-worker")]
            (REQUEST_SLINT_HISTORY_ADVANCE, 1) => {
                let chat = get_mr(0) as usize;
                let Some(&owner) = self.simplex_agent_connection_handles.get(chat) else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                if owner == 0 || !self.ensure_messages_loaded() { return MessageInfo::new(REPLY_BAD_REQUEST, 0); }
                let history = unsafe { messages_history::index() };
                match history.refresh_step(self.blob_transfer, 4) {
                    Ok(true) => {},
                    Ok(false) => return MessageInfo::new(REPLY_SLINT_HISTORY_PENDING, 0),
                    Err(()) => return MessageInfo::new(REPLY_BAD_REQUEST, 0),
                }
                let Ok(legacy) = core::str::from_utf8(self.chat_transcripts.text(chat)) else {
                    return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                };
                match history.migrate_step(owner, legacy, self.blob_transfer, 1) {
                    Ok(true) => {},
                    Ok(false) => return MessageInfo::new(REPLY_SLINT_HISTORY_PENDING, 0),
                    Err(()) => return MessageInfo::new(REPLY_BAD_REQUEST, 0),
                }
                set_mr(0, owner);
                set_mr(1, history.revision());
                return MessageInfo::new(REPLY_SLINT_HISTORY_INFO, 2);
            }
            (REQUEST_SLINT_HISTORY_PAGE | REQUEST_SLINT_HISTORY_PAGE_STEP, 3) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let owner = get_mr(0);
                    let revision = get_mr(1);
                    let start = get_mr(2) as usize;
                    let history = unsafe { messages_history::index() };
                    if owner == 0 || history.revision() != revision {
                        return MessageInfo::new(crate::vault_protocol::REPLY_CONFLICT, 0);
                    }
                    let page = unsafe { self.blob_transfer.as_mut() };
                    let mut output = [0; personal_os_rust_server::vault_blob_record::MAX_PAYLOAD_SIZE];
                    let mut length = 0;
                    let mut next = 0;
                    let max_parts = if request.label() == REQUEST_SLINT_HISTORY_PAGE_STEP { 4 } else { usize::MAX };
                    let mut parts_read = 0;
                    // High bits select the row; the low byte advances inside
                    // one row. Each part is valid UTF-8 even across replies.
                    let start_row = start >> 8;
                    for (index, record) in history.records(owner).filter(|(index, _)| *index >= start_row) {
                        let mut part = if index == start_row { start & 255 } else { 0 };
                        loop {
                            if parts_read >= max_parts {
                                next = (index << 8) | part;
                                break;
                            }
                            let Ok((text, more)) = messages_history::read_row_part(owner, record, part, page) else {
                                return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                            };
                            if length + text.len() + usize::from(!more) > output.len() {
                                next = (index << 8) | part;
                                break;
                            }
                            output[length..length + text.len()].copy_from_slice(text.as_bytes());
                            length += text.len();
                            parts_read += 1;
                            if !more {
                                output[length] = b'\n';
                                length += 1;
                                break;
                            }
                            part += 1;
                        }
                        if next != 0 { break; }
                    }
                    if !page.set_payload(&output[..length]) {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    set_mr(0, length as u64);
                    set_mr(1, next as u64);
                    return MessageInfo::new(REPLY_SLINT_HISTORY_PAGE, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_TRANSCRIPT, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let index = (get_mr(0) as usize).min(CHAT_SLOT_COUNT - 1);
                    if !self.ensure_messages_loaded() {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    let text = self.chat_transcripts.text(index);
                    if !unsafe { self.blob_transfer.as_mut() }.set_payload(text) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, text.len() as MessageRegisterValue);
                    debug_println!(
                        "SIMPLEXIPC|MESSAGES_APP|TRANSCRIPT|{}|{}",
                        index,
                        text.len()
                    );
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_TRANSCRIPT, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_IMAGE, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    match self.incoming_photo() {
                        Some((chat, jpeg)) => {
                            set_mr(0, chat as MessageRegisterValue);
                            set_mr(1, jpeg.len() as MessageRegisterValue);
                            debug_println!(
                                "SIMPLEXIPC|MESSAGES_APP|PHOTO_SERVE|{}|{}",
                                chat,
                                jpeg.len()
                            );
                        }
                        None => {
                            set_mr(0, 0);
                            set_mr(1, 0);
                        }
                    }
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_IMAGE, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_IMAGE_CHUNK, 3) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let chat = get_mr(0) as usize;
                    let id = get_mr(1);
                    let part = get_mr(2);
                    if chat >= CHAT_SLOT_COUNT || id == 0 {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let handle = self.simplex_agent_connection_handles[chat];
                    set_mr(0, id);
                    set_mr(1, 0);
                    let header = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
                    if header.label() != REPLY_OK || header.count() != 2
                        || (get_mr(0) != handle && get_mr(0) != crate::vault_protocol::LIBRARY_OWNER) {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let total = get_mr(1);
                    if part != 0 {
                        set_mr(0, id);
                        set_mr(1, part);
                        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_READ, 2));
                        if reply.label() != REPLY_OK || reply.count() != 0 {
                            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                        }
                    }
                    set_mr(0, total);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_IMAGE_CHUNK, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_IMAGE_CHUNK, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    // Read the offset first. Loading the saved picture calls
                    // the vault, and that call overwrites this register.
                    let offset = get_mr(0) as usize;
                    let copied = {
                        let Some((_, jpeg)) = self.incoming_photo() else {
                            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                        };
                        if offset > jpeg.len() {
                            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                        }
                        let end = (offset + 3000).min(jpeg.len());
                        let mut chunk = [0u8; 3000];
                        let n = end - offset;
                        chunk[..n].copy_from_slice(&jpeg[offset..end]);
                        (chunk, n)
                    };
                    if !unsafe { self.blob_transfer.as_mut() }.set_payload(&copied.0[..copied.1]) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, copied.1 as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_IMAGE_CHUNK, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_GROUP, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let length = self.simplex_agent_group_len;
                    if length > GROUP_URI_CAP {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    if length > 0
                        && !unsafe { self.blob_transfer.as_mut() }
                            .set_payload(unsafe { &GROUP_INVITE_URI[..length] })
                    {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, self.simplex_agent_group_chat as MessageRegisterValue);
                    set_mr(1, length as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_GROUP, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_DELETE, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let chat = get_mr(0) as usize;
                    if chat >= CHAT_SLOT_COUNT {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    self.request_agent_delete_connection(chat);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_DELETE, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_ARCHIVE, 2) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let chat = get_mr(0) as usize;
                    let archived = get_mr(1) != 0;
                    if chat >= CHAT_SLOT_COUNT || !self.ensure_messages_loaded() {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    self.chat_transcripts.set_archived(chat, archived);
                    if !self.persist_messages() {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, self.chat_transcripts.archived_mask() as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_ARCHIVE, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_ARCHIVED_MASK, 0) => {
                #[cfg(feature = "simplex-agent")]
                {
                    if !self.ensure_messages_loaded() {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, self.chat_transcripts.archived_mask() as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_ARCHIVED_MASK, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_PEER_PHOTO, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let index = get_mr(0) as usize;
                    if index >= CHAT_SLOT_COUNT {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let length = unsafe { PEER_PHOTO_LEN[index] };
                    set_mr(0, index as MessageRegisterValue);
                    set_mr(1, length as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_PEER_PHOTO, 2);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_PEER_PHOTO_CHUNK, 2) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let index = get_mr(0) as usize;
                    let offset = get_mr(1) as usize;
                    if index >= CHAT_SLOT_COUNT {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let length = unsafe { PEER_PHOTO_LEN[index] };
                    if offset > length || length > PEER_IMAGE_CAP {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let end = (offset + 3000).min(length);
                    let chunk = unsafe { &PEER_PHOTOS[index][offset..end] };
                    if !unsafe { self.blob_transfer.as_mut() }.set_payload(chunk) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, chunk.len() as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_PEER_PHOTO_CHUNK, 1);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_CREATE_GROUP, 2) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let length = get_mr(0) as usize;
                    if length == 0 || length > 32 {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    };
                    if payload.len() != length
                        || !payload.iter().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.')
                        })
                    {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let contact = get_mr(1) as usize;
                    if !self.request_agent_create_group(payload, contact) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_CREATE_GROUP, 0);
                }
                #[cfg(not(feature = "simplex-agent"))]
                return MessageInfo::new(REPLY_DENIED, 0);
            }
            (REQUEST_SLINT_MESSAGES_SET_PROFILE, 1) => {
                #[cfg(feature = "simplex-agent")]
                {
                    let length = get_mr(0) as usize;
                    if length == 0 || length > 48 {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    let Some(payload) = (unsafe { self.blob_transfer.as_ref() }).payload() else {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    };
                    if payload.len() != length
                        || !payload.iter().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.')
                        })
                    {
                        return MessageInfo::new(REPLY_BAD_REQUEST, 0);
                    }
                    if !self.request_agent_set_profile(payload) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    self.simplex_profile_name = [0; 48];
                    self.simplex_profile_name[..length].copy_from_slice(payload);
                    self.simplex_profile_name_length = length;
                    if !unsafe { self.blob_transfer.as_mut() }.set_payload(payload) {
                        return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
                    }
                    set_mr(0, length as MessageRegisterValue);
                    return MessageInfo::new(REPLY_SLINT_MESSAGES_SET_PROFILE, 1);
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
        {
            // A prior list reply may already be on the page if its notify
            // arrived while this PD was inside another protected call.
            self.poll_agent_list_response();
            if self.agent_connection_count.is_none() {
                self.request_agent_connection_snapshot();
            }
            self.refresh_pending_connection_metadata();
        }
        if !self.ensure_messages_loaded() {
            debug_println!("SLINT MESSAGES SERVICE LOAD FAILED: ensure_messages_loaded");
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }
        #[cfg(feature = "simplex-agent")]
        {
            // ListConnections often finishes during the vault read above.
            // Passive PDs do not run notified() until this PPC returns, so take
            // the reply here before building the Slint payload.
            self.poll_agent_list_response();
        }
        let mut display_text = [0; APP_VIEW_TEXT_CAPACITY];
        let displayed = self.messages.display(&mut display_text);
        if displayed.is_empty() {
            debug_println!("SLINT MESSAGES SERVICE LOAD FAILED: transfer capacity");
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }
        #[cfg(feature = "utm-transcript-worker")]
        let mut text = [0u8; personal_os_rust_server::vault_blob_record::MAX_PAYLOAD_SIZE];
        #[cfg(not(feature = "utm-transcript-worker"))]
        let mut text = [0u8; 1024];
        let mut payload_length = displayed.len().min(text.len());
        text[..payload_length].copy_from_slice(&displayed[..payload_length]);
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
            const CONTACT_LABEL: &[u8] = b"\nCONTACT ";
            let name = &self.simplex_agent_contact_name[..self.simplex_agent_contact_name_length];
            if !name.is_empty() && payload_length + CONTACT_LABEL.len() + name.len() < text.len() {
                text[payload_length..payload_length + CONTACT_LABEL.len()]
                    .copy_from_slice(CONTACT_LABEL);
                payload_length += CONTACT_LABEL.len();
                text[payload_length..payload_length + name.len()].copy_from_slice(name);
                payload_length += name.len();
            }
            const PROFILE_LABEL: &[u8] = b"\nPROFILE ";
            let profile = &self.simplex_profile_name[..self.simplex_profile_name_length];
            if !profile.is_empty()
                && payload_length + PROFILE_LABEL.len() + profile.len() < text.len()
            {
                text[payload_length..payload_length + PROFILE_LABEL.len()]
                    .copy_from_slice(PROFILE_LABEL);
                payload_length += PROFILE_LABEL.len();
                text[payload_length..payload_length + profile.len()].copy_from_slice(profile);
                payload_length += profile.len();
            }
            const CHAT_LABEL: &[u8] = b"\nCHAT ";
            const GROUP_LABEL: &[u8] = b"\nGROUP ";
            for index in 0..self.simplex_agent_chat_count.min(CHAT_SLOT_COUNT) {
                let chat_name = &self.simplex_agent_chat_names[index]
                    [..self.simplex_agent_chat_name_lengths[index]];
                let is_group = self.simplex_agent_group_mask & (1u16 << index) != 0;
                let label = if is_group { GROUP_LABEL } else { CHAT_LABEL };
                if chat_name.is_empty()
                    || payload_length + label.len() + chat_name.len() >= text.len()
                {
                    break;
                }
                text[payload_length..payload_length + label.len()].copy_from_slice(label);
                payload_length += label.len();
                text[payload_length..payload_length + chat_name.len()].copy_from_slice(chat_name);
                payload_length += chat_name.len();
            }
            #[cfg(feature = "utm-transcript-worker")]
            for index in 0..self.simplex_agent_chat_count.min(self.simplex_agent_connection_handles_length).min(CHAT_SLOT_COUNT) {
                const HANDLE: &[u8] = b"\nCHAT_HANDLE ";
                if payload_length + HANDLE.len() + 2 + 1 + 16 >= text.len() { break; }
                text[payload_length..payload_length + HANDLE.len()].copy_from_slice(HANDLE);
                payload_length += HANDLE.len();
                payload_length += write_u16_decimal(&mut text[payload_length..], index as u16);
                text[payload_length] = b' ';
                payload_length += 1;
                payload_length += write_u64_hex(&mut text[payload_length..], self.simplex_agent_connection_handles[index]);
            }
            const PHOTO_LABEL: &[u8] = b"\nPHOTO_MASK ";
            if payload_length + PHOTO_LABEL.len() + 5 < text.len() {
                text[payload_length..payload_length + PHOTO_LABEL.len()]
                    .copy_from_slice(PHOTO_LABEL);
                payload_length += PHOTO_LABEL.len();
                payload_length += write_u16_decimal(
                    &mut text[payload_length..],
                    self.simplex_agent_photo_mask,
                );
            }
        }
        if !unsafe { self.blob_transfer.as_mut() }.set_payload(&text[..payload_length]) {
            debug_println!("SLINT MESSAGES SERVICE LOAD FAILED: transfer capacity");
            return MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0);
        }
        #[cfg(feature = "simplex-agent")]
        {
            self.slint_displayed_handles = self.simplex_agent_connection_handles;
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
            match decode_archive(payload) {
                Some((messages, chats)) => {
                    self.messages = messages;
                    self.chat_transcripts = chats;
                    return self.render_messages_snapshot();
                }
                None => {
                    log(self.name, "FAIL: unrecognized messages archive");
                    return false;
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
        let mut payload = [0; ARCHIVE_CAPACITY];
        let Some(length) = encode_archive(&self.messages, &self.chat_transcripts, &mut payload)
        else {
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

    #[cfg(feature = "simplex-agent")]
    fn read_message_clock() -> Option<u32> {
        const CLOCK: Channel = Channel::new(2);
        const REQUEST_UNIX_TIME: MessageLabel = 0x205;
        const CLOCK_REPLY_OK: MessageLabel = 0x100;
        let reply = CLOCK.pp_call(MessageInfo::new(REQUEST_UNIX_TIME, 0));
        if reply.label() != CLOCK_REPLY_OK || reply.count() != 1 {
            return None;
        }
        let seconds = get_mr(0) as u32;
        if seconds < 1_000_000_000 {
            None
        } else {
            Some(seconds)
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn decode_base64(input: &[u8], out: *mut u8, cap: usize) -> Option<usize> {
        let mut value = 0u32;
        let mut bits = 0u32;
        let mut written = 0usize;
        for &byte in input {
            let digit = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' | b'\n' | b'\r' | b' ' => continue,
                _ => return None,
            };
            value = (value << 6) | u32::from(digit);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                if written >= cap {
                    return None;
                }
                unsafe {
                    out.add(written).write(((value >> bits) & 0xff) as u8);
                }
                written += 1;
            }
        }
        if written < 4 {
            return None;
        }
        let magic = unsafe { (out.read(), out.add(1).read()) };
        if magic != (0xff, 0xd8) {
            None
        } else {
            Some(written)
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn store_outgoing_image(&mut self, data_url: &[u8]) -> bool {
        let Some(comma) = data_url.iter().position(|byte| *byte == b',') else {
            return false;
        };
        if !data_url[..comma].starts_with(b"data:image/") {
            return false;
        }
        let mut decoded = [0u8; PEER_IMAGE_CAP];
        let Some(len) = Self::decode_base64(
            &data_url[comma + 1..],
            decoded.as_mut_ptr(),
            PEER_IMAGE_CAP,
        ) else {
            return false;
        };
        debug_println!("SIMPLEXIPC|MESSAGES_APP|SEND_PHOTO|{}", len);
        true
    }

    /// Expand Slint wire framing into a readable transcript body. Optional
    /// SharedMsgId is embedded as `\x1e#\x1f{id}\x1f` ahead of the display text.
    #[cfg(feature = "simplex-agent")]
    fn display_send_body(body: &[u8], shared_id: &[u8], out: &mut [u8]) -> usize {
        let mut len = 0usize;
        let push = |dst: &mut [u8], len: &mut usize, src: &[u8]| {
            let n = src.len().min(dst.len().saturating_sub(*len));
            dst[*len..*len + n].copy_from_slice(&src[..n]);
            *len += n;
        };
        if !shared_id.is_empty() && shared_id.len() <= 64 {
            push(out, &mut len, b"\x1e#\x1f");
            push(out, &mut len, shared_id);
            push(out, &mut len, b"\x1f");
        }
        if body.starts_with(b"\x1eQ\x1f") {
            let rest = &body[3..];
            let mut parts = rest.split(|&b| b == 0x1f);
            let _sent = parts.next();
            if let (Some(quote), Some(text)) = (parts.next(), parts.next()) {
                push(out, &mut len, "↩ ".as_bytes());
                let quote = if quote.len() > 80 { &quote[..80] } else { quote };
                push(out, &mut len, quote);
                push(out, &mut len, " · ".as_bytes());
                push(out, &mut len, text);
                for extra in parts {
                    push(out, &mut len, b"\x1f");
                    push(out, &mut len, extra);
                }
                return len;
            }
        }
        if body.starts_with(b"\x1eF\x1f") {
            push(out, &mut len, body.get(3..).unwrap_or(b""));
            return len;
        }
        if body.starts_with(b"\x1eL\x1f") {
            let rest = &body[3..];
            let mut parts = rest.splitn(2, |&byte| byte == 0x1f);
            if let (Some(name), Some(text)) = (parts.next(), parts.next()) {
                push(out, &mut len, name);
                push(out, &mut len, b": ");
                push(out, &mut len, text);
                return len;
            }
        }
        if body.starts_with(b"data:image/") {
            push(out, &mut len, b"Photo");
            return len;
        }
        push(out, &mut len, body);
        len
    }

    #[cfg(feature = "simplex-agent")]
    fn apply_send_transcript(&mut self, index: usize, body: &[u8], shared_id: &[u8]) {
        if body.starts_with(b"\x1eE\x1f") {
            let rest = &body[3..];
            let mut parts = rest.splitn(2, |&b| b == 0x1f);
            if let (Some(msg_id), Some(text)) = (parts.next(), parts.next()) {
                if let (Ok(id), Ok(text)) = (core::str::from_utf8(msg_id), core::str::from_utf8(text))
                {
                    self.rewrite_outgoing_by_msgid(index, id, Some(text));
                }
            }
            return;
        }
        if body.starts_with(b"\x1eD\x1f") {
            if let Ok(id) = core::str::from_utf8(&body[3..]) {
                self.rewrite_outgoing_by_msgid(index, id, None);
            }
            return;
        }
        if body.starts_with(b"\x1eR\x1f") {
            self.apply_reaction_wire(index, &body[3..]);
            return;
        }
        let key = MessageKey {
            source: u64::MAX,
            number: self.simplex_agent_send_message_id,
            part: 0,
        };
        if !self.record_chat_line_with_id(index, b"NexatrodeOS", body, shared_id, key) {
            self.simplex_agent_send_status = 4;
            debug_println!("SIMPLEXIPC|MESSAGES_APP|OUTGOING_HISTORY_FAIL");
        }
    }

    /// Wire: `{msgId}\x1f{emoji}[\x1f0]`. Annotate the matching bubble; no new line.
    #[cfg(feature = "simplex-agent")]
    fn apply_reaction_wire(&mut self, index: usize, rest: &[u8]) -> bool {
        let mut parts = rest.split(|&b| b == 0x1f);
        let Some(msg_id) = parts.next() else {
            return true;
        };
        let Some(emoji) = parts.next() else {
            return true;
        };
        if msg_id.is_empty() || emoji.is_empty() {
            return true;
        }
        let add = match parts.next() {
            Some(b"0") => false,
            _ => true,
        };
        if let (Ok(id), Ok(emoji)) = (core::str::from_utf8(msg_id), core::str::from_utf8(emoji)) {
            let emoji = emoji.chars().next().unwrap_or('\0');
            if emoji != '\0' {
                return self.apply_reaction(index, id, emoji, add);
            }
        }
        true
    }

    #[cfg(feature = "simplex-agent")]
    fn history_message_by_shared_id(&mut self, owner: u64, id: &str, out: &mut [u8]) -> Result<Option<(MessageKey, usize)>, ()> {
        use personal_os_rust_server::messages::split_message_time;

        for (_, record) in (unsafe { messages_history::index() }).records(owner) {
            let Some((_, Event::Row(key, line))) =
                messages_history::read(record, unsafe { self.blob_transfer.as_mut() })?
            else {
                continue;
            };
            if key.source != u64::MAX && !(key.source == 0 && line.starts_with("NexatrodeOS: ")) {
                continue;
            }
            let Some((_, body)) = line.split_once(": ") else {
                continue;
            };
            let (_, body) = split_message_time(body);
            if body.strip_prefix("\x1e#\x1f").and_then(|body| body.split_once('\x1f'))
                .is_some_and(|(candidate, _)| candidate == id)
            {
                out.get_mut(..line.len()).ok_or(())?.copy_from_slice(line.as_bytes());
                return Ok(Some((key, line.len())));
            }
        }
        Ok(None)
    }

    #[cfg(feature = "simplex-agent")]
    fn apply_reaction(&mut self, index: usize, msg_id: &str, emoji: char, add: bool) -> bool {
        use core::fmt::Write;
        use personal_os_rust_server::messages::TranscriptBuffer;
        const MARK: &str = "\x1e!\x1f";
        let Ok(owner) = self.ensure_chat_history(index) else {
            return false;
        };
        let mut existing = [0; vault_history::MAX_EVENT_SIZE];
        let (key, len) = match self.history_message_by_shared_id(owner, msg_id, &mut existing) {
            Ok(Some(row)) => row,
            Ok(None) => return true,
            Err(()) => return false,
        };
        let line = core::str::from_utf8(&existing[..len]).unwrap_or("");
        let suffix = if line.ends_with(" ✓") { " ✓" } else { "" };
        let body = line.trim_end_matches(" ✓");
        let (base, reactions) = body.split_once(MARK).unwrap_or((body, ""));
        if reactions.contains(emoji) == add {
            return true;
        }
        let mut bytes = [0; vault_history::MAX_EVENT_SIZE - vault_history::ROW_HEADER];
        let mut updated = TranscriptBuffer::new(&mut bytes);
        if updated.write_str(base).is_err() {
            return false;
        }
        let remaining = reactions.chars().filter(|&c| c != emoji);
        if (add || remaining.clone().next().is_some()) && updated.write_str(MARK).is_err() {
            return false;
        }
        for c in remaining.chain(add.then_some(emoji)) {
            if updated.write_char(c).is_err() {
                return false;
            }
        }
        updated.write_str(suffix).is_ok() && self.append_history_line(owner, key, updated.as_str())
    }

    #[cfg(feature = "simplex-agent")]
    fn rewrite_outgoing_by_msgid(&mut self, index: usize, msg_id: &str, new_text: Option<&str>) {
        use core::fmt::Write;
        use personal_os_rust_server::messages::TranscriptBuffer;
        let Ok(owner) = self.ensure_chat_history(index) else {
            return;
        };
        let mut existing = [0; vault_history::MAX_EVENT_SIZE];
        let Ok(Some((key, len))) = self.history_message_by_shared_id(owner, msg_id, &mut existing) else {
            return;
        };
        let line = core::str::from_utf8(&existing[..len]).unwrap_or("");
        let Some((prefix, _)) = line.split_once("\x1e#\x1f") else {
            return;
        };
        let mut bytes = [0; vault_history::MAX_EVENT_SIZE - vault_history::ROW_HEADER];
        let mut updated = TranscriptBuffer::new(&mut bytes);
        if write!(updated, "{prefix}\x1e#\x1f{msg_id}\x1f").is_err() {
            return;
        }
        for c in new_text.unwrap_or("Message deleted").chars() {
            if updated.write_char(if c == '\n' { '\u{2028}' } else { c }).is_err() {
                return;
            }
        }
        if !self.append_history_line(owner, key, updated.as_str()) {
            self.simplex_agent_send_status = 4;
        }
    }

    #[cfg(feature = "simplex-agent")]
    fn record_chat_line_with_id(
        &mut self,
        index: usize,
        speaker: &[u8],
        body: &[u8],
        shared_id: &[u8],
        key: MessageKey,
    ) -> bool {
        let Ok(owner) = self.ensure_chat_history(index) else {
            return false;
        };
        if unsafe { messages_history::index() }.record_for(owner, key).is_some() {
            return true;
        }
        let mut display = [0u8; 2112];
        let display_len = Self::display_send_body(body, shared_id, &mut display);
        let Ok(body) = core::str::from_utf8(&display[..display_len]) else {
            return false;
        };
        self.append_display_line(owner, key, speaker, body)
    }

    #[cfg(feature = "simplex-agent")]
    fn append_display_line(&mut self, owner: u64, key: MessageKey, speaker: &[u8], body: &str) -> bool {
        use core::fmt::Write;
        use personal_os_rust_server::messages::TranscriptBuffer;

        let Ok(speaker) = core::str::from_utf8(speaker) else {
            return false;
        };
        let mut stamped = [0; 2120];
        let stamped = Self::read_message_clock()
            .and_then(|seconds| prefix_message_time(seconds, body.as_bytes(), &mut stamped))
            .and_then(|length| core::str::from_utf8(&stamped[..length]).ok())
            .unwrap_or(body);
        let mut bytes = [0; vault_history::MAX_EVENT_SIZE - vault_history::ROW_HEADER];
        let mut line = TranscriptBuffer::new(&mut bytes);
        if write!(line, "{speaker}: ").is_err() {
            return false;
        }
        for character in stamped.chars() {
            let character = if character == '\n' { '\u{2028}' } else { character };
            if line.write_char(character).is_err() {
                return false;
            }
        }
        self.append_history_line(owner, key, line.as_str())
    }

    #[cfg(feature = "simplex-agent")]
    fn record_chat_line(&mut self, index: usize, speaker: &[u8], body: &[u8]) -> bool {
        let Ok(owner) = self.ensure_chat_history(index) else { return false };
        let Ok(body) = core::str::from_utf8(body) else { return false };
        let key = self.incoming_history_key();
        unsafe { messages_history::index() }.record_for(owner, key).is_some()
            || self.append_display_line(owner, key, speaker, body)
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
