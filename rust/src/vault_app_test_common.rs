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
    DEVICE_KEYBOARD, DEVICE_TABLET, EVENT_KEY, REPLY_ROUTED_EVENT_BAD, REPLY_ROUTED_EVENT_OK,
    REQUEST_ROUTED_EVENT, decode_routed_event,
};
use crate::notes::{NOTES_BLOB_CAPACITY, NotesApp};
use crate::vault_protocol::{
    APP_TEST_SUPERVISOR_CHANNEL, INITIAL_VALUE, REPLY_BAD_REQUEST, REPLY_DENIED,
    REPLY_NAMESPACE_TEST, REPLY_NO_BLOB, REPLY_NO_NET, REPLY_OK, REPLY_NET_TX_OK,
    REPLY_SEND_SEAL_DEMO_OK, REPLY_SEND_UI_SEALED_OK, REQUEST_NAMESPACE_TEST, REQUEST_NET_TX,
    REQUEST_READ, REQUEST_READ_BLOB, REQUEST_SEND_SEAL_DEMO, REQUEST_SEND_UI_SEALED,
    REQUEST_WRITE, REQUEST_WRITE_BLOB,
};
use personal_os_rust_server::calendar::{CALENDAR_BLOB_CAPACITY, Calendar};
use personal_os_rust_server::contacts::{CONTACTS_BLOB_CAPACITY, ContactsApp};
use personal_os_rust_server::message_seal::{self, MAX_PACKET};
use personal_os_rust_server::messages::{MESSAGES_BLOB_CAPACITY, Messages};
use personal_os_rust_server::messages_net_tx::MessagesNetTxPage;
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;

const VAULT: Channel = Channel::new(0);
const ROUTED_INPUT: Channel = Channel::new(2);
const NET_TX: Channel = Channel::new(3);

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
        }
    }

    fn run_namespace_checks(&self) -> bool {
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
        if channel == ROUTED_INPUT {
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
        if request.label() != REQUEST_NAMESPACE_TEST || request.count() != 0 {
            return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
        }
        let label: MessageLabel = if self.run_namespace_checks() {
            REPLY_NAMESPACE_TEST
        } else {
            REPLY_BAD_REQUEST
        };
        Ok(MessageInfo::new(label, 0))
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
                    let view = unsafe { self.app_view.as_ref() };
                    debug_println!(
                        "UIAPP|{}|VIEW: revision={} length={}",
                        self.name,
                        view.revision.load(core::sync::atomic::Ordering::SeqCst),
                        view.length
                    );
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
                    let view = unsafe { self.app_view.as_ref() };
                    debug_println!(
                        "UIAPP|{}|VIEW: revision={} length={}",
                        self.name,
                        view.revision.load(core::sync::atomic::Ordering::SeqCst),
                        view.length
                    );
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
                    let view = unsafe { self.app_view.as_ref() };
                    debug_println!(
                        "UIAPP|{}|VIEW: revision={} length={}",
                        self.name,
                        view.revision.load(core::sync::atomic::Ordering::SeqCst),
                        view.length
                    );
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
                    let view = unsafe { self.app_view.as_ref() };
                    debug_println!(
                        "UIAPP|{}|VIEW: revision={} length={}",
                        self.name,
                        view.revision.load(core::sync::atomic::Ordering::SeqCst),
                        view.length
                    );
                }
            }
        }
        debug_println!(
            "UIAPP|{}|EVENT: device={} type={} code={} value={} seq={}",
            self.name,
            device_name(event.device),
            event.event_type,
            event.code,
            event.value,
            sequence
        );
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
                    log(self.name, "LOG: messages blob format mismatch; re-initializing");
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
        let length = self.messages.display(&mut text).len();
        unsafe { self.app_view.as_mut() }.set_display_text(&text[..length])
    }

    fn persist_notes(&mut self) -> bool {
        let mut payload = [0; NOTES_BLOB_CAPACITY];
        let Some(length) = self.notes.encode(&mut payload) else {
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

const fn device_name(device: u16) -> &'static str {
    match device {
        DEVICE_KEYBOARD => "keyboard",
        DEVICE_TABLET => "tablet",
        _ => "unknown",
    }
}
