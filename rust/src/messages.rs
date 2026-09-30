//! Offline Messages peer-key shell with vault-backed PMS2 sealed stubs.

use crate::message_seal::{self, KEY_ID_SIZE, MAX_PACKET};

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
                    let marker = if index == self.selected {
                        b"- "
                    } else {
                        b"  "
                    };
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
                let mut offset = write(buffer, 0, b"ADD KEY\n");
                offset = write(
                    buffer,
                    offset,
                    &self.peers[self.selected].key[..self.draft_length],
                );
                &buffer[..offset]
            }
            EditPhase::Compose => {
                let mut offset = write(buffer, 0, b"COMPOSE\n");
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
    fn empty_then_add_persist_and_delete() {
        let mut app = Messages::new();
        let mut buffer = [0; 128];
        assert_eq!(app.display(&mut buffer), b"NO THREADS\nENTER TO ADD");

        assert!(app.apply_key(KEY_ENTER, 1).changed);
        assert_eq!(app.display(&mut buffer), b"ADD KEY\n");

        // BE12CD34
        for code in [48u16, 18, 2, 3, 46, 32, 4, 5] {
            assert!(app.apply_key(code, 1).changed);
        }
        assert_eq!(app.display(&mut buffer), b"ADD KEY\nBE12CD34");
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
        assert_eq!(app.display(&mut buffer), b"COMPOSE\n");
        // HI
        assert!(app.apply_key(35, 1).changed); // H
        assert!(app.apply_key(23, 1).changed); // I
        assert_eq!(app.display(&mut buffer), b"COMPOSE\nHI");
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
        assert_eq!(
            restored.display(&mut buffer),
            b"- KEY BE12CD34\n  SEALED 1"
        );
        assert_eq!(restored.peers[0].sealed_count, 1);
        assert!(restored.peers[0].sealed_len as usize >= message_seal::HEADER_SIZE);
        assert_eq!(&restored.peers[0].sealed[..4], b"PSL1");
    }
}
