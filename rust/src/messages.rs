//! Offline Messages peer-key shell with vault-backed PMS2 sealed stubs.

use crate::message_seal::{self, KEY_ID_SIZE, MAX_PACKET};
use crate::vault_blob_record::MAX_PAYLOAD_SIZE;

pub const PEER_CAPACITY: usize = 3;
pub const KEY_CAPACITY: usize = 8;
pub const BODY_CAPACITY: usize = 32;
pub const MAX_SEALED_BYTES: usize = MAX_PACKET;
pub const MESSAGES_BLOB_CAPACITY: usize =
    6 + PEER_CAPACITY * (1 + KEY_CAPACITY + 1 + 2 + MAX_SEALED_BYTES);
const MAGIC: [u8; 4] = *b"PMS2";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessagesUpdate {
    pub changed: bool,
    pub persist: bool,
    /// Compose sealed a PSL1 packet; net merge may TX it when NET1 is ready.
    pub tx_sealed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditPhase {
    Browse,
    Key,
    Compose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Peer {
    key: [u8; KEY_CAPACITY],
    sealed_count: u8,
    sealed_len: u16,
    sealed: [u8; MAX_SEALED_BYTES],
}

impl Peer {
    const EMPTY: Self = Self {
        key: [0; KEY_CAPACITY],
        sealed_count: 0,
        sealed_len: 0,
        sealed: [0; MAX_SEALED_BYTES],
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Messages {
    peers: [Peer; PEER_CAPACITY],
    count: usize,
    selected: usize,
    phase: EditPhase,
    draft_length: usize,
    body: [u8; BODY_CAPACITY],
    body_length: usize,
}

impl Messages {
    pub const fn new() -> Self {
        Self {
            peers: [Peer::EMPTY; PEER_CAPACITY],
            count: 0,
            selected: 0,
            phase: EditPhase::Browse,
            draft_length: 0,
            body: [0; BODY_CAPACITY],
            body_length: 0,
        }
    }

    pub fn display<'a>(&'a self, buffer: &'a mut [u8; 128]) -> &'a [u8] {
        buffer.fill(0);
        match self.phase {
            EditPhase::Browse if self.count == 0 => {
                let empty = b"NO THREADS\nENTER TO ADD";
                buffer[..empty.len()].copy_from_slice(empty);
                &buffer[..empty.len()]
            }
            EditPhase::Browse => {
                let mut offset = 0;
                for index in 0..self.count {
                    if index > 0 {
                        offset = write(buffer, offset, b"\n");
                    }
                    let marker = if index == self.selected { b"- " } else { b"  " };
                    offset = write(buffer, offset, marker);
                    offset = write(buffer, offset, b"KEY ");
                    offset = write(buffer, offset, &self.peers[index].key);
                    if self.peers[index].sealed_count > 0 {
                        offset = write(buffer, offset, b"\n  SEALED ");
                        offset = write_digit(buffer, offset, self.peers[index].sealed_count);
                    }
                }
                &buffer[..offset]
            }
            EditPhase::Key => {
                // Progress so Enter-before-8 is obvious (Contacts-style clarity).
                let mut offset = write(buffer, 0, b"ADD KEY ");
                offset = write_digit(buffer, offset, self.draft_length as u8);
                offset = write(buffer, offset, b" OF 8\n");
                offset = write(
                    buffer,
                    offset,
                    &self.peers[self.selected].key[..self.draft_length],
                );
                &buffer[..offset]
            }
            EditPhase::Compose => {
                // Show which peer key this seal targets.
                let mut offset = write(buffer, 0, b"COMPOSE KEY ");
                offset = write(buffer, offset, &self.peers[self.selected].key);
                offset = write(buffer, offset, b"\n");
                offset = write(buffer, offset, &self.body[..self.body_length]);
                &buffer[..offset]
            }
        }
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() < 6 || payload[..4] != MAGIC {
            return None;
        }
        let count = payload[4] as usize;
        let selected = payload[5] as usize;
        if count > PEER_CAPACITY
            || (count > 0 && selected >= count)
            || (count == 0 && selected != 0)
        {
            return None;
        }
        let mut messages = Self::new();
        messages.count = count;
        messages.selected = selected;
        let mut cursor = 6;
        for index in 0..count {
            let key_length = *payload.get(cursor)? as usize;
            cursor += 1;
            if key_length != KEY_CAPACITY {
                return None;
            }
            let key_end = cursor.checked_add(key_length)?;
            let key = payload.get(cursor..key_end)?;
            if key.iter().any(|byte| !is_fingerprint_byte(*byte)) {
                return None;
            }
            cursor = key_end;
            let sealed_count = *payload.get(cursor)?;
            cursor += 1;
            let sealed_len = u16::from_le_bytes(payload.get(cursor..cursor + 2)?.try_into().ok()?);
            cursor += 2;
            let sealed_len_usize = sealed_len as usize;
            if sealed_len_usize > MAX_SEALED_BYTES {
                return None;
            }
            let sealed_end = cursor.checked_add(sealed_len_usize)?;
            let sealed = payload.get(cursor..sealed_end)?;
            cursor = sealed_end;
            let mut peer = Peer::EMPTY;
            peer.key.copy_from_slice(key);
            peer.sealed_count = sealed_count;
            peer.sealed_len = sealed_len;
            peer.sealed[..sealed_len_usize].copy_from_slice(sealed);
            messages.peers[index] = peer;
        }
        if cursor != payload.len() {
            return None;
        }
        Some(messages)
    }

    pub fn encode(&self, output: &mut [u8; MESSAGES_BLOB_CAPACITY]) -> Option<usize> {
        if self.phase != EditPhase::Browse || self.count > PEER_CAPACITY {
            return None;
        }
        if self.count > 0 && self.selected >= self.count {
            return None;
        }
        output.fill(0);
        output[..4].copy_from_slice(&MAGIC);
        output[4] = self.count as u8;
        output[5] = self.selected as u8;
        let mut cursor = 6;
        for index in 0..self.count {
            let peer = &self.peers[index];
            if peer.key.iter().any(|byte| !is_fingerprint_byte(*byte)) {
                return None;
            }
            let sealed_len = peer.sealed_len as usize;
            if sealed_len > MAX_SEALED_BYTES {
                return None;
            }
            output[cursor] = KEY_CAPACITY as u8;
            cursor += 1;
            output[cursor..cursor + KEY_CAPACITY].copy_from_slice(&peer.key);
            cursor += KEY_CAPACITY;
            output[cursor] = peer.sealed_count;
            cursor += 1;
            output[cursor..cursor + 2].copy_from_slice(&peer.sealed_len.to_le_bytes());
            cursor += 2;
            output[cursor..cursor + sealed_len].copy_from_slice(&peer.sealed[..sealed_len]);
            cursor += sealed_len;
        }
        Some(cursor)
    }

    pub fn apply_key(&mut self, code: u16, value: u32) -> MessagesUpdate {
        if value != 1 && value != 2 {
            return unchanged();
        }
        match code {
            KEY_ENTER => self.press_enter(),
            KEY_UP => bool_update(self.press_up(), false),
            KEY_DOWN => bool_update(self.press_down(), false),
            KEY_DELETE => bool_update(self.press_delete(), true),
            KEY_BACKSPACE => bool_update(self.press_backspace(), false),
            KEY_INSERT => bool_update(self.press_insert_add(), false),
            _ => {
                if self.phase == EditPhase::Compose {
                    if let Some(letter) = evdev_letter(code) {
                        return bool_update(self.press_body(letter), false);
                    }
                    return unchanged();
                }
                if let Some(character) = fingerprint_character(code) {
                    bool_update(self.press_fingerprint(character), false)
                } else {
                    unchanged()
                }
            }
        }
    }

    fn press_enter(&mut self) -> MessagesUpdate {
        match self.phase {
            EditPhase::Browse => {
                if self.count > 0 {
                    self.body = [0; BODY_CAPACITY];
                    self.body_length = 0;
                    self.phase = EditPhase::Compose;
                    return changed(false);
                }
                if self.count < PEER_CAPACITY {
                    self.selected = self.count;
                    self.peers[self.selected] = Peer::EMPTY;
                    self.draft_length = 0;
                    self.phase = EditPhase::Key;
                    return changed(false);
                }
                unchanged()
            }
            EditPhase::Key => {
                if self.draft_length == 0 {
                    self.phase = EditPhase::Browse;
                    if self.selected == self.count && self.count > 0 {
                        self.selected = self.count - 1;
                    }
                    return changed(false);
                }
                if self.draft_length != KEY_CAPACITY {
                    return unchanged();
                }
                if self.selected == self.count {
                    self.count += 1;
                }
                self.phase = EditPhase::Browse;
                changed(true)
            }
            EditPhase::Compose => {
                if self.body_length == 0 {
                    self.phase = EditPhase::Browse;
                    return changed(false);
                }
                if !self.seal_body() {
                    return unchanged();
                }
                self.phase = EditPhase::Browse;
                sealed_persist()
            }
        }
    }

    /// Latest sealed PSL1 packet for the selected peer, if any.
    pub fn selected_sealed(&self) -> Option<&[u8]> {
        if self.count == 0 || self.selected >= self.count {
            return None;
        }
        let peer = &self.peers[self.selected];
        let len = peer.sealed_len as usize;
        if peer.sealed_count == 0 || len == 0 || len > MAX_SEALED_BYTES {
            None
        } else {
            Some(&peer.sealed[..len])
        }
    }

    fn seal_body(&mut self) -> bool {
        let peer = &mut self.peers[self.selected];
        let mut key_id = [0u8; KEY_ID_SIZE];
        key_id.copy_from_slice(&peer.key);
        let key = message_seal::messages_demo_key(&key_id);
        let mut nonce = [0u8; 12];
        nonce[0] = peer.sealed_count.wrapping_add(1);
        let mut packet = [0u8; MAX_PACKET];
        let Some(len) = message_seal::seal(
            &key,
            &key_id,
            &nonce,
            &self.body[..self.body_length],
            &mut packet,
        ) else {
            return false;
        };
        peer.sealed[..len].copy_from_slice(&packet[..len]);
        peer.sealed_len = len as u16;
        peer.sealed_count = peer.sealed_count.saturating_add(1);
        self.body = [0; BODY_CAPACITY];
        self.body_length = 0;
        true
    }

    fn press_insert_add(&mut self) -> bool {
        if self.phase != EditPhase::Browse || self.count >= PEER_CAPACITY {
            return false;
        }
        self.selected = self.count;
        self.peers[self.selected] = Peer::EMPTY;
        self.draft_length = 0;
        self.phase = EditPhase::Key;
        true
    }

    fn press_up(&mut self) -> bool {
        if self.phase != EditPhase::Browse || self.count == 0 || self.selected == 0 {
            return false;
        }
        self.selected -= 1;
        true
    }

    fn press_down(&mut self) -> bool {
        if self.phase != EditPhase::Browse || self.count == 0 || self.selected + 1 >= self.count {
            return false;
        }
        self.selected += 1;
        true
    }

    fn press_delete(&mut self) -> bool {
        if self.phase != EditPhase::Browse || self.count == 0 {
            return false;
        }
        let index = self.selected;
        for shift in index..self.count - 1 {
            self.peers[shift] = self.peers[shift + 1];
        }
        self.peers[self.count - 1] = Peer::EMPTY;
        self.count -= 1;
        if self.count == 0 {
            self.selected = 0;
        } else if self.selected >= self.count {
            self.selected = self.count - 1;
        }
        true
    }

    fn press_backspace(&mut self) -> bool {
        match self.phase {
            EditPhase::Key if self.draft_length > 0 => {
                self.draft_length -= 1;
                self.peers[self.selected].key[self.draft_length] = 0;
                true
            }
            EditPhase::Compose if self.body_length > 0 => {
                self.body_length -= 1;
                self.body[self.body_length] = 0;
                true
            }
            _ => false,
        }
    }

    fn press_fingerprint(&mut self, character: u8) -> bool {
        if self.phase != EditPhase::Key || self.draft_length >= KEY_CAPACITY {
            return false;
        }
        self.peers[self.selected].key[self.draft_length] = character;
        self.draft_length += 1;
        true
    }

    fn press_body(&mut self, letter: u8) -> bool {
        if self.body_length >= BODY_CAPACITY {
            return false;
        }
        self.body[self.body_length] = letter;
        self.body_length += 1;
        true
    }
}

impl Default for Messages {
    fn default() -> Self {
        Self::new()
    }
}

const KEY_BACKSPACE: u16 = 14;
const KEY_ENTER: u16 = 28;
const KEY_UP: u16 = 103;
const KEY_DOWN: u16 = 108;
const KEY_INSERT: u16 = 110;
const KEY_DELETE: u16 = 111;

fn write(buffer: &mut [u8], offset: usize, bytes: &[u8]) -> usize {
    let end = (offset + bytes.len()).min(buffer.len());
    let copy = end - offset;
    buffer[offset..end].copy_from_slice(&bytes[..copy]);
    end
}

fn write_digit(buffer: &mut [u8], offset: usize, value: u8) -> usize {
    let digit = b'0' + value.min(9);
    write(buffer, offset, &[digit])
}

const fn unchanged() -> MessagesUpdate {
    MessagesUpdate {
        changed: false,
        persist: false,
        tx_sealed: false,
    }
}

const fn changed(persist: bool) -> MessagesUpdate {
    MessagesUpdate {
        changed: true,
        persist,
        tx_sealed: false,
    }
}

const fn sealed_persist() -> MessagesUpdate {
    MessagesUpdate {
        changed: true,
        persist: true,
        tx_sealed: true,
    }
}

const fn bool_update(changed_flag: bool, persist: bool) -> MessagesUpdate {
    MessagesUpdate {
        changed: changed_flag,
        persist: changed_flag && persist,
        tx_sealed: false,
    }
}

const fn is_fingerprint_byte(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'A'..=b'F')
}

fn fingerprint_character(code: u16) -> Option<u8> {
    if let Some(digit) = evdev_digit(code) {
        return Some(digit);
    }
    match evdev_letter(code) {
        Some(letter @ b'A'..=b'F') => Some(letter),
        _ => None,
    }
}

const fn evdev_letter(code: u16) -> Option<u8> {
    match code {
        30 => Some(b'A'),
        48 => Some(b'B'),
        46 => Some(b'C'),
        32 => Some(b'D'),
        18 => Some(b'E'),
        33 => Some(b'F'),
        34 => Some(b'G'),
        35 => Some(b'H'),
        23 => Some(b'I'),
        36 => Some(b'J'),
        37 => Some(b'K'),
        38 => Some(b'L'),
        50 => Some(b'M'),
        49 => Some(b'N'),
        24 => Some(b'O'),
        25 => Some(b'P'),
        16 => Some(b'Q'),
        19 => Some(b'R'),
        31 => Some(b'S'),
        20 => Some(b'T'),
        22 => Some(b'U'),
        47 => Some(b'V'),
        17 => Some(b'W'),
        45 => Some(b'X'),
        21 => Some(b'Y'),
        44 => Some(b'Z'),
        _ => None,
    }
}

/// One transcript per chat. PMS3 wraps the existing PMS2 peer-key blob so an
/// older messages record still loads and is not rewritten empty. A record
/// written with four slots still decodes; the extra slots stay empty.
/// Each slot keeps its newest lines, and the whole archive stays inside one
/// vault blob so eight chats can be stored together.
pub const TRANSCRIPT_LIMIT: usize = 700;
/// One received JPEG kept with the archive. Larger pictures still show until reboot.
pub const PHOTO_CAP: usize = 2048;
const PHOTO_MAGIC: [u8; 4] = *b"IMG1";
pub const CHAT_SLOT_COUNT: usize = 16;
/// One connection handle per chat, stored after the archive mask.
const HANDLE_TRAILER: usize = CHAT_SLOT_COUNT * 8;
/// One generation after the stranded-line repair, so a later boot does not move them again.
const EPOCH_TRAILER: usize = 2;
pub const TRANSCRIPT_BUDGET: usize =
    MAX_PAYLOAD_SIZE - 6 - MESSAGES_BLOB_CAPACITY - 2 - HANDLE_TRAILER - EPOCH_TRAILER;
const ARCHIVE_MAGIC: [u8; 4] = *b"PMS3";
pub const ARCHIVE_CAPACITY: usize = 6
    + MESSAGES_BLOB_CAPACITY
    + CHAT_SLOT_COUNT * (2 + TRANSCRIPT_LIMIT)
    + 2
    + HANDLE_TRAILER
    + EPOCH_TRAILER;

const MESSAGE_CLOCK_MARK: u8 = 0x1f;

/// Prefix a new transcript body with `HH:MM` and a unit separator.
/// Seconds before 2001-09-09 are treated as an unset RTC and left unstamped.
/// A body that already carries the prefix is left unchanged.
pub fn prefix_message_time(unix_seconds: u32, body: &[u8], out: &mut [u8]) -> Option<usize> {
    if unix_seconds < 1_000_000_000 {
        return None;
    }
    if split_message_time(core::str::from_utf8(body).ok()?)
        .0
        .is_some()
    {
        return None;
    }
    let needed = 6 + body.len();
    if needed > out.len() {
        return None;
    }
    let day = unix_seconds % 86_400;
    let hours = day / 3_600;
    let minutes = (day / 60) % 60;
    out[0] = b'0' + (hours / 10) as u8;
    out[1] = b'0' + (hours % 10) as u8;
    out[2] = b':';
    out[3] = b'0' + (minutes / 10) as u8;
    out[4] = b'0' + (minutes % 10) as u8;
    out[5] = MESSAGE_CLOCK_MARK;
    out[6..needed].copy_from_slice(body);
    Some(needed)
}

/// Split a stored body into its UTC `HH:MM` prefix and the text that follows.
/// The delivery check stays on the text.
pub fn split_message_time(body: &str) -> (Option<&str>, &str) {
    let bytes = body.as_bytes();
    if bytes.len() >= 6
        && bytes[2] == b':'
        && bytes[5] == MESSAGE_CLOCK_MARK
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4].is_ascii_digit()
    {
        (Some(&body[..5]), &body[6..])
    } else {
        (None, body)
    }
}

/// Stored on the outgoing attachment row when XFTP reports a terminal failure.
pub const UPLOAD_FAILED_SUFFIX: &str = "\x1eX\x1f";

/// A delivery receipt confirms the SMP offer. The XFTP transfer can still fail.
pub fn outgoing_notice_update(line: &str, failed_upload: bool) -> Option<(&str, &'static str)> {
    if line.ends_with(UPLOAD_FAILED_SUFFIX) {
        return None;
    }
    if failed_upload {
        Some((line.strip_suffix(" ✓").unwrap_or(line), UPLOAD_FAILED_SUFFIX))
    } else if line.ends_with(" ✓") {
        None
    } else {
        Some((line, " ✓"))
    }
}

pub const PHOTO_REFERENCE_PREFIX: &str = "\x1eP\x1f";
pub const ORIGINAL_PHOTO_PREFIX: &str = "\x1eO\x1f";
pub const FILE_REFERENCE_PREFIX: &str = "\x1eF\x1f";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReceivedAttachment {
    Preview,
    Original,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhotoReference<'a> {
    pub message_id: u64,
    pub save_id: u64,
    pub display_id: u64,
    pub caption: &'a str,
    pub original: bool,
}

pub fn photo_reference(body: &str) -> Option<PhotoReference<'_>> {
    if let Some((message_id, save_id, rest)) = split_attachment_reference(body, ORIGINAL_PHOTO_PREFIX) {
        let (display, caption) = rest.split_once('\x1f')?;
        let display_id = display.parse::<u64>().ok()?;
        return Some(PhotoReference { message_id, save_id, display_id, caption, original: true });
    }
    let (message_id, save_id, caption) = split_attachment_reference(body, PHOTO_REFERENCE_PREFIX)?;
    Some(PhotoReference { message_id, save_id, display_id: save_id, caption, original: false })
}

pub fn split_photo_reference(body: &str) -> Option<(u64, u64, &str)> {
    photo_reference(body).map(|r| (r.message_id, r.save_id, r.caption))
}

/// A button can still carry the preview ID between a vault commit and a repaint.
pub fn current_photo_save_id(transcript: &str, selected: u64) -> Option<u64> {
    transcript.lines().find_map(|line| {
        let (_, body) = line.split_once(": ")?;
        let photo = photo_reference(split_message_time(body).1)?;
        (selected != 0 && (photo.save_id == selected || photo.display_id == selected))
            .then_some(photo.save_id)
    })
}

impl ReceivedAttachment {
    /// Decide before allocating another vault object, including after a reboot.
    pub fn needs_blob(self, current: &str) -> bool {
        let photo = photo_reference(current);
        match self {
            Self::Preview => photo.is_none_or(|r| r.display_id == 0),
            Self::Original => photo.is_none_or(|r| !r.original),
            Self::File => split_file_reference(current).is_none(),
        }
    }

    pub fn write_reference(
        self, output: &mut impl core::fmt::Write, current: &str, message_id: u64,
        blob_id: u64, jpeg: bool, caption: &str,
    ) -> core::fmt::Result {
        let previous = photo_reference(current);
        match self {
            Self::Original => {
                let display_id = previous.map(|r| r.display_id)
                    .unwrap_or(if jpeg { blob_id } else { 0 });
                write!(output, "{ORIGINAL_PHOTO_PREFIX}{message_id}\x1f{blob_id}\x1f{display_id}\x1f")?;
            }
            Self::Preview => {
                if let Some(original) = previous.filter(|r| r.original) {
                    write!(output, "{ORIGINAL_PHOTO_PREFIX}{message_id}\x1f{}\x1f{blob_id}\x1f", original.save_id)?;
                } else {
                    write!(output, "{PHOTO_REFERENCE_PREFIX}{message_id}\x1f{blob_id}\x1f")?;
                }
            }
            Self::File => {
                write!(output, "{FILE_REFERENCE_PREFIX}{message_id}\x1f{blob_id}\x1f")?;
            }
        }
        if let Some(photo) = previous {
            return output.write_str(photo.caption);
        }
        for c in caption.chars() {
            output.write_char(if c.is_control() { ' ' } else { c })?;
        }
        Ok(())
    }
}

pub fn split_file_reference(body: &str) -> Option<(u64, u64, &str)> {
    split_attachment_reference(body, FILE_REFERENCE_PREFIX)
}

fn split_attachment_reference<'a>(body: &'a str, prefix: &str) -> Option<(u64, u64, &'a str)> {
    let (agent, rest) = body.strip_prefix(prefix)?.split_once('\x1f')?;
    let (vault, caption) = rest.split_once('\x1f')?;
    let agent = agent.parse::<u64>().ok()?;
    let vault = vault.parse::<u64>().ok()?;
    (agent != 0 && vault != 0).then_some((agent, vault, caption))
}

/// Peer text has its own transcript representation. Escaping record separators
/// and physical newlines keeps arbitrary Unicode in a single, literal bubble.
pub const DIRECT_PREFIX: &str = "\x1ep\x1f";

pub fn write_direct_text(out: &mut impl core::fmt::Write, id: &str, text: &str) -> core::fmt::Result {
    write!(out, "{DIRECT_PREFIX}{id}\x1f")?;
    for c in text.chars() {
        match c {
            '\n' => out.write_str("\x1en")?,
            '\r' => out.write_str("\x1er")?,
            '\x1e' => out.write_str("\x1ee")?,
            _ => out.write_char(c)?,
        }
    }
    Ok(())
}

pub fn split_direct_text(body: &str) -> Option<(&str, &str)> {
    body.strip_prefix(DIRECT_PREFIX)?.split_once('\x1f')
}

pub fn decode_direct_text(encoded: &str, out: &mut impl core::fmt::Write) -> core::fmt::Result {
    let mut chars = encoded.chars();
    while let Some(c) = chars.next() {
        out.write_char(if c == '\x1e' { match chars.next() {
            Some('n') => '\n', Some('r') => '\r', Some('e') => '\x1e', _ => return Err(core::fmt::Error),
        } } else { c })?;
    }
    Ok(())
}

pub struct TranscriptBuffer<'a> {
    bytes: &'a mut [u8],
    len: usize,
}

impl<'a> TranscriptBuffer<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, len: 0 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).expect("written as UTF-8")
    }
}

impl core::fmt::Write for TranscriptBuffer<'_> {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len + text.len();
        self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?
            .copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

pub struct ChatTranscripts {
    slots: [[u8; TRANSCRIPT_LIMIT]; CHAT_SLOT_COUNT],
    lengths: [usize; CHAT_SLOT_COUNT],
    archived: u16,
    /// Connection that owns each slot. Zero means the archive was saved
    /// before transcripts followed the contact instead of the row number.
    handles: [u64; CHAT_SLOT_COUNT],
    /// 1 after the one-time repair that returns lines a new contact left behind.
    epoch: u16,
    /// Chat that owns `photo`, or `CHAT_SLOT_COUNT` when no picture is stored.
    photo_chat: u16,
    photo_len: u16,
    photo: [u8; PHOTO_CAP],
}

impl ChatTranscripts {
    pub const fn new() -> Self {
        Self {
            slots: [[0; TRANSCRIPT_LIMIT]; CHAT_SLOT_COUNT],
            lengths: [0; CHAT_SLOT_COUNT],
            archived: 0,
            handles: [0; CHAT_SLOT_COUNT],
            epoch: 0,
            photo_chat: CHAT_SLOT_COUNT as u16,
            photo_len: 0,
            photo: [0; PHOTO_CAP],
        }
    }

    /// Remember the JPEG a contact sent so the next boot can paint it.
    pub fn store_photo(&mut self, chat: usize, jpeg: &[u8]) -> bool {
        if chat >= CHAT_SLOT_COUNT
            || jpeg.len() < 4
            || jpeg.len() > PHOTO_CAP
            || jpeg[0] != 0xff
            || jpeg[1] != 0xd8
        {
            return false;
        }
        self.photo_chat = chat as u16;
        self.photo_len = jpeg.len() as u16;
        self.photo[..jpeg.len()].copy_from_slice(jpeg);
        true
    }

    pub fn photo(&self) -> Option<(usize, &[u8])> {
        let chat = self.photo_chat as usize;
        let len = self.photo_len as usize;
        if chat >= CHAT_SLOT_COUNT || len < 4 || len > PHOTO_CAP {
            return None;
        }
        let jpeg = &self.photo[..len];
        if jpeg[0] != 0xff || jpeg[1] != 0xd8 {
            return None;
        }
        Some((chat, jpeg))
    }

    fn carry_photo(&self, out: &mut ChatTranscripts, dest: Option<usize>) {
        let Some((chat, jpeg)) = self.photo() else {
            return;
        };
        let Some(dest) = dest else {
            return;
        };
        if dest == chat {
            out.photo_chat = self.photo_chat;
            out.photo_len = self.photo_len;
            out.photo[..jpeg.len()].copy_from_slice(jpeg);
            return;
        }
        let _ = out.store_photo(dest, jpeg);
    }

    pub fn text(&self, index: usize) -> &[u8] {
        let index = index.min(CHAT_SLOT_COUNT - 1);
        &self.slots[index][..self.lengths[index]]
    }

    pub fn clear(&mut self, index: usize) {
        let index = index.min(CHAT_SLOT_COUNT - 1);
        self.slots[index] = [0; TRANSCRIPT_LIMIT];
        self.lengths[index] = 0;
        self.archived &= !(1u16 << index);
    }

    /// Replace the full transcript text for one chat (edit/delete rewrites).
    pub fn replace_text(&mut self, index: usize, text: &str) {
        let index = index.min(CHAT_SLOT_COUNT - 1);
        let bytes = text.as_bytes();
        let len = bytes.len().min(TRANSCRIPT_LIMIT);
        self.slots[index] = [0; TRANSCRIPT_LIMIT];
        self.slots[index][..len].copy_from_slice(&bytes[..len]);
        self.lengths[index] = len;
    }

    pub fn is_archived(&self, index: usize) -> bool {
        let index = index.min(CHAT_SLOT_COUNT - 1);
        self.archived & (1u16 << index) != 0
    }

    pub fn set_archived(&mut self, index: usize, archived: bool) {
        let index = index.min(CHAT_SLOT_COUNT - 1);
        if archived {
            self.archived |= 1u16 << index;
        } else {
            self.archived &= !(1u16 << index);
        }
    }

    pub fn archived_mask(&self) -> u16 {
        self.archived
    }

    /// Put each saved line on the contact that owns it.
    ///
    /// An archive from before handles were stored is split once. Lines from
    /// the earlier cluster stay with the old row order. Lines from the later
    /// cluster, after a gap of at least three hours, stay with the contact
    /// that occupied that row when they were saved. When those two orders
    /// name the same contact, early lines left in front of a newer contact
    /// move once onto the next chat that only has the later cluster. Later
    /// list changes follow the stored handle and do not split again.
    pub fn rehome(&mut self, new_handles: &[u64], legacy_handles: &[u64]) -> bool {
        if new_handles.iter().all(|handle| *handle == 0) {
            return false;
        }
        let mut new_h = [0u64; CHAT_SLOT_COUNT];
        let mut legacy_h = [0u64; CHAT_SLOT_COUNT];
        for (index, handle) in new_handles.iter().take(CHAT_SLOT_COUNT).enumerate() {
            new_h[index] = *handle;
        }
        for (index, handle) in legacy_handles.iter().take(CHAT_SLOT_COUNT).enumerate() {
            legacy_h[index] = *handle;
        }
        let already = self.handles.iter().any(|handle| *handle != 0);
        if already && self.handles == new_h && self.epoch != 0 {
            return false;
        }
        let mut out = if already && self.handles == new_h {
            self.lift_stranded()
        } else if already {
            self.permuted(&new_h)
        } else {
            self.bound_from_rows(&new_h, &legacy_h).lift_stranded()
        };
        out.handles = new_h;
        out.epoch = 1;
        let changed = out.lengths != self.lengths
            || out.slots != self.slots
            || out.archived != self.archived
            || out.handles != self.handles
            || out.epoch != self.epoch;
        *self = out;
        changed
    }

    fn permuted(&self, new_h: &[u64; CHAT_SLOT_COUNT]) -> ChatTranscripts {
        let mut out = ChatTranscripts::new();
        for index in 0..CHAT_SLOT_COUNT {
            let handle = self.handles[index];
            let Some(dest) = handle_index(new_h, handle) else {
                continue;
            };
            out.push_raw(dest, &self.slots[index][..self.lengths[index]]);
            if self.is_archived(index) {
                out.set_archived(dest, true);
            }
        }
        if (self.photo_chat as usize) < CHAT_SLOT_COUNT {
            let dest = handle_index(new_h, self.handles[self.photo_chat as usize]);
            self.carry_photo(&mut out, dest);
        }
        out
    }

    fn bound_from_rows(
        &self,
        new_h: &[u64; CHAT_SLOT_COUNT],
        legacy_h: &[u64; CHAT_SLOT_COUNT],
    ) -> ChatTranscripts {
        let session_at = self.session_start_minute();
        let mut out = ChatTranscripts::new();
        for index in 0..CHAT_SLOT_COUNT {
            let text = &self.slots[index][..self.lengths[index]];
            let mut start = 0;
            while start < text.len() {
                let end = text[start..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|offset| start + offset)
                    .unwrap_or(text.len());
                let line = &text[start..end];
                if !line.is_empty() {
                    let session = session_at.is_some_and(|minute| {
                        line_minute(line).is_some_and(|stamp| stamp >= minute)
                    });
                    let handle = if session {
                        new_h[index]
                    } else {
                        legacy_h[index]
                    };
                    if let Some(dest) = handle_index(new_h, handle) {
                        out.push_raw(dest, line);
                        out.push_raw(dest, b"\n");
                    }
                }
                if end == text.len() {
                    break;
                }
                start = end + 1;
            }
            if self.is_archived(index) {
                if let Some(dest) = handle_index(new_h, legacy_h[index]) {
                    out.set_archived(dest, true);
                }
            }
        }
        self.carry_photo(&mut out, Some(self.photo_chat as usize));
        out
    }

    fn push_raw(&mut self, index: usize, bytes: &[u8]) {
        let index = index.min(CHAT_SLOT_COUNT - 1);
        let room = TRANSCRIPT_LIMIT.saturating_sub(self.lengths[index]);
        let take = bytes.len().min(room);
        let length = self.lengths[index];
        self.slots[index][length..length + take].copy_from_slice(&bytes[..take]);
        self.lengths[index] = length + take;
    }

    /// First minute of the later cluster, when the quiet gap is at least three hours.
    fn session_start_minute(&self) -> Option<u16> {
        let mut minutes = [0u16; 64];
        let mut count = 0usize;
        for index in 0..CHAT_SLOT_COUNT {
            let text = &self.slots[index][..self.lengths[index]];
            let mut start = 0;
            while start < text.len() {
                let end = text[start..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|offset| start + offset)
                    .unwrap_or(text.len());
                if let Some(minute) = line_minute(&text[start..end]) {
                    if count < minutes.len() && !minutes[..count].contains(&minute) {
                        minutes[count] = minute;
                        count += 1;
                    }
                }
                if end == text.len() {
                    break;
                }
                start = end + 1;
            }
        }
        if count < 2 {
            return None;
        }
        for left in 1..count {
            let mut right = left;
            while right > 0 && minutes[right - 1] > minutes[right] {
                minutes.swap(right - 1, right);
                right -= 1;
            }
        }
        let mut best_gap = 0u16;
        let mut session = None;
        for pair in minutes[..count].windows(2) {
            let gap = pair[1] - pair[0];
            if gap > best_gap {
                best_gap = gap;
                session = Some(pair[1]);
            }
        }
        if best_gap >= 180 {
            session
        } else {
            None
        }
    }

    /// Move early lines off a contact that was inserted in front of them.
    ///
    /// The early lines stay only when the next chat's own later lines are
    /// older than this chat's later lines. A chat that simply continued
    /// after a quiet gap keeps its morning lines.
    fn lift_stranded(&self) -> ChatTranscripts {
        let Some(session_at) = self.session_start_minute() else {
            return self.copy_slots();
        };
        let mut has_early = [false; CHAT_SLOT_COUNT];
        let mut has_session = [false; CHAT_SLOT_COUNT];
        let mut min_session = [0u16; CHAT_SLOT_COUNT];
        let mut max_minute = [0u16; CHAT_SLOT_COUNT];
        for index in 0..CHAT_SLOT_COUNT {
            each_line(&self.slots[index][..self.lengths[index]], |line| {
                match line_minute(line) {
                    Some(minute) if minute >= session_at => {
                        if !has_session[index] || minute < min_session[index] {
                            min_session[index] = minute;
                        }
                        if minute > max_minute[index] {
                            max_minute[index] = minute;
                        }
                        has_session[index] = true;
                    }
                    _ => has_early[index] = true,
                }
            });
        }
        let mut move_to = [usize::MAX; CHAT_SLOT_COUNT];
        for index in 0..CHAT_SLOT_COUNT {
            if !has_early[index] || !has_session[index] {
                continue;
            }
            for later in index + 1..CHAT_SLOT_COUNT {
                if has_session[later]
                    && !has_early[later]
                    && max_minute[later] < min_session[index]
                {
                    move_to[index] = later;
                    break;
                }
            }
        }
        if move_to.iter().all(|dest| *dest == usize::MAX) {
            return self.copy_slots();
        }
        let mut out = ChatTranscripts::new();
        out.archived = self.archived;
        out.handles = self.handles;
        for dest in 0..CHAT_SLOT_COUNT {
            for source in 0..dest {
                if move_to[source] != dest {
                    continue;
                }
                each_line(&self.slots[source][..self.lengths[source]], |line| {
                    let session = line_minute(line).is_some_and(|minute| minute >= session_at);
                    if !session {
                        out.push_raw(dest, line);
                        out.push_raw(dest, b"\n");
                    }
                });
            }
            if move_to[dest] == usize::MAX && !move_to[..dest].contains(&dest) {
                out.push_raw(dest, &self.slots[dest][..self.lengths[dest]]);
                continue;
            }
            each_line(&self.slots[dest][..self.lengths[dest]], |line| {
                let session = line_minute(line).is_some_and(|minute| minute >= session_at);
                if move_to[dest] == usize::MAX || session {
                    out.push_raw(dest, line);
                    out.push_raw(dest, b"\n");
                }
            });
        }
        self.carry_photo(&mut out, Some(self.photo_chat as usize));
        out
    }

    fn copy_slots(&self) -> ChatTranscripts {
        let mut out = ChatTranscripts::new();
        out.slots = self.slots;
        out.lengths = self.lengths;
        out.archived = self.archived;
        out.handles = self.handles;
        out.epoch = self.epoch;
        self.carry_photo(&mut out, Some(self.photo_chat as usize));
        out
    }

    pub fn append(&mut self, index: usize, speaker: &str, body: &str) {
        let index = index.min(CHAT_SLOT_COUNT - 1);
        let mut combined = [0u8; TRANSCRIPT_LIMIT + 2 + 32 + 2 + 2048 + 1];
        let mut len = self.lengths[index];
        combined[..len].copy_from_slice(&self.slots[index][..len]);
        if len > 0 && combined[len - 1] != b'\n' {
            combined[len] = b'\n';
            len += 1;
        }
        let line_start = len;
        let speaker = speaker.as_bytes();
        let body = body.as_bytes();
        if len + speaker.len() + 2 + body.len() + 1 > combined.len() {
            return;
        }
        combined[len..len + speaker.len()].copy_from_slice(speaker);
        len += speaker.len();
        combined[len..len + 2].copy_from_slice(b": ");
        len += 2;
        combined[len..len + body.len()].copy_from_slice(body);
        len += body.len();
        combined[len] = b'\n';
        len += 1;
        if len > TRANSCRIPT_LIMIT {
            let tail = len - TRANSCRIPT_LIMIT;
            let cut = combined[tail..len]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| tail + offset + 1)
                .unwrap_or(line_start);
            let kept = len - cut;
            combined.copy_within(cut..len, 0);
            len = kept;
        }
        self.slots[index][..len].copy_from_slice(&combined[..len]);
        self.lengths[index] = len;
        while self.stored_bytes() > TRANSCRIPT_BUDGET && self.lengths[index] > 0 {
            let length = self.lengths[index];
            let cut = self.slots[index][..length]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| offset + 1)
                .unwrap_or(length);
            if cut == 0 || cut > length {
                break;
            }
            self.slots[index].copy_within(cut..length, 0);
            self.lengths[index] = length - cut;
        }
    }

    fn stored_bytes(&self) -> usize {
        self.lengths.iter().map(|length| 2 + *length).sum::<usize>()
    }

    /// Mark the oldest sent line in this chat that the peer has not confirmed.
    /// The check is part of the saved line, so it is still there after a reboot.
    pub fn mark_delivered(&mut self, index: usize) -> bool {
        const SUFFIX: &[u8] = " \u{2713}".as_bytes();
        let index = index.min(CHAT_SLOT_COUNT - 1);
        let length = self.lengths[index];
        let mut start = 0;
        while start < length {
            let end = self.slots[index][start..length]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| start + offset)
                .unwrap_or(length);
            let line = &self.slots[index][start..end];
            if line.starts_with(b"NexatrodeOS: ") && !line.ends_with(SUFFIX) {
                if length + SUFFIX.len() > TRANSCRIPT_LIMIT {
                    return false;
                }
                let mut combined = [0u8; TRANSCRIPT_LIMIT];
                combined[..end].copy_from_slice(&self.slots[index][..end]);
                combined[end..end + SUFFIX.len()].copy_from_slice(SUFFIX);
                let tail = length - end;
                combined[end + SUFFIX.len()..end + SUFFIX.len() + tail]
                    .copy_from_slice(&self.slots[index][end..length]);
                let len = length + SUFFIX.len();
                self.slots[index][..len].copy_from_slice(&combined[..len]);
                self.lengths[index] = len;
                while self.stored_bytes() > TRANSCRIPT_BUDGET && self.lengths[index] > 0 {
                    let current = self.lengths[index];
                    let cut = self.slots[index][..current]
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map(|offset| offset + 1)
                        .unwrap_or(current);
                    if cut == 0 || cut > current {
                        break;
                    }
                    self.slots[index].copy_within(cut..current, 0);
                    self.lengths[index] = current - cut;
                }
                return self.slots[index][..self.lengths[index]].windows(SUFFIX.len()).any(|window| window == SUFFIX);
            }
            if end == length {
                break;
            }
            start = end + 1;
        }
        false
    }
}

fn handle_index(handles: &[u64; CHAT_SLOT_COUNT], handle: u64) -> Option<usize> {
    if handle == 0 {
        return None;
    }
    handles.iter().position(|slot| *slot == handle)
}

fn each_line(text: &[u8], mut visit: impl FnMut(&[u8])) {
    let mut start = 0;
    while start < text.len() {
        let end = text[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| start + offset)
            .unwrap_or(text.len());
        let line = &text[start..end];
        if !line.is_empty() {
            visit(line);
        }
        if end == text.len() {
            break;
        }
        start = end + 1;
    }
}

fn line_minute(line: &[u8]) -> Option<u16> {
    let last = line.len().saturating_sub(5);
    for index in 0..=last {
        if line[index].is_ascii_digit()
            && line[index + 1].is_ascii_digit()
            && line[index + 2] == b':'
            && line[index + 3].is_ascii_digit()
            && line[index + 4].is_ascii_digit()
            && line.get(index + 5) == Some(&MESSAGE_CLOCK_MARK)
        {
            let hour = (line[index] - b'0') * 10 + (line[index + 1] - b'0');
            let minute = (line[index + 3] - b'0') * 10 + (line[index + 4] - b'0');
            if hour < 24 && minute < 60 {
                return Some(u16::from(hour) * 60 + u16::from(minute));
            }
        }
    }
    None
}

pub fn encode_archive(
    messages: &Messages,
    chats: &ChatTranscripts,
    output: &mut [u8],
) -> Option<usize> {
    let mut inner = [0u8; MESSAGES_BLOB_CAPACITY];
    let inner_len = messages.encode(&mut inner)?;
    let mut need = 6 + inner_len + 2 + HANDLE_TRAILER + EPOCH_TRAILER;
    for length in chats.lengths {
        need += 2 + length;
    }
    if output.len() < need || need > MAX_PAYLOAD_SIZE {
        return None;
    }
    output[..4].copy_from_slice(&ARCHIVE_MAGIC);
    output[4..6].copy_from_slice(&(inner_len as u16).to_le_bytes());
    output[6..6 + inner_len].copy_from_slice(&inner[..inner_len]);
    let mut cursor = 6 + inner_len;
    for slot in 0..CHAT_SLOT_COUNT {
        let length = chats.lengths[slot];
        output[cursor..cursor + 2].copy_from_slice(&(length as u16).to_le_bytes());
        cursor += 2;
        output[cursor..cursor + length].copy_from_slice(&chats.slots[slot][..length]);
        cursor += length;
    }
    if cursor + 2 + HANDLE_TRAILER + EPOCH_TRAILER > output.len()
        || cursor + 2 + HANDLE_TRAILER + EPOCH_TRAILER > MAX_PAYLOAD_SIZE
    {
        return None;
    }
    output[cursor..cursor + 2].copy_from_slice(&chats.archived.to_le_bytes());
    cursor += 2;
    for handle in chats.handles {
        output[cursor..cursor + 8].copy_from_slice(&handle.to_le_bytes());
        cursor += 8;
    }
    output[cursor..cursor + 2].copy_from_slice(&chats.epoch.to_le_bytes());
    cursor += 2;
    if let Some((chat, jpeg)) = chats.photo() {
        let extra = 8 + jpeg.len();
        if cursor + extra <= output.len() && cursor + extra <= MAX_PAYLOAD_SIZE {
            output[cursor..cursor + 4].copy_from_slice(&PHOTO_MAGIC);
            cursor += 4;
            output[cursor..cursor + 2].copy_from_slice(&(chat as u16).to_le_bytes());
            cursor += 2;
            output[cursor..cursor + 2].copy_from_slice(&(jpeg.len() as u16).to_le_bytes());
            cursor += 2;
            output[cursor..cursor + jpeg.len()].copy_from_slice(jpeg);
            cursor += jpeg.len();
        }
    }
    Some(cursor)
}

/// PMS2 loads the peer-key shell with empty transcripts. PMS3 loads both.
pub fn decode_archive(payload: &[u8]) -> Option<(Messages, ChatTranscripts)> {
    if payload.len() >= 4 && payload[..4] == MAGIC {
        return Some((Messages::decode(payload)?, ChatTranscripts::new()));
    }
    if payload.len() < 6 || payload[..4] != ARCHIVE_MAGIC {
        return None;
    }
    let inner_len = u16::from_le_bytes([payload[4], payload[5]]) as usize;
    if payload.len() < 6 + inner_len {
        return None;
    }
    let messages = Messages::decode(&payload[6..6 + inner_len])?;
    let mut chats = ChatTranscripts::new();
    let mut cursor = 6 + inner_len;
    for slot in 0..CHAT_SLOT_COUNT {
        if cursor == payload.len() {
            break;
        }
        if cursor + 2 > payload.len() {
            return None;
        }
        let length = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]) as usize;
        cursor += 2;
        if length > TRANSCRIPT_LIMIT || cursor + length > payload.len() {
            return None;
        }
        chats.slots[slot][..length].copy_from_slice(&payload[cursor..cursor + length]);
        chats.lengths[slot] = length;
        cursor += length;
    }
    if cursor + 2 <= payload.len() {
        chats.archived = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]);
        cursor += 2;
    }
    if cursor + HANDLE_TRAILER <= payload.len() {
        for handle in &mut chats.handles {
            *handle = u64::from_le_bytes(payload[cursor..cursor + 8].try_into().ok()?);
            cursor += 8;
        }
        if cursor + EPOCH_TRAILER <= payload.len() {
            chats.epoch = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]);
            cursor += EPOCH_TRAILER;
        }
        if cursor + 8 <= payload.len() && payload[cursor..cursor + 4] == PHOTO_MAGIC {
            let chat = u16::from_le_bytes([payload[cursor + 4], payload[cursor + 5]]);
            let len = u16::from_le_bytes([payload[cursor + 6], payload[cursor + 7]]) as usize;
            cursor += 8;
            if chat as usize >= CHAT_SLOT_COUNT
                || len < 4
                || len > PHOTO_CAP
                || cursor + len > payload.len()
                || payload[cursor] != 0xff
                || payload[cursor + 1] != 0xd8
            {
                return None;
            }
            chats.photo_chat = chat;
            chats.photo_len = len as u16;
            chats.photo[..len].copy_from_slice(&payload[cursor..cursor + len]);
            cursor += len;
        }
    }
    if cursor != payload.len() {
        return None;
    }
    Some((messages, chats))
}

const fn evdev_digit(code: u16) -> Option<u8> {
    match code {
        2 => Some(b'1'),
        3 => Some(b'2'),
        4 => Some(b'3'),
        5 => Some(b'4'),
        6 => Some(b'5'),
        7 => Some(b'6'),
        8 => Some(b'7'),
        9 => Some(b'8'),
        10 => Some(b'9'),
        11 => Some(b'0'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offer_receipt_never_masks_a_failed_upload() {
        let offered = "NexatrodeOS: \x1eF\x1f41\x1f8\x1fnote.txt";
        let delivered = std::format!("{offered} ✓");
        let (base, suffix) = super::outgoing_notice_update(&delivered, true).unwrap();
        assert_eq!(base, offered);
        assert_eq!(suffix, super::UPLOAD_FAILED_SUFFIX);
        let failed = std::format!("{base}{suffix}");
        assert!(super::outgoing_notice_update(&failed, false).is_none());
        assert!(super::outgoing_notice_update(&failed, true).is_none());
        assert_eq!(super::split_file_reference(failed.strip_prefix("NexatrodeOS: ").unwrap()
            .strip_suffix(super::UPLOAD_FAILED_SUFFIX).unwrap()), Some((41, 8, "note.txt")));
    }

    #[test]
    fn empty_then_add_persist_and_delete() {
        let mut app = Messages::new();
        let mut buffer = [0; 128];
        assert_eq!(app.display(&mut buffer), b"NO THREADS\nENTER TO ADD");

        assert!(app.apply_key(KEY_ENTER, 1).changed);
        assert_eq!(app.display(&mut buffer), b"ADD KEY 0 OF 8\n");

        // BE12CD34
        for code in [48u16, 18, 2, 3, 46, 32, 4, 5] {
            assert!(app.apply_key(code, 1).changed);
        }
        assert_eq!(app.display(&mut buffer), b"ADD KEY 8 OF 8\nBE12CD34");
        let commit = app.apply_key(KEY_ENTER, 1);
        assert!(commit.changed && commit.persist);
        assert_eq!(app.display(&mut buffer), b"- KEY BE12CD34");

        let mut encoded = [0; MESSAGES_BLOB_CAPACITY];
        let length = app.encode(&mut encoded).unwrap();
        assert_eq!(&encoded[..4], b"PMS2");
        let restored = Messages::decode(&encoded[..length]).unwrap();
        assert_eq!(restored.display(&mut buffer), b"- KEY BE12CD34");

        let delete = app.apply_key(KEY_DELETE, 1);
        assert!(delete.changed && delete.persist);
        assert_eq!(app.display(&mut buffer), b"NO THREADS\nENTER TO ADD");
    }

    #[test]
    fn compose_seals_and_round_trips_without_plaintext_on_display() {
        let mut app = Messages::new();
        let mut buffer = [0; 128];
        assert!(app.apply_key(KEY_ENTER, 1).changed);
        for code in [48u16, 18, 2, 3, 46, 32, 4, 5] {
            assert!(app.apply_key(code, 1).changed);
        }
        assert!(app.apply_key(KEY_ENTER, 1).persist);

        assert!(app.apply_key(KEY_ENTER, 1).changed);
        assert_eq!(app.display(&mut buffer), b"COMPOSE KEY BE12CD34\n");
        // HI
        assert!(app.apply_key(35, 1).changed); // H
        assert!(app.apply_key(23, 1).changed); // I
        assert_eq!(app.display(&mut buffer), b"COMPOSE KEY BE12CD34\nHI");
        let sealed = app.apply_key(KEY_ENTER, 1);
        assert!(sealed.changed && sealed.persist && sealed.tx_sealed);
        assert_eq!(app.display(&mut buffer), b"- KEY BE12CD34\n  SEALED 1");
        assert!(!buffer.windows(2).any(|window| window == b"HI"));
        let packet = app.selected_sealed().unwrap();
        assert_eq!(&packet[..4], b"PSL1");
        assert_eq!(&packet[4..12], b"BE12CD34");

        let mut encoded = [0; MESSAGES_BLOB_CAPACITY];
        let length = app.encode(&mut encoded).unwrap();
        let restored = Messages::decode(&encoded[..length]).unwrap();
        assert_eq!(restored.display(&mut buffer), b"- KEY BE12CD34\n  SEALED 1");
        assert_eq!(restored.peers[0].sealed_count, 1);
        assert!(restored.peers[0].sealed_len as usize >= message_seal::HEADER_SIZE);
        assert_eq!(&restored.peers[0].sealed[..4], b"PSL1");
    }

    #[test]
    fn message_time_prefix_uses_utc_minutes_and_strips_back_off() {
        let mut out = [0u8; 32];
        assert!(prefix_message_time(0, b"hi", &mut out).is_none());
        assert!(prefix_message_time(999_999_999, b"hi", &mut out).is_none());
        let midday = 20_000 * 86_400 + 45_296;
        let source = "hi \u{2713}";
        let length = prefix_message_time(midday, source.as_bytes(), &mut out).unwrap();
        assert_eq!(&out[..5], b"12:34");
        assert_eq!(out[5], 0x1f);
        assert_eq!(&out[6..length], source.as_bytes());
        let (time, body) = split_message_time(core::str::from_utf8(&out[..length]).unwrap());
        assert_eq!(time, Some("12:34"));
        assert_eq!(body, source);
        assert_eq!(split_message_time("plain"), (None, "plain"));
        assert!(prefix_message_time(midday, &out[..length], &mut [0u8; 32]).is_none());
    }

    #[test]
    fn archive_keeps_peer_keys_and_four_chat_transcripts() {
        let mut app = Messages::new();
        assert!(app.apply_key(KEY_ENTER, 1).changed);
        for code in [48u16, 18, 2, 3, 46, 32, 4, 5] {
            assert!(app.apply_key(code, 1).changed);
        }
        assert!(app.apply_key(KEY_ENTER, 1).persist);
        let mut chats = ChatTranscripts::new();
        chats.append(0, "NexatrodeOS", "yo");
        chats.append(0, "stevez00", "hi77");
        chats.append(1, "Phone", "later");
        let mut encoded = [0; ARCHIVE_CAPACITY];
        let length = encode_archive(&app, &chats, &mut encoded).unwrap();
        assert_eq!(&encoded[..4], b"PMS3");
        let (restored, restored_chats) = decode_archive(&encoded[..length]).unwrap();
        let mut buffer = [0; 128];
        assert_eq!(restored.display(&mut buffer), b"- KEY BE12CD34");
        assert_eq!(restored_chats.text(0), b"NexatrodeOS: yo\nstevez00: hi77\n");
        assert_eq!(restored_chats.text(1), b"Phone: later\n");
        assert!(restored_chats.text(2).is_empty());

        let mut pms2 = [0; MESSAGES_BLOB_CAPACITY];
        let pms2_len = app.encode(&mut pms2).unwrap();
        let (from_old, empty) = decode_archive(&pms2[..pms2_len]).unwrap();
        assert_eq!(from_old.display(&mut buffer), b"- KEY BE12CD34");
        assert!(empty.text(0).is_empty());

        chats.append(7, "Ada", "hello");
        let length = encode_archive(&app, &chats, &mut encoded).unwrap();
        assert!(length <= MAX_PAYLOAD_SIZE);
        let (_restored, eighth) = decode_archive(&encoded[..length]).unwrap();
        assert_eq!(eighth.text(0), b"NexatrodeOS: yo\nstevez00: hi77\n");
        assert_eq!(eighth.text(7), b"Ada: hello\n");
        assert!(eighth.text(4).is_empty());
    }

    #[test]
    fn archive_reads_a_record_that_stopped_at_four_slots() {
        let mut app = Messages::new();
        assert!(app.apply_key(KEY_ENTER, 1).changed);
        for code in [48u16, 18, 2, 3, 46, 32, 4, 5] {
            assert!(app.apply_key(code, 1).changed);
        }
        assert!(app.apply_key(KEY_ENTER, 1).persist);
        let mut inner = [0u8; MESSAGES_BLOB_CAPACITY];
        let inner_len = app.encode(&mut inner).unwrap();
        let slots: [&[u8]; 4] = [
            b"NexatrodeOS: yo\n",
            b"Phone: later\n",
            b"",
            b"stevez00: hi\n",
        ];
        let mut encoded = [0u8; 1024];
        encoded[..4].copy_from_slice(b"PMS3");
        encoded[4..6].copy_from_slice(&(inner_len as u16).to_le_bytes());
        let mut cursor = 6;
        encoded[cursor..cursor + inner_len].copy_from_slice(&inner[..inner_len]);
        cursor += inner_len;
        for slot in slots {
            encoded[cursor..cursor + 2].copy_from_slice(&(slot.len() as u16).to_le_bytes());
            cursor += 2;
            encoded[cursor..cursor + slot.len()].copy_from_slice(slot);
            cursor += slot.len();
        }
        let (restored, chats) = decode_archive(&encoded[..cursor]).unwrap();
        let mut buffer = [0; 128];
        assert_eq!(restored.display(&mut buffer), b"- KEY BE12CD34");
        assert_eq!(chats.text(0), b"NexatrodeOS: yo\n");
        assert_eq!(chats.text(3), b"stevez00: hi\n");
        assert!(chats.text(4).is_empty());
        assert!(chats.text(7).is_empty());
    }

    #[test]
    fn eight_full_chats_stay_inside_the_vault_blob() {
        let mut chats = ChatTranscripts::new();
        for slot in 0..CHAT_SLOT_COUNT {
            for _ in 0..40 {
                chats.append(slot, "NexatrodeOS", "0123456789abcdef");
            }
        }
        let mut total = 0;
        for slot in 0..CHAT_SLOT_COUNT {
            total += 2 + chats.text(slot).len();
            assert!(chats.text(slot).len() <= TRANSCRIPT_LIMIT);
        }
        assert!(total <= TRANSCRIPT_BUDGET);
        let mut encoded = [0; ARCHIVE_CAPACITY];
        let length = encode_archive(&Messages::new(), &chats, &mut encoded).unwrap();
        assert!(length <= MAX_PAYLOAD_SIZE);
        let (_messages, restored) = decode_archive(&encoded[..length]).unwrap();
        assert_eq!(restored.text(7).len(), chats.text(7).len());
        assert!(restored.photo().is_none());
    }

    #[test]
    fn archive_round_trips_distinct_photo_references() {
        let mut chats = ChatTranscripts::new();
        chats.append(1, "Mac", "\x1eP\x1f73\x1f2\x1fFirst photo");
        chats.append(1, "Mac", "\x1eP\x1f74\x1f3\x1fSecond photo");
        let mut encoded = [0; ARCHIVE_CAPACITY];
        let length = encode_archive(&Messages::new(), &chats, &mut encoded).unwrap();
        let (_, restored) = decode_archive(&encoded[..length]).unwrap();
        let mut photos = core::str::from_utf8(restored.text(1)).unwrap().lines()
            .filter_map(|line| split_photo_reference(line.split_once(": ")?.1));
        assert_eq!(photos.next(), Some((73, 2, "First photo")));
        assert_eq!(photos.next(), Some((74, 3, "Second photo")));
        assert_eq!(photos.next(), None);
    }

    #[test]
    fn reloaded_photo_row_saves_the_original_with_its_caption() {
        let mut chats = ChatTranscripts::new();
        chats.append(1, "Mac", "\x1eO\x1f73\x1f502\x1f398\x1f海岸");
        let mut encoded = [0; ARCHIVE_CAPACITY];
        let length = encode_archive(&Messages::new(), &chats, &mut encoded).unwrap();
        let (_, restored) = decode_archive(&encoded[..length]).unwrap();
        let line = core::str::from_utf8(restored.text(1)).unwrap().trim_end();
        let (_, body) = line.split_once(": ").unwrap();
        assert_eq!(split_photo_reference(body), Some((73, 502, "海岸")));
    }

    #[test]
    fn original_upgrades_one_preview_and_both_replays_keep_the_original() {
        let preview = "\x1eP\x1f73\x1f398\x1fSea cliffs";
        assert!(!ReceivedAttachment::Preview.needs_blob(preview));
        assert!(ReceivedAttachment::Original.needs_blob(preview));
        let mut bytes = [0; 128];
        let mut row = TranscriptBuffer::new(&mut bytes);
        ReceivedAttachment::Original.write_reference(&mut row, preview, 73, 502, false, "Photo").unwrap();
        let upgraded = row.as_str();
        assert!(!ReceivedAttachment::Preview.needs_blob(upgraded));
        assert!(!ReceivedAttachment::Original.needs_blob(upgraded));
        let reference = photo_reference(upgraded).unwrap();
        assert_eq!(reference.display_id, 398);
        assert_eq!(reference.save_id, 502);
        assert_eq!(reference.caption, "Sea cliffs");
    }

    #[test]
    fn large_jpeg_keeps_its_small_bubble_image_and_saves_full_bytes() {
        let mut bytes = [0; 128];
        let mut row = TranscriptBuffer::new(&mut bytes);
        ReceivedAttachment::Original.write_reference(
            &mut row, "\x1eP\x1f73\x1f398\x1fCliffs", 73, 502, true, "Cliffs",
        ).unwrap();
        let photo = photo_reference(row.as_str()).unwrap();
        assert_eq!((photo.display_id, photo.save_id), (398, 502));
    }

    #[test]
    fn save_clicked_on_a_stale_preview_uses_the_latest_photo_in_that_chat() {
        let transcript = "Mac: \x1eO\x1f72\x1f501\x1f397\x1fOther\nMac: 12:04\x1f\x1eO\x1f73\x1f502\x1f398\x1fCliffs\n";
        assert_eq!(current_photo_save_id(transcript, 398), Some(502));
        assert_eq!(current_photo_save_id(transcript, 502), Some(502));
        assert_eq!(current_photo_save_id(transcript, 5), None);
    }

    #[test]
    fn archive_keeps_one_received_jpeg() {
        let mut chats = ChatTranscripts::new();
        chats.append(1, "MacHost", "Photo");
        let mut jpeg = [0u8; 758];
        jpeg[0] = 0xff;
        jpeg[1] = 0xd8;
        jpeg[2] = 0xff;
        jpeg[757] = 0xd9;
        assert!(chats.store_photo(1, &jpeg));
        let mut encoded = [0; ARCHIVE_CAPACITY];
        let length = encode_archive(&Messages::new(), &chats, &mut encoded).unwrap();
        assert!(length <= MAX_PAYLOAD_SIZE);
        assert!(encoded[..length].windows(4).any(|mark| mark == PHOTO_MAGIC));
        let (_messages, restored) = decode_archive(&encoded[..length]).unwrap();
        assert_eq!(restored.text(1), chats.text(1));
        let (chat, stored) = restored.photo().unwrap();
        assert_eq!(chat, 1);
        assert_eq!(stored, &jpeg);
    }

    #[test]
    fn delivery_marks_the_oldest_unconfirmed_sent_line() {
        let mut chats = ChatTranscripts::new();
        chats.append(0, "NexatrodeOS", "hi");
        chats.append(0, "stevez00", "hi");
        chats.append(0, "NexatrodeOS", "hi2");
        assert!(chats.mark_delivered(0));
        assert_eq!(
            chats.text(0),
            "NexatrodeOS: hi \u{2713}\nstevez00: hi\nNexatrodeOS: hi2\n".as_bytes()
        );
        assert!(chats.mark_delivered(0));
        assert_eq!(
            chats.text(0),
            "NexatrodeOS: hi \u{2713}\nstevez00: hi\nNexatrodeOS: hi2 \u{2713}\n".as_bytes()
        );
        assert!(!chats.mark_delivered(0));
        let mut encoded = [0; ARCHIVE_CAPACITY];
        let length = encode_archive(&Messages::new(), &chats, &mut encoded).unwrap();
        let (_messages, restored) = decode_archive(&encoded[..length]).unwrap();
        assert_eq!(restored.text(0), chats.text(0));
    }

    #[test]
    fn rehome_keeps_the_old_chat_and_the_later_contact() {
        let mut chats = ChatTranscripts::new();
        chats.replace_text(
            0,
            "NexatrodeOS: 07:34\u{1f}HIHIHHI\nMacHost: 07:35\u{1f}pong\nNexatrodeOS: 16:50\u{1f}11\nMacHost: 16:50\u{1f}back\n",
        );
        chats.replace_text(1, "NexatrodeOS: 16:48\u{1f}YO\n");
        let new_handles = [99u64, 10, 11];
        let legacy_handles = [10u64, 20, 11];
        assert!(chats.rehome(&new_handles, &legacy_handles));
        assert_eq!(
            core::str::from_utf8(chats.text(0)).unwrap(),
            "NexatrodeOS: 16:50\u{1f}11\nMacHost: 16:50\u{1f}back\n"
        );
        assert_eq!(
            core::str::from_utf8(chats.text(1)).unwrap(),
            "NexatrodeOS: 07:34\u{1f}HIHIHHI\nMacHost: 07:35\u{1f}pong\nNexatrodeOS: 16:48\u{1f}YO\n"
        );
        assert!(!chats.rehome(&new_handles, &legacy_handles));
        let swapped = [10u64, 99, 11];
        assert!(chats.rehome(&swapped, &legacy_handles));
        assert!(chats.text(0).windows(7).any(|line| line == b"HIHIHHI"));
        assert!(chats.text(1).windows(2).any(|line| line == b"11"));
    }

    #[test]
    fn rehome_returns_lines_a_new_contact_left_in_front() {
        let mut chats = ChatTranscripts::new();
        chats.replace_text(
            0,
            "NexatrodeOS: 07:34\u{1f}HIHIHHI\nMacHost: 07:35\u{1f}pong\nNexatrodeOS: 16:50\u{1f}11\nMacHost: 16:50\u{1f}back\n",
        );
        chats.replace_text(1, "NexatrodeOS: 16:48\u{1f}YO\n");
        let handles = [99u64, 10, 11];
        assert!(chats.rehome(&handles, &handles));
        assert_eq!(
            core::str::from_utf8(chats.text(0)).unwrap(),
            "NexatrodeOS: 16:50\u{1f}11\nMacHost: 16:50\u{1f}back\n"
        );
        assert_eq!(
            core::str::from_utf8(chats.text(1)).unwrap(),
            "NexatrodeOS: 07:34\u{1f}HIHIHHI\nMacHost: 07:35\u{1f}pong\nNexatrodeOS: 16:48\u{1f}YO\n"
        );
        assert!(!chats.rehome(&handles, &handles));
    }
}
