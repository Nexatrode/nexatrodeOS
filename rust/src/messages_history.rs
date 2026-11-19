//! Messages PD index for durable history. Message bodies stay in the vault.

use core::ptr::NonNull;
use personal_os_rust_server::{
    vault_blob_transfer::BlobTransferPage,
    vault_history::{self, Event, MessageKey},
};
use sel4_microkit::{MessageInfo, get_mr, set_mr};

use crate::vault_protocol::{
    REPLY_NO_BLOB, REPLY_OK, REQUEST_HISTORY_APPEND, REQUEST_HISTORY_END, REQUEST_HISTORY_READ,
    REQUEST_PHOTO_PURGE_CHAT,
};

const VAULT: sel4_microkit::Channel = sel4_microkit::Channel::new(0);
// The vault enforces physical capacity; larger newly created partitions can
// use this larger index without moving the records on existing partitions.
const MAX_RECORDS: usize = vault_history::MAX_INDEXED_RECORDS;

#[derive(Clone, Copy)]
struct Row {
    owner: u64,
    key: MessageKey,
    record: u64,
    peer_id_hash: u64,
}

impl Row {
    const EMPTY: Self = Self {
        owner: 0,
        key: MessageKey::LOCAL,
        record: 0,
        peer_id_hash: 0,
    };
}

fn peer_hash(id: &str) -> u64 {
    id.bytes().fold(0xcbf29ce484222325, |hash, b| (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)) | 1
}

pub struct History {
    rows: [Row; MAX_RECORDS],
    scanned: usize,
    used: usize,
}

impl History {
    const fn new() -> Self {
        Self {
            rows: [Row::EMPTY; MAX_RECORDS],
            scanned: 0,
            used: 0,
        }
    }

    fn apply(&mut self, record: u64, owner: u64, event: Event<'_>) {
        match event {
            Event::Fragment(_) => {}
            Event::Row(key, _) => self.apply_row(record, owner, key, 0),
            Event::Peer(peer) => self.apply_row(record, owner, peer.version.key, peer_hash(peer.version.shared_id)),
            Event::Cleared => {
                for row in &mut self.rows[..self.used] {
                    if row.owner == owner && row.record != 0 {
                        row.owner = 0;
                    }
                }
                self.mark_migrated(owner);
            }
            Event::Migrated => self.mark_migrated(owner),
        }
    }

    fn apply_row(&mut self, record: u64, owner: u64, key: MessageKey, peer_id_hash: u64) {
        let row = Row { owner, key, record, peer_id_hash };
        if let Some(old) = self.find_mut(owner, key) {
            *old = row;
        } else {
            self.rows[self.used] = row;
            self.used += 1;
        }
    }

    /// Hash only narrows vault reads. The caller compares the full stored ID.
    pub fn peer_records(&self, owner: u64, source: u64, id: &str) -> impl Iterator<Item = u64> + '_ {
        let hash = peer_hash(id);
        self.rows[..self.used].iter().filter(move |row| row.owner == owner
            && row.key.source == source && row.record != 0 && row.peer_id_hash == hash)
            .map(|row| row.record)
    }

    fn mark_migrated(&mut self, owner: u64) {
        if !self.migrated(owner) {
            self.rows[self.used] = Row {
                owner,
                ..Row::EMPTY
            };
            self.used += 1;
        }
    }

    fn migrated(&self, owner: u64) -> bool {
        self.rows[..self.used]
            .iter()
            .any(|row| row.owner == owner && row.record == 0)
    }

    fn find_mut(&mut self, owner: u64, key: MessageKey) -> Option<&mut Row> {
        if key == MessageKey::LOCAL {
            return None;
        }
        self.rows[..self.used]
            .iter_mut()
            .find(|row| row.owner == owner && row.record != 0 && row.key == key)
    }

    pub fn record_for(&self, owner: u64, key: MessageKey) -> Option<u64> {
        if key == MessageKey::LOCAL {
            return None;
        }
        self.rows[..self.used]
            .iter()
            .find(|row| row.owner == owner && row.record != 0 && row.key == key)
            .map(|row| row.record)
    }

    pub fn records(&self, owner: u64) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.rows[..self.used]
            .iter()
            .enumerate()
            .filter(move |(_, row)| row.owner == owner && row.record != 0)
            .map(|(index, row)| (index, row.record))
    }

    pub fn revision(&self) -> u64 {
        self.scanned as u64
    }

    pub fn row_at(&self, index: usize) -> Option<(u64, MessageKey, u64)> {
        let row = self.rows.get(index).filter(|row| row.owner != 0 && row.record != 0)?;
        Some((row.owner, row.key, row.record))
    }

    pub fn row_count(&self) -> usize { self.used }

    pub fn refresh(&mut self, mut page: NonNull<BlobTransferPage>) -> Result<(), ()> {
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_HISTORY_END, 0));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            return Err(());
        }
        let end = get_mr(0) as usize;
        if end < self.scanned || end > MAX_RECORDS {
            return Err(());
        }
        for id in self.scanned + 1..=end {
            if let Some((owner, event)) = read(id as u64, unsafe { page.as_mut() })? {
                if matches!(event, Event::Cleared) {
                    set_mr(0, owner);
                    let reply = VAULT.pp_call(MessageInfo::new(REQUEST_PHOTO_PURGE_CHAT, 1));
                    if reply.label() != REPLY_OK || reply.count() != 0 {
                        return Err(());
                    }
                }
                self.apply(id as u64, owner, event);
            }
            self.scanned = id;
        }
        Ok(())
    }

    pub fn append(
        &mut self, owner: u64, bytes: &[u8], page: NonNull<BlobTransferPage>,
    ) -> Result<(), ()> {
        self.append_record(owner, bytes, page).map(|_| ())
    }

    pub fn append_record(
        &mut self,
        owner: u64,
        bytes: &[u8],
        mut page: NonNull<BlobTransferPage>,
    ) -> Result<u64, ()> {
        let event = vault_history::decode_event(bytes).ok_or(())?;
        if owner == 0 || self.scanned >= MAX_RECORDS || !unsafe { page.as_mut() }.set_payload(bytes)
        {
            return Err(());
        }
        set_mr(0, owner);
        let reply = VAULT.pp_call(MessageInfo::new(REQUEST_HISTORY_APPEND, 1));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            return Err(());
        }
        let record = get_mr(0);
        if matches!(event, Event::Cleared) || record != self.scanned as u64 + 1 {
            // A previous append may have reserved an ID before losing power.
            self.refresh(page)?;
            return Ok(record);
        }
        self.apply(record, owner, event);
        self.scanned += 1;
        Ok(record)
    }

    pub fn migrate(
        &mut self,
        owner: u64,
        legacy: &str,
        page: NonNull<BlobTransferPage>,
    ) -> Result<(), ()> {
        if self.migrated(owner) {
            return Ok(());
        }
        let mut encoded = [0; vault_history::MAX_EVENT_SIZE];
        for (index, line) in legacy.lines().enumerate() {
            let mut key = MessageKey {
                source: 0,
                number: index as u64 + 1,
                part: 0,
            };
            if !line.starts_with("NexatrodeOS: ") {
                if let Some((_, body)) = line.split_once(": ") {
                    let (_, body) = personal_os_rust_server::messages::split_message_time(body);
                    if let Some((agent_id, _, _)) =
                        personal_os_rust_server::messages::split_photo_reference(body)
                    {
                        key = MessageKey {
                            source: owner,
                            number: agent_id,
                            part: 0,
                        };
                    }
                }
            }
            if line.is_empty() || self.record_for(owner, key).is_some() {
                continue;
            }
            let len = vault_history::encode_row(key, line, &mut encoded).ok_or(())?;
            self.append(owner, &encoded[..len], page)?;
        }
        self.append(owner, vault_history::MIGRATED, page)
    }
}

static mut HISTORY: History = History::new();

/// The Messages PD invokes this from its serialized Microkit handlers.
pub unsafe fn index() -> &'static mut History {
    unsafe { &mut *core::ptr::addr_of_mut!(HISTORY) }
}

pub fn read(record: u64, page: &mut BlobTransferPage) -> Result<Option<(u64, Event<'_>)>, ()> {
    set_mr(0, record);
    let reply = VAULT.pp_call(MessageInfo::new(REQUEST_HISTORY_READ, 1));
    if reply.label() == REPLY_NO_BLOB && reply.count() == 0 {
        return Ok(None);
    }
    if reply.label() != REPLY_OK || reply.count() != 2 {
        return Err(());
    }
    let owner = get_mr(0);
    // Library metadata shares the encrypted history log but is indexed by the
    // library service, not the chat event decoder.
    if owner == crate::vault_protocol::LIBRARY_OWNER {
        return Ok(None);
    }
    let len = get_mr(1) as usize;
    let bytes = page
        .payload()
        .filter(|bytes| bytes.len() == len)
        .ok_or(())?;
    Ok(Some((owner, vault_history::decode_event(bytes).ok_or(())?)))
}

/// Return one UTF-8 part of an indexed row. Check ownership again after the
/// second vault read so a fragment reference cannot cross a chat boundary.
pub fn read_row_part(
    owner: u64, record: u64, part: usize, page: &mut BlobTransferPage,
) -> Result<(&str, bool), ()> {
    let page_ptr = NonNull::from(&mut *page);
    let (stored_owner, event) = read(record, page)?.ok_or(())?;
    if owner != stored_owner { return Err(()); }
    let (fragment, more) = match event {
        Event::Row(_, line) if part == 0 => return Ok((line, false)),
        Event::Peer(row) => {
            if part == 0 { return Ok((row.first, row.fragment_ids().next().is_some())); }
            let mut ids = row.fragment_ids().skip(part - 1);
            (ids.next().ok_or(())?, ids.next().is_some())
        }
        _ => return Err(()),
    };
    if fragment == 0 || fragment >= record { return Err(()); }
    // The head view is consumed before the shared transfer page is reused.
    match read(fragment, unsafe { &mut *page_ptr.as_ptr() })? {
        Some((fragment_owner, Event::Fragment(text))) if fragment_owner == owner => Ok((text, more)),
        _ => Err(()),
    }
}
